use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ios::IosError;

const EXPECTED_ACCEPTANCE_TESTS: u64 = 3;

pub(crate) struct IosOptionalPluginDeviceRequest {
    pub(crate) release_directory: PathBuf,
    pub(crate) device: String,
    pub(crate) development_team: String,
    pub(crate) output_directory: PathBuf,
    pub(crate) allow_provisioning_updates: bool,
}

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(
            "iOS optional-plugin device verification requires macOS",
        ))
    }
}

pub(crate) fn verify(
    root: &Path,
    request: IosOptionalPluginDeviceRequest,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::verify(root, request, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, request, output, diagnostics);
        unreachable!("the host gate rejects non-macOS verification")
    }
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};
    use std::time::Duration;

    use nix::unistd::{AccessFlags, access};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use super::{EXPECTED_ACCEPTANCE_TESTS, IosError, IosOptionalPluginDeviceRequest};
    use crate::external_process::{self, ExternalProcessErrorKind};
    use crate::ios_optional_release::OptionalReleaseArchiveEvidence;

    const MAX_PROCESS_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
    const MAX_XCODEGEN_SPEC_BYTES: usize = 4 * 1024 * 1024;
    const MAX_XCRESULT_SUMMARY_BYTES: usize = 1024 * 1024;
    const MAX_XCRESULT_TESTS_BYTES: usize = 4 * 1024 * 1024;
    const MAX_XCRESULT_TEST_NODES: usize = 4096;
    const MAX_XCODE_PRODUCT_ENTRIES: usize = 4096;
    const MAX_DEVICE_IDENTIFIER_BYTES: usize = 256;
    const MAX_TEAM_IDENTIFIER_BYTES: usize = 64;
    const DEVICE_XCTEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
    const TEST_CLASS: &str = "VesperPlayerHostDemoTests/VesperOptionalPluginDeviceAcceptanceTests";
    const TEST_CLASS_NAME: &str = "VesperOptionalPluginDeviceAcceptanceTests";

    #[derive(Debug)]
    struct RequiredTools {
        xcodegen: PathBuf,
        xcodebuild: PathBuf,
        xcrun: PathBuf,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct XcresultSummary {
        result: String,
        total_test_count: u64,
        passed_tests: u64,
        failed_tests: u64,
        skipped_tests: u64,
        expected_failures: u64,
    }

    #[derive(Debug, Deserialize)]
    struct XcresultTests {
        #[serde(default, rename = "testNodes")]
        test_nodes: Vec<XcresultTestNode>,
    }

    #[derive(Debug, Deserialize)]
    struct XcresultTestNode {
        #[serde(default)]
        children: Vec<XcresultTestNode>,
        name: String,
        #[serde(rename = "nodeType")]
        node_type: String,
    }

    #[derive(Serialize)]
    struct VerifiedReleaseProvenance<'a> {
        schema_version: u32,
        canonical_release_directory: &'a str,
        archives: &'a [OptionalReleaseArchiveEvidence],
    }

    pub(super) fn verify(
        root: &Path,
        request: IosOptionalPluginDeviceRequest,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        validate_identifier(
            &request.device,
            MAX_DEVICE_IDENTIFIER_BYTES,
            "iOS device identifier",
        )?;
        validate_team_identifier(&request.development_team)?;
        let tools = resolve_required_tools()?;
        let release_directory = require_regular_directory(
            &request.release_directory,
            "complete iOS release directory",
        )?;
        let example_project_directory = require_regular_directory(
            &root.join("examples/ios-swift-host"),
            "iOS example project directory",
        )?;
        let project_manifest = require_regular_file(
            &example_project_directory.join("project.yml"),
            "iOS example XcodeGen manifest",
        )?;
        let player_kit = require_regular_directory(
            &root.join("lib/ios/VesperPlayerKit"),
            "VesperPlayerKit package directory",
        )?;
        let verified = crate::ios_optional_release::prepare_verified_optional_release_snapshot(
            root,
            &release_directory,
        )?;
        let release_directory = verified.canonical_release_directory().to_path_buf();
        let output_directory = create_output_directory(&request.output_directory)?;
        let optional_package = output_directory.join("VerifiedOptionalPluginsPackage");
        verified.materialize_optional_package(&optional_package)?;
        let acceptance_project_directory = output_directory.join("Project");
        fs::create_dir(&acceptance_project_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create isolated iOS optional-plugin project directory '{}': {error}",
                acceptance_project_directory.display()
            ))
        })?;

        let device_spec = acceptance_project_directory.join("device-project.json");
        let resolved_spec = dump_project_spec(
            &tools.xcodegen,
            &project_manifest,
            &example_project_directory,
            diagnostics,
        )?;
        write_device_project_spec(
            &device_spec,
            resolved_spec,
            &player_kit,
            &optional_package,
            verified.optional_frameworks(),
        )?;
        let provenance = output_directory.join("verified-release-inputs.json");
        write_verified_release_provenance(
            &provenance,
            &release_directory,
            verified.archive_evidence(),
        )?;
        let derived_data = output_directory.join("DerivedData");
        let result_bundle = output_directory.join("VesperOptionalPlugins.xcresult");

        let mut generate = Command::new(&tools.xcodegen);
        generate
            .arg("generate")
            .arg("--spec")
            .arg(&device_spec)
            .arg("--project")
            .arg(&acceptance_project_directory)
            .arg("--project-root")
            .arg(&example_project_directory)
            .arg("--no-env")
            .current_dir(&acceptance_project_directory)
            .stdin(Stdio::null());
        run_captured(
            &mut generate,
            "iOS optional-plugin Xcode project generation",
            diagnostics,
        )?;

        let project = require_regular_directory(
            &acceptance_project_directory.join("VesperPlayerHostDemo.xcodeproj"),
            "generated iOS example Xcode project",
        )?;
        let destination = format!("platform=iOS,id={}", request.device);
        let mut build = Command::new(&tools.xcodebuild);
        build
            .arg("build-for-testing")
            .arg("-project")
            .arg(&project)
            .arg("-scheme")
            .arg("VesperPlayerHostDemo")
            .arg("-configuration")
            .arg("Release")
            .arg("-destination")
            .arg(&destination)
            .arg("-destination-timeout")
            .arg("120")
            .arg("-derivedDataPath")
            .arg(&derived_data)
            .arg(format!("DEVELOPMENT_TEAM={}", request.development_team))
            .arg("CODE_SIGN_STYLE=Automatic")
            .arg("ENABLE_TESTABILITY=YES")
            .arg("ARCHS=arm64")
            .arg("ONLY_ACTIVE_ARCH=YES")
            .arg("-parallel-testing-enabled")
            .arg("NO")
            .arg(format!("-only-testing:{TEST_CLASS}"))
            .stdin(Stdio::null());
        if request.allow_provisioning_updates {
            build.arg("-allowProvisioningUpdates");
        }
        run_captured(
            &mut build,
            "iOS optional-plugin Release build-for-testing",
            diagnostics,
        )?;

        let products = require_regular_directory(
            &derived_data.join("Build/Products"),
            "iOS optional-plugin Xcode build products",
        )?;
        let application = require_regular_directory(
            &products.join("Release-iphoneos/VesperPlayerHostDemo.app"),
            "iOS optional-plugin Release application",
        )?;
        let xctestrun = discover_xctestrun(&products)?;
        let profile_hash =
            crate::ios::verify_embedded_optional_framework_contract(&application, true)?;

        let mut test = Command::new(&tools.xcodebuild);
        test.arg("test-without-building")
            .arg("-xctestrun")
            .arg(&xctestrun)
            .arg("-destination")
            .arg(&destination)
            .arg("-destination-timeout")
            .arg("120")
            .arg("-resultBundlePath")
            .arg(&result_bundle)
            .arg("-parallel-testing-enabled")
            .arg("NO")
            .arg(format!("-only-testing:{TEST_CLASS}"))
            .stdin(Stdio::null());
        if request.allow_provisioning_updates {
            test.arg("-allowProvisioningUpdates");
        }
        let test_status = run_captured_status_with_timeout(
            &mut test,
            "iOS optional-plugin Release device XCTest",
            diagnostics,
            DEVICE_XCTEST_TIMEOUT,
        )?;
        let result_bundle = match require_regular_directory(
            &result_bundle,
            "iOS optional-plugin XCResult bundle",
        ) {
            Ok(result_bundle) => result_bundle,
            Err(_) if !test_status.success() => {
                return classify_status(test_status, "iOS optional-plugin Release device XCTest");
            }
            Err(error) => return Err(error),
        };

        let tests = read_xcresult_tests(&tools.xcrun, &result_bundle, diagnostics)?;
        validate_acceptance_test_tree(&tests, test_status.success())?;
        let summary = read_xcresult_summary(&tools.xcrun, &result_bundle, diagnostics)?;
        validate_summary(&summary)?;
        classify_status(test_status, "iOS optional-plugin Release device XCTest")?;
        writeln!(
            output,
            "Verified {EXPECTED_ACCEPTANCE_TESTS} iOS optional-plugin Release tests on device {} (0 failed, 0 skipped).",
            request.device
        )
        .and_then(|_| writeln!(output, "Release directory: {}", release_directory.display()))
        .and_then(|_| writeln!(output, "Verified application: {}", application.display()))
        .and_then(|_| writeln!(output, "FFmpeg profile hash: {profile_hash}"))
        .and_then(|_| writeln!(output, "Provenance: {}", provenance.display()))
        .and_then(|_| writeln!(output, "XCResult: {}", result_bundle.display()))
        .and_then(|_| {
            for artifact in verified.archive_evidence() {
                writeln!(
                    output,
                    "Release artifact SHA-256: {} {}",
                    artifact.archive_name, artifact.release_archive_sha256
                )?;
                writeln!(
                    output,
                    "Tested artifact SHA-256: {} {}",
                    artifact.archive_name, artifact.verified_archive_sha256
                )?;
            }
            Ok(())
        })
        .map_err(|error| output_error("write verification result", error))
    }

    fn discover_xctestrun(products: &Path) -> Result<PathBuf, IosError> {
        let entries = fs::read_dir(products).map_err(|error| {
            IosError::storage(format!(
                "failed to scan iOS optional-plugin Xcode products '{}': {error}",
                products.display()
            ))
        })?;
        let mut matches = Vec::new();
        let mut count = 0_usize;
        for entry in entries {
            count = count.checked_add(1).ok_or_else(|| {
                IosError::conformance("iOS optional-plugin Xcode product count overflowed")
            })?;
            if count > MAX_XCODE_PRODUCT_ENTRIES {
                return Err(IosError::conformance(format!(
                    "iOS optional-plugin Xcode products contain more than {MAX_XCODE_PRODUCT_ENTRIES} entries"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect an iOS optional-plugin Xcode product: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS optional-plugin Xcode product '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_file() && path.extension() == Some(OsStr::new("xctestrun")) {
                matches.push(path);
            }
        }
        match matches.as_slice() {
            [path] => Ok(path.clone()),
            _ => Err(IosError::conformance(format!(
                "iOS optional-plugin device verification requires exactly one xctestrun file, found {} under '{}'",
                matches.len(),
                products.display()
            ))),
        }
    }

    fn write_device_project_spec(
        path: &Path,
        mut resolved_spec: Value,
        player_kit: &Path,
        optional_package: &Path,
        frameworks: &'static [&'static str],
    ) -> Result<(), IosError> {
        let player_kit = require_regular_directory(player_kit, "VesperPlayerKit package")?;
        let player_kit = player_kit.to_str().ok_or_else(|| {
            IosError::storage(format!(
                "VesperPlayerKit package path is not valid UTF-8: {}",
                player_kit.display()
            ))
        })?;
        let optional_package =
            require_regular_directory(optional_package, "verified optional-plugin Swift package")?;
        let optional_artifacts = require_regular_directory(
            &optional_package.join("Artifacts"),
            "verified optional-plugin artifact directory",
        )?;
        let root = resolved_spec
            .as_object_mut()
            .ok_or_else(|| IosError::conformance("XcodeGen project dump is not an object"))?;
        let packages = root
            .get_mut("packages")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| IosError::conformance("XcodeGen project dump has no packages object"))?;
        let player_kit_package = packages
            .get_mut("VesperPlayerKit")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                IosError::conformance(
                    "XcodeGen project dump has no VesperPlayerKit package declaration",
                )
            })?;
        if player_kit_package.len() != 1
            || !player_kit_package.get("path").is_some_and(Value::is_string)
        {
            return Err(IosError::conformance(
                "canonical VesperPlayerKit package declaration must contain only a local path",
            ));
        }
        player_kit_package.insert("path".to_owned(), Value::String(player_kit.to_owned()));
        if packages.contains_key("VesperPlayerOptionalPlugins") {
            return Err(IosError::conformance(
                "canonical iOS example already declares VesperPlayerOptionalPlugins",
            ));
        }
        let targets = root
            .get_mut("targets")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| IosError::conformance("XcodeGen project dump has no targets object"))?;
        let application = targets
            .get_mut("VesperPlayerHostDemo")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                IosError::conformance(
                    "XcodeGen project dump has no VesperPlayerHostDemo application target",
                )
            })?;
        let dependencies = application
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                IosError::conformance(
                    "XcodeGen project dump has no VesperPlayerHostDemo dependency list",
                )
            })?;
        let mut replaced = BTreeSet::new();
        for dependency in dependencies {
            let Some(object) = dependency.as_object() else {
                continue;
            };
            if object
                .get("package")
                .and_then(Value::as_str)
                .is_some_and(|package| package == "VesperPlayerOptionalPlugins")
            {
                return Err(IosError::conformance(
                    "canonical iOS example contains an unexpected optional-plugin package dependency",
                ));
            }
            let Some(framework_path) = object.get("framework").and_then(Value::as_str) else {
                continue;
            };
            let matched = frameworks.iter().copied().find(|framework| {
                framework_path
                    == format!(
                        "../../lib/ios/VesperPlayerOptionalPlugins/Artifacts/{framework}.xcframework"
                    )
            });
            if let Some(framework) = matched {
                if !replaced.insert(framework) {
                    return Err(IosError::conformance(format!(
                        "canonical iOS example declares {framework} more than once"
                    )));
                }
                let verified_framework = require_regular_directory(
                    &optional_artifacts.join(format!("{framework}.xcframework")),
                    &format!("verified {framework} XCFramework"),
                )?;
                let verified_framework = verified_framework.to_str().ok_or_else(|| {
                    IosError::storage(format!(
                        "verified {framework} XCFramework path is not valid UTF-8: {}",
                        verified_framework.display()
                    ))
                })?;
                *dependency = serde_json::json!({
                    "framework": verified_framework,
                    "embed": true,
                    "link": false,
                    "codeSign": true,
                });
            } else if framework_path.contains("VesperPlayerOptionalPlugins/Artifacts") {
                return Err(IosError::conformance(format!(
                    "canonical iOS example contains an unknown optional-plugin artifact dependency: {framework_path}"
                )));
            }
        }
        let missing = frameworks
            .iter()
            .copied()
            .filter(|framework| !replaced.contains(framework))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(IosError::conformance(format!(
                "canonical iOS example is missing optional-plugin artifact dependencies: {}",
                missing.join(", ")
            )));
        }

        let project_directory = path.parent().ok_or_else(|| {
            IosError::storage(format!(
                "isolated iOS optional-plugin project spec must have a parent: {}",
                path.display()
            ))
        })?;
        let generated_directory = project_directory.join("Generated");
        fs::create_dir(&generated_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create isolated iOS optional-plugin generated-file directory '{}': {error}",
                generated_directory.display()
            ))
        })?;
        let generated_info = generated_directory.join("Info.plist");
        let generated_info = generated_info.to_str().ok_or_else(|| {
            IosError::storage(format!(
                "isolated iOS optional-plugin Info.plist path is not valid UTF-8: {}",
                generated_info.display()
            ))
        })?;
        let info = application
            .get_mut("info")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                IosError::conformance(
                    "XcodeGen project dump has no VesperPlayerHostDemo Info.plist declaration",
                )
            })?;
        let canonical_info_path = info
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                IosError::conformance(
                    "canonical VesperPlayerHostDemo Info.plist path must be a string",
                )
            })?
            .to_owned();
        exclude_canonical_info_from_sources(application, &canonical_info_path)?;
        let info = application
            .get_mut("info")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                IosError::worker(
                    "VesperPlayerHostDemo Info.plist declaration disappeared during isolation",
                )
            })?;
        info.insert("path".to_owned(), Value::String(generated_info.to_owned()));

        if contains_workspace_optional_artifact_reference(&resolved_spec) {
            return Err(IosError::worker(
                "isolated iOS optional-plugin project still references workspace artifacts",
            ));
        }
        let bytes = serde_json::to_vec_pretty(&resolved_spec).map_err(|error| {
            IosError::worker(format!(
                "failed to encode isolated iOS optional-plugin device project: {error}"
            ))
        })?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create isolated iOS optional-plugin device project '{}': {error}",
                    path.display()
                ))
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to write isolated iOS optional-plugin device project '{}': {error}",
                    path.display()
                ))
            })
    }

    fn exclude_canonical_info_from_sources(
        application: &mut serde_json::Map<String, Value>,
        canonical_info_path: &str,
    ) -> Result<(), IosError> {
        let info_path = Path::new(canonical_info_path);
        if info_path.is_absolute()
            || info_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(IosError::conformance(format!(
                "canonical VesperPlayerHostDemo Info.plist path must be a normalized relative path: {canonical_info_path}"
            )));
        }
        let sources = application
            .get_mut("sources")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                IosError::conformance(
                    "XcodeGen project dump has no VesperPlayerHostDemo source list",
                )
            })?;
        let mut covering_sources = 0_usize;
        for source in sources {
            let source_path = match source {
                Value::String(path) => path.as_str(),
                Value::Object(object) => {
                    object.get("path").and_then(Value::as_str).ok_or_else(|| {
                        IosError::conformance(
                            "canonical VesperPlayerHostDemo source object has no path string",
                        )
                    })?
                }
                _ => {
                    return Err(IosError::conformance(
                        "canonical VesperPlayerHostDemo source entry must be a path or object",
                    ));
                }
            };
            let source_path = Path::new(source_path);
            if source_path.is_absolute()
                || source_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(IosError::conformance(
                    "canonical VesperPlayerHostDemo source paths must be normalized and relative",
                ));
            }
            let Ok(relative_info) = info_path.strip_prefix(source_path) else {
                continue;
            };
            if relative_info.as_os_str().is_empty() {
                return Err(IosError::conformance(
                    "canonical VesperPlayerHostDemo Info.plist must be contained by a source directory",
                ));
            }
            covering_sources = covering_sources.checked_add(1).ok_or_else(|| {
                IosError::worker("VesperPlayerHostDemo source coverage count overflowed")
            })?;
            let relative_info = relative_info.to_str().ok_or_else(|| {
                IosError::conformance(
                    "canonical VesperPlayerHostDemo Info.plist exclusion is not valid UTF-8",
                )
            })?;
            match source {
                Value::String(path) => {
                    let path = path.clone();
                    *source = serde_json::json!({
                        "path": path,
                        "excludes": [relative_info],
                    });
                }
                Value::Object(object) => {
                    let excludes = object
                        .entry("excludes".to_owned())
                        .or_insert_with(|| Value::Array(Vec::new()))
                        .as_array_mut()
                        .ok_or_else(|| {
                            IosError::conformance(
                                "canonical VesperPlayerHostDemo source excludes must be an array",
                            )
                        })?;
                    if !excludes
                        .iter()
                        .any(|exclude| exclude.as_str() == Some(relative_info))
                    {
                        excludes.push(Value::String(relative_info.to_owned()));
                    }
                }
                _ => unreachable!("source entry shape was validated above"),
            }
        }
        if covering_sources != 1 {
            return Err(IosError::conformance(format!(
                "canonical VesperPlayerHostDemo Info.plist must be covered by exactly one source directory, found {covering_sources}"
            )));
        }
        Ok(())
    }

    fn contains_workspace_optional_artifact_reference(value: &Value) -> bool {
        match value {
            Value::String(value) => value.contains("VesperPlayerOptionalPlugins/Artifacts"),
            Value::Array(values) => values
                .iter()
                .any(contains_workspace_optional_artifact_reference),
            Value::Object(values) => values
                .values()
                .any(contains_workspace_optional_artifact_reference),
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    fn dump_project_spec(
        xcodegen: &Path,
        project_manifest: &Path,
        project_root: &Path,
        diagnostics: &mut dyn Write,
    ) -> Result<Value, IosError> {
        let mut command = Command::new(xcodegen);
        command
            .arg("dump")
            .arg("--type")
            .arg("json")
            .arg("--no-env")
            .arg("--spec")
            .arg(project_manifest)
            .arg("--project-root")
            .arg(project_root)
            .current_dir(project_root)
            .stdin(Stdio::null());
        let result = external_process::run_interruptible_capture(
            &mut command,
            "iOS optional-plugin XcodeGen project dump",
            MAX_XCODEGEN_SPEC_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
        )
        .map_err(map_process_error)?;
        diagnostics.write_all(&result.stderr).map_err(|error| {
            output_error("write iOS optional-plugin XcodeGen dump diagnostics", error)
        })?;
        classify_status(result.status, "iOS optional-plugin XcodeGen project dump")?;
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "iOS optional-plugin XcodeGen project dump is invalid JSON: {error}"
            ))
        })
    }

    fn write_verified_release_provenance(
        path: &Path,
        release_directory: &Path,
        archives: &[OptionalReleaseArchiveEvidence],
    ) -> Result<(), IosError> {
        let canonical_release_directory = release_directory.to_str().ok_or_else(|| {
            IosError::storage(format!(
                "canonical iOS release directory is not valid UTF-8: {}",
                release_directory.display()
            ))
        })?;
        let provenance = VerifiedReleaseProvenance {
            schema_version: 1,
            canonical_release_directory,
            archives,
        };
        let bytes = serde_json::to_vec_pretty(&provenance).map_err(|error| {
            IosError::worker(format!(
                "failed to encode verified iOS release provenance: {error}"
            ))
        })?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create verified iOS release provenance '{}': {error}",
                    path.display()
                ))
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to write verified iOS release provenance '{}': {error}",
                    path.display()
                ))
            })
    }

    fn validate_identifier(value: &str, maximum_bytes: usize, label: &str) -> Result<(), IosError> {
        if value.is_empty()
            || value.len() > maximum_bytes
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(IosError::compatibility(format!(
                "{label} must be non-empty, at most {maximum_bytes} bytes, and contain no surrounding whitespace or control characters"
            )));
        }
        Ok(())
    }

    fn validate_team_identifier(value: &str) -> Result<(), IosError> {
        validate_identifier(
            value,
            MAX_TEAM_IDENTIFIER_BYTES,
            "Apple Development Team identifier",
        )?;
        if !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(IosError::compatibility(
                "Apple Development Team identifier must contain only ASCII letters and digits",
            ));
        }
        Ok(())
    }

    fn create_output_directory(path: &Path) -> Result<PathBuf, IosError> {
        if path.as_os_str().is_empty() {
            return Err(IosError::storage(
                "iOS optional-plugin device output directory must not be empty",
            ));
        }
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(IosError::storage(format!(
                    "iOS optional-plugin device output directory already exists: {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect iOS optional-plugin device output directory '{}': {error}",
                    path.display()
                )));
            }
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                IosError::storage(format!(
                    "iOS optional-plugin device output directory must have an existing parent: {}",
                    path.display()
                ))
            })?;
        require_regular_directory(parent, "iOS optional-plugin device output parent")?;
        fs::create_dir(path).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS optional-plugin device output directory '{}': {error}",
                path.display()
            ))
        })?;
        fs::canonicalize(path).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve iOS optional-plugin device output directory '{}': {error}",
                path.display()
            ))
        })
    }

    fn read_xcresult_summary(
        xcrun: &Path,
        result_bundle: &Path,
        diagnostics: &mut dyn Write,
    ) -> Result<XcresultSummary, IosError> {
        let mut command = Command::new(xcrun);
        command
            .args(["xcresulttool", "get", "test-results", "summary", "--path"])
            .arg(result_bundle)
            .stdin(Stdio::null());
        let result = external_process::run_interruptible_capture(
            &mut command,
            "iOS optional-plugin XCResult summary",
            MAX_XCRESULT_SUMMARY_BYTES,
            MAX_XCRESULT_SUMMARY_BYTES,
        )
        .map_err(map_process_error)?;
        diagnostics.write_all(&result.stderr).map_err(|error| {
            output_error("write iOS optional-plugin XCResult diagnostics", error)
        })?;
        classify_status(result.status, "iOS optional-plugin XCResult summary")?;
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "iOS optional-plugin XCResult summary is invalid JSON: {error}"
            ))
        })
    }

    fn read_xcresult_tests(
        xcrun: &Path,
        result_bundle: &Path,
        diagnostics: &mut dyn Write,
    ) -> Result<XcresultTests, IosError> {
        let mut command = Command::new(xcrun);
        command
            .args(["xcresulttool", "get", "test-results", "tests", "--path"])
            .arg(result_bundle)
            .stdin(Stdio::null());
        let result = external_process::run_interruptible_capture(
            &mut command,
            "iOS optional-plugin XCResult test tree",
            MAX_XCRESULT_TESTS_BYTES,
            MAX_XCRESULT_SUMMARY_BYTES,
        )
        .map_err(map_process_error)?;
        diagnostics.write_all(&result.stderr).map_err(|error| {
            output_error("write iOS optional-plugin XCResult diagnostics", error)
        })?;
        classify_status(result.status, "iOS optional-plugin XCResult test tree")?;
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "iOS optional-plugin XCResult test tree is invalid JSON: {error}"
            ))
        })
    }

    fn validate_acceptance_test_tree(
        tests: &XcresultTests,
        xcode_succeeded: bool,
    ) -> Result<(), IosError> {
        let mut pending = tests.test_nodes.iter().collect::<Vec<_>>();
        let mut visited = 0_usize;
        let mut suites = 0_usize;
        let mut acceptance_cases = 0_usize;
        while let Some(node) = pending.pop() {
            visited += 1;
            if visited > MAX_XCRESULT_TEST_NODES {
                return Err(IosError::conformance(format!(
                    "iOS optional-plugin XCResult test tree contains more than {MAX_XCRESULT_TEST_NODES} nodes"
                )));
            }
            if node.node_type == "Test Suite" && node.name == TEST_CLASS_NAME {
                suites += 1;
                acceptance_cases += count_test_cases(&node.children, &mut visited)?;
            } else {
                pending.extend(node.children.iter());
            }
        }

        if suites == 1 && acceptance_cases == EXPECTED_ACCEPTANCE_TESTS as usize {
            return Ok(());
        }
        let message = format!(
            "iOS optional-plugin Release device acceptance did not execute the expected test class: suites={suites}, cases={acceptance_cases}, expectedCases={EXPECTED_ACCEPTANCE_TESTS}"
        );
        if xcode_succeeded {
            Err(IosError::conformance(message))
        } else {
            Err(IosError::worker(message))
        }
    }

    fn count_test_cases(
        children: &[XcresultTestNode],
        visited: &mut usize,
    ) -> Result<usize, IosError> {
        let mut pending = children.iter().collect::<Vec<_>>();
        let mut cases = 0_usize;
        while let Some(node) = pending.pop() {
            *visited += 1;
            if *visited > MAX_XCRESULT_TEST_NODES {
                return Err(IosError::conformance(format!(
                    "iOS optional-plugin XCResult test tree contains more than {MAX_XCRESULT_TEST_NODES} nodes"
                )));
            }
            if node.node_type == "Test Case" {
                cases += 1;
            }
            pending.extend(node.children.iter());
        }
        Ok(cases)
    }

    fn validate_summary(summary: &XcresultSummary) -> Result<(), IosError> {
        if summary.result != "Passed"
            || summary.total_test_count != EXPECTED_ACCEPTANCE_TESTS
            || summary.passed_tests != EXPECTED_ACCEPTANCE_TESTS
            || summary.failed_tests != 0
            || summary.skipped_tests != 0
            || summary.expected_failures != 0
        {
            return Err(IosError::conformance(format!(
                "iOS optional-plugin Release device acceptance requires {EXPECTED_ACCEPTANCE_TESTS} passed, 0 failed, 0 skipped, and 0 expected failures; result={}, total={}, passed={}, failed={}, skipped={}, expectedFailures={}",
                summary.result,
                summary.total_test_count,
                summary.passed_tests,
                summary.failed_tests,
                summary.skipped_tests,
                summary.expected_failures
            )));
        }
        Ok(())
    }

    fn resolve_required_tools() -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            xcodegen: require_path_command("xcodegen")?,
            xcodebuild: require_path_command("xcodebuild")?,
            xcrun: require_path_command("xcrun")?,
        })
    }

    fn require_path_command(name: &str) -> Result<PathBuf, IosError> {
        let paths = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&paths)
            .find_map(|directory| {
                let candidate = directory.join(name);
                fs::metadata(&candidate)
                    .is_ok_and(|metadata| metadata.is_file())
                    .then_some(candidate)
            })
            .filter(|candidate| access(candidate, AccessFlags::X_OK).is_ok())
            .ok_or_else(|| IosError::compatibility(format!("Missing required command: {name}")))
    }

    fn require_regular_file(path: &Path, label: &str) -> Result<PathBuf, IosError> {
        require_regular_path(path, label, false)
    }

    fn require_regular_directory(path: &Path, label: &str) -> Result<PathBuf, IosError> {
        require_regular_path(path, label, true)
    }

    fn require_regular_path(
        path: &Path,
        label: &str,
        directory: bool,
    ) -> Result<PathBuf, IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        let expected = if directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        };
        if !expected {
            return Err(IosError::storage(format!(
                "{label} '{}' must be a regular non-symlink {}",
                path.display(),
                if directory { "directory" } else { "file" }
            )));
        }
        fs::canonicalize(path)
            .map_err(|error| IosError::storage(format!("failed to resolve {label}: {error}")))
    }

    fn run_captured(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let status = run_captured_status(command, label, diagnostics)?;
        classify_status(status, label)
    }

    fn run_captured_status(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
    ) -> Result<ExitStatus, IosError> {
        let result = external_process::run_interruptible_capture(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
        )
        .map_err(map_process_error)?;
        diagnostics
            .write_all(&result.stdout)
            .and_then(|_| diagnostics.write_all(&result.stderr))
            .and_then(|_| diagnostics.flush())
            .map_err(|error| {
                output_error("write iOS optional-plugin process diagnostics", error)
            })?;
        Ok(result.status)
    }

    fn run_captured_status_with_timeout(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        timeout: Duration,
    ) -> Result<ExitStatus, IosError> {
        let result = external_process::run_interruptible_capture_with_timeout(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
            timeout,
        )
        .map_err(map_process_error)?;
        diagnostics
            .write_all(&result.stdout)
            .and_then(|_| diagnostics.write_all(&result.stderr))
            .and_then(|_| diagnostics.flush())
            .map_err(|error| {
                output_error("write iOS optional-plugin process diagnostics", error)
            })?;
        Ok(result.status)
    }

    fn classify_status(status: ExitStatus, label: &str) -> Result<(), IosError> {
        if status.success() {
            Ok(())
        } else {
            Err(IosError::worker(format!(
                "{label} terminated unsuccessfully ({status})"
            )))
        }
    }

    fn map_process_error(error: external_process::ExternalProcessError) -> IosError {
        match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        }
    }

    fn output_error(operation: &str, error: io::Error) -> IosError {
        IosError::storage(format!("failed to {operation}: {error}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn resolved_project_fixture() -> Value {
            let optional_dependencies = crate::ios_optional_release::optional_frameworks()
                .iter()
                .copied()
                .map(|framework| {
                    serde_json::json!({
                        "framework": format!(
                            "../../lib/ios/VesperPlayerOptionalPlugins/Artifacts/{framework}.xcframework"
                        ),
                        "embed": true,
                        "link": false,
                        "codeSign": true,
                    })
                });
            let mut dependencies = vec![serde_json::json!({
                "package": "VesperPlayerKit",
                "product": "VesperPlayerKit",
            })];
            dependencies.extend(optional_dependencies);
            serde_json::json!({
                "packages": {
                    "VesperPlayerKit": { "path": "../../lib/ios/VesperPlayerKit" }
                },
                "targets": {
                    "VesperPlayerHostDemo": {
                        "dependencies": dependencies,
                        "sources": ["Sources"],
                        "info": {
                            "path": "Sources/Info.plist",
                            "properties": { "UIRequiresFullScreen": true }
                        }
                    }
                }
            })
        }

        fn create_verified_package_fixture(path: &Path) {
            fs::create_dir(path).expect("create verified package fixture");
            let artifacts = path.join("Artifacts");
            fs::create_dir(&artifacts).expect("create verified artifacts fixture");
            for framework in crate::ios_optional_release::optional_frameworks() {
                fs::create_dir(artifacts.join(format!("{framework}.xcframework")))
                    .expect("create verified XCFramework fixture");
            }
        }

        fn passing_summary() -> XcresultSummary {
            XcresultSummary {
                result: "Passed".to_owned(),
                total_test_count: EXPECTED_ACCEPTANCE_TESTS,
                passed_tests: EXPECTED_ACCEPTANCE_TESTS,
                failed_tests: 0,
                skipped_tests: 0,
                expected_failures: 0,
            }
        }

        fn acceptance_test_tree(case_count: usize) -> XcresultTests {
            XcresultTests {
                test_nodes: vec![XcresultTestNode {
                    name: TEST_CLASS_NAME.to_owned(),
                    node_type: "Test Suite".to_owned(),
                    children: (0..case_count)
                        .map(|index| XcresultTestNode {
                            name: format!("testAcceptance{index}()"),
                            node_type: "Test Case".to_owned(),
                            children: Vec::new(),
                        })
                        .collect(),
                }],
            }
        }

        fn bootstrap_failure_test_tree() -> XcresultTests {
            XcresultTests {
                test_nodes: vec![XcresultTestNode {
                    name: "System Failures".to_owned(),
                    node_type: "Test Suite".to_owned(),
                    children: vec![XcresultTestNode {
                        name: "VesperPlayerHostDemo encountered an error".to_owned(),
                        node_type: "Test Case".to_owned(),
                        children: Vec::new(),
                    }],
                }],
            }
        }

        #[test]
        fn acceptance_summary_requires_all_three_tests_to_run_and_pass() {
            validate_summary(&passing_summary()).expect("three passing tests are accepted");

            let mut skipped = passing_summary();
            skipped.passed_tests = 2;
            skipped.skipped_tests = 1;
            let error = validate_summary(&skipped).expect_err("a skipped test must fail the gate");
            assert!(error.to_string().contains("2"));
            assert!(error.to_string().contains("skipped=1"));

            let mut incomplete = passing_summary();
            incomplete.total_test_count = 2;
            incomplete.passed_tests = 2;
            let error =
                validate_summary(&incomplete).expect_err("an incomplete test class must fail");
            assert!(error.to_string().contains("total=2"));
        }

        #[test]
        fn acceptance_tree_distinguishes_xcode_system_failures_from_test_cases() {
            validate_acceptance_test_tree(
                &acceptance_test_tree(EXPECTED_ACCEPTANCE_TESTS as usize),
                false,
            )
            .expect("three acceptance cases prove that the test class executed");

            let error = validate_acceptance_test_tree(&bootstrap_failure_test_tree(), false)
                .expect_err("an Xcode system failure must not imply plugin conformance");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
            assert!(error.to_string().contains("suites=0, cases=0"));

            let error = validate_acceptance_test_tree(&acceptance_test_tree(2), true)
                .expect_err("a successful but incomplete test selection is conformance failure");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);
            assert!(error.to_string().contains("suites=1, cases=2"));
        }

        #[test]
        fn ordinary_xcode_failure_is_worker_failure_without_test_evidence() {
            let status = Command::new("/bin/sh")
                .args(["-c", "exit 65"])
                .status()
                .expect("produce an ordinary xcodebuild failure status");
            let error = classify_status(status, "locked-device bootstrap")
                .expect_err("an ordinary xcodebuild failure must not imply conformance");

            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
            assert!(error.to_string().contains("locked-device bootstrap"));
        }

        #[test]
        fn device_and_team_identifiers_reject_ambiguous_input() {
            validate_identifier("00008140-000471243E29801C", 256, "device")
                .expect("physical device UDID is valid");
            validate_team_identifier("983LPXU7G4").expect("development team is valid");

            assert!(validate_identifier(" device", 256, "device").is_err());
            assert!(validate_identifier("device\n", 256, "device").is_err());
            assert!(validate_team_identifier("983L-PXU7G4").is_err());
        }

        #[test]
        fn xctestrun_discovery_requires_one_regular_top_level_file() {
            let directory = tempfile::tempdir().expect("temporary xctestrun fixture");
            let products = directory.path();
            let first = products.join("VesperPlayerHostDemo_iphoneos.xctestrun");
            fs::write(&first, b"fixture\n").expect("write xctestrun fixture");
            fs::create_dir(products.join("ignored.xctestrun"))
                .expect("create non-file xctestrun fixture");
            assert_eq!(
                discover_xctestrun(products).expect("discover one xctestrun"),
                first
            );

            let second = products.join("Duplicate.xctestrun");
            fs::write(&second, b"duplicate\n").expect("write duplicate xctestrun fixture");
            let duplicate = discover_xctestrun(products)
                .expect_err("duplicate xctestrun files must be rejected");
            assert!(duplicate.to_string().contains("exactly one"));
            fs::remove_file(first).expect("remove first xctestrun fixture");
            fs::remove_file(second).expect("remove second xctestrun fixture");
            let missing =
                discover_xctestrun(products).expect_err("missing xctestrun must be rejected");
            assert!(missing.to_string().contains("found 0"));
        }

        #[test]
        fn device_project_replaces_exactly_the_verified_optional_dependencies() {
            let directory = tempfile::tempdir().expect("temporary device project fixture");
            let package = directory.path().join("verified package ${PATH} - test");
            create_verified_package_fixture(&package);
            let player_kit = directory.path().join("VesperPlayerKit");
            fs::create_dir(&player_kit).expect("create VesperPlayerKit fixture");
            let project = directory.path().join("Project");
            fs::create_dir(&project).expect("create project fixture");
            let spec = project.join("device-project.json");

            write_device_project_spec(
                &spec,
                resolved_project_fixture(),
                &player_kit,
                &package,
                crate::ios_optional_release::optional_frameworks(),
            )
            .expect("write isolated project spec");

            let value: Value =
                serde_json::from_slice(&fs::read(&spec).expect("read isolated project spec"))
                    .expect("parse isolated project spec");
            assert!(!contains_workspace_optional_artifact_reference(&value));
            assert_eq!(
                value["packages"]["VesperPlayerKit"]["path"],
                fs::canonicalize(&player_kit)
                    .expect("canonicalize VesperPlayerKit fixture")
                    .to_str()
                    .expect("UTF-8 fixture path")
            );
            assert!(value["packages"]["VesperPlayerOptionalPlugins"].is_null());
            let dependencies = value["targets"]["VesperPlayerHostDemo"]["dependencies"]
                .as_array()
                .expect("application dependencies");
            let frameworks = dependencies
                .iter()
                .filter_map(|dependency| {
                    dependency["framework"]
                        .as_str()
                        .and_then(|path| Path::new(path).file_stem())
                        .and_then(|name| name.to_str())
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(
                frameworks,
                crate::ios_optional_release::optional_frameworks()
                    .iter()
                    .copied()
                    .collect()
            );
            let verified_root = fs::canonicalize(package.join("Artifacts"))
                .expect("canonicalize verified artifacts fixture");
            for dependency in dependencies
                .iter()
                .filter(|dependency| dependency["framework"].is_string())
            {
                let framework = Path::new(
                    dependency["framework"]
                        .as_str()
                        .expect("verified framework path"),
                );
                assert!(framework.starts_with(&verified_root));
                assert!(framework.is_dir());
                assert_eq!(dependency["embed"], true);
                assert_eq!(dependency["link"], false);
                assert_eq!(dependency["codeSign"], true);
                assert!(dependency["package"].is_null());
                assert!(dependency["product"].is_null());
            }
            assert_eq!(
                value["targets"]["VesperPlayerHostDemo"]["info"]["path"],
                project
                    .join("Generated/Info.plist")
                    .to_str()
                    .expect("UTF-8 fixture path")
            );
            assert_eq!(
                value["targets"]["VesperPlayerHostDemo"]["sources"],
                serde_json::json!([{
                    "path": "Sources",
                    "excludes": ["Info.plist"],
                }])
            );
        }

        #[test]
        fn device_project_rejects_info_without_exact_source_ownership() {
            let directory = tempfile::tempdir().expect("temporary device project fixture");
            let package = directory.path().join("package");
            create_verified_package_fixture(&package);
            let player_kit = directory.path().join("VesperPlayerKit");
            fs::create_dir(&player_kit).expect("create VesperPlayerKit fixture");
            for (label, sources) in [
                ("missing", serde_json::json!(["OtherSources"])),
                (
                    "duplicate",
                    serde_json::json!(["Sources", { "path": "Sources" }]),
                ),
            ] {
                let project = directory.path().join(label);
                fs::create_dir(&project).expect("create project fixture");
                let mut value = resolved_project_fixture();
                value["targets"]["VesperPlayerHostDemo"]["sources"] = sources;
                let error = write_device_project_spec(
                    &project.join("device-project.json"),
                    value,
                    &player_kit,
                    &package,
                    crate::ios_optional_release::optional_frameworks(),
                )
                .expect_err("ambiguous Info.plist source ownership must fail");
                assert!(
                    error.to_string().contains("exactly one source directory"),
                    "unexpected {label} diagnostic: {error}"
                );
            }
        }

        #[test]
        fn device_project_rejects_missing_duplicate_and_unknown_optional_dependencies() {
            let directory = tempfile::tempdir().expect("temporary device project fixture");
            let package = directory.path().join("package");
            create_verified_package_fixture(&package);
            let player_kit = directory.path().join("VesperPlayerKit");
            fs::create_dir(&player_kit).expect("create VesperPlayerKit fixture");
            for (label, mutate) in [("missing", 0_u8), ("duplicate", 1_u8), ("unknown", 2_u8)] {
                let project = directory.path().join(label);
                fs::create_dir(&project).expect("create project fixture");
                let mut value = resolved_project_fixture();
                let dependencies = value["targets"]["VesperPlayerHostDemo"]["dependencies"]
                    .as_array_mut()
                    .expect("application dependencies");
                match mutate {
                    0 => {
                        dependencies.pop();
                    }
                    1 => dependencies.push(dependencies[1].clone()),
                    2 => {
                        dependencies[1]["framework"] = Value::String(
                            "../../lib/ios/VesperPlayerOptionalPlugins/Artifacts/Unknown.xcframework"
                                .to_owned(),
                        );
                    }
                    _ => unreachable!(),
                }
                let error = write_device_project_spec(
                    &project.join("device-project.json"),
                    value,
                    &player_kit,
                    &package,
                    crate::ios_optional_release::optional_frameworks(),
                )
                .expect_err("dependency drift must fail");
                assert!(
                    error.to_string().contains(label)
                        || (label == "duplicate" && error.to_string().contains("more than once")),
                    "unexpected {label} diagnostic: {error}"
                );
            }
        }
    }
}
