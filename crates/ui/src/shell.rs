//! The shell of a tool (an editor): the layout of `Panels::set_tool_layout`.
//!
//! - A menu row and a tool row at the top, a status row at the bottom.
//! - Between them a tree of docks with tabs (`egui_dock`). Each panel of a dock
//!   place is one tab. The user moves a tab, splits a dock with it, groups it with
//!   other tabs, lets it float in the window, and closes it; the "Window" menu opens
//!   it again.
//! - One tab is not a panel: the picture ([`PICTURE`]). The place of its body is the
//!   place of the picture of the renderer (`maki_platform::WindowMargin`). Its title
//!   is the name of the document (`Panels::set_document`).
//! - The layout is written to a file and read at the start. A named layout
//!   ([`Layout`]) says which panels are open and how large the docks are.
//! - The command search lists each command of the command list and each panel.
//!
//! A game has none of this: `run_ui` calls [`show`] only with a tool layout.

use std::path::PathBuf;

use bevy_ecs::world::World;
use egui_dock::{DockArea, DockState, Node, NodeIndex, NodePath, Split, TabViewer};
use maki_platform::ActionState;
use serde::{Deserialize, Serialize};

use crate::commands::{COMMAND_SEARCH, ToolCommands, command_item, found};
use crate::theme::icon;
use crate::{Menu, Panel, Place};

/// The tab with the picture. It cannot be closed, and it does not float.
pub(crate) const PICTURE: &str = "picture";

/// The size of a tab at the time when it starts to float, in points. (`egui_dock`
/// gives it the size of its dock: the bottom dock is as wide as the window.)
const FLOAT_SIZE: egui::Vec2 = egui::vec2(460.0, 520.0);

/// A named layout: which panels are open, and the sizes of the docks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub name: &'static str,
    /// The ids of the tool panels that this layout closes. Each other tool panel is
    /// as its module added it (`ToolPanel::open`).
    pub closed: &'static [&'static str],
    /// True: each debug panel ([`crate::add_panel`]) is open, at the bottom. False:
    /// they are closed.
    pub debug: bool,
    /// The width of the left dock, the width of the right dock, and the height of
    /// the bottom dock, in points of the UI.
    pub sizes: [f32; 3],
}

pub(crate) struct Shell {
    /// The first one is the default.
    layouts: Vec<Layout>,
    /// The name of the layout that was applied last.
    pub(crate) current: String,
    /// `None` until the first frame: all modules have added their panels then.
    pub(crate) dock: Option<DockState<String>>,
    /// The layout file. `None`: the layout is not saved.
    file: Option<PathBuf>,
    /// The text of the layout file as it was read or written last.
    saved: String,
    /// The title of the picture tab.
    pub(crate) document: String,
    /// The tab that becomes the active one of its dock in the next frame.
    pub(crate) focus: Option<String>,
    /// The layout to apply in the next frame.
    pub(crate) request: Option<String>,
    /// The command search, while it is open: its text and the selected row.
    search: Option<(String, usize)>,
    /// The place of the picture tab in the last frame that showed it, in points.
    pub(crate) picture: Option<egui::Rect>,
    pub(crate) themed: bool,
    /// The floating windows of the frame before: the index of the surface, and the
    /// place and the size of its content. A new one gets a size; the layout file
    /// has the places (`egui_dock` does not keep them).
    windows: Vec<(usize, egui::Rect)>,
    /// The layout changed after the layout file was written.
    unsaved: bool,
}

impl Shell {
    pub(crate) fn new(layouts: Vec<Layout>, file: Option<PathBuf>) -> Self {
        assert!(!layouts.is_empty(), "a tool layout needs one layout");
        Self {
            current: layouts[0].name.to_owned(),
            layouts,
            dock: None,
            file,
            saved: String::new(),
            document: String::new(),
            focus: None,
            request: None,
            search: None,
            picture: None,
            themed: false,
            windows: Vec::new(),
            unsaved: true,
        }
    }

    pub(crate) fn layout_names(&self) -> Vec<&'static str> {
        self.layouts.iter().map(|layout| layout.name).collect()
    }
}

/// The dock of a panel in the default layout. `None`: not a tab.
fn dock_of(panel: &Panel) -> Option<Place> {
    match panel.place {
        // A debug panel of a game is a bottom tab of a tool.
        Place::Column | Place::Bottom => Some(Place::Bottom),
        Place::Left => Some(Place::Left),
        Place::Right => Some(Place::Right),
        _ => None,
    }
}

fn opens(layout: &Layout, panel: &Panel) -> bool {
    match panel.place {
        Place::Column => layout.debug,
        _ => panel.default_open && !layout.closed.contains(&panel.name),
    }
}

/// The dock tree of a layout, and the open state of each panel for it. `window`:
/// the size of the window in points.
fn build(layout: &Layout, panels: &mut [Panel], window: egui::Vec2) -> DockState<String> {
    for panel in panels.iter_mut().filter(|panel| dock_of(panel).is_some()) {
        panel.open = opens(layout, panel);
    }
    let tabs = |place: Place| -> Vec<String> {
        let of_place = |panel: &&Panel| panel.open && dock_of(panel) == Some(place);
        let panels = panels.iter().filter(of_place);
        panels.map(|panel| panel.name.to_owned()).collect()
    };
    let mut dock = DockState::new(vec![PICTURE.to_owned()]);
    let tree = dock.main_surface_mut();
    let [left, right, bottom] = layout.sizes;
    let (mut width, height) = (window.x.max(1.0), window.y.max(1.0));
    // The node with the picture. The side docks have the full height, and the
    // bottom dock is between them: the picture has the most room.
    let mut middle = NodeIndex::root();
    // A split has the part of its left (or top) node.
    let side = tabs(Place::Left);
    if !side.is_empty() {
        [middle, _] = tree.split_left(middle, (left / width).min(0.4), side);
        width = (width - left).max(1.0);
    }
    let side = tabs(Place::Right);
    if !side.is_empty() {
        [middle, _] = tree.split_right(middle, 1.0 - (right / width).min(0.4), side);
    }
    let below = tabs(Place::Bottom);
    if !below.is_empty() {
        [middle, _] = tree.split_below(middle, 1.0 - (bottom / height).min(0.6), below);
    }
    tree.set_focused_node(middle);
    dock
}

/// Puts the tab of an open panel into the tree: with the tabs of its dock if one
/// shows, else in a new dock at its side of the picture. `parts`: the part of the
/// window that a new left, right, or bottom dock gets.
fn add_tab(dock: &mut DockState<String>, panels: &[Panel], index: usize, parts: [f32; 3]) {
    let name = panels[index].name;
    let place = dock_of(&panels[index]);
    let same_dock = |other: &&Panel| dock_of(other) == place && other.name != name;
    let mut neighbours = panels.iter().filter(same_dock);
    let neighbour = neighbours.find_map(|other| dock.find_tab_from(|tab| tab == other.name));
    if let Some(path) = neighbour
        && let Ok(leaf) = dock.leaf_mut(path.node_path())
    {
        leaf.append_tab(name.to_owned());
        return;
    }
    let picture = dock.find_tab_from(|tab| tab == PICTURE);
    let leaf = Node::leaf(name.to_owned());
    match (place, picture) {
        (Some(Place::Left), _) => {
            dock.split(NodePath::MAIN_ROOT, Split::Left, parts[0], leaf);
        }
        (Some(Place::Right), _) => {
            dock.split(NodePath::MAIN_ROOT, Split::Right, 1.0 - parts[1], leaf);
        }
        (_, Some(picture)) => {
            dock.split(picture.node_path(), Split::Below, 1.0 - parts[2], leaf);
        }
        _ => dock.push_to_first_leaf(name.to_owned()),
    }
}

/// Makes the tree follow the open state of each panel (`Panels::set_open`, the
/// "Window" menu), and the request for a tab to show. `window`: the size of the
/// window in points.
fn sync(shell: &mut Shell, panels: &[Panel], window: egui::Vec2) {
    // A new dock has the size that the layout of now gives to its side.
    let layout = shell
        .layouts
        .iter()
        .find(|layout| layout.name == shell.current);
    let [left, right, bottom] = layout.unwrap_or(&shell.layouts[0]).sizes;
    let (width, height) = (window.x.max(1.0), window.y.max(1.0));
    let parts = [
        (left / width).min(0.4),
        (right / width).min(0.4),
        (bottom / height).min(0.6),
    ];
    let Some(dock) = &mut shell.dock else { return };
    for (index, panel) in panels.iter().enumerate() {
        if dock_of(panel).is_none() {
            continue;
        }
        match (panel.open, dock.find_tab_from(|tab| tab == panel.name)) {
            (true, None) => add_tab(dock, panels, index, parts),
            (false, Some(path)) => {
                dock.remove_tab(path);
            }
            _ => {}
        }
    }
    if let Some(name) = shell.focus.take()
        && let Some(path) = dock.find_tab_from(|tab| *tab == name)
    {
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(path.node_path());
    }
}

/// What the layout file holds.
#[derive(Serialize, Deserialize)]
struct Saved {
    /// The name of the layout that was applied last.
    layout: String,
    /// Each panel that the app had when the file was written. A panel that is not
    /// here is new: it is open as its module added it.
    panels: Vec<String>,
    dock: DockState<String>,
    /// The floating windows: the index of the surface, and its place and size.
    #[serde(default)]
    windows: Vec<(usize, egui::Rect)>,
}

/// The text of the layout file for the state of now.
pub(crate) fn layout_text(shell: &Shell, panels: &[Panel]) -> String {
    let Some(dock) = &shell.dock else {
        return String::new();
    };
    let tabs = panels.iter().filter(|panel| dock_of(panel).is_some());
    let saved = Saved {
        layout: shell.current.clone(),
        panels: tabs.map(|panel| panel.name.to_owned()).collect(),
        dock: dock.clone(),
        windows: shell.windows.clone(),
    };
    let config = ron::ser::PrettyConfig::default();
    ron::ser::to_string_pretty(&saved, config).unwrap_or_else(|error| {
        tracing::error!("the layout is not written: {error}");
        String::new()
    })
}

/// Takes the layout of a layout file. False: the text is not a layout (an error is
/// logged), and nothing is changed. A tab of a panel that no module has is dropped; a
/// panel that the file does not know stays as it is, and `sync` puts it into its
/// default dock.
pub(crate) fn read_layout(shell: &mut Shell, panels: &mut [Panel], text: &str) -> bool {
    let mut saved: Saved = match ron::from_str(text) {
        Ok(saved) => saved,
        Err(error) => {
            tracing::warn!("the layout file is not read, the default layout is used: {error}");
            return false;
        }
    };
    let is_tab = |tab: &str| {
        let mut tabs = panels.iter().filter(|panel| dock_of(panel).is_some());
        tab == PICTURE || tabs.any(|panel| panel.name == tab)
    };
    saved.dock.retain_tabs(|tab| is_tab(tab));
    let picture = saved.dock.find_tab_from(|tab| tab == PICTURE);
    if picture.is_none_or(|path| path.surface != egui_dock::SurfaceIndex::main()) {
        tracing::warn!("the layout file has no picture, the default layout is used");
        return false;
    }
    for panel in panels.iter_mut().filter(|panel| dock_of(panel).is_some()) {
        if saved.panels.iter().any(|known| known == panel.name) {
            panel.open = saved.dock.find_tab_from(|tab| tab == panel.name).is_some();
        }
    }
    // egui has the place of a window, and it starts with none: the file gives it.
    for (index, rect) in &saved.windows {
        let index = egui_dock::SurfaceIndex(*index);
        if let Some(state) = saved.dock.get_window_state_mut(index) {
            state.set_position(rect.min).set_size(rect.size());
        }
    }
    shell.windows = saved.windows;
    shell.current = saved.layout;
    shell.dock = Some(saved.dock);
    true
}

/// Applies a named layout. Does nothing if no layout has the name.
fn apply(shell: &mut Shell, panels: &mut [Panel], name: &str, window: egui::Vec2) {
    let Some(layout) = shell.layouts.iter().find(|layout| layout.name == name) else {
        return;
    };
    shell.dock = Some(build(&layout.clone(), panels, window));
    shell.current = name.to_owned();
}

/// The tabs of the dock tree: the picture, and the panels.
struct Tabs<'a> {
    panels: &'a mut [Panel],
    world: &'a mut World,
    document: &'a str,
}

impl TabViewer for Tabs<'_> {
    type Tab = String;

    fn id(&mut self, tab: &mut String) -> egui::Id {
        egui::Id::new(("tab", tab.as_str()))
    }

    fn title(&mut self, tab: &mut String) -> egui::WidgetText {
        if tab == PICTURE {
            return format!("{} {}", icon::ROOM, self.document).into();
        }
        let panel = self.panels.iter().find(|panel| panel.name == tab.as_str());
        let title = panel.map_or(tab.clone(), |panel| {
            format!("{} {}", panel.icon, panel.title)
        });
        title.into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut String) {
        let Some(panel) = self
            .panels
            .iter_mut()
            .find(|panel| panel.name == tab.as_str())
        else {
            return;
        };
        if panel.place == Place::Column {
            // A debug panel has no scroll area of its own. In a dock it fills the
            // tab; a window that floats is as large as its content.
            let docked = ui.layer_id().order == egui::Order::Background;
            let scroll = egui::ScrollArea::both().auto_shrink(!docked);
            scroll.show(ui, |ui| (panel.draw)(ui, self.world));
        } else {
            (panel.draw)(ui, self.world);
        }
    }

    fn is_closeable(&self, tab: &String) -> bool {
        tab != PICTURE
    }

    fn allowed_in_windows(&self, tab: &mut String) -> bool {
        tab != PICTURE
    }

    // The renderer draws the picture under the UI: no fill over it.
    fn clear_background(&self, tab: &String) -> bool {
        tab != PICTURE
    }

    // A panel has its own scroll areas.
    fn scroll_bars(&self, _tab: &String) -> [bool; 2] {
        [false, false]
    }
}

/// Draws `add` in the middle of a row that is drawn already: the play buttons of a
/// tool row, between the tools at its left and the layout list at its right.
// ponytail: in a narrow window the middle part is over the left part. A second row
// if a tool has more buttons than a window of 1280 points holds.
fn in_middle(ui: &mut egui::Ui, row: egui::Rect, add: impl FnOnce(&mut egui::Ui)) {
    // The width of the middle part is that of the frame before.
    let id = egui::Id::new("tool row middle");
    let width: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
    let rect = egui::Rect::from_center_size(row.center(), egui::vec2(width, row.height()));
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    add(&mut child);
    let width = child.min_rect().width();
    ui.data_mut(|data| data.insert_temp(id, width));
}

/// One row of the command search: a command of the list, or a panel to show.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub text: String,
    pub key: String,
    pub can_run: bool,
    target: Target,
}

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Command(&'static str),
    Panel(&'static str),
    Layout(&'static str),
}

/// The rows of the command search for a text: each command, each panel of a dock,
/// and each layout.
pub(crate) fn search_rows(
    shell: &Shell,
    panels: &[Panel],
    world: &World,
    query: &str,
) -> Vec<SearchRow> {
    let commands = world.get_resource::<ToolCommands>().cloned();
    let commands = commands.unwrap_or_default();
    let commands = commands.iter().map(|command| SearchRow {
        text: match command.menu {
            "" => command.name.to_owned(),
            menu => format!("{menu}: {}", command.name),
        },
        key: command.key(world),
        can_run: (command.can_run)(world),
        target: Target::Command(command.id),
    });
    let tabs = panels.iter().filter(|panel| dock_of(panel).is_some());
    let tabs = tabs.map(|panel| SearchRow {
        text: format!("Window: {}", panel.title),
        key: String::new(),
        can_run: true,
        target: Target::Panel(panel.name),
    });
    let layouts = shell.layouts.iter().map(|layout| SearchRow {
        text: format!("Layout: {}", layout.name),
        key: String::new(),
        can_run: true,
        target: Target::Layout(layout.name),
    });
    let rows = commands.chain(tabs).chain(layouts);
    rows.filter(|row| found(query, &row.text)).collect()
}

/// Does what a row of the command search stands for.
pub(crate) fn run_search_row(
    shell: &mut Shell,
    panels: &mut [Panel],
    world: &mut World,
    row: &SearchRow,
) {
    match row.target {
        Target::Command(id) => {
            crate::queue_command(world, id);
        }
        Target::Panel(name) => {
            for panel in panels.iter_mut().filter(|panel| panel.name == name) {
                panel.open = true;
            }
            shell.focus = Some(name.to_owned());
        }
        Target::Layout(name) => shell.request = Some(name.to_owned()),
    }
}

/// The command search: a text field and the rows for its text, over the top of the
/// picture. Arrow keys and Enter, or a click, run a row. Escape, or a click at
/// another place, closes it.
fn command_search(
    shell: &mut Shell,
    panels: &mut [Panel],
    world: &mut World,
    ctx: &egui::Context,
    top: egui::Pos2,
) {
    let Some((mut query, mut selected)) = shell.search.take() else {
        return;
    };
    let rows = search_rows(shell, panels, world, &query);
    let key = |key| ctx.input(|input| input.key_pressed(key));
    if key(egui::Key::ArrowDown) {
        selected += 1;
    }
    if key(egui::Key::ArrowUp) {
        selected = selected.saturating_sub(1);
    }
    selected = selected.min(rows.len().saturating_sub(1));
    let mut run = key(egui::Key::Enter).then_some(selected);
    let mut open = !key(egui::Key::Escape);
    let area = egui::Area::new(egui::Id::new("command search"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(top + egui::vec2(0.0, 8.0));
    let response = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(420.0);
            let field = egui::TextEdit::singleline(&mut query)
                .hint_text(format!("{} command, panel, or layout", icon::SEARCH))
                .desired_width(f32::INFINITY);
            let field = ui.add(field);
            if field.changed() {
                selected = 0;
            }
            field.request_focus();
            let scroll = egui::ScrollArea::vertical().max_height(320.0);
            scroll.show(ui, |ui| {
                for (index, row) in rows.iter().enumerate() {
                    let button = egui::Button::new(row.text.as_str())
                        .selected(index == selected)
                        .shortcut_text(row.key.as_str());
                    let button = ui.add_enabled(row.can_run, button);
                    if button.clicked() {
                        run = Some(index);
                    }
                }
                if rows.is_empty() {
                    ui.weak("nothing has this text");
                }
            });
        });
    });
    if response.response.clicked_elsewhere() {
        open = false;
    }
    if let Some(row) = run
        .and_then(|index| rows.get(index))
        .filter(|row| row.can_run)
    {
        run_search_row(shell, panels, world, row);
        open = false;
    }
    shell.search = open.then_some((query, selected));
}

/// The "Window" menu: each panel of a dock, and the layouts.
fn window_menu(ui: &mut egui::Ui, shell: &mut Shell, panels: &mut [Panel]) {
    for panel in panels.iter_mut().filter(|panel| dock_of(panel).is_some()) {
        ui.checkbox(&mut panel.open, format!("{} {}", panel.icon, panel.title));
    }
    ui.separator();
    if ui.button("Reset layout").clicked() {
        shell.request = Some(shell.layouts[0].name.to_owned());
        ui.close();
    }
    ui.menu_button("Layout", |ui| {
        for name in shell.layout_names() {
            if ui.radio(shell.current == name, name).clicked() {
                shell.request = Some(name.to_owned());
                ui.close();
            }
        }
    });
}

/// Draws each open panel of a place that is not a dock, one after the other.
fn draw(place: Place, panels: &mut [Panel], ui: &mut egui::Ui, world: &mut World) {
    for panel in panels.iter_mut() {
        if panel.open && panel.place == place {
            (panel.draw)(ui, world);
        }
    }
}

/// Draws the shell in the root of the egui frame. Returns the room that the picture
/// does not have at the four sides of the window, in points: left, right, top,
/// bottom.
pub(crate) fn show(
    shell: &mut Shell,
    panels: &mut [Panel],
    menus: &mut [Menu],
    root: &mut egui::Ui,
    world: &mut World,
) -> [f32; 4] {
    let ctx = root.ctx().clone();
    let window = ctx.content_rect();
    if shell.dock.is_none() {
        let file = shell.file.as_ref();
        let text = file.and_then(|file| std::fs::read_to_string(file).ok());
        // The open state of the default layout first: a panel that the file does
        // not know keeps it.
        let default = shell.layouts[0].name;
        apply(shell, panels, default, window.size());
        match text {
            Some(text) if read_layout(shell, panels, &text) => shell.saved = text,
            _ => {}
        }
    }
    if let Some(name) = shell.request.take() {
        apply(shell, panels, &name, window.size());
        shell.unsaved = true;
    }
    let pressed = |world: &World| {
        let actions = world.get_resource::<ActionState>();
        actions.is_some_and(|actions| actions.just_pressed(COMMAND_SEARCH))
    };
    if pressed(world) && shell.search.is_none() {
        shell.search = Some((String::new(), 0));
    }

    let commands = world.get_resource::<ToolCommands>().cloned();
    let commands = commands.unwrap_or_default();
    egui::Panel::top("tool top").show(root, |ui| {
        ui.horizontal(|ui| {
            // A menu can have a text field: a click in a menu must not close it.
            // Each button closes the menu itself.
            let stay = egui::containers::menu::MenuConfig::new()
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
            let menu =
                |name: &str| egui::containers::menu::MenuButton::new(name).config(stay.clone());
            for (name, extra) in menus.iter_mut() {
                menu(name).ui(ui, |ui| {
                    for command in commands.iter().filter(|command| command.menu == *name) {
                        command_item(ui, world, command);
                    }
                    if let Some(extra) = extra {
                        ui.separator();
                        extra(ui, world);
                    }
                });
            }
            menu("Window").ui(ui, |ui| window_menu(ui, shell, panels));
            if ui
                .button(icon::SEARCH)
                .on_hover_text(format!(
                    "Search a command ({})",
                    crate::key_label(world, COMMAND_SEARCH)
                ))
                .clicked()
            {
                shell.search = Some((String::new(), 0));
            }
        });
        // The tool row: the tools at the left, the layout list at the right, and
        // play and stop in the middle.
        let row = ui.horizontal(|ui| {
            draw(Place::Top, panels, ui, world);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current = shell.current.clone();
                egui::ComboBox::from_id_salt("layout")
                    .selected_text(current.as_str())
                    .show_ui(ui, |ui| {
                        for name in shell.layout_names() {
                            if ui.selectable_label(current == name, name).clicked() {
                                shell.request = Some(name.to_owned());
                            }
                        }
                    })
                    .response
                    .on_hover_text("The layout of the panels");
            });
        });
        in_middle(ui, row.response.rect, |ui| {
            draw(Place::TopMiddle, panels, ui, world)
        });
        ui.add_space(2.0);
    });
    egui::Panel::bottom("tool status").show(root, |ui| {
        ui.horizontal(|ui| draw(Place::Status, panels, ui, world));
    });
    let docks = root.available_rect_before_wrap();

    sync(shell, panels, window.size());
    let mut room = None;
    if let Some(dock) = &mut shell.dock {
        let mut tabs = Tabs {
            panels: &mut *panels,
            world: &mut *world,
            document: &shell.document,
        };
        DockArea::new(dock)
            .style(egui_dock::Style::from_egui(root.style().as_ref()))
            .show_add_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_inside(root, &mut tabs);
        // A tab that the user closed: its panel is not open.
        for panel in tabs
            .panels
            .iter_mut()
            .filter(|panel| dock_of(panel).is_some())
        {
            panel.open = dock.find_tab_from(|tab| tab == panel.name).is_some();
        }
        // The floating windows: a new one gets its size, and each one its place
        // for the layout file.
        let mut windows = Vec::new();
        for (index, surface) in dock.iter_surfaces_mut_indexed() {
            let egui_dock::Surface::Window(_, state) = surface else {
                continue;
            };
            if !shell.windows.iter().any(|(known, _)| *known == index.0) {
                state.set_size(FLOAT_SIZE);
            }
            // The id that `egui_dock` gives to the window of a surface.
            let id = egui::Id::new(format!("window {index:?}"));
            let rect = ctx.memory(|memory| memory.area_rect(id));
            // Before its first frame, a window has no place.
            windows.push((index.0, rect.unwrap_or(egui::Rect::ZERO)));
        }
        shell.unsaved |= windows != shell.windows;
        shell.windows = windows;
        // The body of the picture tab, while it is the tab that its dock shows.
        let shows = |leaf: &egui_dock::LeafNode<String>| {
            let active = leaf.tabs.get(leaf.active.0);
            active.is_some_and(|tab| tab == PICTURE)
        };
        let mut leaves = dock.iter_leaves();
        let picture = leaves.find_map(|(_, leaf)| shows(leaf).then_some(leaf.viewport));
        if let Some(picture) = picture.filter(|picture| picture.is_positive()) {
            shell.picture = Some(picture);
        }
        room = shell.picture;
    }

    command_search(shell, panels, world, &ctx, docks.center_top());
    crate::widgets::show_notices(&ctx, world, docks.right_bottom());

    // The layout file follows the layout. A change of the layout ends with the
    // release of a mouse button (a drag of a tab, a split, or a window), or it is
    // a command. Written when no button is down: not in each frame of a drag.
    let (released, down) = ctx.input(|input| {
        let pointer = &input.pointer;
        (pointer.any_released(), pointer.any_down())
    });
    shell.unsaved |= released;
    if let Some(file) = shell.file.clone().filter(|_| shell.unsaved && !down) {
        shell.unsaved = false;
        let text = layout_text(shell, panels);
        if text != shell.saved {
            match std::fs::write(&file, &text) {
                Ok(()) => tracing::debug!(path = %file.display(), "layout written"),
                Err(error) => tracing::error!("cannot write {}: {error}", file.display()),
            }
            shell.saved = text;
        }
    }

    let picture = room.unwrap_or(docks);
    [
        picture.min.x - window.min.x,
        window.max.x - picture.max.x,
        picture.min.y - window.min.y,
        window.max.y - picture.max.y,
    ]
}
