//! The watch of the asset files: a file that another program writes again (a
//! texture from a paint program, a model, a sound) is loaded again with no restart.
//!
//! - Each store tells the watch the files that it loaded ([`AssetWatch::set`]), each
//!   with a check: a function that reads the file as the store does.
//! - The watch looks at the times of the files, each file some times in a second.
//!   A file whose time changed waits until the time did not change for
//!   [`SETTLE`] (a program writes a file in parts). Then the check runs.
//! - A file that reads: the message [`AssetChanged`]. Each store forgets the file in
//!   its own module, and the scene module loads the room again. A file that does
//!   not read (half written, broken): the message [`AssetFailed`], and nothing
//!   changes.
//!
//! The rule of the stores stays: one handle, one content. A changed file gets a new
//! handle; the old content lives until the last mesh with it is gone.
//!
//! Off by default (`[assets] watch`): a game that ships, a benchmark, and a test
//! look at no file. The editor turns it on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use bevy_ecs::message::{Message, MessageReader, MessageWriter};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};

use crate::Textures;

/// Each file is looked at one time in this time.
const STEP: Duration = Duration::from_millis(250);
/// A file whose time changed is loaded when the time did not change for this long.
const SETTLE: Duration = Duration::from_millis(300);

/// Reads a file as its store does, to tell if the file is whole.
pub type Check = fn(&Path) -> Result<()>;

/// An asset file changed on the disk, and it reads. Each store forgets the file:
/// the next load of the path reads it again.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct AssetChanged(pub PathBuf);

/// An asset file changed on the disk and does not read. Nothing was changed.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct AssetFailed {
    pub path: PathBuf,
    pub error: String,
}

struct Watched {
    path: PathBuf,
    check: Check,
    /// The time of the file at the last look. `None`: not looked at yet.
    time: Option<Option<SystemTime>>,
    /// The time of the look that found the last change. `None`: at rest.
    changed: Option<Instant>,
}

/// The files that the stores loaded.
#[derive(Resource, Default)]
pub struct AssetWatch {
    /// True: the watch looks at the files.
    pub on: bool,
    /// The files of each store, by the name of the store.
    lists: Vec<(&'static str, Vec<(PathBuf, Check)>)>,
    files: Vec<Watched>,
    /// The next file to look at, and the part of a file that the last frame left.
    next: usize,
    owed: f32,
}

impl AssetWatch {
    /// The files of one store, in place of those that it told before. `store`: its
    /// name. A file that two stores name is watched one time, with the first check.
    pub fn set(&mut self, store: &'static str, files: Vec<(PathBuf, Check)>) {
        match self.lists.iter_mut().find(|(name, _)| *name == store) {
            Some((_, list)) => *list = files,
            None => self.lists.push((store, files)),
        }
        // A file that stays keeps what the watch knows of it.
        let mut old = std::mem::take(&mut self.files);
        for (path, check) in self.lists.iter().flat_map(|(_, list)| list) {
            if self.files.iter().any(|file| file.path == *path) {
                continue;
            }
            let kept = old.iter().position(|file| file.path == *path);
            self.files.push(match kept {
                Some(index) => old.swap_remove(index),
                None => Watched {
                    path: path.clone(),
                    check: *check,
                    time: None,
                    changed: None,
                },
            });
        }
    }

    /// Each watched file.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(|file| file.path.as_path())
    }

    /// Looks at the files that are due after `delta` more time. Returns each file
    /// that changed and is at rest now, with the result of its check. `time_of`:
    /// the time of a file (`None`: no file).
    fn poll(
        &mut self,
        now: Instant,
        delta: Duration,
        time_of: impl Fn(&Path) -> Option<SystemTime>,
    ) -> Vec<(PathBuf, Result<()>)> {
        let mut done = Vec::new();
        if self.files.is_empty() {
            return done;
        }
        // All files in one `STEP`, some in each frame: no frame looks at all.
        self.owed += self.files.len() as f32 * delta.as_secs_f32() / STEP.as_secs_f32();
        let count = (self.owed as usize).min(self.files.len());
        self.owed -= count as f32;
        for _ in 0..count {
            self.next %= self.files.len();
            let file = &mut self.files[self.next];
            self.next += 1;
            let time = time_of(&file.path);
            match file.time {
                // The first look: the file as the store loaded it.
                None => file.time = Some(time),
                Some(old) if old != time => {
                    file.time = Some(time);
                    file.changed = Some(now);
                }
                Some(_) => {
                    let rests = |at: Instant| now.duration_since(at) >= SETTLE;
                    if file.changed.is_some_and(rests) {
                        file.changed = None;
                        done.push((file.path.clone(), (file.check)(&file.path)));
                    }
                }
            }
        }
        done
    }
}

fn time_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

/// Runs in `Update`: looks at the files, and tells of each one that changed.
pub(crate) fn watch_files(
    mut watch: ResMut<AssetWatch>,
    mut changed: MessageWriter<AssetChanged>,
    mut failed: MessageWriter<AssetFailed>,
    mut last: Local<Option<Instant>>,
) {
    use bevy_ecs::change_detection::DetectChangesMut;

    // The clock of the wall: a tool pauses the time of the game.
    let now = Instant::now();
    let delta = last.replace(now).map_or(Duration::ZERO, |last| now - last);
    // No change of the resource in a frame that finds nothing.
    let watch = watch.bypass_change_detection();
    if !watch.on {
        return;
    }
    // A frame that took long looks at each file one time, not many times.
    for (path, result) in watch.poll(now, delta.min(STEP), time_of) {
        match result {
            Ok(()) => {
                tracing::info!(path = %path.display(), "asset file changed: load again");
                changed.write(AssetChanged(path));
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::error!(path = %path.display(), "asset file changed, but it does not read: {error}");
                failed.write(AssetFailed { path, error });
            }
        }
    }
}

/// Runs in `Update`: tells the watch the files of the texture store. Only after a
/// load: the store changes then.
pub(crate) fn watch_textures(textures: Res<Textures>, mut watch: ResMut<AssetWatch>) {
    use bevy_ecs::change_detection::DetectChanges;

    if watch.on && textures.is_changed() {
        watch.set("textures", textures.files());
    }
}

/// Runs in `Update`: the texture store forgets each file that changed. A system
/// that loads a file again in the same frame runs after this one.
pub fn forget_changed(mut changed: MessageReader<AssetChanged>, mut textures: ResMut<Textures>) {
    for AssetChanged(path) in changed.read() {
        textures.forget(path);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    fn whole(_: &Path) -> Result<()> {
        Ok(())
    }

    fn broken(_: &Path) -> Result<()> {
        anyhow::bail!("half of a file")
    }

    /// A watch of some files with times that the test sets, and a clock that the
    /// test moves in steps of 50 ms.
    struct Bench {
        watch: AssetWatch,
        times: RefCell<HashMap<PathBuf, u64>>,
        now: Instant,
    }

    impl Bench {
        fn new(files: &[(&str, Check)]) -> Self {
            let mut watch = AssetWatch::default();
            let list = files
                .iter()
                .map(|(path, check)| (PathBuf::from(path), *check));
            watch.set("test", list.collect());
            let times = files.iter().map(|(path, _)| (PathBuf::from(path), 1));
            Self {
                watch,
                times: RefCell::new(times.collect()),
                now: Instant::now(),
            }
        }

        /// The file is written: it has a new time.
        fn write(&self, path: &str) {
            *self.times.borrow_mut().get_mut(Path::new(path)).unwrap() += 1;
        }

        /// The clock goes on by this many steps of 50 ms. Returns the files that the
        /// watch told of, each with "its check was good".
        fn run(&mut self, steps: u32) -> Vec<(String, bool)> {
            let mut told = Vec::new();
            for _ in 0..steps {
                let delta = Duration::from_millis(50);
                self.now += delta;
                let times = self.times.borrow();
                let time_of = |path: &Path| {
                    let time = times.get(path)?;
                    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(*time))
                };
                for (path, result) in self.watch.poll(self.now, delta, time_of) {
                    told.push((path.to_string_lossy().into_owned(), result.is_ok()));
                }
            }
            told
        }
    }

    #[test]
    fn a_file_that_is_written_in_two_parts_loads_one_time_after_the_second_part() {
        let mut bench = Bench::new(&[("a.png", whole), ("b.glb", whole), ("c.wav", whole)]);
        // The files as they were loaded: nothing to tell.
        assert!(bench.run(20).is_empty());

        // The first part, and 0.2 s later the second part.
        bench.write("a.png");
        assert!(bench.run(4).is_empty());
        bench.write("a.png");
        // Not before the file is at rest for 0.3 s.
        assert!(bench.run(6).is_empty());
        // Then one time, and not again.
        assert_eq!(bench.run(8), [("a.png".to_owned(), true)]);
        assert!(bench.run(20).is_empty());
        // The other files did not change.
    }

    #[test]
    fn a_file_that_does_not_read_is_told_as_failed_and_a_new_list_keeps_the_state() {
        let mut bench = Bench::new(&[("a.png", broken), ("b.glb", whole)]);
        assert!(bench.run(10).is_empty());
        bench.write("a.png");
        bench.write("b.glb");
        assert!(bench.run(2).is_empty());
        // The store tells its files again (a room load): the change is not lost,
        // and a file that the store has no more is not looked at.
        let check: Check = whole;
        bench
            .watch
            .set("test", vec![(PathBuf::from("a.png"), broken)]);
        bench
            .watch
            .set("other", vec![(PathBuf::from("a.png"), check)]);
        assert_eq!(bench.watch.files().count(), 1);
        let told = bench.run(20);
        assert_eq!(told, [("a.png".to_owned(), false)]);
        // A file that is gone, and comes back: a change.
        bench.times.borrow_mut().remove(Path::new("a.png"));
        assert!(bench.run(3).is_empty());
        bench.times.borrow_mut().insert(PathBuf::from("a.png"), 9);
        assert_eq!(bench.run(20), [("a.png".to_owned(), false)]);
    }
}
