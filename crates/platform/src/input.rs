//! Input actions. Gameplay code reads `ActionState`, never keys.

use std::collections::{HashMap, HashSet};

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};
use maki_core::{App, Settings};
use serde::{Deserialize, Serialize};

/// A thing that the player can ask for: `Action("interact")`. The text is the
/// identity: two actions with the same text are the same action.
///
/// The set of actions is open. The module that reads an action defines it as a
/// constant and gives it its default keys with [`add_bindings`]; this crate knows no
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Action(pub &'static str);

/// A physical input that a binding can use: a key, a mouse button, one direction
/// of the mouse wheel, a button of a pad, or one direction of a stick. Letter keys
/// are positions on a US keyboard, so W/A/S/D keep their shape on other layouts.
///
/// One row of the table for each input: the name, the winit key codes that give it
/// (none for an input of the mouse or a pad), and the text for the user, in the
/// part of its kind: keyboard, pointer (the mouse), or pad. A new key is a new row.
macro_rules! keys {
    (
        keyboard { $($key:ident [$($code:ident),*] $label:literal,)* }
        pointer { $($pointer:ident $pointer_label:literal,)* }
        pad { $($pad:ident $pad_label:literal,)* }
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($key,)*
            $($pointer,)*
            $($pad,)*
        }

        impl Key {
            /// Name to show to the user.
            pub fn label(self) -> &'static str {
                match self {
                    $(Key::$key => $label,)*
                    $(Key::$pointer => $pointer_label,)*
                    $(Key::$pad => $pad_label,)*
                }
            }

            /// The key whose label is this text: "F5", "Ctrl", "Esc". For a script of input.
            pub fn from_label(label: &str) -> Option<Key> {
                match label {
                    $($label => Some(Key::$key),)*
                    $($pointer_label => Some(Key::$pointer),)*
                    $($pad_label => Some(Key::$pad),)*
                    _ => None,
                }
            }

            /// True for an input of the mouse, false for a key of the keyboard.
            pub fn is_pointer(self) -> bool {
                matches!(self, $(Key::$pointer)|*)
            }

            /// True for a button or a stick direction of a pad.
            pub fn is_pad(self) -> bool {
                matches!(self, $(Key::$pad)|*)
            }

            /// The key of a winit key code. `None`: no binding can use that key.
            pub(crate) fn from_winit(code: winit::keyboard::KeyCode) -> Option<Key> {
                use winit::keyboard::KeyCode;
                match code {
                    $($(KeyCode::$code => Some(Key::$key),)*)*
                    _ => None,
                }
            }
        }
    };
}

keys! {
    keyboard {
        A [KeyA] "A",
        B [KeyB] "B",
        C [KeyC] "C",
        D [KeyD] "D",
        E [KeyE] "E",
        F [KeyF] "F",
        G [KeyG] "G",
        H [KeyH] "H",
        I [KeyI] "I",
        J [KeyJ] "J",
        K [KeyK] "K",
        L [KeyL] "L",
        M [KeyM] "M",
        N [KeyN] "N",
        O [KeyO] "O",
        P [KeyP] "P",
        Q [KeyQ] "Q",
        R [KeyR] "R",
        S [KeyS] "S",
        T [KeyT] "T",
        U [KeyU] "U",
        V [KeyV] "V",
        W [KeyW] "W",
        X [KeyX] "X",
        Y [KeyY] "Y",
        Z [KeyZ] "Z",
        Digit0 [Digit0, Numpad0] "0",
        Digit1 [Digit1, Numpad1] "1",
        Digit2 [Digit2, Numpad2] "2",
        Digit3 [Digit3, Numpad3] "3",
        Digit4 [Digit4, Numpad4] "4",
        Digit5 [Digit5, Numpad5] "5",
        Digit6 [Digit6, Numpad6] "6",
        Digit7 [Digit7, Numpad7] "7",
        Digit8 [Digit8, Numpad8] "8",
        Digit9 [Digit9, Numpad9] "9",
        Shift [ShiftLeft, ShiftRight] "Shift",
        Control [ControlLeft, ControlRight] "Ctrl",
        Alt [AltLeft, AltRight] "Alt",
        Enter [Enter, NumpadEnter] "Enter",
        Space [Space] "Space",
        Tab [Tab] "Tab",
        Backspace [Backspace] "Backspace",
        Delete [Delete] "Delete",
        Insert [Insert] "Insert",
        Home [Home] "Home",
        End [End] "End",
        ArrowUp [ArrowUp] "Up",
        ArrowDown [ArrowDown] "Down",
        ArrowLeft [ArrowLeft] "Left",
        ArrowRight [ArrowRight] "Right",
        PageUp [PageUp] "Page up",
        PageDown [PageDown] "Page down",
        Escape [Escape] "Esc",
        F1 [F1] "F1",
        F2 [F2] "F2",
        F3 [F3] "F3",
        F4 [F4] "F4",
        F5 [F5] "F5",
        F6 [F6] "F6",
        F7 [F7] "F7",
        F8 [F8] "F8",
        F9 [F9] "F9",
        F10 [F10] "F10",
        F11 [F11] "F11",
        F12 [F12] "F12",
    }
    pointer {
        MouseLeft "Left mouse button",
        MouseRight "Right mouse button",
        MouseMiddle "Middle mouse button",
        WheelUp "Wheel up",
        WheelDown "Wheel down",
    }
    // The pad: names of the Xbox layout, with "Pad" so that "A" stays the key A.
    pad {
        PadA "Pad A",
        PadB "Pad B",
        PadX "Pad X",
        PadY "Pad Y",
        PadLB "Pad LB",
        PadRB "Pad RB",
        PadLT "Pad LT",
        PadRT "Pad RT",
        PadStart "Pad Start",
        PadSelect "Pad Select",
        PadL3 "Pad L3",
        PadR3 "Pad R3",
        PadUp "Pad up",
        PadDown "Pad down",
        PadLeft "Pad left",
        PadRight "Pad right",
        LeftStickUp "Left stick up",
        LeftStickDown "Left stick down",
        LeftStickLeft "Left stick left",
        LeftStickRight "Left stick right",
        RightStickUp "Right stick up",
        RightStickDown "Right stick down",
        RightStickLeft "Right stick left",
        RightStickRight "Right stick right",
    }
}

/// The 8 stick directions: the key, its stick, its axis, its sign.
const STICK_DIRECTIONS: [(Key, Stick, Axis, f32); 8] = [
    (Key::LeftStickUp, Stick::Left, Axis::Y, 1.0),
    (Key::LeftStickDown, Stick::Left, Axis::Y, -1.0),
    (Key::LeftStickLeft, Stick::Left, Axis::X, -1.0),
    (Key::LeftStickRight, Stick::Left, Axis::X, 1.0),
    (Key::RightStickUp, Stick::Right, Axis::Y, 1.0),
    (Key::RightStickDown, Stick::Right, Axis::Y, -1.0),
    (Key::RightStickLeft, Stick::Right, Axis::X, -1.0),
    (Key::RightStickRight, Stick::Right, Axis::X, 1.0),
];

/// A stick of the pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stick {
    Left,
    Right,
}

/// An axis of a stick: X to the right, Y up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
}

/// The `[gamepad]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GamepadSettings {
    /// A stick axis under this value gives nothing, so a stick at rest does not
    /// drift. Past it, the value is rescaled to 0..1. In 0..1.
    pub dead_zone: f32,
}

impl Default for GamepadSettings {
    fn default() -> Self {
        Self { dead_zone: 0.2 }
    }
}

impl Settings for GamepadSettings {
    const TABLE: &'static str = "gamepad";

    fn check(&self) -> Result<(), String> {
        if (0.0..1.0).contains(&self.dead_zone) {
            Ok(())
        } else {
            Err(format!("dead_zone must be in 0..1, not {}", self.dead_zone))
        }
    }
}

/// The value of one axis of a stick: 0 under the dead zone, then rescaled to 0..1.
// ponytail: per axis, not radial; radial if a stick feels square.
fn past_dead_zone(axis: f32, dead_zone: f32) -> f32 {
    ((axis - dead_zone) / (1.0 - dead_zone)).clamp(0.0, 1.0)
}

/// The modifier keys that are down with the key of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };

    fn count(self) -> usize {
        self.ctrl as usize + self.shift as usize + self.alt as usize
    }

    /// True if each modifier of `self` is in `held`.
    fn all_in(self, held: Modifiers) -> bool {
        (!self.ctrl || held.ctrl) && (!self.shift || held.shift) && (!self.alt || held.alt)
    }
}

/// A key with the modifiers that must be down when the key goes down: Ctrl+Z. A
/// table of bindings has a [`Key`] for a plain key, and `Chord::ctrl(Key::Z)` for a
/// chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Chord {
    pub const fn ctrl(key: Key) -> Chord {
        let modifiers = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        Chord { modifiers, key }
    }

    /// Text for the user: "Ctrl+Z".
    pub fn label(self) -> String {
        let Modifiers { ctrl, shift, alt } = self.modifiers;
        let parts = [(ctrl, Key::Control), (shift, Key::Shift), (alt, Key::Alt)];
        let held = parts.into_iter().filter(|part| part.0).map(|part| part.1);
        let labels: Vec<_> = held.chain([self.key]).map(Key::label).collect();
        labels.join("+")
    }
}

impl From<Key> for Chord {
    fn from(key: Key) -> Chord {
        Chord {
            modifiers: Modifiers::NONE,
            key,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// The key, with the modifiers of a chord.
    ///
    /// A key starts the binding of its mode with the most modifiers among those
    /// whose modifiers were all down when the key went down: with Ctrl+S and S in a
    /// table, Ctrl+S does not start the action of S. A key with no chord starts its
    /// action whatever modifiers are down (W walks with Shift down). The choice
    /// holds until the key goes up: Ctrl up before Z does not start the action of Z.
    pub chord: Chord,
    pub action: Action,
    /// Text for the help panel.
    pub description: &'static str,
    /// The binding is in use only while [`InputMode`] has this value.
    pub mode: InputMode,
}

/// Which bindings are in use: a key starts only the actions of the bindings of the
/// current mode. A tool that takes the keys of the game for itself (an editor) adds
/// its bindings with a mode of its own and writes the resource.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputMode(pub &'static str);

impl InputMode {
    /// The game is played. The mode at start.
    pub const GAME: InputMode = InputMode("game");
}

impl Default for InputMode {
    fn default() -> Self {
        Self::GAME
    }
}

/// The single source of truth for controls: the input system and the help panel both read it.
/// Many keys can have the same action. In the order in which the modules were added.
#[derive(Resource, Debug, Clone, Default)]
pub struct Bindings(pub Vec<Binding>);

/// Adds the default keys of the actions of a module: key, action, text for the help
/// panel. Call from `Module::build`. The bindings are those of [`InputMode::GAME`].
pub fn add_bindings(app: &mut App, table: &[(impl Into<Chord> + Copy, Action, &'static str)]) {
    add_mode_bindings(app, InputMode::GAME, table);
}

/// As [`add_bindings`], for the bindings of another mode.
pub fn add_mode_bindings(
    app: &mut App,
    mode: InputMode,
    table: &[(impl Into<Chord> + Copy, Action, &'static str)],
) {
    // The table normally comes from `PlatformModule`. This keeps a module
    // independent of the order in which the modules are added.
    app.init_resource::<Bindings>();
    let bindings = table.iter().map(|&(chord, action, description)| Binding {
        chord: chord.into(),
        action,
        description,
        mode,
    });
    app.resource_mut::<Bindings>().0.extend(bindings);
}

/// State of all actions for the current frame. Updated in the `Input` schedule.
///
/// `just_pressed` and `just_released` are true for one frame. A `FixedUpdate` system
/// runs 0..N times per frame, so it would miss or repeat them: it reads
/// `FixedActionState` instead.
#[derive(Resource, Debug, Default)]
pub struct ActionState {
    pressed: HashSet<Action>,
    // The value of each pressed action, in 0..1 and more than 0.
    values: HashMap<Action, f32>,
    just_pressed: HashSet<Action>,
    just_released: HashSet<Action>,
}

impl ActionState {
    pub fn pressed(&self, action: Action) -> bool {
        self.pressed.contains(&action)
    }

    /// How far the action is pressed, 0..1: 0 when not pressed, 1 by a key, a pad
    /// button, or a script; a stick direction gives its value past the dead zone.
    /// The largest value of the inputs that hold the action.
    pub fn value(&self, action: Action) -> f32 {
        self.values.get(&action).copied().unwrap_or(0.0)
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        self.just_pressed.contains(&action)
    }

    pub fn just_released(&self, action: Action) -> bool {
        self.just_released.contains(&action)
    }

    /// All actions that started this frame, in no specified order.
    pub fn iter_just_pressed(&self) -> impl Iterator<Item = Action> + '_ {
        self.just_pressed.iter().copied()
    }
}

/// `ActionState` for systems in `FixedUpdate`. Same methods.
/// `just_pressed` and `just_released` are true for exactly one fixed step:
/// the first step after the change, also when that step is some frames later.
#[derive(Resource, Debug, Default)]
pub struct FixedActionState(ActionState);

impl std::ops::Deref for FixedActionState {
    type Target = ActionState;
    fn deref(&self) -> &ActionState {
        &self.0
    }
}

impl FixedActionState {
    /// Drops the changes that no fixed step has seen: a press in a pause of the game
    /// is not a press of the first step after it.
    pub fn forget_changes(&mut self) {
        self.0.just_pressed.clear();
        self.0.just_released.clear();
    }
}

/// Runs in `FixedLast`: the step that just ended has seen the changes.
pub(crate) fn clear_fixed_changes(mut fixed: ResMut<FixedActionState>) {
    fixed.forget_changes();
}

/// Tells that some other consumer has the keyboard or the mouse: the UI, when the pointer
/// is on a panel or a text field has the focus. Input that is captured starts no action.
/// The consumer writes it; `update_actions` reads it in the next `Input` schedule.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InputCapture {
    pub keyboard: bool,
    /// The mouse: buttons, wheel, and motion.
    pub pointer: bool,
}

/// The mouse pointer in this frame. Updated in the `Input` schedule. The buttons are
/// keys: bind an action to `Key::MouseLeft`. A drag is that action held, and `delta`.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// Where the pointer is, in physical pixels from the top left corner of the
    /// window. `None`: the pointer is not in the window.
    pub position: Option<[f32; 2]>,
    /// How far the pointer moved since the frame before, in physical pixels. Zero
    /// while another consumer has the pointer (`InputCapture`).
    pub delta: [f32; 2],
    /// `Some` in the frame in which a mouse button went down: where the pointer was
    /// at the press. A click is at this place, not at `position`: the pointer can
    /// move between the press and the end of the frame.
    pub press: Option<[f32; 2]>,
}

impl InputCapture {
    fn has(self, key: Key) -> bool {
        // No consumer of the UI reads the pad.
        if key.is_pad() {
            false
        } else if key.is_pointer() {
            self.pointer
        } else {
            self.keyboard
        }
    }
}

/// Actions that a script holds down. A benchmark or a test drives the game with them,
/// as a player does with keys: an action is pressed while it is in the set or a key
/// of it is down.
#[derive(Resource, Debug, Default)]
pub struct ScriptedActions(pub HashSet<Action>);

/// Key changes from the OS. The window driver sends them between frames.
#[derive(Message, Debug, Clone, Copy)]
pub enum RawInput {
    KeyDown(Key),
    KeyUp(Key),
    /// The window lost keyboard focus. The OS sends no key-up after this.
    FocusLost,
    /// The pointer is at this place of the window, in physical pixels.
    PointerMoved([f32; 2]),
    /// The pointer went out of the window.
    PointerLeft,
    /// One axis of a stick of the pad is at this value, from -1 to 1. The other axis
    /// keeps its value; 0 on both releases the directions of the stick.
    PadAxis(Stick, Axis, f32),
    /// The pad is gone: all its buttons and sticks are released.
    PadLost,
}

pub(crate) fn update_pointer(
    mut events: MessageReader<RawInput>,
    capture: Res<InputCapture>,
    mut pointer: ResMut<Pointer>,
) {
    let before = pointer.position;
    let mut position = before;
    let mut press = None;
    // The events are in the order of the OS: the place at a press is that of the
    // last move before it.
    for event in events.read() {
        match *event {
            RawInput::PointerMoved(at) => position = Some(at),
            RawInput::PointerLeft => position = None,
            RawInput::KeyDown(Key::MouseLeft | Key::MouseRight | Key::MouseMiddle) => {
                press = press.or(position);
            }
            _ => {}
        }
    }
    let delta = match (before, position) {
        (Some(from), Some(to)) if !capture.pointer => [to[0] - from[0], to[1] - from[1]],
        _ => [0.0; 2],
    };
    // Only a different value is a write.
    pointer.set_if_neq(Pointer {
        position,
        delta,
        press,
    });
}

/// Runs in `Input`. A system that writes `RawInput` in `Input` runs before it.
// A system: each argument is a system parameter.
#[allow(clippy::too_many_arguments)]
pub fn update_actions(
    mut events: MessageReader<RawInput>,
    // Each key that is down, with the modifiers that were down when it went down.
    mut held: Local<HashMap<Key, Modifiers>>,
    // The place of each stick (x, y), as the `PadAxis` events gave it.
    mut sticks: Local<[[f32; 2]; 2]>,
    gamepad: Res<GamepadSettings>,
    bindings: Res<Bindings>,
    mode: Res<InputMode>,
    capture: Res<InputCapture>,
    scripted: Res<ScriptedActions>,
    mut state: ResMut<ActionState>,
    mut fixed: ResMut<FixedActionState>,
) {
    // Keys that count as down in this frame: held now, or pressed since the last frame.
    // A key that went down and up between two frames thus is down for one frame.
    // The mouse wheel uses this: one wheel step is a key-down then a key-up.
    let mut down = HashMap::new();
    // A key that was held when the capture started is released: its key-up can go
    // to the other consumer.
    held.retain(|key, _| !capture.has(*key));
    for event in events.read() {
        match *event {
            RawInput::KeyDown(key) => {
                // The repeat of a held key keeps the modifiers of its press.
                if !capture.has(key) && !held.contains_key(&key) {
                    let modifiers = Modifiers {
                        ctrl: held.contains_key(&Key::Control),
                        shift: held.contains_key(&Key::Shift),
                        alt: held.contains_key(&Key::Alt),
                    };
                    held.insert(key, modifiers);
                    down.insert(key, modifiers);
                }
            }
            RawInput::KeyUp(key) => {
                held.remove(&key);
            }
            // Without this, a key held during Alt+Tab would stay down.
            RawInput::FocusLost => {
                held.clear();
                *sticks = Default::default();
            }
            RawInput::PadAxis(stick, axis, value) => sticks[stick as usize][axis as usize] = value,
            RawInput::PadLost => {
                held.retain(|key, _| !key.is_pad());
                *sticks = Default::default();
            }
            RawInput::PointerMoved(_) | RawInput::PointerLeft => {}
        }
    }
    down.extend(held.iter().map(|(key, modifiers)| (*key, *modifiers)));
    // A stick direction past the dead zone is down, with a value; a key-down of it wins.
    let mut values = HashMap::new();
    for (key, stick, axis, sign) in STICK_DIRECTIONS {
        let at = sticks[stick as usize][axis as usize];
        let value = past_dead_zone(sign * at, gamepad.dead_zone);
        if value > 0.0 && !down.contains_key(&key) {
            values.insert(key, value);
            down.insert(key, Modifiers::NONE);
        }
    }

    let mode = *mode;
    // The modifiers of a binding that its key can start now: the binding is of the
    // mode, its key is down, and its modifiers were down at the press.
    let fits = |binding: &Binding| {
        let Chord { modifiers, key } = binding.chord;
        let with = down.get(&key).filter(|_| binding.mode == mode)?;
        modifiers.all_in(*with).then_some(modifiers.count())
    };
    let started = bindings.0.iter().filter(|binding| {
        // The chord with the most modifiers wins over the plain key.
        let same_key = |other: &&Binding| other.chord.key == binding.chord.key;
        let most = bindings.0.iter().filter(same_key).filter_map(fits).max();
        fits(binding).is_some_and(|count| Some(count) == most)
    });
    // A key that is not a stick direction gives 1. The largest value wins.
    let mut action_values: HashMap<Action, f32> = HashMap::new();
    let inputs = started
        .map(|binding| {
            let key = binding.chord.key;
            (binding.action, values.get(&key).copied().unwrap_or(1.0))
        })
        .chain(scripted.0.iter().map(|action| (*action, 1.0)));
    for (action, value) in inputs {
        let max = action_values.entry(action).or_default();
        *max = max.max(value);
    }
    let pressed: HashSet<Action> = action_values.keys().copied().collect();
    state.values = action_values;
    state.just_pressed = &pressed - &state.pressed;
    state.just_released = &state.pressed - &pressed;
    state.pressed = pressed;

    // Changes collect here until a fixed step reads them. A frame can have no step.
    fixed.0.pressed.clone_from(&state.pressed);
    fixed.0.values.clone_from(&state.values);
    fixed.0.just_pressed.extend(&state.just_pressed);
    fixed.0.just_released.extend(&state.just_released);
}
