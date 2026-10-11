// Binding numbers of the bindless descriptor set (set 0).
// Must agree with crates/gpu/src/bindless.rs.
//
// A shader declares only the arrays that it uses, with its own block or image type:
//   layout(set = BINDLESS_SET, binding = BINDLESS_STORAGE_IMAGES, rgba8)
//       uniform writeonly image2D images[];
// The array index of a resource comes from the Rust side (Gpu::image_index and others).

#define BINDLESS_SET 0
#define BINDLESS_STORAGE_BUFFERS 0
#define BINDLESS_STORAGE_IMAGES 1
#define BINDLESS_SAMPLED_IMAGES 2
#define BINDLESS_SAMPLERS 3
#define BINDLESS_ACCELERATION_STRUCTURE 4

// Runtime-sized resource arrays (`images[]`) and indices that are not constants.
#extension GL_EXT_nonuniform_qualifier : require
