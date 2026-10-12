//! Acceleration structures: the search trees that a ray query walks to find a hit.
//!
//! Two levels:
//! - A BLAS (bottom level) holds the triangles of one or more meshes ("geometries"),
//!   in one common space. Rays are faster against one BLAS with many geometries than
//!   against many BLAS that overlap, so meshes that never move apart share one.
//! - The TLAS (top level) holds instances. An instance is a BLAS, a transform, and a
//!   number that the shader gets back on a hit, together with the number of the
//!   geometry in the BLAS. Shaders trace against the TLAS (bindless binding 4).
//!
//! A BLAS is built one time ([`Gpu::create_blas`]). The TLAS is built again
//! ([`Gpu::build_tlas`]) when the caller has a new instance list, so it follows objects
//! that move, appear, or go away.

use anyhow::{Context, Result, ensure};
use ash::vk;

use crate::resources::Garbage;
use crate::{BlasHandle, BufferDesc, BufferHandle, FRAMES_IN_FLIGHT, Gpu, TlasHandle};

/// The GPU reads instances of this size from the instance buffer.
const INSTANCE_BYTES: u64 = size_of::<vk::AccelerationStructureInstanceKHR>() as u64;
/// Vulkan requires this alignment for the address of the instance data.
const INSTANCE_ALIGNMENT: u64 = 16;

/// The triangles of one mesh.
#[derive(Debug, Clone, Copy)]
pub struct BlasGeometry<'a> {
    pub positions: &'a [[f32; 3]],
    /// Three for each triangle.
    pub indices: &'a [u32],
    /// True: the first hit of a ray is final. False: the ray query asks the shader if
    /// a triangle counts at the point of the hit (a texture with holes). Slower: only
    /// for a mesh that needs it.
    pub opaque: bool,
}

/// The content of a BLAS. A hit names the geometry by its place in the list.
#[derive(Debug, Clone, Copy)]
pub struct BlasDesc<'a> {
    pub name: &'a str,
    pub geometries: &'a [BlasGeometry<'a>],
}

/// One object in the TLAS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TlasInstance {
    pub blas: BlasHandle,
    /// From the space of the BLAS to world space: the first three rows of the 4x4
    /// matrix, row by row. The fourth row is always (0, 0, 0, 1).
    pub transform: [f32; 12],
    /// The shader gets this number back on a hit. 24 bits.
    pub custom_index: u32,
    /// True: a ray that culls back faces (`gl_RayFlagsCullBackFacingTrianglesEXT`)
    /// does not cull those of this instance.
    pub two_sided: bool,
}

pub(crate) struct Blas {
    pub raw: vk::AccelerationStructureKHR,
    storage: BufferHandle,
    /// A TLAS instance names its BLAS by this address.
    address: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Tlas {
    pub raw: vk::AccelerationStructureKHR,
    scratch: BufferHandle,
    /// One buffer per frame slot: the CPU writes the instances of frame N+1 while the
    /// GPU can still read those of frame N.
    instances: [BufferHandle; FRAMES_IN_FLIGHT],
    max_instances: u32,
}

/// The geometry of a TLAS: a list of instances in a buffer at `address`.
fn instances_geometry(address: u64) -> vk::AccelerationStructureGeometryKHR<'static> {
    let instances = vk::AccelerationStructureGeometryInstancesDataKHR::default().data(
        vk::DeviceOrHostAddressConstKHR {
            device_address: address,
        },
    );
    vk::AccelerationStructureGeometryKHR::default()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .geometry(vk::AccelerationStructureGeometryDataKHR { instances })
}

/// The size query and the build must use the same type and flags, so both use this.
fn tlas_build_info<'a>(
    geometries: &'a [vk::AccelerationStructureGeometryKHR<'a>],
) -> vk::AccelerationStructureBuildGeometryInfoKHR<'a> {
    vk::AccelerationStructureBuildGeometryInfoKHR::default()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        // Built in each frame, so the build time counts more than the trace time.
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_BUILD)
        .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
        .geometries(geometries)
}

impl Gpu {
    /// Builds a BLAS. The build is complete when the function returns: the BLAS has
    /// its own copy of the triangles.
    pub fn create_blas(&mut self, desc: &BlasDesc) -> Result<BlasHandle> {
        ensure!(
            !desc.geometries.is_empty(),
            "BLAS `{}` has no geometry",
            desc.name
        );
        for geometry in desc.geometries {
            let vertex_count = geometry.positions.len();
            ensure!(
                vertex_count > 0 && geometry.indices.len() >= 3,
                "BLAS `{}` has a geometry with no triangles",
                desc.name
            );
            // The GPU does not check the indices: a wrong one reads memory out of the buffer.
            ensure!(
                geometry
                    .indices
                    .iter()
                    .all(|index| (*index as usize) < vertex_count),
                "BLAS `{}` has an index that is not a vertex",
                desc.name
            );
        }
        // All geometries in one vertex buffer and one index buffer, each at its own
        // address. The two buffers live only for the build.
        let positions: Vec<[f32; 3]> = desc
            .geometries
            .iter()
            .flat_map(|g| g.positions)
            .copied()
            .collect();
        let indices: Vec<u32> = desc
            .geometries
            .iter()
            .flat_map(|g| g.indices)
            .copied()
            .collect();
        let vertices = self.create_build_input(desc.name, as_bytes(&positions))?;
        let indices = self
            .create_build_input(desc.name, as_bytes(&indices))
            .inspect_err(|_| self.destroy_unused_buffer(vertices))?;

        let mut vertex_address = self.buffers[vertices].address;
        let mut index_address = self.buffers[indices].address;
        let mut geometries = Vec::with_capacity(desc.geometries.len());
        let mut ranges = Vec::with_capacity(desc.geometries.len());
        for geometry in desc.geometries {
            let triangles = vk::AccelerationStructureGeometryTrianglesDataKHR::default()
                .vertex_format(vk::Format::R32G32B32_SFLOAT)
                .vertex_data(vk::DeviceOrHostAddressConstKHR {
                    device_address: vertex_address,
                })
                .vertex_stride(12)
                .max_vertex(geometry.positions.len() as u32 - 1)
                .index_type(vk::IndexType::UINT32)
                .index_data(vk::DeviceOrHostAddressConstKHR {
                    device_address: index_address,
                });
            geometries.push(
                vk::AccelerationStructureGeometryKHR::default()
                    .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
                    .geometry(vk::AccelerationStructureGeometryDataKHR { triangles })
                    // Opaque: the first hit is final. Not opaque: the ray query asks
                    // the shader if a triangle counts (a cut-out texture).
                    .flags(if geometry.opaque {
                        vk::GeometryFlagsKHR::OPAQUE
                    } else {
                        vk::GeometryFlagsKHR::empty()
                    }),
            );
            ranges.push(
                vk::AccelerationStructureBuildRangeInfoKHR::default()
                    .primitive_count(geometry.indices.len() as u32 / 3),
            );
            vertex_address += size_of_val(geometry.positions) as u64;
            index_address += size_of_val(geometry.indices) as u64;
        }
        let mut info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            // Built one time and traced in every frame.
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .geometries(&geometries);

        let counts: Vec<u32> = ranges.iter().map(|range| range.primitive_count).collect();
        let built =
            self.create_accel(desc.name, &mut info, &counts)
                .and_then(|(raw, storage, scratch)| {
                    let built = self.run_now(|cmd| unsafe {
                        self.accel
                            .cmd_build_acceleration_structures(cmd, &[info], &[&ranges]);
                    });
                    // Only the build uses the scratch buffer.
                    self.destroy_unused_buffer(scratch);
                    if let Err(error) = built {
                        self.defer(Garbage::Accel(raw));
                        self.destroy_buffer(storage);
                        return Err(error);
                    }
                    Ok((raw, storage))
                });
        self.destroy_unused_buffer(vertices);
        self.destroy_unused_buffer(indices);
        let (raw, storage) = built?;

        let address_info =
            vk::AccelerationStructureDeviceAddressInfoKHR::default().acceleration_structure(raw);
        let address = unsafe {
            self.accel
                .get_acceleration_structure_device_address(&address_info)
        };
        Ok(self.blases.insert(Blas {
            raw,
            storage,
            address,
        }))
    }

    /// A buffer with `data` as its content, for the input of a BLAS build.
    fn create_build_input(&mut self, name: &str, data: &[u8]) -> Result<BufferHandle> {
        let buffer = self.create_buffer(&BufferDesc {
            name,
            size: data.len() as u64,
            cpu_writable: true,
        })?;
        self.write_buffer(buffer, 0, data)
            .inspect_err(|_| self.destroy_unused_buffer(buffer))?;
        Ok(buffer)
    }

    /// Frees a buffer at once. Only for a buffer that no frame has used and whose last
    /// GPU work is complete (`run_now` waits for it). Not deferred, so a load of many
    /// meshes does not hold all its work buffers until the next frame.
    fn destroy_unused_buffer(&mut self, buffer: BufferHandle) {
        if let Some(buffer) = self.buffers.remove(buffer) {
            self.destroy_now(Garbage::Buffer(buffer));
        }
    }

    /// Deferred, as [`Gpu::destroy_buffer`]. The next [`Gpu::build_tlas`] must not name the BLAS.
    pub fn destroy_blas(&mut self, blas: BlasHandle) {
        if let Some(blas) = self.blases.remove(blas) {
            // The structure goes before the buffer that stores it.
            self.defer(Garbage::Accel(blas.raw));
            self.destroy_buffer(blas.storage);
        }
    }

    /// Makes the TLAS, with room for `max_instances`, and puts it in the bindless set.
    /// It has no content before the first [`Gpu::build_tlas`].
    /// One TLAS only: the bindless set has one slot. It lives as long as the `Gpu`.
    // ponytail: fixed capacity. Make a larger TLAS and write the slot again when a
    // scene needs more instances.
    pub fn create_tlas(&mut self, max_instances: u32) -> Result<TlasHandle> {
        ensure!(self.tlases.is_empty(), "a TLAS exists already");
        ensure!(max_instances > 0, "a TLAS needs room for an instance");

        let mut instances = [BufferHandle::default(); FRAMES_IN_FLIGHT];
        for buffer in &mut instances {
            *buffer = self.create_buffer(&BufferDesc {
                name: "tlas instances",
                size: u64::from(max_instances) * INSTANCE_BYTES,
                cpu_writable: true,
            })?;
            ensure!(
                self.buffers[*buffer]
                    .address
                    .is_multiple_of(INSTANCE_ALIGNMENT),
                "the address of the TLAS instance buffer is not a multiple of {INSTANCE_ALIGNMENT}"
            );
        }

        // The size depends on the instance count only, so the address can be empty here.
        let geometries = [instances_geometry(0)];
        let mut info = tlas_build_info(&geometries);
        let (raw, _storage, scratch) = self.create_accel("tlas", &mut info, &[max_instances])?;
        self.bindless
            .write_acceleration_structure(&self.ctx.device, raw);
        Ok(self.tlases.insert(Tlas {
            raw,
            scratch,
            instances,
            max_instances,
        }))
    }

    /// Records a full build of the TLAS from `instances`. Call in a frame, before the
    /// shaders that trace rays. Compute shaders recorded after this call see the new
    /// content; the barriers for that are part of the call.
    pub fn build_tlas(&mut self, tlas: TlasHandle, instances: &[TlasInstance]) -> Result<()> {
        ensure!(self.active.is_some(), "build_tlas outside of a frame");
        let tlas = *self.tlases.get(tlas).context("stale TLAS handle")?;
        ensure!(
            instances.len() <= tlas.max_instances as usize,
            "{} instances, but the TLAS has room for {}",
            instances.len(),
            tlas.max_instances
        );

        let mut raw = Vec::with_capacity(instances.len());
        for instance in instances {
            let blas = self
                .blases
                .get(instance.blas)
                .context("stale BLAS handle")?;
            ensure!(
                instance.custom_index < 1 << 24,
                "the custom index of an instance has 24 bits"
            );
            raw.push(vk::AccelerationStructureInstanceKHR {
                transform: vk::TransformMatrixKHR {
                    matrix: instance.transform,
                },
                // Mask 0xFF: every ray sees the instance.
                instance_custom_index_and_mask: vk::Packed24_8::new(instance.custom_index, 0xFF),
                // No shader binding table (ray queries have none).
                instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                    0,
                    if instance.two_sided {
                        vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8
                    } else {
                        0
                    },
                ),
                acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                    device_handle: blas.address,
                },
            });
        }
        // SAFETY: the instance struct is `repr(C)` and has no padding bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(raw.as_ptr().cast::<u8>(), size_of_val(raw.as_slice()))
        };
        // `begin_frame` waited for the frame that used this slot before, so no GPU
        // work reads this buffer now.
        let buffer = &mut self.buffers[tlas.instances[self.slot]];
        buffer
            .allocation
            .mapped_slice_mut()
            .context("the TLAS instance buffer is not mapped")?[..bytes.len()]
            .copy_from_slice(bytes);

        let geometries = [instances_geometry(buffer.address)];
        let info = tlas_build_info(&geometries)
            .dst_acceleration_structure(tlas.raw)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: self.scratch_address(tlas.scratch),
            });
        let range =
            vk::AccelerationStructureBuildRangeInfoKHR::default().primitive_count(raw.len() as u32);

        use vk::AccessFlags2 as A;
        use vk::PipelineStageFlags2 as S;
        let accel_access = A::ACCELERATION_STRUCTURE_READ_KHR | A::ACCELERATION_STRUCTURE_WRITE_KHR;
        // The build waits for the ray queries and the build of the frame before: they
        // use the same TLAS and scratch buffer.
        let before = [vk::MemoryBarrier2::default()
            .src_stage_mask(S::COMPUTE_SHADER | S::ACCELERATION_STRUCTURE_BUILD_KHR)
            .src_access_mask(accel_access)
            .dst_stage_mask(S::ACCELERATION_STRUCTURE_BUILD_KHR)
            .dst_access_mask(accel_access)];
        // The ray queries of this frame wait for the build.
        let after = [vk::MemoryBarrier2::default()
            .src_stage_mask(S::ACCELERATION_STRUCTURE_BUILD_KHR)
            .src_access_mask(A::ACCELERATION_STRUCTURE_WRITE_KHR)
            .dst_stage_mask(S::COMPUTE_SHADER)
            .dst_access_mask(A::ACCELERATION_STRUCTURE_READ_KHR)];
        let device = &self.ctx.device;
        let cmd = self.frames[self.slot].cmd;
        unsafe {
            device.cmd_pipeline_barrier2(
                cmd,
                &vk::DependencyInfo::default().memory_barriers(&before),
            );
            self.accel
                .cmd_build_acceleration_structures(cmd, &[info], &[&[range]]);
            device
                .cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().memory_barriers(&after));
        }
        Ok(())
    }

    /// Makes an acceleration structure with room for `max_primitives` of each geometry
    /// in `info` (one count for each), and a scratch buffer (work memory) for its build. Sets the two as the
    /// targets of `info`. Returns the structure, its storage buffer, and the scratch buffer.
    fn create_accel(
        &mut self,
        name: &str,
        info: &mut vk::AccelerationStructureBuildGeometryInfoKHR<'_>,
        max_primitives: &[u32],
    ) -> Result<(vk::AccelerationStructureKHR, BufferHandle, BufferHandle)> {
        let mut sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            self.accel.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                info,
                max_primitives,
                &mut sizes,
            );
        }
        let storage = self.create_buffer(&BufferDesc {
            name,
            size: sizes.acceleration_structure_size,
            cpu_writable: false,
        })?;
        let scratch = self
            .create_buffer(&BufferDesc {
                name,
                // Room to move the start to the next multiple of the alignment.
                size: sizes.build_scratch_size + self.scratch_alignment,
                cpu_writable: false,
            })
            .inspect_err(|_| self.destroy_buffer(storage))?;
        let create_info = vk::AccelerationStructureCreateInfoKHR::default()
            .buffer(self.buffers[storage].raw)
            .size(sizes.acceleration_structure_size)
            .ty(info.ty);
        let raw = unsafe { self.accel.create_acceleration_structure(&create_info, None) }
            .inspect_err(|_| {
                self.destroy_buffer(storage);
                self.destroy_buffer(scratch);
            })?;
        info.dst_acceleration_structure = raw;
        info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: self.scratch_address(scratch),
        };
        Ok((raw, storage, scratch))
    }

    /// The first address in a scratch buffer that has the required alignment.
    fn scratch_address(&self, scratch: BufferHandle) -> u64 {
        self.buffers[scratch]
            .address
            .next_multiple_of(self.scratch_alignment)
    }

    /// Runs commands outside of the frame and waits until the GPU has finished them.
    // ponytail: the caller stops until the queue is idle. Record the commands into the
    // command buffer of the frame when an upload during play shows as a stall.
    pub(crate) fn run_now(&self, record: impl FnOnce(vk::CommandBuffer)) -> Result<()> {
        let device = &self.ctx.device;
        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::TRANSIENT)
            .queue_family_index(self.ctx.queue_family);
        let pool = unsafe { device.create_command_pool(&pool_info, None) }?;
        let run = || -> Result<()> {
            let cmd_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .command_buffer_count(1);
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            unsafe {
                let cmd = device.allocate_command_buffers(&cmd_info)?[0];
                device.begin_command_buffer(cmd, &begin_info)?;
                record(cmd);
                device.end_command_buffer(cmd)?;
                let commands = [vk::CommandBufferSubmitInfo::default().command_buffer(cmd)];
                let submit = vk::SubmitInfo2::default().command_buffer_infos(&commands);
                device.queue_submit2(self.ctx.queue, &[submit], vk::Fence::null())?;
                device.queue_wait_idle(self.ctx.queue)?;
            }
            Ok(())
        };
        let result = run();
        // The pool owns the command buffer.
        unsafe { device.destroy_command_pool(pool, None) };
        result
    }
}

/// The bytes of a slice of plain numbers.
fn as_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: the callers use `u32` and `[f32; 3]`, which have no padding bytes, and
    // each byte pattern is a valid `u8`.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), size_of_val(values)) }
}
