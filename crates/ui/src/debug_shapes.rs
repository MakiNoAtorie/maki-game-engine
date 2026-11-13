//! Draws the debug shapes of the game (`maki_scene::DebugShapes`) over the picture,
//! from the active camera.

use bevy_ecs::query::With;
use bevy_ecs::world::World;
use glam::Vec3;
use maki_core::{App, Module};
use maki_render::{project_point, project_segment, ray_basis};
use maki_scene::{ActiveCamera, Camera, DebugShape, DebugShapes};

use crate::add_overlay;

/// Adds the overlay "Debug shapes". Needs `UiModule` to show it.
pub struct DebugShapesModule;

impl Module for DebugShapesModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugShapes>();
        add_overlay(app, "Debug shapes", true, draw);
    }
}

fn draw(ui: &mut egui::Ui, world: &mut World) {
    if world.resource::<DebugShapes>().0.is_empty() {
        return;
    }
    let mut cameras = world.query_filtered::<&Camera, With<ActiveCamera>>();
    let Some(camera) = cameras.iter(world).next().copied() else {
        return;
    };
    let picture = ui.clip_rect();
    // In egui points: the aspect is that of the picture.
    let size = [picture.width(), picture.height()];
    let basis = ray_basis(&camera, size[0] / size[1]);
    let pos = |pixel: [f32; 2]| picture.min + egui::vec2(pixel[0], pixel[1]);
    let painter = ui.painter().with_clip_rect(picture);
    let line = |a: Vec3, b: Vec3, color: egui::Color32| {
        if let Some([a, b]) = project_segment(&camera, basis, size, a, b) {
            painter.line_segment([pos(a), pos(b)], egui::Stroke::new(1.5, color));
        }
    };
    for (shape, [r, g, b]) in &world.resource::<DebugShapes>().0 {
        let color = egui::Rgba::from_rgb(*r, *g, *b).into();
        match shape {
            DebugShape::Line(a, b) => line(*a, *b, color),
            DebugShape::Box(center, size) => {
                let half = *size / 2.0;
                let corner = |i: usize| {
                    let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
                    *center + half * Vec3::new(sign(1), sign(2), sign(4))
                };
                // The 12 edges: the corners that differ in one bit.
                for i in 0..8 {
                    for bit in [1, 2, 4] {
                        if i & bit == 0 {
                            line(corner(i), corner(i | bit), color);
                        }
                    }
                }
            }
            DebugShape::Label(at, text) => {
                if let Some(pixel) = project_point(&camera, basis, size, *at) {
                    let font = egui::FontId::monospace(12.0);
                    let anchor = egui::Align2::CENTER_BOTTOM;
                    painter.text(pos(pixel), anchor, text, font, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shapes that one frame of the overlay paints.
    fn painted(shapes: DebugShapes) -> usize {
        let mut world = World::new();
        let camera = Camera {
            position: Vec3::new(0.0, 5.0, 5.0),
            target: Vec3::ZERO,
            fov_y_degrees: 45.0,
        };
        world.spawn((camera, ActiveCamera));
        world.insert_resource(shapes);
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| draw(ui, &mut world));
        output.textures_delta.clear();
        output.shapes.len()
    }

    #[test]
    fn a_box_is_its_twelve_edges_and_a_shape_behind_the_camera_is_not_drawn() {
        let mut shapes = DebugShapes::default();
        shapes.cube(Vec3::ZERO, Vec3::ONE, [1.0, 0.0, 0.0]);
        shapes.label(Vec3::Y, "here", [1.0, 1.0, 1.0]);
        assert_eq!(painted(shapes), 13);

        let mut behind = DebugShapes::default();
        behind.line(Vec3::new(0.0, 6.0, 7.0), Vec3::new(0.0, 7.0, 8.0), [1.0; 3]);
        assert_eq!(painted(behind), 0);
    }
}
