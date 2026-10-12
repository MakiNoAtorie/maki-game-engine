"""Converts a config file of before part 4 (all keys in one list) to the format with
one table for each module.

    python tools/convert_config.py config.toml

The old file stays as `config.toml.old`. A file that has tables already is not changed.
"""

import json
import sys
import tomllib
from pathlib import Path

# Old key -> (table, key). A key that is not here goes to `[render]` with its name.
MOVED = {
    "window_title": ("window", "title"),
    "window_width": ("window", "width"),
    "window_height": ("window", "height"),
    "vsync": ("window", "vsync"),
    "render_scale": ("render", "scale"),
    "render_width": ("render", "width"),
    "render_height": ("render", "height"),
    "start_room": ("rooms", "start"),
    "start_spawn": ("rooms", "start_spawn"),
    "camera_position": ("camera", "position"),
    "camera_target": ("camera", "target"),
    "camera_fov_degrees": ("camera", "fov_degrees"),
    "camera_follow_time": ("camera", "follow_time"),
    "camera_margin": ("camera", "margin"),
    "player_model_path": ("player", "model_path"),
    "player_walk_speed": ("player", "walk_speed"),
    "player_run_speed": ("player", "run_speed"),
    "fixed_hz": ("time", "fixed_hz"),
    "max_fixed_steps": ("time", "max_fixed_steps"),
}


def convert(old):
    """{old key: value} -> the text of the new file."""
    tables = {}
    for key, value in old.items():
        table, name = MOVED.get(key, ("render", key))
        # A JSON number, string, or list of numbers is the same text in TOML.
        tables.setdefault(table, []).append(f"{name} = {json.dumps(value)}")
    return "".join(f"[{table}]\n" + "\n".join(lines) + "\n\n" for table, lines in tables.items())


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    path = Path(sys.argv[1])
    old = tomllib.loads(path.read_text(encoding="utf-8"))
    if any(isinstance(value, dict) for value in old.values()):
        raise SystemExit(f"{path} has tables: it is in the new format")
    backup = path.with_name(path.name + ".old")
    backup.write_text(path.read_text(encoding="utf-8"), encoding="utf-8")
    path.write_text(convert(old), encoding="utf-8")
    print(f"{path} converted; the old file is {backup}")


if __name__ == "__main__":
    main()
