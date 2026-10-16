"""Replaces text in files and keeps their line ends (CRLF or LF).

    python tools/replace.py FILE OLD NEW [OLD NEW ...]
    python tools/replace.py FILE --script EDITS.py    (EDITS.py sets PAIRS = [(old, new), ...])
    python tools/replace.py --script EDITS.py         (EDITS.py sets EDITS = [(file, old, new), ...])

Each OLD must be in its file one time: else no file is written and the exit code is
1. The edits of one file apply in their order. Text with a backslash or a quote: put
the edits into a script file (written with the Write tool), not on the command line
of a shell.
"""

import runpy
import sys


def edits_from_arguments():
    """[(file, old, new), ...], or None for a wrong command line."""
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "--script":
        return runpy.run_path(args[1])["EDITS"]
    if len(args) == 3 and args[1] == "--script":
        return [(args[0], old, new) for old, new in runpy.run_path(args[2])["PAIRS"]]
    if len(args) >= 3 and len(args) % 2 == 1:
        return [(args[0], old, new) for old, new in zip(args[1::2], args[2::2])]
    return None


def main() -> int:
    edits = edits_from_arguments()
    if edits is None:
        print(__doc__)
        return 2
    # Each file is read once, and written only when all its edits and those of the
    # other files were found.
    texts = {}
    for path, old, new in edits:
        if path not in texts:
            with open(path, encoding="utf-8", newline="") as file:
                text = file.read()
            texts[path] = ("\r\n" in text, text.replace("\r\n", "\n"))
        crlf, text = texts[path]
        old = old.replace("\r\n", "\n")
        count = text.count(old)
        if count != 1:
            found = "not found" if count == 0 else f"found {count} times"
            print(f"{path}: {found}: {old[:80]!r}")
            return 1
        texts[path] = (crlf, text.replace(old, new.replace("\r\n", "\n")))
    for path, (crlf, text) in texts.items():
        if crlf:
            text = text.replace("\n", "\r\n")
        with open(path, "w", encoding="utf-8", newline="") as file:
            file.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
