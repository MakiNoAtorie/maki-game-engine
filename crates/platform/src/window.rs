//! The OS window and the winit event loop that drives the app.
//! winit types stay in this file, with one exception for the UI: `egui-winit` reads
//! winit events, so [`RawWindowEvent`] and [`Window::winit`] show them to the ui crate.

use std::sync::Arc;

use anyhow::Result;
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use maki_core::{App, Config};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use winit::window::WindowId;

use crate::input::{Key, RawInput};

/// The OS window. Exists from before `Startup` until the app ends.
/// The gpu crate makes its surface from the two `raw-window-handle` traits.
#[derive(Resource)]
pub struct Window(Arc<winit::window::Window>);

impl Window {
    /// The winit window. Only for code that hands it to a winit-based library
    /// (`egui-winit`). Other code uses `WindowSize` and the input actions.
    pub fn winit(&self) -> &winit::window::Window {
        &self.0
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

/// Size of the drawable area in physical pixels. Updated in the `Input` schedule.
/// Never 0: a minimized window keeps its last size, and no frame runs until it is restored.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// Sent when the drawable area changed size. Same units as `WindowSize`.
#[derive(Message, Debug, Clone, Copy)]
pub struct WindowResized {
    pub width: u32,
    pub height: u32,
}

/// Each window event as winit sent it. Only for a winit-based library (`egui-winit`):
/// gameplay code reads the input actions.
#[derive(Message, Debug, Clone)]
pub struct RawWindowEvent(pub WindowEvent);

/// Set `0` to true to end the app after the current frame.
#[derive(Resource, Debug, Default)]
pub struct QuitRequested(pub bool);

pub(crate) fn update_window_size(
    mut resized: MessageReader<WindowResized>,
    mut size: ResMut<WindowSize>,
) {
    // Only the last size of a frame is important.
    if let Some(event) = resized.read().last() {
        let new = WindowSize {
            width: event.width,
            height: event.height,
        };
        // Windows sends the old size again when a minimized window comes back. Only a
        // different size is a change: systems that watch `WindowSize` do work on a change.
        if size.set_if_neq(new) {
            tracing::debug!(size = ?new, "window resized");
        }
    }
}

/// Opens the window, runs `Startup`, then runs one frame per redraw until the app quits.
/// Call this in place of `App::run`: winit can make a window only inside its event loop,
/// and `Startup` systems need the window.
pub fn run(app: App) -> Result<()> {
    let event_loop = EventLoop::new()?;
    // Poll: do not wait for OS events between frames.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut runner = Runner {
        app: Some(app),
        window: None,
        minimized: false,
        error: None,
    };
    event_loop.run_app(&mut runner)?;
    runner.error.map_or(Ok(()), Err)
}

struct Runner {
    /// `None` only for a moment in `resumed`, while `App::run` owns the app.
    app: Option<App>,
    window: Option<Arc<winit::window::Window>>,
    /// No frames run while this is true: the game is paused.
    minimized: bool,
    /// The winit callbacks cannot return an error, so it waits here for `run`.
    error: Option<anyhow::Error>,
}

impl ApplicationHandler for Runner {
    // Called one time at start on desktop systems.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut app = self.app.take().expect("app is present outside resumed");

        let config = app.world().resource::<Config>();
        let attributes = winit::window::Window::default_attributes()
            .with_title(&config.window_title)
            .with_inner_size(LogicalSize::new(config.window_width, config.window_height));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                self.error = Some(e.into());
                event_loop.exit();
                return;
            }
        };

        // The OS can give a size different from the request (display scale, screen limits).
        let size = window.inner_size();
        app.insert_resource(Window(window.clone()))
            .insert_resource(WindowSize {
                width: size.width,
                height: size.height,
            });
        self.window = Some(window);
        // `run` does Startup. The driver gives the app back: this handler owns the loop.
        self.app = Some(app.run(|app| app));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // No app: window creation failed and the loop is on its way out.
        let Some(app) = &mut self.app else { return };
        // RedrawRequested is the frame itself, not input.
        if event != WindowEvent::RedrawRequested {
            app.write_message(RawWindowEvent(event.clone()));
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                // A minimized window has a zero size. The renderer cannot use that size,
                // so it is not sent, and frames stop until the window is back.
                let minimized = size.width == 0 || size.height == 0;
                if minimized {
                    // Wait: sleep until the next OS event.
                    event_loop.set_control_flow(ControlFlow::Wait);
                } else {
                    if self.minimized {
                        event_loop.set_control_flow(ControlFlow::Poll);
                        app.reset_clock();
                    }
                    app.write_message(WindowResized {
                        width: size.width,
                        height: size.height,
                    });
                }
                self.minimized = minimized;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // `repeat`: the OS sends more key-downs while a key is held. Not a new press.
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(key) = key_from_winit(code)
                    && !event.repeat
                {
                    app.write_message(match event.state {
                        ElementState::Pressed => RawInput::KeyDown(key),
                        ElementState::Released => RawInput::KeyUp(key),
                    });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // A mouse sends lines, a touchpad sends pixels. Only the direction is used.
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32,
                };
                if y != 0.0 {
                    let key = if y > 0.0 {
                        Key::WheelUp
                    } else {
                        Key::WheelDown
                    };
                    // The wheel has no "held" state: down and up at once is a one-frame press.
                    app.write_message(RawInput::KeyDown(key))
                        .write_message(RawInput::KeyUp(key));
                }
            }
            WindowEvent::Focused(false) => {
                app.write_message(RawInput::FocusLost);
            }
            WindowEvent::RedrawRequested => {
                // Windows repaints a window one time when it is restored, before the
                // Resized event that ends the pause.
                if self.minimized {
                    return;
                }
                app.update();
                if app.world().resource::<QuitRequested>().0 {
                    event_loop.exit();
                }
                // While the user drags the window border or title bar, Windows runs its
                // own loop and `about_to_wait` is not called. Asking for the next frame
                // here keeps frames running during the drag.
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    // Called when all pending OS events are handled. Starts the frame chain at
    // startup and after a pause; each frame then asks for the next one.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window
            && !self.minimized
        {
            window.request_redraw();
        }
    }
}

fn key_from_winit(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyZ => Key::Z,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyO => Key::O,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::F1 => Key::F1,
        KeyCode::F5 => Key::F5,
        KeyCode::Escape => Key::Escape,
        _ => return None,
    })
}
