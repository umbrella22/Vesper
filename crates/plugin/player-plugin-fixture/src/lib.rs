#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::io::Write;
use std::mem::size_of;
use std::ptr;
use std::time::Duration;

use player_plugin_abi::*;

const ID: &[u8] = b"dev.vesper.plugin-fixture";
const NAME: &[u8] = b"Vesper Plugin Fixture";
const CAPABILITIES: &[u8] = br#"{"supported_input_formats":["SingleFile"],"output_formats":[],"supports_cancellation":false}"#;
const SKIPPED: &[u8] = br#"{"Ok":"Skipped"}"#;

const fn bytes(value: &'static [u8]) -> VesperBytes {
    VesperBytes {
        data: value.as_ptr(),
        len: value.len() as u64,
    }
}

unsafe extern "C" fn capabilities(_owner: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    if out.is_null() {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: a non-null output is writable and aligned by the ABI contract.
    unsafe { out.write(bytes(CAPABILITIES)) };
    VESPER_OK
}

unsafe extern "C" fn process(
    _owner: *mut c_void,
    _request: VesperBytes,
    _progress: *const VesperProgressCallbacks,
    out: *mut VesperBytes,
) -> VesperStatus {
    if out.is_null() {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: a non-null output is writable and aligned by the ABI contract.
    unsafe { out.write(bytes(SKIPPED)) };
    VESPER_OK
}

unsafe extern "C" fn release(_owner: *mut c_void, _result: VesperBytes) {
    // The fixture's result refers to immutable static storage.
}

static TABLE: VesperPostDownloadProcessorVtable = VesperPostDownloadProcessorVtable {
    struct_size: size_of::<VesperPostDownloadProcessorVtable>() as u32,
    capabilities_json: Some(capabilities),
    process_json: Some(process),
    assemble_json: None,
    release_result: Some(release),
};

unsafe extern "C" fn interface_at(
    _owner: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceId,
) -> VesperStatus {
    if index != 0 || out.is_null() {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: a non-null output is writable and aligned by the ABI contract.
    unsafe { out.write(VESPER_POST_DOWNLOAD_PROCESSOR_ID) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _owner: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if id.is_null() || out.is_null() {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host provides a readable ID and a writable pointer output.
    unsafe {
        out.write(ptr::null());
        if id.read() != VESPER_POST_DOWNLOAD_PROCESSOR_ID {
            return VESPER_ERR_UNSUPPORTED;
        }
        out.write(ptr::from_ref(&TABLE).cast());
    }
    VESPER_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_native_plugin_entry() -> *const VesperPluginRoot {
    run_worker_fixture_behavior();
    &const {
        VesperPluginRoot {
            struct_size: size_of::<VesperPluginRoot>() as u32,
            abi_major: VESPER_PLUGIN_ABI_MAJOR,
            abi_minor: VESPER_PLUGIN_ABI_MINOR,
            owner: ptr::null_mut(),
            plugin_id: bytes(ID),
            plugin_name: bytes(NAME),
            interface_count: 1,
            interface_at: Some(interface_at),
            query_interface: Some(query),
            destroy_owner: None,
        }
    }
}

fn run_worker_fixture_behavior() {
    let Ok(behavior) = std::env::var("VESPER_PLUGIN_FIXTURE_WORKER_BEHAVIOR") else {
        return;
    };
    match behavior.as_str() {
        "abort" => std::process::abort(),
        "hang" => {
            write_worker_fixture_ready_pid();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "flood" => {
            let bytes = vec![b'x'; 300 * 1024];
            let _ = std::io::stdout().lock().write_all(&bytes);
            let _ = std::io::stderr().lock().write_all(&bytes);
        }
        "spawn-descendant" => spawn_worker_fixture_descendant(),
        "reserve-response" => reserve_worker_response_path(),
        _ => {}
    }
}

#[cfg(unix)]
fn spawn_worker_fixture_descendant() {
    let Ok(child) = std::process::Command::new("/bin/sleep").arg("60").spawn() else {
        return;
    };
    if let Ok(path) = std::env::var("VESPER_PLUGIN_FIXTURE_DESCENDANT_PID_PATH") {
        let _ = std::fs::write(path, child.id().to_string());
    }
}

#[cfg(not(unix))]
fn spawn_worker_fixture_descendant() {}

fn reserve_worker_response_path() {
    let mut arguments = std::env::args_os();
    while let Some(argument) = arguments.next() {
        if argument == "--response"
            && let Some(path) = arguments.next()
        {
            let _ = std::fs::write(path, b"reserved by fixture");
            return;
        }
    }
}

fn write_worker_fixture_ready_pid() {
    if let Ok(path) = std::env::var("VESPER_PLUGIN_FIXTURE_READY_PID_PATH") {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
}
