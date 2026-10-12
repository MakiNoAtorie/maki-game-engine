//! All engine crates as one shared library (`maki_engine_dylib.dll`).
//!
//! A `dylib` holds each crate that it names, in full. Nothing here uses them: the
//! crate `maki-engine` with its feature `dynamic` links this library, and each
//! program or DLL that names `maki-engine` then takes the engine crates from here.

extern crate maki_assets as _;
extern crate maki_audio as _;
extern crate maki_core as _;
extern crate maki_devtools as _;
extern crate maki_editor as _;
extern crate maki_events as _;
extern crate maki_gameplay as _;
extern crate maki_gpu as _;
extern crate maki_platform as _;
extern crate maki_render as _;
extern crate maki_scene as _;
extern crate maki_ui as _;
