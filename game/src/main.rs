//! The program of the game: the engine, and the script crate of the project
//! (`scripts/`). The rooms and their events are data, in `assets/rooms`; what the
//! engine must know of this game is in `project.toml`.

fn main() -> maki_engine::Result {
    maki_engine::run_game(env!("CARGO_MANIFEST_DIR"), maki_game_scripts::add_modules)
}
