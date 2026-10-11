use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use ffmpeg::{Rational, codec, encoder, format, media};
use ffmpeg_next as ffmpeg;
use player_plugin::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream,
    ContentFormatKind, OutputFormat, PostDownloadProcessor, ProcessorCapabilities, ProcessorError,
    ProcessorOutput, ProcessorProgress, StreamKind,
};

use crate::error::FfmpegProcessorError;

#[derive(Debug, Default)]
pub struct FfmpegRemuxProcessor;

impl FfmpegRemuxProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl PostDownloadProcessor for FfmpegRemuxProcessor {
    fn name(&self) -> &str {
        "player-remux-ffmpeg"
    }

    fn supported_input_formats(&self) -> &[ContentFormatKind] {
        static SUPPORTED: [ContentFormatKind; 4] = [
            ContentFormatKind::HlsSegments,
            ContentFormatKind::DashSegments,
            ContentFormatKind::FlvSegments,
            ContentFormatKind::SingleFile,
        ];
        &SUPPORTED
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        ProcessorCapabilities {
            supported_input_formats: self.supported_input_formats().to_vec(),
            output_formats: vec![OutputFormat::Mp4, OutputFormat::Mkv],
            supports_cancellation: true,
            supports_assembly: true,
            supported_assembly_modes: vec![
                AssemblyMode::SeparateAudioVideo,
                AssemblyMode::MultiAudio,
                AssemblyMode::WithSubtitles,
                AssemblyMode::Generic,
            ],
        }
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let input_path = match &input.content_format {
            CompletedContentFormat::HlsSegments { manifest_path, .. } => {
                ensure_demuxer("hls", ContentFormatKind::HlsSegments)?;
                manifest_path
            }
            CompletedContentFormat::DashSegments { manifest_path, .. } => {
                ensure_demuxer("dash", ContentFormatKind::DashSegments)?;
                manifest_path
            }
            CompletedContentFormat::FlvSegments {
                manifest_path,
                segment_paths: _,
            } => {
                ensure_demuxer("concat", ContentFormatKind::FlvSegments)?;
                manifest_path
            }
            CompletedContentFormat::SingleFile { path } => {
                if !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("flv"))
                {
                    return Ok(ProcessorOutput::Skipped);
                }
                ensure_demuxer("flv", ContentFormatKind::SingleFile)?;
                path
            }
        };

        initialize_ffmpeg()?;
        if progress.is_cancelled() {
            return Err(ProcessorError::Cancelled);
        }

        remux_input_to_mp4(input_path, output_path, progress)?;
        Ok(ProcessorOutput::MuxedFile {
            path: output_path.to_path_buf(),
            format: output_format_from_path(output_path),
        })
    }

    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        if input.assembly_mode == AssemblyMode::Single || input.streams.len() <= 1 {
            return self.process(input, output_path, progress);
        }

        initialize_ffmpeg()?;
        if progress.is_cancelled() {
            return Err(ProcessorError::Cancelled);
        }

        let output_format = output_format_from_path(output_path);
        if output_format == OutputFormat::Mp4
            && input
                .streams
                .iter()
                .any(|stream| stream.kind == StreamKind::Subtitle)
        {
            return Err(ProcessorError::UnsupportedFormat(
                ContentFormatKind::Unknown,
            ));
        }

        let sources = assembly_sources(input)?;
        remux_sources_to_output(&sources, output_path, output_format, progress)?;
        Ok(ProcessorOutput::MuxedFile {
            path: output_path.to_path_buf(),
            format: output_format,
        })
    }
}

fn initialize_ffmpeg() -> Result<(), ProcessorError> {
    ffmpeg::init().map_err(|error| {
        FfmpegProcessorError::Initialization(error.to_string()).into_processor_error()
    })
}

fn ensure_demuxer(
    name: &'static str,
    content_format: ContentFormatKind,
) -> Result<(), ProcessorError> {
    let c_name = CString::new(name)
        .map_err(|_| FfmpegProcessorError::MissingDemuxer(name).into_processor_error())?;

    // SAFETY: `c_name` is a live NUL-terminated string and FFmpeg only reads
    // the pointer during this lookup.
    if unsafe { ffmpeg::ffi::av_find_input_format(c_name.as_ptr()).is_null() } {
        return Err(match content_format {
            ContentFormatKind::HlsSegments
            | ContentFormatKind::DashSegments
            | ContentFormatKind::FlvSegments
            | ContentFormatKind::SingleFile => ProcessorError::UnsupportedFormat(content_format),
            _ => FfmpegProcessorError::MissingDemuxer(name).into_processor_error(),
        });
    }

    Ok(())
}

fn remux_input_to_mp4(
    input_path: &Path,
    output_path: &Path,
    progress: &dyn ProcessorProgress,
) -> Result<(), ProcessorError> {
    with_cancellation_token(progress, |progress, token| {
        remux_input_to_mp4_with_opener(input_path, output_path, progress, |path| {
            open_remux_input(path, progress, token.clone())
        })
    })
}

fn remux_input_to_mp4_with_opener(
    input_path: &Path,
    output_path: &Path,
    progress: &dyn ProcessorProgress,
    mut open_input: impl FnMut(&Path) -> Result<format::context::Input, ProcessorError>,
) -> Result<(), ProcessorError> {
    publish_remux_output(output_path, progress, |staged_path| {
        remux_single_input(input_path, staged_path, progress, &mut open_input)
    })
}

fn remux_single_input(
    input_path: &Path,
    output_path: &Path,
    progress: &dyn ProcessorProgress,
    open_input: &mut impl FnMut(&Path) -> Result<format::context::Input, ProcessorError>,
) -> Result<(), ProcessorError> {
    check_cancelled(progress)?;
    let output_path_string = output_path.to_string_lossy().into_owned();
    let mut input_context = open_input(input_path)?;
    check_cancelled(progress)?;
    let mut output_context = format::output(&output_path_string).map_err(|error| {
        FfmpegProcessorError::InvalidPath(format!(
            "failed to create output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;

    let mut stream_mapping = vec![-1; input_context.nb_streams() as _];
    let mut input_time_bases = vec![Rational(0, 1); input_context.nb_streams() as _];
    let mut timestamp_state =
        vec![TimestampRepairState::default(); input_context.nb_streams() as _];
    let mut output_stream_index = 0;

    for (input_stream_index, input_stream) in input_context.streams().enumerate() {
        let medium = input_stream.parameters().medium();
        // Gallery export prioritizes a playable MP4, so only audio/video
        // streams are carried across. Some subtitle or auxiliary streams from
        // HLS/DASH fixtures are rejected by the MP4 muxer during header write.
        if medium != media::Type::Audio && medium != media::Type::Video {
            continue;
        }

        stream_mapping[input_stream_index] = output_stream_index;
        input_time_bases[input_stream_index] = input_stream.time_base();
        output_stream_index += 1;

        let mut output_stream = output_context
            .add_stream(encoder::find(codec::Id::None))
            .map_err(|error| {
                FfmpegProcessorError::Remux(format!(
                    "failed to add output stream for `{}`: {error}",
                    output_path.display()
                ))
                .into_processor_error()
            })?;
        output_stream.set_parameters(input_stream.parameters());
        // SAFETY: `output_stream.parameters()` returns the mutable parameters
        // owned by this output stream; clearing `codec_tag` is the documented
        // FFmpeg remuxing step after copying input parameters.
        unsafe {
            (*output_stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    if output_stream_index == 0 {
        return Err(FfmpegProcessorError::Remux(format!(
            "input `{}` does not contain any MP4-compatible audio/video streams",
            input_path.display()
        ))
        .into_processor_error());
    }

    output_context.set_metadata(input_context.metadata().to_owned());
    output_context.write_header().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to write output header `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    progress.on_progress(0.05);

    while let Some(mut packet) = read_next_packet(&mut input_context, input_path, progress)? {
        let input_stream_index = packet.stream();
        let Some((mapped_stream_index, input_time_base)) =
            packet_stream_route(&stream_mapping, &input_time_bases, input_stream_index)?
        else {
            continue;
        };
        let timestamp_state = timestamp_state
            .get_mut(input_stream_index)
            .ok_or_else(|| unsupported_dynamic_stream(input_stream_index))?;
        repair_packet_timestamps(&mut packet, timestamp_state)?;

        let output_stream = output_context.stream(mapped_stream_index).ok_or_else(|| {
            FfmpegProcessorError::Remux(format!(
                "missing mapped output stream index {} for `{}`",
                mapped_stream_index,
                output_path.display()
            ))
            .into_processor_error()
        })?;

        packet.rescale_ts(input_time_base, output_stream.time_base());
        packet.set_position(-1);
        packet.set_stream(mapped_stream_index);
        packet
            .write_interleaved(&mut output_context)
            .map_err(|error| {
                FfmpegProcessorError::Remux(format!(
                    "failed to write remuxed packet to `{}`: {error}",
                    output_path.display()
                ))
                .into_processor_error()
            })?;
    }

    check_cancelled(progress)?;
    output_context.write_trailer().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to finalize output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    check_cancelled(progress)?;

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct TimestampRepairState {
    last_dts: Option<i64>,
    last_duration: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PacketTimestampValues {
    duration: i64,
    dts: Option<i64>,
    pts: Option<i64>,
}

#[derive(Debug, Clone)]
struct AssemblySource {
    path: PathBuf,
    kind: StreamKind,
    content_kind: ContentFormatKind,
}

struct InputRemuxState {
    context: format::context::Input,
    stream_mapping: Vec<i32>,
    input_time_bases: Vec<Rational>,
    timestamp_state: Vec<TimestampRepairState>,
    path: PathBuf,
}

fn assembly_sources(input: &CompletedDownloadInfo) -> Result<Vec<AssemblySource>, ProcessorError> {
    validate_assembly_request(input)?;
    input
        .streams
        .iter()
        .filter(|stream| {
            matches!(
                stream.kind,
                StreamKind::Combined
                    | StreamKind::Video
                    | StreamKind::Audio
                    | StreamKind::SecondaryAudio
                    | StreamKind::Subtitle
            )
        })
        .map(|stream| {
            Ok(AssemblySource {
                path: stream_source_path(stream)?,
                kind: stream.kind,
                content_kind: stream.content_format.kind(),
            })
        })
        .collect()
}

fn validate_assembly_request(input: &CompletedDownloadInfo) -> Result<(), ProcessorError> {
    let has_video = input
        .streams
        .iter()
        .any(|stream| matches!(stream.kind, StreamKind::Combined | StreamKind::Video));
    let audio_count = input
        .streams
        .iter()
        .filter(|stream| matches!(stream.kind, StreamKind::Audio | StreamKind::SecondaryAudio))
        .count();

    match input.assembly_mode {
        AssemblyMode::SeparateAudioVideo if !has_video || audio_count == 0 => {
            Err(ProcessorError::AbiViolation(
                "SeparateAudioVideo assembly requires video and audio streams".to_owned(),
            ))
        }
        AssemblyMode::MultiAudio if !has_video || audio_count < 2 => {
            Err(ProcessorError::AbiViolation(
                "MultiAudio assembly requires video and at least two audio streams".to_owned(),
            ))
        }
        AssemblyMode::WithSubtitles if !has_video => Err(ProcessorError::AbiViolation(
            "WithSubtitles assembly requires a video or combined stream".to_owned(),
        )),
        _ => Ok(()),
    }
}

fn stream_source_path(stream: &CompletedStream) -> Result<PathBuf, ProcessorError> {
    match &stream.content_format {
        CompletedContentFormat::HlsSegments { manifest_path, .. }
        | CompletedContentFormat::DashSegments { manifest_path, .. }
        | CompletedContentFormat::FlvSegments { manifest_path, .. } => Ok(manifest_path.clone()),
        CompletedContentFormat::SingleFile { path } => Ok(path.clone()),
    }
}

fn remux_sources_to_output(
    sources: &[AssemblySource],
    output_path: &Path,
    output_format: OutputFormat,
    progress: &dyn ProcessorProgress,
) -> Result<(), ProcessorError> {
    with_cancellation_token(progress, |progress, token| {
        remux_sources_to_output_with_opener(sources, output_path, output_format, progress, |path| {
            open_remux_input(path, progress, token.clone())
        })
    })
}

fn remux_sources_to_output_with_opener(
    sources: &[AssemblySource],
    output_path: &Path,
    output_format: OutputFormat,
    progress: &dyn ProcessorProgress,
    mut open_input: impl FnMut(&Path) -> Result<format::context::Input, ProcessorError>,
) -> Result<(), ProcessorError> {
    publish_remux_output(output_path, progress, |staged_path| {
        remux_sources(
            sources,
            staged_path,
            output_format,
            progress,
            &mut open_input,
        )
    })
}

fn remux_sources(
    sources: &[AssemblySource],
    output_path: &Path,
    output_format: OutputFormat,
    progress: &dyn ProcessorProgress,
    open_input: &mut impl FnMut(&Path) -> Result<format::context::Input, ProcessorError>,
) -> Result<(), ProcessorError> {
    check_cancelled(progress)?;
    if sources.is_empty() {
        return Err(ProcessorError::AbiViolation(
            "assembly input must contain at least one stream".to_owned(),
        ));
    }

    let output_path_string = output_path.to_string_lossy().into_owned();
    let mut output_context = format::output(&output_path_string).map_err(|error| {
        FfmpegProcessorError::InvalidPath(format!(
            "failed to create output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;

    let mut input_states = Vec::with_capacity(sources.len());
    let mut output_stream_index = 0_i32;

    for source in sources {
        check_cancelled(progress)?;
        ensure_demuxer_for_source(source)?;
        let input_context = open_input(&source.path)?;
        check_cancelled(progress)?;

        let mut stream_mapping = vec![-1; input_context.nb_streams() as usize];
        let mut input_time_bases = vec![Rational(0, 1); input_context.nb_streams() as usize];
        for (input_stream_index, input_stream) in input_context.streams().enumerate() {
            let medium = input_stream.parameters().medium();
            if !stream_kind_allows_medium(source.kind, medium, output_format) {
                continue;
            }

            stream_mapping[input_stream_index] = output_stream_index;
            input_time_bases[input_stream_index] = input_stream.time_base();
            output_stream_index += 1;

            let mut output_stream = output_context
                .add_stream(encoder::find(codec::Id::None))
                .map_err(|error| {
                    FfmpegProcessorError::Remux(format!(
                        "failed to add output stream for `{}`: {error}",
                        output_path.display()
                    ))
                    .into_processor_error()
                })?;
            output_stream.set_parameters(input_stream.parameters());
            // SAFETY: `output_stream.parameters()` returns the mutable
            // parameters owned by this output stream; clearing `codec_tag`
            // avoids incompatible container tags after copying parameters.
            unsafe {
                (*output_stream.parameters().as_mut_ptr()).codec_tag = 0;
            }
        }

        let timestamp_state =
            vec![TimestampRepairState::default(); input_context.nb_streams() as usize];
        input_states.push(InputRemuxState {
            context: input_context,
            stream_mapping,
            input_time_bases,
            timestamp_state,
            path: source.path.clone(),
        });
    }

    if output_stream_index == 0 {
        return Err(FfmpegProcessorError::Remux(format!(
            "assembly input for `{}` did not contain any compatible streams",
            output_path.display()
        ))
        .into_processor_error());
    }

    output_context.write_header().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to write output header `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    progress.on_progress(0.05);

    let input_count = input_states.len().max(1);
    for (input_index, state) in input_states.iter_mut().enumerate() {
        while let Some(mut packet) = read_next_packet(&mut state.context, &state.path, progress)? {
            let input_stream_index = packet.stream();
            let Some((mapped_stream_index, input_time_base)) = packet_stream_route(
                &state.stream_mapping,
                &state.input_time_bases,
                input_stream_index,
            )?
            else {
                continue;
            };

            let timestamp_state = state
                .timestamp_state
                .get_mut(input_stream_index)
                .ok_or_else(|| unsupported_dynamic_stream(input_stream_index))?;
            repair_packet_timestamps(&mut packet, timestamp_state)?;
            let output_stream = output_context.stream(mapped_stream_index).ok_or_else(|| {
                FfmpegProcessorError::Remux(format!(
                    "missing mapped output stream index {} for `{}`",
                    mapped_stream_index,
                    output_path.display()
                ))
                .into_processor_error()
            })?;

            packet.rescale_ts(input_time_base, output_stream.time_base());
            packet.set_position(-1);
            packet.set_stream(mapped_stream_index);
            packet
                .write_interleaved(&mut output_context)
                .map_err(|error| {
                    FfmpegProcessorError::Remux(format!(
                        "failed to write packet from `{}` to `{}`: {error}",
                        state.path.display(),
                        output_path.display()
                    ))
                    .into_processor_error()
                })?;
        }
        progress.on_progress(0.05 + 0.9 * ((input_index + 1) as f32 / input_count as f32));
    }

    check_cancelled(progress)?;
    output_context.write_trailer().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to finalize output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    check_cancelled(progress)?;
    Ok(())
}

fn packet_stream_route(
    stream_mapping: &[i32],
    input_time_bases: &[Rational],
    input_stream_index: usize,
) -> Result<Option<(usize, Rational)>, ProcessorError> {
    let mapped_stream_index = stream_mapping
        .get(input_stream_index)
        .copied()
        .ok_or_else(|| unsupported_dynamic_stream(input_stream_index))?;
    if mapped_stream_index < 0 {
        return Ok(None);
    }
    let input_time_base = input_time_bases
        .get(input_stream_index)
        .copied()
        .ok_or_else(|| unsupported_dynamic_stream(input_stream_index))?;
    let mapped_stream_index = usize::try_from(mapped_stream_index)
        .map_err(|_| unsupported_dynamic_stream(input_stream_index))?;
    Ok(Some((mapped_stream_index, input_time_base)))
}

fn unsupported_dynamic_stream(input_stream_index: usize) -> ProcessorError {
    ProcessorError::UnsupportedDynamicStream {
        stream_index: u32::try_from(input_stream_index).unwrap_or(u32::MAX),
    }
}

fn check_cancelled(progress: &dyn ProcessorProgress) -> Result<(), ProcessorError> {
    if progress.is_cancelled() {
        Err(ProcessorError::Cancelled)
    } else {
        Ok(())
    }
}

fn read_next_packet(
    input: &mut format::context::Input,
    path: &Path,
    progress: &dyn ProcessorProgress,
) -> Result<Option<ffmpeg::Packet>, ProcessorError> {
    check_cancelled(progress)?;
    let mut packet = ffmpeg::Packet::empty();
    let result = packet.read(input);
    check_cancelled(progress)?;
    match result {
        Ok(()) => Ok(Some(packet)),
        Err(ffmpeg::Error::Eof) => Ok(None),
        Err(error) => Err(FfmpegProcessorError::Remux(format!(
            "failed to read packet from `{}`: {error}",
            path.display()
        ))
        .into_processor_error()),
    }
}

fn open_remux_input(
    path: &Path,
    progress: &dyn ProcessorProgress,
    token: Arc<AtomicBool>,
) -> Result<format::context::Input, ProcessorError> {
    check_cancelled(progress)?;
    let result = format::input_with_interrupt(path, move || token.load(Ordering::Acquire));
    check_cancelled(progress)?;
    result.map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to open input `{}`: {error}",
            path.display()
        ))
        .into_processor_error()
    })
}

struct LatchedProgress<'a> {
    progress: &'a dyn ProcessorProgress,
    token: Arc<AtomicBool>,
}

impl ProcessorProgress for LatchedProgress<'_> {
    fn on_progress(&self, ratio: f32) {
        self.progress.on_progress(ratio);
    }
    fn is_cancelled(&self) -> bool {
        if self.progress.is_cancelled() {
            self.token.store(true, Ordering::Release);
        }
        self.token.load(Ordering::Acquire)
    }
}

fn with_cancellation_token<T>(
    progress: &dyn ProcessorProgress,
    run: impl FnOnce(&dyn ProcessorProgress, Arc<AtomicBool>) -> Result<T, ProcessorError>,
) -> Result<T, ProcessorError> {
    // The FFmpeg callback owns only an atomic. The scoped watcher is the only
    // borrower of progress and is stopped and joined on every return or unwind.
    struct StopWatcher(std::sync::mpsc::Sender<()>);
    impl Drop for StopWatcher {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    check_cancelled(progress)?;
    let token = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let (stop, receiver) = std::sync::mpsc::channel();
        let watcher_token = token.clone();
        scope.spawn(move || {
            loop {
                if progress.is_cancelled() {
                    watcher_token.store(true, Ordering::Release);
                    return;
                }
                match receiver.recv_timeout(Duration::from_millis(25)) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    _ => return,
                }
            }
        });
        let _stop = StopWatcher(stop);
        let latched = LatchedProgress {
            progress,
            token: token.clone(),
        };
        run(&latched, token)
    })
}

struct StagedRemuxOutput(Option<PathBuf>);

impl Drop for StagedRemuxOutput {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

fn publish_remux_output(
    output_path: &Path,
    progress: &dyn ProcessorProgress,
    remux: impl FnOnce(&Path) -> Result<(), ProcessorError>,
) -> Result<(), ProcessorError> {
    check_cancelled(progress)?;
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        ProcessorError::OutputPath(format!("failed to create `{}`: {error}", parent.display()))
    })?;
    static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(0);
    let mut name = output_path
        .file_name()
        .ok_or_else(|| ProcessorError::OutputPath("output path has no filename".into()))?
        .to_os_string();
    name.push(format!(
        ".vesper-remux-{}-{}.",
        std::process::id(),
        NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed)
    ));
    name.push(
        output_path
            .extension()
            .unwrap_or_else(|| std::ffi::OsStr::new("mp4")),
    );
    let staged_path = parent.join(name);
    // Reserve this invocation's sibling path without touching an existing target.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged_path)
        .map_err(|error| {
            ProcessorError::OutputPath(format!(
                "failed to stage `{}`: {error}",
                output_path.display()
            ))
        })?;
    let mut staged = StagedRemuxOutput(Some(staged_path.clone()));
    // All FFmpeg handles close inside this call, before cleanup or publication.
    remux(&staged_path)?;
    check_cancelled(progress)?;
    fs::rename(&staged_path, output_path).map_err(|error| {
        ProcessorError::OutputPath(format!(
            "failed to publish `{}`: {error}",
            output_path.display()
        ))
    })?;
    staged.0 = None;
    progress.on_progress(1.0);
    Ok(())
}

fn repair_packet_timestamps(
    packet: &mut ffmpeg::Packet,
    state: &mut TimestampRepairState,
) -> Result<(), ProcessorError> {
    let repaired = repair_timestamp_values(
        PacketTimestampValues {
            duration: packet.duration(),
            dts: packet.dts(),
            pts: packet.pts(),
        },
        state,
    )?;
    packet.set_duration(repaired.duration);
    packet.set_dts(repaired.dts);
    packet.set_pts(repaired.pts);
    Ok(())
}

fn repair_timestamp_values(
    mut values: PacketTimestampValues,
    state: &mut TimestampRepairState,
) -> Result<PacketTimestampValues, ProcessorError> {
    let duration = if values.duration > 0 {
        values.duration
    } else {
        state.last_duration.unwrap_or(1)
    };
    values.duration = duration;
    state.last_duration = Some(duration);

    if values
        .dts
        .is_none_or(|value| state.last_dts.is_some_and(|last| value <= last))
    {
        values.dts = Some(match state.last_dts {
            Some(last) => last.checked_add(duration).ok_or_else(|| {
                FfmpegProcessorError::Remux(
                    "timestamp repair overflowed while advancing DTS".to_owned(),
                )
                .into_processor_error()
            })?,
            None => 0,
        });
    }

    let dts_value = values.dts.unwrap_or(0);
    if values.pts.is_none_or(|pts| pts < dts_value) {
        values.pts = Some(dts_value);
    }
    state.last_dts = Some(dts_value);
    Ok(values)
}

fn stream_kind_allows_medium(
    kind: StreamKind,
    medium: media::Type,
    output_format: OutputFormat,
) -> bool {
    match kind {
        StreamKind::Combined => {
            medium == media::Type::Audio
                || medium == media::Type::Video
                || (output_format == OutputFormat::Mkv && medium == media::Type::Subtitle)
        }
        StreamKind::Video => medium == media::Type::Video,
        StreamKind::Audio | StreamKind::SecondaryAudio => medium == media::Type::Audio,
        StreamKind::Subtitle => {
            output_format == OutputFormat::Mkv && medium == media::Type::Subtitle
        }
        StreamKind::Auxiliary => false,
    }
}

fn ensure_demuxer_for_source(source: &AssemblySource) -> Result<(), ProcessorError> {
    match source.content_kind {
        ContentFormatKind::HlsSegments => ensure_demuxer("hls", source.content_kind),
        ContentFormatKind::DashSegments => ensure_demuxer("dash", source.content_kind),
        ContentFormatKind::FlvSegments => ensure_demuxer("concat", source.content_kind),
        ContentFormatKind::SingleFile => Ok(()),
        ContentFormatKind::Unknown => Err(ProcessorError::UnsupportedFormat(source.content_kind)),
    }
}

fn output_format_from_path(output_path: &Path) -> OutputFormat {
    match output_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("mkv") => OutputFormat::Mkv,
        _ => OutputFormat::Mp4,
    }
}

trait IntoProcessorError {
    fn into_processor_error(self) -> ProcessorError;
}

impl IntoProcessorError for FfmpegProcessorError {
    fn into_processor_error(self) -> ProcessorError {
        match self {
            FfmpegProcessorError::Initialization(message)
            | FfmpegProcessorError::Remux(message) => ProcessorError::MuxFailed(message),
            FfmpegProcessorError::MissingDemuxer(_) => {
                ProcessorError::UnsupportedFormat(ContentFormatKind::Unknown)
            }
            FfmpegProcessorError::InvalidPath(message) => ProcessorError::OutputPath(message),
        }
    }
}

#[cfg(test)]
#[path = "muxer_read_regressions.rs"]
mod read_regressions;

#[cfg(test)]
mod tests {
    use super::{
        FfmpegRemuxProcessor, PacketTimestampValues, TimestampRepairState, packet_stream_route,
        repair_timestamp_values,
    };
    use player_plugin::{
        AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind,
        DownloadMetadata, OutputFormat, PostDownloadProcessor, ProcessorError, ProcessorOutput,
        ProcessorProgress,
    };
    use std::path::PathBuf;

    #[derive(Debug, Default)]
    struct RecordingProgress {
        ratios: std::sync::Mutex<Vec<f32>>,
    }

    impl RecordingProgress {
        fn ratios(&self) -> Vec<f32> {
            self.ratios
                .lock()
                .map(|ratios| ratios.clone())
                .unwrap_or_default()
        }
    }

    impl ProcessorProgress for RecordingProgress {
        fn on_progress(&self, ratio: f32) {
            if let Ok(mut ratios) = self.ratios.lock() {
                ratios.push(ratio);
            }
        }
    }

    #[test]
    fn packet_stream_route_rejects_stream_added_after_header() {
        let time_base = ffmpeg_next::Rational(1, 1_000);

        assert_eq!(
            packet_stream_route(&[0], &[time_base], 0).expect("header stream should be routable"),
            Some((0, time_base))
        );
        assert_eq!(
            packet_stream_route(&[0], &[time_base], 1)
                .expect_err("post-header stream should be rejected"),
            ProcessorError::UnsupportedDynamicStream { stream_index: 1 }
        );
    }

    #[test]
    fn timestamp_repair_preserves_valid_monotonic_values() {
        let mut state = TimestampRepairState::default();
        let repaired = repair_timestamp_values(
            PacketTimestampValues {
                duration: 1024,
                dts: Some(2048),
                pts: Some(2048),
            },
            &mut state,
        )
        .expect("valid timestamps should repair");

        assert_eq!(
            repaired,
            PacketTimestampValues {
                duration: 1024,
                dts: Some(2048),
                pts: Some(2048),
            }
        );
        assert_eq!(state.last_dts, Some(2048));
        assert_eq!(state.last_duration, Some(1024));
    }

    #[test]
    fn timestamp_repair_reuses_last_positive_duration_for_missing_duration() {
        let mut state = TimestampRepairState::default();
        repair_timestamp_values(
            PacketTimestampValues {
                duration: 1024,
                dts: Some(0),
                pts: Some(0),
            },
            &mut state,
        )
        .expect("first packet should repair");

        let repaired = repair_timestamp_values(
            PacketTimestampValues {
                duration: 0,
                dts: None,
                pts: None,
            },
            &mut state,
        )
        .expect("missing duration should repair");

        assert_eq!(repaired.duration, 1024);
        assert_eq!(repaired.dts, Some(1024));
        assert_eq!(repaired.pts, Some(1024));
    }

    #[test]
    fn timestamp_repair_advances_non_monotonic_dts() {
        let mut state = TimestampRepairState {
            last_dts: Some(4096),
            last_duration: Some(1024),
        };

        let repaired = repair_timestamp_values(
            PacketTimestampValues {
                duration: -1,
                dts: Some(4096),
                pts: Some(4000),
            },
            &mut state,
        )
        .expect("non-monotonic dts should repair");

        assert_eq!(repaired.duration, 1024);
        assert_eq!(repaired.dts, Some(5120));
        assert_eq!(repaired.pts, Some(5120));
        assert_eq!(state.last_dts, Some(5120));
    }

    #[test]
    fn timestamp_repair_raises_pts_below_dts() {
        let mut state = TimestampRepairState::default();

        let repaired = repair_timestamp_values(
            PacketTimestampValues {
                duration: 30,
                dts: Some(90),
                pts: Some(60),
            },
            &mut state,
        )
        .expect("pts should repair");

        assert_eq!(repaired.dts, Some(90));
        assert_eq!(repaired.pts, Some(90));
    }

    #[test]
    fn timestamp_repair_reports_dts_overflow() {
        let mut state = TimestampRepairState {
            last_dts: Some(i64::MAX),
            last_duration: Some(1),
        };

        let error = repair_timestamp_values(
            PacketTimestampValues {
                duration: 0,
                dts: None,
                pts: None,
            },
            &mut state,
        )
        .expect_err("overflow should fail");

        assert!(error.to_string().contains("timestamp repair overflowed"));
    }

    #[test]
    fn ffmpeg_processor_declares_expected_capabilities() {
        let processor = FfmpegRemuxProcessor::new();

        assert_eq!(
            processor.supported_input_formats(),
            &[
                ContentFormatKind::HlsSegments,
                ContentFormatKind::DashSegments,
                ContentFormatKind::FlvSegments,
                ContentFormatKind::SingleFile,
            ]
        );
        assert_eq!(
            processor.capabilities().output_formats,
            vec![OutputFormat::Mp4, OutputFormat::Mkv]
        );
        assert!(processor.capabilities().supports_assembly);
        assert!(
            processor
                .capabilities()
                .supported_assembly_modes
                .contains(&AssemblyMode::SeparateAudioVideo)
        );
    }

    #[test]
    fn ffmpeg_processor_skips_single_file_inputs() {
        let processor = FfmpegRemuxProcessor::new();
        let progress = RecordingProgress::default();

        let result = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                PathBuf::from("/tmp/output.mp4").as_path(),
                &progress,
            )
            .expect("single-file input should be skipped");

        assert_eq!(result, ProcessorOutput::Skipped);
        assert!(progress.ratios().is_empty());
    }
}
