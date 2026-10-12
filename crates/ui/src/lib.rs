//! The debug UI: egui panels in a column at the right of the picture.
//!
//! The picture has a fixed size (the config) and is in the top left corner of the
//! window. The column fills the window at the right of it: no panel hides the picture.
//! This crate tells the platform crate the width of the column (`WindowMargin`), and
//! the window is as large as the two.
//!
//! Data flow of one frame:
//! 1. `Input` schedule: `feed_events` gives the window events to egui (`egui-winit`).
//! 2. `Update` schedule: `run_ui` runs the egui frame. It draws each open panel of the
//!    [`Panels`] registry as a section of the column and keeps the result: triangles and texture changes. Then it
//!    writes `InputCapture`, so input that egui uses starts no action in the next frame.
//! 3. `Extract` schedule: `extract_ui` moves the result into the render world.
//! 4. The `ui_overlay` pass of the frame graph (`overlay` module) draws the triangles
//!    into the window image, after the compose pass and before the copy to the window.
//!
//! A module adds a debug panel with [`add_panel`], and a window on top of the picture
//! (game text) with [`add_window`]. The help panel (`help` module) and
//! the stats panel (`stats` module) are added in the same way.

mod help;
mod overlay;
mod stats;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::{Mut, World};
use maki_core::{App, Config, Extract, Input, Module, Startup, Update};
use maki_platform::{InputCapture, RawWindowEvent, Window, WindowMargin};
use maki_render::{RenderGpu, RenderWorld, setup_renderer};

/// For the `egui::Ui` parameter of a panel function: a module that adds a panel uses
/// this, so it has the same egui version as this crate.
pub use egui;
pub use stats::StatsPanelModule;

/// Draws the content of one panel. It gets the game world: a debug panel can show, and
/// change, all game state. The [`Panels`] resource is not in the world during the call.
pub type PanelFn = Box<dyn FnMut(&mut egui::Ui, &mut World) + Send + Sync>;

struct Panel {
    name: &'static str,
    open: bool,
    /// True: an egui window on top of the picture. False: a section of the column.
    floating: bool,
    draw: PanelFn,
}

/// Width of the panel column in egui points. A window that is wider than the picture
/// and this gives the rest to the column.
const COLUMN_WIDTH: f32 = 440.0;

/// The panel registry: all panels, by name. Each open panel is one section of the
/// column, or one egui window on top of the picture.
#[derive(Resource, Default)]
pub struct Panels(Vec<Panel>);

impl Panels {
    /// False also if no panel has the name.
    pub fn is_open(&self, name: &str) -> bool {
        self.0.iter().any(|panel| panel.name == name && panel.open)
    }

    /// Shows or hides a panel. Does nothing if no panel has the name.
    pub fn set_open(&mut self, name: &str, open: bool) {
        for panel in self.0.iter_mut().filter(|panel| panel.name == name) {
            panel.open = open;
        }
    }

    /// Hides each panel: the window shows the picture alone (a screenshot).
    pub fn close_all(&mut self) {
        for panel in &mut self.0 {
            panel.open = false;
        }
    }

    /// Shows a hidden panel, hides a shown one. Does nothing if no panel has the name.
    pub fn toggle(&mut self, name: &str) {
        for panel in self.0.iter_mut().filter(|panel| panel.name == name) {
            panel.open = !panel.open;
        }
    }
}

/// Adds a debug panel: a section of the column at the right of the picture, folded at
/// the start. Call from `Module::build`. `name` is the title and must be unique. `open`
/// is the state at startup: a panel that is not open has no section. `draw` runs in
/// each frame while the section is unfolded.
// ponytail: no list of panels on screen, so only code can open a closed panel (the help
// panel has an action). Add a menu with one checkbox for each name if that is needed.
pub fn add_panel(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, false, Box::new(draw));
}

/// Adds a window on top of the picture, for text of the game (a message). Not for
/// debug data: that goes into the column, where it hides nothing. The parameters are
/// those of [`add_panel`].
pub fn add_window(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, true, Box::new(draw));
}

fn register(app: &mut App, name: &'static str, open: bool, floating: bool, draw: PanelFn) {
    // `App` gives no direct access to a resource, so a `Startup` system adds the panel.
    // A system can run more than one time; the panel moves out in the first run.
    let mut panel = Some(Panel {
        name,
        open,
        floating,
        draw,
    });
    app.init_resource::<Panels>()
        // Each module adds its own panel: the order changes nothing but the list order.
        .allow_ambiguous_resource::<Panels>()
        .add_systems(Startup, move |mut panels: ResMut<Panels>| {
            if let Some(panel) = panel.take() {
                assert!(
                    panels.0.iter().all(|other| other.name != panel.name),
                    "panel `{}` was added two times",
                    panel.name
                );
                panels.0.push(panel);
            }
        });
}

/// egui and the result of its last frame.
#[derive(Resource, Default)]
struct UiContext {
    ctx: egui::Context,
    /// Turns window events into egui input. `None` until the first frame with a window;
    /// stays `None` in a headless app.
    winit: Option<egui_winit::State>,
    /// The triangles of the last egui frame. They wait here for `Extract`.
    primitives: Vec<egui::ClippedPrimitive>,
    /// The texture changes of the last egui frame (the font image).
    textures: egui::TexturesDelta,
    /// Physical pixels in one egui point (the display scale).
    pixels_per_point: f32,
}

impl Drop for UiContext {
    fn drop(&mut self) {
        // egui panics in a debug build when texture changes are dropped unread. At the
        // end of the app they have no reader.
        self.textures.clear();
    }
}

/// Runs egui, draws the registered panels beside the picture, and adds the help
/// panel. Needs `PlatformModule`. Without `RenderModule` the panels run but are not drawn.
pub struct UiModule;

impl Module for UiModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiContext>()
            .init_resource::<Panels>()
            .add_systems(
                Startup,
                overlay::add_pass
                    .after(setup_renderer)
                    .run_if(resource_exists::<RenderGpu>),
            )
            .add_systems(Input, feed_events)
            // `run_ui` has the full world, because a panel can read all game state.
            // It thus conflicts with each system of `Update`. The order to them changes
            // only if a panel shows a value of this frame or of the frame before.
            .add_systems(Update, run_ui.ambiguous_with_all())
            .add_systems(Extract, extract_ui.run_if(resource_exists::<RenderWorld>));
        help::build(app);
    }
}

fn feed_events(
    mut events: MessageReader<RawWindowEvent>,
    window: Option<Res<Window>>,
    mut ui: ResMut<UiContext>,
) {
    // No window (a headless app): egui gets no input.
    let Some(window) = window else { return };
    let window = window.winit();
    let UiContext { ctx, winit, .. } = &mut *ui;
    let state = winit.get_or_insert_with(|| {
        egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        )
    });
    for event in events.read() {
        // The response tells if egui used this one event. It is not read: `run_ui` asks
        // egui one time, after the frame, if it wants the keyboard or the mouse.
        let _ = state.on_window_event(window, &event.0);
    }
}

/// Runs one egui frame with all open panels.
fn run_ui(world: &mut World) {
    // `resource_scope` takes a resource out of the world for the call, so the panels
    // can get the world.
    world.resource_scope(|world, mut ui: Mut<UiContext>| {
        world.resource_scope(|world, mut panels: Mut<Panels>| {
            let ui = &mut *ui;
            let input = match (&mut ui.winit, world.get_resource::<Window>()) {
                (Some(state), Some(window)) => state.take_egui_input(window.winit()),
                _ => egui::RawInput::default(),
            };
            let config = world.resource::<Config>();
            let picture_pixels =
                egui::vec2(config.window_width as f32, config.window_height as f32);
            let output = ui.ctx.run_ui(input, |root| {
                // The place of the picture in the window, in egui points.
                let picture = egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    picture_pixels / root.ctx().pixels_per_point(),
                );
                // No open panel, no column: the egui frame is empty (a screenshot).
                if panels.0.iter().any(|panel| panel.open && !panel.floating) {
                    // All of the window at the right of the picture.
                    let width = root.ctx().content_rect().width() - picture.width();
                    egui::Panel::right("panels")
                        .exact_size(width.max(COLUMN_WIDTH))
                        .resizable(false)
                        .show(root, |ui| {
                            egui::ScrollArea::both().show(ui, |ui| {
                                for panel in &mut panels.0 {
                                    if panel.open && !panel.floating {
                                        egui::CollapsingHeader::new(panel.name)
                                            .show(ui, |ui| (panel.draw)(ui, world));
                                    }
                                }
                            });
                        });
                }
                for panel in &mut panels.0 {
                    if panel.open && panel.floating {
                        egui::Window::new(panel.name)
                            .collapsible(false)
                            .resizable(false)
                            // At the bottom of the picture, in the middle.
                            .pivot(egui::Align2::CENTER_BOTTOM)
                            .default_pos(picture.center_bottom() - egui::vec2(0.0, 24.0))
                            .constrain_to(picture)
                            .show(root.ctx(), |ui| (panel.draw)(ui, world));
                    }
                }
            });

            ui.primitives = ui.ctx.tessellate(output.shapes, output.pixels_per_point);
            ui.pixels_per_point = output.pixels_per_point;
            // The column in physical pixels: the platform crate makes the window wider by
            // it, after the first frame. Only a different value is a write.
            let margin = WindowMargin {
                right: (COLUMN_WIDTH * output.pixels_per_point).round() as u32,
            };
            if let Some(mut current) = world.get_resource_mut::<WindowMargin>() {
                current.set_if_neq(margin);
            }
            // `append`: if no `Extract` took the changes of the frame before, they stay.
            ui.textures.append(output.textures_delta);
            // The mouse cursor shape, and other requests of egui to the OS.
            if let (Some(state), Some(window)) = (&mut ui.winit, world.get_resource::<Window>()) {
                state.handle_platform_output(window.winit(), output.platform_output);
            }

            // The pointer is on a panel, or a text field has the focus: the input
            // system starts no action from that device in the next frame.
            *world.resource_mut::<InputCapture>() = InputCapture {
                keyboard: ui.ctx.egui_wants_keyboard_input(),
                pointer: ui.ctx.egui_wants_pointer_input(),
            };
        });
    });
}

/// Moves the result of the egui frame into the render world.
fn extract_ui(mut ui: ResMut<UiContext>, mut render_world: ResMut<RenderWorld>) {
    let mut extracted = render_world
        .0
        .get_resource_or_insert_with(overlay::ExtractedUi::default);
    extracted.primitives = std::mem::take(&mut ui.primitives);
    extracted.pixels_per_point = ui.pixels_per_point;
    // Added to the changes that wait there. The pass takes them only in a frame that
    // is drawn, and a lost change would be a missing font image.
    extracted
        .textures
        .get_mut()
        .expect("no thread holds the lock")
        .append(std::mem::take(&mut ui.textures));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::Config;
    use maki_platform::{Bindings, Key, PlatformModule, RawInput};

    use super::*;

    /// No window and no GPU: the egui frame runs, nothing is drawn.
    #[test]
    fn the_help_action_toggles_the_help_panel() {
        let mut app = App::new(Config::default()).unwrap();
        app.add_module(PlatformModule).add_module(UiModule);
        add_panel(&mut app, "Test", false, |_, _| {});
        let help_key = Bindings::default()
            .0
            .iter()
            .find(|binding| binding.action == maki_platform::Action::ToggleHelp)
            .expect("the help action has a key")
            .key;

        app.run(|mut app| {
            let mut frame = |events: &[RawInput]| {
                for event in events {
                    app.write_message(*event);
                }
                app.advance(Duration::from_millis(16));
                let world = app.world();
                (
                    world.resource::<Panels>().is_open(help::HELP),
                    world.resource::<UiContext>().primitives.is_empty(),
                )
            };
            // egui only measures a new window in its first frame.
            let _ = frame(&[]);
            // Open at startup: egui made triangles for it.
            assert_eq!(frame(&[]), (true, false));
            assert_eq!(frame(&[RawInput::KeyDown(help_key)]), (false, true));
            // A held key is one press.
            assert_eq!(frame(&[]), (false, true));
            assert_eq!(frame(&[RawInput::KeyUp(help_key)]), (false, true));
            assert!(frame(&[RawInput::KeyDown(help_key)]).0);
            assert!(!app.world().resource::<Panels>().is_open("Test"));
        });
    }

    #[test]
    fn the_help_panel_has_one_row_for_each_binding() {
        let bindings = Bindings(vec![maki_platform::Binding {
            key: Key::R,
            action: maki_platform::Action::Quit,
            description: "Changed binding",
        }]);
        assert_eq!(help::rows(&bindings), [("R", "Changed binding")]);
    }
}
