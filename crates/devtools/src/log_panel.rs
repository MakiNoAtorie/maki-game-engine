//! The log panel: the log lines of the engine, the game, and the scripts.
//!
//! The log setup of the program (a `tracing` layer of `maki-engine`) writes each line
//! into [`LOG`]: a store of the process, not of an app, so the lines stay when the
//! editor loads new code into a new app. A line equal to the one before it is not
//! kept again: its count goes up.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy_ecs::world::World;
use maki_core::{App, Module};
use maki_ui::egui;
use maki_ui::theme::icon;
use maki_ui::{THEME, ToolPanel, ToolPlace, add_tool_panel};
use tracing::Level;

/// The lines that the store keeps; an older line goes.
const LIMIT: usize = 5000;

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub level: Level,
    /// The module that wrote the line: `maki_scene::room`.
    pub target: String,
    pub text: String,
    /// The source file, as the compiler names it, and the line.
    pub place: Option<(PathBuf, u32)>,
    /// How many equal lines came one after the other.
    pub count: u32,
}

/// The log lines of the process.
pub struct LogStore(Mutex<VecDeque<LogLine>>);

/// The log lines of the process: the log setup writes them, the panel reads them.
pub static LOG: LogStore = LogStore(Mutex::new(VecDeque::new()));

impl LogStore {
    /// Adds a line. The count of 1 is not read: an equal line adds to the last one.
    pub fn push(&self, line: LogLine) {
        // A thread that panicked while it held the lock left whole lines.
        let mut lines = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(last) = lines.back_mut()
            && (last.level, &last.target, &last.text, &last.place)
                == (line.level, &line.target, &line.text, &line.place)
        {
            last.count += 1;
            return;
        }
        if lines.len() == LIMIT {
            lines.pop_front();
        }
        lines.push_back(LogLine { count: 1, ..line });
    }

    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|error| error.into_inner()).clear();
    }

    /// The lines that pass a filter, oldest first.
    // ponytail: a copy of the lines in each frame that the panel shows; a cache by
    // the number of pushes if it shows in a profile.
    pub fn lines(&self, filter: &LogFilter) -> Vec<LogLine> {
        let lines = self.0.lock().unwrap_or_else(|error| error.into_inner());
        lines.iter().filter(|line| filter.passes(line)).cloned().collect()
    }
}

/// What the panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct LogFilter {
    /// Shown: errors, warnings, info, debug, trace.
    pub levels: [bool; 5],
    /// The target starts with this. Empty: any.
    pub source: String,
    /// The text has this, in small or capital letters. Empty: any.
    pub search: String,
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            levels: [true; 5],
            source: String::new(),
            search: String::new(),
        }
    }
}

const LEVELS: [Level; 5] = [Level::ERROR, Level::WARN, Level::INFO, Level::DEBUG, Level::TRACE];

impl LogFilter {
    fn passes(&self, line: &LogLine) -> bool {
        let level = LEVELS.iter().position(|level| *level == line.level);
        level.is_some_and(|level| self.levels[level])
            && line.target.starts_with(self.source.trim())
            && line
                .text
                .to_lowercase()
                .contains(&self.search.trim().to_lowercase())
    }
}

/// Opens a source file at a line: the code editor of the editor.
pub type OpenFile = fn(&mut World, &Path, u32);

/// Adds the panel "Log" to a tool. `open`: what a click on the place of a line does.
pub struct LogPanelModule {
    pub open: OpenFile,
}

impl Module for LogPanelModule {
    fn build(&self, app: &mut App) {
        let panel = ToolPanel {
            id: "log",
            title: "Log",
            icon: icon::INFO,
            place: ToolPlace::Bottom,
            open: true,
        };
        let open = self.open;
        let mut filter = LogFilter::default();
        add_tool_panel(app, panel, move |ui, world| {
            if let Some((file, line)) = log_panel(ui, &mut filter) {
                open(world, &file, line);
            }
        });
    }
}

/// The filters, and the lines. `Some`: the place of a line that was clicked.
fn log_panel(ui: &mut egui::Ui, filter: &mut LogFilter) -> Option<(PathBuf, u32)> {
    ui.horizontal(|ui| {
        let names = ["Errors", "Warnings", "Info", "Debug", "Trace"];
        for (shown, name) in filter.levels.iter_mut().zip(names) {
            ui.toggle_value(shown, name);
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut filter.source).hint_text("source").desired_width(140.0));
        ui.label(icon::SEARCH);
        ui.add(egui::TextEdit::singleline(&mut filter.search).hint_text("search").desired_width(180.0));
        if ui.button("Clear").clicked() {
            LOG.clear();
        }
    });
    let lines = LOG.lines(filter);
    let mut open = None;
    let height = ui.text_style_height(&egui::TextStyle::Body);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, height, lines.len(), |ui, rows| {
            for line in &lines[rows] {
                ui.horizontal(|ui| {
                    let (mark, color) = match line.level {
                        Level::ERROR => (icon::ERROR, THEME.error),
                        Level::WARN => (icon::WARNING, THEME.warning),
                        _ => (icon::INFO, ui.visuals().weak_text_color()),
                    };
                    ui.colored_label(color, mark);
                    ui.weak(line.target.as_str());
                    if let Some((file, number)) = &line.place {
                        let name = file.file_name().unwrap_or_default().to_string_lossy();
                        let link = ui.link(format!("{name}:{number}"));
                        if link.on_hover_text(file.display().to_string()).clicked() {
                            open = Some((file.clone(), *number));
                        }
                    }
                    if line.count > 1 {
                        ui.strong(format!("×{}", line.count));
                    }
                    ui.label(line.text.as_str());
                });
            }
        });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: Level, target: &str, text: &str) -> LogLine {
        LogLine {
            level,
            target: target.to_owned(),
            text: text.to_owned(),
            place: Some((PathBuf::from("game/scripts/src/beacon.rs"), 7)),
            count: 1,
        }
    }

    #[test]
    fn equal_lines_are_one_with_a_count_and_the_filters_choose_the_lines() {
        let store = LogStore(Mutex::new(VecDeque::new()));
        store.push(line(Level::INFO, "maki_scene::room", "room loaded"));
        store.push(line(Level::WARN, "maki_game_scripts::beacon", "too dark"));
        store.push(line(Level::WARN, "maki_game_scripts::beacon", "too dark"));
        store.push(line(Level::WARN, "maki_game_scripts::beacon", "too dark"));
        store.push(line(Level::INFO, "maki_scene::room", "room loaded"));
        let all = store.lines(&LogFilter::default());
        let counts: Vec<u32> = all.iter().map(|line| line.count).collect();
        assert_eq!(counts, [1, 3, 1]);

        let only = |filter: LogFilter| -> Vec<String> {
            let lines = store.lines(&filter);
            lines.into_iter().map(|line| line.text).collect()
        };
        let warnings = LogFilter {
            levels: [true, true, false, false, false],
            ..LogFilter::default()
        };
        assert_eq!(only(warnings), ["too dark"]);
        let scene = LogFilter {
            source: "maki_scene".to_owned(),
            ..LogFilter::default()
        };
        assert_eq!(only(scene), ["room loaded", "room loaded"]);
        let search = LogFilter {
            search: " DARK".to_owned(),
            ..LogFilter::default()
        };
        assert_eq!(only(search), ["too dark"]);

        store.clear();
        assert!(store.lines(&LogFilter::default()).is_empty());
    }

    #[test]
    fn the_store_keeps_the_last_lines_only() {
        let store = LogStore(Mutex::new(VecDeque::new()));
        for number in 0..LIMIT + 10 {
            store.push(line(Level::INFO, "a", &number.to_string()));
        }
        let lines = store.lines(&LogFilter::default());
        assert_eq!(lines.len(), LIMIT);
        assert_eq!(lines[0].text, "10");
    }
}
