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
//! A tool (an editor) has another layout (`Panels::set_tool_layout`): the shell of
//! the `shell` module. A menu row, a tool row, a status row, and docks with tabs:
//! each panel of [`add_tool_panel`] and each debug panel is a tab, and the picture
//! is the tab in the middle (`maki_platform::PictureFit::Window`). The commands of
//! the tool are one list (`commands` module), its widgets one set (`widgets`
//! module), and its look one table (`theme` module).
//!
//! A module adds a debug panel with [`add_panel`], a panel at the left of the picture
//! with [`add_left_panel`], and a window on top of the picture (game text) with
//! [`add_window`]. The help panel (`help` module), the stats panel
//! (`stats` module), the panel of the renderer settings (`render_panel` module), and
//! the message window and game state panel of the events (`events` module) are
//! added in the same way.

mod commands;
mod events;
mod help;
mod overlay;
mod render_panel;
mod shell;
mod stats;
pub mod theme;
pub mod widgets;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::{Mut, World};
use maki_core::{App, Extract, Input, Module, Settings, Startup, Update};
use maki_platform::{InputCapture, Picture, RawWindowEvent, Window, WindowMargin};
use maki_render::{RenderGpu, RenderWorld, setup_renderer};
use serde::{Deserialize, Serialize};

/// For the `egui::Ui` parameter of a panel function: a module that adds a panel uses
/// this, so it has the same egui version as this crate.
pub use commands::{
    COMMAND_SEARCH, ToolCommand, ToolCommands, add_command, command_item, command_keys, key_label,
    queue_command, run_command, run_queued,
};
pub use egui;
pub use events::{EventsUiModule, game_state_panel};
pub use help::TOGGLE_HELP;
pub use render_panel::RenderPanelModule;
pub use shell::{Layout, SearchRow};
pub use stats::{FrameTime, StatsPanelModule};
pub use theme::THEME;
pub use widgets::{Jobs, NoticeLevel, Notices, notify};

/// Draws the content of one panel. It gets the game world: a debug panel can show, and
/// change, all game state. The [`Panels`] resource is not in the world during the call.
pub type PanelFn = Box<dyn FnMut(&mut egui::Ui, &mut World) + Send + Sync>;

/// What is in a menu of the menu row of a tool, under its commands.
type Menu = (&'static str, Option<PanelFn>);

struct Panel {
    /// The id: unique, and the same in each version (the layout file has it).
    name: &'static str,
    /// The text of its tab and of its row in the "Window" menu.
    title: &'static str,
    icon: &'static str,
    open: bool,
    /// As its module added it: a layout of a tool starts from this.
    default_open: bool,
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
    /// A question in the middle of the window. Nothing else gets input while it
    /// is open.
    Modal,
    /// The places of a tool layout: see [`ToolPlace`].
    Top,
    TopMiddle,
    Right,
    Bottom,
    Status,
}

/// A place of the layout of a tool. `Left`, `Right`, and `Bottom` are docks: the
/// panel is a tab, and the place is its dock in the default layout only. The user
/// moves the tab to each other dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPlace {
    /// The tool row under the menus, from its left edge: tools and switches.
    Top,
    /// The middle of the tool row: play and stop.
    TopMiddle,
    /// The dock at the left of the picture: lists of the things of a document.
    Left,
    /// The dock at the right of the picture: the values of the selected thing.
    Right,
    /// The dock under the picture, between the left and the right dock: wide
    /// panels, lists of files, logs.
    Bottom,
    /// One row at the bottom of the window.
    Status,
}

/// A panel of the layout of a tool, for [`add_tool_panel`].
#[derive(Debug, Clone, Copy)]
pub struct ToolPanel {
    /// Unique, and the same in each version: the layout file has it.
    pub id: &'static str,
    /// The text of its tab and of its row in the "Window" menu.
    pub title: &'static str,
    /// One character of `theme::icon`, before the title.
    pub icon: &'static str,
    pub place: ToolPlace,
    /// The state at the start, and in a layout that does not close it.
    pub open: bool,
}

/// The `[ui]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    /// The size of the text and the panels, multiplied with the scale of the
    /// display: 1.5 on a display at 100 % shows as a display at 150 % does.
    pub scale: f32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self { scale: 1.0 }
    }
}

impl Settings for UiSettings {
    const TABLE: &'static str = "ui";

    fn check(&self) -> Result<(), String> {
        if !(0.5..=3.0).contains(&self.scale) {
            return Err(format!("scale must be 0.5 to 3.0, got {}", self.scale));
        }
        Ok(())
    }
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
    /// `Some`: the layout of a tool.
    tool: Option<shell::Shell>,
    /// The menus of the menu row of a tool, in the order of `add_menu`.
    menus: Vec<Menu>,
}

impl Panel {
    /// True: a tab of a dock of a tool layout.
    fn is_tab(&self) -> bool {
        matches!(
            self.place,
            Place::Column | Place::Left | Place::Right | Place::Bottom
        )
    }
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

    /// The layout of a tool: the shell with its docks (`shell` module). `layouts`:
    /// the named layouts, the default one first. `file`: where the layout is saved
    /// and read; `None`: it is not saved.
    pub fn set_tool_layout(&mut self, layouts: Vec<Layout>, file: Option<std::path::PathBuf>) {
        self.tool = Some(shell::Shell::new(layouts, file));
    }

    /// The title of the tab with the picture: the name of the document of a tool.
    pub fn set_document(&mut self, title: &str) {
        if let Some(shell) = self.tool.as_mut().filter(|shell| shell.document != title) {
            shell.document = title.to_owned();
        }
    }

    /// Opens a panel of a tool and makes its tab the one that its dock shows.
    pub fn show(&mut self, name: &str) {
        self.set_open(name, true);
        if let Some(shell) = &mut self.tool {
            shell.focus = Some(name.to_owned());
        }
    }

    /// The names of the layouts of a tool, and the one that was applied last.
    pub fn layouts(&self) -> (Vec<&'static str>, &str) {
        match &self.tool {
            Some(shell) => (shell.layout_names(), &shell.current),
            None => (Vec::new(), ""),
        }
    }

    /// Applies a named layout of a tool in the next frame.
    pub fn apply_layout(&mut self, name: &str) {
        if let Some(shell) = &mut self.tool {
            shell.request = Some(name.to_owned());
        }
    }

    /// The panels that the "Window" menu of a tool lists: the id, the title, and
    /// if it is open.
    pub fn window_list(&self) -> Vec<(&'static str, &'static str, bool)> {
        let tabs = self.panels.iter().filter(|panel| panel.is_tab());
        tabs.map(|panel| (panel.name, panel.title, panel.open))
            .collect()
    }

    /// The tabs that are in one dock with the tab of a panel, with that tab. Empty:
    /// the panel is not open, or the first frame did not run.
    pub fn tabs_with(&self, name: &str) -> Vec<String> {
        let dock = self.tool.as_ref().and_then(|shell| shell.dock.as_ref());
        let path = dock.and_then(|dock| Some((dock, dock.find_tab_from(|tab| tab == name)?)));
        let leaf = path.and_then(|(dock, path)| dock.leaf(path.node_path()).ok());
        leaf.map_or(Vec::new(), |leaf| leaf.tabs.clone())
    }

    /// Does what a row of the command search stands for, as a click on it does.
    // For the tests of other crates only: no tool calls it.
    #[doc(hidden)]
    pub fn run_row(&mut self, world: &mut World, row: &SearchRow) {
        if let Some(shell) = &mut self.tool {
            shell::run_search_row(shell, &mut self.panels, world, row);
        }
    }

    /// The rows of the command search of a tool for a text.
    pub fn search(&self, world: &World, query: &str) -> Vec<SearchRow> {
        match &self.tool {
            Some(shell) => shell::search_rows(shell, &self.panels, world, query),
            None => Vec::new(),
        }
    }

    /// The text of the layout file for the layout of now. Empty: no tool layout, or
    /// the first frame did not run.
    pub fn layout_text(&self) -> String {
        let shell = self.tool.as_ref();
        shell.map_or(String::new(), |shell| {
            shell::layout_text(shell, &self.panels)
        })
    }

    /// Takes the layout of the text of a layout file. False: the text is not a
    /// layout, and nothing is changed.
    pub fn read_layout(&mut self, text: &str) -> bool {
        let Some(shell) = &mut self.tool else {
            return false;
        };
        shell::read_layout(shell, &mut self.panels, text)
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

/// Adds a panel of the layout of a tool (`Panels::set_tool_layout`): one call, and
/// the panel is a tab of its dock, a row of the "Window" menu, and a row of the
/// command search. More than one at a place that is not a dock: one after the
/// other. `draw` is as that of [`add_panel`].
pub fn add_tool_panel(
    app: &mut App,
    panel: ToolPanel,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    let place = match panel.place {
        ToolPlace::Top => Place::Top,
        ToolPlace::TopMiddle => Place::TopMiddle,
        ToolPlace::Left => Place::Left,
        ToolPlace::Right => Place::Right,
        ToolPlace::Bottom => Place::Bottom,
        ToolPlace::Status => Place::Status,
    };
    register(app, panel.id, panel.open, place, Box::new(draw));
    let mut panels = app.resource_mut::<Panels>();
    let added = panels.panels.last_mut().expect("it was added");
    (added.title, added.icon) = (panel.title, panel.icon);
}

/// Adds a menu to the menu row of a tool, after the menus that were added before
/// it. It lists each command of the command list with this name as its
/// `ToolCommand::menu`, and under them what `extra` draws (a list of files, a text
/// field). The "Window" menu is the last one, and is not added.
pub fn add_menu(app: &mut App, name: &'static str, extra: Option<PanelFn>) {
    app.init_resource::<Panels>();
    app.resource_mut::<Panels>().menus.push((name, extra));
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

/// Adds a question in the middle of the window, on top of all: while it is open, no
/// panel and no action of the game gets the pointer. The parameters are those of
/// [`add_panel`].
pub fn add_modal(
    app: &mut App,
    name: &'static str,
    open: bool,
    draw: impl FnMut(&mut egui::Ui, &mut World) + Send + Sync + 'static,
) {
    register(app, name, open, Place::Modal, Box::new(draw));
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
        title: name,
        icon: theme::icon::PANEL,
        open,
        default_open: open,
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
        app.add_settings::<UiSettings>();
        let scale = app.world().resource::<UiSettings>().scale;
        let mut context = UiContext::default();
        context.ctx.set_zoom_factor(scale);
        // egui takes a new zoom at the start of its next frame. An empty frame now:
        // the first frame of the app then has the size of the window in the points
        // of the zoom (the docks of a tool get their sizes from it).
        let output = context.ctx.run_ui(egui::RawInput::default(), |_| {});
        context.textures.append(output.textures_delta);
        app.insert_resource(context)
            .init_resource::<Panels>()
            .add_systems(
                Startup,
                overlay::add_pass
                    .after(setup_renderer)
                    .run_if(resource_exists::<RenderGpu>),
            )
            .init_resource::<ToolCommands>()
            .add_systems(Input, feed_events)
            // Before the egui frame: a panel shows what the command did. It has the
            // full world, as `run_ui`.
            .add_systems(Update, command_keys.before(run_ui).ambiguous_with_all())
            // `run_ui` has the full world, because a panel can read all game state.
            // It thus conflicts with each system of `Update`. The order to them changes
            // only if a panel shows a value of this frame or of the frame before.
            .add_systems(Update, run_ui.ambiguous_with_all())
            // The commands of a menu or the search: `Panels` is back in the world.
            .add_systems(Update, run_queued.after(run_ui).ambiguous_with_all())
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
            // Where the renderer puts the picture: the marks and the windows of the
            // panels are drawn at the same place.
            let place = *world.resource::<Picture>();
            let Panels {
                panels,
                hidden,
                tool,
                menus,
            } = panels;
            // The look of a tool. The panels of a game keep the look of egui.
            if let Some(shell) = tool.as_mut().filter(|shell| !shell.themed) {
                theme::apply(&ui.ctx);
                shell.themed = true;
            }
            let shown = |panel: &Panel, place| panel.open && panel.place == place;
            let left = panels.iter().any(|panel| shown(panel, Place::Left));
            let left_width = if left { LEFT_WIDTH } else { 0.0 };
            // After `close_all` the egui frame is empty (a screenshot).
            let in_menu = |panel: &Panel| matches!(panel.place, Place::Column | Place::Left);
            let column = !*hidden && panels.iter().any(in_menu);
            // The room that the panels take, in egui points: left, right, top, bottom.
            let mut room = [left_width, COLUMN_WIDTH, 0.0, 0.0];
            let output = ui.ctx.run_ui(input, |root| {
                // The place of the picture in the window, in egui points.
                let points = 1.0 / root.ctx().pixels_per_point();
                let picture = egui::Rect::from_min_size(
                    egui::pos2(place.left as f32, place.top as f32) * points,
                    egui::vec2(place.width as f32, place.height as f32) * points,
                );
                if let Some(shell) = tool.as_mut().filter(|_| !*hidden) {
                    room = shell::show(shell, panels, menus, root, world);
                } else if left {
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
                if column && tool.is_none() {
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
                    if shown(panel, Place::Modal) {
                        egui::Modal::new(egui::Id::new(panel.name)).show(root.ctx(), |ui| {
                            ui.strong(panel.name);
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
            let [left, right, top, bottom] = room.map(pixels);
            let margin = WindowMargin {
                left,
                right,
                top,
                bottom,
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
            let pointer = match tool.as_ref().and_then(|shell| shell.picture) {
                // A tool: the docks fill the window, so the pointer is on a panel
                // at each place but on the picture, where no tab floats over it.
                Some(picture) => {
                    let ctx = &ui.ctx;
                    let on_panel = |at: egui::Pos2| {
                        let layer = ctx.layer_id_at(at);
                        let under =
                            layer.is_none_or(|layer| layer.order == egui::Order::Background);
                        !(picture.contains(at) && under)
                    };
                    let at = ctx.input(|input| input.pointer.interact_pos());
                    let down = ctx.input(|input| input.pointer.any_down());
                    ctx.egui_is_using_pointer() || (at.is_some_and(on_panel) && !down)
                }
                None => ui.ctx.egui_wants_pointer_input(),
            };
            *world.resource_mut::<InputCapture>() = InputCapture {
                keyboard: ui.ctx.egui_wants_keyboard_input(),
                pointer,
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
            .chord
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
        let binding = |chord, description| maki_platform::Binding {
            chord,
            action: maki_platform::Action("quit"),
            description,
            mode: maki_platform::InputMode::GAME,
        };
        let bindings = Bindings(vec![
            binding(Key::R.into(), "Changed binding"),
            binding(maki_platform::Chord::ctrl(Key::Z), "A chord"),
        ]);
        let mode = maki_platform::InputMode::GAME;
        let rows = help::rows(&bindings, mode, |_| None);
        let rows: Vec<_> = rows
            .iter()
            .map(|(key, text)| (key.as_str(), *text))
            .collect();
        assert_eq!(rows, [("R", "Changed binding"), ("Ctrl+Z", "A chord")]);
        assert!(help::rows(&bindings, maki_platform::InputMode("other"), |_| None).is_empty());
        // A command of a tool gives its name to the row of its key.
        let named = help::rows(&bindings, mode, |_| Some("Command"));
        assert_eq!(named[0], ("R".to_owned(), "Command"));
    }

    /// A tool app with no window: the shell runs, nothing is drawn. `ids`: its
    /// panels, with their docks.
    fn tool_app(ids: &[(&'static str, ToolPlace)]) -> App {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule).add_module(UiModule);
        for (id, place) in ids {
            let panel = ToolPanel {
                id,
                title: id,
                icon: "",
                place: *place,
                open: true,
            };
            add_tool_panel(&mut app, panel, |_, _| {});
        }
        let layout = |name, debug| Layout {
            name,
            closed: &[],
            debug,
            sizes: [200.0; 3],
        };
        let layouts = vec![layout("Default", false), layout("Debug", true)];
        app.resource_mut::<Panels>().set_tool_layout(layouts, None);
        app
    }

    #[derive(Resource, Default)]
    struct Count(u32);

    /// A module of another crate: one call for its panel, one for its command.
    struct CounterModule;

    const COUNT: maki_platform::Action = maki_platform::Action("test_count");

    impl Module for CounterModule {
        fn build(&self, app: &mut App) {
            app.init_resource::<Count>();
            maki_platform::add_bindings(app, &[(Key::K, COUNT, "Count")]);
            let panel = ToolPanel {
                id: "counter",
                title: "Counter",
                icon: theme::icon::PANEL,
                place: ToolPlace::Left,
                open: true,
            };
            add_tool_panel(app, panel, |ui, world| {
                ui.label(world.resource::<Count>().0.to_string());
            });
            let run = |world: &mut World| world.resource_mut::<Count>().0 += 1;
            let command = ToolCommand {
                menu: "Test",
                action: Some(COUNT),
                ..ToolCommand::new("test.count", "Count one up", run)
            };
            add_command(app, command);
        }
    }

    #[test]
    fn a_module_adds_a_panel_and_a_command_to_the_shell_of_a_tool() {
        let mut app = tool_app(&[("list", ToolPlace::Left), ("log", ToolPlace::Bottom)]);
        app.add_module(CounterModule);
        app.run(|mut app| {
            app.advance(Duration::from_millis(16));
            let world = app.world();
            let panels = world.resource::<Panels>();
            // In "Window", and open.
            let window = panels.window_list();
            assert!(window.contains(&("counter", "Counter", true)), "{window:?}");
            // In its default dock: with the other panel of the left dock.
            assert_eq!(panels.tabs_with("counter"), ["list", "counter"]);
            assert_eq!(panels.tabs_with("log"), ["log"]);
            // A debug panel of a game is a tab that the default layout closes.
            assert!(window.contains(&("Help", "Help", false)), "{window:?}");
            // In the command search, with its key.
            let texts = |query: &str| -> Vec<String> {
                let rows = panels.search(world, query);
                rows.into_iter().map(|row| row.text).collect()
            };
            assert_eq!(texts("count up"), ["Test: Count one up"]);
            assert_eq!(panels.search(world, "count up")[0].key, "K");
            assert_eq!(texts("window counter"), ["Window: Counter"]);
            assert_eq!(texts("layout"), ["Layout: Default", "Layout: Debug"]);
            // Its key runs it.
            app.write_message(RawInput::KeyDown(Key::K));
            app.advance(Duration::from_millis(16));
            assert_eq!(app.world().resource::<Count>().0, 1);

            // "Window" closes and opens a tab; a named layout opens the debug panels.
            app.resource_mut::<Panels>().set_open("counter", false);
            app.advance(Duration::from_millis(16));
            assert!(
                app.world()
                    .resource::<Panels>()
                    .tabs_with("counter")
                    .is_empty()
            );
            app.resource_mut::<Panels>().apply_layout("Debug");
            app.advance(Duration::from_millis(16));
            let panels = app.world().resource::<Panels>();
            assert_eq!(panels.tabs_with("counter"), ["list", "counter"]);
            assert_eq!(panels.tabs_with("Help"), ["Help", "log"]);
            assert_eq!(panels.layouts().1, "Debug");
        });
    }

    #[test]
    fn the_layout_is_written_and_read() {
        let (left, right, bottom) = (ToolPlace::Left, ToolPlace::Right, ToolPlace::Bottom);
        // The app that writes: the user closes one tab.
        let app = tool_app(&[("list", left), ("log", bottom), ("gone", right)]);
        let text = app.run(|mut app| {
            app.advance(Duration::from_millis(16));
            app.resource_mut::<Panels>().set_open("log", false);
            app.advance(Duration::from_millis(16));
            app.world().resource::<Panels>().layout_text()
        });
        assert!(text.contains("gone"), "{text}");

        // A later version reads it: one panel is gone, and one is new.
        let app = tool_app(&[("list", left), ("log", bottom), ("new", right)]);
        app.run(|mut app| {
            app.advance(Duration::from_millis(16));
            assert!(app.resource_mut::<Panels>().read_layout(&text));
            app.advance(Duration::from_millis(16));
            let panels = app.world().resource::<Panels>();
            // As the file has it.
            assert_eq!(panels.tabs_with("list"), ["list"]);
            assert!(!panels.is_open("log"));
            // The new panel is in its default dock, and the gone one is no tab.
            assert_eq!(panels.tabs_with("new"), ["new"]);
            assert!(!panels.layout_text().contains("gone"));
            // The same layout gives the same text, but for the panel list.
            let again = panels.layout_text();
            assert!(app.resource_mut::<Panels>().read_layout(&again));
            assert_eq!(app.world().resource::<Panels>().layout_text(), again);

            // A file that is not a layout: the layout stays.
            assert!(!app.resource_mut::<Panels>().read_layout("not a layout"));
            assert_eq!(app.world().resource::<Panels>().layout_text(), again);
        });
    }
}
