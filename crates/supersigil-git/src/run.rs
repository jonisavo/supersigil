//! Runs the git CLI with a controlled environment.
//!
//! Every child process loses the variables that would point git at another
//! repository or index, and gets `GIT_OPTIONAL_LOCKS=0` so read commands never
//! refresh and rewrite the real index. The user's configuration is otherwise
//! honored; only tests add isolation through [`Git::with_env`].

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::error::GitError;

/// Variables removed from every git child process: each could make git read
/// or write a different repository or index than the one in the working
/// directory. Git sets them for hooks, so a review started from a hook would
/// otherwise inherit them.
const SCRUBBED: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
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
        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotFound(e)
            } else {
                io_error("cannot start git", e)
            }
        })?;
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
    Err(GitError::Failed {
        args: args
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
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
        for key in SCRUBBED {
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
