//! Smooth motion between fixed steps.
//!
//! The simulation runs in fixed steps (`FixedUpdate`), the picture at the frame rate of
//! the display. With a `Transform` that changes only in a fixed step, a frame can show
//! 0 steps of motion and the next one 2: the motion stutters. An entity with
//! [`Interpolated`] keeps the pose of the last two steps, and its `Transform` is the
//! blend of the two for the time of the frame (`Time::alpha`). The picture is thus at
//! most one step behind the simulation.
//!
//! Rule for the simulation: read and write `Interpolated::current`, not `Transform`.
//!
//! A jump to another pose in the same room is a [`Interpolated::snap`]: it counts in
//! [`PoseJumps`], so the renderer drops the history of the pixels that the object left.
//! A room change needs no snap count: the renderer starts the picture again.

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res};

use crate::{Time, Transform};

/// The pose of an entity in the simulation. `App` adds the two systems that use it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[require(Transform)]
pub struct Interpolated {
    /// The pose at the end of the fixed step before the last one.
    pub previous: Transform,
    /// The pose at the end of the last fixed step. `FixedUpdate` systems write it.
    pub current: Transform,
}

impl Interpolated {
    /// At rest at a pose.
    pub fn at(pose: Transform) -> Self {
        Self {
            previous: pose,
            current: pose,
        }
    }

    /// Moves to a pose with no motion between: for a jump to another place. A pose that
    /// changes counts one jump; a snap to the pose it has does not. With no count (a room
    /// change follows, or no mesh moves): `*pose = Interpolated::at(to)`.
    pub fn snap(&mut self, pose: Transform, jumps: &mut PoseJumps) {
        let snapped = Self::at(pose);
        if *self != snapped {
            *self = snapped;
            jumps.0 += 1;
        }
    }
}

/// The number of pose jumps (snaps) since the start. `App` adds it. The renderer sees a
/// new value and drops the history of the pixels that a jumping object left.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PoseJumps(pub u64);

/// Runs in `FixedFirst`: the pose of the step before becomes the previous pose.
pub(crate) fn shift_poses(mut poses: Query<&mut Interpolated>) {
    for mut pose in &mut poses {
        // Only a different value is written, so an entity at rest is not marked as changed.
        if pose.previous != pose.current {
            pose.previous = pose.current;
        }
    }
}

/// Runs in `Update`: writes the blend of the two poses into each `Transform`. A system
/// of `Update` that reads a `Transform` runs `.after(interpolate_transforms)`.
pub fn interpolate_transforms(time: Res<Time>, mut poses: Query<(&Interpolated, &mut Transform)>) {
    for (pose, mut transform) in &mut poses {
        // An entity at rest gets the same value in each frame: its `Transform` is not
        // marked as changed, and the renderer keeps its picture.
        transform.set_if_neq(pose.previous.blend(&pose.current, time.alpha));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_ecs::change_detection::DetectChanges;
    use bevy_ecs::system::Commands;
    use glam::Vec3;

    use super::*;
    use crate::{App, ConfigFile, FixedUpdate, Startup};

    #[test]
    fn the_transform_moves_in_each_frame_between_two_fixed_steps() {
        // 50 Hz: one fixed step per 20 ms. The entity moves 1 m in each step.
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Interpolated::at(Transform::IDENTITY));
        })
        .add_systems(FixedUpdate, |mut poses: Query<&mut Interpolated>| {
            poses.single_mut().unwrap().current.translation.x += 1.0;
        });
        app.run(|mut app| {
            let mut frame = |ms| {
                app.advance(Duration::from_millis(ms));
                let mut transforms = app.world().try_query::<&Transform>().unwrap();
                transforms.single(app.world()).unwrap().translation.x
            };
            // One step: the picture is at the start of that step.
            assert_eq!(frame(20), 0.0);
            // No step in these two frames, and the picture still moves.
            assert_eq!(frame(5), 0.25);
            assert_eq!(frame(5), 0.5);
            // Two steps and no time left: at the start of the last step.
            assert_eq!(frame(30), 2.0);
        });
    }

    /// For each frame: true if the `Transform` was written in it.
    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Written(Vec<bool>);

    #[test]
    fn an_entity_at_rest_is_not_marked_as_changed() {
        use bevy_ecs::schedule::IntoScheduleConfigs;
        use bevy_ecs::system::ResMut;
        use bevy_ecs::world::Ref;

        let mut app = App::new(ConfigFile::default()).unwrap();
        app.init_resource::<Written>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Interpolated::at(Transform::from_translation(Vec3::new(
                    0.1, 2.7, -3.3,
                ))));
            })
            .add_systems(
                crate::Update,
                (|transforms: Query<Ref<Transform>>, mut written: ResMut<Written>| {
                    written.0.push(transforms.single().unwrap().is_changed());
                })
                .after(interpolate_transforms),
            );
        app.run(|mut app| {
            // Frames of many lengths: the blend factor differs from frame to frame.
            for ms in [40, 40, 7, 13, 3, 21, 16, 9, 5] {
                app.advance(Duration::from_millis(ms));
            }
            let written = &app.world().resource::<Written>().0;
            assert!(written[0] && !written[1..].contains(&true), "{written:?}");
        });
    }

    #[test]
    fn a_snap_that_changes_the_pose_counts_one_jump() {
        let mut jumps = PoseJumps::default();
        let mut pose = Interpolated::at(Transform::IDENTITY);
        pose.snap(Transform::IDENTITY, &mut jumps);
        assert_eq!(jumps, PoseJumps(0));
        let there = Transform::from_translation(Vec3::X);
        pose.snap(there, &mut jumps);
        assert_eq!((pose, jumps), (Interpolated::at(there), PoseJumps(1)));
        pose.snap(there, &mut jumps);
        assert_eq!(jumps, PoseJumps(1));
        // In motion toward the pose: the previous pose differs, so it is a jump.
        pose.current = Transform::IDENTITY;
        pose.snap(Transform::IDENTITY, &mut jumps);
        assert_eq!(jumps, PoseJumps(2));
    }
}
