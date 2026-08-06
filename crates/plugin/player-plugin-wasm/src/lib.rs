#![no_std]
#![warn(clippy::undocumented_unsafe_blocks)]

//! Safe guest runtime support for Vesper WASM Component plugins.
//!
//! Plugin authors use [`generate`] for WIT bindings and
//! [`export_component!`] for the final Component export. The crate owns the
//! allocator, panic, and canonical ABI reallocation boundary required by
//! `wasm32-wasip2`, so author crates can deny unsafe code.

extern crate alloc;

pub use wit_bindgen::generate;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2",
    target_feature = "atomics"
))]
compile_error!("Vesper WASM plugins do not support the wasm atomics target feature");

#[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
#[unsafe(export_name = "cabi_realloc")]
unsafe extern "C" fn canonical_abi_realloc(
    old_ptr: *mut u8,
    old_len: usize,
    align: usize,
    new_len: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, handle_alloc_error, realloc};

    if old_len == 0 {
        if new_len == 0 {
            return align as *mut u8;
        }
        let Ok(layout) = Layout::from_size_align(new_len, align) else {
            core::arch::wasm32::unreachable();
        };
        // SAFETY: `layout` was validated above and the returned allocation is
        // owned by the canonical ABI caller until it is passed back here.
        let pointer = unsafe { alloc(layout) };
        if pointer.is_null() {
            handle_alloc_error(layout);
        }
        return pointer;
    }

    if old_ptr.is_null() || new_len == 0 {
        core::arch::wasm32::unreachable();
    }
    let Ok(old_layout) = Layout::from_size_align(old_len, align) else {
        core::arch::wasm32::unreachable();
    };
    let Ok(new_layout) = Layout::from_size_align(new_len, align) else {
        core::arch::wasm32::unreachable();
    };
    // SAFETY: the Component canonical ABI only returns pointers allocated by
    // this function, and `old_layout` preserves the original size/alignment.
    let pointer = unsafe { realloc(old_ptr, old_layout, new_len) };
    if pointer.is_null() {
        handle_alloc_error(new_layout);
    }
    pointer
}

#[doc(hidden)]
pub mod rt {
    pub use wit_bindgen::rt::*;

    pub fn maybe_link_cabi_realloc() {
        #[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
        {
            let realloc = super::canonical_abi_realloc
                as unsafe extern "C" fn(*mut u8, usize, usize, usize) -> *mut u8;
            core::hint::black_box(realloc);
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2")))]
        wit_bindgen::rt::maybe_link_cabi_realloc();
    }
}

/// Exports a generated Vesper Component while linking the bounded guest
/// runtime required by `wasm32-wasip2`.
///
/// The first argument is the module produced by [`generate`]. The second is
/// the type implementing the generated guest traits.
#[macro_export]
macro_rules! export_component {
    ($bindings:ident, $component:ident) => {
        #[cfg(not(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2")))]
        compile_error!("Vesper WASM plugins must target wasm32-wasip2");

        #[allow(unsafe_code)]
        mod __vesper_generated_component_export {
            use super::{$bindings, $component};

            $bindings::export!($component with_types_in $bindings);
        }
    };
}
