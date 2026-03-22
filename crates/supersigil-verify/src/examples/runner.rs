//! Runner dispatch and execution for executable examples.
//!
//! Resolves runner names to built-in or user-defined runners, executes
//! example code, and produces [`ExampleResult`]s by matching output
//! against expected specifications.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use supersigil_core::ExamplesConfig;

use super::matcher;
use super::types::{ExampleOutcome, ExampleResult, ExampleSpec, MatchCheck, MatchFailure};

// ---------------------------------------------------------------------------
// ResolvedRunner
// ---------------------------------------------------------------------------

/// A resolved runner ready for execution.
#[derive(Debug)]
enum ResolvedRunner {
    /// A subprocess runner with a command template.
    Subprocess(String),
    /// The built-in cargo-test runner.
    CargoTest,
    /// The built-in native HTTP runner.
    Http,
}

// ---------------------------------------------------------------------------
// RunnerOutput
// ---------------------------------------------------------------------------

/// Error from example execution — either a timeout or an execution error.
#[derive(Debug)]
enum RunError {
    Timeout,
    Other(String),
}

/// Captured output from a runner execution.
#[derive(Debug)]
struct RunnerOutput {
    stdout: String,
    /// Captured stderr (used for diagnostics in error reporting).
    #[allow(dead_code, reason = "reserved for future diagnostic reporting")]
    stderr: String,
    status: Option<u32>,
}

// ---------------------------------------------------------------------------
// Runner resolution
// ---------------------------------------------------------------------------

/// Resolve a runner name to a built-in or user-defined runner.
///
/// User-defined runners (from config) take precedence over built-ins.
fn resolve_runner(name: &str, config: &ExamplesConfig) -> Result<ResolvedRunner, String> {
    // User-defined runners take precedence
    if let Some(user_runner) = config.runners.get(name) {
        return Ok(ResolvedRunner::Subprocess(user_runner.command.clone()));
    }
    match name {
        "sh" => Ok(ResolvedRunner::Subprocess("sh {file}".into())),
        "cargo-test" => Ok(ResolvedRunner::CargoTest),
        "http" => Ok(ResolvedRunner::Http),
        _ => Err(format!("unknown runner: {name}")),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Execute an example and return the result.
///
/// This is the main entry point for running a single example. It resolves
/// the runner, executes the code, matches the output, and returns the result.
#[must_use]
pub fn run_example(
    spec: &ExampleSpec,
    project_root: &Path,
    config: &ExamplesConfig,
) -> ExampleResult {
    let start = std::time::Instant::now();
    let outcome = match execute(spec, project_root, config) {
        Ok(ref output) => evaluate_output(output, spec),
        Err(RunError::Timeout) => ExampleOutcome::Timeout,
        Err(RunError::Other(e)) => ExampleOutcome::Error(e),
    };
    ExampleResult {
        spec: spec.clone(),
        outcome,
        duration: start.elapsed(),
    }
}

// ---------------------------------------------------------------------------
// Core execution
// ---------------------------------------------------------------------------

/// Execute a single example and return the raw runner output.
fn execute(
    spec: &ExampleSpec,
    project_root: &Path,
    config: &ExamplesConfig,
) -> Result<RunnerOutput, RunError> {
    let dir = tempfile::tempdir()
        .map_err(|e| RunError::Other(format!("failed to create temp dir: {e}")))?;

    // Run setup script if present
    if let Some(setup) = &spec.setup {
        run_setup(setup, dir.path(), project_root, &spec.env).map_err(RunError::Other)?;
    }

    let runner = resolve_runner(&spec.runner, config).map_err(RunError::Other)?;
    match runner {
        ResolvedRunner::Subprocess(template) => run_subprocess(&template, spec, dir.path()),
        ResolvedRunner::CargoTest => run_cargo_test(spec, dir.path()),
        ResolvedRunner::Http => run_http(spec).map_err(RunError::Other),
    }
}

// ---------------------------------------------------------------------------
// Setup script execution
// ---------------------------------------------------------------------------

/// Run a setup script before the example.
fn run_setup(
    setup_path: &Path,
    work_dir: &Path,
    project_root: &Path,
    env: &[(String, String)],
) -> Result<(), String> {
    let full_path = project_root.join(setup_path);
    let output = std::process::Command::new("sh")
        .arg(&full_path)
        .current_dir(work_dir)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .map_err(|e| format!("setup script failed to run: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "setup script exited with {}: {stderr}",
            output.status
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subprocess runner
// ---------------------------------------------------------------------------

/// Run a subprocess-based runner with the given command template.
fn run_subprocess(
    template: &str,
    spec: &ExampleSpec,
    dir: &Path,
) -> Result<RunnerOutput, RunError> {
    // Write code to file
    let file_name = format!("example.{}", spec.lang);
    let file_path = dir.join(&file_name);
    std::fs::write(&file_path, &spec.code)
        .map_err(|e| RunError::Other(format!("failed to write code file: {e}")))?;

    // Interpolate placeholders
    let command = template
        .replace("{file}", &file_path.to_string_lossy())
        .replace("{dir}", &dir.to_string_lossy())
        .replace("{lang}", &spec.lang)
        .replace("{name}", &spec.example_id);

    run_subprocess_with_timeout(&command, dir, &spec.env, Duration::from_secs(spec.timeout))
}

/// Spawn a subprocess and wait for it with timeout enforcement.
fn run_subprocess_with_timeout(
    command: &str,
    dir: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<RunnerOutput, RunError> {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));

    // Create a new process group so timeout kills reach all descendants.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0)
    };

    let child = cmd
        .spawn()
        .map_err(|e| RunError::Other(format!("failed to spawn: {e}")))?;

    run_with_timeout(child, timeout)
}

/// Wait for a child process with timeout. Kills the process if it exceeds
/// the timeout.
fn run_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<RunnerOutput, RunError> {
    let pid = child.id();
    let (tx, rx) = mpsc::channel();

    std::thread::scope(|s| {
        s.spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(timeout) {
            Ok(result) => {
                let output = result.map_err(|e| RunError::Other(format!("wait failed: {e}")))?;
                Ok(RunnerOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    status: output.status.code().map(i32::cast_unsigned),
                })
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Kill the child process so the spawned thread's
                // wait_with_output unblocks and thread::scope can return.
                kill_process(pid);
                Err(RunError::Timeout)
            }
            Err(e) => Err(RunError::Other(format!("channel error: {e}"))),
        }
    })
}

/// Kill a process group by PID. On Unix, sends SIGKILL to the negated PID
/// (the process group). This kills the process and all its descendants.
fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: sending a signal to a known process group. Negate the PID
        // to target the group. If the group has already exited, the signal is
        // harmlessly ignored (ESRCH).
        unsafe {
            libc::kill(-(pid.cast_signed()), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

// ---------------------------------------------------------------------------
// Cargo-test runner
// ---------------------------------------------------------------------------

/// Run the built-in cargo-test runner.
///
/// Scaffolds a minimal Cargo project in the temp directory and runs
/// `cargo test` against the example code.
fn run_cargo_test(spec: &ExampleSpec, dir: &Path) -> Result<RunnerOutput, RunError> {
    // Scaffold minimal Cargo project
    let cargo_toml = "[package]\nname = \"supersigil-example\"\nedition = \"2024\"\n";
    std::fs::write(dir.join("Cargo.toml"), cargo_toml)
        .map_err(|e| RunError::Other(format!("failed to write Cargo.toml: {e}")))?;
    std::fs::create_dir_all(dir.join("tests"))
        .map_err(|e| RunError::Other(format!("failed to create tests dir: {e}")))?;

    // Auto-wrap in #[test] if not already present
    let code = if spec.code.contains("#[test]") {
        spec.code.clone()
    } else {
        format!("#[test]\nfn example_test() {{\n{}\n}}", spec.code)
    };

    let test_file = dir.join("tests").join(format!("{}.rs", spec.example_id));
    std::fs::write(&test_file, &code)
        .map_err(|e| RunError::Other(format!("failed to write test file: {e}")))?;

    // Build the command
    let command = format!(
        "cargo test --test {} --manifest-path {}/Cargo.toml -- --nocapture",
        spec.example_id,
        dir.to_string_lossy()
    );

    run_subprocess_with_timeout(&command, dir, &spec.env, Duration::from_secs(spec.timeout))
}

// ---------------------------------------------------------------------------
// HTTP runner
// ---------------------------------------------------------------------------

/// Run the built-in native HTTP runner.
///
/// Parses the example code as an HTTP request and sends it using ureq.
fn run_http(spec: &ExampleSpec) -> Result<RunnerOutput, String> {
    let parsed = parse_http_request(&spec.code)?;

    let full_url = if parsed.url.starts_with('/') {
        let base = spec
            .env
            .iter()
            .find(|(k, _)| k == "BASE_URL")
            .map(|(_, v)| v.as_str())
            .ok_or("relative URL requires BASE_URL in env")?;
        format!("{}{}", base.trim_end_matches('/'), parsed.url)
    } else {
        parsed.url
    };

    let http_method = ureq::http::Method::from_bytes(parsed.method.as_bytes())
        .map_err(|e| format!("invalid HTTP method '{}': {e}", parsed.method))?;

    let has_body = parsed.body.is_some();
    let body_bytes = parsed.body.unwrap_or_default();

    // Build the http::Request
    let mut builder = ureq::http::Request::builder()
        .method(http_method)
        .uri(&full_url);

    for (key, value) in &parsed.headers {
        builder = builder.header(key.as_str(), value.as_str());
    }

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(spec.timeout)))
        .http_status_as_error(false)
        .build();

    let agent = ureq::Agent::new_with_config(config);

    let result = if has_body {
        let req = builder
            .body(body_bytes.as_bytes())
            .map_err(|e| format!("failed to build request: {e}"))?;
        agent.run(req)
    } else {
        let req = builder
            .body(())
            .map_err(|e| format!("failed to build request: {e}"))?;
        agent.run(req)
    };

    match result {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let body_str = resp.body_mut().read_to_string().unwrap_or_default();
            Ok(RunnerOutput {
                stdout: body_str,
                stderr: String::new(),
                status: Some(u32::from(status)),
            })
        }
        Err(e) => Err(format!("HTTP request failed: {e}")),
    }
}

/// Parsed HTTP request from example code.
struct ParsedHttpRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

/// Parse example code as an HTTP request.
///
/// Format:
/// ```text
/// METHOD URL
/// Header-Name: Header-Value
///
/// optional body
/// ```
fn parse_http_request(code: &str) -> Result<ParsedHttpRequest, String> {
    let mut lines = code.lines();

    // First line: METHOD URL
    let first = lines.next().ok_or("empty HTTP request")?;
    let mut parts = first.splitn(2, ' ');
    let method = parts.next().ok_or("missing HTTP method")?.to_string();
    let url = parts.next().ok_or("missing URL")?.trim().to_string();

    // Headers until empty line
    let mut headers = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;

    for line in lines {
        if in_body {
            body_lines.push(line);
        } else if line.is_empty() {
            in_body = true;
        } else if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }

    let body = if body_lines.is_empty() {
        None
    } else {
        Some(body_lines.join("\n"))
    };

    Ok(ParsedHttpRequest {
        method,
        url,
        headers,
        body,
    })
}

// ---------------------------------------------------------------------------
// Output evaluation
// ---------------------------------------------------------------------------

/// Evaluate runner output against the example's expected specification.
fn evaluate_output(output: &RunnerOutput, spec: &ExampleSpec) -> ExampleOutcome {
    let Some(expected) = &spec.expected else {
        // No Expected: exit code 0 = pass, non-zero = fail
        return match output.status {
            Some(0) => ExampleOutcome::Pass,
            Some(code) => ExampleOutcome::Fail(vec![MatchFailure {
                check: MatchCheck::Status,
                expected: "0".into(),
                actual: code.to_string(),
            }]),
            None => ExampleOutcome::Error("process terminated by signal".into()),
        };
    };

    let failures = matcher::match_output(&output.stdout, output.status, expected);
    if failures.is_empty() {
        ExampleOutcome::Pass
    } else {
        ExampleOutcome::Fail(failures)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use supersigil_core::{RunnerConfig, SourcePosition};

    use super::*;
    use crate::examples::types::{ExpectedSpec, MatchFormat};

    fn make_spec(code: &str, lang: &str, runner: &str) -> ExampleSpec {
        ExampleSpec {
            doc_id: "test/doc".into(),
            example_id: "test-example".into(),
            lang: lang.into(),
            runner: runner.into(),
            verifies: vec![],
            code: code.into(),
            expected: None,
            timeout: 30,
            env: vec![],
            setup: None,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            source_path: PathBuf::from("test.md"),
        }
    }

    // -- Runner resolution tests --

    #[test]
    fn resolve_builtin_sh() {
        let config = ExamplesConfig::default();
        let runner = resolve_runner("sh", &config).unwrap();
        assert!(matches!(runner, ResolvedRunner::Subprocess(cmd) if cmd == "sh {file}"));
    }

    #[test]
    fn resolve_builtin_cargo_test() {
        let config = ExamplesConfig::default();
        let runner = resolve_runner("cargo-test", &config).unwrap();
        assert!(matches!(runner, ResolvedRunner::CargoTest));
    }

    #[test]
    fn resolve_builtin_http() {
        let config = ExamplesConfig::default();
        let runner = resolve_runner("http", &config).unwrap();
        assert!(matches!(runner, ResolvedRunner::Http));
    }

    #[test]
    fn resolve_unknown_runner() {
        let config = ExamplesConfig::default();
        let result = resolve_runner("nonexistent", &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown runner"));
    }

    #[test]
    fn resolve_user_runner_overrides_builtin() {
        let mut config = ExamplesConfig::default();
        config.runners.insert(
            "sh".into(),
            RunnerConfig {
                command: "bash {file}".into(),
            },
        );
        let runner = resolve_runner("sh", &config).unwrap();
        assert!(matches!(runner, ResolvedRunner::Subprocess(cmd) if cmd == "bash {file}"));
    }

    #[test]
    fn resolve_custom_runner() {
        let mut config = ExamplesConfig::default();
        config.runners.insert(
            "pytest".into(),
            RunnerConfig {
                command: "python -m pytest {file}".into(),
            },
        );
        let runner = resolve_runner("pytest", &config).unwrap();
        assert!(
            matches!(runner, ResolvedRunner::Subprocess(cmd) if cmd == "python -m pytest {file}")
        );
    }

    // -- HTTP request parsing tests --

    #[test]
    fn parse_http_get() {
        let parsed = parse_http_request("GET /api/v1/tasks").unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.url, "/api/v1/tasks");
        assert!(parsed.headers.is_empty());
        assert!(parsed.body.is_none());
    }

    #[test]
    fn parse_http_post_with_body() {
        let code = "POST /api/v1/tasks\nContent-Type: application/json\n\n{\"title\": \"test\"}";
        let parsed = parse_http_request(code).unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "/api/v1/tasks");
        assert_eq!(parsed.headers.len(), 1);
        assert_eq!(parsed.headers[0].0, "Content-Type");
        assert_eq!(parsed.headers[0].1, "application/json");
        assert_eq!(parsed.body.unwrap(), "{\"title\": \"test\"}");
    }

    #[test]
    fn parse_http_empty_request() {
        let result = parse_http_request("");
        assert!(result.is_err());
    }

    // -- Integration tests using the sh runner --

    #[test]
    fn sh_runner_echo() {
        let spec = make_spec("echo hello", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_nonzero_exit_fails() {
        let spec = make_spec("exit 1", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Fail(_)),
            "expected Fail, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_with_expected_text() {
        let mut spec = make_spec("echo hello", "sh", "sh");
        spec.expected = Some(ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: None,
            body: Some("hello".into()),
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_with_expected_text_mismatch() {
        let mut spec = make_spec("echo goodbye", "sh", "sh");
        spec.expected = Some(ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: None,
            body: Some("hello".into()),
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Fail(_)),
            "expected Fail, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn unknown_runner_is_error() {
        let spec = make_spec("code", "lang", "nonexistent");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Error(_)),
            "expected Error, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn env_vars_injected() {
        let mut spec = make_spec("echo $MY_VAR", "sh", "sh");
        spec.env = vec![("MY_VAR".into(), "injected_value".into())];
        spec.expected = Some(ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: Some("injected_value".into()),
            body: None,
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn user_runner_overrides_builtin() {
        let mut config = ExamplesConfig::default();
        config.runners.insert(
            "sh".into(),
            RunnerConfig {
                command: "bash {file}".into(),
            },
        );
        let spec = make_spec("echo hello", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &config);
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_with_expected_status() {
        let mut spec = make_spec("exit 42", "sh", "sh");
        spec.expected = Some(ExpectedSpec {
            status: Some(42),
            format: MatchFormat::Text,
            contains: None,
            body: None,
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_captures_stderr_for_debugging() {
        // Verify the runner doesn't crash when there's stderr output
        let spec = make_spec("echo error >&2; echo ok", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_multiline_output() {
        let mut spec = make_spec("echo 'line1'; echo 'line2'", "sh", "sh");
        spec.expected = Some(ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: None,
            body: Some("line1\nline2".into()),
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_placeholder_interpolation() {
        // Test that {lang} and {name} placeholders work
        let mut config = ExamplesConfig::default();
        config.runners.insert(
            "custom".into(),
            RunnerConfig {
                command: "echo lang={lang} name={name}".into(),
            },
        );
        let mut spec = make_spec("", "python", "custom");
        spec.example_id = "my-example".into();
        spec.expected = Some(ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: Some("lang=python".into()),
            body: None,
            body_span: None,
        });
        let result = run_example(&spec, Path::new("/tmp"), &config);
        assert!(
            matches!(result.outcome, ExampleOutcome::Pass),
            "expected Pass, got {:?}",
            result.outcome
        );
    }

    #[test]
    fn sh_runner_duration_recorded() {
        let spec = make_spec("echo hello", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(result.duration.as_nanos() > 0);
    }

    #[test]
    fn sh_runner_enforces_timeout() {
        let mut spec = make_spec("sleep 10", "sh", "sh");
        spec.timeout = 1; // 1-second timeout for a 10-second sleep
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(
            matches!(result.outcome, ExampleOutcome::Timeout),
            "example exceeding timeout should produce Timeout, got {:?}",
            result.outcome,
        );
        // Duration should be close to the timeout, not the full sleep
        assert!(
            result.duration.as_secs() < 5,
            "should not have waited for full sleep: {:?}",
            result.duration,
        );
    }

    #[test]
    fn no_expected_zero_exit_passes() {
        let spec = make_spec("true", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(matches!(result.outcome, ExampleOutcome::Pass));
    }

    #[test]
    fn no_expected_nonzero_exit_fails() {
        let spec = make_spec("false", "sh", "sh");
        let result = run_example(&spec, Path::new("/tmp"), &ExamplesConfig::default());
        assert!(matches!(result.outcome, ExampleOutcome::Fail(_)));
    }
}
