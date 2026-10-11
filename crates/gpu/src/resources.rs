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
    /// Shaders can read and write it (`image2D`, bindless binding 1).
    pub storage: bool,
    /// Shaders can sample it (`texture2D`, bindless binding 2).
    pub sampled: bool,
}

/// How a pass uses an image. Each use has one image layout, one pipeline stage, and
/// one access type in Vulkan: [`Gpu::image_barrier`](crate::Gpu::image_barrier) moves
/// an image from one use to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// A compute shader reads it with `imageLoad`.
    StorageRead,
    /// A compute shader writes it with `imageStore`.
    StorageWrite,
    /// A compute shader reads and writes it.
    StorageReadWrite,
    /// A compute shader reads it through a sampler.
    Sampled,
    /// Source of a blit.
    TransferSrc,
    /// Destination of a blit.
    TransferDst,
    /// The window shows it. For the swapchain image only.
    Present,
}

impl Access {
    /// True if the use changes the content of the image.
    pub fn writes(self) -> bool {
        matches!(
            self,
            Access::StorageWrite | Access::StorageReadWrite | Access::TransferDst
        )
    }

    pub(crate) fn to_vk(self) -> (vk::ImageLayout, vk::PipelineStageFlags2, vk::AccessFlags2) {
        use vk::AccessFlags2 as A;
        use vk::ImageLayout as L;
        use vk::PipelineStageFlags2 as S;
        match self {
            // GENERAL is the only layout that permits storage image access.
            Access::StorageRead => (L::GENERAL, S::COMPUTE_SHADER, A::SHADER_STORAGE_READ),
            Access::StorageWrite => (L::GENERAL, S::COMPUTE_SHADER, A::SHADER_STORAGE_WRITE),
            Access::StorageReadWrite => (
                L::GENERAL,
                S::COMPUTE_SHADER,
                A::SHADER_STORAGE_READ | A::SHADER_STORAGE_WRITE,
            ),
            Access::Sampled => (
                L::SHADER_READ_ONLY_OPTIMAL,
                S::COMPUTE_SHADER,
                A::SHADER_SAMPLED_READ,
            ),
            Access::TransferSrc => (L::TRANSFER_SRC_OPTIMAL, S::BLIT, A::TRANSFER_READ),
            Access::TransferDst => (L::TRANSFER_DST_OPTIMAL, S::BLIT, A::TRANSFER_WRITE),
            // Present is not a pipeline stage: the queue submit orders it with a semaphore.
            Access::Present => (L::PRESENT_SRC_KHR, S::NONE, A::NONE),
        }
    }
}

/// The image of an [`Access`] change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageTarget {
    Image(ImageHandle),
    /// The swapchain image of the current frame.
    Swapchain,
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
    /// The address of the first byte, as shaders see it (`buffer_reference`).
    pub address: u64,
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
