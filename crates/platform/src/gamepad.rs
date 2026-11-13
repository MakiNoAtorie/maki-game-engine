//! The gamepad (gilrs), translated into `RawInput`. gilrs types stay in this file.
//! One player: the first pad that is connected. Events of other pads are ignored.

use gilrs::{Axis, Button, EventType, Gilrs, GilrsBuilder};

use crate::input::{self, Key, RawInput, Stick};

/// A trigger is a button past this value.
const TRIGGER_DOWN: f32 = 0.5;

/// The pads of the OS. The window driver reads them one time per frame.
pub(crate) struct Gamepads {
    gilrs: Gilrs,
    player: Player,
}

impl Gamepads {
    /// `None` if gilrs cannot start: the game then runs without a pad.
    pub(crate) fn new() -> Option<Self> {
        // No filters of gilrs: the dead zone is that of `GamepadSettings`.
        match GilrsBuilder::new().with_default_filters(false).build() {
            Ok(gilrs) => {
                let player = Player {
                    id: gilrs.gamepads().next().map(|(id, _)| id.into()),
                    ..Player::default()
                };
                Some(Self { gilrs, player })
            }
            Err(e) => {
                tracing::warn!("no gamepad input: {e}");
                None
            }
        }
    }

    /// The pad events since the last call.
    pub(crate) fn poll(&mut self, mut send: impl FnMut(RawInput)) {
        while let Some(event) = self.gilrs.next_event() {
            let pad = match event.event {
                EventType::ButtonPressed(button, _) => PadEvent::Pressed(button),
                EventType::ButtonReleased(button, _) => PadEvent::Released(button),
                EventType::ButtonChanged(button, value, _) => PadEvent::Changed(button, value),
                EventType::AxisChanged(axis, value, _) => PadEvent::Axis(axis, value),
                EventType::Disconnected => PadEvent::Disconnected,
                _ => PadEvent::Other,
            };
            self.player.map(event.id.into(), pad, &mut send);
        }
    }

    /// The window lost the focus: `update_actions` releases the pad inputs.
    pub(crate) fn focus_lost(&mut self) {
        self.player.release();
    }
}

/// A gilrs event without its `Code`, which tests cannot make.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PadEvent {
    Pressed(Button),
    Released(Button),
    Changed(Button, f32),
    Axis(Axis, f32),
    Disconnected,
    /// Connected and the rest: only the pad id counts.
    Other,
}

/// The state of player 1: which pad it is, and its triggers.
#[derive(Default)]
struct Player {
    /// The gilrs id of the pad. `None`: no pad, the next pad that sends an event is it.
    id: Option<usize>,
    /// LT and RT: down past `TRIGGER_DOWN`.
    triggers: [bool; 2],
}

impl Player {
    /// Without the focus no event comes: a trigger let go then would stay down
    /// here, and its next press would send no key-down.
    fn release(&mut self) {
        self.triggers = [false; 2];
    }

    fn map(&mut self, pad: usize, event: PadEvent, send: &mut impl FnMut(RawInput)) {
        if self.id.is_none() && event != PadEvent::Disconnected {
            self.id = Some(pad);
        }
        if self.id != Some(pad) {
            return;
        }
        match event {
            // Triggers have no key here: they are analog, `Changed` decides them.
            PadEvent::Pressed(button) => {
                if let Some(key) = button_key(button) {
                    send(RawInput::KeyDown(key));
                }
            }
            PadEvent::Released(button) => {
                if let Some(key) = button_key(button) {
                    send(RawInput::KeyUp(key));
                }
            }
            PadEvent::Changed(button, value) => {
                let (i, key) = match button {
                    Button::LeftTrigger2 => (0, Key::PadLT),
                    Button::RightTrigger2 => (1, Key::PadRT),
                    _ => return,
                };
                let down = value > TRIGGER_DOWN;
                if down != self.triggers[i] {
                    self.triggers[i] = down;
                    send(if down {
                        RawInput::KeyDown(key)
                    } else {
                        RawInput::KeyUp(key)
                    });
                }
            }
            PadEvent::Axis(axis, value) => {
                let (stick, axis) = match axis {
                    Axis::LeftStickX => (Stick::Left, input::Axis::X),
                    Axis::LeftStickY => (Stick::Left, input::Axis::Y),
                    Axis::RightStickX => (Stick::Right, input::Axis::X),
                    Axis::RightStickY => (Stick::Right, input::Axis::Y),
                    _ => return,
                };
                send(RawInput::PadAxis(stick, axis, value));
            }
            PadEvent::Disconnected => {
                *self = Player::default();
                send(RawInput::PadLost);
            }
            _ => {}
        }
    }
}

/// The key of a digital button. gilrs names LB and RB `LeftTrigger` and `RightTrigger`.
fn button_key(button: Button) -> Option<Key> {
    Some(match button {
        Button::South => Key::PadA,
        Button::East => Key::PadB,
        Button::West => Key::PadX,
        Button::North => Key::PadY,
        Button::LeftTrigger => Key::PadLB,
        Button::RightTrigger => Key::PadRB,
        Button::Start => Key::PadStart,
        Button::Select => Key::PadSelect,
        Button::LeftThumb => Key::PadL3,
        Button::RightThumb => Key::PadR3,
        Button::DPadUp => Key::PadUp,
        Button::DPadDown => Key::PadDown,
        Button::DPadLeft => Key::PadLeft,
        Button::DPadRight => Key::PadRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `RawInput` of each event, as strings: `RawInput` has no `PartialEq`.
    fn run(player: &mut Player, events: &[(usize, PadEvent)]) -> Vec<String> {
        let mut out = Vec::new();
        for &(pad, event) in events {
            player.map(pad, event, &mut |input| out.push(format!("{input:?}")));
        }
        out
    }

    #[test]
    fn buttons_axes_and_triggers_become_raw_input() {
        let mut player = Player::default();
        let out = run(
            &mut player,
            &[
                (0, PadEvent::Pressed(Button::South)),
                (0, PadEvent::Released(Button::South)),
                // A trigger: no key from its digital press, a key past 0.5.
                (0, PadEvent::Pressed(Button::LeftTrigger2)),
                (0, PadEvent::Changed(Button::LeftTrigger2, 0.4)),
                (0, PadEvent::Changed(Button::LeftTrigger2, 0.6)),
                (0, PadEvent::Changed(Button::LeftTrigger2, 0.9)),
                (0, PadEvent::Changed(Button::LeftTrigger2, 0.1)),
                (0, PadEvent::Axis(Axis::LeftStickX, 0.5)),
                (0, PadEvent::Axis(Axis::LeftStickY, -1.0)),
                (0, PadEvent::Axis(Axis::RightStickY, 0.25)),
            ],
        );
        assert_eq!(
            out,
            [
                "KeyDown(PadA)",
                "KeyUp(PadA)",
                "KeyDown(PadLT)",
                "KeyUp(PadLT)",
                "PadAxis(Left, X, 0.5)",
                "PadAxis(Left, Y, -1.0)",
                "PadAxis(Right, Y, 0.25)",
            ]
        );
    }

    #[test]
    fn a_trigger_let_go_without_the_focus_presses_again() {
        let mut player = Player::default();
        let lt = |value| (0, PadEvent::Changed(Button::LeftTrigger2, value));
        run(&mut player, &[lt(0.9)]);
        // The trigger goes up while the window has no focus: no event.
        player.release();
        assert_eq!(run(&mut player, &[lt(0.9)]), ["KeyDown(PadLT)"]);
        assert_eq!(player.id, Some(0));
    }

    #[test]
    fn the_first_pad_is_player_1_until_it_is_lost() {
        let mut player = Player::default();
        let a = PadEvent::Pressed(Button::South);
        let out = run(
            &mut player,
            &[
                (3, PadEvent::Other),
                (5, PadEvent::Other),
                // Another pad: ignored, also its loss.
                (5, a),
                (5, PadEvent::Disconnected),
                (3, a),
                (3, PadEvent::Disconnected),
                // Player 1 is gone: the next pad is player 1.
                (5, a),
            ],
        );
        assert_eq!(out, ["KeyDown(PadA)", "PadLost", "KeyDown(PadA)"]);
        assert_eq!(player.id, Some(5));
    }
}
