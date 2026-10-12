//! Input actions. Gameplay code reads `ActionState`, never keys.

use std::collections::HashSet;

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

/// A physical input that a binding can use: a key, or one direction of the mouse wheel.
/// Letter keys are positions on a US keyboard, so W/A/S/D keep their shape on other layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
    R,
    Z,
    L,
    O,
    /// The left or the right Shift key.
    Shift,
    Enter,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    F1,
    F5,
    PageUp,
    PageDown,
    Escape,
    WheelUp,
    WheelDown,
}

impl Key {
    /// True for an input of the mouse, false for a key of the keyboard.
    pub fn is_pointer(self) -> bool {
        matches!(self, Key::WheelUp | Key::WheelDown)
    }

    /// Name to show to the user.
    pub fn label(self) -> &'static str {
        match self {
            Key::W => "W",
            Key::A => "A",
            Key::S => "S",
            Key::D => "D",
            Key::Q => "Q",
            Key::E => "E",
            Key::R => "R",
            Key::Z => "Z",
            Key::L => "L",
            Key::O => "O",
            Key::Shift => "Shift",
            Key::Enter => "Enter",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::F1 => "F1",
            Key::F5 => "F5",
            Key::PageUp => "Page up",
            Key::PageDown => "Page down",
            Key::Escape => "Esc",
            Key::WheelUp => "Wheel up",
            Key::WheelDown => "Wheel down",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub key: Key,
    pub action: Action,
    /// Text for the help panel.
    pub description: &'static str,
}

/// The single source of truth for controls: the input system and the help panel both read it.
/// Many keys can have the same action. In the order in which the modules were added.
#[derive(Resource, Debug, Clone, Default)]
pub struct Bindings(pub Vec<Binding>);

/// Adds the default keys of the actions of a module: key, action, text for the help
/// panel. Call from `Module::build`.
pub fn add_bindings(app: &mut App, table: &[(Key, Action, &'static str)]) {
    // The table normally comes from `PlatformModule`. This keeps a module
    // independent of the order in which the modules are added.
    app.init_resource::<Bindings>();
    let bindings = table.iter().map(|&(key, action, description)| Binding {
        key,
        action,
        description,
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

/// Runs in `FixedLast`: the step that just ended has seen the changes.
pub(crate) fn clear_fixed_changes(mut fixed: ResMut<FixedActionState>) {
    fixed.0.just_pressed.clear();
    fixed.0.just_released.clear();
}

/// Tells that some other consumer has the keyboard or the mouse: the UI, when the pointer
/// is on a panel or a text field has the focus. Input that is captured starts no action.
/// The consumer writes it; `update_actions` reads it in the next `Input` schedule.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InputCapture {
    pub keyboard: bool,
    /// The mouse: buttons and wheel.
    pub pointer: bool,
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
}

pub(crate) fn update_actions(
    mut events: MessageReader<RawInput>,
    mut held: Local<HashSet<Key>>,
    bindings: Res<Bindings>,
    capture: Res<InputCapture>,
    scripted: Res<ScriptedActions>,
    mut state: ResMut<ActionState>,
    mut fixed: ResMut<FixedActionState>,
) {
    // Keys that count as down in this frame: held now, or pressed since the last frame.
    // A key that went down and up between two frames thus is down for one frame.
    // The mouse wheel uses this: one wheel step is a key-down then a key-up.
    let mut down = HashSet::new();
    // A key that was held when the capture started is released: its key-up can go
    // to the other consumer.
    held.retain(|key| !capture.has(*key));
    for event in events.read() {
        match *event {
            RawInput::KeyDown(key) => {
                if !capture.has(key) {
                    held.insert(key);
                    down.insert(key);
                }
            }
            RawInput::KeyUp(key) => {
                held.remove(&key);
            }
            // Without this, a key held during Alt+Tab would stay down.
            RawInput::FocusLost => held.clear(),
        }
    }
    down.extend(held.iter());

    let pressed: HashSet<Action> = bindings
        .0
        .iter()
        .filter(|binding| down.contains(&binding.key))
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
