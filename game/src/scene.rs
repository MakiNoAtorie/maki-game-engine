//! The scene: the room, the model, and the camera as entities, and the copy of the
//! mesh instances into the render world.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::Vec3;
use maki_assets::{Assets, Handle, LoadState, Mesh, Model, load_gltf};
use maki_core::{App, Config, Extract, Module, Startup, Transform, Update};
use maki_render::{Camera, ExtractedInstance, ExtractedInstances, RenderWorld};

/// Draws a mesh at the `Transform` of the entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct MeshInstance(pub Handle<Mesh>);

/// A model that is still loading. When it is ready, `spawn_loaded_models` replaces
/// the entity with one `MeshInstance` entity for each mesh of the model.
#[derive(Component)]
struct PendingModel(Handle<Model>);

pub struct SceneModule;

impl Module for SceneModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_scene)
            .add_systems(Update, spawn_loaded_models)
            .add_systems(Extract, extract_instances);
    }
}

fn spawn_scene(
    mut commands: Commands,
    config: Res<Config>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut models: ResMut<Assets<Model>>,
) {
    // A fixed view. The renderer copies the camera in `Extract`.
    commands.spawn(Camera {
        position: config.camera_position.into(),
        target: config.camera_target.into(),
        fov_y_degrees: config.camera_fov_degrees,
    });

    let room = Mesh::room(config.room_width, config.room_depth, config.room_height);
    commands.spawn((Transform::IDENTITY, MeshInstance(meshes.add(room))));

    match &config.model_path {
        Some(path) => {
            let model = load_gltf(path, &mut models, &mut meshes);
            commands.spawn((Transform::IDENTITY, PendingModel(model)));
        }
        // No model in the config: a cube and a sphere of 1 m, on the floor.
        None => {
            commands.spawn((
                Transform::from_translation(Vec3::new(-1.0, 0.5, 0.0)),
                MeshInstance(meshes.add(Mesh::cube(1.0))),
            ));
            commands.spawn((
                Transform::from_translation(Vec3::new(1.0, 0.5, 0.0)),
                MeshInstance(meshes.add(Mesh::sphere(0.5, 32, 16))),
            ));
        }
    }
}

/// Polls the pending models. This system does not know if the loader is synchronous
/// (the model is ready in the first frame) or not (it is ready some frames later).
fn spawn_loaded_models(
    mut commands: Commands,
    pending: Query<(Entity, &Transform, &PendingModel)>,
    models: Res<Assets<Model>>,
) {
    for (entity, transform, PendingModel(handle)) in &pending {
        match models.state(*handle) {
            Some(LoadState::Loading) => continue,
            Some(LoadState::Ready) => {
                let model = models.get(*handle).expect("the state is Ready");
                for mesh in &model.meshes {
                    commands.spawn((*transform, MeshInstance(*mesh)));
                }
            }
            // The loader logged the reason. The room stays empty.
            Some(LoadState::Failed) | None => {}
        }
        commands.entity(entity).despawn();
    }
}

/// Copies each mesh instance into the render world. The renderer never reads the entities.
fn extract_instances(
    instances: Query<(&Transform, &MeshInstance)>,
    mut render_world: ResMut<RenderWorld>,
) {
    let mut extracted = render_world.0.resource_mut::<ExtractedInstances>();
    extracted.0.clear();
    extracted.0.extend(
        instances
            .iter()
            .map(|(transform, instance)| ExtractedInstance {
                mesh: instance.0,
                transform: transform.matrix(),
            }),
    );
}
