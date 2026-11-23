#![warn(clippy::undocumented_unsafe_blocks)]

mod error;
mod muxer;
mod plugin_export;

pub use plugin_export::vesper_native_plugin_entry;

pub use muxer::FfmpegRemuxProcessor;

#[cfg(test)]
mod tests {
    use super::vesper_native_plugin_entry;

    #[test]
    fn exports_plugin_entry() {
        let root = vesper_native_plugin_entry();
        assert!(!root.is_null());
        // SAFETY: the native entry returned one live root owner.
        let root = unsafe { root.read() };
        assert_eq!(root.abi_major, 2);
        assert_eq!(root.interface_count, 1);
        if let Some(destroy) = root.destroy_owner {
            // SAFETY: this test consumes the returned owner once.
            unsafe { destroy(root.owner) };
        }
    }
}
