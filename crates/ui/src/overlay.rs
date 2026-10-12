//! The `ui_overlay` pass: draws the triangles of egui into the window image.
//!
//! A small egui renderer on the draw calls of the gpu crate. egui gives a list of
//! meshes (2D triangles with a color and a texture coordinate at each vertex) and the
//! changes of its textures (the font image). The pass copies all meshes of the frame
//! into one vertex buffer and one index buffer and does one draw for each mesh.

use std::collections::HashMap;
use std::sync::Mutex;

use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use egui::epaint::{ImageDelta, Primitive};
use egui::{ImageData, TextureId};
use maki_gpu::{
    Access, BufferDesc, BufferHandle, FRAMES_IN_FLIGHT, Format, Gpu, GraphicsPipelineDesc,
    GraphicsPipelineHandle, ImageDesc, ImageHandle, ImageTarget, SamplerDesc, SamplerHandle,
};
use maki_render::{FrameGraph, Pass, PassContext, RenderGpu, ResourceId};

/// The egui output of the frame. A resource of the render world.
#[derive(Resource, Default)]
pub(crate) struct ExtractedUi {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub pixels_per_point: f32,
    /// Texture changes that the pass did not apply yet.
    // A mutex: the pass takes the changes, but a pass gets the render world read-only.
    pub textures: Mutex<egui::TexturesDelta>,
}

impl Drop for ExtractedUi {
    fn drop(&mut self) {
        // See `UiContext::drop`.
        if let Ok(textures) = self.textures.get_mut() {
            textures.clear();
        }
    }
}

/// Adds the pass to the frame graph. A `Startup` system, after `setup_renderer`.
pub(crate) fn add_pass(mut gpu: ResMut<RenderGpu>, mut graph: ResMut<FrameGraph>) {
    let gpu = &mut gpu.0;
    // The compose pass puts the picture into this image and the present pass copies
    // it to the window.
    let output = graph
        .resource("window")
        .expect("the render crate makes the `window` image");
    let format = graph
        .transient_desc(output)
        .expect("`window` is a transient image")
        .format;
    let pipeline = gpu
        .create_graphics_pipeline(&GraphicsPipelineDesc {
            vertex_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv")),
            fragment_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv")),
            target_format: format,
        })
        .expect("ui pipeline");
    // ponytail: one sampler for all textures. egui asks for a filter with each texture
    // (`TextureOptions`); read it when a panel shows an image that needs another one.
    let sampler = gpu
        .create_sampler(&SamplerDesc {
            linear: true,
            repeat: false,
            mipmaps: false,
        })
        .expect("ui sampler");
    graph.add_pass_before(
        "present",
        UiOverlay {
            pipeline,
            sampler,
            output,
            textures: HashMap::new(),
            buffers: Default::default(),
        },
    );
}

struct UiOverlay {
    pipeline: GraphicsPipelineHandle,
    sampler: SamplerHandle,
    output: ResourceId,
    /// The GPU image of each egui texture, and its size in pixels.
    textures: HashMap<TextureId, (ImageHandle, [u32; 2])>,
    /// The vertex and the index buffer of each frame slot: the CPU writes them in each
    /// frame, and the GPU can still read those of the frame before (`Gpu::frame_slot`).
    buffers: [[GrowBuffer; 2]; FRAMES_IN_FLIGHT],
}

/// One draw call: a mesh of egui.
struct Draw {
    scissor: [u32; 4],
    texture: TextureId,
    /// Position of the first vertex of the mesh in the vertex buffer.
    base_vertex: u32,
    /// Positions of the mesh in the index buffer.
    indices: std::ops::Range<u32>,
}

impl Pass for UiOverlay {
    fn name(&self) -> &'static str {
        "ui_overlay"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![(self.output, Access::ColorAttachment)]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let extracted = ctx.world.resource::<ExtractedUi>();
        let mut changes =
            std::mem::take(&mut *extracted.textures.lock().expect("no panic with the lock"));
        // `drain`: egui panics in a debug build when a change is dropped unread.
        for (id, deltas) in changes.set.drain() {
            for delta in &deltas {
                self.set_texture(ctx.gpu, id, delta);
            }
        }

        // All meshes of the frame in two byte lists, in the layouts of `ui.vert`.
        let scale = extracted.pixels_per_point;
        let to_pixel = |point: f32| (point * scale).round().max(0.0) as u32;
        let (mut vertices, mut indices, mut draws) = (Vec::new(), Vec::new(), Vec::new());
        for clipped in &extracted.primitives {
            // A callback is custom draw code of a widget. No panel uses one.
            let Primitive::Mesh(mesh) = &clipped.primitive else {
                continue;
            };
            let clip = clipped.clip_rect;
            let (left, top) = (to_pixel(clip.min.x), to_pixel(clip.min.y));
            let first_index = (indices.len() / 4) as u32;
            draws.push(Draw {
                scissor: [
                    left,
                    top,
                    to_pixel(clip.max.x).saturating_sub(left),
                    to_pixel(clip.max.y).saturating_sub(top),
                ],
                texture: mesh.texture_id,
                base_vertex: (vertices.len() / VERTEX_BYTES) as u32,
                indices: first_index..first_index + mesh.indices.len() as u32,
            });
            for vertex in &mesh.vertices {
                let floats = [vertex.pos.x, vertex.pos.y, vertex.uv.x, vertex.uv.y];
                vertices.extend(floats.into_iter().flat_map(f32::to_ne_bytes));
                vertices.extend(vertex.color.to_array());
            }
            indices.extend(mesh.indices.iter().flat_map(|index| index.to_ne_bytes()));
        }

        if !draws.is_empty() {
            let [vertex_buffer, index_buffer] = &mut self.buffers[ctx.gpu.frame_slot()];
            let vertex_buffer = vertex_buffer.write(ctx.gpu, "ui vertices", &vertices);
            let index_buffer = index_buffer.write(ctx.gpu, "ui indices", &indices);
            let (width, height) = ctx.extent(self.output);
            let sampler = ctx
                .gpu
                .sampler_index(self.sampler)
                .expect("ui sampler is alive");
            let target = ctx.image(self.output);
            ctx.gpu.begin_rendering(target).expect("ui begin_rendering");
            for draw in draws {
                // A texture that egui did not send (a user texture) is not drawn.
                let Some(texture) = self
                    .textures
                    .get(&draw.texture)
                    .and_then(|(image, _)| ctx.gpu.image_index(*image))
                else {
                    continue;
                };
                // Field order as in the `Push` block of `ui.glsl`.
                let push = [
                    (width as f32 / scale).to_bits(),
                    (height as f32 / scale).to_bits(),
                    vertex_buffer,
                    index_buffer,
                    draw.base_vertex,
                    texture,
                    sampler,
                ];
                let push: Vec<u8> = push.into_iter().flat_map(u32::to_ne_bytes).collect();
                ctx.gpu
                    .draw(self.pipeline, &push, draw.scissor, draw.indices)
                    .expect("ui draw");
            }
            ctx.gpu.end_rendering().expect("ui end_rendering");
        }

        // After the draws: a mesh of this frame can still use a texture that egui frees.
        for id in changes.free.drain() {
            if let Some((image, _)) = self.textures.remove(&id) {
                ctx.gpu.destroy_image(image);
            }
        }
    }
}

/// Bytes of one vertex: `Vertex` in `ui.vert`.
const VERTEX_BYTES: usize = 20;

impl UiOverlay {
    /// Applies one texture change of egui: a new image, or new pixels for a part of it.
    fn set_texture(&mut self, gpu: &mut Gpu, id: TextureId, delta: &ImageDelta) {
        let ImageData::Color(pixels) = &delta.image;
        let size = pixels.size.map(|side| side as u32);
        // sRGB-encoded with premultiplied alpha, the same as the vertex colors.
        let bytes: Vec<u8> = pixels.pixels.iter().flat_map(|c| c.to_array()).collect();

        let existing = self.textures.get(&id).copied();
        let (image, origin, old_use) = match (delta.pos, existing) {
            // New pixels for a part of the image. The other pixels stay.
            (Some(origin), Some((image, _))) => (image, origin, Some(Access::Sampled)),
            (Some(_), None) => {
                tracing::error!(?id, "egui changed a part of a texture that does not exist");
                return;
            }
            // The full image, with the size that it has.
            (None, Some((image, old_size))) if old_size == size => {
                (image, [0, 0], Some(Access::Sampled))
            }
            // The full image, new or with a new size.
            (None, old) => {
                if let Some((old, _)) = old {
                    gpu.destroy_image(old);
                }
                let image = gpu
                    .create_image(&ImageDesc {
                        name: "ui texture",
                        width: size[0],
                        height: size[1],
                        mip_levels: 1,
                        format: Format::Rgba8Unorm,
                        storage: false,
                        sampled: true,
                        color_attachment: false,
                    })
                    .expect("ui texture");
                self.textures.insert(id, (image, size));
                (image, [0, 0], None)
            }
        };
        // The graph does not know these images, so the pass records their barriers.
        let target = ImageTarget::Image(image);
        gpu.image_barrier(target, old_use, Access::TransferDst)
            .and_then(|()| gpu.write_image(image, origin.map(|v| v as u32), size, &bytes))
            .and_then(|()| gpu.image_barrier(target, Some(Access::TransferDst), Access::Sampled))
            .expect("ui texture upload");
    }
}

/// A CPU-written buffer that is made again, larger, when the data does not fit.
#[derive(Default)]
struct GrowBuffer(Option<(BufferHandle, u64)>);

impl GrowBuffer {
    /// Copies `data` to the start of the buffer. Returns the bindless index of the buffer.
    fn write(&mut self, gpu: &mut Gpu, name: &str, data: &[u8]) -> u32 {
        let size = data.len() as u64;
        if self.0.is_none_or(|(_, capacity)| capacity < size) {
            if let Some((old, _)) = self.0.take() {
                gpu.destroy_buffer(old);
            }
            // A power of two: a UI that grows a little does not make a buffer each frame.
            let capacity = size.next_power_of_two();
            let desc = BufferDesc {
                name,
                size: capacity,
                cpu_writable: true,
            };
            let buffer = gpu.create_buffer(&desc).expect("ui buffer");
            self.0 = Some((buffer, capacity));
        }
        let (buffer, _) = self.0.expect("set above");
        gpu.write_buffer(buffer, 0, data).expect("ui buffer write");
        gpu.buffer_index(buffer).expect("the buffer is alive")
    }
}
