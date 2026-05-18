use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use player_plugin::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream, DownloadMetadata,
    OutputFormat, PipelineEvent, PostDownloadProcessor, ProcessorError, ProcessorOutput,
    ProcessorProgress, StreamKind,
};

use crate::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};

use super::executor::DownloadExecutor;
use super::manager::DownloadManager;
use super::store::DownloadStore;
use super::types::{
    DownloadAssetIndex, DownloadAssetStream, DownloadContentFormat, DownloadStreamKind,
    DownloadTaskSnapshot,
};

impl<S, E> DownloadManager<S, E>
where
    S: DownloadStore,
    E: DownloadExecutor,
{
    pub(super) fn run_post_processors(
        &self,
        snapshot: &DownloadTaskSnapshot,
    ) -> PlayerResult<Option<PathBuf>> {
        if self.config.post_processors.is_empty() {
            if download_streams_require_assembly(&snapshot.asset_index.streams) {
                let assembly_mode =
                    infer_assembly_mode_from_download_streams(&snapshot.asset_index.streams);
                return Err(assembly_required_error_for_mode(
                    snapshot,
                    assembly_mode,
                    false,
                ));
            }
            return Ok(snapshot.asset_index.completed_path.clone());
        }

        let progress = NoopProcessorProgress;
        let run =
            self.run_post_processor_chain(snapshot, ProcessorOutputPathPolicy::Derived, &progress)?;
        Ok(run.completed_path)
    }

    pub(super) fn export_processed_output(
        &self,
        snapshot: &DownloadTaskSnapshot,
        output_path: Option<&Path>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerResult<PathBuf> {
        let run = self.run_post_processor_chain(
            snapshot,
            ProcessorOutputPathPolicy::Export { output_path },
            progress,
        )?;
        if run.ran_processor
            && let Some(path) = run.completed_path
        {
            return Ok(path);
        }

        Err(PlayerError::with_category(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Capability,
            format!(
                "download task {} has no post-download processor available for export",
                snapshot.task_id.get()
            ),
        ))
    }

    fn run_post_processor_chain(
        &self,
        snapshot: &DownloadTaskSnapshot,
        output_path_policy: ProcessorOutputPathPolicy<'_>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerResult<PostProcessorRun> {
        let mut current_input = self.completed_download_info(snapshot)?;
        let mut current_completed_path = snapshot.asset_index.completed_path.clone();
        let mut ran_processor = false;
        let mut required_assembly = completed_info_requires_assembly(&current_input);

        for processor in &self.config.post_processors {
            let input_kind = current_input.content_format.kind();
            let can_process = if completed_info_requires_assembly(&current_input) {
                processor_can_assemble(processor, &current_input)
            } else {
                processor.supported_input_formats().contains(&input_kind)
            };
            if !can_process {
                continue;
            }

            let resolved_output_path = output_path_policy.resolve(
                snapshot,
                current_completed_path.as_deref(),
                processor,
                ran_processor,
            )?;

            ran_processor = true;
            self.dispatch_pipeline_event(PipelineEvent::PostProcessStarted {
                task_id: snapshot.task_id.get().to_string(),
                processor: processor.name().to_owned(),
            });

            let result = if completed_info_requires_assembly(&current_input) {
                processor.assemble(&current_input, &resolved_output_path, progress)
            } else {
                processor.process(&current_input, &resolved_output_path, progress)
            };

            match result {
                Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                        output_path: path.display().to_string(),
                    });
                    current_completed_path = Some(path.clone());
                    current_input = completed_info_for_processed_output(
                        snapshot,
                        path,
                        current_input.metadata.clone(),
                    );
                    required_assembly = completed_info_requires_assembly(&current_input);
                }
                Ok(ProcessorOutput::Skipped) => {}
                Err(error) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessFailed {
                        task_id: snapshot.task_id.get().to_string(),
                        error: error.to_string(),
                    });
                    return Err(map_processor_error(processor.name(), error));
                }
            }
        }

        if required_assembly {
            return Err(assembly_required_error(
                snapshot,
                &current_input,
                ran_processor,
            ));
        }

        Ok(PostProcessorRun {
            completed_path: current_completed_path,
            ran_processor,
        })
    }

    pub(super) fn export_single_file_output(
        &self,
        snapshot: &DownloadTaskSnapshot,
        output_path: Option<&Path>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerResult<PathBuf> {
        let source_path = resolve_single_file_path(snapshot)?;
        let Some(output_path) = output_path else {
            return Ok(source_path);
        };
        if paths_refer_to_same_file(&source_path, output_path) {
            return Ok(source_path);
        }

        copy_single_file_export(&source_path, output_path, progress)?;
        Ok(output_path.to_path_buf())
    }

    fn completed_download_info(
        &self,
        snapshot: &DownloadTaskSnapshot,
    ) -> PlayerResult<CompletedDownloadInfo> {
        let metadata = DownloadMetadata {
            source_uri: Some(snapshot.source.source.uri().to_owned()),
            manifest_uri: snapshot.source.manifest_uri.clone(),
            total_bytes: snapshot.progress.total_bytes,
            version: snapshot.asset_index.version.clone(),
            etag: snapshot.asset_index.etag.clone(),
            checksum: snapshot.asset_index.checksum.clone(),
            mime_type: None,
            custom: Default::default(),
        };

        let content_format = match snapshot.source.content_format {
            DownloadContentFormat::HlsSegments => CompletedContentFormat::HlsSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::DashSegments => CompletedContentFormat::DashSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::FlvSegments => CompletedContentFormat::FlvSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::SingleFile => CompletedContentFormat::SingleFile {
                path: resolve_single_file_path(snapshot)?,
            },
            DownloadContentFormat::Unknown => {
                return Err(PlayerError::with_category(
                    PlayerErrorCode::Unsupported,
                    PlayerErrorCategory::Capability,
                    format!(
                        "download task {} has unknown content format for post-processing",
                        snapshot.task_id.get()
                    ),
                ));
            }
        };

        Ok(CompletedDownloadInfo {
            asset_id: snapshot.asset_id.as_str().to_owned(),
            task_id: Some(snapshot.task_id.get().to_string()),
            content_format,
            metadata,
            streams: completed_streams_for_snapshot(snapshot)?,
            assembly_mode: infer_assembly_mode_from_download_streams(&snapshot.asset_index.streams),
        })
    }
}

pub(super) struct NoopProcessorProgress;

impl ProcessorProgress for NoopProcessorProgress {
    fn on_progress(&self, _ratio: f32) {}
}

fn completed_info_requires_assembly(input: &CompletedDownloadInfo) -> bool {
    input.assembly_mode != AssemblyMode::Single && input.streams.len() > 1
}

fn download_streams_require_assembly(streams: &[DownloadAssetStream]) -> bool {
    infer_assembly_mode_from_download_streams(streams) != AssemblyMode::Single && streams.len() > 1
}

fn processor_can_assemble(
    processor: &Arc<dyn PostDownloadProcessor>,
    input: &CompletedDownloadInfo,
) -> bool {
    if !processor.supports_assembly() {
        return false;
    }
    let capabilities = processor.capabilities();
    capabilities
        .supported_assembly_modes
        .contains(&input.assembly_mode)
        && input.streams.iter().all(|stream| {
            capabilities
                .supported_input_formats
                .contains(&stream.content_format.kind())
        })
}

#[derive(Clone, Copy)]
enum ProcessorOutputPathPolicy<'a> {
    Derived,
    Export { output_path: Option<&'a Path> },
}

impl ProcessorOutputPathPolicy<'_> {
    fn resolve(
        self,
        snapshot: &DownloadTaskSnapshot,
        current_completed_path: Option<&Path>,
        processor: &Arc<dyn PostDownloadProcessor>,
        ran_processor: bool,
    ) -> PlayerResult<PathBuf> {
        match self {
            Self::Derived => {
                derive_processor_output_path(snapshot, current_completed_path, processor)
            }
            Self::Export { output_path } => {
                if ran_processor {
                    derive_processor_output_path(snapshot, current_completed_path, processor)
                } else if let Some(output_path) = output_path {
                    Ok(output_path.to_path_buf())
                } else {
                    derive_processor_output_path(snapshot, current_completed_path, processor)
                }
            }
        }
    }
}

struct PostProcessorRun {
    completed_path: Option<PathBuf>,
    ran_processor: bool,
}

fn assembly_required_error(
    snapshot: &DownloadTaskSnapshot,
    input: &CompletedDownloadInfo,
    ran_processor: bool,
) -> PlayerError {
    assembly_required_error_for_mode(snapshot, input.assembly_mode, ran_processor)
}

fn assembly_required_error_for_mode(
    snapshot: &DownloadTaskSnapshot,
    assembly_mode: AssemblyMode,
    ran_processor: bool,
) -> PlayerError {
    let detail = if ran_processor {
        "no processor produced an assembled output"
    } else {
        "no processor supports this assembly mode"
    };
    PlayerError::with_category(
        PlayerErrorCode::Unsupported,
        PlayerErrorCategory::Capability,
        format!(
            "download task {} requires post-download assembly for {:?}: {detail}",
            snapshot.task_id.get(),
            assembly_mode
        ),
    )
}

fn completed_info_for_processed_output(
    snapshot: &DownloadTaskSnapshot,
    path: PathBuf,
    metadata: DownloadMetadata,
) -> CompletedDownloadInfo {
    let content_format = CompletedContentFormat::SingleFile { path: path.clone() };
    CompletedDownloadInfo {
        asset_id: snapshot.asset_id.as_str().to_owned(),
        task_id: Some(snapshot.task_id.get().to_string()),
        content_format: content_format.clone(),
        metadata: metadata.clone(),
        streams: vec![CompletedStream {
            stream_id: Some("combined".to_owned()),
            kind: StreamKind::Combined,
            content_format,
            language: None,
            codec: None,
            label: None,
            metadata,
            quality_rank: None,
        }],
        assembly_mode: AssemblyMode::Single,
    }
}

fn completed_streams_for_snapshot(
    snapshot: &DownloadTaskSnapshot,
) -> PlayerResult<Vec<CompletedStream>> {
    let streams = effective_asset_streams(&snapshot.asset_index);
    streams
        .iter()
        .map(|stream| {
            Ok(CompletedStream {
                stream_id: Some(stream.stream_id.clone()),
                kind: download_stream_kind_to_plugin(stream.kind),
                content_format: content_format_for_stream(snapshot, stream)?,
                language: stream.language.clone(),
                codec: stream.codec.clone(),
                label: stream.label.clone(),
                metadata: DownloadMetadata {
                    source_uri: None,
                    manifest_uri: None,
                    total_bytes: None,
                    version: None,
                    etag: None,
                    checksum: None,
                    mime_type: None,
                    custom: stream.metadata.clone().into_iter().collect(),
                },
                quality_rank: stream.quality_rank,
            })
        })
        .collect()
}

fn effective_asset_streams(asset_index: &DownloadAssetIndex) -> Vec<DownloadAssetStream> {
    if !asset_index.streams.is_empty() {
        return asset_index.streams.clone();
    }
    vec![DownloadAssetStream {
        stream_id: "combined".to_owned(),
        kind: DownloadStreamKind::Combined,
        language: None,
        codec: None,
        label: None,
        quality_rank: None,
        resource_ids: asset_index
            .resources
            .iter()
            .map(|resource| resource.resource_id.clone())
            .collect(),
        segment_ids: asset_index
            .segments
            .iter()
            .map(|segment| segment.segment_id.clone())
            .collect(),
        metadata: HashMap::new(),
    }]
}

fn content_format_for_stream(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> PlayerResult<CompletedContentFormat> {
    match snapshot.source.content_format {
        DownloadContentFormat::HlsSegments => Ok(CompletedContentFormat::HlsSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "m3u8")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::DashSegments => Ok(CompletedContentFormat::DashSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "mpd")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::FlvSegments => Ok(CompletedContentFormat::FlvSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "ffconcat")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::SingleFile => Ok(CompletedContentFormat::SingleFile {
            path: resolve_stream_single_file_path(snapshot, stream)?,
        }),
        DownloadContentFormat::Unknown => Err(PlayerError::with_category(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Capability,
            format!(
                "download task {} has unknown content format for stream assembly",
                snapshot.task_id.get()
            ),
        )),
    }
}

fn resolve_stream_manifest_path(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
    extension: &str,
) -> PlayerResult<PathBuf> {
    let target_directory = target_directory_for_snapshot(snapshot)?;
    stream
        .resource_ids
        .iter()
        .filter_map(|resource_id| {
            snapshot
                .asset_index
                .resources
                .iter()
                .find(|resource| &resource.resource_id == resource_id)
        })
        .find_map(|resource| {
            let relative_path = resource.relative_path.as_ref()?;
            relative_path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                .then(|| target_directory.join(relative_path))
        })
        .or_else(|| resolve_manifest_path(snapshot).ok())
        .ok_or_else(|| {
            PlayerError::with_category(
                PlayerErrorCode::InvalidState,
                PlayerErrorCategory::Playback,
                format!(
                    "download task {} is missing a stream manifest for `{}`",
                    snapshot.task_id.get(),
                    stream.stream_id
                ),
            )
        })
}

fn resolve_stream_segment_paths(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> Vec<PathBuf> {
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Vec::new();
    };
    if stream.segment_ids.is_empty() {
        return resolve_segment_paths(snapshot);
    }
    stream
        .segment_ids
        .iter()
        .filter_map(|segment_id| {
            snapshot
                .asset_index
                .segments
                .iter()
                .find(|segment| &segment.segment_id == segment_id)
                .and_then(|segment| segment.relative_path.as_ref())
                .map(|relative_path| target_directory.join(relative_path))
        })
        .collect()
}

fn resolve_stream_single_file_path(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> PlayerResult<PathBuf> {
    let target_directory = target_directory_for_snapshot(snapshot)?;
    stream
        .resource_ids
        .iter()
        .filter_map(|resource_id| {
            snapshot
                .asset_index
                .resources
                .iter()
                .find(|resource| &resource.resource_id == resource_id)
        })
        .find_map(|resource| {
            resource
                .relative_path
                .as_ref()
                .map(|relative_path| target_directory.join(relative_path))
        })
        .or_else(|| resolve_single_file_path(snapshot).ok())
        .ok_or_else(|| {
            PlayerError::with_category(
                PlayerErrorCode::InvalidState,
                PlayerErrorCategory::Playback,
                format!(
                    "download task {} is missing a stream file for `{}`",
                    snapshot.task_id.get(),
                    stream.stream_id
                ),
            )
        })
}

fn target_directory_for_snapshot(snapshot: &DownloadTaskSnapshot) -> PlayerResult<&Path> {
    snapshot.profile.target_directory.as_deref().ok_or_else(|| {
        PlayerError::with_category(
            PlayerErrorCode::InvalidState,
            PlayerErrorCategory::Playback,
            format!(
                "download task {} is missing target directory",
                snapshot.task_id.get()
            ),
        )
    })
}

fn infer_assembly_mode_from_download_streams(streams: &[DownloadAssetStream]) -> AssemblyMode {
    if streams.len() <= 1 {
        return AssemblyMode::Single;
    }

    let has_video = streams
        .iter()
        .any(|stream| matches!(stream.kind, DownloadStreamKind::Video));
    let audio_count = streams
        .iter()
        .filter(|stream| {
            matches!(
                stream.kind,
                DownloadStreamKind::Audio | DownloadStreamKind::SecondaryAudio
            )
        })
        .count();
    let has_subtitle = streams
        .iter()
        .any(|stream| matches!(stream.kind, DownloadStreamKind::Subtitle));

    if has_subtitle {
        AssemblyMode::WithSubtitles
    } else if has_video && audio_count > 1 {
        AssemblyMode::MultiAudio
    } else if has_video && audio_count == 1 {
        AssemblyMode::SeparateAudioVideo
    } else {
        AssemblyMode::Generic
    }
}

fn download_stream_kind_to_plugin(kind: DownloadStreamKind) -> StreamKind {
    match kind {
        DownloadStreamKind::Combined => StreamKind::Combined,
        DownloadStreamKind::Video => StreamKind::Video,
        DownloadStreamKind::Audio => StreamKind::Audio,
        DownloadStreamKind::SecondaryAudio => StreamKind::SecondaryAudio,
        DownloadStreamKind::Subtitle => StreamKind::Subtitle,
        DownloadStreamKind::Auxiliary => StreamKind::Auxiliary,
    }
}

fn derive_processor_output_path(
    snapshot: &DownloadTaskSnapshot,
    current_completed_path: Option<&Path>,
    processor: &Arc<dyn PostDownloadProcessor>,
) -> PlayerResult<PathBuf> {
    let extension = processor
        .capabilities()
        .output_formats
        .first()
        .map(output_format_extension)
        .or_else(|| {
            current_completed_path
                .and_then(Path::extension)
                .and_then(|extension| extension.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "bin".to_owned());

    if let Some(path) = current_completed_path {
        if path.extension().is_some() {
            return Ok(path.with_extension(&extension));
        }
        return Ok(path.join(format!(
            "{}.{extension}",
            sanitize_asset_id(snapshot.asset_id.as_str())
        )));
    }

    if let Some(base_dir) = snapshot.profile.target_directory.as_ref() {
        return Ok(base_dir.join(format!(
            "{}.{extension}",
            sanitize_asset_id(snapshot.asset_id.as_str())
        )));
    }

    Err(PlayerError::with_category(
        PlayerErrorCode::InvalidState,
        PlayerErrorCategory::Playback,
        format!(
            "download task {} has no completed path or target directory for processor `{}` output",
            snapshot.task_id.get(),
            processor.name(),
        ),
    ))
}

fn output_format_extension(format: &OutputFormat) -> String {
    match format {
        OutputFormat::Mp4 => "mp4".to_owned(),
        OutputFormat::Mkv => "mkv".to_owned(),
        OutputFormat::Original => "bin".to_owned(),
    }
}

pub(super) fn should_run_post_processors_on_completion(snapshot: &DownloadTaskSnapshot) -> bool {
    match snapshot.source.content_format {
        DownloadContentFormat::HlsSegments
        | DownloadContentFormat::DashSegments
        | DownloadContentFormat::FlvSegments => snapshot
            .profile
            .target_output_format
            .as_ref()
            .is_none_or(|format| *format == OutputFormat::Mp4),
        DownloadContentFormat::SingleFile => snapshot
            .profile
            .target_output_format
            .as_ref()
            .is_some_and(|format| *format == OutputFormat::Mp4),
        DownloadContentFormat::Unknown => false,
    }
}

fn sanitize_asset_id(asset_id: &str) -> String {
    let sanitized = asset_id
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '_',
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "download".to_owned()
    } else {
        sanitized
    }
}

fn resolve_manifest_path(snapshot: &DownloadTaskSnapshot) -> PlayerResult<PathBuf> {
    if let Some(path) = snapshot.asset_index.completed_path.as_ref()
        && is_manifest_path_for_format(path, snapshot.source.content_format)
    {
        return Ok(path.clone());
    }

    if let Some(path) = snapshot
        .asset_index
        .resources
        .iter()
        .find_map(|resource| {
            resolve_index_path(snapshot, resource.relative_path.as_deref(), &resource.uri)
        })
        .filter(|path| is_manifest_path_for_format(path, snapshot.source.content_format))
    {
        return Ok(path);
    }

    if let Some(path) = snapshot
        .source
        .manifest_uri
        .as_deref()
        .and_then(resolve_uri_to_path)
    {
        return Ok(path);
    }

    Err(PlayerError::with_category(
        PlayerErrorCode::InvalidSource,
        PlayerErrorCategory::Source,
        format!(
            "download task {} is missing a local manifest path for post-processing",
            snapshot.task_id.get()
        ),
    ))
}

fn is_manifest_path_for_format(path: &Path, content_format: DownloadContentFormat) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    match content_format {
        DownloadContentFormat::HlsSegments => extension.eq_ignore_ascii_case("m3u8"),
        DownloadContentFormat::DashSegments => extension.eq_ignore_ascii_case("mpd"),
        DownloadContentFormat::FlvSegments => {
            extension.eq_ignore_ascii_case("ffconcat")
                || extension.eq_ignore_ascii_case("txt")
                || extension.eq_ignore_ascii_case("flv")
        }
        DownloadContentFormat::SingleFile | DownloadContentFormat::Unknown => false,
    }
}

fn resolve_segment_paths(snapshot: &DownloadTaskSnapshot) -> Vec<PathBuf> {
    snapshot
        .asset_index
        .segments
        .iter()
        .filter_map(|segment| {
            resolve_index_path(snapshot, segment.relative_path.as_deref(), &segment.uri)
        })
        .collect()
}

fn resolve_single_file_path(snapshot: &DownloadTaskSnapshot) -> PlayerResult<PathBuf> {
    if let Some(path) = snapshot.asset_index.completed_path.as_ref() {
        return Ok(path.clone());
    }

    if let Some(path) = snapshot.asset_index.resources.iter().find_map(|resource| {
        resolve_index_path(snapshot, resource.relative_path.as_deref(), &resource.uri)
    }) {
        return Ok(path);
    }

    if let Some(path) = resolve_uri_to_path(snapshot.source.source.uri()) {
        return Ok(path);
    }

    Err(PlayerError::with_category(
        PlayerErrorCode::InvalidSource,
        PlayerErrorCategory::Source,
        format!(
            "download task {} is missing a local completed file path for post-processing",
            snapshot.task_id.get()
        ),
    ))
}

fn copy_single_file_export(
    source_path: &Path,
    output_path: &Path,
    progress: &dyn ProcessorProgress,
) -> PlayerResult<()> {
    if progress.is_cancelled() {
        return Err(PlayerError::with_category(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            "download export was cancelled",
        ));
    }

    let total_bytes = fs::metadata(source_path)
        .map_err(|error| export_io_error(source_path, "read source metadata", error))?
        .len();
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| export_io_error(parent, "create export directory", error))?;
    }

    let mut input = File::open(source_path)
        .map_err(|error| export_io_error(source_path, "open export source", error))?;
    let mut output = File::create(output_path)
        .map_err(|error| export_io_error(output_path, "create export output", error))?;
    let mut copied_bytes = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];

    loop {
        if progress.is_cancelled() {
            return Err(PlayerError::with_category(
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Platform,
                "download export was cancelled",
            ));
        }
        let read = input
            .read(&mut buffer)
            .map_err(|error| export_io_error(source_path, "read export source", error))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| export_io_error(output_path, "write export output", error))?;
        copied_bytes = copied_bytes.saturating_add(read as u64);
        if total_bytes > 0 {
            progress.on_progress((copied_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0));
        }
    }
    output
        .sync_all()
        .map_err(|error| export_io_error(output_path, "flush export output", error))?;
    progress.on_progress(1.0);
    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn export_io_error(path: &Path, operation: &str, error: std::io::Error) -> PlayerError {
    PlayerError::with_category(
        PlayerErrorCode::BackendFailure,
        PlayerErrorCategory::Platform,
        format!("failed to {operation} `{}`: {error}", path.display()),
    )
}

fn resolve_index_path(
    snapshot: &DownloadTaskSnapshot,
    relative_path: Option<&Path>,
    uri: &str,
) -> Option<PathBuf> {
    if let Some(relative_path) = relative_path {
        if relative_path.is_absolute() {
            return Some(relative_path.to_path_buf());
        }
        if let Some(base_dir) = snapshot.profile.target_directory.as_ref() {
            return Some(base_dir.join(relative_path));
        }
    }

    resolve_uri_to_path(uri)
}

fn resolve_uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Some(path) = uri.strip_prefix("file://") {
        if path.is_empty() {
            return None;
        }
        return Some(PathBuf::from(path));
    }

    if uri.contains("://") {
        return None;
    }

    if uri.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(uri))
    }
}

fn map_processor_error(processor_name: &str, error: ProcessorError) -> PlayerError {
    match error {
        ProcessorError::UnsupportedFormat(_) => PlayerError::with_category(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Capability,
            format!("post-processor `{processor_name}` does not support this download format"),
        ),
        ProcessorError::PayloadCodec(message) => PlayerError::with_category(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            format!("post-processor `{processor_name}` exchanged invalid payload: {message}"),
        ),
        ProcessorError::AbiViolation(message) => PlayerError::with_category(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            format!("post-processor `{processor_name}` violated plugin ABI: {message}"),
        ),
        ProcessorError::OutputPath(message) => PlayerError::with_category(
            PlayerErrorCode::InvalidArgument,
            PlayerErrorCategory::Input,
            format!("post-processor `{processor_name}` output path error: {message}"),
        ),
        ProcessorError::Cancelled => PlayerError::with_category(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Playback,
            format!("post-processor `{processor_name}` was cancelled"),
        ),
        ProcessorError::MuxFailed(message) => PlayerError::with_category(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            format!("post-processor `{processor_name}` failed: {message}"),
        ),
    }
}
