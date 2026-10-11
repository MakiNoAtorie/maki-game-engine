//! The mesh asset and the generators of simple meshes.

use std::f32::consts::{PI, TAU};

use anyhow::{Result, ensure};
use glam::Vec3;

/// How a surface sends light back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Material {
    /// Sends light back equally in all directions, with the base color (paper, plaster).
    Diffuse,
    /// A perfect mirror. The base color tints the reflection.
    Mirror,
    /// Clear glass: a part of the light is reflected, the rest goes through and bends.
    /// The base color tints the light that goes through.
    Glass {
        /// Index of refraction: how much the light bends. 1.5 for window glass, 1.33 for water.
        ior: f32,
    },
}

/// A triangle mesh with one color and one material. Coordinates are right-handed with +Y up.
/// The front side of a triangle is the side where its corners go counter-clockwise.
///
/// [`Mesh::new`] checks the data, so users (the GPU upload, later the shaders) can
/// trust each mesh: every index is in range and no value is NaN.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    name: String,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
    base_color: [f32; 4],
    material: Material,
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
            positions,
            normals,
            indices,
            base_color,
            material: Material::Diffuse,
        })
    }

    /// The same mesh with another material.
    pub fn with_material(mut self, material: Material) -> Self {
        self.material = material;
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

    pub fn base_color(&self) -> [f32; 4] {
        self.base_color
    }

    pub fn material(&self) -> Material {
        self.material
    }

    /// An open room: a floor and four walls, no ceiling. The normals point into the room.
    /// The floor is at y = 0 with its center at the origin. `width` is along X, `depth`
    /// along Z, `height` along Y.
    pub fn room(width: f32, depth: f32, height: f32) -> Self {
        let corner = Vec3::new(-width / 2.0, 0.0, -depth / 2.0);
        let (x, y, z) = (Vec3::X * width, Vec3::Y * height, Vec3::Z * depth);
        let mut quads = Quads::default();
        // The normal of a quad is `u x v`: each pair below gives the inward direction.
        quads.add(corner, z, x); // floor, faces +Y
        quads.add(corner, y, z); // wall at -X, faces +X
        quads.add(corner + x, z, y); // wall at +X, faces -X
        quads.add(corner, x, y); // wall at -Z, faces +Z
        quads.add(corner + z, y, x); // wall at +Z, faces -Z
        quads.build("room", [0.8, 0.8, 0.8, 1.0])
    }

    /// A cube with its center at the origin. The normals point out.
    pub fn cube(size: f32) -> Self {
        let half = size / 2.0;
        let mut quads = Quads::default();
        // `a = b x c` for each of the three rotations of the axes.
        for (a, b, c) in [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::X, Vec3::Y),
        ] {
            quads.add((a - b - c) * half, b * size, c * size);
            quads.add((-a - b - c) * half, c * size, b * size);
        }
        quads.build("cube", [0.8, 0.3, 0.2, 1.0])
    }

    /// A sphere with its center at the origin. The normals point out.
    /// `segments` is the number of quads around the Y axis (3 or more), `rings` the
    /// number from pole to pole (2 or more). Smaller values are raised to the minimum.
    pub fn sphere(radius: f32, segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(2));
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        // The last vertex of each ring is at the position of the first one. The
        // duplicate keeps the grid regular (and gives texture coordinates a seam later).
        for ring in 0..=rings {
            let (sin_polar, cos_polar) = (PI * ring as f32 / rings as f32).sin_cos();
            for segment in 0..=segments {
                let (sin_around, cos_around) = (TAU * segment as f32 / segments as f32).sin_cos();
                normals.push(Vec3::new(
                    sin_polar * cos_around,
                    cos_polar,
                    sin_polar * sin_around,
                ));
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
            .expect("the sphere generator makes a valid mesh")
    }
}

/// Builds a mesh of flat quads. Each quad has its own four vertices, so edges are sharp.
#[derive(Default)]
struct Quads {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl Quads {
    /// A quad with one corner at `origin` and the sides `u` and `v`. Its front side
    /// and its normal face the direction `u x v`.
    fn add(&mut self, origin: Vec3, u: Vec3, v: Vec3) {
        let first = self.positions.len() as u32;
        self.positions
            .extend([origin, origin + u, origin + u + v, origin + v]);
        self.normals.extend([u.cross(v).normalize(); 4]);
        self.indices
            .extend([0, 1, 2, 0, 2, 3].map(|corner| first + corner));
    }

    fn build(self, name: &str, base_color: [f32; 4]) -> Mesh {
        let Self {
            positions,
            normals,
            indices,
        } = self;
        Mesh::new(name, positions, Some(normals), indices, base_color)
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
