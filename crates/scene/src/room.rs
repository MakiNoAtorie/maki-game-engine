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
//! room and is removed with it. A model file is loaded one time: the next room that
//! names the same path gets the same handle.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::{EulerRot, Quat, Vec3};
use maki_assets::{Assets, Doorway, Handle, LoadState, Material, Mesh, Model, load_gltf};
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
    #[serde(default)]
    pub doorways: Vec<Doorway>,
    #[serde(default)]
    pub props: Vec<PropDef>,
    #[serde(default)]
    pub lights: Vec<LightDef>,
    #[serde(default)]
    pub doors: Vec<DoorDef>,
    #[serde(default)]
    pub movers: Vec<MoverDef>,
    /// Places where the player can enter the room, by name: X, Y, Z.
    #[serde(default)]
    pub spawns: BTreeMap<String, [f32; 3]>,
    /// The events of the room. Not read here: see the module comment.
    #[serde(default, skip_serializing)]
    pub events: IgnoredAny,
}

/// Where a thing is. Each field has a default, so a file gives only what differs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PoseDef {
    pub position: [f32; 3],
    /// Degrees around X, Y, and Z. The turn around Z is done first, then X, then Y.
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
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

/// How a generated mesh looks. A file has its own colors and materials.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LookDef {
    /// Linear RGB. `None`: the color of the generator.
    pub color: Option<[f32; 3]>,
    /// `None`: diffuse.
    pub material: Option<Material>,
    /// A light of its own, linear RGB: the mesh shows with this color and lights
    /// the room by bounce rays. `None`: no light.
    pub emissive: Option<[f32; 3]>,
}

impl LookDef {
    fn apply(&self, mut mesh: Mesh) -> Mesh {
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
    #[serde(default)]
    pub name: Option<String>,
    pub mesh: MeshDef,
    #[serde(default)]
    pub pose: PoseDef,
    #[serde(default)]
    pub look: LookDef,
    /// True: the box around the prop blocks the player.
    #[serde(default = "yes")]
    pub collider: bool,
    /// For a file: the radius of each light of the file, in meters.
    #[serde(default = "default_light_radius")]
    pub light_radius: f32,
}

fn yes() -> bool {
    true
}

fn default_light_radius() -> f32 {
    0.15
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightDef {
    #[serde(default)]
    pub name: Option<String>,
    pub position: [f32; 3],
    /// Meters.
    pub radius: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    pub intensity: f32,
    #[serde(default = "yes")]
    pub on: bool,
    /// A cone light: the direction of the middle of the cone. `None`: straight down.
    #[serde(default)]
    pub direction: Option<[f32; 3]>,
    /// A cone light: degrees from `direction` to the edge of the cone. `None`: light
    /// in all directions.
    #[serde(default)]
    pub cone: Option<f32>,
}

/// A door in a doorway of the room. It turns on the side of the opening that is
/// nearer to the start of the wall, into the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorDef {
    #[serde(default)]
    pub name: Option<String>,
    /// Index in `doorways`, from 0.
    pub doorway: usize,
    /// Seconds from closed to open.
    #[serde(default = "default_door_seconds")]
    pub seconds: f32,
    /// Linear RGB.
    #[serde(default = "default_door_color")]
    pub color: [f32; 3],
    /// True: the door is open when the room loads.
    #[serde(default)]
    pub open: bool,
}

fn default_door_seconds() -> f32 {
    1.2
}

fn default_door_color() -> [f32; 3] {
    [0.35, 0.2, 0.1]
}

/// A thing that moves between two poses: see [`Mover`]. Its mesh is a generated shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoverDef {
    #[serde(default)]
    pub name: Option<String>,
    pub mesh: MeshDef,
    #[serde(default)]
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
    let room = parse_room(&text).map_err(|error| anyhow!("{}:{error}", path.display()))?;
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
        validate(&text).map_err(|error| anyhow!("{}:{error}", path.display()))?;
    }
    Ok((room, text))
}

/// Checks of other crates for a room file: each gets the text of the file and returns
/// the reason if the file is bad. A room that fails a check is not loaded. The events
/// crate checks the events with this, so a transfer into a room with a bad event does
/// not happen.
#[derive(Resource, Default)]
pub struct RoomValidators(pub Vec<RoomValidator>);

/// Gets the text of a room file. `Err`: the reason why the file is bad, with the
/// line and the column first (`3:7: ...`) if it has them.
pub type RoomValidator = fn(&str) -> Result<(), String>;

/// Marks an entity that belongs to the current room. Unload removes it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct RoomEntity;

/// Ask for a room: write `Some`. `load_requested_room` takes it in the next `Update`.
#[derive(Resource, Debug, Default)]
pub struct RoomRequest(pub Option<RoomTarget>);

#[derive(Debug, Clone, PartialEq)]
pub struct RoomTarget {
    pub path: PathBuf,
    /// The spawn point of the room where the player goes. `None`: the player stays
    /// where it is (a reload of the same room).
    pub spawn: Option<String>,
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

/// The models that room files named, by path. They stay loaded over room changes.
#[derive(Resource, Default)]
pub struct RoomModels(HashMap<PathBuf, Handle<Model>>);

/// A model of a prop while it loads. `spawn_loaded_models` replaces the entity with
/// one entity for each mesh and each light of the model.
#[derive(Component)]
pub(crate) struct PendingModel {
    model: Handle<Model>,
    collider: bool,
    light_radius: f32,
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
    mut cache: ResMut<RoomModels>,
    room_entities: Query<Entity, With<RoomEntity>>,
    mut players: Query<&mut Interpolated, With<Player>>,
) {
    // No `take` before this test: a write marks the request as changed in each frame.
    if request.0.is_none() {
        return;
    }
    let target = request.0.take().expect("tested above");
    let start = std::time::Instant::now();
    let loaded = read_room(&target.path, &validators).and_then(|(def, source)| {
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
    let mut spawner = Spawner {
        commands: &mut commands,
        meshes: &mut meshes,
        models: &mut models,
        cache: &mut cache,
        directory,
        owned: Vec::new(),
    };
    spawner.room(&def);
    let owned = spawner.owned;
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
        let shell = self.add(Mesh::room_with_doorways(
            width,
            depth,
            height,
            &def.doorways,
        ));
        self.commands
            .spawn((RoomEntity, Transform::IDENTITY, MeshInstance(shell)));
        for collider in room_colliders(width, depth, height, &def.doorways) {
            self.commands.spawn((RoomEntity, collider));
        }
        for door in &def.doors {
            self.door(door, &def.doorways[door.doorway], width, depth);
        }
        for prop in &def.props {
            self.prop(prop);
        }
        for mover in &def.movers {
            self.mover(mover);
        }
        for light in &def.lights {
            let mut entity = self.commands.spawn((
                RoomEntity,
                // A light can move in fixed steps (a script, a debug control).
                Interpolated::at(Transform::from_translation(light.position.into())),
                Light {
                    radius: light.radius,
                    color: light.color.into(),
                    intensity: light.intensity,
                    on: light.on,
                    direction: light
                        .direction
                        .and_then(|direction| Vec3::from(direction).try_normalize())
                        .unwrap_or(Vec3::NEG_Y),
                    cone_degrees: light.cone.unwrap_or(180.0),
                },
            ));
            if let Some(name) = &light.name {
                entity.insert(Name(name.clone()));
            }
        }
    }

    fn prop(&mut self, prop: &PropDef) {
        let transform = prop.pose.transform();
        let mut entity = match &prop.mesh {
            MeshDef::File(path) => {
                let path = self.directory.join(path);
                let (models, meshes) = (&mut *self.models, &mut *self.meshes);
                let model = *self
                    .cache
                    .0
                    .entry(path.clone())
                    .or_insert_with(|| load_gltf(&path, models, meshes));
                self.commands.spawn((
                    RoomEntity,
                    transform,
                    PendingModel {
                        model,
                        collider: prop.collider,
                        light_radius: prop.light_radius,
                    },
                ))
            }
            generated => {
                let mesh = generated.generate().expect("not a file");
                let mesh = prop.look.apply(mesh);
                let collider = prop.collider.then(|| collider_around(&mesh, &transform));
                let handle = self.add(mesh);
                let mut entity = self
                    .commands
                    .spawn((RoomEntity, transform, MeshInstance(handle)));
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

    fn mover(&mut self, mover: &MoverDef) {
        let Some(mesh) = mover.mesh.generate() else {
            tracing::error!(name = ?mover.name, "a mover cannot have a file as its mesh: left out");
            return;
        };
        let handle = self.add(mover.look.apply(mesh));
        let (a, b) = (mover.a.transform(), mover.b.transform());
        let mut entity = self.commands.spawn((
            RoomEntity,
            MeshInstance(handle),
            Interpolated::at(a),
            Mover::new(a, b, mover.seconds),
        ));
        if let Some(name) = &mover.name {
            entity.insert(Name(name.clone()));
        }
    }

    /// The door leaf: a thin box that fills the doorway when it is closed.
    fn door(&mut self, door: &DoorDef, doorway: &Doorway, width: f32, depth: f32) {
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
                    let mut entity =
                        commands.spawn((RoomEntity, *transform, MeshInstance(*handle)));
                    if pending.collider
                        && let Some(mesh) = meshes.get(*handle)
                    {
                        entity.insert(collider_around(mesh, transform));
                    }
                }
                for light in &model.lights {
                    commands.spawn((
                        RoomEntity,
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
    use maki_core::{App, Config};

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
        let no = RoomValidators(vec![|_| Err("2:1: bad event".to_owned())]);
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
            let mut commands = world.commands();
            let mut spawner = Spawner {
                commands: &mut commands,
                meshes: &mut meshes,
                models: &mut models,
                cache: &mut cache,
                directory: Path::new(""),
                owned: Vec::new(),
            };
            spawner.door(&room.doors[0], &room.doorways[0], room.width, room.depth);
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

        let mut app = App::new(Config::default()).unwrap();
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
}
