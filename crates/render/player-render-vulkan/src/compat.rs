//! Vulkan 1.1-1.4 compatibility helpers for Android and desktop surfaces.

use std::ffi::CStr;

use ash::vk;

#[cfg_attr(
    not(any(target_os = "android", target_os = "linux", target_os = "windows")),
    allow(dead_code)
)]
pub(crate) const VULKAN_1_4: u32 = vk::make_api_version(0, 1, 4, 0);
#[cfg_attr(not(any(test, target_os = "android")), allow(dead_code))]
pub(crate) const FALLBACK_EXTERNAL_SAMPLER_DESCRIPTORS: u32 = 4;

/// Returns whether every required extension name appears in `available`.
#[cfg_attr(
    not(any(target_os = "android", target_os = "linux", target_os = "windows")),
    allow(dead_code)
)]
pub(crate) fn has_extensions(available: &[vk::ExtensionProperties], required: &[&CStr]) -> bool {
    required.iter().all(|&name| {
        available
            .iter()
            .any(|extension| extension.extension_name_as_c_str() == Ok(name))
    })
}

/// Selects the highest instance API version supported by the loader within 1.1..=1.4.
#[cfg_attr(
    not(any(
        test,
        target_os = "android",
        target_os = "linux",
        target_os = "windows"
    )),
    allow(dead_code)
)]
pub(crate) fn select_instance_api_version(loader_version: u32) -> Result<u32, &'static str> {
    if loader_version < vk::API_VERSION_1_1 {
        return Err("Vulkan 1.1 is required");
    }
    Ok(loader_version.min(VULKAN_1_4))
}

/// Resolves the combined image sampler descriptor count for an imported YUV 420 buffer.
///
/// Defined multi-planar YUV 420 formats use at most three descriptors. Vendor external
/// formats query `VK_KHR_maintenance6` / Vulkan 1.4 when available and fall back to a
/// conservative four-descriptor budget on Vulkan 1.1-1.3 drivers.
#[cfg_attr(not(any(test, target_os = "android")), allow(dead_code))]
pub(crate) fn resolve_external_sampler_descriptors(
    external_format: u64,
    maintenance6_count: u32,
) -> u32 {
    if external_format == 0 {
        3
    } else if maintenance6_count != 0 {
        maintenance6_count
    } else {
        FALLBACK_EXTERNAL_SAMPLER_DESCRIPTORS
    }
}

/// Validates mandatory physical-device capabilities for Android AHardwareBuffer import.
#[cfg_attr(not(any(test, target_os = "android")), allow(dead_code))]
pub(crate) fn validate_android_import_requirements(
    vulkan_1_1: bool,
    hardware_buffer_import: bool,
    sampler_ycbcr_conversion: bool,
) -> Result<(), &'static str> {
    if !vulkan_1_1 {
        return Err("Vulkan 1.1 is required");
    }
    if !hardware_buffer_import {
        return Err("AHardwareBuffer external memory is unavailable");
    }
    if !sampler_ycbcr_conversion {
        return Err("sampler YCbCr conversion is unavailable");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_instance_version_across_vulkan_1_1_to_1_4() {
        assert_eq!(
            select_instance_api_version(vk::API_VERSION_1_0),
            Err("Vulkan 1.1 is required")
        );
        assert_eq!(
            select_instance_api_version(vk::API_VERSION_1_1),
            Ok(vk::API_VERSION_1_1)
        );
        assert_eq!(
            select_instance_api_version(vk::API_VERSION_1_2),
            Ok(vk::API_VERSION_1_2)
        );
        assert_eq!(
            select_instance_api_version(vk::API_VERSION_1_3),
            Ok(vk::API_VERSION_1_3)
        );
        assert_eq!(select_instance_api_version(VULKAN_1_4), Ok(VULKAN_1_4));
        assert_eq!(
            select_instance_api_version(vk::make_api_version(0, 1, 5, 0)),
            Ok(VULKAN_1_4)
        );
    }

    #[test]
    fn falls_back_safely_when_maintenance6_descriptor_count_is_absent() {
        assert_eq!(resolve_external_sampler_descriptors(0, 0), 3);
        assert_eq!(resolve_external_sampler_descriptors(0, 8), 3);
        assert_eq!(resolve_external_sampler_descriptors(0x1234, 6), 6);
        assert_eq!(
            resolve_external_sampler_descriptors(0x1234, 0),
            FALLBACK_EXTERNAL_SAMPLER_DESCRIPTORS
        );
    }

    #[test]
    fn validates_android_import_without_requiring_timeline_or_foreign_queue() {
        assert_eq!(
            validate_android_import_requirements(true, true, true),
            Ok(())
        );
        assert_eq!(
            validate_android_import_requirements(false, true, true),
            Err("Vulkan 1.1 is required")
        );
        assert_eq!(
            validate_android_import_requirements(true, false, true),
            Err("AHardwareBuffer external memory is unavailable")
        );
        assert_eq!(
            validate_android_import_requirements(true, true, false),
            Err("sampler YCbCr conversion is unavailable")
        );
    }
}
