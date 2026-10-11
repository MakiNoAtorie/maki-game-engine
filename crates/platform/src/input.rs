//! Input actions. Gameplay code reads `ActionState`, never keys.

use std::collections::HashSet;

use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};

/// A thing that the player can ask for.
// One central enum: the ui and render crates must read some actions (ToggleHelp,
// ResetAccumulation), and they cannot see a type that the game crate defines.
// ponytail: a new action is an edit of this enum. If modules need private actions,
// change to action ids that modules register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    MoveLightForward,
    MoveLightBack,
    MoveLightLeft,
    MoveLightRight,
    LightUp,
    LightDown,
    LightIntensityUp,
    LightIntensityDown,
    ResetAccumulation,
    ToggleHelp,
    Quit,
}

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
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    F1,
    Escape,
    WheelUp,
    WheelDown,
}

impl Key {
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
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::F1 => "F1",
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
/// Many keys can have the same action.
#[derive(Resource, Debug, Clone)]
pub struct Bindings(pub Vec<Binding>);

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        let table = [
            (Key::W, MoveLightForward, "Move light forward"),
            (Key::ArrowUp, MoveLightForward, "Move light forward"),
            (Key::S, MoveLightBack, "Move light back"),
            (Key::ArrowDown, MoveLightBack, "Move light back"),
            (Key::A, MoveLightLeft, "Move light left"),
            (Key::ArrowLeft, MoveLightLeft, "Move light left"),
            (Key::D, MoveLightRight, "Move light right"),
            (Key::ArrowRight, MoveLightRight, "Move light right"),
            (Key::Q, LightDown, "Move light down"),
            (Key::E, LightUp, "Move light up"),
            (Key::WheelUp, LightIntensityUp, "Increase light intensity"),
            (
                Key::WheelDown,
                LightIntensityDown,
                "Decrease light intensity",
            ),
            (Key::R, ResetAccumulation, "Reset accumulation"),
            (Key::F1, ToggleHelp, "Toggle help panel"),
            (Key::Escape, Quit, "Quit"),
        ];
        Self(
            table
                .into_iter()
                .map(|(key, action, description)| Binding {
                    key,
                    action,
                    description,
                })
                .collect(),
        )
    }
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
    mut state: ResMut<ActionState>,
    mut fixed: ResMut<FixedActionState>,
) {
    // Keys that count as down in this frame: held now, or pressed since the last frame.
    // A key that went down and up between two frames thus is down for one frame.
    // The mouse wheel uses this: one wheel step is a key-down then a key-up.
    let mut down = HashSet::new();
    for event in events.read() {
        match *event {
            RawInput::KeyDown(key) => {
                held.insert(key);
                down.insert(key);
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
        .collect();
    state.just_pressed = &pressed - &state.pressed;
    state.just_released = &state.pressed - &pressed;
    state.pressed = pressed;

    // Changes collect here until a fixed step reads them. A frame can have no step.
    fixed.0.pressed.clone_from(&state.pressed);
    fixed.0.just_pressed.extend(&state.just_pressed);
    fixed.0.just_released.extend(&state.just_released);
}
