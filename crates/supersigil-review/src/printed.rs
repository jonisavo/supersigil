//! Terminal output built from text that may come from outside the tool.
//!
//! [`Printed`] collects an output's lines and the commands they print. It
//! escapes every line once with [`escape_control`], so paths, ids, names,
//! and transcript text cannot rewrite the terminal, and adds one note after
//! commands that show a value as `<as given>`. [`Addressing`] builds those
//! commands for the shells they are written for.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt::{self, Write as _};
use std::path::PathBuf;

/// Makes control characters in `text` visible for terminal output.
///
/// Every character in `U+0000..=U+001F` except tab, plus `U+007F` and the C1
/// range `U+0080..=U+009F`, becomes `\x` and two lowercase hex digits, for
/// example `\x1b`. Everything else passes through, so escaping twice
/// changes nothing. See [`Untrusted`] for which values need it.
#[must_use]
pub fn escape_control(text: &str) -> String {
    Untrusted(text).to_string()
}

/// Text from outside the tool that displays with its control characters made
/// visible as [`escape_control`] describes, so an escape sequence in it
/// cannot erase or forge the lines around it.
///
/// Every such value goes through it before it reaches a terminal: session
/// ids, times, branches, record type names, and skip reasons from
/// transcripts; transcript, checkout, and record paths from the file system;
/// and command-line arguments. Error messages may carry any of these
/// unescaped, so the binary escapes each rendered error message once, as a
/// whole, when it prints it.
#[derive(Debug, Clone, Copy)]
pub struct Untrusted<'a>(pub &'a str);

impl fmt::Display for Untrusted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.0.chars() {
            let code = u32::from(c);
            if (code <= 0x1f && c != '\t') || (0x7f..=0x9f).contains(&code) {
                write!(f, "\\x{code:02x}")?;
            } else {
                f.write_char(c)?;
            }
        }
        Ok(())
    }
}

/// An output's lines and the commands they print, for a terminal.
///
/// Lines go in as built, untrusted text and all; [`finish`](Self::finish)
/// escapes each once and adds the note for the commands that show a value
/// as `<as given>` after the last line.
#[derive(Debug, Default)]
pub struct Printed {
    /// The lines, not yet escaped.
    lines: Vec<String>,
    /// The commands printed since the last note.
    commands: Vec<Command>,
}

impl Printed {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `line`.
    pub fn line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Adds each of `lines`.
    pub fn lines<S: Into<String>>(&mut self, lines: impl IntoIterator<Item = S>) {
        self.lines.extend(lines.into_iter().map(Into::into));
    }

    /// Records that a line prints `command`, for the note, and returns its
    /// text to put in that line.
    pub fn command(&mut self, command: Command) -> String {
        let text = command.text.clone();
        self.commands.push(command);
        text
    }

    /// Takes the note for the commands recorded since the last note, naming
    /// the values they show as `<as given>` and telling the reader to pass
    /// them unchanged. `None` when every value prints exactly. For output
    /// that places the note before its last lines.
    pub fn take_note(&mut self) -> Option<String> {
        let note = note(&self.commands);
        self.commands.clear();
        note
    }

    /// The lines, each escaped, then the note for the commands not yet
    /// noted.
    #[must_use]
    pub fn finish(mut self) -> Vec<String> {
        let note = self.take_note();
        self.lines.extend(note);
        self.lines.iter().map(|line| escape_control(line)).collect()
    }

    /// [`finish`](Self::finish)'s lines, each ending in a newline.
    #[must_use]
    pub fn text(self) -> String {
        let mut text = String::new();
        for line in self.finish() {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }
}

impl<S: Into<String>, const N: usize> From<[S; N]> for Printed {
    fn from(lines: [S; N]) -> Self {
        lines.into_iter().collect()
    }
}

impl<S: Into<String>> FromIterator<S> for Printed {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut printed = Self::new();
        printed.lines(iter);
        printed
    }
}

/// Formats `rows` as lines with two spaces between columns. Escapes each
/// cell, then pads each column except the last to its widest cell and
/// trims trailing spaces.
#[must_use]
pub fn aligned<const N: usize>(rows: Vec<[String; N]>) -> Vec<String> {
    let rows: Vec<[String; N]> = rows
        .into_iter()
        .map(|row| row.map(|cell| escape_control(&cell)))
        .collect();
    let mut widths = [0; N];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.into_iter()
        .map(|row| {
            let mut line = String::new();
            for (index, (cell, width)) in row.iter().zip(widths).enumerate() {
                if index + 1 == N {
                    line.push_str(cell);
                } else {
                    let _ = write!(line, "{cell:<width$}  ");
                }
            }
            line.truncate(line.trim_end().len());
            line
        })
        .collect()
}

/// What a printed command shows in place of a flag value that cannot be
/// printed exactly.
const AS_GIVEN: &str = "<as given>";

/// What [`Command`] records for a path filter it shows as `<as given>`.
const PATH_FILTER: &str = "path filter";

/// What [`Command`] records for an item name it shows as `<as given>`.
const ITEM_NAME: &str = "item name";

/// Where the note lists a value shown as `<as given>`: the review address
/// flags, the `review` call's flags in the order given, other flags, item
/// names, then path filters.
type Rank = (u8, usize);

/// Where the note lists `what`, unless it is one of the `review` call's
/// flags given again.
fn rank(what: &str) -> Rank {
    match what {
        "--review" => (0, 0),
        "--checkout" => (0, 1),
        "--review-dir" => (0, 2),
        "--include-untracked" => (2, 0),
        "--record-dir" => (2, 1),
        "--claude-home" => (2, 2),
        ITEM_NAME => (3, 0),
        PATH_FILTER => (4, 0),
        _ => (2, 3),
    }
}

/// The note after `commands` that show a value as `<as given>`, or `None`.
fn note(commands: &[Command]) -> Option<String> {
    let mut shown: BTreeMap<&str, Rank> = BTreeMap::new();
    let mut shell = Shell::default();
    for command in commands {
        for &(rank, what) in &command.as_given {
            shell = command.shell;
            let listed = shown.entry(what).or_insert(rank);
            *listed = (*listed).min(rank);
        }
    }
    let mut flags: Vec<(Rank, &str)> = shown.into_iter().map(|(what, rank)| (rank, what)).collect();
    flags.sort_unstable();
    let flags: Vec<&str> = flags.into_iter().map(|(_, what)| what).collect();
    let (one, several) = shell.reasons();
    match flags.as_slice() {
        [] => None,
        [flag] => Some(format!(
            "The {flag} value {one}, so commands show {AS_GIVEN} for it: pass it as given."
        )),
        flags => Some(format!(
            "The {} values {several}, so commands show {AS_GIVEN} for them: pass them as \
             given.",
            flags.join(" and ")
        )),
    }
}

/// The shells a printed command is written for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shell {
    /// POSIX shells, which read single-quoted values literally.
    #[default]
    Posix,
    /// Windows shells. `cmd.exe` and PowerShell read quotes, `%`, and some
    /// other characters differently, so a value that needs quoting shows as
    /// `<as given>`: pasting a quoted file name could run part of it.
    Windows,
}

impl Shell {
    /// The shells of the platform this runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    /// `word` as a command for these shells shows it: quoted with
    /// [`shell_word`] for POSIX shells; unchanged for Windows shells when
    /// it holds only ASCII letters, digits, and `_-./\:~+` and does not
    /// start with `~`, else `None`.
    #[must_use]
    pub fn word(self, word: &str) -> Option<String> {
        match self {
            Self::Posix => Some(shell_word(word)),
            Self::Windows => {
                let plain = !word.is_empty()
                    && !word.starts_with('~')
                    && word
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || r"_-./\:~+".contains(c));
                plain.then(|| word.to_owned())
            }
        }
    }

    /// Why a value shows as `<as given>`, for one value or several.
    const fn reasons(self) -> (&'static str, &'static str) {
        match self {
            Self::Posix => (
                "is not valid UTF-8 or holds a control character",
                "are not valid UTF-8 or hold a control character",
            ),
            Self::Windows => (
                "is not valid UTF-8, holds a control character, or needs quoting that Windows \
                 shells read differently",
                "are not valid UTF-8, hold a control character, or need quoting that Windows \
                 shells read differently",
            ),
        }
    }
}

/// A command rendered by [`Addressing`], tracking values replaced by
/// `<as given>` so [`Printed`] can add its note after all commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The shells it is written for.
    shell: Shell,
    /// The command as printed.
    text: String,
    /// The flag of each value it shows as `<as given>`, or `path filter`
    /// for a path filter, in the order shown, each with where the note
    /// lists it.
    as_given: Vec<(Rank, &'static str)>,
}

impl Command {
    /// `program` without arguments, for `shell`.
    fn new(program: &str, shell: Shell) -> Self {
        Self {
            shell,
            text: program.to_owned(),
            as_given: Vec::new(),
        }
    }

    /// Appends a space and `words`, printed as given, unless `words` is
    /// empty.
    fn words(&mut self, words: &str) {
        if !words.is_empty() {
            self.text.push(' ');
            self.text.push_str(words);
        }
    }

    /// Appends `<flag>=<value>` using [`Self::value`] so leading `-` in a
    /// value cannot parse as a flag.
    fn flag(&mut self, flag: &'static str, value: &OsStr) {
        self.ranked_flag(flag, rank(flag), value);
    }

    /// As [`Self::flag`], for a flag the note lists at `rank`.
    fn ranked_flag(&mut self, flag: &'static str, rank: Rank, value: &OsStr) {
        let value = self.ranked_value(flag, rank, value);
        self.words(&format!("{flag}={value}"));
    }

    /// Shows `value` as its shells read it ([`Shell::word`]), or
    /// substitutes `<as given>` and records `what` if [`printed`] cannot
    /// represent it exactly or the shells cannot read it safely.
    fn value(&mut self, what: &'static str, value: &OsStr) -> String {
        self.ranked_value(what, rank(what), value)
    }

    /// As [`Self::value`], for a value the note lists at `rank`.
    fn ranked_value(&mut self, what: &'static str, rank: Rank, value: &OsStr) -> String {
        printed(value)
            .and_then(|text| self.shell.word(text))
            .unwrap_or_else(|| {
                self.as_given.push((rank, what));
                AS_GIVEN.to_owned()
            })
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Addresses printed commands. `judge` commands name the review when it
/// differs from the worktree's selection. Every command repeats the supplied
/// `--checkout` and `--review-dir`, and `review` commands the supplied
/// `--record-dir` and `--claude-home`, which choose the evidence read.
///
/// Commands replace non-UTF-8 values or control characters with `<as given>`
/// because lossy or escaped values could name another path, and for Windows
/// shells also values that need quoting. [`Printed`] adds a note once after
/// commands containing these placeholders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Addressing {
    /// The review's first 8 ID digits for an agent session or `--review`
    /// override, when `judge` would otherwise apply to another review.
    pub review: Option<String>,
    /// `--checkout` as the call was given it.
    pub checkout: Option<PathBuf>,
    /// `--review-dir` as the call was given it.
    pub review_dir: Option<PathBuf>,
    /// `--record-dir` as the `review` call was given it.
    pub record_dir: Option<PathBuf>,
    /// `--claude-home` as the `review` call was given it.
    pub claude_home: Option<PathBuf>,
    /// The flags of the `review` call other than the addressing and
    /// evidence flags, each with the value it was given (`None` for a flag
    /// without one), for the command that runs it again.
    pub retry: Vec<(&'static str, Option<String>)>,
    /// The path filters of the `review` call as it was given them.
    pub retry_paths: Vec<String>,
    /// The shells commands are written for.
    pub shell: Shell,
}

impl Addressing {
    /// Builds `supersigil review` with the supplied arguments. Emits
    /// [`retry`](Self::retry) flags as `<flag>=<value>`, then the evidence,
    /// `--checkout`, and `--review-dir` flags, then `--` and any
    /// [`retry_paths`](Self::retry_paths). Values that cannot print exactly
    /// become `<as given>`; the note lists the retry flags among them in
    /// the order first given, after the review address flags.
    #[must_use]
    pub fn retry(&self) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        for (flag, value) in &self.retry {
            match value {
                None => command.words(flag),
                Some(value) => {
                    let first = self.retry.iter().position(|(given, _)| given == flag);
                    command.ranked_flag(flag, (1, first.unwrap_or_default()), OsStr::new(value));
                }
            }
        }
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Builds `supersigil judge <verb>` with `names` as its shells read them,
    /// then literal `options` and review address flags. If any name starts
    /// with `-`, puts all names last after `--` to prevent flag parsing.
    #[must_use]
    pub fn judge(&self, verb: &str, names: &[&str], options: &str) -> Command {
        let dashed = names.iter().any(|name| name.starts_with('-'));
        let mut command = Command::new(&format!("supersigil judge {verb}"), self.shell);
        let names = names
            .iter()
            .map(|name| command.value(ITEM_NAME, OsStr::new(name)))
            .collect::<Vec<_>>()
            .join(" ");
        if !dashed {
            command.words(&names);
        }
        command.words(options);
        if let Some(review) = &self.review {
            command.flag("--review", OsStr::new(review));
        }
        self.address(&mut command);
        if dashed {
            command.words("--");
            command.words(&names);
        }
        command
    }

    /// Builds `supersigil review <words>` with literal `words`, then the
    /// supplied evidence, `--checkout`, and `--review-dir` flags, then `--`
    /// and any [`retry_paths`](Self::retry_paths).
    #[must_use]
    pub fn review(&self, words: &str) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        command.words(words);
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Builds `supersigil review <flag>=<value>`, then as [`Self::review`].
    #[must_use]
    pub fn review_flag(&self, flag: &'static str, value: &str) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        command.flag(flag, OsStr::new(value));
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Appends `--` and the [`retry_paths`](Self::retry_paths), if any, to
    /// `command`.
    fn paths(&self, command: &mut Command) {
        if !self.retry_paths.is_empty() {
            command.words("--");
            for path in &self.retry_paths {
                let path = command.value(PATH_FILTER, OsStr::new(path));
                command.words(&path);
            }
        }
    }

    /// Appends supplied `--record-dir` and `--claude-home` values to `command`.
    fn evidence(&self, command: &mut Command) {
        let flags = [
            ("--record-dir", &self.record_dir),
            ("--claude-home", &self.claude_home),
        ];
        for (flag, value) in flags {
            if let Some(value) = value {
                command.flag(flag, value.as_os_str());
            }
        }
    }

    /// Appends supplied `--checkout` and `--review-dir` values to `command`.
    fn address(&self, command: &mut Command) {
        let flags = [
            ("--checkout", &self.checkout),
            ("--review-dir", &self.review_dir),
        ];
        for (flag, value) in flags {
            if let Some(value) = value {
                command.flag(flag, value.as_os_str());
            }
        }
    }
}

/// Returns `value`'s exact text for commands or names, or `None` for
/// non-UTF-8 values or control characters that output would escape.
#[must_use]
pub fn printed(value: &OsStr) -> Option<&str> {
    value
        .to_str()
        .filter(|text| !text.chars().any(char::is_control))
}

/// Quotes `word` for a POSIX shell, leaving safe nonempty words unchanged
/// and single-quoting the rest.
#[must_use]
pub fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| !c.is_ascii() || c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}
