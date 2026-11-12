//! The skeleton of a skinned model, and the skin of one vertex on the CPU.

use glam::{Mat3, Mat4, Vec3};
use maki_core::Transform;

/// The joints of a skinned model file and their inverse bind matrices.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    /// Parent-first: the parent of a joint has a lower index.
    pub joints: Vec<Joint>,
    /// One for each joint: from the space of the model to the space of the joint in
    /// the bind pose.
    pub inverse_binds: Vec<Mat4>,
}

/// A joint of a [`Skeleton`].
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// The glTF node name; `joint<node index>` for a node with no name.
    pub name: String,
    /// `None`: a root joint, whose `rest` is in the space of the model.
    pub parent: Option<usize>,
    /// The local transform at rest, relative to the parent joint.
    pub rest: Transform,
}

/// The place and the normal of a vertex, bent by up to four joints. `palette` has a
/// matrix for each joint: its model-space pose times its inverse bind matrix. The
/// weights are normalized (`Mesh::with_skin`). The reference of the skin shader.
/// Panics on a joint with a weight that is past the palette.
pub fn skin_vertex(
    position: Vec3,
    normal: Vec3,
    joints: [u32; 4],
    weights: [f32; 4],
    palette: &[Mat4],
) -> (Vec3, Vec3) {
    let blend = joints
        .iter()
        .zip(weights)
        .filter(|(_, weight)| *weight > 0.0)
        .fold(Mat4::ZERO, |sum, (joint, weight)| {
            sum + palette[*joint as usize] * weight
        });
    // No inverse transpose for the normal: a joint has no non-uniform scale (a warning at load).
    let normal = (Mat3::from_mat4(blend) * normal).normalize_or_zero();
    (blend.transform_point3(position), normal)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-5
    }

    #[test]
    fn one_joint_moves_the_vertex_by_its_matrix() {
        let palette = [Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))];
        let (p, n) = skin_vertex(Vec3::X, Vec3::Y, [0; 4], [1.0, 0.0, 0.0, 0.0], &palette);
        assert!(near(p, Vec3::new(2.0, 2.0, 3.0)));
        assert!(near(n, Vec3::Y));
    }

    #[test]
    fn two_joints_blend_by_their_weights() {
        let palette = [
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        ];
        let (p, _) = skin_vertex(
            Vec3::Z,
            Vec3::Y,
            [0, 1, 0, 0],
            [0.5, 0.5, 0.0, 0.0],
            &palette,
        );
        assert!(near(p, Vec3::new(1.0, 0.0, 1.0)));
    }

    #[test]
    fn a_joint_turned_90_degrees_turns_the_vertex_around_its_place() {
        // A joint at (0, 1, 0), turned 90 degrees around Z; the vertex is 1 m along +X from it.
        let place = Vec3::Y;
        let inverse_bind = Mat4::from_translation(place).inverse();
        let pose = Mat4::from_translation(place)
            * Mat4::from_quat(Quat::from_rotation_z(90f32.to_radians()));
        let (p, n) = skin_vertex(
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Y,
            [0; 4],
            [1.0, 0.0, 0.0, 0.0],
            &[pose * inverse_bind],
        );
        assert!(near(p, Vec3::new(0.0, 2.0, 0.0)), "{p}");
        assert!(near(n, -Vec3::X), "{n}");
    }
}
