//! Runs the git CLI with a controlled environment.
//!
//! Every child process loses the variables that would point git at another
//! repository, index, or object store, or change how it reads every
//! pathspec: git locates the repository from its working directory, and the
//! pathspecs this crate builds say how they are read.
//! Every child also gets `GIT_OPTIONAL_LOCKS=0`, so read commands never
//! refresh and rewrite the real index. The user's configuration is otherwise
//! honored; only tests add isolation through [`Git::with_env`].

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use crate::error::GitError;

/// Variables removed from every git child process.
///
/// The first six could make git read or write a different repository,
/// index, or object store than the one its working directory locates. Git
/// sets the first four for hooks, so a review started from a hook would
/// otherwise inherit them; with an inherited object directory, a snapshot
/// would write its objects there and leave none in the repository.
///
/// The last four set how git reads every pathspec of a command. This crate
/// spells each pathspec's meaning itself (`:(literal)` for a path, `./` for
/// `check-ignore`), and an inherited mode would change it: literal mode reads
/// `:(literal)f.txt` as a file of that name, so a scoped review finds
/// nothing, case-insensitive mode widens a scope to other spellings, and
/// `check-ignore` refuses every mode.
const SCRUBBED: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

/// Oldest git this crate supports: `worktree list -z` arrived in 2.36.
pub const MIN_VERSION: GitVersion = GitVersion {
    major: 2,
    minor: 36,
    patch: 0,
};

/// A git version, as reported by `git version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version, 0 when git did not report one.
    pub patch: u32,
}

impl fmt::Display for GitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl GitVersion {
    /// Parses the output of `git version`, for example `git version 2.39.3
    /// (Apple Git-146)` or `git version 2.44.0.windows.1`.
    fn parse(text: &str) -> Option<Self> {
        let version = text.trim().strip_prefix("git version ")?;
        let mut parts = version
            .split_whitespace()
            .next()?
            .split('.')
            .map(leading_number);
        Some(Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next().flatten().unwrap_or(0),
        })
    }
}

/// Parses the digits at the start of `part`, or `None` if there are none.
fn leading_number(part: &str) -> Option<u32> {
    let end = part
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(part.len());
    part[..end].parse().ok()
}

/// What [`Git::output_within`] read.
#[derive(Debug)]
pub(crate) enum Bounded {
    /// Git exited with at most the limit on standard output. Standard error
    /// keeps only its end ([`STDERR_KEPT_BYTES`]).
    Finished(Output),
    /// Standard output passed the limit: reading stopped one byte past it,
    /// and git was killed and reaped.
    TooLarge,
}

/// Bytes of standard error [`Git::output_within`] keeps: its end, far more
/// than [`stderr_tail`] shows.
const STDERR_KEPT_BYTES: usize = 64 * 1024;

/// Runs git in one directory with the scrubbed environment plus any extra
/// variables added with [`Git::with_env`].
#[derive(Debug, Clone)]
pub struct Git {
    cwd: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl Git {
    /// Creates a runner for `cwd` with no extra environment.
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: Vec::new(),
        }
    }

    /// Adds a variable to every child process, after the scrubbing, so it
    /// can also set one of the scrubbed variables deliberately.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Returns a runner for another directory with the same extra environment.
    #[must_use]
    pub fn in_dir(&self, cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: self.env.clone(),
        }
    }

    /// Moves this runner to another directory, keeping its extra environment.
    #[must_use]
    pub(crate) fn moved_to(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Returns the directory git runs in.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Returns a `git` command with the scrubbed environment, the extra
    /// variables, and the working directory set.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new("git");
        for key in SCRUBBED {
            command.env_remove(key);
        }
        command.env("GIT_OPTIONAL_LOCKS", "0");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.current_dir(&self.cwd);
        command
    }

    /// Runs git with `args` and returns its output whatever the exit status.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotFound`] if git cannot be started or
    /// [`GitError::Io`] if waiting for it fails.
    pub fn raw<I, S>(&self, args: I) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run(args, None)
    }

    /// Runs git with `args` and returns standard output.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] if git exits unsuccessfully, or the errors
    /// of [`Git::raw`].
    pub fn output<I, S>(&self, args: I) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, None)?;
        succeeded(&args, output)
    }

    /// Runs git with `args` for a yes-or-no answer: exit 0 is `true`, exit 1
    /// is `false`, and any other status is an error.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`], with the arguments that ran, for an exit
    /// status other than 0 or 1, or the errors of [`Git::raw`].
    pub(crate) fn probe<I, S>(&self, args: I) -> Result<bool, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, None)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(failed(&args, &output)),
        }
    }

    /// Runs git with `args` for a value that may be absent: exit 0 returns
    /// standard output, exit 1 returns `None`, and any other status is an
    /// error.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`], with the arguments that ran, for an exit
    /// status other than 0 or 1, or the errors of [`Git::raw`].
    pub(crate) fn probe_output<I, S>(&self, args: I) -> Result<Option<Vec<u8>>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, None)?;
        match output.status.code() {
            Some(0) => Ok(Some(output.stdout)),
            Some(1) => Ok(None),
            _ => Err(failed(&args, &output)),
        }
    }

    /// Runs git with `args`, writes `input` to its standard input, and returns
    /// standard output. Input is written from a second thread, so git can
    /// produce output of any size while it reads.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] if git exits unsuccessfully, or the errors
    /// of [`Git::raw`].
    pub fn output_with_input<I, S>(&self, args: I, input: &[u8]) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, Some(input))?;
        succeeded(&args, output)
    }

    /// Runs git with `args` and reads at most `max` bytes of its standard
    /// output: one more byte ends the read, and git is killed and reaped, so
    /// output of any size costs at most `max + 1` bytes. Standard error is
    /// read on another thread and only its end is kept, so a child that
    /// writes there without end costs a bounded amount too.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotFound`] if git cannot be started, or
    /// [`GitError::Io`] if reading from or waiting for it fails.
    pub(crate) fn output_within<I, S>(&self, args: I, max: u64) -> Result<Bounded, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command();
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(&mut command)?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        // Not scoped, so nothing waits for it after a kill: a filter git
        // started may hold standard error open for longer than git.
        let stderr = std::thread::spawn(move || read_tail(stderr, STDERR_KEPT_BYTES));
        let mut out = Vec::new();
        let read = stdout.take(max.saturating_add(1)).read_to_end(&mut out);
        if read.is_err() || u64::try_from(out.len()).map_or(true, |len| len > max) {
            // Git may have exited already, so the kill may fail; waiting
            // reaps it either way.
            let _ = child.kill();
            child.wait().map_err(|e| io_error("waiting for git", e))?;
            read.map_err(|e| io_error("reading from git", e))?;
            return Ok(Bounded::TooLarge);
        }
        let status = child.wait().map_err(|e| io_error("waiting for git", e))?;
        let stderr = stderr
            .join()
            .expect("the stderr reader does not panic")
            .map_err(|e| io_error("reading from git", e))?;
        Ok(Bounded::Finished(Output {
            status,
            stdout: out,
            stderr,
        }))
    }

    /// Runs `git version` and parses the result.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] if the output is not a version, or the
    /// errors of [`Git::output`].
    pub fn version(&self) -> Result<GitVersion, GitError> {
        let output = self.output(["version"])?;
        let text = String::from_utf8_lossy(&output);
        GitVersion::parse(&text).ok_or_else(|| GitError::Parse(format!("git version: {text}")))
    }

    /// Spawns git, optionally feeding `input`, and waits for it.
    fn run<I, S>(&self, args: I, input: Option<&[u8]>) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command();
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(&mut command)?;
        let Some(input) = input else {
            return child
                .wait_with_output()
                .map_err(|e| io_error("waiting for git", e));
        };
        let mut stdin = child.stdin.take().expect("stdin is piped");
        std::thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.write_all(input));
            let output = child
                .wait_with_output()
                .map_err(|e| io_error("waiting for git", e))?;
            match writer.join().expect("the stdin writer does not panic") {
                // Git may exit before reading everything, for example on an
                // error it then reports; its exit status says what happened.
                Ok(()) => Ok(output),
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(output),
                Err(e) => Err(io_error("writing to git", e)),
            }
        })
    }
}

/// Starts `command`; git missing from `PATH` is [`GitError::NotFound`].
fn spawn(command: &mut Command) -> Result<Child, GitError> {
    command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotFound(e)
        } else {
            io_error("cannot start git", e)
        }
    })
}

/// Reads `source` to its end and returns its last `keep` bytes, holding at
/// most about twice that while reading.
fn read_tail(mut source: impl Read, keep: usize) -> std::io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let read = match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        tail.extend_from_slice(&chunk[..read]);
        if tail.len() > keep.saturating_mul(2) {
            tail.drain(..tail.len() - keep);
        }
    }
    tail.drain(..tail.len().saturating_sub(keep));
    Ok(tail)
}

/// Collects arguments so they can be both passed to git and quoted in errors.
fn collect<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter().map(|a| a.as_ref().to_owned()).collect()
}

/// Returns standard output, or [`GitError::Failed`] if git exited unsuccessfully.
fn succeeded(args: &[OsString], output: Output) -> Result<Vec<u8>, GitError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(failed(args, &output))
}

/// Builds [`GitError::Failed`] from the arguments that ran and what git
/// answered.
pub(crate) fn failed<S: AsRef<OsStr>>(args: &[S], output: &Output) -> GitError {
    GitError::Failed {
        args: args
            .iter()
            .map(|a| a.as_ref().to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

/// Maps "not found" to `None`, so a missing file is an absence and any other
/// failure stays an error for the caller to give context.
pub(crate) fn found<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Characters of standard error kept by [`stderr_tail`].
const STDERR_TAIL_CHARS: usize = 500;

/// The end of `stderr`, trimmed and at most [`STDERR_TAIL_CHARS`] characters,
/// for reports that keep git's own words without its whole output.
pub(crate) fn stderr_tail(stderr: &str) -> String {
    let chars: Vec<char> = stderr.trim().chars().collect();
    chars[chars.len().saturating_sub(STDERR_TAIL_CHARS)..]
        .iter()
        .collect()
}

/// Wraps an I/O error with what was being done.
pub(crate) fn io_error(context: &str, source: std::io::Error) -> GitError {
    GitError::Io {
        context: context.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A runner in an empty directory that is not inside any repository,
    /// isolated from the machine's git configuration as
    /// `tests/common/mod.rs`'s `isolated` does it: no system configuration,
    /// and `HOME` and `XDG_CONFIG_HOME` inside the test's directory.
    fn outside_a_repository(dir: &tempfile::TempDir) -> Git {
        Git::new(dir.path())
            .with_env("GIT_CEILING_DIRECTORIES", dir.path().parent().unwrap())
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("HOME", dir.path())
            .with_env("XDG_CONFIG_HOME", dir.path())
            .with_env("GIT_AUTHOR_NAME", "Test")
            .with_env("GIT_AUTHOR_EMAIL", "test@example.com")
            .with_env("GIT_COMMITTER_NAME", "Test")
            .with_env("GIT_COMMITTER_EMAIL", "test@example.com")
    }

    #[test]
    fn a_probe_reads_exit_zero_and_one_and_fails_on_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let git = outside_a_repository(&dir);
        assert!(git.probe(["version"]).unwrap());
        assert!(!git.probe(["config", "--get", "no.such"]).unwrap());
        let err = git.probe(["rev-parse", "--git-dir"]).unwrap_err();
        assert!(
            matches!(&err, GitError::Failed { args, status: Some(128), .. } if args == "rev-parse --git-dir"),
            "{err:?}"
        );
    }

    #[test]
    fn a_probe_output_returns_the_output_for_exit_zero_only() {
        let dir = tempfile::tempdir().unwrap();
        let git = outside_a_repository(&dir);
        let out = git.probe_output(["version"]).unwrap().unwrap();
        assert!(out.starts_with(b"git version"));
        assert_eq!(
            git.probe_output(["config", "--get", "no.such"]).unwrap(),
            None
        );
        let err = git.probe_output(["rev-parse", "--git-dir"]).unwrap_err();
        assert!(matches!(
            err,
            GitError::Failed {
                status: Some(128),
                ..
            }
        ));
    }

    #[test]
    fn a_tail_keeps_only_the_last_bytes_of_any_length() {
        let text: Vec<u8> = (0..100_000u32)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        assert_eq!(
            read_tail(text.as_slice(), 10).unwrap(),
            text[text.len() - 10..]
        );
        assert_eq!(read_tail(&text[..20], 100).unwrap(), text[..20]);
        assert!(read_tail(&b""[..], 10).unwrap().is_empty());
    }

    #[test]
    fn found_turns_only_not_found_into_none() {
        use std::io::{Error, ErrorKind};
        assert_eq!(found(Ok(1)).unwrap(), Some(1));
        assert_eq!(
            found::<u8>(Err(Error::from(ErrorKind::NotFound))).unwrap(),
            None
        );
        let err = found::<u8>(Err(Error::from(ErrorKind::PermissionDenied))).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn versions_parse_across_platform_suffixes() {
        let v = |major, minor, patch| GitVersion {
            major,
            minor,
            patch,
        };
        assert_eq!(GitVersion::parse("git version 2.55.0\n"), Some(v(2, 55, 0)));
        assert_eq!(
            GitVersion::parse("git version 2.39.3 (Apple Git-146)"),
            Some(v(2, 39, 3))
        );
        assert_eq!(
            GitVersion::parse("git version 2.44.0.windows.1"),
            Some(v(2, 44, 0))
        );
        assert_eq!(GitVersion::parse("git version 2.36"), Some(v(2, 36, 0)));
        assert_eq!(
            GitVersion::parse("git version 2.36.0.rc1"),
            Some(v(2, 36, 0))
        );
        assert_eq!(GitVersion::parse("hello"), None);
    }

    #[test]
    fn minimum_is_ordered_below_newer_versions() {
        assert!(
            MIN_VERSION
                < GitVersion {
                    major: 2,
                    minor: 55,
                    patch: 0
                }
        );
        assert!(
            MIN_VERSION
                > GitVersion {
                    major: 2,
                    minor: 35,
                    patch: 9
                }
        );
        assert_eq!(MIN_VERSION.to_string(), "2.36.0");
    }

    #[test]
    fn command_scrubs_repository_variables() {
        let git = Git::new("/tmp").with_env("EXTRA", "1");
        let command = git.command();
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let policy = [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_LITERAL_PATHSPECS",
            "GIT_GLOB_PATHSPECS",
            "GIT_NOGLOB_PATHSPECS",
            "GIT_ICASE_PATHSPECS",
        ];
        for key in policy.into_iter().chain(SCRUBBED) {
            assert!(envs.contains(&(key.to_owned(), None)), "{key} not removed");
        }
        assert!(envs.contains(&("GIT_OPTIONAL_LOCKS".to_owned(), Some("0".to_owned()))));
        assert!(envs.contains(&("EXTRA".to_owned(), Some("1".to_owned()))));
    }

    #[test]
    fn extra_env_can_set_a_scrubbed_variable_deliberately() {
        let command = Git::new("/tmp")
            .with_env("GIT_INDEX_FILE", "/tmp/index")
            .command();
        let value = command
            .get_envs()
            .filter(|(k, _)| *k == "GIT_INDEX_FILE")
            .last()
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()));
        assert_eq!(value.as_deref(), Some("/tmp/index"));
    }
}
