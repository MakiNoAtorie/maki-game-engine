//! Source files of other programs: a `.blend`, `.obj`, or `.psd` file in the asset
//! folder of the project. The engine does not read them. The editor converts each
//! one to a result file next to it, with the same name: `chair.blend` gives
//! `chair.glb`, `wall.psd` gives `wall.png`. A room names the result file, and the
//! result file is a part of the project: the game, and a machine with no Blender,
//! need nothing more.
//!
//! - A converter is a row of a table ([`Converters`]): the extensions of its source
//!   files, the extension of its result, and the function that writes the result. A
//!   module adds a row with [`add_converter`].
//! - One thread does the work ([`run`]): two times in a second it looks at the
//!   files of the folder, and converts each source that is newer than its result.
//!   The result is written under a temporary name and then gets its name, so no
//!   reader sees half of a file. The new result is a changed asset file: the watch
//!   of `maki_assets` does the rest.
//! - A source whose conversion failed is not tried again until it is saved again.
//!   The old result stays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use maki_core::App;
use maki_ui::{Jobs, NoticeLevel, Notices};

use crate::Editor;

/// The time between two looks at the folder.
const SCAN: Duration = Duration::from_millis(500);
/// A source that was written less than this long ago is not read yet: a program
/// writes a file in parts.
const SETTLE: Duration = Duration::from_millis(300);

/// What a converter gets beside the two files.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    /// The Blender program. `None`: not found.
    pub blender: Option<PathBuf>,
}

/// Writes the result file of a source file. `result`: the temporary name, with the
/// extension of the result.
pub type Convert = fn(source: &Path, result: &Path, tools: &Tools) -> Result<()>;

/// One row of the converter table.
#[derive(Debug, Clone, Copy)]
pub struct Converter {
    /// The extensions of the source files, in small letters.
    pub sources: &'static [&'static str],
    /// The extension of the result file.
    pub result: &'static str,
    pub convert: Convert,
}

/// The converter table.
#[derive(Resource, Debug, Clone)]
pub struct Converters(pub Vec<Converter>);

impl Default for Converters {
    fn default() -> Self {
        Self(vec![
            Converter {
                sources: &["blend"],
                result: "glb",
                convert: blend_to_glb,
            },
            Converter {
                sources: &["obj"],
                result: "glb",
                convert: obj_to_glb,
            },
            Converter {
                sources: &["psd"],
                result: "png",
                convert: psd_to_png,
            },
        ])
    }
}

impl Converters {
    /// The row for a file, by its extension.
    fn of(&self, source: &Path) -> Option<&Converter> {
        let extension = source.extension()?.to_str()?.to_lowercase();
        let row = |row: &&Converter| row.sources.contains(&extension.as_str());
        self.0.iter().find(row)
    }

    /// The source file of a result file, if the folder has one.
    pub fn source_of(&self, result: &Path) -> Option<PathBuf> {
        let extension = result.extension()?.to_str()?.to_lowercase();
        let rows = self.0.iter().filter(|row| row.result == extension);
        let mut sources = rows
            .flat_map(|row| row.sources)
            .map(|source| result.with_extension(source));
        sources.find(|source| source.is_file())
    }

    /// The result files that the sources of a folder will give and that are not
    /// there yet, with one of these extensions.
    pub fn missing_results(&self, directory: &Path, extensions: &[&str]) -> Vec<PathBuf> {
        let sources = files_below(directory).into_iter();
        let results = sources.filter_map(|source| {
            let row = self.of(&source)?;
            let wanted = extensions.contains(&row.result);
            wanted.then(|| source.with_extension(row.result))
        });
        results.filter(|result| !result.exists()).collect()
    }
}

/// Adds a row to the converter table. Call from `Module::build`, before the editor
/// module starts its thread (the editor module is the last one).
pub fn add_converter(app: &mut App, converter: Converter) {
    app.init_resource::<Converters>();
    app.resource_mut::<Converters>().0.push(converter);
}

/// What the thread tells the editor.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// A conversion starts.
    Started(PathBuf),
    /// The result file is written: the source, and the seconds that it took.
    Done(PathBuf, f32),
    /// The conversion failed: the source, and why. The old result stays.
    Failed(PathBuf, String),
}

/// The files of a folder and of the folders in it.
fn files_below(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut directories = vec![directory.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            match path.is_dir() {
                true => directories.push(path),
                false => found.push(path),
            }
        }
    }
    found.sort();
    found
}

fn written(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

/// The temporary name of a result file: `chair.tmp.glb` for `chair.glb`. It keeps
/// the extension: a program takes the file format from it.
fn temporary(result: &Path) -> PathBuf {
    let extension = result.extension().unwrap_or_default().to_string_lossy();
    result.with_extension(format!("tmp.{extension}"))
}

/// One look at the folder: converts each source file that is newer than its result
/// (or has none), one after the other. `failed`: the sources whose conversion
/// failed, each with its file time then: not tried again until that time changes.
pub fn convert_due(
    directory: &Path,
    converters: &Converters,
    tools: &Tools,
    failed: &mut HashMap<PathBuf, SystemTime>,
    report: &mut dyn FnMut(Report),
) {
    for source in files_below(directory) {
        let (Some(row), Some(saved)) = (converters.of(&source), written(&source)) else {
            continue;
        };
        let result = source.with_extension(row.result);
        // A result that a person changed after the last save of the source stays.
        let due = written(&result).is_none_or(|result| result < saved);
        let at_rest = saved.elapsed().is_ok_and(|age| age >= SETTLE);
        if !due || !at_rest || failed.get(&source) == Some(&saved) {
            continue;
        }
        report(Report::Started(source.clone()));
        let start = std::time::Instant::now();
        let temporary = temporary(&result);
        let converted = (row.convert)(&source, &temporary, tools).and_then(|()| {
            // The file has its name only when it is whole.
            std::fs::rename(&temporary, &result)
                .with_context(|| format!("cannot write {}", result.display()))
        });
        match converted {
            Ok(()) => {
                failed.remove(&source);
                // The source was saved again while it was converted: the result is
                // of the save before. With the time of that save, it is older than
                // the source, and the next look converts again: the last save wins.
                if written(&source) != Some(saved)
                    && let Ok(file) = std::fs::File::options().write(true).open(&result)
                {
                    let _ = file.set_modified(saved);
                }
                report(Report::Done(source, start.elapsed().as_secs_f32()));
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temporary);
                failed.insert(source.clone(), saved);
                report(Report::Failed(source, format!("{error:#}")));
            }
        }
    }
}

/// The thread of the converters: looks at the folder until `stop` is set.
fn run(
    directory: PathBuf,
    converters: Converters,
    tools: Tools,
    reports: Sender<Report>,
    stop: Arc<AtomicBool>,
) {
    let mut failed = HashMap::new();
    while !stop.load(Ordering::Relaxed) {
        // No receiver: the app has ended, and `stop` is set.
        let mut send = |report| drop(reports.send(report));
        convert_due(&directory, &converters, &tools, &mut failed, &mut send);
        std::thread::sleep(SCAN);
    }
}

/// The reports of the converter thread. The thread ends with the app: the editor
/// makes a new app, with a new thread, when it loads the code of a project again.
#[derive(Resource)]
pub(crate) struct ConverterReports {
    reports: Mutex<Receiver<Report>>,
    stop: Arc<AtomicBool>,
}

impl Drop for ConverterReports {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The Blender program: that of the setting; else `blender` of the path of the
/// system; else the newest one in the usual folder of Windows.
pub fn find_blender(setting: &str) -> Option<PathBuf> {
    if !setting.is_empty() {
        return Some(PathBuf::from(setting)).filter(|path| path.is_file());
    }
    let name = if cfg!(windows) {
        "blender.exe"
    } else {
        "blender"
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut of_path = std::env::split_paths(&path).map(|directory| directory.join(name));
    if let Some(found) = of_path.find(|path| path.is_file()) {
        return Some(found);
    }
    let programs = std::env::var_os("ProgramFiles")?;
    let folder = Path::new(&programs).join("Blender Foundation");
    let versions = std::fs::read_dir(folder).ok()?.flatten();
    let mut found: Vec<PathBuf> = versions.map(|entry| entry.path().join(name)).collect();
    found.retain(|path| path.is_file());
    found.sort();
    found.pop()
}

/// Starts the converter thread for the asset folder of the editor. `blender`: the
/// `[editor] blender` setting.
pub(crate) fn start(app: &mut App, directory: &Path, blender: &str) {
    app.init_resource::<Converters>()
        .init_resource::<Jobs>()
        .init_resource::<Notices>();
    let converters = app.world().resource::<Converters>().clone();
    let tools = Tools {
        blender: find_blender(blender),
    };
    match &tools.blender {
        Some(blender) => {
            tracing::info!(blender = %blender.display(), "converter of .blend and .obj files")
        }
        None => tracing::warn!("Blender was not found: .blend and .obj files are not converted"),
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let (directory, stops) = (directory.to_owned(), stop.clone());
    std::thread::spawn(move || run(directory, converters, tools, sender, stops));
    app.insert_resource(ConverterReports {
        reports: Mutex::new(receiver),
        stop,
    });
}

/// Runs in `Update`: what the converter thread did goes to the status row, the jobs,
/// and the log.
pub(crate) fn tell_reports(
    reports: Res<ConverterReports>,
    mut editor: ResMut<Editor>,
    mut jobs: ResMut<Jobs>,
    mut notices: ResMut<Notices>,
) {
    let reports: Vec<Report> = {
        let receiver = reports.reports.lock().expect("no holder panics");
        receiver.try_iter().collect()
    };
    let name = |path: &Path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let job = |path: &Path| format!("convert {}", name(path));
    for report in reports {
        match report {
            Report::Started(source) => {
                jobs.0.push(job(&source));
                tracing::info!(source = %source.display(), "conversion starts");
            }
            Report::Done(source, seconds) => {
                jobs.0.retain(|other| *other != job(&source));
                tracing::info!(source = %source.display(), seconds, "converted");
                editor.status = format!("{}: converted", name(&source));
            }
            Report::Failed(source, error) => {
                jobs.0.retain(|other| *other != job(&source));
                // All of it in the log; the first line in the status row.
                tracing::error!("{} was not converted: {error}", source.display());
                let first = error.lines().next().unwrap_or_default();
                let text = format!("{} was not converted: {first}", name(&source));
                // The status row has one line, and the next file writes over it.
                notices.push(NoticeLevel::Warning, format!("{text}. The log has more."));
                editor.status = text;
            }
        }
    }
}

/// What Blender runs for a `.blend` file that it has open: the export as one glTF
/// file. The result file is the argument after `--`.
// Modifiers are applied: the room shows what Blender shows. +Y is up, as in the
// engine. No camera and no light: a room has its own. The images go into the file.
const EXPORT: &str = "import bpy, sys; \
    bpy.ops.export_scene.gltf(filepath=sys.argv[sys.argv.index('--') + 1], export_format='GLB', \
    export_apply=True, export_yup=True, export_cameras=False, export_lights=False)";

/// The same for an `.obj` file (with its `.mtl` file): an empty scene, the import,
/// the export. The source file is the second argument after `--`.
const IMPORT_OBJ: &str = "import bpy, sys; \
    bpy.ops.wm.read_factory_settings(use_empty=True); \
    bpy.ops.wm.obj_import(filepath=sys.argv[sys.argv.index('--') + 2]); ";

/// Runs Blender with no window. `open`: the `.blend` file to open. `script`: what it
/// runs. `arguments`: for the script, after `--`.
fn blender(tools: &Tools, open: Option<&Path>, script: &str, arguments: &[&Path]) -> Result<()> {
    let Some(blender) = &tools.blender else {
        bail!("Blender was not found. Set `[editor] blender` to its program file");
    };
    let mut command = Command::new(blender);
    // No window; no add-on and no setting of the user; a Python error is an error.
    command.args([
        "--background",
        "--factory-startup",
        "--python-exit-code",
        "1",
    ]);
    command
        .args(open)
        .args(["--python-expr", script, "--"])
        .args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of the child (CREATE_NO_WINDOW).
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .with_context(|| format!("cannot run {}", blender.display()))?;
    if !output.status.success() {
        let (out, error) = (
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        bail!(
            "Blender ended with an error ({})\n{out}\n{error}",
            output.status
        );
    }
    Ok(())
}

fn blend_to_glb(source: &Path, result: &Path, tools: &Tools) -> Result<()> {
    blender(tools, Some(source), EXPORT, &[result])?;
    // Blender ends with no error also when the export wrote nothing.
    result
        .is_file()
        .then_some(())
        .context("Blender wrote no file")
}

fn obj_to_glb(source: &Path, result: &Path, tools: &Tools) -> Result<()> {
    let script = format!(
        "{IMPORT_OBJ}{}",
        EXPORT.trim_start_matches("import bpy, sys; ")
    );
    blender(tools, None, &script, &[result, source])?;
    result
        .is_file()
        .then_some(())
        .context("Blender wrote no file")
}

/// The flat picture of a Photoshop file, as PNG.
fn psd_to_png(source: &Path, result: &Path, _: &Tools) -> Result<()> {
    let (width, height, pixels) = read_psd(&std::fs::read(source)?)?;
    image::save_buffer(
        result,
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

/// The flat picture of a Photoshop file (`.psd`): what the file shows with all its
/// layers, as Photoshop wrote it at the end of the file. Returns the width, the
/// height, and four bytes for each pixel (red, green, blue, alpha).
///
/// Read: RGB and gray, 8 and 16 bits (the low half of 16 bits is lost), not packed
/// and packed (PackBits). Not read: the layers. A file that Photoshop saved with
/// "maximize compatibility" off has no flat picture of its layers: Photoshop then
/// writes a picture that says so, and this is what comes out.
// No crate: the flat picture is the simple part of the format (some 60 lines), and
// the layers, which are the hard part, are not needed.
fn read_psd(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    use anyhow::ensure;

    let mut at = 0;
    let mut take = |count: usize| {
        let part = bytes.get(at..at + count).context("the file ends early");
        at += count;
        part
    };
    // Each number of the file has its high byte first.
    let number = |part: &[u8]| {
        part.iter()
            .fold(0usize, |number, byte| number << 8 | usize::from(*byte))
    };
    ensure!(take(4)? == b"8BPS", "not a Photoshop file");
    ensure!(number(take(2)?) == 1, "a large document (.psb) is not read");
    take(6)?;
    let channels = number(take(2)?);
    let (height, width) = (number(take(4)?), number(take(4)?));
    let (depth, mode) = (number(take(2)?), number(take(2)?));
    ensure!(
        depth == 8 || depth == 16,
        "{depth} bits for each channel are not read: use 8 or 16"
    );
    // The channels of the color, by the color mode: 1 is gray, 3 is RGB.
    let color = match mode {
        1 => 1,
        3 => 3,
        _ => anyhow::bail!("the color mode is not RGB or gray (mode {mode})"),
    };
    ensure!(
        channels >= color && width > 0 && height > 0,
        "the file has no picture"
    );
    // Three parts that are not the flat picture: the color table, the resources,
    // the layers. Each has its length first.
    for _ in 0..3 {
        let length = number(take(4)?);
        take(length)?;
    }
    let packing = number(take(2)?);
    // The picture: each channel in full, one after the other, row by row.
    let row = width * depth / 8;
    let mut planes = vec![vec![0u8; row * height]; channels];
    match packing {
        0 => {
            for plane in &mut planes {
                plane.copy_from_slice(take(row * height)?);
            }
        }
        1 => {
            // First the packed length of each row of each channel.
            let lengths: Vec<usize> = (0..channels * height)
                .map(|_| take(2).map(number))
                .collect::<Result<_>>()?;
            for (plane, lengths) in planes.iter_mut().zip(lengths.chunks(height)) {
                for (line, length) in plane.chunks_mut(row).zip(lengths) {
                    unpack(take(*length)?, line)?;
                }
            }
        }
        _ => anyhow::bail!("the picture is packed in a way that is not read ({packing})"),
    }
    // The first byte of a sample: all of 8 bits, the high half of 16 bits.
    let sample = |plane: usize, pixel: usize| planes[plane][pixel * depth / 8];
    let mut pixels = Vec::with_capacity(width * height * 4);
    for pixel in 0..width * height {
        let gray = sample(0, pixel);
        let (green, blue) = match color {
            3 => (sample(1, pixel), sample(2, pixel)),
            _ => (gray, gray),
        };
        // One channel more than the color: how much of the picture shows.
        let alpha = if channels > color {
            sample(color, pixel)
        } else {
            255
        };
        pixels.extend([gray, green, blue, alpha]);
    }
    Ok((width as u32, height as u32, pixels))
}

/// Unpacks one row (PackBits): a count byte, then that many bytes as they are, or
/// one byte that is written many times.
fn unpack(mut packed: &[u8], row: &mut [u8]) -> Result<()> {
    let mut at = 0;
    while at < row.len() {
        let (&count, rest) = packed.split_first().context("a packed row ends early")?;
        let (run, used) = match count {
            0..=127 => (
                rest.get(..usize::from(count) + 1).map(<[u8]>::to_vec),
                usize::from(count) + 1,
            ),
            // 128 is a byte with no meaning.
            128 => (Some(Vec::new()), 0),
            _ => (
                rest.first()
                    .map(|byte| vec![*byte; 257 - usize::from(count)]),
                1,
            ),
        };
        let run = run.context("a packed row ends early")?;
        let end = (at + run.len()).min(row.len());
        row[at..end].copy_from_slice(&run[..end - at]);
        (at, packed) = (end, &rest[used..]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A converter that code gives: the result has the text of the source, in
    /// capital letters. A source with the text "bad" does not convert.
    fn upper(source: &Path, result: &Path, _: &Tools) -> Result<()> {
        let text = std::fs::read_to_string(source)?;
        // The result is written under the temporary name, beside the source.
        assert_eq!(result.extension().unwrap(), "out");
        assert!(result.to_string_lossy().ends_with(".tmp.out"), "{result:?}");
        assert_eq!(result.parent(), source.parent());
        if text == "first" {
            // The user saves again while the converter runs.
            std::fs::write(source, "second")?;
            set_time(source, SystemTime::now() - Duration::from_secs(40));
        }
        if text == "bad" {
            // Half of a result: it must not stay.
            std::fs::write(result, "HALF")?;
            bail!("the source is bad\nmore lines of the program");
        }
        std::fs::write(result, text.to_uppercase())?;
        Ok(())
    }

    fn set_time(path: &Path, time: SystemTime) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn a_source_that_is_newer_than_its_result_is_converted_by_way_of_a_temporary_name() {
        let directory = std::env::temp_dir().join(format!("maki-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("models")).unwrap();
        let converters = Converters(vec![Converter {
            sources: &["src", "alt"],
            result: "out",
            convert: upper,
        }]);
        let (source, result) = (
            directory.join("models/a.src"),
            directory.join("models/a.out"),
        );
        let mut failed = HashMap::new();
        let look = |failed: &mut HashMap<PathBuf, SystemTime>| {
            let mut reports = Vec::new();
            convert_due(
                &directory,
                &converters,
                &Tools::default(),
                failed,
                &mut |report| {
                    reports.push(match report {
                        Report::Started(_) => "started".to_owned(),
                        Report::Done(..) => "done".to_owned(),
                        Report::Failed(_, error) => format!("failed: {error}"),
                    });
                },
            );
            reports
        };
        let now = SystemTime::now();
        let ago = |seconds: u64| now - Duration::from_secs(seconds);

        // A file that is no source, and a source that was written a moment ago:
        // nothing.
        std::fs::write(directory.join("models/a.txt"), "text").unwrap();
        std::fs::write(&source, "one").unwrap();
        assert!(look(&mut failed).is_empty());
        // At rest, with no result: converted. The steps in their order, and no
        // temporary file is left.
        set_time(&source, ago(100));
        assert_eq!(look(&mut failed), ["started", "done"]);
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "ONE");
        assert_eq!(files_below(&directory).len(), 3);
        // The result is newer: nothing more.
        assert!(look(&mut failed).is_empty());
        assert_eq!(converters.source_of(&result), Some(source.clone()));

        // A result that a person changed after the source: it stays.
        std::fs::write(&result, "by hand").unwrap();
        assert!(look(&mut failed).is_empty());
        // The source is saved again, later than its result: converted again.
        set_time(&result, ago(90));
        std::fs::write(&source, "two").unwrap();
        set_time(&source, ago(80));
        assert_eq!(look(&mut failed), ["started", "done"]);
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "TWO");

        // A converter that fails: the first line of its error, the old result
        // stays, no half file is left, and the source is not tried again.
        set_time(&result, ago(70));
        std::fs::write(&source, "bad").unwrap();
        set_time(&source, ago(60));
        let reports = look(&mut failed);
        assert_eq!(reports[0], "started");
        assert!(
            reports[1].starts_with("failed: the source is bad"),
            "{reports:?}"
        );
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "TWO");
        assert_eq!(files_below(&directory).len(), 3);
        assert!(look(&mut failed).is_empty());
        // Saved again, and good: converted.
        std::fs::write(&source, "three").unwrap();
        set_time(&source, ago(50));
        assert_eq!(look(&mut failed), ["started", "done"]);
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "THREE");

        // A save while the converter runs: the result of the save before, and the
        // next look converts again. The last save wins.
        set_time(&result, ago(49));
        std::fs::write(&source, "first").unwrap();
        set_time(&source, ago(45));
        assert_eq!(look(&mut failed), ["started", "done"]);
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "FIRST");
        assert_eq!(look(&mut failed), ["started", "done"]);
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "SECOND");
        assert!(look(&mut failed).is_empty());

        // A source with no result yet shows in a file list by the name of its result.
        std::fs::write(directory.join("b.alt"), "x").unwrap();
        assert_eq!(
            converters.missing_results(&directory, &["out"]),
            [directory.join("b.out")]
        );
        assert!(converters.missing_results(&directory, &["glb"]).is_empty());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A Photoshop file with a flat picture of one row. `planes`: the bytes of each
    /// channel. `packed`: the rows as PackBits (a count of bytes as they are).
    fn psd(mode: u8, depth: u8, width: u8, planes: &[&[u8]], packed: bool) -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        // Version 1, six bytes of zero, the channels, height 1, the width, the bits
        // of a channel, the color mode.
        bytes.extend([0, 1, 0, 0, 0, 0, 0, 0]);
        bytes.extend([
            0,
            planes.len() as u8,
            0,
            0,
            0,
            1,
            0,
            0,
            0,
            width,
            0,
            depth,
            0,
            mode,
        ]);
        // No color table and no layers: lengths of 0. Resources of 2 bytes.
        bytes.extend([0, 0, 0, 0, 0, 0, 0, 2, 7, 7, 0, 0, 0, 0]);
        bytes.extend([0, u8::from(packed)]);
        if packed {
            for plane in planes {
                bytes.extend([0, plane.len() as u8 + 1]);
            }
        }
        for plane in planes {
            if packed {
                bytes.push(plane.len() as u8 - 1);
            }
            bytes.extend(*plane);
        }
        bytes
    }

    #[test]
    fn the_flat_picture_of_a_photoshop_file_is_written_as_png() {
        // RGB, 8 bits, two pixels: not packed, and packed.
        let rgb: [&[u8]; 3] = [&[200, 1], &[100, 2], &[50, 3]];
        let pixels = [200, 100, 50, 255, 1, 2, 3, 255];
        for packed in [false, true] {
            let read = read_psd(&psd(3, 8, 2, &rgb, packed)).unwrap();
            assert_eq!(read, (2, 1, pixels.to_vec()), "packed: {packed}");
        }
        // One channel more: how much shows. Gray. 16 bits: the high half.
        let with_alpha: [&[u8]; 4] = [&[200], &[100], &[50], &[128]];
        assert_eq!(
            read_psd(&psd(3, 8, 1, &with_alpha, false)).unwrap().2,
            [200, 100, 50, 128]
        );
        assert_eq!(
            read_psd(&psd(1, 8, 2, &[&[9, 90]], true)).unwrap().2,
            [9, 9, 9, 255, 90, 90, 90, 255]
        );
        let deep: [&[u8]; 3] = [&[200, 7], &[100, 7], &[50, 7]];
        assert_eq!(
            read_psd(&psd(3, 16, 1, &deep, false)).unwrap().2,
            [200, 100, 50, 255]
        );
        // A run of one byte, written 3 times (PackBits: 254 is "3 times").
        let mut row = [0; 4];
        unpack(&[254, 5, 0, 9], &mut row).unwrap();
        assert_eq!(row, [5, 5, 5, 9]);

        // What is not read says why: CMYK, a file that ends early, another file.
        let error = |bytes: &[u8]| format!("{:#}", read_psd(bytes).unwrap_err());
        assert!(error(&psd(4, 8, 2, &rgb, false)).contains("color mode"));
        let whole = psd(3, 8, 2, &rgb, true);
        assert!(error(&whole[..whole.len() - 2]).contains("ends early"));
        assert!(error(b"not a picture").contains("not a Photoshop file"));

        // The converter writes the picture as a PNG file.
        let directory = std::env::temp_dir().join(format!("maki-psd-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let (source, result) = (directory.join("wall.psd"), directory.join("wall.tmp.png"));
        std::fs::write(&source, psd(3, 8, 2, &rgb, true)).unwrap();
        psd_to_png(&source, &result, &Tools::default()).unwrap();
        let picture = image::open(&result).unwrap().into_rgba8();
        assert_eq!(
            (picture.dimensions(), picture.into_raw()),
            ((2, 1), pixels.to_vec())
        );
        // A file that is not a Photoshop file: an error, no result.
        std::fs::write(&source, "not a picture").unwrap();
        std::fs::remove_file(&result).unwrap();
        assert!(psd_to_png(&source, &result, &Tools::default()).is_err());
        assert!(!result.exists());
        // With no Blender, a `.blend` file gives the error that names the setting.
        let error = blend_to_glb(&source, &result, &Tools::default()).unwrap_err();
        assert!(format!("{error:#}").contains("[editor] blender"));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
