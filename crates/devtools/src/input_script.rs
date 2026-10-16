//! Input script: a text file feeds keys, text, and the mouse to a tool (the editor) in
//! the process, with no OS focus and no input of the desktop. A check by hand then
//! runs while the user works.
//!
//! `maki-editor game CONFIG --input-script FILE [--input-shots DIR]`
//!
//! One step on each line. `#` starts a comment line.
//! - `wait N`: N frames with no new step.
//! - `press KEYS`, `release KEYS`, `tap KEYS`: the key labels of the bindings joined
//!   by `+` ("W", "F5", "Ctrl+P", "Esc"). `tap` holds the keys for one frame.
//! - `type TEXT`: the rest of the line as text into the egui field with the focus.
//! - `move X Y`, `click X Y`, `drag X1 Y1 X2 Y2`: the left mouse button, at window
//!   pixels (as a shot shows them).
//! - `command ID`: runs a tool command by id ("project.new_script"), as the command
//!   search does.
//! - `shot NAME`: writes the window (picture and panels) to `DIR/NAME.png`.
//! - `quit`: ends the program; changes that are not saved are lost.
//!
//! A key goes to the input actions (`RawInput`, so F5 plays) and to egui
//! (`ScriptedUiInput`, so menus and text fields work). Each step logs
//! `input script step=...` at info.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::world::World;
use maki_core::{App, Input, Module};
use maki_platform::{Key, QuitHold, QuitRequested, RawInput, update_actions};
use maki_render::{FrameGraph, RenderGpu};
use maki_ui::{ScriptedUiInput, egui, run_command};

/// One step of a script, as the runner gives it. `click` and `drag` are some of these.
#[derive(Debug, Clone, PartialEq)]
pub enum InputStep {
    Wait(u32),
    /// A key down (true) or up (false).
    Key(Key, bool),
    Text(String),
    /// The pointer goes to this place of the window, in physical pixels.
    Move([f32; 2]),
    Command(String),
    Shot(String),
    Quit,
}

/// Reads the text of a script. The error names the line.
fn parse_script(text: &str) -> anyhow::Result<Vec<InputStep>> {
    let mut steps = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        parse_line(line, &mut steps).with_context(|| format!("line {}: `{line}`", index + 1))?;
    }
    Ok(steps)
}

fn parse_line(line: &str, steps: &mut Vec<InputStep>) -> anyhow::Result<()> {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    let numbers = || -> anyhow::Result<Vec<f32>> {
        let numbers = rest.split_whitespace().map(str::parse::<f32>);
        Ok(numbers.collect::<Result<_, _>>()?)
    };
    let keys = || -> anyhow::Result<Vec<Key>> {
        let keys = rest.split('+').map(|label| {
            Key::from_label(label.trim()).with_context(|| format!("no key `{label}`"))
        });
        keys.collect()
    };
    let down = |keys: &[Key]| {
        keys.iter()
            .map(|key| InputStep::Key(*key, true))
            .collect::<Vec<_>>()
    };
    // Release in the other order: Ctrl goes up after P.
    let up = |keys: &[Key]| {
        keys.iter()
            .rev()
            .map(|key| InputStep::Key(*key, false))
            .collect::<Vec<_>>()
    };
    let button = |down| InputStep::Key(Key::MouseLeft, down);
    match word {
        "wait" => steps.push(InputStep::Wait(
            rest.parse().context("wait needs a frame count")?,
        )),
        "press" => steps.extend(down(&keys()?)),
        "release" => steps.extend(up(&keys()?)),
        "tap" => {
            let keys = keys()?;
            steps.extend(down(&keys));
            steps.push(InputStep::Wait(1));
            steps.extend(up(&keys));
        }
        "type" if !rest.is_empty() => steps.push(InputStep::Text(rest.to_owned())),
        // egui finds a click or a drag from events in more than one frame.
        "move" | "click" | "drag" => match (word, numbers()?.as_slice()) {
            ("move", &[x, y]) => steps.push(InputStep::Move([x, y])),
            ("click", &[x, y]) => steps.extend([
                InputStep::Move([x, y]),
                InputStep::Wait(1),
                button(true),
                InputStep::Wait(1),
                button(false),
            ]),
            ("drag", &[x1, y1, x2, y2]) => steps.extend([
                InputStep::Move([x1, y1]),
                InputStep::Wait(1),
                button(true),
                InputStep::Wait(1),
                InputStep::Move([(x1 + x2) / 2.0, (y1 + y2) / 2.0]),
                InputStep::Wait(1),
                InputStep::Move([x2, y2]),
                InputStep::Wait(1),
                button(false),
            ]),
            _ => bail!(
                "{word} needs {} numbers",
                if word == "drag" { 4 } else { 2 }
            ),
        },
        "command" if !rest.is_empty() => steps.push(InputStep::Command(rest.to_owned())),
        "shot" if !rest.is_empty() => steps.push(InputStep::Shot(rest.to_owned())),
        "quit" => steps.push(InputStep::Quit),
        _ => bail!("unknown step"),
    }
    Ok(())
}

/// Gives the steps of each frame.
#[derive(Debug, Default)]
struct Runner {
    steps: VecDeque<InputStep>,
    /// Frames to wait before the next step.
    wait: u32,
}

impl Runner {
    fn new(steps: Vec<InputStep>) -> Self {
        Self {
            steps: steps.into(),
            wait: 0,
        }
    }

    fn done(&self) -> bool {
        self.steps.is_empty() && self.wait == 0
    }

    /// The steps of this frame: all up to the next wait, with the wait (for the log). A shot ends the frame too:
    /// the picture of this frame is read at the start of the next one.
    fn frame(&mut self) -> Vec<InputStep> {
        let mut steps = Vec::new();
        if self.wait > 0 {
            self.wait -= 1;
            return steps;
        }
        while let Some(step) = self.steps.pop_front() {
            match step {
                // This frame is the first of the N.
                InputStep::Wait(frames) => {
                    self.wait = frames.saturating_sub(1);
                    steps.push(step);
                    break;
                }
                InputStep::Shot(_) | InputStep::Quit => {
                    steps.push(step);
                    break;
                }
                step => steps.push(step),
            }
        }
        steps
    }
}

/// Runs a script from the first frame. The editor program adds it for `--input-script`.
pub struct InputScriptModule {
    pub steps: Vec<InputStep>,
    /// The folder of the shots.
    pub shots: PathBuf,
}

/// Reads a script file.
pub fn read_input_script(path: &Path) -> anyhow::Result<Vec<InputStep>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse_script(&text).with_context(|| format!("input script {}", path.display()))
}

#[derive(Resource)]
struct ScriptState {
    runner: Runner,
    shots: PathBuf,
    /// The shot to write at the start of the next frame.
    shot: Option<String>,
    /// The place of the pointer: a button event of egui has it.
    pointer: [f32; 2],
    modifiers: egui::Modifiers,
    ended: bool,
}

impl Module for InputScriptModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptedUiInput>()
            .insert_resource(ScriptState {
                runner: Runner::new(self.steps.clone()),
                shots: self.shots.clone(),
                shot: None,
                pointer: [0.0; 2],
                modifiers: egui::Modifiers::NONE,
                ended: false,
            })
            // The keys of a step start their actions in the same frame. It has the
            // whole world (a command); the order to the rest of `Input` is free.
            .add_systems(
                Input,
                run_input_script.before(update_actions).ambiguous_with_all(),
            );
    }
}

fn run_input_script(world: &mut World) {
    world.resource_scope(|world, mut state: bevy_ecs::world::Mut<ScriptState>| {
        if let Some(name) = state.shot.take() {
            let path = state.shots.join(format!("{name}.png"));
            match write_shot(world, &path) {
                Ok(()) => tracing::info!(path = %path.display(), "input script shot"),
                Err(error) => {
                    tracing::error!(path = %path.display(), "input script shot: {error:#}")
                }
            }
        }
        for step in state.runner.frame() {
            tracing::info!(step = ?step, "input script step");
            apply(world, &mut state, step);
        }
        if state.runner.done() && !state.ended && state.shot.is_none() {
            state.ended = true;
            tracing::info!("input script done");
        }
    });
}

fn apply(world: &mut World, state: &mut ScriptState, step: InputStep) {
    let mut ui = Vec::new();
    match step {
        InputStep::Wait(_) => {}
        InputStep::Key(key, down) => {
            world.write_message(if down {
                RawInput::KeyDown(key)
            } else {
                RawInput::KeyUp(key)
            });
            let modifiers = &mut state.modifiers;
            match key {
                Key::Control | Key::Shift | Key::Alt => {
                    match key {
                        // `command` is Ctrl outside macOS.
                        Key::Control => (modifiers.ctrl, modifiers.command) = (down, down),
                        Key::Shift => modifiers.shift = down,
                        _ => modifiers.alt = down,
                    }
                    ui.push(egui::Event::ModifiersChanged(*modifiers));
                }
                Key::MouseLeft | Key::MouseRight | Key::MouseMiddle => {
                    let button = match key {
                        Key::MouseLeft => egui::PointerButton::Primary,
                        Key::MouseRight => egui::PointerButton::Secondary,
                        _ => egui::PointerButton::Middle,
                    };
                    ui.push(egui::Event::PointerButton {
                        pos: state.pointer.into(),
                        button,
                        pressed: down,
                        modifiers: state.modifiers,
                    });
                }
                _ => {
                    // A key that egui does not know (Page up) goes to the actions only.
                    if let Some(egui_key) = egui::Key::from_name(key.label()) {
                        ui.push(egui::Event::Key {
                            key: egui_key,
                            physical_key: Some(egui_key),
                            pressed: down,
                            repeat: false,
                            modifiers: state.modifiers,
                        });
                    }
                }
            }
        }
        InputStep::Text(text) => ui.push(egui::Event::Text(text)),
        InputStep::Move(at) => {
            state.pointer = at;
            world.write_message(RawInput::PointerMoved(at));
            ui.push(egui::Event::PointerMoved(at.into()));
        }
        InputStep::Command(id) => {
            if !run_command(world, &id) {
                tracing::warn!(%id, "input script: the command is unknown or cannot run now");
            }
        }
        InputStep::Shot(name) => state.shot = Some(name),
        InputStep::Quit => {
            // The program ends with no question about changes that are not saved.
            world.resource_mut::<QuitHold>().0 = false;
            world.resource_mut::<QuitRequested>().0 = true;
        }
    }
    world.resource_mut::<ScriptedUiInput>().events.extend(ui);
}

/// Writes the window image of the last frame (the picture and the egui panels) as PNG.
fn write_shot(world: &mut World, path: &Path) -> anyhow::Result<()> {
    let graph = world.get_resource::<FrameGraph>().context("no renderer")?;
    let id = graph.resource("window").context("no window image")?;
    let (image, (width, height), access) = graph.image(id).context("no frame was drawn")?;
    let mut gpu = world.resource_mut::<RenderGpu>();
    let mut pixels = gpu.0.read_image(image, access)?;
    // The window shows no alpha.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image::save_buffer(
        path,
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::ConfigFile;
    use maki_platform::{Action, ActionState, PlatformModule, add_bindings};
    use maki_ui::{ToolCommand, UiModule, add_command};

    use super::*;

    #[test]
    fn a_script_text_gives_its_steps() {
        let text = "# a comment\n\nwait 3\ntap Ctrl+P\ntype New script\nclick 10 20\ncommand editor.save\nshot one\nquit\n";
        let steps = parse_script(text).unwrap();
        use InputStep::*;
        assert_eq!(
            steps,
            [
                Wait(3),
                Key(maki_platform::Key::Control, true),
                Key(maki_platform::Key::P, true),
                Wait(1),
                Key(maki_platform::Key::P, false),
                Key(maki_platform::Key::Control, false),
                Text("New script".into()),
                Move([10.0, 20.0]),
                Wait(1),
                Key(maki_platform::Key::MouseLeft, true),
                Wait(1),
                Key(maki_platform::Key::MouseLeft, false),
                Command("editor.save".into()),
                Shot("one".into()),
                Quit,
            ]
        );
        for bad in [
            "jump",
            "wait",
            "wait x",
            "press Hyper",
            "click 1",
            "drag 1 2 3",
            "type",
        ] {
            let error = parse_script(&format!("wait 1\n{bad}")).unwrap_err();
            assert!(format!("{error:#}").contains("line 2"), "{bad}: {error:#}");
        }
    }

    #[test]
    fn the_runner_gives_the_steps_of_each_frame() {
        use InputStep::*;
        let mut runner = Runner::new(vec![
            Key(maki_platform::Key::W, true),
            Wait(2),
            Key(maki_platform::Key::W, false),
            Shot("a".into()),
            Command("x".into()),
        ]);
        assert_eq!(runner.frame(), [Key(maki_platform::Key::W, true), Wait(2)]);
        assert_eq!(runner.frame(), []);
        assert_eq!(
            runner.frame(),
            [Key(maki_platform::Key::W, false), Shot("a".into())]
        );
        assert!(!runner.done());
        assert_eq!(runner.frame(), [Command("x".into())]);
        assert!(runner.done());
        assert_eq!(runner.frame(), []);
    }

    /// No window and no GPU: the keys reach the actions, and a command runs.
    #[test]
    fn a_script_presses_keys_and_runs_commands() {
        const WALK: Action = Action("walk");
        let mut app = maki_core::App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule).add_module(UiModule);
        add_bindings(&mut app, &[(maki_platform::Key::W, WALK, "Walk")]);
        add_command(
            &mut app,
            ToolCommand::new("test.mark", "Mark", |world| {
                world.resource_mut::<QuitRequested>().0 = true;
            }),
        );
        let steps = parse_script("press W\nwait 2\nrelease W\ncommand test.mark\n").unwrap();
        app.add_module(InputScriptModule {
            steps,
            shots: PathBuf::new(),
        });
        app.run(|mut app| {
            let mut frame = || {
                app.advance(Duration::from_millis(16));
                let walk = app.world().resource::<ActionState>().pressed(WALK);
                (walk, app.world().resource::<QuitRequested>().0)
            };
            assert_eq!(frame(), (true, false));
            assert_eq!(frame(), (true, false));
            assert_eq!(frame(), (false, true));
            app
        });
    }
}
