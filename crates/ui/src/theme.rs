//! The theme: one table for the distances, the text sizes, and the colors that have
//! a meaning, and the icons. A panel takes its values from here, so that the panels
//! of all modules look as one tool.
//!
//! The icons are characters of the symbol fonts that egui has built in
//! (`emoji-icon-font` and `NotoEmoji`; their licenses are in `licenses/`).

use egui::Color32;

pub struct Theme {
    /// The distance between two widgets, and between two rows.
    pub space: [f32; 2],
    /// How far a group in a group moves to the right.
    pub indent: f32,
    /// The text sizes: of a panel, of a note, and of a title.
    pub text: f32,
    pub small: f32,
    pub heading: f32,
    /// The width of the label column of a property grid.
    pub label_width: f32,
    pub error: Color32,
    pub warning: Color32,
    /// The background of a selected row or button.
    pub selection: Color32,
    /// A game runs: the frame of the picture, and the stop button.
    pub play: Color32,
    /// X, Y, and Z: the handles of the gizmo and the marks of a vector.
    pub axes: [Color32; 3],
}

pub const THEME: Theme = Theme {
    space: [6.0, 4.0],
    indent: 14.0,
    text: 13.0,
    small: 10.5,
    heading: 16.0,
    label_width: 118.0,
    error: Color32::from_rgb(255, 110, 90),
    warning: Color32::from_rgb(235, 190, 80),
    selection: Color32::from_rgb(40, 96, 150),
    play: Color32::from_rgb(255, 128, 32),
    axes: [
        Color32::from_rgb(235, 80, 80),
        Color32::from_rgb(90, 210, 90),
        Color32::from_rgb(90, 140, 255),
    ],
};

/// The icons of the tool buttons and of the tabs. One character each.
// Each one is in a font of egui: checked against the character tables of the two
// font files. (`has_glyph` of egui cannot tell for the font that has the
// replacement character.) A character that no font has shows as a box.
pub mod icon {
    pub const PLAY: &str = "▶";
    pub const STOP: &str = "⏹";
    pub const STEP: &str = "⏭";
    pub const MOVE: &str = "↔";
    pub const ROTATE: &str = "↻";
    pub const SCALE: &str = "⛶";
    pub const ADD: &str = "➕";
    pub const RESET: &str = "↺";
    pub const CLOSE: &str = "✖";
    pub const SEARCH: &str = "🔍";
    pub const ERROR: &str = "⛔";
    pub const WARNING: &str = "⚠";
    pub const INFO: &str = "ℹ";
    /// A panel with no icon of its own.
    pub const PANEL: &str = "🗖";
    /// The document tab: a room.
    pub const ROOM: &str = "🏠";
    pub const OUTLINER: &str = "☰";
    pub const INSPECTOR: &str = "🔧";
    pub const EVENTS: &str = "⚡";
    pub const BUILD: &str = "⚙";
    /// A file that the editor makes from a file of another program.
    pub const SOURCE: &str = "↻";
}

/// Gives egui the values of the table. For the layout of a tool: the panels of a
/// game keep the look that they had.
pub(crate) fn apply(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = THEME.space.into();
        style.spacing.indent = THEME.indent;
        style.visuals.selection.bg_fill = THEME.selection;
        style.visuals.error_fg_color = THEME.error;
        style.visuals.warn_fg_color = THEME.warning;
        for (text_style, size, family) in [
            (TextStyle::Body, THEME.text, FontFamily::Proportional),
            (TextStyle::Button, THEME.text, FontFamily::Proportional),
            (TextStyle::Monospace, THEME.text, FontFamily::Monospace),
            (TextStyle::Small, THEME.small, FontFamily::Proportional),
            (TextStyle::Heading, THEME.heading, FontFamily::Proportional),
        ] {
            style
                .text_styles
                .insert(text_style, FontId::new(size, family));
        }
    });
}
