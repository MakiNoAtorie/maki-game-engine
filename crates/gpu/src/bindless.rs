//! Bindless descriptors: one descriptor set that holds every resource.
//!
//! A shader gets a resource by its index in an array, and the index comes from a push
//! constant or a buffer. No per-pass descriptor sets are necessary, and a ray-traced
//! shader can reach the material of any object that a ray hits.
//!
//! Set 0 layout (the same in `crates/render/shaders/bindless.glsl`):
//! - binding 0: storage buffers
//! - binding 1: storage images
//! - binding 2: sampled images
//! - binding 3: samplers
//! - binding 4: one acceleration structure

use anyhow::{Result, bail};
use ash::vk;

const STORAGE_BUFFERS: u32 = 0;
const STORAGE_IMAGES: u32 = 1;
const SAMPLED_IMAGES: u32 = 2;
const SAMPLERS: u32 = 3;
const ACCELERATION_STRUCTURE: u32 = 4;

const MAX_BUFFERS: u32 = 4096;
/// An image has one index for the two image bindings, so they have the same size.
const MAX_IMAGES: u32 = 4096;
const MAX_SAMPLERS: u32 = 64;

/// All pipelines can use this many bytes of push constants.
pub(crate) const PUSH_CONSTANT_BYTES: u32 = 128;

/// Gives out array indices. A freed index is used again.
pub(crate) struct Slots {
    capacity: u32,
    next: u32,
    free: Vec<u32>,
}

impl Slots {
    fn new(capacity: u32) -> Self {
        Self {
            capacity,
            next: 0,
            free: Vec::new(),
        }
    }

    pub fn alloc(&mut self) -> Result<u32> {
        if let Some(index) = self.free.pop() {
            return Ok(index);
        }
        if self.next == self.capacity {
            bail!("bindless array is full ({} slots)", self.capacity);
        }
        self.next += 1;
        Ok(self.next - 1)
    }

    pub fn free(&mut self, index: u32) {
        self.free.push(index);
    }
}

pub(crate) struct Bindless {
    pool: vk::DescriptorPool,
    set_layout: vk::DescriptorSetLayout,
    pub set: vk::DescriptorSet,
    /// Shared by all pipelines: set 0 is the bindless set, plus push constants.
    pub pipeline_layout: vk::PipelineLayout,
    pub buffer_slots: Slots,
    pub image_slots: Slots,
    pub sampler_slots: Slots,
}

impl Bindless {
    pub fn new(device: &ash::Device) -> Result<Self> {
        let arrays = [
            (
                STORAGE_BUFFERS,
                vk::DescriptorType::STORAGE_BUFFER,
                MAX_BUFFERS,
            ),
            (
                STORAGE_IMAGES,
                vk::DescriptorType::STORAGE_IMAGE,
                MAX_IMAGES,
            ),
            (
                SAMPLED_IMAGES,
                vk::DescriptorType::SAMPLED_IMAGE,
                MAX_IMAGES,
            ),
            (SAMPLERS, vk::DescriptorType::SAMPLER, MAX_SAMPLERS),
            (
                ACCELERATION_STRUCTURE,
                vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                1,
            ),
        ];
        let bindings = arrays.map(|(binding, ty, count)| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(ty)
                .descriptor_count(count)
                .stage_flags(vk::ShaderStageFlags::ALL)
        });
        // PARTIALLY_BOUND: empty slots are permitted if no shader reads them.
        // UPDATE_AFTER_BIND + UPDATE_UNUSED_WHILE_PENDING: a slot can be written while
        // frames in flight use the set, if those frames do not read that slot.
        let flags = [vk::DescriptorBindingFlags::PARTIALLY_BOUND
            | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
            | vk::DescriptorBindingFlags::UPDATE_UNUSED_WHILE_PENDING; 5];
        let mut flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&flags);
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
            .bindings(&bindings)
            .push_next(&mut flags_info);
        let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }?;

        let pool_sizes = arrays.map(|(_, ty, descriptor_count)| vk::DescriptorPoolSize {
            ty,
            descriptor_count,
        });
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        let pool = unsafe { device.create_descriptor_pool(&pool_info, None) }?;

        let set_layouts = [set_layout];
        let set_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&set_layouts);
        let set = unsafe { device.allocate_descriptor_sets(&set_info) }?[0];

        let push_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .size(PUSH_CONSTANT_BYTES)];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_ranges);
        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None) }?;

        Ok(Self {
            pool,
            set_layout,
            set,
            pipeline_layout,
            buffer_slots: Slots::new(MAX_BUFFERS),
            image_slots: Slots::new(MAX_IMAGES),
            sampler_slots: Slots::new(MAX_SAMPLERS),
        })
    }

    pub fn write_buffer(&self, device: &ash::Device, index: u32, buffer: vk::Buffer) {
        let info = [vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .range(vk::WHOLE_SIZE)];
        let write = self
            .write(STORAGE_BUFFERS, index, vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&info);
        unsafe { device.update_descriptor_sets(&[write], &[]) };
    }

    /// Writes the view to the storage and/or sampled image array, at the same index.
    pub fn write_image(
        &self,
        device: &ash::Device,
        index: u32,
        view: vk::ImageView,
        storage: bool,
        sampled: bool,
    ) {
        // A descriptor names the layout that the image has when a shader uses it.
        // The layouts agree with `Access::to_vk`.
        use vk::DescriptorType as T;
        use vk::ImageLayout as L;
        let targets = [
            (storage, STORAGE_IMAGES, T::STORAGE_IMAGE, L::GENERAL),
            (
                sampled,
                SAMPLED_IMAGES,
                T::SAMPLED_IMAGE,
                L::SHADER_READ_ONLY_OPTIMAL,
            ),
        ];
        for (_, binding, ty, layout) in targets.into_iter().filter(|target| target.0) {
            let info = [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(layout)];
            let write = self.write(binding, index, ty).image_info(&info);
            unsafe { device.update_descriptor_sets(&[write], &[]) };
        }
    }

    pub fn write_sampler(&self, device: &ash::Device, index: u32, sampler: vk::Sampler) {
        let info = [vk::DescriptorImageInfo::default().sampler(sampler)];
        let write = self
            .write(SAMPLERS, index, vk::DescriptorType::SAMPLER)
            .image_info(&info);
        unsafe { device.update_descriptor_sets(&[write], &[]) };
    }

    fn write(
        &self,
        binding: u32,
        index: u32,
        ty: vk::DescriptorType,
    ) -> vk::WriteDescriptorSet<'_> {
        vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(binding)
            .dst_array_element(index)
            .descriptor_type(ty)
    }

    pub unsafe fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            // The pool owns the set.
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_unique_reused_and_limited() {
        let mut slots = Slots::new(2);
        assert_eq!((slots.alloc().unwrap(), slots.alloc().unwrap()), (0, 1));
        assert!(slots.alloc().is_err());
        slots.free(0);
        assert_eq!(slots.alloc().unwrap(), 0);
        assert!(slots.alloc().is_err());
    }
}
