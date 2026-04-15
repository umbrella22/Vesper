use std::ffi::CString;
use std::fs;
use std::path::Path;

use ffmpeg::{Rational, codec, encoder, format, media};
use ffmpeg_next as ffmpeg;
use player_plugin::{
    CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, OutputFormat,
    PostDownloadProcessor, ProcessorCapabilities, ProcessorError, ProcessorOutput,
    ProcessorProgress,
};

use crate::error::FfmpegProcessorError;

#[derive(Debug, Default)]
pub struct FfmpegPostDownloadProcessor;

impl FfmpegPostDownloadProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl PostDownloadProcessor for FfmpegPostDownloadProcessor {
    fn name(&self) -> &str {
        "player-ffmpeg"
    }

    fn supported_input_formats(&self) -> &[ContentFormatKind] {
        static SUPPORTED: [ContentFormatKind; 2] = [
            ContentFormatKind::HlsSegments,
            ContentFormatKind::DashSegments,
        ];
        &SUPPORTED
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        ProcessorCapabilities {
            supported_input_formats: self.supported_input_formats().to_vec(),
            output_formats: vec![OutputFormat::Mp4],
            supports_cancellation: true,
        }
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let manifest_path = match &input.content_format {
            CompletedContentFormat::HlsSegments { manifest_path, .. } => {
                ensure_demuxer("hls", ContentFormatKind::HlsSegments)?;
                manifest_path
            }
            CompletedContentFormat::DashSegments { manifest_path, .. } => {
                ensure_demuxer("dash", ContentFormatKind::DashSegments)?;
                manifest_path
            }
            CompletedContentFormat::SingleFile { .. } => return Ok(ProcessorOutput::Skipped),
        };

        initialize_ffmpeg()?;
        if progress.is_cancelled() {
            return Err(ProcessorError::Cancelled);
        }

        remux_manifest_to_mp4(manifest_path, output_path, progress)?;
        Ok(ProcessorOutput::MuxedFile {
            path: output_path.to_path_buf(),
            format: OutputFormat::Mp4,
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

    if unsafe { ffmpeg::ffi::av_find_input_format(c_name.as_ptr()).is_null() } {
        return Err(match content_format {
            ContentFormatKind::HlsSegments | ContentFormatKind::DashSegments => {
                ProcessorError::UnsupportedFormat(content_format)
            }
            _ => FfmpegProcessorError::MissingDemuxer(name).into_processor_error(),
        });
    }

    Ok(())
}

fn remux_manifest_to_mp4(
    manifest_path: &Path,
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

    let input_path = manifest_path.to_string_lossy().into_owned();
    let output_path_string = output_path.to_string_lossy().into_owned();

    let mut input_context = format::input(&input_path).map_err(|error| {
        FfmpegProcessorError::Remux(format!(
            "failed to open manifest `{}`: {error}",
            manifest_path.display()
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

    let mut stream_mapping = vec![0; input_context.nb_streams() as _];
    let mut input_time_bases = vec![Rational(0, 1); input_context.nb_streams() as _];
    let mut output_stream_index = 0;

    for (input_stream_index, input_stream) in input_context.streams().enumerate() {
        let medium = input_stream.parameters().medium();
        if medium != media::Type::Audio
            && medium != media::Type::Video
            && medium != media::Type::Subtitle
        {
            stream_mapping[input_stream_index] = -1;
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
        unsafe {
            (*output_stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
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
    use super::FfmpegPostDownloadProcessor;
    use player_plugin::{
        CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DownloadMetadata,
        OutputFormat, PostDownloadProcessor, ProcessorOutput, ProcessorProgress,
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
        let processor = FfmpegPostDownloadProcessor::new();

        assert_eq!(
            processor.supported_input_formats(),
            &[
                ContentFormatKind::HlsSegments,
                ContentFormatKind::DashSegments
            ]
        );
        assert_eq!(
            processor.capabilities().output_formats,
            vec![OutputFormat::Mp4]
        );
    }

    #[test]
    fn ffmpeg_processor_skips_single_file_inputs() {
        let processor = FfmpegPostDownloadProcessor::new();
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
                },
                PathBuf::from("/tmp/output.mp4").as_path(),
                &progress,
            )
            .expect("single-file input should be skipped");

        assert_eq!(result, ProcessorOutput::Skipped);
        assert!(progress.ratios().is_empty());
    }
}
