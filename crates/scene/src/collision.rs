//! Collision boxes: the walls and the props that do not move. The physics makes a fixed
//! collider of each one.

use bevy_ecs::component::Component;
use glam::{Mat4, Vec3};
use maki_assets::Doorway;

/// A box with sides along the axes, in world space, that blocks.
/// Walls and props that do not move get one. It is not tied to the `Transform` of the
/// entity: the code that spawns the entity computes the box.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Collider {
    pub min: Vec3,
    pub max: Vec3,
}

impl Collider {
    /// The box between two opposite corners, in any order.
    pub fn new(a: Vec3, b: Vec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// The box around another box (`min` to `max` in its own space) that `matrix`
    /// puts into the world. Larger than the content if `matrix` has a rotation.
    pub fn around(matrix: Mat4, min: Vec3, max: Vec3) -> Self {
        let corner = |index: u32| {
            let pick = |bit: u32, low: f32, high: f32| if index & bit == 0 { low } else { high };
            matrix.transform_point3(Vec3::new(
                pick(1, min.x, max.x),
                pick(2, min.y, max.y),
                pick(4, min.z, max.z),
            ))
        };
        (1..8).fold(Self::new(corner(0), corner(0)), |all, index| Self {
            min: all.min.min(corner(index)),
            max: all.max.max(corner(index)),
        })
    }
}

/// The boxes that keep the player in a room that `Mesh::room_with_doorways` made with
/// the same arguments: one box behind each piece of wall, and one at the end of each
/// passage.
pub fn room_colliders(width: f32, depth: f32, height: f32, doorways: &[Doorway]) -> Vec<Collider> {
    use maki_assets::Wall;

    // The boxes go this far out of the room. Not less than a passage is long, so the
    // boxes of the wall pieces are also the sides of the passages.
    let thickness = doorways.iter().map(|door| door.depth).fold(1.0, f32::max);
    let mut colliders = Vec::new();
    for wall in [Wall::NegX, Wall::PosX, Wall::NegZ, Wall::PosZ] {
        let (start, along, length, inward) = wall.frame(width, depth);
        let out = -inward;
        let top = Vec3::Y * height;
        let mut piece = |from: f32, to: f32, near: f32, far: f32| {
            if to > from {
                let a = start + along * from + out * near;
                colliders.push(Collider::new(a, start + along * to + out * far + top));
            }
        };
        let mut doors: Vec<_> = doorways.iter().filter(|door| door.wall == wall).collect();
        doors.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        let mut built = 0.0;
        for door in doors {
            let (from, to) = door.span(length);
            // The mesh generator leaves out a doorway that does not fit; so does this.
            if !(from >= built && to <= length && door.width > 0.0 && door.depth > 0.0) {
                continue;
            }
            piece(built, from, 0.0, thickness);
            piece(from, to, door.depth, door.depth + thickness);
            built = to;
        }
        piece(built, length, 0.0, thickness);
    }
    colliders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_around_a_turned_box_holds_all_its_corners() {
        // A box of 2 x 1 x 1, turned a quarter around Y: 1 x 1 x 2.
        let matrix = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let around = Collider::around(matrix, Vec3::new(-1.0, 0.0, -0.5), Vec3::new(1.0, 1.0, 0.5));
        assert!(around.min.distance(Vec3::new(-0.5, 0.0, -1.0)) < 1e-6);
        assert!(around.max.distance(Vec3::new(0.5, 1.0, 1.0)) < 1e-6);
    }
}
