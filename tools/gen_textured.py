# tool: test textures (PNG) and a textured glTF model, made by code | run: python tools/gen_textured.py target/textured
"""Writes the files of the textured test room into a directory:

- `checker.png` (64 x 64, two tiles; linear filter) and `bricks.png` (16 x 16, pixel art;
  nearest filter): the floor and the walls of the room, named by the room file.
- `crate.png` (16 x 16, pixel art): a texture for a cube of the room file.
- `props.glb`: a model with its images in the file. A crate (pixel art, nearest
  sampler), a plant of two crossed cards and a fence (alpha mode MASK: textures with
  holes), and a sign with an emissive texture.

All content is drawn by the code below: no file is made by hand. The same call gives
the same bytes. `tools/bench.py` uses it for the `textured_*` scenarios.
"""
import json
import struct
import sys
import zlib
from array import array
from pathlib import Path


def png(width, height, pixel):
    """A PNG file of 8-bit RGBA. `pixel(x, y)` gives (r, g, b, a); y 0 is the top row."""
    rows = b"".join(b"\0" + bytes(c for x in range(width) for c in pixel(x, y)) for y in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def checker(x, y):
    """Two by two tiles, with a thin dark joint: detail of one pixel for the sharpness check."""
    light = (x // 32 + y // 32) % 2 == 0
    joint = x % 32 == 0 or y % 32 == 0
    if joint:
        return (40, 38, 36, 255)
    return (205, 196, 180, 255) if light else (96, 110, 128, 255)


def bricks(x, y):
    """Pixel art: two rows of bricks, the second row shifted by half a brick."""
    row = y // 8
    mortar = y % 8 == 7 or (x + (4 if row else 0)) % 8 == 7
    if mortar:
        return (150, 146, 138, 255)
    # A fixed speckle, so each brick has a little texture of its own.
    shade = ((x * 7 + y * 13) % 5) * 6
    return (150 + shade, 74 + shade // 2, 58, 255)


def crate(x, y):
    """Pixel art: a wooden crate with a frame and a diagonal brace."""
    frame = x in (0, 1, 14, 15) or y in (0, 1, 14, 15)
    brace = abs(x - y) <= 1
    if frame:
        return (88, 58, 32, 255)
    if brace:
        return (112, 76, 40, 255)
    return (168, 124, 70, 255) if x % 4 else (146, 104, 58, 255)


def leaves(x, y):
    """Leaves on a clear background. The clear pixels have the leaf color too: a mip
    level then has no dark edge around a leaf."""
    green = (58, 130 + (x * 5 + y * 3) % 30, 52)
    inside = False
    # (center x, center y, half width, half height) of each leaf.
    for cx, cy, rx, ry in ((16, 14, 11, 7), (46, 12, 12, 6), (32, 30, 13, 8), (14, 46, 10, 9), (48, 48, 11, 8)):
        if ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1.0:
            inside = True
    # The stem.
    inside = inside or (30 <= x <= 33 and y >= 30)
    return (*green, 255 if inside else 0)


def fence(x, y):
    """Pickets and two rails. Repeats along x."""
    picket = 4 <= x % 16 <= 9 and y >= 3 - min(x % 16 - 4, 9 - x % 16)
    rail = 10 <= y <= 12 or 24 <= y <= 26
    return (196, 188, 170, 255 if picket or rail else 0)


def sign_color(x, y):
    """The sign with its lamp off: a dark plate."""
    return (30, 30, 34, 255)


def sign_light(x, y):
    """The light of the sign: an arrow to the right. Black: no light."""
    shaft = 8 <= x <= 40 and 13 <= y <= 18
    head = 40 <= x <= 54 and abs(y - 15.5) <= (54 - x) * 0.75
    return (80, 255, 120, 255) if shaft or head else (0, 0, 0, 255)


def quad(origin, u, v, uv_size):
    """Positions, normals, texture coordinates, and indices of a quad from `origin`
    with the sides `u` and `v`. The front faces `u x v`. The texture has its top left
    corner at `origin + v` and shows `uv_size` times along u and v."""
    cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
    length = sum(c * c for c in cross) ** 0.5
    normal = tuple(c / length for c in cross)
    add = lambda a, b: tuple(x + y for x, y in zip(a, b))
    positions = [origin, add(origin, u), add(add(origin, u), v), add(origin, v)]
    uvs = [(0.0, uv_size[1]), (uv_size[0], uv_size[1]), (uv_size[0], 0.0), (0.0, 0.0)]
    return positions, [normal] * 4, uvs, [0, 1, 2, 0, 2, 3]


def box(low, high):
    """A box with the texture one time on each side."""
    (x0, y0, z0), (x1, y1, z1) = low, high
    dx, dy, dz = (x1 - x0, 0, 0), (0, y1 - y0, 0), (0, 0, z1 - z0)
    neg = lambda a: tuple(-c for c in a)
    sides = [
        ((x0, y0, z1), dx, dy), ((x1, y0, z0), neg(dx), dy),  # +Z, -Z
        ((x1, y0, z1), neg(dz), dy), ((x0, y0, z0), dz, dy),  # +X, -X
        ((x0, y1, z1), dx, neg(dz)), ((x0, y0, z0), dx, dz),  # +Y, -Y
    ]
    return merge([quad(origin, u, v, (1.0, 1.0)) for origin, u, v in sides])


def merge(parts):
    positions, normals, uvs, indices = [], [], [], []
    for p, n, t, i in parts:
        indices += [index + len(positions) for index in i]
        positions += p
        normals += n
        uvs += t
    return positions, normals, uvs, indices


def write_props(path, images):
    """`props.glb`. `images`: {name: PNG bytes}, stored in the file."""
    names = list(images)
    texture = lambda name, nearest=False: texture_of(names, name, nearest)
    materials = [
        {"name": "crate", "pbrMetallicRoughness": {"baseColorTexture": texture("crate", True), "roughnessFactor": 1.0}},
        {"name": "leaves", "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": True,
         "pbrMetallicRoughness": {"baseColorTexture": texture("leaves"), "roughnessFactor": 1.0}},
        {"name": "fence", "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": True,
         "pbrMetallicRoughness": {"baseColorTexture": texture("fence", True), "roughnessFactor": 1.0}},
        {"name": "sign", "emissiveFactor": [1.0, 1.0, 1.0], "emissiveTexture": texture("sign_light", True),
         "pbrMetallicRoughness": {"baseColorTexture": texture("sign_color"), "roughnessFactor": 1.0}},
    ]
    # Each mesh with its material and the place of its node.
    plant = merge([
        quad((-0.45, 0.0, 0.0), (0.9, 0.0, 0.0), (0.0, 1.2, 0.0), (1.0, 1.0)),
        quad((0.0, 0.0, 0.45), (0.0, 0.0, -0.9), (0.0, 1.2, 0.0), (1.0, 1.0)),
    ])
    meshes = [
        ("crate", box((-0.4, 0.0, -0.4), (0.4, 0.8, 0.4)), 0, (-2.2, 0.0, -1.6)),
        ("plant", plant, 1, (2.3, 0.0, -2.2)),
        # 3 m of fence: the texture repeats six times.
        ("fence", quad((-1.5, 0.0, 0.0), (3.0, 0.0, 0.0), (0.0, 1.0, 0.0), (6.0, 1.0)), 2, (0.6, 0.0, -0.4)),
        # On the back wall, a little in front of it.
        ("sign", quad((-0.7, 0.0, 0.0), (1.4, 0.0, 0.0), (0.0, 0.7, 0.0), (1.0, 1.0)), 3, (-0.5, 1.7, -3.97)),
    ]
    write_glb(path, images, materials, meshes, "maki tools/gen_textured.py")


def texture_of(names, name, nearest=False):
    """The texture of an image of `write_glb`, for a material. `names`: the names of
    the images, in their order."""
    return {"index": names.index(name) * 2 + (1 if nearest else 0)}


def write_glb(path, images, materials, meshes, generator):
    """A `.glb` file. `images`: {name: PNG bytes} for an image in the file, or
    {name: text} for the path of an image file, from the directory of the `.glb`.
    `meshes`: (name, (positions, normals, uvs, indices), material index, place)."""
    names = list(images)
    parts, accessors, gltf_meshes, nodes = [], [], [], []
    for name, (positions, normals, uvs, indices), material, place in meshes:
        first = len(accessors)
        lows = [min(p[axis] for p in positions) for axis in range(3)]
        highs = [max(p[axis] for p in positions) for axis in range(3)]
        for data, kind, code, extra in (
            (positions, "VEC3", "f", {"min": lows, "max": highs}),
            (normals, "VEC3", "f", {}),
            (uvs, "VEC2", "f", {}),
        ):
            parts.append(array(code, [c for value in data for c in value]).tobytes())
            accessors.append({"bufferView": len(parts) - 1, "componentType": 5126, "count": len(data), "type": kind, **extra})
        parts.append(array("I", indices).tobytes())
        accessors.append({"bufferView": len(parts) - 1, "componentType": 5125, "count": len(indices), "type": "SCALAR"})
        attributes = {"POSITION": first, "NORMAL": first + 1, "TEXCOORD_0": first + 2}
        gltf_meshes.append({"name": name, "primitives": [{"attributes": attributes, "indices": first + 3, "material": material}]})
        nodes.append({"name": name, "mesh": len(gltf_meshes) - 1, "translation": list(place)})
    image_views = []
    gltf_images = []
    for name in names:
        data = images[name]
        if isinstance(data, str):
            gltf_images.append({"uri": data, "name": name})
            continue
        parts.append(data + b"\0" * (-len(data) % 4))
        image_views.append((len(parts) - 1, len(data)))
        gltf_images.append({"bufferView": len(parts) - 1, "mimeType": "image/png", "name": name})

    views, offset = [], 0
    for part in parts:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(part)})
        offset += len(part)
    for view, size in image_views:
        views[view]["byteLength"] = size
    blob = b"".join(parts)
    document = {
        "asset": {"version": "2.0", "generator": generator},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        # Two textures for each image: with the linear sampler and with the nearest one.
        "textures": [{"source": image, "sampler": sampler} for image in range(len(names)) for sampler in (0, 1)],
        "samplers": [{"magFilter": 9729, "minFilter": 9987}, {"magFilter": 9728, "minFilter": 9987}],
        "images": gltf_images,
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    if sys.byteorder != "little":
        raise SystemExit("glTF buffers are little-endian; this script does not swap bytes")
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    total = 12 + 8 + len(text) + 8 + len(blob)
    Path(path).write_bytes(
        struct.pack("<4sII", b"glTF", 2, total)
        + struct.pack("<I4s", len(text), b"JSON") + text
        + struct.pack("<I4s", len(blob), b"BIN\0") + blob
    )


def write_textured(directory):
    """Writes all files into `directory`. Returns their names."""
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    files = {
        "checker.png": png(64, 64, checker),
        "bricks.png": png(16, 16, bricks),
        "crate.png": png(16, 16, crate),
    }
    for name, data in files.items():
        (directory / name).write_bytes(data)
    write_props(directory / "props.glb", {
        "crate": files["crate.png"],
        "leaves": png(64, 64, leaves),
        "fence": png(16, 32, fence),
        "sign_color": png(4, 4, sign_color),
        "sign_light": png(64, 32, sign_light),
    })
    return [*files, "props.glb"]


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    for name in write_textured(sys.argv[1]):
        print(Path(sys.argv[1]) / name)
