//! Characters that walk: the [`Walker`] component and the system that moves it, and
//! the player, a walker that the move actions steer.

use std::path::PathBuf;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::{Quat, Vec3};
use maki_assets::{
    AssetChanged, AssetWatch, Assets, Handle, LoadState, Mesh, Model, Textures, load_gltf,
};
use maki_core::{
    App, FixedUpdate, Interpolated, Module, Settings, Startup, Time, Transform, Update,
};
use maki_events::{EventRunner, run_events};
use maki_platform::FixedActionState;
use maki_scene::{ActiveCamera, Camera, Collider, MeshInstance, Player, move_circle};
use serde::{Deserialize, Serialize};

use crate::controls::{MoveCapture, RUN, move_direction};

/// An entity that walks on the floor and stops at colliders. `walk` moves it; some
/// other system tells it where to go (`steer_player` for the player; an NPC module
/// for its characters).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Walker {
    /// The entity is a circle of this radius for collisions, in meters.
    pub radius: f32,
    /// Where to walk: a direction on the floor of length 1, or zero to stand.
    pub direction: Vec3,
    /// Meters per second.
    pub speed: f32,
}

/// The `[player]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerSettings {
    /// glTF file of the player (`.gltf` or `.glb`), with the feet at its origin.
    /// No key: a generated capsule.
    pub model_path: Option<PathBuf>,
    /// Meters per second.
    pub walk_speed: f32,
    /// With the run action.
    pub run_speed: f32,
    /// The player is a circle of this radius for collisions, in meters. Read when
    /// the player is spawned.
    pub radius: f32,
    /// Height of the generated capsule, in meters.
    pub height: f32,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            model_path: None,
            walk_speed: 1.6,
            run_speed: 3.6,
            radius: 0.3,
            height: 1.7,
        }
    }
}

impl Settings for PlayerSettings {
    const TABLE: &'static str = "player";

    fn check(&self) -> Result<(), String> {
        let positive = |value: f32| value.is_finite() && value >= 0.0;
        if !(positive(self.walk_speed) && positive(self.run_speed)) {
            return Err("walk_speed and run_speed must be zero or positive numbers".to_owned());
        }
        if !(self.radius.is_finite() && self.radius > 0.0 && self.height >= 2.0 * self.radius) {
            return Err("radius must be positive, and height at least two times radius".to_owned());
        }
        Ok(())
    }
}

/// The model of the player while it loads. `attach_player_model` replaces it with a
/// `MeshInstance`.
#[derive(Component)]
struct PendingPlayerModel(Handle<Model>);

/// The model of the player, from the file of the settings. It stays on the player:
/// `reload_player_model` removes it when the file changes.
#[derive(Component)]
struct PlayerModel(Handle<Model>);

/// Spawns the player, steers it with the move actions, and moves each [`Walker`].
/// Needs `ControlsModule` (the move actions) and `maki_events::EventsModule`.
pub struct PlayerModule;

impl Module for PlayerModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<PlayerSettings>()
            .init_resource::<Textures>()
            .init_resource::<MoveCapture>()
            .add_systems(Startup, spawn_player)
            // After the events: a transfer command and the player then agree in each step
            // on who moves the player.
            .add_systems(FixedUpdate, (steer_player, walk).chain().after(run_events))
            .add_systems(Update, attach_player_model)
            // The watch of the asset files, if `AssetsModule` is there: the model
            // of the player has no room that loads it again.
            .add_message::<AssetChanged>()
            .add_systems(
                Update,
                reload_player_model
                    .after(maki_assets::forget_changed)
                    .before(attach_player_model),
            );
        let model = app.world().resource::<PlayerSettings>().model_path.clone();
        if let Some(model) = model {
            let check: maki_assets::Check = maki_assets::check_model;
            app.init_resource::<AssetWatch>();
            app.resource_mut::<AssetWatch>()
                .set("player", vec![(model, check)]);
        }
    }
}

pub(crate) fn spawn_player(
    mut commands: Commands,
    settings: Res<PlayerSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
) {
    // The room loader moves the player to a spawn point.
    let pose = Interpolated::at(Transform::IDENTITY);
    let walker = Walker {
        radius: settings.radius,
        direction: Vec3::ZERO,
        speed: 0.0,
    };
    match &settings.model_path {
        Some(path) => {
            let model = load_gltf(path, &mut models, &mut meshes, &mut textures);
            let parts = (PendingPlayerModel(model), PlayerModel(model));
            commands.spawn((Player, walker, pose, parts));
        }
        None => {
            let mesh = meshes.add(capsule(&settings));
            commands.spawn((Player, walker, pose, MeshInstance(mesh)));
        }
    }
}

fn capsule(settings: &PlayerSettings) -> Mesh {
    Mesh::capsule(settings.radius, settings.height, 24, 8)
}

/// Runs in `Update`: the model file of the player, or an image file of it, changed
/// on the disk (`maki_assets::AssetChanged`). The file is read again, and the player
/// gets the new model.
fn reload_player_model(
    mut changed: MessageReader<AssetChanged>,
    mut commands: Commands,
    settings: Res<PlayerSettings>,
    mut players: Query<(Entity, &mut PlayerModel)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
    mut textures: ResMut<Textures>,
) {
    let files: Vec<&AssetChanged> = changed.read().collect();
    let Some(path) = settings.model_path.as_ref().filter(|_| !files.is_empty()) else {
        return;
    };
    for (entity, mut model) in &mut players {
        // A texture of an image file has the name of the file.
        let image = |texture: Option<maki_assets::TextureRef>| {
            let texture = texture.and_then(|texture| textures.get(texture.texture));
            texture.is_some_and(|texture| {
                files
                    .iter()
                    .any(|file| file.0.as_os_str() == &*texture.name)
            })
        };
        let uses = |mesh: &Handle<Mesh>| {
            let mesh = meshes.get(*mesh);
            mesh.is_some_and(|mesh| {
                image(mesh.base_color_texture()) || image(mesh.emissive_texture())
            })
        };
        let own = models
            .get(model.0)
            .is_some_and(|model| model.meshes.iter().any(uses));
        if !(own || files.iter().any(|file| file.0 == *path)) {
            continue;
        }
        let old = models.remove(model.0);
        for mesh in old.map_or(Vec::new(), |model| model.meshes) {
            meshes.remove(mesh);
        }
        model.0 = load_gltf(path, &mut models, &mut meshes, &mut textures);
        commands
            .entity(entity)
            .remove::<MeshInstance>()
            .insert(PendingPlayerModel(model.0));
    }
}

/// Polls the model of the player, as `spawn_loaded_models` of the scene does.
fn attach_player_model(
    mut commands: Commands,
    settings: Res<PlayerSettings>,
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
            .unwrap_or_else(|| meshes.add(capsule(&settings)));
        commands
            .entity(entity)
            .remove::<PendingPlayerModel>()
            .insert(MeshInstance(mesh));
    }
}

/// Runs in `FixedUpdate`: the move actions tell the player where to walk.
pub fn steer_player(
    actions: Res<FixedActionState>,
    capture: Res<MoveCapture>,
    events: Res<EventRunner>,
    settings: Res<PlayerSettings>,
    cameras: Query<&Camera, With<ActiveCamera>>,
    mut players: Query<&mut Walker, With<Player>>,
) {
    // An event that runs has the control: the player reads a message, or a fade runs.
    let direction = if capture.0 || events.blocks_player() {
        Vec3::ZERO
    } else {
        move_direction(&actions, cameras.iter().next())
    };
    let speed = if actions.pressed(RUN) {
        settings.run_speed
    } else {
        settings.walk_speed
    };
    for mut walker in &mut players {
        (walker.direction, walker.speed) = (direction, speed);
    }
}

/// Runs in `FixedUpdate`: one step of each walker. A system that steers a walker runs
/// `.before(walk)`.
pub fn walk(
    time: Res<Time>,
    colliders: Query<&Collider>,
    mut walkers: Query<(&mut Interpolated, &Walker)>,
) {
    // Few boxes: a copy for each step costs less than a search structure.
    let colliders: Vec<Collider> = colliders.iter().copied().collect();
    for (mut pose, walker) in &mut walkers {
        if walker.direction == Vec3::ZERO {
            continue;
        }
        let step = walker.direction * walker.speed * time.fixed_delta.as_secs_f32();
        pose.current.translation =
            move_circle(pose.current.translation, step, walker.radius, &colliders);
        // The walker faces the way it walks: +Z of the model is its front.
        pose.current.rotation = Quat::from_rotation_y(walker.direction.x.atan2(walker.direction.z));
    }
}

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
    fn the_model_of_the_player_is_read_again_when_its_file_changed() {
        let directory = std::env::temp_dir().join(format!("maki-player-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("player.gltf");
        std::fs::write(&file, TRIANGLE).unwrap();
        // A path in a TOML text: no backslash.
        let path = file.to_string_lossy().replace('\\', "/");
        let config = format!("[player]\nmodel_path = \"{path}\"");
        let mut app = App::new(ConfigFile::from_text(&config)).unwrap();
        app.add_module(PlatformModule)
            .add_module(maki_assets::AssetsModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_module(ControlsModule)
            .add_module(PlayerModule);
        // The watch knows the file, also while it is off.
        let watched: Vec<PathBuf> = {
            let watch = app.world().resource::<AssetWatch>();
            watch.files().map(|file| file.to_owned()).collect()
        };
        assert_eq!(watched, [PathBuf::from(&path)]);

        app.run(|mut app| {
            // The mesh that shows the player, and the number of meshes in the store.
            let state = |app: &App| {
                let world = app.world();
                let mut players = world
                    .try_query_filtered::<&MeshInstance, With<Player>>()
                    .unwrap();
                let mesh = players.iter(world).next().map(|instance| instance.0);
                (mesh, world.resource::<Assets<Mesh>>().iter().count())
            };
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            let (first, count) = state(&app);
            assert!(first.is_some() && count == 1);

            // Another file changed: nothing. The model file: a new mesh, and the
            // old one is gone from the store.
            app.write_message(AssetChanged(directory.join("other.png")));
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            assert_eq!(state(&app), (first, 1));
            app.write_message(AssetChanged(PathBuf::from(&path)));
            app.advance(Duration::from_millis(16));
            app.advance(Duration::from_millis(16));
            let (second, count) = state(&app);
            assert!(second.is_some() && second != first && count == 1);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
