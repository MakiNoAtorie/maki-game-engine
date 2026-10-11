//! The player: an entity with the `Player` component that walks with the move actions
//! and stops at colliders.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::{Quat, Vec3};
use maki_assets::{Assets, Handle, LoadState, Mesh, Model, load_gltf};
use maki_core::{App, Config, FixedUpdate, Interpolated, Module, Startup, Time, Transform, Update};
use maki_events::{EventRunner, run_events};
use maki_platform::{Action, FixedActionState};
use maki_scene::{Camera, Collider, MeshInstance, Player, move_circle};

use crate::controls::{MoveTarget, move_direction};

/// The player is a circle of this radius for collisions, in meters.
pub const PLAYER_RADIUS: f32 = 0.3;
/// Height of the generated capsule.
const PLAYER_HEIGHT: f32 = 1.7;

/// The model of the player while it loads. `attach_player_model` replaces it with a
/// `MeshInstance`.
#[derive(Component)]
struct PendingPlayerModel(Handle<Model>);

/// Spawns the player and moves it with the move actions.
pub struct PlayerControllerModule;

impl Module for PlayerControllerModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            // After the events: a transfer command and the player then agree in each step
            // on who moves the player.
            .add_systems(FixedUpdate, move_player.after(run_events))
            .add_systems(Update, attach_player_model);
    }
}

fn spawn_player(
    mut commands: Commands,
    config: Res<Config>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
) {
    // The room loader moves the player to a spawn point.
    let pose = Interpolated::at(Transform::IDENTITY);
    match &config.player_model_path {
        Some(path) => {
            let model = load_gltf(path, &mut models, &mut meshes);
            commands.spawn((Player, pose, PendingPlayerModel(model)));
        }
        None => {
            commands.spawn((Player, pose, MeshInstance(meshes.add(capsule()))));
        }
    }
}

fn capsule() -> Mesh {
    Mesh::capsule(PLAYER_RADIUS, PLAYER_HEIGHT, 24, 8)
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
        let mesh = mesh.copied().unwrap_or_else(|| meshes.add(capsule()));
        commands
            .entity(entity)
            .remove::<PendingPlayerModel>()
            .insert(MeshInstance(mesh));
    }
}

// A system: each argument is a system parameter.
#[allow(clippy::too_many_arguments)]
fn move_player(
    actions: Res<FixedActionState>,
    target: Res<MoveTarget>,
    events: Res<EventRunner>,
    time: Res<Time>,
    config: Res<Config>,
    cameras: Query<&Camera>,
    colliders: Query<&Collider>,
    mut players: Query<&mut Interpolated, With<Player>>,
) {
    // An event that runs has the control: the player reads a message, or a fade runs.
    if *target != MoveTarget::Player || events.blocks_player() {
        return;
    }
    let direction = move_direction(&actions, cameras.iter().next());
    if direction == Vec3::ZERO {
        return;
    }
    let speed = if actions.pressed(Action::Run) {
        config.player_run_speed
    } else {
        config.player_walk_speed
    };
    let step = direction * speed * time.fixed_delta.as_secs_f32();
    // Few boxes: a copy for each step costs less than a search structure.
    let colliders: Vec<Collider> = colliders.iter().copied().collect();
    for mut pose in &mut players {
        pose.current.translation =
            move_circle(pose.current.translation, step, PLAYER_RADIUS, &colliders);
        // The player faces the way it walks: +Z of the model is its front.
        pose.current.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
    }
}
