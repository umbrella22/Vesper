use super::*;
use player_plugin_abi::checked::{MAX_JSON_BYTES, bytes, read_bytes, require_size, sized_pointer};
use player_plugin_abi::*;
use serde::de::DeserializeOwned;
use std::mem::size_of;
use std::panic::{AssertUnwindSafe, catch_unwind};

struct PacketSession {
    inner: packet_session::PacketSession,
    stream_info: Vec<u8>,
}

struct ResourceSession {
    inner: Box<dyn SourceNormalizerResourceSession>,
    status: Vec<u8>,
}

struct RootOwner {
    packet_capabilities: Vec<u8>,
    resource_capabilities: Vec<u8>,
}

unsafe fn capabilities(
    owner: *mut c_void,
    out: *mut VesperBytes,
    select: impl FnOnce(&RootOwner) -> &[u8],
) -> VesperStatus {
    if !sized_pointer(owner.cast::<RootOwner>()) || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the root retains the immutable capabilities until destruction.
    unsafe { out.write(bytes(select(&*owner.cast::<RootOwner>()))) };
    VESPER_OK
}

unsafe extern "C" fn packet_capabilities(
    owner: *mut c_void,
    out: *mut VesperBytes,
) -> VesperStatus {
    // SAFETY: the packet table and its context belong to the same root.
    unsafe { capabilities(owner, out, |owner| &owner.packet_capabilities) }
}

unsafe extern "C" fn resource_capabilities(
    owner: *mut c_void,
    out: *mut VesperBytes,
) -> VesperStatus {
    // SAFETY: the resource table and its context belong to the same root.
    unsafe { capabilities(owner, out, |owner| &owner.resource_capabilities) }
}

fn guarded(call: impl FnOnce() -> VesperStatus) -> VesperStatus {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|payload| {
        std::mem::forget(payload);
        VESPER_ERR_PANIC
    })
}

fn status(error: SourceNormalizerError) -> VesperStatus {
    use SourceNormalizerError::*;
    match error {
        InvalidInput { .. } | AbiViolation { .. } | Configuration { .. } => VESPER_ERR_INVALID_ARG,
        UnsupportedOperation { .. } | UnsupportedRuntimeProfile { .. } => VESPER_ERR_UNSUPPORTED,
        ResourceExhausted { .. } => VESPER_ERR_EXHAUSTED,
        Timeout { .. } => VESPER_ERR_ABORTED,
        _ => VESPER_ERR_INTERNAL,
    }
}

unsafe fn open<T, C>(
    uri: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
    operation: impl FnOnce(&str, C) -> Result<T, SourceNormalizerError>,
) -> VesperStatus
where
    C: DeserializeOwned,
{
    guarded(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: writable handle output supplied by the host.
        unsafe { out.write(ptr::null_mut()) };
        // SAFETY: both live byte ranges are borrowed only for the open call.
        let (Ok(uri), Ok(params)) = (unsafe { read_bytes(uri, 64 * 1024) }, unsafe {
            read_bytes(params, MAX_JSON_BYTES)
        }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let (Ok(uri), Ok(params)) = (std::str::from_utf8(uri), serde_json::from_slice(params))
        else {
            return VESPER_ERR_INVALID_ARG;
        };
        match operation(uri, params) {
            Ok(session) => {
                // SAFETY: transfer new unique ownership to the caller.
                unsafe { out.write(Box::into_raw(Box::new(session)).cast()) };
                VESPER_OK
            }
            Err(error) => status(error),
        }
    })
}

unsafe extern "C" fn open_packet(
    _owner: *mut c_void,
    uri: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
) -> VesperStatus {
    // SAFETY: this is the native open boundary and both views and output follow its contract.
    unsafe {
        open(
            uri,
            params,
            out,
            |uri, mut config: SourceNormalizerPacketSessionConfig| {
                config.input = uri.to_owned();
                player_plugin::validate_source_normalizer_plugin_input(uri, &config.headers)?;
                let inner = packet_session::PacketSession::open(&config)?;
                let stream_info = serde_json::to_vec(inner.get_stream_info())
                    .map_err(|error| SourceNormalizerError::payload_codec(error.to_string()))?;
                Ok(PacketSession { inner, stream_info })
            },
        )
    }
}

unsafe extern "C" fn open_resource(
    _owner: *mut c_void,
    uri: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
) -> VesperStatus {
    // SAFETY: this is the native open boundary and both views and output follow its contract.
    unsafe {
        open(
            uri,
            params,
            out,
            |uri, mut config: SourceNormalizerResourceSessionConfig| {
                config.input = uri.to_owned();
                player_plugin::validate_source_normalizer_plugin_input(uri, &config.headers)?;
                Ok(ResourceSession {
                    inner: FfmpegResourceFactory.open_resource_session(&config)?,
                    status: Vec::new(),
                })
            },
        )
    }
}

unsafe fn with_session<T>(
    raw: *mut VesperSession,
    call: impl FnOnce(&mut T) -> VesperStatus,
) -> VesperStatus {
    if !sized_pointer(raw.cast::<T>()) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host owns this type of session and serializes access via its vtable.
    guarded(|| call(unsafe { &mut *raw.cast::<T>() }))
}

unsafe extern "C" fn stream_info(raw: *mut VesperSession, out: *mut VesperBytes) -> VesperStatus {
    if !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: this operation is exposed only by the packet-session vtable.
    unsafe {
        with_session(raw, |session: &mut PacketSession| {
            out.write(bytes(&session.stream_info));
            VESPER_OK
        })
    }
}

unsafe extern "C" fn resource_status(
    raw: *mut VesperSession,
    out: *mut VesperBytes,
) -> VesperStatus {
    if !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: this operation is exposed only by the resource-session vtable.
    unsafe {
        with_session(raw, |session: &mut ResourceSession| {
            out.write(VesperBytes::default());
            let snapshot = match session.inner.poll() {
                Ok(snapshot) => snapshot,
                Err(error) => return status(error),
            };
            let Ok(encoded) = serde_json::to_vec(&snapshot) else {
                return VESPER_ERR_INTERNAL;
            };
            session.status = encoded;
            out.write(bytes(&session.status));
            VESPER_OK
        })
    }
}

unsafe extern "C" fn read_packet(
    raw: *mut VesperSession,
    out: *mut VesperPacket,
    id: *mut u64,
) -> VesperStatus {
    // SAFETY: out and id are writable sized ABI outputs supplied by the host.
    if !sized_pointer(id) || unsafe { require_size(out, size_of::<VesperPacket>()) }.is_err() {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the operation only uses the packet session allocated by open_packet.
    unsafe {
        with_session(raw, |session: &mut PacketSession| {
            id.write(0);
            out.write(VesperPacket {
                struct_size: size_of::<VesperPacket>() as u32,
                ..Default::default()
            });
            match session.inner.read_packet() {
                Ok(packet_session::Read::TryAgain) => VESPER_TRY_AGAIN,
                Ok(packet_session::Read::EndOfStream) => VESPER_EOF,
                Ok(packet_session::Read::Packet(packet)) => {
                    let time = |value: Option<i64>| value.unwrap_or(i64::MIN);
                    out.write(VesperPacket {
                        struct_size: size_of::<VesperPacket>() as u32,
                        flags: if packet.key_frame {
                            VESPER_PACKET_KEYFRAME
                        } else {
                            0
                        },
                        pts_us: time(packet.pts_us),
                        dts_us: time(packet.dts_us),
                        duration_us: time(packet.duration_us),
                        stream_index: packet.stream_index,
                        data: packet.data.as_ptr(),
                        len: packet.data.len() as u64,
                    });
                    id.write(packet.lease_id);
                    VESPER_OK
                }
                Err(error) => status(error),
            }
        })
    }
}

unsafe extern "C" fn release_packet(raw: *mut VesperSession, id: u64) -> VesperStatus {
    // SAFETY: this is a live serialized packet session from open_packet.
    unsafe {
        with_session(raw, |session: &mut PacketSession| {
            session
                .inner
                .release_packet(id)
                .map_or_else(status, |()| VESPER_OK)
        })
    }
}

unsafe extern "C" fn seek(raw: *mut VesperSession, pts: i64, flags: u32) -> VesperStatus {
    if pts < 0 || flags != VESPER_SEEK_BACKWARD {
        return VESPER_ERR_UNSUPPORTED;
    }
    // SAFETY: seek is exposed only on the packet session's vtable.
    unsafe {
        with_session(raw, |session: &mut PacketSession| {
            if session.inner.has_outstanding_packet() {
                return VESPER_TRY_AGAIN;
            }
            session.inner.seek(pts).map_or_else(status, |()| VESPER_OK)
        })
    }
}

unsafe extern "C" fn flush(raw: *mut VesperSession) -> VesperStatus {
    // SAFETY: flush is exposed only on the packet session's vtable.
    unsafe {
        with_session(raw, |session: &mut PacketSession| {
            if session.inner.has_outstanding_packet() {
                VESPER_TRY_AGAIN
            } else {
                VESPER_OK
            }
        })
    }
}

unsafe extern "C" fn destroy_packet(raw: *mut VesperSession) {
    if !raw.is_null() {
        // SAFETY: destroy consumes the unique packet session and there are no outstanding borrows.
        let _ = guarded(|| {
            unsafe { drop(Box::from_raw(raw.cast::<PacketSession>())) };
            VESPER_OK
        });
    }
}

unsafe extern "C" fn destroy_resource(raw: *mut VesperSession) {
    if !raw.is_null() {
        // SAFETY: destroy consumes the unique resource session after its consumer is closed.
        let _ = guarded(|| {
            unsafe { drop(Box::from_raw(raw.cast::<ResourceSession>())) };
            VESPER_OK
        });
    }
}

static PACKET: VesperSourceNormalizerVtable = VesperSourceNormalizerVtable {
    struct_size: size_of::<VesperSourceNormalizerVtable>() as u32,
    query_capabilities_json: Some(packet_capabilities),
    open: Some(open_packet),
    get_stream_info: Some(stream_info),
    read_packet: Some(read_packet),
    release_packet: Some(release_packet),
    seek: Some(seek),
    flush: Some(flush),
    destroy: Some(destroy_packet),
    get_last_error: None,
};
unsafe extern "C" fn wait_for_resource_update(
    raw: *mut VesperSession,
    timeout_ms: u32,
) -> VesperStatus {
    // SAFETY: the host exclusively owns a resource session and lends it for this call.
    unsafe {
        with_session(raw, |session: &mut ResourceSession| {
            session
                .inner
                .wait_for_update(std::time::Duration::from_millis(u64::from(timeout_ms)))
                .map_or_else(status, |wait| {
                    if wait.updated {
                        VESPER_OK
                    } else {
                        VESPER_TRY_AGAIN
                    }
                })
        })
    }
}

unsafe extern "C" fn cancel_resource(raw: *mut VesperSession) -> VesperStatus {
    // SAFETY: the host exclusively owns a resource session and lends it for this call.
    unsafe {
        with_session(raw, |session: &mut ResourceSession| {
            session.inner.cancel().map_or_else(status, |cancel| {
                if cancel.completed {
                    VESPER_OK
                } else {
                    VESPER_TRY_AGAIN
                }
            })
        })
    }
}

static RESOURCE: VesperSourceNormalizerResourceVtable = VesperSourceNormalizerResourceVtable {
    struct_size: size_of::<VesperSourceNormalizerResourceVtable>() as u32,
    query_capabilities_json: Some(resource_capabilities),
    open: Some(open_resource),
    get_resource_json: Some(resource_status),
    destroy: Some(destroy_resource),
    get_last_error: None,
    wait_for_update: Some(wait_for_resource_update),
    cancel: Some(cancel_resource),
};
const IDS: [VesperInterfaceId; 2] = [
    VESPER_SOURCE_NORMALIZER_ID,
    VESPER_SOURCE_NORMALIZER_RESOURCE_ID,
];

unsafe extern "C" fn interface_at(
    _owner: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceId,
) -> VesperStatus {
    let Some(id) = IDS.get(index as usize) else {
        return VESPER_ERR_INVALID_ARG;
    };
    if !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: caller provides a writable interface ID.
    unsafe { out.write(*id) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _owner: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if !sized_pointer(out) || !sized_pointer(id) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host supplies a readable id and a writable pointer result.
    unsafe {
        out.write(ptr::null());
        let interface = match id.read() {
            VESPER_SOURCE_NORMALIZER_ID => ptr::from_ref(&PACKET).cast(),
            VESPER_SOURCE_NORMALIZER_RESOURCE_ID => ptr::from_ref(&RESOURCE).cast(),
            _ => return VESPER_ERR_UNSUPPORTED,
        };
        out.write(interface);
    }
    VESPER_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_native_plugin_entry() -> *const VesperPluginRoot {
    catch_unwind(AssertUnwindSafe(|| {
        let Ok(packet_capabilities) =
            serde_json::to_vec(&FfmpegPacketFactory.packet_capabilities())
        else {
            return ptr::null();
        };
        let Ok(resource_capabilities) =
            serde_json::to_vec(&FfmpegResourceFactory.resource_capabilities())
        else {
            return ptr::null();
        };
        bind_root(Box::into_raw(Box::new(OwnedRoot {
            owner: RootOwner {
                packet_capabilities,
                resource_capabilities,
            },
            root: VesperPluginRoot {
                struct_size: size_of::<VesperPluginRoot>() as u32,
                abi_major: VESPER_PLUGIN_ABI_MAJOR,
                abi_minor: VESPER_PLUGIN_ABI_MINOR,
                owner: ptr::null_mut(),
                plugin_id: VesperBytes {
                    data: PLUGIN_ID.as_ptr(),
                    len: PLUGIN_ID.len() as u64,
                },
                plugin_name: VesperBytes {
                    data: PLUGIN_NAME.as_ptr(),
                    len: PLUGIN_NAME.len() as u64,
                },
                interface_count: IDS.len() as u32,
                interface_at: Some(interface_at),
                query_interface: Some(query),
                destroy_owner: Some(destroy_root),
            },
        })))
    }))
    .unwrap_or_else(|payload| {
        std::mem::forget(payload);
        ptr::null()
    })
}

// The context address is also the address of the allocation destroyed by the host.
#[repr(C)]
struct OwnedRoot {
    owner: RootOwner,
    root: VesperPluginRoot,
}

fn bind_root(root: *mut OwnedRoot) -> *const VesperPluginRoot {
    // SAFETY: root points to a fresh Box exclusively transferred by entry.
    unsafe {
        (*root).root.owner = root.cast();
        ptr::addr_of!((*root).root)
    }
}

unsafe extern "C" fn destroy_root(owner: *mut c_void) {
    if !owner.is_null() {
        // SAFETY: owner is transferred exactly once from this entry to destroy.
        unsafe { drop(Box::from_raw(owner.cast::<OwnedRoot>())) };
    }
}
