#version 460
// Pass "ui_overlay", vertex stage. The pipeline has no vertex buffers: the shader reads
// the index and the vertex from two storage buffers of the bindless set
// ("vertex pulling"). The draw call gives the range of positions in the index buffer.

#include "ui.glsl"

// One vertex of egui (`epaint::Vertex`), 20 bytes. Scalars and not vec2: a vec2 has an
// alignment of 8 bytes in std430, which would make the struct 24 bytes.
struct Vertex {
    float x, y;  // position in egui points, (0, 0) is the top left corner
    float u, v;  // texture coordinate
    uint color;  // r, g, b, a in one byte each: sRGB-encoded, premultiplied alpha
};

layout(set = BINDLESS_SET, binding = BINDLESS_STORAGE_BUFFERS, std430) readonly buffer Vertices {
    Vertex vertices[];
} vertex_buffers[];
// A second view of the same descriptor array, for buffers that hold indices.
layout(set = BINDLESS_SET, binding = BINDLESS_STORAGE_BUFFERS, std430) readonly buffer Indices {
    uint indices[];
} index_buffers[];

layout(location = 0) out vec2 out_uv;
layout(location = 1) out vec4 out_color;

void main() {
    uint index = index_buffers[push.index_buffer].indices[gl_VertexIndex];
    Vertex vertex = vertex_buffers[push.vertex_buffer].vertices[push.base_vertex + index];
    out_uv = vec2(vertex.u, vertex.v);
    out_color = unpackUnorm4x8(vertex.color);
    // Points to clip space (-1..1). Y goes down in both, so no flip.
    gl_Position = vec4(2.0 * vec2(vertex.x, vertex.y) / push.screen_size - 1.0, 0.0, 1.0);
}
