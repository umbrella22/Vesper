use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::cli_error::{CliError, CliResult};
use crate::external_process::run_interruptible_capture_with_timeout;

mod kit;

const TARGET: &str = "aarch64-unknown-linux-ohos";
const SHARED_PACKAGES: &[&str] = &[
    "player-model",
    "player-runtime",
    "player-download",
    "player-preload",
    "player-playlist",
    "player-platform-mobile",
];
const MAX_TOOL_BYTES: usize = 64 * 1024;
const MAX_CARGO_BYTES: usize = 8 * 1024 * 1024;
const UNVERIFIED: &[&str] = &[
    "Release native linking and dynamic dependencies",
    "Node-API, HAR packaging and independent consumer loading",
    "API 20 and API 26 physical-device playback and resource-loader reuse",
];

#[derive(Debug, Args)]
pub(crate) struct HarmonyArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Extracted Command Line Tools root; also accepts VESPER_HARMONY_COMMAND_LINE_TOOLS.
    #[arg(long, global = true)]
    command_line_tools: Option<PathBuf>,
    #[command(subcommand)]
    command: HarmonyCommand,
}

#[derive(Debug, Subcommand)]
enum HarmonyCommand {
    /// Reports Rust, SDK and bundled tool identities without installing tools.
    Doctor,
    /// Audits the candidate shared mobile dependency tree for the OHOS target.
    AuditDependencies(CargoArgs),
    /// Cross-checks shared Rust crates; this does not link a native library.
    CheckShared(CargoArgs),
    /// Builds the three preview host-kit HARs and separated native symbols.
    KitHar(CargoArgs),
    /// Runs host-kit ownership, source, cache, download and system regressions.
    KitHostContracts,
    /// Verifies staged HARs, checksums and optional consumer native bytes.
    KitVerify(KitVerifyArgs),
    /// Builds a consumer using existing staged host-kit HARs only.
    KitConsumer(KitConsumerArgs),
    /// Runs one artifact-bound scenario on an already running official emulator.
    EmulatorGate(EmulatorArgs),
}

#[derive(Debug, Args)]
struct KitVerifyArgs {
    #[arg(long, default_value = "target/harmony-kit/dist")]
    directory: PathBuf,
    #[arg(long)]
    hap: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct KitConsumerArgs {
    #[arg(long)]
    har: PathBuf,
    #[arg(long)]
    ui: bool,
    #[arg(long)]
    example: bool,
    #[arg(long)]
    no_internet: bool,
}

#[derive(Debug, Args)]
struct EmulatorArgs {
    #[arg(long)]
    instance: String,
    #[arg(long)]
    device: String,
    #[arg(long)]
    hap: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value = "Run kit checks")]
    button: String,
    #[arg(long, default_value = "dev.vesper.harmony.kitconsumer")]
    bundle: String,
}

#[derive(Debug, Args)]
struct CargoArgs {
    /// Uses only locally cached Cargo dependencies.
    #[arg(long)]
    offline: bool,
}

pub(crate) fn run(arguments: HarmonyArgs) -> CliResult<()> {
    let root = crate::contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let tools = arguments
        .command_line_tools
        .or_else(|| {
            std::env::var_os("VESPER_HARMONY_COMMAND_LINE_TOOLS")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        });
    match arguments.command {
        HarmonyCommand::Doctor => doctor(&root, tools.as_deref()),
        HarmonyCommand::AuditDependencies(options) => audit_dependencies(&root, options.offline),
        HarmonyCommand::CheckShared(options) => check_shared(&root, options.offline),
        HarmonyCommand::KitHar(options) => kit::build(&root, tools.as_deref(), options.offline),
        HarmonyCommand::KitHostContracts => kit::host(&root, tools.as_deref()),
        HarmonyCommand::KitVerify(options) => kit::verify(&root, &options),
        HarmonyCommand::KitConsumer(options) => kit::consumer(&root, tools.as_deref(), &options),
        HarmonyCommand::EmulatorGate(options) => kit::emulator(&root, tools.as_deref(), &options),
    }
}

fn emit(report: &Value) -> CliResult<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, report)
        .map_err(|error| CliError::worker(format!("cannot serialize Harmony report: {error}")))?;
    writeln!(output)
        .map_err(|error| CliError::worker(format!("cannot write Harmony report: {error}")))
}

fn capture(command: &mut Command, limit: usize, timeout: Duration) -> Result<String, String> {
    command.env("RUSTUP_AUTO_INSTALL", "0");
    let result = run_interruptible_capture_with_timeout(
        command,
        "Harmony host check",
        limit,
        limit,
        timeout,
    )
    .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "{}:\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout).trim(),
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    String::from_utf8(result.stdout).map_err(|error| format!("tool stdout is not UTF-8: {error}"))
}

fn record(checks: &mut Vec<Value>, id: &str, result: Result<String, String>) {
    let (status, detail) = match result {
        Ok(detail) => ("passed", detail),
        Err(detail) => ("blocked", detail),
    };
    checks.push(json!({ "id": id, "status": status, "detail": detail.trim() }));
}

fn doctor(root: &Path, tools: Option<&Path>) -> CliResult<()> {
    let mut checks = Vec::new();
    // Hvigor creates project-local state even for --version. Probe bundled tools
    // in a disposable directory so a read-only check leaves the checkout intact.
    let tool_directory = tempfile::tempdir().map_err(|error| {
        CliError::worker(format!("cannot create tool probe directory: {error}"))
    })?;
    let probe = |command: &mut Command| capture(command, MAX_TOOL_BYTES, Duration::from_secs(20));
    record(
        &mut checks,
        "rustc",
        probe(Command::new("rustc").arg("--version").current_dir(root)),
    );
    let cfg = probe(
        Command::new("rustc")
            .args(["--print", "cfg", "--target", TARGET])
            .current_dir(root),
    )
    .and_then(|cfg| {
        if [
            "target_arch=\"aarch64\"",
            "target_os=\"linux\"",
            "target_env=\"ohos\"",
        ]
        .iter()
        .all(|required| cfg.lines().any(|line| line == *required))
        {
            Ok(cfg)
        } else {
            Err(format!("unexpected OHOS target configuration: {cfg}"))
        }
    });
    record(&mut checks, "rust_target_cfg", cfg);
    let std = probe(
        Command::new("rustc")
            .args(["--print", "target-libdir", "--target", TARGET])
            .current_dir(root),
    )
    .and_then(|directory| {
        let path = Path::new(directory.trim());
        // Bound filesystem work even if a misconfigured compiler reports another directory.
        let installed = path.read_dir().ok().is_some_and(|entries| {
            entries.take(1024).filter_map(Result::ok).any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("libstd-") && name.ends_with(".rlib") && entry.path().is_file()
            })
        });
        if installed {
            Ok(directory)
        } else {
            Err(format!(
                "OHOS std is missing; run rustup target add {TARGET}"
            ))
        }
    });
    record(&mut checks, "rust_target_std", std);

    if let Some(tools) = tools {
        let tools = if tools.is_absolute() {
            tools.to_path_buf()
        } else {
            root.join(tools)
        };
        let native = tools.join("sdk/default/openharmony/native");
        let sdk = read_sdk_identity(&native);
        record(&mut checks, "sdk_release", sdk);
        record(
            &mut checks,
            "sdk_sysroot",
            if native
                .join("sysroot/usr/include/napi/native_api.h")
                .is_file()
                && native.join("sysroot/usr/lib/aarch64-linux-ohos").is_dir()
            {
                Ok(native.join("sysroot").display().to_string())
            } else {
                Err(format!(
                    "missing arm64 sysroot or Node-API headers under {}",
                    native.display()
                ))
            },
        );
        for (id, relative, args) in [
            (
                "clang",
                "sdk/default/openharmony/native/llvm/bin/clang",
                &["--version"][..],
            ),
            (
                "clang_cpp",
                "sdk/default/openharmony/native/llvm/bin/clang++",
                &["--version"][..],
            ),
            (
                "llvm_readelf",
                "sdk/default/openharmony/native/llvm/bin/llvm-readelf",
                &["--version"][..],
            ),
            (
                "llvm_nm",
                "sdk/default/openharmony/native/llvm/bin/llvm-nm",
                &["--version"][..],
            ),
            ("node", "tool/node/bin/node", &["--version"][..]),
            ("ohpm", "bin/ohpm", &["--version"][..]),
            ("hvigor", "bin/hvigorw", &["--version"][..]),
            ("hdc", "sdk/default/openharmony/toolchains/hdc", &["-v"][..]),
        ] {
            let path = tools.join(relative);
            let result = probe(
                Command::new(&path)
                    .args(args)
                    .current_dir(tool_directory.path())
                    .env("NODE_HOME", tools.join("tool/node"))
                    .env("DEVECO_NODE_HOME", tools.join("tool/node"))
                    .env("DEVECO_SDK_HOME", tools.join("sdk")),
            )
            .and_then(|version| {
                if version.trim().is_empty() {
                    Err(format!("{} returned no version identity", path.display()))
                } else {
                    Ok(format!("{}\n{}", path.display(), version.trim()))
                }
            });
            record(&mut checks, id, result);
        }
    } else {
        record(&mut checks, "command_line_tools", Err(
            "pass --command-line-tools /path/to/command-line-tools or set VESPER_HARMONY_COMMAND_LINE_TOOLS".to_owned(),
        ));
    }
    let passed = checks.iter().all(|check| check["status"] == "passed");
    emit(&json!({
        "schemaVersion": 1,
        "scope": "harmony-host-prerequisites",
        "target": TARGET,
        "plannedMinimumRuntimeApi": 20,
        "status": if passed { "passed" } else { "blocked" },
        "checks": checks,
        "unverified": UNVERIFIED,
    }))?;
    if passed {
        Ok(())
    } else {
        Err(CliError::compatibility(
            "Harmony host prerequisites are incomplete; see the JSON report",
        ))
    }
}

fn read_sdk_identity(native: &Path) -> Result<String, String> {
    let path = native.join("oh-uni-package.json");
    let mut content = String::new();
    File::open(&path)
        .and_then(|file| {
            file.take(MAX_TOOL_BYTES as u64 + 1)
                .read_to_string(&mut content)
        })
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if content.len() > MAX_TOOL_BYTES {
        return Err(format!("SDK identity exceeds {MAX_TOOL_BYTES} bytes"));
    }
    let identity: Value =
        serde_json::from_str(&content).map_err(|error| format!("invalid SDK identity: {error}"))?;
    if identity["apiVersion"] != "26"
        || identity["platformVersion"] != "26.0.0"
        || identity["releaseType"] != "Release"
        || identity["version"].as_str().is_none_or(str::is_empty)
    {
        return Err(format!(
            "expected HarmonyOS SDK 26.0.0 Release (API 26): {identity}"
        ));
    }
    Ok(format!("{}\n{identity}", path.display()))
}

fn cargo_command(root: &Path, operation: &str, offline: bool) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([operation, "--locked", "--target", TARGET]);
    if offline {
        command.arg("--offline");
    }
    for package in SHARED_PACKAGES {
        command.args(["-p", package]);
    }
    command
}

fn audit_dependencies(root: &Path, offline: bool) -> CliResult<()> {
    let mut command = cargo_command(root, "tree", offline);
    command.args([
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    let tree = capture(&mut command, MAX_CARGO_BYTES, Duration::from_secs(120))
        .map_err(CliError::worker)?;
    let packages = tree_packages(&tree)?;
    let forbidden: Vec<_> = packages
        .iter()
        .filter(|package| forbidden_package(package))
        .collect();
    emit(&json!({
        "schemaVersion": 1,
        "scope": "harmony-shared-mobile-dependency-audit",
        "target": TARGET,
        "roots": SHARED_PACKAGES,
        "status": if forbidden.is_empty() { "passed" } else { "failed" },
        "packages": packages,
        "forbiddenPackages": forbidden,
        "cargoTree": tree,
        "unverified": UNVERIFIED,
    }))?;
    if forbidden.is_empty() {
        Ok(())
    } else {
        Err(CliError::conformance(
            "desktop or optional media execution dependencies entered the Harmony shared-mobile candidate",
        ))
    }
}

fn tree_packages(tree: &str) -> CliResult<BTreeSet<String>> {
    let mut packages = BTreeSet::new();
    for line in tree.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let name = fields
            .next()
            .ok_or_else(|| CliError::conformance("missing Cargo package name"))?;
        let version = fields
            .next()
            .and_then(|value| value.strip_prefix('v'))
            .ok_or_else(|| CliError::conformance(format!("invalid Cargo tree record: {line}")))?;
        semver::Version::parse(version).map_err(|error| {
            CliError::conformance(format!("invalid Cargo package version: {error}"))
        })?;
        packages.insert(format!("{name} v{version}"));
    }
    for root in SHARED_PACKAGES {
        if !packages
            .iter()
            .any(|package| package.split_whitespace().next() == Some(root))
        {
            return Err(CliError::conformance(format!(
                "Cargo tree did not include required root {root}"
            )));
        }
    }
    Ok(packages)
}

fn forbidden_package(package: &str) -> bool {
    let name = package.split_whitespace().next().unwrap_or_default();
    matches!(
        name,
        "player-ffi"
            | "player-platform-desktop"
            | "player-platform-linux"
            | "player-platform-windows"
            | "player-platform-macos"
            | "player-host-desktop"
            | "player-platform-native-frame"
            | "player-backend-ffmpeg"
            | "player-audio-cpal"
            | "player-render-wgpu"
            | "cpal"
            | "winit"
            | "wgpu"
    ) || name.starts_with("ffmpeg-")
        || name.starts_with("player-decoder-")
        || name.starts_with("player-remux-")
        || name.starts_with("wasmtime")
}

fn check_shared(root: &Path, offline: bool) -> CliResult<()> {
    let result = capture(
        &mut cargo_command(root, "check", offline),
        MAX_CARGO_BYTES,
        Duration::from_secs(300),
    );
    emit(&json!({
        "schemaVersion": 1,
        "scope": "harmony-shared-rust-check",
        "target": TARGET,
        "roots": SHARED_PACKAGES,
        "status": if result.is_ok() { "passed" } else { "failed" },
        "detail": result.as_ref().map(String::as_str).unwrap_or_else(|error| error.as_str()),
        "unverified": UNVERIFIED,
    }))?;
    result.map(|_| ()).map_err(CliError::worker)
}
