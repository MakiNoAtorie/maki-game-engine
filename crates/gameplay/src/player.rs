//! Characters that walk: the [`Walker`] component and the system that moves it, and
//! the player, a walker that the move actions steer.
//!
//! The player is data: a prefab file that the project file names. Its object has the
//! component `player` ([`PlayerDef`]: the speeds, the radius, the step, and the slope)
//! and `prop` (its mesh: a capsule, or a model file).
//!
//! A walker is the character controller of the physics: an upright capsule that slides
//! along the colliders. It is also a [`Kinematic`] body, so areas and queries see it.

use std::path::{Path, PathBuf};

use maki_assets::FileRef;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::{Changed, With};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, EntityCommands, Local, Query, Res, ResMut};
use glam::{Quat, Vec3};
use maki_assets::{
    AssetChanged, AssetWatch, Assets, Handle, LoadState, Mesh, Model, Textures, load_gltf,
};
use maki_core::{
    App, Fields, FixedUpdate, Interpolated, Module, ResetPlay, Startup, Time, Transform, Update,
    interpolate_transforms,
};
use maki_events::run_events;
use maki_physics::{CharacterSettings, Kinematic, Physics, PhysicsSet, QueryFilter, Shape};
use maki_platform::FixedActionState;
use maki_scene::{
    ActiveCamera, Camera, CurrentRoom, MeshDef, MeshInstance, ObjectDef, Player,
    add_component_type, read_prefab,
};
use serde::{Deserialize, Serialize};

use crate::control::{Controller, control_home, control_home_if_lost};
use crate::controls::{MoveCapture, RUN, move_direction};

/// An entity that walks on the floor and stops at colliders. `walk` moves it; some
/// other system tells it where to go (`steer_player` for the player; an NPC module
/// for its characters).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Walker {
    /// The entity is an upright capsule of this radius for collisions, in meters.
    pub radius: f32,
    /// The full height of the capsule, in meters. It stands on the pose of the entity.
    pub height: f32,
    /// The highest step that the walker goes up, in meters.
    pub step_height: f32,
    /// The steepest slope that the walker goes up, in degrees.
    pub max_slope: f32,
    /// Where to walk: a direction on the floor of length 1, or zero to stand.
    pub direction: Vec3,
    /// Meters per second.
    pub speed: f32,
}

impl Walker {
    /// The collider of the walker: the capsule, from the pose up. A height of less than
    /// two radii gives a sphere.
    pub fn body(&self) -> Kinematic {
        let half_height = (self.height * 0.5 - self.radius).max(0.0);
        Kinematic {
            shape: Shape::Capsule {
                radius: self.radius,
                half_height,
            },
            center: Vec3::Y * (half_height + self.radius),
            sensor: false,
        }
    }
}

/// The component `player` of the player prefab, and of the player entity.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct PlayerDef {
    /// Meters per second.
    #[field(label = "walk speed", range = 0.0..=10.0, step = 0.05)]
    pub walk_speed: f32,
    /// With the run action.
    #[field(label = "run speed", range = 0.0..=10.0, step = 0.05)]
    pub run_speed: f32,
    /// The player is a capsule of this radius for collisions, in meters. Its height is
    /// that of the capsule mesh, or 1.7 m for a model file.
    #[field(range = 0.05..=2.0, step = 0.01)]
    pub radius: f32,
    /// The highest step that the player goes up, in meters.
    #[field(label = "step height", range = 0.0..=1.0, step = 0.01)]
    pub step_height: f32,
    /// The steepest slope that the player goes up, in degrees.
    #[field(label = "max slope", range = 0.0..=90.0, step = 1.0)]
    pub max_slope: f32,
}

impl PlayerDef {
    /// `Err`: why the values cannot be those of a player.
    fn check(&self, mesh: &MeshDef) -> Result<(), String> {
        let speed = |value: f32| value.is_finite() && value >= 0.0;
        if !(speed(self.walk_speed) && speed(self.run_speed)) {
            return Err("walk_speed and run_speed must be zero or positive numbers".to_owned());
        }
        if !(self.radius.is_finite() && self.radius > 0.0) {
            return Err("radius must be a positive number".to_owned());
        }
        if !(self.step_height.is_finite() && self.step_height >= 0.0) {
            return Err("step_height must be zero or a positive number".to_owned());
        }
        if !(0.0..=90.0).contains(&self.max_slope) {
            return Err("max_slope must be from 0 to 90 degrees".to_owned());
        }
        if let MeshDef::Capsule { radius, height } = mesh
            && !(*radius > 0.0 && *height >= 2.0 * radius)
        {
            return Err(
                "a capsule needs a positive radius, and a height of two radii or more".to_owned(),
            );
        }
        Ok(())
    }

    /// The walker of a player of these values and this mesh, at rest.
    fn walker(&self, mesh: &MeshDef) -> Walker {
        let height = match mesh {
            MeshDef::Capsule { height, .. } => *height,
            _ => HEIGHT,
        };
        Walker {
            radius: self.radius,
            height,
            step_height: self.step_height,
            max_slope: self.max_slope,
            direction: Vec3::ZERO,
            speed: 0.0,
        }
    }
}

impl Default for PlayerDef {
    fn default() -> Self {
        Self {
            walk_speed: 1.6,
            run_speed: 3.6,
            radius: 0.3,
            step_height: 0.25,
            max_slope: 45.0,
        }
    }
}

/// The height of a player with no prefab file, and of one with a model file.
const HEIGHT: f32 = 1.7;

/// The mesh of a player with no prefab file, and of a model that does not load.
const CAPSULE: MeshDef = MeshDef::Capsule {
    radius: 0.3,
    height: HEIGHT,
};

/// The model of the player while it loads. `attach_player_model` replaces it with a
/// `MeshInstance`.
#[derive(Component)]
struct PendingPlayerModel(Handle<Model>);

/// The model of the player and its file. It stays on the player:
/// `reload_player` removes it when the file changes.
#[derive(Component)]
struct PlayerModel(Handle<Model>, PathBuf);

/// The prefab file of the player, and the folder that its file names start at.
#[derive(bevy_ecs::resource::Resource, Clone)]
pub(crate) struct PlayerFile {
    rooms: PathBuf,
    prefab: Option<FileRef>,
}

/// Spawns the player from its prefab file, steers it with the move actions, moves it
/// to the spawn point of a room that loads, and moves each [`Walker`]. Needs
/// `ControlsModule` (the move actions), `maki_events::EventsModule`,
/// `maki_physics::PhysicsModule`, and `GameModule` (the rules: when the control is on).
#[derive(Debug, Clone, Default)]
pub struct PlayerModule {
    /// The folder of the room files: a file name in the prefab starts there, as in
    /// a prefab that a room names.
    pub rooms: PathBuf,
    /// The prefab file, from `rooms`. `None`: a capsule with the values of
    /// [`PlayerDef::default`].
    pub prefab: Option<FileRef>,
}

impl Module for PlayerModule {
    fn build(&self, app: &mut App) {
        add_component_type::<PlayerDef>(app, "player");
        let file = PlayerFile {
            rooms: self.rooms.clone(),
            prefab: self.prefab.clone(),
        };
        app.insert_resource(file.clone())
            .init_resource::<Textures>()
            .init_resource::<MoveCapture>()
            .init_resource::<Controller>()
            .add_systems(Startup, spawn_player)
            // Exclusive systems: only the order to the loader and the placement counts.
            .add_systems(ResetPlay, control_home.ambiguous_with_all())
            .add_systems(
                Update,
                control_home_if_lost
                    .after(maki_scene::load_requested_room)
                    .before(place_player)
                    .ambiguous_with_all(),
            )
            // After the events: a transfer command and the player then agree in each step
            // on who moves the player.
            .add_systems(
                FixedUpdate,
                (steer_player, fit_walker_bodies, walk)
                    .chain()
                    .after(run_events)
                    .before(PhysicsSet::Step),
            )
            // Before the interpolation: the player shows at the spawn point in the
            // frame of the load.
            .add_systems(
                Update,
                place_player
                    .after(maki_scene::load_requested_room)
                    .before(interpolate_transforms),
            )
            .add_systems(Update, attach_player_model)
            // The watch of the asset files, if `AssetsModule` is there: the player
            // has no room that loads it again.
            .add_message::<AssetChanged>()
            .add_systems(
                Update,
                reload_player
                    .after(maki_assets::forget_changed)
                    .before(attach_player_model),
            );
        if file.prefab.is_some() {
            let mesh = file.read().map_or(CAPSULE, |(_, mesh)| mesh);
            app.init_resource::<AssetWatch>();
            app.resource_mut::<AssetWatch>()
                .set("player", file.watched(&mesh));
        }
    }
}

impl PlayerFile {
    fn prefab_path(&self) -> Option<PathBuf> {
        Some(self.prefab.as_ref()?.resolve(&self.rooms))
    }

    /// The values and the mesh of the player. An error: the prefab file does not
    /// read, or it has no object with a `player`.
    fn read(&self) -> anyhow::Result<(PlayerDef, MeshDef)> {
        let Some(path) = self.prefab_path() else {
            return Ok((PlayerDef::default(), CAPSULE));
        };
        let prefab = read_prefab(&path)?;
        let is_player = |object: &&ObjectDef| object.components.contains_key("player");
        let object = prefab.objects.iter().find(is_player);
        let object = object.ok_or_else(|| {
            anyhow::anyhow!("{}: no object has the component `player`", path.display())
        })?;
        let def: PlayerDef = object.component("player").expect("found above")?;
        let mesh = match object.prop() {
            Some(prop) => prop?.mesh,
            None => CAPSULE,
        };
        def.check(&mesh)
            .map_err(|reason| anyhow::anyhow!("{}: {reason}", path.display()))?;
        Ok((def, mesh))
    }

    /// The files that the watch looks at: the prefab file, and its model file.
    fn watched(&self, mesh: &MeshDef) -> Vec<(PathBuf, maki_assets::Check)> {
        let check_prefab: maki_assets::Check = |path| read_prefab(path).map(drop);
        let mut files: Vec<_> = self
            .prefab_path()
            .map(|path| (path, check_prefab))
            .into_iter()
            .collect();
        if let MeshDef::File(model) = mesh {
            let check: maki_assets::Check = maki_assets::check_model;
            files.push((model.resolve(&self.rooms), check));
        }
        files
    }

    /// As [`PlayerFile::read`]; an error is logged, and the player is the capsule.
    fn read_or_default(&self) -> (PlayerDef, MeshDef) {
        self.read().unwrap_or_else(|error| {
            tracing::error!("the player is a capsule: {error:#}");
            (PlayerDef::default(), CAPSULE)
        })
    }
}

pub(crate) fn spawn_player(
    mut commands: Commands,
    file: Res<PlayerFile>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
    mut controller: ResMut<Controller>,
) {
    let (def, mesh) = file.read_or_default();
    // `place_player` moves the player to a spawn point.
    let walker = def.walker(&mesh);
    let mut entity = commands.spawn((Player, walker, def, Interpolated::at(Transform::IDENTITY)));
    controller.home = Some(entity.id());
    let stores = (&mut *meshes, &mut *models, &mut *textures);
    give_mesh(&mut entity, &mesh, &file.rooms, stores);
}

/// Gives the player its mesh: a generated one, or a model file that loads.
fn give_mesh(
    entity: &mut EntityCommands,
    mesh: &MeshDef,
    rooms: &Path,
    (meshes, models, textures): (&mut Assets<Mesh>, &mut Assets<Model>, &mut Textures),
) {
    match mesh {
        MeshDef::File(path) => {
            let path = path.resolve(rooms);
            let model = load_gltf(&path, models, meshes, textures);
            entity.insert((PendingPlayerModel(model), PlayerModel(model, path)));
        }
        mesh => drop(entity.insert(MeshInstance(meshes.add(generated(mesh))))),
    }
}

/// The mesh of a generated shape; the capsule for a file.
fn generated(mesh: &MeshDef) -> Mesh {
    let capsule = || CAPSULE.generate().expect("a capsule is a shape");
    mesh.generate().unwrap_or_else(capsule)
}

/// Runs in `Update`: the prefab file of the player, its model file, or an image file
/// of the model changed on the disk (`maki_assets::AssetChanged`). The player gets the
/// values and the mesh of the files as they are now.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn reload_player(
    mut changed: MessageReader<AssetChanged>,
    mut commands: Commands,
    file: Res<PlayerFile>,
    mut players: Query<
        (
            Entity,
            &mut Walker,
            Option<&PlayerModel>,
            Option<&MeshInstance>,
        ),
        With<PlayerDef>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
    watch: Option<ResMut<AssetWatch>>,
    controller: Res<Controller>,
) {
    let files: Vec<PathBuf> = changed.read().map(|changed| changed.0.clone()).collect();
    if files.is_empty() {
        return;
    }
    let prefab = file.prefab_path();
    let prefab_changed = files.iter().any(|path| Some(path) == prefab.as_ref());
    // The prefab can name another model file: the watch follows it.
    if let (true, Some(mut watch)) = (prefab_changed, watch) {
        let mesh = file.read().map_or(CAPSULE, |(_, mesh)| mesh);
        watch.set("player", file.watched(&mesh));
    }
    for (entity, mut walker, model, instance) in &mut players {
        // The body of the prefab, not an object that has the control now.
        if Some(entity) != controller.home {
            continue;
        }
        // A texture of an image file has the name of the file.
        let model_changed = model.is_some_and(|PlayerModel(handle, path)| {
            let image = |texture: Option<maki_assets::TextureRef>| {
                let texture = texture.and_then(|texture| textures.get(texture.texture));
                texture.is_some_and(|texture| {
                    files.iter().any(|file| file.as_os_str() == &*texture.name)
                })
            };
            let uses = |mesh: &Handle<Mesh>| {
                meshes.get(*mesh).is_some_and(|mesh| {
                    image(mesh.base_color_texture()) || image(mesh.emissive_texture())
                })
            };
            let own = models
                .get(*handle)
                .is_some_and(|model| model.meshes.iter().any(uses));
            own || files.contains(path)
        });
        if !(prefab_changed || model_changed) {
            continue;
        }
        // The old mesh, or the old model with its meshes, goes.
        match (model, instance) {
            (Some(PlayerModel(handle, _)), _) => {
                let old = models.remove(*handle);
                for mesh in old.map_or(Vec::new(), |model| model.meshes) {
                    meshes.remove(mesh);
                }
            }
            (None, Some(instance)) => drop(meshes.remove(instance.0)),
            (None, None) => {}
        }
        let (def, mesh) = file.read_or_default();
        *walker = Walker {
            direction: walker.direction,
            speed: walker.speed,
            ..def.walker(&mesh)
        };
        let mut entity = commands.entity(entity);
        entity
            .remove::<(MeshInstance, PendingPlayerModel, PlayerModel)>()
            .insert(def);
        let stores = (&mut *meshes, &mut *models, &mut *textures);
        give_mesh(&mut entity, &mesh, &file.rooms, stores);
    }
}

/// Polls the model of the player, as `spawn_loaded_models` of the scene does.
fn attach_player_model(
    mut commands: Commands,
    pending: Query<(Entity, &PendingPlayerModel)>,
    models: Res<Assets<Model>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (entity, PendingPlayerModel(handle)) in &pending {
        let mesh = match models.state(*handle) {
            Some(LoadState::Loading) => continue,
            // ponytail: one mesh instance for the model. All meshes of a model are one
            // BLAS, so an instance of the first mesh draws them all (the renderer logs
            // a warning for a model of more than one mesh). Add a model instance
            // component when a model must show only a part.
            Some(LoadState::Ready) => models.get(*handle).and_then(|model| model.meshes.first()),
            Some(LoadState::Failed) | None => None,
        };
        // The loader logged a failure. The player then is the capsule.
        let mesh = mesh
            .copied()
            .unwrap_or_else(|| meshes.add(generated(&CAPSULE)));
        commands
            .entity(entity)
            .remove::<PendingPlayerModel>()
            .insert(MeshInstance(mesh));
    }
}

/// Runs in `Update`, after the room loader: the player goes to the spawn point that
/// the load asked for, once for each load.
fn place_player(
    current: Res<CurrentRoom>,
    mut seen: Local<u64>,
    mut players: Query<&mut Interpolated, With<Player>>,
) {
    if current.loads == *seen {
        return;
    }
    *seen = current.loads;
    let Some(position) = current.spawn else {
        return;
    };
    for mut pose in &mut players {
        let mut at = pose.current;
        at.translation = position.into();
        // No blend from the old place.
        pose.snap(at);
    }
}

/// Runs in `FixedUpdate`: the controller. The move actions tell the pawn where to
/// walk, while the rules of the game let them (`Controller::active`).
pub fn steer_player(
    actions: Res<FixedActionState>,
    capture: Res<MoveCapture>,
    controller: Res<Controller>,
    cameras: Query<&Camera, With<ActiveCamera>>,
    mut players: Query<(&mut Walker, &PlayerDef), With<Player>>,
) {
    let direction = if capture.0 || !controller.active {
        Vec3::ZERO
    } else {
        move_direction(&actions, cameras.iter().next())
    };
    for (mut walker, def) in &mut players {
        let speed = if actions.pressed(RUN) {
            def.run_speed
        } else {
            def.walk_speed
        };
        (walker.direction, walker.speed) = (direction, speed);
    }
}

/// Runs in `FixedUpdate`: gives each walker the [`Kinematic`] of its capsule, and a new
/// one when its size changes.
fn fit_walker_bodies(
    mut commands: Commands,
    walkers: Query<(Entity, &Walker, Option<&Kinematic>), Changed<Walker>>,
) {
    for (entity, walker, body) in &walkers {
        let fitted = walker.body();
        // No write of the same body: a write makes the physics build its shape again.
        if body != Some(&fitted) {
            commands.entity(entity).insert(fitted);
        }
    }
}

/// How fast a walker above the floor falls, in meters per second.
// ponytail: a fall at one speed. Add a vertical speed to `Walker` when a character can fall far.
const FALL_SPEED: f32 = 5.0;

/// A step of the controller shorter than this, in meters, is no step, and a walker this
/// near the floor is on it. Against a wall the controller moves by some 1e-7 m in each
/// step: the walker must stand still (the scripts of the benchmark wait for it).
const STILL: f32 = 1e-4;

/// Runs in `FixedUpdate`: one step of each walker, by the character controller of the
/// physics. A system that steers a walker runs `.before(walk)`.
///
/// The rooms are flat, with the floor at y = 0 and no collider under it: a walker
/// above the floor (on a step) falls to it, and never goes below it. The fall is a move
/// of its own after the walk: in the move of the walk it would slide the walker down a
/// slope that it can stand on. A walker that stands on a collider after the walk does
/// not fall: the round end of the capsule goes up the edge of a step as up a slope.
///
/// A standing walker makes no move: Rapier then pushes the capsule out of what it
/// overlaps, also into a wall. So a door leaf that turns into a walker does not push
/// it: the leaf goes through, and the walker can walk out of it.
pub fn walk(
    time: Res<Time>,
    physics: Res<Physics>,
    mut walkers: Query<(Entity, &mut Interpolated, &Walker)>,
) {
    let seconds = time.fixed_delta.as_secs_f32();
    for (entity, mut pose, walker) in &mut walkers {
        let at = pose.current.translation;
        if walker.direction == Vec3::ZERO && at.y <= 0.0 {
            continue;
        }
        let body = walker.body();
        let settings = CharacterSettings {
            step_height: walker.step_height,
            max_slope_degrees: walker.max_slope,
            ..CharacterSettings::default()
        };
        let filter = QueryFilter {
            exclude: Some(entity),
            ..QueryFilter::default()
        };
        // The place after a move from `from` by `wanted`, and whether it stands on a collider.
        let moved = |from: Vec3, wanted: Vec3| {
            let center = from + body.center;
            let moved = physics.move_character(body.shape, center, wanted, &settings, filter);
            let step = if moved.step.length() < STILL {
                Vec3::ZERO
            } else {
                moved.step
            };
            (from + step, moved.grounded)
        };
        let (mut end, grounded) = if walker.direction == Vec3::ZERO {
            (at, false)
        } else {
            moved(at, walker.direction * walker.speed * seconds)
        };
        let fall = end.y.clamp(0.0, FALL_SPEED * seconds);
        if fall > 0.0 && !grounded {
            end = moved(end, Vec3::NEG_Y * fall).0;
        }
        if end.y < STILL {
            end.y = 0.0;
        }
        pose.current.translation = end;
        if walker.direction != Vec3::ZERO {
            // The walker faces the way it walks: +Z of the model is its front.
            let facing = walker.direction.x.atan2(walker.direction.z);
            pose.current.rotation = Quat::from_rotation_y(facing);
        }
    }
}

#[cfg(test)]
mod walk_tests;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::ConfigFile;
    use maki_events::EventsModule;
    use maki_platform::PlatformModule;
    use maki_scene::SceneModule;

    use super::*;
    use crate::ControlsModule;

    /// One triangle.
    const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
"buffers":[{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA","byteLength":36}],
"bufferViews":[{"buffer":0,"byteLength":36}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]}"#;

    #[test]
    fn the_player_is_made_from_its_prefab_and_follows_its_files() {
        let directory = std::env::temp_dir().join(format!("maki-player-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let model = directory.join("player.gltf");
        std::fs::write(&model, TRIANGLE).unwrap();
        let prefab = directory.join("player.prefab");
        let player = |speed: f32, mesh: &str| {
            format!(
                r#"(objects: [(components: {{"player": (walk_speed: {speed:?}, run_speed: 3.0, radius: 0.25), "prop": (mesh: {mesh})}})])"#
            )
        };
        std::fs::write(&prefab, player(1.5, r#"File("player.gltf")"#)).unwrap();
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule)
            .add_module(maki_assets::AssetsModule)
            .add_module(SceneModule)
            .add_module(maki_physics::PhysicsModule)
            .add_module(EventsModule)
            .add_module(ControlsModule)
            .add_module(PlayerModule {
                rooms: directory.clone(),
                prefab: Some(FileRef::from("player.prefab")),
            });
        // The watch knows the two files, also while it is off.
        let watched: Vec<PathBuf> = {
            let watch = app.world().resource::<AssetWatch>();
            watch.files().map(|file| file.to_owned()).collect()
        };
        assert_eq!(watched, [prefab.clone(), model.clone()]);

        app.run(|mut app| {
            // The mesh that shows the player, its values, and the number of meshes
            // in the store.
            let state = |app: &App| {
                let world = app.world();
                let mut players = world
                    .try_query_filtered::<(Option<&MeshInstance>, &PlayerDef, &Walker), With<Player>>()
                    .unwrap();
                let (mesh, def, walker) = players.single(world).unwrap();
                let speeds = (def.walk_speed, walker.radius);
                (mesh.map(|instance| instance.0), speeds, world.resource::<Assets<Mesh>>().iter().count())
            };
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            let (first, speeds, count) = state(&app);
            assert!(first.is_some() && count == 1);
            assert_eq!(speeds, (1.5, 0.25));
            // The body of the player, for areas and queries: a capsule of the radius.
            let world = app.world();
            let mut bodies = world.try_query_filtered::<&Kinematic, With<Player>>().unwrap();
            let body = bodies.single(world).unwrap();
            assert!(matches!(body.shape, Shape::Capsule { radius: 0.25, .. }));

            // Another file changed: nothing. The model file: a new mesh, and the
            // old one is gone from the store.
            app.write_message(AssetChanged(directory.join("other.png")));
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            assert_eq!(state(&app), (first, speeds, 1));
            app.write_message(AssetChanged(model.clone()));
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            let (second, _, count) = state(&app);
            assert!(second.is_some() && second != first && count == 1);

            // The prefab file: the new values, and a capsule in place of the model.
            std::fs::write(&prefab, player(2.0, "Capsule(radius: 0.25, height: 1.6)")).unwrap();
            app.write_message(AssetChanged(prefab.clone()));
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            let (third, speeds, count) = state(&app);
            assert!(third.is_some() && third != second && count == 1);
            assert_eq!(speeds, (2.0, 0.25));
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
