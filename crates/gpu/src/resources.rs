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
    pub struct GraphicsPipelineHandle;
    pub struct BlasHandle;
    pub struct TlasHandle;
}

/// Every buffer is a storage buffer in the bindless set.
// ponytail: no usage field. Every buffer permits all uses that the crate has (storage
// buffer, copy, acceleration structure input and storage). Add a usage field when a use
// has a cost or a limit (vertex data for the UI).
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
    /// As `Rgba8Unorm`, with sRGB-encoded color: a shader that samples the image gets
    /// linear color, and the filter blends linear color. For color textures.
    Rgba8Srgb,
    Rgba16Float,
    /// Four 32-bit float channels. For a mean over many frames: a 16-bit float is
    /// too coarse for the small change that a late frame makes.
    Rgba32Float,
    /// One 32-bit float channel.
    R32Float,
    /// Two 16-bit float channels.
    Rg16Float,
}

impl Format {
    /// Bytes of one pixel.
    pub fn texel_bytes(self) -> u32 {
        match self {
            Format::Rgba8Unorm | Format::Rgba8Srgb | Format::R32Float | Format::Rg16Float => 4,
            Format::Rgba16Float => 8,
            Format::Rgba32Float => 16,
        }
    }

    pub(crate) fn to_vk(self) -> vk::Format {
        match self {
            Format::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
            Format::Rgba8Srgb => vk::Format::R8G8B8A8_SRGB,
            Format::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
            Format::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
            Format::R32Float => vk::Format::R32_SFLOAT,
            Format::Rg16Float => vk::Format::R16G16_SFLOAT,
        }
    }
}

/// A 2D color image.
#[derive(Debug, Clone, Copy)]
pub struct ImageDesc<'a> {
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    /// 1: the image alone. More: each next level has half the width and height (not
    /// below 1 pixel), for a texture that shows small without noise. See
    /// [`mip_level_count`] and [`Gpu::upload_image`](crate::Gpu::upload_image).
    pub mip_levels: u32,
    pub format: Format,
    /// Shaders can read and write it (`image2D`, bindless binding 1).
    pub storage: bool,
    /// Shaders can sample it (`texture2D`, bindless binding 2).
    pub sampled: bool,
    /// A graphics pipeline can draw into it ([`Gpu::begin_rendering`](crate::Gpu::begin_rendering)).
    pub color_attachment: bool,
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
    /// A compute or fragment shader reads it through a sampler.
    Sampled,
    /// A graphics pipeline draws into it, with blending.
    ColorAttachment,
    /// Source of a blit.
    TransferSrc,
    /// Destination of a blit or of [`Gpu::write_image`](crate::Gpu::write_image).
    TransferDst,
    /// The window shows it. For the swapchain image only.
    Present,
}

impl Access {
    /// True if the use changes the content of the image.
    pub fn writes(self) -> bool {
        matches!(
            self,
            Access::StorageWrite
                | Access::StorageReadWrite
                | Access::ColorAttachment
                | Access::TransferDst
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
                S::COMPUTE_SHADER | S::FRAGMENT_SHADER,
                A::SHADER_SAMPLED_READ,
            ),
            // Blending reads the old color, so the use is a read and a write.
            Access::ColorAttachment => (
                L::COLOR_ATTACHMENT_OPTIMAL,
                S::COLOR_ATTACHMENT_OUTPUT,
                A::COLOR_ATTACHMENT_READ | A::COLOR_ATTACHMENT_WRITE,
            ),
            // ALL_TRANSFER: blits and copies.
            Access::TransferSrc => (L::TRANSFER_SRC_OPTIMAL, S::ALL_TRANSFER, A::TRANSFER_READ),
            Access::TransferDst => (L::TRANSFER_DST_OPTIMAL, S::ALL_TRANSFER, A::TRANSFER_WRITE),
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
    /// True: blend between texels. False: use the nearest texel (pixel art stays crisp).
    pub linear: bool,
    /// True: the texture repeats outside 0..1. False: the edge color continues.
    pub repeat: bool,
    /// True: the sampler reads all mip levels of the image and blends between two
    /// levels. A texture that shows smaller than it is then blends between texels
    /// also with `linear: false`: the nearest texel of a far surface is noise.
    /// False: the first level only.
    pub mipmaps: bool,
}

/// The number of mip levels of an image of this size, down to a level of 1 x 1.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    width.max(height).max(1).ilog2() + 1
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
    /// Bytes of one pixel, to check the size of uploaded data.
    pub texel_bytes: u32,
    pub mip_levels: u32,
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
    /// The storage buffer of the structure is a separate `Buffer` entry after this one.
    Accel(vk::AccelerationStructureKHR),
}

/// All mip levels and layers of a color image.
pub(crate) const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: vk::REMAINING_MIP_LEVELS,
    base_array_layer: 0,
    layer_count: 1,
};
