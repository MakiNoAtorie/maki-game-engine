//! Window and raw input (winit), translated into input actions.

mod input;
mod window;

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use maki_core::{App, FixedLast, Input, Module};

pub use input::{Action, ActionState, Binding, Bindings, FixedActionState, Key, RawInput};
pub use window::{QuitRequested, Window, WindowResized, WindowSize, run};

/// Adds input actions and window data to the app. `run` opens the window.
pub struct PlatformModule;

impl Module for PlatformModule {
    fn build(&self, app: &mut App) {
        app.add_message::<RawInput>()
            .add_message::<WindowResized>()
            .init_resource::<Bindings>()
            .init_resource::<ActionState>()
            .init_resource::<FixedActionState>()
            .init_resource::<QuitRequested>()
            .add_systems(FixedLast, input::clear_fixed_changes)
            .add_systems(
                Input,
                (
                    input::update_actions,
                    // `run` adds `WindowSize` with the window. A headless app has none.
                    window::update_window_size.run_if(resource_exists::<WindowSize>),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::Config;

    use super::*;

    fn frame(app: &mut App, events: &[RawInput]) {
        for event in events {
            app.write_message(*event);
        }
        app.advance(Duration::from_millis(16));
    }

    #[test]
    fn keys_become_action_states() {
        use Action::*;
        use RawInput::*;

        let mut app = App::new(Config::default()).unwrap();
        app.add_module(PlatformModule);
        let state = |app: &App, action| {
            let actions = app.world().resource::<ActionState>();
            (
                actions.pressed(action),
                actions.just_pressed(action),
                actions.just_released(action),
            )
        };

        frame(&mut app, &[KeyDown(Key::W)]);
        assert_eq!(state(&app, MoveLightForward), (true, true, false));
        frame(&mut app, &[]);
        assert_eq!(state(&app, MoveLightForward), (true, false, false));

        // Two keys have this action: it stays pressed until both are up.
        frame(&mut app, &[KeyDown(Key::ArrowUp), KeyUp(Key::W)]);
        assert_eq!(state(&app, MoveLightForward), (true, false, false));
        frame(&mut app, &[KeyUp(Key::ArrowUp)]);
        assert_eq!(state(&app, MoveLightForward), (false, false, true));
        frame(&mut app, &[]);
        assert_eq!(state(&app, MoveLightForward), (false, false, false));

        // Down and up between two frames (a wheel step) is a press of one frame.
        frame(&mut app, &[KeyDown(Key::WheelUp), KeyUp(Key::WheelUp)]);
        assert_eq!(state(&app, LightIntensityUp), (true, true, false));
        frame(&mut app, &[]);
        assert_eq!(state(&app, LightIntensityUp), (false, false, true));

        frame(&mut app, &[KeyDown(Key::Q)]);
        frame(&mut app, &[FocusLost]);
        assert_eq!(state(&app, LightDown), (false, false, true));
    }

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct StepsWithPress(u32);

    #[test]
    fn a_fixed_step_sees_each_press_one_time() {
        use bevy_ecs::system::{Res, ResMut};
        use maki_core::FixedUpdate;

        // 50 Hz: one fixed step per 20 ms.
        let mut app = App::new(Config {
            fixed_hz: 50.0,
            ..Default::default()
        })
        .unwrap();
        app.add_module(PlatformModule)
            .init_resource::<StepsWithPress>()
            .add_systems(
                FixedUpdate,
                |actions: Res<FixedActionState>, mut steps: ResMut<StepsWithPress>| {
                    if actions.just_pressed(Action::ResetAccumulation) {
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
        let mut app = App::new(Config::default()).unwrap();
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
