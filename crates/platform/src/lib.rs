//! Window and raw input (winit), translated into input actions.

mod input;
mod window;

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use maki_core::{App, FixedLast, Input, Module};

pub use input::{
    Action, ActionState, Binding, Bindings, Chord, FixedActionState, InputCapture, InputMode, Key,
    Modifiers, Pointer, RawInput, ScriptedActions, add_bindings, add_mode_bindings,
};
pub use window::{
    Picture, PictureFit, QuitHold, QuitRequested, RawWindowEvent, Window, WindowMargin,
    WindowResized, WindowSettings, WindowSize, WindowTitle, run, update_picture,
};

/// Adds input actions and window data to the app. `run` opens the window.
pub struct PlatformModule;

impl Module for PlatformModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<WindowSettings>();
        let window = app.world().resource::<WindowSettings>().clone();
        let margin = WindowMargin::default();
        app.insert_resource(WindowTitle(window.title.clone()))
            // The picture of the first frame, before the first `Extract`.
            .insert_resource(Picture {
                left: margin.left,
                top: margin.top,
                width: window.width.max(1),
                height: window.height.max(1),
            })
            .init_resource::<PictureFit>()
            // It reads what the frame wrote, and it is the one writer of the picture.
            .add_systems(
                maki_core::Extract,
                window::update_picture.ambiguous_with_all(),
            )
            .add_message::<RawInput>()
            .add_message::<WindowResized>()
            .add_message::<RawWindowEvent>()
            .init_resource::<Bindings>()
            .init_resource::<InputCapture>()
            .init_resource::<InputMode>()
            .init_resource::<Pointer>()
            .init_resource::<QuitHold>()
            .init_resource::<ScriptedActions>()
            .init_resource::<ActionState>()
            .init_resource::<FixedActionState>()
            .init_resource::<QuitRequested>()
            // Each writer asks for the end. A module that holds the app open
            // (`QuitHold`) takes the request in this frame or in the next one.
            .allow_ambiguous_resource::<QuitRequested>()
            .init_resource::<WindowMargin>()
            .add_systems(FixedLast, input::clear_fixed_changes)
            .add_systems(
                Input,
                (
                    // In this order: a system that writes `InputCapture` in this
                    // schedule names only the first one.
                    (input::update_actions, input::update_pointer).chain(),
                    // `run` adds `WindowSize` with the window. A headless app has none.
                    window::update_window_size.run_if(resource_exists::<WindowSize>),
                    // It reads only the picture size of the settings. The order to a system that
                    // writes the settings decides only if the window follows in this frame or
                    // in the next one.
                    window::fit_window
                        .run_if(resource_exists::<Window>)
                        .ambiguous_with_all(),
                    window::apply_title
                        .run_if(resource_exists::<Window>)
                        .ambiguous_with_all(),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::ConfigFile;

    use super::*;

    const FORWARD: Action = Action("forward");
    const DOWN: Action = Action("down");
    const UP: Action = Action("up");
    const WHEEL: Action = Action("wheel");
    const RESET: Action = Action("reset");
    const INTERACT: Action = Action("interact");

    /// An app with the platform module and some bindings, as a game module adds them.
    fn app(config: &str) -> App {
        let mut app = App::new(ConfigFile::from_text(config)).unwrap();
        app.add_module(PlatformModule);
        add_bindings(
            &mut app,
            &[
                (Key::W, FORWARD, "Forward"),
                (Key::ArrowUp, FORWARD, "Forward"),
                (Key::Q, DOWN, "Down"),
                (Key::E, UP, "Up"),
                (Key::WheelUp, WHEEL, "Wheel"),
                (Key::R, RESET, "Reset"),
            ],
        );
        app
    }

    fn frame(app: &mut App, events: &[RawInput]) {
        for event in events {
            app.write_message(*event);
        }
        app.advance(Duration::from_millis(16));
    }

    #[test]
    fn keys_become_action_states() {
        use RawInput::*;

        let mut app = app("");
        let state = |app: &App, action| {
            let actions = app.world().resource::<ActionState>();
            (
                actions.pressed(action),
                actions.just_pressed(action),
                actions.just_released(action),
            )
        };

        frame(&mut app, &[KeyDown(Key::W)]);
        assert_eq!(state(&app, FORWARD), (true, true, false));
        frame(&mut app, &[]);
        assert_eq!(state(&app, FORWARD), (true, false, false));

        // Two keys have this action: it stays pressed until both are up.
        frame(&mut app, &[KeyDown(Key::ArrowUp), KeyUp(Key::W)]);
        assert_eq!(state(&app, FORWARD), (true, false, false));
        frame(&mut app, &[KeyUp(Key::ArrowUp)]);
        assert_eq!(state(&app, FORWARD), (false, false, true));
        frame(&mut app, &[]);
        assert_eq!(state(&app, FORWARD), (false, false, false));

        // Down and up between two frames (a wheel step) is a press of one frame.
        frame(&mut app, &[KeyDown(Key::WheelUp), KeyUp(Key::WheelUp)]);
        assert_eq!(state(&app, WHEEL), (true, true, false));
        frame(&mut app, &[]);
        assert_eq!(state(&app, WHEEL), (false, false, true));

        frame(&mut app, &[KeyDown(Key::Q)]);
        frame(&mut app, &[FocusLost]);
        assert_eq!(state(&app, DOWN), (false, false, true));
    }

    #[derive(bevy_ecs::message::Message, Clone, Copy)]
    struct SetCapture(InputCapture);

    #[test]
    fn captured_input_starts_no_action() {
        use RawInput::*;
        use bevy_ecs::message::MessageReader;
        use bevy_ecs::system::ResMut;

        let mut app = app("");
        // The system stands in for the UI, which writes the capture.
        app.add_message::<SetCapture>().add_systems(
            Input,
            (|mut events: MessageReader<SetCapture>, mut capture: ResMut<InputCapture>| {
                if let Some(event) = events.read().last() {
                    *capture = event.0;
                }
            })
            .before(input::update_actions),
        );
        let pressed = |app: &App, action| app.world().resource::<ActionState>().pressed(action);
        let wheel = [KeyDown(Key::WheelUp), KeyUp(Key::WheelUp)];

        frame(&mut app, &[KeyDown(Key::W)]);
        assert!(pressed(&app, FORWARD));

        // The keyboard is captured: the held key is released and a new key starts
        // nothing. The wheel is not part of the keyboard.
        app.write_message(SetCapture(InputCapture {
            keyboard: true,
            pointer: false,
        }));
        frame(&mut app, &[KeyDown(Key::Q), wheel[0], wheel[1]]);
        assert!(!pressed(&app, FORWARD));
        assert!(!pressed(&app, DOWN));
        assert!(pressed(&app, WHEEL));

        // The mouse is captured: the wheel starts nothing, a key does.
        app.write_message(SetCapture(InputCapture {
            keyboard: false,
            pointer: true,
        }));
        frame(&mut app, &[KeyDown(Key::E), wheel[0], wheel[1]]);
        assert!(pressed(&app, UP));
        assert!(!pressed(&app, WHEEL));
    }

    #[test]
    fn a_scripted_action_is_pressed_as_a_key_is() {
        use bevy_ecs::system::{Res, ResMut};
        use maki_core::{Time, Update};

        let mut app = app("");
        // The script holds the action in the frames 2 and 3. It writes in `Update`,
        // so the action state has it one frame later. A scripted action needs no key.
        app.add_systems(
            Update,
            |time: Res<Time>, mut scripted: ResMut<ScriptedActions>| {
                scripted.0.clear();
                if time.frame == 2 || time.frame == 3 {
                    scripted.0.insert(INTERACT);
                }
            },
        );
        let mut states = Vec::new();
        for _ in 0..5 {
            app.advance(Duration::from_millis(16));
            let actions = app.world().resource::<ActionState>();
            states.push((actions.pressed(INTERACT), actions.just_pressed(INTERACT)));
        }
        assert_eq!(
            states,
            [
                (false, false),
                (false, false),
                (true, true),
                (true, false),
                (false, false)
            ]
        );
    }

    #[test]
    fn the_pointer_moves_and_a_mode_has_its_own_bindings() {
        use RawInput::*;

        const PICK: Action = Action("pick");
        let tool = InputMode("tool");
        let mut app = app("");
        add_mode_bindings(&mut app, tool, &[(Key::MouseLeft, PICK, "Pick")]);
        let pointer = |app: &App| *app.world().resource::<Pointer>();
        let pressed = |app: &App, action| app.world().resource::<ActionState>().pressed(action);

        // The first place has no place before it: no motion.
        frame(&mut app, &[PointerMoved([10.0, 10.0])]);
        assert_eq!(pointer(&app).delta, [0.0, 0.0]);
        frame(
            &mut app,
            &[PointerMoved([12.0, 10.0]), PointerMoved([15.0, 12.0])],
        );
        assert_eq!(pointer(&app).position, Some([15.0, 12.0]));
        assert_eq!(pointer(&app).delta, [5.0, 2.0]);
        frame(&mut app, &[]);
        assert_eq!(pointer(&app).delta, [0.0, 0.0]);

        // The game mode: the key of the game, not the button of the tool.
        frame(&mut app, &[KeyDown(Key::MouseLeft), KeyDown(Key::W)]);
        assert!(pressed(&app, FORWARD) && !pressed(&app, PICK));
        // The mode of the tool: the keys that are held start its actions.
        *app.resource_mut::<InputMode>() = tool;
        frame(&mut app, &[]);
        assert!(!pressed(&app, FORWARD) && pressed(&app, PICK));

        frame(&mut app, &[PointerLeft]);
        assert_eq!(pointer(&app).position, None);
    }

    #[test]
    fn a_chord_wins_over_its_plain_key_and_holds_until_the_key_goes_up() {
        use RawInput::*;

        const SAVE: Action = Action("save");
        const UNDO: Action = Action("undo");
        const RUN: Action = Action("run");
        let mut app = app("");
        // Q is "down" in the table, and a chord of Q too. W has no chord.
        add_bindings(
            &mut app,
            &[
                (Chord::ctrl(Key::Q), SAVE, "Save"),
                (Chord::ctrl(Key::Z), UNDO, "Undo"),
            ],
        );
        add_bindings(&mut app, &[(Key::Control, RUN, "A held modifier")]);
        let state = |app: &App, action| {
            let actions = app.world().resource::<ActionState>();
            (actions.pressed(action), actions.just_pressed(action))
        };

        // The plain key.
        frame(&mut app, &[KeyDown(Key::Q)]);
        assert_eq!(
            (state(&app, DOWN), state(&app, SAVE)),
            ((true, true), (false, false))
        );
        // Ctrl after the key: the key keeps its action.
        frame(&mut app, &[KeyDown(Key::Control)]);
        assert_eq!((state(&app, DOWN).0, state(&app, SAVE).0), (true, false));
        frame(&mut app, &[KeyUp(Key::Q)]);

        // The chord, with Ctrl held: not the action of the plain key. The modifier
        // key has an action of its own.
        frame(&mut app, &[KeyDown(Key::Q)]);
        assert_eq!(
            (state(&app, DOWN), state(&app, SAVE)),
            ((false, false), (true, true))
        );
        assert!(state(&app, RUN).0);
        // A key with no chord starts its action with Ctrl down: W walks.
        frame(&mut app, &[KeyDown(Key::W)]);
        assert!(state(&app, FORWARD).0);

        // Ctrl goes up before the key: the chord holds, and the plain key does not
        // start. The action ends one time, when the key goes up.
        let mut released = 0;
        for events in [&[KeyUp(Key::Control)][..], &[], &[KeyUp(Key::Q)], &[]] {
            frame(&mut app, events);
            let actions = app.world().resource::<ActionState>();
            assert!(!actions.pressed(DOWN) && !actions.just_pressed(DOWN));
            released += actions.just_released(SAVE) as u32;
        }
        assert_eq!(released, 1);
        assert_eq!(Chord::ctrl(Key::Q).label(), "Ctrl+Q");
        assert_eq!(Chord::from(Key::Q).label(), "Q");

        // A press and a move in one frame: the press has its place.
        let pointer = |app: &App| *app.world().resource::<Pointer>();
        frame(
            &mut app,
            &[
                PointerMoved([10.0, 20.0]),
                KeyDown(Key::MouseLeft),
                PointerMoved([90.0, 80.0]),
            ],
        );
        assert_eq!(pointer(&app).press, Some([10.0, 20.0]));
        assert_eq!(pointer(&app).position, Some([90.0, 80.0]));
        frame(&mut app, &[]);
        assert_eq!(pointer(&app).press, None);
    }

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct StepsWithPress(u32);

    #[test]
    fn a_fixed_step_sees_each_press_one_time() {
        use bevy_ecs::system::{Res, ResMut};
        use maki_core::FixedUpdate;

        // 50 Hz: one fixed step per 20 ms.
        let mut app = app("[time]\nfixed_hz = 50.0");
        app.init_resource::<StepsWithPress>().add_systems(
            FixedUpdate,
            |actions: Res<FixedActionState>, mut steps: ResMut<StepsWithPress>| {
                if actions.just_pressed(RESET) {
                    steps.0 += 1;
                }
            },
        );
        let mut frame = |events: &[RawInput], ms| {
            for event in events {
                app.write_message(*event);
            }
            app.advance(Duration::from_millis(ms));
            app.world().resource::<StepsWithPress>().0
        };

        // A tap in a frame without a fixed step waits for the next step.
        let tap = [RawInput::KeyDown(Key::R), RawInput::KeyUp(Key::R)];
        assert_eq!(frame(&tap, 5), 0);
        assert_eq!(frame(&[], 5), 0);
        assert_eq!(frame(&[], 10), 1);
        // A frame with three steps shows the press to the first step only.
        assert_eq!(frame(&tap, 60), 2);
    }

    #[test]
    fn resize_message_updates_window_size() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule).insert_resource(WindowSize {
            width: 1,
            height: 1,
        });
        app.write_message(WindowResized {
            width: 800,
            height: 600,
        });
        app.advance(Duration::ZERO);
        assert_eq!(
            *app.world().resource::<WindowSize>(),
            WindowSize {
                width: 800,
                height: 600
            }
        );
    }
}
