//! Meshes on the GPU, and the list of mesh instances to draw.
//!
//! The meshes are uploaded in groups. A group is the meshes of one model, or one mesh
//! that is in no model. Each group gets:
//! - one BLAS (the acceleration structure that rays are traced against) with one
//!   geometry for each mesh. One BLAS for a model and not one for each mesh: rays are
//!   faster against one structure than against many that overlap.
//! - one normal buffer: the normals of the three corners of each triangle, mesh after mesh.
//!
//! One more buffer, the mesh table, has an entry for each mesh with its material and
//! the address of its normals. The meshes of a group have table indices in sequence, in
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
//! - A system of the game writes [`ExtractedInstances`]: which mesh is where.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res, ResMut};
use bevy_ecs::world::World;
use glam::{Mat4, Vec3};
use maki_assets::{Assets, Handle, Material, Mesh, Model};
use maki_core::{App, Extract, Render, Startup};
use maki_gpu::{BlasDesc, BlasGeometry, BlasHandle, BufferDesc, BufferHandle, Gpu};

use crate::{RenderGpu, RenderWorld, setup_renderer};

/// Size of the mesh table, in entries.
// ponytail: fixed size, because a buffer cannot grow. Make a larger table and copy
// the entries when a scene needs more meshes.
pub const MAX_MESHES: u32 = 1024;

/// Bytes of one entry of the mesh table: `MeshEntry` in `scene.glsl`.
const ENTRY_BYTES: usize = 32;
/// `NormalBuffer` in `scene.glsl` declares this alignment for its address.
const BUFFER_ALIGNMENT: u64 = 4;

/// One mesh to draw. A resource of the render world holds the list.
#[derive(Debug, Clone, Copy)]
pub struct ExtractedInstance {
    pub mesh: Handle<Mesh>,
    /// From the space of the mesh to world space.
    pub transform: Mat4,
}

/// All mesh instances of the frame. The game writes the full list in each `Extract`.
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
}

/// What the meshes of one group share.
struct Group {
    normals: BufferHandle,
    first: u32,
    len: u32,
}

/// All meshes that are on the GPU. A resource of the render world.
#[derive(Resource)]
pub struct GpuMeshes {
    table: BufferHandle,
    meshes: HashMap<Handle<Mesh>, GpuMesh>,
    groups: HashMap<BlasHandle, Group>,
    /// True for each table index that a mesh uses.
    used: Vec<bool>,
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
            used: vec![false; MAX_MESHES as usize],
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

    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// Number of meshes in the group of a BLAS. 0 if no group has this BLAS.
    pub(crate) fn get_group_len(&self, blas: BlasHandle) -> u32 {
        self.groups.get(&blas).map_or(0, |group| group.len)
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// Makes the GPU meshes agree with the stores.
    fn sync(&mut self, gpu: &mut Gpu, assets: &Assets<Mesh>, models: &Assets<Model>) {
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
        // The table entries, each with the address of the normals of its mesh.
        let mut entries = Vec::with_capacity(group.len() * ENTRY_BYTES);
        let mut address = normal_address;
        for (_, mesh) in group {
            entries.extend(entry_bytes(address, mesh));
            address += mesh.indices().len() as u64 * 4;
        }

        let positions: Vec<Vec<[f32; 3]>> = group
            .iter()
            .map(|(_, mesh)| mesh.positions().iter().map(|p| p.to_array()).collect())
            .collect();
        let geometries: Vec<_> = group
            .iter()
            .zip(&positions)
            .map(|((_, mesh), positions)| BlasGeometry {
                positions,
                indices: mesh.indices(),
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
            .inspect_err(|_| gpu.destroy_buffer(normals))?;

        let len = group.len() as u32;
        self.used[first as usize..][..group.len()].fill(true);
        self.groups.insert(
            blas,
            Group {
                normals,
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

/// `MeshEntry` of `scene.glsl`, in std430 layout.
fn entry_bytes(normal_address: u64, mesh: &Mesh) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ENTRY_BYTES);
    bytes.extend(normal_address.to_ne_bytes()); // byte 0
    // The `MATERIAL_*` numbers of `scene.glsl`. Only glass uses the index of refraction.
    let (material, ior): (u32, f32) = match mesh.material() {
        Material::Diffuse => (0, 1.0),
        Material::Mirror => (1, 1.0),
        Material::Glass { ior } => (2, ior),
    };
    bytes.extend(material.to_ne_bytes()); // byte 8
    bytes.extend(ior.to_ne_bytes()); // byte 12
    bytes.extend(mesh.base_color().iter().flat_map(|c| c.to_ne_bytes())); // byte 16
    debug_assert_eq!(bytes.len(), ENTRY_BYTES);
    bytes
}

/// Adds the mesh systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    // The store normally comes from `AssetsModule`. This keeps the renderer
    // independent of the order in which the modules are added.
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Model>>()
        .add_systems(Startup, setup_meshes.after(setup_renderer))
        .add_systems(Extract, extract_meshes)
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
    mut gpu: ResMut<RenderGpu>,
    mut render_world: ResMut<RenderWorld>,
) {
    // The store changes only when an asset is added, finished, or removed.
    if assets.is_changed() {
        let mut meshes = render_world.0.resource_mut::<GpuMeshes>();
        meshes.sync(&mut gpu.0, &assets, &models);
        // An instance whose mesh was not on the GPU can show now, or the reverse.
        render_world.reset_accumulation();
    }
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

        let entry = entry_bytes(0x1122_3344_5566_7788, &mesh);
        assert_eq!(entry.len(), ENTRY_BYTES);
        assert_eq!(entry[..8], 0x1122_3344_5566_7788_u64.to_ne_bytes());
        assert_eq!(u32_at(&entry, 8), 2);
        assert_eq!(f32_at(&entry, 12), 1.5);
        assert_eq!(f32_at(&entry, 16), mesh.base_color()[0]);
        assert_eq!(f32_at(&entry, 28), mesh.base_color()[3]);
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
