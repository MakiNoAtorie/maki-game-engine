//! Vulkan backend. The only crate that uses `ash`: no Vulkan type is in the public API.
//!
//! [`Gpu`] owns the device, the swapchain, and all resources. Users make resources from
//! description structs and hold generational handles to them.
//!
//! One frame:
//! 1. [`Gpu::begin_frame`]: waits for a free frame slot and gets a swapchain image.
//! 2. Draw calls ([`Gpu::image_barrier`], [`Gpu::build_tlas`], [`Gpu::dispatch_compute`],
//!    [`Gpu::begin_rendering`], [`Gpu::draw`], [`Gpu::blit_to_swapchain`]) record commands.
//! 3. [`Gpu::end_frame`]: submits the commands and presents the image.
//!
//! [`Gpu::write_timestamp`] between draw calls measures the GPU time of a part of the frame.
//!
//! The CPU records frame N+1 while the GPU still draws frame N (`FRAMES_IN_FLIGHT`).
//! For this reason a destroyed resource is not freed at once: see [`Gpu::destroy_buffer`].
//!
//! Synchronization of images is the caller's task: this crate records no image barrier
//! by itself. The caller tells each change of use with [`Gpu::image_barrier`] (the frame
//! graph of the render crate does this). The exception is [`Gpu::build_tlas`], which
//! orders itself against the compute shaders before and after it.

mod accel;
mod bindless;
mod device;
mod raster;
mod resources;
mod swapchain;

use std::io::Cursor;
use std::mem::ManuallyDrop;

use anyhow::{Context, Result, ensure};
use ash::{khr, vk};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{
    AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use slotmap::SlotMap;

use accel::{Blas, Tlas};
use bindless::{Bindless, PUSH_CONSTANT_BYTES, PUSH_STAGES};
use device::DeviceContext;
use resources::{Buffer, COLOR_RANGE, Garbage, Image, Sampler};
use swapchain::Swapchain;

pub use accel::{BlasDesc, BlasGeometry, TlasInstance};
pub use raster::GraphicsPipelineDesc;
pub use resources::{
    Access, BlasHandle, BufferDesc, BufferHandle, ComputePipelineHandle, Format,
    GraphicsPipelineHandle, ImageDesc, ImageHandle, ImageTarget, SamplerDesc, SamplerHandle,
    TlasHandle, mip_level_count,
};

/// How many frames the CPU can record ahead of the GPU. See [`Gpu::frame_slot`].
pub const FRAMES_IN_FLIGHT: usize = 2;

/// Limit of [`Gpu::write_timestamp`] calls in one frame.
const MAX_TIMESTAMPS: u32 = 64;

#[derive(Debug, Clone, Copy)]
pub struct GpuDesc {
    /// Window size in physical pixels.
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
}

/// Per-frame objects. With one set per frame in flight, the CPU can record
/// a frame while the GPU still reads the command buffer of the frame before.
struct FrameSlot {
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    /// Signaled when the acquired swapchain image is ready for use.
    image_available: vk::Semaphore,
    /// Signaled when the GPU has finished the commands of this slot.
    in_flight: vk::Fence,
    /// Resources freed while this slot was the newest one.
    garbage: Vec<Garbage>,
    /// The GPU writes the timestamps of the frame here. `MAX_TIMESTAMPS` entries.
    timestamps: vk::QueryPool,
    /// Number of timestamps that the last frame of this slot wrote.
    timestamp_count: u32,
}

/// State of the frame between `begin_frame` and `end_frame`.
struct ActiveFrame {
    image_index: u32,
    /// Size of the image that is drawn into, between `begin_rendering` and `end_rendering`.
    rendering: Option<vk::Extent2D>,
}

pub struct Gpu {
    ctx: DeviceContext,
    /// Dropped by hand in `Drop`, before the device.
    allocator: ManuallyDrop<Allocator>,
    swapchain: Swapchain,
    /// True: build the swapchain again before the next frame.
    swapchain_dirty: bool,
    /// See [`Gpu::frame_wait`]: the sum of the frame in work, and of the last frame.
    waiting: std::time::Duration,
    waited: std::time::Duration,
    window_size: (u32, u32),
    vsync: bool,
    bindless: Bindless,
    buffers: SlotMap<BufferHandle, Buffer>,
    images: SlotMap<ImageHandle, Image>,
    samplers: SlotMap<SamplerHandle, Sampler>,
    pipelines: SlotMap<ComputePipelineHandle, vk::Pipeline>,
    graphics_pipelines: SlotMap<GraphicsPipelineHandle, vk::Pipeline>,
    /// The functions of the acceleration structure extension.
    accel: khr::acceleration_structure::Device,
    /// The GPU needs the address of a scratch buffer to be a multiple of this.
    scratch_alignment: u64,
    blases: SlotMap<BlasHandle, Blas>,
    tlases: SlotMap<TlasHandle, Tlas>,
    frames: [FrameSlot; FRAMES_IN_FLIGHT],
    /// Index in `frames` of the current frame, or of the last one between frames.
    slot: usize,
    active: Option<ActiveFrame>,
    /// Nanoseconds in one tick of the GPU clock. `None`: the GPU has no timestamps.
    timestamp_period: Option<f64>,
    /// The bits of a timestamp value that are valid.
    timestamp_mask: u64,
    /// See [`Gpu::timestamps`].
    finished_timestamps: Vec<f64>,
}

impl Gpu {
    /// The window must stay open until the `Gpu` is dropped.
    /// Fails with the names of the missing features if no GPU is sufficient.
    pub fn new(window: &(impl HasWindowHandle + HasDisplayHandle), desc: &GpuDesc) -> Result<Self> {
        // Validation layers find API misuse but are slow, so debug builds only.
        let ctx = DeviceContext::new(window, cfg!(debug_assertions))?;
        let device = &ctx.device;

        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: ctx.instance.clone(),
            device: device.clone(),
            physical_device: ctx.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: true,
            allocation_sizes: Default::default(),
        })?;

        let mut accel_properties = vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
        {
            let mut properties =
                vk::PhysicalDeviceProperties2::default().push_next(&mut accel_properties);
            unsafe {
                ctx.instance
                    .get_physical_device_properties2(ctx.physical_device, &mut properties)
            };
        }

        // The queue family tells how many bits of a timestamp are valid. 0: no timestamps.
        let families = unsafe {
            ctx.instance
                .get_physical_device_queue_family_properties(ctx.physical_device)
        };
        let timestamp_bits = families[ctx.queue_family as usize].timestamp_valid_bits;
        let limits = unsafe {
            ctx.instance
                .get_physical_device_properties(ctx.physical_device)
        }
        .limits;
        let timestamp_period = (timestamp_bits > 0).then_some(f64::from(limits.timestamp_period));

        let mut frames = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            // One pool per slot: a pool reset frees all commands of the slot at once.
            let pool_info = vk::CommandPoolCreateInfo::default()
                .flags(vk::CommandPoolCreateFlags::TRANSIENT)
                .queue_family_index(ctx.queue_family);
            let pool = unsafe { device.create_command_pool(&pool_info, None) }?;
            let cmd_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .command_buffer_count(1);
            // Signaled at start: the first `begin_frame` must not wait.
            let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
            let query_info = vk::QueryPoolCreateInfo::default()
                .query_type(vk::QueryType::TIMESTAMP)
                .query_count(MAX_TIMESTAMPS);
            frames.push(FrameSlot {
                pool,
                cmd: unsafe { device.allocate_command_buffers(&cmd_info) }?[0],
                image_available: unsafe { device.create_semaphore(&Default::default(), None) }?,
                in_flight: unsafe { device.create_fence(&fence_info, None) }?,
                garbage: Vec::new(),
                timestamps: unsafe { device.create_query_pool(&query_info, None) }?,
                timestamp_count: 0,
            });
        }

        Ok(Self {
            bindless: Bindless::new(device)?,
            swapchain: Swapchain::new(&ctx),
            // The first `begin_frame` builds the swapchain.
            swapchain_dirty: true,
            window_size: (desc.width, desc.height),
            vsync: desc.vsync,
            allocator: ManuallyDrop::new(allocator),
            buffers: SlotMap::with_key(),
            images: SlotMap::with_key(),
            samplers: SlotMap::with_key(),
            pipelines: SlotMap::with_key(),
            graphics_pipelines: SlotMap::with_key(),
            accel: khr::acceleration_structure::Device::new(&ctx.instance, device),
            scratch_alignment: u64::from(
                accel_properties.min_acceleration_structure_scratch_offset_alignment,
            )
            .max(1),
            blases: SlotMap::with_key(),
            tlases: SlotMap::with_key(),
            frames: frames
                .try_into()
                .ok()
                .expect("the loop makes FRAMES_IN_FLIGHT slots"),
            slot: 0,
            active: None,
            timestamp_period,
            // All bits for 64 valid bits. The shift is not defined for 64.
            timestamp_mask: u64::MAX >> (64 - timestamp_bits.clamp(1, 64)),
            finished_timestamps: Vec::new(),
            waiting: std::time::Duration::ZERO,
            waited: std::time::Duration::ZERO,
            ctx,
        })
    }

    // ---- Resources ----

    pub fn create_buffer(&mut self, desc: &BufferDesc) -> Result<BufferHandle> {
        ensure!(desc.size > 0, "buffer `{}` has size 0", desc.name);
        let mut buffer = Buffer {
            raw: vk::Buffer::null(),
            allocation: Default::default(),
            size: desc.size,
            address: 0,
            bindless_index: self.bindless.buffer_slots.alloc()?,
        };
        // On an error, `destroy_now` frees the parts that exist. Vulkan and the
        // allocator ignore the parts that are still null.
        if let Err(error) = self.init_buffer(&mut buffer, desc) {
            self.destroy_now(Garbage::Buffer(buffer));
            return Err(error);
        }
        self.bindless
            .write_buffer(&self.ctx.device, buffer.bindless_index, buffer.raw);
        Ok(self.buffers.insert(buffer))
    }

    fn init_buffer(&mut self, buffer: &mut Buffer, desc: &BufferDesc) -> Result<()> {
        let device = &self.ctx.device;
        let info = vk::BufferCreateInfo::default().size(desc.size).usage(
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
        );
        buffer.raw = unsafe { device.create_buffer(&info, None) }?;
        buffer.allocation = self.allocator.allocate(&AllocationCreateDesc {
            name: desc.name,
            requirements: unsafe { device.get_buffer_memory_requirements(buffer.raw) },
            // CpuToGpu memory stays mapped, so `write_buffer` can copy into it.
            location: if desc.cpu_writable {
                MemoryLocation::CpuToGpu
            } else {
                MemoryLocation::GpuOnly
            },
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        let allocation = &buffer.allocation;
        unsafe { device.bind_buffer_memory(buffer.raw, allocation.memory(), allocation.offset()) }?;
        // Valid only after the memory is bound.
        let address_info = vk::BufferDeviceAddressInfo::default().buffer(buffer.raw);
        buffer.address = unsafe { device.get_buffer_device_address(&address_info) };
        Ok(())
    }

    /// Copies `data` into a `cpu_writable` buffer. The copy is immediate: a frame
    /// in flight that reads the same bytes can see the old or the new data.
    pub fn write_buffer(&mut self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<()> {
        let buffer = self
            .buffers
            .get_mut(buffer)
            .context("stale buffer handle")?;
        let start = usize::try_from(offset)?;
        let end = start
            .checked_add(data.len())
            .filter(|end| *end as u64 <= buffer.size)
            .with_context(|| format!("write past the end of a buffer of {} bytes", buffer.size))?;
        let mapped = buffer
            .allocation
            .mapped_slice_mut()
            .context("buffer is not cpu_writable")?;
        mapped[start..end].copy_from_slice(data);
        Ok(())
    }

    /// The image is in the storage and/or sampled image array at one index.
    /// The content is undefined: the first [`Gpu::image_barrier`] has `from: None`.
    pub fn create_image(&mut self, desc: &ImageDesc) -> Result<ImageHandle> {
        ensure!(
            desc.width > 0 && desc.height > 0,
            "image `{}` has no area",
            desc.name
        );
        ensure!(
            (1..=mip_level_count(desc.width, desc.height)).contains(&desc.mip_levels),
            "image `{}` of {} x {} cannot have {} mip levels",
            desc.name,
            desc.width,
            desc.height,
            desc.mip_levels
        );
        let mut image = Image {
            raw: vk::Image::null(),
            view: vk::ImageView::null(),
            allocation: Default::default(),
            extent: vk::Extent2D {
                width: desc.width,
                height: desc.height,
            },
            texel_bytes: desc.format.texel_bytes(),
            mip_levels: desc.mip_levels,
            bindless_index: self.bindless.image_slots.alloc()?,
        };
        // Same error handling as `create_buffer`.
        if let Err(error) = self.init_image(&mut image, desc) {
            self.destroy_now(Garbage::Image(image));
            return Err(error);
        }
        self.bindless.write_image(
            &self.ctx.device,
            image.bindless_index,
            image.view,
            desc.storage,
            desc.sampled,
        );
        Ok(self.images.insert(image))
    }

    fn init_image(&mut self, image: &mut Image, desc: &ImageDesc) -> Result<()> {
        let device = &self.ctx.device;
        let format = desc.format.to_vk();
        let mut usage = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
        if desc.storage {
            usage |= vk::ImageUsageFlags::STORAGE;
        }
        if desc.sampled {
            usage |= vk::ImageUsageFlags::SAMPLED;
        }
        if desc.color_attachment {
            usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
        }
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(image.extent.into())
            .mip_levels(desc.mip_levels)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .usage(usage);
        image.raw = unsafe { device.create_image(&info, None) }?;
        image.allocation = self.allocator.allocate(&AllocationCreateDesc {
            name: desc.name,
            requirements: unsafe { device.get_image_memory_requirements(image.raw) },
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        let allocation = &image.allocation;
        unsafe { device.bind_image_memory(image.raw, allocation.memory(), allocation.offset()) }?;

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image.raw)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(COLOR_RANGE);
        image.view = unsafe { device.create_image_view(&view_info, None) }?;
        Ok(())
    }

    pub fn create_sampler(&mut self, desc: &SamplerDesc) -> Result<SamplerHandle> {
        let bindless_index = self.bindless.sampler_slots.alloc()?;
        let filter = if desc.linear {
            vk::Filter::LINEAR
        } else {
            vk::Filter::NEAREST
        };
        let address_mode = if desc.repeat {
            vk::SamplerAddressMode::REPEAT
        } else {
            vk::SamplerAddressMode::CLAMP_TO_EDGE
        };
        // With mip levels, a texture that shows small is always blended: see `SamplerDesc`.
        let (min_filter, mipmap_mode, max_lod) = if desc.mipmaps {
            (
                vk::Filter::LINEAR,
                vk::SamplerMipmapMode::LINEAR,
                vk::LOD_CLAMP_NONE,
            )
        } else {
            (filter, vk::SamplerMipmapMode::NEAREST, 0.0)
        };
        let info = vk::SamplerCreateInfo::default()
            .mag_filter(filter)
            .min_filter(min_filter)
            .mipmap_mode(mipmap_mode)
            .max_lod(max_lod)
            .address_mode_u(address_mode)
            .address_mode_v(address_mode)
            .address_mode_w(address_mode);
        let raw = match unsafe { self.ctx.device.create_sampler(&info, None) } {
            Ok(raw) => raw,
            Err(error) => {
                self.bindless.sampler_slots.free(bindless_index);
                return Err(error.into());
            }
        };
        self.bindless
            .write_sampler(&self.ctx.device, bindless_index, raw);
        Ok(self.samplers.insert(Sampler {
            raw,
            bindless_index,
        }))
    }

    /// Fills a new image, outside of the frame: `levels[i]` holds the pixels of mip level
    /// `i`, row by row, in the format of the image. The image is then in
    /// [`Access::Sampled`]. For a texture, which is written one time.
    /// It waits for the GPU, as [`Gpu::create_blas`] does.
    pub fn upload_image(&mut self, image: ImageHandle, levels: &[&[u8]]) -> Result<()> {
        let target = self.images.get(image).context("stale image handle")?;
        ensure!(
            levels.len() == target.mip_levels as usize,
            "upload_image: {} levels of data for an image of {} levels",
            levels.len(),
            target.mip_levels
        );
        let (raw, extent, texel_bytes) = (target.raw, target.extent, target.texel_bytes);
        // One copy for each level, all from one staging buffer.
        let mut regions = Vec::with_capacity(levels.len());
        let mut offset = 0;
        for (level, data) in (0u32..).zip(levels) {
            let (width, height) = (
                (extent.width >> level).max(1),
                (extent.height >> level).max(1),
            );
            let bytes = u64::from(width) * u64::from(height) * u64::from(texel_bytes);
            ensure!(
                data.len() as u64 == bytes,
                "upload_image: {} bytes of data for level {level} of {bytes} bytes",
                data.len()
            );
            regions.push(
                vk::BufferImageCopy::default()
                    .buffer_offset(offset)
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: level,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    }),
            );
            offset += bytes;
        }
        let staging = self.create_buffer(&BufferDesc {
            name: "image upload",
            size: offset,
            cpu_writable: true,
        })?;
        let written = self.write_buffer(staging, 0, &levels.concat());
        let source = self.buffers[staging].raw;
        let (sampled_layout, sampled_stage, sampled_access) = Access::Sampled.to_vk();
        let barrier = |old_layout,
                       new_layout,
                       stages: [vk::PipelineStageFlags2; 2],
                       access: [vk::AccessFlags2; 2]| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(stages[0])
                .src_access_mask(access[0])
                .dst_stage_mask(stages[1])
                .dst_access_mask(access[1])
                .old_layout(old_layout)
                .new_layout(new_layout)
                .image(raw)
                .subresource_range(COLOR_RANGE)
        };
        use vk::AccessFlags2 as A;
        use vk::ImageLayout as L;
        use vk::PipelineStageFlags2 as S;
        let to_copy = [barrier(
            L::UNDEFINED,
            L::TRANSFER_DST_OPTIMAL,
            [S::NONE, S::ALL_TRANSFER],
            [A::NONE, A::TRANSFER_WRITE],
        )];
        let to_sample = [barrier(
            L::TRANSFER_DST_OPTIMAL,
            sampled_layout,
            [S::ALL_TRANSFER, sampled_stage],
            [A::TRANSFER_WRITE, sampled_access],
        )];
        let uploaded = written.and_then(|()| {
            self.run_now(|cmd| unsafe {
                let device = &self.ctx.device;
                device.cmd_pipeline_barrier2(
                    cmd,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_copy),
                );
                device.cmd_copy_buffer_to_image(
                    cmd,
                    source,
                    raw,
                    L::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
                device.cmd_pipeline_barrier2(
                    cmd,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_sample),
                );
            })
        });
        // `run_now` waited for the copy: no GPU work reads the buffer.
        if let Some(buffer) = self.buffers.remove(staging) {
            self.destroy_now(Garbage::Buffer(buffer));
        }
        uploaded
    }

    /// Makes a compute pipeline from SPIR-V bytes. The entry point is `main`.
    /// The shader uses the bindless set (set 0) and up to 128 bytes of push constants.
    pub fn create_compute_pipeline(&mut self, spirv: &[u8]) -> Result<ComputePipelineHandle> {
        let device = &self.ctx.device;
        let module = shader_module(device, spirv)?;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(c"main");
        let info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(self.bindless.pipeline_layout);
        let result =
            unsafe { device.create_compute_pipelines(vk::PipelineCache::null(), &[info], None) };
        // The pipeline keeps its own copy of the shader code.
        unsafe { device.destroy_shader_module(module, None) };
        let pipeline = result.map_err(|(_, error)| error)?[0];
        Ok(self.pipelines.insert(pipeline))
    }

    /// The index of the buffer in the bindless storage buffer array.
    /// Stable until the buffer is destroyed.
    pub fn buffer_index(&self, buffer: BufferHandle) -> Option<u32> {
        self.buffers.get(buffer).map(|b| b.bindless_index)
    }

    /// The address of the first byte of the buffer in GPU memory. A shader that gets
    /// this number (in another buffer or a push constant) reads the buffer through a
    /// GLSL `buffer_reference` block, with no descriptor.
    /// Stable until the buffer is destroyed.
    pub fn buffer_address(&self, buffer: BufferHandle) -> Option<u64> {
        self.buffers.get(buffer).map(|b| b.address)
    }

    /// The index of the image in the bindless storage and sampled image arrays.
    pub fn image_index(&self, image: ImageHandle) -> Option<u32> {
        self.images.get(image).map(|i| i.bindless_index)
    }

    /// The index of the sampler in the bindless sampler array.
    pub fn sampler_index(&self, sampler: SamplerHandle) -> Option<u32> {
        self.samplers.get(sampler).map(|s| s.bindless_index)
    }

    /// The index of the frame slot that the current frame uses, in `0..FRAMES_IN_FLIGHT`.
    /// When a frame starts, the GPU has finished the frame before it that used the same
    /// slot. A caller that writes a buffer in each frame keeps one buffer for each slot:
    /// the GPU then never reads a buffer while the CPU writes it.
    pub fn frame_slot(&self) -> usize {
        self.slot
    }

    /// Bytes of GPU memory that the buffers, images, and acceleration structures of
    /// this `Gpu` use now. A resource that waits for its deferred destruction counts.
    pub fn allocated_bytes(&self) -> u64 {
        self.allocator.generate_report().total_allocated_bytes
    }

    /// Deferred destruction: the handle is invalid at once, but the GPU object stays
    /// until the GPU has finished every frame that could use it. Its bindless index
    /// is free for a new resource only after that.
    /// A stale handle does nothing. The same applies to the other `destroy_*` functions.
    pub fn destroy_buffer(&mut self, buffer: BufferHandle) {
        if let Some(buffer) = self.buffers.remove(buffer) {
            self.defer(Garbage::Buffer(buffer));
        }
    }

    pub fn destroy_image(&mut self, image: ImageHandle) {
        if let Some(image) = self.images.remove(image) {
            self.defer(Garbage::Image(image));
        }
    }

    pub fn destroy_sampler(&mut self, sampler: SamplerHandle) {
        if let Some(sampler) = self.samplers.remove(sampler) {
            self.defer(Garbage::Sampler(sampler));
        }
    }

    pub fn destroy_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        if let Some(pipeline) = self.pipelines.remove(pipeline) {
            self.defer(Garbage::Pipeline(pipeline));
        }
    }

    /// Only the frame of the current slot and the frames before it can use the resource.
    /// The GPU runs frames in order, so when the fence of this slot is signaled the next
    /// time (`begin_frame` waits for it), all those frames are done.
    fn defer(&mut self, garbage: Garbage) {
        self.frames[self.slot].garbage.push(garbage);
    }

    /// The GPU must not use the resource.
    fn destroy_now(&mut self, garbage: Garbage) {
        let device = &self.ctx.device;
        let mut free = |allocation| {
            if let Err(error) = self.allocator.free(allocation) {
                tracing::error!(%error, "cannot free GPU memory");
            }
        };
        unsafe {
            match garbage {
                Garbage::Buffer(buffer) => {
                    device.destroy_buffer(buffer.raw, None);
                    free(buffer.allocation);
                    self.bindless.buffer_slots.free(buffer.bindless_index);
                }
                Garbage::Image(image) => {
                    device.destroy_image_view(image.view, None);
                    device.destroy_image(image.raw, None);
                    free(image.allocation);
                    self.bindless.image_slots.free(image.bindless_index);
                }
                Garbage::Sampler(sampler) => {
                    device.destroy_sampler(sampler.raw, None);
                    self.bindless.sampler_slots.free(sampler.bindless_index);
                }
                Garbage::Pipeline(pipeline) => device.destroy_pipeline(pipeline, None),
                Garbage::Accel(accel) => self.accel.destroy_acceleration_structure(accel, None),
            }
        }
    }

    // ---- Frame ----

    /// Size of the swapchain images in pixels. Valid during a frame.
    pub fn swapchain_extent(&self) -> (u32, u32) {
        (self.swapchain.extent.width, self.swapchain.extent.height)
    }

    /// Tells the new window size. The swapchain is built again before the next frame.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.window_size != (width, height) {
            self.window_size = (width, height);
            self.swapchain_dirty = true;
        }
    }

    /// Sets vsync on or off. The swapchain is built again before the next frame.
    pub fn set_vsync(&mut self, vsync: bool) {
        if self.vsync != vsync {
            self.vsync = vsync;
            self.swapchain_dirty = true;
        }
    }

    /// The time that the CPU waited in the last complete frame, in `begin_frame` and
    /// `end_frame`: for the GPU to finish an older frame, and for the display (vsync).
    /// A frame time without this is the time of the CPU work alone.
    pub fn frame_wait(&self) -> std::time::Duration {
        self.waited
    }

    /// True between a `begin_frame` that returned true and `end_frame`.
    pub fn frame_active(&self) -> bool {
        self.active.is_some()
    }

    /// Starts a frame. Returns false if no frame can be drawn now (the swapchain
    /// must be built again, or the window has no area): skip the draw calls and `end_frame`.
    pub fn begin_frame(&mut self) -> Result<bool> {
        assert!(self.active.is_none(), "begin_frame without end_frame");
        let device = &self.ctx.device;

        // Wait until the GPU has finished the frame that used this slot before.
        self.slot = (self.slot + 1) % FRAMES_IN_FLIGHT;
        let wait_start = std::time::Instant::now();
        unsafe { device.wait_for_fences(&[self.frames[self.slot].in_flight], true, u64::MAX) }?;
        self.waiting = wait_start.elapsed();
        for garbage in std::mem::take(&mut self.frames[self.slot].garbage) {
            self.destroy_now(garbage);
        }
        self.read_timestamps()?;

        let device = &self.ctx.device;
        if self.swapchain_dirty {
            // ponytail: a full stop of the GPU on each resize step. Sufficient while
            // a resize is rare; defer the old swapchain as garbage if the stall shows.
            unsafe { device.device_wait_idle() }?;
            let (width, height) = self.window_size;
            if !self
                .swapchain
                .rebuild(&self.ctx, width, height, self.vsync)?
            {
                return Ok(false);
            }
            self.swapchain_dirty = false;
        }

        let frame = &self.frames[self.slot];
        let wait_start = std::time::Instant::now();
        let acquired = unsafe {
            self.swapchain.loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
        };
        self.waiting += wait_start.elapsed();
        let image_index = match acquired {
            // Suboptimal: the image is usable but no longer matches the window.
            Ok((index, suboptimal)) => {
                self.swapchain_dirty |= suboptimal;
                index
            }
            // The window changed and the swapchain cannot be used. Skip this frame.
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.swapchain_dirty = true;
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        };

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            device.reset_fences(&[frame.in_flight])?;
            device.reset_command_pool(frame.pool, vk::CommandPoolResetFlags::empty())?;
            device.begin_command_buffer(frame.cmd, &begin_info)?;
            // A query must be reset before each use.
            device.cmd_reset_query_pool(frame.cmd, frame.timestamps, 0, MAX_TIMESTAMPS);
        }
        self.active = Some(ActiveFrame {
            image_index,
            rendering: None,
        });
        Ok(true)
    }

    /// Reads the timestamps of the frame that used the current slot before. `begin_frame`
    /// waited for that frame, so the values are complete.
    fn read_timestamps(&mut self) -> Result<()> {
        self.finished_timestamps.clear();
        let frame = &mut self.frames[self.slot];
        let count = std::mem::take(&mut frame.timestamp_count);
        let Some(period) = self.timestamp_period.filter(|_| count > 0) else {
            return Ok(());
        };
        let mut ticks = vec![0_u64; count as usize];
        unsafe {
            self.ctx.device.get_query_pool_results(
                frame.timestamps,
                0,
                &mut ticks,
                vk::QueryResultFlags::TYPE_64,
            )
        }?;
        // The mask makes the difference correct also when the GPU clock went past
        // its last valid bit between the two values.
        let mask = self.timestamp_mask;
        let ms = |tick: &u64| (tick.wrapping_sub(ticks[0]) & mask) as f64 * period / 1e6;
        self.finished_timestamps.extend(ticks.iter().map(ms));
        Ok(())
    }

    /// Records the time of the GPU clock at this point of the frame: the GPU writes it
    /// when it has finished all commands before this call. Read the values with
    /// [`Gpu::timestamps`] some frames later. Does nothing on a GPU without timestamps.
    /// Not permitted between `begin_rendering` and `end_rendering`.
    pub fn write_timestamp(&mut self) -> Result<()> {
        let active = self
            .active
            .as_ref()
            .context("write_timestamp outside of a frame")?;
        ensure!(
            active.rendering.is_none(),
            "write_timestamp between begin_rendering and end_rendering"
        );
        if self.timestamp_period.is_none() {
            return Ok(());
        }
        let frame = &mut self.frames[self.slot];
        ensure!(
            frame.timestamp_count < MAX_TIMESTAMPS,
            "more than {MAX_TIMESTAMPS} timestamps in one frame"
        );
        unsafe {
            self.ctx.device.cmd_write_timestamp2(
                frame.cmd,
                vk::PipelineStageFlags2::ALL_COMMANDS,
                frame.timestamps,
                frame.timestamp_count,
            );
        }
        frame.timestamp_count += 1;
        Ok(())
    }

    /// The timestamps of the newest frame that the GPU has finished in the current frame
    /// slot: one value for each [`Gpu::write_timestamp`] call of that frame, in
    /// milliseconds from its first timestamp. That frame is `FRAMES_IN_FLIGHT` frames
    /// older than the current one if no frame was skipped. Empty if it wrote none.
    /// Valid from `begin_frame` to the next `begin_frame`.
    pub fn timestamps(&self) -> &[f64] {
        &self.finished_timestamps
    }

    /// Records a change of use of an image: later commands that use the image as `to`
    /// wait for the earlier commands that used it as `from`, and the image layout changes
    /// if the two uses need different layouts.
    /// `from: None` means that the old content is not necessary (a new image, or the
    /// swapchain image at the start of a frame).
    pub fn image_barrier(
        &mut self,
        target: ImageTarget,
        from: Option<Access>,
        to: Access,
    ) -> Result<()> {
        let active = self
            .active
            .as_ref()
            .context("image_barrier outside of a frame")?;
        let image = match target {
            ImageTarget::Image(handle) => {
                self.images.get(handle).context("stale image handle")?.raw
            }
            ImageTarget::Swapchain => self.swapchain.images[active.image_index as usize],
        };
        // No earlier use: no stage to wait for, and UNDEFINED lets the driver drop the content.
        let (old_layout, src_stage, src_access) = from.map_or(
            (
                vk::ImageLayout::UNDEFINED,
                vk::PipelineStageFlags2::NONE,
                vk::AccessFlags2::NONE,
            ),
            Access::to_vk,
        );
        let (new_layout, dst_stage, dst_access) = to.to_vk();
        let barriers = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(src_stage)
            .src_access_mask(src_access)
            .dst_stage_mask(dst_stage)
            .dst_access_mask(dst_access)
            .old_layout(old_layout)
            .new_layout(new_layout)
            .image(image)
            .subresource_range(COLOR_RANGE)];
        unsafe {
            self.ctx.device.cmd_pipeline_barrier2(
                self.frames[self.slot].cmd,
                &vk::DependencyInfo::default().image_memory_barriers(&barriers),
            );
        }
        Ok(())
    }

    /// Runs a compute shader. `push_constants` is the shader's push constant block as bytes.
    /// Each image that the shader uses must be in the matching [`Access`] state.
    pub fn dispatch_compute(
        &mut self,
        pipeline: ComputePipelineHandle,
        push_constants: &[u8],
        groups: [u32; 3],
    ) -> Result<()> {
        ensure!(self.active.is_some(), "dispatch_compute outside of a frame");
        check_push_constants(push_constants)?;
        let pipeline = *self
            .pipelines
            .get(pipeline)
            .context("stale compute pipeline handle")?;

        let device = &self.ctx.device;
        let cmd = self.frames[self.slot].cmd;
        let layout = self.bindless.pipeline_layout;
        let compute = vk::PipelineBindPoint::COMPUTE;
        unsafe {
            device.cmd_bind_pipeline(cmd, compute, pipeline);
            device.cmd_bind_descriptor_sets(cmd, compute, layout, 0, &[self.bindless.set], &[]);
            if !push_constants.is_empty() {
                device.cmd_push_constants(cmd, layout, PUSH_STAGES, 0, push_constants);
            }
            device.cmd_dispatch(cmd, groups[0], groups[1], groups[2]);
        }
        Ok(())
    }

    /// Copies the image to the full window, scaled with a linear filter.
    /// The image must be in [`Access::TransferSrc`], the swapchain in [`Access::TransferDst`].
    pub fn blit_to_swapchain(&mut self, image: ImageHandle) -> Result<()> {
        let active = self
            .active
            .as_ref()
            .context("blit_to_swapchain outside of a frame")?;
        let source = self.images.get(image).context("stale image handle")?;
        let device = &self.ctx.device;
        let cmd = self.frames[self.slot].cmd;
        let target = self.swapchain.images[active.image_index as usize];

        let layers = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        // A blit region is the box between two corners.
        let corners = |extent: vk::Extent2D| {
            [
                vk::Offset3D::default(),
                vk::Offset3D {
                    x: extent.width as i32,
                    y: extent.height as i32,
                    z: 1,
                },
            ]
        };
        let region = vk::ImageBlit::default()
            .src_subresource(layers)
            .src_offsets(corners(source.extent))
            .dst_subresource(layers)
            .dst_offsets(corners(self.swapchain.extent));
        unsafe {
            device.cmd_blit_image(
                cmd,
                source.raw,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
                vk::Filter::LINEAR,
            );
        }
        Ok(())
    }

    /// Makes `target` black and copies `source` into it, not scaled, with its top left
    /// corner `left` pixels from the left edge. A source that does not fit is cut at
    /// the right and at the bottom.
    /// `source` must be in [`Access::TransferSrc`], `target` in [`Access::TransferDst`].
    pub fn copy_into(&mut self, source: ImageHandle, target: ImageHandle, left: u32) -> Result<()> {
        ensure!(self.active.is_some(), "copy_into outside of a frame");
        let image = |handle| self.images.get(handle).context("stale image handle");
        let (source_raw, source_extent) = image(source).map(|image| (image.raw, image.extent))?;
        let (target_raw, target_extent) = image(target).map(|image| (image.raw, image.extent))?;
        let cmd = self.frames[self.slot].cmd;
        unsafe {
            self.ctx.device.cmd_clear_color_image(
                cmd,
                target_raw,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 1.0],
                },
                &[COLOR_RANGE],
            );
        }
        // Two writes to one image: the copy must wait for the clear.
        let transfer = Some(Access::TransferDst);
        self.image_barrier(ImageTarget::Image(target), transfer, Access::TransferDst)?;

        let layers = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        let width = source_extent
            .width
            .min(target_extent.width.saturating_sub(left));
        let height = source_extent.height.min(target_extent.height);
        // A target that is narrower than `left`: all of the source is cut.
        if width == 0 {
            return Ok(());
        }
        // A box of the same size in the two images: the blit does not scale.
        let corners = |x: u32| {
            [
                vk::Offset3D {
                    x: x as i32,
                    y: 0,
                    z: 0,
                },
                vk::Offset3D {
                    x: (x + width) as i32,
                    y: height as i32,
                    z: 1,
                },
            ]
        };
        let region = vk::ImageBlit::default()
            .src_subresource(layers)
            .src_offsets(corners(0))
            .dst_subresource(layers)
            .dst_offsets(corners(left));
        unsafe {
            self.ctx.device.cmd_blit_image(
                cmd,
                source_raw,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target_raw,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
                vk::Filter::NEAREST,
            );
        }
        Ok(())
    }

    /// Copies the content of an image to the CPU: the pixels row by row, in the format
    /// of the image. `state` is the last use of the image; the image is in that state
    /// again on return. Call between frames. Waits for the GPU, so it is for tools
    /// (benchmark dumps), not for a frame.
    pub fn read_image(&mut self, image: ImageHandle, state: Access) -> Result<Vec<u8>> {
        ensure!(self.active.is_none(), "read_image in a frame");
        let source = self.images.get(image).context("stale image handle")?;
        let (raw, extent) = (source.raw, source.extent);
        let size =
            u64::from(extent.width) * u64::from(extent.height) * u64::from(source.texel_bytes);
        let staging = self.create_buffer(&BufferDesc {
            name: "image readback",
            size,
            cpu_writable: true,
        })?;
        let buffer = self.buffers[staging].raw;

        let (layout, stage, access) = state.to_vk();
        let (copy_layout, copy_stage, copy_access) = Access::TransferSrc.to_vk();
        let barrier = |from: (vk::ImageLayout, _, _), to: (vk::ImageLayout, _, _)| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(from.1)
                .src_access_mask(from.2)
                .dst_stage_mask(to.1)
                .dst_access_mask(to.2)
                .old_layout(from.0)
                .new_layout(to.0)
                .image(raw)
                .subresource_range(COLOR_RANGE)
        };
        let to_copy = [barrier(
            (layout, stage, access),
            (copy_layout, copy_stage, copy_access),
        )];
        let back = [barrier(
            (copy_layout, copy_stage, copy_access),
            (layout, stage, access),
        )];
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(extent.into());
        let device = &self.ctx.device;
        let copied = self.run_now(|cmd| unsafe {
            device.cmd_pipeline_barrier2(
                cmd,
                &vk::DependencyInfo::default().image_memory_barriers(&to_copy),
            );
            device.cmd_copy_image_to_buffer(cmd, raw, copy_layout, buffer, &[region]);
            device.cmd_pipeline_barrier2(
                cmd,
                &vk::DependencyInfo::default().image_memory_barriers(&back),
            );
        });
        let bytes = copied.and_then(|()| {
            let mapped = self.buffers[staging].allocation.mapped_slice();
            Ok(mapped.context("the readback buffer is not mapped")?[..size as usize].to_vec())
        });
        self.destroy_buffer(staging);
        bytes
    }

    /// Submits the commands of the frame and shows the swapchain image.
    /// The swapchain image must be in [`Access::Present`].
    pub fn end_frame(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .context("end_frame without begin_frame")?;
        let device = &self.ctx.device;
        let frame = &self.frames[self.slot];
        let image = active.image_index as usize;

        unsafe { device.end_command_buffer(frame.cmd) }?;

        // ALL_COMMANDS: no command of the frame starts before the swapchain image is
        // available, so the first barrier on that image needs no source stage.
        let wait = [vk::SemaphoreSubmitInfo::default()
            .semaphore(frame.image_available)
            .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
        let signal = [vk::SemaphoreSubmitInfo::default()
            .semaphore(self.swapchain.render_finished[image])
            .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
        let commands = [vk::CommandBufferSubmitInfo::default().command_buffer(frame.cmd)];
        let submit = vk::SubmitInfo2::default()
            .wait_semaphore_infos(&wait)
            .command_buffer_infos(&commands)
            .signal_semaphore_infos(&signal);
        unsafe { device.queue_submit2(self.ctx.queue, &[submit], frame.in_flight) }?;

        let present_wait = [self.swapchain.render_finished[image]];
        let swapchains = [self.swapchain.handle];
        let indices = [active.image_index];
        let present = vk::PresentInfoKHR::default()
            .wait_semaphores(&present_wait)
            .swapchains(&swapchains)
            .image_indices(&indices);
        let wait_start = std::time::Instant::now();
        let presented = unsafe {
            self.swapchain
                .loader
                .queue_present(self.ctx.queue, &present)
        };
        self.waited = self.waiting + wait_start.elapsed();
        match presented {
            Ok(suboptimal) => self.swapchain_dirty |= suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.swapchain_dirty = true,
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

/// Makes a shader module from SPIR-V bytes. The caller destroys it.
fn shader_module(device: &ash::Device, spirv: &[u8]) -> Result<vk::ShaderModule> {
    // `read_spv` checks the size and copies the bytes into aligned words.
    let code = ash::util::read_spv(&mut Cursor::new(spirv)).context("invalid SPIR-V")?;
    let info = vk::ShaderModuleCreateInfo::default().code(&code);
    Ok(unsafe { device.create_shader_module(&info, None) }?)
}

fn check_push_constants(push_constants: &[u8]) -> Result<()> {
    ensure!(
        push_constants.len() <= PUSH_CONSTANT_BYTES as usize
            && push_constants.len().is_multiple_of(4),
        "push constants must be a multiple of 4 bytes and at most {PUSH_CONSTANT_BYTES} bytes"
    );
    Ok(())
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            // After this, nothing is in use and the destroy order below is free.
            if let Err(error) = self.ctx.device.device_wait_idle() {
                tracing::error!(%error, "device_wait_idle failed at shutdown");
            }

            let mut garbage: Vec<Garbage> = Vec::new();
            for frame in &mut self.frames {
                garbage.append(&mut frame.garbage);
            }
            // An acceleration structure goes before the buffer that stores it.
            garbage.extend(self.blases.drain().map(|(_, b)| Garbage::Accel(b.raw)));
            garbage.extend(self.tlases.drain().map(|(_, t)| Garbage::Accel(t.raw)));
            garbage.extend(self.buffers.drain().map(|(_, b)| Garbage::Buffer(b)));
            garbage.extend(self.images.drain().map(|(_, i)| Garbage::Image(i)));
            garbage.extend(self.samplers.drain().map(|(_, s)| Garbage::Sampler(s)));
            garbage.extend(self.pipelines.drain().map(|(_, p)| Garbage::Pipeline(p)));
            let graphics = self.graphics_pipelines.drain();
            garbage.extend(graphics.map(|(_, p)| Garbage::Pipeline(p)));
            for garbage in garbage {
                self.destroy_now(garbage);
            }

            let device = &self.ctx.device;
            for frame in &self.frames {
                // The pool owns the command buffer.
                device.destroy_command_pool(frame.pool, None);
                device.destroy_semaphore(frame.image_available, None);
                device.destroy_fence(frame.in_flight, None);
                device.destroy_query_pool(frame.timestamps, None);
            }
            self.bindless.destroy(device);
            self.swapchain.destroy(device);
            // The allocator frees its memory blocks, so it goes before the device.
            ManuallyDrop::drop(&mut self.allocator);
            self.ctx.destroy();
        }
    }
}
