"""One-line comments (CODING_STANDARDS.md, "Comments"): fails on two or more `//`
lines next to each other that this branch adds to a Rust file. Doc comments
(`///`, `//!`) are out of scope. Old code is not checked: only lines added since
the merge-base with origin/main.

    python tools/comment_lint.py            the branch against origin/main
    python tools/comment_lint.py selftest
"""

import re
import subprocess
import sys

COMMENT = re.compile(r"^\s*//(?![/!])")


def blocks(diff):
    """(file, line) of each run of two or more added `//` lines in a unified diff with -U0."""
    found, file, line, run = [], None, 0, 0
    for text in diff.splitlines():
        if text.startswith("+++ "):
            file = text[6:] if text.startswith("+++ b/") else None
        elif text.startswith("@@"):
            line = int(re.search(r"\+(\d+)", text).group(1))
            run = 0
        elif text.startswith("+") and file and file.endswith(".rs"):
            run = run + 1 if COMMENT.match(text[1:]) else 0
            if run == 2:
                found.append((file, line - 1))
            line += 1
        elif not text.startswith("-"):
            run = 0
    return found


def selftest():
    diff = "\n".join([
        "+++ b/a.rs", "@@ -0,0 +1,6 @@",
        "+// one", "+fn a() {}", "+    // two", "+    // three", "+/// doc", "+/// doc",
        "+++ b/b.py", "@@ -0,0 +1,2 @@", "+// x", "+// y",
    ])
    if blocks(diff) != [("a.rs", 3)]:
        raise SystemExit(f"FAIL comment_lint selftest: {blocks(diff)}")


def main():
    if sys.argv[1:] == ["selftest"]:
        selftest()
        return
    git = lambda *args: subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout.strip()
    base = git("merge-base", "origin/main", "HEAD")
    found = blocks(git("diff", "-U0", base, "--", "*.rs"))
    for file, line in found:
        print(f"FAIL {file}:{line}: a comment of more than one line")
    if found:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
