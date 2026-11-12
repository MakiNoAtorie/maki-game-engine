//! Skinned instances: the meshes of a model with a skeleton, bent by its joints on the GPU.
//!
//! The game world has a [`SkinPalette`] (a matrix for each joint) on the entity of the
//! `MeshInstance` of the model. The mesh assets keep their vertices in the bind pose.
//! For each skinned instance the renderer keeps:
//! - its own block of the mesh table: the entries of the group of its mesh, with the
//!   address of its own normal buffer (the bent normals);
//! - the input of the shader: for each triangle corner its bind position, its bind
//!   normal, its joints, and its weights;
//! - a palette buffer for each frame slot (the CPU writes the next frame's while the
//!   GPU reads this one's);
//! - the current corner positions (the input of its BLAS) and the previous ones (for
//!   the motion of each pixel), and the corner normals;
//! - its own dynamic BLAS, built again after each new pose.
//!
//! Data flow:
//! - `extract_skins` (`Extract`) copies each changed palette into [`ExtractedSkins`].
//! - `sync_skins` (`Extract`) makes and frees the GPU side of the skins, and marks a
//!   skin with a new pose.
//! - The pass `skin` (before `build_tlas`) runs `shaders/skin.comp` for each marked
//!   skin, then builds its BLAS again. `build_tlas` then builds the TLAS, also with
//!   an equal instance list (Vulkan: a TLAS that names a changed BLAS is built again),
//!   and names the previous corner positions of a skin in the instance buffer.
//!
//! The previous positions: the shader reads the old current position of its corner
//! and writes it to the previous buffer before it writes the new one. In a frame with
//! no new pose the instance buffer names the current positions: no motion of the skin.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result, ensure};
use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Mat4;
use maki_assets::{Assets, Handle, Mesh};
use maki_core::{App, Extract, Startup};
use maki_gpu::{
    Access, BlasHandle, BufferDesc, BufferHandle, ComputePipelineHandle, DynamicBlasDesc,
    DynamicGeometry, FRAMES_IN_FLIGHT, Gpu,
};
use maki_scene::{MeshInstance, SkinPalette};

use crate::graph::{Pass, PassContext, ResourceId};
use crate::meshes::{ENTRY_BYTES, GpuMeshes, extract_meshes};
use crate::passes::push_constants;
use crate::{ExtractedFrame, RenderGpu, RenderWorld, setup_renderer, shader_spirv};

/// Bytes of the input of one corner: `Corner` in `shaders/skin.comp`.
const CORNER_INPUT_BYTES: usize = 64;
/// Bytes of the position of one corner: three `f32`, the vertex format of the BLAS.
const POSITION_BYTES: u64 = 12;
/// Threads in a work group of `skin.comp`.
const GROUP_SIZE: u32 = 64;
/// `GpuSkin::skinned` before the first skin pass.
const NEVER: u64 = u64::MAX;

/// A palette that changed since the last frame.
#[derive(Debug, Clone)]
pub struct ExtractedSkin {
    /// The id of the instance (`ExtractedInstance::id`).
    pub id: u64,
    /// The mesh of the instance: its group is the skin.
    pub mesh: Handle<Mesh>,
    pub palette: Vec<Mat4>,
}

/// The skin changes of a frame. `extract_skins` writes it in each `Extract`.
#[derive(Resource, Default)]
pub struct ExtractedSkins {
    pub changed: Vec<ExtractedSkin>,
    /// The ids of the instances that lost their palette (despawned).
    pub removed: Vec<u64>,
}

/// The GPU side of one skinned instance.
pub(crate) struct GpuSkin {
    mesh: Handle<Mesh>,
    /// The BLAS of the group that the skin was made from: another one after a new upload.
    source: BlasHandle,
    /// The block of the mesh table: the custom index of the TLAS instance.
    pub first: u32,
    len: u32,
    pub two_sided: bool,
    pub blas: BlasHandle,
    corners: u32,
    joints: u32,
    inputs: BufferHandle,
    palettes: [BufferHandle; FRAMES_IN_FLIGHT],
    current: BufferHandle,
    previous: BufferHandle,
    normals: BufferHandle,
    /// The pose to bend to. `dirty` until the pass `skin` did.
    palette: Vec<Mat4>,
    // Atomics: the passes get the render world read-only.
    dirty: AtomicBool,
    /// The number of the frame of the last skin pass of this instance.
    skinned: AtomicU64,
}

impl GpuSkin {
    /// The corner positions of the frame before, for the instance buffer: the previous
    /// ones in a frame with a new pose, else the current ones (no motion).
    pub fn previous_positions(&self, frame: u64) -> BufferHandle {
        if self.skinned.load(Ordering::Relaxed) == frame {
            self.previous
        } else {
            self.current
        }
    }

    /// Makes the GPU side of a skin. `Ok(None)`: the meshes are not on the GPU yet.
    fn new(
        gpu: &mut Gpu,
        assets: &Assets<Mesh>,
        meshes: &mut GpuMeshes,
        skin: &ExtractedSkin,
    ) -> Result<Option<Self>> {
        let Some(group) = meshes.group_source(skin.mesh) else {
            return Ok(None);
        };
        let Some(parts) = group
            .meshes
            .iter()
            .map(|handle| assets.get(*handle))
            .collect::<Option<Vec<&Mesh>>>()
        else {
            return Ok(None);
        };
        let inputs: Vec<u8> = parts.iter().flat_map(|mesh| corner_bytes(mesh)).collect();
        let corners = (inputs.len() / CORNER_INPUT_BYTES) as u32;
        ensure!(corners > 0, "a skin with no triangles");
        let geometries: Vec<DynamicGeometry> = parts
            .iter()
            .zip(group.entries.chunks(ENTRY_BYTES))
            .map(|(mesh, entry)| DynamicGeometry {
                corners: mesh.indices().len() as u32,
                // The hole limit of the entry (`MeshEntry.emissive.a`): 0 for no holes.
                opaque: entry[44..48] == [0; 4],
            })
            .collect();
        let (source, two_sided) = (group.blas, group.two_sided);
        let entries = group.entries.to_vec();
        let joints = skin.palette.len() as u32;

        let mut made = Vec::new();
        let mut buffer = |gpu: &mut Gpu, name, size: u64, cpu_writable| -> Result<BufferHandle> {
            let buffer = gpu.create_buffer(&BufferDesc {
                name,
                size,
                cpu_writable,
            })?;
            made.push(buffer);
            Ok(buffer)
        };
        let parts = (|| -> Result<_> {
            let input_buffer = buffer(gpu, "skin input", inputs.len() as u64, true)?;
            gpu.write_buffer(input_buffer, 0, &inputs)?;
            let palette_bytes = u64::from(joints) * 64;
            let palettes: Vec<BufferHandle> = (0..FRAMES_IN_FLIGHT)
                .map(|_| buffer(gpu, "skin palette", palette_bytes, true))
                .collect::<Result<_>>()?;
            let palettes: [BufferHandle; FRAMES_IN_FLIGHT] =
                palettes.try_into().expect("one for each frame slot");
            let positions = u64::from(corners) * POSITION_BYTES;
            let current = buffer(gpu, "skin positions", positions, false)?;
            let previous = buffer(gpu, "skin previous positions", positions, false)?;
            let normals = buffer(gpu, "skin normals", u64::from(corners) * 4, false)?;
            Ok((input_buffer, palettes, current, previous, normals))
        })();
        let destroy = |gpu: &mut Gpu, made: &[BufferHandle]| {
            made.iter().for_each(|buffer| gpu.destroy_buffer(*buffer));
        };
        let (input_buffer, palettes, current, previous, normals) =
            parts.inspect_err(|_| destroy(gpu, &made))?;
        let normal_address = gpu.buffer_address(normals).context("the buffer is new")?;
        let blas = gpu
            .create_dynamic_blas(&DynamicBlasDesc {
                name: "skin",
                vertices: current,
                geometries: &geometries,
            })
            .inspect_err(|_| destroy(gpu, &made))?;
        let first = meshes
            .add_entries(gpu, &skin_entries(&entries, normal_address))
            .inspect_err(|_| {
                gpu.destroy_blas(blas);
                destroy(gpu, &made);
            })?;
        Ok(Some(Self {
            mesh: skin.mesh,
            source,
            first,
            len: (entries.len() / ENTRY_BYTES) as u32,
            two_sided,
            blas,
            corners,
            joints,
            inputs: input_buffer,
            palettes,
            current,
            previous,
            normals,
            palette: skin.palette.clone(),
            dirty: AtomicBool::new(true),
            skinned: AtomicU64::new(NEVER),
        }))
    }

    /// True if the skin can take this palette: the same mesh, group, and joint count.
    fn fits(&self, skin: &ExtractedSkin, meshes: &GpuMeshes) -> bool {
        self.mesh == skin.mesh
            && self.joints as usize == skin.palette.len()
            && meshes
                .group_source(skin.mesh)
                .is_some_and(|group| group.blas == self.source)
    }

    fn destroy(self, gpu: &mut Gpu, meshes: &mut GpuMeshes) {
        meshes.remove_entries(self.first, self.len);
        gpu.destroy_blas(self.blas);
        let buffers = [self.inputs, self.current, self.previous, self.normals];
        for buffer in buffers.into_iter().chain(self.palettes) {
            gpu.destroy_buffer(buffer);
        }
    }
}

/// All skinned instances on the GPU. A resource of the render world.
#[derive(Resource, Default)]
pub(crate) struct GpuSkins {
    skins: HashMap<u64, GpuSkin>,
    /// Palettes whose meshes are not on the GPU yet: tried again in each frame.
    waiting: HashMap<u64, ExtractedSkin>,
    /// Instances whose skin could not be made: they draw in the bind pose, with one
    /// error, until they are despawned (also by a room change).
    failed: HashSet<u64>,
}

impl GpuSkins {
    pub fn get(&self, id: u64) -> Option<&GpuSkin> {
        self.skins.get(&id)
    }

    /// True if the pass `skin` built a BLAS again after the frame `built` (`None`: ever).
    pub fn skinned_after(&self, built: Option<u64>) -> bool {
        let skinned = |skin: &GpuSkin| match skin.skinned.load(Ordering::Relaxed) {
            NEVER => false,
            frame => built.is_none_or(|built| frame > built),
        };
        self.skins.values().any(skinned)
    }

    /// Takes the changes of a frame. True if a skin got a new pose.
    fn sync(
        &mut self,
        gpu: &mut Gpu,
        assets: &Assets<Mesh>,
        meshes: &mut GpuMeshes,
        extracted: ExtractedSkins,
    ) -> bool {
        let Self {
            skins,
            waiting,
            failed,
        } = self;
        for id in extracted.removed {
            waiting.remove(&id);
            failed.remove(&id);
            if let Some(skin) = skins.remove(&id) {
                skin.destroy(gpu, meshes);
            }
        }
        let mut posed = false;
        for skin in extracted.changed {
            if failed.contains(&skin.id) {
                continue;
            }
            match skins.get_mut(&skin.id) {
                Some(gpu_skin) if gpu_skin.fits(&skin, meshes) => {
                    gpu_skin.palette = skin.palette;
                    gpu_skin.dirty.store(true, Ordering::Relaxed);
                    posed = true;
                }
                // New, or another mesh, group, or skeleton: made again.
                _ => {
                    if let Some(old) = skins.remove(&skin.id) {
                        old.destroy(gpu, meshes);
                    }
                    waiting.insert(skin.id, skin);
                }
            }
        }
        waiting.retain(|id, skin| match GpuSkin::new(gpu, assets, meshes, skin) {
            Ok(Some(gpu_skin)) => {
                tracing::info!(
                    instance = id,
                    first_index = gpu_skin.first,
                    corners = gpu_skin.corners,
                    "skin made"
                );
                skins.insert(*id, gpu_skin);
                posed = true;
                false
            }
            Ok(None) => true,
            // The instance draws in the bind pose.
            Err(error) => {
                tracing::error!("cannot make the skin of instance {id}: {error:#}");
                failed.insert(*id);
                false
            }
        });
        posed
    }
}

/// The input of `skin.comp` for the corners of a mesh, in triangle order (as its
/// normals in the mesh table). A mesh with no skin gets weights of zero: the shader
/// leaves it in the pose of the file.
fn corner_bytes(mesh: &Mesh) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(mesh.indices().len() * CORNER_INPUT_BYTES);
    for &index in mesh.indices() {
        let index = index as usize;
        let (position, normal) = (mesh.positions()[index], mesh.normals()[index]);
        let joints = mesh.joints().get(index).copied().unwrap_or_default();
        let weights = mesh.weights().get(index).copied().unwrap_or_default();
        for vector in [position.extend(0.0), normal.extend(0.0)] {
            bytes.extend(vector.to_array().iter().flat_map(|v| v.to_ne_bytes()));
        }
        bytes.extend(joints.iter().flat_map(|v| v.to_ne_bytes()));
        bytes.extend(weights.iter().flat_map(|v| v.to_ne_bytes()));
    }
    bytes
}

/// The table entries of a group with the normal addresses moved into another buffer of
/// the same layout, which starts at `normals`.
fn skin_entries(entries: &[u8], normals: u64) -> Vec<u8> {
    let address = |entry: &[u8]| u64::from_ne_bytes(entry[..8].try_into().expect("8 bytes"));
    let base = address(entries);
    let mut moved = entries.to_vec();
    for entry in moved.chunks_mut(ENTRY_BYTES) {
        let new = address(entry) - base + normals;
        entry[..8].copy_from_slice(&new.to_ne_bytes());
    }
    moved
}

/// Adds the skin systems. Part of `RenderModule`; the pass is in `passes::build_graph`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Startup, setup_skins.after(setup_renderer))
        .add_systems(
            Extract,
            (
                extract_skins,
                sync_skins.after(extract_skins).after(extract_meshes),
            ),
        );
}

fn setup_skins(mut render_world: ResMut<RenderWorld>) {
    render_world.0.init_resource::<ExtractedSkins>();
    render_world.0.init_resource::<GpuSkins>();
}

/// Copies each palette that changed. The passes never read the game world (rule 3).
fn extract_skins(
    skins: Query<(Entity, &MeshInstance, Ref<SkinPalette>)>,
    mut removed: RemovedComponents<SkinPalette>,
    mut render_world: ResMut<RenderWorld>,
) {
    let mut extracted = render_world.0.resource_mut::<ExtractedSkins>();
    extracted.changed.clear();
    extracted.removed.clear();
    extracted
        .removed
        .extend(removed.read().map(|entity| entity.to_bits()));
    for (entity, instance, palette) in &skins {
        // Empty: no pose yet (the default of `SkinPalette`).
        if palette.is_changed() && !palette.0.is_empty() {
            extracted.changed.push(ExtractedSkin {
                id: entity.to_bits(),
                mesh: instance.0,
                palette: palette.0.clone(),
            });
        }
    }
    // A limb moved: the light changed, as for an object that moved.
    if !extracted.changed.is_empty() {
        render_world.soften_accumulation();
    }
}

/// Makes the GPU side of the skins agree with the extracted changes.
fn sync_skins(
    mut gpu: ResMut<RenderGpu>,
    assets: Res<Assets<Mesh>>,
    mut render_world: ResMut<RenderWorld>,
) {
    let world = &mut render_world.0;
    let extracted = std::mem::take(&mut *world.resource_mut::<ExtractedSkins>());
    if extracted.changed.is_empty()
        && extracted.removed.is_empty()
        && world.resource::<GpuSkins>().waiting.is_empty()
    {
        return;
    }
    let posed = world.resource_scope(|world, mut skins: bevy_ecs::world::Mut<GpuSkins>| {
        let mut meshes = world.resource_mut::<GpuMeshes>();
        skins.sync(&mut gpu.0, &assets, &mut meshes, extracted)
    });
    if posed {
        render_world.soften_accumulation();
    }
}

/// Bends the corners of each skin with a new pose and builds its BLAS again. See the
/// module docs.
pub(crate) struct SkinPass {
    pub pipeline: ComputePipelineHandle,
}

impl SkinPass {
    pub fn new(gpu: &mut Gpu) -> Self {
        Self {
            pipeline: gpu
                .create_compute_pipeline(shader_spirv!("skin.comp"))
                .expect("skin pipeline"),
        }
    }
}

impl Pass for SkinPass {
    fn name(&self) -> &'static str {
        "skin"
    }

    // Buffers only: the graph plans barriers for images. See `execute`.
    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        Vec::new()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let Some(skins) = ctx.world.get_resource::<GpuSkins>() else {
            return;
        };
        let dirty: Vec<&GpuSkin> = skins
            .skins
            .values()
            .filter(|skin| skin.dirty.swap(false, Ordering::Relaxed))
            .collect();
        if dirty.is_empty() {
            return;
        }
        let frame = ctx.world.resource::<ExtractedFrame>().frame;
        let slot = ctx.gpu.frame_slot();
        let index = |gpu: &Gpu, buffer| gpu.buffer_index(buffer).expect("the buffer is alive");
        // The frame before may still read the normals and the previous positions.
        ctx.gpu.compute_barrier().expect("skin barrier");
        for skin in &dirty {
            let palette: Vec<u8> = skin
                .palette
                .iter()
                .flat_map(|matrix| matrix.to_cols_array())
                .flat_map(f32::to_ne_bytes)
                .collect();
            ctx.gpu
                .write_buffer(skin.palettes[slot], 0, &palette)
                .expect("palette write");
            // Field order as in the `Push` block of the shader.
            let push = [
                index(ctx.gpu, skin.inputs),
                index(ctx.gpu, skin.palettes[slot]),
                index(ctx.gpu, skin.current),
                index(ctx.gpu, skin.previous),
                index(ctx.gpu, skin.normals),
                skin.corners,
                skin.joints,
                // The first pose has no pose before it: the previous one is the same.
                u32::from(skin.skinned.load(Ordering::Relaxed) != NEVER),
            ];
            ctx.gpu
                .dispatch_compute(
                    self.pipeline,
                    &push_constants(&push),
                    [skin.corners.div_ceil(GROUP_SIZE), 1, 1],
                )
                .expect("skin dispatch");
        }
        // The trace reads the normals and the previous positions of this frame.
        ctx.gpu.compute_barrier().expect("skin barrier");
        for skin in dirty {
            ctx.gpu.rebuild_blas(skin.blas).expect("skin BLAS build");
            skin.skinned.store(frame, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::schedule::Schedule;
    use bevy_ecs::world::World;
    use maki_core::Transform;

    use super::*;

    #[test]
    fn extract_copies_only_a_changed_palette() {
        let mut world = World::new();
        let mut render_world = RenderWorld::default();
        render_world.0.init_resource::<ExtractedSkins>();
        world.insert_resource(render_world);
        let mesh = Assets::<Mesh>::default().add(Mesh::cube(1.0));
        let pose = |x: f32| SkinPalette(vec![Mat4::from_translation(glam::Vec3::X * x)]);
        let entity = world
            .spawn((Transform::IDENTITY, MeshInstance(mesh), pose(1.0)))
            .id();
        // No pose yet: not copied.
        world.spawn((
            Transform::IDENTITY,
            MeshInstance(mesh),
            SkinPalette::default(),
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems(extract_skins);
        let mut extract = |world: &mut World| {
            schedule.run(world);
            world.clear_trackers();
            let skins = world
                .resource::<RenderWorld>()
                .0
                .resource::<ExtractedSkins>();
            let changed = skins.changed.iter().map(|s| (s.id, s.palette.clone()));
            (changed.collect::<Vec<_>>(), skins.removed.clone())
        };
        let id = entity.to_bits();
        assert_eq!(extract(&mut world), (vec![(id, pose(1.0).0)], vec![]));
        // At rest: nothing.
        assert_eq!(extract(&mut world), (vec![], vec![]));
        *world.get_mut::<SkinPalette>(entity).unwrap() = pose(2.0);
        assert_eq!(extract(&mut world), (vec![(id, pose(2.0).0)], vec![]));
        world.despawn(entity);
        assert_eq!(extract(&mut world), (vec![], vec![id]));
    }

    #[test]
    fn a_skin_block_names_its_own_normals_at_the_same_offsets() {
        let mut entries = vec![0u8; 2 * ENTRY_BYTES];
        entries[..8].copy_from_slice(&1000_u64.to_ne_bytes());
        entries[ENTRY_BYTES..][..8].copy_from_slice(&1144_u64.to_ne_bytes());
        entries[8] = 3;
        let moved = skin_entries(&entries, 5000);
        let address = |at: usize| u64::from_ne_bytes(moved[at..at + 8].try_into().unwrap());
        assert_eq!([address(0), address(ENTRY_BYTES)], [5000, 5144]);
        assert_eq!(moved[8..ENTRY_BYTES], entries[8..ENTRY_BYTES]);
    }

    #[test]
    fn corner_input_follows_the_shader_layout() {
        let mesh = Mesh::cube(1.0);
        let bytes = corner_bytes(&mesh);
        assert_eq!(bytes.len(), mesh.indices().len() * CORNER_INPUT_BYTES);
        let f32_at = |at: usize| f32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
        let corner = mesh.indices()[4] as usize;
        let at = 4 * CORNER_INPUT_BYTES;
        assert_eq!(f32_at(at), mesh.positions()[corner].x);
        assert_eq!(f32_at(at + 16 + 8), mesh.normals()[corner].z);
        // No skin: weights of zero.
        assert_eq!(f32_at(at + 48), 0.0);
    }
}
