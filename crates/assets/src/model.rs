//! The model asset and its glTF loader.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use glam::{Mat3, Mat4, Vec3};
use gltf::mesh::Mode;

use crate::{Assets, Handle, Mesh};

/// The content of one model file: a mesh for each primitive of each mesh node.
/// All meshes are in the space of the model, so one transform places the model.
pub struct Model {
    pub meshes: Vec<Handle<Mesh>>,
}

/// Starts to load a glTF file (`.gltf` or `.glb`) and returns its handle at once.
/// The model is not always ready on return: see [`Assets::state`]. A failure is logged.
// ponytail: the file is read on the calling thread before this function returns.
// For async loading, move `read_gltf` to a worker thread and call `finish` from a
// system when the worker is done. For a cooked format, replace `read_gltf`.
// The signature and the callers stay the same in both cases.
pub fn load_gltf(
    path: &Path,
    models: &mut Assets<Model>,
    meshes: &mut Assets<Mesh>,
) -> Handle<Model> {
    let handle = models.reserve();
    let model = match read_gltf(path) {
        Ok(loaded) => Some(Model {
            meshes: loaded.into_iter().map(|mesh| meshes.add(mesh)).collect(),
        }),
        Err(error) => {
            tracing::error!("cannot load model {}: {error:#}", path.display());
            None
        }
    };
    models.finish(handle, model);
    handle
}

/// Reads all triangle primitives of the default scene.
fn read_gltf(path: &Path) -> Result<Vec<Mesh>> {
    let gltf::Gltf { document, blob } = gltf::Gltf::open(path)?;
    // Not `gltf::import`: that also decodes all images, and textures are not used yet.
    let buffers = gltf::import_buffers(&document, path.parent(), blob)?;
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .context("the file has no scene")?;

    let mut meshes = Vec::new();
    // Nodes to visit, each with the transform of its parent.
    let mut stack: Vec<_> = scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    let mut visited = 0;
    while let Some((node, parent)) = stack.pop() {
        // In a valid file each node has one parent. Without this check, a file where
        // a node is its own descendant would make the loop endless.
        visited += 1;
        ensure!(
            visited <= document.nodes().len(),
            "the node hierarchy is not a tree"
        );
        let to_model = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let name = format!("{}/{}", mesh.name().unwrap_or("mesh"), primitive.index());
                if primitive.mode() != Mode::Triangles {
                    tracing::warn!(
                        "{}: `{name}` is not a triangle list, skipped",
                        path.display()
                    );
                    continue;
                }
                meshes.push(read_primitive(&primitive, &buffers, to_model, name)?);
            }
        }
        stack.extend(node.children().map(|child| (child, to_model)));
    }
    ensure!(!meshes.is_empty(), "the file has no triangle mesh");
    Ok(meshes)
}

/// Makes one mesh, with the node transform applied to its vertices.
// ponytail: a mesh that many nodes use is stored one time for each node. Keep the
// node transform apart (one mesh, many instances) when a file makes this expensive.
fn read_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    to_model: Mat4,
    name: String,
) -> Result<Mesh> {
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
    let positions: Vec<Vec3> = reader
        .read_positions()
        .with_context(|| format!("mesh `{name}` has no positions"))?
        .map(|position| to_model.transform_point3(position.into()))
        .collect();
    let mut indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        // No index list: each three vertices in sequence are a triangle.
        None => (0..positions.len() as u32).collect(),
    };
    // A transform that mirrors turns each triangle around: turn it back.
    if to_model.determinant() < 0.0 {
        for triangle in indices.as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
    }
    // Normals need the inverse transpose, or a scale that is not uniform bends them.
    let normal_matrix = Mat3::from_mat4(to_model).inverse().transpose();
    let normals = reader.read_normals().map(|normals| {
        normals
            .map(|normal| (normal_matrix * Vec3::from(normal)).normalize_or_zero())
            .collect()
    });
    let base_color = primitive
        .material()
        .pbr_metallic_roughness()
        .base_color_factor();
    Mesh::new(name, positions, normals, indices, base_color)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::LoadState;

    /// Writes `mesh` as a `.gltf` file with a `.bin` file next to it, and returns the
    /// path of the `.gltf` file. The mesh is on one node with a `translation`.
    fn write_gltf(name: &str, mesh: &Mesh, with_normals: bool, translation: Vec3) -> PathBuf {
        let floats = |values: &[Vec3]| -> Vec<u8> {
            values
                .iter()
                .flat_map(|v| v.to_array())
                .flat_map(f32::to_le_bytes)
                .collect()
        };
        let vertex_count = mesh.positions().len();
        let (min, max) = mesh
            .positions()
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(min, max), p| {
                (min.min(*p), max.max(*p))
            });

        // One buffer view and one accessor for each block of data.
        let mut bin = Vec::new();
        let mut views = Vec::new();
        let mut accessors = Vec::new();
        let mut add = |bytes: Vec<u8>, accessor: String| {
            views.push(format!(
                r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#,
                bin.len(),
                bytes.len()
            ));
            accessors.push(format!(
                r#"{{"bufferView":{},{accessor}}}"#,
                views.len() - 1
            ));
            bin.extend(bytes);
        };
        add(
            floats(mesh.positions()),
            format!(
                r#""componentType":5126,"count":{vertex_count},"type":"VEC3","min":{:?},"max":{:?}"#,
                min.to_array(),
                max.to_array()
            ),
        );
        let mut attributes = r#""POSITION":0"#.to_owned();
        if with_normals {
            add(
                floats(mesh.normals()),
                format!(r#""componentType":5126,"count":{vertex_count},"type":"VEC3""#),
            );
            attributes += r#","NORMAL":1"#;
        }
        add(
            mesh.indices()
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect(),
            format!(
                r#""componentType":5125,"count":{},"type":"SCALAR""#,
                mesh.indices().len()
            ),
        );

        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"mesh":0,"translation":{:?}}}],
"meshes":[{{"name":"fixture","primitives":[{{"attributes":{{{attributes}}},"indices":{},"material":0}}]}}],
"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":{:?}}}}}],
"buffers":[{{"uri":"{name}.bin","byteLength":{}}}],
"bufferViews":[{}],
"accessors":[{}]}}"#,
            translation.to_array(),
            accessors.len() - 1,
            mesh.base_color(),
            bin.len(),
            views.join(","),
            accessors.join(","),
        );

        let dir = std::env::temp_dir().join(format!("maki-assets-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.bin")), bin).unwrap();
        let path = dir.join(format!("{name}.gltf"));
        fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn gltf_file_becomes_meshes_in_model_space() {
        let source = Mesh::cube(1.0);
        let offset = Vec3::new(1.0, 2.0, 3.0);
        let mut models = Assets::default();
        let mut meshes = Assets::default();

        for with_normals in [true, false] {
            let path = write_gltf(
                &format!("cube{with_normals}"),
                &source,
                with_normals,
                offset,
            );
            let model = load_gltf(&path, &mut models, &mut meshes);
            fs::remove_file(&path).unwrap();
            fs::remove_file(path.with_extension("bin")).unwrap();
            let model = models.get(model).expect("the model is ready");
            assert_eq!(model.meshes.len(), 1);
            let mesh = meshes.get(model.meshes[0]).unwrap();

            assert_eq!(mesh.name(), "fixture/0");
            assert_eq!(mesh.indices(), source.indices());
            assert_eq!(mesh.base_color(), source.base_color());
            // The node translation moves the positions and not the normals.
            for (loaded, original) in mesh.positions().iter().zip(source.positions()) {
                assert!(loaded.distance(*original + offset) < 1e-6);
            }
            // Without normals in the file, the generated ones are the same for a cube.
            for (loaded, original) in mesh.normals().iter().zip(source.normals()) {
                assert!(loaded.distance(*original) < 1e-6);
            }
        }
    }

    #[test]
    fn missing_file_gives_a_failed_handle() {
        let mut models = Assets::default();
        let mut meshes = Assets::default();
        let model = load_gltf(Path::new("no such file.gltf"), &mut models, &mut meshes);
        assert_eq!(models.state(model), Some(LoadState::Failed));
        assert_eq!(meshes.iter().count(), 0);
    }
}
