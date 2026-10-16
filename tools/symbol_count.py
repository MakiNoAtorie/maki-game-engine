"""The count of the exported names of a DLL: the engine DLL of the editor build has a
limit of 65,535 (Windows, `LNK1189`). Prints CSV: file, count, limit, margin. Fails
when the count is above the margin.

    python tools/symbol_count.py                the engine DLL (target/dynamic)
    python tools/symbol_count.py FILE.dll
    python tools/symbol_count.py selftest
"""

import os
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENGINE_DLL = ROOT / "target" / "dynamic" / "maki_engine_dylib.dll"
LIMIT = 65535
# About 5,500 below the limit: room for the next stage to see the limit before a link error.
MARGIN = 60000


def count(path):
    """NumberOfNames of the export directory of a PE file (0 if it has no exports)."""
    data = Path(path).read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    sections, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    # The data directories start at 96 in PE32 and at 112 in PE32+ (64-bit).
    magic = struct.unpack_from("<H", data, optional)[0]
    rva, size = struct.unpack_from("<II", data, optional + (112 if magic == 0x20B else 96))
    if size == 0:
        return 0
    table = optional + optional_size
    for i in range(sections):
        virtual_size, virtual, _, raw = struct.unpack_from("<IIII", data, table + 40 * i + 8)
        if virtual <= rva < virtual + virtual_size:
            return struct.unpack_from("<I", data, rva - virtual + raw + 24)[0]
    raise ValueError(f"{path}: the export directory is in no section")


def selftest():
    # A system DLL is on each Windows machine and on CI: it has names, and fewer than the limit.
    dll = Path(os.environ.get("SystemRoot", "C:/Windows")) / "System32" / "kernel32.dll"
    found = count(dll)
    if not 1000 < found < LIMIT:
        raise SystemExit(f"FAIL symbol_count selftest: {found}")


def main():
    if sys.argv[1:] == ["selftest"]:
        selftest()
        return
    path = Path(sys.argv[1]) if sys.argv[1:] else ENGINE_DLL
    if not path.exists():
        # CI has no editor build: the check runs where the engine DLL exists.
        print(f"skip symbol count: no {path.relative_to(ROOT) if path == ENGINE_DLL else path}")
        return
    found = count(path)
    print("file,count,limit,margin")
    print(f"{path.name},{found},{LIMIT},{MARGIN}")
    if found > MARGIN:
        raise SystemExit(f"FAIL {path.name}: {found} exported names, above the margin {MARGIN} (limit {LIMIT})")


if __name__ == "__main__":
    main()
