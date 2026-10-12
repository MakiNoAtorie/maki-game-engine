// The scene on the GPU: the acceleration structure and the mesh data.
// Must agree with crates/render/src/meshes.rs. Include after bindless.glsl.
//
// From a ray hit to a normal and a color:
// 1. Each instance of the acceleration structure is a group of meshes (see meshes.rs).
//    Its custom index is the mesh table index of the first mesh of the group. The ray
//    query gives this index, the number of the mesh in the group (the geometry index),
//    the triangle number, and the barycentric coordinates of the hit.
// 2. mesh_normal() below reads the three corner normals of the triangle and blends
//    them. mesh_material() reads the color and the material, and the textures of a
//    mesh that has them, at the texture coordinates of the hit.
// 3. The result is in the space of the mesh. The ray query also gives the
//    world-to-object matrix of the instance, which turns it into a world normal.
// trace.glsl does these steps.

#extension GL_EXT_buffer_reference : require
#extension GL_EXT_ray_query : require

// All instances of the scene (TLAS). Built again in each frame by the pass "build_tlas".
layout(set = BINDLESS_SET, binding = BINDLESS_ACCELERATION_STRUCTURE)
    uniform accelerationStructureEXT scene_tlas;

// The corner normals of a mesh: three values for each triangle, in triangle order, so
// a hit reads 12 bytes at one place. Each value is a packed normal, see unpack_normal().
// A buffer_reference block is a pointer type: the shader reads the buffer by its GPU
// address (Gpu::buffer_address on the Rust side), with no descriptor.
layout(buffer_reference, std430, buffer_reference_align = 4) readonly buffer NormalBuffer {
    uint corners[];
};

// The parts for textures are in the shader only with `TEXTURES` (see trace.glsl).
#ifdef TEXTURES
// The texture coordinates of a mesh that has a texture: two floats (u, v) for each
// corner of each triangle, in triangle order, as the normals.
layout(buffer_reference, std430, buffer_reference_align = 4) readonly buffer UvBuffer {
    float values[];
};

// The textures of all materials and the samplers, in the bindless arrays.
layout(set = BINDLESS_SET, binding = BINDLESS_SAMPLED_IMAGES) uniform texture2D textures[];
layout(set = BINDLESS_SET, binding = BINDLESS_SAMPLERS) uniform sampler samplers[];

// A texture of a material ("slot"): the index of its image in `textures` in the low
// 16 bits, the index of its sampler in `samplers` above. See src/textures.rs.
const uint NO_TEXTURE = 0xFFFFFFFFu;
#else
// Without textures the shader does not read the address: any 8 bytes.
#define UvBuffer uvec2
#endif

// The values of `MeshEntry.material`: how the surface sends light back.
// Must agree with `Material` in the assets crate (see `entry_bytes` in meshes.rs).
const uint MATERIAL_DIFFUSE = 0u; // equally in all directions, with the base color
const uint MATERIAL_MIRROR = 1u;  // a perfect mirror
const uint MATERIAL_GLASS = 2u;   // clear glass: reflects a part, lets the rest through
// The two rough kinds. A rough surface is many small mirrors (facets) that point in
// directions around the normal; `param` is the roughness: how far around.
const uint MATERIAL_METAL = 3u;   // a rough mirror, with the base color as its tint
const uint MATERIAL_PLASTIC = 4u; // a diffuse surface under a clear, rough coat

// 64 bytes.
struct MeshEntry {
    NormalBuffer normals;
    uint material; // a MATERIAL_* value
    float param;   // glass: index of refraction. Metal and plastic: roughness, 0.05 to 1
    // rgb: the color. a: how long one meter of the surface is in texture coordinates.
    vec4 base_color;
    // rgb: the light that the surface sends out by itself. a: where the alpha of the
    // color texture is below this, the mesh has a hole. 0: no holes.
    vec4 emissive;
    UvBuffer uvs;          // only if the mesh has a texture
    uint base_texture;     // a slot, or NO_TEXTURE. Multiplies `base_color`.
    uint emissive_texture; // the same for `emissive`
};

// The mesh table is a buffer of the bindless array. Its index comes in a push constant.
layout(set = BINDLESS_SET, binding = BINDLESS_STORAGE_BUFFERS, std430) readonly buffer MeshTable {
    MeshEntry meshes[];
} mesh_tables[];

// The material of a mesh: its base color (linear RGB), its MATERIAL_* value, the
// number of that material (`MeshEntry.param`), and its own light. Without its
// textures: see mesh_textures().
void mesh_material(uint table, uint mesh, out vec3 base_color, out uint material, out float param,
    out vec3 emissive) {
    MeshEntry entry = mesh_tables[nonuniformEXT(table)].meshes[mesh];
    base_color = entry.base_color.rgb;
    material = entry.material;
    param = entry.param;
    emissive = entry.emissive.rgb;
}

#ifdef TEXTURES
// The texture coordinates at a point of a triangle. `barycentrics` as for mesh_normal().
vec2 mesh_uv(UvBuffer uvs, uint triangle, vec2 barycentrics) {
    uint first = triangle * 6;
    vec2 uv0 = vec2(uvs.values[first], uvs.values[first + 1]);
    vec2 uv1 = vec2(uvs.values[first + 2], uvs.values[first + 3]);
    vec2 uv2 = vec2(uvs.values[first + 4], uvs.values[first + 5]);
    return uv0 * (1.0 - barycentrics.x - barycentrics.y) + uv1 * barycentrics.x + uv2 * barycentrics.y;
}

// A texture at a mip level. Level 0 is the texture itself, each next level has half
// the size. The level can be a fraction: the sampler blends two levels.
vec4 sample_slot(uint slot, vec2 uv, float level) {
    return textureLod(nonuniformEXT(sampler2D(textures[slot & 0xFFFFu], samplers[slot >> 16u])),
        uv, level);
}

// The mip level at which one texel of a texture is as large as `footprint`: the size
// in meters of the piece of the surface that one sample stands for. A compute shader
// has no screen derivatives, so the caller gets the footprint from its ray (the "ray
// cone" method, Akenine-Moller et al., 2021). With a smaller level the texture has
// detail that one sample cannot show: it sparkles. With a larger one it is blurred.
float texture_level(uint slot, float uvs_per_meter, float footprint) {
    ivec2 size = textureSize(nonuniformEXT(sampler2D(textures[slot & 0xFFFFu], samplers[slot >> 16u])), 0);
    // The footprint in texels.
    return log2(max(footprint * uvs_per_meter * float(max(size.x, size.y)), 1e-6));
}

// Multiplies the color and the own light of a mesh by its textures at a point.
// `footprint`: see texture_level().
void mesh_textures(uint table, uint mesh, uint triangle, vec2 barycentrics, float footprint,
    inout vec3 base_color, inout vec3 emissive) {
    MeshEntry entry = mesh_tables[nonuniformEXT(table)].meshes[mesh];
    // Most meshes have no texture: they take this branch only.
    if ((entry.base_texture & entry.emissive_texture) == NO_TEXTURE) {
        return;
    }
    vec2 uv = mesh_uv(entry.uvs, triangle, barycentrics);
    if (entry.base_texture != NO_TEXTURE) {
        float level = texture_level(entry.base_texture, entry.base_color.a, footprint);
        base_color *= sample_slot(entry.base_texture, uv, level).rgb;
    }
    if (entry.emissive_texture != NO_TEXTURE) {
        float level = texture_level(entry.emissive_texture, entry.base_color.a, footprint);
        emissive *= sample_slot(entry.emissive_texture, uv, level).rgb;
    }
}

// True if a mesh with holes has surface at a point: the alpha of its color texture
// there is not below the limit. Only such a mesh makes the ray query ask (its
// geometry has no opaque flag, see src/meshes.rs).
// Level 0: the test must give the same part of the surface from each distance, or the
// shadow of a leaf would grow with the distance of the lamp.
bool mesh_solid(uint table, uint mesh, uint triangle, vec2 barycentrics) {
    MeshEntry entry = mesh_tables[nonuniformEXT(table)].meshes[mesh];
    vec2 uv = mesh_uv(entry.uvs, triangle, barycentrics);
    return sample_slot(entry.base_texture, uv, 0.0).a >= entry.emissive.a;
}

#endif

// A packed normal to a vector of length 1. Must agree with `pack_normal` in meshes.rs.
// The packing is the octahedral map: the direction is moved onto the octahedron
// |x| + |y| + |z| = 1, and the lower half is folded up into the corners of the square
// of the upper half. x and y of that square are stored, 16 bits each.
vec3 unpack_normal(uint packed) {
    // The shifts give the two signed 16-bit values. -32767 to 32767 is -1.0 to 1.0.
    vec2 square = vec2(ivec2(int(packed << 16) >> 16, int(packed) >> 16)) / 32767.0;
    vec3 n = vec3(square, 1.0 - abs(square.x) - abs(square.y));
    // Negative z: the point is in a folded corner. Fold it back.
    float fold = max(-n.z, 0.0);
    n.x += n.x >= 0.0 ? -fold : fold;
    n.y += n.y >= 0.0 ? -fold : fold;
    return normalize(n);
}

// The normal at a point of a triangle, in the space of the mesh. Not normalized.
// `barycentrics` are the weights of the second and third corner, as a ray query gives them.
vec3 mesh_normal(uint table, uint mesh, uint triangle, vec2 barycentrics) {
    NormalBuffer normals = mesh_tables[nonuniformEXT(table)].meshes[mesh].normals;
    uint first = triangle * 3;
    vec3 n0 = unpack_normal(normals.corners[first]);
    vec3 n1 = unpack_normal(normals.corners[first + 1]);
    vec3 n2 = unpack_normal(normals.corners[first + 2]);
    return n0 * (1.0 - barycentrics.x - barycentrics.y) + n1 * barycentrics.x + n2 * barycentrics.y;
}
