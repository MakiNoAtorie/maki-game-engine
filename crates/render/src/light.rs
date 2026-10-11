//! Lights: a component of the game world, the copies in the render world, and the
//! light buffer that the trace shader reads.

use std::f32::consts::PI;

use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::component::Component;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, ResMut};
use glam::Vec3;
use maki_core::{App, Extract, Startup};
use maki_gpu::{BufferDesc, BufferHandle, FRAMES_IN_FLIGHT};

use crate::{RenderGpu, RenderWorld, setup_renderer};

/// A sphere that sends out the same light in all directions.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// Center of the sphere, in world space.
    pub position: Vec3,
    /// Radius of the sphere, in meters. A larger light gives softer shadows.
    pub radius: f32,
    /// Linear RGB.
    pub color: Vec3,
    /// Brightness as seen from far away: a surface that faces the light at a distance
    /// of `d` meters gets `color * intensity / d²`. Does not change with `radius`.
    pub intensity: f32,
}

impl Light {
    /// The renderer uses this radius for a smaller light.
    pub const MIN_RADIUS: f32 = 0.001;

    /// The light that each point of the sphere surface sends out (radiance).
    /// From each side the sphere looks like a disk of area `pi * r²`, and this value
    /// times that area is `color * intensity`.
    pub fn radiance(&self) -> Vec3 {
        let radius = self.radius.max(Self::MIN_RADIUS);
        self.color * self.intensity / (PI * radius * radius)
    }
}

/// All lights of the frame. A resource of the render world.
#[derive(Resource, Debug, Clone, Default)]
pub struct ExtractedLights(pub Vec<Light>);

/// The light buffer has room for this many lights. More are not used by the renderer.
pub const MAX_LIGHTS: usize = 256;
/// Bytes of one light in the light buffer: `LightData` in `shaders/trace.comp`.
const LIGHT_BYTES: usize = 32;

/// The light buffers on the GPU: one for each frame slot, because the CPU writes the
/// lights of frame N+1 while the GPU can still read those of frame N. The trace pass
/// writes the buffer of its frame. A resource of the render world.
#[derive(Resource)]
pub(crate) struct LightBuffers(pub [BufferHandle; FRAMES_IN_FLIGHT]);

/// `LightData` of `shaders/trace.comp` for each light, in std430 layout.
pub(crate) fn light_bytes(lights: &[Light]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(lights.len() * LIGHT_BYTES);
    for light in lights.iter().take(MAX_LIGHTS) {
        let (p, r) = (light.position, light.radiance());
        // One number for the brightness: the shader picks a light for a shadow ray
        // with a chance in proportion to it.
        let power = light.intensity * light.color.max_element();
        let radius = light.radius.max(Light::MIN_RADIUS);
        let fields = [p.x, p.y, p.z, radius, r.x, r.y, r.z, power];
        bytes.extend(fields.into_iter().flat_map(f32::to_ne_bytes));
    }
    bytes
}

/// Adds the light systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Startup, setup_lights.after(setup_renderer))
        .add_systems(Extract, extract_lights);
}

fn setup_lights(mut gpu: ResMut<RenderGpu>, mut render_world: ResMut<RenderWorld>) {
    let buffers = [(); FRAMES_IN_FLIGHT].map(|()| {
        gpu.0
            .create_buffer(&BufferDesc {
                name: "lights",
                size: (MAX_LIGHTS * LIGHT_BYTES) as u64,
                cpu_writable: true,
            })
            .expect("light buffer")
    });
    render_world.0.insert_resource(LightBuffers(buffers));
}

fn extract_lights(
    lights: Query<Ref<Light>>,
    mut removed: RemovedComponents<Light>,
    mut render_world: ResMut<RenderWorld>,
) {
    // A light is new, was written, or went away: the old samples show other lighting,
    // but the same surfaces. The mean keeps a short history.
    if removed.read().count() > 0 || lights.iter().any(|light| light.is_changed()) {
        render_world.soften_accumulation();
    }
    let lights = lights.iter().map(|light| *light).collect();
    render_world.0.insert_resource(ExtractedLights(lights));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_ecs::system::{Commands, Res};
    use maki_core::{Config, FixedUpdate, Render, Startup, Time};

    use super::*;
    use crate::Accumulation;

    #[test]
    fn light_bytes_follow_the_shader_layout() {
        let light = Light {
            position: Vec3::new(1.0, 2.0, 3.0),
            radius: 0.5,
            color: Vec3::new(1.0, 0.5, 0.25),
            intensity: 8.0,
        };
        let bytes = light_bytes(&[light; MAX_LIGHTS + 1]);
        // More lights than the buffer holds are cut off.
        assert_eq!(bytes.len(), MAX_LIGHTS * LIGHT_BYTES);
        let field = |index: usize| f32::from_ne_bytes(bytes[index * 4..][..4].try_into().unwrap());
        assert_eq!(
            [field(0), field(1), field(2), field(3)],
            [1.0, 2.0, 3.0, 0.5]
        );
        assert_eq!(field(4), light.radiance().x);
        assert_eq!(field(6), light.radiance().z);
        assert_eq!(field(7), 8.0);
    }

    /// The sample count of each frame.
    #[derive(Resource, Default)]
    struct Samples(Vec<u32>);

    #[test]
    fn a_written_light_resets_the_accumulation_and_a_still_light_does_not() {
        // 50 Hz: one fixed step per 20 ms.
        let mut app = App::new(Config {
            fixed_hz: 50.0,
            ..Default::default()
        })
        .unwrap();
        app.init_resource::<RenderWorld>()
            .init_resource::<Samples>();
        // Not `build`: `setup_lights` needs a GPU.
        app.add_systems(Extract, extract_lights);
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Light {
                position: Vec3::ZERO,
                radius: 0.1,
                color: Vec3::ONE,
                intensity: 1.0,
            });
        })
        // The light moves in the fixed steps 3 and 4 only.
        .add_systems(
            FixedUpdate,
            |mut lights: Query<&mut Light>, time: Res<Time>| {
                if time.fixed_tick == 3 || time.fixed_tick == 4 {
                    lights.single_mut().unwrap().position.x += 1.0;
                }
            },
        )
        // Stands in for `render_frame`, which needs a GPU.
        .add_systems(
            Render,
            |mut render_world: ResMut<RenderWorld>, mut samples: ResMut<Samples>| {
                let mut accumulation = render_world.0.resource_mut::<Accumulation>();
                // No history: a light change starts the mean again.
                accumulation.start_frame(u32::MAX, 1, 0);
                samples.0.push(accumulation.samples());
            },
        );

        app.run(|mut app| {
            for _ in 0..6 {
                app.advance(Duration::from_millis(20));
            }
            // A frame with no fixed step: nothing was written.
            app.advance(Duration::from_millis(1));
            // Frame 1 has the new light. Frames 3 and 4 have the moved light.
            assert_eq!(app.world().resource::<Samples>().0, [1, 2, 1, 1, 2, 3, 4]);
        });
    }
}
