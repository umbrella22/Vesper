#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread;
use std::time::Duration;

use player_plugin::{
    DecoderFrameFormat, DecoderMediaKind, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorFrameTimings, FrameProcessorOperationStatus, FrameProcessorReceiveFrameMetadata,
    FrameProcessorSessionConfig, FrameProcessorSessionInfo, FrameProcessorSubmitFrame,
    FrameProcessorSubmitResult, FrameProcessorSubmitStatus, NativeFrame, NativeFrameMetadata,
    NativeFrameReleaseTracking, NativeHandleKind, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
    VesperFrameProcessorOpenSessionResult, VesperFrameProcessorPluginApiV1,
    VesperFrameProcessorReceiveFrameResult, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-frame-processor-diagnostic\0";
const MODE_ENV: &str = "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE";
const SLOW_DELAY_MS_ENV: &str = "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_SLOW_MS";

struct PluginBundle {
    api: VesperFrameProcessorPluginApiV1,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticMode {
    Noop,
    Slow,
    UnsupportedHandle,
    LateOutput,
}

impl DiagnosticMode {
    fn from_env() -> Self {
        match std::env::var(MODE_ENV)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "slow" => Self::Slow,
            "unsupported-handle" | "unsupported" => Self::UnsupportedHandle,
            "late-output" | "late" => Self::LateOutput,
            _ => Self::Noop,
        }
    }
}

#[derive(Debug)]
struct DiagnosticSession {
    mode: DiagnosticMode,
    pending_outputs: VecDeque<NativeFrame>,
    source_frame_ids: VecDeque<Option<u64>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperFrameProcessorPluginApiV1 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(processor_name),
            capabilities_json: Some(processor_capabilities_json),
            open_session_json: Some(processor_open_session_json),
            submit_frame_json: Some(processor_submit_frame_json),
            receive_frame: Some(processor_receive_frame),
            release_frame: Some(processor_release_frame),
            flush_session: Some(processor_flush_session),
            close_session: Some(processor_close_session),
            free_bytes: Some(free_plugin_bytes),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api =
        (&bundle.api as *const VesperFrameProcessorPluginApiV1).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn processor_name(_context: *mut c_void) -> *const c_char {
    catch_unwind(AssertUnwindSafe(|| PLUGIN_NAME.as_ptr().cast::<c_char>()))
        .unwrap_or(std::ptr::null())
}

unsafe extern "C" fn processor_capabilities_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_processor_bytes(|| {
        let mode = DiagnosticMode::from_env();
        serialize_payload(&FrameProcessorCapabilities {
            accepted_input_handle_kinds: match mode {
                DiagnosticMode::UnsupportedHandle => vec![NativeHandleKind::D3D11Texture2D],
                _ => vec![
                    NativeHandleKind::CvPixelBuffer,
                    NativeHandleKind::IoSurface,
                    NativeHandleKind::D3D11Texture2D,
                ],
            },
            output_handle_kinds: vec![
                NativeHandleKind::CvPixelBuffer,
                NativeHandleKind::IoSurface,
                NativeHandleKind::D3D11Texture2D,
            ],
            supports_video_frames: true,
            supports_in_place_passthrough: true,
            preserves_dimensions: true,
            may_change_dimensions: false,
            preserves_color_metadata: true,
            preserves_hdr_metadata: true,
            supports_flush: true,
            max_sessions: Some(1),
            max_in_flight_frames: Some(1),
        })
    })
}

unsafe extern "C" fn processor_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperFrameProcessorOpenSessionResult {
    catch_processor_open(|| {
        let config = match decode_json::<FrameProcessorSessionConfig>(config_json, config_json_len)
        {
            Ok(config) => config,
            Err(error) => return open_error(error),
        };
        let mode = DiagnosticMode::from_env();
        let capabilities = diagnostic_capabilities(mode);
        if !capabilities.supports_input_handle_kind(&config.input_metadata.handle_kind) {
            return open_error(FrameProcessorError::unsupported_handle(format!(
                "{:?}",
                config.input_metadata.handle_kind
            )));
        }

        let session = Box::into_raw(Box::new(DiagnosticSession {
            mode,
            pending_outputs: VecDeque::new(),
            source_frame_ids: VecDeque::new(),
        }));
        let info = FrameProcessorSessionInfo {
            processor_name: Some("player-frame-processor-diagnostic".to_owned()),
            selected_backend: Some(format!("{mode:?}")),
            output_handle_kind: Some(config.input_metadata.handle_kind),
            max_in_flight_frames: Some(1),
        };
        VesperFrameProcessorOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: serialize_payload(&info),
        }
    })
}

unsafe extern "C" fn processor_submit_frame_json(
    _context: *mut c_void,
    session: *mut c_void,
    submit_json: *const u8,
    submit_json_len: usize,
    handle: usize,
) -> VesperPluginProcessResult {
    catch_processor_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticSession>().as_mut() }) else {
            return process_error(FrameProcessorError::NotConfigured);
        };
        let submit = match decode_json::<FrameProcessorSubmitFrame>(submit_json, submit_json_len) {
            Ok(submit) => submit,
            Err(error) => return process_error(error),
        };
        if handle == 0 {
            return process_error(FrameProcessorError::abi_violation(
                "input frame handle must not be null",
            ));
        }
        if !diagnostic_capabilities(session.mode)
            .supports_input_handle_kind(&submit.metadata.handle_kind)
        {
            return process_success(&FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Rejected,
                queue_depth: Some(session.pending_outputs.len() as u32),
                in_flight_frames: Some(session.pending_outputs.len() as u32),
                message: Some("unsupported input handle kind".to_owned()),
            });
        }
        if session.pending_outputs.len() >= 1 {
            return process_success(&FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Backpressure,
                queue_depth: Some(session.pending_outputs.len() as u32),
                in_flight_frames: Some(session.pending_outputs.len() as u32),
                message: Some("diagnostic output is still pending".to_owned()),
            });
        }

        match session.mode {
            DiagnosticMode::Slow => thread::sleep(Duration::from_millis(slow_delay_ms())),
            DiagnosticMode::Noop
            | DiagnosticMode::UnsupportedHandle
            | DiagnosticMode::LateOutput => {}
        }

        let output = allocate_output_frame(&submit.metadata, handle);
        session.source_frame_ids.push_back(submit.metadata.frame_id);
        session.pending_outputs.push_back(output);

        process_success(&FrameProcessorSubmitResult {
            status: FrameProcessorSubmitStatus::Accepted,
            queue_depth: Some(session.pending_outputs.len() as u32),
            in_flight_frames: Some(session.pending_outputs.len() as u32),
            message: None,
        })
    })
}

unsafe extern "C" fn processor_receive_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperFrameProcessorReceiveFrameResult {
    catch_processor_receive(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticSession>().as_mut() }) else {
            return receive_error(FrameProcessorError::NotConfigured);
        };
        let Some(output) = session.pending_outputs.pop_front() else {
            return receive_success(&FrameProcessorReceiveFrameMetadata::pending(), 0);
        };
        let source_frame_id = session.source_frame_ids.pop_front().flatten();
        let process_time_us = match session.mode {
            DiagnosticMode::Slow => slow_delay_ms().saturating_mul(1_000),
            DiagnosticMode::LateOutput => 1_000_000,
            DiagnosticMode::Noop | DiagnosticMode::UnsupportedHandle => 100,
        };
        let mut metadata = FrameProcessorReceiveFrameMetadata::frame(output.metadata);
        metadata.source_frame_id = source_frame_id;
        metadata.timings = FrameProcessorFrameTimings {
            queue_wait_us: Some(0),
            process_time_us: Some(process_time_us),
            submit_to_ready_us: Some(process_time_us),
        };
        if session.mode == DiagnosticMode::LateOutput {
            metadata.message =
                Some("diagnostic output intentionally reports late timing".to_owned());
        }

        receive_success(&metadata, output.handle)
    })
}

unsafe extern "C" fn processor_release_frame(
    _context: *mut c_void,
    _session: *mut c_void,
    _handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    catch_processor_process(|| {
        if handle == 0 {
            return process_error(FrameProcessorError::abi_violation(
                "release_frame handle must not be null",
            ));
        }
        // SAFETY: output handles are allocated with `Box::into_raw` by this
        // diagnostic plugin and released exactly once through this callback.
        let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
        process_success(&FrameProcessorOperationStatus { completed: true })
    })
}

unsafe extern "C" fn processor_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_processor_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticSession>().as_mut() }) else {
            return process_error(FrameProcessorError::NotConfigured);
        };
        release_pending_outputs(session);
        process_success(&FrameProcessorOperationStatus { completed: true })
    })
}

unsafe extern "C" fn processor_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_processor_process(|| {
        if session.is_null() {
            return process_error(FrameProcessorError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // this plugin and close is called once by the host.
        let mut session = unsafe { Box::from_raw(session.cast::<DiagnosticSession>()) };
        release_pending_outputs(&mut session);
        process_success(&FrameProcessorOperationStatus { completed: true })
    })
}

fn diagnostic_capabilities(mode: DiagnosticMode) -> FrameProcessorCapabilities {
    FrameProcessorCapabilities {
        accepted_input_handle_kinds: match mode {
            DiagnosticMode::UnsupportedHandle => vec![NativeHandleKind::D3D11Texture2D],
            _ => vec![
                NativeHandleKind::CvPixelBuffer,
                NativeHandleKind::IoSurface,
                NativeHandleKind::D3D11Texture2D,
            ],
        },
        output_handle_kinds: vec![
            NativeHandleKind::CvPixelBuffer,
            NativeHandleKind::IoSurface,
            NativeHandleKind::D3D11Texture2D,
        ],
        supports_video_frames: true,
        supports_in_place_passthrough: true,
        preserves_dimensions: true,
        may_change_dimensions: false,
        preserves_color_metadata: true,
        preserves_hdr_metadata: true,
        supports_flush: true,
        max_sessions: Some(1),
        max_in_flight_frames: Some(1),
    }
}

fn allocate_output_frame(metadata: &NativeFrameMetadata, input_handle: usize) -> NativeFrame {
    let mut output_metadata = metadata.clone();
    output_metadata.frame_id = metadata.frame_id.or(Some(input_handle as u64));
    output_metadata.release_tracking = Some(NativeFrameReleaseTracking {
        frame_id: output_metadata.frame_id,
        requires_release: false,
    });
    NativeFrame {
        metadata: output_metadata,
        handle: input_handle,
    }
}

fn release_pending_outputs(session: &mut DiagnosticSession) {
    while let Some(output) = session.pending_outputs.pop_front() {
        if output
            .metadata
            .release_tracking
            .as_ref()
            .is_some_and(|tracking| tracking.requires_release)
        {
            // SAFETY: this branch only releases processor-owned outputs. The
            // current diagnostic modes return borrowed passthrough handles and
            // mark them as not requiring release.
            let _ = unsafe { Box::from_raw(output.handle as *mut Vec<u8>) };
        }
    }
    session.source_frame_ids.clear();
}

fn slow_delay_ms() -> u64 {
    std::env::var(SLOW_DELAY_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(50)
}

fn decode_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, FrameProcessorError> {
    if data.is_null() && len > 0 {
        return Err(FrameProcessorError::abi_violation(
            "plugin JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the ABI caller keeps the byte range alive for this synchronous
        // callback.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload)
        .map_err(|error| FrameProcessorError::payload_codec(error.to_string()))
}

fn open_error(error: FrameProcessorError) -> VesperFrameProcessorOpenSessionResult {
    VesperFrameProcessorOpenSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: serialize_payload(&error),
    }
}

fn process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(value),
    }
}

fn process_error(error: FrameProcessorError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: serialize_payload(&error),
    }
}

fn receive_success(
    metadata: &FrameProcessorReceiveFrameMetadata,
    handle: usize,
) -> VesperFrameProcessorReceiveFrameResult {
    VesperFrameProcessorReceiveFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: serialize_payload(metadata),
        handle,
    }
}

fn receive_error(error: FrameProcessorError) -> VesperFrameProcessorReceiveFrameResult {
    VesperFrameProcessorReceiveFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        handle: 0,
    }
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the payload was produced by this dynamic library and has not
        // been reclaimed yet.
        let _ = unsafe { payload.into_vec() };
    }));
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

fn catch_processor_bytes(operation: impl FnOnce() -> VesperPluginBytes) -> VesperPluginBytes {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        serialize_payload(&FrameProcessorError::abi_violation(
            "frame processor callback panicked",
        ))
    })
}

fn catch_processor_open(
    operation: impl FnOnce() -> VesperFrameProcessorOpenSessionResult,
) -> VesperFrameProcessorOpenSessionResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| open_error(FrameProcessorError::internal("callback panicked")))
}

fn catch_processor_process(
    operation: impl FnOnce() -> VesperPluginProcessResult,
) -> VesperPluginProcessResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| process_error(FrameProcessorError::internal("callback panicked")))
}

fn catch_processor_receive(
    operation: impl FnOnce() -> VesperFrameProcessorReceiveFrameResult,
) -> VesperFrameProcessorReceiveFrameResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| receive_error(FrameProcessorError::internal("callback panicked")))
}

#[allow(dead_code)]
fn sample_metadata() -> NativeFrameMetadata {
    NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Nv12,
        codec: "diagnostic-video".to_owned(),
        pts_us: Some(1_000),
        duration_us: Some(33_333),
        width: 2,
        height: 2,
        coded_width: Some(2),
        coded_height: Some(2),
        visible_rect: None,
        handle_kind: NativeHandleKind::IoSurface,
        frame_id: Some(1),
        release_tracking: Some(NativeFrameReleaseTracking {
            frame_id: Some(1),
            requires_release: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticMode, DiagnosticSession, allocate_output_frame, diagnostic_capabilities,
        release_pending_outputs, sample_metadata, slow_delay_ms,
    };
    use player_plugin::{
        FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveStatus, FrameProcessorSubmitFrame,
        NativeHandleKind,
    };

    #[test]
    fn default_mode_is_noop() {
        assert_eq!(DiagnosticMode::from_env(), DiagnosticMode::Noop);
    }

    #[test]
    fn default_slow_delay_is_stable() {
        assert_eq!(slow_delay_ms(), 50);
    }

    #[test]
    fn unsupported_mode_advertises_mismatched_handle() {
        let capabilities = diagnostic_capabilities(DiagnosticMode::UnsupportedHandle);

        assert!(!capabilities.supports_input_handle_kind(&NativeHandleKind::IoSurface));
        assert!(capabilities.supports_input_handle_kind(&NativeHandleKind::D3D11Texture2D));
    }

    #[test]
    fn sample_submit_metadata_is_video() {
        let submit = FrameProcessorSubmitFrame::new(sample_metadata());

        assert_eq!(submit.metadata.codec, "diagnostic-video");
    }

    #[test]
    fn diagnostic_session_starts_empty() {
        let session = DiagnosticSession {
            mode: DiagnosticMode::Noop,
            pending_outputs: std::collections::VecDeque::new(),
            source_frame_ids: std::collections::VecDeque::new(),
        };

        assert!(session.pending_outputs.is_empty());
    }

    #[test]
    fn diagnostic_output_preserves_shape_and_marks_passthrough_release_tracking() {
        let metadata = sample_metadata();
        let output = allocate_output_frame(&metadata, 99);

        assert_eq!(output.metadata.width, metadata.width);
        assert_eq!(output.metadata.height, metadata.height);
        assert_eq!(output.metadata.handle_kind, metadata.handle_kind);
        assert_eq!(output.metadata.frame_id, metadata.frame_id);
        assert_eq!(output.handle, 99);
        assert_eq!(
            output
                .metadata
                .release_tracking
                .as_ref()
                .map(|tracking| tracking.requires_release),
            Some(false)
        );
    }

    #[test]
    fn diagnostic_flush_drops_pending_outputs() {
        let mut session = DiagnosticSession {
            mode: DiagnosticMode::Noop,
            pending_outputs: std::collections::VecDeque::new(),
            source_frame_ids: std::collections::VecDeque::new(),
        };
        session
            .pending_outputs
            .push_back(allocate_output_frame(&sample_metadata(), 100));
        session.source_frame_ids.push_back(Some(7));

        release_pending_outputs(&mut session);

        assert!(session.pending_outputs.is_empty());
        assert!(session.source_frame_ids.is_empty());
    }

    #[test]
    fn diagnostic_receive_metadata_can_report_late_output() {
        let mut metadata = FrameProcessorReceiveFrameMetadata::frame(sample_metadata());
        metadata.timings.submit_to_ready_us = Some(1_000_000);
        metadata.source_frame_id = Some(1);
        metadata.message = Some("diagnostic output intentionally reports late timing".to_owned());

        assert_eq!(metadata.status, FrameProcessorReceiveStatus::Frame);
        assert_eq!(metadata.timings.submit_to_ready_us, Some(1_000_000));
        assert_eq!(metadata.source_frame_id, Some(1));
    }
}
