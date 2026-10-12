//! The OS window and the winit event loop that drives the app.
//! winit types stay in this file and in the key table of `input`, with one exception for the UI: `egui-winit` reads
//! winit events, so [`RawWindowEvent`] and [`Window::winit`] show them to the ui crate.

use std::sync::Arc;

use anyhow::Result;
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};
use maki_core::{App, Settings};
use serde::{Deserialize, Serialize};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
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

/// Room in the window around the picture, in physical pixels. The module that draws
/// there writes it (the panels of the ui crate). [`Picture`] is made from it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WindowMargin {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

/// Where the picture is in the window, and how large it is, in physical pixels. The
/// one place that tells it: the renderer, the UI, and a tool read it, and
/// [`update_picture`] alone writes it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// The top left corner, from the top left corner of the window.
    pub left: u32,
    pub top: u32,
    /// Never 0.
    pub width: u32,
    pub height: u32,
}

impl Picture {
    fn of_settings(config: &WindowSettings, margin: WindowMargin) -> Self {
        Self {
            left: margin.left,
            top: margin.top,
            width: config.width.max(1),
            height: config.height.max(1),
        }
    }

    /// A place of the window (the pointer) as a place in the picture: from its top
    /// left corner. It can be out of the picture.
    pub fn from_window(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [x - self.left as f32, y - self.top as f32]
    }

    pub fn size(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }
}

/// What decides the size of the picture.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PictureFit {
    /// The game: the picture has the size of the `[window]` settings, and the
    /// window is as large as the picture and the margin.
    #[default]
    Settings,
    /// A tool: the user sets the size of the window (it starts maximized), and the
    /// picture has the room that the margin leaves.
    Window,
}

/// A new size of the room for the picture is the size of the picture after this many
/// frames with no other change.
// Each picture size makes each image of the renderer again and starts the picture
// again. In a drag of a window edge or a panel edge the size changes in each frame.
const PICTURE_REST_FRAMES: u32 = 8;

/// Runs in `Extract`: the picture for this frame. A system that copies it for a
/// renderer runs `.after(update_picture)`.
pub fn update_picture(
    config: Res<WindowSettings>,
    margin: Res<WindowMargin>,
    fit: Res<PictureFit>,
    size: Option<Res<WindowSize>>,
    mut picture: ResMut<Picture>,
    // The size that waits, and the frames that it did not change.
    mut waiting: Local<Option<((u32, u32), u32)>>,
) {
    let mut new = Picture::of_settings(&config, *margin);
    if let (PictureFit::Window, Some(size)) = (*fit, size) {
        let room = (
            size.width.saturating_sub(margin.left + margin.right).max(1),
            size.height
                .saturating_sub(margin.top + margin.bottom)
                .max(1),
        );
        let frames = match *waiting {
            Some((size, frames)) if size == room => frames + 1,
            _ => 0,
        };
        *waiting = Some((room, frames));
        // The place follows at once. The old size is cut by the window.
        (new.width, new.height) = if frames >= PICTURE_REST_FRAMES {
            room
        } else {
            (picture.width, picture.height)
        };
    }
    // Only a different value is a write: a change starts the picture again.
    picture.set_if_neq(new);
}

/// The text of the title bar of the window. A tool writes it (the open file).
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct WindowTitle(pub String);

pub(crate) fn apply_title(title: Res<WindowTitle>, window: Res<Window>) {
    use bevy_ecs::change_detection::DetectChanges;
    if title.is_changed() {
        window.0.set_title(&title.0);
    }
}

/// The window and the picture in it. The `[window]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowSettings {
    pub title: String,
    /// Size of the picture in pixels. The window shows it not scaled, and is wider
    /// by the panel column of the UI.
    pub width: u32,
    pub height: u32,
    /// True: each frame waits for the display.
    pub vsync: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            title: "maki".to_owned(),
            width: 1280,
            height: 720,
            vsync: true,
        }
    }
}

impl Settings for WindowSettings {
    const TABLE: &'static str = "window";
}

/// The size of the window for a picture size and a margin.
fn window_size(config: &WindowSettings, margin: WindowMargin) -> PhysicalSize<u32> {
    PhysicalSize::new(
        margin.left + config.width + margin.right,
        margin.top + config.height + margin.bottom,
    )
}

/// Gives the window a new size when the picture size of the settings or the margin
/// changes (a resolution selector, another display scale).
pub(crate) fn fit_window(
    config: Res<WindowSettings>,
    margin: Res<WindowMargin>,
    fit: Res<PictureFit>,
    window: Res<Window>,
    mut last: Local<Option<PhysicalSize<u32>>>,
) {
    // The user sets the size of the window of a tool.
    if *fit == PictureFit::Window {
        return;
    }
    let target = window_size(&config, *margin);
    // Only a new target is a request: a size that the user gave the window stays.
    if last.replace(target) != Some(target) && window.0.inner_size() != target {
        // The OS can give another size (the screen limits). `Resized` tells it.
        let _ = window.0.request_inner_size(target);
    }
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

/// Set `0` to true to end the app after the current frame. The close button of the
/// window does the same.
#[derive(Resource, Debug, Default)]
pub struct QuitRequested(pub bool);

/// True: a quit request does not end the app. The module that wrote this reads
/// `QuitRequested`, sets it back, and decides (an editor with changes that are not
/// saved). To end the app, it sets this to false and the request to true.
#[derive(Resource, Debug, Default)]
pub struct QuitHold(pub bool);

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

        let config = app.world().resource::<WindowSettings>();
        let margin = *app.world().resource::<WindowMargin>();
        let title = &app.world().resource::<WindowTitle>().0;
        let maximized = *app.world().resource::<PictureFit>() == PictureFit::Window;
        let attributes = winit::window::Window::default_attributes()
            .with_title(title)
            .with_inner_size(window_size(config, margin))
            .with_maximized(maximized);
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
            // The next frame ends the app, if no module holds it open (`QuitHold`).
            WindowEvent::CloseRequested => app.resource_mut::<QuitRequested>().0 = true,
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
                    && let Some(key) = Key::from_winit(code)
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
            WindowEvent::MouseInput { state, button, .. } => {
                let key = match button {
                    MouseButton::Left => Key::MouseLeft,
                    MouseButton::Right => Key::MouseRight,
                    MouseButton::Middle => Key::MouseMiddle,
                    _ => return,
                };
                app.write_message(match state {
                    ElementState::Pressed => RawInput::KeyDown(key),
                    ElementState::Released => RawInput::KeyUp(key),
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                let at = [position.x as f32, position.y as f32];
                app.write_message(RawInput::PointerMoved(at));
            }
            WindowEvent::CursorLeft { .. } => {
                app.write_message(RawInput::PointerLeft);
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
                let world = app.world();
                if world.resource::<QuitRequested>().0 && !world.resource::<QuitHold>().0 {
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
