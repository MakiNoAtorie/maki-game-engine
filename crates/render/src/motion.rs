//! Screen motion: where the surface point of each pixel was on the screen one frame
//! before. A temporal filter reads the picture of the frame before at that place.
//!
//! The trace pass writes the `motion` image. For the surface that the camera ray of a
//! pixel hits, it computes where that point was (the transform of its instance in the
//! frame before) and where the camera of the frame before saw it. The difference of
//! the two screen positions, in pixels, is the motion of the pixel.
//!
//! The two inputs come from the CPU in buffers, one buffer for each frame slot (as the
//! light buffers): the CPU writes the data of frame N+1 while the GPU can still read
//! those of frame N.
//! - the camera buffer: the camera of the frame before. `Trace` writes it.
//! - the instance buffer: the transform of the frame before for each TLAS instance, in
//!   TLAS order (a ray hit gives that index). `BuildTlas` writes it.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use glam::{Mat4, Vec3};
use maki_core::{App, Startup};
use maki_gpu::{BufferDesc, BufferHandle, FRAMES_IN_FLIGHT};

use crate::{MAX_INSTANCES, RenderGpu, RenderWorld, setup_renderer};

/// Bytes of the camera buffer: `PreviousCamera` in `shaders/trace.comp`.
const CAMERA_BYTES: usize = 64;
/// Bytes of one instance in the instance buffer: three rows of a matrix.
const INSTANCE_BYTES: usize = 48;

/// A resource of the render world.
#[derive(Resource)]
pub(crate) struct MotionBuffers {
    pub camera: [BufferHandle; FRAMES_IN_FLIGHT],
    pub instances: [BufferHandle; FRAMES_IN_FLIGHT],
}

/// Adds the motion buffers. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Startup, setup_motion.after(setup_renderer));
}

fn setup_motion(mut gpu: ResMut<RenderGpu>, mut render_world: ResMut<RenderWorld>) {
    let mut buffers = |name, size: usize| {
        [(); FRAMES_IN_FLIGHT].map(|()| {
            gpu.0
                .create_buffer(&BufferDesc {
                    name,
                    size: size as u64,
                    cpu_writable: true,
                })
                .expect("motion buffer")
        })
    };
    let buffers = MotionBuffers {
        camera: buffers("previous camera", CAMERA_BYTES),
        instances: buffers(
            "previous transforms",
            MAX_INSTANCES as usize * INSTANCE_BYTES,
        ),
    };
    render_world.0.insert_resource(buffers);
}

/// A camera in the form that makes the projection of a point two dot products and a
/// division. `basis` is `[right, up, forward]` of `ray_basis`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Projection {
    origin: Vec3,
    /// `right` divided by its length squared, and the same for `up`.
    right: Vec3,
    up: Vec3,
    forward: Vec3,
}

impl Projection {
    pub fn new(origin: Vec3, [right, up, forward]: [Vec3; 3]) -> Self {
        Self {
            origin,
            right: right / right.length_squared(),
            up: up / up.length_squared(),
            forward,
        }
    }

    /// Where the camera sees a world point, in pixels of a picture of `size`. (0, 0) is
    /// the top left corner. `None` for a point that is not in front of the camera.
    /// The shader has the same steps (`previous_pixel` in `shaders/trace.comp`): this
    /// copy is for the unit test of the math.
    #[cfg(test)]
    pub fn pixel(&self, point: Vec3, size: glam::Vec2) -> Option<glam::Vec2> {
        let offset = point - self.origin;
        let depth = offset.dot(self.forward);
        // A ray is `forward + x * right + y * up`: x and y of the ray through the point.
        let (x, y) = (offset.dot(self.right), offset.dot(self.up));
        // Picture y goes down, `up` goes up.
        (depth > 0.0).then(|| (glam::Vec2::new(x, -y) / depth * 0.5 + 0.5) * size)
    }

    /// `PreviousCamera` of `shaders/trace.comp`: four vec3, each with 4 bytes of padding.
    pub fn bytes(&self) -> Vec<u8> {
        [self.origin, self.right, self.up, self.forward]
            .iter()
            .flat_map(|v| [v.x, v.y, v.z, 0.0])
            .flat_map(f32::to_ne_bytes)
            .collect()
    }
}

/// The first three rows of a transform matrix, row by row: the form of a TLAS instance
/// and of the instance buffer.
pub(crate) fn matrix_rows(matrix: Mat4) -> [f32; 12] {
    let rows = matrix.transpose().to_cols_array();
    rows[..12].try_into().expect("12 of 16 values")
}

/// The content of the instance buffer.
pub(crate) fn instance_bytes(previous: &[[f32; 12]]) -> Vec<u8> {
    let values = previous.iter().flatten();
    values.flat_map(|value| value.to_ne_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec2;
    use maki_scene::Camera;

    use super::*;
    use crate::ray_basis;

    #[test]
    fn a_point_moves_on_the_screen_when_the_camera_moves() {
        let size = Vec2::new(200.0, 100.0);
        // A vertical field of view of 90 degrees: at a distance of 5 m the picture
        // shows 10 m from top to bottom, so 1 m is 10 pixels.
        let looking_down_z = |x: f32| Camera {
            position: Vec3::new(x, 0.0, 5.0),
            target: Vec3::new(x, 0.0, 0.0),
            fov_y_degrees: 90.0,
        };
        let projection = |camera: Camera| Projection::new(camera.position, ray_basis(&camera, 2.0));
        let (previous, current) = (
            projection(looking_down_z(0.0)),
            projection(looking_down_z(1.0)),
        );

        // The point in the center of the previous picture.
        assert_eq!(
            previous.pixel(Vec3::ZERO, size),
            Some(Vec2::new(100.0, 50.0))
        );
        // One meter up and two to the right: up is toward smaller y.
        let point = Vec3::new(2.0, 1.0, 0.0);
        let before = previous.pixel(point, size).unwrap();
        assert!(before.distance(Vec2::new(120.0, 40.0)) < 1e-4, "{before}");
        // The camera moved 1 m to the right: the point moved 10 pixels to the left.
        let motion = current.pixel(point, size).unwrap() - before;
        assert!(motion.distance(Vec2::new(-10.0, 0.0)) < 1e-4, "{motion}");
        // A point behind the camera is not on the screen.
        assert_eq!(previous.pixel(Vec3::new(0.0, 0.0, 6.0), size), None);
    }

    #[test]
    fn buffer_bytes_follow_the_shader_layouts() {
        let camera = Projection::new(
            Vec3::new(1.0, 2.0, 3.0),
            [Vec3::X * 2.0, Vec3::Y, Vec3::NEG_Z],
        );
        let bytes = camera.bytes();
        assert_eq!(bytes.len(), CAMERA_BYTES);
        let field = |index: usize| f32::from_ne_bytes(bytes[index * 4..][..4].try_into().unwrap());
        assert_eq!([field(0), field(1), field(2)], [1.0, 2.0, 3.0]);
        // `right` of length 2, divided by 4.
        assert_eq!(field(4), 0.5);
        assert_eq!(field(9), 1.0);
        assert_eq!(field(14), -1.0);

        // The translation is the fourth value of each row.
        let rows = matrix_rows(Mat4::from_translation(Vec3::new(7.0, 8.0, 9.0)));
        assert_eq!([rows[3], rows[7], rows[11]], [7.0, 8.0, 9.0]);
        assert_eq!(instance_bytes(&[rows, rows]).len(), 2 * INSTANCE_BYTES);
    }
}
