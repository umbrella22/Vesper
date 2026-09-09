// The CLI parses this request on every host before the macOS compatibility
// gate rejects unsupported execution.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ios::IosError;

const EXPECTED_LIFECYCLE_TESTS: u64 = 5;

pub(crate) struct IosPlaybackDeviceRequest {
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
            "iOS playback lifecycle device verification requires macOS",
        ))
    }
}

pub(crate) fn verify(
    root: &Path,
    request: IosPlaybackDeviceRequest,
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
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};
    use std::time::Duration;

    use nix::unistd::{AccessFlags, access};
    use serde::Deserialize;

    use super::{EXPECTED_LIFECYCLE_TESTS, IosError, IosPlaybackDeviceRequest};
    use crate::external_process::{self, ExternalProcessErrorKind};

    const MAX_PROCESS_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
    const MAX_XCRESULT_SUMMARY_BYTES: usize = 1024 * 1024;
    const MAX_XCRESULT_TESTS_BYTES: usize = 4 * 1024 * 1024;
    const MAX_XCRESULT_TEST_NODES: usize = 4096;
    const MAX_XCODE_PRODUCT_ENTRIES: usize = 4096;
    const MAX_DEVICE_IDENTIFIER_BYTES: usize = 256;
    const MAX_TEAM_IDENTIFIER_BYTES: usize = 64;
    const PROJECT_GENERATION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
    const RELEASE_BUILD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
    const DEVICE_XCTEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
    const METADATA_TOOL_TIMEOUT: Duration = Duration::from_secs(60);
    const TEST_CLASS: &str = "VesperPlayerKitDeviceTests/VesperPlaybackLifecycleDeviceTests";
    const TEST_CLASS_NAME: &str = "VesperPlaybackLifecycleDeviceTests";
    const EXPECTED_TEST_CASES: [&str; 5] = [
        "test720pAVPlayerLifecycleOnPhysicalDevice()",
        "test1080pAVPlayerLifecycleOnPhysicalDevice()",
        "testHlsVodNetworkPlaybackOnPhysicalDevice()",
        "testDashVodNetworkPlaybackOnPhysicalDevice()",
        "testHlsLiveDvrNetworkPlaybackOnPhysicalDevice()",
    ];
    const XCTEST_ENVIRONMENT_KEY: &str =
        "VesperPlayerKitDeviceTests.EnvironmentVariables.VESPER_IOS_PLAYBACK_DEVICE_TESTS";
    const FIXTURES: [&str; 2] = ["device-720p-h264-aac.m4v", "device-1080p-h264-aac.m4v"];

    struct RequiredTools {
        xcodegen: PathBuf,
        xcodebuild: PathBuf,
        xcrun: PathBuf,
        plutil: PathBuf,
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

    pub(super) fn verify(
        root: &Path,
        request: IosPlaybackDeviceRequest,
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
        let project_directory = require_regular_directory(
            &root.join("lib/ios/VesperPlayerKit"),
            "VesperPlayerKit project directory",
        )?;
        let manifest = require_regular_file(
            &project_directory.join("project.yml"),
            "VesperPlayerKit XcodeGen manifest",
        )?;
        let fixture_directory =
            require_regular_directory(&root.join("fixtures/media"), "media fixture directory")?;
        for fixture in FIXTURES {
            require_regular_file(
                &fixture_directory.join(fixture),
                &format!("iOS playback lifecycle fixture {fixture}"),
            )?;
        }
        let output_directory = create_output_directory(&request.output_directory)?;
        let derived_data = output_directory.join("DerivedData");
        let result_bundle = output_directory.join("VesperPlaybackLifecycle.xcresult");

        let mut generate = Command::new(&tools.xcodegen);
        generate
            .args(["generate", "--spec"])
            .arg(&manifest)
            .current_dir(&project_directory)
            .stdin(Stdio::null());
        run_captured_with_timeout(
            &mut generate,
            "iOS playback lifecycle Xcode project generation",
            diagnostics,
            PROJECT_GENERATION_TIMEOUT,
        )?;

        let project = require_regular_directory(
            &project_directory.join("VesperPlayerKit.xcodeproj"),
            "generated VesperPlayerKit Xcode project",
        )?;
        let destination = format!("platform=iOS,id={}", request.device);
        let mut build = Command::new(&tools.xcodebuild);
        build
            .arg("build-for-testing")
            .arg("-project")
            .arg(&project)
            .args([
                "-scheme",
                "VesperPlayerKit",
                "-configuration",
                "Release",
                "-destination",
                &destination,
                "-destination-timeout",
                "120",
                "-derivedDataPath",
            ])
            .arg(&derived_data)
            .arg(format!("DEVELOPMENT_TEAM={}", request.development_team))
            .args([
                "CODE_SIGN_STYLE=Automatic",
                "ENABLE_TESTABILITY=YES",
                "ARCHS=arm64",
                "ONLY_ACTIVE_ARCH=YES",
                "-parallel-testing-enabled",
                "NO",
            ])
            .arg(format!("-only-testing:{TEST_CLASS}"))
            .stdin(Stdio::null());
        if request.allow_provisioning_updates {
            build.arg("-allowProvisioningUpdates");
        }
        run_captured_with_timeout(
            &mut build,
            "iOS playback lifecycle Release build-for-testing",
            diagnostics,
            RELEASE_BUILD_TIMEOUT,
        )?;

        let products = require_regular_directory(
            &derived_data.join("Build/Products"),
            "iOS playback lifecycle Xcode build products",
        )?;
        let xctestrun = discover_xctestrun(&products)?;
        insert_opt_in_environment(&tools.plutil, &xctestrun, diagnostics)?;

        let mut test = Command::new(&tools.xcodebuild);
        test.arg("test-without-building")
            .arg("-xctestrun")
            .arg(&xctestrun)
            .args([
                "-destination",
                &destination,
                "-destination-timeout",
                "120",
                "-resultBundlePath",
            ])
            .arg(&result_bundle)
            .args(["-parallel-testing-enabled", "NO"])
            .arg(format!("-only-testing:{TEST_CLASS}"))
            .stdin(Stdio::null());
        if request.allow_provisioning_updates {
            test.arg("-allowProvisioningUpdates");
        }
        let test_status = run_captured_status_with_timeout(
            &mut test,
            "iOS playback lifecycle Release device XCTest",
            diagnostics,
            DEVICE_XCTEST_TIMEOUT,
        )?;
        let result_bundle = match require_regular_directory(
            &result_bundle,
            "iOS playback lifecycle XCResult bundle",
        ) {
            Ok(result_bundle) => result_bundle,
            Err(_) if !test_status.success() => {
                return classify_status(
                    test_status,
                    "iOS playback lifecycle Release device XCTest",
                );
            }
            Err(error) => return Err(error),
        };
        let tests = read_xcresult_tests(&tools.xcrun, &result_bundle, diagnostics)?;
        validate_lifecycle_test_tree(&tests, test_status.success())?;
        let summary = read_xcresult_summary(&tools.xcrun, &result_bundle, diagnostics)?;
        validate_summary(&summary, test_status.success())?;
        classify_status(test_status, "iOS playback lifecycle Release device XCTest")?;

        writeln!(
            output,
            "Verified {EXPECTED_LIFECYCLE_TESTS} iOS playback lifecycle Release tests on device {} (0 failed, 0 skipped).",
            request.device
        )
        .and_then(|_| writeln!(output, "XCResult: {}", result_bundle.display()))
        .map_err(|error| output_error("write playback lifecycle verification result", error))
    }

    fn insert_opt_in_environment(
        plutil: &Path,
        xctestrun: &Path,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let mut command = Command::new(plutil);
        command
            .args(["-insert", XCTEST_ENVIRONMENT_KEY, "-string", "1"])
            .arg(xctestrun)
            .stdin(Stdio::null());
        run_captured_with_timeout(
            &mut command,
            "iOS playback lifecycle XCTest environment injection",
            diagnostics,
            METADATA_TOOL_TIMEOUT,
        )
    }

    fn discover_xctestrun(products: &Path) -> Result<PathBuf, IosError> {
        let entries = fs::read_dir(products).map_err(|error| {
            IosError::storage(format!(
                "failed to scan iOS playback lifecycle Xcode products '{}': {error}",
                products.display()
            ))
        })?;
        let mut matches = Vec::new();
        let mut count = 0_usize;
        for entry in entries {
            count = count.checked_add(1).ok_or_else(|| {
                IosError::conformance("iOS playback lifecycle Xcode product count overflowed")
            })?;
            if count > MAX_XCODE_PRODUCT_ENTRIES {
                return Err(IosError::conformance(format!(
                    "iOS playback lifecycle Xcode products contain more than {MAX_XCODE_PRODUCT_ENTRIES} entries"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect an iOS playback lifecycle Xcode product: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS playback lifecycle Xcode product '{}': {error}",
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
                "iOS playback lifecycle verification requires exactly one xctestrun file, found {} under '{}'",
                matches.len(),
                products.display()
            ))),
        }
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
        let result = external_process::run_interruptible_capture_with_timeout(
            &mut command,
            "iOS playback lifecycle XCResult summary",
            MAX_XCRESULT_SUMMARY_BYTES,
            MAX_XCRESULT_SUMMARY_BYTES,
            METADATA_TOOL_TIMEOUT,
        )
        .map_err(map_process_error)?;
        diagnostics.write_all(&result.stderr).map_err(|error| {
            output_error("write iOS playback lifecycle XCResult diagnostics", error)
        })?;
        classify_status(result.status, "iOS playback lifecycle XCResult summary")?;
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "iOS playback lifecycle XCResult summary is invalid JSON: {error}"
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
        let result = external_process::run_interruptible_capture_with_timeout(
            &mut command,
            "iOS playback lifecycle XCResult test tree",
            MAX_XCRESULT_TESTS_BYTES,
            MAX_XCRESULT_SUMMARY_BYTES,
            METADATA_TOOL_TIMEOUT,
        )
        .map_err(map_process_error)?;
        diagnostics.write_all(&result.stderr).map_err(|error| {
            output_error("write iOS playback lifecycle XCResult diagnostics", error)
        })?;
        classify_status(result.status, "iOS playback lifecycle XCResult test tree")?;
        serde_json::from_slice(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "iOS playback lifecycle XCResult test tree is invalid JSON: {error}"
            ))
        })
    }

    fn validate_lifecycle_test_tree(
        tests: &XcresultTests,
        xcode_succeeded: bool,
    ) -> Result<(), IosError> {
        let mut pending = tests.test_nodes.iter().collect::<Vec<_>>();
        let mut visited = 0_usize;
        let mut suites = 0_usize;
        let mut case_names = Vec::new();
        while let Some(node) = pending.pop() {
            visited = visited.checked_add(1).ok_or_else(|| {
                IosError::conformance("iOS playback lifecycle XCResult node count overflowed")
            })?;
            if visited > MAX_XCRESULT_TEST_NODES {
                return Err(IosError::conformance(format!(
                    "iOS playback lifecycle XCResult test tree contains more than {MAX_XCRESULT_TEST_NODES} nodes"
                )));
            }
            if node.node_type == "Test Suite" && node.name == TEST_CLASS_NAME {
                suites = suites.checked_add(1).ok_or_else(|| {
                    IosError::conformance("iOS playback lifecycle suite count overflowed")
                })?;
                collect_test_case_names(&node.children, &mut visited, &mut case_names)?;
            } else {
                pending.extend(node.children.iter());
            }
        }

        case_names.sort_unstable();
        let mut expected = EXPECTED_TEST_CASES.to_vec();
        expected.sort_unstable();
        if suites == 1 && case_names == expected {
            return Ok(());
        }

        let message = format!(
            "iOS playback lifecycle acceptance did not execute the expected test class and cases: suites={suites}, cases={case_names:?}, expectedCases={expected:?}"
        );
        if xcode_succeeded {
            Err(IosError::conformance(message))
        } else {
            Err(IosError::worker(message))
        }
    }

    fn collect_test_case_names(
        children: &[XcresultTestNode],
        visited: &mut usize,
        case_names: &mut Vec<String>,
    ) -> Result<(), IosError> {
        let mut pending = children.iter().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            *visited = visited.checked_add(1).ok_or_else(|| {
                IosError::conformance("iOS playback lifecycle XCResult node count overflowed")
            })?;
            if *visited > MAX_XCRESULT_TEST_NODES {
                return Err(IosError::conformance(format!(
                    "iOS playback lifecycle XCResult test tree contains more than {MAX_XCRESULT_TEST_NODES} nodes"
                )));
            }
            if node.node_type == "Test Case" {
                case_names.push(node.name.clone());
            }
            pending.extend(node.children.iter());
        }
        Ok(())
    }

    fn validate_summary(summary: &XcresultSummary, xcode_succeeded: bool) -> Result<(), IosError> {
        if summary.total_test_count != EXPECTED_LIFECYCLE_TESTS {
            let message = format!(
                "iOS playback lifecycle acceptance did not execute exactly {EXPECTED_LIFECYCLE_TESTS} tests: result={}, total={}, passed={}, failed={}, skipped={}, expectedFailures={}",
                summary.result,
                summary.total_test_count,
                summary.passed_tests,
                summary.failed_tests,
                summary.skipped_tests,
                summary.expected_failures
            );
            return if xcode_succeeded {
                Err(IosError::conformance(message))
            } else {
                Err(IosError::worker(message))
            };
        }
        if summary.result != "Passed"
            || summary.passed_tests != EXPECTED_LIFECYCLE_TESTS
            || summary.failed_tests != 0
            || summary.skipped_tests != 0
            || summary.expected_failures != 0
        {
            return Err(IosError::conformance(format!(
                "iOS playback lifecycle acceptance requires {EXPECTED_LIFECYCLE_TESTS} passed, 0 failed, 0 skipped, and 0 expected failures; result={}, total={}, passed={}, failed={}, skipped={}, expectedFailures={}",
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
                "iOS playback lifecycle output directory must not be empty",
            ));
        }
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(IosError::storage(format!(
                    "iOS playback lifecycle output directory already exists: {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect iOS playback lifecycle output directory '{}': {error}",
                    path.display()
                )));
            }
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                IosError::storage(format!(
                    "iOS playback lifecycle output directory must have an existing parent: {}",
                    path.display()
                ))
            })?;
        require_regular_directory(parent, "iOS playback lifecycle output parent")?;
        fs::create_dir(path).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS playback lifecycle output directory '{}': {error}",
                path.display()
            ))
        })?;
        fs::canonicalize(path).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve iOS playback lifecycle output directory '{}': {error}",
                path.display()
            ))
        })
    }

    fn resolve_required_tools() -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            xcodegen: require_path_command("xcodegen")?,
            xcodebuild: require_path_command("xcodebuild")?,
            xcrun: require_path_command("xcrun")?,
            plutil: require_regular_file(Path::new("/usr/bin/plutil"), "Apple plutil")?,
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

    fn run_captured_with_timeout(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        timeout: Duration,
    ) -> Result<(), IosError> {
        let status = run_captured_status_with_timeout(command, label, diagnostics, timeout)?;
        classify_status(status, label)
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
            .map_err(|error| output_error("write iOS playback lifecycle diagnostics", error))?;
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

        fn passing_summary() -> XcresultSummary {
            XcresultSummary {
                result: "Passed".to_owned(),
                total_test_count: EXPECTED_LIFECYCLE_TESTS,
                passed_tests: EXPECTED_LIFECYCLE_TESTS,
                failed_tests: 0,
                skipped_tests: 0,
                expected_failures: 0,
            }
        }

        fn lifecycle_test_tree(case_names: &[&str]) -> XcresultTests {
            XcresultTests {
                test_nodes: vec![XcresultTestNode {
                    name: TEST_CLASS_NAME.to_owned(),
                    node_type: "Test Suite".to_owned(),
                    children: case_names
                        .iter()
                        .map(|name| XcresultTestNode {
                            name: (*name).to_owned(),
                            node_type: "Test Case".to_owned(),
                            children: Vec::new(),
                        })
                        .collect(),
                }],
            }
        }

        #[test]
        fn summary_requires_both_lifecycle_tests_without_skips() {
            validate_summary(&passing_summary(), true).expect("both lifecycle tests pass");

            let mut skipped = passing_summary();
            skipped.passed_tests = 1;
            skipped.skipped_tests = 1;
            let error = validate_summary(&skipped, true).expect_err("skip must fail the gate");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);

            let mut bootstrap_failure = passing_summary();
            bootstrap_failure.total_test_count = 0;
            bootstrap_failure.passed_tests = 0;
            let error = validate_summary(&bootstrap_failure, false)
                .expect_err("missing test execution is a worker failure");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
        }

        #[test]
        fn test_tree_requires_the_two_named_lifecycle_cases() {
            validate_lifecycle_test_tree(&lifecycle_test_tree(&EXPECTED_TEST_CASES), true)
                .expect("the two named lifecycle cases are accepted");

            let wrong_cases = lifecycle_test_tree(&["testUnrelatedOne()", "testUnrelatedTwo()"]);
            let error = validate_lifecycle_test_tree(&wrong_cases, true)
                .expect_err("two unrelated passing cases must not satisfy the gate");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);
            assert!(error.to_string().contains("testUnrelatedOne"));

            let missing_suite = XcresultTests {
                test_nodes: vec![XcresultTestNode {
                    name: "System Failures".to_owned(),
                    node_type: "Test Suite".to_owned(),
                    children: Vec::new(),
                }],
            };
            let error = validate_lifecycle_test_tree(&missing_suite, false)
                .expect_err("bootstrap failure must remain a worker failure");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
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
            let first = products.join("VesperPlayerKit_iphoneos.xctestrun");
            fs::write(&first, b"fixture\n").expect("write xctestrun fixture");
            fs::create_dir(products.join("ignored.xctestrun"))
                .expect("create non-file xctestrun fixture");
            assert_eq!(
                discover_xctestrun(products).expect("discover one xctestrun"),
                first
            );

            fs::write(products.join("Duplicate.xctestrun"), b"duplicate\n")
                .expect("write duplicate xctestrun fixture");
            assert!(discover_xctestrun(products).is_err());
        }
    }
}
