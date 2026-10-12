//! The debug UI: egui panels in a column at the right of the picture, and the panels
//! of a tool at its left.
//!
//! The picture has a fixed size (the `[window]` settings) and is not scaled. The
//! column fills the window at the right of it, and a left panel the room at its left:
//! no panel hides the picture. This crate tells the platform crate the width of the
//! two (`WindowMargin`), and the window is as large as the three. The first row of
//! the column is a menu with one checkbox for each panel.
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
//! A module adds a debug panel with [`add_panel`], a panel at the left of the picture
//! with [`add_left_panel`], and a window on top of the picture (game text) with
//! [`add_window`]. The help panel (`help` module), the stats panel
//! (`stats` module), the panel of the renderer settings (`render_panel` module), and
//! the message window and game state panel of the events (`events` module) are
//! added in the same way.

mod events;
mod help;
mod overlay;
mod render_panel;
mod stats;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::{Mut, World};
use maki_core::{App, Extract, Input, Module, Startup, Update};
use maki_platform::{InputCapture, RawWindowEvent, Window, WindowMargin, WindowSettings};
use maki_render::{RenderGpu, RenderWorld, setup_renderer};

/// For the `egui::Ui` parameter of a panel function: a module that adds a panel uses
/// this, so it has the same egui version as this crate.
pub use egui;
pub use events::EventsUiModule;
pub use help::TOGGLE_HELP;
pub use render_panel::RenderPanelModule;
pub use stats::StatsPanelModule;

/// Draws the content of one panel. It gets the game world: a debug panel can show, and
/// change, all game state. The [`Panels`] resource is not in the world during the call.
pub type PanelFn = Box<dyn FnMut(&mut egui::Ui, &mut World) + Send + Sync>;

struct Panel {
    name: &'static str,
    open: bool,
    /// For a section of the column: it is unfolded when it shows for the first time.
    unfolded: bool,
    place: Place,
    draw: PanelFn,
}

/// Where a panel is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// A section of the column at the right of the picture.
    Column,
    /// A part of the panel at the left of the picture.
    Left,
    /// An egui window on top of the picture.
    Window,
    /// Text with no window in the top left corner of the picture. Gets no input.
    Corner,
    /// Shapes on top of the picture, under each window. Gets no input.
    Overlay,
}

/// Width of the panel column in egui points. A window that is wider than the picture
/// and this gives the rest to the column.
const COLUMN_WIDTH: f32 = 440.0;
/// Width of the panel at the left of the picture, in egui points.
const LEFT_WIDTH: f32 = 240.0;

/// The panel registry: all panels, by name. Each open panel is one section of the
/// column, one part of the left panel, or one egui window on top of the picture.
#[derive(Resource, Default)]
pub struct Panels {
    panels: Vec<Panel>,
    /// True after `close_all`: no column, so no menu.
    hidden: bool,
}

impl Panels {
    fn named(&mut self, name: &str) -> impl Iterator<Item = &mut Panel> {
        self.panels
            .iter_mut()
            .filter(move |panel| panel.name == name)
    }

    /// False also if no panel has the name.
    pub fn is_open(&self, name: &str) -> bool {
        let mut panels = self.panels.iter();
        panels.any(|panel| panel.name == name && panel.open)
    }

    /// Shows or hides a panel. Does nothing if no panel has the name.
    pub fn set_open(&mut self, name: &str, open: bool) {
        self.named(name).for_each(|panel| panel.open = open);
    }

    /// Hides each panel and the menu: the window shows the picture alone (a
    /// screenshot).
    pub fn close_all(&mut self) {
        self.hidden = true;
        for panel in &mut self.panels {
            panel.open = false;
        }
    }

    /// Shows a hidden panel, hides a shown one. Does nothing if no panel has the name.
    pub fn toggle(&mut self, name: &str) {
        self.named(name).for_each(|panel| panel.open = !panel.open);
    }

    /// The section of a panel of the column is unfolded when it shows for the first
    /// time. Call from `Module::build`, after `add_panel`.
    pub fn unfold(&mut self, name: &str) {
        self.named(name).for_each(|panel| panel.unfolded = true);
    }
}

/// Adds a debug panel: a section of the column at the right of the picture, folded at
/// the start. Call from `Module::build`. `name` is the title and must be unique. `open`
/// is the state at startup: a panel that is not open has no section. `draw` runs in
/// each frame while the section is unfolded.
pub fn add_panel(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, Place::Column, Box::new(draw));
}

/// Adds a panel at the left of the picture: for a tool that needs the two sides (the
/// outliner of an editor). The window is wider by the panel while one such panel is
/// open. More than one: one below the other. The parameters are those of [`add_panel`].
pub fn add_left_panel(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, Place::Left, Box::new(draw));
}

/// Adds a small readout in the top left corner of the picture, with no window around
/// it (a frame counter). The pointer goes through it. The parameters are those of
/// [`add_panel`].
pub fn add_corner(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, Place::Corner, Box::new(draw));
}

/// Adds a drawing on top of the picture, for the marks of a tool (the selection and
/// the handles of an editor). `draw` paints with `ui.painter()`; `ui.clip_rect()` is
/// the place of the picture in the window. The shapes are drawn after the picture is
/// done: they are not in the path tracer or in a pass after it. The pointer goes
/// through them. The parameters are those of [`add_panel`].
pub fn add_overlay(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, Place::Overlay, Box::new(draw));
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
    register(app, name, open, Place::Window, Box::new(draw));
}

fn register(app: &mut App, name: &'static str, open: bool, place: Place, draw: PanelFn) {
    app.init_resource::<Panels>()
        // Each system opens or closes its own panel: their order has no effect.
        .allow_ambiguous_resource::<Panels>();
    let mut panels = app.resource_mut::<Panels>();
    assert!(
        panels.panels.iter().all(|other| other.name != name),
        "panel `{name}` was added two times"
    );
    panels.panels.push(Panel {
        name,
        open,
        unfolded: false,
        place,
        draw,
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
            let panels = &mut *panels;
            let input = match (&mut ui.winit, world.get_resource::<Window>()) {
                (Some(state), Some(window)) => state.take_egui_input(window.winit()),
                _ => egui::RawInput::default(),
            };
            let window = world.resource::<WindowSettings>();
            let picture_pixels = egui::vec2(window.width as f32, window.height as f32);
            let Panels { panels, hidden } = panels;
            let shown = |panel: &Panel, place| panel.open && panel.place == place;
            let left = panels.iter().any(|panel| shown(panel, Place::Left));
            let left_width = if left { LEFT_WIDTH } else { 0.0 };
            // After `close_all` the egui frame is empty (a screenshot).
            let in_menu = |panel: &Panel| matches!(panel.place, Place::Column | Place::Left);
            let column = !*hidden && panels.iter().any(in_menu);
            let output = ui.ctx.run_ui(input, |root| {
                // The place of the picture in the window, in egui points.
                let picture = egui::Rect::from_min_size(
                    egui::pos2(left_width, 0.0),
                    picture_pixels / root.ctx().pixels_per_point(),
                );
                if left {
                    egui::Panel::left("left panels")
                        .exact_size(LEFT_WIDTH)
                        .resizable(false)
                        .show(root, |ui| {
                            for panel in panels.iter_mut() {
                                if shown(panel, Place::Left) {
                                    (panel.draw)(ui, world);
                                }
                            }
                        });
                }
                if column {
                    // All of the window at the right of the picture.
                    let width = root.ctx().content_rect().width() - picture.max.x;
                    egui::Panel::right("panels")
                        .exact_size(width.max(COLUMN_WIDTH))
                        .resizable(false)
                        .show(root, |ui| {
                            ui.menu_button("Panels", |ui| {
                                for panel in panels.iter_mut().filter(|panel| in_menu(panel)) {
                                    ui.checkbox(&mut panel.open, panel.name);
                                }
                            });
                            // Not as narrow as the content: a text field of a
                            // panel has the width of the column.
                            let scroll = egui::ScrollArea::both().auto_shrink(false);
                            scroll.show(ui, |ui| {
                                for panel in panels.iter_mut() {
                                    if shown(panel, Place::Column) {
                                        egui::CollapsingHeader::new(panel.name)
                                            .default_open(panel.unfolded)
                                            .show(ui, |ui| (panel.draw)(ui, world));
                                    }
                                }
                            });
                        });
                }
                for panel in panels.iter_mut() {
                    if shown(panel, Place::Overlay) {
                        egui::Area::new(egui::Id::new(panel.name))
                            .order(egui::Order::Background)
                            .fixed_pos(picture.min)
                            .interactable(false)
                            .show(root.ctx(), |ui| {
                                ui.set_clip_rect(picture);
                                (panel.draw)(ui, world)
                            });
                    }
                    if shown(panel, Place::Corner) {
                        egui::Area::new(egui::Id::new(panel.name))
                            .fixed_pos(picture.min + egui::vec2(8.0, 8.0))
                            .interactable(false)
                            .show(root.ctx(), |ui| {
                                // A new area is narrow: the text must not wrap.
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                (panel.draw)(ui, world)
                            });
                    }
                    if shown(panel, Place::Window) {
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
            // The panels in physical pixels: the platform crate makes the window wider
            // by them, after the first frame. Only a different value is a write.
            let pixels = |points: f32| (points * output.pixels_per_point).round() as u32;
            let margin = WindowMargin {
                left: pixels(left_width),
                right: pixels(COLUMN_WIDTH),
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

    use maki_core::ConfigFile;
    use maki_platform::{Bindings, Key, PlatformModule, RawInput};

    use super::*;

    /// No window and no GPU: the egui frame runs, nothing is drawn.
    #[test]
    fn the_help_action_toggles_the_help_panel() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule).add_module(UiModule);
        add_panel(&mut app, "Test", false, |_, _| {});
        let bindings = app.world().resource::<Bindings>();
        let help_key = bindings
            .0
            .iter()
            .find(|binding| binding.action == TOGGLE_HELP)
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
            // With no open panel, the column still has the menu of the panels.
            assert_eq!(frame(&[RawInput::KeyDown(help_key)]), (false, false));
            // A held key is one press.
            assert_eq!(frame(&[]), (false, false));
            assert_eq!(frame(&[RawInput::KeyUp(help_key)]), (false, false));
            assert!(frame(&[RawInput::KeyDown(help_key)]).0);
            assert!(!app.world().resource::<Panels>().is_open("Test"));
            // For a screenshot: no panel and no menu, so nothing to draw.
            app.resource_mut::<Panels>().close_all();
            app.advance(Duration::from_millis(16));
            assert!(app.world().resource::<UiContext>().primitives.is_empty());
        });
    }

    #[test]
    fn the_help_panel_has_one_row_for_each_binding() {
        let bindings = Bindings(vec![maki_platform::Binding {
            key: Key::R,
            action: maki_platform::Action("quit"),
            description: "Changed binding",
            mode: maki_platform::InputMode::GAME,
        }]);
        let mode = maki_platform::InputMode::GAME;
        assert_eq!(help::rows(&bindings, mode), [("R", "Changed binding")]);
        assert!(help::rows(&bindings, maki_platform::InputMode("other")).is_empty());
    }
}
