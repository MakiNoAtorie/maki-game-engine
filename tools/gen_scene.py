# tool: seeded stress scene (spheres in the room; optional mirrors, glass, lights) as a .glb file | run: python tools/gen_scene.py --objects 256 --segments 48 --seed 1 --out target/bench/stress.glb
"""Writes a glTF binary file with `objects` UV spheres at seeded random positions.

Default: one node for each sphere (the engine makes one BLAS and one TLAS instance for
each node). `--merged`: all spheres of one material kind in one mesh, with the same
triangles. The two forms separate the cost of the TLAS from the cost of the triangles.

`--mirror-share` and `--glass-share` give a part of the spheres a mirror material
(metallic 1, roughness 0) or a glass material (KHR_materials_transmission and
KHR_materials_ior). `--lights` adds point lights (KHR_lights_punctual). A loader that
does not read these fields shows diffuse spheres and no lights from the file.

Prints one CSV line: path,objects,triangles,bytes,mirrors,glass,lights.
"""
import argparse
import json
import math
import random
import struct
import sys
from array import array
from pathlib import Path

DIFFUSE = 8  # diffuse base colors, used in turn
MIRROR, GLASS = DIFFUSE, DIFFUSE + 1  # material indices


def sphere(segments, rings):
    """Unit sphere: positions (also the normals) and triangle indices."""
    positions, indices = [], []
    for ring in range(rings + 1):
        polar = math.pi * ring / rings
        for segment in range(segments + 1):
            turn = 2 * math.pi * segment / segments
            positions.append(
                (math.sin(polar) * math.cos(turn), math.cos(polar), math.sin(polar) * math.sin(turn))
            )
    row = segments + 1
    for ring in range(rings):
        for segment in range(segments):
            a, b = ring * row + segment, (ring + 1) * row + segment
            # The quads at the two poles are one triangle each: the other has no area.
            if ring != 0:
                indices += (a, a + 1, b)
            if ring != rings - 1:
                indices += (a + 1, b + 1, b)
    return positions, indices


def placements(objects, room, rng):
    """(center, radius) of each sphere, all inside the room."""
    width, depth, height = room
    out = []
    for _ in range(objects):
        radius = rng.uniform(0.08, 0.3)
        out.append(
            (
                (
                    rng.uniform(-width / 2 + radius, width / 2 - radius),
                    rng.uniform(radius, height - radius),
                    rng.uniform(-depth / 2 + radius, depth / 2 - radius),
                ),
                radius,
            )
        )
    return out


def triangle_count(objects, segments):
    """Known answer: a sphere has segments * 2 * (rings - 1) triangles."""
    return objects * segments * 2 * (max(2, segments // 2) - 1)


def write_scene(
    path, objects=256, segments=48, seed=1, merged=False, room=(8.0, 8.0, 3.0),
    mirror_share=0.0, glass_share=0.0, lights=0,
):
    """Writes the file. Returns (triangles, mirrors, glass spheres)."""
    assert objects > 0 and segments >= 3 and lights >= 0
    assert 0.0 <= mirror_share and 0.0 <= glass_share and mirror_share + glass_share <= 1.0
    rings = max(2, segments // 2)
    rng = random.Random(seed)
    unit_positions, unit_indices = sphere(segments, rings)
    spots = placements(objects, room, rng)
    colors = [[rng.uniform(0.3, 0.9) for _ in range(3)] + [1.0] for _ in range(DIFFUSE)]
    # Drawn after the values above: the same seed gives the same spheres with and
    # without the options below.
    rolls = [rng.random() for _ in range(objects)]
    width, depth, height = room
    light_spots = [
        [rng.uniform(-width / 2 + 0.5, width / 2 - 0.5), rng.uniform(height / 2, height - 0.3),
         rng.uniform(-depth / 2 + 0.5, depth / 2 - 0.5)]
        for _ in range(lights)
    ]

    def material_of(index):
        if rolls[index] < mirror_share:
            return MIRROR
        if rolls[index] < mirror_share + glass_share:
            return GLASS
        return index % DIFFUSE

    special = mirror_share > 0.0 or glass_share > 0.0
    materials = [{"pbrMetallicRoughness": {"baseColorFactor": color}} for color in colors]
    extensions_used = []
    if special:
        materials.append(
            {"pbrMetallicRoughness": {"baseColorFactor": [0.9, 0.9, 0.9, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.0}}
        )
        materials.append(
            {
                "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.0},
                "extensions": {"KHR_materials_transmission": {"transmissionFactor": 1.0}, "KHR_materials_ior": {"ior": 1.5}},
            }
        )
        extensions_used += ["KHR_materials_transmission", "KHR_materials_ior"]

    attributes = {"POSITION": 0, "NORMAL": 1}
    if merged:
        # One list of indices for each material kind. All diffuse spheres get material 0.
        positions, normals, index_lists = [], [], {}
        for index, ((cx, cy, cz), radius) in enumerate(spots):
            base = len(positions)
            positions += [(cx + radius * x, cy + radius * y, cz + radius * z) for x, y, z in unit_positions]
            normals += unit_positions
            kind = material_of(index) if material_of(index) >= DIFFUSE else 0
            index_lists.setdefault(kind, []).extend(base + i for i in unit_indices)
        meshes = [
            {"primitives": [{"attributes": attributes, "indices": 2 + n, "material": kind}]}
            for n, kind in enumerate(index_lists)
        ]
        nodes = [{"mesh": n} for n in range(len(meshes))]
        index_lists = list(index_lists.values())
        stored = sum(len(indices) for indices in index_lists) // 3
    else:
        positions, normals, index_lists = unit_positions, unit_positions, [unit_indices]
        # The meshes share the vertex data and differ only in the material.
        meshes = [
            {"primitives": [{"attributes": attributes, "indices": 2, "material": m}]}
            for m in range(len(materials))
        ]
        nodes = [
            {"mesh": material_of(i), "translation": list(center), "scale": [radius] * 3}
            for i, (center, radius) in enumerate(spots)
        ]
        stored = len(unit_indices) // 3 * objects

    document_extensions = {}
    if lights:
        intensity = 12.0 / lights
        document_extensions["KHR_lights_punctual"] = {
            "lights": [{"type": "point", "color": [1.0, 0.9, 0.8], "intensity": intensity} for _ in range(lights)]
        }
        nodes += [
            {"translation": spot, "extensions": {"KHR_lights_punctual": {"light": n}}}
            for n, spot in enumerate(light_spots)
        ]
        extensions_used.append("KHR_lights_punctual")

    if sys.byteorder != "little":
        raise SystemExit("glTF buffers are little-endian; this script does not swap bytes")
    # Each part is a multiple of 4 bytes.
    parts = [array("f", [c for p in positions for c in p]).tobytes(), array("f", [c for n in normals for c in n]).tobytes()]
    parts += [array("I", indices).tobytes() for indices in index_lists]
    views, offset = [], 0
    for part in parts:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(part)})
        offset += len(part)
    blob = b"".join(parts)
    lows = [min(p[axis] for p in positions) for axis in range(3)]
    highs = [max(p[axis] for p in positions) for axis in range(3)]
    accessors = [
        {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "min": lows, "max": highs},
        {"bufferView": 1, "componentType": 5126, "count": len(normals), "type": "VEC3"},
    ]
    accessors += [
        {"bufferView": 2 + n, "componentType": 5125, "count": len(indices), "type": "SCALAR"}
        for n, indices in enumerate(index_lists)
    ]
    document = {
        "asset": {"version": "2.0", "generator": "maki tools/gen_scene.py"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    if document_extensions:
        document["extensions"] = document_extensions
    if extensions_used:
        document["extensionsUsed"] = extensions_used
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)  # chunks are a multiple of 4 bytes
    total = 12 + 8 + len(text) + 8 + len(blob)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        struct.pack("<4sII", b"glTF", 2, total)
        + struct.pack("<I4s", len(text), b"JSON")
        + text
        + struct.pack("<I4s", len(blob), b"BIN\0")
        + blob
    )

    triangles = triangle_count(objects, segments)
    assert stored == triangles, f"seed {seed}: {stored} triangles, expected {triangles}"
    assert path.stat().st_size == total, f"seed {seed}: file size is not the header size"
    kinds = [material_of(i) for i in range(objects)]
    return triangles, kinds.count(MIRROR), kinds.count(GLASS)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--objects", type=int, default=256)
    parser.add_argument("--segments", type=int, default=48, help="triangles around a sphere; rings = segments / 2")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--merged", action="store_true", help="one mesh for all spheres of a material kind")
    parser.add_argument("--room", type=float, nargs=3, default=(8.0, 8.0, 3.0), metavar=("WIDTH", "DEPTH", "HEIGHT"))
    parser.add_argument("--mirror-share", type=float, default=0.0, help="part of the spheres that are mirrors, 0 to 1")
    parser.add_argument("--glass-share", type=float, default=0.0, help="part of the spheres that are glass, 0 to 1")
    parser.add_argument("--lights", type=int, default=0, help="point lights in the file")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    count, mirrors, glass = write_scene(
        args.out, args.objects, args.segments, args.seed, args.merged, tuple(args.room),
        args.mirror_share, args.glass_share, args.lights,
    )
    print(f"{args.out},{args.objects},{count},{Path(args.out).stat().st_size},{mirrors},{glass},{args.lights}")
