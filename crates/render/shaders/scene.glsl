// The scene on the GPU: the acceleration structure and the mesh data.
// Must agree with crates/render/src/meshes.rs. Include after bindless.glsl.
//
// From a ray hit to a normal and a color:
// 1. Each instance of the acceleration structure carries the mesh table index of its
//    mesh as its custom index. The ray query gives this index, the triangle number,
//    and the barycentric coordinates of the hit.
// 2. mesh_normal() below reads the three corner indices of the triangle, then the
//    three corner normals, and blends them. mesh_base_color() reads the color.
// 3. The result is in the space of the mesh. The ray query also gives the
//    world-to-object matrix of the instance, which turns it into a world normal.
// trace.comp does these steps.

#extension GL_EXT_buffer_reference : require
#extension GL_EXT_ray_query : require

// All instances of the scene (TLAS). Built again in each frame by the pass "build_tlas".
layout(set = BINDLESS_SET, binding = BINDLESS_ACCELERATION_STRUCTURE)
    uniform accelerationStructureEXT scene_tlas;

// 32 bytes: a vec3 in a std430 block has an alignment of 16 bytes.
struct Vertex {
    vec3 position;
    vec3 normal;
};

// A buffer_reference block is a pointer type: the shader reads the buffer by its GPU
// address (Gpu::buffer_address on the Rust side), with no descriptor.
layout(buffer_reference, std430, buffer_reference_align = 16) readonly buffer VertexBuffer {
    Vertex vertices[];
};

layout(buffer_reference, std430, buffer_reference_align = 4) readonly buffer IndexBuffer {
    uint indices[];
};

// 48 bytes.
struct MeshEntry {
    VertexBuffer vertices;
    IndexBuffer indices;
    vec4 base_color;
    uint vertex_count;
    uint index_count;
};

// The mesh table is a buffer of the bindless array. Its index comes in a push constant.
layout(set = BINDLESS_SET, binding = BINDLESS_STORAGE_BUFFERS, std430) readonly buffer MeshTable {
    MeshEntry meshes[];
} mesh_tables[];

// The base color of a mesh, linear RGB.
vec3 mesh_base_color(uint table, uint mesh) {
    return mesh_tables[nonuniformEXT(table)].meshes[mesh].base_color.rgb;
}

// The normal at a point of a triangle, in the space of the mesh. Not normalized.
// `barycentrics` are the weights of the second and third corner, as a ray query gives them.
vec3 mesh_normal(uint table, uint mesh, uint triangle, vec2 barycentrics) {
    MeshEntry entry = mesh_tables[nonuniformEXT(table)].meshes[mesh];
    uint first = triangle * 3;
    vec3 n0 = entry.vertices.vertices[entry.indices.indices[first]].normal;
    vec3 n1 = entry.vertices.vertices[entry.indices.indices[first + 1]].normal;
    vec3 n2 = entry.vertices.vertices[entry.indices.indices[first + 2]].normal;
    return n0 * (1.0 - barycentrics.x - barycentrics.y) + n1 * barycentrics.x + n2 * barycentrics.y;
}
