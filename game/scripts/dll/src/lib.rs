//! The script crate of the project as a DLL: the editor loads it and calls the entry
//! function of the project.

maki_engine::export_project!(maki_game_scripts::add_modules);
