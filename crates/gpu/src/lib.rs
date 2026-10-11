//! Vulkan backend. The only crate that uses `ash`: no Vulkan type is in the public API.
//!
//! [`Gpu`] owns the device, the swapchain, and all resources. Users make resources from
//! description structs and hold generational handles to them.
//!
//! One frame:
//! 1. [`Gpu::begin_frame`]: waits for a free frame slot and gets a swapchain image.
//! 2. Draw calls ([`Gpu::image_barrier`], [`Gpu::dispatch_compute`],
//!    [`Gpu::blit_to_swapchain`]) record commands.
//! 3. [`Gpu::end_frame`]: submits the commands and presents the image.
//!
//! The CPU records frame N+1 while the GPU still draws frame N (`FRAMES_IN_FLIGHT`).
//! For this reason a destroyed resource is not freed at once: see [`Gpu::destroy_buffer`].
//!
//! Synchronization is the caller's task: this crate records no barrier by itself.
//! The caller tells each change of use with [`Gpu::image_barrier`] (the frame graph of
//! the render crate does this).

mod bindless;
mod device;
mod resources;
mod swapchain;

use std::io::Cursor;
use std::mem::ManuallyDrop;

use anyhow::{Context, Result, ensure};
use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{
    AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use slotmap::SlotMap;

use bindless::{Bindless, PUSH_CONSTANT_BYTES};
use device::DeviceContext;
use resources::{Buffer, COLOR_RANGE, Garbage, Image, Sampler};
use swapchain::Swapchain;

pub use resources::{
    Access, BufferDesc, BufferHandle, ComputePipelineHandle, Format, ImageDesc, ImageHandle,
    ImageTarget, SamplerDesc, SamplerHandle,
};

/// How many frames the CPU can record ahead of the GPU.
const FRAMES_IN_FLIGHT: usize = 2;

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
}

/// State of the frame between `begin_frame` and `end_frame`.
struct ActiveFrame {
    image_index: u32,
}

pub struct Gpu {
    ctx: DeviceContext,
    /// Dropped by hand in `Drop`, before the device.
    allocator: ManuallyDrop<Allocator>,
    swapchain: Swapchain,
    /// True: build the swapchain again before the next frame.
    swapchain_dirty: bool,
    window_size: (u32, u32),
    vsync: bool,
    bindless: Bindless,
    buffers: SlotMap<BufferHandle, Buffer>,
    images: SlotMap<ImageHandle, Image>,
    samplers: SlotMap<SamplerHandle, Sampler>,
    pipelines: SlotMap<ComputePipelineHandle, vk::Pipeline>,
    frames: [FrameSlot; FRAMES_IN_FLIGHT],
    /// Index in `frames` of the current frame, or of the last one between frames.
    slot: usize,
    active: Option<ActiveFrame>,
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
            frames.push(FrameSlot {
                pool,
                cmd: unsafe { device.allocate_command_buffers(&cmd_info) }?[0],
                image_available: unsafe { device.create_semaphore(&Default::default(), None) }?,
                in_flight: unsafe { device.create_fence(&fence_info, None) }?,
                garbage: Vec::new(),
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
            frames: frames
                .try_into()
                .ok()
                .expect("the loop makes FRAMES_IN_FLIGHT slots"),
            slot: 0,
            active: None,
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
                | vk::BufferUsageFlags::TRANSFER_DST,
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
        let mut image = Image {
            raw: vk::Image::null(),
            view: vk::ImageView::null(),
            allocation: Default::default(),
            extent: vk::Extent2D {
                width: desc.width,
                height: desc.height,
            },
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
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(image.extent.into())
            .mip_levels(1)
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
        let info = vk::SamplerCreateInfo::default()
            .mag_filter(filter)
            .min_filter(filter)
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

    /// Makes a compute pipeline from SPIR-V bytes. The entry point is `main`.
    /// The shader uses the bindless set (set 0) and up to 128 bytes of push constants.
    pub fn create_compute_pipeline(&mut self, spirv: &[u8]) -> Result<ComputePipelineHandle> {
        let device = &self.ctx.device;
        // `read_spv` checks the size and copies the bytes into aligned words.
        let code = ash::util::read_spv(&mut Cursor::new(spirv)).context("invalid SPIR-V")?;
        let module_info = vk::ShaderModuleCreateInfo::default().code(&code);
        let module = unsafe { device.create_shader_module(&module_info, None) }?;
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

    /// The index of the image in the bindless storage and sampled image arrays.
    pub fn image_index(&self, image: ImageHandle) -> Option<u32> {
        self.images.get(image).map(|i| i.bindless_index)
    }

    /// The index of the sampler in the bindless sampler array.
    pub fn sampler_index(&self, sampler: SamplerHandle) -> Option<u32> {
        self.samplers.get(sampler).map(|s| s.bindless_index)
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
        unsafe { device.wait_for_fences(&[self.frames[self.slot].in_flight], true, u64::MAX) }?;
        for garbage in std::mem::take(&mut self.frames[self.slot].garbage) {
            self.destroy_now(garbage);
        }

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
        let acquired = unsafe {
            self.swapchain.loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
        };
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
        }
        self.active = Some(ActiveFrame { image_index });
        Ok(true)
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
        ensure!(
            push_constants.len() <= PUSH_CONSTANT_BYTES as usize
                && push_constants.len().is_multiple_of(4),
            "push constants must be a multiple of 4 bytes and at most {PUSH_CONSTANT_BYTES} bytes"
        );
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
                device.cmd_push_constants(
                    cmd,
                    layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    push_constants,
                );
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
        match unsafe {
            self.swapchain
                .loader
                .queue_present(self.ctx.queue, &present)
        } {
            Ok(suboptimal) => self.swapchain_dirty |= suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.swapchain_dirty = true,
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
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
            garbage.extend(self.buffers.drain().map(|(_, b)| Garbage::Buffer(b)));
            garbage.extend(self.images.drain().map(|(_, i)| Garbage::Image(i)));
            garbage.extend(self.samplers.drain().map(|(_, s)| Garbage::Sampler(s)));
            garbage.extend(self.pipelines.drain().map(|(_, p)| Garbage::Pipeline(p)));
            for garbage in garbage {
                self.destroy_now(garbage);
            }

            let device = &self.ctx.device;
            for frame in &self.frames {
                // The pool owns the command buffer.
                device.destroy_command_pool(frame.pool, None);
                device.destroy_semaphore(frame.image_available, None);
                device.destroy_fence(frame.in_flight, None);
            }
            self.bindless.destroy(device);
            self.swapchain.destroy(device);
            // The allocator frees its memory blocks, so it goes before the device.
            ManuallyDrop::drop(&mut self.allocator);
            self.ctx.destroy();
        }
    }
}
