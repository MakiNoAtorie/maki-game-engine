//! The panel "Timeline", a tab of the bottom dock: the timeline of the selected
//! object, or of a timeline of the room that the pick at the top selects. A row for
//! each track (its object and its channel), its keys as marks on a time ruler, and a
//! play head.
//!
//! - A click or a drag on the ruler moves the play head (scrub). The view shows the
//!   values of the tracks at the play head (`patch_room`: lights and poses); the
//!   document does not change.
//! - A click on a key selects it; a drag moves its time. "Delete key" removes it.
//! - "Delete track" removes the selected track.
//! - "Add key" adds a key to the selected track at the play head, with the value that the
//!   document has (a field), or that the track gives there (a pose).
//! - "Add track": an object and a channel (a pose, or a live field of a component).
//!
//! Each change is one write of the component `timeline`, so one step of the history.
//! The values and the modes of the keys are edited in the inspector.

use bevy_ecs::world::{Mut, World};
use maki_assets::clip::{Channel, Key, KeyValue, Sample, Track, clip_time};
use maki_core::fields::{Value, read};
use maki_core::{App, Fields, Transform};
use maki_scene::{
    ComponentTypes, CurrentRoom, ObjectId, PoseDef, RoomDef, Timeline, TimelineTrack, patch_room,
    pose_on_rest, set_channel,
};
use maki_ui::egui::{self, Pos2, Rect, Sense, Ui, pos2, vec2};
use maki_ui::theme::icon;
use maki_ui::{THEME, ToolPanel, ToolPlace, add_tool_panel};

use crate::event_panel::picker;
use crate::widgets::{object_names, object_pick};
use crate::{Document, Editor, Item};

/// The id of the panel: the layout "Animate" shows it.
pub(crate) const TIMELINE: &str = "timeline";
/// The width of the column of the track names, in points.
const NAMES: f32 = 180.0;
const RULER: f32 = 18.0;
const ROW: f32 = 20.0;
/// A press nearer than this to a key mark takes the key, in points.
const HIT: f32 = 6.0;
const POSES: [&str; 3] = ["Position", "Rotation", "Scale"];

/// The place of the ruler in the panel: time to points and back.
#[derive(Debug, Clone, Copy)]
struct Ruler {
    area: Rect,
    /// The seconds that the width shows.
    span: f32,
}

impl Ruler {
    fn left(&self) -> f32 {
        self.area.left() + NAMES
    }

    fn width(&self) -> f32 {
        (self.area.right() - 8.0 - self.left()).max(1.0)
    }

    fn x(&self, time: f32) -> f32 {
        self.left() + time / self.span * self.width()
    }

    fn time(&self, x: f32) -> f32 {
        ((x - self.left()) / self.width() * self.span).clamp(0.0, self.span)
    }

    fn row_y(&self, track: usize) -> f32 {
        self.area.top() + RULER + ROW * (track as f32 + 0.5)
    }

    fn row_at(&self, at: Pos2, rows: usize) -> Option<usize> {
        let y = at.y - self.area.top() - RULER;
        (y >= 0.0)
            .then(|| (y / ROW) as usize)
            .filter(|row| *row < rows)
    }

    fn key_at(&self, at: Pos2, timeline: &Timeline) -> Option<(usize, usize)> {
        let mut tracks = timeline.tracks.iter().enumerate();
        tracks.find_map(|(track, of)| {
            let near = |key: &Key| (at - pos2(self.x(key.time), self.row_y(track))).length() <= HIT;
            Some((track, of.track.keys.iter().position(near)?))
        })
    }
}

/// The seconds that the ruler shows. A timeline with a `length` shows it. Else the
/// last key and a quarter more, so that a key can go past the last one.
fn view_span(timeline: &Timeline) -> f32 {
    match timeline.length > 0.0 {
        true => timeline.length,
        false => (timeline.duration() * 1.25).max(1.0),
    }
}

/// What the pointer drags.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Head,
    /// A key, and its time now: the document gets it at the end of the drag.
    Key {
        track: usize,
        key: usize,
        time: f32,
    },
}

/// The state of the panel. Nothing of it is in the document.
#[derive(Debug, Default)]
struct TimelineUi {
    /// The timeline that the panel shows while the selection has none.
    picked: Option<ObjectId>,
    /// The index of the object of the timeline in the frame before.
    shown: Option<usize>,
    /// The play head, in seconds.
    head: f32,
    track: Option<usize>,
    /// A key of `track`.
    key: Option<usize>,
    drag: Option<Drag>,
    /// True after a move of the play head: the view shows the timeline there.
    scrubbed: bool,
    /// True: the view shows the document again in the next frame.
    restore: bool,
    add_object: ObjectId,
    add_channel: String,
    /// Why the last "Key" made no key.
    note: String,
    ruler: Option<Ruler>,
}

impl TimelineUi {
    fn stop_scrub(&mut self) {
        self.restore |= std::mem::take(&mut self.scrubbed);
    }

    /// The room that the view must show: the timeline at the play head while it is
    /// scrubbed, or the document once after that.
    fn preview(&mut self, document: &Document, types: &ComponentTypes) -> Option<RoomDef> {
        if let (true, Some(own)) = (self.scrubbed, self.shown)
            && let Some(Ok(timeline)) = timeline_of(document, own, types)
        {
            return Some(sampled_room(document, own, &timeline, self.head, types));
        }
        std::mem::take(&mut self.restore).then(|| document.room.clone())
    }
}

/// Adds the panel to the bottom dock.
pub(crate) fn add(app: &mut App) {
    let panel = ToolPanel {
        id: TIMELINE,
        title: "Timeline",
        icon: icon::PANEL,
        place: ToolPlace::Bottom,
        open: true,
    };
    let mut state = TimelineUi::default();
    add_tool_panel(app, panel, move |ui, world| {
        world.resource_scope(|world, mut editor: Mut<Editor>| {
            if editor.playing() {
                state.scrubbed = false;
                ui.weak("A game runs.");
                return;
            }
            let no_types = ComponentTypes::default();
            let types = world.get_resource::<ComponentTypes>().unwrap_or(&no_types);
            let primary = editor.primary();
            if timeline_ui(ui, &mut state, &mut editor.document, &primary, types) {
                editor.document.dirty = true;
            }
            let room = state.preview(&editor.document, types);
            if let Some(room) = room {
                show(world, &editor.document, room);
            }
        });
    });
}

/// Patches the view to `room` when it shows another one. A patch that cannot be made
/// (a room that loads) is tried again in the next frame.
fn show(world: &mut World, document: &Document, room: RoomDef) {
    if world.resource::<CurrentRoom>().def == room {
        return;
    }
    let mut shown = document.clone();
    shown.room = room;
    let text = shown.text();
    patch_room(world, &shown.room, text);
}

fn timeline_of(
    document: &Document,
    index: usize,
    types: &ComponentTypes,
) -> Option<Result<Timeline, String>> {
    let tree = document.component_tree(index, Timeline::NAME, types)?;
    Some(tree.and_then(|tree| read(tree).map_err(|error| error.to_string())))
}

/// The index of the object of a track. Object 0 is the object of the timeline.
fn object_index(room: &RoomDef, own: usize, object: ObjectId) -> Option<usize> {
    if object.0 == 0 {
        return Some(own);
    }
    room.find(&object).map(|(index, _)| index)
}

/// The panel. True: the user changed the document.
fn timeline_ui(
    ui: &mut Ui,
    state: &mut TimelineUi,
    document: &mut Document,
    primary: &Item,
    types: &ComponentTypes,
) -> bool {
    let has_timeline = |index: usize| {
        let object = document.room.objects.get(index);
        object.is_some_and(|object| object.components.contains_key(Timeline::NAME))
    };
    let timelines: Vec<(ObjectId, String)> = object_names(&document.room.objects)
        .into_iter()
        .filter(|(id, _)| document.room.find(id).is_some_and(|(i, _)| has_timeline(i)))
        .collect();
    let own = match primary {
        Item::Object(index) if has_timeline(*index) => {
            state.picked = document.room.objects[*index].id;
            Some(*index)
        }
        _ => {
            if timelines.is_empty() {
                ui.weak(
                    "No timeline in the room. Select an object and add the component \
                     \"timeline\" in the Inspector.",
                );
                state.stop_scrub();
                return false;
            }
            // The panel state, not the document: the first timeline until one is picked.
            let mut picked = state
                .picked
                .filter(|id| timelines.iter().any(|(other, _)| other == id))
                .unwrap_or(timelines[0].0);
            let names: Vec<String> = timelines.iter().map(|(_, name)| name.clone()).collect();
            ui.horizontal(|ui| {
                ui.label("Timeline");
                object_pick(&mut picked, &timelines, |text| {
                    picker(ui, "timeline pick", text, &names, &[], false)
                });
            });
            state.picked = Some(picked);
            document.room.find(&picked).map(|(index, _)| index)
        }
    };
    let Some(own) = own else {
        return false;
    };
    if state.shown != Some(own) {
        *state = TimelineUi {
            picked: state.picked,
            shown: Some(own),
            restore: state.scrubbed || state.restore,
            add_channel: POSES[0].to_owned(),
            ..TimelineUi::default()
        };
    }
    let before = match timeline_of(document, own, types) {
        Some(Ok(timeline)) => timeline,
        Some(Err(error)) => {
            let text = format!("The timeline component is invalid: {error}");
            ui.colored_label(THEME.error, text);
            ui.weak("Fix it in the Inspector, or undo (Ctrl+Z).");
            return false;
        }
        None => return false,
    };
    let mut timeline = before.clone();
    // A key or a track that undo removed is not selected.
    state.track = state.track.filter(|track| *track < timeline.tracks.len());
    let keys = |track: usize| timeline.tracks[track].track.keys.len();
    state.key = state
        .key
        .filter(|key| state.track.is_some_and(|t| *key < keys(t)));

    let track = state.track;
    tool_row(ui, state, document, own, &mut timeline, types);
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| ruler(ui, state, &mut timeline, document, own));
    if state.track != track {
        state.note.clear();
    }

    if timeline == before {
        return false;
    }
    document
        .set_component_tree(own, Timeline::NAME, &timeline.tree(), types)
        .is_ok()
}

/// "Add key", "Delete key", "Delete track", the play head time, and "Add track".
fn tool_row(
    ui: &mut Ui,
    state: &mut TimelineUi,
    document: &Document,
    own: usize,
    timeline: &mut Timeline,
    types: &ComponentTypes,
) {
    ui.horizontal(|ui| {
        let key = ui
            .add_enabled(state.track.is_some(), egui::Button::new("Add key"))
            .on_hover_text(
                "Add a key at the play head on the selected track, with the value of now. \
                 A key at the same time gets the new value.",
            )
            .on_disabled_hover_text("Select a track first.");
        if key.clicked()
            && let Some(track) = state.track
        {
            match key_value(document, own, timeline, track, state.head, types) {
                Some(value) => {
                    state.key = Some(set_key(
                        &mut timeline.tracks[track].track,
                        state.head,
                        value,
                    ));
                    state.note.clear();
                }
                None => {
                    state.note = "The object of the track has no such value. Check that it \
                                  has the component."
                        .to_owned()
                }
            }
        }
        let delete = ui
            .add_enabled(state.key.is_some(), egui::Button::new("Delete key"))
            .on_hover_text("Delete the selected key");
        if delete.clicked()
            && let (Some(track), Some(key)) = (state.track, state.key.take())
        {
            timeline.tracks[track].track.keys.remove(key);
        }
        let delete = ui
            .add_enabled(state.track.is_some(), egui::Button::new("Delete track"))
            .on_hover_text("Delete the selected track with its keys");
        if delete.clicked()
            && let Some(track) = state.track.take()
        {
            timeline.tracks.remove(track);
            state.key = None;
        }
        let head = egui::DragValue::new(&mut state.head)
            .range(0.0..=view_span(timeline))
            .speed(0.01)
            .max_decimals(2)
            .suffix(" s");
        if ui.add(head).on_hover_text("Play head time").changed() {
            state.scrubbed = true;
        }
        if state.scrubbed
            && ui
                .button("Back to document")
                .on_hover_text(format!(
                    "The view shows the timeline at {:.2} s. Show the document.",
                    state.head
                ))
                .clicked()
        {
            state.stop_scrub();
        }
        if !state.note.is_empty() {
            ui.colored_label(THEME.warning, &state.note);
        }
        ui.separator();
        // The object 0 is the object of the timeline.
        let mut objects = vec![(ObjectId(0), "this object".to_owned())];
        objects.extend(object_names(&document.room.objects));
        let names: Vec<String> = objects.iter().map(|(_, name)| name.clone()).collect();
        ui.label("Object");
        object_pick(&mut state.add_object, &objects, |text| {
            picker(ui, "timeline object", text, &names, &[], false)
        });
        let channels = object_index(&document.room, own, state.add_object)
            .map(|index| channels(document, index, types))
            .unwrap_or_default();
        // A channel of the object picked before: the first one of this object.
        if !channels.contains(&state.add_channel)
            && let Some(first) = channels.first()
        {
            state.add_channel = first.clone();
        }
        ui.label("Channel");
        picker(
            ui,
            "timeline channel",
            &mut state.add_channel,
            &channels,
            &[],
            false,
        );
        let can_add = channels.contains(&state.add_channel);
        if ui
            .add_enabled(can_add, egui::Button::new("Add track"))
            .on_hover_text("Add a track that animates the channel of the object")
            .clicked()
        {
            timeline.tracks.push(TimelineTrack {
                object: state.add_object,
                track: Track {
                    channel: channel_of(&state.add_channel),
                    ..Track::default()
                },
            });
            (state.track, state.key) = (Some(timeline.tracks.len() - 1), None);
        }
    });
}

/// The rows of the tracks on the time ruler, and their input.
fn ruler(
    ui: &mut Ui,
    state: &mut TimelineUi,
    timeline: &mut Timeline,
    document: &Document,
    own: usize,
) {
    let rows = timeline.tracks.len();
    let size = vec2(
        ui.available_width().max(NAMES + 100.0),
        RULER + ROW * rows.max(1) as f32,
    );
    let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let span = view_span(timeline);
    // A deleted or moved last key shortens the span: the play head stays in view.
    state.head = state.head.min(span);
    let ruler = Ruler { area, span };
    state.ruler = Some(ruler);

    if response.drag_started()
        && let Some(origin) = ui.input(|input| input.pointer.press_origin())
    {
        state.drag = match ruler.key_at(origin, timeline) {
            Some((track, key)) => {
                (state.track, state.key) = (Some(track), Some(key));
                let time = timeline.tracks[track].track.keys[key].time;
                Some(Drag::Key { track, key, time })
            }
            // A drag in the column of the names moves nothing.
            None if origin.x >= ruler.left() => Some(Drag::Head),
            None => None,
        };
    }
    if response.dragged()
        && let Some(at) = response.interact_pointer_pos()
    {
        match &mut state.drag {
            Some(Drag::Head) => {
                state.head = ruler.time(at.x);
                state.scrubbed = true;
            }
            Some(Drag::Key { time, .. }) => *time = ruler.time(at.x),
            None => {}
        }
    }
    if response.drag_stopped()
        && let Some(Drag::Key { track, key, time }) = state.drag.take()
    {
        state.key = Some(move_key(
            &mut timeline.tracks[track].track,
            key,
            time,
            timeline.fps,
        ));
    }
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        match ruler.key_at(at, timeline) {
            Some((track, key)) => (state.track, state.key) = (Some(track), Some(key)),
            None => {
                if let Some(track) = ruler.row_at(at, rows) {
                    (state.track, state.key) = (Some(track), None);
                }
                if at.x >= ruler.left() {
                    state.head = ruler.time(at.x);
                    state.scrubbed = true;
                }
            }
        }
    }
    draw(ui, state, &ruler, timeline, document, own);
}

fn draw(
    ui: &Ui,
    state: &TimelineUi,
    ruler: &Ruler,
    timeline: &Timeline,
    document: &Document,
    own: usize,
) {
    let painter = ui.painter_at(ruler.area);
    let visuals = ui.visuals();
    let font = egui::FontId::proportional(THEME.small);
    let text_color = visuals.text_color();
    let line = egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);
    let area = ruler.area;
    painter.rect_filled(
        Rect::from_min_max(area.min, pos2(area.right(), area.top() + RULER)),
        0.0,
        visuals.faint_bg_color,
    );
    // A tick with its time, about ten over the width.
    let step = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
        .into_iter()
        .find(|step| ruler.span / step <= 10.0)
        .unwrap_or(ruler.span / 10.0);
    // A product, not a sum: a sum of 0.1 drifts in the labels.
    let ticks = (ruler.span / step + 1e-3) as usize;
    for tick in (0..=ticks).map(|index| index as f32 * step) {
        let x = ruler.x(tick);
        painter.vline(x, area.top()..=area.top() + RULER, line);
        let at = pos2(x + 2.0, area.top() + RULER / 2.0);
        painter.text(
            at,
            egui::Align2::LEFT_CENTER,
            format!("{tick}"),
            font.clone(),
            text_color,
        );
    }
    if timeline.tracks.is_empty() {
        painter.text(
            pos2(area.center().x, area.top() + RULER + ROW / 2.0),
            egui::Align2::CENTER_CENTER,
            "No tracks. Pick an object and a channel, then press Add track.",
            font.clone(),
            visuals.weak_text_color(),
        );
    }
    // The names stay in their column: a long one does not run over the keys.
    let names_painter = painter.with_clip_rect(Rect::from_min_max(
        area.min,
        pos2(ruler.left() - 4.0, area.bottom()),
    ));
    let names = object_names(&document.room.objects);
    for (index, of) in timeline.tracks.iter().enumerate() {
        let y = ruler.row_y(index);
        if state.track == Some(index) {
            let row = Rect::from_center_size(pos2(area.center().x, y), vec2(area.width(), ROW));
            painter.rect_filled(row, 0.0, THEME.selection.gamma_multiply(0.25));
        }
        painter.hline(area.left()..=area.right(), y + ROW / 2.0, line);
        let object = match object_index(&document.room, own, of.object) {
            Some(index) if index == own => document.room.objects[own].name.clone(),
            _ => None,
        };
        let object = object.unwrap_or_else(|| crate::widgets::object_label(&of.object, &names));
        let label = format!("{object} {}", channel_label(&of.track));
        let at = pos2(area.left() + 4.0, y);
        names_painter.text(
            at,
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            text_color,
        );
        for (key_index, key) in of.track.keys.iter().enumerate() {
            let time = match state.drag {
                Some(Drag::Key { track, key, time }) if (track, key) == (index, key_index) => time,
                _ => key.time,
            };
            let selected = state.track == Some(index) && state.key == Some(key_index);
            let fill = if selected {
                THEME.selection
            } else {
                text_color
            };
            painter.add(diamond(pos2(ruler.x(time), y), HIT - 1.0, fill));
        }
    }
    let x = ruler.x(state.head);
    painter.vline(x, area.y_range(), egui::Stroke::new(2.0, THEME.play));
}

fn diamond(center: Pos2, radius: f32, fill: egui::Color32) -> egui::Shape {
    let points = [
        vec2(0.0, -radius),
        vec2(radius, 0.0),
        vec2(0.0, radius),
        vec2(-radius, 0.0),
    ];
    let points = points.iter().map(|offset| center + *offset).collect();
    egui::Shape::convex_polygon(points, fill, egui::Stroke::NONE)
}

/// "Position", "Rotation", "Scale", or "component.field"; a joint after it.
fn channel_label(track: &Track) -> String {
    let channel = match &track.channel {
        Channel::Position => POSES[0].to_owned(),
        Channel::Rotation => POSES[1].to_owned(),
        Channel::Scale => POSES[2].to_owned(),
        Channel::Field { component, field } => format!("{component}.{field}"),
    };
    match &track.target {
        Some(joint) => format!("{channel} (joint {joint})"),
        None => channel,
    }
}

/// The channel of a text of [`channels`].
fn channel_of(text: &str) -> Channel {
    match text {
        "Position" => Channel::Position,
        "Rotation" => Channel::Rotation,
        "Scale" => Channel::Scale,
        _ => {
            let (component, field) = text.split_once('.').unwrap_or((text, ""));
            Channel::Field {
                component: component.to_owned(),
                field: field.to_owned(),
            }
        }
    }
}

/// The channels that a track can have on object `index`: the pose, and each live field
/// of its components (what a track writes in the game).
fn channels(document: &Document, index: usize, types: &ComponentTypes) -> Vec<String> {
    let mut list: Vec<String> = POSES.map(str::to_owned).into();
    let Some(object) = document.room.objects.get(index) else {
        return list;
    };
    for name in object.components.keys() {
        let Some(live) = types.get(name).and_then(|of_type| of_type.live) else {
            continue;
        };
        let fields: Vec<String> = match live.fields {
            Some(fields) => fields.iter().map(|field| (*field).to_owned()).collect(),
            None => match document.component_tree(index, name, types) {
                Some(Ok(Value::Object(tree))) => tree.keys().cloned().collect(),
                _ => Vec::new(),
            },
        };
        list.extend(fields.iter().map(|field| format!("{name}.{field}")));
    }
    list
}

/// The value of a new key of track `track` at `time`. A field: its value in the
/// document. A pose: what the track gives there (the pose of the document, so no
/// change, where it has no keys). `None`: the object or the field is not there.
fn key_value(
    document: &Document,
    own: usize,
    timeline: &Timeline,
    track: usize,
    time: f32,
    types: &ComponentTypes,
) -> Option<KeyValue> {
    let of = &timeline.tracks[track];
    let index = object_index(&document.room, own, of.object)?;
    if let Channel::Field { component, field } = &of.track.channel {
        let tree = document.component_tree(index, component, types)?.ok()?;
        return value_of(tree.get(field)?);
    }
    let t = clip_time(time, timeline.duration(), false, timeline.fps);
    Some(match of.track.sample(t, timeline.mode) {
        Some(Sample::Vector(vector)) => KeyValue::Vector(vector.into()),
        Some(Sample::Rotation(rotation)) => {
            let transform = Transform {
                rotation,
                ..Transform::IDENTITY
            };
            KeyValue::Vector(PoseDef::of(&transform).rotation)
        }
        _ if of.track.channel == Channel::Scale => KeyValue::Vector([1.0; 3]),
        _ => KeyValue::Vector([0.0; 3]),
    })
}

/// A field value as a key value. `None`: a value that a key cannot hold.
fn value_of(tree: &Value) -> Option<KeyValue> {
    Some(match tree {
        Value::Number(number) => KeyValue::Number(number.as_f64()? as f32),
        Value::Bool(on) => KeyValue::Switch(*on),
        Value::String(text) => KeyValue::Text(text.clone()),
        Value::Array(items) if items.len() == 3 => {
            let number = |index: usize| items[index].as_f64().map(|n| n as f32);
            KeyValue::Vector([number(0)?, number(1)?, number(2)?])
        }
        _ => return None,
    })
}

/// Moves a key to `time` (on the frames of `fps`, if the timeline has them). A key
/// that is there already is replaced, as `set_key` does. The new index of the key.
fn move_key(track: &mut Track, key: usize, time: f32, fps: Option<f32>) -> usize {
    let time = fps
        .filter(|fps| *fps > 0.0)
        .map_or(time, |fps| (time * fps).round() / fps);
    track.keys[key].time = time;
    // The key stays where it is in the list; a key of the same time goes.
    let mut at = key;
    for other in (0..track.keys.len()).rev() {
        if other != key && (track.keys[other].time - time).abs() < 1e-4 {
            track.keys.remove(other);
            at -= usize::from(other < key);
        }
    }
    at
}

/// Sets the key at `time`: the value of a key there changes, else a new key. Its index.
fn set_key(track: &mut Track, time: f32, value: KeyValue) -> usize {
    if let Some(index) = track
        .keys
        .iter()
        .position(|key| (key.time - time).abs() < 1e-4)
    {
        track.keys[index].value = value;
        return index;
    }
    track.keys.push(Key { time, value });
    track.keys.len() - 1
}

/// The document room with the values of the timeline at `head`: the fields of lights
/// and the poses, which `patch_room` shows with no load. A pose track is relative to
/// the pose of the document, as in the game.
fn sampled_room(
    document: &Document,
    own: usize,
    timeline: &Timeline,
    head: f32,
    types: &ComponentTypes,
) -> RoomDef {
    let mut shown = document.clone();
    let t = clip_time(head, timeline.duration(), false, timeline.fps);
    let mut poses: Vec<(usize, Transform)> = Vec::new();
    for of in &timeline.tracks {
        let (Some(index), None) = (
            object_index(&document.room, own, of.object),
            &of.track.target,
        ) else {
            continue;
        };
        let Some(sample) = of.track.sample(t, timeline.mode) else {
            continue;
        };
        // Only a light: `patch_room` changes no other component.
        if let (Channel::Field { component, field }, Sample::Value(value)) =
            (&of.track.channel, &sample)
        {
            if component == "light"
                && let Some(Ok(mut tree)) = shown.component_tree(index, component, types)
            {
                tree[field.as_str()] = value.clone();
                // A value that the light does not take: the view keeps the old one.
                let _ = shown.set_component_tree(index, component, &tree, types);
            }
            continue;
        }
        let at = match poses.iter().position(|(object, _)| *object == index) {
            Some(at) => at,
            None => {
                poses.push((index, Transform::IDENTITY));
                poses.len() - 1
            }
        };
        // The key math of the game: the preview cannot drift from it.
        set_channel(&mut poses[at].1, &of.track.channel, sample);
    }
    for (index, key) in poses {
        if key == Transform::IDENTITY {
            continue;
        }
        let rest = document.room.objects[index].pose.transform();
        shown.room.objects[index].pose = PoseDef::of(&pose_on_rest(&rest, &key));
    }
    shown.room
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_key_dropped_on_another_key_replaces_it() {
        let key = |time: f32| Key {
            time,
            value: KeyValue::Number(time),
        };
        let mut track = Track {
            keys: vec![key(0.0), key(1.0), key(2.0)],
            ..Track::default()
        };
        let at = move_key(&mut track, 0, 1.00001, None);
        assert_eq!(track.keys.len(), 2);
        assert_eq!(track.keys[at].value, KeyValue::Number(0.0));
        let at = move_key(&mut track, 0, 0.26, Some(4.0));
        assert_eq!(track.keys[at].time, 0.25);
        assert_eq!(view_span(&Timeline::default()), 1.0);
    }

    use super::*;
    use crate::History;
    use maki_core::ConfigFile;
    use maki_events::{CommandRegistry, EventsModule};
    use maki_platform::PlatformModule;
    use maki_scene::{AnimationModule, SceneModule};

    /// A lamp, a cube, and a timeline with one track on the intensity of the lamp.
    fn room() -> String {
        maki_scene::room_text(
            6.0,
            6.0,
            3.0,
            &[
                r#"(id: 1, name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0)})"#,
                r#"(id: 2, name: "cut", components: {"timeline": (length: 2.0, tracks: [(object: 1, track: (channel: Field(component: "light", field: "intensity"), mode: Some(Linear), keys: [(time: 0.0, value: Number(1.0)), (time: 1.0, value: Number(3.0))]))])})"#,
                r#"(id: 3, name: "cube", components: {"prop": (mesh: Cube(1.0))})"#,
            ],
        )
    }

    /// The panel in a headless egui frame, on a document with its history.
    struct Rig {
        app: App,
        context: egui::Context,
        state: TimelineUi,
        document: Document,
        history: History,
        original: RoomDef,
    }

    /// What one frame drew.
    struct Drawn {
        texts: Vec<(String, Rect)>,
        diamonds: usize,
    }

    impl Rig {
        fn new() -> Self {
            let mut app = App::new(ConfigFile::default()).unwrap();
            app.add_module(PlatformModule)
                .add_module(SceneModule)
                .add_module(AnimationModule)
                .add_module(EventsModule);
            let registry = app.world().resource::<CommandRegistry>().clone();
            let path = std::path::Path::new("l64f_timeline.ron");
            let document = Document::parse(path, &room(), &registry).unwrap();
            let mut history = History::default();
            history.reset(&document);
            Self {
                app,
                context: egui::Context::default(),
                state: TimelineUi::default(),
                original: document.room.clone(),
                document,
                history,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> Drawn {
            let Self {
                app,
                context,
                state,
                document,
                history,
                ..
            } = self;
            let types = app.world().resource::<ComponentTypes>();
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 400.0))),
                ..Default::default()
            };
            let mut down = false;
            let mut output = context.run_ui(input, |ui| {
                if timeline_ui(ui, state, document, &Item::Object(1), types) {
                    document.dirty = true;
                }
                down = ui.input(|input| input.pointer.any_down());
            });
            output.textures_delta.clear();
            // As `run_commands`: no step while a button is down.
            history.track(document, &[], down);
            let mut drawn = Drawn {
                texts: Vec::new(),
                diamonds: 0,
            };
            for shape in &output.shapes {
                match &shape.shape {
                    egui::Shape::Text(text) => {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        drawn.texts.push((text.galley.text().to_owned(), rect));
                    }
                    egui::Shape::Path(path) if path.points.len() == 4 && path.closed => {
                        drawn.diamonds += 1;
                    }
                    _ => {}
                }
            }
            drawn
        }

        /// A press and a release at `at`, in two frames.
        fn click(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at), button(at, true)]);
            self.frame(vec![button(at, false)]);
            self.frame(Vec::new());
        }

        fn click_text(&mut self, text: &str) {
            let drawn = self.frame(Vec::new());
            let found = drawn.texts.iter().find(|(shown, _)| shown == text);
            let (_, rect) = found.unwrap_or_else(|| panic!("no {text}: {:?}", drawn.texts));
            self.click(rect.center());
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(from), button(from, true)]);
            let middle = from + (to - from) / 2.0;
            self.frame(vec![egui::Event::PointerMoved(middle)]);
            self.frame(vec![egui::Event::PointerMoved(to)]);
            self.frame(vec![button(to, false)]);
            self.frame(Vec::new());
        }

        fn at(&self, track: usize, time: f32) -> Pos2 {
            let ruler = self.state.ruler.unwrap();
            pos2(ruler.x(time), ruler.row_y(track))
        }

        fn timeline(&self) -> Timeline {
            let types = self.app.world().resource::<ComponentTypes>();
            timeline_of(&self.document, 1, types).unwrap().unwrap()
        }
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn the_panel_shows_the_tracks_and_their_keys_and_writes_nothing() {
        let mut rig = Rig::new();
        rig.frame(Vec::new());
        let drawn = rig.frame(Vec::new());
        let texts: Vec<&str> = drawn.texts.iter().map(|(text, _)| text.as_str()).collect();
        assert!(texts.contains(&"lamp light.intensity"), "{texts:?}");
        assert_eq!(drawn.diamonds, 2);
        assert!(!rig.document.dirty);
        assert_eq!(rig.document.room, rig.original);
        assert_eq!(rig.history.steps(), (0, 0));
    }

    #[test]
    fn key_drag_delete_and_add_track_are_each_one_step_of_the_history() {
        let mut rig = Rig::new();
        rig.frame(Vec::new());
        // A click on the row at 0.5 s: the play head moves, the track is selected.
        let at = rig.at(0, 0.5);
        rig.click(pos2(at.x, at.y + 4.0));
        assert!((rig.state.head - 0.5).abs() < 0.01, "{}", rig.state.head);
        assert_eq!(rig.state.track, Some(0));
        assert_eq!(rig.history.steps(), (0, 0));

        // "Key": the intensity of the lamp in the document, at the play head.
        rig.click_text("Add key");
        let keys = rig.timeline().tracks[0].track.keys.clone();
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[2].value, KeyValue::Number(5.0));
        assert!((keys[2].time - rig.state.head).abs() < 1e-6);
        assert_eq!(rig.history.steps().0, 1);

        // A drag of the key at 1 s to 1.5 s.
        rig.drag(rig.at(0, 1.0), rig.at(0, 1.5));
        let time = rig.timeline().tracks[0].track.keys[1].time;
        assert!((time - 1.5).abs() < 0.01, "{time}");
        assert_eq!(rig.history.steps().0, 2);

        // A click selects the key at 0 s; "Delete key" removes it.
        rig.click(rig.at(0, 0.0));
        assert_eq!(rig.state.key, Some(0));
        rig.click_text("Delete key");
        let keys = rig.timeline().tracks[0].track.keys.clone();
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().all(|key| key.time > 0.1));
        assert_eq!(rig.history.steps().0, 3);

        // "Add track": the position of the cube.
        rig.state.add_object = ObjectId(3);
        rig.click_text("Add track");
        let tracks = rig.timeline().tracks;
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            (tracks[1].object, &tracks[1].track.channel),
            (ObjectId(3), &Channel::Position)
        );
        assert_eq!(rig.history.steps().0, 4);

        // Undo takes back each step.
        let mut selection = Vec::new();
        for _ in 0..4 {
            assert!(rig.history.undo(&mut rig.document, &mut selection));
        }
        assert_eq!(rig.document.room, rig.original);
    }

    #[test]
    fn a_scrub_shows_the_light_at_the_play_head() {
        let rig = Rig::new();
        let types = rig.app.world().resource::<ComponentTypes>();
        let room = sampled_room(&rig.document, 1, &rig.timeline(), 0.5, types);
        let light = room.objects[0].light().unwrap().unwrap();
        assert!((light.intensity - 2.0).abs() < 1e-5, "{}", light.intensity);
        // The document is as it was.
        assert_eq!(rig.document.room, rig.original);
    }
}
