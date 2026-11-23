//! Native export support. All exported sessions are directly owned by their caller.

use std::mem::size_of;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use super::author::*;
use super::checked::*;
use super::*;

mod audio;

struct Owner {
    root: VesperPluginRoot,
    plugin: Plugin,
    audio: Option<VesperAudioProcessorVtable>,
}

fn status(error: PluginError) -> VesperStatus {
    match error {
        PluginError::Status(code) | PluginError::StatusWithDetail(code, _) if code != VESPER_OK => {
            code
        }
        PluginError::Contract(_) => VESPER_ERR_INVALID_ARG,
        _ => VESPER_ERR_INTERNAL,
    }
}

fn catch<R>(call: impl FnOnce() -> R) -> Result<R, ()> {
    catch_unwind(AssertUnwindSafe(call)).map_err(|payload| {
        // A custom panic payload can panic from its destructor.
        std::mem::forget(payload);
    })
}

fn guarded(call: impl FnOnce() -> PluginResult<VesperStatus>) -> VesperStatus {
    match catch(call) {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => status(error),
        Err(()) => VESPER_ERR_PANIC,
    }
}

pub fn export_plugin(factory: fn() -> Plugin) -> *const VesperPluginRoot {
    catch(|| {
        let plugin = factory();
        if !valid_plugin_id(plugin.id) || plugin.name.is_empty() || plugin.name.len() > 255 {
            return ptr::null();
        }
        let audio = plugin
            .audio
            .as_ref()
            .map(|factory| audio::vtable(factory.capabilities()));
        let owner = Box::new(Owner {
            root: VesperPluginRoot {
                struct_size: size_of::<VesperPluginRoot>() as u32,
                abi_major: VESPER_PLUGIN_ABI_MAJOR,
                abi_minor: VESPER_PLUGIN_ABI_MINOR,
                owner: ptr::null_mut(),
                plugin_id: bytes(plugin.id.as_bytes()),
                plugin_name: bytes(plugin.name.as_bytes()),
                interface_count: u32::from(audio.is_some()),
                interface_at: Some(interface_at),
                query_interface: Some(query_interface),
                destroy_owner: Some(destroy_owner),
            },
            plugin,
            audio,
        });
        let raw = Box::into_raw(owner);
        // SAFETY: raw is the sole owner of a fully initialized allocation.
        unsafe {
            (*raw).root.owner = raw.cast();
            ptr::addr_of!((*raw).root)
        }
    })
    .unwrap_or(ptr::null())
}

unsafe fn owner<'a>(ctx: *mut c_void) -> PluginResult<&'a Owner> {
    let ctx = ctx.cast::<Owner>();
    if !sized_pointer(ctx) {
        return Err(PluginError::Contract("invalid plugin owner"));
    }
    // SAFETY: caller guarantees this context comes from a live export root.
    Ok(unsafe { &*ctx })
}

unsafe extern "C" fn interface_at(
    ctx: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceId,
) -> VesperStatus {
    guarded(|| {
        if !sized_pointer(out) {
            return Err(PluginError::Contract("invalid interface output"));
        }
        // SAFETY: a root call passes its own live owner.
        let owner = unsafe { owner(ctx) }?;
        if index != 0 || owner.audio.is_none() {
            return Err(PluginError::Status(VESPER_ERR_UNSUPPORTED));
        }
        // SAFETY: the caller provides aligned writable output for one ID.
        unsafe { out.write(VESPER_AUDIO_PROCESSOR_ID) };
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn query_interface(
    ctx: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    guarded(|| {
        if !sized_pointer(out) || !sized_pointer(id) {
            return Err(PluginError::Contract("invalid interface query"));
        }
        // SAFETY: the caller provides a readable ID and writable output.
        let id = unsafe {
            out.write(ptr::null());
            id.read()
        };
        // SAFETY: a root call passes its own live owner.
        let owner = unsafe { owner(ctx) }?;
        let table = owner
            .audio
            .as_ref()
            .filter(|_| id == VESPER_AUDIO_PROCESSOR_ID)
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        // SAFETY: the table is stored in the root allocation until destroy_owner.
        unsafe { out.write(ptr::from_ref(table).cast()) };
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn destroy_owner(ctx: *mut c_void) {
    if sized_pointer(ctx.cast::<Owner>()) {
        // SAFETY: the host relinquishes its sole ownership of this root exactly once.
        let _ = catch(|| unsafe { drop(Box::from_raw(ctx.cast::<Owner>())) });
    }
}
