// Shared by ui.vert and ui.frag.

#include "bindless.glsl"

// Must agree with `UiOverlay::execute` in src/overlay.rs.
layout(push_constant) uniform Push {
    vec2 screen_size;     // size of the target in egui points
    uint vertex_buffer;   // index in the storage buffer array
    uint index_buffer;    // index in the storage buffer array
    uint base_vertex;     // added to each index: the first vertex of the mesh
    uint texture;         // index in `textures`
    uint texture_sampler; // index in `samplers`
} push;
