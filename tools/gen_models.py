"""Makes the placeholder model and the placeholder texture of the game.

    python tools/gen_models.py

- `textures/wood.png`: 32 x 32, pixel art: four planks.
- `models/cabinet.glb`: a cabinet of 0.9 x 1.6 x 0.45 m with two doors, two metal
  handles, and four feet. Its origin is on the floor, and its front faces +Z. It
  names `textures/wood.png`: the image is not in the model file.

All content is drawn by the code below: no file of a modelling tool. The files are
the same in each run. Put a real file of the same name in their place: the room
files do not change.
"""

from pathlib import Path

from gen_textured import box, merge, png, texture_of, write_glb

OUT = Path(__file__).resolve().parent.parent / "game" / "assets" / "rooms"


def wood(x, y):
    """Four planks from the top to the bottom, each with its tone, a dark joint
    between two of them, and a fixed grain."""
    plank = x // 8
    if x % 8 == 7:
        return (58, 38, 24, 255)
    tone = (0, 10, -8, 4)[plank]
    # The grain: darker lines along the plank, at another height for each plank.
    grain = -14 if (y + plank * 5) % 11 == 0 and (x + y) % 5 else 0
    # A knot.
    knot = -30 if (x - 19) ** 2 + (y - 12) ** 2 <= 2 else 0
    shade = tone + grain + knot
    return (134 + shade, 92 + shade * 2 // 3, 54 + shade // 2, 255)


def cabinet():
    """The materials and the meshes of the cabinet, for `write_glb`."""
    wood_texture = texture_of(["wood"], "wood", nearest=True)
    surface = lambda color: {"baseColorTexture": wood_texture, "baseColorFactor": [*color, 1.0], "roughnessFactor": 1.0}
    materials = [
        {"name": "body", "pbrMetallicRoughness": surface((1.0, 1.0, 1.0))},
        {"name": "door", "pbrMetallicRoughness": surface((0.62, 0.55, 0.5))},
        {"name": "handle", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.74, 0.6, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.35}},
        {"name": "foot", "pbrMetallicRoughness": {"baseColorFactor": [0.08, 0.07, 0.06, 1.0], "roughnessFactor": 1.0}},
    ]
    # The body stands on the feet. The doors are 2 cm in front of it, with a gap
    # between them; a handle is on each door, at the gap.
    doors = merge([box((-0.42, 0.14, 0.225), (-0.01, 1.56, 0.245)), box((0.01, 0.14, 0.225), (0.42, 1.56, 0.245))])
    handles = merge([box((-0.07, 0.8, 0.245), (-0.04, 0.98, 0.275)), box((0.04, 0.8, 0.245), (0.07, 0.98, 0.275))])
    feet = merge([box((x - 0.04, 0.0, z - 0.04), (x + 0.04, 0.1, z + 0.04)) for x in (-0.38, 0.38) for z in (-0.16, 0.16)])
    origin = (0.0, 0.0, 0.0)
    meshes = [
        ("body", box((-0.45, 0.1, -0.225), (0.45, 1.6, 0.225)), 0, origin),
        ("doors", doors, 1, origin),
        ("handles", handles, 2, origin),
        ("feet", feet, 3, origin),
    ]
    return materials, meshes


if __name__ == "__main__":
    for directory in ("textures", "models"):
        (OUT / directory).mkdir(parents=True, exist_ok=True)
    (OUT / "textures" / "wood.png").write_bytes(png(32, 32, wood))
    materials, meshes = cabinet()
    images = {"wood": "../textures/wood.png"}
    write_glb(OUT / "models" / "cabinet.glb", images, materials, meshes, "maki tools/gen_models.py")
    for name in ("textures/wood.png", "models/cabinet.glb"):
        print(f"{OUT / name}: {(OUT / name).stat().st_size} bytes")
