"""The reload test of the loader: a change of the script crate of the project reaches
a program that runs, and the build of it does not write the engine DLL.

Starts `reload_test` (programs/editor/src/bin), waits for its first load, changes
one line of `game/scripts/src/shake.rs`, builds with the build command of the
editor, and shows what the program prints after its second load. The source file is
put back at the end.

    python tools/reload_test.py
"""

import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "game" / "scripts" / "src" / "shake.rs"
# The build command of the editor (`maki_engine::BUILD`), with the DLL package of the
# project. Always the two packages: with the script crate alone, the features of
# some crates are other ones, cargo then builds the engine DLL again, and the program
# that runs has that file.
BUILD = ["cargo", "build", "--profile", "dynamic", "-p", "maki-editor-app", "-p", "maki-game-dll"]
# The row text of the command of the game: the program prints it after each load.
CHANGES = [('"Shake the camera: ', '"Shake the view: ')]


def main() -> int:
    subprocess.run(BUILD, cwd=ROOT, check=True)
    # The DLL of the standard library is not in the directory of the program.
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True).stdout.strip()
    target = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout.split("host: ")[1].split()[0]
    env = dict(os.environ, PATH=str(Path(sysroot) / "lib" / "rustlib" / target / "lib") + os.pathsep + os.environ["PATH"])

    # Bytes, not text: the file goes back with its own line ends.
    old = SOURCE.read_bytes()
    new = old
    for before, after in CHANGES:
        assert before.encode() in new, before
        new = new.replace(before.encode(), after.encode())

    host = subprocess.Popen(
        [str(ROOT / "target" / "dynamic" / "reload_test.exe"), "game", "2"],
        cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        for line in host.stdout:
            print("host |", line.rstrip())
            if line.strip() == "waiting":
                break
        SOURCE.write_bytes(new)
        start = time.perf_counter()
        subprocess.run(BUILD, cwd=ROOT, check=True)
        print(f"build_s,{time.perf_counter() - start:.2f}")
        for line in host.stdout:
            print("host |", line.rstrip())
        return host.wait(timeout=30)
    finally:
        host.kill()
        SOURCE.write_bytes(old)
        # The DLL of the source as it is in the repository.
        subprocess.run(BUILD, cwd=ROOT, check=True, capture_output=True)


if __name__ == "__main__":
    sys.exit(main())
