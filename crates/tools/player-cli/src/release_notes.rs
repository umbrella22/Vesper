use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::release::{
    ReleaseEnvironment, ReleaseError, ReleaseResult, atomic_write_output, git_output,
    git_output_optional,
};

const FFMPEG_SOURCE_PREFIX: &str = "VesperPlayerOptionalPlugins-FFmpeg-";
const FFMPEG_SOURCE_SUFFIX: &str = "-source.tar.xz";

pub fn generate(
    root: &Path,
    environment: &ReleaseEnvironment,
    tag: &str,
    output: Option<&Path>,
) -> ReleaseResult<PathBuf> {
    let tag = tag.strip_prefix("refs/tags/").unwrap_or(tag);
    let version = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(version)
        .map_err(|error| ReleaseError::input(format!("Invalid release tag {tag}: {error}")))?;
    let tag_ref = format!("refs/tags/{tag}");
    let commit_ref = format!("{tag_ref}^{{commit}}");
    git_output(root, &["rev-parse", "--verify", &commit_ref])?;

    // Read the immutable release input, even when the working checkout differs.
    let changelog = git_output(root, &["show", &format!("{tag_ref}:CHANGELOG.md")])?;
    let summaries = changelog_summaries(&changelog, version)?;
    let previous_ref = format!("{tag_ref}^");
    let previous_tag = git_output_optional(
        root,
        &[
            "describe",
            "--tags",
            "--match",
            "v[0-9]*",
            "--abbrev=0",
            &previous_ref,
        ],
    )?
    .filter(|value| !value.is_empty());
    let range = previous_tag
        .as_ref()
        .map(|previous| format!("{previous}..{tag_ref}"))
        .unwrap_or_else(|| tag_ref.clone());
    let repository_url = resolve_repository_url(root, environment)?;
    let compare_url = previous_tag.as_ref().and_then(|previous| {
        repository_url
            .as_ref()
            .map(|repository| format!("{repository}/compare/{previous}...{tag}"))
    });
    let download_base = repository_url
        .as_ref()
        .map(|repository| format!("{repository}/releases/download/{tag}"));

    let output_path = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("dist/release/RELEASE_NOTES.md"));
    let output_directory = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_directory).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to create release notes directory '{}': {error}",
            output_directory.display()
        ))
    })?;
    let ffmpeg_source = find_ffmpeg_source_asset(output_directory)?;
    let contributors = collect_contributors(root, &range)?;
    let notes = render_notes(NotesInput {
        tag,
        previous_tag: previous_tag.as_deref(),
        repository_url: repository_url.as_deref(),
        compare_url: compare_url.as_deref(),
        download_base: download_base.as_deref(),
        release_channel: release_channel(tag),
        ffmpeg_source: &ffmpeg_source,
        summaries: &summaries,
        contributors: &contributors,
    });
    atomic_write_output(&output_path, notes.as_bytes())?;
    Ok(output_path)
}

struct NotesInput<'a> {
    tag: &'a str,
    previous_tag: Option<&'a str>,
    repository_url: Option<&'a str>,
    compare_url: Option<&'a str>,
    download_base: Option<&'a str>,
    release_channel: &'static str,
    ffmpeg_source: &'a str,
    summaries: &'a ChangelogSummaries,
    contributors: &'a [String],
}

struct ChangelogSummaries {
    english: String,
    chinese: String,
}

fn advance_markdown_fence(line: &str, fence: &mut Option<(u8, usize)>) -> bool {
    let was_fenced = fence.is_some();
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() <= 3
        && let Some(&marker @ (b'`' | b'~')) = trimmed.as_bytes().first()
    {
        let length = trimmed.bytes().take_while(|byte| *byte == marker).count();
        match *fence {
            Some((opening, minimum))
                if marker == opening
                    && length >= minimum
                    && trimmed[length..].trim().is_empty() =>
            {
                *fence = None;
            }
            None if length >= 3 => *fence = Some((marker, length)),
            _ => {}
        }
    }
    was_fenced || fence.is_some()
}

fn is_relative_repo_target(target: &str) -> bool {
    let clean = target.strip_prefix("./").unwrap_or(target);
    if clean.is_empty()
        || clean == "."
        || clean == ".."
        || clean.starts_with('/')
        || clean.starts_with('#')
        || clean.starts_with("../")
    {
        return false;
    }
    let prefix = clean.split(['/', '#', '?']).next().unwrap_or("");
    !prefix.contains(':')
}

fn rewrite_inline_relative_links(line: &str, repository_url: &str, tag: &str) -> String {
    let bytes = line.as_bytes();
    let mut output = String::with_capacity(line.len());
    let mut copy_from = 0;
    let mut inline_code: Option<usize> = None;
    let mut bracket_depth: usize = 0;
    let mut last_open_is_image = false;
    let mut index = 0;

    while index < bytes.len() {
        if inline_code.is_none() && bytes[index] == b'\\' && index + 1 < bytes.len() {
            index += 2;
            continue;
        }
        if bytes[index] == b'`' {
            let run = bytes[index..].iter().take_while(|&&b| b == b'`').count();
            match inline_code {
                Some(open_run) if run == open_run => inline_code = None,
                None => inline_code = Some(run),
                _ => {}
            }
            index += run;
            continue;
        }
        if inline_code.is_some() {
            index += 1;
            continue;
        }
        match bytes[index] {
            b'[' => {
                if bracket_depth == 0 {
                    last_open_is_image = index > 0 && bytes[index - 1] == b'!';
                }
                bracket_depth += 1;
                index += 1;
            }
            b']' if bracket_depth > 0 => {
                bracket_depth -= 1;
                if bracket_depth == 0 && !last_open_is_image && bytes.get(index + 1) == Some(&b'(')
                {
                    let target_start = index + 2;
                    let mut depth = 1usize;
                    let mut scan = target_start;
                    while scan < bytes.len() {
                        match bytes[scan] {
                            b'\\' if scan + 1 < bytes.len() => scan += 2,
                            b'(' => {
                                depth += 1;
                                scan += 1;
                            }
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                scan += 1;
                            }
                            _ => scan += 1,
                        }
                    }
                    if depth == 0 {
                        let inner = &line[target_start..scan];
                        let leading_ws = inner.len() - inner.trim_start().len();
                        let trimmed = &inner[leading_ws..];
                        let dest_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
                        let dest = &trimmed[..dest_len];
                        if is_relative_repo_target(dest) {
                            let clean = dest.strip_prefix("./").unwrap_or(dest);
                            let dest_byte_start = target_start + leading_ws;
                            let dest_byte_end = dest_byte_start + dest_len;
                            output.push_str(&line[copy_from..dest_byte_start]);
                            output.push_str(repository_url.trim_end_matches('/'));
                            output.push_str("/blob/");
                            output.push_str(tag);
                            output.push('/');
                            output.push_str(clean);
                            copy_from = dest_byte_end;
                        }
                        index = scan + 1;
                        continue;
                    }
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    output.push_str(&line[copy_from..]);
    output
}

fn rewrite_relative_markdown_links(markdown: &str, repository_url: &str, tag: &str) -> String {
    let mut fence = None::<(u8, usize)>;
    let mut lines = Vec::new();
    for raw in markdown.lines() {
        if advance_markdown_fence(raw, &mut fence) {
            lines.push(raw.to_owned());
        } else {
            lines.push(rewrite_inline_relative_links(raw, repository_url, tag));
        }
    }
    let mut result = lines.join("\n");
    if markdown.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn changelog_summaries(source: &str, version: &str) -> ReleaseResult<ChangelogSummaries> {
    let mut found = false;
    let mut active = false;
    let mut language = None;
    let mut english = None::<String>;
    let mut chinese = None::<String>;
    let mut fence = None::<(u8, usize)>;

    for raw in source.lines() {
        // Headings and locale markers inside fenced examples are content.
        if !advance_markdown_fence(raw, &mut fence) {
            if let Some(heading) = raw.strip_prefix("## ") {
                active = heading.split_whitespace().next() == Some(version);
                if active {
                    if found {
                        return Err(ReleaseError::input(format!(
                            "CHANGELOG.md contains duplicate version {version}."
                        )));
                    }
                    found = true;
                }
                language = None;
                continue;
            }
            if active {
                let target = match raw.trim() {
                    "<!-- release-notes:en -->" => Some(("en", &mut english)),
                    "<!-- release-notes:zh-CN -->" => Some(("zh-CN", &mut chinese)),
                    _ => None,
                };
                if let Some((locale, body)) = target {
                    if body.is_some() {
                        return Err(ReleaseError::input(format!(
                            "CHANGELOG.md version {version} repeats release-notes:{locale}."
                        )));
                    }
                    *body = Some(String::new());
                    language = Some(locale);
                    continue;
                }
            }
        }
        if active {
            let body = match language {
                Some("en") => english.as_mut(),
                Some("zh-CN") => chinese.as_mut(),
                _ => None,
            };
            if let Some(body) = body {
                line(body, raw);
            } else if !raw.trim().is_empty() {
                return Err(ReleaseError::input(format!(
                    "CHANGELOG.md version {version} requires <!-- release-notes:en --> and <!-- release-notes:zh-CN --> before content."
                )));
            }
        }
    }
    if !found {
        return Err(ReleaseError::input(format!(
            "CHANGELOG.md has no entry for version {version}."
        )));
    }
    let required = |body: Option<String>, locale| {
        body.filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_owned())
            .ok_or_else(|| ReleaseError::input(format!(
                "CHANGELOG.md version {version} requires nonempty release-notes:{locale} content."
            )))
    };
    Ok(ChangelogSummaries {
        english: required(english, "en")?,
        chinese: required(chinese, "zh-CN")?,
    })
}

fn render_notes(input: NotesInput<'_>) -> String {
    let mut output = String::new();
    line(&mut output, &format!("# VesperPlayerKit {}", input.tag));
    blank(&mut output);
    line(
        &mut output,
        &format!(
            "VesperPlayerKit {} is a release for the Android and iOS mobile SDK bundles.",
            input.tag
        ),
    );
    blank(&mut output);
    line(&mut output, "## Release Details");
    blank(&mut output);
    match input.previous_tag {
        Some(previous) => line(&mut output, &format!("- Previous version: `{previous}`")),
        None => line(
            &mut output,
            "- Previous version: first tagged VesperPlayerKit release",
        ),
    }
    line(&mut output, &format!("- Release tag: `{}`", input.tag));
    line(
        &mut output,
        &format!("- Release channel: {}", input.release_channel),
    );
    if let (Some(previous), Some(compare)) = (input.previous_tag, input.compare_url) {
        line(
            &mut output,
            &format!(
                "- Compare changes: [`{previous}...{}`]({compare})",
                input.tag
            ),
        );
    }
    blank(&mut output);
    line(&mut output, "## Change Summary");
    blank(&mut output);
    let english_summary = input.repository_url.map_or_else(
        || input.summaries.english.clone(),
        |repo| rewrite_relative_markdown_links(&input.summaries.english, repo, input.tag),
    );
    block(&mut output, &english_summary);
    blank(&mut output);
    line(&mut output, "---");
    blank(&mut output);
    line(
        &mut output,
        &format!("# VesperPlayerKit {} 中文说明", input.tag),
    );
    blank(&mut output);
    line(
        &mut output,
        &format!(
            "VesperPlayerKit {} 是 Android 与 iOS 移动端 SDK 二进制发布包。",
            input.tag
        ),
    );
    blank(&mut output);
    line(&mut output, "## 发布信息");
    blank(&mut output);
    match input.previous_tag {
        Some(previous) => line(&mut output, &format!("- 上一个版本：`{previous}`")),
        None => line(&mut output, "- 上一个版本：首个带标签发布版本"),
    }
    line(&mut output, &format!("- 发布标签：`{}`", input.tag));
    line(
        &mut output,
        &format!("- 发布通道：{}", input.release_channel),
    );
    if let (Some(previous), Some(compare)) = (input.previous_tag, input.compare_url) {
        line(
            &mut output,
            &format!("- 变更对比：[`{previous}...{}`]({compare})", input.tag),
        );
    }
    blank(&mut output);
    line(&mut output, "## 变更摘要");
    blank(&mut output);
    let chinese_summary = input.repository_url.map_or_else(
        || input.summaries.chinese.clone(),
        |repo| rewrite_relative_markdown_links(&input.summaries.chinese, repo, input.tag),
    );
    block(&mut output, &chinese_summary);
    blank(&mut output);
    line(&mut output, "---");
    blank(&mut output);
    line(&mut output, "## Downloads");
    blank(&mut output);
    line(
        &mut output,
        "These downloads are prebuilt binary artifacts. Host applications do not need to run this repository's JNI or FFmpeg generation tasks during their own Gradle / Xcode builds.",
    );
    blank(&mut output);
    line(&mut output, "### Android");
    blank(&mut output);
    download(
        &mut output,
        input.download_base,
        "VesperPlayerKit-android-arm64-v8a.aar",
        "Core Android host-kit AAR",
    );
    download(
        &mut output,
        input.download_base,
        "VesperPlayerKitCompose-android-arm64-v8a.aar",
        "Jetpack Compose binding AAR",
    );
    download(
        &mut output,
        input.download_base,
        "VesperPlayerKitComposeUi-android-arm64-v8a.aar",
        "Optional Compose UI controls AAR",
    );
    download(
        &mut output,
        input.download_base,
        "VesperPlayerAndroidComposeHost-android-arm64-v8a-debug-signed.apk",
        "Android Compose sample APK, debug-signed for side-load evaluation only",
    );
    download(
        &mut output,
        input.download_base,
        "VesperPlayerFlutterHost-android-arm64-v8a-debug-signed.apk",
        "Flutter Android sample APK, debug-signed for side-load evaluation only",
    );
    blank(&mut output);
    line(&mut output, "### iOS");
    blank(&mut output);
    for (asset, label) in [
        (
            "VesperPlayerKit-ios-arm64.framework.zip",
            "iOS device framework",
        ),
        (
            "VesperPlayerKit-ios-simulator-arm64.framework.zip",
            "Apple Silicon simulator framework",
        ),
        ("VesperPlayerKit.xcframework.zip", "Combined XCFramework"),
        (
            "VesperFFmpegAVCodec.xcframework.zip",
            "Optional FFmpeg avcodec runtime component XCFramework",
        ),
        (
            "VesperFFmpegAVFormat.xcframework.zip",
            "Optional FFmpeg avformat runtime component XCFramework",
        ),
        (
            "VesperFFmpegAVUtil.xcframework.zip",
            "Optional FFmpeg avutil runtime component XCFramework",
        ),
        (
            "VesperPlayerRemuxFfmpegPlugin.xcframework.zip",
            "Optional FFmpeg-backed remux plugin XCFramework",
        ),
        (
            "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip",
            "Optional FFmpeg-backed source normalizer plugin XCFramework",
        ),
        (
            "VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip",
            "Optional VideoToolbox decoder plugin XCFramework",
        ),
    ] {
        download(&mut output, input.download_base, asset, label);
    }
    blank(&mut output);
    line(&mut output, "### Checksums and Licensing");
    blank(&mut output);
    download(
        &mut output,
        input.download_base,
        "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip",
        "Mandatory FFmpeg licenses, notices, build metadata, and LGPL relinking instructions for the optional iOS frameworks",
    );
    download(
        &mut output,
        input.download_base,
        input.ffmpeg_source,
        "Exact corresponding FFmpeg source for the optional iOS frameworks",
    );
    download(
        &mut output,
        input.download_base,
        "SHA256SUMS.txt",
        "SHA-256 checksums for release artifacts",
    );
    blank(&mut output);
    line(
        &mut output,
        "Tagged releases include the six optional iOS plugin/runtime XCFrameworks only together with the FFmpeg compliance bundle and exact corresponding source asset. FFmpeg remains separately licensed; its notices, configure metadata, source, and LGPL relinking boundary are not covered by Vesper's Apache-2.0 source license.",
    );
    blank(&mut output);
    line(&mut output, "## Release Contributors");
    blank(&mut output);
    if input.contributors.is_empty() {
        line(&mut output, "- No contributor metadata found");
    } else {
        for contributor in input.contributors {
            line(&mut output, &format!("- {contributor}"));
        }
    }
    output
}

fn line(output: &mut String, value: &str) {
    output.push_str(value);
    output.push('\n');
}

fn blank(output: &mut String) {
    output.push('\n');
}

fn block(output: &mut String, value: &str) {
    output.push_str(value);
    if !value.ends_with('\n') {
        output.push('\n');
    }
}

fn download(output: &mut String, base: Option<&str>, asset: &str, label: &str) {
    match base {
        Some(base) => line(output, &format!("- [{asset}]({base}/{asset}) - {label}")),
        None => line(output, &format!("- `{asset}` - {label}")),
    }
}

fn find_ffmpeg_source_asset(directory: &Path) -> ReleaseResult<String> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| {
        ReleaseError::storage(format!("failed to read '{}': {error}", directory.display()))
    })? {
        let entry = entry.map_err(|error| {
            ReleaseError::storage(format!(
                "failed to read directory entry in '{}': {error}",
                directory.display()
            ))
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with(FFMPEG_SOURCE_PREFIX) && name.ends_with(FFMPEG_SOURCE_SUFFIX) {
            matches.push(name);
        }
    }
    matches.sort();
    if matches.len() != 1 {
        let mut message = format!(
            "Expected exactly one optional iOS FFmpeg source asset beside the release notes, found {}.",
            matches.len()
        );
        for name in matches {
            message.push_str("\n  ");
            message.push_str(&directory.join(name).display().to_string());
        }
        return Err(ReleaseError::input(message));
    }
    matches
        .pop()
        .ok_or_else(|| ReleaseError::input("Unable to resolve the FFmpeg source asset."))
}

fn resolve_repository_url(
    root: &Path,
    environment: &ReleaseEnvironment,
) -> ReleaseResult<Option<String>> {
    if let (Some(server), Some(repository)) = (
        environment.github_server_url.as_deref(),
        environment.github_repository.as_deref(),
    ) {
        return Ok(Some(format!(
            "{}/{}",
            server.trim_end_matches('/'),
            repository.trim_start_matches('/')
        )));
    }
    let Some(mut origin) = git_output_optional(root, &["config", "--get", "remote.origin.url"])?
    else {
        return Ok(None);
    };
    if let Some(value) = origin.strip_suffix(".git") {
        origin = value.to_owned();
    }
    if let Some(value) = origin.strip_prefix("git@github.com:") {
        return Ok(Some(format!("https://github.com/{value}")));
    }
    if origin.starts_with("https://github.com/") || origin.starts_with("http://github.com/") {
        return Ok(Some(origin));
    }
    Ok(None)
}

fn release_channel(tag: &str) -> &'static str {
    let value = tag.strip_prefix('v').unwrap_or(tag);
    let components = value.split('.').collect::<Vec<_>>();
    if components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        "stable"
    } else {
        "prerelease"
    }
}

fn collect_contributors(root: &Path, range: &str) -> ReleaseResult<Vec<String>> {
    let values = git_output(root, &["log", "--format=%ae%x09%an", range])?;
    let mut seen = HashSet::new();
    let mut contributors = Vec::new();
    for line in values.lines() {
        let Some((email, author)) = line.split_once('\t') else {
            continue;
        };
        if seen.insert(email.to_owned()) {
            contributors.push(author.to_owned());
        }
    }
    Ok(contributors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_channel_matches_tag_shape() {
        assert_eq!(release_channel("v0.4.0"), "stable");
        assert_eq!(release_channel("0.4.0"), "stable");
        assert_eq!(release_channel("v0.4.0-rc.1"), "prerelease");
    }
}
