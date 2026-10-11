use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use semver::Version;
use zip::ZipArchive;

use crate::external_process;
use crate::ffmpeg::{
    AndroidArtifact, FfmpegError, FfmpegPlatform, FfmpegRequest, NativeFfmpegProfile,
};
use crate::ffmpeg_source::{
    FfmpegBuildSource, latest_package_series_archive_version,
    latest_package_series_archive_version_from_index, select_latest_series_version,
};
use crate::source_archive::{
    self, SourceArchiveErrorKind, SourceArchiveFormat, SourceArchivePolicy,
};
use crate::{android, gradle};

const ANDROID_RUST_TARGET: &str = "aarch64-linux-android";
const DEFAULT_ANDROID_API_LEVEL: u32 = 26;
const DEFAULT_OPENSSL_SERIES: &str = "3.5";
const DEFAULT_LIBXML2_SERIES: &str = "2.15";
const KNOWN_OPENSSL_VERSION: &str = "3.5.9";
const KNOWN_LIBXML2_VERSION: &str = "2.15.4";
const KNOWN_OPENSSL_SHA256: &str =
    "603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a";
const KNOWN_LIBXML2_SHA256: &str =
    "98087fd181d9070724f3fbc65c7377db03038eb92bd882374daff44940138821";
const DEPENDENCY_METADATA_FILE: &str = "vesper-dependency-build-metadata.txt";
const MAX_SOURCE_CACHE_ENTRIES: usize = 10_000;
const MAX_RELEASE_INDEX_BYTES: usize = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_RUNTIME_ENTRIES: usize = 128;
const MAX_RUNTIME_AAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RUNTIME_AAR_ENTRIES: usize = 4096;

const SOURCE_POLICY: SourceArchivePolicy = SourceArchivePolicy {
    maximum_archive_bytes: 1024 * 1024 * 1024,
    maximum_entries: 100_000,
    maximum_expanded_bytes: 8 * 1024 * 1024 * 1024,
    maximum_path_bytes: 4096,
    maximum_path_depth: 64,
};

pub(crate) fn run(
    root: &Path,
    request: &FfmpegRequest,
    profile: &NativeFfmpegProfile,
    source: &FfmpegBuildSource,
) -> Result<(), FfmpegError> {
    let requested_abis = crate::ffmpeg::flatten_list_values(&request.android_abis);
    let abis = android::resolve_selected_abis(&requested_abis).map_err(map_android_error)?;
    android::require_rust_target(ANDROID_RUST_TARGET).map_err(map_android_error)?;
    let sdk_root = android::android_sdk_root().map_err(map_android_error)?;
    let ndk_version = android::android_ndk_version();
    let ndk_root = android::resolve_ndk_root(&sdk_root, &ndk_version).map_err(map_android_error)?;
    let toolchain = android_toolchain(&ndk_root)?;
    let api_level = android_api_level()?;
    let output = crate::ffmpeg::android_output_directory(root, request, &profile.profile_hash);
    let ffmpeg_archive = ensure_ffmpeg_source(root, source)?;
    let source_sha256 = source_archive::sha256_file(
        &ffmpeg_archive,
        SOURCE_POLICY.maximum_archive_bytes,
        "Android FFmpeg source archive",
    )
    .map_err(map_source_error)?;
    let openssl_output = environment_output_directory(
        root,
        "VESPER_ANDROID_OPENSSL_OUTPUT_DIR",
        "third_party/openssl/android",
    );
    let libxml2_output = environment_output_directory(
        root,
        "VESPER_ANDROID_LIBXML2_OUTPUT_DIR",
        "third_party/libxml2/android",
    );
    let jobs = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);

    if profile.tls_backend == "openssl" {
        let version = resolve_openssl_version(root)?;
        let dependency = AndroidDependencySource::openssl(root, &version)?;
        for abi in &abis {
            ensure_openssl(
                root,
                abi,
                &dependency,
                api_level,
                &toolchain,
                &openssl_output,
                jobs,
            )?;
        }
    }
    if profile.enable_dash {
        let version = libxml2_version(root)?;
        let dependency = AndroidDependencySource::libxml2(root, &version)?;
        for abi in &abis {
            ensure_libxml2(
                root,
                abi,
                &dependency,
                api_level,
                &toolchain,
                &libxml2_output,
                jobs,
            )?;
        }
    }

    let mut pending = Vec::new();
    for abi in &abis {
        let plan = AndroidAbiPlan::new(
            abi,
            api_level,
            &toolchain,
            &output,
            &openssl_output,
            &libxml2_output,
            profile,
            source,
            &ffmpeg_archive,
            &source_sha256,
        )?;
        if !profile.force && plan.is_current(&profile.libraries)? {
            println!("Android FFmpeg prebuilt for {abi} is up to date for profile custom.");
        } else {
            pending.push(plan);
        }
    }

    if !pending.is_empty() {
        let source_stage = tempfile::Builder::new()
            .prefix("vesper-android-ffmpeg-source-")
            .tempdir()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Android FFmpeg source staging: {error}"
                ))
            })?;
        let source_root = source_archive::extract_single_root(
            &ffmpeg_archive,
            source_stage.path(),
            &format!("ffmpeg-{}", source.version),
            SourceArchiveFormat::TarXz,
            SOURCE_POLICY,
            "Android FFmpeg source archive",
        )
        .map_err(map_source_error)?;
        require_regular_file(&source_root.join("configure"), "FFmpeg configure script")?;
        for plan in pending {
            build_ffmpeg_abi(&source_root, &plan, profile, jobs)?;
        }
    }

    if request.android_artifact == AndroidArtifact::RuntimeAar {
        build_runtime_aar(
            root,
            &output,
            &openssl_output,
            &libxml2_output,
            &abis,
            profile,
        )?;
    }

    println!();
    println!("Built Android FFmpeg prebuilts into:");
    println!("  {}", output.display());
    println!("Using FFmpeg source archive:");
    println!("  {}", ffmpeg_archive.display());
    println!("FFmpeg profile:");
    println!("  {}", profile.declared_profile);
    Ok(())
}

struct AndroidToolchain {
    root: PathBuf,
    bin: PathBuf,
    sysroot: PathBuf,
}

struct AndroidAbiPlan {
    abi: String,
    install: PathBuf,
    build: PathBuf,
    configure_line: Vec<String>,
    metadata: String,
    pkg_config_paths: Vec<PathBuf>,
}

impl AndroidAbiPlan {
    #[allow(clippy::too_many_arguments)]
    fn new(
        abi: &str,
        api_level: u32,
        toolchain: &AndroidToolchain,
        output: &Path,
        openssl_output: &Path,
        libxml2_output: &Path,
        profile: &NativeFfmpegProfile,
        source: &FfmpegBuildSource,
        source_archive: &Path,
        source_sha256: &str,
    ) -> Result<Self, FfmpegError> {
        if abi != "arm64-v8a" {
            return Err(FfmpegError::compatibility(format!(
                "unsupported Android FFmpeg ABI: {abi}"
            )));
        }
        let install = output.join(abi);
        let build = tempfile::Builder::new()
            .prefix("vesper-android-ffmpeg-build-")
            .tempdir()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Android FFmpeg build staging: {error}"
                ))
            })?
            .keep();
        let target = "aarch64-linux-android";
        let cc = toolchain.bin.join(format!("{target}{api_level}-clang"));
        let cxx = toolchain.bin.join(format!("{target}{api_level}-clang++"));
        for (path, label) in [
            (&cc, "Android clang"),
            (&cxx, "Android clang++"),
            (&toolchain.bin.join("llvm-ar"), "Android llvm-ar"),
            (&toolchain.bin.join("llvm-nm"), "Android llvm-nm"),
            (&toolchain.bin.join("llvm-ranlib"), "Android llvm-ranlib"),
            (&toolchain.bin.join("llvm-strip"), "Android llvm-strip"),
        ] {
            require_toolchain_executable(path, &toolchain.bin, label)?;
        }
        let mut pkg_config_paths = Vec::new();
        let mut cflags = vec!["-fPIC".to_owned()];
        let mut ldflags = vec!["-Wl,-z,max-page-size=16384".to_owned()];
        if profile.tls_backend == "openssl" {
            let dependency = openssl_output.join(abi);
            pkg_config_paths.push(dependency.join("lib/pkgconfig"));
            cflags.push(format!("-I{}", dependency.join("include").display()));
            ldflags.push(format!("-L{}", dependency.join("lib").display()));
        }
        if profile.enable_dash {
            let dependency = libxml2_output.join(abi);
            pkg_config_paths.push(dependency.join("lib/pkgconfig"));
            cflags.push(format!("-I{}", dependency.join("include").display()));
            ldflags.push(format!("-L{}", dependency.join("lib").display()));
        }
        let mut configure_line = vec![
            "./configure".to_owned(),
            format!("--prefix={}", install.display()),
            "--target-os=android".to_owned(),
            "--arch=aarch64".to_owned(),
            "--cpu=armv8-a".to_owned(),
            format!("--sysroot={}", toolchain.sysroot.display()),
            format!("--cc={}", cc.display()),
            format!("--cxx={}", cxx.display()),
            format!("--ld={}", cc.display()),
            format!("--ar={}", toolchain.bin.join("llvm-ar").display()),
            format!("--nm={}", toolchain.bin.join("llvm-nm").display()),
            format!("--ranlib={}", toolchain.bin.join("llvm-ranlib").display()),
            format!("--strip={}", toolchain.bin.join("llvm-strip").display()),
            format!("--as={}", cc.display()),
            "--enable-cross-compile".to_owned(),
            "--disable-programs".to_owned(),
            "--disable-doc".to_owned(),
            "--disable-debug".to_owned(),
            "--disable-static".to_owned(),
            "--enable-shared".to_owned(),
            "--disable-x86asm".to_owned(),
            format!("--extra-cflags={}", cflags.join(" ")),
            format!("--extra-ldflags={}", ldflags.join(" ")),
        ];
        configure_line.extend(profile.configure_arguments(FfmpegPlatform::Android));
        let mut metadata = profile.metadata_text(
            "android",
            abi,
            &source.version,
            source_archive,
            &source.source_url,
            source_sha256,
            &configure_line,
        );
        if profile.tls_backend == "openssl" {
            append_dependency_metadata(&mut metadata, &openssl_output.join(abi), "openssl")?;
        }
        if profile.enable_dash {
            append_dependency_metadata(&mut metadata, &libxml2_output.join(abi), "libxml2")?;
        }
        Ok(Self {
            abi: abi.to_owned(),
            install,
            build,
            configure_line,
            metadata,
            pkg_config_paths,
        })
    }

    fn is_current(&self, libraries: &[String]) -> Result<bool, FfmpegError> {
        let metadata = self.install.join("vesper-ffmpeg-build-metadata.txt");
        if !regular_file_equals(&metadata, self.metadata.as_bytes())? {
            return Ok(false);
        }
        for library in libraries {
            for relative in [
                format!("lib/pkgconfig/lib{library}.pc"),
                format!("lib/lib{library}.so"),
            ] {
                if !regular_nonempty_file(&self.install.join(relative))? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

impl Drop for AndroidAbiPlan {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.build);
    }
}

fn build_ffmpeg_abi(
    source: &Path,
    plan: &AndroidAbiPlan,
    profile: &NativeFfmpegProfile,
    jobs: usize,
) -> Result<(), FfmpegError> {
    println!("Building Android FFmpeg prebuilt for {}", plan.abi);
    println!("  profile: custom");
    println!("  output: {}", plan.install.display());
    let configure = source.join("configure");
    let mut command = Command::new(&configure);
    command
        .current_dir(&plan.build)
        .args(&plan.configure_line[1..])
        .env("PKG_CONFIG_ALLOW_CROSS", "1")
        .env(
            "PKG_CONFIG_PATH",
            joined_search_path(&plan.pkg_config_paths, env::var_os("PKG_CONFIG_PATH"))?,
        )
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut command, "Android FFmpeg configure")?;
    run_make(&plan.build, &[format!("-j{jobs}")], "Android FFmpeg build")?;

    let parent = plan.install.parent().ok_or_else(|| {
        FfmpegError::storage(format!(
            "Android FFmpeg output has no parent: {}",
            plan.install.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to create Android FFmpeg output parent '{}': {error}",
            parent.display()
        ))
    })?;
    let destdir = tempfile::Builder::new()
        .prefix(".vesper-android-ffmpeg-install-")
        .tempdir_in(parent)
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create FFmpeg install staging: {error}"))
        })?;
    run_make_with_destdir(
        &plan.build,
        destdir.path(),
        &["install".to_owned()],
        "Android FFmpeg install",
    )?;
    let staged = staged_install_path(destdir.path(), &plan.install)?;
    for removable in [staged.join("bin"), staged.join("share")] {
        match fs::remove_dir_all(&removable) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to remove staged FFmpeg directory '{}': {error}",
                    removable.display()
                )));
            }
        }
    }
    fs::write(
        staged.join("vesper-ffmpeg-build-metadata.txt"),
        &plan.metadata,
    )
    .map_err(|error| {
        FfmpegError::storage(format!("failed to write Android FFmpeg metadata: {error}"))
    })?;
    require_complete_ffmpeg_install(&staged, profile)?;
    publish_directory(&staged, &plan.install, "Android FFmpeg prebuilt")
}

struct AndroidDependencySource {
    name: &'static str,
    version: String,
    source_url: String,
    sha256: Option<String>,
}

impl AndroidDependencySource {
    fn openssl(root: &Path, version: &str) -> Result<Self, FfmpegError> {
        Self::new(
            "openssl",
            version,
            environment_text("VESPER_ANDROID_OPENSSL_SOURCE_URL")?,
            environment_text("VESPER_ANDROID_OPENSSL_SOURCE_SHA256")?
                .or(environment_text("VESPER_OPENSSL_SOURCE_SHA256")?),
        )?
        .record_source_digest(root)
    }

    fn libxml2(root: &Path, version: &str) -> Result<Self, FfmpegError> {
        Self::new(
            "libxml2",
            version,
            environment_text("VESPER_ANDROID_LIBXML2_SOURCE_URL")?,
            environment_text("VESPER_ANDROID_LIBXML2_SOURCE_SHA256")?,
        )?
        .record_source_digest(root)
    }

    fn new(
        name: &'static str,
        version: &str,
        source_url: Option<String>,
        sha256: Option<String>,
    ) -> Result<Self, FfmpegError> {
        validate_exact_version(version, name)?;
        let (url, pinned_sha256) = match name {
            "openssl" => (
                format!(
                    "https://github.com/openssl/openssl/releases/download/openssl-{version}/openssl-{version}.tar.gz"
                ),
                (version == KNOWN_OPENSSL_VERSION).then_some(KNOWN_OPENSSL_SHA256),
            ),
            "libxml2" => {
                let series = version
                    .rsplit_once('.')
                    .map(|(series, _)| series)
                    .unwrap_or(version);
                (
                    format!(
                        "https://download.gnome.org/sources/libxml2/{series}/libxml2-{version}.tar.xz"
                    ),
                    (version == KNOWN_LIBXML2_VERSION).then_some(KNOWN_LIBXML2_SHA256),
                )
            }
            _ => {
                return Err(FfmpegError::conformance(
                    "unknown Android FFmpeg dependency",
                ));
            }
        };
        let sha256 = sha256
            .or_else(|| pinned_sha256.map(str::to_owned))
            .map(|value| value.trim().to_ascii_lowercase());
        if sha256.as_deref().is_some_and(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(FfmpegError::conformance(format!(
                "invalid Android {name} source SHA-256"
            )));
        }
        Ok(Self {
            name,
            version: version.to_owned(),
            source_url: source_url.unwrap_or(url),
            sha256,
        })
    }

    fn ensure_archive(&self, root: &Path) -> Result<PathBuf, FfmpegError> {
        let (variable, extension) = match self.name {
            "openssl" => ("VESPER_ANDROID_OPENSSL_SOURCE_ARCHIVE", ".tar.gz"),
            "libxml2" => ("VESPER_ANDROID_LIBXML2_SOURCE_ARCHIVE", ".tar.xz"),
            _ => {
                return Err(FfmpegError::conformance(
                    "unknown Android FFmpeg dependency",
                ));
            }
        };
        let archive_name = format!("{}-{}{extension}", self.name, self.version);
        let path = configured_archive_path(root, variable, &archive_name);
        source_archive::ensure_cached_archive(
            &path,
            std::slice::from_ref(&self.source_url),
            self.sha256.as_deref(),
            SOURCE_POLICY,
            &format!("Android {} source archive", self.name),
        )
        .map_err(map_source_error)
    }

    fn record_source_digest(mut self, root: &Path) -> Result<Self, FfmpegError> {
        let archive = self.ensure_archive(root)?;
        // Every selected patch records its actual archive digest, including a
        // new upstream patch without a checked-in digest. Changing the source
        // bytes invalidates both the dependency and FFmpeg build receipts.
        self.sha256 = Some(
            source_archive::sha256_file(
                &archive,
                SOURCE_POLICY.maximum_archive_bytes,
                &format!("Android {} source archive", self.name),
            )
            .map_err(map_source_error)?,
        );
        Ok(self)
    }

    fn installed_metadata(
        &self,
        install: &Path,
        abi: &str,
        api_level: u32,
    ) -> Result<Option<String>, FfmpegError> {
        let (pc, header, version_macro, libraries): (&str, &str, &str, &[&str]) = match self.name {
            "openssl" => (
                "lib/pkgconfig/openssl.pc",
                "include/openssl/opensslv.h",
                "# define OPENSSL_VERSION_STR",
                &["lib/libssl.so", "lib/libcrypto.so"],
            ),
            "libxml2" => (
                "lib/pkgconfig/libxml-2.0.pc",
                "include/libxml2/libxml/xmlversion.h",
                "#define LIBXML_DOTTED_VERSION",
                &["lib/libxml2.so"],
            ),
            _ => {
                return Err(FfmpegError::conformance(
                    "unknown Android FFmpeg dependency",
                ));
            }
        };
        if pkg_config_version(&install.join(pc))?.as_deref() != Some(&self.version) {
            return Ok(None);
        }
        if !regular_nonempty_file(&install.join(header))? {
            return Ok(None);
        }
        let header_text = fs::read_to_string(install.join(header)).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to read Android {} version header: {error}",
                self.name
            ))
        })?;
        let expected_version = format!("\"{}\"", self.version);
        if !header_text.lines().any(|line| {
            line.strip_prefix(version_macro)
                .is_some_and(|value| value.trim() == expected_version)
        }) {
            return Ok(None);
        }
        let sha256 = self.sha256.as_deref().ok_or_else(|| {
            FfmpegError::conformance(format!(
                "Android {} source digest was not recorded",
                self.name
            ))
        })?;
        let mut metadata = format!(
            "dependency={}\nversion={}\nsource_url={}\nsource_sha256={sha256}\nabi={abi}\nandroid_api_level={api_level}\n",
            self.name, self.version, self.source_url,
        );
        for relative in [pc, header].into_iter().chain(libraries.iter().copied()) {
            // Native installs may use the standard unversioned shared-library symlink.
            let path = match fs::canonicalize(install.join(relative)) {
                Ok(path) => path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => {
                    return Err(FfmpegError::storage(format!(
                        "failed to resolve Android {} build input {relative}: {error}",
                        self.name,
                    )));
                }
            };
            if !regular_nonempty_file(&path)? {
                return Ok(None);
            }
            let sha256 = source_archive::sha256_file(
                &path,
                SOURCE_POLICY.maximum_archive_bytes,
                "Android dependency build input",
            )
            .map_err(map_source_error)?;
            metadata.push_str(&format!("file_sha256:{relative}={sha256}\n"));
        }
        Ok(Some(metadata))
    }

    fn is_current(&self, install: &Path, abi: &str, api_level: u32) -> Result<bool, FfmpegError> {
        let Some(metadata) = self.installed_metadata(install, abi, api_level)? else {
            return Ok(false);
        };
        regular_file_equals(&install.join(DEPENDENCY_METADATA_FILE), metadata.as_bytes())
    }

    fn write_metadata(&self, install: &Path, abi: &str, api_level: u32) -> Result<(), FfmpegError> {
        let metadata = self
            .installed_metadata(install, abi, api_level)?
            .ok_or_else(|| {
                FfmpegError::conformance(format!(
                    "staged Android {} files do not match requested version {} or are incomplete",
                    self.name, self.version,
                ))
            })?;
        fs::write(install.join(DEPENDENCY_METADATA_FILE), metadata).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to write Android {} build metadata: {error}",
                self.name
            ))
        })
    }
}

fn append_dependency_metadata(
    metadata: &mut String,
    install: &Path,
    name: &str,
) -> Result<(), FfmpegError> {
    let path = install.join(DEPENDENCY_METADATA_FILE);
    require_regular_file(&path, "Android dependency build metadata")?;
    let receipt = fs::read_to_string(&path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to read Android {name} build metadata: {error}"
        ))
    })?;
    for line in receipt.lines() {
        metadata.push_str(&format!("{name}_{line}\n"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ensure_openssl(
    root: &Path,
    abi: &str,
    dependency: &AndroidDependencySource,
    api_level: u32,
    toolchain: &AndroidToolchain,
    output: &Path,
    jobs: usize,
) -> Result<(), FfmpegError> {
    let install = output.join(abi);
    if dependency.is_current(&install, abi, api_level)? {
        return Ok(());
    }
    let version = &dependency.version;
    let archive = dependency.ensure_archive(root)?;
    let extraction = tempfile::Builder::new()
        .prefix("vesper-android-openssl-source-")
        .tempdir()
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create OpenSSL source staging: {error}"))
        })?;
    let source_root = source_archive::extract_single_root(
        &archive,
        extraction.path(),
        &format!("openssl-{version}"),
        SourceArchiveFormat::TarGzip,
        SOURCE_POLICY,
        "Android OpenSSL source archive",
    )
    .map_err(map_source_error)?;
    let target = "aarch64-linux-android";
    let cc = toolchain.bin.join(format!("{target}{api_level}-clang"));
    let cxx = toolchain.bin.join(format!("{target}{api_level}-clang++"));
    let mut configure = Command::new(environment_command("PERL", "perl"));
    configure
        .current_dir(&source_root)
        .arg("./Configure")
        .args(["android-arm64", "shared", "no-tests", "no-unit-test"])
        .arg(format!("-D__ANDROID_API__={api_level}"))
        .arg(format!("--prefix={}", install.display()))
        .arg(format!("--openssldir={}", install.join("ssl").display()))
        .env("ANDROID_NDK_HOME", &toolchain.root)
        .env("ANDROID_NDK_ROOT", &toolchain.root)
        .env(
            "PATH",
            joined_search_path(std::slice::from_ref(&toolchain.bin), env::var_os("PATH"))?,
        )
        .env("CC", &cc)
        .env("CXX", &cxx)
        .env("AR", toolchain.bin.join("llvm-ar"))
        .env("AS", &cc)
        .env("RANLIB", toolchain.bin.join("llvm-ranlib"))
        .env("STRIP", toolchain.bin.join("llvm-strip"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut configure, "Android OpenSSL configure")?;
    run_openssl_make(
        &source_root,
        &[format!("-j{jobs}")],
        None,
        toolchain,
        "Android OpenSSL build",
    )?;
    let parent = install.parent().ok_or_else(|| {
        FfmpegError::storage(format!(
            "OpenSSL output has no parent: {}",
            install.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        FfmpegError::storage(format!("failed to create OpenSSL output parent: {error}"))
    })?;
    let destdir = tempfile::Builder::new()
        .prefix(".vesper-android-openssl-install-")
        .tempdir_in(parent)
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create OpenSSL install staging: {error}"))
        })?;
    run_openssl_make(
        &source_root,
        &["install_sw".to_owned()],
        Some(destdir.path()),
        toolchain,
        "Android OpenSSL install",
    )?;
    let staged = staged_install_path(destdir.path(), &install)?;
    dependency.write_metadata(&staged, abi, api_level)?;
    publish_directory(&staged, &install, "Android OpenSSL prebuilt")
}

#[allow(clippy::too_many_arguments)]
fn ensure_libxml2(
    root: &Path,
    abi: &str,
    dependency: &AndroidDependencySource,
    api_level: u32,
    toolchain: &AndroidToolchain,
    output: &Path,
    jobs: usize,
) -> Result<(), FfmpegError> {
    let install = output.join(abi);
    if dependency.is_current(&install, abi, api_level)? {
        return Ok(());
    }
    let version = &dependency.version;
    let archive = dependency.ensure_archive(root)?;
    let extraction = tempfile::Builder::new()
        .prefix("vesper-android-libxml2-source-")
        .tempdir()
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create libxml2 source staging: {error}"))
        })?;
    let source_root = source_archive::extract_single_root(
        &archive,
        extraction.path(),
        &format!("libxml2-{version}"),
        SourceArchiveFormat::TarXz,
        SOURCE_POLICY,
        "Android libxml2 source archive",
    )
    .map_err(map_source_error)?;
    let build = tempfile::Builder::new()
        .prefix("vesper-android-libxml2-build-")
        .tempdir()
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create libxml2 build staging: {error}"))
        })?;
    let target = "aarch64-linux-android";
    let cc = toolchain.bin.join(format!("{target}{api_level}-clang"));
    let cxx = toolchain.bin.join(format!("{target}{api_level}-clang++"));
    let mut configure = Command::new(source_root.join("configure"));
    configure
        .current_dir(build.path())
        .arg(format!("--host={target}"))
        .arg(format!("--prefix={}", install.display()))
        .args(libxml2_configure_arguments(version)?)
        .env("CC", &cc)
        .env("CXX", &cxx)
        .env("AR", toolchain.bin.join("llvm-ar"))
        .env("RANLIB", toolchain.bin.join("llvm-ranlib"))
        .env("STRIP", toolchain.bin.join("llvm-strip"))
        .env("PKG_CONFIG_ALLOW_CROSS", "1")
        .env(
            "CPPFLAGS",
            format!("-I{}", toolchain.sysroot.join("usr/include").display()),
        )
        .env(
            "LDFLAGS",
            format!("-L{}", toolchain.sysroot.join("usr/lib").display()),
        )
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut configure, "Android libxml2 configure")?;
    run_make(
        build.path(),
        &[format!("-j{jobs}")],
        "Android libxml2 build",
    )?;
    let parent = install.parent().ok_or_else(|| {
        FfmpegError::storage(format!(
            "libxml2 output has no parent: {}",
            install.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        FfmpegError::storage(format!("failed to create libxml2 output parent: {error}"))
    })?;
    let destdir = tempfile::Builder::new()
        .prefix(".vesper-android-libxml2-install-")
        .tempdir_in(parent)
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create libxml2 install staging: {error}"))
        })?;
    run_make_with_destdir(
        build.path(),
        destdir.path(),
        &["install".to_owned()],
        "Android libxml2 install",
    )?;
    let staged = staged_install_path(destdir.path(), &install)?;
    dependency.write_metadata(&staged, abi, api_level)?;
    publish_directory(&staged, &install, "Android libxml2 prebuilt")
}

fn libxml2_configure_arguments(version: &str) -> Result<Vec<&'static str>, FfmpegError> {
    let release = Version::parse(version).map_err(|error| {
        FfmpegError::conformance(format!("invalid libxml2 version '{version}': {error}"))
    })?;
    let mut arguments = vec![
        "--enable-shared",
        "--disable-static",
        "--without-iconv",
        "--without-python",
        "--without-icu",
        "--without-http",
        "--without-legacy",
        "--without-html",
    ];
    // libxml2 2.15 removed LZMA support and its configure option.
    if release < Version::new(2, 15, 0) {
        arguments.push("--without-lzma");
    }
    Ok(arguments)
}

fn build_runtime_aar(
    root: &Path,
    ffmpeg_output: &Path,
    openssl_output: &Path,
    libxml2_output: &Path,
    abis: &[String],
    profile: &NativeFfmpegProfile,
) -> Result<(), FfmpegError> {
    let staging = tempfile::Builder::new()
        .prefix("vesper-android-ffmpeg-runtime-")
        .tempdir()
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create runtime AAR staging: {error}"))
        })?;
    let (jni, assets) = runtime_staging_directories(root, staging.path())?;
    let metadata = assets.join("vesper-ffmpeg-runtime");
    fs::create_dir_all(&metadata).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to create runtime metadata staging: {error}"
        ))
    })?;
    for abi in abis {
        let target = jni.join(abi);
        fs::create_dir_all(&target).map_err(|error| {
            FfmpegError::storage(format!("failed to create runtime ABI staging: {error}"))
        })?;
        copy_runtime_libraries(&ffmpeg_output.join(abi).join("lib"), &target, |_| true)?;
        if profile.tls_backend == "openssl" {
            copy_runtime_libraries(&openssl_output.join(abi).join("lib"), &target, |name| {
                name.starts_with("libssl") || name.starts_with("libcrypto")
            })?;
        }
        if profile.enable_dash {
            copy_runtime_libraries(&libxml2_output.join(abi).join("lib"), &target, |name| {
                name.starts_with("libxml2")
            })?;
        }
        let source_metadata = ffmpeg_output
            .join(abi)
            .join("vesper-ffmpeg-build-metadata.txt");
        fs::copy(
            &source_metadata,
            metadata.join(format!("{abi}-metadata.txt")),
        )
        .map_err(|error| {
            FfmpegError::storage(format!(
                "failed to stage Android FFmpeg runtime metadata '{}': {error}",
                source_metadata.display()
            ))
        })?;
    }
    fs::write(
        metadata.join("profile-hash.txt"),
        format!("{}\n", profile.profile_hash),
    )
    .map_err(|error| {
        FfmpegError::storage(format!("failed to write runtime profile receipt: {error}"))
    })?;

    let project = root.join("lib/android");
    let fallback = root.join("examples/android-compose-host");
    let gradle = gradle::resolve(&project, Some(&fallback)).map_err(map_gradle_error)?;
    let gradle_user_home = gradle::service_home(&project);
    let mut command = Command::new(gradle);
    command
        .current_dir(root)
        .arg("-p")
        .arg(&project)
        .arg(":vesper-player-kit-ffmpeg-runtime:assembleRelease")
        .env("GRADLE_USER_HOME", gradle_user_home)
        .env("VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS", &jni)
        .env("VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS", &assets)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut command, "Android FFmpeg runtime AAR build")?;
    let aar = project.join(
        "vesper-player-kit-ffmpeg-runtime/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar",
    );
    verify_runtime_aar(root, abis, profile)?;
    println!("Built Android FFmpeg runtime AAR:");
    println!("  {}", aar.display());
    Ok(())
}

fn runtime_staging_directories(
    root: &Path,
    fallback: &Path,
) -> Result<(PathBuf, PathBuf), FfmpegError> {
    let jni = env::var_os("VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS")
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(root, Path::new(&value)));
    let assets = env::var_os("VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS")
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(root, Path::new(&value)));
    match (jni, assets) {
        (None, None) => Ok((fallback.join("jniLibs"), fallback.join("assets"))),
        (Some(jni), Some(assets)) => {
            require_empty_staging_directory(&jni, "Android FFmpeg runtime JNI staging")?;
            require_runtime_asset_staging_directory(&assets)?;
            Ok((jni, assets))
        }
        _ => Err(FfmpegError::conformance(
            "VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS and VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS must be set together",
        )),
    }
}

fn require_runtime_asset_staging_directory(path: &Path) -> Result<(), FfmpegError> {
    let label = "Android FFmpeg runtime asset staging";
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfmpegError::conformance(format!(
            "{label} must be an existing regular directory '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(FfmpegError::conformance(format!(
            "{label} must be a regular non-symlink directory: {}",
            path.display()
        )));
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    let Some(entry) = entries.next().transpose().map_err(|error| {
        FfmpegError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?
    else {
        return Ok(());
    };
    if entry.file_name() != OsStr::new("vesper-ffmpeg-runtime")
        || !entry
            .file_type()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    entry.path().display()
                ))
            })?
            .is_dir()
        || entries
            .next()
            .transpose()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to inspect {label} '{}': {error}",
                    path.display()
                ))
            })?
            .is_some()
    {
        return Err(FfmpegError::conformance(format!(
            "{label} may contain only an empty vesper-ffmpeg-runtime directory: {}",
            path.display()
        )));
    }
    require_empty_staging_directory(&entry.path(), "Android FFmpeg runtime metadata staging")
}

fn require_empty_staging_directory(path: &Path, label: &str) -> Result<(), FfmpegError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfmpegError::conformance(format!(
            "{label} must be an existing regular directory '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(FfmpegError::conformance(format!(
            "{label} must be a regular non-symlink directory: {}",
            path.display()
        )));
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|error| {
            FfmpegError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?
        .is_some()
    {
        return Err(FfmpegError::conformance(format!(
            "{label} must be empty before staging: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn verify_runtime_aar(
    root: &Path,
    abis: &[String],
    profile: &NativeFfmpegProfile,
) -> Result<(), FfmpegError> {
    let path = root.join(
        "lib/android/vesper-player-kit-ffmpeg-runtime/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar",
    );
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR is missing '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_RUNTIME_AAR_BYTES
    {
        return Err(FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR must be a bounded regular file: {}",
            path.display()
        )));
    }
    let file = File::open(&path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to open Android FFmpeg runtime AAR '{}': {error}",
            path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        FfmpegError::conformance(format!(
            "invalid Android FFmpeg runtime AAR '{}': {error}",
            path.display()
        ))
    })?;
    if archive.is_empty() || archive.len() > MAX_RUNTIME_AAR_ENTRIES {
        return Err(FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR must contain 1..={MAX_RUNTIME_AAR_ENTRIES} entries"
        )));
    }
    let mut names = std::collections::HashSet::new();
    let mut runtime_names = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            FfmpegError::conformance(format!("invalid runtime AAR entry: {error}"))
        })?;
        let name = entry.name();
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|component| component == "..")
            || !names.insert(name.to_owned())
        {
            return Err(FfmpegError::conformance(format!(
                "Android FFmpeg runtime AAR contains an unsafe or duplicate path: {name:?}"
            )));
        }
        if let Some(relative) = name.strip_prefix("jni/")
            && name.ends_with(".so")
        {
            let mut components = relative.split('/');
            let abi = components.next().unwrap_or_default();
            let library = components.next().unwrap_or_default();
            if components.next().is_some()
                || !abis.iter().any(|expected| expected == abi)
                || library.is_empty()
            {
                return Err(FfmpegError::conformance(format!(
                    "Android FFmpeg runtime AAR contains an unexpected JNI path: {name}"
                )));
            }
            runtime_names.insert((abi.to_owned(), library.to_owned()));
        }
    }
    for abi in abis {
        for library in &profile.libraries {
            let name = format!("lib{library}.so");
            if !runtime_names.contains(&(abi.clone(), name.clone())) {
                return Err(FfmpegError::conformance(format!(
                    "Android FFmpeg runtime AAR is missing jni/{abi}/{name}"
                )));
            }
        }
        if profile.enable_dash && !runtime_names.contains(&(abi.clone(), "libxml2.so".to_owned())) {
            return Err(FfmpegError::conformance(format!(
                "Android FFmpeg runtime AAR is missing jni/{abi}/libxml2.so"
            )));
        }
        if profile.tls_backend == "openssl" {
            for library in ["libssl.so", "libcrypto.so"] {
                if !runtime_names.contains(&(abi.clone(), library.to_owned())) {
                    return Err(FfmpegError::conformance(format!(
                        "Android FFmpeg runtime AAR is missing jni/{abi}/{library}"
                    )));
                }
            }
        }
        let metadata_name = format!("assets/vesper-ffmpeg-runtime/{abi}-metadata.txt");
        let metadata = read_zip_text(&mut archive, &metadata_name, MAX_METADATA_BYTES)?;
        require_metadata_value(
            &metadata,
            "profile_hash",
            &profile.profile_hash,
            &metadata_name,
        )?;
    }
    if profile.forbid_openssl
        && runtime_names
            .iter()
            .any(|(_, name)| name.starts_with("libssl") || name.starts_with("libcrypto"))
    {
        return Err(FfmpegError::conformance(
            "Android FFmpeg runtime AAR contains OpenSSL despite profile policy",
        ));
    }
    let receipt = read_zip_text(
        &mut archive,
        "assets/vesper-ffmpeg-runtime/profile-hash.txt",
        1024,
    )?;
    if receipt.trim() != profile.profile_hash {
        return Err(FfmpegError::conformance(format!(
            "Android FFmpeg runtime profile receipt is '{}', expected '{}'",
            receipt.trim(),
            profile.profile_hash
        )));
    }
    Ok(())
}

fn read_zip_text(
    archive: &mut ZipArchive<File>,
    name: &str,
    maximum_bytes: u64,
) -> Result<String, FfmpegError> {
    let mut entry = archive.by_name(name).map_err(|error| {
        FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR is missing '{name}': {error}"
        ))
    })?;
    if !entry.is_file() || entry.size() > maximum_bytes {
        return Err(FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR entry is not a bounded regular file: {name}"
        )));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    Read::by_ref(&mut entry)
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            FfmpegError::storage(format!(
                "failed to read runtime AAR entry '{name}': {error}"
            ))
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR entry exceeds {maximum_bytes} bytes: {name}"
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        FfmpegError::conformance(format!(
            "Android FFmpeg runtime AAR entry is not UTF-8 '{name}': {error}"
        ))
    })
}

fn require_metadata_value(
    source: &str,
    key: &str,
    expected: &str,
    label: &str,
) -> Result<(), FfmpegError> {
    let values = source
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(candidate, _)| *candidate == key)
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    if values == [expected] {
        Ok(())
    } else {
        Err(FfmpegError::conformance(format!(
            "{label} must contain exactly one {key}={expected} record"
        )))
    }
}

fn copy_runtime_libraries(
    source: &Path,
    target: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<(), FfmpegError> {
    let entries = fs::read_dir(source).map_err(|error| {
        FfmpegError::conformance(format!(
            "missing Android runtime library directory '{}': {error}",
            source.display()
        ))
    })?;
    let mut copied = 0_usize;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RUNTIME_ENTRIES {
            return Err(FfmpegError::conformance(format!(
                "Android runtime library directory contains more than {MAX_RUNTIME_ENTRIES} entries"
            )));
        }
        let entry = entry.map_err(|error| {
            FfmpegError::storage(format!("failed to read runtime library entry: {error}"))
        })?;
        let metadata = entry.metadata().map_err(|error| {
            FfmpegError::storage(format!("failed to inspect runtime library entry: {error}"))
        })?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            return Err(FfmpegError::conformance(
                "Android runtime library name is not UTF-8",
            ));
        };
        if metadata.file_type().is_file()
            && name_text.starts_with("lib")
            && name_text.ends_with(".so")
            && include(name_text)
        {
            fs::copy(entry.path(), target.join(&name)).map_err(|error| {
                FfmpegError::storage(format!("failed to stage runtime library: {error}"))
            })?;
            copied += 1;
        } else if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
            return Err(FfmpegError::conformance(format!(
                "Android runtime directory contains a link or special file: {}",
                entry.path().display()
            )));
        }
    }
    if copied == 0 {
        return Err(FfmpegError::conformance(format!(
            "no Android runtime libraries matched in {}",
            source.display()
        )));
    }
    Ok(())
}

fn android_toolchain(ndk_root: &Path) -> Result<AndroidToolchain, FfmpegError> {
    #[cfg(target_os = "macos")]
    let candidates = ["darwin-arm64", "darwin-x86_64"];
    #[cfg(target_os = "linux")]
    let candidates = ["linux-x86_64", "linux-x86_64"];
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = ndk_root;
        return Err(FfmpegError::compatibility(
            "Android FFmpeg source builds are supported only on macOS and Linux hosts",
        ));
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        for candidate in candidates {
            let root = ndk_root.join("toolchains/llvm/prebuilt").join(candidate);
            let bin = root.join("bin");
            let sysroot = root.join("sysroot");
            if bin.is_dir() && sysroot.is_dir() {
                return Ok(AndroidToolchain {
                    root: ndk_root.to_path_buf(),
                    bin,
                    sysroot,
                });
            }
        }
        Err(FfmpegError::compatibility(format!(
            "Android LLVM toolchain is missing under: {}",
            ndk_root.join("toolchains/llvm/prebuilt").display()
        )))
    }
}

fn android_api_level() -> Result<u32, FfmpegError> {
    let Some(value) = environment_text("VESPER_ANDROID_FFMPEG_ANDROID_API")? else {
        return Ok(DEFAULT_ANDROID_API_LEVEL);
    };
    let parsed = value.parse::<u32>().map_err(|error| {
        FfmpegError::conformance(format!(
            "invalid Android FFmpeg API level '{value}': {error}"
        ))
    })?;
    if parsed < DEFAULT_ANDROID_API_LEVEL {
        return Err(FfmpegError::compatibility(format!(
            "Android FFmpeg API level {parsed} is below the supported floor {DEFAULT_ANDROID_API_LEVEL}"
        )));
    }
    Ok(parsed)
}

fn ensure_ffmpeg_source(root: &Path, source: &FfmpegBuildSource) -> Result<PathBuf, FfmpegError> {
    let path = configured_archive_path(
        root,
        "VESPER_ANDROID_FFMPEG_SOURCE_ARCHIVE",
        &source.archive_name,
    );
    let expected = environment_text("VESPER_ANDROID_FFMPEG_SOURCE_SHA256")?
        .or(environment_text("VESPER_FFMPEG_SOURCE_SHA256")?)
        .or_else(|| source.expected_sha256.clone());
    if expected.is_none() {
        eprintln!(
            "warning: Android FFmpeg source {} has no pinned SHA-256; canonical releases must use the checked-in source lock",
            source.version
        );
    }
    source_archive::ensure_cached_archive(
        &path,
        std::slice::from_ref(&source.source_url),
        expected.as_deref(),
        SOURCE_POLICY,
        "Android FFmpeg source archive",
    )
    .map_err(map_source_error)
}

fn resolve_openssl_version(root: &Path) -> Result<String, FfmpegError> {
    let exact = environment_text("VESPER_ANDROID_OPENSSL_VERSION")?
        .or(environment_text("VESPER_OPENSSL_VERSION")?);
    let series = if exact.is_some() {
        None
    } else {
        environment_text("VESPER_ANDROID_OPENSSL_SERIES")?
            .or(environment_text("VESPER_OPENSSL_SERIES")?)
            .or(environment_text("VESPER_OPENSSL_LTS_SERIES")?)
    };
    resolve_openssl_selection(
        exact.as_deref(),
        series.as_deref(),
        || source_cache_names(&source_cache_directory(root)),
        || {
            source_archive::fetch_bounded_text(
                "https://openssl-library.org/source/",
                MAX_RELEASE_INDEX_BYTES,
                "OpenSSL release index lookup",
            )
            .map_err(map_source_error)
        },
    )
}

fn resolve_openssl_selection(
    exact: Option<&str>,
    series: Option<&str>,
    cache_names: impl FnOnce() -> Result<Vec<String>, FfmpegError>,
    release_index: impl FnOnce() -> Result<String, FfmpegError>,
) -> Result<String, FfmpegError> {
    if let Some(exact) = exact {
        validate_exact_version(exact, "OpenSSL")?;
        return Ok(exact.to_owned());
    }
    resolve_dependency_series_selection(
        "openssl",
        series.unwrap_or(DEFAULT_OPENSSL_SERIES),
        cache_names,
        release_index,
    )
}

fn libxml2_version(root: &Path) -> Result<String, FfmpegError> {
    let exact = environment_text("VESPER_ANDROID_LIBXML2_VERSION")?;
    let series = if exact.is_some() {
        None
    } else {
        environment_text("VESPER_ANDROID_LIBXML2_SERIES")?
    };
    resolve_libxml2_selection(
        exact.as_deref(),
        series.as_deref(),
        || source_cache_names(&source_cache_directory(root)),
        || {
            let series = series.as_deref().unwrap_or(DEFAULT_LIBXML2_SERIES);
            source_archive::fetch_bounded_text(
                &format!("https://download.gnome.org/sources/libxml2/{series}/"),
                MAX_RELEASE_INDEX_BYTES,
                "libxml2 release index lookup",
            )
            .map_err(map_source_error)
        },
    )
}

fn resolve_libxml2_selection(
    exact: Option<&str>,
    series: Option<&str>,
    cache_names: impl FnOnce() -> Result<Vec<String>, FfmpegError>,
    release_index: impl FnOnce() -> Result<String, FfmpegError>,
) -> Result<String, FfmpegError> {
    if let Some(exact) = exact {
        validate_exact_version(exact, "libxml2")?;
        return Ok(exact.to_owned());
    }
    resolve_dependency_series_selection(
        "libxml2",
        series.unwrap_or(DEFAULT_LIBXML2_SERIES),
        cache_names,
        release_index,
    )
}

fn resolve_dependency_series_selection(
    package: &str,
    series: &str,
    cache_names: impl FnOnce() -> Result<Vec<String>, FfmpegError>,
    release_index: impl FnOnce() -> Result<String, FfmpegError>,
) -> Result<String, FfmpegError> {
    let (label, extension) = match package {
        "openssl" => ("OpenSSL", ".tar.gz"),
        "libxml2" => ("libxml2", ".tar.xz"),
        _ => {
            return Err(FfmpegError::conformance(
                "unknown Android FFmpeg dependency",
            ));
        }
    };
    validate_series(series, label)?;
    let cached = latest_package_series_archive_version(package, series, cache_names()?, extension);
    let remote = match release_index() {
        Ok(index) => {
            latest_package_series_archive_version_from_index(package, series, &index, extension)
        }
        Err(error) => {
            if let Some(version) = cached {
                validate_exact_version(&version, label)?;
                eprintln!("warning: {error}; using cached {label} {version}");
                return Ok(version);
            }
            return Err(error);
        }
    };
    let version = select_latest_series_version(cached, remote).ok_or_else(|| {
        FfmpegError::conformance(format!(
            "{label} release index contains no exact release for series {series}; set VESPER_ANDROID_{}_VERSION to an exact release or seed the source cache",
            package.to_ascii_uppercase(),
        ))
    })?;
    validate_exact_version(&version, label)?;
    Ok(version)
}

fn validate_exact_version(value: &str, label: &str) -> Result<(), FfmpegError> {
    let version = Version::parse(value).map_err(|error| {
        FfmpegError::conformance(format!("invalid {label} version '{value}': {error}"))
    })?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(FfmpegError::conformance(format!(
            "{label} version must not contain prerelease or build metadata: {value}"
        )));
    }
    Ok(())
}

fn validate_series(value: &str, label: &str) -> Result<(), FfmpegError> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(FfmpegError::conformance(format!(
            "invalid {label} source series: {value}"
        )))
    }
}

fn source_cache_directory(root: &Path) -> PathBuf {
    env::var_os("VESPER_THIRD_PARTY_SOURCE_CACHE_DIR")
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(root, Path::new(&value)))
        .unwrap_or_else(|| root.join("third_party/_cache"))
}

fn source_cache_names(directory: &Path) -> Result<Vec<String>, FfmpegError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(FfmpegError::storage(format!(
                "failed to enumerate source cache '{}': {error}",
                directory.display()
            )));
        }
    };
    let mut names = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_SOURCE_CACHE_ENTRIES {
            return Err(FfmpegError::conformance(format!(
                "source cache contains more than {MAX_SOURCE_CACHE_ENTRIES} entries"
            )));
        }
        let entry = entry.map_err(|error| {
            FfmpegError::storage(format!("failed to read source cache entry: {error}"))
        })?;
        if entry
            .file_type()
            .map_err(|error| {
                FfmpegError::storage(format!("failed to inspect source cache entry: {error}"))
            })?
            .is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn configured_archive_path(root: &Path, variable: &str, archive_name: &str) -> PathBuf {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(root, Path::new(&value)))
        .unwrap_or_else(|| source_cache_directory(root).join(archive_name))
}

fn environment_output_directory(root: &Path, variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(root, Path::new(&value)))
        .unwrap_or_else(|| root.join(fallback))
}

fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn environment_text(name: &str) -> Result<Option<String>, FfmpegError> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(FfmpegError::conformance(format!(
            "environment value is not UTF-8: {name}"
        ))),
    }
}

fn environment_command(variable: &str, fallback: &str) -> OsString {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(fallback))
}

fn staged_install_path(destdir: &Path, install: &Path) -> Result<PathBuf, FfmpegError> {
    if !install.is_absolute() {
        return Err(FfmpegError::conformance(format!(
            "installation prefix must be absolute: {}",
            install.display()
        )));
    }
    let relative = install.strip_prefix(Path::new("/")).map_err(|_| {
        FfmpegError::conformance(format!(
            "installation prefix cannot be staged with DESTDIR: {}",
            install.display()
        ))
    })?;
    Ok(destdir.join(relative))
}

fn run_make(directory: &Path, arguments: &[String], label: &str) -> Result<(), FfmpegError> {
    let mut command = Command::new(environment_command("MAKE", "make"));
    command
        .current_dir(directory)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut command, label)
}

fn run_make_with_destdir(
    directory: &Path,
    destdir: &Path,
    arguments: &[String],
    label: &str,
) -> Result<(), FfmpegError> {
    let mut command = Command::new(environment_command("MAKE", "make"));
    command
        .current_dir(directory)
        .args(arguments)
        .env("DESTDIR", destdir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_build_command(&mut command, label)
}

fn run_openssl_make(
    directory: &Path,
    arguments: &[String],
    destdir: Option<&Path>,
    toolchain: &AndroidToolchain,
    label: &str,
) -> Result<(), FfmpegError> {
    let path = joined_search_path(std::slice::from_ref(&toolchain.bin), env::var_os("PATH"))?;
    let mut command = Command::new(environment_command("MAKE", "make"));
    command
        .current_dir(directory)
        .args(arguments)
        .env("ANDROID_NDK_HOME", &toolchain.root)
        .env("ANDROID_NDK_ROOT", &toolchain.root)
        .env("PATH", path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(destdir) = destdir {
        // OpenSSL's Makefile defines DESTDIR, so an environment value is overridden.
        command.arg(format!("DESTDIR={}", destdir.display()));
    }
    run_build_command(&mut command, label)
}

fn run_build_command(command: &mut Command, label: &str) -> Result<(), FfmpegError> {
    let status = if env::var_os("VESPER_ANDROID_PARENT_SUPERVISES_PROCESS_GROUP").as_deref()
        == Some(OsStr::new("1"))
    {
        external_process::run_inherited_process_group(command, label)
    } else {
        external_process::run_interruptible(command, label)
    }
    .map_err(|error| FfmpegError::worker(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(FfmpegError::worker(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

fn joined_search_path(
    local: &[PathBuf],
    existing: Option<OsString>,
) -> Result<OsString, FfmpegError> {
    let mut paths = local.to_vec();
    if let Some(existing) = existing {
        paths.extend(env::split_paths(&existing));
    }
    env::join_paths(paths).map_err(|error| {
        FfmpegError::conformance(format!("failed to construct PKG_CONFIG_PATH: {error}"))
    })
}

fn publish_directory(source: &Path, target: &Path, label: &str) -> Result<(), FfmpegError> {
    let parent = target.parent().ok_or_else(|| {
        FfmpegError::storage(format!(
            "{label} target has no parent: {}",
            target.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        FfmpegError::storage(format!("failed to create {label} parent: {error}"))
    })?;
    let backup = tempfile::Builder::new()
        .prefix(".vesper-ffmpeg-backup-")
        .tempdir_in(parent)
        .map_err(|error| {
            FfmpegError::storage(format!("failed to create {label} backup: {error}"))
        })?;
    let previous = backup.path().join("previous");
    let had_target = match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_dir() => true,
        Ok(_) => {
            return Err(FfmpegError::conformance(format!(
                "{label} target is not a regular directory: {}",
                target.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(FfmpegError::storage(format!(
                "failed to inspect {label} target '{}': {error}",
                target.display()
            )));
        }
    };
    let cancellation = external_process::InterruptDeferral::start(label)
        .map_err(|error| FfmpegError::worker(error.to_string()))?;
    let mut backup = Some(backup);
    let result = if had_target {
        match fs::rename(target, &previous) {
            Err(error) => Err(FfmpegError::storage(format!(
                "failed to stage previous {label}: {error}"
            ))),
            Ok(()) => match fs::rename(source, target) {
                Ok(()) => Ok(()),
                Err(error) => match fs::rename(&previous, target) {
                    Ok(()) => Err(FfmpegError::storage(format!(
                        "failed to publish {label}: {error}; the previous output was restored"
                    ))),
                    Err(rollback) => {
                        let recovery = backup
                            .take()
                            .map(tempfile::TempDir::keep)
                            .unwrap_or_else(|| parent.to_path_buf());
                        Err(FfmpegError::storage(format!(
                            "failed to publish {label}: {error}; rollback failed: {rollback}; recovery output remains at {}",
                            recovery.join("previous").display()
                        )))
                    }
                },
            },
        }
    } else {
        fs::rename(source, target)
            .map_err(|error| FfmpegError::storage(format!("failed to publish {label}: {error}")))
    };
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(()), false) => Ok(()),
        (Ok(()), true) => Err(FfmpegError::worker(format!(
            "{label} was cancelled after publication"
        ))),
        (Err(error), true) => Err(FfmpegError::worker(format!(
            "{label} was cancelled; {error}"
        ))),
        (Err(error), false) => Err(error),
    }
}

fn require_complete_ffmpeg_install(
    root: &Path,
    profile: &NativeFfmpegProfile,
) -> Result<(), FfmpegError> {
    for library in &profile.libraries {
        for relative in [
            format!("lib/pkgconfig/lib{library}.pc"),
            format!("lib/lib{library}.so"),
        ] {
            if !regular_nonempty_file(&root.join(&relative))? {
                return Err(FfmpegError::conformance(format!(
                    "staged Android FFmpeg install is missing {relative}: {}",
                    root.display()
                )));
            }
        }
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), FfmpegError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfmpegError::compatibility(format!("missing {label} '{}': {error}", path.display()))
    })?;
    if metadata.file_type().is_file() && metadata.len() > 0 {
        Ok(())
    } else {
        Err(FfmpegError::conformance(format!(
            "{label} must be a non-empty regular file: {}",
            path.display()
        )))
    }
}

fn require_toolchain_executable(
    path: &Path,
    toolchain_bin: &Path,
    label: &str,
) -> Result<(), FfmpegError> {
    let metadata = fs::metadata(path).map_err(|error| {
        FfmpegError::compatibility(format!("missing {label} '{}': {error}", path.display()))
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(FfmpegError::conformance(format!(
            "{label} does not resolve to a non-empty regular file: {}",
            path.display()
        )));
    }
    let canonical_bin = toolchain_bin.canonicalize().map_err(|error| {
        FfmpegError::storage(format!(
            "failed to resolve Android toolchain directory '{}': {error}",
            toolchain_bin.display()
        ))
    })?;
    let canonical = path.canonicalize().map_err(|error| {
        FfmpegError::storage(format!(
            "failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(&canonical_bin) {
        return Err(FfmpegError::conformance(format!(
            "{label} resolves outside the selected Android toolchain: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(FfmpegError::conformance(format!(
                "{label} is not executable: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn regular_nonempty_file(path: &Path) -> Result<bool, FfmpegError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file() && metadata.len() > 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(FfmpegError::storage(format!(
            "failed to inspect '{}': {error}",
            path.display()
        ))),
    }
}

fn regular_file_equals(path: &Path, expected: &[u8]) -> Result<bool, FfmpegError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(FfmpegError::storage(format!(
                "failed to inspect metadata '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_METADATA_BYTES {
        return Ok(false);
    }
    let actual = fs::read(path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to read metadata '{}': {error}",
            path.display()
        ))
    })?;
    Ok(actual == expected)
}

fn pkg_config_version(path: &Path) -> Result<Option<String>, FfmpegError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(FfmpegError::storage(format!(
                "failed to inspect pkg-config file '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_METADATA_BYTES {
        return Ok(None);
    }
    let source = fs::read_to_string(path).map_err(|error| {
        FfmpegError::conformance(format!(
            "failed to read UTF-8 pkg-config file '{}': {error}",
            path.display()
        ))
    })?;
    let versions = source
        .lines()
        .filter_map(|line| line.strip_prefix("Version:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if versions.len() == 1 {
        Ok(Some(versions[0].to_owned()))
    } else {
        Ok(None)
    }
}

fn map_source_error(error: source_archive::SourceArchiveError) -> FfmpegError {
    match error.kind() {
        SourceArchiveErrorKind::Storage => FfmpegError::storage(error.to_string()),
        SourceArchiveErrorKind::Conformance => FfmpegError::conformance(error.to_string()),
        SourceArchiveErrorKind::Worker => FfmpegError::worker(error.to_string()),
    }
}

fn map_android_error(error: android::AndroidError) -> FfmpegError {
    match error.kind() {
        android::AndroidErrorKind::Usage | android::AndroidErrorKind::Conformance => {
            FfmpegError::conformance(error.to_string())
        }
        android::AndroidErrorKind::Storage => FfmpegError::storage(error.to_string()),
        android::AndroidErrorKind::Compatibility => FfmpegError::compatibility(error.to_string()),
        android::AndroidErrorKind::Worker => FfmpegError::worker(error.to_string()),
    }
}

fn map_gradle_error(error: gradle::GradleError) -> FfmpegError {
    match error.kind() {
        gradle::GradleErrorKind::Storage => FfmpegError::storage(error.to_string()),
        gradle::GradleErrorKind::Compatibility => FfmpegError::compatibility(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openssl_default_series_refreshes_an_older_cached_patch() {
        assert_eq!(
            resolve_openssl_selection(
                None,
                None,
                || Ok(vec!["openssl-3.5.9.tar.gz".to_owned()]),
                || Ok("openssl-3.5.10.tar.gz openssl-3.6.2.tar.gz".to_owned()),
            )
            .expect("latest release in the default LTS series"),
            "3.5.10"
        );
    }

    #[test]
    fn libxml2_default_series_refreshes_cached_patches_and_keeps_exact_overrides() {
        assert_eq!(
            resolve_libxml2_selection(
                None,
                None,
                || Ok(vec!["libxml2-2.15.4.tar.xz".to_owned()]),
                || Ok("libxml2-2.15.5.tar.xz libxml2-2.16.1.tar.xz".to_owned()),
            )
            .expect("new patch in the current series"),
            "2.15.5"
        );
        assert_eq!(
            resolve_libxml2_selection(
                Some("2.14.6"),
                Some("invalid"),
                || panic!("exact override must not inspect cache"),
                || panic!("exact override must not query index"),
            )
            .expect("explicit exact override"),
            "2.14.6"
        );
        assert_eq!(
            resolve_libxml2_selection(
                None,
                None,
                || Ok(vec!["libxml2-2.15.5.tar.xz".to_owned()]),
                || Err(FfmpegError::worker("libxml2 release index lookup: offline")),
            )
            .expect("offline cache fallback"),
            "2.15.5"
        );
    }

    #[test]
    fn openssl_series_lookup_preserves_failure_cause() {
        let error = resolve_openssl_selection(
            None,
            Some("3.5"),
            || Ok(Vec::new()),
            || {
                Err(FfmpegError::worker(
                    "OpenSSL release index lookup: TLS certificate failure",
                ))
            },
        )
        .expect_err("failed lookup must not yield a series");
        assert!(error.to_string().contains("TLS certificate failure"));
    }

    #[test]
    fn openssl_exact_override_precedes_series_and_rejects_non_exact_versions() {
        assert_eq!(
            resolve_openssl_selection(
                Some("3.6.2"),
                Some("invalid"),
                || panic!("exact override must not inspect cache"),
                || panic!("exact override must not query index"),
            )
            .expect("exact override"),
            "3.6.2"
        );
        for version in ["3.5", "3.5.9-beta", "3.5.9+local"] {
            assert!(
                resolve_openssl_selection(
                    Some(version),
                    None,
                    || Ok(Vec::new()),
                    || Ok(String::new()),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn openssl_explicit_series_preserves_offline_cache_selection() {
        assert_eq!(
            resolve_openssl_selection(
                None,
                Some("3.5"),
                || Ok(vec![
                    "openssl-3.5.7.tar.gz".to_owned(),
                    "openssl-3.6.2.tar.gz".to_owned()
                ]),
                || Err(FfmpegError::worker("OpenSSL release index lookup: offline")),
            )
            .expect("cached exact release"),
            "3.5.7"
        );
    }

    #[test]
    fn openssl_series_lookup_requires_a_matching_exact_release() {
        assert_eq!(
            resolve_openssl_selection(
                None,
                Some("3.5"),
                || Ok(Vec::new()),
                || Ok("openssl-3.5.8.tar.gz openssl-3.5.9.tar.gz openssl-3.6.2.tar.gz".to_owned()),
            )
            .expect("remote exact release"),
            "3.5.9"
        );
        let error = resolve_openssl_selection(
            None,
            Some("3.5"),
            || Ok(Vec::new()),
            || Ok("openssl-3.6.2.tar.gz".to_owned()),
        )
        .expect_err("missing release must not become 3.5");
        assert!(
            error
                .to_string()
                .contains("no exact release for series 3.5")
        );
    }

    #[test]
    fn android_dependency_known_versions_verify_official_sources_and_checksums() {
        let xml = AndroidDependencySource::new("libxml2", KNOWN_LIBXML2_VERSION, None, None)
            .expect("pinned libxml2");
        assert_eq!(xml.version, "2.15.4");
        assert_eq!(
            xml.source_url,
            "https://download.gnome.org/sources/libxml2/2.15/libxml2-2.15.4.tar.xz"
        );
        assert_eq!(
            xml.sha256.as_deref(),
            Some("98087fd181d9070724f3fbc65c7377db03038eb92bd882374daff44940138821")
        );
        let ssl = AndroidDependencySource::new("openssl", KNOWN_OPENSSL_VERSION, None, None)
            .expect("pinned OpenSSL");
        assert_eq!(ssl.version, "3.5.9");
        assert_eq!(
            ssl.source_url,
            "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz"
        );
        assert_eq!(
            ssl.sha256.as_deref(),
            Some("603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a")
        );
    }

    #[test]
    fn libxml2_configure_omits_removed_lzma_option_only_for_new_releases() {
        assert!(
            !libxml2_configure_arguments("2.15.4")
                .expect("new release")
                .contains(&"--without-lzma")
        );
        assert!(
            libxml2_configure_arguments("2.14.6")
                .expect("old override")
                .contains(&"--without-lzma")
        );
    }

    #[test]
    fn android_dependency_patches_accept_optional_checksum_overrides() {
        let source = AndroidDependencySource::new("openssl", "3.5.10", None, None)
            .expect("new patch without a checked-in digest");
        assert!(source.sha256.is_none());
        let source = AndroidDependencySource::new(
            "openssl",
            "3.5.7",
            Some("https://mirror.example/openssl-3.5.7.tar.gz".to_owned()),
            Some("a".repeat(64)),
        )
        .expect("explicit source identity");
        assert_eq!(
            source.source_url,
            "https://mirror.example/openssl-3.5.7.tar.gz"
        );
        assert_eq!(source.sha256, Some("a".repeat(64)));
        assert!(
            AndroidDependencySource::new("libxml2", "2.15.4", None, Some("invalid".to_owned()),)
                .is_err()
        );
    }

    fn write_dependency_fixture(install: &Path, name: &str, version: &str) {
        let files = match name {
            "libxml2" => vec![
                (
                    "lib/pkgconfig/libxml-2.0.pc",
                    format!("Version: {version}\n"),
                ),
                (
                    "include/libxml2/libxml/xmlversion.h",
                    format!("#define LIBXML_DOTTED_VERSION \"{version}\"\n"),
                ),
                ("lib/libxml2.so", "libxml2 fixture".to_owned()),
            ],
            "openssl" => vec![
                ("lib/pkgconfig/openssl.pc", format!("Version: {version}\n")),
                (
                    "include/openssl/opensslv.h",
                    format!("# define OPENSSL_VERSION_STR \"{version}\"\n"),
                ),
                ("lib/libssl.so", "libssl fixture".to_owned()),
                ("lib/libcrypto.so", "libcrypto fixture".to_owned()),
            ],
            _ => panic!("unknown fixture"),
        };
        for (relative, contents) in files {
            let path = install.join(relative);
            fs::create_dir_all(path.parent().expect("file parent")).expect("fixture directory");
            fs::write(path, contents).expect("fixture file");
        }
    }

    #[test]
    fn moving_dependency_patch_records_the_archive_digest_for_reuse() {
        let directory = tempfile::tempdir().expect("source cache fixture");
        let archive = directory
            .path()
            .join("third_party/_cache/openssl-3.5.10.tar.gz");
        fs::create_dir_all(archive.parent().expect("cache parent")).expect("cache directory");
        fs::write(&archive, "first archive bytes").expect("cached archive");
        let source = AndroidDependencySource::new("openssl", "3.5.10", None, None)
            .expect("moving patch")
            .record_source_digest(directory.path())
            .expect("record cached archive digest");
        let install = directory.path().join("install");
        write_dependency_fixture(&install, "openssl", "3.5.10");
        source
            .write_metadata(&install, "arm64-v8a", 26)
            .expect("receipt");
        assert!(source.is_current(&install, "arm64-v8a", 26).expect("reuse"));

        fs::write(&archive, "changed archive bytes").expect("changed cached source");
        let changed = AndroidDependencySource::new("openssl", "3.5.10", None, None)
            .expect("same patch")
            .record_source_digest(directory.path())
            .expect("record changed source digest");
        assert_ne!(source.sha256, changed.sha256);
        assert!(
            !changed
                .is_current(&install, "arm64-v8a", 26)
                .expect("rebuild")
        );
    }

    #[test]
    fn android_dependency_reuse_rejects_missing_old_and_unrecorded_installs() {
        let stage = tempfile::tempdir().expect("install fixture");
        for (name, version, old) in [
            ("libxml2", "2.15.4", "2.14.6"),
            ("openssl", "3.5.9", "3.5.7"),
        ] {
            let install = stage.path().join(name);
            let source = AndroidDependencySource::new(name, version, None, None).expect("source");
            assert!(
                !source
                    .is_current(&install, "arm64-v8a", 26)
                    .expect("missing install")
            );
            write_dependency_fixture(&install, name, old);
            assert!(
                !source
                    .is_current(&install, "arm64-v8a", 26)
                    .expect("old install")
            );
            write_dependency_fixture(&install, name, version);
            assert!(
                !source
                    .is_current(&install, "arm64-v8a", 26)
                    .expect("missing provenance")
            );
            source
                .write_metadata(&install, "arm64-v8a", 26)
                .expect("record install");
            assert!(
                source
                    .is_current(&install, "arm64-v8a", 26)
                    .expect("current install")
            );
            assert!(
                !source
                    .is_current(&install, "arm64-v8a", 27)
                    .expect("different API")
            );
        }
    }

    #[test]
    fn android_dependency_reuse_checks_headers_libraries_and_source_identity() {
        let stage = tempfile::tempdir().expect("install fixture");
        let install = stage.path();
        let source = AndroidDependencySource::new("libxml2", "2.15.4", None, None).expect("source");
        write_dependency_fixture(install, "libxml2", "2.15.4");
        source
            .write_metadata(install, "arm64-v8a", 26)
            .expect("record install");
        fs::write(
            install.join("include/libxml2/libxml/xmlversion.h"),
            "#define LIBXML_DOTTED_VERSION \"2.14.6\"\n",
        )
        .expect("stale header");
        assert!(
            !source
                .is_current(install, "arm64-v8a", 26)
                .expect("stale header")
        );
        write_dependency_fixture(install, "libxml2", "2.15.4");
        fs::write(install.join("lib/libxml2.so"), "different build").expect("changed binary");
        assert!(
            !source
                .is_current(install, "arm64-v8a", 26)
                .expect("changed binary")
        );
        write_dependency_fixture(install, "libxml2", "2.15.4");
        let mirror = AndroidDependencySource::new(
            "libxml2",
            "2.15.4",
            Some("https://mirror.example/libxml2-2.15.4.tar.xz".to_owned()),
            None,
        )
        .expect("mirror");
        assert!(
            !mirror
                .is_current(install, "arm64-v8a", 26)
                .expect("changed source URL")
        );
        let patched = AndroidDependencySource::new("libxml2", "2.15.4", None, Some("b".repeat(64)))
            .expect("patched source");
        assert!(
            !patched
                .is_current(install, "arm64-v8a", 26)
                .expect("changed source hash")
        );
        fs::remove_file(install.join("lib/libxml2.so")).expect("remove library");
        assert!(
            !source
                .is_current(install, "arm64-v8a", 26)
                .expect("missing library")
        );
    }

    #[cfg(unix)]
    #[test]
    fn android_dependency_reuse_follows_installed_shared_library_links() {
        let stage = tempfile::tempdir().expect("install fixture");
        let install = stage.path();
        let source = AndroidDependencySource::new("libxml2", "2.15.4", None, None).expect("source");
        write_dependency_fixture(install, "libxml2", "2.15.4");
        fs::rename(
            install.join("lib/libxml2.so"),
            install.join("lib/libxml2.so.16"),
        )
        .expect("versioned library");
        std::os::unix::fs::symlink("libxml2.so.16", install.join("lib/libxml2.so"))
            .expect("library symlink");
        source
            .write_metadata(install, "arm64-v8a", 26)
            .expect("record linked install");
        assert!(
            source
                .is_current(install, "arm64-v8a", 26)
                .expect("linked install")
        );
    }

    #[test]
    fn android_ffmpeg_reuse_includes_dependency_provenance() {
        let stage = tempfile::tempdir().expect("FFmpeg fixture");
        let dependency_install = stage.path().join("dependency");
        let old = AndroidDependencySource::new("libxml2", "2.14.6", None, Some("c".repeat(64)))
            .expect("old source");
        write_dependency_fixture(&dependency_install, "libxml2", "2.14.6");
        old.write_metadata(&dependency_install, "arm64-v8a", 26)
            .expect("old receipt");
        let base = "Vesper FFmpeg build metadata v2\nexternal_dependencies=libxml2\n";
        let mut old_metadata = base.to_owned();
        append_dependency_metadata(&mut old_metadata, &dependency_install, "libxml2")
            .expect("old provenance");
        let new =
            AndroidDependencySource::new("libxml2", "2.15.4", None, None).expect("new source");
        write_dependency_fixture(&dependency_install, "libxml2", "2.15.4");
        new.write_metadata(&dependency_install, "arm64-v8a", 26)
            .expect("new receipt");
        let mut metadata = base.to_owned();
        append_dependency_metadata(&mut metadata, &dependency_install, "libxml2")
            .expect("new provenance");
        assert!(metadata.contains("libxml2_version=2.15.4\n"));
        assert!(metadata.contains(&format!("libxml2_source_sha256={KNOWN_LIBXML2_SHA256}\n")));
        let install = stage.path().join("ffmpeg");
        fs::create_dir_all(install.join("lib/pkgconfig")).expect("FFmpeg fixture directory");
        fs::write(install.join("lib/libavcodec.so"), "avcodec fixture").expect("FFmpeg library");
        fs::write(
            install.join("lib/pkgconfig/libavcodec.pc"),
            "avcodec fixture",
        )
        .expect("FFmpeg pc");
        let mut plan = AndroidAbiPlan {
            abi: "arm64-v8a".to_owned(),
            install,
            build: stage.path().join("build"),
            configure_line: Vec::new(),
            metadata,
            pkg_config_paths: Vec::new(),
        };
        let libraries = vec!["avcodec".to_owned()];
        for stale in [base, &old_metadata] {
            fs::write(plan.install.join("vesper-ffmpeg-build-metadata.txt"), stale)
                .expect("stale metadata");
            assert!(
                !plan
                    .is_current(&libraries)
                    .expect("stale provenance must rebuild")
            );
        }
        fs::write(
            plan.install.join("vesper-ffmpeg-build-metadata.txt"),
            &plan.metadata,
        )
        .expect("current metadata");
        assert!(
            plan.is_current(&libraries)
                .expect("same dependency provenance can reuse")
        );
        for changed_source in [
            AndroidDependencySource::new(
                "libxml2",
                "2.15.4",
                Some("https://mirror.example/libxml2-2.15.4.tar.xz".to_owned()),
                None,
            )
            .expect("changed source URL"),
            AndroidDependencySource::new("libxml2", "2.15.4", None, Some("d".repeat(64)))
                .expect("changed source checksum"),
        ] {
            changed_source
                .write_metadata(&dependency_install, "arm64-v8a", 26)
                .expect("record changed source identity");
            plan.metadata = base.to_owned();
            append_dependency_metadata(&mut plan.metadata, &dependency_install, "libxml2")
                .expect("updated source provenance");
            assert!(
                !plan
                    .is_current(&libraries)
                    .expect("source identity change must rebuild FFmpeg")
            );
        }
        fs::write(
            dependency_install.join("lib/libxml2.so"),
            "new real library bytes",
        )
        .expect("changed dependency library");
        new.write_metadata(&dependency_install, "arm64-v8a", 26)
            .expect("record new dependency build");
        plan.metadata = base.to_owned();
        append_dependency_metadata(&mut plan.metadata, &dependency_install, "libxml2")
            .expect("updated binary provenance");
        assert!(
            !plan
                .is_current(&libraries)
                .expect("changed dependency .so must rebuild FFmpeg")
        );
    }

    #[test]
    fn openssl_install_stages_files_without_overwriting_the_live_prefix() {
        let directory = tempfile::tempdir().expect("create OpenSSL install fixture");
        let install = directory.path().join("live-prefix");
        fs::create_dir(&install).expect("create live prefix");
        fs::write(install.join("receipt"), b"original").expect("preserve installed file");
        fs::write(directory.path().join("Makefile"), format!(
            "DESTDIR=\n.PHONY: install_sw\ninstall_sw:\n\tmkdir -p \"$(DESTDIR){}\"\n\tprintf staged > \"$(DESTDIR){}/receipt\"\n",
            install.display(), install.display(),
        )).expect("write OpenSSL-style Makefile");
        let destination = directory.path().join("stage");
        let toolchain = AndroidToolchain {
            root: directory.path().to_path_buf(),
            bin: directory.path().to_path_buf(),
            sysroot: directory.path().to_path_buf(),
        };
        run_openssl_make(
            directory.path(),
            &["install_sw".to_owned()],
            Some(&destination),
            &toolchain,
            "OpenSSL install fixture",
        )
        .expect("stage installation");
        let staged = staged_install_path(&destination, &install).expect("resolve staging prefix");
        assert_eq!(
            fs::read(staged.join("receipt")).expect("read staged install"),
            b"staged"
        );
        assert_eq!(
            fs::read(install.join("receipt")).expect("read live install"),
            b"original"
        );
    }

    #[test]
    fn staged_install_paths_preserve_absolute_prefixes() {
        assert_eq!(
            staged_install_path(Path::new("/stage"), Path::new("/output/arm64-v8a"))
                .expect("stage absolute install"),
            Path::new("/stage/output/arm64-v8a")
        );
        assert!(staged_install_path(Path::new("/stage"), Path::new("relative")).is_err());
    }

    #[test]
    fn package_series_resolution_is_patch_aware() {
        let version = latest_package_series_archive_version(
            "openssl",
            "3.5",
            [
                "openssl-3.5.2.tar.gz".to_owned(),
                "openssl-3.5.11.tar.gz".to_owned(),
                "openssl-3.6.1.tar.gz".to_owned(),
            ],
            ".tar.gz",
        );
        assert_eq!(version.as_deref(), Some("3.5.11"));
    }
}
