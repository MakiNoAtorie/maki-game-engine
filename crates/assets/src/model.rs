//! The model asset and its glTF loader.

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use glam::{Mat3, Mat4, Vec3};
use gltf::material::AlphaMode;
use gltf::mesh::Mode;
use gltf::texture::MagFilter;

use crate::{Assets, Handle, Material, Mesh, Texture, TextureRef, Textures};

/// The content of one model file: a mesh for each primitive of each mesh node, and
/// the point lights. All are in the space of the model, so one transform places the model.
pub struct Model {
    pub meshes: Vec<Handle<Mesh>>,
    pub lights: Vec<ModelLight>,
}

/// A point light of a model file (glTF `KHR_lights_punctual`, type `point`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLight {
    /// In the space of the model.
    pub position: Vec3,
    /// Linear RGB.
    pub color: Vec3,
    /// In candela: a surface that faces the light at `d` meters gets `intensity / d²` lux.
    pub intensity: f32,
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
    textures: &mut Textures,
) -> Handle<Model> {
    let handle = models.reserve();
    let model = match read_gltf(path, textures) {
        Ok((loaded, lights)) => Some(Model {
            meshes: loaded.into_iter().map(|mesh| meshes.add(mesh)).collect(),
            lights,
        }),
        Err(error) => {
            tracing::error!("cannot load model {}: {error:#}", path.display());
            None
        }
    };
    models.finish(handle, model);
    handle
}

/// What `read_primitive` needs to find the textures of a material.
struct Images<'a> {
    path: &'a Path,
    buffers: &'a [gltf::buffer::Data],
    textures: &'a mut Textures,
}

impl Images<'_> {
    /// The texture of a material slot. `None`, with a warning, if the mesh cannot
    /// show it.
    fn texture(&mut self, info: Option<gltf::texture::Info>, has_uvs: bool) -> Option<TextureRef> {
        let info = info?;
        if info.tex_coord() != 0 || !has_uvs {
            // ponytail: only the first set of texture coordinates is read.
            tracing::warn!(
                "{}: a texture uses texture coordinates that the mesh does not have in set 0: not shown",
                self.path.display()
            );
            return None;
        }
        let texture = info.texture();
        let image = texture.source();
        let base = self.path.parent();
        let handle = match image.source() {
            // An image file of its own: a second model that names it gets the same texture.
            gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
                self.textures.load(&base.unwrap_or(Path::new("")).join(uri))
            }
            // In the model file.
            source => {
                let (path, buffers) = (self.path, self.buffers);
                self.textures
                    .load_with((path.to_owned(), Some(image.index())), || {
                        let data = gltf::image::Data::from_source(source, base, buffers)?;
                        let name = format!("{}#{}", path.display(), image.index());
                        Texture::new(name, data.width, data.height, rgba8(&data)?)
                    })
            }
        };
        Some(TextureRef {
            texture: handle,
            // The sampler of the file: its filter for a texture that shows large.
            nearest: texture.sampler().mag_filter() == Some(MagFilter::Nearest),
        })
    }
}

/// The pixels of a decoded image as four bytes each.
fn rgba8(image: &gltf::image::Data) -> Result<Vec<u8>> {
    use gltf::image::Format;
    let pixels = &image.pixels;
    Ok(match image.format {
        Format::R8G8B8A8 => pixels.clone(),
        Format::R8G8B8 => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|[r, g, b]| [*r, *g, *b, 255])
            .collect(),
        Format::R8 => pixels.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        Format::R8G8 => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|[v, a]| [*v, *v, *v, *a])
            .collect(),
        other => bail!("the image format {other:?} is not read (8 bits for each channel only)"),
    })
}

/// Reads all triangle primitives and all point lights of the default scene.
fn read_gltf(path: &Path, textures: &mut Textures) -> Result<(Vec<Mesh>, Vec<ModelLight>)> {
    let gltf::Gltf { document, blob } = gltf::Gltf::open(path)?;
    // Not `gltf::import`: that decodes all images of the file. `Images` decodes those
    // that a material uses and that the texture store does not have.
    let buffers = gltf::import_buffers(&document, path.parent(), blob)?;
    if document
        .extensions_used()
        .any(|name| name == "KHR_texture_transform")
    {
        // ponytail: no texture transform. Read `KHR_texture_transform` into the
        // texture coordinates when a file from a modelling tool needs it.
        tracing::warn!(
            "{}: the file uses texture transforms, which are not read",
            path.display()
        );
    }
    let mut images = Images {
        path,
        buffers: &buffers,
        textures,
    };
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .context("the file has no scene")?;

    let mut meshes = Vec::new();
    let mut lights = Vec::new();
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
                meshes.push(read_primitive(&primitive, &mut images, to_model, name)?);
            }
        }
        // Spot and directional lights are skipped: the renderer has sphere lights only.
        if let Some(light) = node.light()
            && matches!(light.kind(), gltf::khr_lights_punctual::Kind::Point)
        {
            lights.push(ModelLight {
                position: to_model.transform_point3(Vec3::ZERO),
                color: light.color().into(),
                intensity: light.intensity(),
            });
        }
        stack.extend(node.children().map(|child| (child, to_model)));
    }
    ensure!(!meshes.is_empty(), "the file has no triangle mesh");
    Ok((meshes, lights))
}

/// Makes one mesh, with the node transform applied to its vertices.
// ponytail: a mesh that many nodes use is stored one time for each node. Keep the
// node transform apart (one mesh, many instances) when a file makes this expensive.
fn read_primitive(
    primitive: &gltf::Primitive,
    images: &mut Images,
    to_model: Mat4,
    name: String,
) -> Result<Mesh> {
    let buffers = images.buffers;
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
    let uvs: Option<Vec<[f32; 2]>> = reader
        .read_tex_coords(0)
        .map(|uvs| uvs.into_f32().collect());
    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let mut mesh = Mesh::new(name, positions, normals, indices, pbr.base_color_factor())?
        .with_material(material_kind(&material))
        .with_emissive(material.emissive_factor());
    let has_uvs = uvs.is_some();
    if let Some(uvs) = uvs {
        mesh = mesh.with_uvs(uvs)?;
    }
    if let Some(texture) = images.texture(pbr.base_color_texture(), has_uvs) {
        mesh = mesh.with_base_color_texture(texture);
    }
    if let Some(texture) = images.texture(material.emissive_texture(), has_uvs) {
        mesh = mesh.with_emissive_texture(texture);
    }
    // ponytail: `BLEND` (a surface that is partly clear) shows as `MASK` with the
    // cutoff 0.5: each point is there or not. Enough for leaves and fences. Glass
    // has its own material kind.
    match material.alpha_mode() {
        AlphaMode::Opaque => {}
        AlphaMode::Mask => mesh = mesh.with_alpha_cutoff(material.alpha_cutoff().unwrap_or(0.5)),
        AlphaMode::Blend => mesh = mesh.with_alpha_cutoff(0.5),
    }
    Ok(mesh)
}

/// A roughness factor from this value on is a diffuse surface.
const FULLY_ROUGH: f32 = 0.9;

/// The index of refraction of glass in a file that gives none (the glTF default).
const DEFAULT_IOR: f32 = 1.5;

/// The renderer has three kinds of surface. glTF describes a surface with numbers from
/// 0 to 1, so the numbers select the nearest kind:
/// - more than half of the light goes through: glass;
/// - metal with a smooth surface: mirror;
/// - all other surfaces: diffuse. This includes the glTF default (a rough metal).
// ponytail: a surface that is partly metal or partly clear shows as one of the kinds.
// Add a metal amount and a transmission amount to `Material` when the shader has them.
fn material_kind(material: &gltf::Material) -> Material {
    let pbr = material.pbr_metallic_roughness();
    let transmission = material
        .transmission()
        .map_or(0.0, |transmission| transmission.transmission_factor());
    if transmission > 0.5 {
        // The file is user input. The shader divides by the index, and an index
        // below 1 is not a real material.
        let ior = material.ior().filter(|ior| ior.is_finite() && *ior >= 1.0);
        Material::Glass {
            ior: ior.unwrap_or(DEFAULT_IOR),
        }
    } else if pbr.roughness_factor() >= FULLY_ROUGH {
        // No reflection that shows. This is also the glTF default (a rough metal),
        // which most files with no material mean as "plain".
        Material::Diffuse
    } else if pbr.metallic_factor() >= 0.5 {
        if pbr.roughness_factor() <= 0.1 {
            Material::Mirror
        } else {
            Material::Metal {
                roughness: pbr.roughness_factor(),
            }
        }
    } else {
        Material::Plastic {
            roughness: pbr.roughness_factor().max(Material::MIN_ROUGHNESS),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::LoadState;

    /// Writes `mesh` as a `.gltf` file with a `.bin` file next to it, and returns the
    /// path of the `.gltf` file. The mesh is on one node with a `translation`.
    /// `surface` is JSON text that is added to the material: fields of
    /// `pbrMetallicRoughness`, then fields of the material. Each text starts with a
    /// comma, or is empty.
    fn write_gltf(
        name: &str,
        mesh: &Mesh,
        with_normals: bool,
        translation: Vec3,
        surface: (&str, &str),
    ) -> PathBuf {
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
"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":{:?}{}}}{}}}],
"extensionsUsed":["KHR_materials_transmission","KHR_materials_ior"],
"buffers":[{{"uri":"{name}.bin","byteLength":{}}}],
"bufferViews":[{}],
"accessors":[{}]}}"#,
            translation.to_array(),
            accessors.len() - 1,
            mesh.base_color(),
            surface.0,
            surface.1,
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
                ("", ""),
            );
            let model = load_gltf(&path, &mut models, &mut meshes, &mut Textures::default());
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
    fn gltf_material_numbers_select_the_material_kind() {
        const SMOOTH_METAL: &str = r#","metallicFactor":1.0,"roughnessFactor":0.0"#;
        let clear = |ior: &str| {
            format!(
                r#","extensions":{{"KHR_materials_transmission":{{"transmissionFactor":1.0}}{ior}}}"#
            )
        };
        let cases = [
            // The glTF default is a rough metal.
            ("default", "", String::new(), Material::Diffuse),
            ("mirror", SMOOTH_METAL, String::new(), Material::Mirror),
            (
                "rough_metal",
                r#","metallicFactor":1.0,"roughnessFactor":0.6"#,
                String::new(),
                Material::Metal { roughness: 0.6 },
            ),
            (
                "smooth_plastic",
                r#","metallicFactor":0.0,"roughnessFactor":0.0"#,
                String::new(),
                Material::Plastic { roughness: 0.05 },
            ),
            (
                "plaster",
                r#","metallicFactor":0.0,"roughnessFactor":0.95"#,
                String::new(),
                Material::Diffuse,
            ),
            (
                "water",
                "",
                clear(r#","KHR_materials_ior":{"ior":1.33}"#),
                Material::Glass { ior: 1.33 },
            ),
            // No index in the file: the glTF default.
            ("glass", "", clear(""), Material::Glass { ior: 1.5 }),
            // Glass wins over metal: a clear surface has no metal look.
            (
                "clear_metal",
                SMOOTH_METAL,
                clear(""),
                Material::Glass { ior: 1.5 },
            ),
            // An index that the shader cannot use.
            (
                "bad_ior",
                "",
                clear(r#","KHR_materials_ior":{"ior":0.0}"#),
                Material::Glass { ior: 1.5 },
            ),
            (
                "mostly_opaque",
                "",
                r#","extensions":{"KHR_materials_transmission":{"transmissionFactor":0.2}}"#
                    .to_owned(),
                Material::Diffuse,
            ),
        ];
        let source = Mesh::cube(1.0);
        let mut models = Assets::default();
        let mut meshes = Assets::default();
        for (name, pbr, material, expected) in cases {
            let path = write_gltf(name, &source, true, Vec3::ZERO, (pbr, &material));
            let model = load_gltf(&path, &mut models, &mut meshes, &mut Textures::default());
            fs::remove_file(&path).unwrap();
            fs::remove_file(path.with_extension("bin")).unwrap();
            let model = models.get(model).expect("the model is ready");
            let mesh = meshes.get(model.meshes[0]).unwrap();
            assert_eq!(mesh.material(), expected, "{name}");
        }
    }

    #[test]
    fn missing_file_gives_a_failed_handle() {
        let mut models = Assets::default();
        let mut meshes = Assets::default();
        let textures = &mut Textures::default();
        let model = load_gltf(
            Path::new("no such file.gltf"),
            &mut models,
            &mut meshes,
            textures,
        );
        assert_eq!(models.state(model), Some(LoadState::Failed));
        assert_eq!(meshes.iter().count(), 0);
    }

    /// Writes a model of one triangle with texture coordinates and a material with
    /// textures: `material` is the JSON text of the material. The image is the file
    /// `image` in the same directory.
    fn write_textured_gltf(name: &str, image: &str, material: &str) -> PathBuf {
        let floats: [f32; 15] = [
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // positions
            0.0, 1.0, 2.0, 1.0, 0.0, -1.0, // texture coordinates
        ];
        let bin: Vec<u8> = floats.iter().flat_map(|v| v.to_le_bytes()).collect();
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"material":0}}]}}],
"materials":[{material}],
"textures":[{{"source":0,"sampler":0}},{{"source":0}}],
"samplers":[{{"magFilter":9728}}],
"images":[{{"uri":"{image}"}}],
"buffers":[{{"uri":"{name}.bin","byteLength":60}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}}]}}"#
        );
        let dir = std::env::temp_dir().join(format!("maki-assets-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.bin")), bin).unwrap();
        let path = dir.join(format!("{name}.gltf"));
        fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn gltf_textures_alpha_mode_and_texture_coordinates_are_read() {
        let dir = std::env::temp_dir().join(format!("maki-assets-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // An image of 2 x 1 pixels, without alpha.
        image::RgbImage::from_raw(2, 1, vec![255, 0, 0, 0, 0, 255])
            .unwrap()
            .save(dir.join("shared.png"))
            .unwrap();
        let (mut models, mut meshes) = (Assets::default(), Assets::default());
        let mut textures = Textures::default();
        let mut load = |name: &str, material: &str, textures: &mut Textures| {
            let path = write_textured_gltf(name, "shared.png", material);
            let model = load_gltf(&path, &mut models, &mut meshes, textures);
            let mesh = models.get(model).expect("the model is ready").meshes[0];
            meshes.get(mesh).unwrap().clone()
        };

        // Texture 0 has the sampler with the nearest filter, texture 1 has none.
        let cutout = load(
            "cutout",
            r#"{"alphaMode":"MASK","alphaCutoff":0.3,"emissiveTexture":{"index":1},
                "pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}"#,
            &mut textures,
        );
        assert_eq!(cutout.uvs(), [[0.0, 1.0], [2.0, 1.0], [0.0, -1.0]]);
        assert_eq!(cutout.alpha_cutoff(), Some(0.3));
        let (base, emissive) = (
            cutout.base_color_texture().unwrap(),
            cutout.emissive_texture().unwrap(),
        );
        assert!(base.nearest && !emissive.nearest);
        // The two textures are one image: one texture asset.
        assert_eq!(base.texture, emissive.texture);
        // The triangle has half a square meter and 2 square units of texture.
        assert_eq!(cutout.uvs_per_meter(), 2.0);

        // A second model with the same image file gets the same texture.
        let plain = load(
            "plain",
            r#"{"alphaMode":"BLEND","pbrMetallicRoughness":{"baseColorTexture":{"index":1}}}"#,
            &mut textures,
        );
        assert_eq!(plain.base_color_texture().unwrap().texture, base.texture);
        assert_eq!(plain.emissive_texture(), None);
        // `BLEND` shows as a cutout at one half.
        assert_eq!(plain.alpha_cutoff(), Some(0.5));
        let texture = textures.get(base.texture).expect("the image is decoded");
        assert_eq!((texture.width, texture.height), (2, 1));
        assert_eq!(texture.pixels, [255, 0, 0, 255, 0, 0, 255, 255]);
        assert_eq!(textures.iter().count(), 1);

        // A texture on set 1 of the texture coordinates is not shown.
        let other_set = load(
            "set1",
            r#"{"pbrMetallicRoughness":{"baseColorTexture":{"index":0,"texCoord":1}}}"#,
            &mut textures,
        );
        assert_eq!(other_set.base_color_texture(), None);
        assert_eq!(other_set.alpha_cutoff(), None);
    }

    #[test]
    fn a_texture_that_no_mesh_uses_is_removed() {
        let dir = std::env::temp_dir().join(format!("maki-assets-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("kept.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255]))
            .save(&file)
            .unwrap();
        let mut textures = Textures::default();
        let mut meshes = Assets::default();
        let kept = textures.load(&file);
        // The same file again: the same texture.
        assert_eq!(textures.load(&file), kept);
        let missing = textures.load(&dir.join("no such file.png"));
        assert!(textures.get(kept).is_some() && textures.get(missing).is_none());
        let made = textures.add(Texture::new("made", 1, 1, vec![0; 4]).unwrap());

        let mesh = meshes.add(Mesh::cube(1.0).with_base_color_texture(TextureRef {
            texture: kept,
            nearest: false,
        }));
        // The failed load and the texture with no user go.
        assert_eq!(textures.remove_unused(&meshes), 2);
        assert!(textures.get(kept).is_some() && textures.get(made).is_none());
        assert_eq!(textures.remove_unused(&meshes), 0);
        // Without its mesh, the last one goes too, and a new load reads the file again.
        meshes.remove(mesh);
        assert_eq!(textures.remove_unused(&meshes), 1);
        assert_ne!(textures.load(&file), kept);
        assert!(Texture::new("short", 2, 2, vec![0; 15]).is_err());
    }
}
