//! The screens of a game: the title, the pause, and the game over are menus of the
//! project. And the commands that a row of such a menu runs: a new game, the load of
//! the save, quit, game over.
//!
//! In `Title` the title menu shows, in `End` the game-over menu. The pause action in
//! play opens the pause menu; when it closes (a row with `CloseMenu`, the back
//! action), the game plays again. A menu of a state closes when the game leaves it.

use std::path::Path;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, ResMut};
use bevy_ecs::world::World;
use maki_assets::DataRef;
use maki_core::{App, Fields, Module, PlayState, ResetPlay, Settings, Update};
use maki_events::{CommandRegistry, GameState, Menu, MenuFiles, Menus};
use maki_platform::QuitRequested;
use maki_scene::{CurrentRoom, RoomRequest, load_requested_room};
use serde::{Deserialize, Serialize};

use crate::StartRoom;

/// The `[screens]` table of the config file: the menu files of the screens, from the
/// directory of the room files. A key that the file does not have takes the menu
/// that the game gave to [`ScreensModule`]. No menu: the state shows nothing.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenSettings {
    /// The menu of the state `Title`.
    pub title: Option<DataRef<Menu>>,
    /// The menu of the state `Pause`. Its close plays again.
    pub pause: Option<DataRef<Menu>>,
    /// The menu of the state `End`.
    pub game_over: Option<DataRef<Menu>>,
}

impl Settings for ScreenSettings {
    const TABLE: &'static str = "screens";
}

impl ScreenSettings {
    /// The menu of a state.
    fn menu(&self, state: PlayState) -> Option<&DataRef<Menu>> {
        match state {
            PlayState::Title => self.title.as_ref(),
            PlayState::Pause => self.pause.as_ref(),
            PlayState::End => self.game_over.as_ref(),
            PlayState::Play | PlayState::Cutscene => None,
        }
    }
}

/// The commands of a play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Fields)]
pub enum PlayCommand {
    /// A new game: the play state and the flags and variables go back to the start,
    /// the start room loads, and the game plays.
    NewGame,
    /// The game goes to the state of the save file and plays. No save file: no change.
    LoadSave,
    /// The program ends.
    QuitGame,
    /// The game is over: the state `End`, with the game-over menu.
    GameOver,
}

/// A new game or a load that a row asked for. A command cannot do them: the
/// `ResetPlay` of them sets the event runner back, and the runner of the command is
/// out of the world while the command runs.
#[derive(Resource, Debug, Default)]
struct PlayRequest(Option<PlayCommand>);

/// The screens and the commands of a play. Needs `maki_events::EventsModule` and
/// `GameModule`; `RoomsModule` for `NewGame`, `SaveModule` for `LoadSave`.
pub struct ScreensModule {
    /// The menus for a config file that names none.
    pub screens: ScreenSettings,
}

impl Module for ScreensModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<ScreenSettings>();
        // The resource holds the menus of this run: of the config, else of the game.
        let mut screens = app.resource_mut::<ScreenSettings>();
        let game = &self.screens;
        screens.title = screens.title.take().or_else(|| game.title.clone());
        screens.pause = screens.pause.take().or_else(|| game.pause.clone());
        screens.game_over = screens.game_over.take().or_else(|| game.game_over.clone());
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(|command: &PlayCommand, step, world| {
            match command {
                PlayCommand::NewGame | PlayCommand::LoadSave => {
                    world.resource_mut::<PlayRequest>().0 = Some(*command);
                }
                PlayCommand::QuitGame => world.resource_mut::<QuitRequested>().0 = true,
                PlayCommand::GameOver => {
                    if !world.resource_mut::<PlayState>().change(PlayState::End) {
                        let from = *world.resource::<PlayState>();
                        tracing::warn!(event = step.event, "GameOver: no end from {from:?}");
                    }
                }
            }
            true
        });
        registry.add_fields::<PlayCommand>();
        registry.add_texts(
            |command: &PlayCommand| {
                Some(
                    match command {
                        PlayCommand::NewGame => "New game",
                        PlayCommand::LoadSave => "Load the save",
                        PlayCommand::QuitGame => "Quit the game",
                        PlayCommand::GameOver => "Game over",
                    }
                    .to_owned(),
                )
            },
            |name| match name {
                "NewGame" => {
                    Some("A new game: the start room, and the flags and variables of the start.")
                }
                "LoadSave" => Some("Loads the save file and plays. No save file: no change."),
                "QuitGame" => Some("Ends the program."),
                "GameOver" => Some("The game is over: the state End and the game-over menu."),
                _ => None,
            },
        );
        registry.add_defaults(|name, _| match name {
            "NewGame" => Some(PlayCommand::NewGame),
            "LoadSave" => Some(PlayCommand::LoadSave),
            "QuitGame" => Some(PlayCommand::QuitGame),
            "GameOver" => Some(PlayCommand::GameOver),
            _ => None,
        });
        app.init_resource::<PlayRequest>()
            // It has the world: its order to the pause action and the loader counts.
            .add_systems(
                Update,
                show_screens
                    .after(crate::game::pause_on_action)
                    .before(load_requested_room)
                    .ambiguous_with_all(),
            )
            .add_systems(ResetPlay, |mut request: ResMut<PlayRequest>| {
                if request.0.is_some() {
                    request.0 = None;
                }
            });
    }
}

/// What [`show_screens`] keeps from frame to frame.
#[derive(Default)]
struct Shown {
    /// The state of the frame before.
    state: Option<PlayState>,
    /// True: the menu of the state opened.
    open: bool,
    /// True: the menu of the state did not read. It is not read again in the state.
    broken: bool,
}

/// Runs in `Update`: a new game or a load that a row asked for, then the menu of the
/// state of the game.
fn show_screens(world: &mut World, mut shown: Local<Shown>) {
    let request = world.resource::<PlayRequest>().0;
    if let Some(request) = request {
        world.resource_mut::<PlayRequest>().0 = None;
        if request == PlayCommand::NewGame {
            new_game(world);
        } else if crate::save::load(world) {
            play(world);
        }
    }
    let state = *world.resource::<PlayState>();
    let before = shown.state.replace(state);
    if before != Some(state) {
        shown.open = false;
        shown.broken = false;
        // The menus of the state before close: a title menu does not stay in play.
        let left = before.and_then(|before| world.resource::<ScreenSettings>().menu(before));
        if left.is_some() && world.resource::<Menus>().has_focus() {
            world.resource_mut::<Menus>().close_all();
        }
    }
    let Some(file) = world.resource::<ScreenSettings>().menu(state).cloned() else {
        return;
    };
    if world.resource::<Menus>().has_focus() {
        return;
    }
    if state == PlayState::Pause && shown.open {
        // The pause menu closed: the game plays again.
        world.resource_mut::<PlayState>().change(PlayState::Play);
    } else if !shown.broken {
        // The menu files and the text tables start at the directory of the room files.
        let room = world.get_resource::<CurrentRoom>();
        let Some(room) = room.filter(|room| room.loads > 0) else {
            return;
        };
        let path = file.file.resolve(room.path.parent().unwrap_or(&room.path));
        // A title menu opens again after a load: the `ResetPlay` of it closes the menus.
        match open(world, &path) {
            true => shown.open = true,
            false => shown.broken = true,
        }
    }
}

/// Opens the menu of a screen. False: the file did not read.
fn open(world: &mut World, path: &Path) -> bool {
    let registry = world.resource::<CommandRegistry>().clone();
    match world.resource_mut::<MenuFiles>().get(path, &registry) {
        Ok(menu) => {
            world.resource_mut::<Menus>().open(menu, None);
            true
        }
        Err(error) => {
            tracing::error!("the menu of the screen: {error}");
            false
        }
    }
}

/// A new game: the play state of each module and the flags and variables go back to
/// the start, the start room loads again, and the game plays.
fn new_game(world: &mut World) {
    world.run_schedule(ResetPlay);
    *world.resource_mut::<GameState>() = GameState::default();
    if let Some(start) = world.get_resource::<StartRoom>() {
        let target = start.target();
        world.resource_mut::<RoomRequest>().0 = Some(target);
    }
    play(world);
}

/// The game plays. After a `ResetPlay` the state is the start state: `Title` or
/// `Play`, and each of them can go to `Play`.
fn play(world: &mut World) {
    world.resource_mut::<PlayState>().change(PlayState::Play);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use maki_core::ConfigFile;
    use maki_events::{MenuRow, MessageBox, ShownMenu, ShownMenus, ShownRow, TextTable, read_menu};
    use maki_platform::{Key, RawInput};

    use super::*;
    use crate::{ControlsModule, GameModule, PlayerModule, RoomsModule, SaveData, SaveModule};

    const TITLE: &str = r#"(back: false, rows: [
        Commands(text: "new", commands: [NewGame]),
        Commands(text: "continue", conditions: [SaveExists], commands: [LoadSave]),
        Commands(text: "quit", commands: [QuitGame]),
    ])"#;
    const PAUSE: &str = r#"(rows: [
        Commands(text: "back", commands: [CloseMenu]),
        Commands(text: "to title", commands: [SetPlayState(Title)]),
        Commands(text: "say", commands: [Message("x")]),
    ])"#;
    const OVER: &str =
        r#"(back: false, rows: [Commands(text: "over", commands: [SetPlayState(Title)])])"#;

    /// A directory with the room `a.ron` (an event that ends the game with the flag
    /// `end`) and the menus of the screens in `menus/`.
    fn directory(test: &str) -> PathBuf {
        let name = format!("maki-screens-{test}-{}", std::process::id());
        let directory = std::env::temp_dir().join(name);
        std::fs::create_dir_all(directory.join("menus")).unwrap();
        for (name, text) in [("title", TITLE), ("pause", PAUSE), ("over", OVER)] {
            let path = directory.join(format!("menus/{name}.menu.ron"));
            std::fs::write(path, text).unwrap();
        }
        let room = maki_scene::room_text(
            4.0,
            4.0,
            3.0,
            &[
                r#"(id: 1, name: "start", components: {"spawn": ()})"#,
                r#"(name: "ender", components: {"event": (pages: [
                    (conditions: [Flag("end")], trigger: Autorun, commands: [GameOver]),
                ])})"#,
            ],
        );
        std::fs::write(directory.join("a.ron"), room).unwrap();
        directory
    }

    /// A headless game in `directory` that starts in the state `start`, with its save
    /// file there.
    fn game(directory: &Path, start: &str) -> App {
        let config = format!("[assets]\nbackground = false\n[rules]\nstart = \"{start}\"");
        let mut app = App::new(ConfigFile::from_text(&config)).unwrap();
        let menu = |name: &str| Some(DataRef::new(format!("menus/{name}.menu.ron").as_str()));
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_assets::AssetsModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule)
            .add_module(ControlsModule)
            .add_module(PlayerModule::default())
            .add_module(GameModule)
            .add_module(RoomsModule {
                start_room: directory.join("a.ron"),
                start_spawn: maki_scene::ObjectId(1),
            })
            .add_module(SaveModule {
                file: directory.join("save.ron"),
                active: true,
            })
            .add_module(ScreensModule {
                screens: ScreenSettings {
                    title: menu("title"),
                    pause: menu("pause"),
                    game_over: menu("over"),
                },
            });
        app
    }

    fn steps(app: &mut App, count: u32) {
        for _ in 0..count {
            app.advance(Duration::from_millis(20));
        }
    }

    /// A tap of a key, then frames with no key: a tap of the same key after it is a
    /// new press.
    fn tap(app: &mut App, key: Key) {
        app.write_message(RawInput::KeyDown(key))
            .write_message(RawInput::KeyUp(key));
        steps(app, 3);
    }

    /// The rows of each shown menu, with a mark at the focus.
    fn rows(app: &App) -> Vec<Vec<String>> {
        let line = |row: &ShownRow| format!("{}{}", if row.focus { "> " } else { "" }, row.text);
        let menu = |menu: &ShownMenu| menu.rows.iter().map(line).collect();
        app.world()
            .resource::<ShownMenus>()
            .0
            .iter()
            .map(menu)
            .collect()
    }

    fn state(app: &App) -> PlayState {
        *app.world().resource::<PlayState>()
    }

    fn flag(app: &App, name: &str) -> bool {
        app.world().resource::<GameState>().flag(name)
    }

    const NONE: [[&str; 0]; 0] = [];

    #[test]
    fn the_title_menu_starts_a_new_game_and_quits() {
        let directory = directory("new");
        game(&directory, "Title").run(|mut app| {
            steps(&mut app, 3);
            assert_eq!(state(&app), PlayState::Title);
            // No save: no "continue".
            assert_eq!(rows(&app), [["> new", "quit"]]);
            app.resource_mut::<GameState>().set_flag("old", true);
            let loads = app.world().resource::<CurrentRoom>().loads;
            tap(&mut app, Key::Z);
            assert_eq!(state(&app), PlayState::Play);
            assert_eq!(rows(&app), NONE);
            assert!(!flag(&app, "old"));
            // The start room loads again.
            assert_eq!(app.world().resource::<CurrentRoom>().loads, loads + 1);
        });
        game(&directory, "Title").run(|mut app| {
            steps(&mut app, 3);
            tap(&mut app, Key::ArrowDown);
            assert!(!app.world().resource::<QuitRequested>().0);
            tap(&mut app, Key::Z);
            assert!(app.world().resource::<QuitRequested>().0);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A save in the room `a.ron` with the flag `saved`.
    fn save_of(directory: &Path) -> SaveData {
        SaveData {
            room: directory.join("a.ron"),
            position: [1.0, 0.0, 1.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            flags: [("saved".to_owned(), true)].into(),
            variables: Default::default(),
            objects: Default::default(),
            items: Default::default(),
        }
    }

    #[test]
    fn continue_shows_with_a_save_and_loads_it() {
        let directory = directory("continue");
        crate::save::write(&directory.join("save.ron"), &save_of(&directory)).unwrap();
        game(&directory, "Title").run(|mut app| {
            steps(&mut app, 3);
            assert_eq!(state(&app), PlayState::Title);
            assert_eq!(rows(&app), [["> new", "continue", "quit"]]);
            // The load at the start set the flag: the pick sets it again.
            app.resource_mut::<GameState>().set_flag("saved", false);
            tap(&mut app, Key::ArrowDown);
            tap(&mut app, Key::Z);
            assert_eq!(state(&app), PlayState::Play);
            assert!(flag(&app, "saved"));
            assert_eq!(rows(&app), NONE);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_pause_menu_closes_into_play_and_game_over_shows_its_menu() {
        let directory = directory("pause");
        game(&directory, "Play").run(|mut app| {
            // A play that does not pause shows no menu.
            steps(&mut app, 5);
            assert_eq!(state(&app), PlayState::Play);
            assert_eq!(rows(&app), NONE);

            // The pause action opens the pause menu; its "back" row plays again.
            tap(&mut app, Key::P);
            assert_eq!(state(&app), PlayState::Pause);
            assert_eq!(rows(&app), [["> back", "to title", "say"]]);
            tap(&mut app, Key::Z);
            assert_eq!(state(&app), PlayState::Play);
            assert_eq!(rows(&app), NONE);
            // The back action closes it too; the pause action ends the pause as before.
            tap(&mut app, Key::P);
            tap(&mut app, Key::X);
            assert_eq!(state(&app), PlayState::Play);
            tap(&mut app, Key::P);
            tap(&mut app, Key::P);
            assert_eq!(state(&app), PlayState::Play);
            assert_eq!(rows(&app), NONE);

            // The game ends: the game-over menu, and from it the title menu.
            app.resource_mut::<GameState>().set_flag("end", true);
            steps(&mut app, 3);
            assert_eq!(state(&app), PlayState::End);
            assert_eq!(rows(&app), [["> over"]]);
            tap(&mut app, Key::Z);
            assert_eq!(state(&app), PlayState::Title);
            assert_eq!(rows(&app), [["> new", "quit"]]);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_message_of_the_pause_menu_closes_and_the_menu_has_the_input_again() {
        let directory = directory("pause-message");
        game(&directory, "Play").run(|mut app| {
            steps(&mut app, 5);
            tap(&mut app, Key::P);
            tap(&mut app, Key::ArrowDown);
            tap(&mut app, Key::ArrowDown);
            tap(&mut app, Key::Z);
            let message = |app: &App| app.world().resource::<MessageBox>().0.clone();
            assert_eq!(message(&app).as_deref(), Some("x"));
            tap(&mut app, Key::Z);
            assert_eq!(message(&app), None);
            assert_eq!(state(&app), PlayState::Pause);
            tap(&mut app, Key::ArrowUp);
            tap(&mut app, Key::ArrowUp);
            tap(&mut app, Key::Z);
            assert_eq!(state(&app), PlayState::Play);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_load_from_the_game_over_plays() {
        let directory = directory("load-over");
        game(&directory, "Title").run(|mut app| {
            steps(&mut app, 3);
            tap(&mut app, Key::Z);
            crate::save::write(&directory.join("save.ron"), &save_of(&directory)).unwrap();
            app.resource_mut::<GameState>().set_flag("end", true);
            steps(&mut app, 3);
            assert_eq!(state(&app), PlayState::End);
            app.resource_mut::<PlayRequest>().0 = Some(PlayCommand::LoadSave);
            steps(&mut app, 3);
            assert!(flag(&app, "saved"));
            assert_eq!(state(&app), PlayState::Play);
            assert_eq!(rows(&app), NONE);
        });
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// The menus of `game/` read with the commands of the modules of the game, and
    /// each text key of them is in the English table.
    #[test]
    fn the_menus_of_the_game_read() {
        let rooms = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/assets/rooms");
        let directory = directory("game");
        let app = game(&directory, "Play");
        let registry = app.world().resource::<CommandRegistry>();
        let table: TextTable =
            maki_assets::read_data(&rooms.join("data/text/en.text.ron")).unwrap();
        let has = |key: &str| table.texts.iter().any(|entry| entry.key == key);
        for name in ["title", "pause", "game_over"] {
            let menu = read_menu(&rooms.join(format!("data/menus/{name}.menu.ron")), registry);
            let menu = menu.unwrap();
            assert!(has(&menu.title.unwrap().0), "{name}");
            for row in &menu.rows {
                if let MenuRow::Commands { text, .. } | MenuRow::Text(text) = row {
                    assert!(has(&text.0), "{name}: {}", text.0);
                }
            }
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
