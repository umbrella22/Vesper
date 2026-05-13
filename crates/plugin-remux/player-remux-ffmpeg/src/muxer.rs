use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};

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
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            FfmpegProcessorError::Io(format!(
                "failed to create parent directory `{}`: {error}",
                parent.display()
            ))
            .into_processor_error()
        })?;
    }

    if output_path.exists() {
        fs::remove_file(output_path).map_err(|error| {
            FfmpegProcessorError::Io(format!(
                "failed to replace existing output `{}`: {error}",
                output_path.display()
            ))
            .into_processor_error()
        })?;
    }

    let input_path_string = input_path.to_string_lossy().into_owned();
    let output_path_string = output_path.to_string_lossy().into_owned();

    let mut input_context = format::input(&input_path_string).map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to open input `{}`: {error}",
            input_path.display()
        ))
        .into_processor_error()
    })?;
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

    for (stream, mut packet) in input_context.packets() {
        if progress.is_cancelled() {
            let _ = fs::remove_file(output_path);
            return Err(ProcessorError::Cancelled);
        }

        let input_stream_index = stream.index();
        let mapped_stream_index = stream_mapping[input_stream_index];
        if mapped_stream_index < 0 {
            continue;
        }
        repair_packet_timestamps(&mut packet, &mut timestamp_state[input_stream_index]);

        let output_stream = output_context
            .stream(mapped_stream_index as _)
            .ok_or_else(|| {
                FfmpegProcessorError::Remux(format!(
                    "missing mapped output stream index {} for `{}`",
                    mapped_stream_index,
                    output_path.display()
                ))
                .into_processor_error()
            })?;

        packet.rescale_ts(
            input_time_bases[input_stream_index],
            output_stream.time_base(),
        );
        packet.set_position(-1);
        packet.set_stream(mapped_stream_index as _);
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

    output_context.write_trailer().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to finalize output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    progress.on_progress(1.0);

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct TimestampRepairState {
    last_dts: Option<i64>,
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
    if sources.is_empty() {
        return Err(ProcessorError::AbiViolation(
            "assembly input must contain at least one stream".to_owned(),
        ));
    }

    prepare_output_path(output_path)?;
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
        ensure_demuxer_for_source(source)?;
        let input_path_string = source.path.to_string_lossy().into_owned();
        let input_context = format::input(&input_path_string).map_err(|error| {
            FfmpegProcessorError::Remux(format!(
                "failed to open input `{}`: {error}",
                source.path.display()
            ))
            .into_processor_error()
        })?;

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
        for (stream, mut packet) in state.context.packets() {
            if progress.is_cancelled() {
                let _ = fs::remove_file(output_path);
                return Err(ProcessorError::Cancelled);
            }

            let input_stream_index = stream.index();
            let mapped_stream_index = state.stream_mapping[input_stream_index];
            if mapped_stream_index < 0 {
                continue;
            }

            repair_packet_timestamps(&mut packet, &mut state.timestamp_state[input_stream_index]);
            let output_stream = output_context
                .stream(mapped_stream_index as usize)
                .ok_or_else(|| {
                    FfmpegProcessorError::Remux(format!(
                        "missing mapped output stream index {} for `{}`",
                        mapped_stream_index,
                        output_path.display()
                    ))
                    .into_processor_error()
                })?;

            packet.rescale_ts(
                state.input_time_bases[input_stream_index],
                output_stream.time_base(),
            );
            packet.set_position(-1);
            packet.set_stream(mapped_stream_index as usize);
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

    output_context.write_trailer().map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to finalize output `{}`: {error}",
            output_path.display()
        ))
        .into_processor_error()
    })?;
    progress.on_progress(1.0);
    Ok(())
}

fn prepare_output_path(output_path: &Path) -> Result<(), ProcessorError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            FfmpegProcessorError::Io(format!(
                "failed to create parent directory `{}`: {error}",
                parent.display()
            ))
            .into_processor_error()
        })?;
    }

    if output_path.exists() {
        fs::remove_file(output_path).map_err(|error| {
            FfmpegProcessorError::Io(format!(
                "failed to replace existing output `{}`: {error}",
                output_path.display()
            ))
            .into_processor_error()
        })?;
    }
    Ok(())
}

fn repair_packet_timestamps(packet: &mut ffmpeg::Packet, state: &mut TimestampRepairState) {
    let duration = packet.duration().max(1);
    if packet.duration() <= 0 {
        packet.set_duration(duration);
    }

    let mut dts = packet.dts();
    if dts.is_none_or(|value| state.last_dts.is_some_and(|last| value <= last)) {
        dts = Some(state.last_dts.map_or(0, |last| last + duration));
        packet.set_dts(dts);
    }

    let dts_value = dts.unwrap_or(0);
    if packet.pts().is_none_or(|pts| pts < dts_value) {
        packet.set_pts(Some(dts_value));
    }
    state.last_dts = Some(dts_value);
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
            | FfmpegProcessorError::Io(message)
            | FfmpegProcessorError::Remux(message) => ProcessorError::MuxFailed(message),
            FfmpegProcessorError::MissingDemuxer(_) => {
                ProcessorError::UnsupportedFormat(ContentFormatKind::Unknown)
            }
            FfmpegProcessorError::InvalidPath(message) => ProcessorError::OutputPath(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegRemuxProcessor;
    use player_plugin::{
        AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind,
        DownloadMetadata, OutputFormat, PostDownloadProcessor, ProcessorOutput, ProcessorProgress,
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
