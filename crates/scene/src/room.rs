//! Rooms as data: the room file format, and load and unload of a room.
//!
//! A room file is RON text (`game/assets/rooms/*.ron`): the size of the room, its
//! doorways, props, lights, doors, movers, and spawn points. [`RoomDef`] is its
//! content. The file also holds the events of the room, which this crate does not
//! read: the events crate reads them from [`CurrentRoom::source`].
//!
//! To change the room, write a [`RoomRequest`]. `load_requested_room` then removes
//! all entities with the [`RoomEntity`] marker and spawns those of the new room. The
//! player, the camera, and the game state are not room entities: they stay.
//!
//! Assets: a mesh that the room makes from numbers (its walls, a cube) belongs to the
//! room and is removed with it. A model file that the new room does not name is
//! removed too, with its meshes: a room change leaves nothing of the old room. A
//! model that the new room names again (a reload) stays loaded.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::{Mut, World};
use glam::{EulerRot, Quat, Vec3};
use maki_assets::{
    Assets, Doorway, Handle, LoadState, Material, Mesh, Model, TextureRef, Textures, load_gltf,
};
use maki_core::{Interpolated, Transform};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::{Collider, Door, Light, MeshInstance, Mover, MoverState, Name, Player, room_colliders};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doorways: Vec<Doorway>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doors: Vec<DoorDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PropDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub movers: Vec<MoverDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<LightDef>,
    /// Places where the player can enter the room, by name: X, Y, Z.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spawns: BTreeMap<String, [f32; 3]>,
    /// The events of the room. Not read here: see the module comment.
    #[serde(default, skip_serializing)]
    pub events: IgnoredAny,
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PoseDef {
    #[serde(skip_serializing_if = "is_default")]
    pub position: [f32; 3],
    /// Degrees around X, Y, and Z. The turn around Z is done first, then X, then Y.
    #[serde(skip_serializing_if = "is_default")]
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
}

/// The shape of a prop or a mover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MeshDef {
    /// A cube of this size with its center at the origin.
    Cube(f32),
    /// A box between two corners.
    Cuboid([f32; 3], [f32; 3]),
    Sphere {
        radius: f32,
        segments: u32,
        rings: u32,
    },
    /// A glTF file (`.gltf` or `.glb`). A path that is not absolute starts at the
    /// directory of the room file. The point lights of the file become lights.
    File(PathBuf),
}

impl MeshDef {
    /// The mesh of a generated shape. `None` for a file.
    fn generate(&self) -> Option<Mesh> {
        Some(match self {
            MeshDef::Cube(size) => Mesh::cube(*size),
            MeshDef::Cuboid(min, max) => Mesh::cuboid((*min).into(), (*max).into()),
            MeshDef::Sphere {
                radius,
                segments,
                rings,
            } => Mesh::sphere(*radius, *segments, *rings),
            MeshDef::File(_) => return None,
        })
    }
}

/// A texture for a generated mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureDef {
    /// An image file (PNG or JPEG). A path that is not absolute starts at the
    /// directory of the room file.
    pub path: PathBuf,
    /// The texture coordinates are multiplied by this: 2.0 shows the image two times
    /// where it showed one time. The coordinates of the room box, a cube, and a
    /// cuboid are meters; those of a sphere go one time around it.
    #[serde(default = "one", skip_serializing_if = "is_one")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
                texture: textures.load(&directory.join(&self.path)),
                nearest: self.filter == Filter::Nearest,
            })
    }
}

/// How a generated mesh looks. A file has its own colors and materials.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LookDef {
    /// The color texture. The color multiplies it: give a texture the color white.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<TextureDef>,
    /// Linear RGB. `None`: the color of the generator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// `None`: diffuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// A light of its own, linear RGB: the mesh shows with this color and lights
    /// the room by bounce rays. `None`: no light.
    #[serde(skip_serializing_if = "Option::is_none")]
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

/// A door in a doorway of the room. It turns on the side of the opening that is
/// nearer to the start of the wall, into the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Index in `doorways`, from 0.
    pub doorway: usize,
    /// Seconds from closed to open.
    #[serde(
        default = "default_door_seconds",
        skip_serializing_if = "is_default_door_seconds"
    )]
    pub seconds: f32,
    /// Linear RGB.
    #[serde(
        default = "default_door_color",
        skip_serializing_if = "is_default_door_color"
    )]
    pub color: [f32; 3],
    /// True: the door is open when the room loads.
    #[serde(default, skip_serializing_if = "is_default")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoverDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub mesh: MeshDef,
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: LookDef,
    /// The pose at the start.
    pub a: PoseDef,
    pub b: PoseDef,
    /// Seconds from `a` to `b`.
    pub seconds: f32,
}

/// The RON dialect of the room files. The events crate reads the same files with it.
/// An optional field takes its value without `Some(...)`.
pub fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
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
    Ok((check_room(path, &text, validators)?, text))
}

/// Reads and checks the text of a room file. `path` is for the error text.
pub fn check_room(path: &Path, text: &str, validators: &RoomValidators) -> Result<RoomDef> {
    let room = parse_room(text).map_err(|error| anyhow!("{}:{error}", path.display()))?;
    for door in &room.doors {
        if door.doorway >= room.doorways.len() {
            return Err(anyhow!(
                "{}: a door names doorway {}, and the room has {}",
                path.display(),
                door.doorway,
                room.doorways.len()
            ));
        }
    }
    for validate in &validators.0 {
        validate(text).map_err(|error| anyhow!("{}:{error}", path.display()))?;
    }
    Ok(room)
}

/// Checks of other crates for a room file: each gets the text of the file and returns
/// the reason if the file is bad. A room that fails a check is not loaded. The events
/// crate checks the events with this, so a transfer into a room with a bad event does
/// not happen.
#[derive(Resource, Default)]
pub struct RoomValidators(pub Vec<RoomValidator>);

/// Gets the text of a room file. `Err`: the reason why the file is bad, with the
/// line and the column first (`3:7: ...`) if it has them.
// A closure, not a plain function: the check of the events holds the command registry.
pub type RoomValidator = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Marks an entity that belongs to the current room. Unload removes it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct RoomEntity;

/// The item of the room file that an entity was made from: the index in its list of
/// [`RoomDef`]. A tool finds the entity of an item, and the item of an entity, with
/// it. The entities of a model file all have the item of their prop.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoomItem {
    Prop(usize),
    Light(usize),
    Door(usize),
    Mover(usize),
}

/// Ask for a room: write `Some`. `load_requested_room` takes it in the next `Update`.
#[derive(Resource, Debug, Default)]
pub struct RoomRequest(pub Option<RoomTarget>);

#[derive(Debug, Clone, PartialEq)]
pub struct RoomTarget {
    pub path: PathBuf,
    /// The spawn point of the room where the player goes. `None`: the player stays
    /// where it is (a reload of the same room).
    pub spawn: Option<String>,
    /// `Some`: the text of the room, in place of the text of the file (a tool shows a
    /// room that is not saved). `path` then is the name of the room, and its
    /// directory is where the files that the room names are.
    pub text: Option<String>,
}

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
    /// The generated meshes of the room. Removed with the room.
    meshes: Vec<Handle<Mesh>>,
}

/// The models of the current room, by path. `load_requested_room` removes those that
/// the next room does not name.
#[derive(Resource, Default)]
pub struct RoomModels(HashMap<PathBuf, Handle<Model>>);

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
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
    mut cache: ResMut<RoomModels>,
    room_entities: Query<Entity, With<RoomEntity>>,
    mut players: Query<&mut Interpolated, With<Player>>,
) {
    // No `take` before this test: a write marks the request as changed in each frame.
    if request.0.is_none() {
        return;
    }
    let mut target = request.0.take().expect("tested above");
    let start = std::time::Instant::now();
    let read = match target.text.take() {
        Some(text) => check_room(&target.path, &text, &validators).map(|def| (def, text)),
        None => read_room(&target.path, &validators),
    };
    let loaded = read.and_then(|(def, source)| {
        let spawn =
            match &target.spawn {
                Some(name) => Some(*def.spawns.get(name).with_context(|| {
                    format!("{}: no spawn point `{name}`", target.path.display())
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
    for mesh in current.meshes.drain(..) {
        meshes.remove(mesh);
    }
    let directory = target.path.parent().unwrap_or(Path::new(""));
    // The models that only the old room used. Without this, each room that was shown
    // keeps its meshes on the GPU, and the mesh table of the renderer gets full.
    let named: Vec<PathBuf> = def
        .props
        .iter()
        .filter_map(|prop| match &prop.mesh {
            MeshDef::File(path) => Some(directory.join(path)),
            _ => None,
        })
        .collect();
    cache.0.retain(|path, model| {
        let keep = named.contains(path);
        if !keep {
            // `None` for a model that failed to load: it has no meshes.
            for mesh in models
                .remove(*model)
                .map_or(Vec::new(), |model| model.meshes)
            {
                meshes.remove(mesh);
            }
        }
        keep
    });
    let mut spawner = Spawner {
        commands: &mut commands,
        meshes: &mut meshes,
        models: &mut models,
        textures: &mut textures,
        cache: &mut cache,
        directory,
        owned: Vec::new(),
    };
    spawner.room(&def);
    let owned = spawner.owned;
    // The textures of the old room that no mesh uses now. The models of the new
    // room are loaded: their meshes are in the store.
    let removed = textures.remove_unused(&meshes);
    if removed > 0 {
        tracing::info!(removed, "textures of the old room removed");
    }
    tracing::info!(
        path = %target.path.display(),
        props = def.props.len(),
        lights = def.lights.len(),
        "room loaded"
    );
    *current = CurrentRoom {
        path: target.path,
        def,
        source,
        loads: current.loads + 1,
        load_time: start.elapsed(),
        meshes: owned,
    };
    if let Some(position) = spawn {
        for mut pose in &mut players {
            let mut at = pose.current;
            at.translation = position.into();
            // No blend from the old place.
            pose.snap(at);
        }
    }
}

/// Spawns the entities of a room.
struct Spawner<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    models: &'a mut Assets<Model>,
    textures: &'a mut Textures,
    cache: &'a mut RoomModels,
    /// Of the room file.
    directory: &'a Path,
    /// The meshes that this room made.
    owned: Vec<Handle<Mesh>>,
}

impl Spawner<'_, '_, '_> {
    fn add(&mut self, mesh: Mesh) -> Handle<Mesh> {
        let handle = self.meshes.add(mesh);
        self.owned.push(handle);
        handle
    }

    fn room(&mut self, def: &RoomDef) {
        let (width, depth, height) = (def.width, def.depth, def.height);
        let shell = if def.floor.is_none() && def.walls.is_none() {
            vec![Mesh::room_with_doorways(
                width,
                depth,
                height,
                &def.doorways,
            )]
        } else {
            // A mesh has one material: the floor and the walls are two meshes.
            let (floor, walls) = Mesh::room_parts(width, depth, height, &def.doorways);
            let parts = [(floor, &def.floor), (walls, &def.walls)];
            let textured = parts.map(|(mesh, texture)| match texture {
                Some(texture) => texture.apply(mesh, self.directory, self.textures),
                None => mesh,
            });
            textured.into()
        };
        for mesh in shell {
            let mesh = self.add(mesh);
            self.commands
                .spawn((RoomEntity, Transform::IDENTITY, MeshInstance(mesh)));
        }
        for collider in room_colliders(width, depth, height, &def.doorways) {
            self.commands.spawn((RoomEntity, collider));
        }
        for (index, door) in def.doors.iter().enumerate() {
            let item = RoomItem::Door(index);
            self.door(door, item, &def.doorways[door.doorway], width, depth);
        }
        for (index, prop) in def.props.iter().enumerate() {
            self.prop(prop, RoomItem::Prop(index));
        }
        for (index, mover) in def.movers.iter().enumerate() {
            self.mover(mover, RoomItem::Mover(index));
        }
        for (index, light) in def.lights.iter().enumerate() {
            let mut entity = self.commands.spawn((
                RoomEntity,
                RoomItem::Light(index),
                // A light can move in fixed steps (a script, a debug control).
                Interpolated::at(Transform::from_translation(light.position.into())),
                light.light(),
            ));
            if let Some(name) = &light.name {
                entity.insert(Name(name.clone()));
            }
        }
    }

    fn prop(&mut self, prop: &PropDef, item: RoomItem) {
        let transform = prop.pose.transform();
        let mut entity = match &prop.mesh {
            MeshDef::File(path) => {
                let path = self.directory.join(path);
                let (models, meshes) = (&mut *self.models, &mut *self.meshes);
                let textures = &mut *self.textures;
                let model = *self
                    .cache
                    .0
                    .entry(path.clone())
                    .or_insert_with(|| load_gltf(&path, models, meshes, textures));
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
                let mesh = generated.generate().expect("not a file");
                let mesh = prop.look.apply(mesh, self.directory, self.textures);
                let collider = prop.collider.then(|| collider_around(&mesh, &transform));
                let handle = self.add(mesh);
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
    }

    fn mover(&mut self, mover: &MoverDef, item: RoomItem) {
        let Some(mesh) = mover.mesh.generate() else {
            tracing::error!(name = ?mover.name, "a mover cannot have a file as its mesh: left out");
            return;
        };
        let mesh = mover.look.apply(mesh, self.directory, self.textures);
        let handle = self.add(mesh);
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
    }

    /// The door leaf: a thin box that fills the doorway when it is closed.
    fn door(&mut self, door: &DoorDef, item: RoomItem, doorway: &Doorway, width: f32, depth: f32) {
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
        let leaf = Mesh::cuboid(
            Vec3::new(0.0, 0.0, -THICKNESS / 2.0),
            Vec3::new(doorway.width, doorway.height, THICKNESS / 2.0),
        )
        .with_base_color([r, g, b, 1.0]);
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
        let handle = self.add(leaf);
        let mut entity = self.commands.spawn((
            RoomEntity,
            item,
            MeshInstance(handle),
            Interpolated::at(if door.open { open } else { closed }),
            mover,
            Door { blocker },
        ));
        if let Some(name) = &door.name {
            entity.insert(Name(name.clone()));
        }
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
/// entity: a light (but for its name), the pose of a prop with a generated mesh, and
/// the poses and the time of a mover.
/// Returns false, with no change, for each other difference: load the room again.
///
/// For a tool that shows each step of a change (an editor, while a slider moves): no
/// mesh is built, and the picture keeps its history. `text` is the room as a text.
pub fn patch_room(world: &mut World, room: &RoomDef, text: String) -> bool {
    let old = &world.resource::<CurrentRoom>().def;
    let same_count = old.lights.len() == room.lights.len()
        && old.props.len() == room.props.len()
        && old.movers.len() == room.movers.len();
    if !same_count {
        return false;
    }
    // The old room with the values that a patch can change taken from the new one.
    let mut patched = old.clone();
    for (light, new) in patched.lights.iter_mut().zip(&room.lights) {
        let name = light.name.take();
        *light = LightDef {
            name,
            ..new.clone()
        };
    }
    for (prop, new) in patched.props.iter_mut().zip(&room.props) {
        if !matches!(prop.mesh, MeshDef::File(_)) {
            prop.pose = new.pose;
        }
    }
    for (mover, new) in patched.movers.iter_mut().zip(&room.movers) {
        (mover.a, mover.b, mover.seconds) = (new.a, new.b, new.seconds);
    }
    if patched != *room {
        return false;
    }

    let mut lights = world.query::<(&RoomItem, &mut Light, &mut Interpolated)>();
    for (item, mut light, mut pose) in lights.iter_mut(world) {
        let RoomItem::Light(index) = *item else {
            continue;
        };
        let def = &room.lights[index];
        // Only a different value is a write: a write is a change for the renderer.
        light.set_if_neq(def.light());
        if pose.current.translation != Vec3::from(def.position) {
            pose.snap(Transform::from_translation(def.position.into()));
        }
    }
    let mut movers = world.query::<(&RoomItem, &mut Mover, &mut Interpolated)>();
    for (item, mut mover, mut pose) in movers.iter_mut(world) {
        let RoomItem::Mover(index) = *item else {
            continue;
        };
        let def = &room.movers[index];
        let new = Mover::new(def.a.transform(), def.b.transform(), def.seconds);
        if mover.set_if_neq(new) {
            pose.snap(new.a);
        }
    }
    world.resource_scope(|world, meshes: Mut<Assets<Mesh>>| {
        let mut props = world.query::<(
            &RoomItem,
            &mut Transform,
            &MeshInstance,
            Option<&mut Collider>,
        )>();
        for (item, mut transform, instance, collider) in props.iter_mut(world) {
            let RoomItem::Prop(index) = *item else {
                continue;
            };
            let new = room.props[index].pose.transform();
            if *transform == new {
                continue;
            }
            *transform = new;
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

    /// A room with one of each thing.
    fn sample() -> RoomDef {
        RoomDef {
            width: 6.0,
            depth: 5.0,
            height: 3.0,
            doorways: vec![Doorway {
                wall: Wall::PosX,
                offset: -1.0,
                width: 1.0,
                height: 2.0,
                depth: 1.0,
            }],
            props: vec![PropDef {
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
                        path: PathBuf::from("ball.png"),
                        scale: 2.0,
                        filter: Filter::Nearest,
                    }),
                },
                collider: true,
                light_radius: 0.15,
            }],
            lights: vec![LightDef {
                name: Some("lamp".to_owned()),
                position: [0.0, 2.0, 0.0],
                radius: 0.1,
                color: [1.0; 3],
                intensity: 5.0,
                on: true,
                direction: None,
                cone: None,
            }],
            doors: vec![DoorDef {
                name: Some("door".to_owned()),
                doorway: 0,
                seconds: 1.0,
                color: [0.3, 0.2, 0.1],
                open: false,
            }],
            movers: vec![MoverDef {
                name: None,
                mesh: MeshDef::Cube(0.5),
                look: LookDef::default(),
                a: PoseDef::default(),
                b: PoseDef {
                    position: [0.0, 1.0, 0.0],
                    ..PoseDef::default()
                },
                seconds: 2.0,
            }],
            floor: None,
            walls: None,
            spawns: BTreeMap::from([("start".to_owned(), [0.0, 0.0, 1.0])]),
            events: IgnoredAny,
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
    fn a_short_room_file_gets_the_defaults_and_its_events_are_not_read() {
        let room = parse_room(
            r#"(width: 4, depth: 4, height: 3,
                props: [(mesh: Cube(1.0), name: "box")],
                events: [(what: "ever", this: (is, 1))])"#,
        )
        .unwrap();
        assert_eq!(room.props[0].name.as_deref(), Some("box"));
        assert_eq!(room.props[0].pose, PoseDef::default());
        assert!(room.props[0].collider);
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
        let no = RoomValidators(vec![Box::new(|_| Err("2:1: bad event".to_owned()))]);
        let text = format!("{:#}", read_room(&path, &no).unwrap_err());
        assert!(text.contains("2:1: bad event"), "{text}");

        std::fs::remove_file(&path).unwrap();
        assert!(read_room(&path, &RoomValidators::default()).is_err());
    }

    #[test]
    fn a_door_turns_into_the_room_on_each_wall() {
        for wall in [Wall::NegX, Wall::PosX, Wall::NegZ, Wall::PosZ] {
            let mut room = sample();
            room.doorways[0].wall = wall;
            let mut world = bevy_ecs::world::World::new();
            let (mut meshes, mut models) = (Assets::<Mesh>::default(), Assets::<Model>::default());
            let mut cache = RoomModels::default();
            let mut textures = Textures::default();
            let mut commands = world.commands();
            let mut spawner = Spawner {
                commands: &mut commands,
                meshes: &mut meshes,
                models: &mut models,
                textures: &mut textures,
                cache: &mut cache,
                directory: Path::new(""),
                owned: Vec::new(),
            };
            let item = RoomItem::Door(0);
            spawner.door(
                &room.doors[0],
                item,
                &room.doorways[0],
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
    fn a_room_change_removes_the_old_room_and_keeps_the_player() {
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
            spawns: BTreeMap::from([("door".to_owned(), [1.0, 0.0, 1.0])]),
            ..RoomDef::default()
        };
        let room_b = write("b.ron", &small);

        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule)
            .add_module(SceneModule)
            .add_message::<RequestRoom>()
            .add_systems(maki_core::Startup, |mut commands: Commands| {
                commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
            })
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
                spawn: spawn.map(str::to_owned),
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
            let mut players = world
                .try_query_filtered::<&Interpolated, With<Player>>()
                .unwrap();
            (
                count(&mut room_entities),
                world.resource::<Assets<Mesh>>().iter().count(),
                players.single(world).unwrap().current.translation,
            )
        };
        app.run(|mut app| {
            // Room A: the shell, 6 wall boxes, a door, a prop, a mover, a light.
            // Meshes: the shell, the door leaf, the prop, the mover.
            let (entities, meshes, player) = load(&mut app, &room_a, Some("start"));
            assert_eq!((entities, meshes), (11, 4));
            assert_eq!(player, Vec3::new(0.0, 0.0, 1.0));

            // Room B: the shell and 4 wall boxes. The meshes of room A are gone.
            let (entities, meshes, player) = load(&mut app, &room_b, Some("door"));
            assert_eq!((entities, meshes), (5, 1));
            assert_eq!(player, Vec3::new(1.0, 0.0, 1.0));

            // A reload with no spawn point: the same room again, the player stays.
            assert_eq!(load(&mut app, &room_b, None), (5, 1, player));
            // A spawn point that the room does not have: nothing changes.
            assert_eq!(load(&mut app, &room_a, Some("nowhere")), (5, 1, player));
            assert_eq!(app.world().resource::<CurrentRoom>().path, room_b);
        });
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
            props: [(mesh: Cube(1.0), look: (texture: (path: "floor.png", scale: 0.5)))],
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
}
