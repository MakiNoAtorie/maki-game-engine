"""Converts room files of before stage 29 to the event format of stage 29.

    python tools/convert_rooms.py game/assets/rooms/*.ron
    python tools/convert_rooms.py --selftest

- `If("flag", [..], [..])` -> `If([Flag("flag")], [..], [..])`
- `VariableAtLeast("n", 3)` -> `Variable("n", AtLeast, 3)`
- `VariableEquals("n", 3)` -> `Variable("n", Equals, 3)`
- an `area` with the size (0, 0) is removed: an event with no area has no `area`.

The text is changed in place, so comments and line breaks stay. Texts in quotes and
comments are not changed. A file in the new format is not changed. The old file
stays as `NAME.old` if the file was changed.
"""

import glob
import re
import sys
from pathlib import Path

STRING = r'"(?:[^"\\]|\\.)*"'
ZERO = r"\s*0(?:\.0*)?\s*"
PATTERN = re.compile(
    # An old `If`: its first argument is a text. A new one starts with `[`.
    rf"\bIf\(\s*(?P<flag>{STRING})\s*,"
    rf"|\bVariable(?P<compare>AtLeast|Equals)\(\s*(?P<name>{STRING})\s*,"
    rf"|\barea:\s*\(\s*center:\s*\([^()]*\)\s*,\s*size:\s*\({ZERO},{ZERO}\)\s*,?\s*\)\s*,?[ \t]*"
    # Last: a text or a comment that holds none of the above at its start.
    rf"|{STRING}|//[^\n]*|/\*.*?\*/",
    re.DOTALL,
)


def convert(text):
    def new(match):
        if match["flag"]:
            return f"If([Flag({match['flag']})],"
        if match["compare"]:
            return f"Variable({match['name']}, {match['compare']},"
        if match[0].startswith("area"):
            return ""
        return match[0]

    return PATTERN.sub(new, text)


def selftest():
    old = """events: [
        // If("in a comment", [], []) stays.
        (name: "a", area: (center: (1.0, 2), size: (0.0, 0)), pages: [
            (conditions: [Flag("x"), VariableAtLeast("n", 3), VariableEquals("m", -1)],
             trigger: Touch, commands: [
                Message("If(\\"quoted\\", [], [])"),
                If("open", [If( "late" , [], [])], [Wait(1.0)]),
            ])]),
        (name: "b", area: (center: (1, 2), size: (1, 0.5)), pages: [])]"""
    new = convert(old)
    assert '// If("in a comment", [], []) stays.' in new
    assert 'Message("If(\\"quoted\\", [], [])")' in new
    assert 'If([Flag("open")], [If([Flag("late")], [], [])], [Wait(1.0)])' in new
    assert 'Variable("n", AtLeast, 3), Variable("m", Equals, -1)' in new
    assert '(name: "a", pages: [' in new
    assert "area: (center: (1, 2), size: (1, 0.5))" in new
    # A file in the new format is not changed.
    assert convert(new) == new
    print("ok")


def main():
    if sys.argv[1:] == ["--selftest"]:
        return selftest()
    # The shell of Windows does not expand `*`.
    paths = [Path(path) for arg in sys.argv[1:] for path in (glob.glob(arg) or [arg])]
    if not paths:
        raise SystemExit(__doc__)
    for path in paths:
        # `newline=""`: the line ends of the file stay.
        with open(path, encoding="utf-8", newline="") as file:
            old = file.read()
        new = convert(old)
        if new == old:
            print(f"{path}: no change")
            continue
        with open(path.with_name(path.name + ".old"), "w", encoding="utf-8", newline="") as file:
            file.write(old)
        with open(path, "w", encoding="utf-8", newline="") as file:
            file.write(new)
        print(f"{path} converted; the old file is {path.name}.old")


if __name__ == "__main__":
    main()
