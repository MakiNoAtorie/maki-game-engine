//! Rooms as data: the room file format, and load and unload of a room.
//!
//! A room file is RON text (`game/assets/rooms/*.ron`): the size of the room and its
//! objects ([`ObjectDef`]). [`RoomDef`] is its content. An event is the component
//! `event` of an object: the events crate reads it.
//!
//! To change the room, write a [`RoomRequest`]. `load_requested_room` then removes
//! all entities with the [`RoomEntity`] marker and spawns those of the new room. The
//! player, the camera, and the game state are not room entities: they stay.
//!
//! Assets: a mesh that the room makes from numbers (its walls, a cube) belongs to the
//! room and is removed with it. A model file that the new room does not name is
//! removed too, with its meshes: a room change leaves nothing of the old room. A
//! model that the new room names again (a reload) stays loaded, and so does a
//! generated mesh that the new room makes from the same numbers: a tool that loads
//! the room again for each step of a change (an editor) makes one new mesh, not all.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::{With, Without};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::{Mut, World};
use glam::{EulerRot, Quat, Vec3};
use maki_assets::{
    AssetChanged, AssetWatch, Assets, Doorway, FileRef, Handle, LoadState, Material, Mesh, Model,
    ModelLoads, TextureRef, Textures,
};
use maki_core::{Fields, Interpolated, Transform};
use serde::{Deserialize, Serialize};

use crate::object::{ComponentText, ComponentTypes, Live, ObjectDef, ObjectId, ObjectRef};
use crate::prefab::{PrefabUse, instance_objects, read_prefab};
use crate::{
    Collider, Door, Light, MeshInstance, Mover, MoverState, Name, SoundSource, room_colliders,
};

/// The content of a room file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomDef {
    /// Inner size in meters, along X.
    pub width: f32,
    /// Along Z.
    pub depth: f32,
    /// Along Y.
    pub height: f32,
    /// The texture of the floor of the room and of its passages. Its coordinates are
    /// meters: with `scale: 1.0` the image is one meter wide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<TextureDef>,
    /// The same for the walls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<TextureDef>,
    /// The id that the editor gives to the next new object. An id is not given
    /// again: the saved state of a deleted object cannot go to a new one.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_id: u32,
    /// The things of the room: each a name, a pose, and components.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectDef>,
}

/// The fields of a room that are of the room itself, not of a thing in it: what a
/// tool shows for "the room".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub struct RoomShell {
    #[field(label = "width (X)", range = 1.0..=100.0, step = 0.05)]
    pub width: f32,
    #[field(label = "depth (Z)", range = 1.0..=100.0, step = 0.05)]
    pub depth: f32,
    #[field(label = "height (Y)", range = 1.0..=100.0, step = 0.05)]
    pub height: f32,
    #[field(label = "floor texture")]
    pub floor: Option<TextureDef>,
    #[field(label = "wall texture")]
    pub walls: Option<TextureDef>,
}

pub(crate) fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A new id of a list of objects (a room, a prefab file) whose next id is `next_id`.
pub(crate) fn new_id(objects: &[ObjectDef], next_id: &mut u32) -> ObjectId {
    let used = objects.iter().filter_map(|object| object.id);
    let next = used.map(|id| id.0 + 1).fold((*next_id).max(1), u32::max);
    *next_id = next + 1;
    ObjectId(next)
}

/// Gives an id to each object with none, in the order. True if one had none.
pub(crate) fn give_ids(objects: &mut [ObjectDef], next_id: &mut u32) -> bool {
    let mut given = false;
    for index in 0..objects.len() {
        if objects[index].id.is_none() {
            objects[index].id = Some(new_id(objects, next_id));
            given = true;
        }
    }
    given
}

impl RoomDef {
    /// A new id, not given before in this room.
    pub fn new_id(&mut self) -> ObjectId {
        new_id(&self.objects, &mut self.next_id)
    }

    /// Gives an id to each object with none, in the order of the objects. Returns
    /// true if one had none.
    pub fn give_ids(&mut self) -> bool {
        give_ids(&mut self.objects, &mut self.next_id)
    }

    /// The first object of the name, with its index (its `RoomItem`).
    pub fn object_named(&self, name: &str) -> Option<(usize, &ObjectDef)> {
        let named = |(_, object): &(usize, &ObjectDef)| object.name.as_deref() == Some(name);
        self.objects.iter().enumerate().find(named)
    }

    /// The first object of the reference, with its index (its `RoomItem`).
    pub fn find(&self, object: &ObjectRef) -> Option<(usize, &ObjectDef)> {
        let found = |(_, def): &(usize, &ObjectDef)| object.is_object(def);
        self.objects.iter().enumerate().find(found)
    }

    /// The position of a spawn point: of the object of the reference with the
    /// component `spawn`.
    pub fn spawn(&self, spawn: &ObjectRef) -> Option<[f32; 3]> {
        let found = |object: &&ObjectDef| object.is_spawn() && spawn.is_object(object);
        Some(self.objects.iter().find(found)?.pose.position)
    }

    /// The names of the spawn points, in name order.
    pub fn spawn_names(&self) -> Vec<String> {
        let spawns = self.objects.iter().filter(|object| object.is_spawn());
        let names: BTreeSet<String> = spawns.filter_map(|object| object.name.clone()).collect();
        names.into_iter().collect()
    }

    /// Each prefab use of the room, with the index of its object. An object whose
    /// text does not read is not there; so for the two below.
    pub fn each_prefab(&self) -> Vec<(usize, PrefabUse)> {
        let objects = self.objects.iter().enumerate();
        objects
            .filter_map(|(index, object)| Some((index, object.prefab()?.ok()?)))
            .collect()
    }

    /// Each doorway of the room, in the order of the objects.
    pub fn each_doorway(&self) -> Vec<Doorway> {
        let objects = self.objects.iter();
        objects
            .filter_map(|object| object.doorway()?.ok())
            .collect()
    }

    /// Each light of the room.
    pub fn each_light(&self) -> Vec<LightDef> {
        let objects = self.objects.iter();
        objects.filter_map(|object| object.light()?.ok()).collect()
    }

    pub fn shell(&self) -> RoomShell {
        RoomShell {
            width: self.width,
            depth: self.depth,
            height: self.height,
            floor: self.floor.clone(),
            walls: self.walls.clone(),
        }
    }

    pub fn set_shell(&mut self, shell: RoomShell) {
        self.width = shell.width;
        self.depth = shell.depth;
        self.height = shell.height;
        self.floor = shell.floor;
        self.walls = shell.walls;
    }
}

// The fields of this file that have a default are not written when they have it
// (`skip_serializing_if`): a file that a tool saves is as short as one that a person
// wrote.

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Where a thing is. Each field has a default, so a file gives only what differs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields, default)]
#[fields(default)]
pub struct PoseDef {
    #[serde(skip_serializing_if = "is_default")]
    pub position: [f32; 3],
    /// Degrees around X, Y, and Z. The turn around Z is done first, then X, then Y.
    #[serde(skip_serializing_if = "is_default")]
    #[field(step = 0.5)]
    pub rotation: [f32; 3],
    #[serde(skip_serializing_if = "is_ones")]
    pub scale: [f32; 3],
}

fn is_ones(scale: &[f32; 3]) -> bool {
    *scale == [1.0; 3]
}

impl Default for PoseDef {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl PoseDef {
    pub fn transform(&self) -> Transform {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        Transform {
            translation: self.position.into(),
            rotation: Quat::from_euler(EulerRot::YXZ, y, x, z),
            scale: self.scale.into(),
        }
    }

    /// The pose of a transform. The angles are those that `transform` reads.
    pub fn of(transform: &Transform) -> Self {
        let (y, x, z) = transform.rotation.to_euler(EulerRot::YXZ);
        Self {
            position: transform.translation.into(),
            rotation: [x, y, z].map(f32::to_degrees),
            scale: transform.scale.into(),
        }
    }
}

/// The component of an object that is a place where the player can enter the room:
/// at the position of the object. Events name it by the name of the object.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct SpawnPoint {}

impl ObjectDef {
    /// A spawn point at `position`.
    pub fn spawn_point(name: String, position: [f32; 3]) -> Self {
        let mut object = ObjectDef {
            name: Some(name),
            pose: PoseDef {
                position,
                ..PoseDef::default()
            },
            ..ObjectDef::default()
        };
        object.set_component("spawn", &SpawnPoint {});
        object
    }

    /// True: the object has the component `spawn`.
    pub fn is_spawn(&self) -> bool {
        self.components.contains_key("spawn")
    }
}

/// The pose in the room of an item of a prefab: `inner` in the space of `outer`.
// ponytail: a turn, then a scale for each axis of the room. A prefab with unequal
// scale and a turned item in it keeps the shape of the item (no shear): the item is
// then not where a matrix product puts each of its points.
pub fn pose_in(outer: &Transform, inner: &Transform) -> Transform {
    Transform {
        translation: outer.translation + outer.rotation * (outer.scale * inner.translation),
        rotation: outer.rotation * inner.rotation,
        scale: outer.scale * inner.scale,
    }
}

/// The inverse of [`pose_in`]: the pose in the space of `outer` of an item that is
/// at `pose` in the room.
pub fn pose_out(outer: &Transform, pose: &Transform) -> Transform {
    let back = outer.rotation.inverse();
    Transform {
        translation: back * (pose.translation - outer.translation) / outer.scale,
        rotation: back * pose.rotation,
        scale: pose.scale / outer.scale,
    }
}

/// The shape of a prop or a mover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum MeshDef {
    /// A cube of this size with its center at the origin.
    #[field(default = MeshDef::Cube(1.0))]
    Cube(#[field(label = "size", range = 0.01..=100.0)] f32),
    /// A box between two corners.
    #[field(default = MeshDef::Cuboid([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]))]
    Cuboid(
        #[field(label = "from")] [f32; 3],
        #[field(label = "to")] [f32; 3],
    ),
    #[field(default = MeshDef::Sphere { radius: 0.5, segments: 32, rings: 16 })]
    Sphere {
        #[field(range = 0.01..=100.0)]
        radius: f32,
        #[field(range = 3.0..=256.0)]
        segments: u32,
        #[field(range = 2.0..=256.0)]
        rings: u32,
    },
    /// A glTF file (`.gltf` or `.glb`). A path that is not absolute starts at the
    /// directory of the room file. The point lights of the file become lights.
    File(#[field(label = "glTF file", file = "model")] FileRef),
    /// A cylinder with a half sphere at each end, standing on the origin: `height`
    /// from the bottom to the top.
    #[field(default = MeshDef::Capsule { radius: 0.3, height: 1.7 })]
    Capsule {
        #[field(range = 0.01..=10.0)]
        radius: f32,
        #[field(range = 0.02..=20.0)]
        height: f32,
    },
}

impl MeshDef {
    /// The mesh of a generated shape. `None` for a file.
    pub fn generate(&self) -> Option<Mesh> {
        Some(match self {
            MeshDef::Cube(size) => Mesh::cube(*size),
            MeshDef::Cuboid(min, max) => Mesh::cuboid((*min).into(), (*max).into()),
            MeshDef::Sphere {
                radius,
                segments,
                rings,
            } => Mesh::sphere(*radius, *segments, *rings),
            // The segments and rings of the player of before.
            MeshDef::Capsule { radius, height } => Mesh::capsule(*radius, *height, 24, 8),
            MeshDef::File(_) => return None,
        })
    }
}

/// A texture for a generated mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct TextureDef {
    /// An image file (PNG or JPEG). A path that is not absolute starts at the
    /// directory of the room file.
    #[field(label = "file", file = "picture")]
    pub path: FileRef,
    /// The texture coordinates are multiplied by this: 2.0 shows the image two times
    /// where it showed one time. The coordinates of the room box, a cube, and a
    /// cuboid are meters; those of a sphere go one time around it.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    #[field(default = one(), range = 0.01..=100.0)]
    pub scale: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter: Filter,
}

fn one() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

/// How a texture shows when its texels are larger than the pixels of the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Fields)]
pub enum Filter {
    /// Texels blend.
    #[default]
    Linear,
    /// Texels keep their hard edges: pixel art.
    Nearest,
}

impl TextureDef {
    /// The mesh with this texture as its color texture.
    fn apply(&self, mesh: Mesh, directory: &Path, textures: &mut Textures) -> Mesh {
        mesh.with_uv_scale(self.scale)
            .with_base_color_texture(TextureRef {
                texture: textures.load(&self.path.resolve(directory)),
                nearest: self.filter == Filter::Nearest,
            })
    }
}

/// How a generated mesh looks. A file has its own colors and materials.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields, default)]
pub struct LookDef {
    /// The color texture. The color multiplies it: give a texture the color white.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<TextureDef>,
    /// Linear RGB. `None`: the color of the generator.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[field(color, some = [1.0; 3])]
    pub color: Option<[f32; 3]>,
    /// `None`: diffuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// A light of its own, linear RGB: the mesh shows with this color and lights
    /// the room by bounce rays. `None`: no light.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[field(color, some = [1.0; 3])]
    pub emissive: Option<[f32; 3]>,
}

impl LookDef {
    fn apply(&self, mut mesh: Mesh, directory: &Path, textures: &mut Textures) -> Mesh {
        if let Some(texture) = &self.texture {
            mesh = texture.apply(mesh, directory, textures);
        }
        if let Some([r, g, b]) = self.color {
            mesh = mesh.with_base_color([r, g, b, 1.0]);
        }
        if let Some(emissive) = self.emissive {
            mesh = mesh.with_emissive(emissive);
        }
        match self.material {
            Some(material) => mesh.with_material(material),
            None => mesh,
        }
    }
}

/// A thing that does not move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub mesh: MeshDef,
    #[serde(default, skip_serializing_if = "is_default")]
    pub pose: PoseDef,
    /// How a generated mesh looks. A file has its own colors and materials.
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: LookDef,
    /// True: the box around the prop blocks the player.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub collider: bool,
    /// For a file: the radius of each light of the file, in meters.
    #[serde(
        default = "default_light_radius",
        skip_serializing_if = "is_default_light_radius"
    )]
    pub light_radius: f32,
}

impl Default for PropDef {
    /// A cube of 1 m at the origin.
    fn default() -> Self {
        Self {
            name: None,
            mesh: MeshDef::Cube(1.0),
            pose: PoseDef::default(),
            look: LookDef::default(),
            collider: true,
            light_radius: default_light_radius(),
        }
    }
}

impl PropDef {
    /// The prop as an object with the component `prop`.
    pub fn into_object(self) -> ObjectDef {
        let prop = Prop {
            mesh: self.mesh,
            look: self.look,
            collider: self.collider,
            light_radius: self.light_radius,
        };
        ObjectDef {
            id: None,
            name: self.name,
            pose: self.pose,
            parent: None,
            components: [("prop".to_owned(), ComponentText::of(&prop))].into(),
        }
    }
}

/// The component of an object that shows a mesh: a generated shape or a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
#[fields(default)]
pub struct Prop {
    pub mesh: MeshDef,
    /// How a generated mesh looks. A file has its own colors and materials.
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: LookDef,
    /// True: the box around the prop blocks the player.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[field(label = "blocks the player")]
    pub collider: bool,
    /// For a file: the radius of each light of the file, in meters.
    #[serde(
        default = "default_light_radius",
        skip_serializing_if = "is_default_light_radius"
    )]
    #[field(label = "light radius", range = 0.01..=10.0)]
    pub light_radius: f32,
}

impl Default for Prop {
    /// A cube of 1 m.
    fn default() -> Self {
        let PropDef {
            mesh,
            look,
            collider,
            light_radius,
            ..
        } = PropDef::default();
        Self {
            mesh,
            look,
            collider,
            light_radius,
        }
    }
}

impl ObjectDef {
    /// The object as a prop, if it has the component `prop`. `Err`: its text is not
    /// a prop.
    pub fn prop(&self) -> Option<Result<PropDef, ron::error::SpannedError>> {
        Some(self.component::<Prop>("prop")?.map(|prop| PropDef {
            name: self.name.clone(),
            mesh: prop.mesh,
            pose: self.pose,
            look: prop.look,
            collider: prop.collider,
            light_radius: prop.light_radius,
        }))
    }
}

fn yes() -> bool {
    true
}

fn default_light_radius() -> f32 {
    0.15
}

fn is_default_light_radius(radius: &f32) -> bool {
    *radius == default_light_radius()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub position: [f32; 3],
    /// Meters.
    pub radius: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    pub intensity: f32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub on: bool,
    /// A cone light: the direction of the middle of the cone. `None`: straight down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f32; 3]>,
    /// A cone light: degrees from `direction` to the edge of the cone. `None`: light
    /// in all directions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cone: Option<f32>,
    /// A sound that loops at the place of the light: a lamp that hums.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<SoundDef>,
}

/// A sound that loops at the place of a thing of the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
#[fields(default)]
pub struct SoundDef {
    /// A sound file (WAV or Ogg Vorbis). A path that is not absolute starts at the
    /// directory of the room file.
    #[field(label = "sound file", file = "sound")]
    pub file: FileRef,
    /// 1.0: as loud as the file is.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    #[field(range = 0.0..=4.0)]
    pub volume: f32,
    /// Meters. The sound is silent for a listener that is farther away.
    #[serde(default = "default_reach", skip_serializing_if = "is_default_reach")]
    #[field(label = "reach (radius)", range = 0.1..=100.0, step = 0.05)]
    pub reach: f32,
    /// False: silent until an event turns the sound on.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[field(label = "on at the start")]
    pub on: bool,
}

impl Default for SoundDef {
    fn default() -> Self {
        Self {
            file: FileRef::default(),
            volume: 1.0,
            reach: default_reach(),
            on: true,
        }
    }
}

fn default_reach() -> f32 {
    12.0
}

fn is_default_reach(reach: &f32) -> bool {
    *reach == default_reach()
}

impl SoundDef {
    /// The component of the sound. `directory`: of the room file.
    pub fn source(&self, directory: &Path) -> SoundSource {
        SoundSource {
            path: self.file.resolve(directory),
            volume: self.volume,
            reach: self.reach,
            on: self.on,
        }
    }
}

impl LightDef {
    /// The component of the light. Its place is `position`.
    pub fn light(&self) -> Light {
        Light {
            radius: self.radius,
            color: self.color.into(),
            intensity: self.intensity,
            on: self.on,
            direction: self
                .direction
                .and_then(|direction| Vec3::from(direction).try_normalize())
                .unwrap_or(Vec3::NEG_Y),
            cone_degrees: self.cone.unwrap_or(180.0),
        }
    }
}

/// The component of an object that gives light at the place of the object: the
/// fields of [`LightDef`] without its name and its place, which are of the object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct LightSource {
    /// Meters.
    #[field(range = 0.01..=10.0, step = 0.005)]
    pub radius: f32,
    /// Linear RGB.
    #[field(color)]
    pub color: [f32; 3],
    #[field(range = 0.0..=10000.0, step = 0.1)]
    pub intensity: f32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[field(default = true)]
    pub on: bool,
    /// A cone light: the direction of the middle of the cone. `None`: straight down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[field(label = "direction of the cone (not set: down)", some = [0.0, -1.0, 0.0])]
    pub direction: Option<[f32; 3]>,
    /// A cone light: degrees from `direction` to the edge of the cone. `None`: light
    /// in all directions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[field(label = "cone (degrees to the edge)", range = 1.0..=180.0, step = 0.5, some = 30.0)]
    pub cone: Option<f32>,
    /// A sound that loops at the place of the light: a lamp that hums.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<SoundDef>,
}

impl LightSource {
    /// The live form of the component `light`: the tree of a [`Light`] of the game,
    /// and a tree written back into it. The sound is another component: it is not a
    /// live field.
    pub(crate) const LIVE: Live = Live {
        fields: Some(&["radius", "color", "intensity", "on", "direction", "cone"]),
        get: |world, entity| {
            let light = world.get::<Light>(entity)?;
            let source = LightSource {
                radius: light.radius,
                color: light.color.into(),
                intensity: light.intensity,
                on: light.on,
                direction: Some(light.direction.into()),
                cone: (light.cone_degrees < 180.0).then_some(light.cone_degrees),
                sound: None,
            };
            Some(source.tree())
        },
        set: |world, entity, tree| {
            let source = maki_core::fields::read::<LightSource>(tree).map_err(|e| e.to_string())?;
            let mut light = world
                .get_mut::<Light>(entity)
                .ok_or("the entity has no light")?;
            light.set_if_neq(LightDef::from_source(source).light());
            Ok(())
        },
    };
}

impl DoorLeaf {
    /// The live form of the component `door`: `seconds`, and `open` (a closed door
    /// opens). The color is of the mesh: a new one needs work of the renderer.
    pub(crate) const LIVE: Live = Live {
        fields: Some(&["seconds", "open"]),
        get: |world, entity| {
            world.get::<Door>(entity)?;
            let mover = world.get::<Mover>(entity)?;
            let leaf = DoorLeaf {
                seconds: mover.duration,
                color: default_door_color(),
                open: matches!(mover.state, MoverState::AtB | MoverState::ToB(_)),
            };
            Some(leaf.tree())
        },
        set: |world, entity, tree| {
            let leaf = maki_core::fields::read::<DoorLeaf>(tree).map_err(|e| e.to_string())?;
            let mut mover = world
                .get_mut::<Mover>(entity)
                .ok_or("the entity has no door")?;
            let mut new = *mover;
            new.duration = leaf.seconds;
            if leaf.open {
                new.go_to_b();
            } else {
                new.go_to_a();
            }
            mover.set_if_neq(new);
            Ok(())
        },
    };
}

impl LightDef {
    /// A light at the origin with no name, with the values of the component.
    fn from_source(source: LightSource) -> Self {
        let LightSource {
            radius,
            color,
            intensity,
            on,
            direction,
            cone,
            sound,
        } = source;
        LightDef {
            name: None,
            position: [0.0; 3],
            radius,
            color,
            intensity,
            on,
            direction,
            cone,
            sound,
        }
    }

    /// The light as an object with the component `light`.
    pub fn into_object(self) -> ObjectDef {
        let LightDef {
            name,
            position,
            radius,
            color,
            intensity,
            on,
            direction,
            cone,
            sound,
        } = self;
        let light = LightSource {
            radius,
            color,
            intensity,
            on,
            direction,
            cone,
            sound,
        };
        ObjectDef {
            id: None,
            name,
            pose: PoseDef {
                position,
                ..PoseDef::default()
            },
            parent: None,
            components: [("light".to_owned(), ComponentText::of(&light))].into(),
        }
    }
}

impl ObjectDef {
    /// The object as a light, if it has the component `light`. Its place is the
    /// position of the object. `Err`: its text is not a light.
    pub fn light(&self) -> Option<Result<LightDef, ron::error::SpannedError>> {
        Some(
            self.component::<LightSource>("light")?
                .map(|light| LightDef {
                    name: self.name.clone(),
                    position: self.pose.position,
                    radius: light.radius,
                    color: light.color,
                    intensity: light.intensity,
                    on: light.on,
                    direction: light.direction,
                    cone: light.cone,
                    sound: light.sound,
                }),
        )
    }

    /// Writes `light` as the component `light`, and its place as the position of the
    /// object. Its name is not taken.
    pub fn set_light(&mut self, light: LightDef) {
        self.pose.position = light.position;
        let object = light.into_object();
        let text = object.components["light"].clone();
        self.components.insert("light".to_owned(), text);
    }
}

impl Prop {
    /// The live form of the component `prop`: only `collider`. A new mesh or look needs
    /// work of the renderer. The entity of a generated mesh: a door or a mover is not one.
    // ponytail: a file mesh has an entity per mesh; only the first one changes.
    pub(crate) const LIVE: Live = Live {
        fields: Some(&["collider"]),
        get: |world, entity| {
            let entity = world.get_entity(entity).ok()?;
            if !entity.contains::<MeshInstance>() || entity.contains::<Mover>() {
                return None;
            }
            let collider = entity.contains::<Collider>();
            Some(
                Prop {
                    collider,
                    ..Prop::default()
                }
                .tree(),
            )
        },
        set: |world, entity, tree| {
            let prop = maki_core::fields::read::<Prop>(tree).map_err(|e| e.to_string())?;
            let found = world.get_entity(entity).map_err(|e| e.to_string())?;
            if found.contains::<Collider>() == prop.collider {
                return Ok(());
            }
            if !prop.collider {
                world.entity_mut(entity).remove::<Collider>();
                return Ok(());
            }
            let (Some(mesh), Some(transform)) =
                (found.get::<MeshInstance>(), found.get::<Transform>())
            else {
                return Err("the prop has no mesh or no place".to_owned());
            };
            let meshes = world.resource::<Assets<Mesh>>();
            let mesh = meshes
                .get(mesh.0)
                .ok_or("the mesh of the prop is not loaded")?;
            let collider = collider_around(mesh, transform);
            world.entity_mut(entity).insert(collider);
            Ok(())
        },
    };
}

/// The component of an object that is a door in the doorway of the same object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct DoorLeaf {
    /// Seconds from closed to open.
    #[serde(
        default = "default_door_seconds",
        skip_serializing_if = "is_default_door_seconds"
    )]
    #[field(
        label = "seconds to open",
        range = 0.05..=60.0,
        step = 0.05,
        default = default_door_seconds()
    )]
    pub seconds: f32,
    /// Linear RGB.
    #[serde(
        default = "default_door_color",
        skip_serializing_if = "is_default_door_color"
    )]
    #[field(color)]
    pub color: [f32; 3],
    /// True: the door is open when the room loads.
    #[serde(default, skip_serializing_if = "is_default")]
    #[field(label = "open at the start", default = false)]
    pub open: bool,
}

fn default_door_seconds() -> f32 {
    1.2
}

fn is_default_door_seconds(seconds: &f32) -> bool {
    *seconds == default_door_seconds()
}

fn default_door_color() -> [f32; 3] {
    [0.35, 0.2, 0.1]
}

fn is_default_door_color(color: &[f32; 3]) -> bool {
    *color == default_door_color()
}

/// A thing that moves between two poses: see [`Mover`]. Its mesh is a generated shape.
#[derive(Debug, Clone, PartialEq)]
pub struct MoverDef {
    pub name: Option<String>,
    pub mesh: MeshDef,
    pub look: LookDef,
    /// The pose at the start.
    pub a: PoseDef,
    pub b: PoseDef,
    /// Seconds from `a` to `b`.
    pub seconds: f32,
}

/// The component of an object that moves between the pose of the object (`a` of
/// [`MoverDef`]) and a second pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct MoverPath {
    pub mesh: MeshDef,
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: LookDef,
    /// In the space of the room, as the pose of the object.
    #[field(label = "second pose (b)")]
    pub b: PoseDef,
    /// Seconds from the pose of the object to `b`.
    #[field(label = "seconds from a to b", range = 0.05..=600.0, step = 0.05)]
    pub seconds: f32,
}

impl MoverDef {
    /// The mover as an object with the component `mover`. Pose `a` is the pose of
    /// the object.
    pub fn into_object(self) -> ObjectDef {
        let mut object = ObjectDef {
            name: self.name.clone(),
            ..ObjectDef::default()
        };
        object.set_mover(self);
        object
    }
}

impl ObjectDef {
    /// The opening of the object in a wall, if it has the component `doorway`.
    pub fn doorway(&self) -> Option<Result<Doorway, ron::error::SpannedError>> {
        self.component("doorway")
    }

    /// The door of the object, if it has the component `door`. It is in the doorway
    /// of the object.
    pub fn door(&self) -> Option<Result<DoorLeaf, ron::error::SpannedError>> {
        self.component("door")
    }

    /// The object as a mover, if it has the component `mover`.
    pub fn mover(&self) -> Option<Result<MoverDef, ron::error::SpannedError>> {
        Some(self.component::<MoverPath>("mover")?.map(|path| MoverDef {
            name: self.name.clone(),
            mesh: path.mesh,
            look: path.look,
            a: self.pose,
            b: path.b,
            seconds: path.seconds,
        }))
    }

    /// Writes `mover` as the component `mover`, and its pose `a` as the pose of the
    /// object. Its name is not taken.
    pub fn set_mover(&mut self, mover: MoverDef) {
        self.pose = mover.a;
        let path = MoverPath {
            mesh: mover.mesh,
            look: mover.look,
            b: mover.b,
            seconds: mover.seconds,
        };
        self.set_component("mover", &path);
    }
}

/// The RON dialect of the room files. The events crate reads the same files with it.
/// An optional field takes its value without `Some(...)`.
pub fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// The text of a room file of `width` x `depth` x `height` meters with these objects,
/// each in the RON form of an object: for a test that writes a room. The form of the
/// file is here only, so a change of the format changes no test.
pub fn room_text(width: f32, depth: f32, height: f32, objects: &[&str]) -> String {
    let objects = objects.join(",\n");
    format!("(width: {width:?}, depth: {depth:?}, height: {height:?}, objects: [\n{objects}\n])")
}

/// The room in a RON text. The error has the line and the column.
pub fn parse_room(text: &str) -> Result<RoomDef, ron::error::SpannedError> {
    ron_options().from_str(text)
}

/// Reads and checks a room file. Returns the room and the text of the file. The error
/// names the file, and the line for a syntax error.
pub fn read_room(path: &Path, validators: &RoomValidators) -> Result<(RoomDef, String)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read room file {}", path.display()))?;
    let room = check_room(path, &text, validators)?;
    // Not in `check_room`: a tool opens a room whose prefab file is not there, so
    // that the name can be set right. The loader then shows the room without it.
    let directory = path.parent().unwrap_or(Path::new(""));
    for (_, prefab) in room.each_prefab() {
        read_prefab(&prefab.file.resolve(directory))
            .with_context(|| format!("{}: a prefab of the room", path.display()))?;
    }
    Ok((room, text))
}

/// Reads and checks the text of a room file. `path` is for the error text.
pub fn check_room(path: &Path, text: &str, validators: &RoomValidators) -> Result<RoomDef> {
    let room = parse_room(text).map_err(|error| anyhow!("{}:{error}", path.display()))?;
    for (index, object) in room.objects.iter().enumerate() {
        let errors = [
            ("prop", object.prop().and_then(Result::err)),
            ("light", object.light().and_then(Result::err)),
            ("doorway", object.doorway().and_then(Result::err)),
            ("door", object.door().and_then(Result::err)),
            ("mover", object.mover().and_then(Result::err)),
            ("prefab", object.prefab().and_then(Result::err)),
            (
                "spawn",
                object
                    .component::<SpawnPoint>("spawn")
                    .and_then(Result::err),
            ),
        ];
        if let Some((component, Some(error))) = errors.into_iter().find(|(_, e)| e.is_some()) {
            let name = object.name.as_deref().unwrap_or("");
            return Err(anyhow!(
                "{}: object {index} {name}: component {component}: {error}",
                path.display()
            ));
        }
    }
    for (index, object) in room.objects.iter().enumerate() {
        let name = object.name.as_deref().unwrap_or("");
        if let Some(parent) = &object.parent
            && room.find(parent).is_none()
        {
            return Err(anyhow!(
                "{}: object {index} {name}: no parent object `{parent}`",
                path.display()
            ));
        }
        if object.door().is_some() && object.doorway().is_none() {
            return Err(anyhow!(
                "{}: object {index} {name}: a door needs a doorway in the same object",
                path.display()
            ));
        }
    }
    let check = RoomCheck {
        path,
        room: &room,
        text,
    };
    let mut errors = Vec::new();
    for problem in validators.0.iter().flat_map(|validate| validate(&check)) {
        let line = format!("{}:{}", path.display(), problem.text);
        if problem.warning {
            tracing::warn!("{line}");
        } else {
            errors.push(line);
        }
    }
    if !errors.is_empty() {
        return Err(anyhow!("{}", errors.join("\n")));
    }
    Ok(room)
}

/// Checks of other crates for a room file. A room with an error of a check is not
/// loaded; a warning is logged. The events crate checks the events with this, so a
/// transfer into a room with a bad event does not happen.
#[derive(Resource, Default)]
pub struct RoomValidators(pub Vec<RoomValidator>);

/// Returns what is wrong in a room file.
// A closure, not a plain function: the check of the events holds the command registry.
pub type RoomValidator = Box<dyn Fn(&RoomCheck) -> Vec<RoomProblem> + Send + Sync>;

/// What a [`RoomValidator`] gets: a room file that reads as a room.
pub struct RoomCheck<'a> {
    /// The file. Its directory is where the files that the room names are.
    pub path: &'a Path,
    pub room: &'a RoomDef,
    pub text: &'a str,
}

/// One thing that a [`RoomValidator`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomProblem {
    /// False: an error, and the room is not loaded.
    pub warning: bool,
    /// The reason, with the line first (`3: ...` or `3:7: ...`) if it has one.
    pub text: String,
}

/// Marks an entity that belongs to the current room. Unload removes it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct RoomEntity;

/// The object of the room file that an entity was made from: its index in
/// [`RoomDef::objects`]. A tool finds the entities of an object, and the object of an
/// entity, with it. The entities of a model file and of a prefab file all have the
/// object that names the file.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoomItem(pub usize);

/// Ask for a room: write `Some`. `load_requested_room` takes it in the next `Update`.
#[derive(Resource, Debug, Default)]
pub struct RoomRequest(pub Option<RoomTarget>);

#[derive(Debug, Clone, PartialEq)]
pub struct RoomTarget {
    pub path: PathBuf,
    /// The spawn point of the room where the player goes. `None`: the player stays
    /// where it is (a reload of the same room).
    pub spawn: Option<ObjectRef>,
    /// `Some`: the text of the room, in place of the text of the file (a tool shows a
    /// room that is not saved). `path` then is the name of the room, and its
    /// directory is where the files that the room names are. The validators do not
    /// run for it: the tool shows what is wrong in its room, and shows the room.
    pub text: Option<String>,
}

/// A text that the loader takes in place of the file of a path. A tool sets it while
/// it plays a room that is not saved: a `Transfer` away and back shows the room of
/// the tool, not the file on the disk. As for `RoomTarget::text`, the validators do
/// not run for it.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct RoomOverride(pub Option<(PathBuf, String)>);

/// The room that is loaded. Change detection tells a system that the room changed.
#[derive(Resource, Debug, Default)]
pub struct CurrentRoom {
    /// Empty before the first room.
    pub path: PathBuf,
    pub def: RoomDef,
    /// The text of the room file.
    pub source: String,
    /// The number of rooms that were loaded, with this one. A system that waits for
    /// a room change compares it: a reload gives the same path.
    pub loads: u64,
    /// The time that the loader took for this room: the file, the generated meshes,
    /// the model files that were not loaded before. Without the upload to the GPU.
    pub load_time: std::time::Duration,
    /// The place of the spawn point that the load asked for. The player module moves
    /// the player there.
    pub spawn: Option<[f32; 3]>,
    /// The generated meshes of the room, each with its key: the numbers that it was
    /// made from, as a text. Removed with the room, but for those that the next
    /// room makes from the same numbers.
    meshes: Vec<(String, Handle<Mesh>)>,
}

/// The models of the current room, by path. `load_requested_room` removes those that
/// the next room does not name.
#[derive(Resource, Default)]
pub struct RoomModels {
    by_path: HashMap<PathBuf, Handle<Model>>,
    /// What a file that changed on the disk made old (`reload_changed`): the models
    /// of the file, and the generated meshes with its texture. The room that is
    /// loaded next does not take them, and removes them.
    old_models: Vec<Handle<Model>>,
    old_meshes: Vec<Handle<Mesh>>,
}

/// Removes a model and its meshes from the stores.
fn remove_model(model: Handle<Model>, models: &mut Assets<Model>, meshes: &mut Assets<Mesh>) {
    // `None` for a model that failed to load: it has no meshes.
    let removed = models.remove(model);
    for mesh in removed.map_or(Vec::new(), |model| model.meshes) {
        meshes.remove(mesh);
    }
}

/// Runs in `Update`, after the stores forgot the files that changed on the disk
/// (`maki_assets::AssetChanged`) and before the room loader: what the room has of
/// such a file is marked as old, and the room is asked for again, as it shows now
/// and with the player in place. The loader then reads the file again.
pub(crate) fn reload_changed(
    mut changed: MessageReader<AssetChanged>,
    textures: Res<Textures>,
    meshes: Res<Assets<Mesh>>,
    models: Res<Assets<Model>>,
    current: Res<CurrentRoom>,
    mut cache: ResMut<RoomModels>,
    mut request: ResMut<RoomRequest>,
) {
    if changed.is_empty() {
        return;
    }
    let files: Vec<&Path> = changed
        .read()
        .map(|AssetChanged(path)| path.as_path())
        .collect();
    // A texture of an image file has the name of the file. A texture that did not
    // load (the file was not there, or broken) is tried again too.
    let of_file = |texture: Option<TextureRef>| {
        texture.is_some_and(|texture| match textures.get(texture.texture) {
            Some(texture) => files.contains(&Path::new(&texture.name)),
            None => true,
        })
    };
    let uses = |mesh: &Handle<Mesh>| {
        let mesh = meshes.get(*mesh);
        mesh.is_some_and(|mesh| {
            of_file(mesh.base_color_texture()) || of_file(mesh.emissive_texture())
        })
    };
    let old_meshes = current.meshes.iter().map(|(_, mesh)| *mesh);
    let old_meshes: Vec<Handle<Mesh>> = old_meshes.filter(uses).collect();
    // The model of a file that changed, and a model with an image file that changed.
    let old = |path: &PathBuf, model: &Handle<Model>| {
        let model = models.get(*model);
        files.contains(&path.as_path()) || model.is_some_and(|model| model.meshes.iter().any(uses))
    };
    let models_of = cache
        .by_path
        .iter()
        .filter(|(path, model)| old(path, model));
    let old_models: Vec<PathBuf> = models_of.map(|(path, _)| path.clone()).collect();
    // A prefab file is read at each load of the room.
    let prefabs = prefab_files(&current);
    let prefab = files
        .iter()
        .any(|file| prefabs.iter().any(|prefab| prefab == file));
    if old_meshes.is_empty() && old_models.is_empty() && !prefab {
        return;
    }
    for path in old_models {
        let model = cache.by_path.remove(&path).expect("found above");
        cache.old_models.push(model);
    }
    cache.old_meshes.extend(old_meshes);
    // A request that waits (a room change) loads the files again in any case.
    if request.0.is_none() && current.loads > 0 {
        request.0 = Some(RoomTarget {
            path: current.path.clone(),
            spawn: None,
            // The room as it shows, not as its file is: a tool shows a document.
            text: Some(current.source.clone()),
        });
    }
}

/// Runs in `Update`, after the room loader: tells the watch the model files and the
/// prefab files of the room.
pub(crate) fn watch_models(
    cache: Res<RoomModels>,
    current: Res<CurrentRoom>,
    watch: Option<ResMut<AssetWatch>>,
) {
    use bevy_ecs::change_detection::DetectChanges;

    let Some(mut watch) = watch.filter(|watch| watch.on) else {
        return;
    };
    if cache.is_changed() {
        let check: maki_assets::Check = maki_assets::check_model;
        let files = cache.by_path.keys().map(|path| (path.clone(), check));
        watch.set("models", files.collect());
    }
    if current.is_changed() {
        let check: maki_assets::Check = |path| read_prefab(path).map(drop);
        let files = prefab_files(&current).into_iter();
        watch.set("prefabs", files.map(|path| (path, check)).collect());
    }
}

/// The prefab files that the current room names.
fn prefab_files(current: &CurrentRoom) -> Vec<PathBuf> {
    let directory = current.path.parent().unwrap_or(Path::new(""));
    let prefabs = current.def.each_prefab().into_iter();
    prefabs
        .map(|(_, used)| used.file.resolve(directory))
        .collect()
}

/// A model of a prop while it loads. `spawn_loaded_models` replaces the entity with
/// one entity for each mesh and each light of the model.
#[derive(Component)]
pub(crate) struct PendingModel {
    model: Handle<Model>,
    collider: bool,
    light_radius: f32,
    item: RoomItem,
}

/// Runs in `Update`: unloads the current room and loads the requested one. A room
/// file that is bad, or that has no spawn point of the requested name, changes
/// nothing: the error is logged.
#[allow(clippy::too_many_arguments)]
pub fn load_requested_room(
    mut commands: Commands,
    mut request: ResMut<RoomRequest>,
    mut current: ResMut<CurrentRoom>,
    validators: Res<RoomValidators>,
    room_override: Res<RoomOverride>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
    mut cache: ResMut<RoomModels>,
    mut loads: ResMut<ModelLoads>,
    types: Res<ComponentTypes>,
    room_entities: Query<Entity, With<RoomEntity>>,
) {
    // No `take` before this test: a write marks the request as changed in each frame.
    if request.0.is_none() {
        return;
    }
    let mut target = request.0.take().expect("tested above");
    let start = std::time::Instant::now();
    let of_tool = room_override.0.as_ref();
    let of_tool = of_tool.filter(|(path, _)| *path == target.path);
    let text = target.text.take().or(of_tool.map(|(_, text)| text.clone()));
    let read = match text {
        Some(text) => {
            check_room(&target.path, &text, &RoomValidators::default()).map(|def| (def, text))
        }
        None => read_room(&target.path, &validators),
    };
    let loaded = read.and_then(|(def, source)| {
        let spawn =
            match &target.spawn {
                Some(spawn) => Some(def.spawn(spawn).with_context(|| {
                    format!("{}: no spawn point `{spawn}`", target.path.display())
                })?),
                None => None,
            };
        Ok((def, source, spawn))
    });
    let (def, source, spawn) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            tracing::error!("room not loaded: {error:#}");
            return;
        }
    };

    for entity in &room_entities {
        commands.entity(entity).despawn();
    }
    let directory = target.path.parent().unwrap_or(Path::new(""));
    // The models that only the old room used. Without this, each room that was shown
    // keeps its meshes on the GPU, and the mesh table of the renderer gets full.
    // ponytail: the models of the props of the room. A model that only a prefab
    // names is read again in the next room that names the prefab.
    let named: Vec<PathBuf> = def
        .objects
        .iter()
        .filter_map(|object| object.prop()?.ok())
        .filter_map(|prop| match &prop.mesh {
            MeshDef::File(path) => Some(path.resolve(directory)),
            _ => None,
        })
        .collect();
    let cache = &mut *cache;
    cache.by_path.retain(|path, model| {
        let keep = named.contains(path);
        if !keep {
            remove_model(*model, &mut models, &mut meshes);
        }
        keep
    });
    // The models of files that changed on the disk: this room reads the files again.
    for model in cache.old_models.drain(..) {
        remove_model(model, &mut models, &mut meshes);
    }
    // A generated mesh with a texture of such a file is not taken: no key is empty.
    let mut old = std::mem::take(&mut current.meshes);
    for (key, mesh) in &mut old {
        if cache.old_meshes.contains(mesh) {
            key.clear();
        }
    }
    cache.old_meshes.clear();
    let mut spawner = Spawner {
        commands: &mut commands,
        types: &types,
        meshes: &mut meshes,
        models: &mut models,
        textures: &mut textures,
        cache,
        loads: &mut loads,
        directory,
        old,
        owned: Vec::new(),
        made: 0,
        id: None,
    };
    spawner.room(&def);
    let (owned, unused) = (spawner.owned, spawner.old);
    let reused = owned.len() - spawner.made;
    // The meshes of the old room that the new one did not take.
    for (_, mesh) in unused {
        meshes.remove(mesh);
    }
    // The textures of the old room that no mesh uses now. The models of the new
    // room are loaded: their meshes are in the store.
    let removed = textures.remove_unused(&meshes);
    if removed > 0 {
        tracing::info!(removed, "textures of the old room removed");
    }
    tracing::info!(
        path = %target.path.display(),
        objects = def.objects.len(),
        meshes_kept = reused,
        time = ?start.elapsed(),
        "room loaded"
    );
    *current = CurrentRoom {
        path: target.path,
        def,
        source,
        loads: current.loads + 1,
        load_time: start.elapsed(),
        spawn,
        meshes: owned,
    };
}

/// Spawns the entities of a room.
struct Spawner<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    /// For the changes of a prefab instance.
    types: &'a ComponentTypes,
    meshes: &'a mut Assets<Mesh>,
    models: &'a mut Assets<Model>,
    textures: &'a mut Textures,
    cache: &'a mut RoomModels,
    loads: &'a mut ModelLoads,
    /// Of the room file.
    directory: &'a Path,
    /// The generated meshes of the room before, with their keys: this room takes
    /// those that it would make again.
    old: Vec<(String, Handle<Mesh>)>,
    /// The generated meshes of this room, with their keys.
    owned: Vec<(String, Handle<Mesh>)>,
    /// How many of `owned` were made, not taken from `old`.
    made: usize,
    /// The id of the room object whose entities are made now. The entities of a
    /// prefab instance get the id of the instance.
    id: Option<ObjectId>,
}

impl Spawner<'_, '_, '_> {
    /// The mesh of a key: that of the room before if it had one with the key, else
    /// the one that `make` gives. Two things of a room with the same key have a mesh
    /// each, as before: one mesh of the old room goes to one thing.
    ///
    /// The key must have each number that the mesh is made from. The directory of
    /// the room file is a part of each key: a texture path starts there.
    fn mesh(&mut self, key: String, make: impl FnOnce(&mut Self) -> Mesh) -> Handle<Mesh> {
        let key = format!("{}|{key}", self.directory.display());
        let kept = self.old.iter().position(|(old, _)| *old == key);
        let handle = match kept {
            Some(index) => self.old.swap_remove(index).1,
            None => {
                self.made += 1;
                let mesh = make(self);
                self.meshes.add(mesh)
            }
        };
        self.owned.push((key, handle));
        handle
    }

    fn room(&mut self, def: &RoomDef) {
        let (width, depth, height) = (def.width, def.depth, def.height);
        let doorways = def.each_doorway();
        let size = format!("room {width} {depth} {height} {doorways:?}");
        let shell = if def.floor.is_none() && def.walls.is_none() {
            vec![self.mesh(size, |_| {
                Mesh::room_with_doorways(width, depth, height, &doorways)
            })]
        } else {
            // A mesh has one material: the floor and the walls are two meshes.
            let parts = [(0, "floor", &def.floor), (1, "walls", &def.walls)];
            let part = |(index, name, texture): (usize, &str, &Option<TextureDef>)| {
                self.mesh(format!("{size} {name} {texture:?}"), |spawner| {
                    let parts = Mesh::room_parts(width, depth, height, &doorways);
                    let mesh = if index == 0 { parts.0 } else { parts.1 };
                    match texture {
                        Some(texture) => texture.apply(mesh, spawner.directory, spawner.textures),
                        None => mesh,
                    }
                })
            };
            parts.map(part).into()
        };
        for mesh in shell {
            self.commands
                .spawn((RoomEntity, Transform::IDENTITY, MeshInstance(mesh)));
        }
        for collider in room_colliders(width, depth, height, &doorways) {
            self.commands.spawn((RoomEntity, collider));
        }
        for (index, object) in def.objects.iter().enumerate() {
            self.object(object, RoomItem(index), width, depth);
        }
    }

    /// The entities of the components of the scene of an object. `check_room` read
    /// the text of each of them.
    ///
    /// The components of `bevy_ecs` of the object (`add_object_component`: the
    /// scripts) go to one entity, the object entity: that of its prop with a
    /// generated mesh, of its light, door, or mover, else a new one. It has the pose
    /// of the object as `Interpolated`, so a script that moves it moves the mesh.
    fn object(&mut self, object: &ObjectDef, item: RoomItem, width: f32, depth: f32) {
        self.id = object.id;
        let mut main = None;
        if let Some(Ok(prop)) = object.prop() {
            main = self.prop(&prop, item);
        }
        let of_prop = main.is_some();
        if let Some(Ok(light)) = object.light() {
            main = main.or(Some(self.light(&light, item)));
        }
        if let (Some(Ok(door)), Some(Ok(doorway))) = (object.door(), object.doorway()) {
            let door = self.door(&object.name, &door, item, &doorway, width, depth);
            main = main.or(Some(door));
        }
        if let Some(Ok(mover)) = object.mover() {
            main = main.or(self.mover(&mover, item));
        }
        if let Some(Ok(used)) = object.prefab() {
            self.prefab(&used, item, width, depth);
        }

        let inserts: Vec<_> = object
            .components
            .iter()
            .filter_map(|(name, text)| Some((name, text, self.types.get(name)?.insert?)))
            .collect();
        if inserts.is_empty() {
            return;
        }
        let mut entity = match main {
            Some(entity) => self.commands.entity(entity),
            None => {
                let mut entity = self.commands.spawn((RoomEntity, item));
                if let Some(name) = &object.name {
                    entity.insert(Name(name.clone()));
                }
                if let Some(id) = self.id {
                    entity.insert(id);
                }
                entity
            }
        };
        // A light, a door, and a mover have their pose already; a prop has a
        // `Transform` only.
        if of_prop || main.is_none() {
            entity.insert(Interpolated::at(object.pose.transform()));
        }
        for (name, text, insert) in inserts {
            if let Err(error) = insert(text, &mut entity) {
                let object = object.name.as_deref().unwrap_or("");
                tracing::error!("object {object}: component {name} left out: {error}");
            }
        }
    }

    fn light(&mut self, light: &LightDef, item: RoomItem) -> Entity {
        let mut entity = self.commands.spawn((
            RoomEntity,
            item,
            // A light can move in fixed steps (a script, a debug control).
            Interpolated::at(Transform::from_translation(light.position.into())),
            light.light(),
        ));
        if let Some(name) = &light.name {
            entity.insert(Name(name.clone()));
        }
        if let Some(id) = self.id {
            entity.insert(id);
        }
        if let Some(sound) = &light.sound {
            entity.insert(sound.source(self.directory));
        }
        entity.id()
    }

    /// The items of a prefab file, each at its pose in the room. A file that does
    /// not read: an error in the log, and the room shows without it.
    /// The objects of a prefab file, each at its place in the room, with the changes
    /// of the instance. A file that does not read: an error in the log, and the room
    /// shows without it.
    // ponytail: the file is read at each load of the room. A store by path when a
    // room names many prefabs.
    fn prefab(&mut self, used: &PrefabUse, item: RoomItem, width: f32, depth: f32) {
        let prefab = match read_prefab(&used.file.resolve(self.directory)) {
            Ok(prefab) => prefab,
            Err(error) => {
                tracing::error!("prefab not shown: {error:#}");
                return;
            }
        };
        let id = self.id;
        for mut object in instance_objects(used, &prefab, self.types) {
            object.id = id;
            self.object(&object, item, width, depth);
        }
    }

    /// The entity of a generated mesh. `None` for a file: its meshes come later.
    fn prop(&mut self, prop: &PropDef, item: RoomItem) -> Option<Entity> {
        let transform = prop.pose.transform();
        let file = matches!(prop.mesh, MeshDef::File(_));
        let mut entity = match &prop.mesh {
            MeshDef::File(path) => {
                let path = path.resolve(self.directory);
                let (models, meshes) = (&mut *self.models, &mut *self.meshes);
                let (textures, loads) = (&mut *self.textures, &mut *self.loads);
                let model = *self
                    .cache
                    .by_path
                    .entry(path.clone())
                    .or_insert_with(|| loads.load(&path, models, meshes, textures));
                self.commands.spawn((
                    RoomEntity,
                    transform,
                    PendingModel {
                        model,
                        collider: prop.collider,
                        light_radius: prop.light_radius,
                        item,
                    },
                ))
            }
            generated => {
                let handle = self.mesh(format!("{generated:?} {:?}", prop.look), |spawner| {
                    let mesh = generated.generate().expect("not a file");
                    prop.look.apply(mesh, spawner.directory, spawner.textures)
                });
                let mesh = self.meshes.get(handle).expect("a generated mesh is ready");
                let collider = prop.collider.then(|| collider_around(mesh, &transform));
                let mut entity =
                    self.commands
                        .spawn((RoomEntity, item, transform, MeshInstance(handle)));
                if let Some(collider) = collider {
                    entity.insert(collider);
                }
                entity
            }
        };
        if let Some(name) = &prop.name {
            entity.insert(Name(name.clone()));
        }
        if let Some(id) = self.id {
            entity.insert(id);
        }
        (!file).then(|| entity.id())
    }

    fn mover(&mut self, mover: &MoverDef, item: RoomItem) -> Option<Entity> {
        if matches!(mover.mesh, MeshDef::File(_)) {
            tracing::error!(name = ?mover.name, "a mover cannot have a file as its mesh: left out");
            return None;
        }
        let handle = self.mesh(format!("{:?} {:?}", mover.mesh, mover.look), |spawner| {
            let mesh = mover.mesh.generate().expect("not a file");
            mover.look.apply(mesh, spawner.directory, spawner.textures)
        });
        let (a, b) = (mover.a.transform(), mover.b.transform());
        let mut entity = self.commands.spawn((
            RoomEntity,
            item,
            MeshInstance(handle),
            Interpolated::at(a),
            Mover::new(a, b, mover.seconds),
        ));
        if let Some(name) = &mover.name {
            entity.insert(Name(name.clone()));
        }
        if let Some(id) = self.id {
            entity.insert(id);
        }
        Some(entity.id())
    }

    /// The door leaf: a thin box that fills the doorway when it is closed.
    fn door(
        &mut self,
        name: &Option<String>,
        door: &DoorLeaf,
        item: RoomItem,
        doorway: &Doorway,
        width: f32,
        depth: f32,
    ) -> Entity {
        const THICKNESS: f32 = 0.06;
        // The open door stands at this angle to the wall, in degrees. Not a quarter
        // turn: the open door then does not lie in the side of the passage.
        const OPEN_DEGREES: f32 = 85.0;

        let (start, along, length, inward) = doorway.wall.frame(width, depth);
        let (from, _) = doorway.span(length);
        // The origin of the mesh is on the hinge, and the leaf goes along +X. The two
        // poses of the door then differ only in the rotation, and the blend between
        // them is a turn around the hinge.
        let [r, g, b] = door.color;
        let key = format!("door {} {} {:?}", doorway.width, doorway.height, door.color);
        let handle = self.mesh(key, |_| {
            Mesh::cuboid(
                Vec3::new(0.0, 0.0, -THICKNESS / 2.0),
                Vec3::new(doorway.width, doorway.height, THICKNESS / 2.0),
            )
            .with_base_color([r, g, b, 1.0])
        });
        let hinge = start + along * from;
        // A turn around Y by an angle moves +X to (cos, 0, -sin).
        let facing = |degrees: f32| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            Vec3::new(cos, 0.0, -sin)
        };
        let pose = |degrees: f32| Transform {
            translation: hinge,
            rotation: Quat::from_euler(EulerRot::YXZ, degrees.to_radians(), 0.0, 0.0),
            scale: Vec3::ONE,
        };
        // +X along the wall: no turn for a wall along X, a quarter turn for one along Z.
        let closed_degrees = if along.x > 0.5 { 0.0 } else { -90.0 };
        // Of the two ways to open, the one into the room.
        let open_degrees = [closed_degrees - OPEN_DEGREES, closed_degrees + OPEN_DEGREES]
            .into_iter()
            .find(|degrees| facing(*degrees).dot(inward) > 0.0)
            .expect("one of the two ways goes into the room");
        let (closed, open) = (pose(closed_degrees), pose(open_degrees));
        // A little thicker than the leaf, so a fast player cannot step over it.
        let blocker = Collider::new(
            hinge - inward * 0.1,
            hinge + along * doorway.width + Vec3::Y * doorway.height + inward * 0.1,
        );
        let mut mover = Mover::new(closed, open, door.seconds);
        if door.open {
            mover.state = MoverState::AtB;
        }
        let mut entity = self.commands.spawn((
            RoomEntity,
            item,
            MeshInstance(handle),
            Interpolated::at(if door.open { open } else { closed }),
            mover,
            Door { blocker },
        ));
        if let Some(name) = name {
            entity.insert(Name(name.clone()));
        }
        if let Some(id) = self.id {
            entity.insert(id);
        }
        entity.id()
    }
}

/// The box around a mesh at a transform.
fn collider_around(mesh: &Mesh, transform: &Transform) -> Collider {
    let (min, max) = mesh.positions().iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(min, max), position| (min.min(*position), max.max(*position)),
    );
    Collider::around(transform.matrix(), min, max)
}

/// Changes the entities of the current room in place, so they show `room`. Only for a
/// room that differs from the current one in values that need no new mesh and no new
/// entity: a light (but for its name and its sound), the pose of a prop with a generated mesh, and
/// the poses and the time of a mover.
/// Returns false, with no change, for each other difference: load the room again.
///
/// For a tool that shows each step of a change (an editor, while a slider moves): no
/// mesh is built, and the picture keeps its history. `text` is the room as a text.
pub fn patch_room(world: &mut World, room: &RoomDef, text: String) -> bool {
    let old = &world.resource::<CurrentRoom>().def;
    if old.objects.len() != room.objects.len() {
        return false;
    }
    // The old room with the values that a patch can change taken from the new one.
    let mut patched = old.clone();
    for (object, new) in patched.objects.iter_mut().zip(&room.objects) {
        // The meshes of a file and of a prefab are placed when they load.
        let file = matches!(object.prop(), Some(Ok(prop)) if matches!(prop.mesh, MeshDef::File(_)));
        if !file && object.prefab().is_none() {
            object.pose = new.pose;
        }
        // A light, but for its sound: a component of its own.
        if let (Some(Ok(light)), Some(Ok(new))) = (object.light(), new.light()) {
            object.set_light(LightDef {
                sound: light.sound,
                ..new
            });
        }
        // The second pose and the time of a mover.
        if let (Some(Ok(mover)), Some(Ok(new))) = (object.mover(), new.mover()) {
            object.set_mover(MoverDef {
                b: new.b,
                seconds: new.seconds,
                ..mover
            });
        }
    }
    if patched != *room {
        return false;
    }

    // Not the lights of a model file: they have the object of the file.
    let mut lights = world.query::<(&RoomItem, &mut Light, &mut Interpolated)>();
    for (item, mut light, mut pose) in lights.iter_mut(world) {
        let Some(Ok(def)) = room.objects[item.0].light() else {
            continue;
        };
        // Only a different value is a write: a write is a change for the renderer.
        light.set_if_neq(def.light());
        if pose.current.translation != Vec3::from(def.position) {
            pose.snap(Transform::from_translation(def.position.into()));
        }
    }
    // A door has a `Mover` too: its object has no `mover`, and it is left as it is.
    let mut movers = world.query::<(&RoomItem, &mut Mover, &mut Interpolated)>();
    for (item, mut mover, mut pose) in movers.iter_mut(world) {
        let Some(Ok(def)) = room.objects[item.0].mover() else {
            continue;
        };
        let new = Mover::new(def.a.transform(), def.b.transform(), def.seconds);
        if mover.set_if_neq(new) {
            pose.snap(new.a);
        }
    }
    world.resource_scope(|world, meshes: Mut<Assets<Mesh>>| {
        // The mesh of a prop. A door and a mover have their own pose.
        let mut props = world.query_filtered::<(
            &RoomItem,
            &mut Transform,
            &MeshInstance,
            Option<&mut Collider>,
            Option<&mut Interpolated>,
        ), Without<Mover>>();
        for (item, mut transform, instance, collider, pose) in props.iter_mut(world) {
            let object = &room.objects[item.0];
            let Some(Ok(prop)) = object.prop() else {
                continue;
            };
            if matches!(prop.mesh, MeshDef::File(_)) {
                continue;
            }
            let new = prop.pose.transform();
            if *transform == new {
                continue;
            }
            *transform = new;
            // The object entity of a script: the blend of the poses writes the
            // `Transform` in each frame.
            if let Some(mut pose) = pose {
                pose.snap(new);
            }
            if let (Some(mut collider), Some(mesh)) = (collider, meshes.get(instance.0)) {
                *collider = collider_around(mesh, &new);
            }
        }
    });
    // No change for the readers of the room: it was not loaded again.
    let mut current = world.resource_mut::<CurrentRoom>();
    let current = current.bypass_change_detection();
    current.def = room.clone();
    current.source = text;
    true
}

/// Runs in `Update`. Polls the pending models. This system does not know if the
/// loader is synchronous (the model is ready in the first frame) or not (it is ready
/// some frames later).
pub(crate) fn spawn_loaded_models(
    mut commands: Commands,
    pending: Query<(Entity, &Transform, &PendingModel)>,
    models: Res<Assets<Model>>,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, transform, pending) in &pending {
        match models.state(pending.model) {
            Some(LoadState::Loading) => continue,
            Some(LoadState::Ready) => {
                let model = models.get(pending.model).expect("the state is Ready");
                for handle in &model.meshes {
                    let mut entity = commands.spawn((
                        RoomEntity,
                        pending.item,
                        *transform,
                        MeshInstance(*handle),
                    ));
                    if pending.collider
                        && let Some(mesh) = meshes.get(*handle)
                    {
                        entity.insert(collider_around(mesh, transform));
                    }
                }
                for light in &model.lights {
                    commands.spawn((
                        RoomEntity,
                        pending.item,
                        Transform::from_translation(
                            transform.matrix().transform_point3(light.position),
                        ),
                        Light {
                            radius: pending.light_radius,
                            color: light.color,
                            intensity: light.intensity,
                            ..Light::default()
                        },
                    ));
                }
            }
            // The loader logged the reason. The prop does not show.
            Some(LoadState::Failed) | None => {}
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_assets::Wall;
    use maki_core::{App, ConfigFile};

    use super::*;
    use bevy_ecs::schedule::IntoScheduleConfigs;

    use crate::SceneModule;
    use crate::object::add_object_component;
    use bevy_ecs::query::Has;

    #[test]
    fn a_live_form_that_gets_its_own_value_back_writes_nothing() {
        use bevy_ecs::change_detection::DetectChanges;
        let mut world = World::new();
        let blocker = Collider::new(Vec3::ZERO, Vec3::ONE);
        let lamp = world.spawn(Light::default()).id();
        let mover = Mover::new(Transform::IDENTITY, Transform::IDENTITY, 1.0);
        let door = world.spawn((mover, Door { blocker })).id();
        world.clear_trackers();
        for live in [LightSource::LIVE, DoorLeaf::LIVE] {
            for entity in [lamp, door] {
                if let Some(tree) = (live.get)(&world, entity) {
                    (live.set)(&mut world, entity, tree).unwrap();
                }
            }
        }
        assert!(!world.entity(lamp).get_ref::<Light>().unwrap().is_changed());
        assert!(!world.entity(door).get_ref::<Mover>().unwrap().is_changed());
    }

    /// A room with one of each thing: objects 0 crate, 1 ball, 2 lamp, 3 door, 4
    /// mover, 5 the spawn point "start".
    fn sample() -> RoomDef {
        let ball = PropDef {
            name: Some("ball".to_owned()),
            mesh: MeshDef::Sphere {
                radius: 0.5,
                segments: 8,
                rings: 4,
            },
            pose: PoseDef {
                position: [1.0, 0.5, 0.0],
                ..PoseDef::default()
            },
            look: LookDef {
                color: Some([1.0, 0.0, 0.0]),
                material: Some(Material::Glass { ior: 1.5 }),
                emissive: None,
                texture: Some(TextureDef {
                    path: "ball.png".into(),
                    scale: 2.0,
                    filter: Filter::Nearest,
                }),
            },
            collider: true,
            light_radius: 0.15,
        };
        let lamp = LightDef {
            name: Some("lamp".to_owned()),
            position: [0.0, 2.0, 0.0],
            radius: 0.1,
            color: [1.0; 3],
            intensity: 5.0,
            on: true,
            direction: None,
            cone: None,
            sound: Some(SoundDef {
                file: "hum.wav".into(),
                volume: 0.5,
                reach: 6.0,
                on: false,
            }),
        };
        let mut door = ObjectDef {
            name: Some("door".to_owned()),
            ..ObjectDef::default()
        };
        let doorway = Doorway {
            wall: Wall::PosX,
            offset: -1.0,
            width: 1.0,
            height: 2.0,
            depth: 1.0,
        };
        door.set_component("doorway", &doorway);
        let leaf = DoorLeaf {
            seconds: 1.0,
            color: [0.3, 0.2, 0.1],
            open: false,
        };
        door.set_component("door", &leaf);
        let mover = MoverDef {
            name: None,
            mesh: MeshDef::Cube(0.5),
            look: LookDef::default(),
            a: PoseDef::default(),
            b: PoseDef {
                position: [0.0, 1.0, 0.0],
                ..PoseDef::default()
            },
            seconds: 2.0,
        };
        RoomDef {
            width: 6.0,
            depth: 5.0,
            height: 3.0,
            next_id: 0,
            objects: vec![
                PropDef {
                    name: Some("crate".to_owned()),
                    pose: PoseDef {
                        position: [-1.0, 0.5, 0.0],
                        ..PoseDef::default()
                    },
                    ..PropDef::default()
                }
                .into_object(),
                ball.into_object(),
                lamp.into_object(),
                door,
                mover.into_object(),
                ObjectDef::spawn_point("start".to_owned(), [0.0, 0.0, 1.0]),
            ],
            floor: None,
            walls: None,
        }
    }

    #[test]
    fn a_room_written_by_code_reads_back_equal() {
        let room = sample();
        let config = ron::ser::PrettyConfig::default();
        let text = ron_options().to_string_pretty(&room, config).unwrap();
        assert_eq!(parse_room(&text).unwrap(), room);
    }

    #[test]
    fn each_item_kind_goes_to_its_tree_and_back() {
        use maki_core::fields::{FieldTypes, read};

        // The table of each kind names the fields as serde does: the tree of a
        // kind with nothing set (`zero`) reads as the kind. What was read gives a
        // tree that reads as itself. (Not `zero` itself: the end of a range is an
        // `f64`, 0.1, and the field has the `f32` next to it.)
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(SceneModule);
        let types = app.world().resource::<FieldTypes>();
        let names: Vec<&str> = types.iter().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            [
                "door", "doorway", "light", "mover", "object", "prefab", "prop", "room", "spawn"
            ]
        );
        for (name, kind) in types.iter() {
            let tree =
                (kind.check)(&kind.kind.zero()).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!((kind.check)(&tree), Ok(tree), "{name}");
        }

        // A thing with values goes to its tree and back with no change, also a
        // number that is not exact in binary (0.15).
        let room = sample();
        fn round_trip<T: Fields + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
            object: &ObjectDef,
            name: &str,
        ) {
            let value: T = object.component(name).unwrap().unwrap();
            assert_eq!(read::<T>(value.tree()).unwrap(), value, "{name}");
        }
        round_trip::<Prop>(&room.objects[1], "prop");
        round_trip::<LightSource>(&room.objects[2], "light");
        round_trip::<DoorLeaf>(&room.objects[3], "door");
        round_trip::<Doorway>(&room.objects[3], "doorway");
        round_trip::<MoverPath>(&room.objects[4], "mover");
        // The room itself: its own fields, and no list of its things.
        let mut shell = room.shell();
        assert_eq!(read::<RoomShell>(shell.tree()).unwrap(), shell);
        shell.width = 9.0;
        let mut changed = room.clone();
        changed.set_shell(shell);
        assert_eq!((changed.width, &changed.objects), (9.0, &room.objects));
    }

    #[test]
    fn a_room_text_of_a_test_reads_as_its_room() {
        let text = room_text(
            4.0,
            5.0,
            3.0,
            &[r#"(name: "box", components: {"prop": (mesh: Cube(1.0))})"#],
        );
        let room = parse_room(&text).unwrap();
        assert_eq!((room.width, room.depth, room.height), (4.0, 5.0, 3.0));
        assert_eq!(room.objects[0].name.as_deref(), Some("box"));
    }

    #[test]
    fn a_short_room_file_gets_the_defaults() {
        let room = parse_room(
            r#"(width: 4, depth: 4, height: 3,
                objects: [(name: "box", components: {"prop": (mesh: Cube(1.0))})])"#,
        )
        .unwrap();
        let prop = room.objects[0].prop().unwrap().unwrap();
        assert_eq!(prop.name.as_deref(), Some("box"));
        assert_eq!(prop.pose, PoseDef::default());
        assert!(prop.collider);
    }

    #[test]
    fn a_bad_room_file_gives_the_file_and_the_line() {
        let path = std::env::temp_dir().join(format!("maki-room-{}.ron", std::process::id()));
        // A wrong field name in line 3.
        std::fs::write(
            &path,
            "(\n  width: 4, depth: 4, height: 3,\n  lamps: [],\n)",
        )
        .unwrap();
        let error = read_room(&path, &RoomValidators::default()).unwrap_err();
        let text = format!("{error:#}");
        assert!(
            text.contains("maki-room-") && text.contains(":3:"),
            "{text}"
        );

        // A door in a doorway that the room does not have.
        std::fs::write(
            &path,
            "(width: 4, depth: 4, height: 3, doors: [(doorway: 0)])",
        )
        .unwrap();
        assert!(read_room(&path, &RoomValidators::default()).is_err());

        // A check of another crate.
        std::fs::write(&path, "(width: 4, depth: 4, height: 3)").unwrap();
        assert!(read_room(&path, &RoomValidators::default()).is_ok());
        let problem = |warning| {
            let text = "2:1: bad event".to_owned();
            let validator: RoomValidator = Box::new(move |_| {
                vec![RoomProblem {
                    warning,
                    text: text.clone(),
                }]
            });
            RoomValidators(vec![validator])
        };
        // A warning: the room loads.
        assert!(read_room(&path, &problem(true)).is_ok());
        let no = problem(false);
        let text = format!("{:#}", read_room(&path, &no).unwrap_err());
        assert!(text.contains("2:1: bad event"), "{text}");

        std::fs::remove_file(&path).unwrap();
        assert!(read_room(&path, &RoomValidators::default()).is_err());
    }

    #[test]
    fn a_door_turns_into_the_room_on_each_wall() {
        for wall in [Wall::NegX, Wall::PosX, Wall::NegZ, Wall::PosZ] {
            let room = sample();
            let door = &room.objects[3];
            let mut doorway = door.doorway().unwrap().unwrap();
            doorway.wall = wall;
            let mut world = bevy_ecs::world::World::new();
            let (mut meshes, mut models) = (Assets::<Mesh>::default(), Assets::<Model>::default());
            let mut cache = RoomModels::default();
            let mut loads = ModelLoads::default();
            let mut textures = Textures::default();
            let mut commands = world.commands();
            let types = ComponentTypes::default();
            let mut spawner = Spawner {
                commands: &mut commands,
                types: &types,
                meshes: &mut meshes,
                models: &mut models,
                textures: &mut textures,
                cache: &mut cache,
                loads: &mut loads,
                directory: Path::new(""),
                old: Vec::new(),
                owned: Vec::new(),
                made: 0,
                id: None,
            };
            let leaf = door.door().unwrap().unwrap();
            spawner.door(
                &door.name,
                &leaf,
                RoomItem(3),
                &doorway,
                room.width,
                room.depth,
            );
            world.flush();
            let mover = *world.query::<&Mover>().single(&world).unwrap();
            let (_, along, _, inward) = wall.frame(room.width, room.depth);
            // Closed: the leaf is in the wall. Open: it points into the room.
            assert!(
                (mover.a.rotation * Vec3::X).distance(along) < 1e-5,
                "{wall:?}"
            );
            assert!((mover.b.rotation * Vec3::X).dot(inward) > 0.9, "{wall:?}");
        }
    }

    #[test]
    fn a_room_change_removes_the_old_room_and_gives_the_spawn_point() {
        let directory = std::env::temp_dir().join(format!("maki-rooms-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let write = |name: &str, room: &RoomDef| {
            let text = ron_options().to_string(room).unwrap();
            std::fs::write(directory.join(name), text).unwrap();
            directory.join(name)
        };
        let room_a = write("a.ron", &sample());
        let small = RoomDef {
            width: 3.0,
            depth: 3.0,
            height: 2.0,
            objects: vec![ObjectDef::spawn_point("door".to_owned(), [1.0, 0.0, 1.0])],
            ..RoomDef::default()
        };
        let room_b = write("b.ron", &small);

        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule)
            .add_message::<RequestRoom>()
            .add_systems(
                maki_core::Update,
                (|mut messages: bevy_ecs::message::MessageReader<RequestRoom>,
                  mut request: ResMut<RoomRequest>| {
                    if let Some(message) = messages.read().last() {
                        request.0 = Some(message.0.clone());
                    }
                })
                .before(load_requested_room),
            );
        let load = |app: &mut App, path: &Path, spawn: Option<&str>| {
            app.write_message(RequestRoom(RoomTarget {
                path: path.to_owned(),
                spawn: spawn.map(ObjectRef::from),
                text: None,
            }));
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let count = |query: &mut bevy_ecs::query::QueryState<Entity, With<RoomEntity>>| {
                query.iter(world).count()
            };
            let mut room_entities = world
                .try_query_filtered::<Entity, With<RoomEntity>>()
                .unwrap();
            // The place that the player module moves the player to.
            (
                count(&mut room_entities),
                world.resource::<Assets<Mesh>>().iter().count(),
                world.resource::<CurrentRoom>().spawn,
            )
        };
        app.run(|mut app| {
            // Room A: the shell, 6 wall boxes, a door, an object, a prop, a mover, a
            // light. Meshes: the shell, the door leaf, the object, the prop, the mover.
            let (entities, meshes, spawn) = load(&mut app, &room_a, Some("start"));
            assert_eq!((entities, meshes), (12, 5));
            assert_eq!(spawn, Some([0.0, 0.0, 1.0]));

            // Room B: the shell and 4 wall boxes. The meshes of room A are gone.
            let (entities, meshes, spawn) = load(&mut app, &room_b, Some("door"));
            assert_eq!((entities, meshes), (5, 1));
            assert_eq!(spawn, Some([1.0, 0.0, 1.0]));

            // A reload with no spawn point: the same room again, no place to go to.
            assert_eq!(load(&mut app, &room_b, None), (5, 1, None));
            // A spawn point that the room does not have: nothing changes.
            assert_eq!(load(&mut app, &room_a, Some("nowhere")), (5, 1, None));
            assert_eq!(app.world().resource::<CurrentRoom>().path, room_b);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_reload_keeps_each_mesh_that_is_made_from_the_same_numbers() {
        use std::collections::HashSet;

        let directory = std::env::temp_dir().join(format!("maki-reload-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("room.ron");
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        // Loads the room and gives the handles of all meshes.
        let mut load = |room: &RoomDef| -> HashSet<Handle<Mesh>> {
            std::fs::write(&path, ron_options().to_string(room).unwrap()).unwrap();
            app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: path.clone(),
                spawn: None,
                text: None,
            });
            app.advance(Duration::from_millis(16));
            let meshes = app.world().resource::<Assets<Mesh>>();
            meshes.iter().map(|(handle, _)| handle).collect()
        };
        // The shell, the door leaf, the object, the ball, the mover, and a second
        // ball that is made from the same numbers as the first one.
        // The ball is object 1; a change of a prop is a new text of its object.
        let change = |room: &mut RoomDef, index: usize, change: &dyn Fn(&mut PropDef)| {
            let mut prop = room.objects[index].prop().unwrap().unwrap();
            change(&mut prop);
            room.objects[index] = prop.into_object();
        };
        let mut room = sample();
        change(&mut room, 1, &|prop| prop.look.texture = None);
        room.objects.push(room.objects[1].clone());
        let first = load(&room);
        assert_eq!(first.len(), 6);
        // The same room again: the same meshes.
        assert_eq!(load(&room), first);
        // Another color for one ball: one new mesh, and its old one is gone.
        let last = room.objects.len() - 1;
        change(&mut room, last, &|prop| {
            prop.look.color = Some([0.0, 1.0, 0.0])
        });
        let colored = load(&room);
        assert_eq!(colored.len(), 6);
        assert_eq!(colored.intersection(&first).count(), 5);
        // A wider room: the shell is new, the things in it are not.
        room.width += 1.0;
        let wide = load(&room);
        assert_eq!(wide.intersection(&colored).count(), 5);
        // The collider of a mesh that was kept is at the place of its prop.
        change(&mut room, 1, &|prop| prop.pose.position = [2.0, 0.5, 1.0]);
        assert_eq!(load(&room), wide);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// Test access to the room request: `App` gives no mutable world.
    #[derive(bevy_ecs::message::Message, Clone)]
    struct RequestRoom(RoomTarget);

    #[test]
    fn a_room_takes_textures_from_its_file_and_the_next_room_removes_them() {
        use maki_assets::Textures;

        let directory = std::env::temp_dir().join(format!("maki-textures-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["floor.png", "bricks.png"] {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]))
                .save(directory.join(name))
                .unwrap();
        }
        let textured = r#"(
            width: 4, depth: 4, height: 3,
            floor: (path: "floor.png"),
            walls: (path: "bricks.png", scale: 2.0, filter: Nearest),
            objects: [(components: {"prop": (mesh: Cube(1.0), look: (texture: (path: "floor.png", scale: 0.5)))})],
        )"#;
        std::fs::write(directory.join("textured.ron"), textured).unwrap();
        std::fs::write(
            directory.join("plain.ron"),
            "(width: 4, depth: 4, height: 3)",
        )
        .unwrap();
        let def = parse_room(textured).unwrap();
        assert_eq!(
            def.walls.as_ref().map(|walls| walls.filter),
            Some(Filter::Nearest)
        );
        assert_eq!(def.floor.as_ref().map(|floor| floor.scale), Some(1.0));
        // What the editor writes is read as the same room.
        assert_eq!(
            parse_room(&ron_options().to_string(&def).unwrap()).unwrap(),
            def
        );

        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        let mut load = |name: &str| {
            app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: directory.join(name),
                spawn: None,
                text: None,
            });
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let meshes: Vec<Mesh> = world
                .resource::<Assets<Mesh>>()
                .iter()
                .map(|(_, mesh)| mesh.clone())
                .collect();
            (meshes, world.resource::<Textures>().iter().count())
        };
        // The floor, the walls, and the cube. Two image files: two textures.
        let (meshes, textures) = load("textured.ron");
        assert_eq!((meshes.len(), textures), (3, 2));
        let named = |name: &str| meshes.iter().find(|mesh| mesh.name() == name).unwrap();
        let (floor, walls, cube) = (named("room floor"), named("room walls"), named("cube"));
        assert_eq!(
            floor.base_color_texture().unwrap().texture,
            cube.base_color_texture().unwrap().texture
        );
        assert!(
            walls.base_color_texture().unwrap().nearest
                && !floor.base_color_texture().unwrap().nearest
        );
        // Meters, times the scale.
        assert!((walls.uvs_per_meter() - 2.0).abs() < 1e-6);
        assert!((cube.uvs_per_meter() - 0.5).abs() < 1e-6);

        // A room with no texture is one mesh, and the textures are gone.
        let (meshes, textures) = load("plain.ron");
        assert_eq!((meshes.len(), textures), (1, 0));
        assert_eq!(load("textured.ron").1, 2);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_moved_with_its_id_file_loads_by_its_id() {
        use maki_assets::Textures;

        let directory = std::env::temp_dir().join(format!("maki-moved-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("new")).unwrap();
        let moved = directory.join("new/floor.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]))
            .save(&moved)
            .unwrap();
        std::fs::write(maki_assets::FileId::file_of(&moved), "00000000000000a1").unwrap();
        let room = |floor: &str| format!("(width: 4, depth: 4, height: 3, floor: (path: {floor}))");
        let by_id = room(r#"(path: "old/floor.png", id: "00000000000000a1")"#);
        std::fs::write(directory.join("by_id.ron"), by_id).unwrap();
        std::fs::write(directory.join("by_path.ron"), room(r#""old/floor.png""#)).unwrap();

        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        let mut textures = |name: &str| {
            app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: directory.join(name),
                spawn: None,
                text: None,
            });
            app.advance(Duration::from_millis(16));
            app.world().resource::<Textures>().iter().count()
        };
        // A path alone does not look for the file; the id finds it.
        assert_eq!(textures("by_path.ron"), 0);
        assert_eq!(textures("by_id.ron"), 1);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A glTF file with one triangle: the corners (0, 0, 0), (1, 0, 0), (0, 1, 0).
    const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
"buffers":[{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA","byteLength":36}],
"bufferViews":[{"buffer":0,"byteLength":36}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]}"#;

    /// The watch of the asset files, with real files and the clock of the wall: a
    /// file that code writes again, as another program does.
    #[test]
    fn a_file_that_is_written_again_shows_with_no_new_room_and_a_broken_file_changes_nothing() {
        use maki_assets::Textures;

        let directory = std::env::temp_dir().join(format!("maki-watch-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("models")).unwrap();
        let paint = |color: [u8; 3]| {
            let [r, g, b] = color;
            image::RgbaImage::from_pixel(2, 2, image::Rgba([r, g, b, 255]))
                .save(directory.join("floor.png"))
                .unwrap();
        };
        paint([200, 0, 0]);
        let model = directory.join("models/triangle.gltf");
        std::fs::write(&model, TRIANGLE).unwrap();
        let room = r#"(width: 4, depth: 4, height: 3, floor: (path: "floor.png"),
            objects: [
                (name: "start", pose: (position: (1.0, 0.0, 1.0)), components: {"spawn": ()}),
                (components: {"prop": (mesh: Cube(1.0))}),
                (components: {"prop": (mesh: File("models/triangle.gltf"))}),
            ])"#;
        std::fs::write(directory.join("room.ron"), room).unwrap();

        // On the thread of the test: a model is there in the frame of its load.
        let config = ConfigFile::from_text(
            "[assets]
background = false
watch = true",
        );
        let mut app = App::new(config).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path: directory.join("room.ron"),
            spawn: Some("start".into()),
            text: None,
        });
        // The color of the floor texture, the mesh of the cube, the model, and the
        // count of the room loads.
        let state = |app: &App| {
            let world = app.world();
            let (meshes, textures) = (
                world.resource::<Assets<Mesh>>(),
                world.resource::<Textures>(),
            );
            let named = |name: &str| meshes.iter().find(|(_, mesh)| mesh.name() == name);
            let floor = named("room floor").and_then(|(_, mesh)| mesh.base_color_texture());
            let color =
                floor.and_then(|floor| Some(textures.get(floor.texture)?.pixels[..3].to_vec()));
            let cube = named("cube").map(|(handle, _)| handle);
            let model = world
                .resource::<RoomModels>()
                .by_path
                .values()
                .next()
                .copied();
            let loads = world.resource::<CurrentRoom>().loads;
            (color, cube, model, loads, textures.iter().count())
        };
        // Frames of 20 ms of real time, until `done` or for this many frames.
        let frames = |app: &mut App, count: u32, done: &dyn Fn(&App) -> bool| {
            for _ in 0..count {
                std::thread::sleep(Duration::from_millis(20));
                app.advance(Duration::from_millis(20));
                if done(app) {
                    return true;
                }
            }
            false
        };
        // The watch takes its first look at each file.
        frames(&mut app, 25, &|_| false);
        let first = state(&app);
        assert_eq!(
            (first.0.as_deref(), first.3, first.4),
            (Some(&[200, 0, 0][..]), 1, 1)
        );

        // The texture in another color: the floor has it in less than 1 s, with one
        // room load. The cube is the mesh of before; the old texture is gone.
        let start = std::time::Instant::now();
        paint([0, 0, 200]);
        let blue = |app: &App| state(app).0.as_deref() == Some(&[0, 0, 200][..]);
        assert!(frames(&mut app, 100, &blue), "the floor did not change");
        let time = start.elapsed();
        assert!(time < Duration::from_secs(1), "{time:?}");
        let second = state(&app);
        assert_eq!(
            (second.1, second.2, second.3, second.4),
            (first.1, first.2, 2, 1)
        );

        // A file that does not decode (half of a file): nothing changes.
        std::fs::write(directory.join("floor.png"), "not a picture").unwrap();
        assert!(!frames(&mut app, 50, &|app| state(app) != second));

        // The model file, written again: a new model, with one more room load. The
        // texture file is whole again: the floor is loaded with it.
        paint([0, 200, 0]);
        std::fs::write(
            &model,
            TRIANGLE.replace("AAAAAAAAAAAAAAAAAACAPw", "AAAAAAAAAAAAAAAAAAAAQA"),
        )
        .unwrap();
        let new_model = |app: &App| {
            let now = state(app);
            now.2 != second.2 && now.0.as_deref() == Some(&[0, 200, 0][..])
        };
        assert!(
            frames(&mut app, 100, &new_model),
            "the model did not change"
        );
        let third = state(&app);
        assert_eq!((third.1, third.4), (first.1, 1));
        // The player is where the spawn point put it: a load again moves no player.
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn two_rooms_that_name_one_model_file_load_it_one_time() {
        let directory = std::env::temp_dir().join(format!("maki-models-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("models")).unwrap();
        std::fs::write(directory.join("models/triangle.gltf"), TRIANGLE).unwrap();
        let rooms = [
            (
                "a.ron",
                r#"objects: [
                    (components: {"prop": (mesh: Cube(1.0))}),
                    (components: {"prop": (mesh: File("models/triangle.gltf"))}),
                ]"#,
            ),
            (
                "b.ron",
                r#"objects: [(
                    pose: (position: (1.0, 0.0, 0.0)),
                    components: {"prop": (mesh: File("models/triangle.gltf"))},
                )]"#,
            ),
            ("c.ron", ""),
        ];
        for (name, props) in rooms {
            let text = format!("(width: 4, depth: 4, height: 3, {props})");
            std::fs::write(directory.join(name), text).unwrap();
        }
        // On the thread of the test: the model is there in the frame of the load.
        let config = ConfigFile::from_text("[assets]\nbackground = false");
        let mut app = App::new(config).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        // Loads a room. Gives the meshes of the model, and the place of each prop
        // that shows one of them.
        let mut load = |name: &str| {
            app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: directory.join(name),
                spawn: None,
                text: None,
            });
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let meshes = world.resource::<Assets<Mesh>>();
            let of_model: Vec<Handle<Mesh>> = meshes
                .iter()
                .filter(|(_, mesh)| mesh.name() == "mesh/0")
                .map(|(handle, _)| handle)
                .collect();
            let mut shown = world
                .try_query::<(&MeshInstance, &Transform, &RoomItem)>()
                .unwrap();
            let places: Vec<(Vec3, RoomItem)> = shown
                .iter(world)
                .filter(|(mesh, ..)| of_model.contains(&mesh.0))
                .map(|(_, transform, item)| (transform.translation, *item))
                .collect();
            (of_model, places)
        };
        let (first, places) = load("a.ron");
        assert_eq!(first.len(), 1);
        assert_eq!(places, [(Vec3::ZERO, RoomItem(1))]);
        // The next room names the file: the same mesh, at the place of its prop.
        let (second, places) = load("b.ron");
        assert_eq!(second, first);
        assert_eq!(places, [(Vec3::X, RoomItem(0))]);
        // A room with no model: the mesh is gone. Then the file is read again.
        assert!(load("c.ron").0.is_empty());
        let (again, _) = load("a.ron");
        assert!(again.len() == 1 && again != first);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_prefab_file_shows_in_each_room_that_names_it_and_a_change_of_it_in_all() {
        let directory = std::env::temp_dir().join(format!("maki-prefabs-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("prefabs")).unwrap();
        // A table, a chair 1 m in front of it, and a lamp 2 m above the origin.
        let table = r#"(objects: [
            (name: "table", pose: (position: (0.0, 0.5, 0.0)), components: {"prop": (mesh: Cube(1.0))}),
            (name: "chair", pose: (position: (0.0, 0.2, 1.0)), components: {"prop": (mesh: Cube(0.4))}),
            (pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 3.0)}),
        ])"#;
        let file = directory.join("prefabs/table.prefab");
        std::fs::write(&file, table).unwrap();
        // Room A has it at a place. Room B has it two times, one with a quarter turn,
        // and a cube as object 2.
        let rooms = [
            (
                "a.ron",
                r#"objects: [(
                    pose: (position: (1.0, 0.0, -1.0)),
                    components: {"prefab": (file: "prefabs/table.prefab")},
                )]"#,
            ),
            (
                "b.ron",
                r#"objects: [
                    (components: {"prefab": (file: "prefabs/table.prefab")}),
                    (
                        pose: (position: (2.0, 0.0, 0.0), rotation: (0.0, 90.0, 0.0)),
                        components: {"prefab": (file: "prefabs/table.prefab")},
                    ),
                    (components: {"prop": (mesh: Cube(1.0))}),
                ]"#,
            ),
        ];
        for (name, items) in rooms {
            let text = format!("(width: 8, depth: 8, height: 3, {items})");
            std::fs::write(directory.join(name), text).unwrap();
        }
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule);
        // Loads a room. Gives the place of each mesh and each light of a prefab,
        // with the number of the prefab, in the order of the numbers and of X and Z.
        let mut load = |name: &str| {
            app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: directory.join(name),
                spawn: None,
                text: None,
            });
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let mut meshes = world
                .try_query::<(&RoomItem, &Transform, &MeshInstance)>()
                .unwrap();
            let mut lights = world
                .try_query::<(&RoomItem, &Interpolated, &Light)>()
                .unwrap();
            let meshes = meshes
                .iter(world)
                .map(|(item, at, _)| (*item, at.translation));
            let lights = lights
                .iter(world)
                .map(|(item, at, _)| (*item, at.current.translation));
            let mut places: Vec<(usize, [i32; 3])> = meshes
                .chain(lights)
                .filter_map(|(item, at)| match item {
                    // Objects 0 and 1 are the prefabs.
                    RoomItem(index) if index < 2 => {
                        Some((index, (at * 10.0).round().as_ivec3().into()))
                    }
                    _ => None,
                })
                .collect();
            places.sort();
            places
        };
        // In decimeters.
        assert_eq!(
            load("a.ron"),
            [(0, [10, 2, 0]), (0, [10, 5, -10]), (0, [10, 20, -10])]
        );
        let in_b = load("b.ron");
        assert_eq!(
            in_b[..3],
            [(0, [0, 2, 10]), (0, [0, 5, 0]), (0, [0, 20, 0])]
        );
        // A quarter turn around Y moves +Z to +X.
        assert_eq!(
            in_b[3..],
            [(1, [20, 5, 0]), (1, [20, 20, 0]), (1, [30, 2, 0])]
        );

        // The chair goes to the other side of the table: in each room.
        std::fs::write(&file, table.replace("(0.0, 0.2, 1.0)", "(0.0, 0.2, -1.0)")).unwrap();
        assert_eq!(load("b.ron")[0], (0, [0, 2, -10]));
        assert_eq!(load("a.ron")[0], (0, [10, 2, -20]));

        // A prefab file that does not read: the game does not go into the room.
        std::fs::write(&file, "(props: [(mesh: Ball)])").unwrap();
        assert_eq!(load("b.ron")[0], (0, [10, 2, -20]));
        assert!(
            app.world()
                .resource::<CurrentRoom>()
                .path
                .ends_with("a.ron")
        );
        let error = read_room(&directory.join("b.ron"), &RoomValidators::default()).unwrap_err();
        assert!(
            format!("{error:#}").contains("table.prefab:1:"),
            "{error:#}"
        );

        // A pose is the same after `pose_in` and `pose_out`, and as a `PoseDef`.
        let pose = |position, rotation, scale| PoseDef {
            position,
            rotation,
            scale,
        };
        let outer = pose([1.0, 0.0, -2.0], [0.0, 130.0, 0.0], [2.0, 1.0, 2.0]).transform();
        let inner = pose([0.5, 0.2, 1.0], [10.0, -40.0, 5.0], [1.0, 3.0, 1.0]);
        let back = PoseDef::of(&pose_out(&outer, &pose_in(&outer, &inner.transform())));
        let near = |a: [f32; 3], b: [f32; 3]| Vec3::from(a).distance(b.into()) < 1e-3;
        assert!(
            near(back.position, inner.position)
                && near(back.rotation, inner.rotation)
                && near(back.scale, inner.scale),
            "{back:?}"
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A component of a project, for the tests of the object entity.
    #[derive(Component, Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
    #[serde(default)]
    struct Spin {
        speed: f32,
    }

    #[test]
    fn the_components_of_bevy_ecs_of_an_object_go_to_one_entity_with_its_pose() {
        let room = r#"(width: 6.0, depth: 6.0, height: 3.0, objects: [
            (name: "top", pose: (position: (1.0, 0.5, 0.0)), components: {"prop": (mesh: Cube(0.5)), "spin": (speed: 2.0)}),
            (name: "empty", pose: (position: (-1.0, 0.0, 2.0)), components: {"spin": (speed: 3.0)}),
            (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 1.0), "spin": ()}),
            (name: "lost", components: {"wobble": (speed: 1.0)}),
        ])"#;
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(SceneModule);
        add_object_component::<Spin>(&mut app, "spin");
        app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path: PathBuf::from("spin.ron"),
            spawn: None,
            text: Some(room.to_owned()),
        });
        app.run(|mut app| {
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let mut spins = world
                .try_query::<(
                    &RoomItem,
                    &Spin,
                    &Name,
                    &Interpolated,
                    Has<MeshInstance>,
                    Has<Light>,
                )>()
                .unwrap();
            let mut found: Vec<_> = spins
                .iter(world)
                .map(|(item, spin, name, pose, mesh, light)| {
                    (
                        item.0,
                        spin.speed,
                        name.0.clone(),
                        pose.current.translation,
                        mesh,
                        light,
                    )
                })
                .collect();
            found.sort_by_key(|found| found.0);
            assert_eq!(
                found,
                [
                    // The mesh of the prop is on the object entity: a script moves it.
                    (
                        0,
                        2.0,
                        "top".to_owned(),
                        Vec3::new(1.0, 0.5, 0.0),
                        true,
                        false
                    ),
                    (
                        1,
                        3.0,
                        "empty".to_owned(),
                        Vec3::new(-1.0, 0.0, 2.0),
                        false,
                        false
                    ),
                    (
                        2,
                        0.0,
                        "lamp".to_owned(),
                        Vec3::new(0.0, 2.0, 0.0),
                        false,
                        true
                    ),
                ]
            );
            // A component that no module registered stays text: no entity.
            let mut items = world.try_query::<&RoomItem>().unwrap();
            assert!(items.iter(world).all(|item| item.0 != 3));
        });
    }

    #[test]
    fn an_object_with_an_id_has_it_on_its_entity_and_one_with_none_loads_as_before() {
        let room = r#"(width: 6.0, depth: 6.0, height: 3.0, next_id: 8, objects: [
            (id: 7, name: "box", components: {"prop": (mesh: Cube(0.5))}),
            (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 1.0)}),
        ])"#;
        let def = parse_room(room).unwrap();
        assert_eq!(def.next_id, 8);
        assert_eq!(def.objects[0].id, Some(ObjectId(7)));
        // The id is the first field, and a number.
        let text = ron_options().to_string(&def).unwrap();
        assert!(text.contains("next_id:8"), "{text}");
        assert!(text.contains("(id:7,name:\"box\""), "{text}");
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(SceneModule);
        app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path: PathBuf::from("ids.ron"),
            spawn: None,
            text: Some(room.to_owned()),
        });
        app.run(|mut app| {
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let mut named = world.try_query::<(&Name, Option<&ObjectId>)>().unwrap();
            let mut found: Vec<_> = named
                .iter(world)
                .map(|(name, id)| (name.0.clone(), id.copied()))
                .collect();
            found.sort();
            assert_eq!(
                found,
                [
                    ("box".to_owned(), Some(ObjectId(7))),
                    ("lamp".to_owned(), None)
                ]
            );
        });
    }
}
