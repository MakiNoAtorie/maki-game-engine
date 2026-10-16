//! The rules of the game (the game mode of Unreal, small): the state at the start of
//! a play, the pause action, and when the controller steers the pawn. The event
//! commands that change the state of the game and move the control.
//!
//! The state itself is `maki_core::PlayState`, so that each crate can read it.

use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, FixedUpdate, Interpolated, Module, PlayState, ResetPlay, Settings, Update};
use maki_events::{CommandRegistry, EventRunner, run_events};
use maki_platform::{Action, ActionState, Key, add_bindings};
use maki_scene::ObjectId;
use serde::{Deserialize, Serialize};

use crate::control::{Controller, give_control};
use crate::player::steer_player;

/// Changes between play and pause.
pub const PAUSE: Action = Action("pause");

/// The `[rules]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameRules {
    /// The state at the start of a play: `"Title"` or `"Play"`.
    pub start: PlayState,
    /// False: the pause action does nothing.
    pub pause: bool,
}

impl Default for GameRules {
    fn default() -> Self {
        Self {
            start: PlayState::Play,
            pause: true,
        }
    }
}

impl Settings for GameRules {
    const TABLE: &'static str = "rules";

    fn check(&self) -> Result<(), String> {
        match self.start {
            PlayState::Title | PlayState::Play => Ok(()),
            _ => Err("start must be \"Title\" or \"Play\"".to_owned()),
        }
    }
}

/// The event commands of this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GameCommand {
    /// The game goes to this state, if the table of `PlayState::can_change`
    /// allows it; else a warning, and no change. `Pause` needs the pause action on
    /// (`GameRules::pause`): only the action ends a pause.
    SetPlayState(PlayState),
    /// The control goes to the object. No object (`ObjectId::default()`, 0): back to
    /// the body of the player prefab.
    GiveControl(ObjectId),
}

/// The rules of the game, the pause action, and the commands `SetPlayState` and
/// `GiveControl`. Needs `maki_events::EventsModule` and `PlayerModule`.
pub struct GameModule;

impl Module for GameModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<GameRules>();
        add_bindings(app, &[(Key::P, PAUSE, "Pause")]);
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(|command: &GameCommand, step, world| {
            match command {
                GameCommand::SetPlayState(to) => {
                    let from = *world.resource::<PlayState>();
                    // No fixed step runs in a pause: only the pause action ends it.
                    let stuck = *to == PlayState::Pause && !world.resource::<GameRules>().pause;
                    if stuck || !world.resource_mut::<PlayState>().change(*to) {
                        tracing::warn!(
                            event = step.event,
                            "no change of the game from {from:?} to {to:?}"
                        );
                    }
                }
                GameCommand::GiveControl(name) => match named(world, name) {
                    Some(target) => drop(give_control(world, target)),
                    None => tracing::warn!(event = step.event, "GiveControl: no object `{name}`"),
                },
            }
            true
        });
        registry.add_texts(
            |command: &GameCommand| {
                Some(match command {
                    GameCommand::SetPlayState(state) => format!("State: {state:?}"),
                    GameCommand::GiveControl(object) if *object == ObjectId::default() => {
                        "Control: the player".to_owned()
                    }
                    GameCommand::GiveControl(name) => format!("Control: {name}"),
                })
            },
            |name| match name {
                "SetPlayState" => {
                    Some("The game goes to a state: Title, Play, Pause, Cutscene, End.")
                }
                "GiveControl" => {
                    Some("The move actions steer the object of this name. No name: the player.")
                }
                _ => None,
            },
        );
        registry.add_defaults(|name, _| match name {
            "SetPlayState" => Some(GameCommand::SetPlayState(PlayState::Cutscene)),
            "GiveControl" => Some(GameCommand::GiveControl(ObjectId::default())),
            _ => None,
        });
        app.init_resource::<Controller>()
            .add_systems(
                FixedUpdate,
                apply_rules.after(run_events).before(steer_player),
            )
            .add_systems(Update, pause_on_action)
            .add_systems(ResetPlay, start_state);
    }
}

/// The first entity of the object; the home body for no object.
fn named(world: &mut World, object: &ObjectId) -> Option<Entity> {
    if *object == ObjectId::default() {
        return world.resource::<Controller>().home;
    }
    let mut objects = world.query_filtered::<(Entity, &ObjectId), With<Interpolated>>();
    objects
        .iter(world)
        .find_map(|(entity, id)| (id == object).then_some(entity))
}

/// Runs in `FixedUpdate`: the rule of the control. The pawn takes the move actions
/// in the state play, while no event blocks it (a message, a fade).
pub(crate) fn apply_rules(
    state: Res<PlayState>,
    events: Res<EventRunner>,
    mut controller: ResMut<Controller>,
) {
    let active = *state == PlayState::Play && !events.blocks_player();
    if controller.active != active {
        controller.active = active;
    }
}

/// Runs in `Update`: the pause action changes between play and pause. In `Update`,
/// as no fixed step runs in a pause.
fn pause_on_action(actions: Res<ActionState>, rules: Res<GameRules>, mut state: ResMut<PlayState>) {
    if !(rules.pause && actions.just_pressed(PAUSE)) {
        return;
    }
    let to = match *state {
        PlayState::Pause => PlayState::Play,
        _ => PlayState::Pause,
    };
    state.change(to);
}

/// Runs in `ResetPlay`: a play starts in the start state of the rules.
fn start_state(rules: Res<GameRules>, mut state: ResMut<PlayState>) {
    if *state != rules.start {
        *state = rules.start;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_ecs::component::Component;
    use bevy_ecs::system::Commands;
    use glam::Vec3;
    use maki_core::{ConfigFile, Fields, Startup, Time, Transform};
    use maki_events::{EventsModule, GameState, Script, ScriptContext, add_script};
    use maki_platform::{PlatformModule, RawInput};
    use maki_scene::{Name, Player, SceneModule};

    use super::*;
    use crate::{ControlsModule, PlayerModule};

    const STEP: Duration = Duration::from_millis(20);

    /// The commands that [`Driver`] calls: the variable `call` of the game state is
    /// an index into this list, plus one.
    fn calls() -> [GameCommand; 7] {
        [
            GameCommand::GiveControl(ObjectId(5)),
            GameCommand::GiveControl(ObjectId::default()),
            GameCommand::SetPlayState(PlayState::Play),
            GameCommand::SetPlayState(PlayState::Cutscene),
            GameCommand::SetPlayState(PlayState::Title),
            GameCommand::GiveControl(ObjectId(99)),
            GameCommand::SetPlayState(PlayState::Pause),
        ]
    }

    /// A script that calls a command of [`calls`] when the test asks.
    #[derive(Component, Default, Serialize, Deserialize, Fields)]
    struct Driver {}

    impl Script for Driver {
        fn step(&mut self, context: &mut ScriptContext) {
            let call = context.state().variable("call");
            if call > 0 {
                context.state().set_variable("call", 0);
                context.call(calls()[call as usize - 1].clone());
            }
        }
    }

    /// A headless game with the player at the origin and the object "box" with the
    /// driver script at x = 3.
    fn game(config: &str) -> App {
        let mut app = App::new(ConfigFile::from_text(&format!(
            "[time]\nfixed_hz = 50.0\n{config}"
        )))
        .unwrap();
        app.add_module(PlatformModule)
            .add_module(maki_assets::AssetsModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_module(ControlsModule)
            .add_module(PlayerModule::default())
            .add_module(GameModule);
        add_script::<Driver>(&mut app, "driver");
        app.add_systems(Startup, |mut commands: Commands| {
            let at = Transform::from_translation(Vec3::new(3.0, 0.0, 0.0));
            commands.spawn((
                Driver {},
                Name("box".to_owned()),
                ObjectId(5),
                Interpolated::at(at),
            ));
        });
        app
    }

    fn call(app: &mut App, index: usize) {
        app.resource_mut::<GameState>()
            .set_variable("call", index as i32 + 1);
        app.advance(STEP);
    }

    /// The positions of the home body and of the box, and the name of the pawn.
    fn state(app: &App) -> (Vec3, Vec3, String) {
        let world = app.world();
        let home = world.resource::<Controller>().home.unwrap();
        let position = |entity| {
            world
                .get::<Interpolated>(entity)
                .unwrap()
                .current
                .translation
        };
        let mut boxes = world.try_query::<(Entity, &Name)>().unwrap();
        let (the_box, _) = boxes.single(world).unwrap();
        let mut pawns = world.try_query_filtered::<Entity, With<Player>>().unwrap();
        let pawns: Vec<Entity> = pawns.iter(world).collect();
        let pawn = match pawns[..] {
            [pawn] if pawn == home => "home",
            [pawn] if pawn == the_box => "box",
            _ => "not one",
        };
        (position(home), position(the_box), pawn.to_owned())
    }

    fn walk(app: &mut App, steps: u32) {
        for _ in 0..steps {
            app.advance(STEP);
        }
    }

    #[test]
    fn the_control_moves_to_another_object_and_back() {
        let app = game("");
        app.run(|mut app| {
            app.write_message(RawInput::KeyDown(Key::W));
            walk(&mut app, 5);
            let (home, the_box, pawn) = state(&app);
            assert_eq!(pawn, "home");
            assert!(home.z < 0.0 && the_box == Vec3::new(3.0, 0.0, 0.0));

            // A script calls the command, as a row: the box walks, the home body stands.
            call(&mut app, 0);
            let (home_before, _, pawn) = state(&app);
            assert_eq!(pawn, "box");
            walk(&mut app, 5);
            let (home_after, the_box, _) = state(&app);
            assert_eq!(home_after, home_before);
            assert!(the_box.z < 0.0 && the_box.x == 3.0);

            // An unknown name changes nothing; an empty name gives the control back.
            call(&mut app, 5);
            assert_eq!(state(&app).2, "box");
            call(&mut app, 1);
            assert_eq!(state(&app).2, "home");

            // A new play gives the control back too.
            call(&mut app, 0);
            assert_eq!(state(&app).2, "box");
            app.reset_play();
            assert_eq!(state(&app).2, "home");
        });
    }

    #[test]
    fn the_game_changes_states_by_rows_scripts_and_the_pause_action() {
        let app = game("[rules]\nstart = \"Title\"");
        app.run(|mut app| {
            let play_state = |app: &App| *app.world().resource::<PlayState>();
            // The rules give the start state; the pawn does not move in the title.
            assert_eq!(play_state(&app), PlayState::Title);
            app.write_message(RawInput::KeyDown(Key::W));
            walk(&mut app, 5);
            assert_eq!(state(&app).0, Vec3::ZERO);

            // A row reads the command as the script calls it.
            let registry = app.world().resource::<CommandRegistry>().clone();
            let row = registry.parse_command("SetPlayState(Cutscene)").unwrap();
            assert_eq!(row.get::<GameCommand>(), Some(&calls()[3]));

            call(&mut app, 2);
            assert_eq!(play_state(&app), PlayState::Play);
            walk(&mut app, 3);
            assert!(state(&app).0.z < 0.0);

            // A cutscene takes the control; a change that the table refuses: none.
            call(&mut app, 3);
            let before = state(&app).0;
            walk(&mut app, 3);
            assert_eq!(state(&app).0, before);
            call(&mut app, 4);
            assert_eq!(play_state(&app), PlayState::Cutscene);

            // The pause action: only from play. No fixed step in the pause.
            call(&mut app, 2);
            app.write_message(RawInput::KeyDown(Key::P))
                .write_message(RawInput::KeyUp(Key::P));
            walk(&mut app, 1);
            assert_eq!(play_state(&app), PlayState::Pause);
            let tick = app.world().resource::<Time>().fixed_tick;
            walk(&mut app, 3);
            assert_eq!(app.world().resource::<Time>().fixed_tick, tick);
            app.write_message(RawInput::KeyDown(Key::P))
                .write_message(RawInput::KeyUp(Key::P));
            walk(&mut app, 2);
            assert_eq!(play_state(&app), PlayState::Play);
            assert!(app.world().resource::<Time>().fixed_tick > tick);

            // A new play starts in the start state.
            app.reset_play();
            assert_eq!(play_state(&app), PlayState::Title);
        });
    }

    #[test]
    fn a_row_does_not_pause_a_game_with_no_pause_action() {
        let app = game(
            "[rules]
pause = false",
        );
        app.run(|mut app| {
            call(&mut app, 6);
            assert_eq!(*app.world().resource::<PlayState>(), PlayState::Play);
        });
        let app = game("");
        app.run(|mut app| {
            call(&mut app, 6);
            assert_eq!(*app.world().resource::<PlayState>(), PlayState::Pause);
        });
    }

    #[test]
    fn the_pawn_stands_while_an_event_blocks_it() {
        let directory = std::env::temp_dir().join(format!("maki-rules-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let room = r#"(width: 8, depth: 8, height: 3, objects: [(id: 1, name: "start", components: {"spawn": ()}),
            (name: "wait", components: {"event": (pages: [
                (trigger: Autorun, commands: [Wait(0.2), SetFlag("done", true)]),
                (conditions: [Flag("done")], trigger: Interact, commands: []),
            ])}),
        ])"#;
        std::fs::write(directory.join("a.ron"), room).unwrap();
        let mut app = game(
            "[assets]
background = false",
        );
        app.add_module(crate::RoomsModule {
            start_room: directory.join("a.ron"),
            start_spawn: ObjectId(1),
        });
        app.run(|mut app| {
            let z = |app: &App| {
                let world = app.world();
                let home = world.resource::<Controller>().home.unwrap();
                world
                    .get::<Interpolated>(home)
                    .unwrap()
                    .current
                    .translation
                    .z
            };
            app.write_message(RawInput::KeyDown(Key::W));
            walk(&mut app, 5);
            assert_eq!(z(&app), 0.0);
            walk(&mut app, 10);
            assert!(app.world().resource::<GameState>().flag("done"));
            assert!(z(&app) < 0.0);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
