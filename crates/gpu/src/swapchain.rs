//! The swapchain: the images that the window shows.

use anyhow::{Context, Result, ensure};
use ash::{khr, vk};

use crate::device::DeviceContext;

pub(crate) struct Swapchain {
    pub loader: khr::swapchain::Device,
    /// Null until the first successful `rebuild`, and while the window has no area.
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub extent: vk::Extent2D,
    /// One per image. Signaled when the frame's commands are done; present waits for it.
    /// Per image and not per frame: present can hold a semaphore longer than one frame.
    pub render_finished: Vec<vk::Semaphore>,
}

impl Swapchain {
    pub fn new(ctx: &DeviceContext) -> Self {
        Self {
            loader: khr::swapchain::Device::new(&ctx.instance, &ctx.device),
            handle: vk::SwapchainKHR::null(),
            images: Vec::new(),
            extent: vk::Extent2D::default(),
            render_finished: Vec::new(),
        }
    }

    /// Makes the swapchain for the given window size and destroys the old one.
    /// The GPU must be idle. Returns false if the window has no area: no swapchain can exist then.
    pub fn rebuild(
        &mut self,
        ctx: &DeviceContext,
        width: u32,
        height: u32,
        vsync: bool,
    ) -> Result<bool> {
        let (physical_device, surface) = (ctx.physical_device, ctx.surface);
        let caps = unsafe {
            ctx.surface_loader
                .get_physical_device_surface_capabilities(physical_device, surface)
        }?;

        // `u32::MAX` means the surface takes its size from the swapchain.
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        if extent.width == 0 || extent.height == 0 {
            return Ok(false);
        }

        let formats = unsafe {
            ctx.surface_loader
                .get_physical_device_surface_formats(physical_device, surface)
        }?;
        // An sRGB format: the GPU encodes linear color values on write.
        let format = formats
            .iter()
            .find(|f| {
                matches!(
                    f.format,
                    vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
                ) && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .unwrap_or(&formats[0]);

        let modes = unsafe {
            ctx.surface_loader
                .get_physical_device_surface_present_modes(physical_device, surface)
        }?;
        // FIFO is vsync and always exists. MAILBOX and IMMEDIATE do not wait for the display.
        let present_mode = [vk::PresentModeKHR::MAILBOX, vk::PresentModeKHR::IMMEDIATE]
            .into_iter()
            .find(|mode| !vsync && modes.contains(mode))
            .unwrap_or(vk::PresentModeKHR::FIFO);

        // The frame is blitted into the swapchain image, so the image must permit that.
        ensure!(
            caps.supported_usage_flags
                .contains(vk::ImageUsageFlags::TRANSFER_DST),
            "the window surface does not support copies into its images (TRANSFER_DST)"
        );
        // OPAQUE: the window is not see-through. Some systems offer only other modes.
        let composite_alpha = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::INHERIT,
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        ]
        .into_iter()
        .find(|mode| caps.supported_composite_alpha.contains(*mode))
        .context("the window surface has no composite alpha mode")?;

        // One more than the minimum, so the driver has a free image while one is shown.
        let mut image_count = caps.min_image_count + 1;
        if caps.max_image_count != 0 {
            image_count = image_count.min(caps.max_image_count);
        }

        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            // TRANSFER_DST: the frame is blitted into the image.
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(composite_alpha)
            .present_mode(present_mode)
            .clipped(true)
            // The driver can reuse resources of the old swapchain.
            .old_swapchain(self.handle);
        let handle = unsafe { self.loader.create_swapchain(&info, None) }?;
        unsafe { self.destroy(&ctx.device) };

        self.handle = handle;
        self.extent = extent;
        self.images = unsafe { self.loader.get_swapchain_images(handle) }?;
        for _ in &self.images {
            let semaphore = unsafe { ctx.device.create_semaphore(&Default::default(), None) }?;
            self.render_finished.push(semaphore);
        }
        tracing::debug!(?extent, ?present_mode, format = ?format.format, images = self.images.len(), "swapchain built");
        Ok(true)
    }

    /// The GPU must be idle.
    pub unsafe fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            for semaphore in self.render_finished.drain(..) {
                device.destroy_semaphore(semaphore, None);
            }
            // Destroying a null handle is permitted.
            self.loader.destroy_swapchain(self.handle, None);
        }
        self.handle = vk::SwapchainKHR::null();
        self.images.clear();
    }
}
