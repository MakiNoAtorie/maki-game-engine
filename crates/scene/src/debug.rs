//! Debug shapes: lines, boxes, and labels that code of the game puts in the picture.
//!
//! A script adds shapes in its step; the shapes stay until the next fixed step, so a
//! pause of the editor keeps them. A tool draws them over the picture
//! (`maki_ui::DebugShapesModule`): they are not in the path tracer.

use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use glam::Vec3;

#[derive(Debug, Clone, PartialEq)]
pub enum DebugShape {
    Line(Vec3, Vec3),
    /// A box along the axes: its center and its full size.
    Box(Vec3, Vec3),
    Label(Vec3, String),
}

/// The shapes of the last fixed step, each with its color (linear RGB, 0 to 1).
#[derive(Resource, Debug, Default)]
pub struct DebugShapes(pub Vec<(DebugShape, [f32; 3])>);

impl DebugShapes {
    pub fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 3]) {
        self.0.push((DebugShape::Line(a, b), color));
    }

    pub fn cube(&mut self, center: Vec3, size: Vec3, color: [f32; 3]) {
        self.0.push((DebugShape::Box(center, size), color));
    }

    pub fn label(&mut self, at: Vec3, text: impl Into<String>, color: [f32; 3]) {
        self.0.push((DebugShape::Label(at, text.into()), color));
    }
}

/// Runs first in each fixed step: the shapes of the step before go.
pub(crate) fn clear(mut shapes: ResMut<DebugShapes>) {
    if !shapes.0.is_empty() {
        shapes.0.clear();
    }
}
