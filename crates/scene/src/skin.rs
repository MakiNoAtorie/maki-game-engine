//! The pose of the joints of a skinned model.
//!
//! The object entity of a model with a skeleton has a [`SkinPose`]: the local
//! transform of each joint at the last two fixed steps, as `Interpolated` has for an
//! object. The animator writes `current`. In `Update`, [`make_palettes`] blends the two
//! poses by `Time::alpha` into the [`SkinPalette`] that the renderer reads.

use std::sync::Arc;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res};
use glam::Mat4;
use maki_assets::Skeleton;
use maki_core::{App, FixedFirst, ResetPlay, Time, Transform, Update, interpolate_transforms};

/// The local joint transforms (relative to the parent joint) of a skinned model.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(SkinPalette)]
pub struct SkinPose {
    pub skeleton: Arc<Skeleton>,
    /// One for each joint: the pose at the end of the fixed step before the last one.
    pub previous: Vec<Transform>,
    /// One for each joint: the pose at the end of the last fixed step.
    pub current: Vec<Transform>,
}

impl SkinPose {
    /// At rest: each joint at its rest transform.
    pub fn rest(skeleton: Arc<Skeleton>) -> Self {
        let rest: Vec<Transform> = skeleton.joints.iter().map(|joint| joint.rest).collect();
        Self {
            skeleton,
            previous: rest.clone(),
            current: rest,
        }
    }
}

/// One matrix for each joint: its model-space pose times its inverse bind matrix (the
/// `palette` of `maki_assets::skin::skin_vertex`). On the entity of the `MeshInstance`
/// of the model; written only when it differs, so a pose at rest makes no skin work.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct SkinPalette(pub Vec<Mat4>);

/// Adds the systems of the joint poses.
pub(crate) fn add(app: &mut App) {
    app.add_systems(FixedFirst, shift_skins)
        .add_systems(Update, make_palettes.after(interpolate_transforms))
        .add_systems(ResetPlay, reset_skins);
}

/// Runs in `FixedFirst`: the pose of the step before becomes the previous pose.
fn shift_skins(mut poses: Query<&mut SkinPose>) {
    for mut pose in &mut poses {
        // Only a different value is written: a pose at rest is not marked as changed.
        if pose.previous != pose.current {
            let pose = &mut *pose;
            pose.previous.clone_from(&pose.current);
        }
    }
}

/// Runs in `Update`: the palette of the blend of the two poses.
pub fn make_palettes(time: Res<Time>, mut skins: Query<(&SkinPose, &mut SkinPalette)>) {
    for (pose, mut palette) in &mut skins {
        palette.set_if_neq(SkinPalette(palette_of(pose, time.alpha)));
    }
}

/// The palette of the pose at the part `t` of the way from `previous` to `current`.
fn palette_of(pose: &SkinPose, t: f32) -> Vec<Mat4> {
    let skeleton = &pose.skeleton;
    let mut model: Vec<Mat4> = Vec::with_capacity(skeleton.joints.len());
    for (index, joint) in skeleton.joints.iter().enumerate() {
        let local = pose.previous[index].blend(&pose.current[index], t).matrix();
        // Parent-first: the parent has its model-space pose already.
        model.push(match joint.parent {
            Some(parent) => model[parent] * local,
            None => local,
        });
    }
    model
        .iter()
        .zip(&skeleton.inverse_binds)
        .map(|(pose, inverse_bind)| *pose * *inverse_bind)
        .collect()
}

/// Runs in `ResetPlay`: each joint goes back to its rest.
fn reset_skins(mut poses: Query<&mut SkinPose>) {
    for mut pose in &mut poses {
        let rest = SkinPose::rest(pose.skeleton.clone());
        pose.set_if_neq(rest);
    }
}
