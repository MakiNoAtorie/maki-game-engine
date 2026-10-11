//! Rasterization: graphics pipelines, draws into a color image, and image upload.
//! For 2D overlays (the UI). The path tracer does not use it.

use std::ops::Range;

use anyhow::{Context, Result, ensure};
use ash::vk;

use crate::bindless::PUSH_STAGES;
use crate::resources::{BufferDesc, Format, GraphicsPipelineHandle, ImageHandle};
use crate::{Garbage, Gpu, check_push_constants, shader_module};

/// A pipeline that draws triangles into one color image.
///
/// It has no vertex buffers: the vertex shader reads its data from storage buffers of
/// the bindless set, with `gl_VertexIndex` as the index. The buffer indices come in
/// push constants, as for a compute shader.
/// No depth test and no culling. The blend is for premultiplied alpha:
/// `source + target * (1 - source alpha)`.
// ponytail: one fixed pipeline state, sufficient for a 2D overlay. Add fields (blend
// mode, depth) when a second graphics pass needs another state.
#[derive(Debug, Clone, Copy)]
pub struct GraphicsPipelineDesc<'a> {
    /// SPIR-V bytes. The entry point of each shader is `main`.
    pub vertex_spirv: &'a [u8],
    pub fragment_spirv: &'a [u8],
    /// Format of the image that the pipeline draws into.
    pub target_format: Format,
}

impl Gpu {
    pub fn create_graphics_pipeline(
        &mut self,
        desc: &GraphicsPipelineDesc,
    ) -> Result<GraphicsPipelineHandle> {
        let device = &self.ctx.device;
        let vertex = shader_module(device, desc.vertex_spirv)?;
        let fragment = match shader_module(device, desc.fragment_spirv) {
            Ok(fragment) => fragment,
            Err(error) => {
                unsafe { device.destroy_shader_module(vertex, None) };
                return Err(error);
            }
        };
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment)
                .name(c"main"),
        ];
        // Empty: the vertex shader pulls its data from storage buffers.
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        // Only the counts: the rectangles are dynamic state, set for each draw.
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        // Dynamic rendering: the pipeline names the format of its target, and no render
        // pass object or framebuffer object is necessary.
        let formats = [desc.target_format.to_vk()];
        let mut rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.bindless.pipeline_layout)
            .push_next(&mut rendering);
        let result =
            unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) };
        // The pipeline keeps its own copy of the shader code.
        unsafe {
            device.destroy_shader_module(vertex, None);
            device.destroy_shader_module(fragment, None);
        }
        let pipeline = result.map_err(|(_, error)| error)?[0];
        Ok(self.graphics_pipelines.insert(pipeline))
    }

    /// Deferred, as [`Gpu::destroy_buffer`].
    pub fn destroy_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        if let Some(pipeline) = self.graphics_pipelines.remove(pipeline) {
            self.defer(Garbage::Pipeline(pipeline));
        }
    }

    /// Starts a group of [`Gpu::draw`] calls into the image. The old content of the image
    /// stays. The image must be in [`Access::ColorAttachment`](crate::Access::ColorAttachment).
    /// Only `draw` calls are permitted until [`Gpu::end_rendering`].
    pub fn begin_rendering(&mut self, target: ImageHandle) -> Result<()> {
        let active = self
            .active
            .as_mut()
            .context("begin_rendering outside of a frame")?;
        ensure!(
            active.rendering.is_none(),
            "begin_rendering without end_rendering"
        );
        let image = self.images.get(target).context("stale image handle")?;
        let extent = image.extent;
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(image.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            // LOAD: the draws go on top of the picture that is in the image.
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)];
        let info = vk::RenderingInfo::default()
            .render_area(extent.into())
            .layer_count(1)
            .color_attachments(&attachments);
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let device = &self.ctx.device;
        let cmd = self.frames[self.slot].cmd;
        unsafe {
            device.cmd_begin_rendering(cmd, &info);
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            // One bind for all draws of the group: every pipeline uses the same set.
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.bindless.pipeline_layout,
                0,
                &[self.bindless.set],
                &[],
            );
        }
        active.rendering = Some(extent);
        Ok(())
    }

    /// Draws triangles: the vertex shader runs one time for each number in `vertices`
    /// (`gl_VertexIndex`), and each three results are one triangle.
    /// `scissor` is `[x, y, width, height]` in pixels of the target: nothing is drawn
    /// outside of it. The part of it that is outside the image is ignored.
    pub fn draw(
        &mut self,
        pipeline: GraphicsPipelineHandle,
        push_constants: &[u8],
        scissor: [u32; 4],
        vertices: Range<u32>,
    ) -> Result<()> {
        let extent = self
            .active
            .as_ref()
            .and_then(|active| active.rendering)
            .context("draw outside of begin_rendering and end_rendering")?;
        check_push_constants(push_constants)?;
        let pipeline = *self
            .graphics_pipelines
            .get(pipeline)
            .context("stale graphics pipeline handle")?;

        // Vulkan does not permit a scissor that goes past the range of a signed number.
        let [x, y, width, height] = scissor;
        let (x, y) = (x.min(extent.width), y.min(extent.height));
        let scissor = vk::Rect2D {
            offset: vk::Offset2D {
                x: x as i32,
                y: y as i32,
            },
            extent: vk::Extent2D {
                width: width.min(extent.width - x),
                height: height.min(extent.height - y),
            },
        };
        if scissor.extent.width == 0 || scissor.extent.height == 0 || vertices.is_empty() {
            return Ok(());
        }

        let device = &self.ctx.device;
        let cmd = self.frames[self.slot].cmd;
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            device.cmd_set_scissor(cmd, 0, &[scissor]);
            if !push_constants.is_empty() {
                let layout = self.bindless.pipeline_layout;
                device.cmd_push_constants(cmd, layout, PUSH_STAGES, 0, push_constants);
            }
            device.cmd_draw(cmd, vertices.len() as u32, 1, vertices.start, 0);
        }
        Ok(())
    }

    pub fn end_rendering(&mut self) -> Result<()> {
        let active = self
            .active
            .as_mut()
            .context("end_rendering outside of a frame")?;
        ensure!(
            active.rendering.take().is_some(),
            "end_rendering without begin_rendering"
        );
        unsafe {
            self.ctx
                .device
                .cmd_end_rendering(self.frames[self.slot].cmd)
        };
        Ok(())
    }

    /// Copies pixels into a rectangle of the image. `data` holds the pixels row by row,
    /// in the format of the image. `origin` and `size` are `[x, y]` and `[width, height]`.
    /// The image must be in [`Access::TransferDst`](crate::Access::TransferDst).
    /// Call in a frame, not between `begin_rendering` and `end_rendering`: the copy is
    /// a command of the frame.
    pub fn write_image(
        &mut self,
        image: ImageHandle,
        origin: [u32; 2],
        size: [u32; 2],
        data: &[u8],
    ) -> Result<()> {
        let active = self
            .active
            .as_ref()
            .context("write_image outside of a frame")?;
        ensure!(
            active.rendering.is_none(),
            "write_image between begin_rendering and end_rendering"
        );
        let target = self.images.get(image).context("stale image handle")?;
        let raw = target.raw;
        let inside = |axis: usize, limit: u32| {
            origin[axis]
                .checked_add(size[axis])
                .is_some_and(|end| end <= limit)
        };
        ensure!(
            size[0] > 0
                && size[1] > 0
                && inside(0, target.extent.width)
                && inside(1, target.extent.height),
            "write_image: the rectangle is empty or goes past the edge of the image"
        );
        // The GPU reads this many bytes from the staging buffer.
        let bytes = u64::from(size[0]) * u64::from(size[1]) * u64::from(target.texel_bytes);
        ensure!(
            data.len() as u64 == bytes,
            "write_image: {} bytes of data for a rectangle of {bytes} bytes",
            data.len()
        );

        // The CPU cannot write image memory directly. The data goes into a buffer, and
        // the GPU copies the buffer into the image when it runs the frame.
        let staging = self.create_buffer(&BufferDesc {
            name: "image upload",
            size: bytes,
            cpu_writable: true,
        })?;
        self.write_buffer(staging, 0, data)?;
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_offset(vk::Offset3D {
                x: origin[0] as i32,
                y: origin[1] as i32,
                z: 0,
            })
            .image_extent(vk::Extent3D {
                width: size[0],
                height: size[1],
                depth: 1,
            });
        unsafe {
            self.ctx.device.cmd_copy_buffer_to_image(
                self.frames[self.slot].cmd,
                self.buffers[staging].raw,
                raw,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }
        // Deferred: the buffer stays until the GPU has done the copy.
        self.destroy_buffer(staging);
        Ok(())
    }
}
