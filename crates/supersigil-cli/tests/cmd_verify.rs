mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[path = "cmd_verify/examples.rs"]
mod examples;
#[path = "cmd_verify/flags.rs"]
mod flags;
#[path = "cmd_verify/plugins.rs"]
mod plugins;
#[path = "cmd_verify/projects.rs"]
mod projects;

fn write_config(root: &Path, content: &str) {
    fs::write(root.join("supersigil.toml"), content).unwrap();
    fs::create_dir_all(root.join("specs")).unwrap();
}

fn setup_explicit_evidence_fixture(root: &Path, config: &str) {
    write_config(root, config);
    write_requirement_with_explicit_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "# explicit authored evidence\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
}

fn write_requirement_with_explicit_evidence(root: &Path) {
    common::write_spec_doc(
        root,
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

fn write_requirement_for_plugin_evidence(root: &Path) {
    common::write_spec_doc(
        root,
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

fn write_requirement_with_shared_file_glob_evidence(root: &Path) {
    common::write_spec_doc(
        root,
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
  </Criterion>
  <Criterion id="ac-2">
    Must keep the session alive
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

fn setup_plugin_failure_fixture(root: &Path) {
    setup_explicit_evidence_fixture(
        root,
        r#"paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = ["rust"]
"#,
    );
}

fn setup_partial_plugin_warning_fixture(root: &Path, extra_config: &str) {
    common::setup_project_with_rust_plugin_and_tests(root, "tests/**/*.rs", extra_config);
    write_requirement_for_plugin_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\n#[verifies(\"auth/req#ac-1\")]\nfn login_succeeds() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/bad.rs"),
        "#[verifies(\"auth/req#ac-1\")] fn { broken\n",
    )
    .unwrap();
}

fn setup_missing_evidence_fixture(root: &Path) {
    common::setup_project_with_rust_plugin_and_tests(root, "tests/**/*.rs", "");
    write_requirement_for_plugin_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\nfn login_succeeds() {}\n",
    )
    .unwrap();
}

fn setup_shared_file_glob_fixture(root: &Path) {
    write_config(
        root,
        r#"paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = []
"#,
    );
    write_requirement_with_shared_file_glob_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "# shared authored evidence\n",
    )
    .unwrap();
}

fn setup_explicit_evidence_only_fixture(root: &Path) {
    setup_explicit_evidence_fixture(
        root,
        r#"paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = []
"#,
    );
}

fn setup_clean_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_spec_doc(
        root,
        "specs/examples.md",
        "examples/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="examples-1">cargo-test examples run during verify</Criterion>
</AcceptanceCriteria>

<Example
  id="cargo-pass"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>
  <Expected status="0" contains="cargo-test-pass" />
</Example>
```

```rust supersigil-ref=cargo-pass
#[test]
fn cargo_pass() {
    println!("cargo-test-pass");
}
```"#,
    );
}

fn setup_failing_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_spec_doc(
        root,
        "specs/examples.md",
        "examples/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="examples-1">cargo-test examples run during verify</Criterion>
</AcceptanceCriteria>

<Example
  id="cargo-pass"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>
  <Expected status="0" contains="cargo-test-pass" />
</Example>

<Example
  id="cargo-fail"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>
  <Expected status="0" />
</Example>
```

```rust supersigil-ref=cargo-pass
#[test]
fn cargo_pass() {
    println!("cargo-test-pass");
}
```

```rust supersigil-ref=cargo-fail
#[test]
fn cargo_fail() {
    assert_eq!(1, 2);
}
```"#,
    );
}

fn setup_non_blocking_failing_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_spec_doc(
        root,
        "specs/examples.md",
        "examples/req",
        Some("requirements"),
        Some("draft"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="examples-1">draft examples can fail without blocking verify</Criterion>
</AcceptanceCriteria>

<Example
  id="body-mismatch"
  lang="sh"
  runner="sh"
  verifies="examples/req#examples-1"
>
  <Expected status="0" format="regex" />
</Example>
```

```sh supersigil-ref=body-mismatch
printf 'line1\nline2\n'
```

```regex supersigil-ref=body-mismatch#expected
expected-output
```"#,
    );
}
