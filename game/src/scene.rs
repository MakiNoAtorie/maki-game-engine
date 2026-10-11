//! The scene: the room, the model, and the camera as entities, and the copy of the
//! mesh instances into the render world.

use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use glam::{Quat, Vec3};
use maki_assets::{Assets, Handle, LoadState, Material, Mesh, Model, load_gltf};
use maki_core::{App, Config, Extract, Module, Startup, Transform, Update};
use maki_render::{Camera, ExtractedInstance, ExtractedInstances, Light, RenderWorld};

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
        // No model in the config: a test scene for the materials. A cube and a sphere
        // of 1 m on the floor, a glass ball in front of them, and two mirrors.
        None => {
            commands.spawn((
                Transform::from_translation(Vec3::new(-1.0, 0.5, 0.0)),
                MeshInstance(meshes.add(Mesh::cube(1.0))),
            ));
            commands.spawn((
                Transform::from_translation(Vec3::new(1.0, 0.5, 0.0)),
                MeshInstance(meshes.add(Mesh::sphere(0.5, 32, 16))),
            ));
            // Many triangles: light that goes through shows the flat triangles more
            // than light that a diffuse surface sends back.
            let glass = Mesh::sphere(0.6, 96, 48)
                .with_material(Material::Glass { ior: 1.5 })
                .with_base_color([1.0, 1.0, 1.0, 1.0]);
            commands.spawn((
                Transform::from_translation(Vec3::new(1.6, 0.6, 1.9)),
                MeshInstance(meshes.add(glass)),
            ));
            // One mirror mesh: a cube of 1 m, made thin and wide by the scale of
            // each instance.
            let mirror = meshes.add(
                Mesh::cube(1.0)
                    .with_material(Material::Mirror)
                    .with_base_color([0.9, 0.9, 0.9, 1.0]),
            );
            // Behind the objects, with the top tilted back, so it faces the camera.
            commands.spawn((
                Transform {
                    translation: Vec3::new(0.0, 1.3, -2.2),
                    rotation: Quat::from_rotation_x(-0.25),
                    scale: Vec3::new(4.5, 2.6, 0.06),
                },
                MeshInstance(mirror),
            ));
            // At the left, turned to face the objects.
            commands.spawn((
                Transform {
                    translation: Vec3::new(-3.0, 1.1, 0.8),
                    rotation: Quat::from_rotation_y(1.1),
                    scale: Vec3::new(2.4, 2.2, 0.06),
                },
                MeshInstance(mirror),
            ));
        }
    }
}

/// Each point light of a model becomes a sphere light with the radius of the config.
/// Polls the pending models. This system does not know if the loader is synchronous
/// (the model is ready in the first frame) or not (it is ready some frames later).
fn spawn_loaded_models(
    mut commands: Commands,
    pending: Query<(Entity, &Transform, &PendingModel)>,
    models: Res<Assets<Model>>,
    config: Res<Config>,
) {
    for (entity, transform, PendingModel(handle)) in &pending {
        match models.state(*handle) {
            Some(LoadState::Loading) => continue,
            Some(LoadState::Ready) => {
                let model = models.get(*handle).expect("the state is Ready");
                for mesh in &model.meshes {
                    commands.spawn((*transform, MeshInstance(*mesh)));
                }
                for light in &model.lights {
                    commands.spawn(Light {
                        position: transform.matrix().transform_point3(light.position),
                        radius: config.light_radius,
                        color: light.color,
                        intensity: light.intensity,
                    });
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
    instances: Query<(Ref<Transform>, Ref<MeshInstance>)>,
    mut removed: RemovedComponents<MeshInstance>,
    mut render_world: ResMut<RenderWorld>,
) {
    // An instance is new, moved, got another mesh, or went away: the old samples of
    // the renderer show another scene.
    let changed = |(transform, instance): (Ref<Transform>, Ref<MeshInstance>)| {
        transform.is_changed() || instance.is_changed()
    };
    if removed.read().count() > 0 || instances.iter().any(changed) {
        render_world.reset_accumulation();
    }
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
