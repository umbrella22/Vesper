use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_uchar, c_void};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use player_core::MediaSource;
use player_platform_apple::{VIDEOTOOLBOX_BACKEND_NAME, probe_videotoolbox_hardware_decode};
use player_runtime::{
    PlayerAudioInfo, PlayerMediaInfo, PlayerRuntimeAdapterFactory, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PlayerVideoInfo, PlayerVideoSurfaceKind,
    PlayerVideoSurfaceTarget,
};

use crate::native::{
    MacosAvFoundationBridge, MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext,
    MacosAvFoundationSnapshot, MacosManagedNativeSessionController, MacosNativeCommandSink,
    MacosNativePlayerCommand, MacosNativePlayerProbe, MacosNativePlayerRuntimeAdapterFactory,
    MacosPlayerItemStatus, MacosTimeControlStatus,
};

pub fn macos_system_native_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: OnceLock<MacosNativePlayerRuntimeAdapterFactory> = OnceLock::new();
    FACTORY.get_or_init(|| {
        let bridge = MacosAvFoundationBridge::new(
            MacosAvFoundationBridgeContext {
                video_surface: None,
            },
            Arc::new(MacosSystemAvFoundationBridgeBindings),
        );
        MacosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(bridge))
    })
}

pub fn install_default_macos_system_native_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    player_runtime::register_default_runtime_adapter_factory(
        macos_system_native_runtime_adapter_factory(),
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosSystemAvFoundationBridgeBindings;

struct MacosSystemNativeCommandSink {
    session_handle: *mut c_void,
    callback_context: *mut MacosNativeCallbackContext,
}

struct MacosNativeCallbackContext {
    controller: MacosManagedNativeSessionController,
}

unsafe impl Send for MacosSystemNativeCommandSink {}

impl Drop for MacosSystemNativeCommandSink {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            player_macos_avfoundation_destroy_session(self.session_handle);
            drop(Box::from_raw(self.callback_context));
        }
    }
}

impl MacosNativeCommandSink for MacosSystemNativeCommandSink {
    fn submit_command(&mut self, command: MacosNativePlayerCommand) -> PlayerRuntimeResult<()> {
        #[cfg(target_os = "macos")]
        {
            let (succeeded, error_message) = match command {
                MacosNativePlayerCommand::Play => {
                    invoke_native_session_command(|buffer, len| unsafe {
                        player_macos_avfoundation_session_play(self.session_handle, buffer, len)
                    })
                }
                MacosNativePlayerCommand::Pause => {
                    invoke_native_session_command(|buffer, len| unsafe {
                        player_macos_avfoundation_session_pause(self.session_handle, buffer, len)
                    })
                }
                MacosNativePlayerCommand::SeekTo { position } => {
                    invoke_native_session_command(|buffer, len| unsafe {
                        player_macos_avfoundation_session_seek_to(
                            self.session_handle,
                            position.as_millis().min(u128::from(u64::MAX)) as u64,
                            buffer,
                            len,
                        )
                    })
                }
                MacosNativePlayerCommand::Stop => {
                    invoke_native_session_command(|buffer, len| unsafe {
                        player_macos_avfoundation_session_stop(self.session_handle, buffer, len)
                    })
                }
                MacosNativePlayerCommand::SetPlaybackRate { rate } => {
                    invoke_native_session_command(|buffer, len| unsafe {
                        player_macos_avfoundation_session_set_playback_rate(
                            self.session_handle,
                            rate,
                            buffer,
                            len,
                        )
                    })
                }
            };

            if succeeded {
                return Ok(());
            }

            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                if error_message.is_empty() {
                    "AVFoundation session command failed".to_owned()
                } else {
                    error_message
                },
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = command;
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "AVFoundation session commands are only available on macOS targets",
            ))
        }
    }

    fn attach_video_surface(
        &mut self,
        video_surface: PlayerVideoSurfaceTarget,
    ) -> PlayerRuntimeResult<()> {
        #[cfg(target_os = "macos")]
        {
            let surface = MacosAvFoundationSurfaceTarget::from_runtime_surface(video_surface);
            let (succeeded, error_message) = invoke_native_session_command(|buffer, len| unsafe {
                player_macos_avfoundation_session_attach_surface(
                    self.session_handle,
                    surface,
                    buffer,
                    len,
                )
            });

            if succeeded {
                return Ok(());
            }

            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                if error_message.is_empty() {
                    "AVFoundation failed to attach the requested video surface".to_owned()
                } else {
                    error_message
                },
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = video_surface;
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "AVFoundation surface attachment is only available on macOS targets",
            ))
        }
    }

    fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
        #[cfg(target_os = "macos")]
        {
            let (succeeded, error_message) = invoke_native_session_command(|buffer, len| unsafe {
                player_macos_avfoundation_session_detach_surface(self.session_handle, buffer, len)
            });

            if succeeded {
                return Ok(());
            }

            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                if error_message.is_empty() {
                    "AVFoundation failed to detach the current video surface".to_owned()
                } else {
                    error_message
                },
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "AVFoundation surface detachment is only available on macOS targets",
            ))
        }
    }
}

impl MacosAvFoundationBridgeBindings for MacosSystemAvFoundationBridgeBindings {
    fn probe_source(
        &self,
        _context: &MacosAvFoundationBridgeContext,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<MacosNativePlayerProbe> {
        probe_source_with_avfoundation(source)
    }

    fn create_command_sink(
        &self,
        context: MacosAvFoundationBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
        controller: MacosManagedNativeSessionController,
    ) -> PlayerRuntimeResult<Box<dyn MacosNativeCommandSink>> {
        let surface = if media_info.best_video.is_some() {
            context
                .video_surface
                .or(options.video_surface)
                .ok_or_else(|| {
                    PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidArgument,
                        "macos native playback requires a video surface target",
                    )
                })?
        } else {
            context
                .video_surface
                .or(options.video_surface)
                .unwrap_or(PlayerVideoSurfaceTarget {
                    kind: PlayerVideoSurfaceKind::PlayerLayer,
                    handle: 0,
                })
        };

        #[cfg(target_os = "macos")]
        {
            let source_c_string = CString::new(source.uri()).map_err(|_| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::InvalidSource,
                    "media source contains an interior NUL byte and cannot be passed to AVFoundation",
                )
            })?;
            let callback_context =
                Box::into_raw(Box::new(MacosNativeCallbackContext { controller }));
            let callbacks = MacosAvFoundationCallbacks {
                on_snapshot: Some(macos_on_snapshot),
                on_first_frame_ready: Some(macos_on_first_frame_ready),
                on_interruption_changed: Some(macos_on_interruption_changed),
                on_seek_completed: Some(macos_on_seek_completed),
                on_error: Some(macos_on_error),
                context: callback_context.cast(),
            };
            let surface = MacosAvFoundationSurfaceTarget::from_runtime_surface(surface);
            let mut session_handle = std::ptr::null_mut();
            let mut error_message = [0 as c_char; 256];
            let created = unsafe {
                player_macos_avfoundation_create_session(
                    source_c_string.as_ptr(),
                    surface,
                    callbacks,
                    &mut session_handle,
                    error_message.as_mut_ptr(),
                    error_message.len(),
                )
            };
            if !created {
                unsafe {
                    drop(Box::from_raw(callback_context));
                }
                return Err(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    c_string_buffer_to_string(&error_message),
                ));
            }

            return Ok(Box::new(MacosSystemNativeCommandSink {
                session_handle,
                callback_context,
            }));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = source;
            let _ = surface;
            let _ = controller;
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "AVFoundation session wiring is only available on macOS targets",
            ))
        }
    }
}

pub fn probe_source_with_avfoundation(
    source: &MediaSource,
) -> PlayerRuntimeResult<MacosNativePlayerProbe> {
    #[cfg(target_os = "macos")]
    {
        let source_c_string = CString::new(source.uri()).map_err(|_| {
            PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidSource,
                "media source contains an interior NUL byte and cannot be passed to AVFoundation",
            )
        })?;

        let mut probe = MacosAvFoundationProbeResult::default();
        let succeeded =
            unsafe { player_macos_avfoundation_probe(source_c_string.as_ptr(), &mut probe) };
        if !succeeded {
            let message = c_string_buffer_to_string(&probe.error_message);
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidSource,
                if message.is_empty() {
                    "AVFoundation failed to probe the media source".to_owned()
                } else {
                    message
                },
            ));
        }

        let media_info = PlayerMediaInfo {
            source_uri: source.uri().to_owned(),
            source_kind: source.kind(),
            source_protocol: source.protocol(),
            duration: (probe.has_duration != 0).then_some(Duration::from_millis(probe.duration_ms)),
            bit_rate: (probe.has_bit_rate != 0).then_some(probe.bit_rate),
            audio_streams: probe.audio_streams as usize,
            video_streams: probe.video_streams as usize,
            best_video: probe.video.present().then(|| PlayerVideoInfo {
                codec: probe.video.codec_string(),
                width: probe.video.width,
                height: probe.video.height,
                frame_rate: (probe.video.frame_rate > 0.0).then_some(probe.video.frame_rate),
            }),
            best_audio: probe.audio.present().then(|| PlayerAudioInfo {
                codec: probe.audio.codec_string(),
                sample_rate: probe.audio.sample_rate,
                channels: probe.audio.channels,
            }),
            track_catalog: Default::default(),
            track_selection: Default::default(),
        };

        let video_decode = media_info.best_video.as_ref().map(native_video_decode_info);

        Ok(MacosNativePlayerProbe {
            media_info,
            startup: PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode,
                plugin_diagnostics: Vec::new(),
            },
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = source;
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "AVFoundation probing is only available on macOS targets",
        ))
    }
}

fn native_video_decode_info(video: &PlayerVideoInfo) -> PlayerVideoDecodeInfo {
    let support = probe_videotoolbox_hardware_decode(&video.codec);
    PlayerVideoDecodeInfo {
        selected_mode: if support.hardware_available {
            PlayerVideoDecodeMode::Hardware
        } else {
            PlayerVideoDecodeMode::Software
        },
        hardware_available: support.hardware_available,
        hardware_backend: support
            .hardware_backend
            .or_else(|| Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned())),
        fallback_reason: support.fallback_reason,
    }
}

fn invoke_native_session_command<F>(call: F) -> (bool, String)
where
    F: FnOnce(*mut c_char, usize) -> bool,
{
    let mut error_message = [0 as c_char; 256];
    let succeeded = call(error_message.as_mut_ptr(), error_message.len());
    (succeeded, c_string_buffer_to_string(&error_message))
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationSurfaceTarget {
    kind: u32,
    handle: usize,
}

#[cfg(target_os = "macos")]
impl MacosAvFoundationSurfaceTarget {
    fn from_runtime_surface(surface: PlayerVideoSurfaceTarget) -> Self {
        Self {
            kind: match surface.kind {
                PlayerVideoSurfaceKind::NsView => 0,
                PlayerVideoSurfaceKind::UiView => 1,
                PlayerVideoSurfaceKind::PlayerLayer => 2,
                PlayerVideoSurfaceKind::MetalLayer => 3,
            },
            handle: surface.handle,
        }
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationSnapshotRepr {
    item_status: u32,
    time_control_status: u32,
    playback_rate: c_float,
    position_ms: u64,
    has_duration: c_uchar,
    duration_ms: u64,
    reached_end: c_uchar,
    error_message: [c_char; 256],
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationCallbacks {
    on_snapshot: Option<extern "C" fn(*mut c_void, MacosAvFoundationSnapshotRepr)>,
    on_first_frame_ready: Option<extern "C" fn(*mut c_void, u64)>,
    on_interruption_changed: Option<extern "C" fn(*mut c_void, c_uchar)>,
    on_seek_completed: Option<extern "C" fn(*mut c_void, u64)>,
    on_error: Option<extern "C" fn(*mut c_void, *const c_char)>,
    context: *mut c_void,
}

#[cfg(target_os = "macos")]
extern "C" fn macos_on_snapshot(context: *mut c_void, snapshot: MacosAvFoundationSnapshotRepr) {
    let Some(context) = (unsafe { context.cast::<MacosNativeCallbackContext>().as_ref() }) else {
        return;
    };
    context
        .controller
        .apply_snapshot(MacosAvFoundationSnapshot {
            item_status: match snapshot.item_status {
                1 => MacosPlayerItemStatus::ReadyToPlay,
                2 => MacosPlayerItemStatus::Failed,
                _ => MacosPlayerItemStatus::Unknown,
            },
            time_control_status: match snapshot.time_control_status {
                1 => MacosTimeControlStatus::WaitingToPlay,
                2 => MacosTimeControlStatus::Playing,
                _ => MacosTimeControlStatus::Paused,
            },
            playback_rate: snapshot.playback_rate,
            position: Duration::from_millis(snapshot.position_ms),
            duration: (snapshot.has_duration != 0)
                .then_some(Duration::from_millis(snapshot.duration_ms)),
            reached_end: snapshot.reached_end != 0,
            error_message: {
                let message = c_string_buffer_to_string(&snapshot.error_message);
                if message.is_empty() {
                    None
                } else {
                    Some(message)
                }
            },
        });
}

#[cfg(target_os = "macos")]
extern "C" fn macos_on_seek_completed(context: *mut c_void, position_ms: u64) {
    let Some(context) = (unsafe { context.cast::<MacosNativeCallbackContext>().as_ref() }) else {
        return;
    };
    context
        .controller
        .report_seek_completed(Duration::from_millis(position_ms));
}

#[cfg(target_os = "macos")]
extern "C" fn macos_on_first_frame_ready(context: *mut c_void, position_ms: u64) {
    let Some(context) = (unsafe { context.cast::<MacosNativeCallbackContext>().as_ref() }) else {
        return;
    };
    context
        .controller
        .report_first_frame_ready(Duration::from_millis(position_ms));
}

#[cfg(target_os = "macos")]
extern "C" fn macos_on_interruption_changed(context: *mut c_void, interrupted: c_uchar) {
    let Some(context) = (unsafe { context.cast::<MacosNativeCallbackContext>().as_ref() }) else {
        return;
    };
    context
        .controller
        .report_interruption_changed(interrupted != 0);
}

#[cfg(target_os = "macos")]
extern "C" fn macos_on_error(context: *mut c_void, message: *const c_char) {
    let Some(context) = (unsafe { context.cast::<MacosNativeCallbackContext>().as_ref() }) else {
        return;
    };
    let message = if message.is_null() {
        "AVFoundation reported an unknown error".to_owned()
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    };
    context
        .controller
        .report_error(PlayerRuntimeErrorCode::BackendFailure, message);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationVideoProbe {
    present: c_uchar,
    codec: [c_char; 32],
    width: u32,
    height: u32,
    frame_rate: f64,
}

#[cfg(target_os = "macos")]
impl MacosAvFoundationVideoProbe {
    fn present(&self) -> bool {
        self.present != 0
    }

    fn codec_string(&self) -> String {
        c_string_buffer_to_string(&self.codec)
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationAudioProbe {
    present: c_uchar,
    codec: [c_char; 32],
    sample_rate: u32,
    channels: u16,
}

#[cfg(target_os = "macos")]
impl MacosAvFoundationAudioProbe {
    fn present(&self) -> bool {
        self.present != 0
    }

    fn codec_string(&self) -> String {
        c_string_buffer_to_string(&self.codec)
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacosAvFoundationProbeResult {
    success: c_uchar,
    has_duration: c_uchar,
    duration_ms: u64,
    has_bit_rate: c_uchar,
    bit_rate: u64,
    audio_streams: u32,
    video_streams: u32,
    video: MacosAvFoundationVideoProbe,
    audio: MacosAvFoundationAudioProbe,
    error_message: [c_char; 256],
}

#[cfg(target_os = "macos")]
impl Default for MacosAvFoundationProbeResult {
    fn default() -> Self {
        Self {
            success: 0,
            has_duration: 0,
            duration_ms: 0,
            has_bit_rate: 0,
            bit_rate: 0,
            audio_streams: 0,
            video_streams: 0,
            video: MacosAvFoundationVideoProbe {
                present: 0,
                codec: [0; 32],
                width: 0,
                height: 0,
                frame_rate: 0.0,
            },
            audio: MacosAvFoundationAudioProbe {
                present: 0,
                codec: [0; 32],
                sample_rate: 0,
                channels: 0,
            },
            error_message: [0; 256],
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn player_macos_avfoundation_probe(
        source: *const c_char,
        out_result: *mut MacosAvFoundationProbeResult,
    ) -> bool;

    fn player_macos_avfoundation_create_session(
        source: *const c_char,
        surface: MacosAvFoundationSurfaceTarget,
        callbacks: MacosAvFoundationCallbacks,
        out_session: *mut *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_destroy_session(session_handle: *mut c_void);

    fn player_macos_avfoundation_session_play(
        session_handle: *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_pause(
        session_handle: *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_seek_to(
        session_handle: *mut c_void,
        position_ms: u64,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_set_playback_rate(
        session_handle: *mut c_void,
        rate: c_float,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_attach_surface(
        session_handle: *mut c_void,
        surface: MacosAvFoundationSurfaceTarget,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_detach_surface(
        session_handle: *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn player_macos_avfoundation_session_stop(
        session_handle: *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;
}

#[cfg(target_os = "macos")]
fn c_string_buffer_to_string(buffer: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(not(target_os = "macos"))]
fn c_string_buffer_to_string(_buffer: &[c_char]) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use std::os::raw::c_void;
    use std::path::Path;

    use super::{MacosSystemAvFoundationBridgeBindings, probe_source_with_avfoundation};
    use crate::native::{
        MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext,
        MacosManagedNativeSessionController, MacosNativePlayerCommand,
    };
    use player_core::MediaSource;
    use player_runtime::{
        PlayerRuntimeErrorCode, PlayerRuntimeOptions, PlayerVideoDecodeMode,
        PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget,
    };

    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn player_macos_test_create_player_layer() -> *mut c_void;
        fn player_macos_test_release_object(handle: *mut c_void);
    }

    #[test]
    fn system_probe_reads_fixture_via_avfoundation() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let probe = probe_source_with_avfoundation(&MediaSource::new(test_video_path))
            .expect("system AVFoundation probe should succeed on macOS");

        assert_eq!(probe.media_info.video_streams, 1);
        assert_eq!(probe.media_info.audio_streams, 1);
        assert_eq!(
            probe
                .media_info
                .best_video
                .as_ref()
                .expect("video stream should exist")
                .width,
            960
        );
        assert_eq!(
            probe
                .startup
                .video_decode
                .as_ref()
                .expect("native probe should report decode diagnostics")
                .selected_mode,
            PlayerVideoDecodeMode::Hardware
        );
    }

    #[test]
    fn system_bindings_require_surface_before_creating_native_session() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let bindings = MacosSystemAvFoundationBridgeBindings;
        let probe = bindings
            .probe_source(
                &MacosAvFoundationBridgeContext {
                    video_surface: None,
                },
                &MediaSource::new(test_video_path.clone()),
                &PlayerRuntimeOptions::default(),
            )
            .expect("probe should succeed");

        let error = match bindings.create_command_sink(
            MacosAvFoundationBridgeContext {
                video_surface: None,
            },
            &MediaSource::new(test_video_path),
            &PlayerRuntimeOptions::default(),
            &probe.media_info,
            &probe.startup,
            MacosManagedNativeSessionController::default(),
        ) {
            Ok(_) => {
                panic!("system bindings should reject video session creation without a surface")
            }
            Err(error) => error,
        };

        assert_eq!(error.code(), PlayerRuntimeErrorCode::InvalidArgument);
    }

    #[test]
    fn system_bindings_create_native_session_with_player_layer_surface() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let bindings = MacosSystemAvFoundationBridgeBindings;
        let probe = bindings
            .probe_source(
                &MacosAvFoundationBridgeContext {
                    video_surface: None,
                },
                &MediaSource::new(test_video_path.clone()),
                &PlayerRuntimeOptions::default(),
            )
            .expect("probe should succeed");
        let layer_handle = unsafe { player_macos_test_create_player_layer() };
        assert!(
            !layer_handle.is_null(),
            "test player layer handle should be created"
        );

        let options =
            PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: layer_handle as usize,
            });
        let mut sink = bindings
            .create_command_sink(
                MacosAvFoundationBridgeContext {
                    video_surface: None,
                },
                &MediaSource::new(test_video_path),
                &options,
                &probe.media_info,
                &probe.startup,
                MacosManagedNativeSessionController::default(),
            )
            .expect("system bindings should create a native session with a player layer surface");
        sink.submit_command(MacosNativePlayerCommand::SetPlaybackRate { rate: 1.5 })
            .expect("native session should accept playback rate updates");
        sink.submit_command(MacosNativePlayerCommand::Play)
            .expect("native session should accept play");
        sink.submit_command(MacosNativePlayerCommand::Pause)
            .expect("native session should accept pause");
        drop(sink);

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
}
