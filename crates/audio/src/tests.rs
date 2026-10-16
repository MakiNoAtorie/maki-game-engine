use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::system::Commands;
use maki_core::{ConfigFile, Interpolated, Startup};
use maki_events::{EventsModule, GameState};
use maki_platform::PlatformModule;
use maki_scene::{RoomRequest, RoomTarget, RoomValidators, SceneModule, read_room};

use super::*;

/// A WAV file with one tone: one channel, 16 bits, 8000 Hz.
fn write_wav(path: &Path, seconds: f32) {
    const RATE: u32 = 8000;
    let samples = (seconds * RATE as f32) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + samples * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    // The size of the format part, PCM, one channel.
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(RATE.to_le_bytes());
    // Bytes in a second, bytes in a sample, bits in a sample.
    bytes.extend((RATE * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((samples * 2).to_le_bytes());
    for index in 0..samples {
        let sample = ((index as f32 * 0.3).sin() * 8000.0) as i16;
        bytes.extend(sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

/// A directory with three sound files and a room. The lamp of the room hums.
fn room_directory(name: &str, events: &str) -> (PathBuf, PathBuf) {
    let directory = std::env::temp_dir().join(format!("maki-audio-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    write_wav(&directory.join("click.wav"), 0.05);
    write_wav(&directory.join("hum.wav"), 0.5);
    write_wav(&directory.join("music.wav"), 1.0);
    let room = format!(
        r#"(width: 8, depth: 8, height: 3, objects: [
            (name: "start", components: {{"spawn": ()}}),
            (name: "lamp", pose: (position: (2, 2, 0)), components: {{"light": (radius: 0.1,
                color: (1, 1, 1), intensity: 5.0, sound: (file: "hum.wav", volume: 0.5, reach: 9.0))}}),
            {}])"#,
        maki_events::short_events(events)
    );
    let path = directory.join("room.ron");
    std::fs::write(&path, room).unwrap();
    (directory, path)
}

/// The game with no window and no sound device, in a room.
fn app(room: &Path) -> App {
    let mut app = App::new(ConfigFile::from_text("[audio]\noutput = false")).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule)
        .add_module(AudioModule);
    let target = RoomTarget {
        path: room.to_owned(),
        spawn: Some("start".into()),
        text: None,
    };
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut request: ResMut<RoomRequest>| {
            commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
            commands.spawn((ActiveCamera, Camera::default()));
            request.0 = Some(target.clone());
        },
    );
    app.check_config().unwrap();
    app
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_secs_f32(1.0 / 60.0));
    }
}

/// The names of the files that play: the sounds of `play`, the music, the sounds at
/// a place.
fn plays(app: &App) -> (Vec<String>, Option<String>, Vec<String>) {
    let audio = app.world().resource::<Audio>();
    let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
    let names = |paths: Vec<&Path>| paths.into_iter().map(name).collect();
    (
        names(audio.effects()),
        audio.music().map(name),
        names(audio.sources()),
    )
}

#[test]
fn the_commands_of_a_room_file_start_and_stop_sounds_with_no_device() {
    let events = r#"(name: "start", pages: [(trigger: Autorun, commands: [
        PlaySound("click.wav", 1.0), PlayMusic("music.wav", 0.5), Wait(0.5),
        SoundOff("lamp"), StopMusic, Wait(0.2),
        SoundOn("lamp"), SetFlag("done", true)])])"#;
    let (directory, room) = room_directory("commands", events);
    let click = |file: &str| vec![file.to_owned()];
    let none = Vec::<String>::new;

    app(&room).run(|mut app| {
        assert!(!app.world().resource::<Audio>().has_device());
        // The room loads, the event starts, its commands run: the click, the
        // music, and the hum of the lamp.
        frames(&mut app, 4);
        let music = Some("music.wav".to_owned());
        assert_eq!(
            plays(&app),
            (click("click.wav"), music.clone(), click("hum.wav"))
        );
        // The click is 0.05 s long: it has ended, and is not kept. The music and
        // the hum are loops.
        frames(&mut app, 12);
        assert_eq!(plays(&app), (none(), music, click("hum.wav")));

        // After the first wait: the sound of the lamp is off, the music stops.
        frames(&mut app, 20);
        assert_eq!(plays(&app), (none(), None, none()));
        // After the second wait: the sound of the lamp is on again.
        frames(&mut app, 14);
        assert!(app.world().resource::<GameState>().flag("done"));
        assert_eq!(plays(&app), (none(), None, click("hum.wav")));

        // A game that stands still is silent, and goes on with its sounds.
        app.resource_mut::<Time>().paused = true;
        frames(&mut app, 2);
        assert_eq!(plays(&app).2, none());
        app.resource_mut::<Time>().paused = false;
        frames(&mut app, 2);
        assert_eq!(plays(&app).2, click("hum.wav"));

        // A file that is asked for two times is one sound. A sound that has ended
        // frees its place: 200 clicks, and the output has 128 places.
        for _ in 0..200 {
            let mut audio = app.resource_mut::<Audio>();
            let sound = audio.load(&directory.join("click.wav"));
            assert_eq!(sound, audio.load(&directory.join("click.wav")));
            assert_eq!(audio.sound(sound).unwrap().seconds(), 0.05);
            audio.play(sound, 1.0);
            frames(&mut app, 1);
        }
        let playing = plays(&app).0.len();
        assert!((1..8).contains(&playing), "{playing}");
        // A file that is not a sound: no sound, and `play` does nothing.
        let mut audio = app.resource_mut::<Audio>();
        let bad = audio.load(&room);
        assert!(audio.sound(bad).is_none());
        audio.play(bad, 1.0);

        // The play signal (a tool stops the game): all sound ends.
        audio.play_music(&directory.join("music.wav"), 1.0);
        assert!(audio.music().is_some());
        app.reset_play();
        assert_eq!(plays(&app), (none(), None, none()));
        // The room is still there: its lamp hums again in the game that starts.
        frames(&mut app, 1);
        assert_eq!(plays(&app), (none(), None, click("hum.wav")));
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_loader_checks_the_names_and_the_files_of_the_audio_commands() {
    let check = |name: &str, events: &str| {
        let (directory, room) = room_directory(name, events);
        let app = app(&room);
        let result = read_room(&room, app.world().resource::<RoomValidators>());
        std::fs::remove_dir_all(&directory).unwrap();
        result.map(|_| ()).map_err(|error| format!("{error:#}"))
    };
    let event = |commands: &str| {
        format!(r#"(name: "a", pages: [(trigger: Interact, commands: [{commands}])])"#)
    };
    assert_eq!(
        check(
            "good",
            &event(r#"SoundOff("lamp"), PlaySound("click.wav", 1.0)"#)
        ),
        Ok(())
    );
    // A file that is not there is a warning: the room loads.
    assert_eq!(check("file", &event(r#"PlayMusic("no.ogg", 1.0)"#)), Ok(()));
    // A name with no sound is an error, with the line of the command.
    let error = check("name", &event(r#"SoundOn("lamb")"#)).unwrap_err();
    let wanted = "5: event `a`: the room has no light `lamb` with a sound";
    assert!(error.ends_with(wanted), "{error}");
}

#[test]
fn the_right_side_of_the_picture_is_the_right_ear() {
    // The camera of the game: above and in front of its target, looking down.
    let camera = Camera {
        position: Vec3::new(0.0, 9.0, 7.0),
        target: Vec3::new(0.0, 0.5, 0.0),
        ..Camera::default()
    };
    assert!((facing(&camera) * Vec3::X).distance(Vec3::X) < 1e-5);
    // Straight down, and from the other side of the room.
    let down = Camera {
        position: Vec3::new(0.0, 9.0, 0.0),
        target: Vec3::ZERO,
        ..Camera::default()
    };
    assert!((facing(&down) * Vec3::X).distance(Vec3::X) < 1e-5);
    let behind = Camera {
        position: Vec3::new(0.0, 9.0, -7.0),
        ..camera
    };
    assert!((facing(&behind) * Vec3::X).distance(Vec3::NEG_X) < 1e-5);
}

#[test]
fn a_sound_file_that_changed_is_read_again_and_what_plays_it_starts_again() {
    let events = r#"(name: "start", pages: [(trigger: Autorun, commands: [
        PlayMusic("music.wav", 0.5)])])"#;
    let (directory, room) = room_directory("changed", events);
    // The length of the sound that the store has for a file.
    let seconds = |app: &mut App, file: &str| {
        let mut audio = app.resource_mut::<Audio>();
        let handle = audio.load(&directory.join(file));
        audio.sound(handle).map(Sound::seconds)
    };
    app(&room).run(|mut app| {
        frames(&mut app, 10);
        let none = Vec::<String>::new;
        let music = Some("music.wav".to_owned());
        let hum = vec!["hum.wav".to_owned()];
        assert_eq!(plays(&app), (none(), music.clone(), hum.clone()));
        assert_eq!(seconds(&mut app, "hum.wav"), Some(0.5));
        // The files that the watch looks at: the hum, and the music.
        let mut files = app.world().resource::<Audio>().files();
        files.sort();
        assert_eq!(
            files,
            [directory.join("hum.wav"), directory.join("music.wav")]
        );

        // Another program writes the hum again, with another length.
        write_wav(&directory.join("hum.wav"), 0.25);
        app.write_message(AssetChanged(directory.join("hum.wav")));
        frames(&mut app, 2);
        // The lamp hums the new file; the music goes on.
        assert_eq!(seconds(&mut app, "hum.wav"), Some(0.25));
        assert_eq!(plays(&app), (none(), music.clone(), hum.clone()));

        // The music file: the music starts again.
        app.write_message(AssetChanged(directory.join("music.wav")));
        frames(&mut app, 2);
        assert_eq!(plays(&app), (none(), music, hum));
    });
    std::fs::remove_dir_all(&directory).unwrap();
}
