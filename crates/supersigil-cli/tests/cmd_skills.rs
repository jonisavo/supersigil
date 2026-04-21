//! Integration tests for the `skills` command.

mod common;

use std::fs;

use assert_cmd::assert::OutputAssertExt;
use common::supersigil_cmd;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[verifies(
    "skills-install/req#req-4-1",
    "skills-install/req#req-4-3",
    "skills-install/req#req-4-4"
)]
#[test]
fn skills_install_without_toml_uses_default_path() {
    let tmp = TempDir::new().unwrap();
    supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join(".agents/skills/ss-feature-development/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/ss-feature-specification/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/ss-retroactive-specification/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/ss-spec-driven-development/SKILL.md")
            .exists()
    );
}

#[verifies("skills-install/req#req-4-1")]
#[test]
fn skills_install_with_path_flag() {
    let tmp = TempDir::new().unwrap();
    supersigil_cmd()
        .args(["skills", "install", "--path", "custom/dir"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("custom/dir/ss-feature-development/SKILL.md")
            .exists()
    );
    assert!(!tmp.path().join(".agents/skills").exists());
}

#[verifies("skills-install/req#req-4-3")]
#[test]
fn skills_install_reads_path_from_toml() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n\n[skills]\npath = \"my/skills\"\n",
    )
    .unwrap();

    supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("my/skills/ss-feature-development/SKILL.md")
            .exists()
    );
}

#[verifies("skills-install/req#req-4-3")]
#[test]
fn skills_install_path_flag_overrides_toml() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n\n[skills]\npath = \"from/toml\"\n",
    )
    .unwrap();

    supersigil_cmd()
        .args(["skills", "install", "--path", "from/flag"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("from/flag/ss-feature-development/SKILL.md")
            .exists()
    );
    assert!(!tmp.path().join("from/toml").exists());
}

#[verifies("skills-install/req#req-4-5")]
#[test]
fn skills_install_prints_count() {
    let tmp = TempDir::new().unwrap();
    let output = supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let combined = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        combined.contains("6 skills"),
        "should print skill count: {combined}"
    );
}

#[test]
fn skills_install_prints_skill_chooser() {
    let tmp = TempDir::new().unwrap();
    let output = supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ss-feature-development"),
        "should print skill chooser to stderr: {stderr}"
    );
}

#[verifies("skills-install/req#req-4-2")]
#[test]
fn skills_install_overwrites_existing() {
    let tmp = TempDir::new().unwrap();
    let skill_path = tmp
        .path()
        .join(".agents/skills/ss-feature-development/SKILL.md");

    supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let original = fs::read_to_string(&skill_path).unwrap();
    fs::write(&skill_path, "tampered").unwrap();

    supersigil_cmd()
        .args(["skills", "install"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let restored = fs::read_to_string(&skill_path).unwrap();
    assert_eq!(original, restored);
}
