//! The camera: the copy of the `Camera` component in the render world, and its rays.

use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, ResMut};
use glam::Vec3;
use maki_core::{App, Extract};
use maki_scene::Camera;

use crate::RenderWorld;

/// The vectors `[right, up, forward]` from which a shader makes the ray of a pixel:
/// `direction = forward + x * right + y * up`, with `x` and `y` from -1 to 1 across
/// the picture. `forward` has length 1. The lengths of `right` and `up` hold the
/// field of view. `aspect` is the width of the picture divided by its height.
pub fn ray_basis(camera: &Camera, aspect: f32) -> [Vec3; 3] {
    let forward = (camera.target - camera.position)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z);
    // A camera that looks straight up or down gets no "right" from the Y axis.
    let up_hint = if forward.cross(Vec3::Y).length_squared() < 1e-6 {
        Vec3::NEG_Z
    } else {
        Vec3::Y
    };
    let right = forward.cross(up_hint).normalize();
    let up = right.cross(forward);
    // Half of the picture height, at a distance of 1 in front of the camera.
    let half_height = (camera.fov_y_degrees.to_radians() / 2.0).tan();
    [right * half_height * aspect, up * half_height, forward]
}

/// The camera of the frame. A resource of the render world.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ExtractedCamera {
    pub current: Camera,
    /// The camera of the frame before. Equal to `current` in the first frame. A pass
    /// computes where a point was on the screen with it.
    pub previous: Camera,
}

/// Adds the camera systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Extract, extract_camera);
}

// ponytail: the first camera wins. Add an "active" marker when the game has more than one.
fn extract_camera(
    cameras: Query<Ref<Camera>>,
    mut removed: RemovedComponents<Camera>,
    mut render_world: ResMut<RenderWorld>,
) {
    // A camera is new, was written, or went away: the picture needs new samples. The
    // old ones stay: the denoiser finds them at their new place in the picture.
    if removed.read().count() > 0 || cameras.iter().any(|camera| camera.is_changed()) {
        tracing::trace!("the camera changed");
        render_world.wake_accumulation();
    }
    let current = cameras.iter().next().map_or_else(Camera::default, |c| *c);
    // The resource still holds the camera of the frame before.
    let previous = render_world
        .0
        .get_resource::<ExtractedCamera>()
        .map_or(current, |camera| camera.current);
    render_world
        .0
        .insert_resource(ExtractedCamera { current, previous });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extracted_camera_carries_the_camera_of_the_frame_before() {
        use bevy_ecs::schedule::Schedule;
        use bevy_ecs::world::World;

        let mut world = World::new();
        world.insert_resource(RenderWorld::default());
        let at = |x| Camera {
            position: Vec3::new(x, 0.0, 0.0),
            ..Camera::default()
        };
        let entity = world.spawn(at(1.0)).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(extract_camera);
        let mut extract = |world: &mut World| {
            schedule.run(world);
            *world
                .resource::<RenderWorld>()
                .0
                .resource::<ExtractedCamera>()
        };

        let first = extract(&mut world);
        assert_eq!((first.current, first.previous), (at(1.0), at(1.0)));
        *world.get_mut::<Camera>(entity).unwrap() = at(2.0);
        let moved = extract(&mut world);
        assert_eq!((moved.current, moved.previous), (at(2.0), at(1.0)));
        let rest = extract(&mut world);
        assert_eq!((rest.current, rest.previous), (at(2.0), at(2.0)));
    }

    #[test]
    fn ray_basis_is_square_and_holds_the_field_of_view() {
        let angled = Camera {
            position: Vec3::new(0.0, 8.0, 7.0),
            target: Vec3::ZERO,
            fov_y_degrees: 90.0,
        };
        let down = Camera {
            position: Vec3::ZERO,
            target: Vec3::new(0.0, -1.0, 0.0),
            fov_y_degrees: 90.0,
        };
        for camera in [angled, down] {
            let [right, up, forward] = ray_basis(&camera, 2.0);
            let to_target = (camera.target - camera.position).normalize();
            assert!((forward - to_target).length() < 1e-5);
            assert!(right.dot(up).abs() < 1e-5 && right.dot(forward).abs() < 1e-5);
            assert!(up.dot(forward).abs() < 1e-5);
            // tan(45 degrees) = 1, and the picture is two times as wide as it is high.
            assert!((up.length() - 1.0).abs() < 1e-5 && (right.length() - 2.0).abs() < 1e-5);
        }
        // The top of the picture is up in the world.
        assert!(ray_basis(&angled, 1.0)[1].y > 0.0);
    }
}
