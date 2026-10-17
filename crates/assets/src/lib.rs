//! Assets: data that is loaded or generated one time and used by many entities.
//!
//! An asset lives in an [`Assets`] store. Users hold a [`Handle`] and ask the store for
//! the data each time they need it. A handle exists before its data: [`Assets::get`]
//! returns `None` until the asset is ready. Code that obeys this rule does not change
//! when a loader becomes asynchronous or reads a different file format.

mod data;
mod file_ref;
mod mesh;
mod model;
mod texture;
mod watch;

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use maki_core::{App, Module, Settings, Update};
use serde::{Deserialize, Serialize};
use slotmap::{DefaultKey, SlotMap};

pub use data::{
    DATA_FOLDER, DataAsset, DataAssets, DataFile, DataRef, DataType, DataTypes, add_data_type,
    data_text, read_data,
};
pub use file_ref::{FileId, FileRef, forget_id_tables};
pub use mesh::{Doorway, Material, Mesh, Wall};
pub use model::{Model, ModelLight, ModelLoads, check_model, load_gltf};
pub use texture::{Texture, TextureRef, Textures, check_image};
pub use watch::{AssetChanged, AssetFailed, AssetWatch, Check, forget_changed};

/// A generational reference to an asset of type `T`. A handle of a removed asset never
/// matches a new asset.
pub struct Handle<T> {
    key: DefaultKey,
    // `fn() -> T`: the handle is `Send`, `Sync`, and `Copy` for every `T`.
    marker: PhantomData<fn() -> T>,
}

// Written by hand: `derive` would demand the same trait of `T`.
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T> Eq for Handle<T> {}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({:?})", self.key)
    }
}

/// What [`Assets::state`] tells about a handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// The loader has not finished. Ask again in a later frame.
    Loading,
    Ready,
    /// The loader logged the reason. The asset never becomes ready.
    Failed,
}

enum Slot<T> {
    Loading,
    Ready(T),
    Failed,
}

/// The store of all assets of type `T`. An ECS resource.
#[derive(Resource)]
pub struct Assets<T: Send + Sync + 'static> {
    slots: SlotMap<DefaultKey, Slot<T>>,
}

impl<T: Send + Sync + 'static> Default for Assets<T> {
    fn default() -> Self {
        Self {
            slots: SlotMap::new(),
        }
    }
}

impl<T: Send + Sync + 'static> Assets<T> {
    /// Stores an asset that exists now (generated data). It is ready at once.
    pub fn add(&mut self, asset: T) -> Handle<T> {
        self.handle(Slot::Ready(asset))
    }

    /// Makes a handle for an asset that a loader supplies later with [`Assets::finish`].
    pub fn reserve(&mut self) -> Handle<T> {
        self.handle(Slot::Loading)
    }

    /// The loader gives its result: `Some` makes the asset ready, `None` marks it failed.
    /// Does nothing if the asset was removed while it loaded.
    pub fn finish(&mut self, handle: Handle<T>, asset: Option<T>) {
        if let Some(slot) = self.slots.get_mut(handle.key) {
            *slot = asset.map_or(Slot::Failed, Slot::Ready);
        }
    }

    /// `None` while the asset loads, after it failed, and after it was removed.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match self.slots.get(handle.key) {
            Some(Slot::Ready(asset)) => Some(asset),
            _ => None,
        }
    }

    /// `None` if the asset was removed.
    pub fn state(&self, handle: Handle<T>) -> Option<LoadState> {
        self.slots.get(handle.key).map(|slot| match slot {
            Slot::Loading => LoadState::Loading,
            Slot::Ready(_) => LoadState::Ready,
            Slot::Failed => LoadState::Failed,
        })
    }

    /// Removes the asset. All handles to it become stale. Users that keep data made
    /// from the asset (the renderer keeps GPU buffers) free that data when they see
    /// that the handle is stale.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        match self.slots.remove(handle.key) {
            Some(Slot::Ready(asset)) => Some(asset),
            _ => None,
        }
    }

    /// All ready assets.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().filter_map(|(key, slot)| match slot {
            Slot::Ready(asset) => Some((Self::wrap(key), asset)),
            _ => None,
        })
    }

    fn handle(&mut self, slot: Slot<T>) -> Handle<T> {
        Self::wrap(self.slots.insert(slot))
    }

    fn wrap(key: DefaultKey) -> Handle<T> {
        Handle {
            key,
            marker: PhantomData,
        }
    }
}

/// The `[assets]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetsSettings {
    /// True: a model file is read on another thread, and shows some frames after it
    /// was asked for. False: on the thread of the game, in the frame that asks for
    /// it: each run then does the same work in each frame (a benchmark).
    pub background: bool,
    /// True: a file of an asset that another program writes again is loaded again
    /// (`AssetWatch`). False: no file is looked at after its load: a game that
    /// ships, a benchmark. The editor watches with each value.
    pub watch: bool,
}

impl Default for AssetsSettings {
    fn default() -> Self {
        Self {
            background: true,
            watch: false,
        }
    }
}

impl Settings for AssetsSettings {
    const TABLE: &'static str = "assets";
}

/// Adds the asset stores to the app, and the system that stores the models that
/// were read on other threads.
pub struct AssetsModule;

impl Module for AssetsModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<AssetsSettings>();
        let AssetsSettings { background, watch } = *app.world().resource::<AssetsSettings>();
        app.init_resource::<AssetWatch>()
            // Each store tells its own files.
            .allow_ambiguous_resource::<AssetWatch>()
            .add_message::<AssetChanged>()
            .add_message::<AssetFailed>()
            .add_systems(
                Update,
                (watch::watch_files, forget_changed, watch::watch_textures).chain(),
            );
        // The data files, as the routine files: read at the first use, forgotten at a change.
        app.init_resource::<DataTypes>()
            .init_resource::<DataAssets>()
            .add_systems(
                Update,
                (data::forget_changed_data, data::watch_data)
                    .chain()
                    .after(watch::watch_files),
            );
        app.resource_mut::<AssetWatch>().on |= watch;
        // The scene module makes the same resource: the order of the two is free.
        app.init_resource::<ModelLoads>();
        app.resource_mut::<ModelLoads>().background = background;
        app.add_systems(Update, model::finish_model_loads)
            .allow_ambiguous_resource::<ModelLoads>();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Model>>()
            .init_resource::<Textures>()
            // Each module adds its own assets and keeps the handles: the order of two
            // modules changes nothing that they can see.
            .allow_ambiguous_resource::<Assets<Mesh>>()
            .allow_ambiguous_resource::<Assets<Model>>()
            .allow_ambiguous_resource::<Textures>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_exists_before_its_asset_and_goes_stale_on_remove() {
        let mut assets = Assets::<u32>::default();
        let handle = assets.reserve();
        assert_eq!(assets.state(handle), Some(LoadState::Loading));
        assert_eq!(assets.get(handle), None);
        assert_eq!(assets.iter().count(), 0);

        assets.finish(handle, Some(7));
        assert_eq!(assets.state(handle), Some(LoadState::Ready));
        assert_eq!(assets.get(handle), Some(&7));

        let failed = assets.reserve();
        assets.finish(failed, None);
        assert_eq!(assets.state(failed), Some(LoadState::Failed));
        assert_eq!(assets.get(failed), None);

        assert_eq!(assets.remove(handle), Some(7));
        // The slot of the removed asset is used again, but the old handle does not match.
        let other = assets.add(8);
        assert_eq!(assets.state(handle), None);
        assert_eq!(assets.get(handle), None);
        assets.finish(handle, Some(9));
        assert_eq!(assets.get(other), Some(&8));
    }
}
