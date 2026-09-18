#![cfg(vesper_source_checkout)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CHANGELOG: &str = "# Changelog\n\n## Unreleased\n\n- Future change.\n\n\
## 0.6.2 - 2026-09-18\n\n<!-- release-notes:en -->\n\n### Fixed\n\n\
- Restore the retained inline surface after fullscreen,\n  including portrait video.\n\
- Preserve **manual PiP** after automatic entry is disabled.\n\n\
<!-- release-notes:zh-CN -->\n\n### 修复\n\n\
- 退出全屏后恢复保留的内嵌播放画面，包括竖屏视频。\n\
- 关闭自动进入后，仍然支持**手动画中画**。\n\n\
## 0.6.1 - 2026-09-17\n\n- Earlier change.\n";

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run fixture git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture(changelog: &str, tag: &str, previous: bool) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("create release notes fixture");
    let root = directory.path();
    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Release Fixture"]);
    git(root, &["config", "user.email", "release@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "tag.gpgsign", "false"]);
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write workspace marker");
    if previous {
        git(root, &["add", "Cargo.toml"]);
        git(root, &["commit", "--quiet", "-m", "Initial release"]);
        git(root, &["tag", "v0.6.0"]);
        git(
            root,
            &["commit", "--quiet", "--allow-empty", "-m", "Plugin release"],
        );
        git(root, &["tag", "plugin-sdk-v0.6.1"]);
    }
    fs::write(root.join("CHANGELOG.md"), changelog).expect("write tagged changelog");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            "Short commit title is not a release summary",
        ],
    );
    git(root, &["tag", tag]);
    fs::create_dir(root.join("assets")).expect("create asset fixture");
    fs::write(
        root.join("assets/VesperPlayerOptionalPlugins-FFmpeg-fixture-source.tar.xz"),
        [],
    )
    .expect("write source asset name fixture");
    directory
}

fn output_path(root: &Path) -> PathBuf {
    root.join("assets/RELEASE_NOTES.md")
}

fn generate(root: &Path, tag: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vesper"))
        .current_dir(root)
        .env("VESPER_REPO_ROOT", root)
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REPOSITORY", "example/vesper")
        .args(["release", "notes", tag])
        .arg(output_path(root))
        .output()
        .expect("generate release notes")
}

#[test]
fn release_summaries_use_tagged_changelog_in_each_language() {
    let directory = fixture(CHANGELOG, "v0.6.2", true);
    let root = directory.path();
    fs::write(
        root.join("CHANGELOG.md"),
        "Uncommitted content must not be published.",
    )
    .expect("dirty working changelog");
    let output = generate(root, "v0.6.2");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let notes = fs::read_to_string(output_path(root)).expect("read notes");
    let (english, chinese) = notes
        .split_once("# VesperPlayerKit v0.6.2 中文说明")
        .expect("both languages");
    assert!(english.contains("### Fixed\n\n- Restore the retained inline surface after fullscreen,\n  including portrait video."));
    assert!(english.contains("**manual PiP**"));
    assert!(!english.contains("退出全屏"));
    assert!(chinese.contains("### 修复\n\n- 退出全屏后恢复保留的内嵌播放画面，包括竖屏视频。"));
    assert!(!chinese.contains("Restore the retained inline surface"));
    assert!(notes.contains("Previous version: `v0.6.0`"));
    for excluded in [
        "Short commit title",
        "Future change",
        "Earlier change",
        "Uncommitted content",
        "<!-- release-notes:",
    ] {
        assert!(!notes.contains(excluded), "unexpected content: {excluded}");
    }
}

#[test]
fn first_prerelease_uses_its_exact_changelog_section() {
    let changelog = CHANGELOG.replace("## 0.6.2 -", "## 0.6.2-rc.1 -");
    let directory = fixture(&changelog, "v0.6.2-rc.1", false);
    let output = generate(directory.path(), "v0.6.2-rc.1");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let notes = fs::read_to_string(output_path(directory.path())).expect("read notes");
    assert!(notes.contains("Restore the retained inline surface"));
    assert!(notes.contains("退出全屏后恢复"));
    assert!(notes.contains("Release channel: prerelease"));
}

#[test]
fn missing_version_or_translation_does_not_publish_a_commit_fallback() {
    for (changelog, diagnostic) in [
        (CHANGELOG.replace("## 0.6.2 -", "## 0.6.2-rc.1 -"), "0.6.2"),
        (
            CHANGELOG
                .split("<!-- release-notes:zh-CN -->")
                .next()
                .expect("English block")
                .to_owned(),
            "zh-CN",
        ),
        (
            CHANGELOG.replace("<!-- release-notes:en -->", ""),
            "release-notes:en",
        ),
    ] {
        let directory = fixture(&changelog, "v0.6.2", true);
        fs::write(output_path(directory.path()), "Existing notes").expect("write old output");
        let output = generate(directory.path(), "v0.6.2");
        assert!(!output.status.success(), "invalid changelog was accepted");
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        assert_eq!(
            fs::read_to_string(output_path(directory.path())).expect("read old notes"),
            "Existing notes"
        );
    }
}

#[test]
fn changelog_fenced_examples_are_preserved_as_content() {
    let changelog = CHANGELOG.replace(
        "- Preserve **manual PiP** after automatic entry is disabled.",
        "- Preserve **manual PiP** after automatic entry is disabled.\n\n```markdown\n## 0.6.2 - example\n<!-- release-notes:zh-CN -->\n```",
    );
    let directory = fixture(&changelog, "v0.6.2", true);
    let output = generate(directory.path(), "v0.6.2");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let notes = fs::read_to_string(output_path(directory.path())).expect("read notes");
    assert!(notes.contains("```markdown\n## 0.6.2 - example\n<!-- release-notes:zh-CN -->\n```"));
}

#[test]
fn duplicate_version_or_empty_translation_is_rejected() {
    for changelog in [
        format!("{CHANGELOG}\n{CHANGELOG}"),
        CHANGELOG.replace("### 修复\n\n- 退出全屏后恢复保留的内嵌播放画面，包括竖屏视频。\n- 关闭自动进入后，仍然支持**手动画中画**。", ""),
    ] {
        let directory = fixture(&changelog, "v0.6.2", true);
        let output = generate(directory.path(), "v0.6.2");
        assert!(!output.status.success(), "invalid changelog was accepted");
        assert!(!output_path(directory.path()).exists());
    }
}
