// Public command stubs remain available on every host while their release
// implementation is compiled only on macOS.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::Write;
use std::path::Path;

use crate::ios::IosError;

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    ensure_macos("optional iOS plugin release verification requires macOS")
}

pub(crate) fn ensure_release_supported_host() -> Result<(), IosError> {
    ensure_macos("iOS release verification requires macOS")
}

fn ensure_macos(message: &'static str) -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(message))
    }
}

pub(crate) fn verify_optional_plugins_release(
    root: &Path,
    release_directory: Option<&Path>,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::verify(root, release_directory, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, release_directory, output, diagnostics);
        unreachable!("the host gate rejects non-macOS verification")
    }
}

pub(crate) fn verify_release(
    root: &Path,
    release_directory: Option<&Path>,
    complete: bool,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_release_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::verify_release(root, release_directory, complete, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, release_directory, complete, output, diagnostics);
        unreachable!("the host gate rejects non-macOS verification")
    }
}

pub(crate) fn stage_ffmpeg_compliance_assets(
    root: &Path,
    framework_directory: &Path,
    output_directory: &Path,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::stage_ffmpeg_compliance_assets(root, framework_directory, output_directory)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, framework_directory, output_directory);
        unreachable!("the host gate rejects non-macOS compliance staging")
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn prepare_verified_optional_release_snapshot(
    root: &Path,
    release_directory: &Path,
) -> Result<VerifiedOptionalReleaseSnapshot, IosError> {
    implementation::prepare_verified_optional_release_snapshot(root, release_directory)
}

#[cfg(target_os = "macos")]
pub(crate) struct VerifiedOptionalReleaseSnapshot {
    prepared: implementation::PreparedRelease,
}

#[cfg(target_os = "macos")]
pub(crate) use implementation::OptionalReleaseArchiveEvidence;

#[cfg(target_os = "macos")]
pub(crate) fn optional_frameworks() -> &'static [&'static str] {
    &implementation::OPTIONAL_FRAMEWORKS
}

#[cfg(target_os = "macos")]
impl VerifiedOptionalReleaseSnapshot {
    pub(crate) fn canonical_release_directory(&self) -> &Path {
        &self.prepared.canonical_release_directory
    }

    pub(crate) fn optional_frameworks(&self) -> &'static [&'static str] {
        optional_frameworks()
    }

    pub(crate) fn archive_evidence(&self) -> &[OptionalReleaseArchiveEvidence] {
        &self.prepared.optional_archives
    }

    pub(crate) fn materialize_optional_package(&self, destination: &Path) -> Result<(), IosError> {
        implementation::materialize_swift_package(&self.prepared, destination)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn preflight_plugin_framework_archive(
    path: &Path,
    framework: &str,
    uses_ffmpeg: bool,
    slices: &[crate::ios_plugin::IosPluginSlice],
) -> Result<(), IosError> {
    implementation::preflight_plugin_framework_archive(path, framework, uses_ffmpeg, slices)
}

#[cfg(target_os = "macos")]
pub(crate) fn preflight_runtime_framework_archive(
    path: &Path,
    framework: &str,
) -> Result<(), IosError> {
    implementation::preflight_runtime_framework_archive(path, framework)
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::{BTreeMap, BTreeSet};
    use std::env;
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use player_plugin_loader::EmbeddedPluginRegistry;
    use semver::Version;
    use sha2::{Digest, Sha256};
    use tempfile::{NamedTempFile, TempDir};
    use unicode_casefold::UnicodeCaseFold;
    use unicode_normalization::UnicodeNormalization;
    use xz2::read::XzDecoder;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

    use super::IosError;
    use crate::external_process::{self, ExternalProcessErrorKind};
    use crate::ffmpeg_source::{
        FfmpegSourcePolicy, FfmpegSourcePolicyError, FfmpegSourcePolicyErrorKind,
        source_asset_version, source_url_for_version,
    };

    const COMPLIANCE_ASSET: &str = "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip";
    const COMPLIANCE_ROOT: &str = "VesperPlayerOptionalPlugins-FFmpeg-Compliance";

    pub(super) const OPTIONAL_FRAMEWORKS: [&str; 7] = [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
        "VesperPlayerRemuxFfmpegPlugin",
        "VesperPlayerSourceNormalizerFfmpegPlugin",
        "VesperPlayerDecoderVideoToolboxPlugin",
        "VesperPlayerFrameProcessorDiagnosticPlugin",
    ];
    const FFMPEG_FRAMEWORKS: [&str; 5] = [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
        "VesperPlayerRemuxFfmpegPlugin",
        "VesperPlayerSourceNormalizerFfmpegPlugin",
    ];
    const OPTIONAL_SLICES: [(&str, &str); 2] = [
        ("ios-arm64", "ios-arm64-vesper-ffmpeg-build-metadata.txt"),
        (
            "ios-arm64-simulator",
            "ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
        ),
    ];
    const CORE_ASSETS: [&str; 3] = [
        "VesperPlayerKit-ios-arm64.framework.zip",
        "VesperPlayerKit-ios-simulator-arm64.framework.zip",
        "VesperPlayerKit.xcframework.zip",
    ];

    // These release-policy limits leave measured headroom over the 2026-07-30 fixtures:
    // 57 ZIP entries / 3.1 MiB expanded and 10,230 tar entries / 96.3 MiB expanded.
    const MAX_RELEASE_ASSETS: usize = 16;
    const MAX_RELEASE_ASSET_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_RELEASE_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
    const MAX_CORE_RELEASE_ASSET_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_CORE_RELEASE_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_CORE_ZIP_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_CORE_ZIP_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_ZIP_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_ZIP_ENTRIES: usize = 256;
    const MAX_ZIP_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
    const MAX_ZIP_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_ZIP_COMPRESSION_RATIO: u64 = 100;
    const MAX_COMPLIANCE_ENTRIES: usize = 64;
    const MAX_COMPLIANCE_ENTRY_BYTES: u64 = 1024 * 1024;
    const MAX_COMPLIANCE_EXPANDED_BYTES: u64 = 2 * 1024 * 1024;
    const MAX_ARCHIVE_PATH_BYTES: usize = 1024;
    const MAX_ARCHIVE_PATH_DEPTH: usize = 16;
    const MAX_APPLEDOUBLE_ENTRY_BYTES: u64 = 64 * 1024;
    const MAX_SMALL_RECORD_BYTES: u64 = 64 * 1024;
    const MAX_TAR_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_TAR_ENTRIES: usize = 20_000;
    const MAX_TAR_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
    const MAX_TAR_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_TAR_STREAM_BYTES: u64 = 160 * 1024 * 1024;
    const MAX_TAR_COMPRESSION_RATIO: u64 = 32;
    const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
    const UNIX_REGULAR_FILE: u32 = 0o100000;
    const UNIX_DIRECTORY: u32 = 0o040000;
    const ZIP_CENTRAL_DIRECTORY_HEADER: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    const ZIP_END_OF_CENTRAL_DIRECTORY: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const ZIP_EOCD_MINIMUM_BYTES: usize = 22;
    const ZIP_MAXIMUM_COMMENT_BYTES: usize = u16::MAX as usize;

    const BUILD_METADATA_KEYS: [&str; 22] = [
        "platform",
        "target",
        "profile",
        "declared_profile",
        "declared_platform",
        "profile_hash",
        "tls_backend",
        "enable_dash",
        "libraries",
        "demuxers",
        "muxers",
        "protocols",
        "decoders",
        "parsers",
        "bsfs",
        "external_dependencies",
        "license_flags",
        "ffmpeg_version",
        "source_archive",
        "source_url",
        "source_sha256",
        "configure_line",
    ];
    const SOURCE_RECORD_KEYS: [&str; 11] = [
        "component",
        "ffmpeg_version",
        "license_mode",
        "linkage",
        "declared_profile",
        "profile_hash",
        "source_url",
        "source_asset",
        "source_sha256",
        "local_changes",
        "external_dependencies",
    ];
    const SLICE_INVARIANT_KEYS: [&str; 20] = [
        "platform",
        "profile",
        "declared_profile",
        "declared_platform",
        "profile_hash",
        "tls_backend",
        "enable_dash",
        "libraries",
        "demuxers",
        "muxers",
        "protocols",
        "decoders",
        "parsers",
        "bsfs",
        "external_dependencies",
        "license_flags",
        "ffmpeg_version",
        "source_archive",
        "source_url",
        "source_sha256",
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ArchiveNodeKind {
        File,
        Directory,
    }

    #[derive(Debug)]
    struct ZipPolicy {
        expected_nodes: Option<BTreeMap<String, ArchiveNodeKind>>,
        required_root: Option<&'static str>,
        maximum_entries: usize,
        maximum_entry_bytes: u64,
        maximum_expanded_bytes: u64,
    }

    #[derive(Debug)]
    pub(super) struct PreparedRelease {
        _temporary: TempDir,
        pub(super) directory: PathBuf,
        pub(super) canonical_release_directory: PathBuf,
        pub(super) optional_archives: Vec<OptionalReleaseArchiveEvidence>,
        optional_report: Option<OptionalReleaseReport>,
    }

    #[derive(Debug, Clone, serde::Serialize)]
    pub(crate) struct OptionalReleaseArchiveEvidence {
        pub(crate) archive_name: String,
        pub(crate) product_name: String,
        pub(crate) release_archive_sha256: String,
        pub(crate) verified_archive_sha256: String,
    }

    #[derive(Debug)]
    struct OptionalReleaseReport {
        profile_hash: String,
        source_asset: String,
        source_sha256: String,
    }

    #[derive(Debug)]
    struct OpenedReleaseAsset {
        file: File,
        identity: FileIdentity,
        display_path: PathBuf,
    }

    #[derive(Debug)]
    struct OpenedReleaseAssets {
        directory: File,
        directory_identity: FileIdentity,
        display_path: PathBuf,
        files: BTreeMap<String, OpenedReleaseAsset>,
    }

    impl OpenedReleaseAssets {
        fn verify_unchanged(&self) -> Result<(), IosError> {
            let directory_metadata = self.directory.metadata().map_err(|error| {
                IosError::storage(format!(
                    "failed to re-inspect optional iOS release directory '{}': {error}",
                    self.display_path.display()
                ))
            })?;
            if !directory_metadata.file_type().is_dir()
                || file_identity(&directory_metadata) != self.directory_identity
            {
                return Err(IosError::storage(format!(
                    "optional iOS release directory changed while it was snapshotted: {}",
                    self.display_path.display()
                )));
            }
            for asset in self.files.values() {
                let metadata = asset.file.metadata().map_err(|error| {
                    IosError::storage(format!(
                        "failed to re-inspect optional iOS release asset '{}': {error}",
                        asset.display_path.display()
                    ))
                })?;
                if !metadata.file_type().is_file() || file_identity(&metadata) != asset.identity {
                    return Err(IosError::storage(format!(
                        "optional iOS release asset changed while the release was snapshotted: {}",
                        asset.display_path.display()
                    )));
                }
            }
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FrameworkEvidence {
        profile_hash: String,
        declared_profile: String,
        external_dependencies: String,
        source_archive: String,
        device_metadata: Vec<u8>,
        simulator_metadata: Vec<u8>,
        device_input_fingerprint: String,
        simulator_input_fingerprint: String,
    }

    #[derive(Debug, Clone)]
    struct ReleaseSource {
        version: Version,
        version_text: String,
        source_url: String,
        source_asset: String,
        source_sha256: String,
    }

    impl ReleaseSource {
        fn new(version: Version, source_asset: String, source_sha256: String) -> Self {
            let source_url = source_url_for_version(&version);
            let version_text = version.to_string();
            Self {
                version,
                version_text,
                source_url,
                source_asset,
                source_sha256,
            }
        }
    }

    #[derive(Debug)]
    struct ReadBudget<R> {
        inner: R,
        remaining: u64,
    }

    impl<R> ReadBudget<R> {
        const fn new(inner: R, maximum_bytes: u64) -> Self {
            Self {
                inner,
                remaining: maximum_bytes,
            }
        }
    }

    impl<R: Read> Read for ReadBudget<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                let mut probe = [0_u8; 1];
                return match self.inner.read(&mut probe) {
                    Ok(0) => Ok(0),
                    Ok(_) => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "decompressed tar stream exceeds its release-policy limit",
                    )),
                    Err(error) => Err(error),
                };
            }
            let maximum = usize::try_from(self.remaining)
                .unwrap_or(usize::MAX)
                .min(buffer.len());
            let count = self.inner.read(&mut buffer[..maximum])?;
            self.remaining = self.remaining.saturating_sub(count as u64);
            Ok(count)
        }
    }

    pub(super) fn verify(
        root: &Path,
        release_directory: Option<&Path>,
        output: &mut dyn Write,
        _diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let release_directory = release_directory
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join("dist/release/ios"));
        let prepared = prepare_release_snapshot(root, &release_directory)?;
        let report = prepared.optional_report.as_ref().ok_or_else(|| {
            IosError::worker("optional iOS release verification produced no report")
        })?;
        write_optional_release_report(report, output)
    }

    pub(super) fn verify_release(
        root: &Path,
        release_directory: Option<&Path>,
        complete: bool,
        output: &mut dyn Write,
        _diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let release_directory = release_directory
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join("dist/release/ios"));
        let prepared = if complete {
            prepare_release_snapshot(root, &release_directory)?
        } else {
            prepare_core_release_snapshot(&release_directory)?
        };
        crate::ios_core_release::verify(root, &prepared.directory)?;
        write_core_release_report(&release_directory, complete, output)?;
        if let Some(report) = prepared.optional_report.as_ref() {
            write_optional_release_report(report, output)?;
        }
        Ok(())
    }

    pub(super) fn prepare_verified_optional_release_snapshot(
        root: &Path,
        release_directory: &Path,
    ) -> Result<super::VerifiedOptionalReleaseSnapshot, IosError> {
        let prepared = prepare_release_snapshot(root, release_directory)?;
        Ok(super::VerifiedOptionalReleaseSnapshot { prepared })
    }

    pub(super) fn stage_ffmpeg_compliance_assets(
        root: &Path,
        framework_directory: &Path,
        output_directory: &Path,
    ) -> Result<(), IosError> {
        let output_metadata = fs::symlink_metadata(output_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect optional iOS release stage '{}': {error}",
                output_directory.display()
            ))
        })?;
        if !output_metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "optional iOS release stage is not a regular non-symlink directory: {}",
                output_directory.display()
            )));
        }

        let policy = FfmpegSourcePolicy::load(root).map_err(map_source_policy_error)?;
        let release_lock = policy.release();
        let release_source = ReleaseSource::new(
            release_lock.version().clone(),
            format!(
                "VesperPlayerOptionalPlugins-FFmpeg-{}-source.tar.xz",
                release_lock.version()
            ),
            release_lock.source_sha256().to_owned(),
        );

        let mut reference_evidence = None::<FrameworkEvidence>;
        for framework in FFMPEG_FRAMEWORKS {
            let archive = framework_directory.join(format!("{framework}.xcframework.zip"));
            preflight_zip(
                &archive,
                &framework_zip_policy(framework),
                &format!("{framework} XCFramework"),
            )?;
            verify_xcframework_manifest(&archive, framework)?;
            verify_framework_binary_records(&archive, framework)?;
            if let Some(plugin) = crate::ios_plugin::IOS_PLUGIN_SPECS
                .iter()
                .find(|plugin| plugin.framework_name == framework)
            {
                verify_plugin_framework_registry_archive(root, &archive, plugin)?;
            }
            let evidence = verify_ffmpeg_framework_records(&archive, framework, &release_source)?;
            match &reference_evidence {
                None => reference_evidence = Some(evidence),
                Some(reference) => compare_framework_evidence(reference, &evidence, framework)?,
            }
        }
        let evidence = reference_evidence.ok_or_else(|| {
            IosError::worker("the FFmpeg-backed optional framework list is empty")
        })?;

        let recorded_source = PathBuf::from(&evidence.source_archive);
        if recorded_source.extension().and_then(|value| value.to_str()) != Some("xz")
            || recorded_source
                .file_name()
                .and_then(|value| value.to_str())
                .is_none_or(|value| !value.ends_with(".tar.xz"))
        {
            return Err(IosError::conformance(format!(
                "unsupported FFmpeg source archive recorded by the iOS build: {}",
                recorded_source.display()
            )));
        }
        let source = if recorded_source.is_absolute() {
            recorded_source
        } else {
            root.join(recorded_source)
        };
        let source_output = output_directory.join(&release_source.source_asset);
        copy_regular_file_snapshot(&source, &source_output, "recorded FFmpeg source archive")?;
        let source_sha256 = sha256_file(&source_output, "staged FFmpeg source archive")?;
        if source_sha256 != release_source.source_sha256 {
            return Err(IosError::conformance(format!(
                "the FFmpeg source archive no longer matches the SHA-256 recorded at build time:\n  archive:  {}\n  recorded: {}\n  actual:   {source_sha256}",
                source.display(),
                release_source.source_sha256
            )));
        }
        preflight_source_tar(&source_output, &release_source.version)?;

        let compliance_output = output_directory.join(COMPLIANCE_ASSET);
        write_compliance_archive(
            root,
            &compliance_output,
            &source_output,
            &evidence,
            &release_source,
        )?;
        let compliance_policy = compliance_zip_policy();
        preflight_zip(
            &compliance_output,
            &compliance_policy,
            "FFmpeg compliance archive",
        )?;
        verify_compliance_bundle(
            root,
            &compliance_output,
            &source_output,
            &evidence,
            &release_source,
        )
    }

    fn write_compliance_archive(
        root: &Path,
        destination: &Path,
        source_archive: &Path,
        framework: &FrameworkEvidence,
        release_source: &ReleaseSource,
    ) -> Result<(), IosError> {
        let source_files = read_source_license_files(source_archive, &release_source.version)?;
        let notices = read_regular_file_bounded(
            &root.join("THIRD_PARTY_NOTICES.md"),
            MAX_COMPLIANCE_ENTRY_BYTES,
            "Vesper third-party notices",
        )?;
        let source_record = format!(
            concat!(
                "component=FFmpeg\n",
                "ffmpeg_version={}\n",
                "license_mode=LGPL-2.1-or-later\n",
                "linkage=dynamic-frameworks\n",
                "declared_profile={}\n",
                "profile_hash={}\n",
                "source_url={}\n",
                "source_asset={}\n",
                "source_sha256={}\n",
                "local_changes=none\n",
                "external_dependencies={}\n"
            ),
            release_source.version_text,
            framework.declared_profile,
            framework.profile_hash,
            release_source.source_url,
            release_source.source_asset,
            release_source.source_sha256,
            framework.external_dependencies,
        );
        let notice = format!(
            concat!(
                "This optional iOS binary distribution uses libraries from the FFmpeg project\n",
                "under the GNU Lesser General Public License version 2.1 or later.\n\n",
                "FFmpeg is not licensed under Vesper's Apache-2.0 license. The exact\n",
                "corresponding FFmpeg source is published in the same release as:\n\n",
                "  {}\n\n",
                "Source SHA-256: {}\n",
                "Build profile: {} ({})\n\n",
                "The two Vesper FFmpeg plugin frameworks link to the separately distributed\n",
                "VesperFFmpegAVCodec, VesperFFmpegAVFormat, and VesperFFmpegAVUtil dynamic\n",
                "frameworks. See RELINKING.md for replacement and rebuild instructions.\n"
            ),
            release_source.source_asset,
            release_source.source_sha256,
            framework.declared_profile,
            framework.profile_hash,
        );
        let building = format!(
            concat!(
                "# Rebuilding the iOS FFmpeg frameworks\n\n",
                "This compliance bundle corresponds to FFmpeg {} and profile\n",
                "`{}` (`{}`). The original source archive is\n",
                "`{}`, whose SHA-256 is recorded in `SOURCE.txt`.\n\n",
                "From the matching Vesper release tag, stage the same device and Apple Silicon\n",
                "Simulator artifacts with:\n\n",
                "```sh\n",
                "VESPER_APPLE_FFMPEG_VERSION={} \\\n",
                "VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE=/path/to/{} \\\n",
                "VESPER_APPLE_FFMPEG_SOURCE_URL={} \\\n",
                "VESPER_APPLE_FFMPEG_FORCE=1 \\\n",
                "  ./scripts/vesper ios stage-optional-plugins-release /tmp/vesper-ios-release \\\n",
                "  --profile {} \\\n",
                "  ios-arm64 ios-simulator-arm64\n",
                "```\n\n",
                "The exact per-slice FFmpeg configure lines are preserved under\n",
                "`build-metadata/`. Vesper extracts the upstream archive without applying\n",
                "source patches; `changes.diff` is therefore intentionally empty.\n"
            ),
            release_source.version_text,
            framework.declared_profile,
            framework.profile_hash,
            release_source.source_asset,
            release_source.version_text,
            release_source.source_asset,
            release_source.source_url,
            framework.declared_profile,
        );
        let relinking = concat!(
            "# Replacing the FFmpeg dynamic frameworks\n\n",
            "The optional iOS plugins use top-level dynamic framework dependencies:\n\n",
            "- `VesperFFmpegAVCodec.framework`\n",
            "- `VesperFFmpegAVFormat.framework`\n",
            "- `VesperFFmpegAVUtil.framework`\n\n",
            "To use a modified, interface-compatible FFmpeg build:\n\n",
            "1. Rebuild the three component XCFrameworks with the command in `BUILDING.md`.\n",
            "   For a modified FFmpeg tree, create a `.tar.xz` archive with one top-level\n",
            "   source directory and point `VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE` at that\n",
            "   archive before rebuilding.\n",
            "2. Replace the corresponding XCFramework inputs in the host application before\n",
            "   the App target performs Embed & Sign.\n",
            "3. Preserve each framework name, bundle executable name, and\n",
            "   `@rpath/<Name>.framework/<Name>` install name so the plugin dependencies\n",
            "   continue to resolve.\n",
            "4. Build and sign the host application normally with the replacement\n",
            "   frameworks.\n\n",
            "The released Remux and SourceNormalizer plugin frameworks do not contain a\n",
            "second static copy of FFmpeg. Final application distributors remain responsible\n",
            "for preserving this notice, source availability, relinking rights, and\n",
            "LGPL-compatible reverse-engineering terms in their own distribution.\n",
        );
        let readme = format!(
            concat!(
                "# Vesper optional iOS FFmpeg compliance bundle\n\n",
                "This directory accompanies the FFmpeg-backed iOS optional-plugin XCFrameworks.\n",
                "It records the exact source, licenses, notices, build configuration, and dynamic\n",
                "framework replacement path for profile `{}`.\n\n",
                "The source archive is a separate asset in the same release:\n",
                "`{}`.\n\n",
                "The FFmpeg redistribution boundary covers the three `VesperFFmpegAV*`\n",
                "component frameworks plus the Remux and SourceNormalizer FFmpeg plugins. The\n",
                "VideoToolbox Decoder and diagnostic FrameProcessor plugins do not bundle or\n",
                "link FFmpeg.\n"
            ),
            framework.profile_hash, release_source.source_asset,
        );

        let mut nodes = BTreeMap::<String, Option<Vec<u8>>>::new();
        nodes.insert(format!("{COMPLIANCE_ROOT}/"), None);
        nodes.insert(format!("{COMPLIANCE_ROOT}/build-metadata/"), None);
        for (relative, bytes) in [
            ("BUILDING.md", building.into_bytes()),
            (
                "COPYING.LGPLv2.1",
                required_source_file(&source_files, "COPYING.LGPLv2.1")?.to_vec(),
            ),
            (
                "COPYING.LGPLv3",
                required_source_file(&source_files, "COPYING.LGPLv3")?.to_vec(),
            ),
            (
                "FFMPEG_LICENSE.md",
                required_source_file(&source_files, "LICENSE.md")?.to_vec(),
            ),
            ("NOTICE.txt", notice.into_bytes()),
            ("README.md", readme.into_bytes()),
            ("RELINKING.md", relinking.as_bytes().to_vec()),
            ("SOURCE.txt", source_record.into_bytes()),
            ("VESPER_THIRD_PARTY_NOTICES.md", notices),
            (
                "build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt",
                framework.device_metadata.clone(),
            ),
            (
                "build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
                framework.simulator_metadata.clone(),
            ),
            ("changes.diff", Vec::new()),
        ] {
            nodes.insert(format!("{COMPLIANCE_ROOT}/{relative}"), Some(bytes));
        }

        let output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create FFmpeg compliance archive '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut writer = ZipWriter::new(output);
        for (name, bytes) in nodes {
            let is_directory = bytes.is_none();
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(DateTime::default())
                .unix_permissions(if is_directory { 0o755 } else { 0o644 });
            if let Some(bytes) = bytes {
                writer.start_file(&name, options).map_err(|error| {
                    IosError::storage(format!(
                        "failed to create compliance archive entry '{name}': {error}"
                    ))
                })?;
                writer.write_all(&bytes).map_err(|error| {
                    IosError::storage(format!(
                        "failed to write compliance archive entry '{name}': {error}"
                    ))
                })?;
            } else {
                writer.add_directory(&name, options).map_err(|error| {
                    IosError::storage(format!(
                        "failed to create compliance archive directory '{name}': {error}"
                    ))
                })?;
            }
        }
        let output = writer.finish().map_err(|error| {
            IosError::storage(format!(
                "failed to finish FFmpeg compliance archive '{}': {error}",
                destination.display()
            ))
        })?;
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to synchronize FFmpeg compliance archive '{}': {error}",
                destination.display()
            ))
        })?;
        set_read_only(destination, "FFmpeg compliance archive")
    }

    fn prepare_release_snapshot(
        root: &Path,
        release_directory: &Path,
    ) -> Result<PreparedRelease, IosError> {
        let release_directory =
            canonical_release_directory(release_directory, "optional iOS release")?;
        let policy = FfmpegSourcePolicy::load(root).map_err(map_source_policy_error)?;
        let (source_asset, source_version) = discover_source_asset(&release_directory, &policy)?;
        let mut assets = collect_release_assets(&release_directory, &source_asset)?;
        let temporary = tempfile::Builder::new()
            .prefix("vesper-ios-optional-release-snapshot.")
            .tempdir()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create optional iOS release snapshot: {error}"
                ))
            })?;
        let inputs = temporary.path().join("inputs");
        let verified = temporary.path().join("verified");
        fs::create_dir(&inputs)
            .and_then(|()| fs::create_dir(&verified))
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create optional iOS release snapshot directories: {error}"
                ))
            })?;

        for (name, source) in &mut assets.files {
            copy_opened_file_snapshot(source, &inputs.join(name), "release asset")?;
        }
        assets.verify_unchanged()?;

        for core_asset in CORE_ASSETS {
            if assets.files.contains_key(core_asset) {
                let input = inputs.join(core_asset);
                preflight_zip(&input, &core_zip_policy(core_asset), core_asset)?;
            }
        }

        let source_input = inputs.join(&source_asset);
        let source_sha256 = sha256_file(&source_input, "FFmpeg source archive")?;
        if source_sha256 != policy.release().source_sha256() {
            return Err(IosError::conformance(format!(
                "optional iOS FFmpeg source SHA-256 is '{source_sha256}', expected locked release SHA-256 '{}'",
                policy.release().source_sha256()
            )));
        }
        let release_source = ReleaseSource::new(source_version, source_asset, source_sha256);
        preflight_source_tar(&source_input, &release_source.version)?;
        copy_regular_file_snapshot(
            &source_input,
            &verified.join(&release_source.source_asset),
            "verified FFmpeg source archive",
        )?;

        let mut reference_evidence = None::<FrameworkEvidence>;
        let mut optional_archives = Vec::with_capacity(OPTIONAL_FRAMEWORKS.len());
        for framework in OPTIONAL_FRAMEWORKS {
            let archive_name = format!("{framework}.xcframework.zip");
            let input = inputs.join(&archive_name);
            let policy = framework_zip_policy(framework);
            preflight_zip(&input, &policy, &format!("{framework} XCFramework"))?;
            verify_xcframework_manifest(&input, framework)?;
            verify_framework_binary_records(&input, framework)?;
            if let Some(plugin) = crate::ios_plugin::IOS_PLUGIN_SPECS
                .iter()
                .find(|plugin| plugin.framework_name == framework)
            {
                verify_plugin_framework_registry_archive(root, &input, plugin)?;
            }
            if FFMPEG_FRAMEWORKS.contains(&framework) {
                let evidence = verify_ffmpeg_framework_records(&input, framework, &release_source)?;
                match &reference_evidence {
                    None => reference_evidence = Some(evidence),
                    Some(reference) => compare_framework_evidence(reference, &evidence, framework)?,
                }
            }
            let release_archive_sha256 =
                sha256_file(&input, &format!("{framework} release XCFramework archive"))?;
            let verified_archive = verified.join(&archive_name);
            write_sanitized_zip(
                &input,
                &verified_archive,
                &policy,
                &format!("{framework} XCFramework"),
            )?;
            let verified_archive_sha256 = sha256_file(
                &verified_archive,
                &format!("{framework} verified XCFramework archive"),
            )?;
            optional_archives.push(OptionalReleaseArchiveEvidence {
                archive_name,
                product_name: framework.to_owned(),
                release_archive_sha256,
                verified_archive_sha256,
            });
        }

        let compliance_input = inputs.join(COMPLIANCE_ASSET);
        let compliance_policy = compliance_zip_policy();
        preflight_zip(
            &compliance_input,
            &compliance_policy,
            "FFmpeg compliance archive",
        )?;
        let evidence = reference_evidence.ok_or_else(|| {
            IosError::worker("the FFmpeg-backed optional framework list is empty")
        })?;
        verify_compliance_bundle(
            root,
            &compliance_input,
            &source_input,
            &evidence,
            &release_source,
        )?;
        write_sanitized_zip(
            &compliance_input,
            &verified.join(COMPLIANCE_ASSET),
            &compliance_policy,
            "FFmpeg compliance archive",
        )?;

        for core_asset in CORE_ASSETS {
            if assets.files.contains_key(core_asset) {
                let input = inputs.join(core_asset);
                copy_regular_file_snapshot(
                    &input,
                    &verified.join(core_asset),
                    "core iOS release archive",
                )?;
            }
        }

        Ok(PreparedRelease {
            _temporary: temporary,
            directory: verified,
            canonical_release_directory: release_directory,
            optional_archives,
            optional_report: Some(OptionalReleaseReport {
                profile_hash: evidence.profile_hash,
                source_asset: release_source.source_asset,
                source_sha256: release_source.source_sha256,
            }),
        })
    }

    fn prepare_core_release_snapshot(
        release_directory: &Path,
    ) -> Result<PreparedRelease, IosError> {
        let release_directory = canonical_release_directory(release_directory, "iOS release")?;
        let expected_names = CORE_ASSETS.into_iter().map(str::to_owned).collect();
        let mut assets = collect_named_release_assets(
            &release_directory,
            &expected_names,
            MAX_CORE_RELEASE_ASSET_BYTES,
            MAX_CORE_RELEASE_TOTAL_BYTES,
            "iOS release",
        )?;
        let temporary = tempfile::Builder::new()
            .prefix("vesper-ios-release-snapshot.")
            .tempdir()
            .map_err(|error| {
                IosError::storage(format!("failed to create iOS release snapshot: {error}"))
            })?;
        let verified = temporary.path().join("verified");
        fs::create_dir(&verified).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS release snapshot directory: {error}"
            ))
        })?;

        for (name, source) in &mut assets.files {
            let destination = verified.join(name);
            copy_opened_file_snapshot(source, &destination, "iOS release asset")?;
            if CORE_ASSETS.contains(&name.as_str()) {
                preflight_zip(&destination, &core_zip_policy(name), name)?;
            }
        }
        assets.verify_unchanged()?;

        Ok(PreparedRelease {
            _temporary: temporary,
            directory: verified,
            canonical_release_directory: release_directory,
            optional_archives: Vec::new(),
            optional_report: None,
        })
    }

    fn canonical_release_directory(path: &Path, label: &str) -> Result<PathBuf, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} directory '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "{label} path must be a regular non-symlink directory: {}",
                path.display()
            )));
        }
        fs::canonicalize(path).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve {label} directory '{}': {error}",
                path.display()
            ))
        })
    }

    fn write_optional_release_report(
        report: &OptionalReleaseReport,
        output: &mut dyn Write,
    ) -> Result<(), IosError> {
        writeln!(output, "Verified optional iOS plugin release artifacts:")
            .map_err(output_error)?;
        for framework in OPTIONAL_FRAMEWORKS {
            writeln!(output, "  {framework}.xcframework.zip").map_err(output_error)?;
        }
        writeln!(output, "  {COMPLIANCE_ASSET}").map_err(output_error)?;
        writeln!(output, "  {}", report.source_asset).map_err(output_error)?;
        writeln!(output, "  FFmpeg profile hash: {}", report.profile_hash).map_err(output_error)?;
        writeln!(output, "  FFmpeg source SHA-256: {}", report.source_sha256).map_err(output_error)
    }

    fn write_core_release_report(
        release_directory: &Path,
        complete: bool,
        output: &mut dyn Write,
    ) -> Result<(), IosError> {
        let scope = if complete { "complete" } else { "core" };
        writeln!(
            output,
            "Verified VesperPlayerKit iOS release assets ({scope}):"
        )
        .and_then(|()| writeln!(output, "  {}", release_directory.display()))
        .map_err(output_error)
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::worker(format!(
            "failed to write optional iOS release verification output: {error}"
        ))
    }

    fn core_zip_policy(name: &str) -> ZipPolicy {
        let root = if name == "VesperPlayerKit.xcframework.zip" {
            "VesperPlayerKit.xcframework"
        } else {
            "VesperPlayerKit.framework"
        };
        ZipPolicy {
            expected_nodes: None,
            required_root: Some(root),
            maximum_entries: MAX_ZIP_ENTRIES,
            maximum_entry_bytes: MAX_CORE_ZIP_ENTRY_BYTES,
            maximum_expanded_bytes: MAX_CORE_ZIP_EXPANDED_BYTES,
        }
    }

    fn map_source_policy_error(error: FfmpegSourcePolicyError) -> IosError {
        match error.kind() {
            FfmpegSourcePolicyErrorKind::Storage => IosError::storage(error.to_string()),
            FfmpegSourcePolicyErrorKind::Invalid => IosError::conformance(error.to_string()),
        }
    }

    fn discover_source_asset(
        release_directory: &Path,
        policy: &FfmpegSourcePolicy,
    ) -> Result<(String, Version), IosError> {
        let entries = fs::read_dir(release_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to enumerate optional iOS release directory '{}': {error}",
                release_directory.display()
            ))
        })?;
        let mut source = None;
        for (index, entry) in entries.enumerate() {
            if index >= MAX_RELEASE_ASSETS {
                return Err(IosError::conformance(format!(
                    "optional iOS release directory contains more than {MAX_RELEASE_ASSETS} assets"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to read an optional iOS release directory entry: {error}"
                ))
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                IosError::conformance("optional iOS release asset name is not UTF-8")
            })?;
            if source_asset_version(&name).is_none() {
                continue;
            }
            let version = policy
                .parse_compatible_source_asset(&name)
                .map_err(map_source_policy_error)?;
            if &version != policy.release().version() {
                return Err(IosError::conformance(format!(
                    "optional iOS FFmpeg source version '{version}' does not match locked release version '{}'",
                    policy.release().version()
                )));
            }
            if let Some((previous, _)) = source.as_ref() {
                return Err(IosError::conformance(format!(
                    "optional iOS release contains ambiguous FFmpeg source assets: {previous}, {name}"
                )));
            }
            source = Some((name, version));
        }
        source.ok_or_else(|| {
            IosError::conformance(format!(
                "optional iOS release is missing a FFmpeg source asset satisfying '{}'",
                policy.compatibility_requirement()
            ))
        })
    }

    fn collect_release_assets(
        release_directory: &Path,
        source_asset: &str,
    ) -> Result<OpenedReleaseAssets, IosError> {
        let required = required_optional_assets(source_asset)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut allowed = required.clone();
        allowed.extend(CORE_ASSETS.into_iter().map(str::to_owned));
        collect_release_assets_with_policy(
            release_directory,
            &allowed,
            &required,
            true,
            MAX_RELEASE_ASSET_BYTES,
            MAX_RELEASE_TOTAL_BYTES,
            "optional iOS release",
        )
    }

    fn collect_named_release_assets(
        release_directory: &Path,
        required: &BTreeSet<String>,
        maximum_asset_bytes: u64,
        maximum_total_bytes: u64,
        label: &str,
    ) -> Result<OpenedReleaseAssets, IosError> {
        collect_release_assets_with_policy(
            release_directory,
            required,
            required,
            required.len() > CORE_ASSETS.len(),
            maximum_asset_bytes,
            maximum_total_bytes,
            label,
        )
    }

    fn collect_release_assets_with_policy(
        release_directory: &Path,
        allowed: &BTreeSet<String>,
        required: &BTreeSet<String>,
        reject_unexpected: bool,
        maximum_asset_bytes: u64,
        maximum_total_bytes: u64,
        label: &str,
    ) -> Result<OpenedReleaseAssets, IosError> {
        use rustix::fs::{Dir, Mode, OFlags, open, openat};

        let directory = open(
            release_directory,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} directory '{}': {error}",
                release_directory.display()
            ))
        })?;
        let root_metadata = directory.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} directory '{}': {error}",
                release_directory.display()
            ))
        })?;
        if !root_metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "{label} path must be a regular non-symlink directory: {}",
                release_directory.display()
            )));
        }
        let directory_identity = file_identity(&root_metadata);
        let mut entries = Dir::read_from(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to enumerate {label} directory '{}': {error}",
                release_directory.display()
            ))
        })?;

        let mut assets = BTreeMap::new();
        let mut entry_count = 0_usize;
        let mut total_bytes = 0_u64;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|error| {
                IosError::storage(format!("failed to read a {label} directory entry: {error}"))
            })?;
            if matches!(entry.file_name().to_bytes(), b"." | b"..") {
                continue;
            }
            entry_count += 1;
            if entry_count > MAX_RELEASE_ASSETS {
                return Err(IosError::conformance(format!(
                    "{label} directory contains more than {MAX_RELEASE_ASSETS} assets"
                )));
            }
            let name = std::str::from_utf8(entry.file_name().to_bytes())
                .map(str::to_owned)
                .map_err(|error| {
                    IosError::conformance(format!("{label} asset name is not UTF-8: {error}"))
                })?;
            let display_path = release_directory.join(&name);
            if name.contains(".dylib") {
                return Err(IosError::conformance(format!(
                    "iOS releases must not contain bare dylibs: {}",
                    display_path.display()
                )));
            }
            if !allowed.contains(&name) {
                if reject_unexpected {
                    return Err(IosError::conformance(format!(
                        "Unexpected top-level iOS release asset: {}",
                        display_path.display()
                    )));
                }
                continue;
            }
            let file = openat(
                &directory,
                entry.file_name(),
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map(File::from)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to open {label} asset '{}': {error}",
                    display_path.display()
                ))
            })?;
            let metadata = file.metadata().map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} asset '{}': {error}",
                    display_path.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(IosError::conformance(format!(
                    "{label} assets must be regular non-symlink files: {}",
                    display_path.display()
                )));
            }
            if metadata.len() > maximum_asset_bytes {
                return Err(IosError::conformance(format!(
                    "{label} asset exceeds {maximum_asset_bytes} bytes: {}",
                    display_path.display()
                )));
            }
            total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                IosError::conformance(format!("{label} asset size sum overflowed"))
            })?;
            if total_bytes > maximum_total_bytes {
                return Err(IosError::conformance(format!(
                    "{label} assets exceed {maximum_total_bytes} total bytes"
                )));
            }
            let identity = file_identity(&metadata);
            assets.insert(
                name,
                OpenedReleaseAsset {
                    file,
                    identity,
                    display_path,
                },
            );
        }

        for required_name in required {
            if !assets.contains_key(required_name) {
                return Err(IosError::conformance(format!(
                    "Missing {label} artifact: {}",
                    release_directory.join(required_name).display()
                )));
            }
        }
        let opened = OpenedReleaseAssets {
            directory,
            directory_identity,
            display_path: release_directory.to_path_buf(),
            files: assets,
        };
        opened.verify_unchanged()?;
        Ok(opened)
    }

    fn required_optional_assets(source_asset: &str) -> Vec<String> {
        let mut required = OPTIONAL_FRAMEWORKS
            .into_iter()
            .map(|framework| format!("{framework}.xcframework.zip"))
            .collect::<Vec<_>>();
        required.push(COMPLIANCE_ASSET.to_owned());
        required.push(source_asset.to_owned());
        required
    }

    fn framework_zip_policy(framework: &str) -> ZipPolicy {
        if let Some(plugin) = crate::ios_plugin::IOS_PLUGIN_SPECS
            .iter()
            .find(|plugin| plugin.framework_name == framework)
        {
            return ZipPolicy {
                expected_nodes: Some(expected_plugin_framework_nodes(
                    framework,
                    plugin.uses_ffmpeg,
                    &[
                        crate::ios_plugin::IosPluginSlice::DeviceArm64,
                        crate::ios_plugin::IosPluginSlice::SimulatorArm64,
                    ],
                )),
                required_root: None,
                maximum_entries: MAX_ZIP_ENTRIES,
                maximum_entry_bytes: MAX_ZIP_ENTRY_BYTES,
                maximum_expanded_bytes: MAX_ZIP_EXPANDED_BYTES,
            };
        }
        runtime_framework_zip_policy(framework)
    }

    fn runtime_framework_zip_policy(framework: &str) -> ZipPolicy {
        ZipPolicy {
            expected_nodes: Some(expected_framework_nodes(framework)),
            required_root: None,
            maximum_entries: MAX_ZIP_ENTRIES,
            maximum_entry_bytes: MAX_ZIP_ENTRY_BYTES,
            maximum_expanded_bytes: MAX_ZIP_EXPANDED_BYTES,
        }
    }

    pub(super) fn preflight_plugin_framework_archive(
        path: &Path,
        framework: &str,
        uses_ffmpeg: bool,
        slices: &[crate::ios_plugin::IosPluginSlice],
    ) -> Result<(), IosError> {
        let policy = ZipPolicy {
            expected_nodes: Some(expected_plugin_framework_nodes(
                framework,
                uses_ffmpeg,
                slices,
            )),
            required_root: None,
            maximum_entries: MAX_ZIP_ENTRIES,
            maximum_entry_bytes: MAX_ZIP_ENTRY_BYTES,
            maximum_expanded_bytes: MAX_ZIP_EXPANDED_BYTES,
        };
        preflight_zip(path, &policy, "iOS plugin XCFramework archive")
    }

    pub(super) fn preflight_runtime_framework_archive(
        path: &Path,
        framework: &str,
    ) -> Result<(), IosError> {
        preflight_zip(
            path,
            &runtime_framework_zip_policy(framework),
            "iOS FFmpeg runtime XCFramework archive",
        )
    }

    fn compliance_zip_policy() -> ZipPolicy {
        ZipPolicy {
            expected_nodes: Some(expected_compliance_nodes()),
            required_root: None,
            maximum_entries: MAX_COMPLIANCE_ENTRIES,
            maximum_entry_bytes: MAX_COMPLIANCE_ENTRY_BYTES,
            maximum_expanded_bytes: MAX_COMPLIANCE_EXPANDED_BYTES,
        }
    }

    #[cfg(test)]
    fn generic_zip_policy() -> ZipPolicy {
        ZipPolicy {
            expected_nodes: None,
            required_root: None,
            maximum_entries: MAX_ZIP_ENTRIES,
            maximum_entry_bytes: MAX_ZIP_ENTRY_BYTES,
            maximum_expanded_bytes: MAX_ZIP_EXPANDED_BYTES,
        }
    }

    fn expected_framework_nodes(framework: &str) -> BTreeMap<String, ArchiveNodeKind> {
        let root = format!("{framework}.xcframework");
        let mut nodes = BTreeMap::from([
            (root.clone(), ArchiveNodeKind::Directory),
            (format!("{root}/Info.plist"), ArchiveNodeKind::File),
        ]);
        for (slice, metadata_name) in OPTIONAL_SLICES {
            let slice_root = format!("{root}/{slice}");
            let framework_root = format!("{slice_root}/{framework}.framework");
            nodes.insert(slice_root, ArchiveNodeKind::Directory);
            nodes.insert(framework_root.clone(), ArchiveNodeKind::Directory);
            nodes.insert(
                format!("{framework_root}/Headers"),
                ArchiveNodeKind::Directory,
            );
            nodes.insert(
                format!("{framework_root}/Headers/{framework}.h"),
                ArchiveNodeKind::File,
            );
            nodes.insert(
                format!("{framework_root}/Info.plist"),
                ArchiveNodeKind::File,
            );
            nodes.insert(
                format!("{framework_root}/Modules"),
                ArchiveNodeKind::Directory,
            );
            nodes.insert(
                format!("{framework_root}/Modules/module.modulemap"),
                ArchiveNodeKind::File,
            );
            nodes.insert(
                format!("{framework_root}/{framework}"),
                ArchiveNodeKind::File,
            );
            if FFMPEG_FRAMEWORKS.contains(&framework) {
                nodes.insert(
                    format!("{framework_root}/binary-sha256.txt"),
                    ArchiveNodeKind::File,
                );
                nodes.insert(
                    format!("{framework_root}/input-fingerprint.txt"),
                    ArchiveNodeKind::File,
                );
                nodes.insert(
                    format!("{framework_root}/{metadata_name}"),
                    ArchiveNodeKind::File,
                );
                nodes.insert(
                    format!("{framework_root}/profile-hash.txt"),
                    ArchiveNodeKind::File,
                );
            }
        }
        nodes
    }

    fn expected_plugin_framework_nodes(
        framework: &str,
        uses_ffmpeg: bool,
        slices: &[crate::ios_plugin::IosPluginSlice],
    ) -> BTreeMap<String, ArchiveNodeKind> {
        let root = format!("{framework}.xcframework");
        let mut nodes = BTreeMap::from([
            (root.clone(), ArchiveNodeKind::Directory),
            (format!("{root}/Info.plist"), ArchiveNodeKind::File),
        ]);
        for slice in slices {
            let (identifier, metadata_name) = match slice {
                crate::ios_plugin::IosPluginSlice::DeviceArm64 => {
                    ("ios-arm64", "ios-arm64-vesper-ffmpeg-build-metadata.txt")
                }
                crate::ios_plugin::IosPluginSlice::SimulatorArm64 => (
                    "ios-arm64-simulator",
                    "ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
                ),
            };
            let slice_root = format!("{root}/{identifier}");
            let framework_root = format!("{slice_root}/{framework}.framework");
            nodes.extend([
                (slice_root, ArchiveNodeKind::Directory),
                (framework_root.clone(), ArchiveNodeKind::Directory),
                (
                    format!("{framework_root}/Headers"),
                    ArchiveNodeKind::Directory,
                ),
                (
                    format!("{framework_root}/Headers/{framework}.h"),
                    ArchiveNodeKind::File,
                ),
                (
                    format!("{framework_root}/Info.plist"),
                    ArchiveNodeKind::File,
                ),
                (
                    format!("{framework_root}/Modules"),
                    ArchiveNodeKind::Directory,
                ),
                (
                    format!("{framework_root}/Modules/module.modulemap"),
                    ArchiveNodeKind::File,
                ),
                (
                    format!("{framework_root}/{framework}"),
                    ArchiveNodeKind::File,
                ),
                (
                    format!("{framework_root}/vesper-plugin-registry.json"),
                    ArchiveNodeKind::File,
                ),
            ]);
            if uses_ffmpeg {
                nodes.extend([
                    (
                        format!("{framework_root}/binary-sha256.txt"),
                        ArchiveNodeKind::File,
                    ),
                    (
                        format!("{framework_root}/input-fingerprint.txt"),
                        ArchiveNodeKind::File,
                    ),
                    (
                        format!("{framework_root}/{metadata_name}"),
                        ArchiveNodeKind::File,
                    ),
                    (
                        format!("{framework_root}/profile-hash.txt"),
                        ArchiveNodeKind::File,
                    ),
                ]);
            }
        }
        nodes
    }

    fn expected_compliance_nodes() -> BTreeMap<String, ArchiveNodeKind> {
        let mut nodes = BTreeMap::from([(COMPLIANCE_ROOT.to_owned(), ArchiveNodeKind::Directory)]);
        for file in [
            "BUILDING.md",
            "COPYING.LGPLv2.1",
            "COPYING.LGPLv3",
            "FFMPEG_LICENSE.md",
            "NOTICE.txt",
            "README.md",
            "RELINKING.md",
            "SOURCE.txt",
            "VESPER_THIRD_PARTY_NOTICES.md",
            "changes.diff",
        ] {
            nodes.insert(format!("{COMPLIANCE_ROOT}/{file}"), ArchiveNodeKind::File);
        }
        nodes.insert(
            format!("{COMPLIANCE_ROOT}/build-metadata"),
            ArchiveNodeKind::Directory,
        );
        for file in [
            "ios-arm64-vesper-ffmpeg-build-metadata.txt",
            "ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
        ] {
            nodes.insert(
                format!("{COMPLIANCE_ROOT}/build-metadata/{file}"),
                ArchiveNodeKind::File,
            );
        }
        nodes
    }

    fn preflight_zip(path: &Path, policy: &ZipPolicy, label: &str) -> Result<(), IosError> {
        let metadata = fs::metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if metadata.len() > MAX_ZIP_ARCHIVE_BYTES {
            return Err(IosError::conformance(format!(
                "{label} exceeds {MAX_ZIP_ARCHIVE_BYTES} compressed bytes: {}",
                path.display()
            )));
        }
        let raw_entry_count = validate_raw_zip_directory(path, policy.maximum_entries, label)?;
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            IosError::conformance(format!("invalid {label} '{}': {error}", path.display()))
        })?;
        if archive.len() != raw_entry_count {
            return Err(IosError::conformance(format!(
                "{label} contains duplicate central-directory file names"
            )));
        }
        if archive.is_empty() || archive.len() > policy.maximum_entries {
            return Err(IosError::conformance(format!(
                "{label} must contain 1 to {} entries, found {}",
                policy.maximum_entries,
                archive.len()
            )));
        }

        let mut logical_nodes = BTreeMap::new();
        let mut logical_collision_nodes = BTreeMap::new();
        let mut archive_nodes = BTreeMap::new();
        let mut appledouble_files = Vec::new();
        let mut appledouble_directories = Vec::new();
        let mut expanded_bytes = 0_u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                IosError::conformance(format!(
                    "failed to read {label} central directory entry {index}: {error}"
                ))
            })?;
            let name = strict_zip_name(&entry, label)?;
            let kind = zip_entry_kind(&entry, &name, label)?;
            let canonical_name = validate_archive_path(&name, kind, label)?;
            insert_archive_node(&mut archive_nodes, &canonical_name, kind, label)?;
            if entry.encrypted() {
                return Err(IosError::conformance(format!(
                    "{label} entry must not be encrypted: {name}"
                )));
            }
            if entry.size() > policy.maximum_entry_bytes {
                return Err(IosError::conformance(format!(
                    "{label} entry exceeds {} bytes: {name}",
                    policy.maximum_entry_bytes
                )));
            }
            expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
                IosError::conformance(format!("{label} expanded size overflowed"))
            })?;
            if expanded_bytes > policy.maximum_expanded_bytes {
                return Err(IosError::conformance(format!(
                    "{label} expands beyond {} bytes",
                    policy.maximum_expanded_bytes
                )));
            }
            if kind == ArchiveNodeKind::File
                && entry.size() > MAX_APPLEDOUBLE_ENTRY_BYTES
                && (entry.compressed_size() == 0
                    || entry
                        .compressed_size()
                        .saturating_mul(MAX_ZIP_COMPRESSION_RATIO)
                        < entry.size())
            {
                return Err(IosError::conformance(format!(
                    "{label} entry exceeds the {MAX_ZIP_COMPRESSION_RATIO}:1 compression-ratio limit: {name}"
                )));
            }

            if canonical_name == "__MACOSX" || canonical_name.starts_with("__MACOSX/") {
                if kind == ArchiveNodeKind::Directory {
                    appledouble_directories.push(canonical_name);
                } else {
                    if entry.size() > MAX_APPLEDOUBLE_ENTRY_BYTES {
                        return Err(IosError::conformance(format!(
                            "{label} AppleDouble sidecar exceeds {MAX_APPLEDOUBLE_ENTRY_BYTES} bytes: {name}"
                        )));
                    }
                    appledouble_files.push(canonical_name);
                }
            } else {
                insert_archive_node(&mut logical_collision_nodes, &canonical_name, kind, label)?;
                logical_nodes.insert(canonical_name, kind);
            }

            if kind == ArchiveNodeKind::Directory
                && (entry.size() != 0 || entry.compressed_size() != 0)
            {
                return Err(IosError::conformance(format!(
                    "{label} directory entry must have an empty payload: {name}"
                )));
            }
            if kind == ArchiveNodeKind::File {
                let declared_bytes = entry.size();
                copy_declared_bytes(
                    &mut entry,
                    &mut io::sink(),
                    declared_bytes,
                    policy.maximum_entry_bytes,
                    &format!("{label} entry '{name}'"),
                )?;
            }
        }

        validate_appledouble_pairs(
            &logical_collision_nodes,
            &appledouble_files,
            &appledouble_directories,
            label,
        )?;
        if let Some(root) = policy.required_root {
            let prefix = format!("{root}/");
            if logical_nodes.get(root) != Some(&ArchiveNodeKind::Directory)
                || logical_nodes
                    .keys()
                    .any(|path| path != root && !path.starts_with(&prefix))
            {
                return Err(IosError::conformance(format!(
                    "{label} must contain exactly one {root} root"
                )));
            }
        }
        if let Some(expected) = &policy.expected_nodes
            && &logical_nodes != expected
        {
            let missing = expected
                .keys()
                .filter(|path| !logical_nodes.contains_key(*path))
                .cloned()
                .collect::<Vec<_>>();
            let unexpected = logical_nodes
                .keys()
                .filter(|path| !expected.contains_key(*path))
                .cloned()
                .collect::<Vec<_>>();
            return Err(IosError::conformance(format!(
                "{label} payload does not match the canonical release layout:\n  missing: {}\n  unexpected: {}",
                display_paths(&missing),
                display_paths(&unexpected)
            )));
        }
        Ok(())
    }

    fn validate_raw_zip_directory(
        path: &Path,
        maximum_entries: usize,
        label: &str,
    ) -> Result<usize, IosError> {
        let bytes = fs::read(path).map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} central directory '{}': {error}",
                path.display()
            ))
        })?;
        if bytes.len() < ZIP_EOCD_MINIMUM_BYTES {
            return Err(IosError::conformance(format!(
                "{label} is too short to contain a ZIP end-of-central-directory record"
            )));
        }
        let search_start = bytes
            .len()
            .saturating_sub(ZIP_EOCD_MINIMUM_BYTES + ZIP_MAXIMUM_COMMENT_BYTES);
        let eocd = (search_start..=bytes.len() - ZIP_EOCD_MINIMUM_BYTES)
            .rev()
            .find(|offset| {
                bytes[*offset..*offset + 4] == ZIP_END_OF_CENTRAL_DIRECTORY
                    && read_u16(&bytes, *offset + 20).is_some_and(|comment_bytes| {
                        *offset + ZIP_EOCD_MINIMUM_BYTES + comment_bytes as usize == bytes.len()
                    })
            })
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "{label} has no canonical ZIP end-of-central-directory record"
                ))
            })?;

        let disk = read_u16(&bytes, eocd + 4).unwrap_or(u16::MAX);
        let central_disk = read_u16(&bytes, eocd + 6).unwrap_or(u16::MAX);
        let disk_entries = read_u16(&bytes, eocd + 8).unwrap_or(u16::MAX);
        let total_entries = read_u16(&bytes, eocd + 10).unwrap_or(u16::MAX);
        let central_bytes = read_u32(&bytes, eocd + 12).unwrap_or(u32::MAX);
        let central_offset = read_u32(&bytes, eocd + 16).unwrap_or(u32::MAX);
        if disk != 0
            || central_disk != 0
            || disk_entries != total_entries
            || total_entries == u16::MAX
            || central_bytes == u32::MAX
            || central_offset == u32::MAX
        {
            return Err(IosError::conformance(format!(
                "{label} must be a single-disk non-ZIP64 archive"
            )));
        }
        let total_entries = total_entries as usize;
        if total_entries == 0 || total_entries > maximum_entries {
            return Err(IosError::conformance(format!(
                "{label} central directory declares {total_entries} entries; expected 1 to {maximum_entries}"
            )));
        }
        let central_start = central_offset as usize;
        let central_end = central_start
            .checked_add(central_bytes as usize)
            .filter(|end| *end == eocd)
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "{label} has a non-canonical ZIP central-directory offset or size"
                ))
            })?;

        let mut cursor = central_start;
        let mut count = 0_usize;
        let mut names = BTreeMap::new();
        while cursor < central_end {
            if cursor + 46 > central_end
                || bytes[cursor..cursor + 4] != ZIP_CENTRAL_DIRECTORY_HEADER
            {
                return Err(IosError::conformance(format!(
                    "{label} contains a malformed central-directory record"
                )));
            }
            let name_bytes = read_u16(&bytes, cursor + 28).unwrap_or(u16::MAX) as usize;
            let extra_bytes = read_u16(&bytes, cursor + 30).unwrap_or(u16::MAX) as usize;
            let comment_bytes = read_u16(&bytes, cursor + 32).unwrap_or(u16::MAX) as usize;
            let starting_disk = read_u16(&bytes, cursor + 34).unwrap_or(u16::MAX);
            if starting_disk != 0 {
                return Err(IosError::conformance(format!(
                    "{label} central-directory entry references another disk"
                )));
            }
            let record_bytes = 46_usize
                .checked_add(name_bytes)
                .and_then(|size| size.checked_add(extra_bytes))
                .and_then(|size| size.checked_add(comment_bytes))
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "{label} central-directory entry size overflowed"
                    ))
                })?;
            let record_end = cursor
                .checked_add(record_bytes)
                .filter(|end| *end <= central_end)
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "{label} central-directory entry exceeds its declared boundary"
                    ))
                })?;
            let name_start = cursor + 46;
            let name_end = name_start + name_bytes;
            let name = std::str::from_utf8(&bytes[name_start..name_end]).map_err(|error| {
                IosError::conformance(format!(
                    "{label} contains a non-UTF-8 central-directory name: {error}"
                ))
            })?;
            let inferred_kind = if name.ends_with('/') {
                ArchiveNodeKind::Directory
            } else {
                ArchiveNodeKind::File
            };
            let canonical = validate_archive_path(name, inferred_kind, label)?;
            insert_archive_node(&mut names, &canonical, inferred_kind, label)?;
            count += 1;
            cursor = record_end;
        }
        if count != total_entries {
            return Err(IosError::conformance(format!(
                "{label} central-directory count mismatch: declared {total_entries}, parsed {count}"
            )));
        }
        Ok(count)
    }

    fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
        let value = bytes.get(offset..offset.checked_add(2)?)?;
        Some(u16::from_le_bytes([value[0], value[1]]))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
    }

    fn strict_zip_name<R: Read>(
        entry: &zip::read::ZipFile<'_, R>,
        label: &str,
    ) -> Result<String, IosError> {
        std::str::from_utf8(entry.name_raw())
            .map(str::to_owned)
            .map_err(|error| {
                IosError::conformance(format!("{label} contains a non-UTF-8 entry name: {error}"))
            })
    }

    fn zip_entry_kind<R: Read>(
        entry: &zip::read::ZipFile<'_, R>,
        name: &str,
        label: &str,
    ) -> Result<ArchiveNodeKind, IosError> {
        let mode = entry.unix_mode().ok_or_else(|| {
            IosError::conformance(format!(
                "{label} entry is missing Unix file-type metadata: {name}"
            ))
        })?;
        match (entry.is_dir(), mode & UNIX_FILE_TYPE_MASK) {
            (true, UNIX_DIRECTORY) => Ok(ArchiveNodeKind::Directory),
            (false, UNIX_REGULAR_FILE) => Ok(ArchiveNodeKind::File),
            _ => Err(IosError::conformance(format!(
                "{label} contains a symlink or unsupported file type: {name}"
            ))),
        }
    }

    fn validate_archive_path(
        path: &str,
        kind: ArchiveNodeKind,
        label: &str,
    ) -> Result<String, IosError> {
        if path.is_empty()
            || path.len() > MAX_ARCHIVE_PATH_BYTES
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || path.chars().any(char::is_control)
        {
            return Err(IosError::conformance(format!(
                "{label} contains an invalid archive path: {path:?}"
            )));
        }
        let canonical = if kind == ArchiveNodeKind::Directory {
            path.strip_suffix('/').unwrap_or(path)
        } else {
            if path.ends_with('/') {
                return Err(IosError::conformance(format!(
                    "{label} regular-file path ends with '/': {path}"
                )));
            }
            path
        };
        let components = canonical.split('/').collect::<Vec<_>>();
        if canonical.is_empty()
            || components.len() > MAX_ARCHIVE_PATH_DEPTH
            || components
                .iter()
                .any(|component| component.is_empty() || matches!(*component, "." | ".."))
        {
            return Err(IosError::conformance(format!(
                "{label} contains a traversing, empty, or over-depth archive path: {path}"
            )));
        }
        Ok(canonical.to_owned())
    }

    fn insert_archive_node(
        nodes: &mut BTreeMap<String, ArchiveNodeKind>,
        path: &str,
        kind: ArchiveNodeKind,
        label: &str,
    ) -> Result<(), IosError> {
        let normalized = normalized_archive_path(path);
        if nodes.contains_key(&normalized) {
            return Err(IosError::conformance(format!(
                "{label} contains duplicate or Unicode/case-colliding path: {path}"
            )));
        }
        for (existing, existing_kind) in nodes.iter() {
            if *existing_kind == ArchiveNodeKind::File
                && normalized
                    .strip_prefix(existing)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            {
                return Err(IosError::conformance(format!(
                    "{label} path descends from a regular file: {path}"
                )));
            }
            if kind == ArchiveNodeKind::File
                && existing
                    .strip_prefix(&normalized)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            {
                return Err(IosError::conformance(format!(
                    "{label} regular file conflicts with a descendant path: {path}"
                )));
            }
        }
        nodes.insert(normalized, kind);
        Ok(())
    }

    fn normalized_archive_path(path: &str) -> String {
        path.nfc().case_fold().nfc().collect()
    }

    fn validate_appledouble_pairs(
        logical_nodes: &BTreeMap<String, ArchiveNodeKind>,
        sidecars: &[String],
        directories: &[String],
        label: &str,
    ) -> Result<(), IosError> {
        let mut sidecar_ancestors = BTreeSet::new();
        for sidecar in sidecars {
            let relative = sidecar.strip_prefix("__MACOSX/").ok_or_else(|| {
                IosError::conformance(format!("invalid {label} AppleDouble path: {sidecar}"))
            })?;
            let (parent, file_name) = relative.rsplit_once('/').unwrap_or(("", relative));
            let target_name = file_name
                .strip_prefix("._")
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "{label} contains a non-sidecar file under __MACOSX: {sidecar}"
                    ))
                })?;
            let target = if parent.is_empty() {
                target_name.to_owned()
            } else {
                format!("{parent}/{target_name}")
            };
            if !logical_nodes.contains_key(&normalized_archive_path(&target)) {
                return Err(IosError::conformance(format!(
                    "{label} contains an unpaired AppleDouble sidecar: {sidecar}"
                )));
            }
            let mut current = Some(sidecar.as_str());
            while let Some(path) = current {
                if let Some((parent, _)) = path.rsplit_once('/') {
                    sidecar_ancestors.insert(normalized_archive_path(parent));
                    current = Some(parent);
                } else {
                    current = None;
                }
            }
        }
        for directory in directories {
            if !sidecar_ancestors.contains(&normalized_archive_path(directory)) {
                return Err(IosError::conformance(format!(
                    "{label} contains an unrelated __MACOSX directory: {directory}"
                )));
            }
        }
        Ok(())
    }

    fn display_paths(paths: &[String]) -> String {
        if paths.is_empty() {
            "none".to_owned()
        } else {
            paths.join(", ")
        }
    }

    pub(super) fn materialize_swift_package(
        prepared: &PreparedRelease,
        destination: &Path,
    ) -> Result<(), IosError> {
        let parent = destination.parent().ok_or_else(|| {
            IosError::storage(format!(
                "verified optional-plugin Swift package must have a parent: {}",
                destination.display()
            ))
        })?;
        let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect verified optional-plugin Swift package parent '{}': {error}",
                parent.display()
            ))
        })?;
        if !parent_metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "verified optional-plugin Swift package parent must be a regular non-symlink directory: {}",
                parent.display()
            )));
        }
        fs::create_dir(destination).map_err(|error| {
            IosError::storage(format!(
                "failed to create verified optional-plugin Swift package '{}': {error}",
                destination.display()
            ))
        })?;
        let artifacts = destination.join("Artifacts");
        fs::create_dir(&artifacts).map_err(|error| {
            IosError::storage(format!(
                "failed to create verified optional-plugin artifact directory '{}': {error}",
                artifacts.display()
            ))
        })?;
        write_verified_package_manifest(&destination.join("Package.swift"))?;

        if prepared.optional_archives.len() != OPTIONAL_FRAMEWORKS.len() {
            return Err(IosError::worker(
                "verified optional-plugin snapshot has an incomplete framework evidence set",
            ));
        }
        for (framework, archive_evidence) in OPTIONAL_FRAMEWORKS
            .into_iter()
            .zip(&prepared.optional_archives)
        {
            let expected_archive = format!("{framework}.xcframework.zip");
            if archive_evidence.product_name != framework
                || archive_evidence.archive_name != expected_archive
            {
                return Err(IosError::worker(format!(
                    "verified optional-plugin snapshot mapping drifted for {framework}"
                )));
            }
            let archive = prepared.directory.join(&archive_evidence.archive_name);
            let actual_sha256 = sha256_file(
                &archive,
                &format!("{framework} retained verified XCFramework archive"),
            )?;
            if actual_sha256 != archive_evidence.verified_archive_sha256 {
                return Err(IosError::conformance(format!(
                    "retained verified {framework} XCFramework archive changed before materialization"
                )));
            }
            let policy = framework_zip_policy(framework);
            preflight_zip(
                &archive,
                &policy,
                &format!("retained verified {framework} XCFramework"),
            )?;
            materialize_xcframework_archive(&archive, &artifacts, framework, &policy)?;
        }
        Ok(())
    }

    fn write_verified_package_manifest(path: &Path) -> Result<(), IosError> {
        let artifacts = OPTIONAL_FRAMEWORKS
            .iter()
            .map(|framework| format!("    \"{framework}\","))
            .collect::<Vec<_>>()
            .join("\n");
        let manifest = format!(
            "// swift-tools-version: 5.10\n\
             import PackageDescription\n\n\
             private let artifactNames = [\n{artifacts}\n]\n\n\
             let package = Package(\n\
                 name: \"VesperPlayerOptionalPlugins\",\n\
                 platforms: [.iOS(.v17)],\n\
                 products: artifactNames.map {{ artifactName in\n\
                     .library(name: artifactName, targets: [artifactName])\n\
                 }},\n\
                 targets: artifactNames.map {{ artifactName in\n\
                     .binaryTarget(\n\
                         name: artifactName,\n\
                         path: \"Artifacts/\\(artifactName).xcframework\"\n\
                     )\n\
                 }}\n\
             )\n"
        );
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create verified optional-plugin Swift package manifest '{}': {error}",
                    path.display()
                ))
            })?;
        file.write_all(manifest.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to write verified optional-plugin Swift package manifest '{}': {error}",
                    path.display()
                ))
            })
    }

    fn materialize_xcframework_archive(
        archive_path: &Path,
        artifacts: &Path,
        framework: &str,
        policy: &ZipPolicy,
    ) -> Result<(), IosError> {
        use std::os::unix::fs::PermissionsExt;

        let expected_nodes = policy.expected_nodes.as_ref().ok_or_else(|| {
            IosError::worker(format!(
                "verified {framework} XCFramework has no exact extraction policy"
            ))
        })?;
        let expected_root = format!("{framework}.xcframework");
        let expected_prefix = format!("{expected_root}/");
        let mut directory_modes = Vec::new();
        for (path, kind) in expected_nodes {
            if *kind != ArchiveNodeKind::Directory {
                continue;
            }
            if path != &expected_root && !path.starts_with(&expected_prefix) {
                return Err(IosError::worker(format!(
                    "verified {framework} extraction policy contains an unexpected root: {path}"
                )));
            }
            let destination = artifacts.join(path);
            fs::create_dir(&destination).map_err(|error| {
                IosError::storage(format!(
                    "failed to create verified {framework} XCFramework directory '{}': {error}",
                    destination.display()
                ))
            })?;
            directory_modes.push((destination, 0o755_u32));
        }

        let input = open_regular_file_nofollow(archive_path).map_err(|error| {
            IosError::storage(format!(
                "failed to open retained verified {framework} XCFramework archive '{}': {error}",
                archive_path.display()
            ))
        })?;
        let mut archive = ZipArchive::new(input).map_err(|error| {
            IosError::conformance(format!(
                "invalid retained verified {framework} XCFramework archive: {error}"
            ))
        })?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                IosError::conformance(format!(
                    "failed to read retained verified {framework} XCFramework entry {index}: {error}"
                ))
            })?;
            let label = format!("retained verified {framework} XCFramework");
            let name = strict_zip_name(&entry, &label)?;
            let kind = zip_entry_kind(&entry, &name, &label)?;
            let canonical = validate_archive_path(&name, kind, &label)?;
            if kind == ArchiveNodeKind::Directory {
                if let Some((_, mode)) = directory_modes
                    .iter_mut()
                    .find(|(path, _)| *path == artifacts.join(&canonical))
                {
                    *mode = entry.unix_mode().unwrap_or(0o755) & 0o777;
                }
                continue;
            }
            if !canonical.starts_with(&expected_prefix) {
                return Err(IosError::conformance(format!(
                    "retained verified {framework} XCFramework entry has an unexpected root: {canonical}"
                )));
            }
            let destination = artifacts.join(&canonical);
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&destination)
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to create verified {framework} XCFramework file '{}': {error}",
                        destination.display()
                    ))
                })?;
            let declared_bytes = entry.size();
            copy_declared_bytes(
                &mut entry,
                &mut output,
                declared_bytes,
                policy.maximum_entry_bytes,
                &format!("verified {framework} XCFramework entry '{canonical}'"),
            )?;
            output.sync_all().map_err(|error| {
                IosError::storage(format!(
                    "failed to synchronize verified {framework} XCFramework file '{}': {error}",
                    destination.display()
                ))
            })?;
            let mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
            fs::set_permissions(&destination, fs::Permissions::from_mode(mode)).map_err(
                |error| {
                    IosError::storage(format!(
                        "failed to preserve verified {framework} XCFramework file mode '{}': {error}",
                        destination.display()
                    ))
                },
            )?;
        }
        for (directory, mode) in directory_modes.into_iter().rev() {
            fs::set_permissions(&directory, fs::Permissions::from_mode(mode)).map_err(|error| {
                IosError::storage(format!(
                    "failed to preserve verified {framework} XCFramework directory mode '{}': {error}",
                    directory.display()
                ))
            })?;
        }
        Ok(())
    }

    fn write_sanitized_zip(
        source: &Path,
        destination: &Path,
        policy: &ZipPolicy,
        label: &str,
    ) -> Result<(), IosError> {
        let input = File::open(source).map_err(|error| {
            IosError::storage(format!(
                "failed to reopen {label} '{}': {error}",
                source.display()
            ))
        })?;
        let mut archive = ZipArchive::new(input).map_err(|error| {
            IosError::conformance(format!("invalid {label} '{}': {error}", source.display()))
        })?;
        let output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create sanitized {label} '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut writer = ZipWriter::new(output);
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                IosError::conformance(format!("failed to read {label} entry {index}: {error}"))
            })?;
            let name = strict_zip_name(&entry, label)?;
            let kind = zip_entry_kind(&entry, &name, label)?;
            let canonical = validate_archive_path(&name, kind, label)?;
            if canonical == "__MACOSX" || canonical.starts_with("__MACOSX/") {
                continue;
            }
            let mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(DateTime::default())
                .unix_permissions(mode);
            match kind {
                ArchiveNodeKind::Directory => writer.add_directory(&name, options),
                ArchiveNodeKind::File => writer.start_file(&name, options),
            }
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to write sanitized {label} entry '{name}': {error}"
                ))
            })?;
            if kind == ArchiveNodeKind::File {
                let declared_bytes = entry.size();
                copy_declared_bytes(
                    &mut entry,
                    &mut writer,
                    declared_bytes,
                    policy.maximum_entry_bytes,
                    &format!("{label} entry '{name}'"),
                )?;
            }
        }
        let output = writer.finish().map_err(|error| {
            IosError::storage(format!("failed to finish sanitized {label}: {error}"))
        })?;
        output.sync_all().map_err(|error| {
            IosError::storage(format!("failed to synchronize sanitized {label}: {error}"))
        })?;
        set_read_only(destination, label)
    }

    fn copy_declared_bytes(
        reader: &mut dyn Read,
        writer: &mut dyn Write,
        declared_bytes: u64,
        maximum_bytes: u64,
        label: &str,
    ) -> Result<(), IosError> {
        if declared_bytes > maximum_bytes {
            return Err(IosError::conformance(format!(
                "{label} declares {declared_bytes} bytes, exceeding the {maximum_bytes}-byte limit"
            )));
        }
        let mut remaining = declared_bytes;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining != 0 {
            let requested = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(buffer.len());
            let count = reader.read(&mut buffer[..requested]).map_err(|error| {
                IosError::conformance(format!("failed to read {label}: {error}"))
            })?;
            if count == 0 {
                return Err(IosError::conformance(format!(
                    "{label} ended before its declared {declared_bytes} bytes"
                )));
            }
            writer.write_all(&buffer[..count]).map_err(|error| {
                IosError::storage(format!("failed to write {label} snapshot: {error}"))
            })?;
            remaining -= count as u64;
        }
        let mut probe = [0_u8; 1];
        match reader.read(&mut probe) {
            Ok(0) => Ok(()),
            Ok(_) => Err(IosError::conformance(format!(
                "{label} expands beyond its declared {declared_bytes} bytes"
            ))),
            Err(error) => Err(IosError::conformance(format!(
                "failed to finish reading {label}: {error}"
            ))),
        }
    }

    fn verify_ffmpeg_framework_records(
        archive_path: &Path,
        framework: &str,
        release_source: &ReleaseSource,
    ) -> Result<FrameworkEvidence, IosError> {
        let root = format!("{framework}.xcframework");
        let mut profiles = Vec::new();
        let mut metadata_records = Vec::new();
        let mut metadata_bytes = Vec::new();
        let mut input_fingerprints = Vec::new();
        for (slice, metadata_name) in OPTIONAL_SLICES {
            let framework_root = format!("{root}/{slice}/{framework}.framework");
            let profile_path = format!("{framework_root}/profile-hash.txt");
            let profile_bytes =
                read_zip_entry(archive_path, &profile_path, MAX_SMALL_RECORD_BYTES)?;
            let profile = parse_profile_record(&profile_bytes, &profile_path)?.to_owned();
            validate_profile_grammar(&profile, &profile_path)?;
            profiles.push(profile);

            let metadata_path = format!("{framework_root}/{metadata_name}");
            let bytes = read_zip_entry(archive_path, &metadata_path, MAX_SMALL_RECORD_BYTES)?;
            let metadata = parse_metadata_record(
                &bytes,
                Some("Vesper FFmpeg build metadata v2"),
                &BUILD_METADATA_KEYS,
                &metadata_path,
            )?;
            validate_build_metadata(
                &metadata,
                slice,
                &profiles[profiles.len() - 1],
                &metadata_path,
                release_source,
            )?;
            metadata_records.push(metadata);
            metadata_bytes.push(bytes);

            let fingerprint_path = format!("{framework_root}/input-fingerprint.txt");
            let fingerprint_bytes =
                read_zip_entry(archive_path, &fingerprint_path, MAX_SMALL_RECORD_BYTES)?;
            let fingerprint = parse_profile_record(&fingerprint_bytes, &fingerprint_path)?;
            validate_input_fingerprint(fingerprint, &fingerprint_path)?;
            input_fingerprints.push(fingerprint.to_owned());

            let binary_path = format!("{framework_root}/{framework}");
            let checksum_path = format!("{framework_root}/binary-sha256.txt");
            verify_zip_binary_checksum(archive_path, &binary_path, &checksum_path)?;
        }
        if profiles[0] != profiles[1] {
            return Err(IosError::conformance(format!(
                "FFmpeg profile hash mismatch between device and simulator in {framework}"
            )));
        }
        for key in SLICE_INVARIANT_KEYS {
            if metadata_records[0].get(key) != metadata_records[1].get(key) {
                return Err(IosError::conformance(format!(
                    "FFmpeg metadata mismatch between device and simulator for {key} in {framework}"
                )));
            }
        }
        Ok(FrameworkEvidence {
            profile_hash: profiles.remove(0),
            declared_profile: required_metadata_value(
                &metadata_records[0],
                "declared_profile",
                framework,
            )?
            .to_owned(),
            external_dependencies: required_metadata_value(
                &metadata_records[0],
                "external_dependencies",
                framework,
            )?
            .to_owned(),
            source_archive: required_metadata_value(
                &metadata_records[0],
                "source_archive",
                framework,
            )?
            .to_owned(),
            device_metadata: metadata_bytes.remove(0),
            simulator_metadata: metadata_bytes.remove(0),
            device_input_fingerprint: input_fingerprints.remove(0),
            simulator_input_fingerprint: input_fingerprints.remove(0),
        })
    }

    fn compare_framework_evidence(
        expected: &FrameworkEvidence,
        actual: &FrameworkEvidence,
        framework: &str,
    ) -> Result<(), IosError> {
        if expected.profile_hash != actual.profile_hash {
            return Err(IosError::conformance(format!(
                "optional iOS artifacts do not share one FFmpeg profile hash: {framework}"
            )));
        }
        if expected.declared_profile != actual.declared_profile {
            return Err(IosError::conformance(format!(
                "optional iOS artifacts do not share one declared FFmpeg profile: {framework}"
            )));
        }
        if expected.source_archive != actual.source_archive {
            return Err(IosError::conformance(format!(
                "optional iOS artifacts do not share one FFmpeg source archive: {framework}"
            )));
        }
        if expected.device_metadata != actual.device_metadata
            || expected.simulator_metadata != actual.simulator_metadata
        {
            return Err(IosError::conformance(format!(
                "FFmpeg build metadata differs across optional frameworks: {framework}"
            )));
        }
        if expected.device_input_fingerprint != actual.device_input_fingerprint
            || expected.simulator_input_fingerprint != actual.simulator_input_fingerprint
        {
            return Err(IosError::conformance(format!(
                "FFmpeg input fingerprints differ across optional frameworks: {framework}"
            )));
        }
        Ok(())
    }

    fn validate_input_fingerprint(value: &str, label: &str) -> Result<(), IosError> {
        let (metadata, checksums) = value.split_once('-').ok_or_else(|| {
            IosError::conformance(format!(
                "FFmpeg input fingerprint must contain two SHA-256 values: {label}"
            ))
        })?;
        if [metadata, checksums].into_iter().any(|digest| {
            digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        }) {
            return Err(IosError::conformance(format!(
                "FFmpeg input fingerprint is invalid: {label}"
            )));
        }
        Ok(())
    }

    fn validate_build_metadata(
        metadata: &BTreeMap<String, String>,
        slice: &str,
        profile: &str,
        label: &str,
        release_source: &ReleaseSource,
    ) -> Result<(), IosError> {
        let expected_target = match slice {
            "ios-arm64" => "ios-arm64",
            "ios-arm64-simulator" => "ios-simulator-arm64",
            _ => {
                return Err(IosError::worker(format!(
                    "unknown optional iOS slice: {slice}"
                )));
            }
        };
        require_metadata_value(metadata, "platform", "apple", label)?;
        require_metadata_value(metadata, "target", expected_target, label)?;
        require_metadata_value(metadata, "declared_platform", "ios", label)?;
        require_metadata_value(
            metadata,
            "ffmpeg_version",
            &release_source.version_text,
            label,
        )?;
        require_metadata_value(metadata, "source_url", &release_source.source_url, label)?;
        require_metadata_value(
            metadata,
            "source_sha256",
            &release_source.source_sha256,
            label,
        )?;
        require_metadata_value(metadata, "profile_hash", profile, label)?;
        require_metadata_value(metadata, "license_flags", "", label)?;
        let declared_profile = required_metadata_value(metadata, "declared_profile", label)?;
        validate_profile_name(declared_profile, label)?;
        let configure = required_metadata_value(metadata, "configure_line", label)?;
        if !configure
            .split_ascii_whitespace()
            .any(|part| part == "--enable-shared")
            || configure.contains("--enable-gpl")
            || configure.contains("--enable-nonfree")
        {
            return Err(IosError::conformance(format!(
                "{label} must describe an LGPL-oriented shared FFmpeg build"
            )));
        }
        Ok(())
    }

    fn verify_compliance_source_record(
        archive_path: &Path,
        framework: &FrameworkEvidence,
        release_source: &ReleaseSource,
    ) -> Result<(), IosError> {
        let path = format!("{COMPLIANCE_ROOT}/SOURCE.txt");
        let bytes = read_zip_entry(archive_path, &path, MAX_SMALL_RECORD_BYTES)?;
        let record = parse_metadata_record(&bytes, None, &SOURCE_RECORD_KEYS, &path)?;
        for (key, expected) in [
            ("component", "FFmpeg"),
            ("ffmpeg_version", release_source.version_text.as_str()),
            ("license_mode", "LGPL-2.1-or-later"),
            ("linkage", "dynamic-frameworks"),
            ("source_url", release_source.source_url.as_str()),
            ("source_asset", release_source.source_asset.as_str()),
            ("source_sha256", release_source.source_sha256.as_str()),
            ("local_changes", "none"),
        ] {
            require_metadata_value(&record, key, expected, &path)?;
        }
        require_metadata_value(
            &record,
            "declared_profile",
            &framework.declared_profile,
            &path,
        )?;
        require_metadata_value(&record, "profile_hash", &framework.profile_hash, &path).and_then(
            |()| {
                require_metadata_value(
                    &record,
                    "external_dependencies",
                    &framework.external_dependencies,
                    &path,
                )
            },
        )
    }

    fn verify_compliance_bundle(
        root: &Path,
        archive_path: &Path,
        source_archive: &Path,
        framework: &FrameworkEvidence,
        release_source: &ReleaseSource,
    ) -> Result<(), IosError> {
        verify_compliance_source_record(archive_path, framework, release_source)?;

        let mut entries = BTreeMap::new();
        for relative in [
            "README.md",
            "NOTICE.txt",
            "SOURCE.txt",
            "BUILDING.md",
            "RELINKING.md",
            "COPYING.LGPLv2.1",
            "COPYING.LGPLv3",
            "FFMPEG_LICENSE.md",
            "VESPER_THIRD_PARTY_NOTICES.md",
            "build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt",
            "build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
        ] {
            let path = format!("{COMPLIANCE_ROOT}/{relative}");
            let bytes = read_zip_entry(archive_path, &path, MAX_COMPLIANCE_ENTRY_BYTES)?;
            if bytes.is_empty() {
                return Err(IosError::conformance(format!(
                    "Missing or empty compliance bundle entry: {relative}"
                )));
            }
            entries.insert(relative, bytes);
        }
        let changes_path = format!("{COMPLIANCE_ROOT}/changes.diff");
        if !read_zip_entry(archive_path, &changes_path, MAX_COMPLIANCE_ENTRY_BYTES)?.is_empty() {
            return Err(IosError::conformance(
                "changes.diff must be empty when local_changes=none.",
            ));
        }

        compare_release_bytes(
            &framework.device_metadata,
            required_compliance_entry(
                &entries,
                "build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt",
            )?,
            "The device FFmpeg build metadata copy",
        )?;
        compare_release_bytes(
            &framework.simulator_metadata,
            required_compliance_entry(
                &entries,
                "build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
            )?,
            "The Simulator FFmpeg build metadata copy",
        )?;

        let source_files = read_source_license_files(source_archive, &release_source.version)?;
        compare_release_bytes(
            required_source_file(&source_files, "COPYING.LGPLv2.1")?,
            required_compliance_entry(&entries, "COPYING.LGPLv2.1")?,
            "The LGPL-2.1 license copy",
        )?;
        compare_release_bytes(
            required_source_file(&source_files, "COPYING.LGPLv3")?,
            required_compliance_entry(&entries, "COPYING.LGPLv3")?,
            "The LGPL-3.0 license copy",
        )?;
        compare_release_bytes(
            required_source_file(&source_files, "LICENSE.md")?,
            required_compliance_entry(&entries, "FFMPEG_LICENSE.md")?,
            "The FFmpeg license summary",
        )?;
        let notices = read_regular_file_bounded(
            &root.join("THIRD_PARTY_NOTICES.md"),
            MAX_COMPLIANCE_ENTRY_BYTES,
            "Vesper third-party notices",
        )?;
        compare_release_bytes(
            &notices,
            required_compliance_entry(&entries, "VESPER_THIRD_PARTY_NOTICES.md")?,
            "The Vesper third-party notices copy",
        )?;

        let readme = required_compliance_entry(&entries, "README.md")?;
        require_text_snippets(
            readme,
            "Compliance README",
            [
                release_source.source_asset.as_str(),
                framework.profile_hash.as_str(),
                "VesperFFmpegAV*",
                "Remux",
                "SourceNormalizer",
            ],
        )?;
        let source_sha = format!("Source SHA-256: {}", release_source.source_sha256);
        let build_profile = format!(
            "Build profile: {} ({})",
            framework.declared_profile, framework.profile_hash
        );
        require_text_snippets(
            required_compliance_entry(&entries, "NOTICE.txt")?,
            "FFmpeg notice",
            [
                "GNU Lesser General Public License version 2.1 or later",
                "not licensed under Vesper's Apache-2.0 license",
                release_source.source_asset.as_str(),
                source_sha.as_str(),
                build_profile.as_str(),
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
                "RELINKING.md",
            ],
        )?;
        require_text_snippets(
            required_compliance_entry(&entries, "RELINKING.md")?,
            "FFmpeg relinking instructions",
            [
                "VesperFFmpegAVCodec.framework",
                "VesperFFmpegAVFormat.framework",
                "VesperFFmpegAVUtil.framework",
                "BUILDING.md",
                "Embed & Sign",
                "@rpath/<Name>.framework/<Name>",
                "source availability",
                "relinking rights",
                "LGPL-compatible reverse-engineering terms",
            ],
        )?;
        let version_environment = format!(
            "VESPER_APPLE_FFMPEG_VERSION={}",
            release_source.version_text
        );
        let archive_environment = format!(
            "VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE=/path/to/{}",
            release_source.source_asset
        );
        let url_environment = format!(
            "VESPER_APPLE_FFMPEG_SOURCE_URL={}",
            release_source.source_url
        );
        let profile_argument = format!("--profile {}", framework.declared_profile);
        require_text_snippets(
            required_compliance_entry(&entries, "BUILDING.md")?,
            "BUILDING.md",
            [
                version_environment.as_str(),
                archive_environment.as_str(),
                url_environment.as_str(),
                "VESPER_APPLE_FFMPEG_FORCE=1",
                profile_argument.as_str(),
            ],
        )
    }

    fn required_compliance_entry<'a>(
        entries: &'a BTreeMap<&str, Vec<u8>>,
        name: &str,
    ) -> Result<&'a [u8], IosError> {
        entries.get(name).map(Vec::as_slice).ok_or_else(|| {
            IosError::worker(format!(
                "verified compliance entry disappeared from the in-memory map: {name}"
            ))
        })
    }

    fn compare_release_bytes(
        expected: &[u8],
        actual: &[u8],
        description: &str,
    ) -> Result<(), IosError> {
        if expected != actual {
            return Err(IosError::conformance(format!(
                "{description} does not match its release source"
            )));
        }
        Ok(())
    }

    fn require_text_snippets<'a>(
        bytes: &[u8],
        description: &str,
        snippets: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), IosError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            IosError::conformance(format!("{description} is not UTF-8: {error}"))
        })?;
        for snippet in snippets {
            if !text.contains(snippet) {
                return Err(IosError::conformance(format!(
                    "{description} is missing required release text: {snippet}"
                )));
            }
        }
        Ok(())
    }

    fn read_source_license_files(
        source_archive: &Path,
        version: &Version,
    ) -> Result<BTreeMap<String, Vec<u8>>, IosError> {
        let file = File::open(source_archive).map_err(|error| {
            IosError::storage(format!(
                "failed to open FFmpeg source archive '{}': {error}",
                source_archive.display()
            ))
        })?;
        let decoder = XzDecoder::new(file);
        let limited = ReadBudget::new(decoder, MAX_TAR_STREAM_BYTES);
        let mut archive = tar::Archive::new(limited);
        let root = format!("ffmpeg-{version}");
        let required = ["COPYING.LGPLv2.1", "COPYING.LGPLv3", "LICENSE.md"];
        let mut files = BTreeMap::new();
        for entry in archive.entries().map_err(|error| {
            IosError::conformance(format!("invalid FFmpeg source tar archive: {error}"))
        })? {
            let mut entry = entry.map_err(|error| {
                IosError::conformance(format!("invalid FFmpeg source tar entry: {error}"))
            })?;
            let path = std::str::from_utf8(entry.path_bytes().as_ref())
                .map_err(|error| {
                    IosError::conformance(format!(
                        "FFmpeg source tar contains a non-UTF-8 path: {error}"
                    ))
                })?
                .to_owned();
            let Some(relative) = path.strip_prefix(&format!("{root}/")) else {
                continue;
            };
            if !required.contains(&relative) {
                continue;
            }
            if !entry.header().entry_type().is_file() {
                return Err(IosError::conformance(format!(
                    "FFmpeg source license is not a regular file: {path}"
                )));
            }
            let declared_bytes = entry.size();
            let mut bytes = Vec::new();
            copy_declared_bytes(
                &mut entry,
                &mut bytes,
                declared_bytes,
                MAX_COMPLIANCE_ENTRY_BYTES,
                &format!("FFmpeg source entry '{path}'"),
            )?;
            files.insert(relative.to_owned(), bytes);
            if files.len() == required.len() {
                break;
            }
        }
        for name in required {
            if !files.contains_key(name) {
                return Err(IosError::conformance(format!(
                    "FFmpeg source archive is missing required license file: {name}"
                )));
            }
        }
        Ok(files)
    }

    fn required_source_file<'a>(
        files: &'a BTreeMap<String, Vec<u8>>,
        name: &str,
    ) -> Result<&'a [u8], IosError> {
        files.get(name).map(Vec::as_slice).ok_or_else(|| {
            IosError::worker(format!(
                "verified FFmpeg source license disappeared from the in-memory map: {name}"
            ))
        })
    }

    fn read_regular_file_bounded(
        path: &Path,
        maximum_bytes: u64,
        label: &str,
    ) -> Result<Vec<u8>, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() > maximum_bytes {
            return Err(IosError::conformance(format!(
                "{label} must be a regular non-symlink file no larger than {maximum_bytes} bytes: {}",
                path.display()
            )));
        }
        fs::read(path).map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })
    }

    fn parse_metadata_record(
        bytes: &[u8],
        header: Option<&str>,
        expected_keys: &[&str],
        label: &str,
    ) -> Result<BTreeMap<String, String>, IosError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| IosError::conformance(format!("{label} is not UTF-8: {error}")))?;
        if text.contains('\r')
            || text
                .chars()
                .any(|character| character.is_control() && character != '\n')
        {
            return Err(IosError::conformance(format!(
                "{label} contains non-canonical control characters"
            )));
        }
        let mut lines = text.split('\n').collect::<Vec<_>>();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
            return Err(IosError::conformance(format!(
                "{label} must contain exact non-empty records"
            )));
        }
        if let Some(expected_header) = header {
            if lines.first() != Some(&expected_header) {
                return Err(IosError::conformance(format!(
                    "{label} has an invalid metadata header"
                )));
            }
            lines.remove(0);
        }
        let allowed = expected_keys.iter().copied().collect::<BTreeSet<_>>();
        let mut values = BTreeMap::new();
        for line in lines {
            let (key, value) = line.split_once('=').ok_or_else(|| {
                IosError::conformance(format!("{label} contains a malformed metadata record"))
            })?;
            if !allowed.contains(key) {
                return Err(IosError::conformance(format!(
                    "{label} contains unknown metadata key '{key}'"
                )));
            }
            if values.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(IosError::conformance(format!(
                    "Duplicate FFmpeg metadata key '{key}': {label}"
                )));
            }
        }
        for key in &allowed {
            if !values.contains_key(*key) {
                return Err(IosError::conformance(format!(
                    "Missing FFmpeg metadata key '{key}': {label}"
                )));
            }
        }
        Ok(values)
    }

    fn required_metadata_value<'a>(
        metadata: &'a BTreeMap<String, String>,
        key: &str,
        label: &str,
    ) -> Result<&'a str, IosError> {
        metadata.get(key).map(String::as_str).ok_or_else(|| {
            IosError::conformance(format!("Missing FFmpeg metadata key '{key}': {label}"))
        })
    }

    fn require_metadata_value(
        metadata: &BTreeMap<String, String>,
        key: &str,
        expected: &str,
        label: &str,
    ) -> Result<(), IosError> {
        let actual = required_metadata_value(metadata, key, label)?;
        if actual != expected {
            return Err(IosError::conformance(format!(
                "Unexpected FFmpeg metadata value for '{key}' in {label}: expected {expected:?}, found {actual:?}"
            )));
        }
        Ok(())
    }

    fn parse_profile_record<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, IosError> {
        let record = bytes
            .strip_suffix(b"\r\n")
            .or_else(|| bytes.strip_suffix(b"\n"))
            .unwrap_or(bytes);
        let record = std::str::from_utf8(record).map_err(|error| {
            IosError::conformance(format!(
                "FFmpeg profile hash is not UTF-8 in {label}: {error}"
            ))
        })?;
        if record.is_empty()
            || record
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(IosError::conformance(format!(
                "FFmpeg profile hash must be one exact non-whitespace record: {label}"
            )));
        }
        Ok(record)
    }

    fn validate_profile_grammar(profile: &str, label: &str) -> Result<(), IosError> {
        if profile == "legacy" {
            return Ok(());
        }
        let (name, hash) = profile.rsplit_once('-').ok_or_else(|| {
            IosError::conformance(format!(
                "FFmpeg profile hash must be 'legacy' or '<profile>-<12 lowercase hex>': {label}"
            ))
        })?;
        validate_profile_name(name, label)?;
        if hash.len() != 12
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(IosError::conformance(format!(
                "FFmpeg profile hash must end in 12 lowercase hexadecimal characters: {label}"
            )));
        }
        Ok(())
    }

    fn validate_profile_name(name: &str, label: &str) -> Result<(), IosError> {
        if name.is_empty()
            || name.len() > 64
            || !name.as_bytes()[0].is_ascii_lowercase()
            || !name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(IosError::conformance(format!(
                "invalid FFmpeg profile name in {label}: {name:?}"
            )));
        }
        Ok(())
    }

    fn verify_zip_binary_checksum(
        archive_path: &Path,
        binary_path: &str,
        checksum_path: &str,
    ) -> Result<(), IosError> {
        let checksum_bytes = read_zip_entry(archive_path, checksum_path, 256)?;
        let checksum = parse_sha256_record(&checksum_bytes, checksum_path)?;
        let binary = read_zip_entry(archive_path, binary_path, MAX_ZIP_ENTRY_BYTES)?;
        let actual = hex::encode(Sha256::digest(&binary));
        if actual != checksum {
            return Err(IosError::conformance(format!(
                "FFmpeg-backed framework binary SHA-256 mismatch:\n  binary:   {binary_path}\n  recorded: {checksum}\n  actual:   {actual}"
            )));
        }
        Ok(())
    }

    fn parse_sha256_record<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, IosError> {
        let record = parse_profile_record(bytes, label)?;
        if record.len() != 64
            || !record
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(IosError::conformance(format!(
                "invalid lowercase SHA-256 record: {label}"
            )));
        }
        Ok(record)
    }

    fn read_zip_entry(path: &Path, name: &str, maximum_bytes: u64) -> Result<Vec<u8>, IosError> {
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open ZIP archive '{}': {error}",
                path.display()
            ))
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            IosError::conformance(format!("invalid ZIP archive '{}': {error}", path.display()))
        })?;
        let mut entry = archive.by_name(name).map_err(|error| {
            IosError::conformance(format!("missing ZIP entry '{name}': {error}"))
        })?;
        if entry.size() > maximum_bytes {
            return Err(IosError::conformance(format!(
                "ZIP entry '{name}' exceeds {maximum_bytes} bytes"
            )));
        }
        let declared_bytes = entry.size();
        let capacity = usize::try_from(declared_bytes).map_err(|_| {
            IosError::conformance(format!("ZIP entry '{name}' is too large for this host"))
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        copy_declared_bytes(
            &mut entry,
            &mut bytes,
            declared_bytes,
            maximum_bytes,
            &format!("ZIP entry '{name}'"),
        )?;
        Ok(bytes)
    }

    #[derive(Debug)]
    struct ArchivedFrameworkPlist {
        executable: String,
        bundle_identifier: String,
        bundle_name: String,
        bundle_version: String,
        short_version: String,
        package_type: String,
        minimum_os: String,
        supported_platforms: Vec<String>,
        dt_platform_name: String,
    }

    fn verify_xcframework_manifest(archive: &Path, framework: &str) -> Result<(), IosError> {
        let plist_path = format!("{framework}.xcframework/Info.plist");
        let value = read_archived_plist_json(archive, &plist_path)?;
        let object = value.as_object().ok_or_else(|| {
            IosError::conformance(format!(
                "archived XCFramework manifest is not a dictionary: {plist_path}"
            ))
        })?;
        for (key, expected) in [
            ("CFBundlePackageType", "XFWK"),
            ("XCFrameworkFormatVersion", "1.0"),
        ] {
            if object.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
                return Err(IosError::conformance(format!(
                    "Unexpected XCFramework {key}: {plist_path}"
                )));
            }
        }
        let libraries = object
            .get("AvailableLibraries")
            .and_then(serde_json::Value::as_array)
            .filter(|libraries| libraries.len() == 2)
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "XCFramework manifest must contain exactly two libraries: {plist_path}"
                ))
            })?;
        let mut records = BTreeMap::new();
        for library in libraries {
            let library = library.as_object().ok_or_else(|| {
                IosError::conformance(format!(
                    "XCFramework AvailableLibraries entry is not a dictionary: {plist_path}"
                ))
            })?;
            let identifier = library
                .get("LibraryIdentifier")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "XCFramework library omits LibraryIdentifier: {plist_path}"
                    ))
                })?;
            if records.insert(identifier, library).is_some() {
                return Err(IosError::conformance(format!(
                    "XCFramework manifest repeats LibraryIdentifier '{identifier}': {plist_path}"
                )));
            }
        }
        for (identifier, expected_variant) in [
            ("ios-arm64", None),
            ("ios-arm64-simulator", Some("simulator")),
        ] {
            let library = records.get(identifier).ok_or_else(|| {
                IosError::conformance(format!(
                    "XCFramework manifest omits LibraryIdentifier '{identifier}': {plist_path}"
                ))
            })?;
            let expected_library_path = format!("{framework}.framework");
            let expected_binary_path = format!("{framework}.framework/{framework}");
            for (key, expected) in [
                ("LibraryPath", expected_library_path.as_str()),
                ("BinaryPath", expected_binary_path.as_str()),
                ("SupportedPlatform", "ios"),
            ] {
                if library.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
                    return Err(IosError::conformance(format!(
                        "Unexpected XCFramework {key} for '{identifier}': {plist_path}"
                    )));
                }
            }
            let architectures = library
                .get("SupportedArchitectures")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "XCFramework library omits SupportedArchitectures: {plist_path}"
                    ))
                })?;
            if architectures.as_slice() != [serde_json::Value::String("arm64".to_owned())] {
                return Err(IosError::conformance(format!(
                    "Unexpected XCFramework SupportedArchitectures for '{identifier}': {plist_path}"
                )));
            }
            let actual_variant = library
                .get("SupportedPlatformVariant")
                .and_then(serde_json::Value::as_str);
            if actual_variant != expected_variant {
                return Err(IosError::conformance(format!(
                    "Unexpected XCFramework SupportedPlatformVariant for '{identifier}': {plist_path}"
                )));
            }
        }
        Ok(())
    }

    fn verify_framework_binary_records(archive: &Path, framework: &str) -> Result<(), IosError> {
        for (slice, expected_platform, expected_supported_platform, expected_dt_platform) in [
            ("ios-arm64", "IOS", "iPhoneOS", "iphoneos"),
            (
                "ios-arm64-simulator",
                "IOSSIMULATOR",
                "iPhoneSimulator",
                "iphonesimulator",
            ),
        ] {
            let framework_root = format!("{framework}.xcframework/{slice}/{framework}.framework");
            let plist_path = format!("{framework_root}/Info.plist");
            let plist = read_archived_framework_plist(archive, &plist_path)?;
            if plist.executable != framework
                || plist.bundle_identifier.is_empty()
                || plist.bundle_name.is_empty()
                || plist.bundle_version.is_empty()
                || plist.short_version.is_empty()
                || plist.package_type != "FMWK"
                || plist.supported_platforms != [expected_supported_platform]
                || plist.dt_platform_name != expected_dt_platform
            {
                return Err(IosError::conformance(format!(
                    "Invalid framework bundle metadata: {plist_path}"
                )));
            }
            if let Some(expected_bundle) = expected_framework_bundle_identifier(framework)
                && plist.bundle_identifier != expected_bundle
            {
                return Err(IosError::conformance(format!(
                    "Unexpected framework bundle identifier in {plist_path}: expected {expected_bundle}, found {}",
                    plist.bundle_identifier
                )));
            }

            let binary_path = format!("{framework_root}/{framework}");
            let binary = read_zip_entry(archive, &binary_path, MAX_ZIP_ENTRY_BYTES)?;
            let temporary = tempfile::Builder::new()
                .prefix(&format!("{framework}-"))
                .tempdir()
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to create temporary framework binary for {binary_path}: {error}"
                    ))
                })?;
            let temporary_binary = temporary.path().join(framework);
            let mut temporary_file = File::create(&temporary_binary).map_err(|error| {
                IosError::storage(format!(
                    "failed to create temporary framework binary for {binary_path}: {error}"
                ))
            })?;
            temporary_file.write_all(&binary).map_err(|error| {
                IosError::storage(format!(
                    "failed to stage framework binary for {binary_path}: {error}"
                ))
            })?;
            temporary_file.flush().map_err(|error| {
                IosError::storage(format!(
                    "failed to flush framework binary for {binary_path}: {error}"
                ))
            })?;

            let mut lipo = Command::new(configured_release_tool("LIPO", "lipo"));
            lipo.args(["-archs"])
                .arg(&temporary_binary)
                .stdin(Stdio::null());
            let architectures = run_release_tool(&mut lipo, "framework architecture inspection")?;
            if std::str::from_utf8(&architectures).map(str::trim).ok() != Some("arm64") {
                return Err(IosError::conformance(format!(
                    "Optional iOS XCFramework slices must contain only arm64: {binary_path}"
                )));
            }

            let mut install_name = Command::new(configured_release_tool("OTOOL", "otool"));
            install_name
                .arg("-D")
                .arg(&temporary_binary)
                .stdin(Stdio::null());
            let install_name =
                run_release_tool(&mut install_name, "framework install-name inspection")?;
            let install_name = std::str::from_utf8(&install_name).map_err(|error| {
                IosError::conformance(format!(
                    "otool -D output is not UTF-8 for {binary_path}: {error}"
                ))
            })?;
            let expected_install_name = format!("@rpath/{framework}.framework/{framework}");
            if install_name
                .lines()
                .skip(1)
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                != Some(expected_install_name.as_str())
            {
                return Err(IosError::conformance(format!(
                    "Unexpected framework install name for {binary_path}"
                )));
            }

            let mut dependencies = Command::new(configured_release_tool("OTOOL", "otool"));
            dependencies
                .arg("-L")
                .arg(&temporary_binary)
                .stdin(Stdio::null());
            let dependencies =
                run_release_tool(&mut dependencies, "framework dependency inspection")?;
            let dependencies = std::str::from_utf8(&dependencies).map_err(|error| {
                IosError::conformance(format!(
                    "otool -L output is not UTF-8 for {binary_path}: {error}"
                ))
            })?;
            let dependencies = crate::ios::parse_otool_dependencies(dependencies, &binary_path)?;
            crate::ios::validate_framework_dependency_list(&binary_path, framework, &dependencies)?;

            let mut vtool = Command::new(configured_release_tool("XCRUN", "xcrun"));
            vtool
                .args(["vtool", "-show-build"])
                .arg(&temporary_binary)
                .stdin(Stdio::null());
            let metadata = run_release_tool(&mut vtool, "framework Mach-O platform inspection")?;
            let metadata = std::str::from_utf8(&metadata).map_err(|error| {
                IosError::conformance(format!(
                    "xcrun vtool output is not UTF-8 for {binary_path}: {error}"
                ))
            })?;
            let metadata = crate::ios::parse_vtool_build_metadata(metadata, &binary_path)?;
            if metadata.platform != expected_platform {
                return Err(IosError::conformance(format!(
                    "Unexpected framework Mach-O build platform for {binary_path}: expected {expected_platform}, found {}",
                    metadata.platform
                )));
            }
            if crate::ios::normalize_apple_version(
                &metadata.minimum_os,
                "framework Mach-O minimum OS",
            )? != crate::ios::normalize_apple_version(
                &plist.minimum_os,
                "framework MinimumOSVersion",
            )? {
                return Err(IosError::conformance(format!(
                    "Framework Mach-O minimum OS does not match Info.plist: {binary_path}"
                )));
            }
        }
        Ok(())
    }

    fn expected_framework_bundle_identifier(framework: &str) -> Option<&'static str> {
        match framework {
            "VesperFFmpegAVCodec" => Some("io.github.umbrella22.vesper.ffmpeg.avcodec"),
            "VesperFFmpegAVFormat" => Some("io.github.umbrella22.vesper.ffmpeg.avformat"),
            "VesperFFmpegAVUtil" => Some("io.github.umbrella22.vesper.ffmpeg.avutil"),
            _ => crate::ios_plugin::IOS_PLUGIN_SPECS
                .iter()
                .find(|plugin| plugin.framework_name == framework)
                .map(|plugin| plugin.bundle_identifier),
        }
    }

    fn configured_release_tool(variable: &str, default: &str) -> std::ffi::OsString {
        env::var_os(variable)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.into())
    }

    fn run_release_tool(command: &mut Command, label: &str) -> Result<Vec<u8>, IosError> {
        let result = external_process::run_interruptible_capture(
            command,
            label,
            MAX_SMALL_RECORD_BYTES as usize,
            MAX_SMALL_RECORD_BYTES as usize,
        )
        .map_err(|error| match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        })?;
        if !result.status.success() {
            let diagnostic = String::from_utf8_lossy(&result.stderr);
            return Err(IosError::conformance(format!(
                "{label} failed with {}{}{}",
                result.status,
                if diagnostic.is_empty() { "" } else { ": " },
                diagnostic.trim_end()
            )));
        }
        Ok(result.stdout)
    }

    fn verify_plugin_framework_registry_archive(
        root: &Path,
        archive: &Path,
        plugin: &crate::ios_plugin::IosPluginSpec,
    ) -> Result<(), IosError> {
        for slice in [
            crate::ios_plugin::IosPluginSlice::DeviceArm64,
            crate::ios_plugin::IosPluginSlice::SimulatorArm64,
        ] {
            let framework_root = format!(
                "{}.xcframework/{}/{}.framework",
                plugin.framework_name,
                match slice {
                    crate::ios_plugin::IosPluginSlice::DeviceArm64 => "ios-arm64",
                    crate::ios_plugin::IosPluginSlice::SimulatorArm64 => "ios-arm64-simulator",
                },
                plugin.framework_name,
            );
            let plist_path = format!("{framework_root}/Info.plist");
            let plist = read_archived_framework_plist(archive, &plist_path)?;
            if plist.executable != plugin.framework_name
                || plist.bundle_identifier != plugin.bundle_identifier
            {
                return Err(IosError::conformance(format!(
                    "iOS plugin archive framework identity is invalid: {plist_path}"
                )));
            }

            let registry_path = format!("{framework_root}/vesper-plugin-registry.json");
            let actual = read_zip_entry(
                archive,
                &registry_path,
                player_plugin_loader::MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES as u64,
            )?;
            let expected = crate::ios_plugin_release::canonical_registry_fragment(
                root,
                plugin.id,
                slice,
                &plist.minimum_os,
            )?;
            validate_plugin_registry_fragment(
                &actual,
                &expected,
                slice.rust_target(),
                &registry_path,
            )?;
        }
        Ok(())
    }

    fn validate_plugin_registry_fragment(
        actual: &[u8],
        expected: &[u8],
        expected_target: &str,
        label: &str,
    ) -> Result<(), IosError> {
        EmbeddedPluginRegistry::parse(actual, expected_target, "arm64").map_err(|error| {
            IosError::conformance(format!(
                "iOS plugin archive registry fragment is invalid ({label}): {error}"
            ))
        })?;
        if actual != expected {
            return Err(IosError::conformance(format!(
                "iOS plugin archive registry fragment does not match its canonical descriptor and framework metadata: {label}"
            )));
        }
        Ok(())
    }

    fn read_archived_framework_plist(
        archive: &Path,
        plist_path: &str,
    ) -> Result<ArchivedFrameworkPlist, IosError> {
        let value = read_archived_plist_json(archive, plist_path)?;
        let object = value.as_object().ok_or_else(|| {
            IosError::conformance(format!(
                "archived iOS plugin framework plist is not a dictionary: {plist_path}"
            ))
        })?;
        let required = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    IosError::conformance(format!(
                        "archived iOS plugin framework plist omits {key}: {plist_path}"
                    ))
                })
        };
        let supported_platforms = object
            .get("CFBundleSupportedPlatforms")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "archived iOS plugin framework plist omits CFBundleSupportedPlatforms: {plist_path}"
                ))
            })?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    IosError::conformance(format!(
                        "archived framework CFBundleSupportedPlatforms is invalid: {plist_path}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ArchivedFrameworkPlist {
            executable: required("CFBundleExecutable")?,
            bundle_identifier: required("CFBundleIdentifier")?,
            bundle_name: required("CFBundleName")?,
            bundle_version: required("CFBundleVersion")?,
            short_version: required("CFBundleShortVersionString")?,
            package_type: required("CFBundlePackageType")?,
            minimum_os: required("MinimumOSVersion")?,
            supported_platforms,
            dt_platform_name: required("DTPlatformName")?,
        })
    }

    fn read_archived_plist_json(
        archive: &Path,
        plist_path: &str,
    ) -> Result<serde_json::Value, IosError> {
        let bytes = read_zip_entry(archive, plist_path, MAX_SMALL_RECORD_BYTES)?;
        let mut temporary = NamedTempFile::new().map_err(|error| {
            IosError::storage(format!(
                "failed to create temporary framework plist for {plist_path}: {error}"
            ))
        })?;
        temporary.write_all(&bytes).map_err(|error| {
            IosError::storage(format!(
                "failed to stage framework plist for {plist_path}: {error}"
            ))
        })?;
        temporary.flush().map_err(|error| {
            IosError::storage(format!(
                "failed to flush framework plist for {plist_path}: {error}"
            ))
        })?;

        let mut command = Command::new("/usr/bin/plutil");
        command
            .args(["-convert", "json", "-o", "-"])
            .arg(temporary.path())
            .stdin(Stdio::null());
        let result = external_process::run_interruptible_capture(
            &mut command,
            "archived iOS plugin framework plist conversion",
            MAX_SMALL_RECORD_BYTES as usize,
            MAX_SMALL_RECORD_BYTES as usize,
        )
        .map_err(|error| match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        })?;
        if !result.status.success() {
            return Err(IosError::conformance(format!(
                "invalid archived iOS plugin framework plist: {plist_path}"
            )));
        }
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "archived iOS plugin framework plist is invalid JSON after conversion ({plist_path}): {error}"
            ))
        })
    }

    fn preflight_source_tar(path: &Path, version: &Version) -> Result<(), IosError> {
        let metadata = fs::metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect FFmpeg source archive '{}': {error}",
                path.display()
            ))
        })?;
        if metadata.len() > MAX_TAR_ARCHIVE_BYTES {
            return Err(IosError::conformance(format!(
                "FFmpeg source archive exceeds {MAX_TAR_ARCHIVE_BYTES} compressed bytes"
            )));
        }
        let file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open FFmpeg source archive '{}': {error}",
                path.display()
            ))
        })?;
        let decoder = XzDecoder::new(file);
        let limited = ReadBudget::new(decoder, MAX_TAR_STREAM_BYTES);
        let mut archive = tar::Archive::new(limited);
        let entries = archive.entries().map_err(|error| {
            IosError::conformance(format!("invalid FFmpeg source tar archive: {error}"))
        })?;
        let mut nodes = BTreeMap::new();
        let mut entry_count = 0_usize;
        let mut expanded_bytes = 0_u64;
        for entry in entries {
            entry_count = entry_count
                .checked_add(1)
                .ok_or_else(|| IosError::conformance("FFmpeg source tar entry count overflowed"))?;
            if entry_count > MAX_TAR_ENTRIES {
                return Err(IosError::conformance(format!(
                    "FFmpeg source tar contains more than {MAX_TAR_ENTRIES} entries"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::conformance(format!("invalid FFmpeg source tar entry: {error}"))
            })?;
            let entry_type = entry.header().entry_type();
            let kind = if entry_type.is_file() {
                ArchiveNodeKind::File
            } else if entry_type.is_dir() {
                ArchiveNodeKind::Directory
            } else {
                return Err(IosError::conformance(format!(
                    "FFmpeg source tar contains a link or unsupported file type: {:?}",
                    entry.path_bytes()
                )));
            };
            let path_bytes = entry.path_bytes();
            let path = std::str::from_utf8(path_bytes.as_ref()).map_err(|error| {
                IosError::conformance(format!(
                    "FFmpeg source tar contains a non-UTF-8 path: {error}"
                ))
            })?;
            let canonical = validate_archive_path(path, kind, "FFmpeg source tar")?;
            let root = format!("ffmpeg-{version}");
            if canonical != root && !canonical.starts_with(&format!("{root}/")) {
                return Err(IosError::conformance(format!(
                    "FFmpeg source tar entry is outside the canonical '{root}/' root: {path}"
                )));
            }
            insert_archive_node(&mut nodes, &canonical, kind, "FFmpeg source tar")?;
            if entry.size() > MAX_TAR_ENTRY_BYTES {
                return Err(IosError::conformance(format!(
                    "FFmpeg source tar entry exceeds {MAX_TAR_ENTRY_BYTES} bytes: {path}"
                )));
            }
            expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
                IosError::conformance("FFmpeg source tar expanded size overflowed")
            })?;
            if expanded_bytes > MAX_TAR_EXPANDED_BYTES {
                return Err(IosError::conformance(format!(
                    "FFmpeg source tar expands beyond {MAX_TAR_EXPANDED_BYTES} bytes"
                )));
            }
        }
        if entry_count == 0 {
            return Err(IosError::conformance("FFmpeg source tar is empty"));
        }
        if expanded_bytes > metadata.len().saturating_mul(MAX_TAR_COMPRESSION_RATIO) {
            return Err(IosError::conformance(format!(
                "FFmpeg source tar exceeds the {MAX_TAR_COMPRESSION_RATIO}:1 compression-ratio limit"
            )));
        }
        Ok(())
    }

    fn copy_opened_file_snapshot(
        source: &mut OpenedReleaseAsset,
        destination: &Path,
        label: &str,
    ) -> Result<(), IosError> {
        let before = source.file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened {label} '{}': {error}",
                source.display_path.display()
            ))
        })?;
        if !before.file_type().is_file() || file_identity(&before) != source.identity {
            return Err(IosError::storage(format!(
                "{label} changed before it was copied: {}",
                source.display_path.display()
            )));
        }
        source.file.seek(SeekFrom::Start(0)).map_err(|error| {
            IosError::storage(format!(
                "failed to rewind {label} '{}': {error}",
                source.display_path.display()
            ))
        })?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create {label} snapshot '{}': {error}",
                    destination.display()
                ))
            })?;
        copy_regular_file_bytes(
            &mut source.file,
            &mut output,
            before.len(),
            label,
            &source.display_path,
        )?;
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to synchronize {label} snapshot '{}': {error}",
                destination.display()
            ))
        })?;
        let after = source.file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect opened {label} '{}': {error}",
                source.display_path.display()
            ))
        })?;
        if !after.file_type().is_file() || file_identity(&after) != source.identity {
            return Err(IosError::storage(format!(
                "{label} changed while it was copied: {}",
                source.display_path.display()
            )));
        }
        set_read_only(destination, label)
    }

    fn copy_regular_file_snapshot(
        source: &Path,
        destination: &Path,
        label: &str,
    ) -> Result<(), IosError> {
        let before = fs::symlink_metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                source.display()
            ))
        })?;
        if !before.file_type().is_file() {
            return Err(IosError::conformance(format!(
                "{label} must be a regular non-symlink file: {}",
                source.display()
            )));
        }
        let identity = file_identity(&before);
        let mut input = open_regular_file_nofollow(source).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} '{}': {error}",
                source.display()
            ))
        })?;
        let opened = input.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect opened {label} '{}': {error}",
                source.display()
            ))
        })?;
        if file_identity(&opened) != identity || !opened.file_type().is_file() {
            return Err(IosError::storage(format!(
                "{label} changed while it was opened: {}",
                source.display()
            )));
        }
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create {label} snapshot '{}': {error}",
                    destination.display()
                ))
            })?;
        copy_regular_file_bytes(&mut input, &mut output, before.len(), label, source)?;
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to synchronize {label} snapshot '{}': {error}",
                destination.display()
            ))
        })?;
        let after = fs::symlink_metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to re-inspect {label} '{}': {error}",
                source.display()
            ))
        })?;
        if !after.file_type().is_file() || file_identity(&after) != identity {
            return Err(IosError::storage(format!(
                "{label} changed while it was copied: {}",
                source.display()
            )));
        }
        set_read_only(destination, label)
    }

    fn copy_regular_file_bytes(
        reader: &mut File,
        writer: &mut File,
        declared_bytes: u64,
        label: &str,
        source: &Path,
    ) -> Result<(), IosError> {
        if declared_bytes > MAX_RELEASE_ASSET_BYTES {
            return Err(IosError::conformance(format!(
                "{label} exceeds {MAX_RELEASE_ASSET_BYTES} bytes: {}",
                source.display()
            )));
        }
        let mut remaining = declared_bytes;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining != 0 {
            let requested = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(buffer.len());
            let count = reader.read(&mut buffer[..requested]).map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    source.display()
                ))
            })?;
            if count == 0 {
                return Err(IosError::storage(format!(
                    "{label} ended before its declared {declared_bytes} bytes: {}",
                    source.display()
                )));
            }
            writer.write_all(&buffer[..count]).map_err(|error| {
                IosError::storage(format!(
                    "failed to write {label} snapshot '{}': {error}",
                    source.display()
                ))
            })?;
            remaining -= count as u64;
        }
        let mut probe = [0_u8; 1];
        match reader.read(&mut probe) {
            Ok(0) => Ok(()),
            Ok(_) => Err(IosError::storage(format!(
                "{label} grew beyond its declared {declared_bytes} bytes while it was copied: {}",
                source.display()
            ))),
            Err(error) => Err(IosError::storage(format!(
                "failed to finish reading {label} '{}': {error}",
                source.display()
            ))),
        }
    }

    #[cfg(unix)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
        length: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    }

    #[cfg(unix)]
    fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
        use std::os::unix::fs::MetadataExt;

        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }

    #[cfg(not(unix))]
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FileIdentity {
        length: u64,
        modified: Option<std::time::SystemTime>,
    }

    #[cfg(not(unix))]
    fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
        FileIdentity {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    #[cfg(unix)]
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

    #[cfg(not(unix))]
    fn open_regular_file_nofollow(path: &Path) -> io::Result<File> {
        File::open(path)
    }

    fn set_read_only(path: &Path, label: &str) -> Result<(), IosError> {
        let mut permissions = fs::metadata(path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} snapshot '{}': {error}",
                    path.display()
                ))
            })?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).map_err(|error| {
            IosError::storage(format!(
                "failed to make {label} snapshot read-only '{}': {error}",
                path.display()
            ))
        })
    }

    fn sha256_file(path: &Path, label: &str) -> Result<String, IosError> {
        let mut file = File::open(path).map_err(|error| {
            IosError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    path.display()
                ))
            })?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            IosError::storage(format!(
                "failed to rewind {label} '{}': {error}",
                path.display()
            ))
        })?;
        Ok(hex::encode(hasher.finalize()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use xz2::write::XzEncoder;
        use zip::write::SimpleFileOptions;

        #[test]
        fn plugin_framework_archive_requires_one_registry_fragment_per_slice() {
            let framework = "VesperPluginFixture";
            let nodes = expected_plugin_framework_nodes(
                framework,
                false,
                &[
                    crate::ios_plugin::IosPluginSlice::DeviceArm64,
                    crate::ios_plugin::IosPluginSlice::SimulatorArm64,
                ],
            );

            for slice in ["ios-arm64", "ios-arm64-simulator"] {
                assert_eq!(
                    nodes.get(&format!(
                        "{framework}.xcframework/{slice}/{framework}.framework/vesper-plugin-registry.json"
                    )),
                    Some(&ArchiveNodeKind::File)
                );
            }
        }

        #[test]
        fn combined_release_uses_distinct_runtime_and_plugin_archive_shapes() {
            let runtime = framework_zip_policy("VesperFFmpegAVCodec")
                .expected_nodes
                .expect("runtime archive shape");
            assert!(
                runtime
                    .keys()
                    .all(|path| !path.ends_with("vesper-plugin-registry.json"))
            );

            let plugin = framework_zip_policy("VesperPlayerRemuxFfmpegPlugin")
                .expected_nodes
                .expect("plugin archive shape");
            for slice in ["ios-arm64", "ios-arm64-simulator"] {
                assert_eq!(
                    plugin.get(&format!(
                        "VesperPlayerRemuxFfmpegPlugin.xcframework/{slice}/VesperPlayerRemuxFfmpegPlugin.framework/vesper-plugin-registry.json"
                    )),
                    Some(&ArchiveNodeKind::File)
                );
            }
        }

        #[test]
        fn source_asset_discovery_requires_the_exact_release_lock() {
            let policy = FfmpegSourcePolicy::test_fixture();
            let temporary = tempfile::tempdir().expect("create release fixture");
            let locked = "VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz";
            fs::write(temporary.path().join(locked), b"fixture")
                .expect("write locked source asset");
            let (name, version) = discover_source_asset(temporary.path(), &policy)
                .expect("discover the locked patch release");
            assert_eq!(name, locked);
            assert_eq!(version, Version::new(8, 1, 2));

            fs::remove_file(temporary.path().join(locked)).expect("remove locked source asset");
            let compatible = "VesperPlayerOptionalPlugins-FFmpeg-8.1.3-source.tar.xz";
            fs::write(temporary.path().join(compatible), b"fixture")
                .expect("write compatible source asset");
            let error = discover_source_asset(temporary.path(), &policy)
                .expect_err("reject a compatible but unlocked patch release");
            assert!(error.to_string().contains("locked release version '8.1.2'"));

            fs::remove_file(temporary.path().join(compatible))
                .expect("remove compatible source asset");
            fs::write(
                temporary
                    .path()
                    .join("VesperPlayerOptionalPlugins-FFmpeg-8.2.0-source.tar.xz"),
                b"fixture",
            )
            .expect("write incompatible source asset");
            let error = discover_source_asset(temporary.path(), &policy)
                .expect_err("reject a version outside the compatibility policy");
            assert!(error.to_string().contains(">=8.1.0, <8.2.0"));
        }

        #[test]
        fn plugin_registry_fragment_validation_rejects_malformed_swapped_and_tampered_content() {
            fn fragment(target: &str, minimum_os: &str, framework: &str) -> Vec<u8> {
                serde_json::to_vec(&serde_json::json!({
                    "schema_version": 1,
                    "target": target,
                    "architecture": "arm64",
                    "minimum_os": minimum_os,
                    "artifacts": [{
                        "plugin_id": "dev.vesper.fixture",
                        "transport": "native",
                        "locator": {
                            "kind": "apple-framework",
                            "name": framework,
                            "bundle_identifier": "dev.vesper.fixture"
                        },
                        "integrity": {
                            "kind": "apple-code-signature",
                            "validation": "same-team-as-host-or-simulator-ad-hoc"
                        },
                        "package": {
                            "version": "1.0.0",
                            "publisher": "dev.vesper.publisher",
                            "descriptor_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        },
                        "capabilities": [{
                            "interface_id": "e9479dbc-42d2-575e-b39e-a24bc512fbc7",
                            "instance_id": "dev.vesper.fixture.post-download",
                            "interface_major": 1,
                            "interface_minor": 0
                        }]
                    }]
                }))
                .expect("serialize registry fixture")
            }

            let target = "aarch64-apple-ios";
            let expected = fragment(target, "17.0", "FixturePlugin");
            validate_plugin_registry_fragment(&expected, &expected, target, "fixture")
                .expect("canonical fragment");

            let malformed =
                validate_plugin_registry_fragment(b"{", &expected, target, "malformed fixture")
                    .expect_err("malformed fragment must fail");
            assert!(malformed.to_string().contains("invalid"));

            let swapped = fragment("aarch64-apple-ios-sim", "17.0", "FixturePlugin");
            let swapped =
                validate_plugin_registry_fragment(&swapped, &expected, target, "swapped fixture")
                    .expect_err("simulator fragment in device slice must fail");
            assert!(swapped.to_string().contains("target"));

            for tampered in [
                fragment(target, "17.0", "OtherPlugin"),
                fragment(target, "18.0", "FixturePlugin"),
            ] {
                let error = validate_plugin_registry_fragment(
                    &tampered,
                    &expected,
                    target,
                    "tampered fixture",
                )
                .expect_err("canonical identity drift must fail");
                assert!(error.to_string().contains("canonical descriptor"));
            }
        }

        #[test]
        fn profile_record_is_exact_and_has_canonical_grammar() {
            for value in [b"legacy\n".as_slice(), b"custom-e1eabc3db7bc\r\n"] {
                let parsed = parse_profile_record(value, "fixture").expect("parse profile");
                validate_profile_grammar(parsed, "fixture").expect("validate profile");
            }
            for value in [
                b" custom-e1eabc3db7bc\n".as_slice(),
                b"custom-e1eabc3db7bc\nextra\n",
                b"custom-E1EABC3DB7BC\n",
                b"custom-e1eabc3db7b\n",
            ] {
                let result = parse_profile_record(value, "fixture")
                    .and_then(|parsed| validate_profile_grammar(parsed, "fixture"));
                assert!(result.is_err(), "unexpectedly accepted {value:?}");
            }
        }

        #[test]
        fn archive_paths_reject_ambiguous_representations() {
            for path in [
                "../escape",
                "/absolute",
                "C:/prefix",
                "a\\b",
                "a//b",
                "a/./b",
                "a/../b",
                "control\0byte",
            ] {
                assert!(
                    validate_archive_path(path, ArchiveNodeKind::File, "fixture").is_err(),
                    "unexpectedly accepted {path:?}"
                );
            }
            assert_eq!(
                validate_archive_path("Root/valid file", ArchiveNodeKind::File, "fixture")
                    .expect("valid path"),
                "Root/valid file"
            );
        }

        #[test]
        fn archive_nodes_reject_unicode_casefold_collisions() {
            let mut nodes = BTreeMap::new();
            insert_archive_node(
                &mut nodes,
                "Root/Cafe\u{301}.txt",
                ArchiveNodeKind::File,
                "fixture",
            )
            .expect("insert first path");
            assert!(
                insert_archive_node(
                    &mut nodes,
                    "root/CAFÉ.txt",
                    ArchiveNodeKind::File,
                    "fixture",
                )
                .is_err()
            );
        }

        #[test]
        fn metadata_parser_rejects_duplicate_unknown_and_lossy_records() {
            let keys = ["first", "second"];
            assert!(
                parse_metadata_record(b"first=one\nsecond=two\n", None, &keys, "fixture").is_ok()
            );
            for bytes in [
                b"first=one\nfirst=two\nsecond=three\n".as_slice(),
                b"first=one\nunknown=two\nsecond=three\n",
                b"first=one\r\nsecond=two\r\n",
                b"first=one\n\nsecond=two\n",
            ] {
                assert!(
                    parse_metadata_record(bytes, None, &keys, "fixture").is_err(),
                    "unexpectedly accepted {bytes:?}"
                );
            }
        }

        #[test]
        fn zip_preflight_rejects_unpaired_appledouble_and_symlinks() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let expected = BTreeMap::from([
                ("Root".to_owned(), ArchiveNodeKind::Directory),
                ("Root/file".to_owned(), ArchiveNodeKind::File),
            ]);
            let policy = ZipPolicy {
                expected_nodes: Some(expected.clone()),
                required_root: None,
                maximum_entries: 16,
                maximum_entry_bytes: 1024,
                maximum_expanded_bytes: 4096,
            };

            let valid = temporary.path().join("valid.zip");
            write_test_zip(
                &valid,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, b""),
                    ("Root/file", UNIX_REGULAR_FILE | 0o644, b"payload"),
                    ("__MACOSX/", UNIX_DIRECTORY | 0o755, b""),
                    ("__MACOSX/Root/", UNIX_DIRECTORY | 0o755, b""),
                    (
                        "__MACOSX/Root/._file",
                        UNIX_REGULAR_FILE | 0o644,
                        b"sidecar",
                    ),
                ],
            );
            preflight_zip(&valid, &policy, "fixture").expect("valid ZIP");

            let unpaired = temporary.path().join("unpaired.zip");
            write_test_zip(
                &unpaired,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, b""),
                    ("Root/file", UNIX_REGULAR_FILE | 0o644, b"payload"),
                    (
                        "__MACOSX/Root/._missing",
                        UNIX_REGULAR_FILE | 0o644,
                        b"sidecar",
                    ),
                ],
            );
            assert!(preflight_zip(&unpaired, &policy, "fixture").is_err());

            let symlink_source = temporary.path().join("symlink-source.zip");
            let symlink = temporary.path().join("symlink.zip");
            write_test_zip(
                &symlink_source,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, b""),
                    ("Root/file", UNIX_REGULAR_FILE | 0o644, b"target"),
                ],
            );
            rewrite_central_directory_mode(&symlink_source, &symlink, "Root/file", 0o120777);
            assert!(preflight_zip(&symlink, &policy, "fixture").is_err());
        }

        #[test]
        fn zip_preflight_rejects_duplicates_and_compression_bombs() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let expected = BTreeMap::from([
                ("Root".to_owned(), ArchiveNodeKind::Directory),
                ("Root/file".to_owned(), ArchiveNodeKind::File),
            ]);
            let policy = ZipPolicy {
                expected_nodes: Some(expected),
                required_root: None,
                maximum_entries: 16,
                maximum_entry_bytes: 1024 * 1024,
                maximum_expanded_bytes: 1024 * 1024,
            };

            let duplicate_source = temporary.path().join("duplicate-source.zip");
            let duplicate = temporary.path().join("duplicate.zip");
            write_test_zip(
                &duplicate_source,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, b""),
                    ("Root/file", UNIX_REGULAR_FILE | 0o644, b"first"),
                    ("Root/FILE", UNIX_REGULAR_FILE | 0o644, b"second"),
                ],
            );
            rewrite_zip_entry_name(&duplicate_source, &duplicate, "Root/FILE", "Root/file");
            assert!(preflight_zip(&duplicate, &policy, "fixture").is_err());

            let compressed = temporary.path().join("compressed.zip");
            write_compressed_test_zip(&compressed, 256 * 1024);
            assert!(preflight_zip(&compressed, &policy, "fixture").is_err());
        }

        #[test]
        fn sanitized_zip_drops_paired_appledouble_payloads() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let source = temporary.path().join("source.zip");
            let sanitized = temporary.path().join("sanitized.zip");
            write_test_zip(
                &source,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, b""),
                    ("Root/file", UNIX_REGULAR_FILE | 0o644, b"payload"),
                    ("__MACOSX/", UNIX_DIRECTORY | 0o755, b""),
                    ("__MACOSX/Root/", UNIX_DIRECTORY | 0o755, b""),
                    (
                        "__MACOSX/Root/._file",
                        UNIX_REGULAR_FILE | 0o644,
                        b"ignored metadata",
                    ),
                ],
            );
            write_sanitized_zip(&source, &sanitized, &generic_zip_policy(), "fixture")
                .expect("sanitize ZIP");

            let file = File::open(&sanitized).expect("open sanitized ZIP");
            let mut archive = ZipArchive::new(file).expect("read sanitized ZIP");
            let names = (0..archive.len())
                .map(|index| {
                    archive
                        .by_index(index)
                        .expect("read ZIP entry")
                        .name()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(names, ["Root/", "Root/file"]);
        }

        #[test]
        fn binary_checksum_record_is_verified_against_payload() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let valid = temporary.path().join("valid-checksum.zip");
            let payload = b"framework binary";
            let checksum = format!("{}\n", hex::encode(Sha256::digest(payload)));
            write_test_zip_owned(
                &valid,
                &[
                    ("binary", UNIX_REGULAR_FILE | 0o755, payload.to_vec()),
                    (
                        "binary-sha256.txt",
                        UNIX_REGULAR_FILE | 0o644,
                        checksum.into_bytes(),
                    ),
                ],
                CompressionMethod::Stored,
            );
            verify_zip_binary_checksum(&valid, "binary", "binary-sha256.txt")
                .expect("valid checksum");

            let invalid = temporary.path().join("invalid-checksum.zip");
            write_test_zip_owned(
                &invalid,
                &[
                    ("binary", UNIX_REGULAR_FILE | 0o755, payload.to_vec()),
                    (
                        "binary-sha256.txt",
                        UNIX_REGULAR_FILE | 0o644,
                        format!("{}\n", "0".repeat(64)).into_bytes(),
                    ),
                ],
                CompressionMethod::Stored,
            );
            assert!(verify_zip_binary_checksum(&invalid, "binary", "binary-sha256.txt").is_err());
        }

        #[test]
        fn source_tar_preflight_rejects_links_and_accepts_regular_tree() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let version = Version::new(8, 1, 3);
            let valid = temporary.path().join("valid.tar.xz");
            write_test_source_tar(&valid, false, &version);
            preflight_source_tar(&valid, &version).expect("valid source tar");

            let linked = temporary.path().join("linked.tar.xz");
            write_test_source_tar(&linked, true, &version);
            assert!(preflight_source_tar(&linked, &version).is_err());
        }

        #[test]
        fn forged_zip_size_cannot_expand_beyond_declared_budget() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let source = temporary.path().join("source.zip");
            let forged = temporary.path().join("forged.zip");
            let sanitized = temporary.path().join("sanitized.zip");
            write_compressed_test_zip(&source, 256 * 1024);
            rewrite_zip_uncompressed_size(&source, &forged, "Root/file", 1);
            let policy = ZipPolicy {
                expected_nodes: Some(BTreeMap::from([
                    ("Root".to_owned(), ArchiveNodeKind::Directory),
                    ("Root/file".to_owned(), ArchiveNodeKind::File),
                ])),
                required_root: None,
                maximum_entries: 16,
                maximum_entry_bytes: 1024 * 1024,
                maximum_expanded_bytes: 1024 * 1024,
            };
            let error = preflight_zip(&forged, &policy, "fixture")
                .expect_err("forged size must fail during preflight payload validation");
            assert!(error.to_string().contains("declared 1 bytes"));

            let error = write_sanitized_zip(&forged, &sanitized, &policy, "fixture")
                .expect_err("forged size must fail during sanitized copy validation");
            assert!(error.to_string().contains("declared 1 bytes"));

            let mut output = Vec::new();
            let error =
                copy_declared_bytes(&mut io::repeat(7), &mut output, 1, 1024, "infinite fixture")
                    .expect_err("reader must stop after the one-byte probe");
            assert_eq!(output.len(), 1);
            assert!(error.to_string().contains("declared 1 bytes"));
        }

        #[test]
        fn zip_preflight_rejects_corrupt_deflated_payloads() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let valid = temporary.path().join("valid.zip");
            let corrupt = temporary.path().join("corrupt.zip");
            write_compressed_test_zip(&valid, 4 * 1024);
            let policy = ZipPolicy {
                expected_nodes: Some(BTreeMap::from([
                    ("Root".to_owned(), ArchiveNodeKind::Directory),
                    ("Root/file".to_owned(), ArchiveNodeKind::File),
                ])),
                required_root: None,
                maximum_entries: 16,
                maximum_entry_bytes: 1024 * 1024,
                maximum_expanded_bytes: 1024 * 1024,
            };
            preflight_zip(&valid, &policy, "fixture").expect("valid deflated payload");
            corrupt_zip_payload(&valid, &corrupt, "Root/file");
            let error = preflight_zip(&corrupt, &policy, "fixture")
                .expect_err("corrupt deflated payload must fail preflight");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);
        }

        #[test]
        fn compliance_external_dependencies_match_framework_metadata() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let archive = temporary.path().join("compliance.zip");
            let release_source = ReleaseSource::new(
                Version::new(8, 1, 3),
                "VesperPlayerOptionalPlugins-FFmpeg-8.1.3-source.tar.xz".to_owned(),
                "a".repeat(64),
            );
            let source_record = format!(
                "component=FFmpeg\nffmpeg_version={}\nlicense_mode=LGPL-2.1-or-later\nlinkage=dynamic-frameworks\ndeclared_profile=source-normalizer\nprofile_hash=custom-e1eabc3db7bc\nsource_url={}\nsource_asset={}\nsource_sha256={}\nlocal_changes=none\nexternal_dependencies=none\n",
                release_source.version_text,
                release_source.source_url,
                release_source.source_asset,
                release_source.source_sha256,
            );
            write_test_zip_owned(
                &archive,
                &[(
                    "VesperPlayerOptionalPlugins-FFmpeg-Compliance/SOURCE.txt",
                    UNIX_REGULAR_FILE | 0o644,
                    source_record.into_bytes(),
                )],
                CompressionMethod::Stored,
            );
            let matching_evidence = FrameworkEvidence {
                profile_hash: "custom-e1eabc3db7bc".to_owned(),
                declared_profile: "source-normalizer".to_owned(),
                external_dependencies: "none".to_owned(),
                source_archive: "ffmpeg-8.1.3.tar.xz".to_owned(),
                device_metadata: Vec::new(),
                simulator_metadata: Vec::new(),
                device_input_fingerprint: String::new(),
                simulator_input_fingerprint: String::new(),
            };
            verify_compliance_source_record(&archive, &matching_evidence, &release_source)
                .expect("a consistent compatible FFmpeg 8.1.3 record must pass");

            let drifted_evidence = FrameworkEvidence {
                external_dependencies: "libxml2".to_owned(),
                ..matching_evidence
            };
            let error =
                verify_compliance_source_record(&archive, &drifted_evidence, &release_source)
                    .expect_err("dependency drift must fail");
            assert!(error.to_string().contains("external_dependencies"));
        }

        #[test]
        fn opened_release_assets_remain_anchored_after_path_replacement() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let release = temporary.path().join("release");
            let moved = temporary.path().join("moved-release");
            let source_asset = "VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz";
            fs::create_dir(&release).expect("create release fixture");
            write_required_release_files(&release, b"original", source_asset);
            let mut assets =
                collect_release_assets(&release, source_asset).expect("open release assets");

            fs::rename(&release, &moved).expect("move opened release directory");
            fs::create_dir(&release).expect("create replacement release directory");
            write_required_release_files(&release, b"replacement", source_asset);

            let destination = temporary.path().join("snapshot");
            let asset = assets
                .files
                .get_mut(COMPLIANCE_ASSET)
                .expect("opened compliance asset");
            copy_opened_file_snapshot(asset, &destination, "fixture").expect("copy anchored asset");
            assert_eq!(fs::read(destination).expect("read snapshot"), b"original");
        }

        fn write_test_zip(path: &Path, entries: &[(&str, u32, &[u8])]) {
            let file = File::create(path).expect("create ZIP fixture");
            let mut writer = ZipWriter::new(file);
            for (name, mode, bytes) in entries {
                let options = SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Stored)
                    .unix_permissions(*mode);
                if mode & UNIX_FILE_TYPE_MASK == UNIX_DIRECTORY {
                    writer
                        .add_directory(*name, options)
                        .expect("write ZIP directory");
                } else {
                    writer.start_file(*name, options).expect("write ZIP file");
                    writer.write_all(bytes).expect("write ZIP payload");
                }
            }
            writer.finish().expect("finish ZIP fixture");
        }

        fn write_compressed_test_zip(path: &Path, payload_bytes: usize) {
            write_test_zip_owned(
                path,
                &[
                    ("Root/", UNIX_DIRECTORY | 0o755, Vec::new()),
                    (
                        "Root/file",
                        UNIX_REGULAR_FILE | 0o644,
                        vec![0_u8; payload_bytes],
                    ),
                ],
                CompressionMethod::Deflated,
            );
        }

        fn write_test_zip_owned(
            path: &Path,
            entries: &[(&str, u32, Vec<u8>)],
            compression: CompressionMethod,
        ) {
            let file = File::create(path).expect("create ZIP fixture");
            let mut writer = ZipWriter::new(file);
            for (name, mode, bytes) in entries {
                let options = SimpleFileOptions::default()
                    .compression_method(compression)
                    .unix_permissions(*mode);
                if mode & UNIX_FILE_TYPE_MASK == UNIX_DIRECTORY {
                    writer
                        .add_directory(*name, options)
                        .expect("write ZIP directory");
                } else {
                    writer.start_file(*name, options).expect("write ZIP file");
                    writer.write_all(bytes).expect("write ZIP payload");
                }
            }
            writer.finish().expect("finish ZIP fixture");
        }

        fn write_test_source_tar(path: &Path, include_link: bool, version: &Version) {
            let output = File::create(path).expect("create source tar fixture");
            let encoder = XzEncoder::new(output, 6);
            let mut builder = tar::Builder::new(encoder);
            let root = format!("ffmpeg-{version}");

            let mut directory = tar::Header::new_gnu();
            directory.set_entry_type(tar::EntryType::Directory);
            directory.set_mode(0o755);
            directory.set_size(0);
            directory.set_cksum();
            builder
                .append_data(&mut directory, format!("{root}/"), io::empty())
                .expect("write tar directory");

            let payload = b"license";
            let mut file = tar::Header::new_gnu();
            file.set_entry_type(tar::EntryType::Regular);
            file.set_mode(0o644);
            file.set_size(payload.len() as u64);
            file.set_cksum();
            builder
                .append_data(&mut file, format!("{root}/LICENSE.md"), payload.as_slice())
                .expect("write tar file");

            if include_link {
                let mut link = tar::Header::new_gnu();
                link.set_entry_type(tar::EntryType::Link);
                link.set_mode(0o644);
                link.set_size(0);
                link.set_link_name(format!("{root}/LICENSE.md"))
                    .expect("set tar link target");
                link.set_cksum();
                builder
                    .append_data(&mut link, format!("{root}/LICENSE-LINK.md"), io::empty())
                    .expect("write tar link");
            }

            builder.finish().expect("finish tar fixture");
            let encoder = builder.into_inner().expect("recover xz encoder");
            encoder.finish().expect("finish xz fixture");
        }

        fn rewrite_central_directory_mode(
            source: &Path,
            output: &Path,
            target_path: &str,
            unix_mode: u32,
        ) {
            const CENTRAL_DIRECTORY_HEADER: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

            let mut bytes = fs::read(source).expect("read ZIP fixture");
            let mut cursor = 0_usize;
            let mut found = false;
            while cursor + 46 <= bytes.len() {
                if bytes[cursor..cursor + 4] != CENTRAL_DIRECTORY_HEADER {
                    cursor += 1;
                    continue;
                }
                let name_length =
                    u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;
                let extra_length =
                    u16::from_le_bytes([bytes[cursor + 30], bytes[cursor + 31]]) as usize;
                let comment_length =
                    u16::from_le_bytes([bytes[cursor + 32], bytes[cursor + 33]]) as usize;
                let entry_length = 46 + name_length + extra_length + comment_length;
                assert!(cursor + entry_length <= bytes.len());
                if bytes[cursor + 46..cursor + 46 + name_length] == *target_path.as_bytes() {
                    bytes[cursor + 5] = 3;
                    bytes[cursor + 38..cursor + 42]
                        .copy_from_slice(&(unix_mode << 16).to_le_bytes());
                    found = true;
                    break;
                }
                cursor += entry_length;
            }
            assert!(found, "central directory entry must exist");
            fs::write(output, bytes).expect("write ZIP fixture with rewritten mode");
        }

        fn rewrite_zip_entry_name(
            source: &Path,
            output: &Path,
            target_path: &str,
            replacement_path: &str,
        ) {
            assert_eq!(target_path.len(), replacement_path.len());
            let mut bytes = fs::read(source).expect("read ZIP fixture");
            let mut replacements = 0_usize;
            for cursor in 0..=bytes.len().saturating_sub(target_path.len()) {
                if bytes[cursor..cursor + target_path.len()] == *target_path.as_bytes() {
                    bytes[cursor..cursor + target_path.len()]
                        .copy_from_slice(replacement_path.as_bytes());
                    replacements += 1;
                }
            }
            assert_eq!(
                replacements, 2,
                "local and central directory names must exist"
            );
            fs::write(output, bytes).expect("write ZIP fixture with rewritten names");
        }

        fn rewrite_zip_uncompressed_size(
            source: &Path,
            output: &Path,
            target_path: &str,
            replacement_size: u32,
        ) {
            const LOCAL_HEADER: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

            let mut bytes = fs::read(source).expect("read ZIP fixture");
            let mut local_rewritten = false;
            let mut central_rewritten = false;
            let mut cursor = 0_usize;
            while cursor + 30 <= bytes.len() {
                if bytes[cursor..cursor + 4] == LOCAL_HEADER {
                    let name_length =
                        u16::from_le_bytes([bytes[cursor + 26], bytes[cursor + 27]]) as usize;
                    let name_start = cursor + 30;
                    if name_start + name_length <= bytes.len()
                        && bytes[name_start..name_start + name_length] == *target_path.as_bytes()
                    {
                        bytes[cursor + 22..cursor + 26]
                            .copy_from_slice(&replacement_size.to_le_bytes());
                        local_rewritten = true;
                    }
                } else if bytes[cursor..cursor + 4] == ZIP_CENTRAL_DIRECTORY_HEADER {
                    let name_length =
                        u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;
                    let name_start = cursor + 46;
                    if name_start + name_length <= bytes.len()
                        && bytes[name_start..name_start + name_length] == *target_path.as_bytes()
                    {
                        bytes[cursor + 24..cursor + 28]
                            .copy_from_slice(&replacement_size.to_le_bytes());
                        central_rewritten = true;
                    }
                }
                cursor += 1;
            }
            assert!(local_rewritten, "local ZIP entry must exist");
            assert!(central_rewritten, "central ZIP entry must exist");
            fs::write(output, bytes).expect("write ZIP fixture with forged size");
        }

        fn corrupt_zip_payload(source: &Path, output: &Path, target_path: &str) {
            const LOCAL_HEADER: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

            let mut bytes = fs::read(source).expect("read ZIP fixture");
            let mut cursor = 0_usize;
            let mut found = false;
            while cursor + 30 <= bytes.len() {
                if bytes[cursor..cursor + 4] != LOCAL_HEADER {
                    cursor += 1;
                    continue;
                }
                let compressed_size = u32::from_le_bytes([
                    bytes[cursor + 18],
                    bytes[cursor + 19],
                    bytes[cursor + 20],
                    bytes[cursor + 21],
                ]) as usize;
                let name_length =
                    u16::from_le_bytes([bytes[cursor + 26], bytes[cursor + 27]]) as usize;
                let extra_length =
                    u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;
                let name_start = cursor + 30;
                let payload_start = name_start + name_length + extra_length;
                let payload_end = payload_start + compressed_size;
                assert!(payload_end <= bytes.len());
                if bytes[name_start..name_start + name_length] == *target_path.as_bytes() {
                    assert!(compressed_size > 0, "target payload must be compressed");
                    bytes[payload_start + compressed_size / 2] ^= 0x80;
                    found = true;
                    break;
                }
                cursor = payload_end;
            }
            assert!(found, "ZIP payload entry must exist");
            fs::write(output, bytes).expect("write ZIP fixture with corrupt payload");
        }

        fn write_required_release_files(directory: &Path, payload: &[u8], source_asset: &str) {
            for name in required_optional_assets(source_asset) {
                fs::write(directory.join(name), payload).expect("write release fixture asset");
            }
        }
    }
}
