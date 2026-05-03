use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn pick_local_media_file() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        pick_local_media_file_macos()
    }

    #[cfg(target_os = "linux")]
    {
        pick_local_media_file_linux()
    }

    #[cfg(target_os = "windows")]
    {
        pick_local_media_file_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        bail!("desktop file picking is only supported on macOS, Linux, and Windows")
    }
}

#[cfg(target_os = "macos")]
fn pick_local_media_file_macos() -> Result<Option<PathBuf>> {
    let script = r#"
try
    POSIX path of (choose file with prompt "Select a media file to play")
on error number -128
    return ""
end try
"#;
    parse_dialog_stdout(run_dialog_command(
        Command::new("osascript").arg("-e").arg(script),
        "failed to launch macOS open-file dialog",
    )?)
}

#[cfg(target_os = "linux")]
fn pick_local_media_file_linux() -> Result<Option<PathBuf>> {
    match run_dialog_command(
        Command::new("zenity")
            .arg("--file-selection")
            .arg("--title=Select a media file to play"),
        "failed to launch zenity open-file dialog",
    ) {
        Ok(stdout) => return parse_dialog_stdout(stdout),
        Err(error) if is_command_not_found(&error) => {}
        Err(error) => return Err(error),
    }

    parse_dialog_stdout(run_dialog_command(
        Command::new("kdialog")
            .arg("--getopenfilename")
            .arg(".")
            .arg("Media Files(*)"),
        "failed to launch kdialog open-file dialog",
    )?)
}

#[cfg(target_os = "windows")]
fn pick_local_media_file_windows() -> Result<Option<PathBuf>> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'Select a media file to play'
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    Write-Output $dialog.FileName
}
"#;
    parse_dialog_stdout(run_dialog_command(
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-STA")
            .arg("-Command")
            .arg(script),
        "failed to launch Windows open-file dialog",
    )?)
}

fn run_dialog_command(command: &mut Command, spawn_context: &str) -> Result<String> {
    let output = command.output().with_context(|| spawn_context.to_owned())?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(String::new());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("dialog exited with status {}", output.status)
        };
        bail!("{detail}");
    }

    String::from_utf8(output.stdout).context("dialog output was not valid UTF-8")
}

fn parse_dialog_stdout(stdout: String) -> Result<Option<PathBuf>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(trimmed)))
}

#[cfg(target_os = "linux")]
fn is_command_not_found(error: &anyhow::Error) -> bool {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>())
        .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::NotFound)
}
