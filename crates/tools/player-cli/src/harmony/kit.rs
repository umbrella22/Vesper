use super::{EmulatorArgs, KitConsumerArgs, KitVerifyArgs, MAX_CARGO_BYTES, capture};
use crate::cli_error::{CliError, CliResult};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn run(root: &Path, script: &str, arguments: Vec<OsString>) -> CliResult<()> {
    let source = root.join("scripts/harmony").join(script);
    if !source.is_file() {
        return Err(CliError::manifest_or_package(format!(
            "missing Harmony verifier: {}",
            source.display()
        )));
    }
    let output = capture(
        Command::new("python3")
            .arg(source)
            .args(arguments)
            .current_dir(root),
        MAX_CARGO_BYTES,
        Duration::from_secs(1200),
    )
    .map_err(CliError::worker)?;
    std::io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|error| CliError::worker(error.to_string()))
}

fn tools(path: Option<&Path>) -> CliResult<Vec<OsString>> {
    let path = path.ok_or_else(|| {
        CliError::usage("provide --command-line-tools or VESPER_HARMONY_COMMAND_LINE_TOOLS")
    })?;
    Ok(vec!["--tools".into(), path.as_os_str().to_owned()])
}

pub(super) fn build(root: &Path, path: Option<&Path>, offline: bool) -> CliResult<()> {
    let mut arguments = tools(path)?;
    if offline {
        arguments.push("--offline".into());
    }
    run(root, "build-kit.py", arguments)
}

pub(super) fn host(root: &Path, path: Option<&Path>) -> CliResult<()> {
    run(root, "host-contracts.py", tools(path)?)
}

pub(super) fn verify(root: &Path, options: &KitVerifyArgs) -> CliResult<()> {
    let mut arguments = vec![
        "--directory".into(),
        options.directory.as_os_str().to_owned(),
    ];
    if let Some(hap) = &options.hap {
        arguments.extend(["--hap".into(), hap.as_os_str().to_owned()]);
    }
    run(root, "verify-artifacts.py", arguments)
}

pub(super) fn consumer(
    root: &Path,
    path: Option<&Path>,
    options: &KitConsumerArgs,
) -> CliResult<()> {
    let mut arguments = tools(path)?;
    arguments.extend(["--har".into(), options.har.as_os_str().to_owned()]);
    if options.ui {
        arguments.push("--ui".into());
    }
    if options.example {
        arguments.push("--example".into());
    }
    if options.no_internet {
        arguments.push("--no-internet".into());
    }
    run(root, "build-consumer.py", arguments)
}

pub(super) fn emulator(root: &Path, path: Option<&Path>, options: &EmulatorArgs) -> CliResult<()> {
    let mut arguments = tools(path)?;
    arguments.extend([
        "--instance".into(),
        options.instance.clone().into(),
        "--device".into(),
        options.device.clone().into(),
        "--hap".into(),
        options.hap.as_os_str().to_owned(),
        "--output".into(),
        options.output.as_os_str().to_owned(),
        "--button".into(),
        options.button.clone().into(),
        "--bundle".into(),
        options.bundle.clone().into(),
    ]);
    run(root, "emulator-smoke.py", arguments)
}
