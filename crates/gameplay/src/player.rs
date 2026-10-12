//! Characters that walk: the [`Walker`] component and the system that moves it, and
//! the player, a walker that the move actions steer.

use std::path::PathBuf;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::{Quat, Vec3};
use maki_assets::{Assets, Handle, LoadState, Mesh, Model, Textures, load_gltf};
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
            .add_systems(Update, attach_player_model);
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
            commands.spawn((Player, walker, pose, PendingPlayerModel(model)));
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
