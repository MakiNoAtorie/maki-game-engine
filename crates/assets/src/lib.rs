//! Assets: data that is loaded or generated one time and used by many entities.
//!
//! An asset lives in an [`Assets`] store. Users hold a [`Handle`] and ask the store for
//! the data each time they need it. A handle exists before its data: [`Assets::get`]
//! returns `None` until the asset is ready. Code that obeys this rule does not change
//! when a loader becomes asynchronous or reads a different file format.

mod mesh;
mod model;

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use bevy_ecs::resource::Resource;
use maki_core::{App, Module};
use slotmap::{DefaultKey, SlotMap};

pub use mesh::{Material, Mesh};
pub use model::{Model, load_gltf};

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

/// Adds the asset stores to the app.
pub struct AssetsModule;

impl Module for AssetsModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Model>>();
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
