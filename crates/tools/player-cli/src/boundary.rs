use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

const MAX_BOUNDARY_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_BOUNDARY_SOURCE_FILES: usize = 20_000;

const UNKNOWN_CAPABILITY_KIND_PATH: &str =
    "lib/flutter/vesper_player_platform_interface/lib/src/models/plugin_diagnostic_models.dart";
const UNKNOWN_CAPABILITY_PATH: &str = "lib/flutter/vesper_player_platform_interface/lib/src/models/plugins/plugin_capability_models.dart";
const UNKNOWN_CAPABILITY_TEST_PATH: &str =
    "lib/flutter/vesper_player_platform_interface/test/events_test.dart";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryScanOptions {
    pub show_warnings: bool,
    pub show_all_warnings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryScanReport {
    output: String,
    failure: Option<String>,
}

impl BoundaryScanReport {
    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Fail,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Issue {
    severity: Severity,
    path: String,
    line: Option<usize>,
    message: &'static str,
}

impl Issue {
    fn location(&self) -> String {
        self.line
            .map(|line| format!("{}:{line}", self.path))
            .unwrap_or_else(|| self.path.clone())
    }
}

pub fn scan(root: &Path, options: BoundaryScanOptions) -> Result<BoundaryScanReport, String> {
    let issues = scan_issues(root, options)?;
    let mut output = Vec::new();
    if options.show_warnings || options.show_all_warnings {
        for issue in &issues {
            let severity = match issue.severity {
                Severity::Fail => "FAIL",
                Severity::Warn => "WARN",
            };
            output.push(format!(
                "{severity}: {}: {}",
                issue.location(),
                issue.message
            ));
        }
    }

    let failures = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Fail)
        .map(|issue| {
            format!(
                "Boundary invariant failed: {}: {}",
                issue.location(),
                issue.message
            )
        })
        .collect::<Vec<_>>();
    let warning_count = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Warn)
        .count();
    let failure = if failures.is_empty() {
        if options.show_warnings || options.show_all_warnings {
            output.push(format!(
                "Boundary invariant scan passed with {warning_count} warning candidate{}.",
                plural_suffix(warning_count)
            ));
        } else {
            output.push(format!(
                "Boundary invariant scan passed. Re-run with --warnings to inspect {warning_count} focused warning candidate{}, or --all-warnings for the broad scan.",
                plural_suffix(warning_count)
            ));
        }
        None
    } else {
        Some(failures.join("\n"))
    };

    Ok(BoundaryScanReport {
        output: if output.is_empty() {
            String::new()
        } else {
            format!("{}\n", output.join("\n"))
        },
        failure,
    })
}

fn scan_issues(root: &Path, options: BoundaryScanOptions) -> Result<Vec<Issue>, String> {
    let mut issues = Vec::new();
    scan_unknown_capability_contract(root, &mut issues)?;
    scan_swift_waits(root, &mut issues)?;
    scan_kotlin_cancellation(root, options, &mut issues)?;
    scan_rust_release_ordering(root, &mut issues)?;
    scan_ios_utility_queue(root, &mut issues)?;
    Ok(issues)
}

fn scan_unknown_capability_contract(root: &Path, issues: &mut Vec<Issue>) -> Result<(), String> {
    let kind_text = read_relative(root, UNKNOWN_CAPABILITY_KIND_PATH)?;
    let kind_expression = compile(
        r"(?s)enum\s+VesperPluginCapabilityKind\s*\{[^}]*\bunknown\b",
        "unknown capability kind",
    )?;
    if !kind_expression.is_match(&kind_text) {
        issues.push(Issue {
            severity: Severity::Fail,
            path: UNKNOWN_CAPABILITY_KIND_PATH.to_owned(),
            line: None,
            message: "VesperPluginCapabilityKind must expose an unknown case for forward-compatible capability unions.",
        });
    }

    let capability_text = read_relative(root, UNKNOWN_CAPABILITY_PATH)?;
    let constructor_expression = compile(
        r"(?s)const\s+VesperPluginCapability\._unknown.*?;",
        "unknown capability constructor",
    )?;
    match constructor_expression.find(&capability_text) {
        None => issues.push(Issue {
            severity: Severity::Fail,
            path: UNKNOWN_CAPABILITY_PATH.to_owned(),
            line: None,
            message: "VesperPluginCapability._unknown must exist so unknown capability records do not map to concrete union branches.",
        }),
        Some(constructor)
            if !constructor
                .as_str()
                .contains("kind = VesperPluginCapabilityKind.unknown") =>
        {
            issues.push(Issue {
                severity: Severity::Fail,
                path: UNKNOWN_CAPABILITY_PATH.to_owned(),
                line: Some(line_number_at(&capability_text, constructor.start())),
                message: "VesperPluginCapability._unknown must set kind to VesperPluginCapabilityKind.unknown, not decoder/frameProcessor/sourceNormalizer.",
            });
        }
        Some(_) => {}
    }

    if !read_relative(root, UNKNOWN_CAPABILITY_TEST_PATH)?
        .contains("VesperPluginCapabilityKind.unknown")
    {
        issues.push(Issue {
            severity: Severity::Fail,
            path: UNKNOWN_CAPABILITY_TEST_PATH.to_owned(),
            line: None,
            message: "Flutter plugin diagnostic tests must assert unknown capability kind preservation.",
        });
    }
    Ok(())
}

fn scan_swift_waits(root: &Path, issues: &mut Vec<Issue>) -> Result<(), String> {
    let wait_expression = compile(r"\bwait\s*\(\s*\)", "Swift wait")?;
    for path in source_files(root, &["lib/ios/VesperPlayerKit/Sources"], &["swift"])? {
        let text = read_source(&path.absolute, &path.relative)?;
        for (index, line) in text.lines().enumerate() {
            if wait_expression.is_match(line) {
                issues.push(Issue {
                    severity: Severity::Fail,
                    path: path.relative.clone(),
                    line: Some(index + 1),
                    message: "Swift production wait() calls must use a timeout or async API; unbounded waits can deadlock host/plugin boundaries.",
                });
            }
        }
    }
    Ok(())
}

fn scan_kotlin_cancellation(
    root: &Path,
    options: BoundaryScanOptions,
    issues: &mut Vec<Issue>,
) -> Result<(), String> {
    let catch_expression = compile(
        r"catch\s*\(\s*([A-Za-z_]\w*|_)\s*:\s*(?:Exception|Throwable|RuntimeException)\s*\)",
        "Kotlin catch",
    )?;
    let function_expression = compile(r"\bfun\b", "Kotlin function")?;
    let cancellation_catch_expression = compile(
        r"catch\s*\([^)]*:\s*CancellationException\s*\)",
        "Kotlin cancellation catch",
    )?;
    let coroutine_expression = compile(
        r"\bsuspend\s+fun\b|\b(?:launch|async)\s*\{|withContext\s*\(|runInterruptible\s*\{|[A-Za-z0-9_]+Async\s*\(",
        "Kotlin coroutine context",
    )?;

    for path in source_files(root, &["lib/android", "lib/flutter"], &["kt"])? {
        if path.relative.contains("/src/test/") || path.relative.contains("/androidTest/") {
            continue;
        }
        let text = read_source(&path.absolute, &path.relative)?;
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if options.show_all_warnings && line.contains("runCatching") {
                issues.push(Issue {
                    severity: Severity::Warn,
                    path: path.relative.clone(),
                    line: Some(index + 1),
                    message: "Review runCatching at lifecycle/protocol boundaries; it may hide CancellationException or unsupported platform failures.",
                });
            }

            let Some(captures) = catch_expression.captures(line) else {
                continue;
            };
            let catch_variable = captures.get(1).map(|value| value.as_str()).unwrap_or("_");
            let window = joined_window(&lines, index.saturating_sub(8), index + 4);
            if window.contains("CancellationException") {
                continue;
            }
            let function_context = kotlin_function_context(&lines, index, &function_expression);
            if function_context.contains("CancellationException") {
                let prior =
                    joined_window(&lines, index.saturating_sub(80), index.saturating_sub(1));
                if cancellation_catch_expression.is_match(&prior) {
                    continue;
                }
            }
            let catch_block = kotlin_block(&lines, index);
            if catch_variable != "_" {
                let throw_expression = compile(
                    &format!(r"\bthrow\s+{}\b", regex::escape(catch_variable)),
                    "Kotlin rethrow",
                )?;
                if throw_expression.is_match(&catch_block) {
                    continue;
                }
            }
            if !options.show_all_warnings {
                let local_context = joined_window(&lines, index.saturating_sub(12), index);
                let context = format!("{function_context}\n{local_context}");
                if !coroutine_expression.is_match(&context) {
                    continue;
                }
            }
            issues.push(Issue {
                severity: Severity::Warn,
                path: path.relative.clone(),
                line: Some(index + 1),
                message: "Exception catch near suspend/lifecycle code should usually handle CancellationException before mapping failures.",
            });
        }
    }
    Ok(())
}

fn scan_rust_release_ordering(root: &Path, issues: &mut Vec<Issue>) -> Result<(), String> {
    let operation_expression = compile(
        r"\.(?:remove|swap_remove|pop|take)\b",
        "Rust ownership operation",
    )?;
    let iterator_take_expression = compile(r"\.take\s*\(\s*[^)\s]", "Rust iterator take")?;
    let boundary_expression = compile(r"(?i)release|close|dispose", "Rust release boundary")?;
    let safe_order_expression = compile(
        r"(?s)release.*\?\s*;.*\.(?:remove|swap_remove)|release_result|Ok\(\(\)\)",
        "Rust release ordering",
    )?;

    for path in source_files(root, &["crates/ffi", "crates/plugin"], &["rs"])? {
        if path.relative.contains("/tests/") {
            continue;
        }
        let text = read_source(&path.absolute, &path.relative)?;
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if !operation_expression.is_match(line)
                || iterator_take_expression.is_match(line)
                || line.contains("sessions.remove(handle);")
            {
                continue;
            }
            let window = joined_window(&lines, index.saturating_sub(6), index + 10);
            if !boundary_expression.is_match(&window)
                || safe_order_expression.is_match(&window)
                || window.contains("registry lock")
                || window.contains("drop(session)")
                || window.contains("drop(registry)")
                || window.contains("worker.join")
                || window.contains("schedule_resource_cleanup")
                || window.contains("pending_frames.insert(handle, frame)")
                || window.contains("rejected_frames_pending_cleanup.push(frame)")
                || window.contains("outputs.push(output)")
                || window.contains("leased_packet = Some(packet)")
            {
                continue;
            }
            issues.push(Issue {
                severity: Severity::Warn,
                path: path.relative.clone(),
                line: Some(index + 1),
                message: "Review release ownership ordering; tracked resources should be removed only after fallible release succeeds.",
            });
        }
    }
    Ok(())
}

fn scan_ios_utility_queue(root: &Path, issues: &mut Vec<Issue>) -> Result<(), String> {
    let relative = "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/NativePlayer/VesperBoundedUtilityQueue.swift";
    let path = root.join(relative);
    if !path.is_file() {
        return Ok(());
    }
    let text = read_source(&path, relative)?;
    for (index, line) in text.lines().enumerate() {
        if line.contains("DispatchQueue.global") {
            issues.push(Issue {
                severity: Severity::Warn,
                path: relative.to_owned(),
                line: Some(index + 1),
                message: "Review bounded queue emergency fallback; required cleanup should stay bounded or document its escape hatch.",
            });
        }
    }
    Ok(())
}

struct SourcePath {
    absolute: PathBuf,
    relative: String,
}

fn source_files(
    root: &Path,
    source_roots: &[&str],
    extensions: &[&str],
) -> Result<Vec<SourcePath>, String> {
    let mut sources = Vec::new();
    for relative_root in source_roots {
        let absolute_root = root.join(relative_root);
        if !absolute_root.exists() {
            continue;
        }
        let mut directories = vec![absolute_root];
        while let Some(directory) = directories.pop() {
            let entries = fs::read_dir(&directory).map_err(|error| {
                format!(
                    "failed to read boundary scan directory '{}': {error}",
                    directory.display()
                )
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| {
                    format!(
                        "failed to read a boundary scan entry under '{}': {error}",
                        directory.display()
                    )
                })?;
                let file_type = entry.file_type().map_err(|error| {
                    format!(
                        "failed to inspect boundary source '{}': {error}",
                        entry.path().display()
                    )
                })?;
                if file_type.is_dir() {
                    if is_generated_directory(&entry.file_name()) {
                        continue;
                    }
                    directories.push(entry.path());
                } else if file_type.is_file()
                    && entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|extension| extensions.contains(&extension))
                {
                    let relative = entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|error| {
                            format!(
                                "failed to make boundary source '{}' relative: {error}",
                                entry.path().display()
                            )
                        })?
                        .to_str()
                        .ok_or_else(|| {
                            format!(
                                "boundary source path '{}' is not valid UTF-8",
                                entry.path().display()
                            )
                        })?
                        .to_owned();
                    sources.push(SourcePath {
                        absolute: entry.path(),
                        relative,
                    });
                    if sources.len() > MAX_BOUNDARY_SOURCE_FILES {
                        return Err(format!(
                            "boundary scan exceeds {MAX_BOUNDARY_SOURCE_FILES} source files"
                        ));
                    }
                }
            }
        }
    }
    sources.sort_by(|left, right| left.relative.split('/').cmp(right.relative.split('/')));
    Ok(sources)
}

fn read_relative(root: &Path, relative: &str) -> Result<String, String> {
    read_source(&root.join(relative), relative)
}

fn read_source(path: &Path, label: &str) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect boundary source {label}: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "boundary source {label} is not a regular non-symlink file"
        ));
    }
    if metadata.len() > MAX_BOUNDARY_FILE_BYTES as u64 {
        return Err(format!(
            "boundary source {label} exceeds {MAX_BOUNDARY_FILE_BYTES} bytes"
        ));
    }
    fs::read_to_string(path)
        .map_err(|error| format!("failed to read boundary source {label}: {error}"))
}

fn kotlin_block(lines: &[&str], start_index: usize) -> String {
    let Some(start_line) = lines.get(start_index) else {
        return String::new();
    };
    let Some(brace_index) = start_line.find('{') else {
        return (*start_line).to_owned();
    };
    let mut block = vec![*start_line];
    let remainder = &start_line[(brace_index + 1)..];
    let mut depth = 1_i64 + brace_delta(remainder);
    let mut index = start_index + 1;
    while depth > 0 && index < lines.len() {
        block.push(lines[index]);
        depth += brace_delta(lines[index]);
        index += 1;
    }
    block.join("\n")
}

fn brace_delta(line: &str) -> i64 {
    line.chars()
        .fold(0_i64, |depth, character| match character {
            '{' => depth + 1,
            '}' => depth - 1,
            _ => depth,
        })
}

fn kotlin_function_context(
    lines: &[&str],
    catch_index: usize,
    function_expression: &Regex,
) -> String {
    let start = catch_index.saturating_sub(180);
    let function_start = (start..=catch_index).rev().find(|index| {
        lines
            .get(*index)
            .is_some_and(|line| function_expression.is_match(line))
    });
    function_start
        .map(|index| joined_window(lines, index, catch_index))
        .unwrap_or_default()
}

fn joined_window(lines: &[&str], start: usize, inclusive_end: usize) -> String {
    if lines.is_empty() || start >= lines.len() {
        return String::new();
    }
    let end = inclusive_end.min(lines.len() - 1);
    if start > end {
        return String::new();
    }
    lines[start..=end].join("\n")
}

fn line_number_at(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())].lines().count().max(1)
}

fn compile(pattern: &str, label: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|error| format!("invalid {label} expression: {error}"))
}

fn is_generated_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("build" | ".build" | ".gradle" | ".dart_tool" | "target")
    )
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotlin_block_stops_after_the_balanced_catch_body() {
        let lines = [
            "catch (error: Exception) {",
            "  if (retry) { handle(error) }",
            "}",
            "next()",
        ];

        assert_eq!(
            kotlin_block(&lines, 0),
            "catch (error: Exception) {\n  if (retry) { handle(error) }\n}"
        );
    }

    #[test]
    fn report_keeps_failures_on_stderr_and_optional_candidates_on_stdout() {
        let report = BoundaryScanReport {
            output: "WARN: example\n".to_owned(),
            failure: Some("Boundary invariant failed: example".to_owned()),
        };

        assert_eq!(report.output(), "WARN: example\n");
        assert_eq!(report.failure(), Some("Boundary invariant failed: example"));
    }
}
