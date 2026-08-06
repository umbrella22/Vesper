//! Generated native export support. This module is not an author-facing API.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::*;

const MAX_EXPORTED_INTERFACES: usize = 64;

struct ExportCallScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportInterfaceKind {
    PostDownloadProcessor,
    PipelineEventHook,
    BenchmarkSink,
    NativeDecoder,
    FrameProcessor,
    SourceNormalizerPacket,
    SourceNormalizerResource,
}

impl ExportInterfaceKind {
    pub const fn interface_id(self) -> VesperInterfaceId {
        match self {
            Self::PostDownloadProcessor => POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
            Self::PipelineEventHook => PIPELINE_EVENT_HOOK_INTERFACE_ID,
            Self::BenchmarkSink => BENCHMARK_SINK_INTERFACE_ID,
            Self::NativeDecoder => NATIVE_DECODER_INTERFACE_ID,
            Self::FrameProcessor => FRAME_PROCESSOR_INTERFACE_ID,
            Self::SourceNormalizerPacket => SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
            Self::SourceNormalizerResource => SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
        }
    }
}

#[derive(Debug)]
pub struct ExportFailure {
    status: VesperStatus,
    payload: Vec<u8>,
}

impl ExportFailure {
    pub fn failure(payload: Vec<u8>) -> Self {
        Self {
            status: status::FAILURE,
            payload,
        }
    }

    pub fn with_status(status: VesperStatus, payload: Vec<u8>) -> Self {
        let status = if status == status::OK || status > status::PANIC {
            status::FAILURE
        } else {
            status
        };
        Self { status, payload }
    }

    #[doc(hidden)]
    pub const fn status(&self) -> VesperStatus {
        self.status
    }

    #[doc(hidden)]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Debug)]
pub enum ExportInvocation {
    Json(Vec<u8>),
    OpenSession {
        session_id: VesperSessionId,
        payload: Vec<u8>,
    },
    NativeFrame {
        metadata: Vec<u8>,
        native_handle: u64,
        lease_id: VesperLeaseId,
        requires_release: bool,
    },
    PcmFrame {
        metadata: Vec<u8>,
        data: Vec<u8>,
    },
    Packet {
        metadata: Vec<u8>,
        data: Vec<u8>,
        lease_id: VesperLeaseId,
    },
}

/// Records resource-state changes performed by a generated interface call.
///
/// The raw ABI uses this signal to update its borrowed-buffer registry only
/// after the Safe SDK has changed the corresponding author-side lease state.
#[derive(Debug, Default)]
pub struct ExportCallEffects {
    packet_lease_state_changed: Cell<bool>,
}

impl ExportCallEffects {
    pub fn mark_packet_lease_state_changed(&self) {
        self.packet_lease_state_changed.set(true);
    }

    fn packet_lease_state_changed(&self) -> bool {
        self.packet_lease_state_changed.get()
    }
}

#[derive(Debug)]
pub enum ExportOperation<'a> {
    Capabilities,
    NativeRequirements,
    PostDownloadProcess {
        input_json: &'a [u8],
        output_path: &'a [u8],
        progress: ExportProgress<'a>,
        assemble: bool,
    },
    PipelineEvent {
        event_json: &'a [u8],
    },
    BenchmarkBatch {
        batch_json: &'a [u8],
    },
    BenchmarkFlush,
    OpenSession {
        config_json: &'a [u8],
    },
    DecoderSendPacket {
        session_id: VesperSessionId,
        packet_json: &'a [u8],
        packet_data: &'a [u8],
    },
    DecoderReceiveNativeFrame {
        session_id: VesperSessionId,
    },
    DecoderReceivePcmFrame {
        session_id: VesperSessionId,
    },
    DecoderReleaseNativeFrame {
        session_id: VesperSessionId,
        lease_id: VesperLeaseId,
        disposition: u32,
    },
    FrameSubmit {
        session_id: VesperSessionId,
        submit_json: &'a [u8],
        native_handle: u64,
    },
    FrameReceive {
        session_id: VesperSessionId,
    },
    FrameRelease {
        session_id: VesperSessionId,
        lease_id: VesperLeaseId,
    },
    PacketRead {
        session_id: VesperSessionId,
    },
    PacketRelease {
        session_id: VesperSessionId,
        lease_id: VesperLeaseId,
        effects: &'a ExportCallEffects,
    },
    PacketSeek {
        session_id: VesperSessionId,
        seek_json: &'a [u8],
        effects: &'a ExportCallEffects,
    },
    ResourcePoll {
        session_id: VesperSessionId,
    },
    ResourceWait {
        session_id: VesperSessionId,
        timeout_ms: u64,
    },
    ResourceCancel {
        session_id: VesperSessionId,
    },
    SessionFlush {
        session_id: VesperSessionId,
        effects: &'a ExportCallEffects,
    },
    SessionClose {
        session_id: VesperSessionId,
        effects: &'a ExportCallEffects,
    },
}

impl ExportOperation<'_> {
    fn is_cleanup(&self) -> bool {
        matches!(
            self,
            Self::DecoderReleaseNativeFrame { .. }
                | Self::FrameRelease { .. }
                | Self::PacketRelease { .. }
                | Self::ResourceCancel { .. }
                | Self::SessionFlush { .. }
                | Self::SessionClose { .. }
        )
    }
}

pub trait ExportInterface: Send + Sync {
    fn kind(&self) -> ExportInterfaceKind;
    fn instance_id(&self) -> &str;
    fn minor_version(&self) -> u16 {
        VESPER_INTERFACE_MINOR
    }
    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure>;
}

pub trait ExportPlugin: Send + Sync + 'static {
    fn plugin_id(&self) -> &str;
    fn plugin_name(&self) -> &str;
    fn interfaces(&self) -> Vec<Arc<dyn ExportInterface>>;
}

#[derive(Debug, Clone, Copy)]
pub struct ExportProgress<'a> {
    callbacks: Option<VesperProgressCallbacks>,
    scope: PhantomData<&'a ExportCallScope>,
}

// SAFETY: the raw progress callback contract requires the host context and
// callbacks to support concurrent scoped calls until the enclosing processing
// callback returns. The lifetime marker prevents use after that return.
unsafe impl Send for ExportProgress<'_> {}
// SAFETY: same host callback contract and lifetime bound as the `Send` impl.
unsafe impl Sync for ExportProgress<'_> {}

impl ExportProgress<'_> {
    fn none() -> Self {
        Self {
            callbacks: None,
            scope: PhantomData,
        }
    }

    pub fn on_progress(&self, ratio: f64) {
        let Some(callbacks) = self.callbacks else {
            return;
        };
        let Some(callback) = callbacks.on_progress else {
            return;
        };
        // SAFETY: the host callback context is borrowed for the synchronous
        // plugin call that owns this `ExportProgress` value.
        let _ = unsafe { callback(callbacks.context, ratio) };
    }

    pub fn is_cancelled(&self) -> bool {
        let Some(callbacks) = self.callbacks else {
            return false;
        };
        let Some(callback) = callbacks.is_cancelled else {
            return false;
        };
        let mut cancelled = 0;
        // SAFETY: the output and callback context are live for this synchronous
        // call. Callback failure is treated as cancellation.
        let result = unsafe { callback(callbacks.context, &mut cancelled) };
        result != status::OK || cancelled != 0
    }
}

enum RawTable {
    PostDownload(VesperPostDownloadProcessor),
    PipelineEventHook(VesperPipelineEventHook),
    BenchmarkSink(VesperBenchmarkSink),
    NativeDecoder(VesperNativeDecoder),
    FrameProcessor(VesperFrameProcessor),
    SourceNormalizerPacket(VesperSourceNormalizerPacket),
    SourceNormalizerResource(VesperSourceNormalizerResource),
}

impl RawTable {
    fn header(&self) -> &VesperInterfaceHeader {
        match self {
            Self::PostDownload(table) => &table.header,
            Self::PipelineEventHook(table) => &table.header,
            Self::BenchmarkSink(table) => &table.header,
            Self::NativeDecoder(table) => &table.header,
            Self::FrameProcessor(table) => &table.header,
            Self::SourceNormalizerPacket(table) => &table.header,
            Self::SourceNormalizerResource(table) => &table.header,
        }
    }

    fn header_mut(&mut self) -> &mut VesperInterfaceHeader {
        match self {
            Self::PostDownload(table) => &mut table.header,
            Self::PipelineEventHook(table) => &mut table.header,
            Self::BenchmarkSink(table) => &mut table.header,
            Self::NativeDecoder(table) => &mut table.header,
            Self::FrameProcessor(table) => &mut table.header,
            Self::SourceNormalizerPacket(table) => &mut table.header,
            Self::SourceNormalizerResource(table) => &mut table.header,
        }
    }
}

struct ExportContext {
    interface: Arc<dyn ExportInterface>,
    instance_id: Box<[u8]>,
    poisoned: AtomicBool,
    packet_buffers: Mutex<HashMap<(VesperSessionId, VesperLeaseId), Box<[u8]>>>,
    table: RawTable,
}

// SAFETY: `table` is immutable after construction, the interface is `Send +
// Sync`, and poisoning uses an atomic flag. Raw context pointers only point
// back to this stable boxed value.
unsafe impl Send for ExportContext {}
// SAFETY: same synchronization and stable-address argument as above.
unsafe impl Sync for ExportContext {}

struct ExportOwner {
    plugin_id: Box<[u8]>,
    plugin_name: Box<[u8]>,
    contexts: Vec<Box<ExportContext>>,
    root: VesperPluginRoot,
}

// SAFETY: contexts satisfy the root factory `Send + Sync` contract and the
// remaining fields are immutable byte storage and function pointers.
unsafe impl Send for ExportOwner {}
// SAFETY: same reasoning as above.
unsafe impl Sync for ExportOwner {}

/// Builds one root owner and returns its root pointer. Generated entry points
/// call this function so author factories cannot unwind across the C ABI.
pub fn export_plugin<P>(factory: impl FnOnce() -> P) -> *const VesperPluginRoot
where
    P: ExportPlugin,
{
    catch_unwind(AssertUnwindSafe(|| build_owner(factory())))
        .ok()
        .and_then(Result::ok)
        .unwrap_or(std::ptr::null())
}

fn build_owner(plugin: impl ExportPlugin) -> Result<*const VesperPluginRoot, ()> {
    let interfaces = plugin.interfaces();
    let plugin_id = plugin.plugin_id().as_bytes().to_vec().into_boxed_slice();
    let plugin_name = plugin.plugin_name().as_bytes().to_vec().into_boxed_slice();
    // Drop the author factory value before allocating and leaking the owner. A
    // panicking destructor is caught by `export_plugin` while all cloned
    // interface references are still owned by ordinary stack values.
    drop(plugin);
    if interfaces.is_empty() || interfaces.len() > MAX_EXPORTED_INTERFACES {
        return Err(());
    }
    let mut contexts = Vec::with_capacity(interfaces.len());
    for interface in interfaces {
        let instance_id = interface
            .instance_id()
            .as_bytes()
            .to_vec()
            .into_boxed_slice();
        let mut context = Box::new(ExportContext {
            table: table_for(interface.kind(), interface.minor_version()),
            interface,
            instance_id,
            poisoned: AtomicBool::new(false),
            packet_buffers: Mutex::new(HashMap::new()),
        });
        let context_ptr = std::ptr::from_mut(context.as_mut()).cast::<c_void>();
        context.table.header_mut().context = context_ptr;
        contexts.push(context);
    }
    let mut owner = Box::new(ExportOwner {
        root: VesperPluginRoot {
            struct_size: abi_size::<VesperPluginRoot>(),
            abi_major: VESPER_PLUGIN_ABI_MAJOR,
            abi_minor: VESPER_PLUGIN_ABI_MINOR,
            owner: std::ptr::null_mut(),
            plugin_id: VesperByteSlice::empty(),
            plugin_name: VesperByteSlice::empty(),
            interface_count: contexts.len() as u32,
            reserved: 0,
            interface_at: Some(export_interface_at),
            query_interface: Some(export_query_interface),
            free_bytes: Some(export_free_bytes),
            destroy_owner: Some(export_destroy_owner),
        },
        plugin_id,
        plugin_name,
        contexts,
    });
    let owner_ptr = std::ptr::from_mut(owner.as_mut());
    owner.root.owner = owner_ptr.cast();
    owner.root.plugin_id = byte_slice(&owner.plugin_id);
    owner.root.plugin_name = byte_slice(&owner.plugin_name);
    let root_ptr = std::ptr::from_ref(&owner.root);
    std::mem::forget(owner);
    Ok(root_ptr)
}

fn table_for(kind: ExportInterfaceKind, minor: u16) -> RawTable {
    let header = |struct_size, interface_id| {
        VesperInterfaceHeader::new(
            struct_size,
            interface_id,
            VESPER_INTERFACE_MAJOR,
            minor,
            std::ptr::null_mut(),
        )
    };
    match kind {
        ExportInterfaceKind::PostDownloadProcessor => {
            RawTable::PostDownload(VesperPostDownloadProcessor {
                header: header(
                    abi_size::<VesperPostDownloadProcessor>(),
                    POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
                ),
                capabilities_json: Some(export_capabilities),
                process_json: Some(export_post_download_process),
                assemble_json: Some(export_post_download_assemble),
            })
        }
        ExportInterfaceKind::PipelineEventHook => {
            RawTable::PipelineEventHook(VesperPipelineEventHook {
                header: header(
                    abi_size::<VesperPipelineEventHook>(),
                    PIPELINE_EVENT_HOOK_INTERFACE_ID,
                ),
                on_event_json: Some(export_pipeline_event),
            })
        }
        ExportInterfaceKind::BenchmarkSink => RawTable::BenchmarkSink(VesperBenchmarkSink {
            header: header(
                abi_size::<VesperBenchmarkSink>(),
                BENCHMARK_SINK_INTERFACE_ID,
            ),
            on_event_batch_json: Some(export_benchmark_batch),
            flush_json: Some(export_benchmark_flush),
        }),
        ExportInterfaceKind::NativeDecoder => RawTable::NativeDecoder(VesperNativeDecoder {
            header: header(
                abi_size::<VesperNativeDecoder>(),
                NATIVE_DECODER_INTERFACE_ID,
            ),
            capabilities_json: Some(export_capabilities),
            native_requirements_json: Some(export_native_requirements),
            open_session_json: Some(export_open_session),
            send_packet: Some(export_decoder_send_packet),
            receive_native_frame: Some(export_decoder_receive_native_frame),
            release_native_frame: Some(export_decoder_release_native_frame),
            flush_session: Some(export_session_flush),
            close_session: Some(export_session_close),
            receive_pcm_frame: Some(export_decoder_receive_pcm_frame),
        }),
        ExportInterfaceKind::FrameProcessor => RawTable::FrameProcessor(VesperFrameProcessor {
            header: header(
                abi_size::<VesperFrameProcessor>(),
                FRAME_PROCESSOR_INTERFACE_ID,
            ),
            capabilities_json: Some(export_capabilities),
            open_session_json: Some(export_open_session),
            submit_frame_json: Some(export_frame_submit),
            receive_frame: Some(export_frame_receive),
            release_frame: Some(export_frame_release),
            flush_session: Some(export_session_flush),
            close_session: Some(export_session_close),
        }),
        ExportInterfaceKind::SourceNormalizerPacket => {
            RawTable::SourceNormalizerPacket(VesperSourceNormalizerPacket {
                header: header(
                    abi_size::<VesperSourceNormalizerPacket>(),
                    SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
                ),
                capabilities_json: Some(export_capabilities),
                open_session_json: Some(export_open_session),
                read_packet: Some(export_packet_read),
                release_packet: Some(export_packet_release),
                flush_session: Some(export_session_flush),
                close_session: Some(export_session_close),
                seek_session_json: Some(export_packet_seek),
            })
        }
        ExportInterfaceKind::SourceNormalizerResource => {
            RawTable::SourceNormalizerResource(VesperSourceNormalizerResource {
                header: header(
                    abi_size::<VesperSourceNormalizerResource>(),
                    SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
                ),
                capabilities_json: Some(export_capabilities),
                open_session_json: Some(export_open_session),
                poll_session: Some(export_resource_poll),
                wait_session_update: Some(export_resource_wait),
                cancel_session: Some(export_resource_cancel),
                close_session: Some(export_session_close),
            })
        }
    }
}

unsafe extern "C" fn export_interface_at(
    owner: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceDescriptor,
) -> VesperStatus {
    catch_status(|| {
        let owner =
            // SAFETY: validated by the root callback contract.
            unsafe { owner_ref(owner)? };
        let context = owner
            .contexts
            .get(index as usize)
            .ok_or(status::NOT_FOUND)?;
        let out =
            // SAFETY: host owns and initializes the output for this call.
            unsafe { prepare_out(out)? };
        let header = context.table.header();
        *out = VesperInterfaceDescriptor {
            struct_size: abi_size::<VesperInterfaceDescriptor>(),
            interface_id: header.interface_id,
            major: header.major,
            minor: header.minor,
            instance_id: byte_slice(&context.instance_id),
        };
        Ok(status::OK)
    })
}

unsafe extern "C" fn export_query_interface(
    owner: *mut c_void,
    interface_id: *const VesperInterfaceId,
    instance_id: VesperByteSlice,
    requested_major: u16,
    minimum_minor: u16,
    out: *mut *const VesperInterfaceHeader,
) -> VesperStatus {
    catch_status(|| {
        let owner =
            // SAFETY: validated by the root callback contract.
            unsafe { owner_ref(owner)? };
        if interface_id.is_null() || out.is_null() {
            return Err(status::INVALID_ARGUMENT);
        }
        // SAFETY: output is non-null and host-owned for this call. Clearing it
        // prevents a failed query from accidentally reusing a stale table.
        unsafe { *out = std::ptr::null() };
        // SAFETY: pointer is borrowed and non-null for this call.
        let interface_id = unsafe { *interface_id };
        let scope = ExportCallScope;
        let instance_id =
            // SAFETY: host bytes are borrowed for this call.
            unsafe { input_slice(instance_id, &scope)? };
        let Some(context) = owner.contexts.iter().find(|context| {
            let header = context.table.header();
            header.interface_id == interface_id
                && header.major == requested_major
                && header.minor >= minimum_minor
                && context.instance_id.as_ref() == instance_id
        }) else {
            return Err(status::NOT_FOUND);
        };
        // SAFETY: output pointer is non-null and host-owned.
        unsafe { *out = std::ptr::from_ref(context.table.header()) };
        Ok(status::OK)
    })
}

unsafe extern "C" fn export_free_bytes(_owner: *mut c_void, bytes: VesperOwnedBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: root outputs are allocated with `VesperOwnedBytes::from_vec`
        // and transfer ownership back exactly once.
        drop(unsafe { bytes.into_vec() });
    }));
}

unsafe extern "C" fn export_destroy_owner(owner: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !owner.is_null() {
            // SAFETY: the root owner is allocated by `build_owner` and this
            // callback is invoked exactly once by the checked loader owner.
            drop(unsafe { Box::from_raw(owner.cast::<ExportOwner>()) });
        }
    }));
}

unsafe extern "C" fn export_capabilities(
    context: *mut c_void,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: forwarded ABI pointers retain their callback contracts.
    unsafe { invoke_json(context, ExportOperation::Capabilities, out) }
}

unsafe extern "C" fn export_native_requirements(
    context: *mut c_void,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: forwarded ABI pointers retain their callback contracts.
    unsafe { invoke_json(context, ExportOperation::NativeRequirements, out) }
}

unsafe extern "C" fn export_post_download_process(
    context: *mut c_void,
    input_json: VesperByteSlice,
    output_path: VesperByteSlice,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: all inputs are borrowed for this synchronous callback.
    unsafe { export_post_download(context, input_json, output_path, progress, out, false) }
}

unsafe extern "C" fn export_post_download_assemble(
    context: *mut c_void,
    input_json: VesperByteSlice,
    output_path: VesperByteSlice,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: all inputs are borrowed for this synchronous callback.
    unsafe { export_post_download(context, input_json, output_path, progress, out, true) }
}

unsafe fn export_post_download(
    context: *mut c_void,
    input_json: VesperByteSlice,
    output_path: VesperByteSlice,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperJsonOut,
    assemble: bool,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(input_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(input_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    let Ok(output_path) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(output_path, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    let Ok(progress) =
        // SAFETY: callbacks are borrowed for this call.
        (unsafe { read_progress(progress, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::PostDownloadProcess {
                input_json,
                output_path,
                progress,
                assemble,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_pipeline_event(
    context: *mut c_void,
    event_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(event_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(event_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_json(context, ExportOperation::PipelineEvent { event_json }, out) }
}

unsafe extern "C" fn export_benchmark_batch(
    context: *mut c_void,
    batch_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(batch_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(batch_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_json(context, ExportOperation::BenchmarkBatch { batch_json }, out) }
}

unsafe extern "C" fn export_benchmark_flush(
    context: *mut c_void,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_json(context, ExportOperation::BenchmarkFlush, out) }
}

unsafe extern "C" fn export_open_session(
    context: *mut c_void,
    config_json: VesperByteSlice,
    out: *mut VesperOpenSessionOut,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(config_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(config_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_open(context, ExportOperation::OpenSession { config_json }, out) }
}

unsafe extern "C" fn export_decoder_send_packet(
    context: *mut c_void,
    session_id: VesperSessionId,
    packet_json: VesperByteSlice,
    packet_data: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    if packet_data.len > VESPER_MAX_PACKET_BYTES {
        return status::INVALID_ARGUMENT;
    }
    let scope = ExportCallScope;
    let (Ok(packet_json), Ok(packet_data)) = (
        // SAFETY: inputs are borrowed for this call.
        unsafe { input_slice(packet_json, &scope) },
        // SAFETY: inputs are borrowed for this call.
        unsafe { input_slice(packet_data, &scope) },
    ) else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::DecoderSendPacket {
                session_id,
                packet_json,
                packet_data,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_decoder_receive_native_frame(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperNativeFrameOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_native_frame(
            context,
            ExportOperation::DecoderReceiveNativeFrame { session_id },
            out,
        )
    }
}

unsafe extern "C" fn export_decoder_receive_pcm_frame(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperPcmFrameOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_pcm_frame(
            context,
            ExportOperation::DecoderReceivePcmFrame { session_id },
            out,
        )
    }
}

unsafe extern "C" fn export_decoder_release_native_frame(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: VesperLeaseId,
    disposition: u32,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::DecoderReleaseNativeFrame {
                session_id,
                lease_id,
                disposition,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_frame_submit(
    context: *mut c_void,
    session_id: VesperSessionId,
    submit_json: VesperByteSlice,
    native_handle: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(submit_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(submit_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::FrameSubmit {
                session_id,
                submit_json,
                native_handle,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_frame_receive(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperNativeFrameOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_native_frame(context, ExportOperation::FrameReceive { session_id }, out) }
}

unsafe extern "C" fn export_frame_release(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: VesperLeaseId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::FrameRelease {
                session_id,
                lease_id,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_packet_read(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperPacketOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_packet(context, ExportOperation::PacketRead { session_id }, out) }
}

unsafe extern "C" fn export_packet_release(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: VesperLeaseId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    if !has_packet_buffer(context, session_id, lease_id) {
        return status::STALE_HANDLE;
    }
    let effects = ExportCallEffects::default();
    // SAFETY: output retains its host-owned callback contract.
    let result = unsafe {
        invoke_json(
            context,
            ExportOperation::PacketRelease {
                session_id,
                lease_id,
                effects: &effects,
            },
            out,
        )
    };
    if effects.packet_lease_state_changed() {
        remove_packet_buffer(context, session_id, lease_id);
    }
    result
}

unsafe extern "C" fn export_packet_seek(
    context: *mut c_void,
    session_id: VesperSessionId,
    seek_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let scope = ExportCallScope;
    let Ok(seek_json) =
        // SAFETY: input is borrowed for this call.
        (unsafe { input_slice(seek_json, &scope) })
    else {
        return status::INVALID_ARGUMENT;
    };
    let effects = ExportCallEffects::default();
    // SAFETY: output retains its host-owned callback contract.
    let result = unsafe {
        invoke_json(
            context,
            ExportOperation::PacketSeek {
                session_id,
                seek_json,
                effects: &effects,
            },
            out,
        )
    };
    if effects.packet_lease_state_changed() {
        remove_session_packet_buffers(context, session_id);
    }
    result
}

unsafe extern "C" fn export_resource_poll(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_json(context, ExportOperation::ResourcePoll { session_id }, out) }
}

unsafe extern "C" fn export_resource_wait(
    context: *mut c_void,
    session_id: VesperSessionId,
    timeout_ms: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe {
        invoke_json(
            context,
            ExportOperation::ResourceWait {
                session_id,
                timeout_ms,
            },
            out,
        )
    }
}

unsafe extern "C" fn export_resource_cancel(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    // SAFETY: output retains its host-owned callback contract.
    unsafe { invoke_json(context, ExportOperation::ResourceCancel { session_id }, out) }
}

unsafe extern "C" fn export_session_flush(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let effects = ExportCallEffects::default();
    // SAFETY: output retains its host-owned callback contract.
    let result = unsafe {
        invoke_json(
            context,
            ExportOperation::SessionFlush {
                session_id,
                effects: &effects,
            },
            out,
        )
    };
    if effects.packet_lease_state_changed() {
        remove_session_packet_buffers(context, session_id);
    }
    result
}

unsafe extern "C" fn export_session_close(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let effects = ExportCallEffects::default();
    // SAFETY: output retains its host-owned callback contract.
    let result = unsafe {
        invoke_json(
            context,
            ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            },
            out,
        )
    };
    if effects.packet_lease_state_changed() {
        remove_session_packet_buffers(context, session_id);
    }
    result
}

unsafe fn invoke_json(
    context: *mut c_void,
    operation: ExportOperation<'_>,
    out: *mut VesperJsonOut,
) -> VesperStatus {
    let out =
        // SAFETY: host owns and initializes this output.
        match unsafe { prepare_out(out) } {
            Ok(out) => out,
            Err(status) => return status,
        };
    match invoke(context, operation) {
        Ok(ExportInvocation::Json(payload)) => {
            out.payload = VesperOwnedBytes::from_vec(payload);
            status::OK
        }
        Ok(_) => poison_for_contract_violation(context),
        Err(failure) => {
            out.payload = VesperOwnedBytes::from_vec(failure.payload);
            failure.status
        }
    }
}

unsafe fn invoke_open(
    context: *mut c_void,
    operation: ExportOperation<'_>,
    out: *mut VesperOpenSessionOut,
) -> VesperStatus {
    let out =
        // SAFETY: host owns and initializes this output.
        match unsafe { prepare_out(out) } {
            Ok(out) => out,
            Err(status) => return status,
        };
    match invoke(context, operation) {
        Ok(ExportInvocation::OpenSession {
            session_id,
            payload,
        }) if session_id != 0 => {
            out.session_id = session_id;
            out.payload = VesperOwnedBytes::from_vec(payload);
            status::OK
        }
        Ok(_) => poison_for_contract_violation(context),
        Err(failure) => {
            out.payload = VesperOwnedBytes::from_vec(failure.payload);
            failure.status
        }
    }
}

unsafe fn invoke_native_frame(
    context: *mut c_void,
    operation: ExportOperation<'_>,
    out: *mut VesperNativeFrameOut,
) -> VesperStatus {
    let out =
        // SAFETY: host owns and initializes this output.
        match unsafe { prepare_out(out) } {
            Ok(out) => out,
            Err(status) => return status,
        };
    match invoke(context, operation) {
        Ok(ExportInvocation::NativeFrame {
            metadata,
            native_handle,
            lease_id,
            requires_release,
        }) if requires_release == (lease_id != 0) => {
            out.metadata = VesperOwnedBytes::from_vec(metadata);
            out.native_handle = native_handle;
            out.lease_id = lease_id;
            out.requires_release = u32::from(requires_release);
            status::OK
        }
        Ok(_) => poison_for_contract_violation(context),
        Err(failure) => {
            out.metadata = VesperOwnedBytes::from_vec(failure.payload);
            failure.status
        }
    }
}

unsafe fn invoke_pcm_frame(
    context: *mut c_void,
    operation: ExportOperation<'_>,
    out: *mut VesperPcmFrameOut,
) -> VesperStatus {
    let out =
        // SAFETY: host owns and initializes this output.
        match unsafe { prepare_out(out) } {
            Ok(out) => out,
            Err(status) => return status,
        };
    match invoke(context, operation) {
        Ok(ExportInvocation::PcmFrame { metadata, data }) => {
            out.metadata = VesperOwnedBytes::from_vec(metadata);
            out.data = VesperOwnedBytes::from_vec(data);
            status::OK
        }
        Ok(_) => poison_for_contract_violation(context),
        Err(failure) => {
            out.metadata = VesperOwnedBytes::from_vec(failure.payload);
            failure.status
        }
    }
}

unsafe fn invoke_packet(
    context: *mut c_void,
    operation: ExportOperation<'_>,
    out: *mut VesperPacketOut,
) -> VesperStatus {
    let ExportOperation::PacketRead { session_id } = operation else {
        return poison_for_contract_violation(context);
    };
    let out =
        // SAFETY: host owns and initializes this output.
        match unsafe { prepare_out(out) } {
            Ok(out) => out,
            Err(status) => return status,
        };
    match invoke(context, ExportOperation::PacketRead { session_id }) {
        Ok(ExportInvocation::Packet {
            metadata,
            data,
            lease_id,
        }) if lease_id == 0 && data.is_empty() => {
            out.metadata = VesperOwnedBytes::from_vec(metadata);
            status::OK
        }
        Ok(ExportInvocation::Packet {
            metadata,
            data,
            lease_id,
        }) if lease_id != 0 => {
            let Some(context_ref) = export_context(context) else {
                return status::INVALID_ARGUMENT;
            };
            let mut buffers = context_ref
                .packet_buffers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if buffers
                .keys()
                .filter(|(candidate_session_id, _)| *candidate_session_id == session_id)
                .count()
                >= VESPER_MAX_LEASES_PER_SESSION
                || buffers.contains_key(&(session_id, lease_id))
            {
                drop(buffers);
                return poison_for_contract_violation(context);
            }
            let data = data.into_boxed_slice();
            let borrowed_data = byte_slice(&data);
            buffers.insert((session_id, lease_id), data);
            out.metadata = VesperOwnedBytes::from_vec(metadata);
            out.data = borrowed_data;
            out.lease_id = lease_id;
            status::OK
        }
        Ok(_) => poison_for_contract_violation(context),
        Err(failure) => {
            out.metadata = VesperOwnedBytes::from_vec(failure.payload);
            failure.status
        }
    }
}

fn invoke(
    context: *mut c_void,
    operation: ExportOperation<'_>,
) -> Result<ExportInvocation, ExportFailure> {
    let Some(context) = NonNull::new(context.cast::<ExportContext>()) else {
        return Err(ExportFailure::with_status(
            status::INVALID_ARGUMENT,
            Vec::new(),
        ));
    };
    // SAFETY: table contexts point to stable boxed `ExportContext` values for
    // the complete root owner lifetime.
    let context = unsafe { context.as_ref() };
    if context.poisoned.load(Ordering::Acquire) && !operation.is_cleanup() {
        return Err(ExportFailure::with_status(status::POISONED, Vec::new()));
    }
    match catch_unwind(AssertUnwindSafe(|| context.interface.invoke(operation))) {
        Ok(Err(failure)) if failure.status == status::ABI_VIOLATION => {
            context.poisoned.store(true, Ordering::Release);
            Err(failure)
        }
        Ok(result) => result,
        Err(_) => {
            context.poisoned.store(true, Ordering::Release);
            Err(ExportFailure::with_status(status::PANIC, Vec::new()))
        }
    }
}

fn poison_for_contract_violation(context: *mut c_void) -> VesperStatus {
    if let Some(context) = NonNull::new(context.cast::<ExportContext>()) {
        // SAFETY: callback contexts always point to a live `ExportContext`.
        unsafe { context.as_ref() }
            .poisoned
            .store(true, Ordering::Release);
    }
    status::ABI_VIOLATION
}

unsafe fn owner_ref<'a>(owner: *mut c_void) -> Result<&'a ExportOwner, VesperStatus> {
    let owner = NonNull::new(owner.cast::<ExportOwner>()).ok_or(status::INVALID_ARGUMENT)?;
    // SAFETY: root callbacks receive the unique live owner pointer.
    Ok(unsafe { owner.as_ref() })
}

unsafe fn prepare_out<'a, T: Default>(out: *mut T) -> Result<&'a mut T, VesperStatus> {
    if out.is_null() {
        return Err(status::INVALID_ARGUMENT);
    }
    // SAFETY: all ABI output structs begin with `struct_size` and the host
    // guarantees the first word is readable.
    let struct_size = unsafe { out.cast::<u32>().read_unaligned() };
    if struct_size < abi_size::<T>() {
        return Err(status::ABI_VIOLATION);
    }
    // SAFETY: the advertised host capacity covers the complete output.
    unsafe { out.write(T::default()) };
    // SAFETY: output remains uniquely borrowed for this callback.
    Ok(unsafe { &mut *out })
}

unsafe fn input_slice<'a>(
    bytes: VesperByteSlice,
    _scope: &'a ExportCallScope,
) -> Result<&'a [u8], VesperStatus> {
    let len = usize::try_from(bytes.len).map_err(|_| status::INVALID_ARGUMENT)?;
    if len == 0 {
        return Ok(&[]);
    }
    if bytes.data.is_null() {
        return Err(status::INVALID_ARGUMENT);
    }
    // SAFETY: callback contracts borrow this readable range for the call.
    Ok(unsafe { std::slice::from_raw_parts(bytes.data, len) })
}

unsafe fn read_progress<'a>(
    progress: *const VesperProgressCallbacks,
    _scope: &'a ExportCallScope,
) -> Result<ExportProgress<'a>, VesperStatus> {
    if progress.is_null() {
        return Ok(ExportProgress::none());
    }
    // SAFETY: the host guarantees the first size word is readable.
    let struct_size = unsafe { progress.cast::<u32>().read_unaligned() };
    if struct_size < abi_size::<VesperProgressCallbacks>() {
        return Err(status::ABI_VIOLATION);
    }
    // SAFETY: the advertised size covers the complete callback table.
    let callbacks = unsafe { progress.read_unaligned() };
    Ok(ExportProgress {
        callbacks: Some(callbacks),
        scope: PhantomData,
    })
}

fn export_context(context: *mut c_void) -> Option<&'static ExportContext> {
    let context = NonNull::new(context.cast::<ExportContext>())?;
    // SAFETY: generated callback contexts point to stable boxed values owned
    // by the live plugin root. Callers never retain the returned reference.
    Some(unsafe { context.as_ref() })
}

fn has_packet_buffer(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: VesperLeaseId,
) -> bool {
    export_context(context).is_some_and(|context| {
        context
            .packet_buffers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(&(session_id, lease_id))
    })
}

fn remove_packet_buffer(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: VesperLeaseId,
) {
    if let Some(context) = export_context(context) {
        context
            .packet_buffers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&(session_id, lease_id));
    }
}

fn remove_session_packet_buffers(context: *mut c_void, session_id: VesperSessionId) {
    if let Some(context) = export_context(context) {
        context
            .packet_buffers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(candidate_session_id, _), _| *candidate_session_id != session_id);
    }
}

fn byte_slice(bytes: &[u8]) -> VesperByteSlice {
    if bytes.is_empty() {
        VesperByteSlice::empty()
    } else {
        VesperByteSlice {
            data: bytes.as_ptr(),
            len: bytes.len() as u64,
        }
    }
}

fn catch_status(f: impl FnOnce() -> Result<VesperStatus, VesperStatus>) -> VesperStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(status)) => status,
        Ok(Err(status)) => status,
        Err(_) => status::PANIC,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    struct HookInterface;

    impl ExportInterface for HookInterface {
        fn kind(&self) -> ExportInterfaceKind {
            ExportInterfaceKind::PipelineEventHook
        }

        fn instance_id(&self) -> &str {
            "dev.vesper.fixture.hook"
        }

        fn invoke(
            &self,
            operation: ExportOperation<'_>,
        ) -> Result<ExportInvocation, ExportFailure> {
            match operation {
                ExportOperation::PipelineEvent { event_json } => {
                    Ok(ExportInvocation::Json(event_json.to_vec()))
                }
                _ => Err(ExportFailure::failure(Vec::new())),
            }
        }
    }

    struct Plugin;

    impl ExportPlugin for Plugin {
        fn plugin_id(&self) -> &str {
            "dev.vesper.fixture"
        }

        fn plugin_name(&self) -> &str {
            "Fixture"
        }

        fn interfaces(&self) -> Vec<Arc<dyn ExportInterface>> {
            vec![Arc::new(HookInterface)]
        }
    }

    struct PacketInterface {
        closed: AtomicBool,
        panic_on_seek: AtomicBool,
        releases: AtomicUsize,
    }

    impl ExportInterface for PacketInterface {
        fn kind(&self) -> ExportInterfaceKind {
            ExportInterfaceKind::SourceNormalizerPacket
        }

        fn instance_id(&self) -> &str {
            "dev.vesper.fixture.packet"
        }

        fn invoke(
            &self,
            operation: ExportOperation<'_>,
        ) -> Result<ExportInvocation, ExportFailure> {
            match operation {
                ExportOperation::Capabilities => Ok(ExportInvocation::Json(b"{}".to_vec())),
                ExportOperation::OpenSession { .. } => Ok(ExportInvocation::OpenSession {
                    session_id: 0x0000_0001_0000_0001,
                    payload: b"{}".to_vec(),
                }),
                ExportOperation::PacketRead { .. } => Ok(ExportInvocation::Packet {
                    metadata: b"{\"status\":\"Packet\"}".to_vec(),
                    data: b"owned packet bytes".to_vec(),
                    lease_id: 17,
                }),
                ExportOperation::PacketRelease { effects, .. } => {
                    effects.mark_packet_lease_state_changed();
                    self.releases.fetch_add(1, Ordering::Relaxed);
                    Ok(ExportInvocation::Json(b"{}".to_vec()))
                }
                ExportOperation::PacketSeek {
                    seek_json, effects, ..
                } => {
                    if seek_json == b"malformed" {
                        return Err(ExportFailure::with_status(
                            status::INVALID_ARGUMENT,
                            Vec::new(),
                        ));
                    }
                    effects.mark_packet_lease_state_changed();
                    if self.panic_on_seek.load(Ordering::Acquire) {
                        panic!("fixture panic");
                    }
                    Ok(ExportInvocation::Json(b"{}".to_vec()))
                }
                ExportOperation::SessionClose { effects, .. } => {
                    effects.mark_packet_lease_state_changed();
                    self.closed.store(true, Ordering::Release);
                    Ok(ExportInvocation::Json(b"{}".to_vec()))
                }
                ExportOperation::SessionFlush { effects, .. } => {
                    effects.mark_packet_lease_state_changed();
                    Ok(ExportInvocation::Json(b"{}".to_vec()))
                }
                _ => Err(ExportFailure::with_status(status::UNSUPPORTED, Vec::new())),
            }
        }
    }

    struct PacketPlugin {
        interface: Arc<PacketInterface>,
    }

    impl ExportPlugin for PacketPlugin {
        fn plugin_id(&self) -> &str {
            "dev.vesper.packet-fixture"
        }

        fn plugin_name(&self) -> &str {
            "Packet Fixture"
        }

        fn interfaces(&self) -> Vec<Arc<dyn ExportInterface>> {
            vec![self.interface.clone()]
        }
    }

    struct InvalidFrameInterface;

    impl ExportInterface for InvalidFrameInterface {
        fn kind(&self) -> ExportInterfaceKind {
            ExportInterfaceKind::FrameProcessor
        }

        fn instance_id(&self) -> &str {
            "dev.vesper.fixture.invalid-frame"
        }

        fn invoke(
            &self,
            operation: ExportOperation<'_>,
        ) -> Result<ExportInvocation, ExportFailure> {
            match operation {
                ExportOperation::Capabilities => Ok(ExportInvocation::Json(b"{}".to_vec())),
                ExportOperation::OpenSession { .. } => Ok(ExportInvocation::OpenSession {
                    session_id: 1,
                    payload: b"{}".to_vec(),
                }),
                ExportOperation::FrameReceive { .. } => Ok(ExportInvocation::NativeFrame {
                    metadata: b"{}".to_vec(),
                    native_handle: 9,
                    lease_id: 3,
                    requires_release: false,
                }),
                ExportOperation::SessionClose { .. } => Ok(ExportInvocation::Json(b"{}".to_vec())),
                _ => Err(ExportFailure::with_status(status::UNSUPPORTED, Vec::new())),
            }
        }
    }

    struct InvalidFramePlugin;

    impl ExportPlugin for InvalidFramePlugin {
        fn plugin_id(&self) -> &str {
            "dev.vesper.invalid-frame-fixture"
        }

        fn plugin_name(&self) -> &str {
            "Invalid Frame Fixture"
        }

        fn interfaces(&self) -> Vec<Arc<dyn ExportInterface>> {
            vec![Arc::new(InvalidFrameInterface)]
        }
    }

    struct PanickingDropPlugin {
        interface: Arc<HookInterface>,
    }

    impl ExportPlugin for PanickingDropPlugin {
        fn plugin_id(&self) -> &str {
            "dev.vesper.drop-fixture"
        }

        fn plugin_name(&self) -> &str {
            "Drop Fixture"
        }

        fn interfaces(&self) -> Vec<Arc<dyn ExportInterface>> {
            vec![self.interface.clone()]
        }
    }

    impl Drop for PanickingDropPlugin {
        fn drop(&mut self) {
            panic!("drop panic");
        }
    }

    #[test]
    fn exported_root_enumerates_queries_invokes_and_frees() {
        let root_ptr = export_plugin(|| Plugin);
        assert!(!root_ptr.is_null());
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut descriptor = VesperInterfaceDescriptor::default();
        // SAFETY: root callbacks and host output follow the generated contract.
        assert_eq!(
            unsafe { root.interface_at.expect("interface_at")(root.owner, 0, &mut descriptor) },
            status::OK
        );
        let mut table = std::ptr::null();
        // SAFETY: descriptor bytes remain root-owned for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &descriptor.interface_id,
                    descriptor.instance_id,
                    descriptor.major,
                    descriptor.minor,
                    &mut table,
                )
            },
            status::OK
        );
        assert!(!table.is_null());
        // SAFETY: fixed interface id identifies the concrete hook table.
        let hook = unsafe { table.cast::<VesperPipelineEventHook>().read() };
        let input = b"{\"event\":\"ready\"}";
        let mut out = VesperJsonOut::default();
        // SAFETY: all callback pointers and borrowed inputs are valid here.
        assert_eq!(
            unsafe {
                hook.on_event_json.expect("on_event")(
                    hook.header.context,
                    byte_slice(input),
                    &mut out,
                )
            },
            status::OK
        );
        // SAFETY: callback output was allocated by the generated root.
        assert_eq!(unsafe { out.payload.into_vec() }, input);
        // SAFETY: owner is live and destroyed exactly once at the end.
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn undersized_out_is_rejected_without_overwriting_canary() {
        #[repr(C)]
        struct OutWithCanary {
            out: VesperJsonOut,
            canary: u64,
        }
        let root_ptr = export_plugin(|| Plugin);
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut descriptor = VesperInterfaceDescriptor::default();
        // SAFETY: generated callback contract.
        unsafe { root.interface_at.expect("interface_at")(root.owner, 0, &mut descriptor) };
        let mut table = std::ptr::null();
        // SAFETY: generated callback contract.
        unsafe {
            root.query_interface.expect("query")(
                root.owner,
                &descriptor.interface_id,
                descriptor.instance_id,
                descriptor.major,
                descriptor.minor,
                &mut table,
            )
        };
        // SAFETY: fixed interface id identifies the hook table.
        let hook = unsafe { table.cast::<VesperPipelineEventHook>().read() };
        let mut guarded = OutWithCanary {
            out: VesperJsonOut {
                struct_size: abi_size::<VesperJsonOut>() - 1,
                ..VesperJsonOut::default()
            },
            canary: 0xfeed_beef_dead_cafe,
        };
        // SAFETY: output prefix is readable but intentionally undersized.
        assert_eq!(
            unsafe {
                hook.on_event_json.expect("on_event")(
                    hook.header.context,
                    VesperByteSlice::empty(),
                    &mut guarded.out,
                )
            },
            status::ABI_VIOLATION
        );
        assert_eq!(guarded.out.payload, VesperOwnedBytes::empty());
        assert_eq!(guarded.canary, 0xfeed_beef_dead_cafe);
        // SAFETY: owner is live and destroyed once.
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn packet_storage_is_owner_backed_and_cleanup_survives_poisoning() {
        let interface = Arc::new(PacketInterface {
            closed: AtomicBool::new(false),
            panic_on_seek: AtomicBool::new(true),
            releases: AtomicUsize::new(0),
        });
        let root_ptr = export_plugin({
            let interface = interface.clone();
            move || PacketPlugin { interface }
        });
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut table = std::ptr::null();
        let instance = byte_slice(b"dev.vesper.fixture.packet");
        // SAFETY: root query inputs and output are valid for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
                    instance,
                    VESPER_INTERFACE_MAJOR,
                    0,
                    &mut table,
                )
            },
            status::OK
        );
        // SAFETY: fixed interface id identifies the packet table.
        let table = unsafe { table.cast::<VesperSourceNormalizerPacket>().read() };
        let mut open = VesperOpenSessionOut::default();
        // SAFETY: table callback and host output are valid.
        assert_eq!(
            unsafe {
                table.open_session_json.expect("open")(
                    table.header.context,
                    VesperByteSlice::empty(),
                    &mut open,
                )
            },
            status::OK
        );
        let mut packet = VesperPacketOut::default();
        // SAFETY: table callback and host output are valid.
        assert_eq!(
            unsafe {
                table.read_packet.expect("read")(table.header.context, open.session_id, &mut packet)
            },
            status::OK
        );
        // SAFETY: packet bytes remain export-owned until the matching release.
        let packet_bytes =
            unsafe { std::slice::from_raw_parts(packet.data.data, packet.data.len as usize) };
        assert_eq!(packet_bytes, b"owned packet bytes");

        let mut release = VesperJsonOut::default();
        // SAFETY: wrong-session values are deliberately passed to the checked trampoline.
        assert_eq!(
            unsafe {
                table.release_packet.expect("release")(
                    table.header.context,
                    open.session_id + 1,
                    packet.lease_id,
                    &mut release,
                )
            },
            status::STALE_HANDLE
        );
        assert_eq!(interface.releases.load(Ordering::Relaxed), 0);

        let mut seek = VesperJsonOut::default();
        // SAFETY: fixture intentionally panics inside this valid callback.
        assert_eq!(
            unsafe {
                table.seek_session_json.expect("seek")(
                    table.header.context,
                    open.session_id,
                    VesperByteSlice::empty(),
                    &mut seek,
                )
            },
            status::PANIC
        );
        // SAFETY: seek invalidates the ABI lease even when the implementation panics.
        assert_eq!(
            unsafe {
                table.release_packet.expect("release")(
                    table.header.context,
                    open.session_id,
                    packet.lease_id,
                    &mut release,
                )
            },
            status::STALE_HANDLE
        );
        assert_eq!(interface.releases.load(Ordering::Relaxed), 0);

        let mut close = VesperJsonOut::default();
        // SAFETY: close is a cleanup operation and the session is still live.
        assert_eq!(
            unsafe {
                table.close_session.expect("close")(
                    table.header.context,
                    open.session_id,
                    &mut close,
                )
            },
            status::OK
        );
        assert!(interface.closed.load(Ordering::Acquire));
        // SAFETY: owner is live and destroyed exactly once.
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn packet_seek_discards_export_buffers_before_the_next_read() {
        let interface = Arc::new(PacketInterface {
            closed: AtomicBool::new(false),
            panic_on_seek: AtomicBool::new(false),
            releases: AtomicUsize::new(0),
        });
        let root_ptr = export_plugin({
            let interface = interface.clone();
            move || PacketPlugin { interface }
        });
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut table = std::ptr::null();
        // SAFETY: root query inputs and output are valid for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
                    byte_slice(b"dev.vesper.fixture.packet"),
                    VESPER_INTERFACE_MAJOR,
                    0,
                    &mut table,
                )
            },
            status::OK
        );
        // SAFETY: fixed interface id identifies the packet table.
        let table = unsafe { table.cast::<VesperSourceNormalizerPacket>().read() };
        let mut open = VesperOpenSessionOut::default();
        // SAFETY: table callback and host output are valid.
        assert_eq!(
            unsafe {
                table.open_session_json.expect("open")(
                    table.header.context,
                    VesperByteSlice::empty(),
                    &mut open,
                )
            },
            status::OK
        );

        for _ in 0..=VESPER_MAX_LEASES_PER_SESSION {
            let mut packet = VesperPacketOut::default();
            // SAFETY: the previous seek invalidated the prior packet lease.
            assert_eq!(
                unsafe {
                    table.read_packet.expect("read")(
                        table.header.context,
                        open.session_id,
                        &mut packet,
                    )
                },
                status::OK
            );
            assert_eq!(packet.lease_id, 17);
            // SAFETY: metadata is generated-owner allocated and consumed once.
            let _ = unsafe { packet.metadata.into_vec() };

            let mut seek = VesperJsonOut::default();
            // SAFETY: seek input and output satisfy the callback contract.
            assert_eq!(
                unsafe {
                    table.seek_session_json.expect("seek")(
                        table.header.context,
                        open.session_id,
                        VesperByteSlice::empty(),
                        &mut seek,
                    )
                },
                status::OK
            );
            // SAFETY: payload is generated-owner allocated and consumed once.
            let _ = unsafe { seek.payload.into_vec() };
        }

        let mut close = VesperJsonOut::default();
        // SAFETY: close is called once for the live session.
        assert_eq!(
            unsafe {
                table.close_session.expect("close")(
                    table.header.context,
                    open.session_id,
                    &mut close,
                )
            },
            status::OK
        );
        assert!(interface.closed.load(Ordering::Acquire));
        // SAFETY: outputs and owner allocations are consumed exactly once.
        let _ = unsafe { open.payload.into_vec() };
        let _ = unsafe { close.payload.into_vec() };
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn packet_seek_keeps_export_buffer_until_author_lease_state_changes() {
        let interface = Arc::new(PacketInterface {
            closed: AtomicBool::new(false),
            panic_on_seek: AtomicBool::new(false),
            releases: AtomicUsize::new(0),
        });
        let root_ptr = export_plugin({
            let interface = interface.clone();
            move || PacketPlugin { interface }
        });
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut table = std::ptr::null();
        // SAFETY: root query inputs and output are valid for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
                    byte_slice(b"dev.vesper.fixture.packet"),
                    VESPER_INTERFACE_MAJOR,
                    0,
                    &mut table,
                )
            },
            status::OK
        );
        // SAFETY: fixed interface id identifies the packet table.
        let table = unsafe { table.cast::<VesperSourceNormalizerPacket>().read() };
        let mut open = VesperOpenSessionOut::default();
        // SAFETY: table callback and host output are valid for this call.
        assert_eq!(
            unsafe {
                table.open_session_json.expect("open")(
                    table.header.context,
                    VesperByteSlice::empty(),
                    &mut open,
                )
            },
            status::OK
        );

        let mut packet = VesperPacketOut::default();
        // SAFETY: session and host output are valid.
        assert_eq!(
            unsafe {
                table.read_packet.expect("read")(table.header.context, open.session_id, &mut packet)
            },
            status::OK
        );
        // SAFETY: metadata is generated-owner allocated and consumed once.
        let _ = unsafe { packet.metadata.into_vec() };

        let mut malformed_seek = VesperJsonOut::default();
        // SAFETY: malformed JSON bytes remain readable for this call.
        assert_eq!(
            unsafe {
                table.seek_session_json.expect("seek")(
                    table.header.context,
                    open.session_id,
                    byte_slice(b"malformed"),
                    &mut malformed_seek,
                )
            },
            status::INVALID_ARGUMENT
        );
        let mut release = VesperJsonOut::default();
        // SAFETY: rejected seek did not change author lease state, so release remains valid.
        assert_eq!(
            unsafe {
                table.release_packet.expect("release")(
                    table.header.context,
                    open.session_id,
                    packet.lease_id,
                    &mut release,
                )
            },
            status::OK
        );
        assert_eq!(interface.releases.load(Ordering::Relaxed), 1);
        // SAFETY: generated outputs are consumed exactly once.
        let _ = unsafe { malformed_seek.payload.into_vec() };
        let _ = unsafe { release.payload.into_vec() };

        let mut next_packet = VesperPacketOut::default();
        // SAFETY: the prior lease was released and the session remains live.
        assert_eq!(
            unsafe {
                table.read_packet.expect("read")(
                    table.header.context,
                    open.session_id,
                    &mut next_packet,
                )
            },
            status::OK
        );
        // SAFETY: metadata is generated-owner allocated and consumed once.
        let _ = unsafe { next_packet.metadata.into_vec() };

        let mut undersized_seek = VesperJsonOut::default();
        undersized_seek.struct_size = 0;
        // SAFETY: the deliberately undersized output exposes only its size word.
        assert_eq!(
            unsafe {
                table.seek_session_json.expect("seek")(
                    table.header.context,
                    open.session_id,
                    VesperByteSlice::empty(),
                    &mut undersized_seek,
                )
            },
            status::ABI_VIOLATION
        );
        let mut next_release = VesperJsonOut::default();
        // SAFETY: the rejected output prevented the interface call, so release remains valid.
        assert_eq!(
            unsafe {
                table.release_packet.expect("release")(
                    table.header.context,
                    open.session_id,
                    next_packet.lease_id,
                    &mut next_release,
                )
            },
            status::OK
        );
        assert_eq!(interface.releases.load(Ordering::Relaxed), 2);

        let mut close = VesperJsonOut::default();
        // SAFETY: close is called once for the live session.
        assert_eq!(
            unsafe {
                table.close_session.expect("close")(
                    table.header.context,
                    open.session_id,
                    &mut close,
                )
            },
            status::OK
        );
        assert!(interface.closed.load(Ordering::Acquire));
        // SAFETY: generated outputs and owner are consumed exactly once.
        let _ = unsafe { open.payload.into_vec() };
        let _ = unsafe { next_release.payload.into_vec() };
        let _ = unsafe { close.payload.into_vec() };
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn inconsistent_native_frame_lease_poisons_but_still_allows_close() {
        let root_ptr = export_plugin(|| InvalidFramePlugin);
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut table = std::ptr::null();
        // SAFETY: root query inputs and output are valid for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &FRAME_PROCESSOR_INTERFACE_ID,
                    byte_slice(b"dev.vesper.fixture.invalid-frame"),
                    VESPER_INTERFACE_MAJOR,
                    0,
                    &mut table,
                )
            },
            status::OK
        );
        // SAFETY: fixed interface id identifies the frame processor table.
        let table = unsafe { table.cast::<VesperFrameProcessor>().read() };
        let mut open = VesperOpenSessionOut::default();
        // SAFETY: callback contract is satisfied.
        assert_eq!(
            unsafe {
                table.open_session_json.expect("open")(
                    table.header.context,
                    VesperByteSlice::empty(),
                    &mut open,
                )
            },
            status::OK
        );
        let mut frame = VesperNativeFrameOut::default();
        // SAFETY: callback contract is satisfied; the fixture returns an invalid lease shape.
        assert_eq!(
            unsafe {
                table.receive_frame.expect("receive")(
                    table.header.context,
                    open.session_id,
                    &mut frame,
                )
            },
            status::ABI_VIOLATION
        );
        let mut close = VesperJsonOut::default();
        // SAFETY: cleanup remains valid after the contract violation poisoned the interface.
        assert_eq!(
            unsafe {
                table.close_session.expect("close")(
                    table.header.context,
                    open.session_id,
                    &mut close,
                )
            },
            status::OK
        );
        // SAFETY: owner is live and destroyed exactly once.
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn panicking_factory_drop_does_not_leak_cloned_interfaces() {
        let interface = Arc::new(HookInterface);
        let root_ptr = export_plugin({
            let interface = interface.clone();
            move || PanickingDropPlugin { interface }
        });
        assert!(root_ptr.is_null());
        assert_eq!(Arc::strong_count(&interface), 1);
    }

    #[test]
    fn failed_query_clears_the_output_pointer() {
        let root_ptr = export_plugin(|| Plugin);
        // SAFETY: export_plugin returned a live root owner.
        let root = unsafe { root_ptr.read() };
        let mut table = std::ptr::dangling();
        // SAFETY: root query inputs and output are valid for this call.
        assert_eq!(
            unsafe {
                root.query_interface.expect("query")(
                    root.owner,
                    &VesperInterfaceId([0xff; 16]),
                    byte_slice(b"dev.vesper.missing"),
                    VESPER_INTERFACE_MAJOR,
                    0,
                    &mut table,
                )
            },
            status::NOT_FOUND
        );
        assert!(table.is_null());
        // SAFETY: owner is live and destroyed exactly once.
        unsafe { root.destroy_owner.expect("destroy")(root.owner) };
    }

    #[test]
    fn decoder_packet_limit_is_checked_before_borrowing_the_input_range() {
        let mut out = VesperJsonOut::default();
        let oversized = VesperByteSlice {
            data: std::ptr::dangling(),
            len: VESPER_MAX_PACKET_BYTES + 1,
        };

        // SAFETY: the oversized length must be rejected before the deliberately
        // non-dereferenceable input pointer is inspected.
        let result = unsafe {
            export_decoder_send_packet(
                std::ptr::null_mut(),
                1,
                VesperByteSlice::empty(),
                oversized,
                &mut out,
            )
        };

        assert_eq!(result, status::INVALID_ARGUMENT);
    }
}
