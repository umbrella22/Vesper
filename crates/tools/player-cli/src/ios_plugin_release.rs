use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::ios::IosError;
use crate::ios_plugin::IosPluginId;

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(
            "staging an iOS plugin release requires macOS",
        ))
    }
}

pub(crate) fn stage(
    root: &Path,
    plugin_id: IosPluginId,
    arguments: Vec<OsString>,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    let stage_ffmpeg_runtime = std::env::var_os("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE").as_deref()
        != Some(OsStr::new("1"));
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::stage(
            root,
            plugin_id,
            arguments,
            stage_ffmpeg_runtime,
            None,
            output,
            diagnostics,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            root,
            plugin_id,
            arguments,
            stage_ffmpeg_runtime,
            output,
            diagnostics,
        );
        unreachable!("the host gate rejects non-macOS iOS plugin releases")
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn stage_for_aggregate(
    root: &Path,
    plugin_id: IosPluginId,
    arguments: Vec<OsString>,
    stage_ffmpeg_runtime: bool,
    guard: &crate::ios_plugin::IosPluginBuildGuard,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;
    implementation::stage(
        root,
        plugin_id,
        arguments,
        stage_ffmpeg_runtime,
        Some(guard),
        output,
        diagnostics,
    )
}

/// Build and publish the standalone iOS FFmpeg component framework release.
///
/// The standalone command shares the same source lock, immutable input
/// snapshot, framework validation, and release metadata as an FFmpeg-backed
/// plugin release. Keeping this entrypoint here prevents the public CLI from
/// delegating release semantics to a shell worker.
pub(crate) fn stage_ffmpeg_runtime(
    root: &Path,
    arguments: Vec<OsString>,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::stage_ffmpeg_runtime(root, arguments, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, arguments, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS FFmpeg runtime releases")
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn canonical_registry_fragment(
    root: &Path,
    plugin_id: IosPluginId,
    slice: crate::ios_plugin::IosPluginSlice,
    minimum_os: &str,
) -> Result<Vec<u8>, IosError> {
    implementation::canonical_registry_fragment(root, plugin_id, slice, minimum_os)
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};

    use player_cli::{
        CanonicalPluginDescriptor, EmbeddedRegistryFragment, EmbeddedRegistryTarget,
        PluginProjectManifest,
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use zip::ZipArchive;

    use super::{IosError, IosPluginId};
    use crate::external_process::{self, BoundedProcessOutput, ExternalProcessErrorKind};
    use crate::ffmpeg_source::FfmpegSourceLock;
    use crate::ios_plugin::{
        self, IosPluginBuildEnvironment, IosPluginBuildRequest, IosPluginSlice,
    };

    const PROJECT_FILE: &str = "lib/ios/VesperPlayerKit/project.yml";
    const EMBEDDED_PLUGIN_REGISTRY_FILE: &str = "vesper-plugin-registry.json";
    const MAX_PROCESS_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    const MAX_PROJECT_FILE_BYTES: u64 = 1024 * 1024;
    const MAX_METADATA_BYTES: u64 = 1024 * 1024;
    const MAX_BINARY_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_XCFRAMEWORK_PLIST_BYTES: usize = 1024 * 1024;
    const MAX_RELEASE_ZIP_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_RELEASE_TREE_ENTRIES: usize = 64;
    const MAX_FFMPEG_SNAPSHOT_ENTRIES: usize = 100_000;
    const MAX_FFMPEG_SNAPSHOT_DEPTH: usize = 64;
    const MAX_FFMPEG_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    const PROMOTION_JOURNAL_VERSION: u32 = 1;
    const PROMOTION_JOURNAL_FILE: &str = "ios-plugin-release-transaction.json";
    const MAX_PROMOTION_JOURNAL_BYTES: u64 = 64 * 1024;
    const PREPARED_OWNER_JOURNAL_VERSION: u32 = 1;
    const PREPARED_OWNER_JOURNAL_FILE: &str = "ios-plugin-release-prepared.json";
    const MAX_PREPARED_OWNER_JOURNAL_BYTES: u64 = 16 * 1024;
    const REQUIRED_PREPARED_OWNERS: usize = 2;
    const REQUIRED_PROMOTION_RECORDS: usize = 2;
    const MAX_FFMPEG_RUNTIME_COMPONENTS: usize = 8;
    const MAX_PROMOTION_RECORDS: usize =
        REQUIRED_PROMOTION_RECORDS + MAX_FFMPEG_RUNTIME_COMPONENTS + 1;
    const MAX_PROMOTION_TREE_ENTRIES: usize = 256;
    const MAX_PROMOTION_TREE_DEPTH: usize = 16;
    const MAX_PROMOTION_TREE_BYTES: u64 = 2 * MAX_BINARY_BYTES;
    const MAX_QUARANTINE_CLEANUP_ENTRIES: usize =
        2 * MAX_FFMPEG_SNAPSHOT_ENTRIES + 16 * MAX_PROMOTION_TREE_ENTRIES;
    const MAX_QUARANTINE_CLEANUP_DEPTH: usize = MAX_FFMPEG_SNAPSHOT_DEPTH + 8;
    const MAX_QUARANTINE_CLEANUP_PASSES: usize = 4;

    #[derive(Debug)]
    struct ReleaseRequest {
        output_directory: PathBuf,
        profile: Option<String>,
        dry_run: bool,
        slices: Vec<IosPluginSlice>,
        ffmpeg: Option<ReleaseFfmpeg>,
        version: String,
        build: String,
        minimum_os: String,
    }

    #[derive(Debug)]
    struct ReleaseFfmpeg {
        declared_profile: String,
        profile_hash: String,
        output_directory: PathBuf,
        runtime_libraries: Vec<String>,
        raw_arguments: Vec<OsString>,
        source_lock: FfmpegSourceLock,
        native_profile: crate::ffmpeg::NativeFfmpegProfile,
    }

    #[derive(Debug)]
    struct RequiredTools {
        xcodebuild: PathBuf,
        install_name_tool: PathBuf,
        otool: PathBuf,
        lipo: PathBuf,
        plutil: PathBuf,
        ditto: PathBuf,
    }

    #[derive(Debug)]
    struct ReleaseOutcome {
        framework_name: &'static str,
        xcframework: PathBuf,
        zip: PathBuf,
        runtime_zips: Vec<PathBuf>,
    }

    #[derive(Debug)]
    enum RuntimeReleaseOutcome {
        Prepared {
            ffmpeg_directory: PathBuf,
        },
        Staged {
            output_directory: PathBuf,
            runtime_directory: PathBuf,
            archives: Vec<PathBuf>,
        },
    }

    #[derive(Debug)]
    struct FfmpegSnapshot {
        output_directory: PathBuf,
        tree_snapshot: PromotionNodeSnapshot,
        resolved_profile: String,
        slices: BTreeMap<IosPluginSlice, FfmpegSnapshotSlice>,
    }

    #[derive(Debug)]
    struct FfmpegSnapshotSlice {
        metadata: Vec<u8>,
        library_checksums: Vec<u8>,
        input_fingerprint: String,
    }

    #[derive(Debug)]
    struct RuntimeArchiveSourceSnapshot {
        path: PathBuf,
        snapshot: PromotionNodeSnapshot,
    }

    #[derive(Clone, Copy, Debug)]
    struct DirectorySnapshotLimits {
        maximum_entries: usize,
        maximum_depth: usize,
        maximum_bytes: u64,
        digest_domain: &'static [u8],
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DirectoryDigestSemantics {
        ImmutableIdentity,
        BuildContent,
    }

    #[derive(Debug)]
    struct BoundedDirectoryDigest {
        identity: FileIdentity,
        payload_bytes: u64,
        sha256: String,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct RawOutputMarker {
        format: String,
        plugin_id: String,
        cargo_profile: String,
        slices: Vec<String>,
        #[serde(default)]
        ios_deployment_target: Option<String>,
        ffmpeg_profile: Option<String>,
        ffmpeg_inputs: Vec<RawOutputFfmpegInput>,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct RawOutputFfmpegInput {
        slice: String,
        input_fingerprint: String,
    }

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct FileIdentity {
        device: u64,
        inode: u64,
    }

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum PreparedOwnerRole {
        Build,
        Assets,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct PreparedOwner {
        role: PreparedOwnerRole,
        path: PathBuf,
        identity: Option<FileIdentity>,
        parent: PathBuf,
        parent_identity: FileIdentity,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct PreparedOwnerJournal {
        version: u32,
        transaction_id: [u8; 16],
        root: PathBuf,
        root_identity: FileIdentity,
        state_directory: PathBuf,
        state_directory_identity: FileIdentity,
        build_parent: PathBuf,
        build_parent_identity: FileIdentity,
        output_directory: PathBuf,
        output_directory_identity: FileIdentity,
        plugin_id: String,
        owners: Vec<PreparedOwner>,
    }

    #[derive(Clone, Copy, Debug)]
    struct PreparedOwnerExpectation {
        journal_identity: FileIdentity,
        transaction_id: [u8; 16],
    }

    #[derive(Debug)]
    struct PreparedOwnersGuard {
        root: PathBuf,
        journal_path: PathBuf,
        journal_identity: FileIdentity,
        journal: PreparedOwnerJournal,
        armed: bool,
    }

    impl PreparedOwnersGuard {
        fn work_path(&self) -> &Path {
            self.owner(PreparedOwnerRole::Build).path.as_path()
        }

        fn assets_path(&self) -> &Path {
            self.owner(PreparedOwnerRole::Assets).path.as_path()
        }

        fn owner(&self, role: PreparedOwnerRole) -> &PreparedOwner {
            for owner in &self.journal.owners {
                if owner.role == role {
                    return owner;
                }
            }
            unreachable!("validated prepared-owner journal omitted a required owner")
        }

        fn expectation(&self) -> PreparedOwnerExpectation {
            PreparedOwnerExpectation {
                journal_identity: self.journal_identity,
                transaction_id: self.journal.transaction_id,
            }
        }

        fn handoff_to_promotion(&mut self) {
            self.armed = false;
        }

        fn create_owner(&mut self, role: PreparedOwnerRole) -> Result<(), IosError> {
            let index = self
                .journal
                .owners
                .iter()
                .position(|owner| owner.role == role)
                .ok_or_else(|| {
                    IosError::worker("prepared iOS plugin release omitted a required owner")
                })?;
            let owner = &self.journal.owners[index];
            if owner.identity.is_some() {
                return Err(IosError::worker(
                    "prepared iOS plugin release owner was created more than once",
                ));
            }
            if directory_identity(&owner.parent, "iOS plugin release staging parent")?
                != owner.parent_identity
            {
                return Err(IosError::storage(format!(
                    "iOS plugin release staging parent '{}' changed before owner creation",
                    owner.parent.display()
                )));
            }
            fs::create_dir(&owner.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin release staging owner '{}': {error}",
                    owner.path.display()
                ))
            })?;
            sync_directory(&owner.path)?;
            sync_directory(&owner.parent)?;
            let identity = directory_identity(&owner.path, "iOS plugin release staging owner")?;
            self.journal.owners[index].identity = Some(identity);
            run_before_prepared_journal_replace_test_hook(role)?;
            self.journal_identity = persist_prepared_owner_journal(
                &self.journal_path,
                &self.journal,
                Some(self.journal_identity),
            )
            .map_err(JournalPersistenceFailure::into_error)?;
            Ok(())
        }
    }

    impl Drop for PreparedOwnersGuard {
        fn drop(&mut self) {
            if self.armed {
                let _ = recover_prepared_owner_journal(
                    &self.root,
                    &self.journal_path,
                    Some(self.expectation()),
                );
            }
        }
    }

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum PromotionDecision {
        Rollback,
        Commit,
    }

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum PromotionNodeKind {
        File,
        Directory,
    }

    #[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct PromotionNodeSnapshot {
        identity: FileIdentity,
        payload_bytes: u64,
        snapshot_sha256: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct PromotionRecord {
        parent: PathBuf,
        parent_identity: FileIdentity,
        target: PathBuf,
        source: PathBuf,
        owner: PathBuf,
        owner_identity: FileIdentity,
        node_kind: PromotionNodeKind,
        old: Option<PromotionNodeSnapshot>,
        new: PromotionNodeSnapshot,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct PromotionJournal {
        version: u32,
        transaction_id: [u8; 16],
        root: PathBuf,
        root_identity: FileIdentity,
        state_directory: PathBuf,
        state_directory_identity: FileIdentity,
        build_parent: PathBuf,
        build_parent_identity: FileIdentity,
        output_directory: PathBuf,
        output_directory_identity: FileIdentity,
        plugin_id: String,
        decision: PromotionDecision,
        records: Vec<PromotionRecord>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PromotionPlacement {
        Before,
        After,
        ExternalReplacement,
        RollbackCleanupPending,
        CommitCleanupPending,
        RolledBackAndCleaned,
        CommittedAndCleaned,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct XcframeworkManifest {
        available_libraries: Vec<XcframeworkLibrary>,
        #[serde(rename = "XCFrameworkFormatVersion")]
        format_version: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct XcframeworkLibrary {
        binary_path: String,
        library_identifier: String,
        library_path: String,
        supported_architectures: Vec<String>,
        supported_platform: String,
        supported_platform_variant: Option<String>,
    }

    pub(super) fn stage(
        root: &Path,
        plugin_id: IosPluginId,
        arguments: Vec<OsString>,
        stage_ffmpeg_runtime: bool,
        held_guard: Option<&ios_plugin::IosPluginBuildGuard>,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        let request = parse_release_request(root, plugin_id, arguments)?;
        if request.dry_run {
            return print_dry_run(plugin_id, &request, output);
        }

        let cancellation = external_process::InterruptDeferral::start("iOS plugin release")
            .map_err(map_external_process_error)?;
        let result = stage_transaction(
            root,
            plugin_id,
            request,
            stage_ffmpeg_runtime,
            held_guard,
            diagnostics,
            &cancellation,
        );
        let cancelled = cancellation.finish();
        match result {
            Ok(_outcome) if cancelled => Err(IosError::worker(format!(
                "iOS {} release was cancelled after its outputs were committed",
                plugin.description
            ))),
            Ok(outcome) => report_release(outcome, output),
            Err(error) if cancelled => Err(IosError::worker(format!(
                "iOS {} release was cancelled; {error}",
                plugin.description
            ))),
            Err(error) => Err(error),
        }
    }

    pub(super) fn stage_ffmpeg_runtime(
        root: &Path,
        arguments: Vec<OsString>,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let request = parse_runtime_release_request(root, arguments)?;
        if request.dry_run {
            return print_runtime_dry_run(&request, output);
        }

        let guard = ios_plugin::acquire_build_guard(root)?;
        ios_plugin::validate_build_guard(root, &guard)?;
        let cancellation = external_process::InterruptDeferral::start("iOS FFmpeg runtime release")
            .map_err(map_external_process_error)?;
        run_after_runtime_guard_test_hook(&cancellation)?;
        let result =
            stage_ffmpeg_runtime_transaction(root, request, &guard, diagnostics, &cancellation);
        let cancelled = cancellation.finish();
        match result {
            Ok(_) if cancelled => Err(IosError::worker(
                "iOS FFmpeg runtime release was cancelled after its outputs were committed",
            )),
            Ok(outcome) => report_runtime_release(outcome, output),
            Err(error) if cancelled => Err(IosError::worker(format!(
                "iOS FFmpeg runtime release was cancelled; {error}"
            ))),
            Err(error) => Err(error),
        }
    }

    fn parse_runtime_release_request(
        root: &Path,
        arguments: Vec<OsString>,
    ) -> Result<ReleaseRequest, IosError> {
        let mut output_directory = root.join("dist/release/ios");
        let mut profile = "default".to_owned();
        let mut dry_run = false;
        let mut slice_values = Vec::<OsString>::new();
        let mut index = 0;
        if let Some(first) = arguments.first()
            && !os_str_starts_with(first, b"--")
            && !is_slice(first)
        {
            output_directory = absolute_output_path(Path::new(first))?;
            index = 1;
        }
        while index < arguments.len() {
            let argument = &arguments[index];
            match argument.to_str() {
                Some("--profile") => {
                    index += 1;
                    let value = arguments
                        .get(index)
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| {
                            IosError::compatibility("--profile requires a UTF-8 value")
                        })?;
                    validate_profile_name(value)?;
                    profile = value.to_owned();
                }
                Some(value) if value.starts_with("--profile=") => {
                    let value = value.trim_start_matches("--profile=");
                    validate_profile_name(value)?;
                    profile = value.to_owned();
                }
                Some("--dry-run") => dry_run = true,
                Some("ios-arm64" | "ios-simulator-arm64") => slice_values.push(argument.clone()),
                Some(value) => {
                    return Err(IosError::compatibility(format!(
                        "unknown iOS FFmpeg runtime release argument: {value}"
                    )));
                }
                None => {
                    return Err(IosError::compatibility(
                        "iOS FFmpeg runtime release options and slices must be valid UTF-8",
                    ));
                }
            }
            index += 1;
        }

        let slices = parse_slices(&slice_values)?;
        validate_release_slices(&slices, "FFmpeg component framework release")?;
        let (version, build) = resolve_release_version(root)?;
        let minimum_os = read_optional_utf8_environment("VESPER_APPLE_IOS_DEPLOYMENT_TARGET")?
            .unwrap_or_else(|| "17.0".to_owned());
        let ffmpeg = Some(resolve_release_ffmpeg(root, &profile, &slices)?);
        Ok(ReleaseRequest {
            output_directory,
            profile: Some(profile),
            dry_run,
            slices,
            ffmpeg,
            version,
            build,
            minimum_os,
        })
    }

    fn print_runtime_dry_run(
        request: &ReleaseRequest,
        output: &mut dyn Write,
    ) -> Result<(), IosError> {
        let ffmpeg = request.ffmpeg.as_ref().ok_or_else(|| {
            IosError::worker("iOS FFmpeg runtime release has no resolved profile")
        })?;
        writeln!(output, "Resolved iOS FFmpeg component framework release:")
            .map_err(output_error)?;
        writeln!(output, "profile={}", ffmpeg.declared_profile).map_err(output_error)?;
        writeln!(output, "profile_hash={}", ffmpeg.profile_hash).map_err(output_error)?;
        writeln!(
            output,
            "ffmpeg_output_directory={}",
            ffmpeg.output_directory.display()
        )
        .map_err(output_error)?;
        writeln!(output, "Selected slices:").map_err(output_error)?;
        for slice in &request.slices {
            writeln!(output, "  {}", slice.as_str()).map_err(output_error)?;
        }
        writeln!(output, "Output zips:").map_err(output_error)?;
        for library in &ffmpeg.runtime_libraries {
            writeln!(
                output,
                "  {}",
                request
                    .output_directory
                    .join(format!(
                        "{}.xcframework.zip",
                        ffmpeg_framework_name(library)?
                    ))
                    .display()
            )
            .map_err(output_error)?;
        }
        output.flush().map_err(output_error)
    }

    fn stage_ffmpeg_runtime_transaction(
        root: &Path,
        request: ReleaseRequest,
        guard: &ios_plugin::IosPluginBuildGuard,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<RuntimeReleaseOutcome, IosError> {
        ios_plugin::validate_build_guard(root, guard)?;
        let ffmpeg = request
            .ffmpeg
            .as_ref()
            .ok_or_else(|| IosError::worker("iOS FFmpeg runtime release has no FFmpeg profile"))?;
        let reuse_inputs = runtime_reuse_inputs_requested()?;
        let prepare_only = runtime_prepare_only_requested()?;
        if reuse_inputs && prepare_only {
            return Err(IosError::compatibility(
                "iOS FFmpeg runtime release cannot prepare and reuse inputs at the same time",
            ));
        }

        // Keep the immutable input snapshot outside the release destination so
        // validation failures cannot leave a partially-created release tree.
        let snapshot_owner = tempfile::Builder::new()
            .prefix(".vesper-ios-ffmpeg-runtime-snapshot-")
            .tempdir()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime snapshot owner: {error}"
                ))
            })?;

        if !reuse_inputs {
            prepare_runtime_inputs(
                root,
                &request,
                ffmpeg,
                &request.output_directory,
                diagnostics,
                cancellation,
            )?;
        }

        let snapshot_directory = snapshot_owner.path().join("ffmpeg-snapshot");
        let snapshot =
            snapshot_ffmpeg_inputs(ffmpeg, &request.slices, &snapshot_directory, cancellation)?;
        if reuse_inputs {
            validate_runtime_input_fingerprint_overrides(&snapshot, &request.slices)?;
        }
        if prepare_only {
            return Ok(RuntimeReleaseOutcome::Prepared {
                ffmpeg_directory: ffmpeg.output_directory.clone(),
            });
        }

        let output_directory = prepare_runtime_output_directory(&request.output_directory)?;
        let runtime_parent_path = root.join("lib/ios/VesperPlayerKit/.build");
        fs::create_dir_all(&runtime_parent_path).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS FFmpeg runtime build parent '{}': {error}",
                runtime_parent_path.display()
            ))
        })?;
        let runtime_parent =
            canonical_directory(&runtime_parent_path, "iOS FFmpeg runtime build parent")?;
        let runtime_target = runtime_parent.join("player-ffmpeg-runtime");
        if paths_overlap(&runtime_target, &output_directory) {
            return Err(IosError::storage(
                "iOS FFmpeg runtime output overlaps the release output directory",
            ));
        }

        let runtime_owner = tempfile::Builder::new()
            .prefix(".vesper-ios-ffmpeg-runtime-")
            .tempdir_in(&runtime_parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime staging owner: {error}"
                ))
            })?;
        let asset_owner = tempfile::Builder::new()
            .prefix(".vesper-ios-ffmpeg-runtime-assets-")
            .tempdir_in(output_directory.parent().ok_or_else(|| {
                IosError::storage("iOS FFmpeg runtime release output has no parent")
            })?)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime asset staging owner: {error}"
                ))
            })?;

        let tools = resolve_required_tools()?;
        let runtime_build = runtime_owner.path().join("runtime-build");
        stage_runtime(
            &request,
            ffmpeg,
            &snapshot,
            &runtime_build,
            asset_owner.path(),
            &tools,
            diagnostics,
            cancellation,
        )?;
        let staged_runtime = runtime_build.join("xcframeworks");
        validate_runtime_directory(&staged_runtime, ffmpeg)?;
        verify_runtime_archives(asset_owner.path(), ffmpeg, &snapshot, &request.slices)?;

        publish_runtime_outputs(
            &staged_runtime,
            &runtime_target,
            asset_owner.path(),
            &output_directory,
            ffmpeg,
            &snapshot,
            &request.slices,
            cancellation,
        )?;
        ios_plugin::validate_build_guard(root, guard)?;
        Ok(RuntimeReleaseOutcome::Staged {
            archives: ffmpeg
                .runtime_libraries
                .iter()
                .map(|library| {
                    ffmpeg_framework_name(library).map(|framework| {
                        output_directory.join(format!("{framework}.xcframework.zip"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            output_directory,
            runtime_directory: runtime_target,
        })
    }

    fn report_runtime_release(
        outcome: RuntimeReleaseOutcome,
        output: &mut dyn Write,
    ) -> Result<(), IosError> {
        match outcome {
            RuntimeReleaseOutcome::Prepared { ffmpeg_directory } => {
                writeln!(output, "Prepared Apple FFmpeg prebuilts into:")
                    .and_then(|()| writeln!(output, "  {}", ffmpeg_directory.display()))
                    .map_err(output_error)?;
            }
            RuntimeReleaseOutcome::Staged {
                output_directory,
                runtime_directory,
                archives,
            } => {
                writeln!(
                    output,
                    "Staged iOS FFmpeg component framework release artifacts:"
                )
                .map_err(output_error)?;
                for archive in archives {
                    writeln!(output, "  {}", archive.display()).map_err(output_error)?;
                }
                writeln!(output, "Canonical XCFrameworks:")
                    .and_then(|()| writeln!(output, "  {}", runtime_directory.display()))
                    .and_then(|()| writeln!(output, "Release directory:"))
                    .and_then(|()| writeln!(output, "  {}", output_directory.display()))
                    .map_err(output_error)?;
            }
        }
        output.flush().map_err(output_error)
    }

    fn prepare_runtime_output_directory(path: &Path) -> Result<PathBuf, IosError> {
        fs::create_dir_all(path).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS FFmpeg runtime release output '{}': {error}",
                path.display()
            ))
        })?;
        canonical_directory(path, "iOS FFmpeg runtime release output")
    }

    fn runtime_reuse_inputs_requested() -> Result<bool, IosError> {
        match env::var("VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS") {
            Ok(value) if value == "1" => Ok(true),
            Ok(value) if value == "0" || value.is_empty() => Ok(false),
            Ok(_) => Err(IosError::compatibility(
                "VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS must be 0 or 1",
            )),
            Err(env::VarError::NotPresent) => Ok(false),
            Err(env::VarError::NotUnicode(_)) => Err(IosError::compatibility(
                "VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS must be valid UTF-8",
            )),
        }
    }

    fn runtime_prepare_only_requested() -> Result<bool, IosError> {
        match env::var("VESPER_IOS_FFMPEG_RUNTIME_PREPARE_ONLY") {
            Ok(value) if value == "1" => Ok(true),
            Ok(value) if value == "0" || value.is_empty() => Ok(false),
            Ok(_) => Err(IosError::compatibility(
                "VESPER_IOS_FFMPEG_RUNTIME_PREPARE_ONLY must be 0 or 1",
            )),
            Err(env::VarError::NotPresent) => Ok(false),
            Err(env::VarError::NotUnicode(_)) => Err(IosError::compatibility(
                "VESPER_IOS_FFMPEG_RUNTIME_PREPARE_ONLY must be valid UTF-8",
            )),
        }
    }

    fn validate_runtime_input_fingerprint_overrides(
        snapshot: &FfmpegSnapshot,
        slices: &[IosPluginSlice],
    ) -> Result<(), IosError> {
        for slice in slices {
            let name = match slice {
                IosPluginSlice::DeviceArm64 => "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_ARM64",
                IosPluginSlice::SimulatorArm64 => {
                    "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_SIMULATOR_ARM64"
                }
            };
            let value = env::var(name).map_err(|error| match error {
                env::VarError::NotPresent => IosError::compatibility(format!(
                    "Missing required FFmpeg input fingerprint override for {}",
                    slice.as_str()
                )),
                env::VarError::NotUnicode(_) => {
                    IosError::compatibility(format!("{name} must be valid UTF-8"))
                }
            })?;
            if !is_input_fingerprint(&value) {
                return Err(IosError::compatibility(format!(
                    "Invalid FFmpeg input fingerprint override for {}",
                    slice.as_str()
                )));
            }
            let expected = snapshot
                .slices
                .get(slice)
                .ok_or_else(|| IosError::worker("FFmpeg snapshot omitted a selected slice"))?;
            if value != expected.input_fingerprint {
                return Err(IosError::conformance(format!(
                    "FFmpeg input fingerprint override does not match the immutable snapshot for {}",
                    slice.as_str()
                )));
            }
        }
        Ok(())
    }

    fn is_input_fingerprint(value: &str) -> bool {
        let mut parts = value.split('-');
        let Some(first) = parts.next() else {
            return false;
        };
        let Some(second) = parts.next() else {
            return false;
        };
        parts.next().is_none()
            && first.len() == 64
            && second.len() == 64
            && first
                .bytes()
                .chain(second.bytes())
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    }

    fn validate_runtime_directory(path: &Path, ffmpeg: &ReleaseFfmpeg) -> Result<(), IosError> {
        let expected = ffmpeg
            .runtime_libraries
            .iter()
            .map(|library| {
                ffmpeg_framework_name(library)
                    .map(|name| OsString::from(format!("{name}.xcframework")))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if read_directory_names(
            path,
            "iOS FFmpeg runtime XCFramework directory",
            MAX_RELEASE_TREE_ENTRIES,
        )? != expected
        {
            return Err(IosError::conformance(format!(
                "iOS FFmpeg runtime XCFramework directory has an unexpected payload: {}",
                path.display()
            )));
        }
        Ok(())
    }

    #[derive(Debug)]
    struct RuntimePublicationRecord {
        source: PathBuf,
        target: PathBuf,
        kind: PromotionNodeKind,
        old_identity: Option<FileIdentity>,
        published_identity: FileIdentity,
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_runtime_outputs(
        staged_runtime: &Path,
        runtime_target: &Path,
        staged_assets: &Path,
        output_directory: &Path,
        ffmpeg: &ReleaseFfmpeg,
        snapshot: &FfmpegSnapshot,
        slices: &[IosPluginSlice],
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut records = Vec::new();
        let result = (|| {
            for library in &ffmpeg.runtime_libraries {
                check_cancellation(cancellation, "iOS FFmpeg runtime release publication")?;
                let framework = ffmpeg_framework_name(library)?;
                let name = format!("{framework}.xcframework.zip");
                let source = staged_assets.join(&name);
                let target = output_directory.join(&name);
                records.push(publish_runtime_node(
                    source,
                    target,
                    PromotionNodeKind::File,
                )?);
            }
            check_cancellation(cancellation, "iOS FFmpeg runtime release publication")?;
            records.push(publish_runtime_node(
                staged_runtime.to_path_buf(),
                runtime_target.to_path_buf(),
                PromotionNodeKind::Directory,
            )?);
            verify_runtime_archives(output_directory, ffmpeg, snapshot, slices)?;
            validate_runtime_directory(runtime_target, ffmpeg)?;
            Ok::<(), IosError>(())
        })();

        match result {
            Ok(()) => {
                for record in &records {
                    if record.old_identity.is_some() {
                        remove_runtime_node(&record.source, record.kind)?;
                    }
                }
                sync_directory(output_directory)?;
                if let Some(parent) = runtime_target.parent() {
                    sync_directory(parent)?;
                }
                Ok(())
            }
            Err(error) => {
                let rollback = rollback_runtime_publication(&records);
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(append_error(error, rollback_error.to_string())),
                }
            }
        }
    }

    fn publish_runtime_node(
        source: PathBuf,
        target: PathBuf,
        kind: PromotionNodeKind,
    ) -> Result<RuntimePublicationRecord, IosError> {
        validate_runtime_node(&source, kind, "staged iOS FFmpeg runtime output")?;
        let old_identity = match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || (kind == PromotionNodeKind::File && !metadata.file_type().is_file())
                    || (kind == PromotionNodeKind::Directory && !metadata.file_type().is_dir())
                {
                    return Err(IosError::conformance(format!(
                        "iOS FFmpeg runtime target has an unexpected type: {}",
                        target.display()
                    )));
                }
                Some(metadata_identity(&metadata))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect iOS FFmpeg runtime target '{}': {error}",
                    target.display()
                )));
            }
        };
        let parent = target.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS FFmpeg runtime target has no parent: {}",
                target.display()
            ))
        })?;
        if old_identity.is_some() {
            exchange_paths(&source, &target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically replace iOS FFmpeg runtime target '{}': {error}",
                    target.display()
                ))
            })?;
        } else {
            rename_noreplace(&source, &target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS FFmpeg runtime target '{}': {error}",
                    target.display()
                ))
            })?;
        }
        sync_directory(parent)?;
        let published_identity =
            runtime_node_identity(&target, kind, "published iOS FFmpeg runtime output")?;
        Ok(RuntimePublicationRecord {
            source,
            target,
            kind,
            old_identity,
            published_identity,
        })
    }

    fn rollback_runtime_publication(records: &[RuntimePublicationRecord]) -> Result<(), IosError> {
        for record in records.iter().rev() {
            let current = runtime_node_identity(
                &record.target,
                record.kind,
                "published iOS FFmpeg runtime output",
            )?;
            if current != record.published_identity {
                return Err(IosError::storage(format!(
                    "iOS FFmpeg runtime target changed before rollback: {}",
                    record.target.display()
                )));
            }
            if let Some(old_identity) = record.old_identity {
                if runtime_node_identity(
                    &record.source,
                    record.kind,
                    "previous iOS FFmpeg runtime output",
                )? != old_identity
                {
                    return Err(IosError::storage(format!(
                        "previous iOS FFmpeg runtime output changed before rollback: {}",
                        record.source.display()
                    )));
                }
                exchange_paths(&record.source, &record.target).map_err(|error| {
                    IosError::storage(format!(
                        "failed to restore iOS FFmpeg runtime target '{}': {error}",
                        record.target.display()
                    ))
                })?;
            } else {
                rename_noreplace(&record.target, &record.source).map_err(|error| {
                    IosError::storage(format!(
                        "failed to remove newly published iOS FFmpeg runtime target '{}': {error}",
                        record.target.display()
                    ))
                })?;
            }
            if let Some(parent) = record.target.parent() {
                sync_directory(parent)?;
            }
        }
        Ok(())
    }

    fn validate_runtime_node(
        path: &Path,
        kind: PromotionNodeKind,
        label: &str,
    ) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if metadata.file_type().is_symlink()
            || (kind == PromotionNodeKind::File && !metadata.file_type().is_file())
            || (kind == PromotionNodeKind::Directory && !metadata.file_type().is_dir())
        {
            return Err(IosError::conformance(format!(
                "{label} '{}' has an unexpected type",
                path.display()
            )));
        }
        Ok(())
    }

    fn runtime_node_identity(
        path: &Path,
        kind: PromotionNodeKind,
        label: &str,
    ) -> Result<FileIdentity, IosError> {
        validate_runtime_node(path, kind, label)?;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        Ok(metadata_identity(&metadata))
    }

    fn remove_runtime_node(path: &Path, kind: PromotionNodeKind) -> Result<(), IosError> {
        validate_runtime_node(path, kind, "previous iOS FFmpeg runtime output")?;
        match kind {
            PromotionNodeKind::File => fs::remove_file(path),
            PromotionNodeKind::Directory => fs::remove_dir_all(path),
        }
        .map_err(|error| {
            IosError::storage(format!(
                "failed to remove previous iOS FFmpeg runtime output '{}': {error}",
                path.display()
            ))
        })
    }

    fn parse_release_request(
        root: &Path,
        plugin_id: IosPluginId,
        arguments: Vec<OsString>,
    ) -> Result<ReleaseRequest, IosError> {
        let plugin = plugin_id.spec();
        let mut output_directory = root.join("dist/release/ios");
        let mut profile = plugin.release_ffmpeg_profile.map(str::to_owned);
        let mut dry_run = false;
        let mut slice_values = Vec::<OsString>::new();
        let mut index = 0;
        if let Some(first) = arguments.first()
            && !os_str_starts_with(first, b"--")
            && !is_slice(first)
        {
            output_directory = absolute_output_path(Path::new(first))?;
            index = 1;
        }
        while index < arguments.len() {
            let argument = &arguments[index];
            match argument.to_str() {
                Some("--profile") => {
                    if !plugin.uses_ffmpeg {
                        return Err(IosError::compatibility(
                            "--profile is only supported for FFmpeg-backed iOS plugins",
                        ));
                    }
                    index += 1;
                    let value = arguments
                        .get(index)
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| {
                            IosError::compatibility("--profile requires a UTF-8 value")
                        })?;
                    validate_profile_name(value)?;
                    profile = Some(value.to_owned());
                }
                Some(value) if value.starts_with("--profile=") => {
                    if !plugin.uses_ffmpeg {
                        return Err(IosError::compatibility(
                            "--profile is only supported for FFmpeg-backed iOS plugins",
                        ));
                    }
                    let value = value.trim_start_matches("--profile=");
                    validate_profile_name(value)?;
                    profile = Some(value.to_owned());
                }
                Some("--dry-run") => dry_run = true,
                Some("ios-arm64" | "ios-simulator-arm64") => slice_values.push(argument.clone()),
                Some(value) => {
                    return Err(IosError::compatibility(format!(
                        "unknown iOS {} release argument: {value}",
                        plugin.description
                    )));
                }
                None => {
                    return Err(IosError::compatibility(
                        "iOS plugin release options and slices must be valid UTF-8",
                    ));
                }
            }
            index += 1;
        }

        let slices = parse_slices(&slice_values)?;
        validate_release_slices(&slices, plugin.description)?;
        let (version, build) = resolve_release_version(root)?;
        let minimum_os = read_optional_utf8_environment("VESPER_APPLE_IOS_DEPLOYMENT_TARGET")?
            .unwrap_or_else(|| "17.0".to_owned());
        let ffmpeg = match profile.as_deref() {
            Some(profile) => Some(resolve_release_ffmpeg(root, profile, &slices)?),
            None => None,
        };
        Ok(ReleaseRequest {
            output_directory,
            profile,
            dry_run,
            slices,
            ffmpeg,
            version,
            build,
            minimum_os,
        })
    }

    fn absolute_output_path(path: &Path) -> Result<PathBuf, IosError> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            env::current_dir()
                .map(|directory| directory.join(path))
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to resolve iOS plugin release output directory: {error}"
                    ))
                })
        }
    }

    fn os_str_starts_with(value: &OsStr, prefix: &[u8]) -> bool {
        use std::os::unix::ffi::OsStrExt;

        value.as_bytes().starts_with(prefix)
    }

    fn is_slice(value: &OsStr) -> bool {
        matches!(value.to_str(), Some("ios-arm64" | "ios-simulator-arm64"))
    }

    fn validate_profile_name(value: &str) -> Result<(), IosError> {
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err(IosError::compatibility(
                "the iOS plugin release profile must contain 1 to 128 non-control characters",
            ));
        }
        Ok(())
    }

    fn parse_slices(values: &[OsString]) -> Result<Vec<IosPluginSlice>, IosError> {
        let slices = if values.is_empty() {
            vec![IosPluginSlice::DeviceArm64, IosPluginSlice::SimulatorArm64]
        } else {
            values
                .iter()
                .map(|value| match value.to_str() {
                    Some("ios-arm64") => Ok(IosPluginSlice::DeviceArm64),
                    Some("ios-simulator-arm64") => Ok(IosPluginSlice::SimulatorArm64),
                    Some(value) => Err(IosError::compatibility(format!(
                        "unsupported Apple slice: {value}"
                    ))),
                    None => Err(IosError::compatibility(
                        "Apple slice names must be valid UTF-8",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let unique = slices.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != slices.len() {
            return Err(IosError::compatibility(
                "iOS plugin release slices must not be repeated",
            ));
        }
        Ok(slices)
    }

    fn validate_release_slices(
        slices: &[IosPluginSlice],
        plugin_description: &str,
    ) -> Result<(), IosError> {
        if slices.contains(&IosPluginSlice::DeviceArm64)
            && slices.contains(&IosPluginSlice::SimulatorArm64)
        {
            return Ok(());
        }
        Err(IosError::compatibility(format!(
            "iOS {plugin_description} release requires both ios-arm64 and ios-simulator-arm64 slices"
        )))
    }

    fn resolve_release_version(root: &Path) -> Result<(String, String), IosError> {
        let project = require_repository_file(root, PROJECT_FILE, "iOS project metadata")?;
        let text = read_bounded_utf8(&project, MAX_PROJECT_FILE_BYTES, "iOS project metadata")?;
        let project_version = capture_project_value(&text, "CFBundleShortVersionString")?;
        let project_build = capture_project_value(&text, "CFBundleVersion")?;
        let version =
            read_optional_utf8_environment("VESPER_RELEASE_VERSION")?.unwrap_or(project_version);
        let build = read_optional_utf8_environment("VESPER_RELEASE_BUILD")?
            .or(read_optional_utf8_environment("VESPER_RELEASE_IOS_BUILD")?)
            .unwrap_or(project_build);
        if version.is_empty()
            || version.len() > 128
            || version.chars().any(char::is_control)
            || build.is_empty()
            || build.len() > 32
            || !build.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(IosError::compatibility(
                "iOS plugin release version metadata is invalid",
            ));
        }
        Ok((version, build))
    }

    fn capture_project_value(text: &str, key: &str) -> Result<String, IosError> {
        let prefix = format!("{key}: \"");
        let values = text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                line.strip_prefix(&prefix)
                    .and_then(|value| value.strip_suffix('"'))
            })
            .collect::<Vec<_>>();
        match values.as_slice() {
            [value] if !value.is_empty() => Ok((*value).to_owned()),
            _ => Err(IosError::conformance(format!(
                "iOS project metadata must declare exactly one {key} value"
            ))),
        }
    }

    fn read_optional_utf8_environment(name: &str) -> Result<Option<String>, IosError> {
        match env::var(name) {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(env::VarError::NotUnicode(_)) => Err(IosError::compatibility(format!(
                "{name} must be valid UTF-8"
            ))),
        }
    }

    fn resolve_release_ffmpeg(
        root: &Path,
        profile: &str,
        slices: &[IosPluginSlice],
    ) -> Result<ReleaseFfmpeg, IosError> {
        let source_lock = crate::ios_release::canonical_ffmpeg_release_source_lock(root)?;
        let slice_names = slices
            .iter()
            .map(|slice| slice.as_str().to_owned())
            .collect::<Vec<_>>();
        let resolved = crate::ffmpeg::resolve_apple_release_profile(root, profile, &slice_names)
            .map_err(map_ffmpeg_error)?;
        let unique_runtime_libraries = resolved.runtime_libraries.iter().collect::<BTreeSet<_>>();
        if resolved.runtime_libraries.is_empty()
            || resolved.runtime_libraries.len() > MAX_FFMPEG_RUNTIME_COMPONENTS
            || unique_runtime_libraries.len() != resolved.runtime_libraries.len()
        {
            return Err(IosError::conformance(
                "The resolved iOS FFmpeg profile must contain a bounded unique runtime library set.",
            ));
        }
        Ok(ReleaseFfmpeg {
            declared_profile: resolved.declared_profile,
            profile_hash: resolved.profile_hash,
            output_directory: resolved.output_directory,
            runtime_libraries: resolved.runtime_libraries,
            raw_arguments: resolved
                .worker_arguments
                .into_iter()
                .map(OsString::from)
                .collect(),
            source_lock,
            native_profile: resolved.native_profile,
        })
    }

    fn map_ffmpeg_error(error: crate::ffmpeg::FfmpegError) -> IosError {
        match error.kind() {
            crate::ffmpeg::FfmpegErrorKind::Storage => IosError::storage(error.to_string()),
            crate::ffmpeg::FfmpegErrorKind::Compatibility => {
                IosError::compatibility(error.to_string())
            }
            crate::ffmpeg::FfmpegErrorKind::Conformance => IosError::conformance(error.to_string()),
            crate::ffmpeg::FfmpegErrorKind::Worker => IosError::worker(error.to_string()),
        }
    }
    fn print_dry_run(
        plugin_id: IosPluginId,
        request: &ReleaseRequest,
        output: &mut dyn Write,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        writeln!(output, "Resolved iOS {} release:", plugin.description).map_err(output_error)?;
        if let Some(ffmpeg) = &request.ffmpeg {
            writeln!(output, "profile={}", ffmpeg.declared_profile).map_err(output_error)?;
            writeln!(output, "profile_hash={}", ffmpeg.profile_hash).map_err(output_error)?;
            writeln!(
                output,
                "ffmpeg_output_directory={}",
                ffmpeg.output_directory.display()
            )
            .map_err(output_error)?;
        }
        writeln!(output, "Selected slices:").map_err(output_error)?;
        for slice in &request.slices {
            writeln!(output, "  {}", slice.as_str()).map_err(output_error)?;
        }
        if let Some(ffmpeg) = &request.ffmpeg {
            writeln!(output, "Build arguments:").map_err(output_error)?;
            for argument in &ffmpeg.raw_arguments {
                writeln!(output, "  {}", argument.to_string_lossy()).map_err(output_error)?;
            }
            for slice in &request.slices {
                writeln!(output, "  {}", slice.as_str()).map_err(output_error)?;
            }
            writeln!(output, "Required runtime zips:").map_err(output_error)?;
            for library in &ffmpeg.runtime_libraries {
                writeln!(
                    output,
                    "  {}",
                    request
                        .output_directory
                        .join(format!(
                            "{}.xcframework.zip",
                            ffmpeg_framework_name(library)?
                        ))
                        .display()
                )
                .map_err(output_error)?;
            }
        }
        writeln!(output, "Output zip:").map_err(output_error)?;
        writeln!(
            output,
            "  {}",
            request
                .output_directory
                .join(format!("{}.xcframework.zip", plugin.framework_name))
                .display()
        )
        .map_err(output_error)?;
        output.flush().map_err(output_error)
    }

    fn prepare_release_owners(
        root: &Path,
        plugin_id: IosPluginId,
        state_directory: &Path,
        build_parent: &Path,
        output_directory: &Path,
        journal_path: &Path,
    ) -> Result<PreparedOwnersGuard, IosError> {
        let canonical_root = canonical_directory(root, "iOS plugin release repository")?;
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id).map_err(|error| {
            IosError::storage(format!(
                "failed to obtain system randomness for iOS plugin release transaction: {error}"
            ))
        })?;
        let suffix = encode_transaction_id(transaction_id);
        let build_owner = build_parent.join(format!(".vesper-ios-plugin-release-{suffix}"));
        let asset_owner = output_directory.join(format!(".vesper-ios-plugin-assets-{suffix}"));
        for owner in [&build_owner, &asset_owner] {
            match fs::symlink_metadata(owner) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(IosError::storage(format!(
                        "iOS plugin release staging owner already exists: {}",
                        owner.display()
                    )));
                }
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to inspect iOS plugin release staging owner '{}': {error}",
                        owner.display()
                    )));
                }
            }
        }

        let build_parent_identity =
            directory_identity(build_parent, "iOS plugin release build parent")?;
        let output_directory_identity =
            directory_identity(output_directory, "iOS plugin release output directory")?;
        let journal = PreparedOwnerJournal {
            version: PREPARED_OWNER_JOURNAL_VERSION,
            transaction_id,
            root: canonical_root.clone(),
            root_identity: directory_identity(&canonical_root, "iOS plugin release repository")?,
            state_directory: state_directory.to_path_buf(),
            state_directory_identity: directory_identity(
                state_directory,
                "iOS plugin release state directory",
            )?,
            build_parent: build_parent.to_path_buf(),
            build_parent_identity,
            output_directory: output_directory.to_path_buf(),
            output_directory_identity,
            plugin_id: plugin_id.as_str().to_owned(),
            owners: vec![
                PreparedOwner {
                    role: PreparedOwnerRole::Build,
                    path: build_owner,
                    identity: None,
                    parent: build_parent.to_path_buf(),
                    parent_identity: build_parent_identity,
                },
                PreparedOwner {
                    role: PreparedOwnerRole::Assets,
                    path: asset_owner,
                    identity: None,
                    parent: output_directory.to_path_buf(),
                    parent_identity: output_directory_identity,
                },
            ],
        };
        validate_prepared_owner_journal(root, &journal)?;
        let journal_identity = match persist_prepared_owner_journal(journal_path, &journal, None) {
            Ok(identity) => identity,
            Err(failure) if !failure.publication_may_be_visible() => {
                return Err(failure.into_error());
            }
            Err(failure) => {
                let error = failure.into_error();
                return match recover_prepared_owner_journal(
                    root,
                    journal_path,
                    Some(PreparedOwnerExpectation {
                        journal_identity: file_identity_if_regular(journal_path)?.ok_or_else(
                            || {
                                IosError::storage(
                                    "published iOS plugin prepared-owner journal disappeared",
                                )
                            },
                        )?,
                        transaction_id,
                    }),
                ) {
                    Ok(()) => Err(error),
                    Err(recovery) => Err(append_error(error, recovery.to_string())),
                };
            }
        };
        let mut guard = PreparedOwnersGuard {
            root: canonical_root,
            journal_path: journal_path.to_path_buf(),
            journal_identity,
            journal,
            armed: true,
        };
        guard.create_owner(PreparedOwnerRole::Build)?;
        guard.create_owner(PreparedOwnerRole::Assets)?;
        validate_prepared_owner_journal(root, &guard.journal)?;
        Ok(guard)
    }

    fn encode_transaction_id(transaction_id: [u8; 16]) -> String {
        let mut encoded = String::with_capacity(transaction_id.len() * 2);
        for byte in transaction_id {
            encoded.push_str(&format!("{byte:02x}"));
        }
        encoded
    }

    fn stage_transaction(
        root: &Path,
        plugin_id: IosPluginId,
        mut request: ReleaseRequest,
        stage_ffmpeg_runtime: bool,
        held_guard: Option<&ios_plugin::IosPluginBuildGuard>,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<ReleaseOutcome, IosError> {
        let plugin = plugin_id.spec();
        let plugin_descriptor = read_ios_plugin_descriptor(root, plugin_id)?;
        let owned_guard = held_guard
            .is_none()
            .then(|| ios_plugin::acquire_build_guard(root))
            .transpose()?;
        let guard = held_guard.or(owned_guard.as_ref()).ok_or_else(|| {
            IosError::worker("iOS plugin release transaction is missing its repository lock")
        })?;
        ios_plugin::validate_build_guard(root, guard)?;
        check_cancellation(cancellation, "iOS plugin release")?;

        let requested_build_parent = root.join("lib/ios/VesperPlayerKit/.build");
        fs::create_dir_all(&requested_build_parent).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS plugin build parent '{}': {error}",
                requested_build_parent.display()
            ))
        })?;
        let build_parent =
            canonical_directory(&requested_build_parent, "iOS plugin release build parent")?;
        let state_directory = build_parent.join("vesper-cli-state");
        fs::create_dir_all(&state_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS plugin release state directory '{}': {error}",
                state_directory.display()
            ))
        })?;
        let state_directory =
            canonical_directory(&state_directory, "iOS plugin release state directory")?;
        let journal_path = state_directory.join(PROMOTION_JOURNAL_FILE);
        let prepared_journal_path = state_directory.join(PREPARED_OWNER_JOURNAL_FILE);
        recover_promotion_journal(root, &journal_path)?;
        recover_prepared_owner_journal(root, &prepared_journal_path, None)?;
        check_cancellation(cancellation, "iOS plugin release")?;

        fs::create_dir_all(&request.output_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS plugin release output '{}': {error}",
                request.output_directory.display()
            ))
        })?;
        request.output_directory = canonical_directory(
            &request.output_directory,
            "iOS plugin release output directory",
        )?;
        let canonical_plugin_target = build_parent
            .join(plugin.build_directory)
            .join(format!("{}.xcframework", plugin.framework_name));
        if request.output_directory == canonical_plugin_target
            || request
                .output_directory
                .starts_with(&canonical_plugin_target)
            || request.output_directory == state_directory
            || request.output_directory.starts_with(&state_directory)
        {
            return Err(IosError::storage(
                "iOS plugin release output directory overlaps a managed build or transaction path",
            ));
        }
        let tools = resolve_required_tools()?;
        let prepared = prepare_release_owners(
            root,
            plugin_id,
            &state_directory,
            &build_parent,
            &request.output_directory,
            &prepared_journal_path,
        )?;
        run_after_prepared_owner_test_hook(cancellation)?;
        let runtime_staged = request.ffmpeg.is_some() && stage_ffmpeg_runtime;
        let reuse_runtime_inputs = request.ffmpeg.is_some() && runtime_reuse_inputs_requested()?;
        if let Some(ffmpeg) = &request.ffmpeg
            && runtime_staged
            && !reuse_runtime_inputs
        {
            prepare_runtime_inputs(
                root,
                &request,
                ffmpeg,
                prepared.assets_path(),
                diagnostics,
                cancellation,
            )?;
        }

        let ffmpeg_snapshot = request
            .ffmpeg
            .as_ref()
            .map(|ffmpeg| {
                snapshot_ffmpeg_inputs(
                    ffmpeg,
                    &request.slices,
                    &prepared.work_path().join("ffmpeg-snapshot"),
                    cancellation,
                )
            })
            .transpose()?;
        if reuse_runtime_inputs {
            let snapshot = ffmpeg_snapshot.as_ref().ok_or_else(|| {
                IosError::worker("FFmpeg runtime input reuse has no immutable snapshot")
            })?;
            validate_runtime_input_fingerprint_overrides(snapshot, &request.slices)?;
        }
        let mut runtime_source_snapshots = Vec::new();
        let mut runtime_promotion_source = None::<PathBuf>;
        if let (Some(ffmpeg), Some(snapshot)) = (&request.ffmpeg, &ffmpeg_snapshot) {
            let runtime_archive_directory = if runtime_staged {
                let runtime_build = prepared.work_path().join("ffmpeg-runtime");
                stage_runtime(
                    &request,
                    ffmpeg,
                    snapshot,
                    &runtime_build,
                    prepared.assets_path(),
                    &tools,
                    diagnostics,
                    cancellation,
                )?;
                let staged_runtime = runtime_build.join("xcframeworks");
                let promotion_source = prepared.work_path().join("player-ffmpeg-runtime");
                fs::rename(&staged_runtime, &promotion_source).map_err(|error| {
                    IosError::storage(format!(
                        "failed to move staged iOS FFmpeg runtime into its promotion owner: {error}"
                    ))
                })?;
                sync_directory(prepared.work_path())?;
                runtime_promotion_source = Some(promotion_source);
                prepared.assets_path().to_path_buf()
            } else {
                let (directory, sources) = snapshot_runtime_archives(
                    &request.output_directory,
                    ffmpeg,
                    prepared.assets_path(),
                    cancellation,
                )?;
                runtime_source_snapshots = sources;
                directory
            };
            verify_runtime_archives(
                &runtime_archive_directory,
                ffmpeg,
                snapshot,
                &request.slices,
            )?;
        }

        let raw = prepared.work_path().join("raw");
        let mut raw_arguments = vec![OsString::from("release")];
        if let Some(ffmpeg) = &request.ffmpeg {
            raw_arguments.extend(ffmpeg.raw_arguments.iter().cloned());
        }
        raw_arguments.extend(
            request
                .slices
                .iter()
                .map(|slice| OsString::from(slice.as_str())),
        );
        ios_plugin::build_in_deferral_holding_lock(
            root,
            IosPluginBuildRequest {
                plugin_id,
                output_directory: raw.clone(),
                arguments: raw_arguments,
                environment: IosPluginBuildEnvironment {
                    ios_deployment_target: Some(request.minimum_os.clone()),
                    declared_ffmpeg_profile: request.profile.clone(),
                    declared_ffmpeg_platform: request.ffmpeg.as_ref().map(|_| "ios".to_owned()),
                    ffmpeg_output_directory: ffmpeg_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.output_directory.clone()),
                    ffmpeg_input_fingerprints: ffmpeg_snapshot
                        .as_ref()
                        .map(|snapshot| {
                            snapshot
                                .slices
                                .iter()
                                .map(|(slice, snapshot)| {
                                    (*slice, snapshot.input_fingerprint.clone())
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    skip_ffmpeg_prebuilds: request.ffmpeg.as_ref().map(|_| true),
                    ffmpeg_overlays_resolved: request.ffmpeg.is_some(),
                },
            },
            &mut io::sink(),
            diagnostics,
            cancellation,
            guard,
        )?;
        run_after_raw_build_test_hook(cancellation)?;
        if let Some(snapshot) = &ffmpeg_snapshot {
            verify_raw_snapshot_marker(
                &raw,
                plugin_id,
                &request.slices,
                &request.minimum_os,
                snapshot,
            )?;
        }

        let frameworks = prepared.work_path().join("frameworks");
        let xcframework = prepared
            .work_path()
            .join(format!("{}.xcframework", plugin.framework_name));
        build_frameworks(
            root,
            plugin_id,
            &plugin_descriptor,
            &request,
            &raw,
            &frameworks,
            &xcframework,
            ffmpeg_snapshot.as_ref(),
            &tools,
            diagnostics,
            cancellation,
        )?;
        validate_xcframework(
            plugin_id,
            &plugin_descriptor,
            &request,
            &xcframework,
            ffmpeg_snapshot.as_ref(),
            &tools,
            diagnostics,
            cancellation,
        )?;

        let staged_zip = prepared
            .assets_path()
            .join(format!("{}.xcframework.zip", plugin.framework_name));
        create_zip(&xcframework, &staged_zip, &tools, diagnostics, cancellation)?;
        crate::ios_optional_release::preflight_plugin_framework_archive(
            &staged_zip,
            plugin.framework_name,
            plugin.uses_ffmpeg,
            &request.slices,
        )?;
        validate_plugin_archive_registry_fragments(
            &staged_zip,
            plugin_id,
            &plugin_descriptor,
            &request,
        )?;

        let canonical_xcframework = build_parent
            .join(plugin.build_directory)
            .join(format!("{}.xcframework", plugin.framework_name));
        let final_zip = request
            .output_directory
            .join(format!("{}.xcframework.zip", plugin.framework_name));
        let runtime_zips = request
            .ffmpeg
            .as_ref()
            .map(|ffmpeg| {
                ffmpeg
                    .runtime_libraries
                    .iter()
                    .map(|library| {
                        ffmpeg_framework_name(library).map(|framework| {
                            request
                                .output_directory
                                .join(format!("{framework}.xcframework.zip"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        if let Some(snapshot) = &ffmpeg_snapshot {
            verify_ffmpeg_snapshot(snapshot, cancellation)?;
        }
        verify_runtime_archive_sources(&runtime_source_snapshots, cancellation)?;
        promote_release_outputs(
            root,
            plugin_id,
            prepared,
            &xcframework,
            &canonical_xcframework,
            &request.output_directory,
            plugin.framework_name,
            &runtime_zips,
            runtime_promotion_source.as_deref(),
            &runtime_source_snapshots,
            cancellation,
            &journal_path,
        )?;

        Ok(ReleaseOutcome {
            framework_name: plugin.framework_name,
            xcframework: canonical_xcframework,
            zip: final_zip,
            runtime_zips,
        })
    }

    fn report_release(outcome: ReleaseOutcome, output: &mut dyn Write) -> Result<(), IosError> {
        writeln!(output, "Staged optional iOS plugin release artifact:").map_err(output_error)?;
        writeln!(output, "  {}", outcome.zip.display()).map_err(output_error)?;
        writeln!(output, "Canonical XCFramework:").map_err(output_error)?;
        writeln!(output, "  {}", outcome.xcframework.display()).map_err(output_error)?;
        if !outcome.runtime_zips.is_empty() {
            writeln!(
                output,
                "Requires matching top-level FFmpeg component frameworks:"
            )
            .map_err(output_error)?;
            for path in outcome.runtime_zips {
                writeln!(output, "  {}", path.display()).map_err(output_error)?;
            }
        }
        let _ = outcome.framework_name;
        output.flush().map_err(output_error)
    }

    fn prepare_runtime_inputs(
        root: &Path,
        request: &ReleaseRequest,
        ffmpeg: &ReleaseFfmpeg,
        _output_directory: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let slices = request
            .slices
            .iter()
            .map(|slice| slice.as_str().to_owned())
            .collect::<Vec<_>>();
        let source = ffmpeg.source_lock.locked_build_source();
        crate::ffmpeg_apple::run_holding_repository_lock_with_canonical_source(
            root,
            &ffmpeg.output_directory,
            &slices,
            &request.minimum_os,
            &ffmpeg.native_profile,
            &source,
            diagnostics,
            cancellation,
        )
        .map_err(map_ffmpeg_error)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "runtime staging coordinates independent release inputs and keeps cancellation and diagnostics explicit"
    )]
    fn stage_runtime(
        request: &ReleaseRequest,
        ffmpeg: &ReleaseFfmpeg,
        snapshot: &FfmpegSnapshot,
        build_directory: &Path,
        output_directory: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        verify_ffmpeg_snapshot(snapshot, cancellation)?;
        fs::create_dir(build_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS FFmpeg runtime build directory '{}': {error}",
                build_directory.display()
            ))
        })?;
        let framework_staging = build_directory.join("frameworks");
        let xcframework_staging = build_directory.join("xcframeworks");
        for directory in [&framework_staging, &xcframework_staging] {
            fs::create_dir(directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime staging '{}': {error}",
                    directory.display()
                ))
            })?;
        }

        for slice in &request.slices {
            check_cancellation(cancellation, "iOS FFmpeg runtime framework construction")?;
            let snapshot_slice = snapshot.slices.get(slice).ok_or_else(|| {
                IosError::worker("FFmpeg runtime snapshot omitted a selected slice")
            })?;
            let slice_staging = framework_staging.join(slice.as_str());
            fs::create_dir(&slice_staging).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime slice staging '{}': {error}",
                    slice_staging.display()
                ))
            })?;
            for library in &ffmpeg.runtime_libraries {
                build_runtime_framework(
                    request,
                    ffmpeg,
                    snapshot,
                    snapshot_slice,
                    *slice,
                    library,
                    &slice_staging,
                    tools,
                    diagnostics,
                    cancellation,
                )?;
            }
            for library in &ffmpeg.runtime_libraries {
                let framework_name = ffmpeg_framework_name(library)?;
                verify_runtime_sibling_dependencies(
                    &slice_staging
                        .join(format!("{framework_name}.framework"))
                        .join(framework_name),
                    &slice_staging,
                    ffmpeg,
                    tools,
                    diagnostics,
                    cancellation,
                )?;
            }
        }

        for library in &ffmpeg.runtime_libraries {
            check_cancellation(cancellation, "iOS FFmpeg runtime XCFramework construction")?;
            let framework_name = ffmpeg_framework_name(library)?;
            let xcframework = xcframework_staging.join(format!("{framework_name}.xcframework"));
            let mut command = Command::new(&tools.xcodebuild);
            command.arg("-create-xcframework");
            for slice in &request.slices {
                let framework = framework_staging
                    .join(slice.as_str())
                    .join(format!("{framework_name}.framework"));
                verify_exact_arm64(
                    &framework.join(framework_name),
                    tools,
                    diagnostics,
                    cancellation,
                )?;
                command.arg("-framework").arg(framework);
            }
            command.arg("-output").arg(&xcframework);
            run_required_command(
                &mut command,
                "iOS FFmpeg runtime XCFramework construction",
                diagnostics,
                cancellation,
            )?;
            create_zip(
                &xcframework,
                &output_directory.join(format!("{framework_name}.xcframework.zip")),
                tools,
                diagnostics,
                cancellation,
            )?;
        }
        verify_ffmpeg_snapshot(snapshot, cancellation)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_runtime_framework(
        request: &ReleaseRequest,
        ffmpeg: &ReleaseFfmpeg,
        snapshot: &FfmpegSnapshot,
        snapshot_slice: &FfmpegSnapshotSlice,
        slice: IosPluginSlice,
        library: &str,
        slice_staging: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let framework_name = ffmpeg_framework_name(library)?;
        let framework = slice_staging.join(format!("{framework_name}.framework"));
        fs::create_dir(&framework).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS FFmpeg runtime framework '{}': {error}",
                framework.display()
            ))
        })?;
        for directory in [framework.join("Headers"), framework.join("Modules")] {
            fs::create_dir(&directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFmpeg runtime framework directory '{}': {error}",
                    directory.display()
                ))
            })?;
        }

        let source_root = snapshot
            .output_directory
            .join(ffmpeg_slice_directory(slice));
        let checksum_path = source_root.join("vesper-ffmpeg-library-sha256.txt");
        if read_bounded_bytes(
            &checksum_path,
            MAX_METADATA_BYTES,
            "FFmpeg snapshot library checksums",
        )? != snapshot_slice.library_checksums
        {
            return Err(IosError::storage(
                "FFmpeg snapshot checksums changed before runtime framework construction",
            ));
        }
        let checksums =
            parse_snapshot_library_checksums(&snapshot_slice.library_checksums, &checksum_path)?;
        let checksum_key = format!("{library}_sha256");
        let expected_source_checksum = checksums.get(&checksum_key).ok_or_else(|| {
            IosError::conformance(format!(
                "FFmpeg runtime snapshot omits the {library} library checksum"
            ))
        })?;
        let source_binary = source_root
            .join("lib/arm64")
            .join(format!("lib{library}.dylib"));
        if &sha256_file(&source_binary, MAX_BINARY_BYTES)? != expected_source_checksum {
            return Err(IosError::conformance(format!(
                "FFmpeg runtime source checksum mismatch: {}",
                source_binary.display()
            )));
        }
        let binary = framework.join(framework_name);
        copy_regular_file(&source_binary, &binary, MAX_BINARY_BYTES, cancellation)?;
        prepare_dynamic_framework_binary(
            framework_name,
            &ffmpeg.runtime_libraries,
            &binary,
            "iOS FFmpeg runtime framework",
            tools,
            diagnostics,
            cancellation,
        )?;
        write_text_file(
            &framework.join("binary-sha256.txt"),
            &format!("{}\n", sha256_file(&binary, MAX_BINARY_BYTES)?),
        )?;

        let metadata_source = source_root.join("vesper-ffmpeg-build-metadata.txt");
        let metadata_destination = framework.join(metadata_file_name(slice));
        copy_regular_file(
            &metadata_source,
            &metadata_destination,
            MAX_METADATA_BYTES,
            cancellation,
        )?;
        if read_bounded_bytes(
            &metadata_destination,
            MAX_METADATA_BYTES,
            "iOS FFmpeg runtime metadata",
        )? != snapshot_slice.metadata
        {
            return Err(IosError::storage(
                "FFmpeg snapshot metadata changed during runtime framework construction",
            ));
        }
        write_text_file(
            &framework.join("profile-hash.txt"),
            &format!("{}\n", ffmpeg.profile_hash),
        )?;
        write_text_file(
            &framework.join("input-fingerprint.txt"),
            &format!("{}\n", snapshot_slice.input_fingerprint),
        )?;
        write_framework_module(&framework, framework_name)?;
        write_framework_plist(
            &framework.join("Info.plist"),
            framework_name,
            ffmpeg_bundle_identifier(library)?,
            platform_name(slice),
            &request.version,
            &request.build,
            &request.minimum_os,
            tools,
            diagnostics,
            cancellation,
        )?;
        validate_runtime_framework(&framework, framework_name, ffmpeg, snapshot_slice, slice)
    }

    #[derive(Debug, Default)]
    struct SnapshotBudget {
        entries: usize,
        bytes: u64,
    }

    fn snapshot_ffmpeg_inputs(
        ffmpeg: &ReleaseFfmpeg,
        slices: &[IosPluginSlice],
        output_directory: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<FfmpegSnapshot, IosError> {
        fs::create_dir(output_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create immutable FFmpeg release snapshot '{}': {error}",
                output_directory.display()
            ))
        })?;
        let mut budget = SnapshotBudget::default();
        let mut snapshot_slices = BTreeMap::new();
        let mut resolved_profile = None::<String>;
        for slice in slices {
            check_cancellation(cancellation, "FFmpeg release input snapshot")?;
            let relative = ffmpeg_slice_directory(*slice);
            let source = ffmpeg.output_directory.join(relative);
            let destination = output_directory.join(relative);
            copy_ffmpeg_snapshot_tree(&source, &destination, &mut budget, cancellation)?;
            let metadata_path = destination.join("vesper-ffmpeg-build-metadata.txt");
            let metadata = read_bounded_bytes(
                &metadata_path,
                MAX_METADATA_BYTES,
                "FFmpeg snapshot build metadata",
            )?;
            let library_checksums_path = destination.join("vesper-ffmpeg-library-sha256.txt");
            let library_checksums = read_bounded_bytes(
                &library_checksums_path,
                MAX_METADATA_BYTES,
                "FFmpeg snapshot library checksums",
            )?;
            let slice_profile =
                validate_snapshot_metadata(&metadata, ffmpeg, *slice, &metadata_path)?;
            match &resolved_profile {
                Some(profile) if profile != &slice_profile => {
                    return Err(IosError::conformance(
                        "FFmpeg snapshot slices do not share one resolved profile",
                    ));
                }
                Some(_) => {}
                None => resolved_profile = Some(slice_profile),
            }
            validate_snapshot_libraries(
                &destination,
                &library_checksums,
                &library_checksums_path,
                ffmpeg,
                cancellation,
            )?;
            let content_fingerprint = bounded_directory_content_fingerprint(
                &destination,
                "FFmpeg release build input",
                Some(cancellation),
                DirectorySnapshotLimits {
                    maximum_entries: MAX_FFMPEG_SNAPSHOT_ENTRIES,
                    maximum_depth: MAX_FFMPEG_SNAPSHOT_DEPTH,
                    maximum_bytes: MAX_FFMPEG_SNAPSHOT_BYTES,
                    digest_domain: b"vesper-ios-plugin-ffmpeg-build-content-v1\0",
                },
            )?;
            let input_fingerprint = format!(
                "{}-{content_fingerprint}",
                hex::encode(Sha256::digest(&metadata))
            );
            snapshot_slices.insert(
                *slice,
                FfmpegSnapshotSlice {
                    metadata,
                    library_checksums,
                    input_fingerprint,
                },
            );
        }
        let tree_snapshot = bounded_directory_snapshot(
            output_directory,
            "immutable FFmpeg release snapshot",
            Some(cancellation),
            DirectorySnapshotLimits {
                maximum_entries: MAX_FFMPEG_SNAPSHOT_ENTRIES,
                maximum_depth: MAX_FFMPEG_SNAPSHOT_DEPTH,
                maximum_bytes: MAX_FFMPEG_SNAPSHOT_BYTES,
                digest_domain: b"vesper-ios-plugin-ffmpeg-snapshot-v1\0",
            },
        )?;
        Ok(FfmpegSnapshot {
            output_directory: output_directory.to_path_buf(),
            tree_snapshot,
            resolved_profile: resolved_profile.ok_or_else(|| {
                IosError::worker("FFmpeg snapshot did not contain a selected slice")
            })?,
            slices: snapshot_slices,
        })
    }

    fn verify_ffmpeg_snapshot(
        snapshot: &FfmpegSnapshot,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let current = bounded_directory_snapshot(
            &snapshot.output_directory,
            "immutable FFmpeg release snapshot",
            Some(cancellation),
            DirectorySnapshotLimits {
                maximum_entries: MAX_FFMPEG_SNAPSHOT_ENTRIES,
                maximum_depth: MAX_FFMPEG_SNAPSHOT_DEPTH,
                maximum_bytes: MAX_FFMPEG_SNAPSHOT_BYTES,
                digest_domain: b"vesper-ios-plugin-ffmpeg-snapshot-v1\0",
            },
        )?;
        if current != snapshot.tree_snapshot {
            return Err(IosError::storage(
                "immutable FFmpeg release snapshot changed after it was created",
            ));
        }
        Ok(())
    }

    fn snapshot_runtime_archives(
        source_directory: &Path,
        ffmpeg: &ReleaseFfmpeg,
        destination_directory: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(PathBuf, Vec<RuntimeArchiveSourceSnapshot>), IosError> {
        let metadata = fs::symlink_metadata(destination_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect FFmpeg runtime archive snapshot owner '{}': {error}",
                destination_directory.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "FFmpeg runtime archive snapshot owner is not a regular directory: {}",
                destination_directory.display()
            )));
        }
        let mut budget = SnapshotBudget::default();
        let mut sources = Vec::with_capacity(ffmpeg.runtime_libraries.len());
        for library in &ffmpeg.runtime_libraries {
            check_cancellation(cancellation, "FFmpeg runtime archive snapshot")?;
            let framework = ffmpeg_framework_name(library)?;
            let name = format!("{framework}.xcframework.zip");
            let source = source_directory.join(&name);
            validate_regular_file(
                &source,
                MAX_RELEASE_ZIP_BYTES,
                "existing iOS FFmpeg runtime archive",
            )?;
            let source_snapshot = promotion_node_snapshot(
                &source,
                PromotionNodeKind::File,
                "existing iOS FFmpeg runtime archive",
                Some(cancellation),
            )?;
            budget.entries = budget
                .entries
                .checked_add(1)
                .ok_or_else(|| IosError::conformance("FFmpeg runtime archive count overflowed"))?;
            copy_stable_snapshot_file(
                &source,
                &destination_directory.join(name),
                &mut budget,
                cancellation,
            )?;
            sources.push(RuntimeArchiveSourceSnapshot {
                path: source,
                snapshot: source_snapshot,
            });
        }
        Ok((destination_directory.to_path_buf(), sources))
    }

    fn verify_runtime_archive_sources(
        sources: &[RuntimeArchiveSourceSnapshot],
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        for source in sources {
            let current = promotion_node_snapshot(
                &source.path,
                PromotionNodeKind::File,
                "existing iOS FFmpeg runtime archive",
                Some(cancellation),
            )?;
            if current != source.snapshot {
                return Err(IosError::storage(format!(
                    "iOS FFmpeg runtime archive changed after it was snapshotted: {}",
                    source.path.display()
                )));
            }
        }
        Ok(())
    }

    fn verify_raw_snapshot_marker(
        raw_output: &Path,
        plugin_id: IosPluginId,
        slices: &[IosPluginSlice],
        minimum_os: &str,
        snapshot: &FfmpegSnapshot,
    ) -> Result<(), IosError> {
        let marker_path = raw_output.join(".vesper-ios-plugin-output");
        let bytes = read_bounded_bytes(
            &marker_path,
            16 * 1024,
            "iOS plugin raw output ownership marker",
        )?;
        let actual: RawOutputMarker = serde_json::from_slice(&bytes).map_err(|error| {
            IosError::conformance(format!(
                "iOS plugin raw output marker '{}' is invalid: {error}",
                marker_path.display()
            ))
        })?;
        let expected = RawOutputMarker {
            format: "vesper-ios-plugin-output".to_owned(),
            plugin_id: plugin_id.as_str().to_owned(),
            cargo_profile: "release".to_owned(),
            slices: slices
                .iter()
                .map(|slice| slice.as_str().to_owned())
                .collect(),
            ios_deployment_target: Some(minimum_os.to_owned()),
            ffmpeg_profile: Some(snapshot.resolved_profile.clone()),
            ffmpeg_inputs: slices
                .iter()
                .map(|slice| {
                    snapshot
                        .slices
                        .get(slice)
                        .map(|snapshot| RawOutputFfmpegInput {
                            slice: slice.as_str().to_owned(),
                            input_fingerprint: snapshot.input_fingerprint.clone(),
                        })
                        .ok_or_else(|| {
                            IosError::worker("FFmpeg snapshot omitted a selected raw build slice")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        if actual != expected {
            return Err(IosError::conformance(
                "iOS plugin raw output does not match its immutable FFmpeg snapshot",
            ));
        }
        Ok(())
    }

    fn copy_ffmpeg_snapshot_tree(
        source: &Path,
        destination: &Path,
        budget: &mut SnapshotBudget,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let source_metadata = fs::symlink_metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect FFmpeg release input '{}': {error}",
                source.display()
            ))
        })?;
        if !source_metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "FFmpeg release input must be a regular non-symlink directory: {}",
                source.display()
            )));
        }
        let canonical_source = fs::canonicalize(source).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve FFmpeg release input '{}': {error}",
                source.display()
            ))
        })?;
        let metadata = fs::symlink_metadata(&canonical_source).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect FFmpeg release input '{}': {error}",
                canonical_source.display()
            ))
        })?;
        if !metadata.file_type().is_dir()
            || metadata_identity(&metadata) != metadata_identity(&source_metadata)
        {
            return Err(IosError::storage(format!(
                "FFmpeg release input changed while it was resolved: {}",
                source.display()
            )));
        }
        fs::create_dir(destination).map_err(|error| {
            IosError::storage(format!(
                "failed to create FFmpeg snapshot slice '{}': {error}",
                destination.display()
            ))
        })?;

        let mut pending = VecDeque::from([(
            canonical_source.clone(),
            destination.to_path_buf(),
            0_usize,
            metadata_identity(&metadata),
        )]);
        while let Some((source_directory, destination_directory, depth, expected_identity)) =
            pending.pop_front()
        {
            check_cancellation(cancellation, "FFmpeg release input snapshot")?;
            if depth > MAX_FFMPEG_SNAPSHOT_DEPTH {
                return Err(IosError::conformance(format!(
                    "FFmpeg release input exceeds traversal depth {MAX_FFMPEG_SNAPSHOT_DEPTH}: {}",
                    source.display()
                )));
            }
            let directory_metadata = fs::symlink_metadata(&source_directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect FFmpeg release input directory '{}': {error}",
                    source_directory.display()
                ))
            })?;
            if !directory_metadata.file_type().is_dir()
                || metadata_identity(&directory_metadata) != expected_identity
            {
                return Err(IosError::storage(format!(
                    "FFmpeg release input directory changed during snapshot: {}",
                    source_directory.display()
                )));
            }
            let remaining_entries = MAX_FFMPEG_SNAPSHOT_ENTRIES
                .checked_sub(budget.entries)
                .ok_or_else(|| {
                    IosError::conformance("FFmpeg snapshot entry budget was exceeded")
                })?;
            let mut children = collect_directory_entries_bounded(
                &source_directory,
                remaining_entries,
                "FFmpeg release input",
                IosError::conformance(format!(
                    "FFmpeg release input exceeds {MAX_FFMPEG_SNAPSHOT_ENTRIES} entries"
                )),
            )?;
            children.sort_by_key(|entry| entry.file_name());
            let initial_names = children
                .iter()
                .map(fs::DirEntry::file_name)
                .collect::<Vec<_>>();
            for child in children {
                check_cancellation(cancellation, "FFmpeg release input snapshot")?;
                budget.entries = budget.entries.checked_add(1).ok_or_else(|| {
                    IosError::conformance("FFmpeg snapshot entry count overflowed")
                })?;
                if budget.entries > MAX_FFMPEG_SNAPSHOT_ENTRIES {
                    return Err(IosError::conformance(format!(
                        "FFmpeg release input exceeds {MAX_FFMPEG_SNAPSHOT_ENTRIES} entries"
                    )));
                }
                let source_path = child.path();
                let destination_path = destination_directory.join(child.file_name());
                let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to inspect FFmpeg release input '{}': {error}",
                        source_path.display()
                    ))
                })?;
                if metadata.file_type().is_dir() {
                    let identity = metadata_identity(&metadata);
                    fs::create_dir(&destination_path).map_err(|error| {
                        IosError::storage(format!(
                            "failed to create FFmpeg snapshot directory '{}': {error}",
                            destination_path.display()
                        ))
                    })?;
                    pending.push_back((source_path, destination_path, depth + 1, identity));
                    continue;
                }
                let regular_source = if metadata.file_type().is_symlink() {
                    let target = fs::read_link(&source_path).map_err(|error| {
                        IosError::storage(format!(
                            "failed to read FFmpeg input symlink '{}': {error}",
                            source_path.display()
                        ))
                    })?;
                    let mut components = target.components();
                    if target.is_absolute()
                        || !matches!(components.next(), Some(Component::Normal(_)))
                        || components.next().is_some()
                    {
                        return Err(IosError::conformance(format!(
                            "FFmpeg input symlink must target one sibling file: {}",
                            source_path.display()
                        )));
                    }
                    let resolved = fs::canonicalize(&source_path).map_err(|error| {
                        IosError::storage(format!(
                            "failed to resolve FFmpeg input symlink '{}': {error}",
                            source_path.display()
                        ))
                    })?;
                    if !resolved.starts_with(&canonical_source)
                        || resolved.parent() != source_path.parent()
                    {
                        return Err(IosError::conformance(format!(
                            "FFmpeg input symlink does not resolve to a sibling file: {}",
                            source_path.display()
                        )));
                    }
                    resolved
                } else if metadata.file_type().is_file() {
                    source_path
                } else {
                    return Err(IosError::conformance(format!(
                        "FFmpeg release input contains an unsupported node: {}",
                        source_path.display()
                    )));
                };
                copy_stable_snapshot_file(
                    &regular_source,
                    &destination_path,
                    budget,
                    cancellation,
                )?;
            }
            let final_metadata = fs::symlink_metadata(&source_directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to re-inspect FFmpeg release input directory '{}': {error}",
                    source_directory.display()
                ))
            })?;
            let mut final_names = collect_directory_names_bounded(
                &source_directory,
                initial_names.len(),
                "FFmpeg release input",
                IosError::storage(format!(
                    "FFmpeg release input directory '{}' changed during snapshot",
                    source_directory.display()
                )),
            )?;
            final_names.sort();
            if !final_metadata.file_type().is_dir()
                || metadata_identity(&final_metadata) != expected_identity
                || final_names != initial_names
            {
                return Err(IosError::storage(format!(
                    "FFmpeg release input directory changed during snapshot: {}",
                    source_directory.display()
                )));
            }
        }
        Ok(())
    }

    fn copy_stable_snapshot_file(
        source: &Path,
        destination: &Path,
        budget: &mut SnapshotBudget,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let initial = fs::symlink_metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect FFmpeg snapshot source '{}': {error}",
                source.display()
            ))
        })?;
        if !initial.file_type().is_file() {
            return Err(IosError::conformance(format!(
                "FFmpeg snapshot source is not a regular file: {}",
                source.display()
            )));
        }
        let identity = metadata_identity(&initial);
        budget.bytes = budget
            .bytes
            .checked_add(initial.len())
            .ok_or_else(|| IosError::conformance("FFmpeg snapshot byte count overflowed"))?;
        if budget.bytes > MAX_FFMPEG_SNAPSHOT_BYTES {
            return Err(IosError::conformance(format!(
                "FFmpeg release input exceeds {MAX_FFMPEG_SNAPSHOT_BYTES} bytes"
            )));
        }

        let mut input = File::open(source).map_err(|error| {
            IosError::storage(format!(
                "failed to open FFmpeg snapshot source '{}': {error}",
                source.display()
            ))
        })?;
        let opened = input.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened FFmpeg snapshot source '{}': {error}",
                source.display()
            ))
        })?;
        if !opened.file_type().is_file()
            || metadata_identity(&opened) != identity
            || opened.len() != initial.len()
        {
            return Err(IosError::storage(format!(
                "FFmpeg snapshot source changed while it was opened: {}",
                source.display()
            )));
        }
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create FFmpeg snapshot file '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut copied = 0_u64;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            check_cancellation(cancellation, "FFmpeg release input snapshot")?;
            let count = input.read(&mut buffer).map_err(|error| {
                IosError::storage(format!(
                    "failed to read FFmpeg snapshot source '{}': {error}",
                    source.display()
                ))
            })?;
            if count == 0 {
                break;
            }
            copied = copied
                .checked_add(count as u64)
                .ok_or_else(|| IosError::conformance("FFmpeg snapshot file size overflowed"))?;
            if copied > initial.len() {
                return Err(IosError::storage(format!(
                    "FFmpeg snapshot source grew while it was copied: {}",
                    source.display()
                )));
            }
            digest.update(&buffer[..count]);
            output.write_all(&buffer[..count]).map_err(|error| {
                IosError::storage(format!(
                    "failed to write FFmpeg snapshot file '{}': {error}",
                    destination.display()
                ))
            })?;
        }
        if copied != initial.len() {
            return Err(IosError::storage(format!(
                "FFmpeg snapshot source changed size while it was copied: {}",
                source.display()
            )));
        }
        output
            .set_permissions(initial.permissions())
            .and_then(|()| output.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to synchronize FFmpeg snapshot file '{}': {error}",
                    destination.display()
                ))
            })?;
        let final_metadata = fs::symlink_metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect FFmpeg snapshot source '{}': {error}",
                source.display()
            ))
        })?;
        let copied_digest = hex::encode(digest.finalize());
        if !final_metadata.file_type().is_file()
            || metadata_identity(&final_metadata) != identity
            || final_metadata.len() != initial.len()
            || sha256_file(source, initial.len())? != copied_digest
        {
            return Err(IosError::storage(format!(
                "FFmpeg snapshot source changed while it was copied: {}",
                source.display()
            )));
        }
        Ok(())
    }

    fn validate_snapshot_metadata(
        bytes: &[u8],
        ffmpeg: &ReleaseFfmpeg,
        slice: IosPluginSlice,
        path: &Path,
    ) -> Result<String, IosError> {
        let values = parse_ffmpeg_metadata(bytes, path)?;
        let ffmpeg_version = ffmpeg.source_lock.version().to_string();
        for (key, expected) in [
            ("platform", "apple"),
            ("target", ffmpeg_metadata_target(slice)),
            ("declared_profile", ffmpeg.declared_profile.as_str()),
            ("declared_platform", "ios"),
            ("profile_hash", ffmpeg.profile_hash.as_str()),
            ("ffmpeg_version", ffmpeg_version.as_str()),
            ("source_url", ffmpeg.source_lock.source_url()),
            ("source_sha256", ffmpeg.source_lock.source_sha256()),
        ] {
            if values.get(key).map(String::as_str) != Some(expected) {
                return Err(IosError::conformance(format!(
                    "FFmpeg snapshot metadata '{}' has an unexpected {key}",
                    path.display()
                )));
            }
        }
        values
            .get("profile")
            .filter(|profile| !profile.is_empty())
            .cloned()
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "FFmpeg snapshot metadata '{}' omits its resolved profile",
                    path.display()
                ))
            })
    }

    fn parse_ffmpeg_metadata(
        bytes: &[u8],
        path: &Path,
    ) -> Result<BTreeMap<String, String>, IosError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            IosError::conformance(format!(
                "FFmpeg snapshot metadata '{}' is not UTF-8: {error}",
                path.display()
            ))
        })?;
        if !text.ends_with('\n') || text.contains('\r') {
            return Err(IosError::conformance(format!(
                "FFmpeg snapshot metadata '{}' must use LF-terminated records",
                path.display()
            )));
        }
        let mut lines = text.lines();
        if lines.next() != Some("Vesper FFmpeg build metadata v2") {
            return Err(IosError::conformance(format!(
                "FFmpeg snapshot metadata '{}' has an unsupported header",
                path.display()
            )));
        }
        let mut values = BTreeMap::new();
        for line in lines {
            let (key, value) = line.split_once('=').ok_or_else(|| {
                IosError::conformance(format!(
                    "FFmpeg snapshot metadata '{}' contains a malformed record",
                    path.display()
                ))
            })?;
            if key.is_empty() || values.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(IosError::conformance(format!(
                    "FFmpeg snapshot metadata '{}' contains an empty or duplicate key",
                    path.display()
                )));
            }
        }
        Ok(values)
    }

    fn validate_snapshot_libraries(
        slice_directory: &Path,
        checksum_bytes: &[u8],
        checksum_path: &Path,
        ffmpeg: &ReleaseFfmpeg,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let checksums = parse_snapshot_library_checksums(checksum_bytes, checksum_path)?;
        if checksums.len() != ffmpeg.runtime_libraries.len() {
            return Err(IosError::conformance(format!(
                "FFmpeg snapshot checksums do not exactly match the release profile: {}",
                checksum_path.display()
            )));
        }
        for library in &ffmpeg.runtime_libraries {
            check_cancellation(cancellation, "FFmpeg release input snapshot validation")?;
            let key = format!("{library}_sha256");
            let expected = checksums.get(&key).ok_or_else(|| {
                IosError::conformance(format!(
                    "FFmpeg snapshot omits the {library} library checksum"
                ))
            })?;
            let binary = slice_directory
                .join("lib/arm64")
                .join(format!("lib{library}.dylib"));
            let actual = sha256_file(&binary, MAX_BINARY_BYTES)?;
            if &actual != expected {
                return Err(IosError::conformance(format!(
                    "FFmpeg snapshot library checksum mismatch: {}",
                    binary.display()
                )));
            }
        }
        Ok(())
    }

    fn parse_snapshot_library_checksums(
        checksum_bytes: &[u8],
        checksum_path: &Path,
    ) -> Result<BTreeMap<String, String>, IosError> {
        let checksum_text = std::str::from_utf8(checksum_bytes).map_err(|error| {
            IosError::conformance(format!(
                "FFmpeg snapshot checksums '{}' are not UTF-8: {error}",
                checksum_path.display()
            ))
        })?;
        if !checksum_text.ends_with('\n') || checksum_text.contains('\r') {
            return Err(IosError::conformance(format!(
                "FFmpeg snapshot checksums '{}' must use LF-terminated records",
                checksum_path.display()
            )));
        }
        let mut checksums = BTreeMap::new();
        for line in checksum_text.lines() {
            let (key, value) = line.split_once('=').ok_or_else(|| {
                IosError::conformance(format!(
                    "FFmpeg snapshot checksum record is malformed: {}",
                    checksum_path.display()
                ))
            })?;
            if key.is_empty()
                || value.len() != 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                || checksums.insert(key.to_owned(), value.to_owned()).is_some()
            {
                return Err(IosError::conformance(format!(
                    "FFmpeg snapshot checksum record is invalid: {}",
                    checksum_path.display()
                )));
            }
        }
        Ok(checksums)
    }

    fn verify_runtime_archives(
        archive_directory: &Path,
        ffmpeg: &ReleaseFfmpeg,
        snapshot: &FfmpegSnapshot,
        slices: &[IosPluginSlice],
    ) -> Result<(), IosError> {
        for library in &ffmpeg.runtime_libraries {
            let framework = ffmpeg_framework_name(library)?;
            let archive = archive_directory.join(format!("{framework}.xcframework.zip"));
            crate::ios_optional_release::preflight_runtime_framework_archive(&archive, framework)?;
            for slice in slices {
                let snapshot_slice = snapshot
                    .slices
                    .get(slice)
                    .ok_or_else(|| IosError::worker("FFmpeg snapshot omitted a selected slice"))?;
                let framework_root = format!(
                    "{framework}.xcframework/{}/{framework}.framework",
                    xcframework_slice_identifier(*slice)
                );
                let profile_path = format!("{framework_root}/profile-hash.txt");
                let profile = read_zip_record(&archive, &profile_path, MAX_METADATA_BYTES)?;
                if parse_single_record(&profile, &profile_path)? != ffmpeg.profile_hash {
                    return Err(IosError::conformance(format!(
                        "iOS FFmpeg runtime profile hash mismatch for {}/{framework}",
                        slice.as_str()
                    )));
                }
                let metadata_path = format!("{framework_root}/{}", metadata_file_name(*slice));
                let metadata = read_zip_record(&archive, &metadata_path, MAX_METADATA_BYTES)?;
                if metadata != snapshot_slice.metadata {
                    return Err(IosError::conformance(format!(
                        "iOS FFmpeg runtime metadata does not match the immutable input snapshot for {}/{framework}",
                        slice.as_str()
                    )));
                }
                let fingerprint_path = format!("{framework_root}/input-fingerprint.txt");
                let fingerprint = read_zip_record(&archive, &fingerprint_path, MAX_METADATA_BYTES)?;
                if parse_single_record(&fingerprint, &fingerprint_path)?
                    != snapshot_slice.input_fingerprint
                {
                    return Err(IosError::conformance(format!(
                        "iOS FFmpeg runtime input fingerprint does not match the immutable snapshot for {}/{framework}",
                        slice.as_str()
                    )));
                }
                let checksum_path = format!("{framework_root}/binary-sha256.txt");
                let checksum = read_zip_record(&archive, &checksum_path, 256)?;
                let checksum = parse_sha256_record(&checksum, &checksum_path)?;
                let binary_path = format!("{framework_root}/{framework}");
                let actual = sha256_zip_entry(&archive, &binary_path, MAX_BINARY_BYTES)?;
                if actual != checksum {
                    return Err(IosError::conformance(format!(
                        "iOS FFmpeg runtime binary checksum mismatch: {binary_path}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn read_zip_record(path: &Path, name: &str, maximum: u64) -> Result<Vec<u8>, IosError> {
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS runtime archive '{}': {error}",
                path.display()
            ))
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            IosError::conformance(format!(
                "invalid iOS runtime archive '{}': {error}",
                path.display()
            ))
        })?;
        let mut entry = archive.by_name(name).map_err(|error| {
            IosError::conformance(format!(
                "missing iOS runtime archive entry '{name}': {error}"
            ))
        })?;
        if entry.size() > maximum {
            return Err(IosError::conformance(format!(
                "iOS runtime archive entry '{name}' exceeds {maximum} bytes"
            )));
        }
        let declared = entry.size();
        let capacity = usize::try_from(declared).map_err(|_| {
            IosError::conformance(format!(
                "iOS runtime archive entry '{name}' is too large for this host"
            ))
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        (&mut entry)
            .take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::conformance(format!(
                    "failed to decompress iOS runtime archive entry '{name}': {error}"
                ))
            })?;
        if bytes.len() as u64 != declared || bytes.len() as u64 > maximum {
            return Err(IosError::conformance(format!(
                "iOS runtime archive entry '{name}' payload size does not match its record"
            )));
        }
        Ok(bytes)
    }

    fn sha256_zip_entry(path: &Path, name: &str, maximum: u64) -> Result<String, IosError> {
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS runtime archive '{}': {error}",
                path.display()
            ))
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            IosError::conformance(format!(
                "invalid iOS runtime archive '{}': {error}",
                path.display()
            ))
        })?;
        let mut entry = archive.by_name(name).map_err(|error| {
            IosError::conformance(format!(
                "missing iOS runtime archive entry '{name}': {error}"
            ))
        })?;
        if entry.size() > maximum {
            return Err(IosError::conformance(format!(
                "iOS runtime archive entry '{name}' exceeds {maximum} bytes"
            )));
        }
        let declared = entry.size();
        let mut count = 0_u64;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(|error| {
                IosError::conformance(format!(
                    "failed to decompress iOS runtime archive entry '{name}': {error}"
                ))
            })?;
            if read == 0 {
                break;
            }
            count = count.checked_add(read as u64).ok_or_else(|| {
                IosError::conformance("iOS runtime archive entry size overflowed")
            })?;
            if count > maximum {
                return Err(IosError::conformance(format!(
                    "iOS runtime archive entry '{name}' exceeds {maximum} bytes"
                )));
            }
            digest.update(&buffer[..read]);
        }
        if count != declared {
            return Err(IosError::conformance(format!(
                "iOS runtime archive entry '{name}' payload size does not match its record"
            )));
        }
        Ok(hex::encode(digest.finalize()))
    }

    fn parse_single_record<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, IosError> {
        let record = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        let record = std::str::from_utf8(record).map_err(|error| {
            IosError::conformance(format!(
                "iOS runtime record '{label}' is not UTF-8: {error}"
            ))
        })?;
        if record.is_empty()
            || record
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(IosError::conformance(format!(
                "iOS runtime record '{label}' must contain one non-whitespace value"
            )));
        }
        Ok(record)
    }

    fn parse_sha256_record<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, IosError> {
        let record = parse_single_record(bytes, label)?;
        if record.len() != 64
            || !record
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(IosError::conformance(format!(
                "iOS runtime checksum '{label}' is not a lowercase SHA-256 value"
            )));
        }
        Ok(record)
    }

    fn read_bounded_bytes(path: &Path, maximum: u64, label: &str) -> Result<Vec<u8>, IosError> {
        validate_regular_file(path, maximum, label)?;
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut bytes = Vec::new();
        file.take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.len() as u64 > maximum {
            return Err(IosError::conformance(format!(
                "{label} exceeds {maximum} bytes: {}",
                path.display()
            )));
        }
        Ok(bytes)
    }

    fn metadata_identity(metadata: &fs::Metadata) -> FileIdentity {
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    const fn ffmpeg_metadata_target(slice: IosPluginSlice) -> &'static str {
        match slice {
            IosPluginSlice::DeviceArm64 => "ios-arm64",
            IosPluginSlice::SimulatorArm64 => "ios-simulator-arm64",
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_frameworks(
        root: &Path,
        plugin_id: IosPluginId,
        plugin_descriptor: &CanonicalPluginDescriptor,
        request: &ReleaseRequest,
        raw: &Path,
        frameworks: &Path,
        xcframework: &Path,
        ffmpeg_snapshot: Option<&FfmpegSnapshot>,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        fs::create_dir_all(frameworks).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS plugin framework staging '{}': {error}",
                frameworks.display()
            ))
        })?;
        let mut framework_paths = Vec::new();
        for slice in &request.slices {
            check_cancellation(cancellation, "iOS plugin framework construction")?;
            let raw_binary = match slice {
                IosPluginSlice::DeviceArm64 => raw.join("iphoneos").join(plugin.dylib_name),
                IosPluginSlice::SimulatorArm64 => {
                    raw.join("iphonesimulator").join(plugin.dylib_name)
                }
            };
            let framework = frameworks
                .join(slice.as_str())
                .join(format!("{}.framework", plugin.framework_name));
            fs::create_dir_all(framework.join("Headers")).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin framework Headers '{}': {error}",
                    framework.display()
                ))
            })?;
            fs::create_dir_all(framework.join("Modules")).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin framework Modules '{}': {error}",
                    framework.display()
                ))
            })?;
            let binary = framework.join(plugin.framework_name);
            copy_regular_file(&raw_binary, &binary, MAX_BINARY_BYTES, cancellation)?;
            prepare_framework_binary(
                plugin_id,
                request,
                &binary,
                tools,
                diagnostics,
                cancellation,
            )?;
            if let Some(ffmpeg) = &request.ffmpeg {
                let snapshot = ffmpeg_snapshot.ok_or_else(|| {
                    IosError::worker("FFmpeg-backed iOS plugin release has no input snapshot")
                })?;
                let snapshot_slice = snapshot.slices.get(slice).ok_or_else(|| {
                    IosError::worker("FFmpeg-backed iOS plugin snapshot omitted a selected slice")
                })?;
                write_text_file(
                    &framework.join("binary-sha256.txt"),
                    &format!("{}\n", sha256_file(&binary, MAX_BINARY_BYTES)?),
                )?;
                let metadata_source = snapshot
                    .output_directory
                    .join(ffmpeg_slice_directory(*slice))
                    .join("vesper-ffmpeg-build-metadata.txt");
                copy_regular_file(
                    &metadata_source,
                    &framework.join(metadata_file_name(*slice)),
                    MAX_METADATA_BYTES,
                    cancellation,
                )?;
                if read_bounded_bytes(
                    &metadata_source,
                    MAX_METADATA_BYTES,
                    "FFmpeg snapshot build metadata",
                )? != snapshot_slice.metadata
                {
                    return Err(IosError::storage(
                        "FFmpeg snapshot metadata changed before plugin framework construction",
                    ));
                }
                let checksum_source = snapshot
                    .output_directory
                    .join(ffmpeg_slice_directory(*slice))
                    .join("vesper-ffmpeg-library-sha256.txt");
                if read_bounded_bytes(
                    &checksum_source,
                    MAX_METADATA_BYTES,
                    "FFmpeg snapshot library checksums",
                )? != snapshot_slice.library_checksums
                {
                    return Err(IosError::storage(
                        "FFmpeg snapshot checksums changed before plugin framework construction",
                    ));
                }
                write_text_file(
                    &framework.join("profile-hash.txt"),
                    &format!("{}\n", ffmpeg.profile_hash),
                )?;
                write_text_file(
                    &framework.join("input-fingerprint.txt"),
                    &format!("{}\n", snapshot_slice.input_fingerprint),
                )?;
            }
            write_framework_module(&framework, plugin.framework_name)?;
            write_framework_plist(
                &framework.join("Info.plist"),
                plugin.framework_name,
                plugin.bundle_identifier,
                platform_name(*slice),
                &request.version,
                &request.build,
                &request.minimum_os,
                tools,
                diagnostics,
                cancellation,
            )?;
            write_embedded_plugin_registry_fragment(
                plugin_descriptor,
                plugin_id,
                *slice,
                &request.minimum_os,
                &framework,
            )?;
            validate_framework(
                plugin_id,
                plugin_descriptor,
                request,
                *slice,
                &framework,
                ffmpeg_snapshot,
            )?;
            verify_exact_arm64(&binary, tools, diagnostics, cancellation)?;
            framework_paths.push(framework);
        }

        let mut command = Command::new(&tools.xcodebuild);
        command.arg("-create-xcframework");
        for framework in &framework_paths {
            command.arg("-framework").arg(framework);
        }
        command.arg("-output").arg(xcframework);
        run_required_command(
            &mut command,
            "iOS plugin XCFramework construction",
            diagnostics,
            cancellation,
        )?;
        let _ = root;
        Ok(())
    }

    fn prepare_framework_binary(
        plugin_id: IosPluginId,
        request: &ReleaseRequest,
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        let runtime_libraries = request
            .ffmpeg
            .as_ref()
            .map(|ffmpeg| ffmpeg.runtime_libraries.as_slice())
            .unwrap_or_default();
        prepare_dynamic_framework_binary(
            plugin.framework_name,
            runtime_libraries,
            binary,
            "iOS plugin framework",
            tools,
            diagnostics,
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_dynamic_framework_binary(
        framework_name: &str,
        runtime_libraries: &[String],
        binary: &Path,
        label: &str,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let expected_install_name = format!("@rpath/{framework_name}.framework/{framework_name}");
        let mut install_id = Command::new(&tools.install_name_tool);
        install_id
            .args(["-id", expected_install_name.as_str()])
            .arg(binary);
        let operation = format!("{label} install-name update");
        run_required_command(&mut install_id, &operation, diagnostics, cancellation)?;

        let rpaths = read_rpaths(binary, tools, diagnostics, cancellation)?;
        for stale in [
            "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks",
            "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks",
            "@loader_path/Frameworks",
        ] {
            if rpaths.contains(stale) {
                let mut command = Command::new(&tools.install_name_tool);
                command.args(["-delete_rpath", stale]).arg(binary);
                let operation = format!("{label} stale-rpath removal");
                run_required_command(&mut command, &operation, diagnostics, cancellation)?;
            }
        }
        if !rpaths.contains("@loader_path/..") {
            let mut command = Command::new(&tools.install_name_tool);
            command.args(["-add_rpath", "@loader_path/.."]).arg(binary);
            let operation = format!("{label} loader-rpath update");
            run_required_command(&mut command, &operation, diagnostics, cancellation)?;
        }

        let runtime_frameworks = runtime_libraries
            .iter()
            .map(|library| ffmpeg_framework_name(library).map(str::to_owned))
            .collect::<Result<BTreeSet<_>, _>>()?;
        for dependency in read_dependencies(binary, tools, diagnostics, cancellation)? {
            let Some(library) = dependency_ffmpeg_library(&dependency) else {
                continue;
            };
            let framework = ffmpeg_framework_name(library)?;
            if !runtime_frameworks.contains(framework) {
                return Err(IosError::conformance(format!(
                    "{label} depends on an FFmpeg component absent from its release profile: {dependency}"
                )));
            }
            let replacement = format!("@rpath/{framework}.framework/{framework}");
            let mut command = Command::new(&tools.install_name_tool);
            command
                .arg("-change")
                .arg(&dependency)
                .arg(replacement)
                .arg(binary);
            let operation = format!("{label} FFmpeg dependency rewrite");
            run_required_command(&mut command, &operation, diagnostics, cancellation)?;
        }

        let final_rpaths = read_rpaths(binary, tools, diagnostics, cancellation)?;
        if !final_rpaths.contains("@loader_path/..")
            || final_rpaths.iter().any(|value| {
                matches!(
                    value.as_str(),
                    "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks"
                        | "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
                        | "@loader_path/Frameworks"
                )
            })
        {
            return Err(IosError::conformance(format!(
                "{label} has an invalid final rpath set"
            )));
        }
        let dependencies = read_dependencies(binary, tools, diagnostics, cancellation)?;
        if dependencies
            .iter()
            .any(|dependency| dependency_ffmpeg_library(dependency).is_some())
        {
            return Err(IosError::conformance(format!(
                "{label} retains an unwrapped FFmpeg dependency"
            )));
        }
        let install_name = read_install_name(binary, tools, diagnostics, cancellation)?;
        if install_name != expected_install_name {
            return Err(IosError::conformance(format!(
                "{label} has an unexpected install name: {install_name}"
            )));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_runtime_sibling_dependencies(
        binary: &Path,
        sibling_root: &Path,
        ffmpeg: &ReleaseFfmpeg,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let expected = ffmpeg
            .runtime_libraries
            .iter()
            .map(|library| {
                let framework = ffmpeg_framework_name(library)?;
                Ok((
                    format!("@rpath/{framework}.framework/{framework}"),
                    framework,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, IosError>>()?;
        for dependency in read_dependencies(binary, tools, diagnostics, cancellation)? {
            if dependency_ffmpeg_library(&dependency).is_some() {
                return Err(IosError::conformance(format!(
                    "iOS FFmpeg runtime framework retains an unwrapped dependency: {dependency}"
                )));
            }
            if !dependency.starts_with("@rpath/VesperFFmpeg") {
                continue;
            }
            let framework = expected.get(&dependency).ok_or_else(|| {
                IosError::conformance(format!(
                    "iOS FFmpeg runtime framework has an unexpected component dependency: {dependency}"
                ))
            })?;
            let sibling = sibling_root.join(format!("{framework}.framework"));
            let metadata = fs::symlink_metadata(&sibling).map_err(|error| {
                IosError::conformance(format!(
                    "missing sibling FFmpeg framework required by '{}': {error}",
                    binary.display()
                ))
            })?;
            if !metadata.file_type().is_dir() {
                return Err(IosError::conformance(format!(
                    "sibling FFmpeg framework must be a regular non-symlink directory: {}",
                    sibling.display()
                )));
            }
        }
        Ok(())
    }

    fn validate_runtime_framework(
        framework: &Path,
        framework_name: &str,
        ffmpeg: &ReleaseFfmpeg,
        snapshot_slice: &FfmpegSnapshotSlice,
        slice: IosPluginSlice,
    ) -> Result<(), IosError> {
        let expected = BTreeSet::from([
            OsString::from("Headers"),
            OsString::from("Info.plist"),
            OsString::from("Modules"),
            OsString::from(framework_name),
            OsString::from("binary-sha256.txt"),
            OsString::from("input-fingerprint.txt"),
            OsString::from(metadata_file_name(slice)),
            OsString::from("profile-hash.txt"),
        ]);
        if read_directory_names(
            framework,
            "iOS FFmpeg runtime framework",
            MAX_RELEASE_TREE_ENTRIES,
        )? != expected
        {
            return Err(IosError::conformance(format!(
                "iOS FFmpeg runtime framework has an unexpected payload: {}",
                framework.display()
            )));
        }
        let expected_header = format!("/* Binary distribution marker for {framework_name}. */\n");
        let expected_module = format!(
            "framework module {framework_name} {{\n  umbrella header \"{framework_name}.h\"\n  export *\n  module * {{ export * }}\n}}\n"
        );
        if read_bounded_utf8(
            &framework
                .join("Headers")
                .join(format!("{framework_name}.h")),
            MAX_METADATA_BYTES,
            "iOS FFmpeg runtime framework header",
        )? != expected_header
            || read_bounded_utf8(
                &framework.join("Modules/module.modulemap"),
                MAX_METADATA_BYTES,
                "iOS FFmpeg runtime framework module map",
            )? != expected_module
        {
            return Err(IosError::conformance(
                "iOS FFmpeg runtime framework module payload is invalid",
            ));
        }
        let binary = framework.join(framework_name);
        let checksum = read_bounded_bytes(
            &framework.join("binary-sha256.txt"),
            256,
            "iOS FFmpeg runtime binary checksum",
        )?;
        if parse_sha256_record(&checksum, "iOS FFmpeg runtime binary checksum")?
            != sha256_file(&binary, MAX_BINARY_BYTES)?
        {
            return Err(IosError::conformance(
                "iOS FFmpeg runtime framework binary checksum does not match its binary",
            ));
        }
        let profile = read_bounded_bytes(
            &framework.join("profile-hash.txt"),
            MAX_METADATA_BYTES,
            "iOS FFmpeg runtime profile hash",
        )?;
        if parse_single_record(&profile, "iOS FFmpeg runtime profile hash")? != ffmpeg.profile_hash
        {
            return Err(IosError::conformance(
                "iOS FFmpeg runtime framework profile hash does not match its release profile",
            ));
        }
        let fingerprint = read_bounded_bytes(
            &framework.join("input-fingerprint.txt"),
            MAX_METADATA_BYTES,
            "iOS FFmpeg runtime input fingerprint",
        )?;
        if parse_single_record(&fingerprint, "iOS FFmpeg runtime input fingerprint")?
            != snapshot_slice.input_fingerprint
        {
            return Err(IosError::conformance(
                "iOS FFmpeg runtime framework input fingerprint does not match its snapshot",
            ));
        }
        let metadata = read_bounded_bytes(
            &framework.join(metadata_file_name(slice)),
            MAX_METADATA_BYTES,
            "iOS FFmpeg runtime metadata",
        )?;
        if metadata != snapshot_slice.metadata {
            return Err(IosError::conformance(
                "iOS FFmpeg runtime framework metadata does not match its snapshot",
            ));
        }
        Ok(())
    }

    fn read_rpaths(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<BTreeSet<String>, IosError> {
        let mut command = Command::new(&tools.otool);
        command.arg("-l").arg(binary);
        let result = run_process(
            &mut command,
            "iOS plugin rpath inspection",
            diagnostics,
            cancellation,
        )?;
        let output = parse_process_utf8(&result.stdout, "otool rpath output")?;
        Ok(output
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_ascii_whitespace();
                (fields.next() == Some("path"))
                    .then(|| fields.next().map(str::to_owned))
                    .flatten()
            })
            .collect())
    }

    fn read_dependencies(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<Vec<String>, IosError> {
        let mut command = Command::new(&tools.otool);
        command.arg("-L").arg(binary);
        let result = run_process(
            &mut command,
            "iOS plugin dependency inspection",
            diagnostics,
            cancellation,
        )?;
        let output = parse_process_utf8(&result.stdout, "otool dependency output")?;
        Ok(output
            .lines()
            .skip(1)
            .filter_map(|line| line.split_ascii_whitespace().next().map(str::to_owned))
            .collect())
    }

    fn read_install_name(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<String, IosError> {
        let mut command = Command::new(&tools.otool);
        command.arg("-D").arg(binary);
        let result = run_process(
            &mut command,
            "iOS plugin install-name inspection",
            diagnostics,
            cancellation,
        )?;
        parse_process_utf8(&result.stdout, "otool install-name output")?
            .lines()
            .nth(1)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| IosError::conformance("otool omitted the iOS plugin install name"))
    }

    fn dependency_ffmpeg_library(dependency: &str) -> Option<&'static str> {
        let name = Path::new(dependency).file_name()?.to_str()?;
        [
            ("libavcodec", "avcodec"),
            ("libavdevice", "avdevice"),
            ("libavfilter", "avfilter"),
            ("libavformat", "avformat"),
            ("libavutil", "avutil"),
            ("libpostproc", "postproc"),
            ("libswresample", "swresample"),
            ("libswscale", "swscale"),
        ]
        .into_iter()
        .find_map(|(prefix, library)| {
            (name.starts_with(prefix) && name.contains(".dylib")).then_some(library)
        })
    }

    fn write_framework_module(framework: &Path, name: &str) -> Result<(), IosError> {
        write_text_file(
            &framework.join("Headers").join(format!("{name}.h")),
            &format!("/* Binary distribution marker for {name}. */\n"),
        )?;
        write_text_file(
            &framework.join("Modules/module.modulemap"),
            &format!(
                "framework module {name} {{\n  umbrella header \"{name}.h\"\n  export *\n  module * {{ export * }}\n}}\n"
            ),
        )
    }

    fn read_ios_plugin_descriptor(
        root: &Path,
        plugin_id: IosPluginId,
    ) -> Result<CanonicalPluginDescriptor, IosError> {
        let manifest = root
            .join("plugins")
            .join(plugin_id.as_str())
            .join("vesper-plugin.toml");
        let source = read_bounded_utf8(&manifest, MAX_PROJECT_FILE_BYTES, "iOS plugin manifest")?;
        let project = PluginProjectManifest::from_toml(&source).map_err(|error| {
            IosError::conformance(format!(
                "iOS plugin manifest '{}' is invalid: {error}",
                manifest.display()
            ))
        })?;
        project
            .descriptor()
            .clone()
            .canonicalize()
            .map_err(|error| {
                IosError::conformance(format!(
                    "iOS plugin manifest '{}' is invalid: {error}",
                    manifest.display()
                ))
            })
    }

    pub(super) fn canonical_registry_fragment(
        root: &Path,
        plugin_id: IosPluginId,
        slice: IosPluginSlice,
        minimum_os: &str,
    ) -> Result<Vec<u8>, IosError> {
        let descriptor = read_ios_plugin_descriptor(root, plugin_id)?;
        ios_plugin_registry_fragment(&descriptor, plugin_id, slice, minimum_os)
            .map(|fragment| fragment.canonical_json().to_vec())
    }

    fn ios_plugin_registry_fragment(
        descriptor: &CanonicalPluginDescriptor,
        plugin_id: IosPluginId,
        slice: IosPluginSlice,
        minimum_os: &str,
    ) -> Result<EmbeddedRegistryFragment, IosError> {
        let plugin = plugin_id.spec();
        EmbeddedRegistryFragment::generate(
            descriptor,
            &EmbeddedRegistryTarget::AppleFramework {
                target: slice.rust_target().to_owned(),
                architecture: "arm64".to_owned(),
                minimum_os: minimum_os.to_owned(),
                framework_name: plugin.framework_name.to_owned(),
                bundle_identifier: plugin.bundle_identifier.to_owned(),
            },
        )
        .map_err(|error| {
            IosError::conformance(format!(
                "failed to generate {} registry fragment: {error}",
                plugin.description
            ))
        })
    }

    fn write_embedded_plugin_registry_fragment(
        descriptor: &CanonicalPluginDescriptor,
        plugin_id: IosPluginId,
        slice: IosPluginSlice,
        minimum_os: &str,
        framework: &Path,
    ) -> Result<(), IosError> {
        let fragment = ios_plugin_registry_fragment(descriptor, plugin_id, slice, minimum_os)?;
        let destination = framework.join(EMBEDDED_PLUGIN_REGISTRY_FILE);
        fs::write(&destination, fragment.canonical_json()).map_err(|error| {
            IosError::storage(format!(
                "failed to write iOS plugin registry fragment '{}': {error}",
                destination.display()
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn write_framework_plist(
        output: &Path,
        framework_name: &str,
        bundle_identifier: &str,
        platform: &str,
        version: &str,
        build: &str,
        minimum_os: &str,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let sdk = match platform {
            "iPhoneOS" => "iphoneos",
            "iPhoneSimulator" => "iphonesimulator",
            _ => return Err(IosError::worker("invalid internal iOS framework platform")),
        };
        let sdk_version = optional_tool_value(
            &tools.xcodebuild,
            &["-sdk", sdk, "-version", "SDKVersion"],
            cancellation,
        )?;
        let sdk_build = optional_tool_value(
            &tools.xcodebuild,
            &["-sdk", sdk, "-version", "ProductBuildVersion"],
            cancellation,
        )?;
        let xcode = optional_tool_value(&tools.xcodebuild, &["-version"], cancellation)?;
        let xcode_version = xcode
            .lines()
            .find_map(|line| line.strip_prefix("Xcode "))
            .map(str::to_owned);
        let xcode_build = xcode
            .lines()
            .find_map(|line| line.strip_prefix("Build version "))
            .map(str::to_owned);
        let machine_build = optional_tool_value(
            Path::new("/usr/bin/sw_vers"),
            &["-buildVersion"],
            cancellation,
        )?;
        let mut values = serde_json::Map::new();
        for (key, value) in [
            ("CFBundleDevelopmentRegion", "en".to_owned()),
            ("CFBundleExecutable", framework_name.to_owned()),
            ("CFBundleIdentifier", bundle_identifier.to_owned()),
            ("CFBundleInfoDictionaryVersion", "6.0".to_owned()),
            ("CFBundleName", framework_name.to_owned()),
            ("CFBundlePackageType", "FMWK".to_owned()),
            ("CFBundleShortVersionString", version.to_owned()),
            ("CFBundleVersion", build.to_owned()),
            ("MinimumOSVersion", minimum_os.to_owned()),
            (
                "DTCompiler",
                "com.apple.compilers.llvm.clang.1_0".to_owned(),
            ),
            ("DTPlatformName", sdk.to_owned()),
            ("DTSDKName", format!("{sdk}{sdk_version}")),
        ] {
            values.insert(key.to_owned(), serde_json::Value::String(value));
        }
        for (key, value) in [
            ("BuildMachineOSBuild", machine_build),
            ("DTPlatformBuild", sdk_build.clone()),
            ("DTPlatformVersion", sdk_version),
            ("DTSDKBuild", sdk_build),
            ("DTXcodeBuild", xcode_build.unwrap_or_default()),
        ] {
            if !value.is_empty() {
                values.insert(key.to_owned(), serde_json::Value::String(value));
            }
        }
        if let Some(version) = xcode_version {
            values.insert(
                "DTXcode".to_owned(),
                serde_json::Value::String(format_xcode_version(&version)?),
            );
        }
        values.insert(
            "CFBundleSupportedPlatforms".to_owned(),
            serde_json::json!([platform]),
        );
        values.insert("UIDeviceFamily".to_owned(), serde_json::json!([1, 2]));
        let json_path = output.with_extension("plist.json");
        let bytes = serde_json::to_vec(&values).map_err(|error| {
            IosError::conformance(format!("failed to serialize framework plist: {error}"))
        })?;
        fs::write(&json_path, bytes).map_err(|error| {
            IosError::storage(format!(
                "failed to write staged framework plist JSON '{}': {error}",
                json_path.display()
            ))
        })?;
        let mut command = Command::new(&tools.plutil);
        command
            .args(["-convert", "xml1", "-o"])
            .arg(output)
            .arg(&json_path);
        let result = run_required_command(
            &mut command,
            "iOS plugin framework plist conversion",
            diagnostics,
            cancellation,
        );
        let cleanup = fs::remove_file(&json_path);
        result?;
        cleanup.map_err(|error| {
            IosError::storage(format!(
                "failed to remove staged framework plist JSON '{}': {error}",
                json_path.display()
            ))
        })?;
        validate_regular_file(
            output,
            MAX_METADATA_BYTES,
            "iOS plugin framework Info.plist",
        )
    }

    fn optional_tool_value(
        executable: &Path,
        arguments: &[&str],
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<String, IosError> {
        let mut command = Command::new(executable);
        command.args(arguments).stdin(Stdio::null());
        match external_process::run_interruptible_capture_in_deferral(
            &mut command,
            "Apple framework metadata query",
            MAX_XCFRAMEWORK_PLIST_BYTES,
            MAX_XCFRAMEWORK_PLIST_BYTES,
            cancellation,
        ) {
            Ok(result) if result.status.success() => {
                Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
            }
            Ok(_) => Ok(String::new()),
            Err(error) if error.kind() == ExternalProcessErrorKind::Compatibility => {
                Ok(String::new())
            }
            Err(error) => Err(map_external_process_error(error)),
        }
    }

    fn format_xcode_version(version: &str) -> Result<String, IosError> {
        let mut components = version.split('.');
        let major = components.next().unwrap_or_default();
        let minor = components.next().unwrap_or("0");
        if major.is_empty()
            || !major.bytes().all(|byte| byte.is_ascii_digit())
            || !minor.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(IosError::conformance(format!(
                "xcodebuild returned an invalid Xcode version: {version}"
            )));
        }
        Ok(format!("{major}{minor}0"))
    }

    fn validate_framework(
        plugin_id: IosPluginId,
        plugin_descriptor: &CanonicalPluginDescriptor,
        request: &ReleaseRequest,
        slice: IosPluginSlice,
        framework: &Path,
        ffmpeg_snapshot: Option<&FfmpegSnapshot>,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        let mut expected = BTreeSet::from([
            OsString::from("Headers"),
            OsString::from("Info.plist"),
            OsString::from("Modules"),
            OsString::from(plugin.framework_name),
            OsString::from(EMBEDDED_PLUGIN_REGISTRY_FILE),
        ]);
        if plugin.uses_ffmpeg {
            expected.extend([
                OsString::from("binary-sha256.txt"),
                OsString::from("input-fingerprint.txt"),
                OsString::from(metadata_file_name(slice)),
                OsString::from("profile-hash.txt"),
            ]);
        }
        if read_directory_names(framework, "iOS plugin framework", MAX_RELEASE_TREE_ENTRIES)?
            != expected
        {
            return Err(IosError::conformance(format!(
                "iOS plugin framework has an unexpected payload: {}",
                framework.display()
            )));
        }
        let expected_header =
            BTreeSet::from([OsString::from(format!("{}.h", plugin.framework_name))]);
        if read_directory_names(&framework.join("Headers"), "framework Headers", 2)?
            != expected_header
            || read_directory_names(&framework.join("Modules"), "framework Modules", 2)?
                != BTreeSet::from([OsString::from("module.modulemap")])
        {
            return Err(IosError::conformance(
                "iOS plugin framework module payload is invalid",
            ));
        }
        validate_regular_file(
            &framework.join(plugin.framework_name),
            MAX_BINARY_BYTES,
            "iOS plugin framework binary",
        )?;
        let registry_path = framework.join(EMBEDDED_PLUGIN_REGISTRY_FILE);
        let registry_bytes = read_bounded_bytes(
            &registry_path,
            MAX_METADATA_BYTES,
            "iOS plugin registry fragment",
        )?;
        let expected_registry =
            ios_plugin_registry_fragment(plugin_descriptor, plugin_id, slice, &request.minimum_os)?;
        if registry_bytes != expected_registry.canonical_json() {
            return Err(IosError::conformance(format!(
                "iOS plugin registry fragment does not match the canonical plugin descriptor: {}",
                registry_path.display()
            )));
        }
        if let Some(ffmpeg) = &request.ffmpeg {
            let checksum = read_bounded_utf8(
                &framework.join("binary-sha256.txt"),
                MAX_METADATA_BYTES,
                "iOS plugin binary checksum",
            )?;
            if checksum.trim_end()
                != sha256_file(&framework.join(plugin.framework_name), MAX_BINARY_BYTES)?
            {
                return Err(IosError::conformance(
                    "iOS plugin framework binary checksum does not match its binary",
                ));
            }
            let profile = read_bounded_utf8(
                &framework.join("profile-hash.txt"),
                MAX_METADATA_BYTES,
                "iOS plugin profile hash",
            )?;
            if profile.trim_end() != ffmpeg.profile_hash {
                return Err(IosError::conformance(
                    "iOS plugin framework profile hash does not match its release profile",
                ));
            }
            let expected_fingerprint = ffmpeg_snapshot
                .and_then(|snapshot| snapshot.slices.get(&slice))
                .map(|snapshot| snapshot.input_fingerprint.as_str())
                .ok_or_else(|| {
                    IosError::worker("FFmpeg-backed iOS plugin validation has no input snapshot")
                })?;
            let fingerprint = read_bounded_utf8(
                &framework.join("input-fingerprint.txt"),
                MAX_METADATA_BYTES,
                "iOS plugin input fingerprint",
            )?;
            if fingerprint.strip_suffix('\n').unwrap_or(&fingerprint) != expected_fingerprint {
                return Err(IosError::conformance(
                    "iOS plugin framework input fingerprint does not match its release snapshot",
                ));
            }
        }
        Ok(())
    }

    fn verify_exact_arm64(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(&tools.lipo);
        command.args(["-archs"]).arg(binary);
        let result = run_process(
            &mut command,
            "iOS plugin architecture inspection",
            diagnostics,
            cancellation,
        )?;
        let architectures = parse_process_utf8(&result.stdout, "lipo architecture output")?
            .split_ascii_whitespace()
            .collect::<BTreeSet<_>>();
        if architectures != BTreeSet::from(["arm64"]) {
            return Err(IosError::conformance(format!(
                "iOS plugin binary must contain exactly arm64, found: {}",
                architectures.into_iter().collect::<Vec<_>>().join(" ")
            )));
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "XCFramework validation keeps the artifact, descriptor, toolchain, diagnostics, and cancellation inputs explicit"
    )]
    fn validate_xcframework(
        plugin_id: IosPluginId,
        plugin_descriptor: &CanonicalPluginDescriptor,
        request: &ReleaseRequest,
        xcframework: &Path,
        ffmpeg_snapshot: Option<&FfmpegSnapshot>,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        let mut expected_root = request
            .slices
            .iter()
            .map(|slice| OsString::from(xcframework_slice_identifier(*slice)))
            .collect::<BTreeSet<_>>();
        expected_root.insert(OsString::from("Info.plist"));
        if read_directory_names(
            xcframework,
            "iOS plugin XCFramework",
            MAX_RELEASE_TREE_ENTRIES,
        )? != expected_root
        {
            return Err(IosError::conformance(
                "iOS plugin XCFramework has an unexpected root payload",
            ));
        }
        let mut command = Command::new(&tools.plutil);
        command
            .args(["-convert", "json", "-o", "-"])
            .arg(xcframework.join("Info.plist"));
        let result = run_process(
            &mut command,
            "iOS plugin XCFramework manifest conversion",
            diagnostics,
            cancellation,
        )?;
        if result.stdout.len() > MAX_XCFRAMEWORK_PLIST_BYTES {
            return Err(IosError::conformance(
                "iOS plugin XCFramework manifest exceeds its schema limit",
            ));
        }
        let manifest: XcframeworkManifest =
            serde_json::from_slice(&result.stdout).map_err(|error| {
                IosError::conformance(format!(
                    "iOS plugin XCFramework manifest is invalid: {error}"
                ))
            })?;
        if manifest.format_version != "1.0"
            || manifest.available_libraries.len() != request.slices.len()
        {
            return Err(IosError::conformance(
                "iOS plugin XCFramework manifest has an unsupported format or slice count",
            ));
        }
        let mut found = BTreeSet::new();
        for library in manifest.available_libraries {
            let slice = request
                .slices
                .iter()
                .copied()
                .find(|slice| xcframework_slice_identifier(*slice) == library.library_identifier)
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "iOS plugin XCFramework contains an unexpected slice: {}",
                        library.library_identifier
                    ))
                })?;
            if !found.insert(slice)
                || library.library_path != format!("{}.framework", plugin.framework_name)
                || library.binary_path
                    != format!(
                        "{}.framework/{}",
                        plugin.framework_name, plugin.framework_name
                    )
                || library.supported_architectures != ["arm64"]
                || library.supported_platform != "ios"
                || library.supported_platform_variant.as_deref()
                    != match slice {
                        IosPluginSlice::DeviceArm64 => None,
                        IosPluginSlice::SimulatorArm64 => Some("simulator"),
                    }
            {
                return Err(IosError::conformance(format!(
                    "iOS plugin XCFramework manifest entry is invalid for {}",
                    library.library_identifier
                )));
            }
            let slice_root = xcframework.join(&library.library_identifier);
            if read_directory_names(&slice_root, "XCFramework slice", 2)?
                != BTreeSet::from([OsString::from(&library.library_path)])
            {
                return Err(IosError::conformance(
                    "iOS plugin XCFramework slice has an unexpected payload",
                ));
            }
            let framework = slice_root.join(&library.library_path);
            validate_framework(
                plugin_id,
                plugin_descriptor,
                request,
                slice,
                &framework,
                ffmpeg_snapshot,
            )?;
            verify_exact_arm64(
                &framework.join(plugin.framework_name),
                tools,
                diagnostics,
                cancellation,
            )?;
        }
        if found.len() != request.slices.len() {
            return Err(IosError::conformance(
                "iOS plugin XCFramework is missing a requested slice",
            ));
        }
        Ok(())
    }

    fn validate_plugin_archive_registry_fragments(
        archive: &Path,
        plugin_id: IosPluginId,
        plugin_descriptor: &CanonicalPluginDescriptor,
        request: &ReleaseRequest,
    ) -> Result<(), IosError> {
        let plugin = plugin_id.spec();
        for slice in &request.slices {
            let path = format!(
                "{}.xcframework/{}/{}.framework/{EMBEDDED_PLUGIN_REGISTRY_FILE}",
                plugin.framework_name,
                xcframework_slice_identifier(*slice),
                plugin.framework_name,
            );
            let actual = read_zip_record(archive, &path, MAX_METADATA_BYTES)?;
            let expected = ios_plugin_registry_fragment(
                plugin_descriptor,
                plugin_id,
                *slice,
                &request.minimum_os,
            )?;
            if actual != expected.canonical_json() {
                return Err(IosError::conformance(format!(
                    "iOS plugin archive registry fragment does not match the canonical plugin descriptor: {path}"
                )));
            }
        }
        Ok(())
    }

    fn create_zip(
        xcframework: &Path,
        output: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(&tools.ditto);
        command
            .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
            .arg(xcframework)
            .arg(output);
        run_required_command(
            &mut command,
            "iOS plugin XCFramework archive creation",
            diagnostics,
            cancellation,
        )?;
        validate_regular_file(
            output,
            MAX_RELEASE_ZIP_BYTES,
            "iOS plugin XCFramework archive",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn promote_release_outputs(
        root: &Path,
        plugin_id: IosPluginId,
        mut prepared: PreparedOwnersGuard,
        staged_xcframework: &Path,
        canonical_xcframework: &Path,
        output_directory: &Path,
        plugin_framework: &str,
        runtime_zips: &[PathBuf],
        runtime_promotion_source: Option<&Path>,
        runtime_source_snapshots: &[RuntimeArchiveSourceSnapshot],
        cancellation: &external_process::InterruptDeferral,
        journal_path: &Path,
    ) -> Result<(), IosError> {
        let canonical_parent = canonical_xcframework.parent().ok_or_else(|| {
            IosError::worker("canonical iOS plugin XCFramework path has no parent")
        })?;
        fs::create_dir_all(canonical_parent).map_err(|error| {
            IosError::storage(format!(
                "failed to create canonical iOS plugin build directory '{}': {error}",
                canonical_parent.display()
            ))
        })?;
        let canonical_parent =
            canonical_directory(canonical_parent, "canonical iOS plugin XCFramework parent")?;
        if canonical_xcframework.parent() != Some(canonical_parent.as_path()) {
            return Err(IosError::storage(
                "canonical iOS plugin XCFramework parent resolves through an unsupported alias",
            ));
        }
        sync_directory(&canonical_parent)?;
        let build_parent = prepared.work_path().parent().ok_or_else(|| {
            IosError::worker("iOS plugin release staging directory has no parent")
        })?;
        if directory_identity(build_parent, "iOS plugin release build parent")?
            != prepared.journal.build_parent_identity
        {
            return Err(IosError::storage(
                "iOS plugin release build parent changed before promotion",
            ));
        }
        sync_directory(build_parent)?;
        sync_directory(output_directory)?;
        if let Some(parent) = output_directory.parent() {
            sync_directory(parent)?;
        }

        let work_owner = prepared.work_path().to_path_buf();
        let asset_owner = prepared.assets_path().to_path_buf();
        let mut records = Vec::new();
        sync_promotion_source(
            staged_xcframework,
            PromotionNodeKind::Directory,
            cancellation,
        )?;
        records.push(prepare_promotion_record(
            staged_xcframework,
            canonical_xcframework,
            &work_owner,
            PromotionNodeKind::Directory,
            cancellation,
        )?);

        let plugin_zip_name = format!("{plugin_framework}.xcframework.zip");
        let plugin_zip_source = asset_owner.join(&plugin_zip_name);
        sync_promotion_source(&plugin_zip_source, PromotionNodeKind::File, cancellation)?;
        records.push(prepare_promotion_record(
            &plugin_zip_source,
            &output_directory.join(&plugin_zip_name),
            &asset_owner,
            PromotionNodeKind::File,
            cancellation,
        )?);
        let mut sorted_runtime_zips = runtime_zips.to_vec();
        sorted_runtime_zips.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        for target in &sorted_runtime_zips {
            let name = target.file_name().ok_or_else(|| {
                IosError::worker("iOS FFmpeg runtime zip target has no file name")
            })?;
            let source = asset_owner.join(name);
            sync_promotion_source(&source, PromotionNodeKind::File, cancellation)?;
            let record = prepare_promotion_record(
                &source,
                target,
                &asset_owner,
                PromotionNodeKind::File,
                cancellation,
            )?;
            if let Some(original) = runtime_source_snapshots
                .iter()
                .find(|snapshot| snapshot.path == *target)
                && record.old.as_ref() != Some(&original.snapshot)
            {
                return Err(IosError::storage(format!(
                    "iOS FFmpeg runtime archive changed after it was snapshotted: {}",
                    target.display()
                )));
            }
            records.push(record);
        }
        if let Some(source) = runtime_promotion_source {
            let target = prepared.journal.build_parent.join("player-ffmpeg-runtime");
            sync_promotion_source(source, PromotionNodeKind::Directory, cancellation)?;
            records.push(prepare_promotion_record(
                source,
                &target,
                &work_owner,
                PromotionNodeKind::Directory,
                cancellation,
            )?);
        }
        if records.len() > MAX_PROMOTION_RECORDS {
            return Err(IosError::conformance(format!(
                "iOS plugin release has more than {MAX_PROMOTION_RECORDS} promotion records"
            )));
        }
        sync_directory(&work_owner)?;
        sync_directory(&asset_owner)?;
        check_cancellation(cancellation, "iOS plugin release commit")?;

        let canonical_root = canonical_directory(root, "iOS plugin release repository")?;
        let state_directory = journal_path
            .parent()
            .ok_or_else(|| IosError::worker("iOS plugin release journal path has no parent"))?;
        if directory_identity(state_directory, "iOS plugin release state directory")?
            != prepared.journal.state_directory_identity
        {
            return Err(IosError::storage(
                "iOS plugin release state directory changed before journal creation",
            ));
        }
        let mut journal = PromotionJournal {
            version: PROMOTION_JOURNAL_VERSION,
            transaction_id: prepared.journal.transaction_id,
            root: canonical_root.clone(),
            root_identity: directory_identity(&canonical_root, "iOS plugin release repository")?,
            state_directory: state_directory.to_path_buf(),
            state_directory_identity: prepared.journal.state_directory_identity,
            build_parent: build_parent.to_path_buf(),
            build_parent_identity: prepared.journal.build_parent_identity,
            output_directory: output_directory.to_path_buf(),
            output_directory_identity: prepared.journal.output_directory_identity,
            plugin_id: plugin_id.as_str().to_owned(),
            decision: PromotionDecision::Rollback,
            records,
        };
        validate_promotion_journal(root, &journal)?;
        for record in &journal.records {
            preflight_promotion_record(record, PromotionDecision::Rollback)?;
        }
        preflight_promotion_owners(&journal)?;

        let promotion_journal_identity =
            match persist_promotion_journal(journal_path, &journal, None) {
                Ok(identity) => identity,
                Err(failure) => {
                    if failure.publication_may_be_visible() {
                        prepared.handoff_to_promotion();
                    }
                    return Err(failure.into_error());
                }
            };
        let prepared_path = prepared.journal_path.clone();
        let prepared_expectation = prepared.expectation();
        prepared.handoff_to_promotion();

        for (index, record) in journal.records.iter().enumerate() {
            if let Err(error) = check_cancellation(cancellation, "iOS plugin release commit")
                .and_then(|()| apply_promotion_record(record))
                .and_then(|()| run_after_promotion_test_hook(index + 1, cancellation))
            {
                return Err(rollback_promotion_error(
                    root,
                    journal_path,
                    &prepared_path,
                    prepared_expectation,
                    error,
                ));
            }
        }
        for record in &journal.records {
            match classify_promotion_record(record) {
                Ok(PromotionPlacement::After) => {}
                Ok(_) => {
                    return Err(rollback_promotion_error(
                        root,
                        journal_path,
                        &prepared_path,
                        prepared_expectation,
                        IosError::storage(
                            "iOS plugin release outputs changed before their durable commit decision",
                        ),
                    ));
                }
                Err(error) => {
                    return Err(rollback_promotion_error(
                        root,
                        journal_path,
                        &prepared_path,
                        prepared_expectation,
                        error,
                    ));
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(rollback_promotion_error(
                root,
                journal_path,
                &prepared_path,
                prepared_expectation,
                IosError::worker("iOS plugin release commit was cancelled"),
            ));
        }

        journal.decision = PromotionDecision::Commit;
        match persist_promotion_journal(journal_path, &journal, Some(promotion_journal_identity)) {
            Ok(_) => {}
            Err(JournalPersistenceFailure::BeforePublish(error)) => {
                return Err(rollback_promotion_error(
                    root,
                    journal_path,
                    &prepared_path,
                    prepared_expectation,
                    error,
                ));
            }
            Err(JournalPersistenceFailure::AfterPublish(error)) => {
                return Err(append_error(
                    error,
                    "the commit decision may be visible but is not confirmed durable; recovery is required before another iOS plugin release",
                ));
            }
        }

        let hook_error = run_after_durable_commit_test_hook(cancellation).err();
        let recovery =
            recover_release_transactions(root, journal_path, &prepared_path, prepared_expectation);
        match (hook_error, recovery) {
            (None, Ok(())) => Ok(()),
            (Some(error), Ok(())) => Err(error),
            (None, Err(error)) => Err(error),
            (Some(error), Err(recovery)) => Err(append_error(error, recovery.to_string())),
        }
    }

    fn prepare_promotion_record(
        source: &Path,
        target: &Path,
        owner: &Path,
        node_kind: PromotionNodeKind,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<PromotionRecord, IosError> {
        let parent = target.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin release target '{}' has no parent",
                target.display()
            ))
        })?;
        let parent_identity = directory_identity(parent, "iOS plugin release target parent")?;
        let owner_identity = directory_identity(owner, "iOS plugin release staging owner")?;
        let new = promotion_node_snapshot(
            source,
            node_kind,
            "staged iOS plugin release output",
            Some(cancellation),
        )?;
        sync_promotion_source(source, node_kind, cancellation)?;
        let durable_new = promotion_node_snapshot(
            source,
            node_kind,
            "staged iOS plugin release output",
            Some(cancellation),
        )?;
        if durable_new != new {
            return Err(IosError::storage(format!(
                "staged iOS plugin release output '{}' changed while it was synchronized",
                source.display()
            )));
        }
        let old = optional_promotion_node_snapshot(
            target,
            node_kind,
            "existing iOS plugin release output",
            Some(cancellation),
        )?;
        if old.as_ref().is_some_and(|old| old.identity == new.identity) {
            return Err(IosError::storage(format!(
                "iOS plugin release source and target share one filesystem identity: {}",
                target.display()
            )));
        }
        Ok(PromotionRecord {
            parent: parent.to_path_buf(),
            parent_identity,
            target: target.to_path_buf(),
            source: source.to_path_buf(),
            owner: owner.to_path_buf(),
            owner_identity,
            node_kind,
            old,
            new,
        })
    }

    fn apply_promotion_record(record: &PromotionRecord) -> Result<(), IosError> {
        if classify_promotion_record(record)? != PromotionPlacement::Before {
            return Err(IosError::storage(format!(
                "iOS plugin release target '{}' is not ready for promotion",
                record.target.display()
            )));
        }
        if record.old.is_some() {
            exchange_paths(&record.source, &record.target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically exchange iOS plugin release target '{}': {error}",
                    record.target.display()
                ))
            })?;
        } else {
            rename_noreplace(&record.source, &record.target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS plugin release target '{}': {error}",
                    record.target.display()
                ))
            })?;
        }
        sync_promotion_record_parents(record)?;
        if classify_promotion_record(record)? == PromotionPlacement::After {
            Ok(())
        } else {
            Err(IosError::storage(format!(
                "iOS plugin release target '{}' changed during promotion",
                record.target.display()
            )))
        }
    }

    fn rollback_promotion_record(record: &PromotionRecord) -> Result<(), IosError> {
        if classify_promotion_record(record)? != PromotionPlacement::After {
            return Err(IosError::storage(format!(
                "iOS plugin release target '{}' is not ready for rollback",
                record.target.display()
            )));
        }
        if record.old.is_some() {
            exchange_paths(&record.source, &record.target).map_err(|error| {
                IosError::storage(format!(
                    "failed to restore previous iOS plugin release target '{}': {error}",
                    record.target.display()
                ))
            })?;
        } else {
            rename_noreplace(&record.target, &record.source).map_err(|error| {
                IosError::storage(format!(
                    "failed to remove newly published iOS plugin release target '{}': {error}",
                    record.target.display()
                ))
            })?;
        }
        sync_promotion_record_parents(record)?;
        if classify_promotion_record(record)? == PromotionPlacement::Before {
            Ok(())
        } else {
            Err(IosError::storage(format!(
                "iOS plugin release target '{}' changed during rollback",
                record.target.display()
            )))
        }
    }

    fn sync_promotion_record_parents(record: &PromotionRecord) -> Result<(), IosError> {
        let mut parents = BTreeSet::from([record.parent.as_path()]);
        if let Some(source_parent) = record.source.parent() {
            parents.insert(source_parent);
        }
        for parent in parents {
            sync_directory(parent)?;
        }
        Ok(())
    }

    #[derive(Debug)]
    enum JournalPersistenceFailure {
        BeforePublish(IosError),
        AfterPublish(IosError),
    }

    impl JournalPersistenceFailure {
        fn publication_may_be_visible(&self) -> bool {
            matches!(self, Self::AfterPublish(_))
        }

        fn into_error(self) -> IosError {
            match self {
                Self::BeforePublish(error) | Self::AfterPublish(error) => error,
            }
        }
    }

    fn persist_journal_bytes(
        path: &Path,
        bytes: &[u8],
        maximum_bytes: u64,
        description: &str,
        temporary_prefix: &str,
        replace_identity: Option<FileIdentity>,
    ) -> Result<FileIdentity, JournalPersistenceFailure> {
        let parent = path.parent().ok_or_else(|| {
            JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                "{description} '{}' has no parent",
                path.display()
            )))
        })?;
        if bytes.len() as u64 > maximum_bytes {
            return Err(JournalPersistenceFailure::BeforePublish(IosError::worker(
                format!("{description} exceeds {maximum_bytes} bytes"),
            )));
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(temporary_prefix)
            .tempfile_in(parent)
            .map_err(|error| {
                JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                    "failed to create {description} beside '{}': {error}",
                    path.display()
                )))
            })?;
        temporary.write_all(bytes).map_err(|error| {
            JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                "failed to write {description} '{}': {error}",
                temporary.path().display()
            )))
        })?;
        temporary.as_file_mut().sync_all().map_err(|error| {
            JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                "failed to sync {description} '{}': {error}",
                temporary.path().display()
            )))
        })?;
        let temporary_identity = temporary
            .as_file()
            .metadata()
            .map(|metadata| metadata_identity(&metadata))
            .map_err(|error| {
                JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                    "failed to inspect temporary {description} '{}': {error}",
                    temporary.path().display()
                )))
            })?;

        let Some(expected_identity) = replace_identity else {
            temporary.persist_noclobber(path).map_err(|error| {
                JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                    "failed to create {description} '{}': {}",
                    path.display(),
                    error.error
                )))
            })?;
            let published =
                file_identity_if_regular(path).map_err(JournalPersistenceFailure::AfterPublish)?;
            if published != Some(temporary_identity) {
                return Err(JournalPersistenceFailure::AfterPublish(IosError::storage(
                    format!(
                        "published {description} '{}' changed before verification",
                        path.display()
                    ),
                )));
            }
            sync_directory(parent).map_err(JournalPersistenceFailure::AfterPublish)?;
            return Ok(temporary_identity);
        };

        let temporary_path = temporary.path().to_path_buf();
        exchange_paths(&temporary_path, path).map_err(|error| {
            JournalPersistenceFailure::BeforePublish(IosError::storage(format!(
                "failed to atomically replace {description} '{}': {error}",
                path.display()
            )))
        })?;
        let published = file_identity_if_regular(path);
        let displaced = file_identity_if_regular(&temporary_path);
        if published
            .as_ref()
            .is_ok_and(|identity| *identity == Some(temporary_identity))
            && displaced
                .as_ref()
                .is_ok_and(|identity| *identity == Some(expected_identity))
        {
            temporary.close().map_err(|error| {
                JournalPersistenceFailure::AfterPublish(IosError::storage(format!(
                    "failed to remove the displaced {description} after replacing '{}': {error}",
                    path.display()
                )))
            })?;
            if file_identity_if_regular(path).map_err(JournalPersistenceFailure::AfterPublish)?
                != Some(temporary_identity)
            {
                return Err(JournalPersistenceFailure::AfterPublish(IosError::storage(
                    format!(
                        "published {description} '{}' changed before durability sync",
                        path.display()
                    ),
                )));
            }
            sync_directory(parent).map_err(JournalPersistenceFailure::AfterPublish)?;
            return Ok(temporary_identity);
        }

        let can_restore = file_identity_if_regular(path)
            .is_ok_and(|identity| identity == Some(temporary_identity));
        if can_restore && exchange_paths(&temporary_path, path).is_ok() {
            let restored_temporary = file_identity_if_regular(&temporary_path);
            let restored_canonical = fs::symlink_metadata(path);
            if restored_temporary
                .as_ref()
                .is_ok_and(|identity| *identity == Some(temporary_identity))
                && restored_canonical.is_ok()
            {
                let restore_sync = sync_directory(parent);
                let close = temporary.close();
                if restore_sync.is_ok() && close.is_ok() {
                    return Err(JournalPersistenceFailure::BeforePublish(IosError::storage(
                        format!(
                            "{description} '{}' changed before atomic replacement",
                            path.display()
                        ),
                    )));
                }
            }
        }
        Err(JournalPersistenceFailure::AfterPublish(IosError::storage(
            format!(
                "{description} '{}' changed during atomic replacement; publication state is uncertain",
                path.display()
            ),
        )))
    }

    fn persist_prepared_owner_journal(
        path: &Path,
        journal: &PreparedOwnerJournal,
        replace_identity: Option<FileIdentity>,
    ) -> Result<FileIdentity, JournalPersistenceFailure> {
        let bytes = serde_json::to_vec_pretty(journal).map_err(|error| {
            JournalPersistenceFailure::BeforePublish(IosError::worker(format!(
                "failed to serialize iOS plugin prepared-owner journal: {error}"
            )))
        })?;
        persist_journal_bytes(
            path,
            &bytes,
            MAX_PREPARED_OWNER_JOURNAL_BYTES,
            "iOS plugin prepared-owner journal",
            ".ios-plugin-prepared-owner-journal-",
            replace_identity,
        )
    }

    struct LoadedPreparedOwnerJournal {
        journal: PreparedOwnerJournal,
        identity: FileIdentity,
    }

    fn read_prepared_owner_journal(
        path: &Path,
    ) -> Result<Option<LoadedPreparedOwnerJournal>, IosError> {
        restore_journal_removal(path, "iOS plugin prepared-owner journal")?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect iOS plugin prepared-owner journal '{}': {error}",
                    path.display()
                )));
            }
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_PREPARED_OWNER_JOURNAL_BYTES {
            return Err(IosError::storage(format!(
                "iOS plugin prepared-owner journal '{}' is not a bounded regular non-symlink file",
                path.display()
            )));
        }
        let identity = metadata_identity(&metadata);
        let file = open_regular_file_nofollow(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS plugin prepared-owner journal '{}': {error}",
                path.display()
            ))
        })?;
        let opened_metadata = file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened iOS plugin prepared-owner journal '{}': {error}",
                path.display()
            ))
        })?;
        if !opened_metadata.file_type().is_file()
            || metadata_identity(&opened_metadata) != identity
            || opened_metadata.len() != metadata.len()
        {
            return Err(IosError::storage(format!(
                "iOS plugin prepared-owner journal '{}' changed while it was opened",
                path.display()
            )));
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len()).unwrap_or(MAX_PREPARED_OWNER_JOURNAL_BYTES as usize),
        );
        file.take(MAX_PREPARED_OWNER_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read iOS plugin prepared-owner journal '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.len() as u64 > MAX_PREPARED_OWNER_JOURNAL_BYTES {
            return Err(IosError::storage(format!(
                "iOS plugin prepared-owner journal '{}' exceeds its size limit",
                path.display()
            )));
        }
        let final_metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect iOS plugin prepared-owner journal '{}': {error}",
                path.display()
            ))
        })?;
        if !final_metadata.file_type().is_file()
            || metadata_identity(&final_metadata) != identity
            || final_metadata.len() != metadata.len()
        {
            return Err(IosError::storage(format!(
                "iOS plugin prepared-owner journal '{}' changed while it was read",
                path.display()
            )));
        }
        serde_json::from_slice(&bytes)
            .map(|journal| Some(LoadedPreparedOwnerJournal { journal, identity }))
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to parse iOS plugin prepared-owner journal '{}': {error}",
                    path.display()
                ))
            })
    }

    fn recover_prepared_owner_journal(
        root: &Path,
        path: &Path,
        expectation: Option<PreparedOwnerExpectation>,
    ) -> Result<(), IosError> {
        let Some(loaded) = read_prepared_owner_journal(path)? else {
            return Ok(());
        };
        if expectation.is_some_and(|expectation| {
            expectation.journal_identity != loaded.identity
                || expectation.transaction_id != loaded.journal.transaction_id
        }) {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal changed before owned cleanup",
            ));
        }
        let missing_output_parent =
            validate_prepared_owner_journal_for_recovery(root, &loaded.journal)?;
        confirm_journal_durable(path, loaded.identity, "iOS plugin prepared-owner journal")?;
        for owner in &loaded.journal.owners {
            preflight_prepared_owner(owner, missing_output_parent)?;
        }
        for owner in &loaded.journal.owners {
            cleanup_prepared_owner(owner, missing_output_parent)?;
        }
        for owner in &loaded.journal.owners {
            if missing_output_parent && owner.role == PreparedOwnerRole::Assets {
                ensure_prepared_owner_parent_absent(owner)?;
                continue;
            }
            for path in [&owner.path, &prepared_owner_quarantine_path(owner)?] {
                match fs::symlink_metadata(path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(IosError::storage(format!(
                            "iOS plugin prepared staging owner '{}' remains after cleanup",
                            path.display()
                        )));
                    }
                    Err(error) => {
                        return Err(IosError::storage(format!(
                            "failed to verify iOS plugin prepared staging owner '{}': {error}",
                            path.display()
                        )));
                    }
                }
            }
        }
        remove_journal_durably(
            path,
            loaded.identity,
            loaded.journal.state_directory_identity,
            "iOS plugin prepared-owner journal",
        )
    }

    fn preflight_prepared_owner(
        owner: &PreparedOwner,
        missing_output_parent: bool,
    ) -> Result<(), IosError> {
        if missing_output_parent && owner.role == PreparedOwnerRole::Assets {
            return ensure_prepared_owner_parent_absent(owner);
        }
        if directory_identity(&owner.parent, "iOS plugin prepared staging parent")?
            != owner.parent_identity
        {
            return Err(IosError::storage(format!(
                "iOS plugin prepared staging parent '{}' changed identity",
                owner.parent.display()
            )));
        }
        let quarantine = prepared_owner_quarantine_path(owner)?;
        let current =
            optional_directory_metadata(&owner.path, "iOS plugin prepared staging owner")?;
        let quarantined = optional_directory_metadata(
            &quarantine,
            "iOS plugin prepared staging owner quarantine",
        )?;
        match owner.identity {
            Some(expected) => match (current, quarantined) {
                (None, None) => Ok(()),
                (Some(metadata), None) | (None, Some(metadata))
                    if metadata_identity(&metadata) == expected =>
                {
                    Ok(())
                }
                _ => Err(IosError::storage(format!(
                    "iOS plugin prepared staging owner '{}' has an unknown identity or placement",
                    owner.path.display()
                ))),
            },
            None => {
                if quarantined.is_some() {
                    return Err(IosError::storage(format!(
                        "uncommitted iOS plugin staging owner has an unexpected quarantine: {}",
                        quarantine.display()
                    )));
                }
                let Some(_) = current else {
                    return Ok(());
                };
                ensure_directory_empty(&owner.path, "uncommitted iOS plugin staging owner")
            }
        }
    }

    fn cleanup_prepared_owner(
        owner: &PreparedOwner,
        missing_output_parent: bool,
    ) -> Result<(), IosError> {
        if missing_output_parent && owner.role == PreparedOwnerRole::Assets {
            return ensure_prepared_owner_parent_absent(owner);
        }
        match owner.identity {
            Some(expected) => {
                let quarantine = prepared_owner_quarantine_path(owner)?;
                if optional_directory_metadata(
                    &quarantine,
                    "iOS plugin prepared staging owner quarantine",
                )?
                .is_none()
                    && optional_directory_metadata(
                        &owner.path,
                        "iOS plugin prepared staging owner",
                    )?
                    .is_some()
                {
                    rename_noreplace(&owner.path, &quarantine).map_err(|error| {
                        IosError::storage(format!(
                            "failed to quarantine iOS plugin prepared staging owner '{}': {error}",
                            owner.path.display()
                        ))
                    })?;
                    sync_directory(&owner.parent)?;
                }
                let Some(metadata) = optional_directory_metadata(
                    &quarantine,
                    "iOS plugin prepared staging owner quarantine",
                )?
                else {
                    sync_directory(&owner.parent)?;
                    return Ok(());
                };
                if metadata_identity(&metadata) != expected {
                    restore_quarantined_path(&quarantine, &owner.path, &owner.parent)?;
                    return Err(IosError::storage(format!(
                        "iOS plugin prepared staging owner '{}' changed while it was quarantined",
                        owner.path.display()
                    )));
                }
                remove_quarantined_owner_directory(
                    &quarantine,
                    &owner.parent,
                    owner.parent_identity,
                    expected,
                    "iOS plugin prepared staging owner",
                )?;
            }
            None => {
                let Some(_) = optional_directory_metadata(
                    &owner.path,
                    "uncommitted iOS plugin staging owner",
                )?
                else {
                    sync_directory(&owner.parent)?;
                    return Ok(());
                };
                ensure_directory_empty(&owner.path, "uncommitted iOS plugin staging owner")?;
                fs::remove_dir(&owner.path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to remove empty uncommitted iOS plugin staging owner '{}': {error}",
                        owner.path.display()
                    ))
                })?;
            }
        }
        sync_directory(&owner.parent)
    }

    fn ensure_prepared_owner_parent_absent(owner: &PreparedOwner) -> Result<(), IosError> {
        match fs::symlink_metadata(&owner.parent) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Ok(_) => Err(IosError::storage(format!(
                "iOS plugin prepared staging parent '{}' reappeared during recovery",
                owner.parent.display()
            ))),
            Err(error) => Err(IosError::storage(format!(
                "failed to confirm missing iOS plugin prepared staging parent '{}': {error}",
                owner.parent.display()
            ))),
        }
    }

    fn prepared_owner_quarantine_path(owner: &PreparedOwner) -> Result<PathBuf, IosError> {
        let name = owner.path.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin prepared staging owner '{}' has no file name",
                owner.path.display()
            ))
        })?;
        let mut quarantine_name = name.to_os_string();
        quarantine_name.push(".removing");
        Ok(owner.parent.join(quarantine_name))
    }

    fn optional_directory_metadata(
        path: &Path,
        label: &str,
    ) -> Result<Option<fs::Metadata>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(metadata)),
            Ok(_) => Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink directory",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn ensure_directory_empty(path: &Path, label: &str) -> Result<(), IosError> {
        let mut entries = fs::read_dir(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        match entries.next() {
            None => Ok(()),
            Some(Ok(_)) => Err(IosError::storage(format!(
                "{label} '{}' is not empty",
                path.display()
            ))),
            Some(Err(error)) => Err(IosError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn restore_quarantined_path(
        quarantine: &Path,
        original: &Path,
        parent: &Path,
    ) -> Result<(), IosError> {
        match rename_noreplace(quarantine, original) {
            Ok(()) => sync_directory(parent),
            Err(error) => Err(IosError::storage(format!(
                "failed to restore quarantined iOS plugin path '{}' to '{}': {error}",
                quarantine.display(),
                original.display()
            ))),
        }
    }

    #[derive(Debug, Default)]
    struct QuarantineCleanupBudget {
        entries: usize,
    }

    fn remove_quarantined_owner_directory(
        quarantine: &Path,
        parent: &Path,
        parent_identity: FileIdentity,
        expected_identity: FileIdentity,
        description: &str,
    ) -> Result<(), IosError> {
        use rustix::fs::{AtFlags, Mode, OFlags, fstat, fsync, open, openat, unlinkat};

        let name = quarantine.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "quarantined {description} '{}' has no file name",
                quarantine.display()
            ))
        })?;
        let parent_fd = open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            IosError::storage(format!(
                "failed to open quarantined {description} parent '{}': {error}",
                parent.display()
            ))
        })?;
        verify_opened_directory(
            &parent_fd,
            parent,
            parent_identity,
            &format!("quarantined {description} parent"),
        )?;
        let root_fd = openat(
            &parent_fd,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            IosError::storage(format!(
                "failed to open quarantined {description} '{}': {error}",
                quarantine.display()
            ))
        })?;
        verify_opened_directory(
            &root_fd,
            quarantine,
            expected_identity,
            &format!("quarantined {description}"),
        )?;
        run_recovery_cleanup_test_hook(quarantine)?;
        verify_opened_directory(
            &parent_fd,
            parent,
            parent_identity,
            &format!("quarantined {description} parent"),
        )?;
        verify_opened_directory(
            &root_fd,
            quarantine,
            expected_identity,
            &format!("quarantined {description}"),
        )?;
        verify_named_directory(&parent_fd, name, expected_identity, quarantine, description)?;

        let root_stat = fstat(&root_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened quarantined {description} '{}': {error}",
                quarantine.display()
            ))
        })?;
        let mut budget = QuarantineCleanupBudget::default();
        empty_opened_quarantine_directory(
            &root_fd,
            root_stat.st_dev as u64,
            0,
            &mut budget,
            quarantine,
            description,
        )?;

        verify_opened_directory(
            &parent_fd,
            parent,
            parent_identity,
            &format!("quarantined {description} parent"),
        )?;
        verify_opened_directory(
            &root_fd,
            quarantine,
            expected_identity,
            &format!("quarantined {description}"),
        )?;
        verify_named_directory(&parent_fd, name, expected_identity, quarantine, description)?;
        fsync(&root_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to sync emptied quarantined {description} '{}': {error}",
                quarantine.display()
            ))
        })?;
        unlinkat(&parent_fd, name, AtFlags::REMOVEDIR).map_err(|error| {
            IosError::storage(format!(
                "failed to remove emptied quarantined {description} '{}': {error}",
                quarantine.display()
            ))
        })?;
        fsync(&parent_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to sync quarantined {description} parent '{}': {error}",
                parent.display()
            ))
        })
    }

    fn verify_opened_directory(
        directory: &rustix::fd::OwnedFd,
        expected_path: &Path,
        expected_identity: FileIdentity,
        description: &str,
    ) -> Result<(), IosError> {
        use rustix::fs::{FileType, RawMode, fstat, getpath};

        let stat = fstat(directory).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened {description} '{}': {error}",
                expected_path.display()
            ))
        })?;
        if FileType::from_raw_mode(stat.st_mode as RawMode) != FileType::Directory
            || stat_identity(&stat) != expected_identity
        {
            return Err(IosError::storage(format!(
                "opened {description} '{}' changed identity",
                expected_path.display()
            )));
        }
        let opened = getpath(directory).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve opened {description} '{}': {error}",
                expected_path.display()
            ))
        })?;
        let opened = PathBuf::from(OsString::from_vec(opened.into_bytes()));
        if opened != expected_path {
            return Err(IosError::storage(format!(
                "opened {description} moved from '{}' to '{}' during cleanup",
                expected_path.display(),
                opened.display()
            )));
        }
        Ok(())
    }

    fn verify_named_directory(
        parent: &rustix::fd::OwnedFd,
        name: &OsStr,
        expected_identity: FileIdentity,
        path: &Path,
        description: &str,
    ) -> Result<(), IosError> {
        use rustix::fs::{AtFlags, FileType, RawMode, statat};

        let stat = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect quarantined {description} '{}': {error}",
                path.display()
            ))
        })?;
        if FileType::from_raw_mode(stat.st_mode as RawMode) != FileType::Directory
            || stat_identity(&stat) != expected_identity
        {
            return Err(IosError::storage(format!(
                "quarantined {description} '{}' changed before descriptor-relative cleanup",
                path.display()
            )));
        }
        Ok(())
    }

    fn empty_opened_quarantine_directory(
        directory: &rustix::fd::OwnedFd,
        root_device: u64,
        depth: usize,
        budget: &mut QuarantineCleanupBudget,
        root: &Path,
        description: &str,
    ) -> Result<(), IosError> {
        use rustix::fs::{
            AtFlags, Dir, FileType, Mode, OFlags, RawMode, fstat, openat, statat, unlinkat,
        };

        if depth > MAX_QUARANTINE_CLEANUP_DEPTH {
            return Err(IosError::storage(format!(
                "quarantined {description} '{}' exceeds cleanup depth {MAX_QUARANTINE_CLEANUP_DEPTH}",
                root.display()
            )));
        }
        for _ in 0..MAX_QUARANTINE_CLEANUP_PASSES {
            let mut saw_entry = false;
            let mut entries = Dir::read_from(directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to enumerate quarantined {description} '{}': {error}",
                    root.display()
                ))
            })?;
            for entry in &mut entries {
                let entry = entry.map_err(|error| {
                    IosError::storage(format!(
                        "failed to read quarantined {description} entry under '{}': {error}",
                        root.display()
                    ))
                })?;
                let name = entry.file_name();
                if matches!(name.to_bytes(), b"." | b"..") {
                    continue;
                }
                saw_entry = true;
                budget.entries = budget.entries.checked_add(1).ok_or_else(|| {
                    IosError::storage(format!(
                        "quarantined {description} cleanup entry count overflowed"
                    ))
                })?;
                if budget.entries > MAX_QUARANTINE_CLEANUP_ENTRIES {
                    return Err(IosError::storage(format!(
                        "quarantined {description} '{}' exceeds cleanup limit {MAX_QUARANTINE_CLEANUP_ENTRIES}",
                        root.display()
                    )));
                }
                let initial = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(stat) => stat,
                    Err(rustix::io::Errno::NOENT) => continue,
                    Err(error) => {
                        return Err(IosError::storage(format!(
                            "failed to inspect quarantined {description} entry '{}': {error}",
                            String::from_utf8_lossy(name.to_bytes())
                        )));
                    }
                };
                if initial.st_dev as u64 != root_device {
                    return Err(IosError::storage(format!(
                        "quarantined {description} contains a cross-device entry: {}",
                        String::from_utf8_lossy(name.to_bytes())
                    )));
                }
                let initial_kind = FileType::from_raw_mode(initial.st_mode as RawMode);
                if initial_kind == FileType::Directory {
                    let child = openat(
                        directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|error| {
                        IosError::storage(format!(
                            "failed to open quarantined {description} directory '{}': {error}",
                            String::from_utf8_lossy(name.to_bytes())
                        ))
                    })?;
                    let opened = fstat(&child).map_err(|error| {
                        IosError::storage(format!(
                            "failed to inspect opened quarantined {description} directory '{}': {error}",
                            String::from_utf8_lossy(name.to_bytes())
                        ))
                    })?;
                    if FileType::from_raw_mode(opened.st_mode as RawMode) != FileType::Directory
                        || stat_identity(&opened) != stat_identity(&initial)
                    {
                        return Err(IosError::storage(format!(
                            "quarantined {description} directory '{}' changed while it was opened",
                            String::from_utf8_lossy(name.to_bytes())
                        )));
                    }
                    empty_opened_quarantine_directory(
                        &child,
                        root_device,
                        depth + 1,
                        budget,
                        root,
                        description,
                    )?;
                    let current = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(
                        |error| {
                            IosError::storage(format!(
                                "failed to re-inspect quarantined {description} directory '{}': {error}",
                                String::from_utf8_lossy(name.to_bytes())
                            ))
                        },
                    )?;
                    if FileType::from_raw_mode(current.st_mode as RawMode) != FileType::Directory
                        || stat_identity(&current) != stat_identity(&initial)
                    {
                        return Err(IosError::storage(format!(
                            "quarantined {description} directory '{}' changed before removal",
                            String::from_utf8_lossy(name.to_bytes())
                        )));
                    }
                    match unlinkat(directory, name, AtFlags::REMOVEDIR) {
                        Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                        Err(rustix::io::Errno::NOTEMPTY) => continue,
                        Err(error) => {
                            return Err(IosError::storage(format!(
                                "failed to remove quarantined {description} directory '{}': {error}",
                                String::from_utf8_lossy(name.to_bytes())
                            )));
                        }
                    }
                } else {
                    let current = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(
                        |error| {
                            IosError::storage(format!(
                                "failed to re-inspect quarantined {description} entry '{}': {error}",
                                String::from_utf8_lossy(name.to_bytes())
                            ))
                        },
                    )?;
                    if FileType::from_raw_mode(current.st_mode as RawMode) != initial_kind
                        || stat_identity(&current) != stat_identity(&initial)
                    {
                        return Err(IosError::storage(format!(
                            "quarantined {description} entry '{}' changed before removal",
                            String::from_utf8_lossy(name.to_bytes())
                        )));
                    }
                    match unlinkat(directory, name, AtFlags::empty()) {
                        Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                        Err(error) => {
                            return Err(IosError::storage(format!(
                                "failed to remove quarantined {description} entry '{}': {error}",
                                String::from_utf8_lossy(name.to_bytes())
                            )));
                        }
                    }
                }
            }
            if !saw_entry {
                return Ok(());
            }
        }
        Err(IosError::storage(format!(
            "quarantined {description} '{}' did not become empty within {MAX_QUARANTINE_CLEANUP_PASSES} passes",
            root.display()
        )))
    }

    fn stat_identity(stat: &rustix::fs::Stat) -> FileIdentity {
        FileIdentity {
            device: stat.st_dev as u64,
            inode: stat.st_ino,
        }
    }

    fn file_identity_if_regular(path: &Path) -> Result<Option<FileIdentity>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                Ok(Some(metadata_identity(&metadata)))
            }
            Ok(_) => Err(IosError::storage(format!(
                "iOS plugin journal '{}' is not a regular non-symlink file",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect iOS plugin journal '{}': {error}",
                path.display()
            ))),
        }
    }

    fn persist_promotion_journal(
        path: &Path,
        journal: &PromotionJournal,
        replace_identity: Option<FileIdentity>,
    ) -> Result<FileIdentity, JournalPersistenceFailure> {
        let bytes = serde_json::to_vec_pretty(journal).map_err(|error| {
            JournalPersistenceFailure::BeforePublish(IosError::worker(format!(
                "failed to serialize iOS plugin release journal: {error}"
            )))
        })?;
        persist_journal_bytes(
            path,
            &bytes,
            MAX_PROMOTION_JOURNAL_BYTES,
            "iOS plugin release journal",
            ".ios-plugin-release-journal-",
            replace_identity,
        )
    }

    struct LoadedPromotionJournal {
        journal: PromotionJournal,
        identity: FileIdentity,
    }

    fn read_promotion_journal(path: &Path) -> Result<Option<LoadedPromotionJournal>, IosError> {
        restore_journal_removal(path, "iOS plugin release journal")?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect iOS plugin release journal '{}': {error}",
                    path.display()
                )));
            }
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_PROMOTION_JOURNAL_BYTES {
            return Err(IosError::storage(format!(
                "iOS plugin release journal '{}' is not a bounded regular non-symlink file",
                path.display()
            )));
        }
        let identity = metadata_identity(&metadata);
        let file = open_regular_file_nofollow(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS plugin release journal '{}': {error}",
                path.display()
            ))
        })?;
        let opened_metadata = file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened iOS plugin release journal '{}': {error}",
                path.display()
            ))
        })?;
        if !opened_metadata.file_type().is_file()
            || metadata_identity(&opened_metadata) != identity
            || opened_metadata.len() != metadata.len()
        {
            return Err(IosError::storage(format!(
                "iOS plugin release journal '{}' changed while it was opened",
                path.display()
            )));
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len()).unwrap_or(MAX_PROMOTION_JOURNAL_BYTES as usize),
        );
        file.take(MAX_PROMOTION_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read iOS plugin release journal '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.len() as u64 > MAX_PROMOTION_JOURNAL_BYTES {
            return Err(IosError::storage(format!(
                "iOS plugin release journal '{}' exceeds its size limit",
                path.display()
            )));
        }
        let final_metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect iOS plugin release journal '{}': {error}",
                path.display()
            ))
        })?;
        if !final_metadata.file_type().is_file()
            || metadata_identity(&final_metadata) != identity
            || final_metadata.len() != metadata.len()
        {
            return Err(IosError::storage(format!(
                "iOS plugin release journal '{}' changed while it was read",
                path.display()
            )));
        }
        serde_json::from_slice(&bytes)
            .map(|journal| Some(LoadedPromotionJournal { journal, identity }))
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to parse iOS plugin release journal '{}': {error}",
                    path.display()
                ))
            })
    }

    fn confirm_journal_durable(
        path: &Path,
        expected_identity: FileIdentity,
        description: &str,
    ) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {description} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata_identity(&metadata) != expected_identity {
            return Err(IosError::storage(format!(
                "{description} '{}' changed before durability confirmation",
                path.display()
            )));
        }
        let file = open_regular_file_nofollow(path).map_err(|error| {
            IosError::storage(format!(
                "failed to reopen {description} '{}': {error}",
                path.display()
            ))
        })?;
        let opened = file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened {description} '{}': {error}",
                path.display()
            ))
        })?;
        if !opened.file_type().is_file() || metadata_identity(&opened) != expected_identity {
            return Err(IosError::storage(format!(
                "{description} '{}' changed while confirming durability",
                path.display()
            )));
        }
        file.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync {description} '{}': {error}",
                path.display()
            ))
        })?;
        let parent = path.parent().ok_or_else(|| {
            IosError::storage(format!("{description} '{}' has no parent", path.display()))
        })?;
        sync_directory(parent)?;
        let final_metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect {description} '{}': {error}",
                path.display()
            ))
        })?;
        if !final_metadata.file_type().is_file()
            || metadata_identity(&final_metadata) != expected_identity
        {
            return Err(IosError::storage(format!(
                "{description} '{}' changed during durability confirmation",
                path.display()
            )));
        }
        Ok(())
    }

    fn remove_journal_durably(
        path: &Path,
        expected_identity: FileIdentity,
        parent_identity: FileIdentity,
        description: &str,
    ) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {description} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata_identity(&metadata) != expected_identity {
            return Err(IosError::storage(format!(
                "{description} '{}' changed before cleanup",
                path.display()
            )));
        }
        let parent = path.parent().ok_or_else(|| {
            IosError::storage(format!("{description} '{}' has no parent", path.display()))
        })?;
        let removal = journal_removal_path(path)?;
        match fs::symlink_metadata(&removal) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(IosError::storage(format!(
                    "{description} removal quarantine already exists: {}",
                    removal.display()
                )));
            }
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect {description} removal quarantine '{}': {error}",
                    removal.display()
                )));
            }
        }
        rename_noreplace(path, &removal).map_err(|error| {
            IosError::storage(format!(
                "failed to quarantine {description} '{}': {error}",
                path.display(),
            ))
        })?;
        sync_directory(parent)?;
        let quarantined = fs::symlink_metadata(&removal).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        if !quarantined.file_type().is_file()
            || metadata_identity(&quarantined) != expected_identity
        {
            restore_quarantined_path(&removal, path, parent)?;
            return Err(IosError::storage(format!(
                "{description} '{}' changed while it was quarantined",
                path.display()
            )));
        }
        remove_quarantined_journal_file(
            &removal,
            parent,
            parent_identity,
            expected_identity,
            description,
        )
    }

    fn remove_quarantined_journal_file(
        removal: &Path,
        parent: &Path,
        parent_identity: FileIdentity,
        expected_identity: FileIdentity,
        description: &str,
    ) -> Result<(), IosError> {
        use rustix::fs::{
            AtFlags, FileType, Mode, OFlags, RawMode, fstat, fsync, getpath, open, openat, statat,
            unlinkat,
        };

        let name = removal.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "quarantined {description} '{}' has no file name",
                removal.display()
            ))
        })?;
        let parent_fd = open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            IosError::storage(format!(
                "failed to open quarantined {description} parent '{}': {error}",
                parent.display()
            ))
        })?;
        verify_opened_directory(
            &parent_fd,
            parent,
            parent_identity,
            &format!("quarantined {description} parent"),
        )?;
        let file_fd = openat(
            &parent_fd,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            IosError::storage(format!(
                "failed to open quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        let opened = fstat(&file_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        if FileType::from_raw_mode(opened.st_mode as RawMode) != FileType::RegularFile
            || stat_identity(&opened) != expected_identity
        {
            return Err(IosError::storage(format!(
                "quarantined {description} '{}' changed identity before removal",
                removal.display()
            )));
        }
        let opened_path = getpath(&file_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve opened quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        if OsString::from_vec(opened_path.into_bytes()).as_os_str() != removal.as_os_str() {
            return Err(IosError::storage(format!(
                "opened quarantined {description} is no longer named '{}'",
                removal.display()
            )));
        }
        run_journal_cleanup_test_hook(description)?;
        verify_opened_directory(
            &parent_fd,
            parent,
            parent_identity,
            &format!("quarantined {description} parent"),
        )?;
        let opened = fstat(&file_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect opened quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        let opened_path = getpath(&file_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to re-resolve opened quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        let opened_path = OsString::from_vec(opened_path.into_bytes());
        let named = statat(&parent_fd, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        if FileType::from_raw_mode(opened.st_mode as RawMode) != FileType::RegularFile
            || stat_identity(&opened) != expected_identity
            || opened_path.as_os_str() != removal.as_os_str()
            || FileType::from_raw_mode(named.st_mode as RawMode) != FileType::RegularFile
            || stat_identity(&named) != expected_identity
        {
            return Err(IosError::storage(format!(
                "quarantined {description} '{}' changed before descriptor-relative removal",
                removal.display()
            )));
        }
        unlinkat(&parent_fd, name, AtFlags::empty()).map_err(|error| {
            IosError::storage(format!(
                "failed to remove quarantined {description} '{}': {error}",
                removal.display()
            ))
        })?;
        fsync(&parent_fd).map_err(|error| {
            IosError::storage(format!(
                "failed to sync quarantined {description} parent '{}': {error}",
                parent.display()
            ))
        })
    }

    fn journal_removal_path(path: &Path) -> Result<PathBuf, IosError> {
        let parent = path.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin journal '{}' has no parent",
                path.display()
            ))
        })?;
        let name = path.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin journal '{}' has no file name",
                path.display()
            ))
        })?;
        let mut removal_name = name.to_os_string();
        removal_name.push(".removing");
        Ok(parent.join(removal_name))
    }

    fn restore_journal_removal(path: &Path, description: &str) -> Result<(), IosError> {
        let removal = journal_removal_path(path)?;
        let current = optional_regular_file_metadata(path, description)?;
        let quarantined =
            optional_regular_file_metadata(&removal, &format!("{description} removal quarantine"))?;
        match (current, quarantined) {
            (_, None) => Ok(()),
            (None, Some(_)) => {
                let parent = path.parent().ok_or_else(|| {
                    IosError::storage(format!("{description} '{}' has no parent", path.display()))
                })?;
                rename_noreplace(&removal, path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to restore quarantined {description} '{}': {error}",
                        removal.display()
                    ))
                })?;
                sync_directory(parent)
            }
            (Some(_), Some(_)) => Err(IosError::storage(format!(
                "{description} and its removal quarantine both exist"
            ))),
        }
    }

    fn optional_regular_file_metadata(
        path: &Path,
        label: &str,
    ) -> Result<Option<fs::Metadata>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => Ok(Some(metadata)),
            Ok(_) => Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink file",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn recover_promotion_journal(root: &Path, path: &Path) -> Result<(), IosError> {
        let Some(loaded) = read_promotion_journal(path)? else {
            return Ok(());
        };
        validate_promotion_journal(root, &loaded.journal)?;
        confirm_journal_durable(path, loaded.identity, "iOS plugin release journal")?;
        for record in &loaded.journal.records {
            preflight_promotion_record(record, loaded.journal.decision)?;
        }
        preflight_promotion_owners(&loaded.journal)?;

        match loaded.journal.decision {
            PromotionDecision::Rollback => {
                for record in loaded.journal.records.iter().rev() {
                    if classify_promotion_record(record)? == PromotionPlacement::After {
                        rollback_promotion_record(record)?;
                    }
                }
            }
            PromotionDecision::Commit => {
                for record in &loaded.journal.records {
                    if classify_promotion_record(record)? == PromotionPlacement::Before {
                        apply_promotion_record(record)?;
                    }
                }
            }
        }
        verify_promotion_targets(&loaded.journal)?;
        cleanup_promotion_owners(&loaded.journal)?;
        verify_promotion_targets(&loaded.journal)?;
        for (owner, _) in promotion_owners(&loaded.journal)? {
            let quarantine = owner_quarantine_path(&owner)?;
            for path in [&owner, &quarantine] {
                match fs::symlink_metadata(path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(IosError::storage(format!(
                            "iOS plugin release recovery owner '{}' remains after cleanup",
                            path.display()
                        )));
                    }
                    Err(error) => {
                        return Err(IosError::storage(format!(
                            "failed to verify iOS plugin release recovery owner '{}': {error}",
                            path.display()
                        )));
                    }
                }
            }
        }
        remove_journal_durably(
            path,
            loaded.identity,
            loaded.journal.state_directory_identity,
            "iOS plugin release journal",
        )
    }

    fn preflight_promotion_record(
        record: &PromotionRecord,
        decision: PromotionDecision,
    ) -> Result<(), IosError> {
        let placement = classify_promotion_record(record)?;
        let compatible = match decision {
            PromotionDecision::Rollback => matches!(
                placement,
                PromotionPlacement::Before
                    | PromotionPlacement::After
                    | PromotionPlacement::ExternalReplacement
                    | PromotionPlacement::RollbackCleanupPending
                    | PromotionPlacement::RolledBackAndCleaned
            ),
            PromotionDecision::Commit => matches!(
                placement,
                PromotionPlacement::Before
                    | PromotionPlacement::After
                    | PromotionPlacement::CommitCleanupPending
                    | PromotionPlacement::CommittedAndCleaned
            ),
        };
        if compatible {
            Ok(())
        } else {
            Err(IosError::storage(format!(
                "iOS plugin release target '{}' cannot satisfy its durable decision",
                record.target.display()
            )))
        }
    }

    fn preflight_promotion_owners(journal: &PromotionJournal) -> Result<(), IosError> {
        for (owner, expected_identity) in promotion_owners(journal)? {
            let quarantine = owner_quarantine_path(&owner)?;
            let current = optional_directory_metadata(&owner, "iOS plugin release recovery owner")?;
            let quarantined = optional_directory_metadata(
                &quarantine,
                "iOS plugin release recovery owner quarantine",
            )?;
            match (current, quarantined) {
                (None, None) => {}
                (Some(metadata), None) | (None, Some(metadata))
                    if metadata_identity(&metadata) == expected_identity => {}
                _ => {
                    return Err(IosError::storage(format!(
                        "iOS plugin release recovery owner '{}' has an unknown identity or placement",
                        owner.display()
                    )));
                }
            }
        }
        Ok(())
    }

    fn cleanup_promotion_owners(journal: &PromotionJournal) -> Result<(), IosError> {
        for (owner, expected_identity) in promotion_owners(journal)? {
            let parent = owner.parent().ok_or_else(|| {
                IosError::storage(format!(
                    "iOS plugin release recovery owner '{}' has no parent",
                    owner.display()
                ))
            })?;
            let quarantine = owner_quarantine_path(&owner)?;
            if optional_directory_metadata(
                &quarantine,
                "iOS plugin release recovery owner quarantine",
            )?
            .is_none()
                && optional_directory_metadata(&owner, "iOS plugin release recovery owner")?
                    .is_some()
            {
                rename_noreplace(&owner, &quarantine).map_err(|error| {
                    IosError::storage(format!(
                        "failed to quarantine iOS plugin release recovery owner '{}': {error}",
                        owner.display()
                    ))
                })?;
                sync_directory(parent)?;
            }
            let Some(metadata) = optional_directory_metadata(
                &quarantine,
                "iOS plugin release recovery owner quarantine",
            )?
            else {
                sync_directory(parent)?;
                continue;
            };
            if metadata_identity(&metadata) != expected_identity {
                restore_quarantined_path(&quarantine, &owner, parent)?;
                return Err(IosError::storage(format!(
                    "iOS plugin release recovery owner '{}' changed while it was quarantined",
                    owner.display()
                )));
            }
            remove_quarantined_owner_directory(
                &quarantine,
                parent,
                promotion_owner_parent_identity(journal, parent)?,
                expected_identity,
                "iOS plugin release recovery owner",
            )?;
        }
        Ok(())
    }

    fn promotion_owner_parent_identity(
        journal: &PromotionJournal,
        parent: &Path,
    ) -> Result<FileIdentity, IosError> {
        if parent == journal.build_parent {
            Ok(journal.build_parent_identity)
        } else if parent == journal.output_directory {
            Ok(journal.output_directory_identity)
        } else {
            Err(IosError::storage(format!(
                "iOS plugin release recovery owner has an unexpected parent: {}",
                parent.display()
            )))
        }
    }

    fn owner_quarantine_path(owner: &Path) -> Result<PathBuf, IosError> {
        let parent = owner.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin release owner '{}' has no parent",
                owner.display()
            ))
        })?;
        let name = owner.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "iOS plugin release owner '{}' has no file name",
                owner.display()
            ))
        })?;
        let mut quarantine_name = name.to_os_string();
        quarantine_name.push(".removing");
        Ok(parent.join(quarantine_name))
    }

    fn promotion_owners(
        journal: &PromotionJournal,
    ) -> Result<BTreeMap<PathBuf, FileIdentity>, IosError> {
        let mut owners = BTreeMap::new();
        for record in &journal.records {
            match owners.insert(record.owner.clone(), record.owner_identity) {
                Some(identity) if identity != record.owner_identity => {
                    return Err(IosError::storage(format!(
                        "iOS plugin release journal records conflicting identities for owner '{}'",
                        record.owner.display()
                    )));
                }
                _ => {}
            }
        }
        Ok(owners)
    }

    fn verify_promotion_targets(journal: &PromotionJournal) -> Result<(), IosError> {
        for record in &journal.records {
            if journal.decision == PromotionDecision::Rollback
                && classify_promotion_record(record)? == PromotionPlacement::ExternalReplacement
            {
                continue;
            }
            let target = optional_promotion_node_snapshot(
                &record.target,
                record.node_kind,
                "iOS plugin release transaction target",
                None,
            )?;
            let expected = match journal.decision {
                PromotionDecision::Rollback => record.old.as_ref(),
                PromotionDecision::Commit => Some(&record.new),
            };
            if target.as_ref() != expected {
                return Err(IosError::storage(format!(
                    "iOS plugin release target '{}' does not match its durable decision",
                    record.target.display()
                )));
            }
        }
        Ok(())
    }

    fn classify_promotion_record(record: &PromotionRecord) -> Result<PromotionPlacement, IosError> {
        let target = optional_promotion_node_snapshot(
            &record.target,
            record.node_kind,
            "iOS plugin release transaction target",
            None,
        )?;
        let owner =
            optional_directory_identity(&record.owner, "iOS plugin release recovery owner")?;
        let source = optional_promotion_node_snapshot(
            &record.source,
            record.node_kind,
            "iOS plugin release transaction source",
            None,
        );

        if target == record.old {
            return match owner {
                None => Ok(PromotionPlacement::RolledBackAndCleaned),
                Some(identity) if identity == record.owner_identity => match source {
                    Ok(source) if source.as_ref() == Some(&record.new) => {
                        Ok(PromotionPlacement::Before)
                    }
                    _ => Ok(PromotionPlacement::RollbackCleanupPending),
                },
                Some(_) => Err(unknown_promotion_placement(record)),
            };
        }
        if target.as_ref() == Some(&record.new) {
            return match owner {
                None => Ok(PromotionPlacement::CommittedAndCleaned),
                Some(identity) if identity == record.owner_identity => match source {
                    Ok(source) if source == record.old => Ok(PromotionPlacement::After),
                    _ => Ok(PromotionPlacement::CommitCleanupPending),
                },
                Some(_) => Err(unknown_promotion_placement(record)),
            };
        }
        Ok(PromotionPlacement::ExternalReplacement)
    }

    fn unknown_promotion_placement(record: &PromotionRecord) -> IosError {
        IosError::storage(format!(
            "iOS plugin release paths '{}' and '{}' have unknown identities; recovery stopped",
            record.target.display(),
            record.source.display()
        ))
    }

    fn recover_release_transactions(
        root: &Path,
        promotion_path: &Path,
        prepared_path: &Path,
        prepared_expectation: PreparedOwnerExpectation,
    ) -> Result<(), IosError> {
        recover_promotion_journal(root, promotion_path)?;
        recover_prepared_owner_journal(root, prepared_path, Some(prepared_expectation))
    }

    fn rollback_promotion_error(
        root: &Path,
        path: &Path,
        prepared_path: &Path,
        prepared_expectation: PreparedOwnerExpectation,
        error: IosError,
    ) -> IosError {
        match recover_release_transactions(root, path, prepared_path, prepared_expectation) {
            Ok(()) => error,
            Err(recovery) => append_error(error, recovery.to_string()),
        }
    }

    fn append_error(error: IosError, suffix: impl AsRef<str>) -> IosError {
        let message = format!("{error}; {}", suffix.as_ref());
        match error.kind() {
            crate::ios::IosErrorKind::Storage => IosError::storage(message),
            crate::ios::IosErrorKind::Compatibility => IosError::compatibility(message),
            crate::ios::IosErrorKind::Conformance => IosError::conformance(message),
            crate::ios::IosErrorKind::Worker => IosError::worker(message),
        }
    }

    fn validate_prepared_owner_journal(
        root: &Path,
        journal: &PreparedOwnerJournal,
    ) -> Result<(), IosError> {
        validate_prepared_owner_journal_with_mode(root, journal, false).map(|_| ())
    }

    fn validate_prepared_owner_journal_for_recovery(
        root: &Path,
        journal: &PreparedOwnerJournal,
    ) -> Result<bool, IosError> {
        validate_prepared_owner_journal_with_mode(root, journal, false)
    }

    fn validate_prepared_owner_journal_with_mode(
        root: &Path,
        journal: &PreparedOwnerJournal,
        allow_missing_output_parent: bool,
    ) -> Result<bool, IosError> {
        if journal.version != PREPARED_OWNER_JOURNAL_VERSION {
            return Err(IosError::storage(format!(
                "unsupported iOS plugin prepared-owner journal version {}",
                journal.version
            )));
        }
        if journal.transaction_id == [0; 16] {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal has a nil transaction identity",
            ));
        }
        let canonical_root = canonical_directory(root, "iOS plugin release journal repository")?;
        if journal.root != canonical_root
            || directory_identity(&canonical_root, "iOS plugin release journal repository")?
                != journal.root_identity
        {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal belongs to a different repository identity",
            ));
        }
        let expected_build_parent = canonical_root.join("lib/ios/VesperPlayerKit/.build");
        if journal.build_parent != expected_build_parent
            || directory_identity(&expected_build_parent, "iOS plugin release build parent")?
                != journal.build_parent_identity
        {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal build parent changed identity",
            ));
        }
        let expected_state_directory = expected_build_parent.join("vesper-cli-state");
        if journal.state_directory != expected_state_directory
            || directory_identity(
                &expected_state_directory,
                "iOS plugin release state directory",
            )? != journal.state_directory_identity
        {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal state directory changed identity",
            ));
        }
        let output_directory_identity = optional_directory_identity(
            &journal.output_directory,
            "iOS plugin release output directory",
        )?;
        let missing_output_parent = match output_directory_identity {
            Some(identity) => {
                if canonical_directory(
                    &journal.output_directory,
                    "iOS plugin release output directory",
                )? != journal.output_directory
                    || identity != journal.output_directory_identity
                {
                    return Err(IosError::storage(
                        "iOS plugin prepared-owner journal output directory changed identity",
                    ));
                }
                false
            }
            None if allow_missing_output_parent => true,
            None => {
                return Err(IosError::storage(
                    "iOS plugin prepared-owner journal output directory is missing",
                ));
            }
        };
        let plugin_id = journal_plugin_id(&journal.plugin_id)?;
        let plugin = plugin_id.spec();
        let canonical_plugin_target = expected_build_parent
            .join(plugin.build_directory)
            .join(format!("{}.xcframework", plugin.framework_name));
        if journal.output_directory == canonical_plugin_target
            || journal
                .output_directory
                .starts_with(&canonical_plugin_target)
            || journal.output_directory == expected_state_directory
            || journal
                .output_directory
                .starts_with(&expected_state_directory)
        {
            return Err(IosError::storage(
                "iOS plugin prepared-owner journal output overlaps a managed path",
            ));
        }
        if journal.owners.len() != REQUIRED_PREPARED_OWNERS {
            return Err(IosError::storage(format!(
                "iOS plugin prepared-owner journal must contain exactly {REQUIRED_PREPARED_OWNERS} owners"
            )));
        }
        let suffix = encode_transaction_id(journal.transaction_id);
        let expected = [
            (
                PreparedOwnerRole::Build,
                expected_build_parent.join(format!(".vesper-ios-plugin-release-{suffix}")),
                expected_build_parent,
                journal.build_parent_identity,
            ),
            (
                PreparedOwnerRole::Assets,
                journal
                    .output_directory
                    .join(format!(".vesper-ios-plugin-assets-{suffix}")),
                journal.output_directory.clone(),
                journal.output_directory_identity,
            ),
        ];
        for (owner, (role, path, parent, parent_identity)) in journal.owners.iter().zip(expected) {
            let quarantine = prepared_owner_quarantine_path(owner)?;
            for candidate in [&owner.path, &owner.parent, &quarantine] {
                validate_journal_path(candidate)?;
            }
            if owner.role != role
                || owner.path != path
                || owner.parent != parent
                || owner.path.parent() != Some(owner.parent.as_path())
                || owner.parent_identity != parent_identity
            {
                return Err(IosError::storage(
                    "iOS plugin prepared-owner journal contains an invalid owner path or parent",
                ));
            }
            if missing_output_parent && owner.role == PreparedOwnerRole::Assets {
                ensure_prepared_owner_parent_absent(owner)?;
                continue;
            }
            if directory_identity(&owner.parent, "iOS plugin prepared staging parent")?
                != owner.parent_identity
            {
                return Err(IosError::storage(
                    "iOS plugin prepared-owner journal contains an invalid owner path or parent",
                ));
            }
            match (
                owner.identity,
                optional_directory_identity(&owner.path, "iOS plugin prepared staging owner")?,
            ) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(IosError::storage(format!(
                        "iOS plugin prepared staging owner '{}' changed identity",
                        owner.path.display()
                    )));
                }
                _ => {}
            }
        }
        if paths_overlap(&journal.owners[0].path, &journal.owners[1].path)
            || journal.owners.iter().any(|owner| {
                paths_overlap(&owner.path, &journal.state_directory)
                    || prepared_owner_quarantine_path(owner).is_ok_and(|quarantine| {
                        paths_overlap(&quarantine, &journal.state_directory)
                    })
            })
        {
            return Err(IosError::storage(
                "iOS plugin prepared staging owners overlap another managed path",
            ));
        }
        Ok(missing_output_parent)
    }

    fn validate_promotion_journal(root: &Path, journal: &PromotionJournal) -> Result<(), IosError> {
        if journal.version != PROMOTION_JOURNAL_VERSION {
            return Err(IosError::storage(format!(
                "unsupported iOS plugin release journal version {}",
                journal.version
            )));
        }
        if journal.transaction_id == [0; 16] {
            return Err(IosError::storage(
                "iOS plugin release journal has a nil transaction identity",
            ));
        }
        let canonical_root = canonical_directory(root, "iOS plugin release journal repository")?;
        if journal.root != canonical_root
            || directory_identity(&canonical_root, "iOS plugin release journal repository")?
                != journal.root_identity
        {
            return Err(IosError::storage(
                "iOS plugin release journal belongs to a different repository identity",
            ));
        }
        let expected_build_parent = canonical_root.join("lib/ios/VesperPlayerKit/.build");
        if journal.build_parent != expected_build_parent
            || directory_identity(&expected_build_parent, "iOS plugin release build parent")?
                != journal.build_parent_identity
        {
            return Err(IosError::storage(
                "iOS plugin release journal build parent changed identity",
            ));
        }
        let expected_state_directory = expected_build_parent.join("vesper-cli-state");
        if journal.state_directory != expected_state_directory
            || directory_identity(
                &expected_state_directory,
                "iOS plugin release state directory",
            )? != journal.state_directory_identity
        {
            return Err(IosError::storage(
                "iOS plugin release journal state directory changed identity",
            ));
        }
        if canonical_directory(
            &journal.output_directory,
            "iOS plugin release output directory",
        )? != journal.output_directory
            || directory_identity(
                &journal.output_directory,
                "iOS plugin release output directory",
            )? != journal.output_directory_identity
        {
            return Err(IosError::storage(
                "iOS plugin release journal output directory changed identity",
            ));
        }
        let plugin_id = journal_plugin_id(&journal.plugin_id)?;
        let plugin = plugin_id.spec();
        if journal.records.len() < REQUIRED_PROMOTION_RECORDS
            || journal.records.len() > MAX_PROMOTION_RECORDS
        {
            return Err(IosError::storage(format!(
                "iOS plugin release journal must contain {REQUIRED_PROMOTION_RECORDS} to {MAX_PROMOTION_RECORDS} records"
            )));
        }

        let expected_framework = expected_build_parent
            .join(plugin.build_directory)
            .join(format!("{}.xcframework", plugin.framework_name));
        let expected_plugin_zip = journal
            .output_directory
            .join(format!("{}.xcframework.zip", plugin.framework_name));
        if journal.records[0].node_kind != PromotionNodeKind::Directory
            || journal.records[0].target != expected_framework
            || journal.records[1].node_kind != PromotionNodeKind::File
            || journal.records[1].target != expected_plugin_zip
        {
            return Err(IosError::storage(
                "iOS plugin release journal omits or reorders its required framework records",
            ));
        }

        let mut target_paths = BTreeSet::new();
        let mut source_paths = BTreeSet::new();
        let mut runtime_names = Vec::new();
        let mut build_owner = None::<(PathBuf, FileIdentity)>;
        let mut asset_owner = None::<(PathBuf, FileIdentity)>;
        let expected_runtime_directory = expected_build_parent.join("player-ffmpeg-runtime");
        let mut runtime_directory_record = false;
        let last_record = journal.records.len() - 1;
        for (index, record) in journal.records.iter().enumerate() {
            for path in [
                &record.parent,
                &record.target,
                &record.source,
                &record.owner,
            ] {
                validate_journal_path(path)?;
            }
            if record.target.parent() != Some(record.parent.as_path())
                || record.source.parent() != Some(record.owner.as_path())
                || directory_identity(&record.parent, "iOS plugin release journal parent")?
                    != record.parent_identity
            {
                return Err(IosError::storage(
                    "iOS plugin release journal contains an invalid parent identity",
                ));
            }
            let owner_quarantine = owner_quarantine_path(&record.owner)?;
            validate_journal_path(&owner_quarantine)?;
            let current =
                optional_directory_metadata(&record.owner, "iOS plugin release journal owner")?;
            let quarantined = optional_directory_metadata(
                &owner_quarantine,
                "iOS plugin release journal owner quarantine",
            )?;
            match (current, quarantined) {
                (None, None) => {}
                (Some(metadata), None) | (None, Some(metadata))
                    if metadata_identity(&metadata) == record.owner_identity => {}
                _ => {
                    return Err(IosError::storage(format!(
                        "iOS plugin release journal owner '{}' has an unknown identity or placement",
                        record.owner.display()
                    )));
                }
            }
            validate_promotion_snapshot(&record.new, record.node_kind)?;
            if let Some(old) = &record.old {
                validate_promotion_snapshot(old, record.node_kind)?;
                if old.identity == record.new.identity {
                    return Err(IosError::storage(
                        "iOS plugin release journal reuses one identity for old and new outputs",
                    ));
                }
            }
            if !target_paths.insert(record.target.clone())
                || !source_paths.insert(record.source.clone())
            {
                return Err(IosError::storage(
                    "iOS plugin release journal contains duplicate source or target paths",
                ));
            }

            let target_name = record.target.file_name().ok_or_else(|| {
                IosError::storage("iOS plugin release journal target has no file name")
            })?;
            if record.source.file_name() != Some(target_name) {
                return Err(IosError::storage(
                    "iOS plugin release journal source and target names do not match",
                ));
            }
            if index == 0 {
                if record.owner.parent() != Some(expected_build_parent.as_path())
                    || !has_path_prefix(&record.owner, ".vesper-ios-plugin-release-")
                {
                    return Err(IosError::storage(
                        "iOS plugin release journal contains an invalid build staging owner",
                    ));
                }
                merge_owner_identity(&mut build_owner, &record.owner, record.owner_identity)?;
            } else if index == last_record
                && record.node_kind == PromotionNodeKind::Directory
                && record.target == expected_runtime_directory
            {
                if !plugin.uses_ffmpeg
                    || runtime_directory_record
                    || record.owner.parent() != Some(expected_build_parent.as_path())
                    || !has_path_prefix(&record.owner, ".vesper-ios-plugin-release-")
                {
                    return Err(IosError::storage(
                        "iOS plugin release journal contains an invalid canonical FFmpeg runtime record",
                    ));
                }
                merge_owner_identity(&mut build_owner, &record.owner, record.owner_identity)?;
                runtime_directory_record = true;
            } else {
                if record.owner.parent() != Some(journal.output_directory.as_path())
                    || record.parent != journal.output_directory
                    || !has_path_prefix(&record.owner, ".vesper-ios-plugin-assets-")
                {
                    return Err(IosError::storage(
                        "iOS plugin release journal contains an invalid asset staging owner",
                    ));
                }
                merge_owner_identity(&mut asset_owner, &record.owner, record.owner_identity)?;
                if index > 1 {
                    if record.node_kind != PromotionNodeKind::File
                        || !plugin.uses_ffmpeg
                        || !is_ffmpeg_runtime_zip_name(target_name)
                    {
                        return Err(IosError::storage(
                            "iOS plugin release journal contains an unsupported runtime archive",
                        ));
                    }
                    runtime_names.push(target_name.to_os_string());
                }
            }
        }
        if build_owner.is_none() || asset_owner.is_none() {
            return Err(IosError::storage(
                "iOS plugin release journal omits a required staging owner",
            ));
        }
        if !runtime_names.windows(2).all(|names| names[0] < names[1]) {
            return Err(IosError::storage(
                "iOS plugin release journal runtime archives are not uniquely sorted",
            ));
        }
        let targets = target_paths.iter().collect::<Vec<_>>();
        for (index, left) in targets.iter().enumerate() {
            for right in targets.iter().skip(index + 1) {
                if paths_overlap(left, right) {
                    return Err(IosError::storage(
                        "iOS plugin release journal contains overlapping targets",
                    ));
                }
            }
        }
        for (owner, _) in promotion_owners(journal)? {
            let quarantine = owner_quarantine_path(&owner)?;
            if target_paths
                .iter()
                .any(|target| paths_overlap(&owner, target) || paths_overlap(&quarantine, target))
                || paths_overlap(&owner, &journal.state_directory)
                || paths_overlap(&quarantine, &journal.state_directory)
            {
                return Err(IosError::storage(
                    "iOS plugin release journal staging owner overlaps a durable path",
                ));
            }
        }
        Ok(())
    }

    fn journal_plugin_id(value: &str) -> Result<IosPluginId, IosError> {
        match value {
            "remux-ffmpeg" => Ok(IosPluginId::RemuxFfmpeg),
            "source-normalizer-ffmpeg" => Ok(IosPluginId::SourceNormalizerFfmpeg),
            "decoder-videotoolbox" => Ok(IosPluginId::DecoderVideoToolbox),
            "frame-processor-diagnostic" => Ok(IosPluginId::FrameProcessorDiagnostic),
            "performance-diagnostics" => Ok(IosPluginId::PerformanceDiagnostics),
            _ => Err(IosError::storage(format!(
                "iOS plugin release journal contains an unknown plugin ID: {value}"
            ))),
        }
    }

    fn merge_owner_identity(
        slot: &mut Option<(PathBuf, FileIdentity)>,
        path: &Path,
        identity: FileIdentity,
    ) -> Result<(), IosError> {
        match slot {
            Some((expected_path, expected_identity))
                if expected_path != path || *expected_identity != identity =>
            {
                Err(IosError::storage(
                    "iOS plugin release journal splits one owner role across paths",
                ))
            }
            Some(_) => Ok(()),
            None => {
                *slot = Some((path.to_path_buf(), identity));
                Ok(())
            }
        }
    }

    fn validate_promotion_snapshot(
        snapshot: &PromotionNodeSnapshot,
        node_kind: PromotionNodeKind,
    ) -> Result<(), IosError> {
        if snapshot.snapshot_sha256.len() != 64
            || !snapshot
                .snapshot_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || match node_kind {
                PromotionNodeKind::File => snapshot.payload_bytes > MAX_RELEASE_ZIP_BYTES,
                PromotionNodeKind::Directory => snapshot.payload_bytes > MAX_PROMOTION_TREE_BYTES,
            }
        {
            return Err(IosError::storage(
                "iOS plugin release journal contains an invalid node snapshot",
            ));
        }
        Ok(())
    }

    fn validate_journal_path(path: &Path) -> Result<(), IosError> {
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(IosError::storage(format!(
                "iOS plugin release journal contains a non-canonical path '{}'",
                path.display()
            )));
        }
        reject_symlink_components(path)
    }

    fn reject_symlink_components(path: &Path) -> Result<(), IosError> {
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(IosError::storage(format!(
                        "iOS plugin release journal path contains a symlink: {}",
                        current.display()
                    )));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to inspect iOS plugin release journal path '{}': {error}",
                        current.display()
                    )));
                }
            }
        }
        Ok(())
    }

    fn has_path_prefix(path: &Path, prefix: &str) -> bool {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
    }

    fn paths_overlap(left: &Path, right: &Path) -> bool {
        left == right || left.starts_with(right) || right.starts_with(left)
    }

    fn is_ffmpeg_runtime_zip_name(name: &OsStr) -> bool {
        matches!(
            name.to_str(),
            Some(
                "VesperFFmpegAVCodec.xcframework.zip"
                    | "VesperFFmpegAVDevice.xcframework.zip"
                    | "VesperFFmpegAVFilter.xcframework.zip"
                    | "VesperFFmpegAVFormat.xcframework.zip"
                    | "VesperFFmpegAVUtil.xcframework.zip"
                    | "VesperFFmpegPostproc.xcframework.zip"
                    | "VesperFFmpegSWResample.xcframework.zip"
                    | "VesperFFmpegSWScale.xcframework.zip"
            )
        )
    }

    fn promotion_node_snapshot(
        path: &Path,
        node_kind: PromotionNodeKind,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
    ) -> Result<PromotionNodeSnapshot, IosError> {
        match node_kind {
            PromotionNodeKind::File => file_promotion_snapshot(path, label, cancellation),
            PromotionNodeKind::Directory => directory_promotion_snapshot(path, label, cancellation),
        }
    }

    fn optional_promotion_node_snapshot(
        path: &Path,
        node_kind: PromotionNodeKind,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
    ) -> Result<Option<PromotionNodeSnapshot>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(_) => promotion_node_snapshot(path, node_kind, label, cancellation).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn file_promotion_snapshot(
        path: &Path,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
    ) -> Result<PromotionNodeSnapshot, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_RELEASE_ZIP_BYTES {
            return Err(IosError::storage(format!(
                "{label} '{}' is not a bounded regular non-symlink file",
                path.display()
            )));
        }
        let identity = metadata_identity(&metadata);
        let mut digest = Sha256::new();
        digest.update(b"vesper-ios-plugin-promotion-file-v1\0");
        digest.update(metadata.mode().to_le_bytes());
        digest.update(metadata.len().to_le_bytes());
        hash_stable_file_payload(path, &metadata, &mut digest, cancellation, label)?;
        Ok(PromotionNodeSnapshot {
            identity,
            payload_bytes: metadata.len(),
            snapshot_sha256: hex::encode(digest.finalize()),
        })
    }

    fn directory_promotion_snapshot(
        path: &Path,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
    ) -> Result<PromotionNodeSnapshot, IosError> {
        bounded_directory_snapshot(
            path,
            label,
            cancellation,
            DirectorySnapshotLimits {
                maximum_entries: MAX_PROMOTION_TREE_ENTRIES,
                maximum_depth: MAX_PROMOTION_TREE_DEPTH,
                maximum_bytes: MAX_PROMOTION_TREE_BYTES,
                digest_domain: b"vesper-ios-plugin-promotion-directory-v1\0",
            },
        )
    }

    fn collect_directory_entries_bounded(
        directory: &Path,
        maximum_entries: usize,
        label: &str,
        limit_error: IosError,
    ) -> Result<Vec<fs::DirEntry>, IosError> {
        let entries = fs::read_dir(directory).map_err(|error| {
            IosError::storage(format!(
                "failed to scan {label} directory '{}': {error}",
                directory.display()
            ))
        })?;
        let mut collected = Vec::with_capacity(maximum_entries.min(1024));
        for entry in entries {
            if collected.len() == maximum_entries {
                return Err(limit_error);
            }
            collected.push(entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} entry under '{}': {error}",
                    directory.display()
                ))
            })?);
        }
        Ok(collected)
    }

    fn collect_directory_names_bounded(
        directory: &Path,
        maximum_entries: usize,
        label: &str,
        limit_error: IosError,
    ) -> Result<Vec<OsString>, IosError> {
        let entries = fs::read_dir(directory).map_err(|error| {
            IosError::storage(format!(
                "failed to re-scan {label} directory '{}': {error}",
                directory.display()
            ))
        })?;
        let mut names = Vec::with_capacity(maximum_entries.min(1024));
        for entry in entries {
            if names.len() == maximum_entries {
                return Err(limit_error);
            }
            names.push(
                entry
                    .map_err(|error| {
                        IosError::storage(format!(
                            "failed to re-read {label} entry under '{}': {error}",
                            directory.display()
                        ))
                    })?
                    .file_name(),
            );
        }
        Ok(names)
    }

    fn bounded_directory_snapshot(
        path: &Path,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
        limits: DirectorySnapshotLimits,
    ) -> Result<PromotionNodeSnapshot, IosError> {
        let digest = bounded_directory_digest(
            path,
            label,
            cancellation,
            limits,
            DirectoryDigestSemantics::ImmutableIdentity,
        )?;
        Ok(PromotionNodeSnapshot {
            identity: digest.identity,
            payload_bytes: digest.payload_bytes,
            snapshot_sha256: digest.sha256,
        })
    }

    fn bounded_directory_content_fingerprint(
        path: &Path,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
        limits: DirectorySnapshotLimits,
    ) -> Result<String, IosError> {
        bounded_directory_digest(
            path,
            label,
            cancellation,
            limits,
            DirectoryDigestSemantics::BuildContent,
        )
        .map(|digest| digest.sha256)
    }

    fn bounded_directory_digest(
        path: &Path,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
        limits: DirectorySnapshotLimits,
        semantics: DirectoryDigestSemantics,
    ) -> Result<BoundedDirectoryDigest, IosError> {
        let root_metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !root_metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink directory",
                path.display()
            )));
        }
        let identity = metadata_identity(&root_metadata);
        let mut digest = Sha256::new();
        digest.update(limits.digest_domain);
        digest.update(root_metadata.mode().to_le_bytes());
        let mut pending = VecDeque::from([(path.to_path_buf(), PathBuf::new(), 0_usize, identity)]);
        let mut entries = 0_usize;
        let mut total_bytes = 0_u64;
        while let Some((directory, relative_directory, depth, expected_identity)) =
            pending.pop_front()
        {
            check_snapshot_cancellation(cancellation, label)?;
            if depth > limits.maximum_depth {
                return Err(IosError::storage(format!(
                    "{label} '{}' exceeds tree depth {}",
                    path.display(),
                    limits.maximum_depth,
                )));
            }
            let directory_metadata = fs::symlink_metadata(&directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} directory '{}': {error}",
                    directory.display()
                ))
            })?;
            if !directory_metadata.file_type().is_dir()
                || metadata_identity(&directory_metadata) != expected_identity
            {
                return Err(IosError::storage(format!(
                    "{label} directory '{}' changed while it was scanned",
                    directory.display()
                )));
            }
            let remaining_entries = limits
                .maximum_entries
                .checked_sub(entries)
                .ok_or_else(|| IosError::storage(format!("{label} entry budget was exceeded")))?;
            let mut children = collect_directory_entries_bounded(
                &directory,
                remaining_entries,
                label,
                IosError::storage(format!(
                    "{label} '{}' exceeds {} entries",
                    path.display(),
                    limits.maximum_entries,
                )),
            )?;
            children.sort_by_key(fs::DirEntry::file_name);
            let initial_names = children
                .iter()
                .map(fs::DirEntry::file_name)
                .collect::<Vec<_>>();
            for child in children {
                check_snapshot_cancellation(cancellation, label)?;
                entries = entries
                    .checked_add(1)
                    .ok_or_else(|| IosError::storage(format!("{label} entry count overflowed")))?;
                if entries > limits.maximum_entries {
                    return Err(IosError::storage(format!(
                        "{label} '{}' exceeds {} entries",
                        path.display(),
                        limits.maximum_entries,
                    )));
                }
                let child_path = child.path();
                let relative = relative_directory.join(child.file_name());
                let metadata = fs::symlink_metadata(&child_path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to inspect {label} entry '{}': {error}",
                        child_path.display()
                    ))
                })?;
                hash_promotion_path(&mut digest, &relative);
                if semantics == DirectoryDigestSemantics::ImmutableIdentity {
                    hash_file_identity(&mut digest, metadata_identity(&metadata));
                }
                digest.update(metadata.mode().to_le_bytes());
                if metadata.file_type().is_dir() {
                    digest.update(b"D");
                    pending.push_back((
                        child_path,
                        relative,
                        depth + 1,
                        metadata_identity(&metadata),
                    ));
                } else if metadata.file_type().is_file() {
                    digest.update(b"F");
                    digest.update(metadata.len().to_le_bytes());
                    total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                        IosError::storage(format!("{label} expanded size overflowed"))
                    })?;
                    if total_bytes > limits.maximum_bytes {
                        return Err(IosError::storage(format!(
                            "{label} '{}' exceeds {} bytes",
                            path.display(),
                            limits.maximum_bytes,
                        )));
                    }
                    hash_stable_file_payload(
                        &child_path,
                        &metadata,
                        &mut digest,
                        cancellation,
                        label,
                    )?;
                } else {
                    return Err(IosError::storage(format!(
                        "{label} contains a symlink or special file: {}",
                        child_path.display()
                    )));
                }
            }
            let final_metadata = fs::symlink_metadata(&directory).map_err(|error| {
                IosError::storage(format!(
                    "failed to re-inspect {label} directory '{}': {error}",
                    directory.display()
                ))
            })?;
            let mut final_names = collect_directory_names_bounded(
                &directory,
                initial_names.len(),
                label,
                IosError::storage(format!(
                    "{label} directory '{}' changed while it was scanned",
                    directory.display()
                )),
            )?;
            final_names.sort();
            if !final_metadata.file_type().is_dir()
                || metadata_identity(&final_metadata) != expected_identity
                || final_names != initial_names
            {
                return Err(IosError::storage(format!(
                    "{label} directory '{}' changed while it was scanned",
                    directory.display()
                )));
            }
        }
        if directory_identity(path, label)? != identity {
            return Err(IosError::storage(format!(
                "{label} '{}' changed while it was scanned",
                path.display()
            )));
        }
        Ok(BoundedDirectoryDigest {
            identity,
            payload_bytes: total_bytes,
            sha256: hex::encode(digest.finalize()),
        })
    }

    fn hash_promotion_path(hasher: &mut Sha256, path: &Path) {
        let bytes = path.as_os_str().as_encoded_bytes();
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }

    fn hash_file_identity(hasher: &mut Sha256, identity: FileIdentity) {
        hasher.update(identity.device.to_le_bytes());
        hasher.update(identity.inode.to_le_bytes());
    }

    fn hash_stable_file_payload(
        path: &Path,
        metadata: &fs::Metadata,
        hasher: &mut Sha256,
        cancellation: Option<&external_process::InterruptDeferral>,
        label: &str,
    ) -> Result<(), IosError> {
        let identity = metadata_identity(metadata);
        let mut file = open_regular_file_nofollow(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} file '{}': {error}",
                path.display()
            ))
        })?;
        let opened = file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened {label} file '{}': {error}",
                path.display()
            ))
        })?;
        if !opened.file_type().is_file()
            || metadata_identity(&opened) != identity
            || opened.len() != metadata.len()
        {
            return Err(IosError::storage(format!(
                "{label} file '{}' changed while it was opened",
                path.display()
            )));
        }
        let mut buffer = [0_u8; 64 * 1024];
        let mut read_bytes = 0_u64;
        loop {
            check_snapshot_cancellation(cancellation, label)?;
            let count = file.read(&mut buffer).map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} file '{}': {error}",
                    path.display()
                ))
            })?;
            if count == 0 {
                break;
            }
            read_bytes = read_bytes
                .checked_add(count as u64)
                .ok_or_else(|| IosError::storage(format!("{label} file size overflowed")))?;
            if read_bytes > metadata.len() {
                return Err(IosError::storage(format!(
                    "{label} file '{}' grew while it was read",
                    path.display()
                )));
            }
            hasher.update(&buffer[..count]);
        }
        let final_metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect {label} file '{}': {error}",
                path.display()
            ))
        })?;
        if !final_metadata.file_type().is_file()
            || metadata_identity(&final_metadata) != identity
            || final_metadata.len() != metadata.len()
            || read_bytes != metadata.len()
        {
            return Err(IosError::storage(format!(
                "{label} file '{}' changed while it was read",
                path.display()
            )));
        }
        Ok(())
    }

    fn sync_promotion_source(
        path: &Path,
        node_kind: PromotionNodeKind,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        match node_kind {
            PromotionNodeKind::File => {
                check_cancellation(cancellation, "iOS plugin release candidate sync")?;
                let file = open_regular_file_nofollow(path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to open iOS plugin release candidate '{}': {error}",
                        path.display()
                    ))
                })?;
                file.sync_all().map_err(|error| {
                    IosError::storage(format!(
                        "failed to sync iOS plugin release candidate '{}': {error}",
                        path.display()
                    ))
                })
            }
            PromotionNodeKind::Directory => {
                let identity = directory_identity(path, "iOS plugin release candidate")?;
                let mut pending = VecDeque::from([(path.to_path_buf(), 0_usize)]);
                let mut directories = Vec::new();
                let mut entries = 0_usize;
                let mut bytes = 0_u64;
                while let Some((directory, depth)) = pending.pop_front() {
                    check_cancellation(cancellation, "iOS plugin release candidate sync")?;
                    if depth > MAX_PROMOTION_TREE_DEPTH {
                        return Err(IosError::storage(format!(
                            "iOS plugin release candidate exceeds tree depth {MAX_PROMOTION_TREE_DEPTH}: {}",
                            path.display()
                        )));
                    }
                    directories.push(directory.clone());
                    for child in fs::read_dir(&directory).map_err(|error| {
                        IosError::storage(format!(
                            "failed to scan iOS plugin release candidate '{}': {error}",
                            directory.display()
                        ))
                    })? {
                        check_cancellation(cancellation, "iOS plugin release candidate sync")?;
                        let child = child.map_err(|error| {
                            IosError::storage(format!(
                                "failed to read iOS plugin release candidate under '{}': {error}",
                                directory.display()
                            ))
                        })?;
                        entries = entries.checked_add(1).ok_or_else(|| {
                            IosError::storage("iOS plugin release candidate entry count overflowed")
                        })?;
                        if entries > MAX_PROMOTION_TREE_ENTRIES {
                            return Err(IosError::storage(format!(
                                "iOS plugin release candidate exceeds {MAX_PROMOTION_TREE_ENTRIES} entries"
                            )));
                        }
                        let metadata = fs::symlink_metadata(child.path()).map_err(|error| {
                            IosError::storage(format!(
                                "failed to inspect iOS plugin release candidate '{}': {error}",
                                child.path().display()
                            ))
                        })?;
                        if metadata.file_type().is_dir() {
                            pending.push_back((child.path(), depth + 1));
                        } else if metadata.file_type().is_file() {
                            bytes = bytes.checked_add(metadata.len()).ok_or_else(|| {
                                IosError::storage("iOS plugin release candidate size overflowed")
                            })?;
                            if bytes > MAX_PROMOTION_TREE_BYTES {
                                return Err(IosError::storage(format!(
                                    "iOS plugin release candidate exceeds {MAX_PROMOTION_TREE_BYTES} bytes"
                                )));
                            }
                            open_regular_file_nofollow(&child.path())
                                .and_then(|file| file.sync_all())
                                .map_err(|error| {
                                    IosError::storage(format!(
                                        "failed to sync iOS plugin release candidate '{}': {error}",
                                        child.path().display()
                                    ))
                                })?;
                        } else {
                            return Err(IosError::storage(format!(
                                "iOS plugin release candidate contains a symlink or special file: {}",
                                child.path().display()
                            )));
                        }
                    }
                }
                for directory in directories.into_iter().rev() {
                    sync_directory(&directory)?;
                }
                if directory_identity(path, "iOS plugin release candidate")? != identity {
                    return Err(IosError::storage(format!(
                        "iOS plugin release candidate '{}' changed while it was synced",
                        path.display()
                    )));
                }
                Ok(())
            }
        }
    }

    fn check_snapshot_cancellation(
        cancellation: Option<&external_process::InterruptDeferral>,
        label: &str,
    ) -> Result<(), IosError> {
        if cancellation.is_some_and(external_process::InterruptDeferral::is_cancelled) {
            Err(IosError::worker(format!("{label} scan was cancelled")))
        } else {
            Ok(())
        }
    }

    fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, IosError> {
        let canonical = fs::canonicalize(path).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve {label} '{}': {error}",
                path.display()
            ))
        })?;
        let metadata = fs::symlink_metadata(&canonical).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                canonical.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink directory",
                canonical.display()
            )));
        }
        Ok(canonical)
    }

    fn directory_identity(path: &Path, label: &str) -> Result<FileIdentity, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink directory",
                path.display()
            )));
        }
        Ok(metadata_identity(&metadata))
    }

    fn optional_directory_identity(
        path: &Path,
        label: &str,
    ) -> Result<Option<FileIdentity>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(metadata_identity(&metadata))),
            Ok(_) => Err(IosError::storage(format!(
                "{label} '{}' is not a regular non-symlink directory",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn open_regular_file_nofollow(path: &Path) -> io::Result<File> {
        use rustix::fs::{Mode, OFlags, open};

        open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(io::Error::from)
    }

    fn exchange_paths(left: &Path, right: &Path) -> io::Result<()> {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE).map_err(io::Error::from)
    }

    fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(io::Error::from)
    }

    fn sync_directory(path: &Path) -> Result<(), IosError> {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to sync iOS plugin release directory '{}': {error}",
                    path.display()
                ))
            })
    }

    fn run_after_prepared_owner_test_hook(
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            wait_for_test_gate(
                "VESPER_TEST_PLUGIN_PREPARED_READY",
                "VESPER_TEST_PLUGIN_PREPARED_RELEASE",
                "iOS plugin release prepared owners",
                Some(cancellation),
            )?;
        }
        #[cfg(not(debug_assertions))]
        let _ = cancellation;
        Ok(())
    }

    fn run_after_runtime_guard_test_hook(
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            wait_for_test_gate(
                "VESPER_TEST_IOS_FFMPEG_RUNTIME_READY",
                "VESPER_TEST_IOS_FFMPEG_RUNTIME_RELEASE",
                "iOS FFmpeg runtime release build guard",
                Some(cancellation),
            )?;
        }
        #[cfg(not(debug_assertions))]
        let _ = cancellation;
        Ok(())
    }

    fn run_before_prepared_journal_replace_test_hook(
        role: PreparedOwnerRole,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        if role == PreparedOwnerRole::Build {
            wait_for_test_gate(
                "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_REPLACE_READY",
                "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_REPLACE_RELEASE",
                "iOS plugin prepared-owner journal replacement",
                None,
            )?;
        }
        #[cfg(not(debug_assertions))]
        let _ = role;
        Ok(())
    }

    fn run_after_raw_build_test_hook(
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            wait_for_test_gate(
                "VESPER_TEST_PLUGIN_RAW_OUTPUT_READY",
                "VESPER_TEST_PLUGIN_RAW_OUTPUT_RELEASE",
                "iOS plugin release raw output",
                Some(cancellation),
            )?;
        }
        #[cfg(not(debug_assertions))]
        let _ = cancellation;
        Ok(())
    }

    fn run_after_promotion_test_hook(
        promoted_records: usize,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            if promoted_records == 1 {
                wait_for_test_gate(
                    "VESPER_TEST_PLUGIN_COMMIT_READY",
                    "VESPER_TEST_PLUGIN_COMMIT_RELEASE",
                    "iOS plugin release partial promotion",
                    Some(cancellation),
                )?;
            }
            if let Some(value) = env::var_os("VESPER_TEST_PLUGIN_COMMIT_FAIL_AFTER") {
                let value = value
                    .to_str()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| {
                        IosError::worker(
                            "VESPER_TEST_PLUGIN_COMMIT_FAIL_AFTER must be an unsigned integer",
                        )
                    })?;
                if promoted_records == value {
                    return Err(IosError::storage(format!(
                        "injected iOS plugin release failure after {promoted_records} promotions"
                    )));
                }
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = (promoted_records, cancellation);
        Ok(())
    }

    fn run_after_durable_commit_test_hook(
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            wait_for_test_gate(
                "VESPER_TEST_PLUGIN_DURABLE_COMMIT_READY",
                "VESPER_TEST_PLUGIN_DURABLE_COMMIT_RELEASE",
                "iOS plugin release durable commit",
                Some(cancellation),
            )?;
        }
        #[cfg(not(debug_assertions))]
        let _ = cancellation;
        Ok(())
    }

    fn run_recovery_cleanup_test_hook(_owner: &Path) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            if env::var_os("VESPER_TEST_PLUGIN_RECOVERY_CLEANUP_FAIL").as_deref()
                == Some(OsStr::new("1"))
            {
                return Err(IosError::storage(
                    "injected iOS plugin release recovery cleanup failure",
                ));
            }
            wait_for_test_gate(
                "VESPER_TEST_PLUGIN_OWNER_CLEANUP_READY",
                "VESPER_TEST_PLUGIN_OWNER_CLEANUP_RELEASE",
                "iOS plugin release owner cleanup",
                None,
            )?;
        }
        Ok(())
    }

    fn run_journal_cleanup_test_hook(description: &str) -> Result<(), IosError> {
        #[cfg(debug_assertions)]
        {
            let (ready, release) = if description == "iOS plugin release journal" {
                (
                    "VESPER_TEST_PLUGIN_PROMOTION_JOURNAL_CLEANUP_READY",
                    "VESPER_TEST_PLUGIN_PROMOTION_JOURNAL_CLEANUP_RELEASE",
                )
            } else if description == "iOS plugin prepared-owner journal" {
                (
                    "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_CLEANUP_READY",
                    "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_CLEANUP_RELEASE",
                )
            } else {
                return Err(IosError::worker(format!(
                    "unsupported iOS plugin journal cleanup test description: {description}"
                )));
            };
            wait_for_test_gate(ready, release, description, None)?;
        }
        #[cfg(not(debug_assertions))]
        let _ = description;
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn wait_for_test_gate(
        ready_name: &str,
        release_name: &str,
        label: &str,
        cancellation: Option<&external_process::InterruptDeferral>,
    ) -> Result<(), IosError> {
        use std::time::{Duration, Instant};

        let Some(ready) = env::var_os(ready_name) else {
            return Ok(());
        };
        let release = env::var_os(release_name).ok_or_else(|| {
            IosError::worker(format!(
                "{release_name} is required when {ready_name} is set"
            ))
        })?;
        fs::write(PathBuf::from(ready), b"ready\n").map_err(|error| {
            IosError::storage(format!("failed to publish {label} test gate: {error}"))
        })?;
        let release = PathBuf::from(release);
        let deadline = Instant::now() + Duration::from_secs(30);
        while !release.exists()
            && !cancellation.is_some_and(external_process::InterruptDeferral::is_cancelled)
        {
            if Instant::now() >= deadline {
                return Err(IosError::worker(format!(
                    "timed out waiting for {label} test gate"
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    fn copy_regular_file(
        source: &Path,
        destination: &Path,
        maximum: u64,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        validate_regular_file(source, maximum, "iOS plugin release copy source")?;
        let mut input = File::open(source).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS plugin release copy source '{}': {error}",
                source.display()
            ))
        })?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin release copy '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut limited = (&mut input).take(maximum + 1);
        let copied = io::copy(&mut limited, &mut output).map_err(|error| {
            IosError::storage(format!(
                "failed to copy iOS plugin release file '{}': {error}",
                source.display()
            ))
        })?;
        if copied > maximum {
            return Err(IosError::conformance(format!(
                "iOS plugin release copy source '{}' exceeds {maximum} bytes",
                source.display()
            )));
        }
        check_cancellation(cancellation, "iOS plugin release file copy")?;
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync iOS plugin release copy '{}': {error}",
                destination.display()
            ))
        })?;
        let permissions = fs::metadata(source)
            .map_err(|error| {
                IosError::storage(format!("failed to inspect copy permissions: {error}"))
            })?
            .permissions();
        fs::set_permissions(destination, permissions).map_err(|error| {
            IosError::storage(format!(
                "failed to preserve iOS plugin release permissions '{}': {error}",
                destination.display()
            ))
        })
    }

    fn validate_regular_file(path: &Path, maximum: u64, label: &str) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a non-empty regular non-symlink file of at most {maximum} bytes",
                path.display()
            )));
        }
        Ok(())
    }

    fn write_text_file(path: &Path, value: &str) -> Result<(), IosError> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                IosError::storage(format!("failed to create '{}': {error}", path.display()))
            })?;
        file.write_all(value.as_bytes()).map_err(|error| {
            IosError::storage(format!("failed to write '{}': {error}", path.display()))
        })?;
        file.sync_all().map_err(|error| {
            IosError::storage(format!("failed to sync '{}': {error}", path.display()))
        })
    }

    fn sha256_file(path: &Path, maximum: u64) -> Result<String, IosError> {
        validate_regular_file(path, maximum, "iOS plugin checksum input")?;
        let mut file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open checksum input '{}': {error}",
                path.display()
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut input = (&mut file).take(maximum.saturating_add(1));
        let mut buffer = [0_u8; 64 * 1024];
        let mut copied = 0_u64;
        loop {
            let read = input.read(&mut buffer).map_err(|error| {
                IosError::storage(format!("failed to hash '{}': {error}", path.display()))
            })?;
            if read == 0 {
                break;
            }
            copied += u64::try_from(read).map_err(|_| {
                IosError::storage("iOS plugin checksum read size cannot be represented")
            })?;
            hasher.update(&buffer[..read]);
        }
        if copied > maximum {
            return Err(IosError::conformance(
                "iOS plugin checksum input grew during hashing",
            ));
        }
        Ok(hex::encode(hasher.finalize()))
    }

    fn read_directory_names(
        path: &Path,
        label: &str,
        maximum: usize,
    ) -> Result<BTreeSet<OsString>, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a regular non-symlink directory",
                path.display()
            )));
        }
        let mut names = BTreeSet::new();
        for entry in fs::read_dir(path).map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })? {
            if names.len() >= maximum {
                return Err(IosError::conformance(format!(
                    "{label} exceeds its {maximum}-entry limit"
                )));
            }
            names.insert(
                entry
                    .map_err(|error| IosError::storage(format!("failed to read {label}: {error}")))?
                    .file_name(),
            );
        }
        Ok(names)
    }

    fn platform_name(slice: IosPluginSlice) -> &'static str {
        match slice {
            IosPluginSlice::DeviceArm64 => "iPhoneOS",
            IosPluginSlice::SimulatorArm64 => "iPhoneSimulator",
        }
    }

    fn ffmpeg_slice_directory(slice: IosPluginSlice) -> &'static str {
        match slice {
            IosPluginSlice::DeviceArm64 => "ios",
            IosPluginSlice::SimulatorArm64 => "ios-simulator",
        }
    }

    fn xcframework_slice_identifier(slice: IosPluginSlice) -> &'static str {
        match slice {
            IosPluginSlice::DeviceArm64 => "ios-arm64",
            IosPluginSlice::SimulatorArm64 => "ios-arm64-simulator",
        }
    }

    fn metadata_file_name(slice: IosPluginSlice) -> &'static str {
        match slice {
            IosPluginSlice::DeviceArm64 => "ios-arm64-vesper-ffmpeg-build-metadata.txt",
            IosPluginSlice::SimulatorArm64 => {
                "ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt"
            }
        }
    }

    fn run_required_command(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let result = run_process(command, label, diagnostics, cancellation)?;
        diagnostics
            .write_all(&result.stdout)
            .map_err(diagnostics_error)?;
        diagnostics.flush().map_err(diagnostics_error)
    }

    fn run_process(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<BoundedProcessOutput, IosError> {
        command.stdin(Stdio::null());
        let result = external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_external_process_error)?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        diagnostics.flush().map_err(diagnostics_error)?;
        classify_status(result.status, label)?;
        Ok(result)
    }

    fn parse_process_utf8<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, IosError> {
        std::str::from_utf8(bytes)
            .map_err(|error| IosError::conformance(format!("{label} is not UTF-8: {error}")))
    }

    fn resolve_required_tools() -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            xcodebuild: require_path_command("xcodebuild")?,
            install_name_tool: require_path_command("install_name_tool")?,
            otool: require_path_command("otool")?,
            lipo: require_path_command("lipo")?,
            plutil: require_path_command("plutil")?,
            ditto: require_path_command("ditto")?,
        })
    }

    fn require_path_command(name: &str) -> Result<PathBuf, IosError> {
        use nix::unistd::{AccessFlags, access};

        let paths = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&paths)
            .find_map(|directory| {
                let directory = if directory.as_os_str().is_empty() {
                    env::current_dir().ok()?
                } else {
                    directory
                };
                let candidate = directory.join(name);
                fs::metadata(&candidate)
                    .is_ok_and(|metadata| {
                        metadata.is_file() && access(&candidate, AccessFlags::X_OK).is_ok()
                    })
                    .then_some(candidate)
            })
            .ok_or_else(|| IosError::compatibility(format!("Missing required command: {name}")))
    }

    fn require_repository_file(
        root: &Path,
        relative: &str,
        label: &str,
    ) -> Result<PathBuf, IosError> {
        let mut path = root.to_path_buf();
        for component in Path::new(relative).components() {
            let Component::Normal(component) = component else {
                return Err(IosError::compatibility(format!(
                    "{label} must use a normalized repository path"
                )));
            };
            path.push(component);
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} '{}': {error}",
                    path.display()
                ))
            })?;
            let last = path == root.join(relative);
            if (last && !metadata.file_type().is_file())
                || (!last && !metadata.file_type().is_dir())
            {
                return Err(IosError::compatibility(format!(
                    "{label} '{}' must be a regular non-symlink {}",
                    path.display(),
                    if last { "file" } else { "directory" }
                )));
            }
        }
        Ok(path)
    }

    fn read_bounded_utf8(path: &Path, maximum: u64, label: &str) -> Result<String, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() > maximum {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a regular non-symlink file of at most {maximum} bytes",
                path.display()
            )));
        }
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut bytes = Vec::new();
        file.take(maximum + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.len() as u64 > maximum {
            return Err(IosError::conformance(format!(
                "{label} exceeds {maximum} bytes"
            )));
        }
        String::from_utf8(bytes)
            .map_err(|error| IosError::conformance(format!("{label} must be UTF-8: {error}")))
    }

    fn ffmpeg_framework_name(library: &str) -> Result<&'static str, IosError> {
        match library {
            "avcodec" => Ok("VesperFFmpegAVCodec"),
            "avdevice" => Ok("VesperFFmpegAVDevice"),
            "avfilter" => Ok("VesperFFmpegAVFilter"),
            "avformat" => Ok("VesperFFmpegAVFormat"),
            "avutil" => Ok("VesperFFmpegAVUtil"),
            "postproc" => Ok("VesperFFmpegPostproc"),
            "swresample" => Ok("VesperFFmpegSWResample"),
            "swscale" => Ok("VesperFFmpegSWScale"),
            value => Err(IosError::conformance(format!(
                "unsupported iOS FFmpeg runtime library: {value}"
            ))),
        }
    }

    fn ffmpeg_bundle_identifier(library: &str) -> Result<&'static str, IosError> {
        match library {
            "avcodec" => Ok("io.github.umbrella22.vesper.ffmpeg.avcodec"),
            "avdevice" => Ok("io.github.umbrella22.vesper.ffmpeg.avdevice"),
            "avfilter" => Ok("io.github.umbrella22.vesper.ffmpeg.avfilter"),
            "avformat" => Ok("io.github.umbrella22.vesper.ffmpeg.avformat"),
            "avutil" => Ok("io.github.umbrella22.vesper.ffmpeg.avutil"),
            "postproc" => Ok("io.github.umbrella22.vesper.ffmpeg.postproc"),
            "swresample" => Ok("io.github.umbrella22.vesper.ffmpeg.swresample"),
            "swscale" => Ok("io.github.umbrella22.vesper.ffmpeg.swscale"),
            value => Err(IosError::conformance(format!(
                "unsupported iOS FFmpeg runtime library: {value}"
            ))),
        }
    }

    fn map_external_process_error(error: external_process::ExternalProcessError) -> IosError {
        match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        }
    }

    fn classify_status(status: ExitStatus, label: &str) -> Result<(), IosError> {
        if status.success() {
            Ok(())
        } else if status.code().is_none_or(|code| code >= 128) {
            Err(IosError::worker(format!(
                "{label} failed with status {status}"
            )))
        } else {
            Err(IosError::conformance(format!(
                "{label} failed with status {status}"
            )))
        }
    }

    fn check_cancellation(
        cancellation: &external_process::InterruptDeferral,
        label: &str,
    ) -> Result<(), IosError> {
        if cancellation.is_cancelled() {
            Err(IosError::worker(format!("{label} was cancelled")))
        } else {
            Ok(())
        }
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::storage(format!(
            "failed to write iOS plugin release output: {error}"
        ))
    }

    fn diagnostics_error(error: io::Error) -> IosError {
        IosError::storage(format!(
            "failed to write iOS plugin release diagnostics: {error}"
        ))
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::OsString;
        use std::fs;
        use std::path::{Path, PathBuf};

        use super::{
            DirectorySnapshotLimits, PREPARED_OWNER_JOURNAL_FILE, PREPARED_OWNER_JOURNAL_VERSION,
            PreparedOwner, PreparedOwnerJournal, PreparedOwnerRole, ReleaseFfmpeg, ReleaseRequest,
            RequiredTools, bounded_directory_content_fingerprint,
            collect_directory_entries_bounded, directory_identity, encode_transaction_id,
            ios_plugin_registry_fragment, persist_prepared_owner_journal,
            read_ios_plugin_descriptor, recover_prepared_owner_journal, snapshot_ffmpeg_inputs,
            stage_runtime, validate_release_slices, validate_snapshot_metadata,
            verify_runtime_archives,
        };
        use crate::ffmpeg_source::FfmpegSourcePolicy;
        use crate::ios::{IosError, IosErrorKind};
        use crate::ios_plugin::{IosPluginId, IosPluginSlice};
        use player_cli::PluginDescriptor;

        #[cfg(unix)]
        fn write_executable(path: &Path, source: &str) {
            use std::os::unix::fs::PermissionsExt;

            fs::write(path, source).expect("write fixture executable");
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("make fixture executable");
        }

        fn native_profile(
            declared_profile: &str,
            profile_hash: &str,
            libraries: &[String],
        ) -> crate::ffmpeg::NativeFfmpegProfile {
            crate::ffmpeg::NativeFfmpegProfile {
                build_profile: "custom".to_owned(),
                declared_profile: declared_profile.to_owned(),
                declared_platform: "ios",
                profile_hash: profile_hash.to_owned(),
                tls_backend: "none".to_owned(),
                enable_dash: false,
                libraries: libraries.to_vec(),
                demuxers: vec!["mov".to_owned()],
                muxers: vec!["mp4".to_owned()],
                protocols: vec!["file".to_owned(), "pipe".to_owned()],
                decoders: Vec::new(),
                parsers: Vec::new(),
                bsfs: Vec::new(),
                extra_configure_args: Vec::new(),
                force: false,
                forbid_network: true,
                forbid_openssl: true,
            }
        }

        #[test]
        fn runtime_archives_are_built_from_the_immutable_snapshot() {
            use sha2::{Digest, Sha256};

            let directory = tempfile::tempdir().expect("create runtime staging fixture");
            let policy = FfmpegSourcePolicy::test_fixture();
            let source_lock = policy.release().clone();
            let source_directory = directory.path().join("mutable FFmpeg source");
            let libraries = vec!["avcodec".to_owned(), "avutil".to_owned()];
            let native_profile = native_profile("fixture", "fixture-hash", &libraries);
            let ffmpeg = ReleaseFfmpeg {
                declared_profile: "fixture".to_owned(),
                profile_hash: "fixture-hash".to_owned(),
                output_directory: source_directory.clone(),
                runtime_libraries: libraries.clone(),
                raw_arguments: Vec::new(),
                source_lock,
                native_profile,
            };
            let slices = vec![IosPluginSlice::DeviceArm64, IosPluginSlice::SimulatorArm64];
            for slice in &slices {
                let root = source_directory.join(super::ffmpeg_slice_directory(*slice));
                fs::create_dir_all(root.join("lib/arm64"))
                    .expect("create fixture FFmpeg library directory");
                let mut checksums = String::new();
                for library in &libraries {
                    let payload = format!("fixture {} {library}\n", slice.as_str());
                    fs::write(
                        root.join("lib/arm64").join(format!("lib{library}.dylib")),
                        payload.as_bytes(),
                    )
                    .expect("write fixture FFmpeg library");
                    checksums.push_str(&format!(
                        "{library}_sha256={}\n",
                        hex::encode(Sha256::digest(payload.as_bytes()))
                    ));
                }
                fs::write(root.join("vesper-ffmpeg-library-sha256.txt"), checksums)
                    .expect("write fixture FFmpeg checksums");
                fs::write(
                    root.join("vesper-ffmpeg-build-metadata.txt"),
                    format!(
                        concat!(
                            "Vesper FFmpeg build metadata v2\n",
                            "platform=apple\n",
                            "target={}\n",
                            "declared_profile=fixture\n",
                            "declared_platform=ios\n",
                            "profile_hash=fixture-hash\n",
                            "profile=custom\n",
                            "ffmpeg_version={}\n",
                            "source_url={}\n",
                            "source_sha256={}\n",
                        ),
                        super::ffmpeg_metadata_target(*slice),
                        ffmpeg.source_lock.version(),
                        ffmpeg.source_lock.source_url(),
                        ffmpeg.source_lock.source_sha256(),
                    ),
                )
                .expect("write fixture FFmpeg metadata");
            }

            let cancellation = crate::external_process::InterruptDeferral::start(
                "iOS FFmpeg runtime staging test",
            )
            .expect("start runtime staging cancellation scope");
            let snapshot_directory = directory.path().join("immutable snapshot");
            let snapshot =
                snapshot_ffmpeg_inputs(&ffmpeg, &slices, &snapshot_directory, &cancellation)
                    .expect("snapshot FFmpeg runtime inputs");
            fs::remove_dir_all(&source_directory)
                .expect("remove mutable FFmpeg source after snapshot");

            let tools_directory = directory.path().join("tools");
            fs::create_dir(&tools_directory).expect("create runtime staging tools");
            write_executable(
                &tools_directory.join("install_name_tool"),
                "#!/bin/sh\nexit 0\n",
            );
            write_executable(
                &tools_directory.join("lipo"),
                "#!/bin/sh\nif [ \"${1:-}\" = -archs ]; then printf 'arm64\\n'; fi\n",
            );
            write_executable(
                &tools_directory.join("otool"),
                r#"#!/bin/sh
set -eu
case "${1:-}" in
  -l) printf '          path @loader_path/.. (offset 12)\n' ;;
  -D)
    name=$(/usr/bin/basename "$2")
    printf '%s:\n@rpath/%s.framework/%s\n' "$2" "$name" "$name"
    ;;
  -L)
    printf '%s:\n' "$2"
    case "$2" in
      */VesperFFmpegAVCodec)
        printf '    @rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil (compatibility version 1.0.0, current version 1.0.0)\n'
        ;;
    esac
    ;;
esac
"#,
            );
            write_executable(
                &tools_directory.join("xcodebuild"),
                r#"#!/bin/bash
set -eu
if [ "${1:-}" = -version ]; then
  printf 'Xcode 26.0\nBuild version 17A000\n'
  exit 0
fi
if [ "${1:-}" = -sdk ]; then
  case "${4:-}" in
    SDKVersion) printf '26.0\n' ;;
    ProductBuildVersion) printf '23A000\n' ;;
  esac
  exit 0
fi
[ "${1:-}" = -create-xcframework ]
frameworks=()
output=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -framework) frameworks+=("$2"); shift 2 ;;
    -output) output=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ "${#frameworks[@]}" -eq 2 ]
[ -n "$output" ]
name=$(/usr/bin/basename "${frameworks[0]}" .framework)
entries=
separator=
for framework in "${frameworks[@]}"; do
  case "$framework" in
    */ios-simulator-arm64/*)
      identifier=ios-arm64-simulator
      variant=',"SupportedPlatformVariant":"simulator"'
      ;;
    *)
      identifier=ios-arm64
      variant=
      ;;
  esac
  /bin/mkdir -p "$output/$identifier"
  /bin/cp -R "$framework" "$output/$identifier/$name.framework"
  entry='{"BinaryPath":"'"$name"'.framework/'"$name"'","LibraryIdentifier":"'"$identifier"'","LibraryPath":"'"$name"'.framework","SupportedArchitectures":["arm64"],"SupportedPlatform":"ios"'"$variant"'}'
  entries="$entries$separator$entry"
  separator=,
done
printf '%s\n' '{"AvailableLibraries":['"$entries"'],"XCFrameworkFormatVersion":"1.0"}' > "$output/Info.plist"
"#,
            );
            let tools = RequiredTools {
                xcodebuild: tools_directory.join("xcodebuild"),
                install_name_tool: tools_directory.join("install_name_tool"),
                otool: tools_directory.join("otool"),
                lipo: tools_directory.join("lipo"),
                plutil: PathBuf::from("/usr/bin/plutil"),
                ditto: PathBuf::from("/usr/bin/ditto"),
            };
            let output_directory = directory.path().join("runtime archives");
            fs::create_dir(&output_directory).expect("create runtime archive output");
            let request = ReleaseRequest {
                output_directory: output_directory.clone(),
                profile: Some("fixture".to_owned()),
                dry_run: false,
                slices: slices.clone(),
                ffmpeg: None,
                version: "0.4.0".to_owned(),
                build: "4".to_owned(),
                minimum_os: "17.0".to_owned(),
            };
            let mut diagnostics = Vec::new();
            stage_runtime(
                &request,
                &ffmpeg,
                &snapshot,
                &directory.path().join("runtime build"),
                &output_directory,
                &tools,
                &mut diagnostics,
                &cancellation,
            )
            .expect("stage FFmpeg runtime archives from snapshot");
            verify_runtime_archives(&output_directory, &ffmpeg, &snapshot, &slices)
                .expect("verify staged FFmpeg runtime archives");
            for library in &libraries {
                let framework = super::ffmpeg_framework_name(library)
                    .expect("resolve fixture FFmpeg framework");
                assert!(
                    output_directory
                        .join(format!("{framework}.xcframework.zip"))
                        .is_file()
                );
            }
            assert!(!cancellation.finish());
        }

        #[test]
        fn prepared_owner_recovery_preserves_journal_while_output_parent_is_unavailable() {
            let directory = tempfile::tempdir().expect("create prepared owner recovery fixture");
            let root = directory.path().join("repository");
            let build_parent = root.join("lib/ios/VesperPlayerKit/.build");
            let state_directory = build_parent.join("vesper-cli-state");
            let output_directory = directory.path().join("ephemeral aggregate output");
            for path in [&state_directory, &output_directory] {
                fs::create_dir_all(path).expect("create prepared owner recovery directory");
            }

            let root = fs::canonicalize(&root).expect("canonical fixture repository");
            let build_parent =
                fs::canonicalize(&build_parent).expect("canonical fixture build parent");
            let state_directory =
                fs::canonicalize(&state_directory).expect("canonical fixture state directory");
            let output_directory =
                fs::canonicalize(&output_directory).expect("canonical fixture output directory");
            let transaction_id = [7_u8; 16];
            let suffix = encode_transaction_id(transaction_id);
            let build_owner = build_parent.join(format!(".vesper-ios-plugin-release-{suffix}"));
            let asset_owner = output_directory.join(format!(".vesper-ios-plugin-assets-{suffix}"));
            fs::create_dir(&build_owner).expect("create prepared build owner");
            fs::write(build_owner.join("retained.txt"), b"retained build output\n")
                .expect("write retained build output");
            fs::create_dir(&asset_owner).expect("create prepared asset owner");
            fs::write(asset_owner.join("staged.zip"), b"staged asset\n")
                .expect("write staged asset");

            let journal = PreparedOwnerJournal {
                version: PREPARED_OWNER_JOURNAL_VERSION,
                transaction_id,
                root: root.clone(),
                root_identity: directory_identity(&root, "fixture repository")
                    .expect("fixture repository identity"),
                state_directory: state_directory.clone(),
                state_directory_identity: directory_identity(
                    &state_directory,
                    "fixture state directory",
                )
                .expect("fixture state identity"),
                build_parent: build_parent.clone(),
                build_parent_identity: directory_identity(&build_parent, "fixture build parent")
                    .expect("fixture build parent identity"),
                output_directory: output_directory.clone(),
                output_directory_identity: directory_identity(
                    &output_directory,
                    "fixture output directory",
                )
                .expect("fixture output identity"),
                plugin_id: IosPluginId::FrameProcessorDiagnostic.as_str().to_owned(),
                owners: vec![
                    PreparedOwner {
                        role: PreparedOwnerRole::Build,
                        path: build_owner.clone(),
                        identity: Some(
                            directory_identity(&build_owner, "fixture build owner")
                                .expect("fixture build owner identity"),
                        ),
                        parent: build_parent.clone(),
                        parent_identity: directory_identity(&build_parent, "fixture build parent")
                            .expect("fixture build parent identity"),
                    },
                    PreparedOwner {
                        role: PreparedOwnerRole::Assets,
                        path: asset_owner.clone(),
                        identity: Some(
                            directory_identity(&asset_owner, "fixture asset owner")
                                .expect("fixture asset owner identity"),
                        ),
                        parent: output_directory.clone(),
                        parent_identity: directory_identity(
                            &output_directory,
                            "fixture output parent",
                        )
                        .expect("fixture output parent identity"),
                    },
                ],
            };
            let journal_path = state_directory.join(PREPARED_OWNER_JOURNAL_FILE);
            persist_prepared_owner_journal(&journal_path, &journal, None)
                .expect("persist prepared owner journal");

            let unavailable_output = directory.path().join("temporarily unavailable output");
            fs::rename(&output_directory, &unavailable_output)
                .expect("simulate a temporarily unavailable external output parent");
            let validation_error = super::validate_prepared_owner_journal(&root, &journal)
                .expect_err("normal validation must reject a missing output parent");
            assert!(
                validation_error
                    .to_string()
                    .contains("output directory is missing")
            );
            let recovery_error = recover_prepared_owner_journal(&root, &journal_path, None)
                .expect_err("recovery must preserve ownership while the output parent is missing");
            assert!(
                recovery_error
                    .to_string()
                    .contains("output directory is missing")
            );
            assert!(build_owner.is_dir());
            assert!(journal_path.is_file());
            assert!(
                unavailable_output
                    .join(asset_owner.file_name().expect("asset owner name"))
                    .is_dir()
            );

            fs::rename(&unavailable_output, &output_directory)
                .expect("restore the original output parent identity");
            recover_prepared_owner_journal(&root, &journal_path, None)
                .expect("recover owners after the original output parent returns");
            assert!(!build_owner.exists());
            assert!(!asset_owner.exists());
            assert!(output_directory.is_dir());
            assert!(!journal_path.exists());
        }

        #[test]
        fn prepared_owner_recovery_rejects_a_recreated_output_parent() {
            let directory = tempfile::tempdir().expect("create prepared owner identity fixture");
            let root = directory.path().join("repository");
            let build_parent = root.join("lib/ios/VesperPlayerKit/.build");
            let state_directory = build_parent.join("vesper-cli-state");
            let output_directory = directory.path().join("ephemeral aggregate output");
            for path in [&state_directory, &output_directory] {
                fs::create_dir_all(path).expect("create prepared owner identity directory");
            }

            let root = fs::canonicalize(&root).expect("canonical fixture repository");
            let build_parent =
                fs::canonicalize(&build_parent).expect("canonical fixture build parent");
            let state_directory =
                fs::canonicalize(&state_directory).expect("canonical fixture state directory");
            let output_directory =
                fs::canonicalize(&output_directory).expect("canonical fixture output directory");
            let transaction_id = [9_u8; 16];
            let suffix = encode_transaction_id(transaction_id);
            let build_owner = build_parent.join(format!(".vesper-ios-plugin-release-{suffix}"));
            let asset_owner = output_directory.join(format!(".vesper-ios-plugin-assets-{suffix}"));
            fs::create_dir(&build_owner).expect("create prepared build owner");
            fs::create_dir(&asset_owner).expect("create prepared asset owner");

            let journal = PreparedOwnerJournal {
                version: PREPARED_OWNER_JOURNAL_VERSION,
                transaction_id,
                root: root.clone(),
                root_identity: directory_identity(&root, "fixture repository")
                    .expect("fixture repository identity"),
                state_directory: state_directory.clone(),
                state_directory_identity: directory_identity(
                    &state_directory,
                    "fixture state directory",
                )
                .expect("fixture state identity"),
                build_parent: build_parent.clone(),
                build_parent_identity: directory_identity(&build_parent, "fixture build parent")
                    .expect("fixture build parent identity"),
                output_directory: output_directory.clone(),
                output_directory_identity: directory_identity(
                    &output_directory,
                    "fixture output directory",
                )
                .expect("fixture output identity"),
                plugin_id: IosPluginId::FrameProcessorDiagnostic.as_str().to_owned(),
                owners: vec![
                    PreparedOwner {
                        role: PreparedOwnerRole::Build,
                        path: build_owner.clone(),
                        identity: Some(
                            directory_identity(&build_owner, "fixture build owner")
                                .expect("fixture build owner identity"),
                        ),
                        parent: build_parent.clone(),
                        parent_identity: directory_identity(&build_parent, "fixture build parent")
                            .expect("fixture build parent identity"),
                    },
                    PreparedOwner {
                        role: PreparedOwnerRole::Assets,
                        path: asset_owner.clone(),
                        identity: Some(
                            directory_identity(&asset_owner, "fixture asset owner")
                                .expect("fixture asset owner identity"),
                        ),
                        parent: output_directory.clone(),
                        parent_identity: directory_identity(
                            &output_directory,
                            "fixture output parent",
                        )
                        .expect("fixture output parent identity"),
                    },
                ],
            };
            let journal_path = state_directory.join(PREPARED_OWNER_JOURNAL_FILE);
            persist_prepared_owner_journal(&journal_path, &journal, None)
                .expect("persist prepared owner journal");

            fs::remove_dir_all(&output_directory).expect("remove original output parent");
            fs::create_dir(&output_directory).expect("recreate output parent with a new identity");
            fs::write(
                output_directory.join("must-not-be-touched.txt"),
                b"replacement\n",
            )
            .expect("write replacement parent sentinel");

            let error = recover_prepared_owner_journal(&root, &journal_path, None)
                .expect_err("reject a recreated output parent");
            assert!(error.to_string().contains("changed identity"));
            assert!(build_owner.is_dir());
            assert!(journal_path.is_file());
            assert_eq!(
                fs::read(output_directory.join("must-not-be-touched.txt"))
                    .expect("read replacement parent sentinel"),
                b"replacement\n"
            );
        }

        #[test]
        fn distributable_plugin_release_requires_device_and_simulator_slices() {
            validate_release_slices(
                &[IosPluginSlice::DeviceArm64, IosPluginSlice::SimulatorArm64],
                "fixture plugin",
            )
            .expect("complete release slice set");

            for slices in [
                vec![IosPluginSlice::DeviceArm64],
                vec![IosPluginSlice::SimulatorArm64],
                Vec::new(),
            ] {
                let error = validate_release_slices(&slices, "fixture plugin")
                    .expect_err("incomplete release slice set must fail");
                assert_eq!(error.kind(), IosErrorKind::Compatibility);
                assert!(error.to_string().contains("ios-arm64"));
                assert!(error.to_string().contains("ios-simulator-arm64"));
            }
        }

        #[test]
        fn ffmpeg_snapshot_metadata_requires_the_canonical_source_lock() {
            let policy = FfmpegSourcePolicy::test_fixture();
            let source_lock = policy.release().clone();
            let libraries = vec!["avcodec".to_owned()];
            let native_profile = native_profile("source-normalizer", "custom-fixture", &libraries);
            let ffmpeg = ReleaseFfmpeg {
                declared_profile: "source-normalizer".to_owned(),
                profile_hash: "custom-fixture".to_owned(),
                output_directory: PathBuf::from("fixture"),
                runtime_libraries: libraries,
                raw_arguments: vec![OsString::from("--enable-avcodec")],
                source_lock,
                native_profile,
            };
            let metadata = format!(
                concat!(
                    "Vesper FFmpeg build metadata v2\n",
                    "platform=apple\n",
                    "target=ios-arm64\n",
                    "declared_profile=source-normalizer\n",
                    "declared_platform=ios\n",
                    "profile_hash=custom-fixture\n",
                    "profile=custom\n",
                    "ffmpeg_version={}\n",
                    "source_url={}\n",
                    "source_sha256={}\n",
                ),
                ffmpeg.source_lock.version(),
                ffmpeg.source_lock.source_url(),
                ffmpeg.source_lock.source_sha256(),
            );
            let path = Path::new("vesper-ffmpeg-build-metadata.txt");
            assert_eq!(
                validate_snapshot_metadata(
                    metadata.as_bytes(),
                    &ffmpeg,
                    IosPluginSlice::DeviceArm64,
                    path,
                )
                .expect("canonical FFmpeg metadata"),
                "custom"
            );

            for (canonical, replacement, key) in [
                (
                    ffmpeg.source_lock.version().to_string(),
                    "8.1.999".to_owned(),
                    "ffmpeg_version",
                ),
                (
                    ffmpeg.source_lock.source_url().to_owned(),
                    "https://ffmpeg.org/releases/ffmpeg-8.1.999.tar.xz".to_owned(),
                    "source_url",
                ),
                (
                    ffmpeg.source_lock.source_sha256().to_owned(),
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                    "source_sha256",
                ),
            ] {
                let altered = metadata.replacen(&canonical, &replacement, 1);
                let error = validate_snapshot_metadata(
                    altered.as_bytes(),
                    &ffmpeg,
                    IosPluginSlice::DeviceArm64,
                    path,
                )
                .expect_err("reject noncanonical FFmpeg snapshot metadata");
                assert!(error.to_string().contains(&format!("unexpected {key}")));
            }
        }

        #[test]
        fn embedded_registry_fragment_tracks_the_framework_slice_target() {
            let descriptor = PluginDescriptor::from_toml(
                r#"
schema_version = 1

[plugin]
id = "dev.vesper.fixture"
name = "Fixture"
version = "1.2.3"
description = "Fixture plugin"
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.fixture.post-download"
interface_major = 1
interface_minor = 0
stability = "stable"
"#,
            )
            .expect("valid plugin descriptor")
            .canonicalize()
            .expect("canonical plugin descriptor");

            for (slice, expected_target) in [
                (IosPluginSlice::DeviceArm64, "aarch64-apple-ios"),
                (IosPluginSlice::SimulatorArm64, "aarch64-apple-ios-sim"),
            ] {
                let fragment = ios_plugin_registry_fragment(
                    &descriptor,
                    IosPluginId::RemuxFfmpeg,
                    slice,
                    "17.4",
                )
                .expect("registry fragment");
                let value: serde_json::Value =
                    serde_json::from_slice(fragment.canonical_json()).expect("registry JSON");

                assert_eq!(value["target"], expected_target);
                assert_eq!(value["architecture"], "arm64");
                assert_eq!(value["minimum_os"], "17.4");
                assert_eq!(
                    value["artifacts"][0]["locator"]["name"],
                    "VesperPlayerRemuxFfmpegPlugin"
                );
                assert_eq!(
                    value["artifacts"][0]["locator"]["bundle_identifier"],
                    "io.github.umbrella22.vesper.player.remux-ffmpeg-plugin"
                );
            }
        }

        #[test]
        fn ios_release_reads_the_complete_plugin_project_manifest() {
            let directory = tempfile::tempdir().expect("create plugin project fixture");
            let manifest_directory = directory.path().join("plugins/frame-processor-diagnostic");
            fs::create_dir_all(&manifest_directory).expect("create plugin manifest directory");
            fs::write(
                manifest_directory.join("vesper-plugin.toml"),
                r#"
schema_version = 1

[plugin]
id = "dev.vesper.frame-processor-diagnostic"
name = "Vesper Diagnostic Frame Processor"
version = "0.4.0"
description = "Diagnostic FrameProcessor fixture"
license = "Apache-2.0"
publisher = "io.github.umbrella22"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "fc050597-b7b7-5c81-83b9-b42555f8b825"
instance_id = "dev.vesper.frame-processor-diagnostic.frame"
interface_major = 1
interface_minor = 0
stability = "experimental"

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "dist/libvesper_frame_processor_diagnostic.dylib"
path = "artifacts/aarch64-apple-darwin/libvesper_frame_processor_diagnostic.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "fc050597-b7b7-5c81-83b9-b42555f8b825", instance_id = "dev.vesper.frame-processor-diagnostic.frame" }]

[[package_files]]
source = "../../LICENSE"
path = "licenses/LICENSE"
kind = "license"

[[package_files]]
source = "../../NOTICE"
path = "notices/NOTICE"
kind = "notice"
"#,
            )
            .expect("write complete plugin project manifest");

            let descriptor =
                read_ios_plugin_descriptor(directory.path(), IosPluginId::FrameProcessorDiagnostic)
                    .expect("read complete plugin project manifest");

            assert_eq!(
                descriptor.descriptor().plugin.id,
                "dev.vesper.frame-processor-diagnostic"
            );
            let canonical_json =
                std::str::from_utf8(descriptor.json()).expect("canonical descriptor UTF-8");
            assert!(!canonical_json.contains("artifacts"));
            assert!(!canonical_json.contains("package_files"));
        }

        #[test]
        fn ffmpeg_build_content_fingerprint_includes_header_bytes() {
            let directory = tempfile::tempdir().expect("create fingerprint fixture");
            let include = directory.path().join("include/libavutil");
            fs::create_dir_all(&include).expect("create fingerprint include directory");
            let header = include.join("avutil.h");
            fs::write(&header, b"header generation one\n").expect("write initial header");

            let limits = DirectorySnapshotLimits {
                maximum_entries: 4,
                maximum_depth: 4,
                maximum_bytes: 1024,
                digest_domain: b"vesper-ios-plugin-test-content-v1\0",
            };
            let initial = bounded_directory_content_fingerprint(
                directory.path(),
                "FFmpeg fingerprint fixture",
                None,
                limits,
            )
            .expect("fingerprint initial header");

            fs::write(&header, b"header generation two\n").expect("replace header in place");
            let changed = bounded_directory_content_fingerprint(
                directory.path(),
                "FFmpeg fingerprint fixture",
                None,
                limits,
            )
            .expect("fingerprint changed header");

            assert_ne!(initial, changed);
        }

        #[test]
        fn directory_enumeration_rejects_one_entry_over_its_limit() {
            let directory = tempfile::tempdir().expect("create enumeration fixture");
            for name in ["a", "b", "c"] {
                fs::write(directory.path().join(name), name).expect("write enumeration entry");
            }

            let error = collect_directory_entries_bounded(
                directory.path(),
                2,
                "bounded enumeration fixture",
                IosError::conformance("bounded enumeration limit reached"),
            )
            .expect_err("reject an entry beyond the enumeration limit");

            assert_eq!(error.kind(), IosErrorKind::Conformance);
            assert!(
                error
                    .to_string()
                    .contains("bounded enumeration limit reached")
            );
        }
    }
}
