//! Meshes on the GPU, and the list of mesh instances to draw.
//!
//! The meshes are uploaded in groups. A group is the meshes of one model, or one mesh
//! that is in no model. Each group gets:
//! - one BLAS (the acceleration structure that rays are traced against) with one
//!   geometry for each mesh. One BLAS for a model and not one for each mesh: rays are
//!   faster against one structure than against many that overlap.
//! - one normal buffer: the normals of the three corners of each triangle, mesh after mesh.
//! - one buffer of texture coordinates, with the same order, for the meshes of the
//!   group that have a texture. A group with no texture has none.
//!
//! One more buffer, the mesh table, has an entry for each mesh with its material, its
//! textures (the `textures` module), and the addresses of its normals and texture
//! coordinates. The meshes of a group have table indices in sequence, in
//! the order of the BLAS geometries: a hit gives the first index (from the TLAS
//! instance) and the geometry number, and the sum is the table index of the mesh.
//! `shaders/scene.glsl` has the same layouts on the shader side.
//!
//! The positions and indices go to the GPU only for the BLAS build: the BLAS keeps its
//! own copy, and no shader reads them.
//!
//! Data flow, all in the `Extract` schedule:
//! - `extract_meshes` (here) compares the mesh store with [`GpuMeshes`]: it uploads
//!   new meshes and frees the buffers of removed ones.
//! - `extract_instances` (here) copies each entity with a [`MeshInstance`] into
//!   [`ExtractedInstances`]: which mesh is where, and where it was one frame before.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Query, Res, ResMut};
use bevy_ecs::world::World;
use glam::{Mat4, Vec3};
use maki_assets::{Assets, Handle, Material, Mesh, Model, Textures};
use maki_core::{App, Extract, Render, Startup, Transform};
use maki_gpu::{BlasDesc, BlasGeometry, BlasHandle, BufferDesc, BufferHandle, Gpu};
use maki_scene::MeshInstance;

use crate::textures::{GpuTextures, NO_TEXTURE};
use crate::{RenderGpu, RenderWorld, setup_renderer};

/// Size of the mesh table, in entries.
// ponytail: fixed size, because a buffer cannot grow. Make a larger table and copy
// the entries when a scene needs more meshes.
pub const MAX_MESHES: u32 = 1024;

/// Bytes of one entry of the mesh table: `MeshEntry` in `scene.glsl`.
const ENTRY_BYTES: usize = 64;
/// `NormalBuffer` and `UvBuffer` in `scene.glsl` declare this alignment for their address.
const BUFFER_ALIGNMENT: u64 = 4;
/// Bytes of the texture coordinates of one triangle corner: two `f32`.
const UV_BYTES: u64 = 8;

/// One mesh to draw. A resource of the render world holds the list.
#[derive(Debug, Clone, Copy)]
pub struct ExtractedInstance {
    /// The same number in each frame for the same entity, so a pass can find the
    /// instance of the frame before. Not an index: the order of the list can change.
    pub id: u64,
    pub mesh: Handle<Mesh>,
    /// From the space of the mesh to world space.
    pub transform: Mat4,
    /// `transform` of the frame before. Equal to `transform` for a new instance. A pass
    /// computes the screen motion of a point from the two.
    pub previous_transform: Mat4,
}

/// All mesh instances of the frame. `extract_instances` writes the full list in each `Extract`.
#[derive(Resource, Default)]
pub struct ExtractedInstances(pub Vec<ExtractedInstance>);

/// The GPU side of one mesh asset.
#[derive(Debug)]
pub struct GpuMesh {
    /// Index of the mesh in the mesh table. Shaders name the mesh by this number.
    pub index: u32,
    pub vertex_count: u32,
    pub index_count: u32,
    /// The triangles of the group of this mesh, as rays see them. An instance in the
    /// TLAS names it, and draws all meshes of the group.
    pub(crate) blas: BlasHandle,
    /// Table index of the first mesh of the group, and the number of meshes in it.
    pub(crate) group_first: u32,
    pub(crate) group_len: u32,
    /// False: the camera does not see the back side of the group (`Mesh::open_back`).
    pub(crate) two_sided: bool,
}

/// How much of the texture code the trace pass needs: each step costs GPU time for
/// all rays, so a scene pays only for what its meshes use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextureUse {
    None,
    Textures,
    /// Textures, and a mesh with holes.
    Cutout,
}

/// What the meshes of one group share.
struct Group {
    normals: BufferHandle,
    /// `None`: no mesh of the group has a texture.
    uvs: Option<BufferHandle>,
    /// A mesh of the group has holes (a cut-out texture).
    cutout: bool,
    first: u32,
    len: u32,
}

/// All meshes that are on the GPU. A resource of the render world.
#[derive(Resource)]
pub struct GpuMeshes {
    table: BufferHandle,
    meshes: HashMap<Handle<Mesh>, GpuMesh>,
    groups: HashMap<BlasHandle, Group>,
    textures: GpuTextures,
    /// True for each table index that a mesh uses.
    used: Vec<bool>,
    /// The time of the last `sync`.
    last_sync_time: std::time::Duration,
}

impl GpuMeshes {
    fn new(gpu: &mut Gpu) -> Result<Self> {
        let table = gpu.create_buffer(&BufferDesc {
            name: "mesh table",
            size: u64::from(MAX_MESHES) * ENTRY_BYTES as u64,
            cpu_writable: true,
        })?;
        Ok(Self {
            table,
            meshes: HashMap::new(),
            groups: HashMap::new(),
            textures: GpuTextures::new(gpu)?,
            used: vec![false; MAX_MESHES as usize],
            last_sync_time: std::time::Duration::ZERO,
        })
    }

    /// The mesh table. A shader gets its bindless index (`Gpu::buffer_index`).
    pub fn table(&self) -> BufferHandle {
        self.table
    }

    /// `None` if the mesh is not on the GPU: not loaded yet, removed, or failed to upload.
    pub fn get(&self, mesh: Handle<Mesh>) -> Option<&GpuMesh> {
        self.meshes.get(&mesh)
    }

    /// The time that the last change of the mesh store took here: the uploads and the
    /// BLAS builds, with their wait for the GPU. For a benchmark.
    pub fn last_sync_time(&self) -> std::time::Duration {
        self.last_sync_time
    }

    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// The number of textures on the GPU.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// What the trace shader needs for the meshes that are on the GPU.
    pub(crate) fn texture_use(&self) -> TextureUse {
        if self.groups.values().any(|group| group.cutout) {
            TextureUse::Cutout
        } else if self.groups.values().any(|group| group.uvs.is_some()) {
            TextureUse::Textures
        } else {
            TextureUse::None
        }
    }

    /// Number of meshes in the group of a BLAS. 0 if no group has this BLAS.
    pub(crate) fn get_group_len(&self, blas: BlasHandle) -> u32 {
        self.groups.get(&blas).map_or(0, |group| group.len)
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// Makes the GPU meshes and textures agree with the stores.
    fn sync(
        &mut self,
        gpu: &mut Gpu,
        assets: &Assets<Mesh>,
        models: &Assets<Model>,
        textures: &Textures,
    ) {
        let start = std::time::Instant::now();
        // The textures first: the table entry of a mesh names its textures.
        // ponytail: a texture that is not loaded when its mesh is uploaded never
        // shows. With a loader thread, write the table entry again when it arrives.
        self.textures.sync(gpu, textures);
        // This is where the GPU memory of a mesh is freed while the game runs.
        // `destroy_buffer` waits until no frame in flight can read the buffer.
        // At shutdown, `Gpu` frees all buffers that are left.
        // A BLAS cannot lose a geometry. So a group goes away as a whole when one of its
        // meshes is removed, and the loop below uploads the meshes that are left again.
        let dead_groups: Vec<BlasHandle> = self
            .meshes
            .iter()
            .filter(|(handle, _)| assets.get(**handle).is_none())
            .map(|(_, mesh)| mesh.blas)
            .collect();
        for blas in dead_groups {
            let Some(group) = self.groups.remove(&blas) else {
                continue; // Two dead meshes of one group.
            };
            self.meshes.retain(|_, mesh| mesh.blas != blas);
            gpu.destroy_blas(blas);
            gpu.destroy_buffer(group.normals);
            if let Some(uvs) = group.uvs {
                gpu.destroy_buffer(uvs);
            }
            self.used[group.first as usize..][..group.len as usize].fill(false);
            tracing::info!(
                first_index = group.first,
                meshes = group.len,
                "meshes removed from GPU"
            );
        }

        // The meshes of each model as one group, then each mesh that is left alone.
        let new = |meshes: &HashMap<_, _>, handle: Handle<Mesh>| {
            let mesh = assets.get(handle)?;
            (!meshes.contains_key(&handle)).then_some((handle, mesh))
        };
        let model_groups: Vec<Vec<_>> = models
            .iter()
            .map(|(_, model)| {
                model
                    .meshes
                    .iter()
                    .filter_map(|h| new(&self.meshes, *h))
                    .collect()
            })
            .collect();
        for group in model_groups {
            self.upload_logged(gpu, &group);
        }
        let single: Vec<_> = assets
            .iter()
            .filter_map(|(h, _)| new(&self.meshes, h))
            .collect();
        for mesh in single {
            // False if the mesh was in two models: it is on the GPU now.
            if !self.meshes.contains_key(&mesh.0) {
                self.upload_logged(gpu, &[mesh]);
            }
        }
        self.last_sync_time = start.elapsed();
    }

    fn upload_logged(&mut self, gpu: &mut Gpu, group: &[(Handle<Mesh>, &Mesh)]) {
        let Some((_, first)) = group.first() else {
            return;
        };
        match self.upload(gpu, group) {
            Ok(first_index) => tracing::info!(
                name = first.name(),
                first_index,
                meshes = group.len(),
                "meshes uploaded"
            ),
            // Tried again when the store changes the next time.
            Err(error) => tracing::error!(
                "cannot upload mesh `{}` and {} more: {error:#}",
                first.name(),
                group.len() - 1
            ),
        }
    }

    /// Uploads the meshes as one group. Returns the table index of the first one.
    fn upload(&mut self, gpu: &mut Gpu, group: &[(Handle<Mesh>, &Mesh)]) -> Result<u32> {
        let first = free_range(&self.used, group.len())
            .context("the mesh table is full (MAX_MESHES)")? as u32;
        let name = group[0].1.name();

        let normal_bytes: Vec<u8> = group
            .iter()
            .flat_map(|(_, mesh)| normal_bytes(mesh))
            .collect();
        let (normals, normal_address) = create_filled(gpu, name, &normal_bytes)?;
        // The textures of each mesh, as the table names them.
        let slots: Vec<[u32; 2]> = group
            .iter()
            .map(|(_, mesh)| {
                [mesh.base_color_texture(), mesh.emissive_texture()]
                    .map(|texture| self.textures.slot(gpu, texture))
            })
            .collect();
        let textured = |slots: &[u32; 2]| slots.iter().any(|slot| *slot != NO_TEXTURE);
        // Texture coordinates only for the meshes that have a texture.
        let uv_bytes: Vec<u8> = group
            .iter()
            .zip(&slots)
            .filter(|(_, slots)| textured(slots))
            .flat_map(|((_, mesh), _)| uv_bytes(mesh))
            .collect();
        let uvs = if uv_bytes.is_empty() {
            None
        } else {
            Some(create_filled(gpu, name, &uv_bytes).inspect_err(|_| gpu.destroy_buffer(normals))?)
        };
        let destroy_buffers = |gpu: &mut Gpu| {
            gpu.destroy_buffer(normals);
            if let Some((uvs, _)) = uvs {
                gpu.destroy_buffer(uvs);
            }
        };
        // The table entries, each with the addresses of the data of its mesh.
        let mut entries = Vec::with_capacity(group.len() * ENTRY_BYTES);
        let mut address = normal_address;
        let mut uv_address = uvs.map_or(0, |(_, address)| address);
        for ((_, mesh), slots) in group.iter().zip(&slots) {
            let corners = mesh.indices().len() as u64;
            let mesh_uvs = if textured(slots) { uv_address } else { 0 };
            entries.extend(entry_bytes(address, mesh_uvs, *slots, mesh));
            address += corners * 4;
            if textured(slots) {
                uv_address += corners * UV_BYTES;
            }
        }

        let positions: Vec<Vec<[f32; 3]>> = group
            .iter()
            .map(|(_, mesh)| mesh.positions().iter().map(|p| p.to_array()).collect())
            .collect();
        let geometries: Vec<_> = group
            .iter()
            .zip(&positions)
            .zip(&slots)
            .map(|(((_, mesh), positions), slots)| BlasGeometry {
                positions,
                indices: mesh.indices(),
                // Holes need the alpha of the color texture. Only such a mesh pays
                // for the test in the ray query loop.
                opaque: mesh.alpha_cutoff().is_none() || slots[0] == NO_TEXTURE,
            })
            .collect();
        // The BLAS is built here, one time: the triangles of a mesh do not change.
        let blas = gpu
            .create_blas(&BlasDesc {
                name,
                geometries: &geometries,
            })
            .and_then(|blas| {
                // No frame in flight reads these entries: no instance names a free index.
                gpu.write_buffer(self.table, u64::from(first) * ENTRY_BYTES as u64, &entries)
                    .inspect_err(|_| gpu.destroy_blas(blas))?;
                Ok(blas)
            })
            .inspect_err(|_| destroy_buffers(gpu))?;

        let len = group.len() as u32;
        // The camera looks through the back side of the room box only. A model has
        // parts with one side (a leaf, a sheet of cloth): they show from the two sides.
        let two_sided = !group.iter().all(|(_, mesh)| mesh.open_back());
        self.used[first as usize..][..group.len()].fill(true);
        self.groups.insert(
            blas,
            Group {
                normals,
                uvs: uvs.map(|(buffer, _)| buffer),
                cutout: geometries.iter().any(|geometry| !geometry.opaque),
                first,
                len,
            },
        );
        for (index, (handle, mesh)) in (first..).zip(group) {
            let gpu_mesh = GpuMesh {
                index,
                // `Mesh::new` checked that the counts fit in a u32.
                vertex_count: mesh.positions().len() as u32,
                index_count: mesh.indices().len() as u32,
                blas,
                group_first: first,
                group_len: len,
                two_sided,
            };
            self.meshes.insert(*handle, gpu_mesh);
        }
        Ok(first)
    }
}

/// The start of the first run of `len` free entries.
fn free_range(used: &[bool], len: usize) -> Option<usize> {
    let mut start = 0;
    for (index, used) in used.iter().enumerate() {
        if *used {
            start = index + 1;
        } else if index + 1 - start == len {
            return Some(start);
        }
    }
    None
}

/// Makes a buffer with `data` as its content. Returns the buffer and its GPU address.
// ponytail: the buffer is in CPU-writable memory. On the test GPU (RTX 5060 Ti) that
// memory is as fast as GPU-only memory for the shaders (measured, run `C2`). Upload
// through a staging buffer into GPU-only memory if a system or a scene size shows a loss.
fn create_filled(gpu: &mut Gpu, name: &str, data: &[u8]) -> Result<(BufferHandle, u64)> {
    let buffer = gpu.create_buffer(&BufferDesc {
        name,
        size: data.len() as u64,
        cpu_writable: true,
    })?;
    let address = gpu.buffer_address(buffer).expect("the buffer is new");
    let filled = gpu.write_buffer(buffer, 0, data).and_then(|()| {
        ensure!(
            address.is_multiple_of(BUFFER_ALIGNMENT),
            "the address of buffer `{name}` is not a multiple of {BUFFER_ALIGNMENT}"
        );
        Ok((buffer, address))
    });
    filled.inspect_err(|_| gpu.destroy_buffer(buffer))
}

/// A normal in 32 bits: the octahedral map, x in the low 16 bits and y in the high 16
/// bits, each a signed number where 32767 is 1.0. `unpack_normal` in `scene.glsl` is
/// the reverse and explains the map.
fn pack_normal(normal: Vec3) -> u32 {
    let sum = normal.abs().element_sum();
    // A normal of length 0 (a bad file) has no direction: any valid value will do.
    let n = if sum > 0.0 { normal / sum } else { Vec3::Z };
    let (mut x, mut y) = (n.x, n.y);
    if n.z < 0.0 {
        // The lower half of the octahedron goes to the corners of the square.
        x = (1.0 - n.y.abs()).copysign(n.x);
        y = (1.0 - n.x.abs()).copysign(n.y);
    }
    let bits = |value: f32| u32::from((value * 32767.0).round() as i16 as u16);
    bits(x) | bits(y) << 16
}

/// `NormalBuffer` of `scene.glsl`: the packed normal of each corner of each triangle.
fn normal_bytes(mesh: &Mesh) -> Vec<u8> {
    let packed: Vec<u32> = mesh.normals().iter().map(|n| pack_normal(*n)).collect();
    let corners = mesh.indices().iter().map(|index| packed[*index as usize]);
    corners.flat_map(u32::to_ne_bytes).collect()
}

/// `UvBuffer` of `scene.glsl`: the texture coordinates of each corner of each
/// triangle, two `f32`. Not packed: coordinates in meters go far past 0 to 1, and a
/// 16-bit float is then too coarse for a texture of some hundred pixels.
fn uv_bytes(mesh: &Mesh) -> Vec<u8> {
    let corners = mesh
        .indices()
        .iter()
        .map(|index| mesh.uvs()[*index as usize]);
    corners.flatten().flat_map(f32::to_ne_bytes).collect()
}

/// `MeshEntry` of `scene.glsl`, in std430 layout. `textures`: the slots of the color
/// texture and of the emissive texture.
fn entry_bytes(normal_address: u64, uv_address: u64, textures: [u32; 2], mesh: &Mesh) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ENTRY_BYTES);
    bytes.extend(normal_address.to_ne_bytes()); // byte 0
    // The `MATERIAL_*` numbers of `scene.glsl`, and the number of the material:
    // the index of refraction of glass, the roughness of metal and plastic.
    let roughness = |roughness: f32| roughness.clamp(Material::MIN_ROUGHNESS, 1.0);
    let (material, param): (u32, f32) = match mesh.material() {
        Material::Diffuse => (0, 1.0),
        Material::Mirror => (1, 1.0),
        Material::Glass { ior } => (2, ior),
        Material::Metal { roughness: r } => (3, roughness(r)),
        Material::Plastic { roughness: r } => (4, roughness(r)),
    };
    bytes.extend(material.to_ne_bytes()); // byte 8
    bytes.extend(param.to_ne_bytes()); // byte 12
    // The fourth value of the two colors holds two numbers of the textures.
    let [r, g, b, alpha] = mesh.base_color();
    let color = [r, g, b, mesh.uvs_per_meter()];
    bytes.extend(color.iter().flat_map(|c| c.to_ne_bytes())); // byte 16
    // The shader tests the alpha of the texture alone, so the alpha of the color is
    // in the limit. 0: no holes.
    let cutoff = match mesh.alpha_cutoff() {
        Some(cutoff) if textures[0] != NO_TEXTURE => cutoff / alpha.max(1e-6),
        _ => 0.0,
    };
    let [r, g, b] = mesh.emissive();
    bytes.extend([r, g, b, cutoff].iter().flat_map(|c| c.to_ne_bytes())); // byte 32
    bytes.extend(uv_address.to_ne_bytes()); // byte 48
    bytes.extend(textures.iter().flat_map(|slot| slot.to_ne_bytes())); // byte 56
    debug_assert_eq!(bytes.len(), ENTRY_BYTES);
    bytes
}

/// Adds the mesh systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    // The store normally comes from `AssetsModule`. This keeps the renderer
    // independent of the order in which the modules are added.
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Model>>()
        .init_resource::<Textures>()
        .add_systems(Startup, setup_meshes.after(setup_renderer))
        .add_systems(Extract, (extract_meshes, extract_instances))
        .add_systems(Render, log_scene);
}

fn setup_meshes(mut gpu: ResMut<RenderGpu>, mut render_world: ResMut<RenderWorld>) {
    let meshes = GpuMeshes::new(&mut gpu.0).expect("mesh table buffer");
    render_world.0.insert_resource(meshes);
    render_world.0.init_resource::<ExtractedInstances>();
}

/// Reads the mesh store of the game world. The passes see only [`GpuMeshes`].
fn extract_meshes(
    assets: Res<Assets<Mesh>>,
    models: Res<Assets<Model>>,
    textures: Res<Textures>,
    mut gpu: ResMut<RenderGpu>,
    mut render_world: ResMut<RenderWorld>,
) {
    // The store changes only when an asset is added, finished, or removed. A texture
    // comes and goes with the meshes that use it.
    if assets.is_changed() {
        let mut meshes = render_world.0.resource_mut::<GpuMeshes>();
        meshes.sync(&mut gpu.0, &assets, &models, &textures);
        // For a check that a room change frees what the old room had. A destroyed
        // resource is counted until no frame in flight can use it.
        tracing::debug!(
            meshes = meshes.len(),
            textures = meshes.texture_count(),
            gpu_bytes = gpu.0.allocated_bytes(),
            "meshes and textures on the GPU"
        );
        // An instance whose mesh was not on the GPU can show now, or the reverse.
        render_world.reset_accumulation();
    }
}

/// Copies each mesh instance into the render world. The renderer never reads the entities.
fn extract_instances(
    instances: Query<(Entity, Ref<Transform>, Ref<MeshInstance>)>,
    mut removed: RemovedComponents<MeshInstance>,
    mut render_world: ResMut<RenderWorld>,
    // The transform that each entity had in the last frame.
    mut last: Local<HashMap<Entity, Mat4>>,
) {
    // An instance is new, moved, got another mesh, or went away. The denoiser finds
    // the old samples of the object itself with the `motion` image. The light on all
    // other surfaces changed too (the shadow of the object moved): a short history.
    let changed = |(_, transform, instance): (Entity, Ref<Transform>, Ref<MeshInstance>)| {
        transform.is_changed() || instance.is_changed()
    };
    if removed.read().count() > 0 || instances.iter().any(changed) {
        tracing::trace!("an instance changed");
        render_world.soften_accumulation();
    }
    let mut extracted = render_world.0.resource_mut::<ExtractedInstances>();
    extracted.0.clear();
    // A new map and not an update of `last`: an entity that went away drops out.
    let mut current = HashMap::with_capacity(last.len());
    for (entity, transform, instance) in &instances {
        let transform = transform.matrix();
        extracted.0.push(ExtractedInstance {
            id: entity.to_bits(),
            mesh: instance.0,
            transform,
            previous_transform: last.get(&entity).copied().unwrap_or(transform),
        });
        current.insert(entity, transform);
    }
    *last = current;
}

/// What the renderer has to draw, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneCounts {
    meshes: usize,
    instances: usize,
    /// Instances whose mesh is not on the GPU. They are not drawn.
    instances_without_mesh: usize,
    /// Sum over the instances that are drawn.
    triangles: u64,
}

impl SceneCounts {
    fn of(world: &World) -> Self {
        let meshes = world.resource::<GpuMeshes>();
        let instances = &world.resource::<ExtractedInstances>().0;
        let drawn = || instances.iter().filter_map(|i| meshes.get(i.mesh));
        Self {
            meshes: meshes.len(),
            instances: instances.len(),
            instances_without_mesh: instances.len() - drawn().count(),
            triangles: drawn().map(|mesh| u64::from(mesh.index_count / 3)).sum(),
        }
    }
}

/// Logs the content of the render world each time it changes.
fn log_scene(render_world: Res<RenderWorld>, mut logged: Local<Option<SceneCounts>>) {
    let counts = SceneCounts::of(&render_world.0);
    if *logged != Some(counts) {
        *logged = Some(counts);
        tracing::info!(
            meshes = counts.meshes,
            instances = counts.instances,
            instances_without_mesh = counts.instances_without_mesh,
            triangles = counts.triangles,
            "render scene"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_at(bytes: &[u8], offset: usize) -> f32 {
        f32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn buffer_bytes_follow_the_shader_layouts() {
        let mesh = Mesh::cube(2.0).with_material(Material::Glass { ior: 1.5 });

        // One packed normal for each index, in index order.
        let normals = normal_bytes(&mesh);
        assert_eq!(normals.len(), mesh.indices().len() * 4);
        let corner = mesh.normals()[mesh.indices()[4] as usize];
        assert_eq!(u32_at(&normals, 16), pack_normal(corner));

        let entry = entry_bytes(0x1122_3344_5566_7788, 0, [NO_TEXTURE; 2], &mesh);
        assert_eq!(entry.len(), ENTRY_BYTES);
        assert_eq!(entry[..8], 0x1122_3344_5566_7788_u64.to_ne_bytes());
        assert_eq!(u32_at(&entry, 8), 2);
        assert_eq!(f32_at(&entry, 12), 1.5);
        assert_eq!(f32_at(&entry, 16), mesh.base_color()[0]);
        // A cube has texture coordinates in meters.
        assert_eq!(f32_at(&entry, 28), 1.0);
        // No light of its own, no holes, no texture.
        assert_eq!(f32_at(&entry, 32), 0.0);
        assert_eq!(f32_at(&entry, 44), 0.0);
        assert_eq!(entry[48..56], [0; 8]);
        assert_eq!([u32_at(&entry, 56), u32_at(&entry, 60)], [NO_TEXTURE; 2]);

        // A texture with holes: the limit, over the alpha of the color. Two floats
        // of texture coordinates for each index.
        let leaf = Mesh::cube(1.0)
            .with_base_color([1.0, 1.0, 1.0, 0.5])
            .with_alpha_cutoff(0.25);
        let entry = entry_bytes(0, 0x99, [7 | 3 << 16, NO_TEXTURE], &leaf);
        assert_eq!(f32_at(&entry, 44), 0.5);
        assert_eq!(entry[48..56], 0x99_u64.to_ne_bytes());
        assert_eq!(u32_at(&entry, 56), 7 | 3 << 16);
        let uvs = uv_bytes(&leaf);
        assert_eq!(uvs.len(), leaf.indices().len() * UV_BYTES as usize);
        let corner = leaf.uvs()[leaf.indices()[4] as usize];
        assert_eq!([f32_at(&uvs, 32), f32_at(&uvs, 36)], corner);
        // With no texture the holes cannot be read: none.
        assert_eq!(f32_at(&entry_bytes(0, 0, [NO_TEXTURE; 2], &leaf), 44), 0.0);

        // A roughness below the limit is the limit; the light of the mesh follows the color.
        let lamp = Mesh::cube(1.0)
            .with_material(Material::Metal { roughness: 0.0 })
            .with_emissive([3.0, 2.0, 1.0]);
        let entry = entry_bytes(0, 0, [NO_TEXTURE; 2], &lamp);
        assert_eq!(u32_at(&entry, 8), 3);
        assert_eq!(f32_at(&entry, 12), Material::MIN_ROUGHNESS);
        assert_eq!(
            [f32_at(&entry, 32), f32_at(&entry, 36), f32_at(&entry, 40)],
            [3.0, 2.0, 1.0]
        );
    }

    #[test]
    fn an_instance_keeps_its_id_and_carries_the_transform_of_the_frame_before() {
        use bevy_ecs::schedule::Schedule;

        let mut world = World::new();
        let mut render_world = RenderWorld::default();
        render_world.0.init_resource::<ExtractedInstances>();
        world.insert_resource(render_world);
        let mesh = Assets::<Mesh>::default().add(Mesh::cube(1.0));
        let at = |x| Transform::from_translation(Vec3::new(x, 0.0, 0.0));
        let entity = world.spawn((at(1.0), MeshInstance(mesh))).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(extract_instances);
        let mut extract = |world: &mut World| {
            schedule.run(world);
            let render_world = &world.resource::<RenderWorld>().0;
            render_world.resource::<ExtractedInstances>().0.clone()
        };

        // New: no frame before, so no motion.
        let first = extract(&mut world);
        assert_eq!(first[0].previous_transform, at(1.0).matrix());

        *world.get_mut::<Transform>(entity).unwrap() = at(3.0);
        let moved = extract(&mut world);
        assert_eq!(moved[0].id, first[0].id);
        assert_eq!(moved[0].transform, at(3.0).matrix());
        assert_eq!(moved[0].previous_transform, at(1.0).matrix());

        // At rest: the two transforms agree again.
        let rest = extract(&mut world);
        assert_eq!(rest[0].previous_transform, at(3.0).matrix());

        // Another entity gets another id, and the old one drops out.
        world.despawn(entity);
        world.spawn((at(5.0), MeshInstance(mesh)));
        let other = extract(&mut world);
        assert_eq!(other.len(), 1);
        assert_ne!(other[0].id, first[0].id);
        assert_eq!(other[0].previous_transform, at(5.0).matrix());
    }

    #[test]
    fn a_group_gets_the_first_free_run_of_table_entries() {
        let used = [true, false, true, false, false, false, true];
        assert_eq!(free_range(&used, 1), Some(1));
        assert_eq!(free_range(&used, 3), Some(3));
        assert_eq!(free_range(&used, 4), None);
    }

    /// `unpack_normal` of `scene.glsl`, step by step in Rust.
    fn unpack_normal(packed: u32) -> Vec3 {
        let x = f32::from(packed as u16 as i16) / 32767.0;
        let y = f32::from((packed >> 16) as u16 as i16) / 32767.0;
        let z = 1.0 - x.abs() - y.abs();
        let fold = (-z).max(0.0);
        let unfold = |value: f32| value + if value >= 0.0 { -fold } else { fold };
        Vec3::new(unfold(x), unfold(y), z).normalize()
    }

    #[test]
    fn a_packed_normal_unpacks_to_the_same_direction() {
        // A fixed sequence of directions over the full sphere (a Fibonacci spiral).
        let directions = (0..2000).map(|i| {
            let z = 1.0 - (i as f32 + 0.5) / 1000.0;
            let angle = i as f32 * 2.399_963;
            let radius = (1.0 - z * z).sqrt();
            Vec3::new(radius * angle.cos(), radius * angle.sin(), z)
        });
        let axes = [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z];
        for normal in directions.chain(axes) {
            let error = (unpack_normal(pack_normal(normal)) - normal)
                .abs()
                .max_element();
            assert!(error < 1e-4, "{normal}: error {error}");
        }
        // No direction, but a valid normal.
        assert!(unpack_normal(pack_normal(Vec3::ZERO)).is_normalized());
    }
}
