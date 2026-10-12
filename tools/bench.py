# tool: run the game benchmark over a scenario matrix, append summary rows to tools/bench_history.csv, compare runs | run: python tools/bench.py run --label baseline ; python tools/bench.py compare baseline after
"""Benchmark driver for `maki-game --bench-frames`.

run      Builds the release binary, runs each scenario, and appends one row for each
         measured column to tools/bench_history.csv (median, p95, p99, min, max over
         the frames after the warmup). Appends the environment to tools/bench_env.csv.
compare  Prints a CSV with the medians of two stored runs and the change in percent.
         A run is named by a part of its run_id (the label); the newest match is used.
reference Makes the reference picture of a scenario: a static light, 65536 samples for
         each pixel, and no filter, in target/bench/reference/<scenario>.dump. About one
         minute. `--set table.key=value` changes config keys, `--name` the file name.
quality  Runs a scenario with a static light (or `--moving`) and prints two numbers for
         the last picture against the reference: the error (relative mean squared
         error, `relmse`, without the 0.01 % of the pixels with the largest error) and
         the sharpness (gradient energy of the picture over that
         of the reference at the edges of the reference: 1.0 is as sharp, below is
         blurred). `noise` is the same ratio at all other pixels: above 1.0 is noise.
         `relmse` alone prefers a soft picture: read the numbers together. Also writes
         <label>.png: picture, reference, difference. `--crop X,Y,W,H` measures a part.
         For `walk_720p` use `--frames 180`: the picture of the last frame with motion.
         `--set table.key=value` changes config keys.
         Compare two settings at equal GPU time, not at equal frame count.
same     Compares the pictures that two runs of one scenario dumped (exact labels):
         `ok`, or the number of pixels that differ, the largest difference, and the means.
selftest Checks the statistics against known answers.

A scenario is a config file plus an optional generated scene. The window is the same
in all scenarios, so `tonemap`, `ui_overlay`, and `present` do the same work in each.
"""
import argparse
import array
import csv
import json
import platform
import statistics
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True  # no tools/__pycache__ in the repository
from gen_scene import triangle_count, write_scene  # noqa: E402
from gen_textured import write_textured  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target" / "bench"  # ignored by git
REFERENCE = WORK / "reference"
HISTORY = ROOT / "tools" / "bench_history.csv"
ENVIRONMENT = ROOT / "tools" / "bench_env.csv"
HISTORY_HEADER = ["run_id", "commit", "name", "params", "n", "unit", "median", "p95", "p99", "min", "max"]
ENVIRONMENT_HEADER = ["run_id", "commit", "gpu", "driver", "os", "build", "frames", "warmup"]
WINDOW = (1280, 720)

# name -> (internal resolution, arguments of `write_scene`, more config keys,
# optional: more command line arguments).
# Scene `None`: the test scene of the game (cube, sphere, glass ball, two mirrors).
# To measure a new config key, add a scenario: "test_1080p_spp4": ((1920, 1080), None, {"render.samples_per_frame": 4}).
SCENARIOS = {
    "test_360p": ((640, 360), None, {}),
    "test_720p": ((1280, 720), None, {}),
    "test_1080p": ((1920, 1080), None, {}),
    # More GPU time for each frame: a small change shows above the timer noise.
    "test_2160p": ((3840, 2160), None, {}),
    # 256 BLAS and 256 TLAS instances, 565248 triangles.
    "stress_720p": ((1280, 720), dict(objects=256, segments=48, seed=1), {}),
    # The same triangles in one BLAS and one instance.
    "stress_merged_720p": ((1280, 720), dict(objects=256, segments=48, seed=1, merged=True), {}),
    # The size of a game level: 1000 BLAS, 1000 instances, 3968000 triangles.
    "heavy_1080p": ((1920, 1080), dict(objects=1000, segments=64, seed=1), {}),
    "heavy_merged_1080p": ((1920, 1080), dict(objects=1000, segments=64, seed=1, merged=True), {}),
    # One sphere in four is a mirror, one in four is glass: long paths.
    "heavy_mixed_1080p": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25), {}),
    # Four samples for each pixel and frame: the cost of a sample without the cost of a frame.
    "test_1080p_spp4": ((1920, 1080), None, {"render.samples_per_frame": 4}),
    "heavy_mixed_1080p_spp4": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25), {"render.samples_per_frame": 4}),
    # A path limit of 6 rays in place of 12.
    "heavy_mixed_1080p_rays6": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25), {"render.max_path_rays": 6}),
    # No denoiser and no light history: only the path tracer, as before these two existed.
    "test_1080p_raw": ((1920, 1080), None, {"render.denoise_passes": 0, "render.light_history_samples": 0}),
    "heavy_mixed_1080p_raw": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25), {"render.denoise_passes": 0, "render.light_history_samples": 0, "render.max_path_rays": 12}),
    # The light does not move: the accumulate pass reads the old mean in each frame.
    "static_1080p": ((1920, 1080), None, {}, ["--bench-static"]),
    # Complete after 8 frames: the cost of a frame that only shows the picture.
    "idle_1080p": ((1920, 1080), None, {"render.max_samples": 8}, ["--bench-static"]),
    # Eight point lights in the file, and the light of the config: nine lights.
    "heavy_lights8_1080p": ((1920, 1080), dict(objects=1000, segments=64, seed=1, lights=8), {}),
    # The player walks and the door opens (the walk script of game/src/scripts.rs). The
    # camera has no follow delay: at the end of the script nothing moves.
    "walk_720p": ((1280, 720), None, {"camera.follow_time": 0.0}, ["--bench-script", "walk"]),
    # The full loop of the game: room A, room B, the switch, and back (the trip script).
    # Two room loads, with events, messages, and fades. Use `--frames 900 --warmup 0`.
    "trip_720p": ((1280, 720), None, {}, ["--bench-script", "trip"]),
    # Room C: rough metal and plastic, a surface with a light of its own, a cone light.
    "materials_720p": ((1280, 720), None, {"rooms.start": "room_c.ron"}),
    # Two diffuse bounces for each path.
    "test_1080p_bounces2": ((1920, 1080), None, {"render.max_bounces": 2}),
    "heavy_mixed_lights8_1080p_bounces2": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25, lights=8), {"render.max_bounces": 2}),
    "heavy_mixed_lights8_1080p": ((1920, 1080), dict(objects=1000, segments=64, seed=1, mirror_share=0.25, glass_share=0.25, lights=8), {}),
    # The textured test room (tools/gen_textured.py, `TEXTURED_ROOM` below): floor and
    # wall textures, pixel art, textures with holes, an emissive texture, and a mirror
    # and a glass ball that show them.
    "textured_720p": ((1280, 720), "textured", {}),
    "textured_1080p": ((1920, 1080), "textured", {}),
    # The same with the player in motion (the walk script; no door in this room).
    "textured_walk_720p": ((1280, 720), "textured", {"camera.follow_time": 0.0}, ["--bench-script", "walk"]),
}


# The room file of a scenario with a generated scene: room A of the game (its size, its
# door, its light), with the scene file as the one prop. `MODEL` is the path, quoted.
MODEL_ROOM = """(
    width: 8.0, depth: 8.0, height: 3.0,
    doorways: [(wall: NegZ, offset: 3.0, width: 1.0, height: 2.1, depth: 1.2)],
    doors: [(doorway: 0)],
    props: [(mesh: File(MODEL), collider: false)],
    lights: [(position: (0.0, 2.0, 2.0), radius: 0.15, color: (1.0, 0.9, 0.8), intensity: 12.0)],
    spawns: {"start": (-0.5, 0.0, 2.6)},
)
"""


# The room file of the `textured_*` scenarios. The files that it names are next to it.
TEXTURED_ROOM = """(
    width: 8.0, depth: 8.0, height: 3.0,
    floor: (path: "checker.png"),
    walls: (path: "bricks.png", scale: 2.0, filter: Nearest),
    props: [
        (mesh: File("props.glb"), collider: false),
        (mesh: Cube(0.7), pose: (position: (2.6, 0.35, 1.6), rotation: (0.0, 25.0, 0.0)),
            look: (color: (1.0, 1.0, 1.0), texture: (path: "crate.png", scale: 1.4286, filter: Nearest))),
        (mesh: Sphere(radius: 0.5, segments: 48, rings: 24), pose: (position: (-3.0, 0.5, 0.6)), look: (color: (0.9, 0.9, 0.9), material: Mirror)),
        (mesh: Sphere(radius: 0.45, segments: 48, rings: 24), pose: (position: (0.9, 0.45, 2.2)), look: (color: (1.0, 1.0, 1.0), material: Glass(ior: 1.5))),
    ],
    lights: [(position: (0.0, 2.3, 1.0), radius: 0.15, color: (1.0, 0.92, 0.82), intensity: 12.0)],
    spawns: {"start": (-0.5, 0.0, 2.6)},
)
"""


def summarize(values):
    """median, p95, p99, min, max. Percentiles with linear interpolation."""
    cuts = statistics.quantiles(values, n=100, method="inclusive")
    return statistics.median(values), cuts[94], cuts[98], min(values), max(values)


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True).stdout.strip()


def commit_id():
    dirty = subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=ROOT).returncode != 0
    return git("rev-parse", "--short", "HEAD") + ("-dirty" if dirty else "")


def gpu_info():
    try:
        query = ["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"]
        name, driver = subprocess.run(query, capture_output=True, text=True, check=True).stdout.strip().split(", ")
        return name, driver
    except (OSError, subprocess.CalledProcessError, ValueError):
        return "", ""


def append(path, header, rows):
    new = not path.exists()
    with path.open("a", newline="") as file:
        writer = csv.writer(file)
        if new:
            writer.writerow(header)
        writer.writerows(rows)


def config_text(config):
    """The text of a config file. A key is `table.key`, as the game has one table for
    each module: {"render.max_bounces": 2} gives `[render]` with `max_bounces = 2`."""
    tables = {}
    for key, value in config.items():
        table, dot, name = key.partition(".")
        if not dot:
            raise SystemExit(f"config key `{key}`: write `table.key`, for example `render.max_bounces`")
        # A JSON number, string, or list of numbers is the same text in TOML.
        tables.setdefault(table, []).append(f"{name} = {json.dumps(value)}")
    return "".join(f"[{table}]\n" + "\n".join(lines) + "\n\n" for table, lines in tables.items())


def run_scenario(name, frames, warmup, label, overrides=None, arguments=(), dump=None):
    """Runs the game one time. Returns (params, {column: [values after the warmup]}).
    The picture of the last frame goes to `dump` (default: target/bench/<name>/<label>.dump).
    `overrides` are more config keys, `arguments` more command line arguments."""
    (width, height), scene, extra, *more = SCENARIOS[name]
    extra = {**extra, **(overrides or {})}
    # A scenario can name another room of the game with the key `rooms.start`.
    room_name = extra.pop("rooms.start", "room_a.ron")
    # A key of `extra` takes the place of the same key below: TOML permits a key one time.
    # No sound device: a benchmark is silent, and its work is the same on each machine.
    # No loader thread: a model file is there in the frame that asks for it, in each run.
    fixed = {"window.width": WINDOW[0], "window.height": WINDOW[1], "render.width": width, "render.height": height, "audio.output": False, "assets.background": False}
    if "render.samples_per_frame" not in extra:
        # Not the automatic count: each run must do the same work.
        fixed["render.samples_per_frame"] = 1
    arguments = [*(more[0] if more else []), *arguments]
    dump = dump or WORK / name / f"{label}.dump"
    work = WORK / name
    work.mkdir(parents=True, exist_ok=True)
    config = {**fixed, **extra}
    params = f"render={width}x{height};window={WINDOW[0]}x{WINDOW[1]};frames={frames};warmup={warmup}"
    params += "".join(f";{key}={value}" for key, value in extra.items())
    params += "".join(f";{argument}" for argument in arguments)
    if scene == "textured":
        write_textured(work)
        room_path = work / "room.ron"
        room_path.write_text(TEXTURED_ROOM)
        params += ";textured"
    elif scene:
        scene_text = ";".join(f"{key}={value}" for key, value in scene.items())
        # The generator is seeded: a file with these arguments in its name has this content.
        scene_path = work / f"scene;{scene_text}.glb".replace(";", "_").replace("=", "")
        if not scene_path.exists():
            write_scene(scene_path, **scene)
        triangles = triangle_count(scene["objects"], scene["segments"])
        # The room of the game, with the model in place of its props.
        room_path = work / "room.ron"
        room_path.write_text(MODEL_ROOM.replace("MODEL", json.dumps(scene_path.resolve().as_posix())))
        params += f";{scene_text};triangles={triangles}"
    else:
        room_path = ROOT / "game" / "assets" / "rooms" / room_name
    config["rooms.start"] = room_path.resolve().as_posix()
    (work / "config.toml").write_text(config_text(config))
    output = work / "bench.csv"
    output.unlink(missing_ok=True)

    binary = ROOT / "target" / "release" / ("maki-game.exe" if sys.platform == "win32" else "maki-game")
    # The game writes bench.csv into its working directory.
    result = subprocess.run(
        [binary, "config.toml", "--bench-frames", str(frames), "--bench-dump", dump, *arguments], cwd=work, capture_output=True, text=True
    )
    (work / "log.txt").write_text(result.stdout + result.stderr)
    if result.returncode != 0 or not output.exists():
        raise SystemExit(f"FAIL {name}: exit code {result.returncode}, log in {work / 'log.txt'}")

    columns = {}
    with output.open(newline="") as file:
        for row in csv.DictReader(file):
            # A frame with an empty cell is dropped as a whole, so all columns keep the same frames.
            if int(row["frame"]) <= warmup or "" in row.values():
                continue
            for key, value in row.items():
                if key != "frame":
                    columns.setdefault(key.removesuffix("_ms"), []).append(float(value))
            gpu = sum(float(value) for key, value in row.items() if key.startswith("gpu_"))
            columns.setdefault("gpu_total", []).append(gpu)
    if not columns or len(columns["cpu"]) < min(10, frames - warmup):
        raise SystemExit(f"FAIL {name}: too few complete frames after the warmup, see {output}")
    return params, columns


def command_run(args):
    names = args.only or list(SCENARIOS)
    unknown = [name for name in names if name not in SCENARIOS]
    if unknown:
        raise SystemExit(f"unknown scenario {unknown}; known: {list(SCENARIOS)}")
    build()

    run_id = time.strftime("%Y%m%dT%H%M%S") + "-" + args.label
    commit = commit_id()
    rows = []
    for name in names:
        params, columns = run_scenario(name, args.frames, args.warmup, args.label)
        for column, values in columns.items():
            stats = (f"{value:.3f}" for value in summarize(values))
            rows.append([run_id, commit, f"{name}/{column}", params, len(values), "ms", *stats])
    append(HISTORY, HISTORY_HEADER, rows)
    gpu, driver = gpu_info()
    system = f"{platform.system()} {platform.release()} {platform.version()}"
    append(ENVIRONMENT, ENVIRONMENT_HEADER, [[run_id, commit, gpu, driver, system, "release", args.frames, args.warmup]])
    print(f"{run_id}: {len(rows)} rows appended to {HISTORY.relative_to(ROOT).as_posix()}")


def stored_run(part):
    """{name: median} of the newest run whose run_id contains `part`."""
    if not HISTORY.exists():
        raise SystemExit(f"no {HISTORY}")
    with HISTORY.open(newline="") as file:
        rows = [row for row in csv.DictReader(file) if part in row["run_id"]]
    if not rows:
        raise SystemExit(f"no run with `{part}` in its run_id")
    newest = max(row["run_id"] for row in rows)
    return newest, {row["name"]: float(row["median"]) for row in rows if row["run_id"] == newest}


def command_compare(args):
    (id_a, a), (id_b, b) = stored_run(args.a), stored_run(args.b)
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(["name", f"median_ms {id_a}", f"median_ms {id_b}", "change_percent"])
    for name in a:
        if name in b:
            change = (b[name] - a[name]) / a[name] * 100 if a[name] else float("nan")
            writer.writerow([name, a[name], b[name], f"{change:+.1f}"])


def build():
    result = subprocess.run(
        ["cargo", "build", "--release", "-p", "maki-game"], cwd=ROOT, capture_output=True, text=True
    )
    if result.returncode != 0:
        raise SystemExit("FAIL build:\n" + result.stderr[-2000:])


def read_dump(path):
    """The header (8 bytes: width, height) and the RGBA floats of a picture dump."""
    data = path.read_bytes()
    return data[:8], array.array("f", data[8:])


def relmse(picture, reference):
    """Mean over the pixels and R, G, B of (x - r)^2 / (r^2 + 0.01), without the one
    pixel in 10000 with the largest error. A lamp is some hundred times brighter than
    the room: one lamp pixel at another place (an edge, a reflection on glass) has
    more error than the rest of the picture. The usual rule for this metric."""
    errors = []
    for i in range(0, len(reference), 4):
        error = 0.0
        for c in (i, i + 1, i + 2):
            r = reference[c]
            error += (picture[c] - r) ** 2 / (r * r + 0.01)
        errors.append(error)
    errors.sort()
    kept = errors[:len(errors) - len(errors) // 10000]
    return sum(kept) / (len(kept) * 3)


def size_of(header):
    return struct.unpack("<2I", header)


def crop(picture, width, box):
    """The part `box` (x, y, w, h) of a picture of RGBA floats, as a new picture."""
    x, y, w, h = box
    part = array.array("f")
    for row in range(y, y + h):
        part.extend(picture[(row * width + x) * 4:(row * width + x + w) * 4])
    return part


def gradients(picture, width):
    """For each pixel: the squared differences to its right and lower neighbor, summed.
    The value of a pixel is its brightness b as b / (1 + b): the light itself and a
    dark corner then count about the same, as they do for the eye after the tone curve."""
    values = [0.0] * (len(picture) // 4)
    for i in range(len(values)):
        b = 0.2126 * picture[4 * i] + 0.7152 * picture[4 * i + 1] + 0.0722 * picture[4 * i + 2]
        values[i] = b / (1.0 + b) if b > 0.0 else 0.0
    energy = [0.0] * len(values)
    for i, v in enumerate(values):
        if (i + 1) % width:
            energy[i] = (values[i + 1] - v) ** 2
        if i + width < len(values):
            energy[i] += (values[i + width] - v) ** 2
    return energy


# A pixel of the reference with more gradient energy than this is an edge: a step of
# about 3 % of the display range to a neighbor.
EDGE = 1e-3


def sharpness(picture, reference, width):
    """(sharpness, noise): the gradient energy of the picture over that of the
    reference, at the edge pixels of the reference and at all other pixels.
    Sharpness: 1.0 is as sharp, below is blurred. Noise: 1.0 is as smooth, above is
    noise or a pattern that the reference does not have. Noise at an edge also raises
    the sharpness, so a value near 1.0 counts only if `noise` is near 1.0 too."""
    ours, theirs = gradients(picture, width), gradients(reference, width)
    sums = {True: [0.0, 0.0], False: [0.0, 0.0]}
    for a, b in zip(ours, theirs):
        both = sums[b > EDGE]
        both[0] += a
        both[1] += b
    return tuple(sums[edge][0] / max(sums[edge][1], 1e-12) for edge in (True, False))


def side_by_side(path, picture, reference, width):
    """One PNG: the picture, the reference, and four times their difference."""
    from PIL import Image  # only this command needs it

    def display(values):
        # A plain curve for a look at the picture: x / (1 + x), gamma 2.2.
        return bytes(
            min(255, int(255 * (max(v, 0.0) / (1.0 + max(v, 0.0))) ** (1 / 2.2) + 0.5))
            for i, v in enumerate(values) if i % 4 != 3
        )

    height = len(picture) // 4 // width
    difference = array.array("f", (4.0 * abs(a - b) for a, b in zip(picture, reference)))
    sheet = Image.new("RGB", (3 * width, height))
    for index, values in enumerate((picture, reference, difference)):
        sheet.paste(Image.frombytes("RGB", (width, height), display(values)), (index * width, 0))
    sheet.save(path)


REFERENCE_SAMPLES = 65536
# `WALK_FRAMES` of game/src/bench.rs: the frame that shows the last motion of the walk script.
WALK_FRAMES = 180


def command_reference(args):
    build()
    path = REFERENCE / f"{args.name or args.scenario}.dump"
    path.parent.mkdir(parents=True, exist_ok=True)
    # 64 samples in a frame: the largest count, so the fewest frames.
    # No filter: the reference is the mean over time and nothing else, also for the
    # anti-aliasing (`taa_blend = 0`: the plain mean of all frames at rest). It has the
    # limit of bounce light of the default config (`indirect_clamp`): the limit changes
    # what the picture converges to, and the reference is the target of the denoiser.
    # `--set render.indirect_clamp=0 --name NAME` gives the picture without the limit.
    overrides = {"render.max_samples": REFERENCE_SAMPLES, "render.samples_per_frame": 64, "render.denoise_passes": 0, "render.denoise_direct_passes": 0, "render.denoise_specular_passes": 0, "render.history_clamp": 0.0, "render.taa_blend": 0.0, **parse_overrides(args.set)}
    frames = REFERENCE_SAMPLES // 64
    # The picture starts again when the walk script ends.
    if "walk" in (SCENARIOS[args.scenario][3:] or [[]])[0]:
        frames += WALK_FRAMES
    run_scenario(args.scenario, frames, 0, "reference", overrides, ["--bench-static"], path)
    print(f"{path.relative_to(ROOT).as_posix()}: {REFERENCE_SAMPLES} samples")


def parse_overrides(pairs):
    # A value is a JSON value (number, true, list) or, if it is not valid JSON, a string.
    def value(text):
        try:
            return json.loads(text)
        except ValueError:
            return text

    return {key: value(text) for key, text in (pair.split("=", 1) for pair in pairs)}


def command_quality(args):
    reference_path = REFERENCE / f"{args.reference or args.scenario}.dump"
    if not reference_path.exists():
        raise SystemExit(f"no {reference_path}: run `reference {args.reference or args.scenario}` first")
    build()
    overrides = parse_overrides(args.set)
    arguments = [] if args.moving else ["--bench-static"]
    if args.light_frames is not None:
        arguments = ["--bench-light-frames", str(args.light_frames)]
    # The first frames have the cost of the load.
    warmup = min(10, args.frames // 4)
    params, columns = run_scenario(args.scenario, args.frames, warmup, args.label, overrides, arguments)
    header, picture = read_dump(WORK / args.scenario / f"{args.label}.dump")
    reference_header, reference = read_dump(reference_path)
    if header != reference_header:
        raise SystemExit("FAIL: the picture and the reference differ in size")
    width = size_of(header)[0]
    if args.crop:
        box = [int(value) for value in args.crop.split(",")]
        picture, reference, width = crop(picture, width, box), crop(reference, width, box), box[2]
        params += f";crop={args.crop}"
    error = relmse(picture, reference)
    sharp, noise = sharpness(picture, reference, width)
    sheet = WORK / args.scenario / f"{args.label}.png"
    side_by_side(sheet, picture, reference, width)
    gpu_ms = statistics.median(columns["gpu_total"])
    run_id = time.strftime("%Y%m%dT%H%M%S") + "-" + args.label
    commit = commit_id()
    n = len(columns["gpu_total"])
    append(HISTORY, HISTORY_HEADER, [
        [run_id, commit, f"{args.scenario}/relmse", params, 1, "relmse", *[f"{error:.6g}"] * 5],
        [run_id, commit, f"{args.scenario}/sharpness", params, 1, "ratio", *[f"{sharp:.4f}"] * 5],
        [run_id, commit, f"{args.scenario}/noise", params, 1, "ratio", *[f"{noise:.4f}"] * 5],
        [run_id, commit, f"{args.scenario}/gpu_total", params, n, "ms", *(f"{v:.3f}" for v in summarize(columns["gpu_total"]))],
    ])
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(["label", "frames", "relmse", "sharpness", "noise", "gpu_total_ms", "gpu_ms_all_frames", "sheet"])
    writer.writerow([args.label, args.frames, f"{error:.6g}", f"{sharp:.4f}", f"{noise:.4f}", f"{gpu_ms:.3f}", f"{gpu_ms * args.frames:.1f}", sheet.relative_to(ROOT).as_posix()])


def command_same(args):
    """Compares the pictures that two runs of a scenario dumped."""
    a, b = ((WORK / args.scenario / f"{label}.dump").read_bytes() for label in (args.a, args.b))
    if a == b:
        print("ok")
        return
    if a[:8] != b[:8]:
        raise SystemExit("FAIL: the pictures differ in size")
    x, y = array.array("f", a[8:]), array.array("f", b[8:])
    differences = [max(abs(x[i + c] - y[i + c]) for c in range(3)) for i in range(0, len(x), 4)]
    changed = sum(d > 0 for d in differences)
    mean = lambda values: sum(values[c] for i in range(0, len(values), 4) for c in (i, i + 1, i + 2)) / (len(values) * 0.75)
    print(f"pixels_changed,{changed}")
    print(f"pixels,{len(differences)}")
    print(f"max_difference,{max(differences):.6g}")
    print(f"mean_a,{mean(x):.6g}")
    print(f"mean_b,{mean(y):.6g}")
    raise SystemExit(1)


def command_selftest(_args):
    # Two pixels. Errors: (1 - 0)^2 / 0.01 = 100 in one channel, (3 - 2)^2 / 4.01 in one; 6 channels.
    reference = array.array("f", [0, 0, 0, 1, 2, 2, 2, 1])
    picture = array.array("f", [1, 0, 0, 1, 2, 3, 2, 1])
    expected_error = (100 + 1 / 4.01) / 6
    if abs(relmse(picture, reference) - expected_error) > 1e-9:
        raise SystemExit(f"FAIL relmse: expected {expected_error}, actual {relmse(picture, reference)}")
    # Two rows of 0, 0, 1e9, 1e9 (0, 0, 1, 1 after the curve): one edge of height 1 in
    # each row. With 0.5 in place of the second 0: two steps of 0.5, half the energy.
    edge = array.array("f", [v for b in (0, 0, 1e9, 1e9) * 2 for v in (b, b, b, 1)])
    soft = array.array("f", [v for b in (0, 1, 1e9, 1e9) * 2 for v in (b, b, b, 1)])
    # The two edge pixels of `soft` hold one of its two steps: a quarter of the energy.
    # The other step is at pixels that are flat in the reference.
    if abs(sum(gradients(edge, 4)) - 2.0) > 1e-6 or abs(sharpness(soft, edge, 4)[0] - 0.25) > 1e-6:
        raise SystemExit(f"FAIL sharpness: {sum(gradients(edge, 4))}, {sharpness(soft, edge, 4)}")
    if list(crop(edge, 4, (2, 1, 2, 1)))[:4] != [1e9, 1e9, 1e9, 1]:
        raise SystemExit("FAIL crop")
    # 1..100: the median is between 50 and 51; rank p of 99 intervals is 1 + 99 * p.
    expected = (50.5, 95.05, 99.01, 1, 100)
    actual = summarize(list(range(1, 101)))
    close = all(abs(x - y) < 1e-9 for x, y in zip(actual, expected))
    if not close:
        raise SystemExit(f"FAIL summarize: expected {expected}, actual {actual}")
    print("ok")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--label", required=True, help="part of the run_id, for `compare`")
    run.add_argument("--frames", type=int, default=700)
    run.add_argument("--warmup", type=int, default=100, help="first frames that are not measured")
    run.add_argument("--only", nargs="+", metavar="SCENARIO")
    run.set_defaults(function=command_run)
    compare = commands.add_parser("compare")
    compare.add_argument("a")
    compare.add_argument("b")
    compare.set_defaults(function=command_compare)
    reference = commands.add_parser("reference", help="make the reference picture of a scenario")
    reference.add_argument("scenario")
    reference.add_argument("--set", nargs="*", default=[], metavar="TABLE.KEY=VALUE", help="config keys, for example render.max_bounces=3")
    reference.add_argument("--name", help="file name of the reference (default: the scenario)")
    reference.set_defaults(function=command_reference)
    quality = commands.add_parser("quality", help="error of a picture against the reference")
    quality.add_argument("scenario")
    quality.add_argument("--label", required=True)
    quality.add_argument("--frames", type=int, default=64)
    quality.add_argument("--set", nargs="*", default=[], metavar="TABLE.KEY=VALUE", help="config keys, for example render.max_bounces=3")
    quality.add_argument("--moving", action="store_true", help="the light moves; use a multiple of 240 frames")
    quality.add_argument("--light-frames", type=int, metavar="N", help="the light moves in N frames (a multiple of 240), then it stops")
    quality.add_argument("--reference", metavar="SCENARIO", help="use the reference of another scenario")
    quality.add_argument("--crop", metavar="X,Y,W,H", help="measure this part of the picture, in pixels")
    quality.set_defaults(function=command_quality)
    same = commands.add_parser("same", help="compare the dumped pictures of two runs")
    same.add_argument("scenario")
    same.add_argument("a")
    same.add_argument("b")
    same.set_defaults(function=command_same)
    commands.add_parser("selftest").set_defaults(function=command_selftest)
    arguments = parser.parse_args()
    arguments.function(arguments)
