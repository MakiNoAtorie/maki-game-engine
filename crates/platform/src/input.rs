//! Input actions. Gameplay code reads `ActionState`, never keys.

use std::collections::{HashMap, HashSet};

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};
use maki_core::App;

/// A thing that the player can ask for: `Action("interact")`. The text is the
/// identity: two actions with the same text are the same action.
///
/// The set of actions is open. The module that reads an action defines it as a
/// constant and gives it its default keys with [`add_bindings`]; this crate knows no
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Action(pub &'static str);

/// A physical input that a binding can use: a key, a mouse button, or one direction
/// of the mouse wheel. Letter keys are positions on a US keyboard, so W/A/S/D keep
/// their shape on other layouts.
///
/// One row of the table for each input: the name, the winit key codes that give it
/// (none for an input of the mouse), and the text for the user. A new key is a new row.
macro_rules! keys {
    ($($key:ident [$($code:ident),*] $label:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($key,)*
        }

        impl Key {
            /// Name to show to the user.
            pub fn label(self) -> &'static str {
                match self {
                    $(Key::$key => $label,)*
                }
            }

            /// The key whose label is this text: "F5", "Ctrl", "Esc". For a script of input.
            pub fn from_label(label: &str) -> Option<Key> {
                match label {
                    $($label => Some(Key::$key),)*
                    _ => None,
                }
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
    MouseLeft [] "Left mouse button",
    MouseRight [] "Right mouse button",
    MouseMiddle [] "Middle mouse button",
    WheelUp [] "Wheel up",
    WheelDown [] "Wheel down",
}

impl Key {
    /// True for an input of the mouse, false for a key of the keyboard.
    pub fn is_pointer(self) -> bool {
        use Key::*;
        matches!(
            self,
            MouseLeft | MouseRight | MouseMiddle | WheelUp | WheelDown
        )
    }
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
    just_pressed: HashSet<Action>,
    just_released: HashSet<Action>,
}

impl ActionState {
    pub fn pressed(&self, action: Action) -> bool {
        self.pressed.contains(&action)
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
        if key.is_pointer() {
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
            RawInput::FocusLost => held.clear(),
            RawInput::PointerMoved(_) | RawInput::PointerLeft => {}
        }
    }
    down.extend(held.iter().map(|(key, modifiers)| (*key, *modifiers)));

    let mode = *mode;
    // The modifiers of a binding that its key can start now: the binding is of the
    // mode, its key is down, and its modifiers were down at the press.
    let fits = |binding: &Binding| {
        let Chord { modifiers, key } = binding.chord;
        let with = down.get(&key).filter(|_| binding.mode == mode)?;
        modifiers.all_in(*with).then_some(modifiers.count())
    };
    let pressed: HashSet<Action> = bindings
        .0
        .iter()
        .filter(|binding| {
            // The chord with the most modifiers wins over the plain key.
            let same_key = |other: &&Binding| other.chord.key == binding.chord.key;
            let most = bindings.0.iter().filter(same_key).filter_map(fits).max();
            fits(binding).is_some_and(|count| Some(count) == most)
        })
        .map(|binding| binding.action)
        .chain(scripted.0.iter().copied())
        .collect();
    state.just_pressed = &pressed - &state.pressed;
    state.just_released = &state.pressed - &pressed;
    state.pressed = pressed;

    // Changes collect here until a fixed step reads them. A frame can have no step.
    fixed.0.pressed.clone_from(&state.pressed);
    fixed.0.just_pressed.extend(&state.just_pressed);
    fixed.0.just_released.extend(&state.just_released);
}
