//! Collision: boxes that block, and a circle that moves between them. No physics engine.

use bevy_ecs::component::Component;
use glam::{Mat4, Vec3};
use maki_assets::Doorway;

/// A box with sides along the axes, in world space, that the player cannot enter.
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

/// Moves a circle on the floor plane (X and Z) by `step` and keeps it out of the boxes.
/// Returns the new center. Y does not change and is not tested: each box blocks at
/// each height.
///
/// The circle makes the full step, and each box that it then overlaps pushes it out
/// by the shortest way: away from the nearest point of the box. Against a wall that is
/// straight back, so a move at an angle loses only the part that goes into the wall,
/// and the circle slides along it. At a corner the push is to the side: the circle
/// slides around the corner.
// ponytail: a step that is longer than the radius can jump over a thin box. The
// fastest step of the player is 0.06 m. Split the step if something moves faster.
pub fn move_circle(position: Vec3, step: Vec3, radius: f32, colliders: &[Collider]) -> Vec3 {
    let (mut x, mut z) = (position.x + step.x, position.z + step.z);
    // Two rounds: the push of one box can move the circle into a box before it.
    for _ in 0..2 {
        for collider in colliders {
            let (min, max) = (collider.min, collider.max);
            // The point of the box that is nearest to the center.
            let (near_x, near_z) = (x.clamp(min.x, max.x), z.clamp(min.z, max.z));
            let (away_x, away_z) = (x - near_x, z - near_z);
            let distance = away_x.hypot(away_z);
            if distance >= radius {
                continue;
            }
            if distance > 0.0 {
                x = near_x + away_x / distance * radius;
                z = near_z + away_z / distance * radius;
                continue;
            }
            // The center is in the box: out through the nearest face.
            let faces = [x - min.x, max.x - x, z - min.z, max.z - z];
            let nearest = (0..4).min_by(|a, b| faces[*a].total_cmp(&faces[*b]));
            match nearest.expect("four faces") {
                0 => x = min.x - radius,
                1 => x = max.x + radius,
                2 => z = min.z - radius,
                _ => z = max.z + radius,
            }
        }
    }
    Vec3::new(x, position.y, z)
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
    use maki_assets::Wall;

    use super::*;

    /// A box from x = 2 to 4 and z = -1 to 1.
    const BOX: Collider = Collider {
        min: Vec3::new(2.0, 0.0, -1.0),
        max: Vec3::new(4.0, 2.0, 1.0),
    };

    #[test]
    fn a_move_into_a_box_stops_at_its_face_and_a_move_along_it_does_not_stop() {
        let start = Vec3::new(0.0, 0.0, 0.0);
        // Into the box: the edge of the circle stops at the face x = 2.
        let stopped = move_circle(start, Vec3::X * 2.2, 0.5, &[BOX]);
        assert_eq!(stopped, Vec3::new(1.5, 0.0, 0.0));
        // Along the face, in contact with it: no stop.
        let along = move_circle(stopped, Vec3::Z * 0.7, 0.5, &[BOX]);
        assert_eq!(along, Vec3::new(1.5, 0.0, 0.7));
        // At an angle into the face: the part along the face stays.
        let slide = move_circle(stopped, Vec3::new(1.0, 0.0, 0.25), 0.5, &[BOX]);
        assert_eq!(slide, Vec3::new(1.5, 0.0, 0.25));
        // Away from the box, and with no box: free.
        assert_eq!(move_circle(stopped, Vec3::NEG_X, 0.5, &[BOX]).x, 0.5);
        assert_eq!(move_circle(start, Vec3::X * 3.0, 0.5, &[]).x, 3.0);
        // A move with the edge of the circle in the box, short of its center.
        assert_eq!(move_circle(start, Vec3::X * 1.8, 0.5, &[BOX]).x, 1.5);
    }

    #[test]
    fn a_circle_slides_around_a_corner() {
        // The center arrives 0.4 beside the corner (2, -1): the corner pushes it to
        // the side, to the distance of the radius.
        let start = Vec3::new(0.0, 0.0, -1.4);
        let moved = move_circle(start, Vec3::X * 2.0, 0.5, &[BOX]);
        assert!(moved.distance(Vec3::new(2.0, 0.0, -1.5)) < 1e-6, "{moved}");
        // At 0.5 or more beside the box, the circle passes.
        let free = move_circle(Vec3::new(0.0, 0.0, -1.5), Vec3::X * 2.0, 0.5, &[BOX]);
        assert_eq!(free.x, 2.0);
    }

    #[test]
    fn the_boxes_of_a_room_keep_a_circle_in_the_room_and_in_its_passage() {
        let door = Doorway {
            wall: Wall::NegZ,
            offset: 3.0,
            width: 1.0,
            height: 2.0,
            depth: 1.5,
        };
        let colliders = room_colliders(8.0, 8.0, 3.0, &[door]);
        let walk = |from: Vec3, step: Vec3| {
            // Many small steps, as the game does.
            (0..400).fold(from, |at, _| move_circle(at, step / 400.0, 0.3, &colliders))
        };
        // Each wall stops the circle at its radius.
        assert!((walk(Vec3::ZERO, Vec3::X * 9.0).x - 3.7).abs() < 1e-4);
        assert!((walk(Vec3::ZERO, Vec3::NEG_X * 9.0).x + 3.7).abs() < 1e-4);
        assert!((walk(Vec3::ZERO, Vec3::Z * 9.0).z - 3.7).abs() < 1e-4);
        assert!((walk(Vec3::ZERO, Vec3::NEG_Z * 9.0).z + 3.7).abs() < 1e-4);
        // Through the doorway: to the end of the passage, and not out of its sides.
        let in_passage = walk(Vec3::new(3.0, 0.0, 0.0), Vec3::NEG_Z * 9.0);
        assert!((in_passage.z + 5.2).abs() < 1e-4, "{in_passage}");
        assert!((walk(in_passage, Vec3::X * 9.0).x - 3.2).abs() < 1e-4);
        assert!((walk(in_passage, Vec3::NEG_X * 9.0).x - 2.8).abs() < 1e-4);
    }

    #[test]
    fn the_box_around_a_turned_box_holds_all_its_corners() {
        // A box of 2 x 1 x 1, turned a quarter around Y: 1 x 1 x 2.
        let matrix = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let around = Collider::around(matrix, Vec3::new(-1.0, 0.0, -0.5), Vec3::new(1.0, 1.0, 0.5));
        assert!(around.min.distance(Vec3::new(-0.5, 0.0, -1.0)) < 1e-6);
        assert!(around.max.distance(Vec3::new(0.5, 1.0, 1.0)) < 1e-6);
    }
}
