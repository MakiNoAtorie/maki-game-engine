//! Vulkan instance, GPU selection, and the logical device with its one queue.

use std::borrow::Cow;
use std::ffi::{CStr, c_void};

use anyhow::{Context, Result, bail};
use ash::{ext, khr, vk};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

const DEVICE_EXTENSIONS: [&CStr; 4] = [
    khr::swapchain::NAME,
    khr::acceleration_structure::NAME,
    khr::ray_query::NAME,
    // Required by the acceleration structure extension.
    khr::deferred_host_operations::NAME,
];

/// The objects that live for the full run. Everything else is made from these.
pub(crate) struct DeviceContext {
    /// Keeps the Vulkan library loaded.
    _entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub surface_loader: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue_family: u32,
    /// Does graphics, compute, and present.
    pub queue: vk::Queue,
}

impl DeviceContext {
    pub fn new(
        window: &(impl HasWindowHandle + HasDisplayHandle),
        validation: bool,
    ) -> Result<Self> {
        let display = window.display_handle()?.as_raw();
        let entry = unsafe { ash::Entry::load() }
            .context("cannot load the Vulkan library: install a GPU driver with Vulkan support")?;

        // `None` means a Vulkan 1.0 loader.
        let version =
            unsafe { entry.try_enumerate_instance_version() }?.unwrap_or(vk::API_VERSION_1_0);
        if version < vk::API_VERSION_1_3 {
            bail!("missing feature: Vulkan 1.3 (the installed Vulkan loader is older)");
        }

        // The window system tells which extensions a surface needs.
        let mut extensions = ash_window::enumerate_required_extensions(display)?.to_vec();
        let mut layers = Vec::new();
        if validation {
            let available = unsafe { entry.enumerate_instance_layer_properties() }?;
            let installed = available.iter().any(|layer| {
                layer
                    .layer_name_as_c_str()
                    .is_ok_and(|name| name == VALIDATION_LAYER)
            });
            if installed {
                layers.push(VALIDATION_LAYER.as_ptr());
                extensions.push(ext::debug_utils::NAME.as_ptr());
                tracing::info!("Vulkan validation layer is on");
            } else {
                tracing::warn!(
                    "Vulkan validation layer not found: install the Vulkan SDK. Running without validation"
                );
            }
        }

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"maki")
            .api_version(vk::API_VERSION_1_3);
        // Synchronization validation is off by default in the layer. It finds missing
        // and wrong barriers.
        let sync_validation = [vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION];
        let mut validation_features =
            vk::ValidationFeaturesEXT::default().enabled_validation_features(&sync_validation);
        let mut instance_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&layers)
            .enabled_extension_names(&extensions);
        if !layers.is_empty() {
            instance_info = instance_info.push_next(&mut validation_features);
        }
        let instance = unsafe { entry.create_instance(&instance_info, None) }?;

        let debug = if layers.is_empty() {
            None
        } else {
            let loader = ext::debug_utils::Instance::new(&entry, &instance);
            let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(debug_callback));
            let messenger = unsafe { loader.create_debug_utils_messenger(&info, None) }?;
            Some((loader, messenger))
        };

        let surface_loader = khr::surface::Instance::new(&entry, &instance);
        let surface = unsafe {
            ash_window::create_surface(
                &entry,
                &instance,
                display,
                window.window_handle()?.as_raw(),
                None,
            )
        }?;

        let (physical_device, queue_family) =
            pick_physical_device(&instance, &surface_loader, surface)?;
        let device = create_device(&instance, physical_device, queue_family)?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        Ok(Self {
            _entry: entry,
            instance,
            debug,
            surface_loader,
            surface,
            physical_device,
            device,
            queue_family,
            queue,
        })
    }

    /// The caller must destroy all objects made from the device first.
    pub unsafe fn destroy(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

/// Sends validation layer messages to `tracing`.
unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    let message =
        unsafe { (*data).message_as_c_str() }.map_or(Cow::Borrowed(""), CStr::to_string_lossy);
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        tracing::error!(target: "vulkan", "{message}");
    } else {
        tracing::warn!(target: "vulkan", "{message}");
    }
    // FALSE: do not abort the Vulkan call that caused the message.
    vk::FALSE
}

/// The feature structs that `required_features` reads and `create_device` enables.
#[derive(Default)]
struct Features<'a> {
    core: vk::PhysicalDeviceFeatures,
    v12: vk::PhysicalDeviceVulkan12Features<'a>,
    v13: vk::PhysicalDeviceVulkan13Features<'a>,
    acceleration_structure: vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'a>,
    ray_query: vk::PhysicalDeviceRayQueryFeaturesKHR<'a>,
}

/// Each required feature: its name for the error message, and its flag in `Features`.
/// This list is the single source for the support check and for device creation.
fn required_features<'f>(f: &'f mut Features<'_>) -> [(&'static str, &'f mut vk::Bool32); 17] {
    const INDEXING: &str = "descriptor indexing";
    [
        ("buffer device address", &mut f.v12.buffer_device_address),
        (INDEXING, &mut f.v12.descriptor_indexing),
        // The parts of descriptor indexing that the bindless set uses.
        (INDEXING, &mut f.v12.runtime_descriptor_array),
        (INDEXING, &mut f.v12.descriptor_binding_partially_bound),
        (
            INDEXING,
            &mut f.v12.descriptor_binding_update_unused_while_pending,
        ),
        (
            INDEXING,
            &mut f.v12.descriptor_binding_storage_buffer_update_after_bind,
        ),
        (
            INDEXING,
            &mut f.v12.descriptor_binding_storage_image_update_after_bind,
        ),
        (
            INDEXING,
            &mut f.v12.descriptor_binding_sampled_image_update_after_bind,
        ),
        (
            INDEXING,
            &mut f.v12.shader_storage_buffer_array_non_uniform_indexing,
        ),
        (
            INDEXING,
            &mut f.v12.shader_storage_image_array_non_uniform_indexing,
        ),
        (
            INDEXING,
            &mut f.v12.shader_sampled_image_array_non_uniform_indexing,
        ),
        (
            INDEXING,
            &mut f.core.shader_storage_buffer_array_dynamic_indexing,
        ),
        (
            INDEXING,
            &mut f.core.shader_storage_image_array_dynamic_indexing,
        ),
        (
            INDEXING,
            &mut f.core.shader_sampled_image_array_dynamic_indexing,
        ),
        ("synchronization2", &mut f.v13.synchronization2),
        ("dynamic rendering", &mut f.v13.dynamic_rendering),
        ("ray query", &mut f.ray_query.ray_query),
    ]
}

/// The acceleration structure flags are apart from `required_features` only
/// because both are in one struct, and a struct can be borrowed one time in the list.
fn required_as_features<'f>(f: &'f mut Features<'_>) -> [(&'static str, &'f mut vk::Bool32); 2] {
    let accel = &mut f.acceleration_structure;
    [
        ("acceleration structure", &mut accel.acceleration_structure),
        (
            "acceleration structure (descriptor update after bind)",
            &mut accel.descriptor_binding_acceleration_structure_update_after_bind,
        ),
    ]
}

/// Returns the names of all required items that the GPU does not have, and a usable queue family.
fn missing_requirements(
    instance: &ash::Instance,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
) -> Result<(Vec<String>, Option<u32>)> {
    let mut missing = Vec::new();

    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    if properties.api_version < vk::API_VERSION_1_3 {
        // A driver older than 1.3 does not know the 1.3 feature struct, so stop here.
        return Ok((vec!["Vulkan 1.3".to_owned()], None));
    }

    let extensions = unsafe { instance.enumerate_device_extension_properties(physical_device) }?;
    for required in DEVICE_EXTENSIONS {
        let supported = extensions.iter().any(|e| {
            e.extension_name_as_c_str()
                .is_ok_and(|name| name == required)
        });
        if !supported {
            missing.push(format!("extension {}", required.to_string_lossy()));
        }
    }

    let mut features = Features::default();
    {
        let mut query = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut features.v12)
            .push_next(&mut features.v13)
            .push_next(&mut features.acceleration_structure)
            .push_next(&mut features.ray_query);
        unsafe { instance.get_physical_device_features2(physical_device, &mut query) };
        features.core = query.features;
    }
    let mut check = |list: &[(&'static str, &mut vk::Bool32)]| {
        for (name, flag) in list {
            if **flag == vk::FALSE && !missing.iter().any(|m| m == name) {
                missing.push((*name).to_owned());
            }
        }
    };
    check(&required_features(&mut features));
    check(&required_as_features(&mut features));

    let families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    let mut queue_family = None;
    for (index, family) in families.iter().enumerate() {
        let index = index as u32;
        let draws = family
            .queue_flags
            .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE);
        let presents = unsafe {
            surface_loader.get_physical_device_surface_support(physical_device, index, surface)
        }?;
        if draws && presents {
            queue_family = Some(index);
            break;
        }
    }
    if queue_family.is_none() {
        missing.push("a graphics+compute queue that can present to the window".to_owned());
    }

    Ok((missing, queue_family))
}

/// Picks a GPU that has all requirements. A discrete GPU wins over an integrated one.
fn pick_physical_device(
    instance: &ash::Instance,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32)> {
    let mut best = None;
    let mut report = String::new();
    for physical_device in unsafe { instance.enumerate_physical_devices() }? {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let name = properties
            .device_name_as_c_str()
            .map_or(Cow::Borrowed("unknown GPU"), CStr::to_string_lossy);
        let (missing, queue_family) =
            missing_requirements(instance, surface_loader, surface, physical_device)?;

        if let (true, Some(queue_family)) = (missing.is_empty(), queue_family) {
            let discrete = properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU;
            tracing::info!(gpu = %name, discrete, "usable GPU");
            if best.is_none() || discrete {
                best = Some((physical_device, queue_family));
            }
        } else {
            report += &format!("\n  {name}: missing {}", missing.join(", "));
        }
    }
    best.with_context(|| {
        if report.is_empty() {
            "no GPU with a Vulkan driver found".to_owned()
        } else {
            format!("no GPU has all required Vulkan features:{report}")
        }
    })
}

fn create_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    queue_family: u32,
) -> Result<ash::Device> {
    let mut features = Features::default();
    for (_, flag) in required_features(&mut features) {
        *flag = vk::TRUE;
    }
    for (_, flag) in required_as_features(&mut features) {
        *flag = vk::TRUE;
    }
    let mut enabled = vk::PhysicalDeviceFeatures2::default()
        .features(features.core)
        .push_next(&mut features.v12)
        .push_next(&mut features.v13)
        .push_next(&mut features.acceleration_structure)
        .push_next(&mut features.ray_query);

    let priorities = [1.0];
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&priorities)];
    let extensions = DEVICE_EXTENSIONS.map(CStr::as_ptr);
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extensions)
        .push_next(&mut enabled);
    Ok(unsafe { instance.create_device(physical_device, &info, None) }?)
}
