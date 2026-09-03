use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::release::{
    ReleaseEnvironment, ReleaseError, ReleaseResult, atomic_write_output, git_output,
    git_output_optional,
};

const FFMPEG_SOURCE_PREFIX: &str = "VesperPlayerOptionalPlugins-FFmpeg-";
const FFMPEG_SOURCE_SUFFIX: &str = "-source.tar.xz";

const INITIAL_RELEASE_EN: &str = r#"## Fixes

- This is the first VesperPlayerKit release candidate, so there is no prior release regression set. This cut includes pre-release stability fixes for Dart error-code construction, iOS insecure-HTTP rejection, Android / CI release dependency wiring, Android sample APK packaging, and iOS framework staging.
- FFmpeg, Gradle, bridge-shim, Android sample APK, and iOS framework release scripts were tightened so mobile binary artifacts can be generated reliably from the tag workflow.

## New Capabilities

- Android ships a core host-kit AAR, Compose binding, Compose UI package, external playback extension, and split FFmpeg runtime package for arm64-v8a devices.
- iOS ships a device framework, Apple Silicon simulator framework, combined XCFramework, and eight optional sibling XCFrameworks covering the three FFmpeg runtime components, two FFmpeg-backed plugins, the VideoToolbox decoder plugin, the diagnostic FrameProcessor plugin, and the performance diagnostics BenchmarkSink plugin.
- Flutter and Android Compose sample apps are published with the release for quick integration checks.
- Core capabilities include DASH / HLS bridging, offline download and export, remote media references, request-header forwarding, SegmentBase / byte-range handling, DLNA / AirPlay external playback, and FFmpeg remux post-processing.

## Improvements

- Android defaults to hardware decoding and the SurfaceView path, with release artifacts split by module so host apps can depend only on the capabilities they need.
- iOS keeps the SPM / XCFramework distribution path and separates all optional plugins from the main SDK. Tagged releases publish the FFmpeg-backed frameworks only with the mandatory compliance bundle and exact corresponding source, preserving FFmpeg's independent license and LGPL relinking boundary.
- The release flow generates checksums and verifies Android / iOS artifacts contain only the expected arm64 slices.
"#;

const INITIAL_RELEASE_ZH: &str = r#"## 修复问题

- 这是 VesperPlayerKit 的首次候选发布，没有历史版本回归修复对比。本轮发布前已补齐 Dart 错误码构造、iOS 不安全 HTTP 拦截、Android / CI 发布依赖声明、Android 示例 APK 打包，以及 iOS framework 暂存等稳定性问题。
- 修正 FFmpeg、Gradle、bridge shim、Android 示例 APK 和 iOS framework 发布脚本细节，让移动端二进制产物可以由 tag 工作流稳定生成。

## 新增功能

- Android 提供核心 Host Kit AAR、Compose 绑定、Compose UI 包、外部播放扩展和 FFmpeg Runtime 拆分包，面向 arm64-v8a 设备发布。
- iOS 提供真机 framework、Apple Silicon 模拟器 framework、合并 XCFramework，以及由三个 FFmpeg runtime component、两个 FFmpeg-backed 插件、VideoToolbox decoder 插件、diagnostic FrameProcessor 插件和性能诊断 BenchmarkSink 插件组成的八个可选同级 XCFramework。
- Flutter 示例和 Android Compose 示例随 release 一起提供，方便快速验证接入效果。
- 核心能力覆盖 DASH / HLS 桥接、离线下载与导出、远程媒体引用、请求头透传、SegmentBase / byte-range 处理、DLNA / AirPlay 外部播放，以及 FFmpeg remux 后处理。

## 优化改进

- Android 默认走硬件解码和 SurfaceView 路径，发布产物按模块拆分，便于宿主应用只接入需要的能力。
- iOS 保持 SPM / XCFramework 分发路径，并把全部可选插件与主 SDK 分离。Tagged Release 仅在同时提供强制合规包和精确对应源码时发布 FFmpeg-backed frameworks，以保留 FFmpeg 独立许可和 LGPL relinking 边界。
- 发布流程会生成校验和，并校验 Android / iOS 产物只包含预期的 arm64 切片。
"#;

const INCREMENTAL_RELEASE_EN: &str = r#"## Fixes

- Fixes for this version are listed in the module-grouped change summary below.

## New Capabilities

- New capabilities for this version are listed in the change summary below and reflected in the platform-specific downloads.

## Improvements

- Build, release, platform integration, and runtime improvements are listed in the change summary below.
"#;

const INCREMENTAL_RELEASE_ZH: &str = r#"## 修复问题

- 本版本的修复项请查看下方按模块整理的变更摘要。

## 新增功能

- 本版本新增能力请查看下方变更摘要和对应平台下载产物。

## 优化改进

- 构建、发布、平台集成和运行时优化请查看下方变更摘要。
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitGroup {
    Mobile,
    Desktop,
    Core,
    Media,
    Tooling,
    Docs,
    Other,
}

impl CommitGroup {
    const ORDERED: [Self; 7] = [
        Self::Mobile,
        Self::Desktop,
        Self::Core,
        Self::Media,
        Self::Tooling,
        Self::Docs,
        Self::Other,
    ];

    const fn english(self) -> &'static str {
        match self {
            Self::Mobile => "Mobile Platform Kits",
            Self::Desktop => "Desktop Runtime & Demo",
            Self::Core => "Core Runtime & FFI",
            Self::Media => "Media Pipeline",
            Self::Tooling => "CI & Release Tooling",
            Self::Docs => "Docs & Planning",
            Self::Other => "Other Changes",
        }
    }

    const fn chinese(self) -> &'static str {
        match self {
            Self::Mobile => "移动端平台套件",
            Self::Desktop => "桌面运行时与示例",
            Self::Core => "核心运行时与 FFI",
            Self::Media => "媒体管线",
            Self::Tooling => "CI 与发布工具",
            Self::Docs => "文档与规划",
            Self::Other => "其他变更",
        }
    }
}

#[derive(Debug, Clone)]
struct ReleaseCommit {
    group: CommitGroup,
    short_sha: String,
    subject: String,
    author: String,
}

pub fn generate(
    root: &Path,
    environment: &ReleaseEnvironment,
    tag: &str,
    output: Option<&Path>,
) -> ReleaseResult<PathBuf> {
    let commit_ref = format!("{tag}^{{commit}}");
    git_output(root, &["rev-parse", "--verify", &commit_ref])?;

    let previous_ref = format!("{tag}^");
    let previous_tag =
        git_output_optional(root, &["describe", "--tags", "--abbrev=0", &previous_ref])?
            .filter(|value| !value.is_empty());
    let range = previous_tag
        .as_ref()
        .map(|previous| format!("{previous}..{tag}"))
        .unwrap_or_else(|| tag.to_owned());
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
    let commits = collect_commits(root, &range)?;
    let contributors = collect_contributors(root, &range)?;
    let notes = render_notes(NotesInput {
        tag,
        previous_tag: previous_tag.as_deref(),
        compare_url: compare_url.as_deref(),
        download_base: download_base.as_deref(),
        release_channel: release_channel(tag),
        ffmpeg_source: &ffmpeg_source,
        commits: &commits,
        contributors: &contributors,
    });
    atomic_write_output(&output_path, notes.as_bytes())?;
    Ok(output_path)
}

struct NotesInput<'a> {
    tag: &'a str,
    previous_tag: Option<&'a str>,
    compare_url: Option<&'a str>,
    download_base: Option<&'a str>,
    release_channel: &'static str,
    ffmpeg_source: &'a str,
    commits: &'a [ReleaseCommit],
    contributors: &'a [String],
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
    block(
        &mut output,
        if input.previous_tag.is_some() {
            INCREMENTAL_RELEASE_EN
        } else {
            INITIAL_RELEASE_EN
        },
    );
    blank(&mut output);
    line(&mut output, "## Change Summary");
    blank(&mut output);
    if input.previous_tag.is_none() {
        line(
            &mut output,
            "- This is the first GitHub release. The commit history has been condensed into the capability summary above for first-time integration review.",
        );
    } else if input.commits.is_empty() {
        line(
            &mut output,
            "- No non-merge commits were found in this range.",
        );
    } else {
        emit_grouped_commits(&mut output, input.commits, false);
    }
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
    block(
        &mut output,
        if input.previous_tag.is_some() {
            INCREMENTAL_RELEASE_ZH
        } else {
            INITIAL_RELEASE_ZH
        },
    );
    blank(&mut output);
    line(&mut output, "## 变更摘要");
    blank(&mut output);
    if input.previous_tag.is_none() {
        line(
            &mut output,
            "- 这是首次 GitHub Release。英文提交历史已整理为上方能力摘要，方便首次接入评估。",
        );
    } else if input.commits.is_empty() {
        line(&mut output, "- 此范围内没有非合并提交。");
    } else {
        emit_grouped_commits(&mut output, input.commits, true);
    }
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
        (
            "VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip",
            "Optional diagnostic FrameProcessor plugin XCFramework",
        ),
        (
            "VesperPlayerPerformanceDiagnosticsPlugin.xcframework.zip",
            "Optional performance diagnostics BenchmarkSink plugin XCFramework",
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
        "Tagged releases include the eight optional iOS plugin/runtime XCFrameworks only together with the FFmpeg compliance bundle and exact corresponding source asset. FFmpeg remains separately licensed; its notices, configure metadata, source, and LGPL relinking boundary are not covered by Vesper's Apache-2.0 source license.",
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

fn emit_grouped_commits(output: &mut String, commits: &[ReleaseCommit], chinese: bool) {
    for group in CommitGroup::ORDERED {
        let matching = commits
            .iter()
            .filter(|commit| commit.group == group)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        let label = if chinese {
            group.chinese()
        } else {
            group.english()
        };
        line(output, &format!("### {label}"));
        blank(output);
        for commit in matching {
            let subject = if chinese {
                translate_commit_subject(&commit.subject)
            } else {
                commit.subject.as_str()
            };
            line(
                output,
                &format!("- `{}` {} ({})", commit.short_sha, subject, commit.author),
            );
        }
        blank(output);
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

fn collect_commits(root: &Path, range: &str) -> ReleaseResult<Vec<ReleaseCommit>> {
    let hashes = git_output(root, &["log", "--no-merges", "--format=%H", range])?;
    let mut commits = Vec::new();
    for sha in hashes.lines().filter(|line| !line.is_empty()) {
        let short_sha = git_output(root, &["rev-parse", "--short", sha])?;
        let subject = git_output(root, &["log", "-1", "--format=%s", sha])?;
        let author = git_output(root, &["log", "-1", "--format=%an", sha])?;
        let changed_paths = git_output(root, &["show", "--pretty=", "--name-only", sha])?;
        commits.push(ReleaseCommit {
            group: classify_commit_group(&changed_paths),
            short_sha,
            subject,
            author,
        });
    }
    Ok(commits)
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

fn classify_commit_group(changed_paths: &str) -> CommitGroup {
    if any_path_starts_with(
        changed_paths,
        &[
            "lib/android/",
            "lib/ios/",
            "examples/android-compose-host/",
            "examples/ios-swift-host/",
            "crates/platform/mobile/",
            "crates/platform/jni/",
        ],
    ) {
        return CommitGroup::Mobile;
    }
    if any_path_starts_with(
        changed_paths,
        &[
            "examples/basic-player/",
            "crates/platform/desktop/",
            "crates/platform/common/player-platform-desktop/",
            "crates/platform/common/player-platform-apple/",
        ],
    ) {
        return CommitGroup::Desktop;
    }
    if any_path_starts_with(changed_paths, &["crates/core/"]) {
        return CommitGroup::Core;
    }
    if any_path_starts_with(
        changed_paths,
        &["crates/backend/", "crates/audio/", "crates/render/"],
    ) {
        return CommitGroup::Media;
    }
    if any_path_starts_with(changed_paths, &[".github/workflows/", "scripts/"]) {
        return CommitGroup::Tooling;
    }
    if changed_paths
        .lines()
        .any(|path| path.starts_with("docs/") || path == "ROADMAP.md" || path == "README.md")
    {
        return CommitGroup::Docs;
    }
    CommitGroup::Other
}

fn any_path_starts_with(changed_paths: &str, prefixes: &[&str]) -> bool {
    changed_paths
        .lines()
        .any(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)))
}

fn translate_commit_subject(subject: &str) -> &str {
    match subject {
        "fix: add error codes to VesperPlayerError for better error handling fix: reject insecure HTTP URLs in VesperForegroundDownloadExecutor" => {
            "为 VesperPlayerError 补齐错误码以改进错误处理，并在 VesperForegroundDownloadExecutor 中拒绝不安全的 HTTP URL"
        }
        "refactor: rename error ordinal functions for consistency with JNI terminology" => {
            "重命名错误序号相关函数，使其与 JNI 术语保持一致"
        }
        "Add scripts for synchronizing and verifying VesperPlayerKit bridge shim" => {
            "新增 VesperPlayerKit bridge shim 同步与校验脚本"
        }
        "fix: reorder ffmpeg command arguments for consistency and clarity" => {
            "调整 FFmpeg 命令参数顺序，使脚本更一致、更清晰"
        }
        "Refactor Android build scripts to improve Gradle resolution and add sample APK staging" => {
            "重构 Android 构建脚本，改进 Gradle 解析并新增示例 APK 暂存"
        }
        "feat(download): implement download types and structures for asset management" => {
            "实现下载资产管理所需的类型与数据结构"
        }
        "feat(dlna): refactor DLNA session methods for improved async handling and error management" => {
            "重构 DLNA 会话方法，改进异步处理与错误管理"
        }
        "feat: Add external playback support and AirPlay integration" => {
            "新增外部播放支持与 AirPlay 集成"
        }
        "feat(dash): enhance DASH support with remote media references and request headers" => {
            "增强 DASH 对远程媒体引用和请求头的支持"
        }
        "feat(dash): add support for remote media references in DASH resource resolvers and tests" => {
            "在 DASH 资源解析器与测试中加入远程媒体引用支持"
        }
        "Enhance DASH handling with support for SegmentBase and byte range requests" => {
            "增强 DASH 处理能力，支持 SegmentBase 与字节范围请求"
        }
        "feat(relay): add prewarm functionality and validation for DASH sources in VesperRelayServer" => {
            "在 VesperRelayServer 中为 DASH 源加入预热能力和校验"
        }
        "feat(dlna): add refresh functionality for external routes and improve diagnostic logging" => {
            "为外部路由加入刷新能力，并改进诊断日志"
        }
        "Add support for local DASH sources and enhance DASH parsing" => {
            "新增本地 DASH 源支持并增强 DASH 解析"
        }
        "feat(dlna): add asynchronous methods for playback control and loading media" => {
            "为播放控制和媒体加载加入异步 DLNA 方法"
        }
        "feat(external-playback): enhance diagnostic messages with HTTP status in VesperRelayServer" => {
            "在 VesperRelayServer 外部播放诊断信息中加入 HTTP 状态"
        }
        "feat(dlna): improve error handling in VesperDlnaSoapClient and add detailed failure messages" => {
            "改进 VesperDlnaSoapClient 错误处理并加入更详细的失败信息"
        }
        "feat(dlna): enhance DLNA device route matching and discovery handling" => {
            "增强 DLNA 设备路由匹配与发现处理"
        }
        "refactor!(mobile): release version 0.3.0, refactor module structure and FFmpeg build process" => {
            "发布 0.3.0 移动端结构，重构模块划分与 FFmpeg 构建流程"
        }
        _ => subject,
    }
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

    #[test]
    fn path_groups_keep_legacy_priority() {
        assert_eq!(
            classify_commit_group("scripts/build.sh\nlib/android/build.gradle.kts\n"),
            CommitGroup::Mobile
        );
        assert_eq!(
            classify_commit_group("crates/core/player-runtime/src/lib.rs\n"),
            CommitGroup::Core
        );
        assert_eq!(classify_commit_group("LICENSE\n"), CommitGroup::Other);
    }
}
