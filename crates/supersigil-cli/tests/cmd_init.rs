use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

#[test]
fn init_non_tty_creates_config_and_skills() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(tmp.path().join("supersigil.toml").exists());
    assert!(
        tmp.path()
            .join(".agents/skills/feature-development/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/feature-specification/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/retroactive-specification/SKILL.md")
            .exists()
    );
    assert!(
        tmp.path()
            .join(".agents/skills/spec-driven-development/SKILL.md")
            .exists()
    );
}

#[test]
fn init_no_skills_creates_config_only() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--no-skills"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(tmp.path().join("supersigil.toml").exists());
    assert!(!tmp.path().join(".agents/skills").exists());
}

#[test]
fn init_skills_path_writes_to_custom_dir_and_updates_toml() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--skills-path", "custom/skills"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("custom/skills/feature-development/SKILL.md")
            .exists()
    );

    let toml_content = fs::read_to_string(tmp.path().join("supersigil.toml")).unwrap();
    assert!(toml_content.contains("[skills]"), "toml: {toml_content}");
    assert!(
        toml_content.contains(r#"path = "custom/skills""#),
        "toml: {toml_content}"
    );
}

#[test]
fn init_skills_flag_without_path_uses_default() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--skills"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path()
            .join(".agents/skills/feature-development/SKILL.md")
            .exists()
    );

    let toml_content = fs::read_to_string(tmp.path().join("supersigil.toml")).unwrap();
    assert!(
        !toml_content.contains("[skills]"),
        "default path should not be persisted: {toml_content}"
    );
}

#[test]
fn init_yes_flag_creates_config_and_skills() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "-y"])
        .current_dir(tmp.path())
        .assert()
        .success();

    assert!(tmp.path().join("supersigil.toml").exists());
    assert!(
        tmp.path()
            .join(".agents/skills/feature-development/SKILL.md")
            .exists()
    );
}

#[test]
fn init_skills_and_no_skills_conflict() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--skills", "--no-skills"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[test]
fn init_prints_skill_count() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin_cmd!("supersigil")
        .args(["init", "-y"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let combined = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        combined.contains("4 skills"),
        "should print skill count: {combined}"
    );
}

#[test]
fn init_skills_path_and_no_skills_conflict() {
    let tmp = TempDir::new().unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--skills-path", "custom", "--no-skills"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[test]
fn init_fails_if_config_exists() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("supersigil.toml"),
        "paths = [\"specs/**/*.mdx\"]\n",
    )
    .unwrap();
    cargo_bin_cmd!("supersigil")
        .args(["init", "--no-skills"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}
