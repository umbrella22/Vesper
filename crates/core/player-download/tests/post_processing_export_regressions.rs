use player_download::{
    DownloadAssetIndex, DownloadContentFormat, DownloadManager, DownloadManagerConfig,
    DownloadProfile, DownloadSegmentRecord, DownloadSource, DownloadTaskId, DownloadTaskStatus,
    InMemoryDownloadExecutor, InMemoryDownloadStore, PlayerErrorCategory, PlayerErrorCode,
    PostDownloadProcessorRegistration,
};
use player_model::MediaSource;
use player_plugin::{
    CompletedDownloadInfo, ContentFormatKind, OutputFormat, PostDownloadProcessor,
    ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const MANIFEST_BYTES: &[u8] = b"manifest fixture, never an MP4 export";
const MEDIA_BYTES: &[u8] = b"muxed MP4 fixture";
const OLD_EXPORT_BYTES: &[u8] = b"previous export";
const SEGMENT_FORMATS: [DownloadContentFormat; 3] = [
    DownloadContentFormat::HlsSegments,
    DownloadContentFormat::DashSegments,
    DownloadContentFormat::FlvSegments,
];

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vesper-skipped-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("fixture directory should be unique");
        Self(path)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

enum FixtureProcessor {
    Skip,
    Mux { path: PathBuf },
}

impl PostDownloadProcessor for FixtureProcessor {
    fn name(&self) -> &str {
        "export-regression-fixture"
    }

    fn supported_input_formats(&self) -> &[ContentFormatKind] {
        &[
            ContentFormatKind::HlsSegments,
            ContentFormatKind::DashSegments,
            ContentFormatKind::FlvSegments,
            ContentFormatKind::SingleFile,
        ]
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        ProcessorCapabilities {
            supported_input_formats: self.supported_input_formats().to_vec(),
            output_formats: vec![OutputFormat::Mp4],
            supports_cancellation: false,
            supports_assembly: false,
            supported_assembly_modes: Vec::new(),
        }
    }

    fn process(
        &self,
        _input: &CompletedDownloadInfo,
        _output_path: &Path,
        _progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        match self {
            Self::Skip => Ok(ProcessorOutput::Skipped),
            Self::Mux { path } => {
                fs::write(path, MEDIA_BYTES)
                    .map_err(|error| ProcessorError::MuxFailed(error.to_string()))?;
                Ok(ProcessorOutput::MuxedFile {
                    path: path.clone(),
                    format: OutputFormat::Mp4,
                })
            }
        }
    }
}

struct NoProgress;

impl ProcessorProgress for NoProgress {
    fn on_progress(&self, _ratio: f32) {}
}

fn completed_task(
    directory: &Path,
    format: DownloadContentFormat,
    processors: Vec<FixtureProcessor>,
    process_on_completion: bool,
) -> (
    DownloadManager<InMemoryDownloadStore, InMemoryDownloadExecutor>,
    DownloadTaskId,
    PathBuf,
) {
    let extension = match format {
        DownloadContentFormat::HlsSegments => "m3u8",
        DownloadContentFormat::DashSegments => "mpd",
        DownloadContentFormat::FlvSegments => "ffconcat",
        DownloadContentFormat::SingleFile => "mp4",
        DownloadContentFormat::Unknown => unreachable!("fixture format must be known"),
    };
    let source_path = directory.join(format!("input.{extension}"));
    let source_bytes = if format == DownloadContentFormat::SingleFile {
        MEDIA_BYTES
    } else {
        MANIFEST_BYTES
    };
    fs::write(&source_path, source_bytes).expect("source fixture should exist");
    let segments = if format == DownloadContentFormat::SingleFile {
        Vec::new()
    } else {
        fs::write(directory.join("segment.bin"), MEDIA_BYTES)
            .expect("segment fixture should exist");
        vec![DownloadSegmentRecord {
            segment_id: "segment-1".to_owned(),
            uri: "https://fixture.test/segment.bin".to_owned(),
            relative_path: Some(PathBuf::from("segment.bin")),
            sequence: Some(1),
            byte_range: None,
            size_bytes: Some(MEDIA_BYTES.len() as u64),
            checksum: None,
        }]
    };
    let mut manager = DownloadManager::new(
        DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: process_on_completion,
            post_processors: processors
                .into_iter()
                .map(|processor| PostDownloadProcessorRegistration::built_in(Arc::new(processor)))
                .collect(),
            ..DownloadManagerConfig::default()
        },
        InMemoryDownloadStore::default(),
        InMemoryDownloadExecutor::default(),
    );
    let now = Instant::now();
    let task_id = manager
        .create_task(
            "export-regression",
            DownloadSource::new(MediaSource::new("https://fixture.test/media"), format),
            DownloadProfile {
                target_directory: Some(directory.to_path_buf()),
                target_output_format: Some(OutputFormat::Mp4),
                ..DownloadProfile::default()
            },
            DownloadAssetIndex {
                content_format: format,
                segments,
                ..DownloadAssetIndex::default()
            },
            now,
        )
        .expect("task should be created");
    let completed = manager
        .complete_task(task_id, Some(source_path.clone()), now)
        .expect("download completion should succeed")
        .expect("completed task should exist");
    assert_eq!(completed.status, DownloadTaskStatus::Completed);
    assert_eq!(
        completed.asset_index.completed_path,
        Some(source_path.clone())
    );
    (manager, task_id, source_path)
}

#[test]
fn skipped_segment_export_is_unsupported_and_preserves_files() {
    for format in SEGMENT_FORMATS {
        for process_on_completion in [false, true] {
            let directory = FixtureDirectory::new();
            let (manager, task_id, manifest_path) = completed_task(
                &directory.0,
                format,
                vec![FixtureProcessor::Skip, FixtureProcessor::Skip],
                process_on_completion,
            );
            let existing_output = directory.0.join("existing.mp4");
            let new_output = directory.0.join("new.mp4");
            fs::write(&existing_output, OLD_EXPORT_BYTES).expect("previous export should exist");
            for output in [
                None,
                Some(new_output.as_path()),
                Some(existing_output.as_path()),
            ] {
                let result = manager.export_task_output(task_id, output, &NoProgress);
                assert_eq!(
                    fs::read(&manifest_path).expect("manifest survives"),
                    MANIFEST_BYTES
                );
                assert_eq!(
                    fs::read(directory.0.join("segment.bin")).expect("segment survives"),
                    MEDIA_BYTES
                );
                assert_eq!(
                    fs::read(&existing_output).expect("previous export survives"),
                    OLD_EXPORT_BYTES
                );
                assert!(
                    !new_output.exists(),
                    "a skipped processor must not publish a manifest as MP4"
                );
                let error =
                    result.expect_err("skipped segments must not be reported as exported media");
                assert_eq!(error.code(), PlayerErrorCode::Unsupported, "{format:?}");
                assert_eq!(error.category(), PlayerErrorCategory::Capability);
                assert!(
                    error
                        .to_string()
                        .contains("no post-download processor produced a single-file output")
                );
            }
        }
    }
}

#[test]
fn segment_export_without_processor_is_unsupported() {
    for format in SEGMENT_FORMATS {
        let directory = FixtureDirectory::new();
        let (manager, task_id, manifest_path) = completed_task(&directory.0, format, vec![], false);
        let output = directory.0.join("export.mp4");
        let error = manager
            .export_task_output(task_id, Some(&output), &NoProgress)
            .expect_err("segment export requires a processor");
        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        assert_eq!(error.category(), PlayerErrorCategory::Capability);
        assert!(!output.exists());
        assert_eq!(
            fs::read(manifest_path).expect("manifest survives"),
            MANIFEST_BYTES
        );
    }
}

#[test]
fn muxed_segment_export_survives_skipped_processors() {
    for format in SEGMENT_FORMATS {
        for skip_before in [false, true] {
            for skip_after in [false, true] {
                let directory = FixtureDirectory::new();
                let muxed_path = directory.0.join("processor.mp4");
                let mut processors = Vec::new();
                if skip_before {
                    processors.push(FixtureProcessor::Skip);
                }
                processors.push(FixtureProcessor::Mux {
                    path: muxed_path.clone(),
                });
                if skip_after {
                    processors.push(FixtureProcessor::Skip);
                }
                let (manager, task_id, manifest_path) =
                    completed_task(&directory.0, format, processors, false);
                let output = directory.0.join("export.mp4");
                fs::write(&output, OLD_EXPORT_BYTES).expect("previous export should exist");
                assert_eq!(
                    manager
                        .export_task_output(task_id, Some(&output), &NoProgress)
                        .expect("actual muxed output can be exported"),
                    output
                );
                assert_eq!(fs::read(&output).expect("export exists"), MEDIA_BYTES);
                assert_eq!(
                    fs::read(&manifest_path).expect("manifest survives"),
                    MANIFEST_BYTES
                );
                assert_eq!(
                    manager
                        .export_task_output(task_id, None, &NoProgress)
                        .expect("export without destination returns the processor output"),
                    muxed_path
                );
            }
        }
    }
}

#[test]
fn skipped_single_file_export_preserves_copy_behavior() {
    let directory = FixtureDirectory::new();
    let (manager, task_id, source_path) = completed_task(
        &directory.0,
        DownloadContentFormat::SingleFile,
        vec![FixtureProcessor::Skip],
        false,
    );
    let output = directory.0.join("export.mp4");
    assert_eq!(
        manager
            .export_task_output(task_id, Some(&output), &NoProgress)
            .expect("single-file media remains exportable after a skip"),
        output
    );
    assert_eq!(fs::read(output).expect("export exists"), MEDIA_BYTES);
    for destination in [None, Some(source_path.as_path())] {
        assert_eq!(
            manager
                .export_task_output(task_id, destination, &NoProgress)
                .expect("the completed single file remains available"),
            source_path
        );
    }
    assert_eq!(fs::read(source_path).expect("source survives"), MEDIA_BYTES);
}
