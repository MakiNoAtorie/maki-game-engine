//! The mesh asset and the generators of simple meshes.

use std::f32::consts::{PI, TAU};

use anyhow::{Result, ensure};
use glam::Vec3;
use maki_core::Fields;
use serde::{Deserialize, Serialize};

use crate::TextureRef;

/// How a surface sends light back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Fields)]
pub enum Material {
    /// Sends light back equally in all directions, with the base color (paper, plaster).
    Diffuse,
    /// A perfect mirror. The base color tints the reflection.
    Mirror,
    /// Clear glass: a part of the light is reflected, the rest goes through and bends.
    /// The base color tints the light that goes through.
    #[field(default = Material::Glass { ior: 1.5 })]
    Glass {
        /// Index of refraction: how much the light bends. 1.5 for window glass, 1.33 for water.
        #[field(range = 1.0..=3.0)]
        ior: f32,
    },
    /// A rough mirror (brushed steel, worn brass): the reflection is soft, more so
    /// with more roughness. The base color tints it. No diffuse light.
    #[field(default = Material::Metal { roughness: 0.3 })]
    Metal {
        /// 0.05 (almost a mirror) to 1.0 (no reflection that shows a shape).
        #[field(range = 0.05..=1.0)]
        roughness: f32,
    },
    /// A diffuse surface with the base color under a clear, rough coat (plastic,
    /// paint, wet stone): the coat reflects some percent of the light, more at a flat
    /// angle, with no tint.
    #[field(default = Material::Plastic { roughness: 0.3 })]
    Plastic {
        /// As for `Metal`.
        #[field(range = 0.05..=1.0)]
        roughness: f32,
    },
}

impl Material {
    /// The lowest roughness that the renderer uses. Below it, the bright spot of a
    /// small lamp is smaller than a pixel: use `Mirror`.
    pub const MIN_ROUGHNESS: f32 = 0.05;
}

/// A wall of a room, named by the side of the room that it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Fields)]
pub enum Wall {
    #[field(label = "NegX (west)")]
    NegX,
    #[field(label = "PosX (east)")]
    PosX,
    #[field(label = "NegZ (back)")]
    NegZ,
    #[field(label = "PosZ (front)")]
    PosZ,
}

impl Wall {
    /// The line of the wall on the floor of a room of `width` x `depth` meters with
    /// its center at the origin: `(start, along, length, inward)`. The wall goes from
    /// `start` for `length` meters in the direction `along` (+Z or +X). `inward` points
    /// into the room.
    pub fn frame(self, width: f32, depth: f32) -> (Vec3, Vec3, f32, Vec3) {
        let (x, z) = (width / 2.0, depth / 2.0);
        match self {
            Wall::NegX => (Vec3::new(-x, 0.0, -z), Vec3::Z, depth, Vec3::X),
            Wall::PosX => (Vec3::new(x, 0.0, -z), Vec3::Z, depth, Vec3::NEG_X),
            Wall::NegZ => (Vec3::new(-x, 0.0, -z), Vec3::X, width, Vec3::Z),
            Wall::PosZ => (Vec3::new(-x, 0.0, z), Vec3::X, width, Vec3::NEG_Z),
        }
    }
}

/// An opening in a wall of a room, with a short passage behind it that ends at a wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct Doorway {
    pub wall: Wall,
    /// Where the center of the opening is along the wall: its X coordinate for a Z
    /// wall, its Z coordinate for an X wall.
    #[field(range = -50.0..=50.0, step = 0.05)]
    pub offset: f32,
    #[field(range = 0.1..=100.0, step = 0.05)]
    pub width: f32,
    #[field(range = 0.1..=100.0, step = 0.05)]
    pub height: f32,
    /// Length of the passage behind the wall, in meters.
    #[field(label = "passage depth", range = 0.1..=100.0, step = 0.05)]
    pub depth: f32,
}

impl Doorway {
    /// The opening along a wall of `length` meters, in meters from the start of the
    /// wall: `(from, to)`.
    pub fn span(&self, length: f32) -> (f32, f32) {
        let center = length / 2.0 + self.offset;
        (center - self.width / 2.0, center + self.width / 2.0)
    }
}

/// A triangle mesh with one material: a kind, a color, and its textures. Coordinates
/// are right-handed with +Y up. The front side of a triangle is the side where its
/// corners go counter-clockwise.
///
/// [`Mesh::new`] checks the data, so users (the GPU upload, later the shaders) can
/// trust each mesh: every index is in range and no value is NaN.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    name: String,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
    uvs: Vec<[f32; 2]>,
    base_color: [f32; 4],
    material: Material,
    emissive: [f32; 3],
    base_color_texture: Option<TextureRef>,
    emissive_texture: Option<TextureRef>,
    alpha_cutoff: Option<f32>,
    open_back: bool,
}

impl Mesh {
    /// `normals: None` computes smooth normals from the triangles.
    /// `base_color` is linear RGBA, as the base color factor of a glTF material.
    /// The material is [`Material::Diffuse`]; see [`Mesh::with_material`].
    pub fn new(
        name: impl Into<String>,
        positions: Vec<Vec3>,
        normals: Option<Vec<Vec3>>,
        indices: Vec<u32>,
        base_color: [f32; 4],
    ) -> Result<Self> {
        let name = name.into();
        ensure!(!positions.is_empty(), "mesh `{name}` has no vertex");
        ensure!(
            u32::try_from(positions.len()).is_ok(),
            "mesh `{name}` has more vertices than a u32 index can address"
        );
        ensure!(
            !indices.is_empty() && indices.len().is_multiple_of(3),
            "mesh `{name}` has {} indices: not a list of triangles",
            indices.len()
        );
        ensure!(
            indices
                .iter()
                .all(|index| (*index as usize) < positions.len()),
            "mesh `{name}` has an index past its {} vertices",
            positions.len()
        );
        let normals = normals.unwrap_or_else(|| smooth_normals(&positions, &indices));
        ensure!(
            normals.len() == positions.len(),
            "mesh `{name}` has {} positions but {} normals",
            positions.len(),
            normals.len()
        );
        ensure!(
            positions.iter().chain(&normals).all(|v| v.is_finite()),
            "mesh `{name}` has a position or normal that is not a finite number"
        );
        Ok(Self {
            name,
            uvs: vec![[0.0; 2]; positions.len()],
            positions,
            normals,
            indices,
            base_color,
            material: Material::Diffuse,
            emissive: [0.0; 3],
            base_color_texture: None,
            emissive_texture: None,
            alpha_cutoff: None,
            open_back: false,
        })
    }

    /// The same mesh with texture coordinates: one for each position. (0, 0) is the
    /// top left corner of the texture, (1, 1) the bottom right one; a texture repeats
    /// outside of that range.
    pub fn with_uvs(mut self, uvs: Vec<[f32; 2]>) -> Result<Self> {
        ensure!(
            uvs.len() == self.positions.len(),
            "mesh `{}` has {} positions but {} texture coordinates",
            self.name,
            self.positions.len(),
            uvs.len()
        );
        ensure!(
            uvs.iter().flatten().all(|value| value.is_finite()),
            "mesh `{}` has a texture coordinate that is not a finite number",
            self.name
        );
        self.uvs = uvs;
        Ok(self)
    }

    /// The same mesh with each texture coordinate multiplied by `scale`: 2.0 shows a
    /// texture two times where it showed one time.
    pub fn with_uv_scale(mut self, scale: f32) -> Self {
        for uv in &mut self.uvs {
            *uv = uv.map(|value| value * scale);
        }
        self
    }

    /// The same mesh with a texture for its color. The base color multiplies it.
    pub fn with_base_color_texture(mut self, texture: TextureRef) -> Self {
        self.base_color_texture = Some(texture);
        self
    }

    /// The same mesh with a texture for its own light. The emissive color multiplies it.
    pub fn with_emissive_texture(mut self, texture: TextureRef) -> Self {
        self.emissive_texture = Some(texture);
        self
    }

    /// The same mesh with holes: where the alpha of its color (the base color times
    /// the texture) is below `cutoff`, there is no surface. Light and rays go through.
    pub fn with_alpha_cutoff(mut self, cutoff: f32) -> Self {
        self.alpha_cutoff = Some(cutoff);
        self
    }

    /// The same mesh, and the camera does not see its back side: the room box, which
    /// the camera looks into through its near walls. For light and reflections the
    /// mesh stays solid from the two sides.
    pub fn with_open_back(mut self) -> Self {
        self.open_back = true;
        self
    }

    /// The same mesh with another material.
    pub fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }

    /// The same mesh with a light of its own (linear RGB, as radiance): it shows with
    /// this color, and bounce rays that find it bring its light to the room.
    pub fn with_emissive(mut self, emissive: [f32; 3]) -> Self {
        self.emissive = emissive;
        self
    }

    /// The same mesh with another base color (linear RGBA).
    pub fn with_base_color(mut self, base_color: [f32; 4]) -> Self {
        self.base_color = base_color;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// One unit normal for each position.
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    /// Three indices for each triangle.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// One texture coordinate for each position. All zero for a mesh that has none.
    pub fn uvs(&self) -> &[[f32; 2]] {
        &self.uvs
    }

    pub fn base_color_texture(&self) -> Option<TextureRef> {
        self.base_color_texture
    }

    pub fn emissive_texture(&self) -> Option<TextureRef> {
        self.emissive_texture
    }

    /// `Some`: the mesh has holes where the alpha of its color is below this.
    pub fn alpha_cutoff(&self) -> Option<f32> {
        self.alpha_cutoff
    }

    /// True: the camera does not see the back side. See [`Mesh::with_open_back`].
    pub fn open_back(&self) -> bool {
        self.open_back
    }

    /// How long one meter of the surface is in texture coordinates: the median over
    /// the triangles. The renderer selects the mip level of a texture with it.
    /// 0.0 for a mesh with no texture coordinates.
    pub fn uvs_per_meter(&self) -> f32 {
        let mut ratios: Vec<f32> = self
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|triangle| {
                let [a, b, c] = triangle.map(|index| index as usize);
                let p = &self.positions;
                let area = (p[b] - p[a]).cross(p[c] - p[a]).length();
                let uv = |index: usize| glam::Vec2::from(self.uvs[index]);
                let uv_area = (uv(b) - uv(a)).perp_dot(uv(c) - uv(a)).abs();
                (area > 0.0).then(|| (uv_area / area).sqrt())
            })
            .collect();
        ratios.sort_by(f32::total_cmp);
        ratios.get(ratios.len() / 2).copied().unwrap_or(0.0)
    }

    pub fn base_color(&self) -> [f32; 4] {
        self.base_color
    }

    pub fn material(&self) -> Material {
        self.material
    }

    /// The light that the surface sends out by itself. Zero for most meshes.
    pub fn emissive(&self) -> [f32; 3] {
        self.emissive
    }

    /// An open room: a floor and four walls, no ceiling. The normals point into the room.
    /// The floor is at y = 0 with its center at the origin. `width` is along X, `depth`
    /// along Z, `height` along Y.
    pub fn room(width: f32, depth: f32, height: f32) -> Self {
        Self::room_with_doorways(width, depth, height, &[])
    }

    /// [`Mesh::room`] with openings in its walls. Behind each opening is a passage: a
    /// floor, two sides, a top, and a wall at its end. A doorway that does not fit in
    /// its wall, or that overlaps one before it, is left out with a warning.
    ///
    /// The texture coordinates are meters (see `planar_uv`): a texture that repeats
    /// keeps its size in a room of any size.
    pub fn room_with_doorways(width: f32, depth: f32, height: f32, doorways: &[Doorway]) -> Self {
        let quads = room_quads(width, depth, height, doorways);
        quads.build("room", ROOM_COLOR, |_| true).with_open_back()
    }

    /// The same room as two meshes, so each can have its own texture: the floors (of
    /// the room and of the passages), and all the rest.
    pub fn room_parts(width: f32, depth: f32, height: f32, doorways: &[Doorway]) -> (Self, Self) {
        let quads = room_quads(width, depth, height, doorways);
        let is_floor = |normal: Vec3| normal.y > 0.5;
        let floor = quads.build("room floor", ROOM_COLOR, is_floor);
        let walls = quads.build("room walls", ROOM_COLOR, |normal| !is_floor(normal));
        (floor.with_open_back(), walls.with_open_back())
    }

    /// A cube with its center at the origin. The normals point out.
    pub fn cube(size: f32) -> Self {
        Self::cuboid(Vec3::splat(-size / 2.0), Vec3::splat(size / 2.0))
    }

    /// A box with sides along the axes, from the corner `min` to the corner `max`.
    /// The normals point out. The origin of the mesh can be outside of the box: a door
    /// is a box with the origin on its hinge.
    ///
    /// The corners can be in any order, and a side has a length of 1 mm at least: a
    /// box with no height (a number that a person types in a tool, digit by digit)
    /// is a thin plate, not a mesh with no normals.
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        let (min, max) = (min.min(max), min.max(max));
        let size = (max - min).max(Vec3::splat(1e-3));
        // The texture coordinates start at the corner `min`: a texture that is as
        // large as a side shows one time on it, with no cut in the middle.
        let mut quads = Quads {
            uv_origin: min,
            ..Quads::default()
        };
        // `a = b x c` for each of the three rotations of the axes.
        for (a, b, c) in [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::X, Vec3::Y),
        ] {
            let (a, b, c) = (a * size, b * size, c * size);
            quads.add(min + a, b, c);
            quads.add(min, c, b);
        }
        quads.build("cube", [0.8, 0.3, 0.2, 1.0], |_| true)
    }

    /// A capsule that stands on the origin: a cylinder along Y with a half sphere at
    /// each end. `height` is the full height, not less than two times the radius.
    /// `segments` is the number of quads around the Y axis (3 or more), `rings` the
    /// number in each half sphere (1 or more). Smaller values are raised to the minimum.
    pub fn capsule(radius: f32, height: f32, segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(1));
        // The centers of the two half spheres are this far from the middle.
        let half = (height / 2.0 - radius).max(0.0);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        // The rows of vertices from the top: the upper half sphere, then the lower one.
        // The band between the two is the cylinder.
        let rows = 2 * (rings + 1);
        for row in 0..rows {
            let (ring, center) = if row <= rings {
                (row, half)
            } else {
                (row - 1, -half)
            };
            let (sin_polar, cos_polar) = (PI * ring as f32 / (2 * rings) as f32).sin_cos();
            for segment in 0..=segments {
                let (sin_around, cos_around) = (TAU * segment as f32 / segments as f32).sin_cos();
                let normal = Vec3::new(sin_polar * cos_around, cos_polar, sin_polar * sin_around);
                positions.push(normal * radius + Vec3::Y * (center + half + radius));
                normals.push(normal);
                // The texture goes one time around, from the top to the bottom.
                uvs.push([
                    segment as f32 / segments as f32,
                    row as f32 / (rows - 1) as f32,
                ]);
            }
        }
        for row in 0..rows - 1 {
            // A capsule with no cylinder is a sphere: the band has no area.
            if row == rings && half == 0.0 {
                continue;
            }
            for segment in 0..segments {
                let a = row * (segments + 1) + segment;
                let b = a + segments + 1;
                let (c, d) = (b + 1, a + 1);
                // As for the sphere: one triangle of a quad at a pole has no area.
                if row != rows - 2 {
                    indices.extend([a, c, b]);
                }
                if row != 0 {
                    indices.extend([a, d, c]);
                }
            }
        }
        Self::new(
            "capsule",
            positions,
            Some(normals),
            indices,
            [0.75, 0.7, 0.6, 1.0],
        )
        .and_then(|mesh| mesh.with_uvs(uvs))
        .expect("the capsule generator makes a valid mesh")
    }

    /// A sphere with its center at the origin. The normals point out.
    /// `segments` is the number of quads around the Y axis (3 or more), `rings` the
    /// number from pole to pole (2 or more). Smaller values are raised to the minimum.
    pub fn sphere(radius: f32, segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(2));
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        // The last vertex of each ring is at the position of the first one. The
        // duplicate keeps the grid regular, and it is the seam of the texture
        // coordinates: the texture goes one time around, from pole to pole.
        for ring in 0..=rings {
            let (sin_polar, cos_polar) = (PI * ring as f32 / rings as f32).sin_cos();
            for segment in 0..=segments {
                let (sin_around, cos_around) = (TAU * segment as f32 / segments as f32).sin_cos();
                normals.push(Vec3::new(
                    sin_polar * cos_around,
                    cos_polar,
                    sin_polar * sin_around,
                ));
                uvs.push([segment as f32 / segments as f32, ring as f32 / rings as f32]);
            }
        }
        for ring in 0..rings {
            for segment in 0..segments {
                let a = ring * (segments + 1) + segment;
                let b = a + segments + 1;
                let (c, d) = (b + 1, a + 1);
                // At a pole, two corners of the quad are the same point: one of the
                // two triangles has no area and is left out.
                if ring != rings - 1 {
                    indices.extend([a, c, b]);
                }
                if ring != 0 {
                    indices.extend([a, d, c]);
                }
            }
        }
        let positions = normals.iter().map(|normal| *normal * radius).collect();
        let base_color = [0.2, 0.4, 0.8, 1.0];
        Self::new("sphere", positions, Some(normals), indices, base_color)
            .and_then(|mesh| mesh.with_uvs(uvs))
            .expect("the sphere generator makes a valid mesh")
    }
}

const ROOM_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

/// The quads of a room: see [`Mesh::room_with_doorways`].
fn room_quads(width: f32, depth: f32, height: f32, doorways: &[Doorway]) -> Quads {
    let corner = Vec3::new(-width / 2.0, 0.0, -depth / 2.0);
    let mut quads = Quads::default();
    // The normal of a quad is `u x v`.
    quads.add(corner, Vec3::Z * depth, Vec3::X * width); // floor, faces +Y
    let up = Vec3::Y;
    for wall in [Wall::NegX, Wall::PosX, Wall::NegZ, Wall::PosZ] {
        let (start, along, length, inward) = wall.frame(width, depth);
        let mut doors: Vec<_> = doorways.iter().filter(|door| door.wall == wall).collect();
        doors.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        // The wall is built up to this distance from its start.
        let mut built = 0.0;
        for door in doors {
            let (from, to) = door.span(length);
            // `!(..)` is true also for NaN.
            if !(from >= built && to <= length && door.width > 0.0 && door.depth > 0.0)
                || !(door.height > 0.0 && door.height <= height)
            {
                tracing::warn!(?door, "the doorway does not fit in its wall: left out");
                continue;
            }
            if from > built {
                let piece = along * (from - built);
                quads.add_facing(start + along * built, piece, up * height, inward);
            }
            let (side, top) = (up * door.height, along * door.width);
            let opening = start + along * from;
            if door.height < height {
                quads.add_facing(opening + side, top, up * (height - door.height), inward);
            }
            // The passage goes out of the room.
            let out = -inward * door.depth;
            quads.add_facing(opening, top, out, up);
            quads.add_facing(opening, out, side, along);
            quads.add_facing(opening + top, out, side, -along);
            quads.add_facing(opening + out, top, side, inward);
            quads.add_facing(opening + side, top, out, -up);
            built = to;
        }
        if length > built {
            let piece = along * (length - built);
            quads.add_facing(start + along * built, piece, up * height, inward);
        }
    }
    quads
}

/// The texture coordinate of a point of a flat surface with sides along the axes, in
/// meters. The texture is upright on a wall and not mirrored for a viewer in front of
/// it; on a floor its top is at -Z. Surfaces that touch continue the pattern.
fn planar_uv(position: Vec3, normal: Vec3) -> [f32; 2] {
    if normal.y.abs() > 0.5 {
        [position.x, position.z * normal.y.signum()]
    } else {
        // "Right" for a viewer who looks at the surface.
        let right = Vec3::Y.cross(normal);
        [position.dot(right), -position.y]
    }
}

/// Builds a mesh of flat quads. Each quad has its own four vertices, so edges are sharp.
#[derive(Default)]
struct Quads {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    /// The point with the texture coordinates (0, 0).
    uv_origin: Vec3,
}

impl Quads {
    /// A quad with one corner at `origin` and the sides `u` and `v`. Its front side
    /// and its normal face the direction `u x v`.
    fn add(&mut self, origin: Vec3, u: Vec3, v: Vec3) {
        let corners = [origin, origin + u, origin + u + v, origin + v];
        let normal = u.cross(v).normalize();
        self.positions.extend(corners);
        self.normals.extend([normal; 4]);
        self.uvs
            .extend(corners.map(|corner| planar_uv(corner - self.uv_origin, normal)));
    }

    /// A quad as `add` makes it, with its front side toward `facing`.
    fn add_facing(&mut self, origin: Vec3, u: Vec3, v: Vec3, facing: Vec3) {
        if u.cross(v).dot(facing) >= 0.0 {
            self.add(origin, u, v);
        } else {
            self.add(origin, v, u);
        }
    }

    /// The mesh of the quads whose normal passes `keep`.
    fn build(&self, name: &str, base_color: [f32; 4], keep: impl Fn(Vec3) -> bool) -> Mesh {
        let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
        let mut indices = Vec::new();
        // Each quad has four vertices in sequence.
        for quad in (0..self.positions.len()).step_by(4) {
            if !keep(self.normals[quad]) {
                continue;
            }
            let first = positions.len() as u32;
            positions.extend(&self.positions[quad..quad + 4]);
            normals.extend(&self.normals[quad..quad + 4]);
            uvs.extend(&self.uvs[quad..quad + 4]);
            indices.extend([0, 1, 2, 0, 2, 3].map(|corner| first + corner));
        }
        Mesh::new(name, positions, Some(normals), indices, base_color)
            .and_then(|mesh| mesh.with_uvs(uvs))
            .expect("the quad generator makes a valid mesh")
    }
}

/// A normal for each vertex: the mean of the normals of its triangles, weighted by
/// triangle area. All indices must be in range.
fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|index| index as usize);
        // The length of the cross product is two times the triangle area.
        let weighted = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        for index in [a, b, c] {
            normals[index] += weighted;
        }
    }
    // A vertex with no triangle of nonzero area gets an arbitrary unit normal.
    normals
        .iter()
        .map(|normal| normal.try_normalize().unwrap_or(Vec3::Y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_with_no_height_or_with_its_corners_in_the_other_order_is_a_valid_mesh() {
        let flat = Mesh::cuboid(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 0.0, 0.5));
        assert!(flat.normals().iter().all(|normal| normal.is_finite()));
        let turned = Mesh::cuboid(Vec3::ONE, Vec3::ZERO);
        assert_eq!(
            turned.normals(),
            Mesh::cuboid(Vec3::ZERO, Vec3::ONE).normals()
        );
        assert_eq!(Mesh::cube(0.0).positions().len(), 24);
    }

    /// Each triangle has area, and its front side agrees with its three vertex normals.
    fn assert_winding_agrees_with_normals(mesh: &Mesh) {
        for triangle in mesh.indices().as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| index as usize);
            let p = mesh.positions();
            let face = (p[b] - p[a]).cross(p[c] - p[a]);
            assert!(face.length() > 1e-6, "{}: empty triangle", mesh.name());
            for index in [a, b, c] {
                let normal = mesh.normals()[index];
                assert!((normal.length() - 1.0).abs() < 1e-5);
                assert!(face.dot(normal) > 0.0, "{}: wrong winding", mesh.name());
            }
        }
    }

    #[test]
    fn generated_meshes_have_texture_coordinates() {
        // The room: meters. The floor has (x, z), a wall is upright.
        let room = Mesh::room(8.0, 6.0, 3.0);
        assert!(room.open_back());
        for ((position, normal), uv) in room.positions().iter().zip(room.normals()).zip(room.uvs())
        {
            if normal.y > 0.5 {
                assert_eq!(*uv, [position.x, position.z]);
            } else {
                assert_eq!(uv[1], -position.y);
                assert!(uv[0].abs() == position.x.abs() || uv[0].abs() == position.z.abs());
            }
        }
        assert!((room.uvs_per_meter() - 1.0).abs() < 1e-6);
        assert!((room.with_uv_scale(2.0).uvs_per_meter() - 2.0).abs() < 1e-6);

        // The two parts of a room have the triangles of the room.
        let door = Doorway {
            wall: Wall::NegZ,
            offset: 0.0,
            width: 1.0,
            height: 2.0,
            depth: 1.0,
        };
        let whole = Mesh::room_with_doorways(8.0, 6.0, 3.0, &[door]);
        let (floor, walls) = Mesh::room_parts(8.0, 6.0, 3.0, &[door]);
        assert_eq!(
            floor.indices().len() + walls.indices().len(),
            whole.indices().len()
        );
        // The floor of the room and that of the passage.
        assert_eq!(floor.indices().len(), 12);
        assert!(floor.normals().iter().all(|normal| *normal == Vec3::Y));
        assert!(floor.open_back() && walls.open_back());

        // A cuboid: from 0 at its corner to its size in meters, on each side.
        let cuboid = Mesh::cuboid(Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.5, 4.0, 6.0));
        assert!(!cuboid.open_back());
        let (low, high) =
            cuboid
                .uvs()
                .iter()
                .fold(([f32::MAX; 2], [f32::MIN; 2]), |(low, high), uv| {
                    (
                        [low[0].min(uv[0]), low[1].min(uv[1])],
                        [high[0].max(uv[0]), high[1].max(uv[1])],
                    )
                });
        assert_eq!((low, high), ([-3.0, -3.0], [3.0, 3.0]));
        // A sphere and a capsule: one time around, from the top to the bottom.
        for mesh in [Mesh::sphere(1.0, 8, 4), Mesh::capsule(0.3, 1.7, 8, 2)] {
            let uvs = mesh.uvs();
            assert_eq!((uvs[0], uvs[uvs.len() - 1]), ([0.0, 0.0], [1.0, 1.0]));
            assert!(
                uvs.iter()
                    .flatten()
                    .all(|value| (0.0..=1.0).contains(value))
            );
        }
        assert!(Mesh::cube(1.0).with_uvs(vec![[0.0; 2]; 3]).is_err());
    }

    #[test]
    fn room_is_open_and_faces_inward() {
        let mesh = Mesh::room(8.0, 6.0, 3.0);
        assert_eq!((mesh.positions().len(), mesh.indices().len()), (20, 30));
        assert_winding_agrees_with_normals(&mesh);
        let center = Vec3::new(0.0, 1.5, 0.0);
        for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
            assert!((center - *position).dot(*normal) > 0.0);
            // No ceiling: no surface faces down.
            assert!(normal.y >= 0.0);
            assert!(position.x.abs() <= 4.0 && position.z.abs() <= 3.0);
            assert!((0.0..=3.0).contains(&position.y));
        }
    }

    #[test]
    fn a_doorway_opens_the_wall_and_adds_a_passage() {
        let door = Doorway {
            wall: Wall::NegZ,
            offset: 3.0,
            width: 1.0,
            height: 2.0,
            depth: 0.5,
        };
        let mesh = Mesh::room_with_doorways(8.0, 6.0, 3.0, &[door]);
        // The plain room, with 3 quads in place of one wall, and the 5 quads of the passage.
        assert_eq!(mesh.positions().len(), 20 + 4 * 7);
        assert_winding_agrees_with_normals(&mesh);
        // No wall surface in the opening.
        let in_opening = |p: &Vec3| p.z == -3.0 && p.x > 2.5 && p.x < 3.5 && p.y < 2.0;
        assert!(!mesh.positions().iter().any(in_opening));
        // The passage ends half a meter behind the wall.
        let back = mesh.positions().iter().map(|p| p.z).fold(0.0, f32::min);
        assert_eq!(back, -3.5);

        // A doorway that is wider than its wall is left out.
        let wide = Doorway { width: 9.0, ..door };
        assert_eq!(
            Mesh::room_with_doorways(8.0, 6.0, 3.0, &[wide]),
            Mesh::room(8.0, 6.0, 3.0)
        );
    }

    #[test]
    fn a_capsule_stands_on_the_origin() {
        let capsule = Mesh::capsule(0.3, 1.7, 8, 2);
        assert_winding_agrees_with_normals(&capsule);
        let (low, high) = capsule
            .positions()
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), p| {
                (low.min(p.y), high.max(p.y))
            });
        assert!(low.abs() < 1e-6 && (high - 1.7).abs() < 1e-6);
        assert!(
            capsule
                .positions()
                .iter()
                .all(|p| p.with_y(0.0).length() < 0.3 + 1e-6)
        );
        // No cylinder: a sphere.
        assert_winding_agrees_with_normals(&Mesh::capsule(0.5, 1.0, 8, 2));
    }

    #[test]
    fn a_cuboid_goes_from_corner_to_corner() {
        let door = Mesh::cuboid(Vec3::new(0.0, 0.0, -0.03), Vec3::new(1.0, 2.0, 0.03));
        assert_winding_agrees_with_normals(&door);
        let fold =
            |f: fn(Vec3, Vec3) -> Vec3, start| door.positions().iter().copied().fold(start, f);
        assert_eq!(fold(Vec3::min, Vec3::MAX), Vec3::new(0.0, 0.0, -0.03));
        assert_eq!(fold(Vec3::max, Vec3::MIN), Vec3::new(1.0, 2.0, 0.03));
    }

    #[test]
    fn cube_and_sphere_face_outward() {
        let cube = Mesh::cube(2.0);
        assert_eq!((cube.positions().len(), cube.indices().len()), (24, 36));
        let sphere = Mesh::sphere(0.5, 8, 4);
        // 5 rings of 9 vertices. 8 quads in each of 4 bands, less one triangle at each pole.
        assert_eq!(
            (sphere.positions().len(), sphere.indices().len()),
            (45, 144)
        );
        for mesh in [&cube, &sphere] {
            assert_winding_agrees_with_normals(mesh);
            for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
                assert!(position.dot(*normal) > 0.0);
            }
        }
        assert!(cube.positions().iter().all(|p| p.abs() == Vec3::ONE));
        assert!(
            sphere
                .positions()
                .iter()
                .all(|p| (p.length() - 0.5).abs() < 1e-6)
        );
    }

    #[test]
    fn missing_normals_are_generated() {
        let cube = Mesh::cube(1.0);
        let mesh = Mesh::new(
            "no normals",
            cube.positions().to_vec(),
            None,
            cube.indices().to_vec(),
            [1.0; 4],
        )
        .unwrap();
        // The cube has four vertices for each side, so smooth normals are the side normals.
        for (generated, expected) in mesh.normals().iter().zip(cube.normals()) {
            assert!(generated.distance(*expected) < 1e-6);
        }
    }

    #[test]
    fn bad_mesh_data_is_rejected() {
        let triangle = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let new = |positions: &[Vec3], normals: Option<Vec<Vec3>>, indices: &[u32]| {
            Mesh::new(
                "bad",
                positions.to_vec(),
                normals,
                indices.to_vec(),
                [1.0; 4],
            )
        };
        assert!(new(&triangle, None, &[0, 1, 2]).is_ok());
        assert!(new(&[], None, &[]).is_err());
        assert!(new(&triangle, None, &[0, 1]).is_err());
        assert!(new(&triangle, None, &[0, 1, 3]).is_err());
        assert!(new(&triangle, Some(vec![Vec3::Z]), &[0, 1, 2]).is_err());
        assert!(new(&[Vec3::NAN, Vec3::X, Vec3::Y], None, &[0, 1, 2]).is_err());
    }
}
