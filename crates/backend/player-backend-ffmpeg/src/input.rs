use std::ffi::{CString, c_int, c_void};
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use player_model::{MediaSource, MediaSourceKind, MediaSourceProtocol};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputOpenPurpose {
    Probe,
    VideoDecode,
    AudioDecode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputOpenProfile {
    Default,
    RemoteHls,
}

pub(crate) struct FfmpegInput {
    inner: ffmpeg::format::context::Input,
    _interrupt: Option<FfmpegInputInterrupt>,
}

impl FfmpegInput {
    fn new(inner: ffmpeg::format::context::Input) -> Self {
        Self {
            inner,
            _interrupt: None,
        }
    }

    fn with_interrupt(
        inner: ffmpeg::format::context::Input,
        interrupt: FfmpegInputInterrupt,
    ) -> Self {
        Self {
            inner,
            _interrupt: Some(interrupt),
        }
    }
}

impl Deref for FfmpegInput {
    type Target = ffmpeg::format::context::Input;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for FfmpegInput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub(crate) struct FfmpegInputInterrupt {
    flag: Arc<AtomicBool>,
}

impl FfmpegInputInterrupt {
    pub(crate) fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag }
    }

    pub(crate) fn callback(&self) -> ffmpeg::ffi::AVIOInterruptCB {
        ffmpeg::ffi::AVIOInterruptCB {
            callback: Some(ffmpeg_interrupt_callback),
            opaque: Arc::as_ptr(&self.flag).cast_mut().cast::<c_void>(),
        }
    }
}

pub(crate) extern "C" fn ffmpeg_interrupt_callback(opaque: *mut c_void) -> c_int {
    if opaque.is_null() {
        return 0;
    }

    let flag = unsafe { &*(opaque.cast::<AtomicBool>()) };
    i32::from(flag.load(Ordering::SeqCst))
}

pub(crate) fn supports_input_format(name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };

    unsafe { !ffmpeg::ffi::av_find_input_format(name.as_ptr()).is_null() }
}

pub(crate) fn open_media_input(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<FfmpegInput> {
    let profile = input_open_profile_for_source(source);
    if profile == InputOpenProfile::Default && interrupt_flag.is_none() {
        return ffmpeg::format::input(&source.uri())
            .map(FfmpegInput::new)
            .with_context(|| format!("failed to open media source: {}", source.uri()));
    }

    open_media_input_with_profile(source, purpose, profile, interrupt_flag)
}

fn open_media_input_with_profile(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<FfmpegInput> {
    let source_uri = source.uri();
    let source_uri_cstr =
        CString::new(source_uri).context("media source URI contained an interior NUL byte")?;
    let interrupt = interrupt_flag.map(FfmpegInputInterrupt::new);
    let interrupt_state = interrupt.as_ref().map(|interrupt| interrupt.flag.clone());
    let options = input_open_dictionary(profile, purpose);

    unsafe {
        let mut format_context = if interrupt.is_some() {
            ffmpeg::ffi::avformat_alloc_context()
        } else {
            ptr::null_mut()
        };

        if interrupt.is_some() && format_context.is_null() {
            anyhow::bail!("failed to allocate FFmpeg format context");
        }

        if let Some(interrupt) = interrupt.as_ref() {
            (*format_context).interrupt_callback = interrupt.callback();
        }

        let mut raw_options = options.disown();
        let open_started_at = Instant::now();
        let open_result = ffmpeg::ffi::avformat_open_input(
            &mut format_context,
            source_uri_cstr.as_ptr(),
            ptr::null_mut(),
            &mut raw_options,
        );
        let open_duration = open_started_at.elapsed();
        ffmpeg::Dictionary::own(raw_options);

        if open_result < 0 {
            if !format_context.is_null() {
                ffmpeg::ffi::avformat_close_input(&mut format_context);
            }
            log_input_open_failure(
                source,
                purpose,
                profile,
                open_duration,
                Duration::ZERO,
                interrupt_state.as_deref(),
                "avformat_open_input",
                open_result,
            );
            return Err(anyhow::Error::new(ffmpeg::Error::from(open_result))
                .context(format!("failed to open media source: {source_uri}")));
        }

        let stream_info_started_at = Instant::now();
        let stream_info_result =
            ffmpeg::ffi::avformat_find_stream_info(format_context, ptr::null_mut());
        let stream_info_duration = stream_info_started_at.elapsed();

        if stream_info_result < 0 {
            ffmpeg::ffi::avformat_close_input(&mut format_context);
            log_input_open_failure(
                source,
                purpose,
                profile,
                open_duration,
                stream_info_duration,
                interrupt_state.as_deref(),
                "avformat_find_stream_info",
                stream_info_result,
            );
            return Err(anyhow::Error::new(ffmpeg::Error::from(stream_info_result))
                .context(format!("failed to inspect media streams: {source_uri}")));
        }

        log_input_open_success(
            source,
            purpose,
            profile,
            open_duration,
            stream_info_duration,
            interrupt_state.as_deref(),
        );
        let input = ffmpeg::format::context::Input::wrap(format_context);
        Ok(match interrupt {
            Some(interrupt) => FfmpegInput::with_interrupt(input, interrupt),
            None => FfmpegInput::new(input),
        })
    }
}

pub(crate) fn input_open_profile_for_source(source: &MediaSource) -> InputOpenProfile {
    if source.kind() == MediaSourceKind::Remote && source.protocol() == MediaSourceProtocol::Hls {
        InputOpenProfile::RemoteHls
    } else {
        InputOpenProfile::Default
    }
}

fn input_open_dictionary(
    profile: InputOpenProfile,
    purpose: InputOpenPurpose,
) -> ffmpeg::Dictionary<'static> {
    let mut options = ffmpeg::Dictionary::new();

    for (key, value) in input_open_tuning_options(profile, purpose) {
        options.set(key, value);
    }

    options
}

pub(crate) fn input_open_tuning_options(
    profile: InputOpenProfile,
    purpose: InputOpenPurpose,
) -> &'static [(&'static str, &'static str)] {
    match (profile, purpose) {
        (InputOpenProfile::Default, _) => &[],
        (InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode) => &[
            ("http_multiple", "0"),
            ("probesize", "524288"),
            ("formatprobesize", "524288"),
            ("analyzeduration", "2000000"),
            ("fpsprobesize", "4"),
            ("rw_timeout", "15000000"),
            ("allowed_media_types", "audio"),
        ],
        (InputOpenProfile::RemoteHls, _) => &[
            ("http_multiple", "0"),
            ("probesize", "524288"),
            ("formatprobesize", "524288"),
            ("analyzeduration", "2000000"),
            ("fpsprobesize", "4"),
            ("rw_timeout", "15000000"),
        ],
    }
}

fn log_input_open_success(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    open_duration: Duration,
    stream_info_duration: Duration,
    interrupt_flag: Option<&AtomicBool>,
) {
    if profile == InputOpenProfile::Default {
        return;
    }

    info!(
        source = source.uri(),
        purpose = purpose.label(),
        profile = profile.label(),
        tuning = input_open_tuning_summary(profile, purpose),
        interrupted = interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst)),
        open_input_ms = open_duration.as_millis(),
        find_stream_info_ms = stream_info_duration.as_millis(),
        total_ms = open_duration.as_millis() + stream_info_duration.as_millis(),
        "opened FFmpeg media input"
    );
}

fn log_input_open_failure(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    open_duration: Duration,
    stream_info_duration: Duration,
    interrupt_flag: Option<&AtomicBool>,
    phase: &'static str,
    error_code: i32,
) {
    if profile == InputOpenProfile::Default
        && !interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst))
    {
        return;
    }

    warn!(
        source = source.uri(),
        purpose = purpose.label(),
        profile = profile.label(),
        tuning = input_open_tuning_summary(profile, purpose),
        phase,
        interrupted = interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst)),
        open_input_ms = open_duration.as_millis(),
        find_stream_info_ms = stream_info_duration.as_millis(),
        error_code,
        error = %ffmpeg::Error::from(error_code),
        "failed to open FFmpeg media input"
    );
}

impl InputOpenPurpose {
    fn label(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::VideoDecode => "video_decode",
            Self::AudioDecode => "audio_decode",
        }
    }
}

impl InputOpenProfile {
    fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::RemoteHls => "remote_hls",
        }
    }
}

pub(crate) fn input_open_tuning_summary(
    profile: InputOpenProfile,
    purpose: InputOpenPurpose,
) -> &'static str {
    match (profile, purpose) {
        (InputOpenProfile::Default, _) => "default",
        (InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode) => {
            "http_multiple=0,probesize=524288,formatprobesize=524288,analyzeduration=2000000,fpsprobesize=4,rw_timeout=15000000,allowed_media_types=audio"
        }
        (InputOpenProfile::RemoteHls, _) => {
            "http_multiple=0,probesize=524288,formatprobesize=524288,analyzeduration=2000000,fpsprobesize=4,rw_timeout=15000000"
        }
    }
}
