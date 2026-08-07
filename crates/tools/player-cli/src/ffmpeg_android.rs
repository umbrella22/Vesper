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
    latest_package_series_archive_version_from_index,
};
use crate::source_archive::{
    self, SourceArchiveErrorKind, SourceArchiveFormat, SourceArchivePolicy,
};
use crate::{android, gradle};

const ANDROID_RUST_TARGET: &str = "aarch64-linux-android";
const DEFAULT_ANDROID_API_LEVEL: u32 = 26;
const DEFAULT_OPENSSL_SERIES: &str = "3.5";
const DEFAULT_LIBXML2_VERSION: &str = "2.14.6";
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
        for abi in &abis {
            ensure_openssl(
                root,
                abi,
                &version,
                api_level,
                &toolchain,
                &openssl_output,
                jobs,
            )?;
        }
    }
    if profile.enable_dash {
        let version = libxml2_version()?;
        for abi in &abis {
            ensure_libxml2(
                root,
                abi,
                &version,
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
        if !profile.force && plan.is_current(profile)? {
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
        let metadata = profile.metadata_text(
            "android",
            abi,
            &source.version,
            source_archive,
            &source.source_url,
            source_sha256,
            &configure_line,
        );
        Ok(Self {
            abi: abi.to_owned(),
            install,
            build,
            configure_line,
            metadata,
            pkg_config_paths,
        })
    }

    fn is_current(&self, profile: &NativeFfmpegProfile) -> Result<bool, FfmpegError> {
        let metadata = self.install.join("vesper-ffmpeg-build-metadata.txt");
        if !regular_file_equals(&metadata, self.metadata.as_bytes())? {
            return Ok(false);
        }
        for library in &profile.libraries {
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

#[allow(clippy::too_many_arguments)]
fn ensure_openssl(
    root: &Path,
    abi: &str,
    version: &str,
    api_level: u32,
    toolchain: &AndroidToolchain,
    output: &Path,
    jobs: usize,
) -> Result<(), FfmpegError> {
    let install = output.join(abi);
    let pc = install.join("lib/pkgconfig/openssl.pc");
    if pkg_config_version(&pc)?.as_deref() == Some(version) {
        return Ok(());
    }
    let archive_name = format!("openssl-{version}.tar.gz");
    let series = version
        .rsplit_once('.')
        .map(|(series, _)| series)
        .unwrap_or(version);
    let source =
        configured_archive_path(root, "VESPER_ANDROID_OPENSSL_SOURCE_ARCHIVE", &archive_name);
    let primary = environment_text("VESPER_ANDROID_OPENSSL_SOURCE_URL")?
        .unwrap_or_else(|| format!("https://www.openssl.org/source/{archive_name}"));
    let urls = vec![
        primary,
        format!("https://www.openssl.org/source/old/{series}/{archive_name}"),
        format!("https://www.openssl-library.org/source/{archive_name}"),
        format!("https://www.openssl-library.org/source/old/{series}/{archive_name}"),
    ];
    let expected = environment_text("VESPER_ANDROID_OPENSSL_SOURCE_SHA256")?
        .or(environment_text("VESPER_OPENSSL_SOURCE_SHA256")?);
    let archive = source_archive::ensure_cached_archive(
        &source,
        &urls,
        expected.as_deref(),
        SOURCE_POLICY,
        "Android OpenSSL source archive",
    )
    .map_err(map_source_error)?;
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
        .arg(format!("--prefix={}", install.display()))
        .arg(format!("--openssldir={}", install.join("ssl").display()))
        .env("ANDROID_NDK_HOME", &toolchain.root)
        .env("ANDROID_NDK_ROOT", &toolchain.root)
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
    if pkg_config_version(&staged.join("lib/pkgconfig/openssl.pc"))?.as_deref() != Some(version) {
        return Err(FfmpegError::conformance(
            "staged Android OpenSSL pkg-config version does not match the requested version",
        ));
    }
    publish_directory(&staged, &install, "Android OpenSSL prebuilt")
}

#[allow(clippy::too_many_arguments)]
fn ensure_libxml2(
    root: &Path,
    abi: &str,
    version: &str,
    api_level: u32,
    toolchain: &AndroidToolchain,
    output: &Path,
    jobs: usize,
) -> Result<(), FfmpegError> {
    let install = output.join(abi);
    let pc = install.join("lib/pkgconfig/libxml-2.0.pc");
    if pkg_config_version(&pc)?.as_deref() == Some(version) {
        return Ok(());
    }
    let archive_name = format!("libxml2-{version}.tar.xz");
    let series = version
        .rsplit_once('.')
        .map(|(series, _)| series)
        .unwrap_or(version);
    let source =
        configured_archive_path(root, "VESPER_ANDROID_LIBXML2_SOURCE_ARCHIVE", &archive_name);
    let url = environment_text("VESPER_ANDROID_LIBXML2_SOURCE_URL")?.unwrap_or_else(|| {
        format!("https://download.gnome.org/sources/libxml2/{series}/{archive_name}")
    });
    let expected = environment_text("VESPER_ANDROID_LIBXML2_SOURCE_SHA256")?;
    let archive = source_archive::ensure_cached_archive(
        &source,
        &[url],
        expected.as_deref(),
        SOURCE_POLICY,
        "Android libxml2 source archive",
    )
    .map_err(map_source_error)?;
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
        .args([
            "--enable-shared",
            "--disable-static",
            "--without-iconv",
            "--without-python",
            "--without-lzma",
            "--without-icu",
            "--without-http",
            "--without-legacy",
            "--without-html",
        ])
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
    if pkg_config_version(&staged.join("lib/pkgconfig/libxml-2.0.pc"))?.as_deref() != Some(version)
    {
        return Err(FfmpegError::conformance(
            "staged Android libxml2 pkg-config version does not match the requested version",
        ));
    }
    publish_directory(&staged, &install, "Android libxml2 prebuilt")
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
    if let Some(exact) = environment_text("VESPER_ANDROID_OPENSSL_VERSION")?
        .or(environment_text("VESPER_OPENSSL_VERSION")?)
    {
        validate_exact_version(&exact, "OpenSSL")?;
        return Ok(exact);
    }
    let series = environment_text("VESPER_ANDROID_OPENSSL_SERIES")?
        .or(environment_text("VESPER_OPENSSL_SERIES")?)
        .or(environment_text("VESPER_OPENSSL_LTS_SERIES")?)
        .unwrap_or_else(|| DEFAULT_OPENSSL_SERIES.to_owned());
    validate_series(&series, "OpenSSL")?;
    let cache = source_cache_directory(root);
    let names = source_cache_names(&cache)?;
    if let Some(version) =
        latest_package_series_archive_version("openssl", &series, names.clone(), ".tar.gz")
    {
        return Ok(version);
    }
    let remote = source_archive::fetch_bounded_text(
        "https://openssl-library.org/source/",
        MAX_RELEASE_INDEX_BYTES,
        "OpenSSL release index lookup",
    )
    .ok();
    if let Some(version) = remote.as_deref().and_then(|index| {
        latest_package_series_archive_version_from_index("openssl", &series, index, ".tar.gz")
    }) {
        return Ok(version);
    }
    eprintln!(
        "warning: could not resolve the latest OpenSSL patch for series {series}; falling back to {series}"
    );
    Ok(series)
}

fn libxml2_version() -> Result<String, FfmpegError> {
    let version = environment_text("VESPER_ANDROID_LIBXML2_VERSION")?
        .unwrap_or_else(|| DEFAULT_LIBXML2_VERSION.to_owned());
    validate_exact_version(&version, "libxml2")?;
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
        command.env("DESTDIR", destdir);
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
