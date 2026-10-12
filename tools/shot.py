# tool: PNG screenshots of the game as the window shows it, with no panel | run: python tools/shot.py OUT.png [--room room_b.ron --spawn west] [--frames 300] [--script walk] [--set table.key=value ...]
"""Runs the release build for some frames with `--bench-shot` and writes the last
picture as a PNG. For a look at the picture: tone curve, bloom, grade."""
import argparse
import array
import json
import struct
import subprocess
import sys
from pathlib import Path

from PIL import Image

from bench import config_text

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target" / "shot"

parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
parser.add_argument("out")
parser.add_argument("--room", default="room_a.ron", help="a file in game/assets/rooms")
parser.add_argument("--spawn", default="start")
parser.add_argument("--frames", type=int, default=300)
parser.add_argument("--script", help="walk: the light then does not move")
parser.add_argument("--set", nargs="*", default=[], metavar="TABLE.KEY=VALUE", help="config keys (JSON values), for example render.exposure=2")
args = parser.parse_args()

def value(text):
    try:
        return json.loads(text)
    except ValueError:
        return text

keys = {
    "rooms.start": (ROOT / "game" / "assets" / "rooms" / args.room).as_posix(),
    "rooms.start_spawn": args.spawn,
    "render.samples_per_frame": 1,
    **{key: value(text) for key, text in (pair.split("=", 1) for pair in args.set)},
}
WORK.mkdir(parents=True, exist_ok=True)
(WORK / "config.toml").write_text(config_text(keys))
dump = WORK / "shot.dump"
binary = ROOT / "target" / "release" / ("maki-game.exe" if sys.platform == "win32" else "maki-game")
command = [binary, "config.toml", "--bench-frames", str(args.frames), "--bench-dump", dump, "--bench-shot"]
command += ["--bench-script", args.script] if args.script else ["--bench-static"]
# The game writes bench.csv into its working directory. The room files that an event
# names are relative to the repository root: a script that changes the room (trip)
# does not work from here.
result = subprocess.run(command, cwd=WORK, capture_output=True, text=True)
if result.returncode != 0 or not dump.exists():
    raise SystemExit(f"FAIL: exit code {result.returncode}\n{result.stderr[-2000:]}")
data = dump.read_bytes()
width, height = struct.unpack("<2I", data[:8])
values = array.array("f", data[8:])
pixels = bytes(min(255, int(v * 255.0 + 0.5)) for i, v in enumerate(values) if i % 4 != 3)
Image.frombytes("RGB", (width, height), pixels).save(args.out)
print(args.out)
