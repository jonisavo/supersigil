//! Hook execution for external process hooks.
//!
//! Hooks receive the interim verification report as JSON on stdin and produce
//! findings on stdout as `[[level, message], ...]` JSON arrays.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::report::{Finding, ReportSeverity, RuleName};

/// Maximum bytes read from hook stdout/stderr before truncation.
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Run a set of hook commands and collect findings.
///
/// Each command is executed as a shell command (`sh -c`). The interim
/// verification report is written to the child's stdin as JSON.
///
/// - Valid stdout is parsed as `[[level, message], ...]` producing
///   `RuleName::HookOutput` findings.
/// - Invalid JSON on stdout produces a `RuleName::HookFailure` warning.
/// - Non-zero exit produces a `RuleName::HookFailure` error.
/// - Timeout produces a `RuleName::HookFailure` error (process is killed).
///
/// This function never returns `Err`; all failures become findings.
#[must_use]
pub fn run_hooks(commands: &[String], report_json: &str, timeout_seconds: u64) -> Vec<Finding> {
    let mut findings = Vec::new();
    for cmd in commands {
        findings.extend(run_single_hook(cmd, report_json, timeout_seconds));
    }
    findings
}

/// Execute a single hook command and return any findings it produces.
fn run_single_hook(cmd: &str, report_json: &str, timeout_seconds: u64) -> Vec<Finding> {
    let child = Command::new("sh")
        .args(["-c", cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return vec![Finding::new(
                RuleName::HookFailure,
                None,
                format!("hook `{cmd}` failed to spawn: {e}"),
                None,
            )];
        }
    };

    // Capture PID so we can kill on timeout (before moving child into thread).
    let child_id = child.id();

    // Write report JSON to stdin, then close it.
    if let Some(mut stdin) = child.stdin.take() {
        // Ignore write errors — the hook may have exited early.
        let _ = stdin.write_all(report_json.as_bytes());
        // stdin is dropped (closed) here.
    }

    // Wait for the child with a timeout using a channel + thread.
    let timeout = Duration::from_secs(timeout_seconds);
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            let _ = handle.join();
            process_output(cmd, &output)
        }
        Ok(Err(e)) => {
            let _ = handle.join();
            vec![Finding::new(
                RuleName::HookFailure,
                None,
                format!("hook `{cmd}` I/O error: {e}"),
                None,
            )]
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Kill the child process via its PID.
            kill_process(child_id);
            // Wait for the thread to finish (child should exit after kill).
            let _ = handle.join();
            vec![Finding::new(
                RuleName::HookFailure,
                None,
                format!("hook `{cmd}` timed out after {timeout_seconds}s and was killed"),
                None,
            )]
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            vec![Finding::new(
                RuleName::HookFailure,
                None,
                format!("hook `{cmd}` channel disconnected unexpectedly"),
                None,
            )]
        }
    }
}

/// Send SIGKILL to a process by PID. Best-effort; errors are ignored.
fn kill_process(pid: u32) {
    let _ = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Process completed hook output into findings.
fn process_output(cmd: &str, output: &std::process::Output) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Non-zero exit → error finding
    if !output.status.success() {
        let stderr = truncated_string(&output.stderr);
        let exit_info = match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "killed by signal".to_string(),
        };
        findings.push(Finding::new(
            RuleName::HookFailure,
            None,
            format!("hook `{cmd}` failed ({exit_info}): {stderr}"),
            None,
        ));
        return findings;
    }

    // Truncate stdout to 64KB before parsing
    let stdout_bytes = if output.stdout.len() > MAX_OUTPUT_BYTES {
        &output.stdout[..MAX_OUTPUT_BYTES]
    } else {
        &output.stdout
    };
    let stdout = String::from_utf8_lossy(stdout_bytes);
    let stdout = stdout.trim();

    // Empty stdout is fine — no findings
    if stdout.is_empty() {
        return findings;
    }

    // Parse as JSON array of [level, message] pairs
    let parsed: Result<Vec<(String, String)>, _> = serde_json::from_str(stdout);
    match parsed {
        Ok(pairs) => {
            for (level, message) in pairs {
                let severity = match level.as_str() {
                    "error" => ReportSeverity::Error,
                    "info" => ReportSeverity::Info,
                    // "warning" and any unrecognised level default to Warning
                    _ => ReportSeverity::Warning,
                };
                findings.push(Finding {
                    rule: RuleName::HookOutput,
                    doc_id: None,
                    message,
                    effective_severity: severity,
                    raw_severity: severity,
                    position: None,
                });
            }
        }
        Err(e) => {
            findings.push(Finding {
                rule: RuleName::HookFailure,
                doc_id: None,
                message: format!("hook `{cmd}` produced invalid JSON on stdout: {e}"),
                effective_severity: ReportSeverity::Warning,
                raw_severity: ReportSeverity::Warning,
                position: None,
            });
        }
    }

    findings
}

/// Convert bytes to a UTF-8 string, truncating at `MAX_OUTPUT_BYTES`.
fn truncated_string(bytes: &[u8]) -> String {
    let truncated = if bytes.len() > MAX_OUTPUT_BYTES {
        &bytes[..MAX_OUTPUT_BYTES]
    } else {
        bytes
    };
    String::from_utf8_lossy(truncated).into_owned()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_stdout_parsed_as_findings() {
        let findings = run_hooks(
            &[r#"echo '[["warning", "custom check failed"]]'"#.to_string()],
            "{}",
            5,
        );
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookOutput);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Warning);
        assert_eq!(findings[0].message, "custom check failed");
    }

    #[test]
    fn hook_invalid_json_produces_warning() {
        let findings = run_hooks(&["echo 'not json'".to_string()], "{}", 5);
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookFailure);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Warning);
        assert!(
            findings[0].message.contains("invalid JSON"),
            "message should mention invalid JSON, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn hook_nonzero_exit_produces_error() {
        let findings = run_hooks(&["bash -c 'exit 1'".to_string()], "{}", 5);
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookFailure);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Error);
        assert!(
            findings[0].message.contains("exit code 1"),
            "message should mention exit code, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn hook_timeout_produces_error() {
        let findings = run_hooks(&["sleep 60".to_string()], "{}", 1);
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookFailure);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Error);
        assert!(
            findings[0].message.contains("timed out"),
            "message should mention timeout, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn hook_output_truncated_at_64kb() {
        // Generate >64KB of output. `yes` outputs "y\n" repeatedly.
        // We use head -c to get exactly 128KB then the hook should still work.
        let findings = run_hooks(
            &["head -c 131072 /dev/zero | tr '\\0' 'A'".to_string()],
            "{}",
            5,
        );
        // The output is 128KB of 'A's which is not valid JSON,
        // so we should get a warning about invalid JSON — but no OOM/panic.
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookFailure);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Warning);
        assert!(
            findings[0].message.contains("invalid JSON"),
            "message should mention invalid JSON for truncated output, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn hook_multiple_findings_from_stdout() {
        let findings = run_hooks(
            &[r#"echo '[["error", "critical issue"], ["info", "just a note"]]'"#.to_string()],
            "{}",
            5,
        );
        assert_eq!(findings.len(), 2, "expected 2 findings, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookOutput);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Error);
        assert_eq!(findings[0].message, "critical issue");
        assert_eq!(findings[1].rule, RuleName::HookOutput);
        assert_eq!(findings[1].effective_severity, ReportSeverity::Info);
        assert_eq!(findings[1].message, "just a note");
    }

    #[test]
    fn hook_empty_stdout_produces_no_findings() {
        // true exits 0 and produces no output
        let findings = run_hooks(&["true".to_string()], "{}", 5);
        assert!(
            findings.is_empty(),
            "expected no findings, got: {findings:?}",
        );
    }

    #[test]
    fn hook_receives_report_json_on_stdin() {
        // Hook reads stdin via `cat` and uses it in output.
        // Use a simple marker to avoid JSON quoting issues.
        let report_json = "hello-from-verify";
        // The hook reads stdin and constructs a JSON array with the content.
        // We use jq-free approach: read stdin, build JSON manually.
        let findings = run_hooks(
            &[r#"input="$(cat)"; echo "[[ \"info\", \"got: ${input}\" ]]""#.to_string()],
            report_json,
            5,
        );
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookOutput);
        assert!(
            findings[0].message.contains("hello-from-verify"),
            "hook should have received stdin content, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn hook_unknown_level_defaults_to_warning() {
        let findings = run_hooks(
            &[r#"echo '[["banana", "unknown level"]]'"#.to_string()],
            "{}",
            5,
        );
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].rule, RuleName::HookOutput);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Warning);
    }

    #[test]
    fn run_hooks_with_empty_commands() {
        let findings = run_hooks(&[], "{}", 5);
        assert!(
            findings.is_empty(),
            "expected no findings for empty commands, got: {findings:?}",
        );
    }

    #[test]
    fn run_hooks_multiple_commands() {
        let findings = run_hooks(
            &[
                r#"echo '[["info", "from hook 1"]]'"#.to_string(),
                r#"echo '[["warning", "from hook 2"]]'"#.to_string(),
            ],
            "{}",
            5,
        );
        assert_eq!(findings.len(), 2, "expected 2 findings, got: {findings:?}");
        assert_eq!(findings[0].message, "from hook 1");
        assert_eq!(findings[1].message, "from hook 2");
    }
}
