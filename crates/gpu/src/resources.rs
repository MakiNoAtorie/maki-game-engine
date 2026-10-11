//! Buffers, images, and samplers: the description structs that users fill in,
//! the handles they get back, and the Vulkan objects behind the handles.

use ash::vk;
use gpu_allocator::vulkan::Allocation;

slotmap::new_key_type! {
    /// Generational handles: a handle of a destroyed resource never matches a new resource.
    pub struct BufferHandle;
    pub struct ImageHandle;
    pub struct SamplerHandle;
    pub struct ComputePipelineHandle;
}

/// Every buffer is a storage buffer in the bindless set.
// ponytail: no usage flags yet. Add a usage field when a buffer needs one
// (acceleration structure storage, vertex data for the UI).
#[derive(Debug, Clone, Copy)]
pub struct BufferDesc<'a> {
    /// Shown in allocator reports.
    pub name: &'a str,
    pub size: u64,
    /// True: the CPU can write the buffer with `Gpu::write_buffer`.
    pub cpu_writable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba8Unorm,
    Rgba16Float,
}

impl Format {
    pub(crate) fn to_vk(self) -> vk::Format {
        match self {
            Format::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
            Format::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        }
    }
}

/// A 2D color image with one mip level.
#[derive(Debug, Clone, Copy)]
pub struct ImageDesc<'a> {
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// Shaders can write it (`image2D`, bindless binding 1).
    pub storage: bool,
    /// Shaders can sample it (`texture2D`, bindless binding 2).
    pub sampled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SamplerDesc {
    /// True: blend between texels. False: use the nearest texel.
    pub linear: bool,
    /// True: the texture repeats outside 0..1. False: the edge color continues.
    pub repeat: bool,
}

pub(crate) struct Buffer {
    pub raw: vk::Buffer,
    pub allocation: Allocation,
    pub size: u64,
    pub bindless_index: u32,
}

pub(crate) struct Image {
    pub raw: vk::Image,
    pub view: vk::ImageView,
    pub allocation: Allocation,
    pub extent: vk::Extent2D,
    pub bindless_index: u32,
}

pub(crate) struct Sampler {
    pub raw: vk::Sampler,
    pub bindless_index: u32,
}

/// A freed resource that waits until no frame in flight can use it.
pub(crate) enum Garbage {
    Buffer(Buffer),
    Image(Image),
    Sampler(Sampler),
    Pipeline(vk::Pipeline),
}

/// All mip levels and layers of a color image.
pub(crate) const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};
