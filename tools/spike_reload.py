"""Spike of part 6: a change of the script crate reaches a program that runs.

Starts `spike-host`, waits for its first load, changes two lines of
`spike/scripts/src/lib.rs`, builds the script DLL, and shows what the host prints
after its second load. The source file is put back at the end.

    python tools/spike_reload.py
"""

import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "spike" / "scripts" / "src" / "lib.rs"
# Always the two packages. With the script crate alone, the features of some crates
# are other ones: cargo then builds the engine DLL again, and the host has that file.
BUILD = ["cargo", "build", "--profile", "dynamic", "-p", "spike-scripts", "-p", "spike-host"]
CHANGES = [('LABEL: &str = "v1"', 'LABEL: &str = "v2"'), ("PAD: usize = 1;", "PAD: usize = 64;")]


def main() -> int:
    subprocess.run(BUILD, cwd=ROOT, check=True)
    # The DLL of the standard library is not in the directory of the program.
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True).stdout.strip()
    target = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout.split("host: ")[1].split()[0]
    env = dict(os.environ, PATH=str(Path(sysroot) / "lib" / "rustlib" / target / "lib") + os.pathsep + os.environ["PATH"])

    old = SOURCE.read_text(encoding="utf-8")
    new = old
    for before, after in CHANGES:
        assert before in new, before
        new = new.replace(before, after)

    host = subprocess.Popen(
        [str(ROOT / "target" / "dynamic" / "spike-host.exe"), "2"],
        cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        for line in host.stdout:
            print("host |", line.rstrip())
            if line.strip() == "waiting":
                break
        SOURCE.write_text(new, encoding="utf-8", newline="\n")
        start = time.perf_counter()
        subprocess.run(BUILD, cwd=ROOT, check=True)
        print(f"build_s,{time.perf_counter() - start:.2f}")
        for line in host.stdout:
            print("host |", line.rstrip())
        return host.wait(timeout=30)
    finally:
        host.kill()
        SOURCE.write_text(old, encoding="utf-8", newline="\n")
        # The DLL of the source as it is in the repository.
        subprocess.run(BUILD, cwd=ROOT, check=True, capture_output=True)


if __name__ == "__main__":
    sys.exit(main())
