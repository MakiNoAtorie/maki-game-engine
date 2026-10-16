//! The components that describe the world: what is where, with no word on how it is
//! drawn. Gameplay crates write them; the render crate copies them in `Extract`.
//!
//! This crate must not depend on `gpu`, `render`, or `ui`.

mod collision;
mod mover;
mod object;
mod prefab;
mod room;

use bevy_ecs::component::Component;
use bevy_ecs::schedule::IntoScheduleConfigs;
use glam::Vec3;
use maki_assets::{Handle, Mesh};
use maki_core::{App, FixedUpdate, Module, ResetPlay, Update, interpolate_transforms};

pub use collision::{Collider, move_circle, room_colliders};
pub use mover::{Door, Mover, MoverState, move_movers};
pub use object::{
    ComponentText, ComponentType, ComponentTypes, Live, ObjectDef, ObjectId, ObjectRef,
    add_component_reader, add_component_type, add_live_component_type, add_object_component,
};
pub use prefab::{
    PrefabDef, PrefabFile, PrefabUse, change_key, changed_text, difference, instance_objects,
    key_changes_by_id, merge, prefab_text, read_prefab,
};
pub use room::{
    CurrentRoom, DoorLeaf, Filter, LightDef, LightSource, LookDef, MeshDef, MoverDef, MoverPath,
    PoseDef, Prop, PropDef, RoomCheck, RoomDef, RoomEntity, RoomItem, RoomModels, RoomOverride,
    RoomProblem, RoomRequest, RoomShell, RoomTarget, RoomValidator, RoomValidators, SoundDef,
    SpawnPoint, TextureDef, check_room, load_requested_room, parse_room, patch_room, pose_in,
    pose_out, read_room, ron_options, room_text,
};

/// Adds the systems of the world components (movers, doors) and the room loader.
pub struct SceneModule;

impl Module for SceneModule {
    fn build(&self, app: &mut App) {
        // The item kinds of a room file, for the tools.
        maki_core::add_field_type::<RoomShell>(app, "room");
        // The fields of an object but its components.
        maki_core::add_field_type::<ObjectDef>(app, "object");
        // The types of components of the scene.
        add_live_component_type::<Prop>(app, "prop", Prop::LIVE);
        add_live_component_type::<LightSource>(app, "light", LightSource::LIVE);
        add_component_type::<maki_assets::Doorway>(app, "doorway");
        add_live_component_type::<DoorLeaf>(app, "door", DoorLeaf::LIVE);
        add_component_type::<MoverPath>(app, "mover");
        add_component_type::<PrefabFile>(app, "prefab");
        add_component_type::<SpawnPoint>(app, "spawn");
        // The stores normally come from `AssetsModule`. This keeps the loader
        // independent of the order in which the modules are added.
        app.init_resource::<maki_assets::Assets<Mesh>>()
            .init_resource::<maki_assets::Assets<maki_assets::Model>>()
            .init_resource::<maki_assets::Textures>()
            // Without `AssetsModule`, a model file is read in the frame that asks
            // for it.
            .init_resource::<maki_assets::ModelLoads>()
            .init_resource::<CurrentRoom>()
            .init_resource::<RoomRequest>()
            .init_resource::<RoomModels>()
            .init_resource::<RoomValidators>()
            .init_resource::<room::RoomOverride>()
            .init_resource::<ScreenFade>()
            // Each writer asks for a room; the loader takes the last request.
            .allow_ambiguous_resource::<RoomRequest>()
            // The watch of the asset files, if `AssetsModule` is there.
            .add_message::<maki_assets::AssetChanged>()
            // Each store tells the watch its own files.
            .allow_ambiguous_resource::<maki_assets::AssetWatch>()
            // A game starts with the picture, not in the black of a fade.
            .add_systems(
                ResetPlay,
                |mut fade: bevy_ecs::system::ResMut<ScreenFade>| {
                    use bevy_ecs::change_detection::DetectChangesMut;
                    fade.set_if_neq(ScreenFade(0.0));
                },
            )
            .add_systems(
                FixedUpdate,
                (mover::move_movers, mover::block_doors).chain(),
            )
            .add_systems(
                Update,
                (
                    // Before the interpolation: a player that is moved to the spawn
                    // point of the load (the player module) shows there in this frame.
                    room::load_requested_room.before(interpolate_transforms),
                    room::spawn_loaded_models.after(interpolate_transforms),
                    // After the stores forgot a changed file: the loader reads it.
                    room::reload_changed
                        .after(maki_assets::forget_changed)
                        .before(room::load_requested_room),
                    room::watch_models.after(room::load_requested_room),
                ),
            );
    }
}

/// A name for an entity of a room file. Events find doors and lights by it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Name(pub String);

/// How dark a fade makes the picture: 0.0 is the picture, 1.0 is black. The renderer
/// reads it. The user interface is drawn over the fade.
#[derive(bevy_ecs::resource::Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct ScreenFade(pub f32);

/// Marks the entity that the player controls.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Player;

/// Draws a mesh at the `Transform` of the entity. Any module spawns entities with this
/// component; the renderer finds them all.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MeshInstance(pub Handle<Mesh>);

/// A sphere that sends out the same light in all directions. Its center is the
/// translation of the `Transform` of the entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// Radius of the sphere, in meters. A larger light gives softer shadows.
    pub radius: f32,
    /// Linear RGB.
    pub color: Vec3,
    /// Brightness as seen from far away: a surface that faces the light at a distance
    /// of `d` meters gets `color * intensity / d²`. Does not change with `radius`.
    pub intensity: f32,
    /// False: the light gives no light and does not show.
    pub on: bool,
    /// For a cone light (a flashlight): the middle of the cone, length 1.
    pub direction: Vec3,
    /// The angle from `direction` to the edge of the cone, in degrees. 180 and more:
    /// light in all directions. The outer fifth of the cone is a soft edge. In its
    /// cone, the light is as bright as a light for all directions.
    pub cone_degrees: f32,
}

/// A small white light for all directions.
impl Default for Light {
    fn default() -> Self {
        Self {
            radius: 0.1,
            color: Vec3::ONE,
            intensity: 1.0,
            on: true,
            direction: Vec3::NEG_Y,
            cone_degrees: 180.0,
        }
    }
}

/// A sound file that plays as a loop at the place of the entity (the translation of
/// its `Transform`). This crate only describes it: a sound module plays each entity
/// that has one, as the renderer draws each `MeshInstance`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct SoundSource {
    pub path: std::path::PathBuf,
    /// 1.0: as loud as the file is.
    pub volume: f32,
    /// Meters. The sound is silent for a listener that is farther away.
    pub reach: f32,
    /// False: silent.
    pub on: bool,
}

/// Marks the camera whose view the picture shows. One entity has it at a time: a
/// module that brings a second camera (an editor) moves the marker.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ActiveCamera;

/// A point of view. The picture shows that of the entity with [`ActiveCamera`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the camera is, in world space.
    pub position: Vec3,
    /// The point in world space that is in the center of the picture.
    pub target: Vec3,
    /// The angle between the top and the bottom edge of the picture, in degrees.
    pub fov_y_degrees: f32,
}

/// At the origin, looking along -Z. In use only while the game has no camera.
impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            target: Vec3::NEG_Z,
            fov_y_degrees: 60.0,
        }
    }
}
