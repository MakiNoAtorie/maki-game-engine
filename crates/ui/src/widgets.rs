//! The widgets of a tool: one form for each job, for the panels of all modules.
//!
//! - The property grid: [`property_grid`] and its rows ([`row`], [`number`],
//!   [`vector`], [`color`], [`text`], [`toggle`], [`choice`], [`optional`]). A label
//!   column and a value column. Each row function returns true in a frame where the
//!   user changed the value.
//! - The tree: [`tree_filter`], [`tree_group`], [`tree_row`].
//! - The tool button: [`tool_button`], for a command of the command list.
//! - The notice: [`notify`]. It shows over a corner of the window and stops no work.
//! - The parts of a status row: [`jobs_label`], [`frame_time_label`].

use std::time::{Duration, Instant};

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use egui::{Color32, DragValue, Response, Ui};

use crate::theme::{THEME, icon};
use crate::{FrameTime, ToolCommands};

// ---- The property grid ----

const GRID_LEFT: &str = "property grid left";

/// A group of rows with one label column: the values of all rows start at the same
/// place, also those of a group in a group.
pub fn property_grid<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = egui::Id::new(GRID_LEFT);
    let left = ui.cursor().left();
    ui.data_mut(|data| data.insert_temp(id, left));
    let result = add(ui);
    ui.data_mut(|data| data.remove::<f32>(id));
    result
}

/// One row: the label in the label column, and what `add` draws in the value
/// column. An empty label takes no room.
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        if !label.is_empty() {
            // Not in a grid: the column starts where this row starts.
            let start = ui.cursor().left();
            let left = ui.data(|data| data.get_temp(egui::Id::new(GRID_LEFT)));
            let width = (left.unwrap_or(start) + THEME.label_width - start).max(40.0);
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(size, layout, |ui| {
                ui.set_min_width(width);
                ui.add(egui::Label::new(label).truncate());
            });
        }
        add(ui)
    })
    .inner
}

/// "Back to default", at the end of a row, while the value is not the default.
/// True: the value was set back.
fn reset<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, default: Option<T>) -> bool {
    let Some(default) = default.filter(|default| default != value) else {
        return false;
    };
    let clicked = ui
        .small_button(icon::RESET)
        .on_hover_text("Back to default")
        .clicked();
    if clicked {
        *value = default;
    }
    clicked
}

/// The widget of a number. A value that is out of the range of the widget stays as
/// it is until the user changes it: a look at a value does not change it.
pub fn drag<Num: egui::emath::Numeric>(value: &mut Num) -> DragValue<'_> {
    DragValue::new(value).clamp_existing_to_range(false)
}

/// The unit of a value, from its label: meters, degrees, seconds. Empty: a count
/// or a factor.
// ponytail: by the words of the labels. A parameter of each widget call if a label
// gets the wrong unit.
fn unit(label: &str) -> &'static str {
    let has = |words: &[&str]| words.iter().any(|word| label.contains(word));
    if has(&["seconds"]) {
        " s"
    } else if has(&["degrees", "rotation"]) {
        " deg"
    } else if has(&[
        "width", "depth", "height", "offset", "size", "radius", "position", "from", "to", "reach",
    ]) {
        " m"
    } else {
        ""
    }
}

/// A number with its unit. Drag it, or click it and type. `default`: the row has
/// "back to default".
pub fn number(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    default: Option<f32>,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    row(ui, label, |ui| {
        let drag = drag(value).speed(speed).range(range).suffix(unit(label));
        ui.add(drag).changed() | reset(ui, value, default)
    })
}

/// Three numbers of X, Y, and Z, each with its letter in the color of its axis.
pub fn vector(
    ui: &mut Ui,
    label: &str,
    value: &mut [f32; 3],
    default: Option<[f32; 3]>,
    speed: f32,
) -> bool {
    row(ui, label, |ui| {
        let mut changed = false;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (axis, part) in value.iter_mut().enumerate() {
                ui.colored_label(THEME.axes[axis], ["X", "Y", "Z"][axis]);
                changed |= ui
                    .add(drag(part).speed(speed).suffix(unit(label)))
                    .changed();
            }
        });
        changed | reset(ui, value, default)
    })
}

/// Linear RGB. The numbers can go above 1 (a surface with a light of its own); the
/// color button shows and sets the part up to 1.
pub fn color(ui: &mut Ui, label: &str, value: &mut [f32; 3]) -> bool {
    row(ui, label, |ui| {
        // On a copy: the button writes the color back also with no change, with the
        // rounding of its color space.
        let mut picked = *value;
        let mut changed = ui.color_edit_button_rgb(&mut picked).changed();
        if changed {
            *value = picked;
        }
        for part in value {
            let drag = drag(part).speed(0.01).range(0.0..=1000.0);
            changed |= ui.add(drag).changed();
        }
        changed
    })
}

/// A text field. The value changes when the field loses the focus, not with each
/// key: half of a file name is not a file.
pub fn text(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    row(ui, label, |ui| {
        let id = ui.make_persistent_id(label);
        // The text while it is typed.
        let typed = ui.ctx().memory(|memory| memory.has_focus(id));
        let typed = typed.then(|| ui.ctx().data(|data| data.get_temp::<String>(id)));
        let mut buffer = typed.flatten().unwrap_or_else(|| value.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut buffer).id(id));
        if response.has_focus() {
            ui.ctx().data_mut(|data| data.insert_temp(id, buffer));
            return false;
        }
        ui.ctx().data_mut(|data| data.remove::<String>(id));
        let changed = response.lost_focus() && buffer != *value;
        if changed {
            *value = buffer;
        }
        changed
    })
}

/// A checkbox, in the value column.
pub fn toggle(ui: &mut Ui, label: &str, value: &mut bool, default: Option<bool>) -> bool {
    row(ui, label, |ui| {
        ui.checkbox(value, "").changed() | reset(ui, value, default)
    })
}

/// A list to choose one option from. `same` tells if the value is that option (for
/// an enum with fields: the same variant).
pub fn choice<T: Clone>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(&str, T)],
    same: impl Fn(&T, &T) -> bool,
) -> bool {
    row(ui, label, |ui| {
        let current = options.iter().find(|(_, option)| same(option, value));
        let mut changed = false;
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.map_or("?", |(name, _)| name))
            .show_ui(ui, |ui| {
                for (name, option) in options {
                    let selected = same(option, value);
                    if ui.selectable_label(selected, *name).clicked() && !selected {
                        *value = option.clone();
                        changed = true;
                    }
                }
            });
        changed
    })
}

/// The same variant of an enum.
pub fn same_kind<T>(a: &T, b: &T) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// A value that a thing can be without: a checkbox, and the rows of the value while
/// it is set.
pub fn optional<T>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    default: impl FnOnce() -> T,
    edit: impl FnOnce(&mut Ui, &mut T) -> bool,
) -> bool {
    let mut set = value.is_some();
    let mut changed = ui.checkbox(&mut set, label).changed();
    if changed {
        *value = set.then(default);
    }
    if let Some(value) = value {
        changed |= ui.indent(label, |ui| edit(ui, value)).inner;
    }
    changed
}

// ---- The tree ----

/// The filter field of a tree.
pub fn tree_filter(ui: &mut Ui, filter: &mut String) {
    let field = egui::TextEdit::singleline(filter)
        .hint_text(format!("{} filter", icon::SEARCH))
        .desired_width(f32::INFINITY);
    ui.add(field);
}

/// True: the row with this label shows with this filter.
pub fn tree_shows(filter: &str, label: &str) -> bool {
    label.to_lowercase().contains(&filter.to_lowercase())
}

/// A group of rows with a title and the count of its rows. It can be folded.
pub fn tree_group(ui: &mut Ui, title: &str, count: usize, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(format!("{title} ({count})"))
        .id_salt(title)
        .default_open(true)
        .show(ui, add);
}

/// One row of a tree.
pub struct TreeRow<'a> {
    pub label: &'a str,
    pub selected: bool,
    /// The color of the label (an error, a warning). `None`: the color of text.
    pub color: Option<Color32>,
    /// Empty: no hover text.
    pub hover: &'a str,
    /// True in one frame: the label becomes a text field (rename in place).
    pub rename: bool,
}

pub struct TreeRowResponse {
    /// Of the row: `clicked`, `double_clicked`, and `context_menu` for the menu of
    /// the right mouse button.
    pub response: Response,
    /// The new name, in the frame where a rename ended with Enter or with a click
    /// at another place. Escape ends it with no name.
    pub renamed: Option<String>,
}

/// One row of a tree: a label that can be selected, and renamed in place. `salt`:
/// the identity of the row among the rows of the tree.
pub fn tree_row(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    row: TreeRow,
) -> TreeRowResponse {
    let id = ui.id().with(("tree row", salt));
    // The row that is renamed, and its text.
    let state = egui::Id::new("tree rename");
    let renamed: Option<(egui::Id, String)> = ui.data(|data| data.get_temp(state));
    let mut buffer = match renamed {
        Some((row_id, buffer)) if row_id == id => Some(buffer),
        _ => None,
    };
    if row.rename {
        buffer = Some(row.label.to_owned());
    }
    let Some(mut buffer) = buffer else {
        let text = egui::RichText::new(row.label);
        let text = row.color.map_or(text.clone(), |color| text.color(color));
        let mut response = ui.selectable_label(row.selected, text);
        if !row.hover.is_empty() {
            response = response.on_hover_text(row.hover);
        }
        return TreeRowResponse {
            response,
            renamed: None,
        };
    };
    let field = egui::TextEdit::singleline(&mut buffer)
        .id(id)
        .desired_width(f32::INFINITY);
    let response = ui.add(field);
    if row.rename {
        response.request_focus();
    }
    let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let done = response.lost_focus() || cancelled;
    ui.data_mut(|data| {
        if done {
            data.remove::<(egui::Id, String)>(state);
        } else {
            data.insert_temp(state, (id, buffer.clone()));
        }
    });
    TreeRowResponse {
        response,
        renamed: (done && !cancelled && buffer != row.label).then_some(buffer),
    }
}

// ---- The tool button ----

/// The button of a command of the command list: `text` is its icon (or an icon and
/// a word), the hover text is the name and the key of the command, and a click
/// runs it. Grey while the command cannot run. `selected`: for a button that shows
/// a state (the tool that is in use).
pub fn tool_button(
    ui: &mut Ui,
    world: &mut World,
    id: &str,
    text: impl Into<egui::WidgetText>,
    selected: bool,
) -> Response {
    let commands = world.resource::<ToolCommands>().clone();
    let Some(command) = commands.get(id) else {
        return ui.colored_label(THEME.error, format!("no command `{id}`"));
    };
    let button = egui::Button::new(text).selected(selected);
    let response = ui
        .add_enabled((command.can_run)(world), button)
        .on_hover_text(command.hover(world));
    if response.clicked() {
        (command.run)(world);
    }
    response
}

// ---- The notice ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warning,
    /// It stays until the user closes it.
    Error,
}

struct Notice {
    level: NoticeLevel,
    text: String,
    since: Instant,
}

/// The notices that show now. Write with [`notify`].
#[derive(Resource, Default)]
pub struct Notices(Vec<Notice>);

impl Notices {
    /// The texts of the notices that show, the oldest first.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|notice| notice.text.as_str())
    }
}

/// How long a notice shows that is not an error.
const NOTICE_TIME: Duration = Duration::from_secs(6);

/// Tells the user a thing, with no window that must be answered: the text shows
/// over the bottom right corner of the window for some seconds (an error: until the
/// user closes it), and the work goes on.
pub fn notify(world: &mut World, level: NoticeLevel, text: impl Into<String>) {
    let notice = Notice {
        level,
        text: text.into(),
        since: Instant::now(),
    };
    world
        .get_resource_or_insert_with(Notices::default)
        .0
        .push(notice);
}

/// Draws the notices. `corner`: the bottom right corner of the room for them.
pub(crate) fn show_notices(ctx: &egui::Context, world: &mut World, corner: egui::Pos2) {
    let Some(mut notices) = world.get_resource_mut::<Notices>() else {
        return;
    };
    // The clock of the wall: a tool pauses the time of the game.
    let shows = |notice: &Notice| {
        notice.level == NoticeLevel::Error || notice.since.elapsed() < NOTICE_TIME
    };
    if !notices.0.iter().all(shows) {
        notices.0.retain(shows);
    }
    if notices.0.is_empty() {
        return;
    }
    let mut closed = None;
    egui::Area::new(egui::Id::new("notices"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(corner - egui::vec2(8.0, 8.0))
        .show(ctx, |ui| {
            for (index, notice) in notices.0.iter().enumerate() {
                let (mark, color) = match notice.level {
                    NoticeLevel::Info => (icon::INFO, ui.visuals().text_color()),
                    NoticeLevel::Warning => (icon::WARNING, THEME.warning),
                    NoticeLevel::Error => (icon::ERROR, THEME.error),
                };
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.horizontal_top(|ui| {
                        if ui
                            .small_button(icon::CLOSE)
                            .on_hover_text("Close")
                            .clicked()
                        {
                            closed = Some(index);
                        }
                        ui.colored_label(color, mark);
                        ui.add(egui::Label::new(notice.text.as_str()).wrap());
                    });
                });
            }
        });
    if let Some(index) = closed {
        notices.0.remove(index);
    }
}

// ---- The parts of a status row ----

/// The jobs that run behind the work of the user (a build, an import, a bake): the
/// module that runs one writes its name here while it runs.
#[derive(Resource, Default)]
pub struct Jobs(pub Vec<String>);

/// The part of a status row with the jobs that run.
pub fn jobs_label(ui: &mut Ui, world: &World) {
    let jobs = world.get_resource::<Jobs>();
    match jobs.map_or(&[][..], |jobs| &jobs.0) {
        [] => ui.weak("no job"),
        jobs => {
            ui.spinner();
            ui.label(jobs.join(", "))
        }
    };
}

/// The part of a status row with the frame time: the time that the frame would
/// have with no wait for the display.
pub fn frame_time_label(ui: &mut Ui, world: &World) {
    if let Some(time) = world.get_resource::<FrameTime>() {
        let fps = 1000.0 / time.mean_ms.max(f32::EPSILON);
        let text = format!("{:.2} ms  {fps:.0} fps", time.mean_ms);
        ui.label(egui::RichText::new(text).monospace());
    }
}
