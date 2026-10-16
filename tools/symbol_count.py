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
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENGINE_DLL = ROOT / "target" / "dynamic" / "maki_engine_dylib.dll"
LIMIT = 65535
# About 5,500 below the limit: room for the next stage to see the limit before a link error.
MARGIN = 60000


# PE layout (offsets in bytes).
E_LFANEW = 0x3C  # DOS header: offset of the PE header
OPTIONAL_HEADER = 24  # from the PE header: signature (4) + file header (20)
DATA_DIRECTORIES = {0x10B: 96, 0x20B: 112}  # from the optional header: PE32, PE32+ (64-bit)
SECTION_HEADER_SIZE = 40
EXPORT_NUMBER_OF_NAMES = 24  # from the start of the export directory


def count(path):
    """NumberOfNames of the export directory of a PE file (0 if it has no exports)."""
    data = Path(path).read_bytes()
    pe = struct.unpack_from("<I", data, E_LFANEW)[0]
    # File header: NumberOfSections at +6, SizeOfOptionalHeader at +20.
    sections, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + OPTIONAL_HEADER
    magic = struct.unpack_from("<H", data, optional)[0]
    # The export directory is the first data directory.
    rva, size = struct.unpack_from("<II", data, optional + DATA_DIRECTORIES[magic])
    if size == 0:
        return 0
    table = optional + optional_size
    for i in range(sections):
        # Section header: VirtualSize at +8, VirtualAddress at +12, PointerToRawData at +20.
        virtual_size, virtual, _, raw = struct.unpack_from("<IIII", data, table + SECTION_HEADER_SIZE * i + 8)
        if virtual <= rva < virtual + virtual_size:
            return struct.unpack_from("<I", data, rva - virtual + raw + EXPORT_NUMBER_OF_NAMES)[0]
    raise ValueError(f"{path}: the export directory is in no section")


def missing_opt_tables(root=ROOT):
    """Engine crates of the DLL that have no `[profile.dynamic.package.<name>]` table at opt-level 2."""
    manifests = [*root.glob("crates/*/Cargo.toml"), root / "crates/engine/dylib/Cargo.toml"]
    names = []
    for m in manifests:
        toml = tomllib.loads(m.read_text(encoding="utf-8"))
        if not toml.get("lib", {}).get("proc-macro"):
            names.append(toml["package"]["name"])
    tables = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["profile"]["dynamic"]["package"]
    return [n for n in names if tables.get(n, {}).get("opt-level") != 2]


def selftest():
    # A system DLL is on each Windows machine and on CI: it has names, and fewer than the limit.
    dll = Path(os.environ.get("SystemRoot", "C:/Windows")) / "System32" / "kernel32.dll"
    found = count(dll)
    if not 1000 < found < LIMIT:
        raise SystemExit(f"FAIL symbol_count selftest: {found}")
    if missing_opt_tables():
        raise SystemExit(f"FAIL symbol_count selftest: opt tables {missing_opt_tables()}")


def main():
    if sys.argv[1:] == ["selftest"]:
        selftest()
        return
    missing = missing_opt_tables()
    if missing:
        raise SystemExit(f"FAIL Cargo.toml: no [profile.dynamic.package.<name>] opt-level 2 for {', '.join(missing)}")
    path = Path(sys.argv[1]) if sys.argv[1:] else ENGINE_DLL
    if not path.exists():
        raise SystemExit(f"FAIL symbol count: no {path}")
    found = count(path)
    print("file,count,limit,margin")
    print(f"{path.name},{found},{LIMIT},{MARGIN}")
    if found > MARGIN:
        raise SystemExit(f"FAIL {path.name}: {found} exported names, above the margin {MARGIN} (limit {LIMIT})")


if __name__ == "__main__":
    main()
