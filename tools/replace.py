"""Replaces text in a file and keeps its line ends (CRLF or LF).

    python tools/replace.py FILE OLD NEW [OLD NEW ...]
    python tools/replace.py FILE --script EDITS.py    (EDITS.py sets PAIRS = [(old, new), ...])

Each OLD must be in the file: else nothing is written and the exit code is 1. Text
with a backslash or a quote: put the pairs into a script file (written with the
Write tool), not on the command line of a shell.
"""

import runpy
import sys


def main() -> int:
    if len(sys.argv) == 4 and sys.argv[2] == "--script":
        pairs = runpy.run_path(sys.argv[3])["PAIRS"]
    elif len(sys.argv) >= 4 and len(sys.argv) % 2 == 0:
        pairs = list(zip(sys.argv[2::2], sys.argv[3::2]))
    else:
        print(__doc__)
        return 2
    path = sys.argv[1]
    with open(path, encoding="utf-8", newline="") as file:
        text = file.read()
    crlf = "\r\n" in text
    text = text.replace("\r\n", "\n")
    for old, new in pairs:
        old = old.replace("\r\n", "\n")
        if old not in text:
            print(f"{path}: not found: {old[:80]!r}")
            return 1
        text = text.replace(old, new.replace("\r\n", "\n"))
    if crlf:
        text = text.replace("\n", "\r\n")
    with open(path, "w", encoding="utf-8", newline="") as file:
        file.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
