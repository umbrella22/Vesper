//! Raw C-compatible contracts for Vesper Plugin Platform.
//!
//! Plugin authors should depend on `player-plugin`, not this crate. This crate
//! intentionally exposes raw pointers and function tables for the generated
//! native boundary and the checked host loader.

#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::mem::size_of;

#[doc(hidden)]
pub mod export;
mod interfaces;

pub use interfaces::*;

/// NUL-terminated native entry symbol exported by every native plugin.
pub const VESPER_PLUGIN_ENTRY_SYMBOL: &[u8] = b"vesper_plugin_entry\0";
/// Human-readable native entry symbol without the terminating NUL byte.
pub const VESPER_PLUGIN_ENTRY_SYMBOL_NAME: &str = "vesper_plugin_entry";

/// Root ABI major implemented by this crate.
pub const VESPER_PLUGIN_ABI_MAJOR: u16 = 1;
/// Root ABI minor implemented by this crate.
pub const VESPER_PLUGIN_ABI_MINOR: u16 = 0;

/// Maximum UTF-8 byte length of a reverse-DNS plugin identity.
pub const VESPER_MAX_PLUGIN_ID_BYTES: usize = 255;
/// Maximum UTF-8 byte length of a human-readable plugin name.
pub const VESPER_MAX_PLUGIN_NAME_BYTES: usize = 255;
/// Maximum UTF-8 byte length of a capability instance identity.
pub const VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES: usize = 255;
/// Maximum number of interfaces exposed by one root.
pub const VESPER_MAX_INTERFACES_PER_PLUGIN: u32 = 64;
/// Maximum number of concurrently registered sessions per interface instance.
pub const VESPER_MAX_SESSIONS_PER_INTERFACE: usize = 1_024;
/// Maximum number of concurrently retained leases in one session.
pub const VESPER_MAX_LEASES_PER_SESSION: usize = 64;
/// Maximum size accepted for one plugin-owned ABI allocation.
pub const VESPER_MAX_OWNED_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum size accepted for one borrowed source-normalizer packet payload.
pub const VESPER_MAX_PACKET_BYTES: u64 = 16 * 1024 * 1024;

/// Raw status returned across the native ABI.
///
/// Callers must preserve unknown values so a newer plugin cannot be mistaken
/// for a successful older plugin.
pub type VesperStatus = u32;

/// Stable plugin status values.
pub mod status {
    use super::VesperStatus;

    pub const OK: VesperStatus = 0;
    pub const FAILURE: VesperStatus = 1;
    pub const INVALID_ARGUMENT: VesperStatus = 2;
    pub const INCOMPATIBLE: VesperStatus = 3;
    pub const STALE_HANDLE: VesperStatus = 4;
    pub const POISONED: VesperStatus = 5;
    pub const UNSUPPORTED: VesperStatus = 6;
    pub const CANCELLED: VesperStatus = 7;
    pub const TIMEOUT: VesperStatus = 8;
    pub const ABI_VIOLATION: VesperStatus = 9;
    pub const NOT_FOUND: VesperStatus = 10;
    pub const AMBIGUOUS: VesperStatus = 11;
    pub const EXHAUSTED: VesperStatus = 12;
    pub const PANIC: VesperStatus = 13;
}

/// Canonical UUID bytes identifying a typed plugin interface.
///
/// Bytes use RFC 4122 network order. Consumers must not reinterpret them as a
/// native-endian `u128`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VesperInterfaceId(pub [u8; 16]);

pub const POST_DOWNLOAD_PROCESSOR_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xe9, 0x47, 0x9d, 0xbc, 0x42, 0xd2, 0x57, 0x5e, 0xb3, 0x9e, 0xa2, 0x4b, 0xc5, 0x12, 0xfb, 0xc7,
]);
pub const PIPELINE_EVENT_HOOK_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xc7, 0xa6, 0x94, 0x75, 0x79, 0xb2, 0x5b, 0x5e, 0xa4, 0x77, 0x08, 0x84, 0x4a, 0x5d, 0xa5, 0xd1,
]);
pub const BENCHMARK_SINK_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0x2d, 0x8e, 0x5b, 0xe8, 0xb1, 0xde, 0x5e, 0x83, 0x8f, 0xe0, 0x61, 0x18, 0xaa, 0xbc, 0x51, 0x18,
]);
pub const NATIVE_DECODER_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xd6, 0x8b, 0xe0, 0xed, 0x19, 0x58, 0x59, 0x22, 0x8b, 0x7a, 0xbc, 0x67, 0x78, 0xa2, 0x6b, 0x43,
]);
pub const FRAME_PROCESSOR_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xfc, 0x05, 0x05, 0x97, 0xb7, 0xb7, 0x5c, 0x81, 0x83, 0xb9, 0xb4, 0x25, 0x55, 0xf8, 0xb8, 0x25,
]);
pub const SOURCE_NORMALIZER_PACKET_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xa2, 0xd6, 0x53, 0xfa, 0xd6, 0xce, 0x5f, 0x14, 0x93, 0xb8, 0xa8, 0x18, 0xa7, 0xa7, 0x7f, 0xdf,
]);
pub const SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([
    0xb7, 0x6d, 0x1f, 0x06, 0x62, 0xd7, 0x5d, 0x71, 0xaa, 0x06, 0x27, 0x80, 0xe4, 0xb4, 0xfd, 0x0d,
]);

/// Host-owned bytes borrowed for one synchronous ABI call.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VesperByteSlice {
    pub data: *const u8,
    pub len: u64,
}

impl VesperByteSlice {
    pub const fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }
}

impl Default for VesperByteSlice {
    fn default() -> Self {
        Self::empty()
    }
}

/// Plugin-owned bytes released through `VesperPluginRoot::free_bytes`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VesperOwnedBytes {
    pub data: *mut u8,
    pub len: u64,
}

impl VesperOwnedBytes {
    pub const fn empty() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
        }
    }

    #[doc(hidden)]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        let mut bytes = bytes.into_boxed_slice();
        let result = Self {
            data: bytes.as_mut_ptr(),
            len: bytes.len() as u64,
        };
        std::mem::forget(bytes);
        result
    }

    /// Reclaims bytes allocated by [`Self::from_vec`].
    ///
    /// # Safety
    ///
    /// The value must have been returned by `from_vec` in the current dynamic
    /// library and must not have been reclaimed previously.
    #[doc(hidden)]
    pub unsafe fn into_vec(self) -> Vec<u8> {
        if self.data.is_null() || self.len == 0 {
            return Vec::new();
        }
        let Ok(len) = usize::try_from(self.len) else {
            return Vec::new();
        };
        // SAFETY: guaranteed by the caller contract. `from_vec` transfers a
        // boxed slice with exactly this length.
        unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.data, len)).into_vec() }
    }
}

impl Default for VesperOwnedBytes {
    fn default() -> Self {
        Self::empty()
    }
}

/// Header prefix shared by every typed interface table.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperInterfaceHeader {
    pub struct_size: u32,
    pub interface_id: VesperInterfaceId,
    pub major: u16,
    pub minor: u16,
    pub context: *mut c_void,
}

impl VesperInterfaceHeader {
    pub const fn new(
        struct_size: u32,
        interface_id: VesperInterfaceId,
        major: u16,
        minor: u16,
        context: *mut c_void,
    ) -> Self {
        Self {
            struct_size,
            interface_id,
            major,
            minor,
            context,
        }
    }
}

/// Host-initialized result for root interface enumeration.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperInterfaceDescriptor {
    pub struct_size: u32,
    pub interface_id: VesperInterfaceId,
    pub major: u16,
    pub minor: u16,
    /// Borrowed reverse-DNS capability instance identity.
    pub instance_id: VesperByteSlice,
}

impl Default for VesperInterfaceDescriptor {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            interface_id: VesperInterfaceId([0; 16]),
            major: 0,
            minor: 0,
            instance_id: VesperByteSlice::empty(),
        }
    }
}

/// Stable native root returned by `vesper_plugin_entry`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPluginRoot {
    pub struct_size: u32,
    pub abi_major: u16,
    pub abi_minor: u16,
    /// Unique owner for all interfaces and plugin-owned byte allocations.
    pub owner: *mut c_void,
    /// Borrowed validated reverse-DNS plugin identity.
    pub plugin_id: VesperByteSlice,
    /// Borrowed human-readable plugin name.
    pub plugin_name: VesperByteSlice,
    pub interface_count: u32,
    pub reserved: u32,
    pub interface_at: Option<
        unsafe extern "C" fn(
            owner: *mut c_void,
            index: u32,
            out_descriptor: *mut VesperInterfaceDescriptor,
        ) -> VesperStatus,
    >,
    pub query_interface: Option<
        unsafe extern "C" fn(
            owner: *mut c_void,
            interface_id: *const VesperInterfaceId,
            instance_id: VesperByteSlice,
            requested_major: u16,
            minimum_minor: u16,
            out_interface: *mut *const VesperInterfaceHeader,
        ) -> VesperStatus,
    >,
    pub free_bytes: Option<unsafe extern "C" fn(owner: *mut c_void, bytes: VesperOwnedBytes)>,
    pub destroy_owner: Option<unsafe extern "C" fn(owner: *mut c_void)>,
}

/// Native plugin entry point signature.
pub type VesperPluginEntryPoint = unsafe extern "C" fn() -> *const VesperPluginRoot;

/// Returns the ABI size field for a concrete `repr(C)` type.
pub const fn abi_size<T>() -> u32 {
    size_of::<T>() as u32
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, offset_of, size_of};

    use super::*;

    #[test]
    fn interface_ids_use_canonical_uuid_bytes() {
        assert_eq!(
            POST_DOWNLOAD_PROCESSOR_INTERFACE_ID.0,
            [
                0xe9, 0x47, 0x9d, 0xbc, 0x42, 0xd2, 0x57, 0x5e, 0xb3, 0x9e, 0xa2, 0x4b, 0xc5, 0x12,
                0xfb, 0xc7,
            ]
        );
        assert_eq!(SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID.0[0], 0xb7);
        assert_eq!(SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID.0[15], 0x0d);
    }

    #[test]
    fn interface_header_is_a_stable_table_prefix() {
        assert_eq!(offset_of!(VesperInterfaceHeader, struct_size), 0);
        assert_eq!(offset_of!(VesperInterfaceHeader, interface_id), 4);
        assert_eq!(offset_of!(VesperInterfaceHeader, major), 20);
        assert_eq!(offset_of!(VesperInterfaceHeader, minor), 22);
        assert!(offset_of!(VesperInterfaceHeader, context) >= 24);
        assert_eq!(
            align_of::<VesperInterfaceHeader>(),
            align_of::<*mut c_void>()
        );
    }

    #[test]
    fn host_initialized_descriptor_advertises_its_capacity() {
        let descriptor = VesperInterfaceDescriptor::default();
        assert_eq!(
            descriptor.struct_size as usize,
            size_of::<VesperInterfaceDescriptor>()
        );
        assert_eq!(descriptor.interface_id, VesperInterfaceId([0; 16]));
        assert_eq!(descriptor.instance_id, VesperByteSlice::empty());
    }

    #[test]
    fn raw_status_keeps_unknown_values() {
        let unknown: VesperStatus = u32::MAX;
        assert_ne!(unknown, status::OK);
        assert_ne!(unknown, status::FAILURE);
    }

    #[test]
    fn root_places_size_and_version_before_pointer_fields() {
        assert_eq!(offset_of!(VesperPluginRoot, struct_size), 0);
        assert_eq!(offset_of!(VesperPluginRoot, abi_major), 4);
        assert_eq!(offset_of!(VesperPluginRoot, abi_minor), 6);
        assert_eq!(offset_of!(VesperPluginRoot, owner), 8);
        assert!(size_of::<VesperPluginRoot>() >= size_of::<VesperInterfaceHeader>());
    }
}
