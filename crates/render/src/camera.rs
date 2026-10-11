//! The camera: a component of the game world, and its copy in the render world.

use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, ResMut};
use glam::Vec3;
use maki_core::{App, Extract};

use crate::RenderWorld;

/// The point of view of the picture. The game spawns one entity with this component.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the camera is, in world space.
    pub position: Vec3,
    /// The point in world space that is in the center of the picture.
    pub target: Vec3,
    /// The angle between the top and the bottom edge of the picture, in degrees.
    pub fov_y_degrees: f32,
}

/// At the origin, looking along -Z. In use only while the game has no camera.
impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            target: Vec3::NEG_Z,
            fov_y_degrees: 60.0,
        }
    }
}

impl Camera {
    /// The vectors `[right, up, forward]` from which a shader makes the ray of a pixel:
    /// `direction = forward + x * right + y * up`, with `x` and `y` from -1 to 1 across
    /// the picture. `forward` has length 1. The lengths of `right` and `up` hold the
    /// field of view. `aspect` is the width of the picture divided by its height.
    pub fn ray_basis(&self, aspect: f32) -> [Vec3; 3] {
        let forward = (self.target - self.position)
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
        let half_height = (self.fov_y_degrees.to_radians() / 2.0).tan();
        [right * half_height * aspect, up * half_height, forward]
    }
}

/// The camera of the frame. A resource of the render world.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ExtractedCamera(pub Camera);

/// Adds the camera systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Extract, extract_camera);
}

// ponytail: the first camera wins. Add an "active" marker when the game has more than one.
fn extract_camera(cameras: Query<&Camera>, mut render_world: ResMut<RenderWorld>) {
    let camera = cameras.iter().next().copied().unwrap_or_default();
    render_world.0.insert_resource(ExtractedCamera(camera));
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let [right, up, forward] = camera.ray_basis(2.0);
            let to_target = (camera.target - camera.position).normalize();
            assert!((forward - to_target).length() < 1e-5);
            assert!(right.dot(up).abs() < 1e-5 && right.dot(forward).abs() < 1e-5);
            assert!(up.dot(forward).abs() < 1e-5);
            // tan(45 degrees) = 1, and the picture is two times as wide as it is high.
            assert!((up.length() - 1.0).abs() < 1e-5 && (right.length() - 2.0).abs() < 1e-5);
        }
        // The top of the picture is up in the world.
        assert!(angled.ray_basis(1.0)[1].y > 0.0);
    }
}
