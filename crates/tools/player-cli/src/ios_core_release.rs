#![cfg(target_os = "macos")]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;
use zip::ZipArchive;

use crate::external_process::{self, ExternalProcessErrorKind};
use crate::ios::{self, IosError};

const FRAMEWORK_NAME: &str = "VesperPlayerKit";
const FRAMEWORK_ROOT: &str = "VesperPlayerKit.framework";
const XCFRAMEWORK_ROOT: &str = "VesperPlayerKit.xcframework";
const BUNDLE_IDENTIFIER: &str = "io.github.umbrella22.vesper.lib.ioshost";
const DEVICE_ARCHIVE: &str = "VesperPlayerKit-ios-arm64.framework.zip";
const SIMULATOR_ARCHIVE: &str = "VesperPlayerKit-ios-simulator-arm64.framework.zip";
const XCFRAMEWORK_ARCHIVE: &str = "VesperPlayerKit.xcframework.zip";
const MAX_ARCHIVE_ENTRIES: usize = 512;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 2_048;
const MAX_TREE_DEPTH: usize = 32;
const MAX_SMALL_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_REGULAR_FILE: u32 = 0o100000;
const UNIX_DIRECTORY: u32 = 0o040000;

struct CoreMetadata {
    version: String,
    build: String,
    minimum_os: String,
}

#[derive(Clone, Copy)]
struct SliceSpec {
    supported_platform: &'static str,
    dt_platform: &'static str,
    macho_platform: &'static str,
    interface_target: &'static str,
    sdk: &'static str,
    clang_target_suffix: &'static str,
}

const DEVICE: SliceSpec = SliceSpec {
    supported_platform: "iPhoneOS",
    dt_platform: "iphoneos",
    macho_platform: "IOS",
    interface_target: "arm64-apple-ios",
    sdk: "iphoneos",
    clang_target_suffix: "",
};

const SIMULATOR: SliceSpec = SliceSpec {
    supported_platform: "iPhoneSimulator",
    dt_platform: "iphonesimulator",
    macho_platform: "IOSSIMULATOR",
    interface_target: "arm64-apple-ios-simulator",
    sdk: "iphonesimulator",
    clang_target_suffix: "-simulator",
};

pub(crate) fn verify(root: &Path, release_directory: &Path) -> Result<(), IosError> {
    let metadata = load_core_metadata(root)?;
    let temporary = tempfile::Builder::new()
        .prefix("vesper-ios-core-release-rust.")
        .tempdir()
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create core iOS verifier workspace: {error}"
            ))
        })?;

    let device = extract_archive(
        &release_directory.join(DEVICE_ARCHIVE),
        FRAMEWORK_ROOT,
        &temporary.path().join("standalone device"),
        "device framework archive",
    )?;
    let simulator = extract_archive(
        &release_directory.join(SIMULATOR_ARCHIVE),
        FRAMEWORK_ROOT,
        &temporary.path().join("standalone simulator"),
        "Simulator framework archive",
    )?;
    let xcframework = extract_archive(
        &release_directory.join(XCFRAMEWORK_ARCHIVE),
        XCFRAMEWORK_ROOT,
        &temporary.path().join("XCFramework"),
        "XCFramework archive",
    )?;

    validate_xcframework(&xcframework)?;
    let xc_device = xcframework.join("ios-arm64").join(FRAMEWORK_ROOT);
    let xc_simulator = xcframework.join("ios-arm64-simulator").join(FRAMEWORK_ROOT);

    for (framework, slice) in [
        (&device, DEVICE),
        (&simulator, SIMULATOR),
        (&xc_device, DEVICE),
        (&xc_simulator, SIMULATOR),
    ] {
        validate_framework(framework, slice, &metadata)?;
    }

    compare_trees(&device, &xc_device, "device")?;
    compare_trees(&simulator, &xc_simulator, "Simulator")?;

    verify_consumer(&device, DEVICE, &metadata, &temporary, "standalone-device")?;
    verify_consumer(
        &simulator,
        SIMULATOR,
        &metadata,
        &temporary,
        "standalone-simulator",
    )?;
    verify_consumer(
        &xc_device,
        DEVICE,
        &metadata,
        &temporary,
        "xcframework-device",
    )?;
    verify_consumer(
        &xc_simulator,
        SIMULATOR,
        &metadata,
        &temporary,
        "xcframework-simulator",
    )
}

fn load_core_metadata(root: &Path) -> Result<CoreMetadata, IosError> {
    let path = root.join("lib/ios/VesperPlayerKit/Sources/Generated-Info.plist");
    let object = ios::read_plist_dictionary(&path, "VesperPlayerKit release")?;
    let version = ios::required_plist_string(&object, "CFBundleShortVersionString", &path)?;
    let build = ios::required_plist_string(&object, "CFBundleVersion", &path)?;
    let minimum_os = match env::var("VESPER_APPLE_IOS_DEPLOYMENT_TARGET") {
        Ok(value) if !value.is_empty() => value,
        Ok(_) | Err(env::VarError::NotPresent) => "17.0".to_owned(),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(IosError::conformance(
                "VESPER_APPLE_IOS_DEPLOYMENT_TARGET must be valid UTF-8",
            ));
        }
    };
    let _ = ios::normalize_apple_version(&minimum_os, "iOS deployment target")?;
    Ok(CoreMetadata {
        version: version.to_owned(),
        build: build.to_owned(),
        minimum_os,
    })
}

fn extract_archive(
    archive_path: &Path,
    expected_root: &str,
    destination: &Path,
    label: &str,
) -> Result<PathBuf, IosError> {
    let metadata = fs::symlink_metadata(archive_path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            archive_path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(IosError::conformance(format!(
            "{label} must be a regular non-symlink file: {}",
            archive_path.display()
        )));
    }
    fs::create_dir(destination).map_err(|error| {
        IosError::storage(format!(
            "failed to create {label} extraction directory '{}': {error}",
            destination.display()
        ))
    })?;
    let file = File::open(archive_path).map_err(|error| {
        IosError::storage(format!(
            "failed to open {label} '{}': {error}",
            archive_path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        IosError::conformance(format!(
            "invalid {label} '{}': {error}",
            archive_path.display()
        ))
    })?;
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(IosError::conformance(format!(
            "{label} must contain 1 to {MAX_ARCHIVE_ENTRIES} entries"
        )));
    }
    let mut seen = BTreeSet::new();
    let mut expanded_bytes = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            IosError::conformance(format!("failed to read {label} entry {index}: {error}"))
        })?;
        let raw_name = std::str::from_utf8(entry.name_raw()).map_err(|error| {
            IosError::conformance(format!("{label} contains a non-UTF-8 path: {error}"))
        })?;
        let is_directory = entry.is_dir();
        let name = validate_archive_name(raw_name, is_directory, expected_root, label)?;
        if !seen.insert(name.clone()) {
            return Err(IosError::conformance(format!(
                "{label} contains a duplicate path: {name}"
            )));
        }
        if is_fixture_path(&name) {
            return Err(IosError::conformance(format!(
                "{label} contains test fixture resources: {name}"
            )));
        }
        let mode = entry.unix_mode().ok_or_else(|| {
            IosError::conformance(format!("{label} entry omits Unix file type: {name}"))
        })?;
        let expected_type = if is_directory {
            UNIX_DIRECTORY
        } else {
            UNIX_REGULAR_FILE
        };
        if mode & UNIX_FILE_TYPE_MASK != expected_type {
            return Err(IosError::conformance(format!(
                "{label} contains a symlink or unsupported file type: {name}"
            )));
        }
        if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(IosError::conformance(format!(
                "{label} entry exceeds {MAX_ARCHIVE_ENTRY_BYTES} bytes: {name}"
            )));
        }
        expanded_bytes = expanded_bytes
            .checked_add(entry.size())
            .ok_or_else(|| IosError::conformance(format!("{label} expanded size overflowed")))?;
        if expanded_bytes > MAX_ARCHIVE_EXPANDED_BYTES {
            return Err(IosError::conformance(format!(
                "{label} expands beyond {MAX_ARCHIVE_EXPANDED_BYTES} bytes"
            )));
        }

        let output_path = destination.join(&name);
        if is_directory {
            fs::create_dir_all(&output_path).map_err(|error| {
                IosError::storage(format!(
                    "failed to create extracted directory '{}': {error}",
                    output_path.display()
                ))
            })?;
        } else {
            let parent = output_path.parent().ok_or_else(|| {
                IosError::conformance(format!("{label} entry has no parent: {name}"))
            })?;
            fs::create_dir_all(parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to create extracted parent '{}': {error}",
                    parent.display()
                ))
            })?;
            let mut output = File::create(&output_path).map_err(|error| {
                IosError::storage(format!(
                    "failed to create extracted file '{}': {error}",
                    output_path.display()
                ))
            })?;
            let copied = io::copy(&mut entry, &mut output).map_err(|error| {
                IosError::storage(format!("failed to extract {label} entry '{name}': {error}"))
            })?;
            if copied != entry.size() {
                return Err(IosError::conformance(format!(
                    "{label} entry size changed while extracting: {name}"
                )));
            }
        }
        fs::set_permissions(&output_path, fs::Permissions::from_mode(mode & 0o777)).map_err(
            |error| {
                IosError::storage(format!(
                    "failed to set extracted permissions '{}': {error}",
                    output_path.display()
                ))
            },
        )?;
    }
    let expected = destination.join(expected_root);
    let entries = directory_entry_names(destination, label)?;
    if entries != BTreeSet::from([expected_root.to_owned()])
        || !fs::symlink_metadata(&expected).is_ok_and(|metadata| metadata.file_type().is_dir())
    {
        return Err(IosError::conformance(format!(
            "{label} must contain exactly one {expected_root} root"
        )));
    }
    Ok(expected)
}

fn validate_archive_name(
    raw_name: &str,
    is_directory: bool,
    expected_root: &str,
    label: &str,
) -> Result<String, IosError> {
    if raw_name.is_empty()
        || raw_name.starts_with('/')
        || raw_name.contains('\\')
        || raw_name.contains(':')
        || raw_name.chars().any(char::is_control)
    {
        return Err(IosError::conformance(format!(
            "{label} contains an unsafe archive path: {raw_name:?}"
        )));
    }
    let name = if is_directory {
        raw_name.strip_suffix('/').unwrap_or(raw_name)
    } else {
        if raw_name.ends_with('/') {
            return Err(IosError::conformance(format!(
                "{label} regular file ends with '/': {raw_name}"
            )));
        }
        raw_name
    };
    let components = Path::new(name).components().collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > MAX_TREE_DEPTH
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(IosError::conformance(format!(
            "{label} contains an unsafe archive path: {raw_name}"
        )));
    }
    if name == "__MACOSX"
        || name.starts_with("__MACOSX/")
        || components
            .iter()
            .any(|component| component.as_os_str().to_string_lossy().starts_with("._"))
    {
        return Err(IosError::conformance(format!(
            "{label} contains AppleDouble metadata: {raw_name}"
        )));
    }
    let prefix = format!("{expected_root}/");
    if name != expected_root && !name.starts_with(&prefix) {
        return Err(IosError::conformance(format!(
            "{label} contains an unexpected top-level path: {raw_name}"
        )));
    }
    Ok(name.to_owned())
}

fn validate_xcframework(root: &Path) -> Result<(), IosError> {
    let expected_root = BTreeSet::from([
        "Info.plist".to_owned(),
        "ios-arm64".to_owned(),
        "ios-arm64-simulator".to_owned(),
    ]);
    if directory_entry_names(root, "XCFramework root")? != expected_root {
        return Err(IosError::conformance(
            "VesperPlayerKit XCFramework has unexpected top-level payload",
        ));
    }
    let info = root.join("Info.plist");
    let object = ios::read_plist_dictionary(&info, "XCFramework")?;
    for (key, expected) in [
        ("CFBundlePackageType", "XFWK"),
        ("XCFrameworkFormatVersion", "1.0"),
    ] {
        if object.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
            return Err(IosError::conformance(format!(
                "Unexpected XCFramework {key}: {}",
                info.display()
            )));
        }
    }
    let libraries = object
        .get("AvailableLibraries")
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() == 2)
        .ok_or_else(|| {
            IosError::conformance("XCFramework manifest must declare exactly two libraries")
        })?;
    let mut records = BTreeMap::new();
    for library in libraries {
        let record = library.as_object().ok_or_else(|| {
            IosError::conformance("XCFramework library record is not a dictionary")
        })?;
        let identifier = record
            .get("LibraryIdentifier")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| IosError::conformance("XCFramework library omits its identifier"))?;
        if records.insert(identifier, record).is_some() {
            return Err(IosError::conformance(format!(
                "XCFramework repeats library identifier: {identifier}"
            )));
        }
    }
    for (identifier, variant) in [
        ("ios-arm64", None),
        ("ios-arm64-simulator", Some("simulator")),
    ] {
        let record = records.get(identifier).ok_or_else(|| {
            IosError::conformance(format!("XCFramework omits library: {identifier}"))
        })?;
        for (key, expected) in [
            ("LibraryPath", FRAMEWORK_ROOT),
            ("BinaryPath", "VesperPlayerKit.framework/VesperPlayerKit"),
            ("SupportedPlatform", "ios"),
        ] {
            if record.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
                return Err(IosError::conformance(format!(
                    "Unexpected XCFramework {key} for {identifier}"
                )));
            }
        }
        let architectures = record
            .get("SupportedArchitectures")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "XCFramework {identifier} omits SupportedArchitectures"
                ))
            })?;
        if architectures.as_slice() != [serde_json::Value::String("arm64".to_owned())] {
            return Err(IosError::conformance(format!(
                "Unexpected XCFramework architectures for {identifier}"
            )));
        }
        if record
            .get("SupportedPlatformVariant")
            .and_then(serde_json::Value::as_str)
            != variant
        {
            return Err(IosError::conformance(format!(
                "Unexpected XCFramework platform variant for {identifier}"
            )));
        }
        let slice = root.join(identifier);
        if directory_entry_names(&slice, "XCFramework slice")?
            != BTreeSet::from([FRAMEWORK_ROOT.to_owned()])
        {
            return Err(IosError::conformance(format!(
                "Unexpected XCFramework slice payload for {identifier}"
            )));
        }
    }
    Ok(())
}

fn validate_framework(
    framework: &Path,
    slice: SliceSpec,
    expected: &CoreMetadata,
) -> Result<(), IosError> {
    require_directory(framework, "framework")?;
    validate_framework_tree(framework)?;
    let info = framework.join("Info.plist");
    let binary = framework.join(FRAMEWORK_NAME);
    require_file(&info, MAX_SMALL_FILE_BYTES, "framework Info.plist")?;
    require_file(&binary, MAX_BINARY_BYTES, "framework binary")?;
    let object = ios::read_plist_dictionary(&info, "framework")?;
    for (key, value) in [
        ("CFBundlePackageType", "FMWK"),
        ("CFBundleExecutable", FRAMEWORK_NAME),
        ("CFBundleIdentifier", BUNDLE_IDENTIFIER),
        ("CFBundleShortVersionString", expected.version.as_str()),
        ("CFBundleVersion", expected.build.as_str()),
        ("MinimumOSVersion", expected.minimum_os.as_str()),
        ("DTPlatformName", slice.dt_platform),
    ] {
        if ios::required_plist_string(&object, key, &info)? != value {
            return Err(IosError::conformance(format!(
                "Unexpected VesperPlayerKit {key}: {}",
                framework.display()
            )));
        }
    }
    let _ = ios::required_plist_string(&object, "CFBundleName", &info)?;
    let platforms = object
        .get("CFBundleSupportedPlatforms")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            IosError::conformance(format!(
                "framework omits CFBundleSupportedPlatforms: {}",
                info.display()
            ))
        })?;
    if platforms.as_slice()
        != [serde_json::Value::String(
            slice.supported_platform.to_owned(),
        )]
    {
        return Err(IosError::conformance(format!(
            "VesperPlayerKit must declare exactly one supported platform ({})",
            slice.supported_platform
        )));
    }

    ios::validate_foundation_bundle(framework, FRAMEWORK_NAME, BUNDLE_IDENTIFIER)?;
    ios::validate_framework_install_name(&binary, FRAMEWORK_NAME)?;
    ios::validate_framework_architecture(&binary, FRAMEWORK_NAME)?;
    let dependencies = ios::read_macho_dependencies(&binary)?;
    ios::validate_framework_dependency_list(
        &binary.display().to_string(),
        FRAMEWORK_NAME,
        &dependencies,
    )?;
    validate_macho_platform(&binary, slice, &expected.minimum_os)?;
    validate_binary_markers(&binary)?;
    validate_interfaces(framework, slice.interface_target)
}

fn validate_framework_tree(framework: &Path) -> Result<(), IosError> {
    if fs::symlink_metadata(framework.join("Resources")).is_ok() {
        return Err(IosError::conformance(format!(
            "iOS shallow frameworks must keep resources at the framework root: {}",
            framework.display()
        )));
    }
    let mut pending = VecDeque::from([(framework.to_path_buf(), 0_usize)]);
    let mut entries = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_TREE_DEPTH {
            return Err(IosError::conformance(
                "framework tree exceeds its depth limit",
            ));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to enumerate framework '{}': {error}",
                directory.display()
            ))
        })? {
            entries += 1;
            if entries > MAX_TREE_ENTRIES {
                return Err(IosError::conformance(
                    "framework tree exceeds its entry limit",
                ));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!("failed to inspect framework entry: {error}"))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect framework entry '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "framework contains a symlink: {}",
                    path.display()
                )));
            }
            let relative = path.strip_prefix(framework).map_err(|error| {
                IosError::worker(format!("failed to relativize framework entry: {error}"))
            })?;
            let relative_text = relative.to_string_lossy();
            if is_fixture_path(&relative_text) {
                return Err(IosError::conformance(format!(
                    "framework contains test fixture resources: {}",
                    path.display()
                )));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if metadata.file_type().is_dir() {
                if name == "Frameworks" || name.ends_with(".framework") {
                    return Err(IosError::conformance(format!(
                        "iOS framework contains nested code: {}",
                        path.display()
                    )));
                }
                pending.push_back((path, depth + 1));
            } else if !metadata.file_type().is_file() {
                return Err(IosError::conformance(format!(
                    "framework contains an unsupported file type: {}",
                    path.display()
                )));
            } else if name.contains(".dylib") {
                return Err(IosError::conformance(format!(
                    "iOS framework contains a standalone dylib: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn validate_macho_platform(
    binary: &Path,
    slice: SliceSpec,
    expected_minimum_os: &str,
) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("XCRUN", "xcrun"));
    command
        .args(["vtool", "-show-build"])
        .arg(binary)
        .stdin(Stdio::null());
    let output = run_tool(&mut command, "framework Mach-O platform inspection")?;
    let output = std::str::from_utf8(&output).map_err(|error| {
        IosError::conformance(format!("xcrun vtool output is not UTF-8: {error}"))
    })?;
    let metadata = ios::parse_vtool_build_metadata(output, &binary.display().to_string())?;
    if metadata.platform != slice.macho_platform || metadata.minimum_os != expected_minimum_os {
        return Err(IosError::conformance(format!(
            "Unexpected VesperPlayerKit Mach-O platform metadata: {}\n  platform: {} (expected {})\n  minimum OS: {} (expected {})",
            binary.display(),
            metadata.platform,
            slice.macho_platform,
            metadata.minimum_os,
            expected_minimum_os
        )));
    }
    Ok(())
}

fn validate_interfaces(framework: &Path, target: &str) -> Result<(), IosError> {
    let module = framework
        .join("Modules")
        .join("VesperPlayerKit.swiftmodule");
    for suffix in ["swiftinterface", "private.swiftinterface"] {
        let path = module.join(format!("{target}.{suffix}"));
        let bytes = read_bounded_file(&path, MAX_SMALL_FILE_BYTES, "Swift textual interface")?;
        let source = std::str::from_utf8(&bytes).map_err(|error| {
            IosError::conformance(format!(
                "VesperPlayerKit textual interface is not UTF-8 '{}': {error}",
                path.display()
            ))
        })?;
        for private in [
            "VesperPlayerKitBridgeShim",
            "PlayerFfi",
            "VesperRuntime",
            "vesper_",
        ] {
            if source.contains(private) {
                return Err(IosError::conformance(format!(
                    "VesperPlayerKit textual interface leaks private bridge declarations: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn validate_binary_markers(binary: &Path) -> Result<(), IosError> {
    let bytes = read_bounded_file(binary, MAX_BINARY_BYTES, "framework binary")?;
    let lower = bytes.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
    for marker in [
        b"assets/subtitle_contract".as_slice(),
        b"fixtures/contracts",
        b"fixtures/media",
        b"tiny-aac.m4a",
        b"tiny-h264-aac.m4v",
        b"tiny-h264-aac-mediacodec.m4v",
    ] {
        if lower
            .windows(marker.len())
            .any(|candidate| candidate == marker)
        {
            return Err(IosError::conformance(format!(
                "Release binary contains test fixture markers: {}",
                binary.display()
            )));
        }
    }
    Ok(())
}

fn verify_consumer(
    framework: &Path,
    slice: SliceSpec,
    metadata: &CoreMetadata,
    temporary: &TempDir,
    label: &str,
) -> Result<(), IosError> {
    let root = temporary.path().join(format!("consumer {label}"));
    let copied_framework = root.join(FRAMEWORK_ROOT);
    let module_cache = root.join("module cache");
    fs::create_dir_all(&module_cache).map_err(|error| {
        IosError::storage(format!("failed to create consumer workspace: {error}"))
    })?;
    copy_tree(framework, &copied_framework)?;
    remove_binary_swiftmodules(&copied_framework.join("Modules"))?;
    let source = root.join("Consumer.swift");
    fs::write(
        &source,
        "import VesperPlayerKit\n\n@MainActor\npublic func makePlayer() {\n    _ = VesperPlayerControllerFactory.makeDefault()\n}\n",
    )
    .map_err(|error| IosError::storage(format!("failed to write consumer probe: {error}")))?;
    let binary = root.join("libVesperPlayerKitConsumer.dylib");

    let mut sdk = Command::new(configured_tool("XCRUN", "xcrun"));
    sdk.args(["--sdk", slice.sdk, "--show-sdk-path"])
        .stdin(Stdio::null());
    let sdk_path = run_tool(&mut sdk, "Apple SDK path lookup")?;
    let sdk_path = std::str::from_utf8(&sdk_path)
        .map_err(|error| IosError::compatibility(format!("SDK path is not UTF-8: {error}")))?
        .trim();
    if sdk_path.is_empty() {
        return Err(IosError::compatibility(format!(
            "xcrun returned no SDK path for {}",
            slice.sdk
        )));
    }
    let target = format!(
        "arm64-apple-ios{}{}",
        metadata.minimum_os, slice.clang_target_suffix
    );
    let mut swiftc = Command::new(configured_tool("XCRUN", "xcrun"));
    swiftc
        .args(["--sdk", slice.sdk, "swiftc", "-swift-version", "5"])
        .arg("-target")
        .arg(target)
        .arg("-sdk")
        .arg(sdk_path)
        .arg("-module-cache-path")
        .arg(&module_cache)
        .arg("-F")
        .arg(&root)
        .args(["-framework", FRAMEWORK_NAME, "-emit-library"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .stdin(Stdio::null());
    let _ = run_tool(
        &mut swiftc,
        &format!("VesperPlayerKit consumer smoke ({label})"),
    )?;
    let dependencies = ios::read_macho_dependencies(&binary)?;
    let expected = "@rpath/VesperPlayerKit.framework/VesperPlayerKit";
    if !dependencies.iter().any(|dependency| dependency == expected) {
        return Err(IosError::conformance(format!(
            "Consumer smoke did not link VesperPlayerKit: {label}"
        )));
    }
    if dependencies
        .iter()
        .any(|dependency| dependency.contains("VesperPlayerKitBridgeShim"))
    {
        return Err(IosError::conformance(format!(
            "Consumer smoke links the private BridgeShim module: {label}"
        )));
    }
    Ok(())
}

fn compare_trees(left: &Path, right: &Path, label: &str) -> Result<(), IosError> {
    let left_nodes = collect_tree_nodes(left)?;
    let right_nodes = collect_tree_nodes(right)?;
    if left_nodes != right_nodes {
        return Err(IosError::conformance(format!(
            "Standalone {label} framework differs from the XCFramework {label} slice"
        )));
    }
    for (relative, is_directory) in left_nodes {
        if !is_directory {
            let left_bytes = read_bounded_file(
                &left.join(&relative),
                MAX_ARCHIVE_ENTRY_BYTES,
                "standalone framework file",
            )?;
            let right_bytes = read_bounded_file(
                &right.join(&relative),
                MAX_ARCHIVE_ENTRY_BYTES,
                "XCFramework slice file",
            )?;
            if left_bytes != right_bytes {
                return Err(IosError::conformance(format!(
                    "Standalone {label} framework differs from its XCFramework slice: {}",
                    relative.display()
                )));
            }
        }
    }
    Ok(())
}

fn collect_tree_nodes(root: &Path) -> Result<BTreeMap<PathBuf, bool>, IosError> {
    let mut nodes = BTreeMap::new();
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_TREE_DEPTH {
            return Err(IosError::conformance(
                "framework comparison exceeds its depth limit",
            ));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to compare '{}': {error}",
                directory.display()
            ))
        })? {
            if nodes.len() >= MAX_TREE_ENTRIES {
                return Err(IosError::conformance(
                    "framework comparison exceeds its entry limit",
                ));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to read framework comparison entry: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!("failed to inspect '{}': {error}", path.display()))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "framework comparison encountered a symlink: {}",
                    path.display()
                )));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|error| IosError::worker(format!("failed to compare paths: {error}")))?
                .to_path_buf();
            let is_directory = metadata.file_type().is_dir();
            if !is_directory && !metadata.file_type().is_file() {
                return Err(IosError::conformance(format!(
                    "framework comparison encountered an unsupported file: {}",
                    path.display()
                )));
            }
            nodes.insert(relative, is_directory);
            if is_directory {
                pending.push_back((path, depth + 1));
            }
        }
    }
    Ok(nodes)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), IosError> {
    fs::create_dir(destination).map_err(|error| {
        IosError::storage(format!("failed to create consumer framework copy: {error}"))
    })?;
    let mut pending = VecDeque::from([(source.to_path_buf(), destination.to_path_buf(), 0_usize)]);
    let mut count = 0_usize;
    while let Some((source_directory, destination_directory, depth)) = pending.pop_front() {
        if depth > MAX_TREE_DEPTH {
            return Err(IosError::conformance(
                "framework copy exceeds its depth limit",
            ));
        }
        for entry in fs::read_dir(&source_directory)
            .map_err(|error| IosError::storage(format!("failed to copy framework tree: {error}")))?
        {
            count += 1;
            if count > MAX_TREE_ENTRIES {
                return Err(IosError::conformance(
                    "framework copy exceeds its entry limit",
                ));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!("failed to read framework copy entry: {error}"))
            })?;
            let source_path = entry.path();
            let destination_path = destination_directory.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
                IosError::storage(format!("failed to inspect framework copy entry: {error}"))
            })?;
            if metadata.file_type().is_dir() {
                fs::create_dir(&destination_path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to create framework copy directory: {error}"
                    ))
                })?;
                pending.push_back((source_path, destination_path, depth + 1));
            } else if metadata.file_type().is_file() {
                fs::copy(&source_path, &destination_path).map_err(|error| {
                    IosError::storage(format!("failed to copy framework file: {error}"))
                })?;
                fs::set_permissions(&destination_path, metadata.permissions()).map_err(
                    |error| {
                        IosError::storage(format!(
                            "failed to preserve framework permissions: {error}"
                        ))
                    },
                )?;
            } else {
                return Err(IosError::conformance(format!(
                    "framework copy encountered an unsupported file: {}",
                    source_path.display()
                )));
            }
        }
    }
    Ok(())
}

fn remove_binary_swiftmodules(root: &Path) -> Result<(), IosError> {
    if !root.is_dir() {
        return Ok(());
    }
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut count = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_TREE_DEPTH {
            return Err(IosError::conformance(
                "Swift module tree exceeds its depth limit",
            ));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!("failed to enumerate Swift modules: {error}"))
        })? {
            count += 1;
            if count > MAX_TREE_ENTRIES {
                return Err(IosError::conformance(
                    "Swift module tree exceeds its entry limit",
                ));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!("failed to inspect Swift module: {error}"))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!("failed to inspect Swift module path: {error}"))
            })?;
            if metadata.file_type().is_dir() {
                pending.push_back((path, depth + 1));
            } else if metadata.file_type().is_file()
                && path.extension() == Some(OsStr::new("swiftmodule"))
            {
                fs::remove_file(&path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to remove binary Swift module '{}': {error}",
                        path.display()
                    ))
                })?;
            }
        }
    }
    Ok(())
}

fn directory_entry_names(directory: &Path, label: &str) -> Result<BTreeSet<String>, IosError> {
    fs::read_dir(directory)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to enumerate {label} '{}': {error}",
                directory.display()
            ))
        })?
        .map(|entry| {
            entry
                .map_err(|error| IosError::storage(format!("failed to read {label}: {error}")))?
                .file_name()
                .into_string()
                .map_err(|_| IosError::conformance(format!("{label} contains a non-UTF-8 name")))
        })
        .collect()
}

fn require_directory(path: &Path, label: &str) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::conformance(format!("Missing {label} '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::conformance(format!(
            "{label} must be a regular non-symlink directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn require_file(path: &Path, maximum_bytes: u64, label: &str) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::conformance(format!("Missing {label} '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > maximum_bytes {
        return Err(IosError::conformance(format!(
            "{label} must be a regular file no larger than {maximum_bytes} bytes: {}",
            path.display()
        )));
    }
    Ok(())
}

fn read_bounded_file(path: &Path, maximum_bytes: u64, label: &str) -> Result<Vec<u8>, IosError> {
    require_file(path, maximum_bytes, label)?;
    fs::read(path).map_err(|error| {
        IosError::storage(format!(
            "failed to read {label} '{}': {error}",
            path.display()
        ))
    })
}

fn is_fixture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let components = lower.split('/').collect::<Vec<_>>();
    let fixture_component = |component: &str| {
        matches!(
            component,
            "subtitle_contract"
                | "testfixture"
                | "testfixtures"
                | "test-fixture"
                | "test-fixtures"
                | "test_fixture"
                | "test_fixtures"
                | "testasset"
                | "testassets"
                | "test-asset"
                | "test-assets"
                | "test_asset"
                | "test_assets"
                | "testdata"
        )
    };
    if components
        .iter()
        .any(|component| fixture_component(component))
    {
        return true;
    }
    if components
        .windows(2)
        .any(|parts| parts[0] == "fixtures" && matches!(parts[1], "contracts" | "media"))
    {
        return true;
    }
    components.last().is_some_and(|name| {
        matches!(
            *name,
            "tiny-aac.m4a" | "tiny-h264-aac.m4v" | "tiny-h264-aac-mediacodec.m4v"
        )
    })
}

fn configured_tool(variable: &str, default: &str) -> std::ffi::OsString {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.into())
}

fn run_tool(command: &mut Command, label: &str) -> Result<Vec<u8>, IosError> {
    let result = external_process::run_interruptible_capture(
        command,
        label,
        MAX_TOOL_OUTPUT_BYTES,
        MAX_TOOL_OUTPUT_BYTES,
    )
    .map_err(|error| match error.kind() {
        ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            IosError::worker(error.to_string())
        }
    })?;
    if !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let diagnostic = [stdout.trim_end(), stderr.trim_end()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(IosError::conformance(format!(
            "{label} exited unsuccessfully ({}){}{}",
            result.status,
            if diagnostic.is_empty() { "" } else { ":\n" },
            diagnostic
        )));
    }
    Ok(result.stdout)
}
