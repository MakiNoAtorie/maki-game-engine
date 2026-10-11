//! Meshes on the GPU, and the list of mesh instances to draw.
//!
//! Each mesh asset gets a vertex buffer and an index buffer. One more buffer, the mesh
//! table, has an entry for each mesh with the addresses of its two buffers. A shader
//! that knows the index of a mesh in the table can thus read all of its data.
//! `shaders/scene.glsl` has the same layouts on the shader side.
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
use glam::Mat4;
use maki_assets::{Assets, Handle, Mesh};
use maki_core::{App, Extract, Render, Startup};
use maki_gpu::{BufferDesc, BufferHandle, Gpu};

use crate::{RenderGpu, RenderWorld, setup_renderer};

/// Size of the mesh table, in entries.
// ponytail: fixed size, because a buffer cannot grow. Make a larger table and copy
// the entries when a scene needs more meshes.
pub const MAX_MESHES: u32 = 1024;

/// Bytes of one vertex in a vertex buffer: `Vertex` in `scene.glsl`.
const VERTEX_BYTES: usize = 32;
/// Bytes of one entry of the mesh table: `MeshEntry` in `scene.glsl`.
const ENTRY_BYTES: usize = 48;
/// `VertexBuffer` in `scene.glsl` declares this alignment for its address.
const BUFFER_ALIGNMENT: u64 = 16;

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
    vertices: BufferHandle,
    indices: BufferHandle,
}

/// All meshes that are on the GPU. A resource of the render world.
#[derive(Resource)]
pub struct GpuMeshes {
    table: BufferHandle,
    meshes: HashMap<Handle<Mesh>, GpuMesh>,
    /// Table indices that no mesh uses. The last one is used first.
    free_indices: Vec<u32>,
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
            free_indices: (0..MAX_MESHES).rev().collect(),
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

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// Makes the GPU meshes agree with the store.
    fn sync(&mut self, gpu: &mut Gpu, assets: &Assets<Mesh>) {
        // This is where the GPU memory of a mesh is freed while the game runs.
        // `destroy_buffer` waits until no frame in flight can read the buffer.
        // At shutdown, `Gpu` frees all buffers that are left.
        self.meshes.retain(|handle, mesh| {
            let alive = assets.get(*handle).is_some();
            if !alive {
                gpu.destroy_buffer(mesh.vertices);
                gpu.destroy_buffer(mesh.indices);
                self.free_indices.push(mesh.index);
                tracing::info!(index = mesh.index, "mesh removed from GPU");
            }
            alive
        });

        for (handle, mesh) in assets.iter() {
            if self.meshes.contains_key(&handle) {
                continue;
            }
            match self.upload(gpu, mesh) {
                Ok(gpu_mesh) => {
                    tracing::info!(
                        name = mesh.name(),
                        index = gpu_mesh.index,
                        vertices = gpu_mesh.vertex_count,
                        indices = gpu_mesh.index_count,
                        "mesh uploaded"
                    );
                    self.meshes.insert(handle, gpu_mesh);
                }
                // Tried again when the store changes the next time.
                Err(error) => tracing::error!("cannot upload mesh `{}`: {error:#}", mesh.name()),
            }
        }
    }

    fn upload(&mut self, gpu: &mut Gpu, mesh: &Mesh) -> Result<GpuMesh> {
        let index = *self
            .free_indices
            .last()
            .context("the mesh table is full (MAX_MESHES)")?;
        let (vertices, vertex_address) = create_filled(gpu, mesh.name(), &vertex_bytes(mesh))?;
        let (indices, index_address) = create_filled(gpu, mesh.name(), &index_bytes(mesh))
            .inspect_err(|_| gpu.destroy_buffer(vertices))?;

        // `Mesh::new` checked that the counts fit in a u32.
        let gpu_mesh = GpuMesh {
            index,
            vertex_count: mesh.positions().len() as u32,
            index_count: mesh.indices().len() as u32,
            vertices,
            indices,
        };
        let entry = entry_bytes(vertex_address, index_address, mesh);
        // No frame in flight reads this entry: no instance names a free index.
        if let Err(error) =
            gpu.write_buffer(self.table, u64::from(index) * ENTRY_BYTES as u64, &entry)
        {
            gpu.destroy_buffer(vertices);
            gpu.destroy_buffer(indices);
            return Err(error);
        }
        self.free_indices.pop();
        Ok(gpu_mesh)
    }
}

/// Makes a buffer with `data` as its content. Returns the buffer and its GPU address.
// ponytail: the buffer is in CPU-writable memory, which the GPU can read slower than
// its own memory on some systems. Upload through a staging buffer into GPU-only
// memory when the gpu crate has a buffer copy.
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

/// `Vertex` of `scene.glsl` for each vertex: position at byte 0, normal at byte 16.
/// A `vec3` in a std430 block has an alignment of 16 bytes, hence the two zeros.
fn vertex_bytes(mesh: &Mesh) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(mesh.positions().len() * VERTEX_BYTES);
    for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
        let vertex = [p.x, p.y, p.z, 0.0, n.x, n.y, n.z, 0.0];
        bytes.extend(vertex.into_iter().flat_map(f32::to_ne_bytes));
    }
    bytes
}

fn index_bytes(mesh: &Mesh) -> Vec<u8> {
    mesh.indices()
        .iter()
        .flat_map(|index| index.to_ne_bytes())
        .collect()
}

/// `MeshEntry` of `scene.glsl`: the field order and the zeros at the end follow std430.
fn entry_bytes(vertex_address: u64, index_address: u64, mesh: &Mesh) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ENTRY_BYTES);
    bytes.extend(vertex_address.to_ne_bytes()); // byte 0
    bytes.extend(index_address.to_ne_bytes()); // byte 8
    bytes.extend(mesh.base_color().iter().flat_map(|c| c.to_ne_bytes())); // byte 16
    bytes.extend((mesh.positions().len() as u32).to_ne_bytes()); // byte 32
    bytes.extend((mesh.indices().len() as u32).to_ne_bytes()); // byte 36
    // The largest alignment in the struct is 16 (the color), so its size is 48.
    bytes.resize(ENTRY_BYTES, 0);
    bytes
}

/// Adds the mesh systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    // The store normally comes from `AssetsModule`. This keeps the renderer
    // independent of the order in which the modules are added.
    app.init_resource::<Assets<Mesh>>()
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
    mut gpu: ResMut<RenderGpu>,
    mut render_world: ResMut<RenderWorld>,
) {
    // The store changes only when an asset is added, finished, or removed.
    if assets.is_changed() {
        let mut meshes = render_world.0.resource_mut::<GpuMeshes>();
        meshes.sync(&mut gpu.0, &assets);
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
        let mesh = Mesh::cube(2.0);

        let vertices = vertex_bytes(&mesh);
        assert_eq!(vertices.len(), mesh.positions().len() * VERTEX_BYTES);
        // The second vertex: position at byte 0 and normal at byte 16 of its block.
        let (position, normal) = (mesh.positions()[1], mesh.normals()[1]);
        assert_eq!(f32_at(&vertices, VERTEX_BYTES + 8), position.z);
        assert_eq!(f32_at(&vertices, VERTEX_BYTES + 16), normal.x);

        assert_eq!(index_bytes(&mesh).len(), mesh.indices().len() * 4);

        let entry = entry_bytes(0x1122_3344_5566_7788, 0x10, &mesh);
        assert_eq!(entry.len(), ENTRY_BYTES);
        assert_eq!(entry[..8], 0x1122_3344_5566_7788_u64.to_ne_bytes());
        assert_eq!(entry[8..16], 0x10_u64.to_ne_bytes());
        assert_eq!(f32_at(&entry, 16), mesh.base_color()[0]);
        assert_eq!(f32_at(&entry, 28), mesh.base_color()[3]);
        assert_eq!(u32_at(&entry, 32), 24);
        assert_eq!(u32_at(&entry, 36), 36);
    }
}
