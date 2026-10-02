//! Recognizes file writes a shell command makes from a quoted heredoc.
//!
//! `cat > path <<'EOF'` hands `cat` exactly the lines between the opening
//! line and the terminator: with a quoted delimiter the shell expands
//! nothing in them. Such a statement is the one shell form whose written
//! bytes can be read off the command text. This module finds those
//! statements and the directory each runs in. It reads text only: nothing
//! is executed, and whether a write happened is for the caller to confirm
//! against the harness's report.
//!
//! The reading is deliberately narrow. A statement counts only when it is a
//! top-level statement of the command, is `cat` with no arguments, has one
//! bare `>` or `>>` redirection to a literal path and one `<<` heredoc whose
//! delimiter is one single-quoted string, or one double-quoted string
//! holding no `$`, backtick, or backslash, and runs in the
//! foreground, outside a pipeline, and not as the alternative after `||`.
//! Recognition stops at the first construct that is not a simple command (a
//! subshell or group, a reserved word, a command substitution, an unclosed
//! quote, a heredoc without its terminator, an unquoted heredoc whose body
//! joins lines or expands a command or arithmetic), at a statement that
//! leaves the shell, and at one that may change what a later `cat` means
//! (`alias`, `set`, `export`, `eval`, an assignment to `PATH`, and their
//! like): what was recognized before it stands, nothing after it is. A write that is missed costs attribution; a
//! write that is wrong would be a false record, so every doubt is resolved
//! by not recognizing.
//!
//! A backslash followed by a newline continues a line. The shell removes
//! the pair wherever it reads a command, even inside an operator or a
//! reserved word, and so does the reader here ([`Lexer::peek`]). It stays
//! as written in single quotes, in comments, and in heredoc bodies.
//!
//! One thing the text cannot show is the shell the command runs in: an
//! alias or a function named `cat` from the user's profile, or another
//! `cat` on its `PATH`. The reading assumes `cat` copies its input. The
//! statements it stops at are those known to change what a later statement
//! means; the shell has more ways than a reader of text can list. The
//! caller's checks against the harness's report are what stand behind both.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the transcript parser calls this module once shell edits are recorded"
    )
)]

use std::path::{Component, Path, PathBuf};

/// A heredoc file write found in a command's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellWrite {
    /// The target joined onto the directory its statement runs in, with
    /// `.` components dropped and nothing else resolved: `..` stays, and
    /// the path is not checked against any checkout.
    pub path: PathBuf,
    /// Whether the redirection is `>>`, which appends, and not `>`.
    pub append: bool,
    /// The heredoc body: every line before the terminator, each with its
    /// newline.
    pub body: String,
}

/// Finds the heredoc writes in `command`, run from the directory `cwd`.
///
/// A relative target is joined onto the directory its statement runs in.
/// That is `cwd` until a statement may have moved the shell. A `cd` with one
/// literal argument that is absolute or starts with `./` or `../` (the
/// forms the shell does not look up in `CDPATH`) moves it for the
/// statements chained after it with `&&`, which run only when the `cd`
/// succeeded. Once the chain ends, whether the `cd` happened is no longer
/// known, so the directory is unknown from there on, as it is after any
/// other form of `cd` and after `pushd`, `popd`, `exec`, `builtin`, or
/// `command`. While it is unknown, only writes to absolute targets are
/// returned. A list run in the background (`a && b &`) writes and moves
/// nothing here.
pub(super) fn shell_writes(command: &str, cwd: &Path) -> Vec<ShellWrite> {
    let mut scan = Scan {
        lexer: Lexer {
            text: command,
            pos: 0,
        },
        dir: Some(cwd.to_path_buf()),
        drafts: Vec::new(),
        heredocs: Vec::new(),
        read: 0,
        list: Vec::new(),
    };
    // A stop ends recognition; what was completed before it stands.
    let _: Result<(), Stop> = scan.run();
    scan.finish()
}

/// The reader met a construct it does not read as a simple command, or a
/// statement after which the rest of the command does not run here.
struct Stop;

/// Reserved words that open or continue a compound command, a group, a
/// negation, a conditional expression, or a timed pipeline when they start
/// a statement.
const RESERVED: [&str; 20] = [
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "function", "select", "coproc", "time", "{", "}", "!", "[[",
];

/// Programs after which the shell's working directory is no longer known.
const MOVES: [&str; 4] = ["cd", "pushd", "popd", "exec"];

/// Builtins that run the program they are given, which is the one that
/// counts.
const WRAPPERS: [&str; 2] = ["builtin", "command"];

/// Builtins that can change what a later statement means: which program
/// `cat` names, whether a statement runs at all, what a redirection does,
/// or how much a program may write. Those that set a variable are here
/// because the variable may be `PATH`.
const REDEFINES: [&str; 22] = [
    "alias",
    "unalias",
    "shopt",
    "set",
    "enable",
    "trap",
    "export",
    "declare",
    "typeset",
    "local",
    "readonly",
    "unset",
    "hash",
    "eval",
    "source",
    ".",
    "read",
    "mapfile",
    "readarray",
    "getopts",
    "let",
    "ulimit",
];

/// Programs that end the shell, or the script it reads, where they run.
const LEAVES: [&str; 3] = ["exit", "return", "logout"];

/// How a statement is joined to its predecessor, or what ends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Join {
    /// `;`, a newline, or the start or end of the command.
    Sequence,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `|` or `|&`.
    Pipe,
    /// A single `&`.
    Background,
}

/// How a word was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordKind {
    /// Unquoted, with nothing the shell would expand or unescape.
    Plain,
    /// One single-quoted string, or one double-quoted string holding no
    /// `$`, backtick, or backslash.
    Quoted,
    /// Anything else: an expansion, an escape, or mixed quoting.
    Other,
}

/// One shell word with its quotes removed.
#[derive(Debug)]
struct Word {
    text: String,
    kind: WordKind,
    /// Whether the word holds no quote and no escape. Only such a word can
    /// be a reserved word, and only such a heredoc delimiter leaves its
    /// body open to expansion.
    bare: bool,
}

impl Word {
    /// Whether the shell passes on exactly `text`, whatever its environment.
    fn literal(&self) -> bool {
        self.kind != WordKind::Other
    }

    /// The variable this word assigns, for an assignment such as
    /// `RUST_LOG=debug` or `PATH+=:bin`.
    fn assigned(&self) -> Option<&str> {
        let (name, _) = self.text.split_once('=')?;
        let name = name.strip_suffix('+').unwrap_or(name);
        let valid = !name.is_empty()
            && name
                .chars()
                .enumerate()
                .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()));
        valid.then_some(name)
    }
}

/// One redirection of a statement.
#[derive(Debug)]
enum Redirect {
    /// A bare `>` or `>>` to a word.
    Output { append: bool, target: Word },
    /// A heredoc; `heredoc` indexes it in [`Scan::heredocs`], and `quoted`
    /// says the body reaches the program byte for byte: the operator is
    /// `<<` and its delimiter is wholly quoted.
    Heredoc { quoted: bool, heredoc: usize },
    /// Any other redirection.
    Other,
}

/// The words and redirections of one simple command.
#[derive(Debug, Default)]
struct Statement {
    words: Vec<Word>,
    redirects: Vec<Redirect>,
}

/// A heredoc, whose body starts after the line that opens it.
#[derive(Debug)]
struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
    /// Whether the delimiter is bare, so that the shell processes the
    /// body: it joins a line ending in a backslash with the next before
    /// looking for the terminator, and it runs the expansions in it.
    expands: bool,
    /// The body, once its lines were read.
    body: Option<String>,
}

/// A recognized write whose body may not have been read yet.
#[derive(Debug)]
struct Draft {
    path: PathBuf,
    append: bool,
    /// Index of its heredoc in [`Scan::heredocs`].
    heredoc: usize,
}

/// The reader's state across one command.
struct Scan<'a> {
    lexer: Lexer<'a>,
    /// The directory the next list runs in, when known.
    dir: Option<PathBuf>,
    drafts: Vec<Draft>,
    /// Every heredoc opened so far, in order.
    heredocs: Vec<Heredoc>,
    /// How many of `heredocs` have had their body read.
    read: usize,
    /// The statements of the list being read (statements joined by `&&`,
    /// `||`, or `|`), each with how it is joined to the one before it.
    /// Nothing in a list takes effect before its end shows whether the
    /// whole list runs in the background.
    list: Vec<(Statement, Join)>,
}

impl Scan<'_> {
    /// Reads statements to the end of the command or the first stop.
    fn run(&mut self) -> Result<(), Stop> {
        let mut joined = Join::Sequence;
        let mut statement = Statement::default();
        loop {
            self.lexer.skip_blanks();
            let Some(c) = self.lexer.peek() else {
                self.push(statement, joined);
                return self.end_list(false);
            };
            let ended = match c {
                '\n' => {
                    self.lexer.pos += 1;
                    Some(Join::Sequence)
                }
                '#' => {
                    self.lexer.skip_line();
                    None
                }
                ';' => {
                    self.lexer.pos += 1;
                    if self.lexer.eat(';') {
                        return Err(Stop);
                    }
                    Some(Join::Sequence)
                }
                '&' => {
                    self.lexer.pos += 1;
                    if self.lexer.eat('&') {
                        Some(Join::And)
                    } else if self.lexer.eat('>') {
                        self.lexer.eat('>');
                        self.lexer.target()?;
                        statement.redirects.push(Redirect::Other);
                        None
                    } else {
                        Some(Join::Background)
                    }
                }
                '|' => {
                    self.lexer.pos += 1;
                    if self.lexer.eat('|') {
                        Some(Join::Or)
                    } else {
                        self.lexer.eat('&');
                        Some(Join::Pipe)
                    }
                }
                '<' | '>' => {
                    self.redirect(&mut statement, false)?;
                    None
                }
                _ => {
                    let word = self.lexer.word()?;
                    if statement.words.is_empty()
                        && word.bare
                        && RESERVED.contains(&word.text.as_str())
                    {
                        return Err(Stop);
                    }
                    let descriptor = word.kind == WordKind::Plain
                        && word.text.bytes().all(|b| b.is_ascii_digit())
                        && matches!(self.lexer.peek(), Some('<' | '>'));
                    if descriptor {
                        self.redirect(&mut statement, true)?;
                    } else {
                        statement.words.push(word);
                    }
                    None
                }
            };
            let Some(ended) = ended else { continue };
            let empty = statement.words.is_empty() && statement.redirects.is_empty();
            match ended {
                Join::And | Join::Or | Join::Pipe => {
                    self.push(std::mem::take(&mut statement), joined);
                    joined = ended;
                }
                // A newline after `&&`, `||`, or `|` continues the list: the
                // operator still joins the statement on the next line.
                Join::Sequence if c == '\n' && empty && joined != Join::Sequence => {}
                Join::Sequence | Join::Background => {
                    self.push(std::mem::take(&mut statement), joined);
                    self.end_list(ended == Join::Background)?;
                    joined = Join::Sequence;
                }
            }
            if c == '\n' {
                self.bodies()?;
            }
        }
    }

    /// Adds a statement that has anything in it to the current list.
    fn push(&mut self, statement: Statement, joined: Join) {
        if !statement.words.is_empty() || !statement.redirects.is_empty() {
            self.list.push((statement, joined));
        }
    }

    /// Acts on the statements of the list just ended, in order. A list run
    /// in the background runs in a subshell, apart from this command: it
    /// moves no directory here, and its writes are not recognized.
    fn end_list(&mut self, background: bool) -> Result<(), Stop> {
        let list = std::mem::take(&mut self.list);
        if background {
            return Ok(());
        }
        // Whether a statement of this list may have moved the shell.
        let mut moved = false;
        let mut stopped = Ok(());
        for (i, (statement, joined)) in list.iter().enumerate() {
            // After `||`, a statement runs whether or not an earlier `cd`
            // of the list succeeded.
            if *joined == Join::Or && moved {
                self.dir = None;
            }
            let ended = list.get(i + 1).map_or(Join::Sequence, |(_, next)| *next);
            stopped = self.complete(statement, *joined, ended, &mut moved);
            if stopped.is_err() {
                break;
            }
        }
        // Past the list, nothing says whether its `cd` ran and succeeded.
        if moved {
            self.dir = None;
        }
        stopped
    }

    /// Reads one redirection at `<` or `>` into `statement`. `descriptor`
    /// says a file descriptor number preceded the operator.
    fn redirect(&mut self, statement: &mut Statement, descriptor: bool) -> Result<(), Stop> {
        let redirect = match self.lexer.operator() {
            Operator::Heredoc { strip_tabs } => {
                let delimiter = self.lexer.target()?;
                let quoted = !strip_tabs && !descriptor && delimiter.kind == WordKind::Quoted;
                self.heredocs.push(Heredoc {
                    expands: delimiter.bare,
                    delimiter: delimiter.text,
                    strip_tabs,
                    body: None,
                });
                Redirect::Heredoc {
                    quoted,
                    heredoc: self.heredocs.len() - 1,
                }
            }
            Operator::Output { append } if !descriptor => Redirect::Output {
                append,
                target: self.lexer.target()?,
            },
            Operator::Output { .. } | Operator::Other => {
                self.lexer.target()?;
                Redirect::Other
            }
        };
        statement.redirects.push(redirect);
        Ok(())
    }

    /// Acts on a statement of a foreground list: moves the directory for a
    /// `cd`, loses it for a program that may move it (setting `moved` in
    /// both cases), and drafts a heredoc write. Stops at a statement that
    /// leaves the shell whenever the command reaches it, since nothing
    /// after it runs, and at one after which `cat` may no longer mean what
    /// it does now: a builtin of [`REDEFINES`], a program named through an
    /// expansion (which could be one of them), or an assignment to `PATH`.
    fn complete(
        &mut self,
        statement: &Statement,
        joined: Join,
        ended: Join,
        moved: &mut bool,
    ) -> Result<(), Stop> {
        let Some(at) = statement.words.iter().position(|w| w.assigned().is_none()) else {
            // Assignments alone stay in the shell: a new `PATH` names
            // another `cat`.
            let path = statement.words.iter().any(|w| w.assigned() == Some("PATH"));
            return if path { Err(Stop) } else { Ok(()) };
        };
        let mut program = &statement.words[at];
        let mut arguments = &statement.words[at + 1..];
        // Runs in this shell, and not only when what precedes it failed.
        let direct = joined != Join::Pipe && joined != Join::Or && ended != Join::Pipe;
        // `builtin name …` and `command name …` run `name`. `command -v`
        // and `-V` only ask where a name comes from.
        let mut wrapped = false;
        while program.literal() && WRAPPERS.contains(&program.text.as_str()) {
            let options = arguments
                .iter()
                .take_while(|word| word.text.starts_with('-'))
                .count();
            if !arguments[..options].iter().all(Word::literal) {
                return Err(Stop);
            }
            let asks = program.text == "command"
                && arguments[..options]
                    .iter()
                    .any(|word| word.text.contains(['v', 'V']));
            let Some(inner) = arguments.get(options).filter(|_| !asks) else {
                break;
            };
            program = inner;
            arguments = &arguments[options + 1..];
            wrapped = true;
        }
        if !program.literal() || REDEFINES.contains(&program.text.as_str()) {
            return Err(Stop);
        }
        let name = program.text.as_str();
        // `printf -v name` sets a variable, and so does a `%n` conversion
        // in its format. Either may also reach it through an expansion.
        let sets_variable = name == "printf"
            && arguments.first().is_some_and(|first| {
                !first.literal() || first.text.starts_with("-v") || stores_count(&first.text)
            });
        if sets_variable {
            return Err(Stop);
        }
        // `exec` with a program replaces the shell; with redirections only
        // it goes on.
        let leaves = LEAVES.contains(&name) || (name == "exec" && !arguments.is_empty());
        if leaves && direct && joined == Join::Sequence {
            return Err(Stop);
        }
        if wrapped {
            // Whatever ran, it was not the plain `cd` or `cat` read below.
            *moved = true;
            self.dir = None;
            return Ok(());
        }
        if name == "cd" {
            *moved = true;
            self.dir = match arguments {
                // A plain relative name may be found through `CDPATH`.
                [to] if direct
                    && at == 0
                    && statement.redirects.is_empty()
                    && to.literal()
                    && (to.text == "."
                        || to.text == ".."
                        || to.text.starts_with(['/'])
                        || to.text.starts_with("./")
                        || to.text.starts_with("../")) =>
                {
                    self.resolve(&to.text)
                }
                _ => None,
            };
        } else if MOVES.contains(&name) {
            *moved = true;
            self.dir = None;
        } else if name == "cat" && at == 0 && arguments.is_empty() && direct {
            self.draft(&statement.redirects);
        }
        Ok(())
    }

    /// Drafts the write of a `cat` statement whose redirections are one
    /// bare output to a literal target and one quoted heredoc.
    fn draft(&mut self, redirects: &[Redirect]) {
        let (append, target, heredoc) = match redirects {
            [
                Redirect::Output { append, target },
                Redirect::Heredoc {
                    quoted: true,
                    heredoc,
                },
            ]
            | [
                Redirect::Heredoc {
                    quoted: true,
                    heredoc,
                },
                Redirect::Output { append, target },
            ] => (*append, target, *heredoc),
            _ => return,
        };
        if !target.literal() || target.text.is_empty() {
            return;
        }
        let Some(path) = self.resolve(&target.text) else {
            return;
        };
        self.drafts.push(Draft {
            path,
            append,
            heredoc,
        });
    }

    /// `path` as the shell would resolve it from the current directory, or
    /// `None` when it is relative and the directory is unknown. `.`
    /// components name the directory they are in and are dropped.
    fn resolve(&self, path: &str) -> Option<PathBuf> {
        let joined = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.dir.as_ref()?.join(path)
        };
        Some(
            joined
                .components()
                .filter(|component| !matches!(component, Component::CurDir))
                .collect(),
        )
    }

    /// Reads the bodies of the heredocs opened on the line just ended.
    fn bodies(&mut self) -> Result<(), Stop> {
        while let Some(heredoc) = self.heredocs.get_mut(self.read) {
            let body = self
                .lexer
                .body(&heredoc.delimiter, heredoc.strip_tabs, heredoc.expands)
                .ok_or(Stop)?;
            heredoc.body = Some(body);
            self.read += 1;
        }
        Ok(())
    }

    /// The recognized writes, without those whose body was never read.
    fn finish(self) -> Vec<ShellWrite> {
        let Self {
            drafts,
            mut heredocs,
            ..
        } = self;
        drafts
            .into_iter()
            .filter_map(|draft| {
                Some(ShellWrite {
                    body: heredocs[draft.heredoc].body.take()?,
                    path: draft.path,
                    append: draft.append,
                })
            })
            .collect()
    }
}

/// Whether a `printf` format holds a `%n` conversion, which stores a count
/// in the variable its argument names. Flags, a width, a precision, and a
/// length modifier may stand between the `%` and the `n`; `%%` is a
/// percent sign.
fn stores_count(format: &str) -> bool {
    let mut rest = format;
    while let Some(at) = rest.find('%') {
        let conversion = rest[at + 1..]
            .trim_start_matches(|c: char| c.is_ascii_digit() || "-+ #.*$'".contains(c))
            .trim_start_matches(['h', 'l', 'L', 'q', 'j', 'z', 't']);
        match conversion.chars().next() {
            Some('n') => return true,
            Some(c) => rest = &conversion[c.len_utf8()..],
            None => return false,
        }
    }
    false
}

/// A redirection operator.
enum Operator {
    /// `>` or `>>`.
    Output { append: bool },
    /// `<<` or `<<-`.
    Heredoc { strip_tabs: bool },
    /// `<`, `<<<`, `<>`, `<&`, `>&`, or `>|`.
    Other,
}

/// A cursor over the command text.
struct Lexer<'a> {
    text: &'a str,
    /// Byte offset of the next unread character.
    pos: usize,
}

impl Lexer<'_> {
    /// The next character of the command as the shell reads it: after any
    /// backslash-newline pairs, which continue a line and leave nothing.
    /// The cursor is left at that character.
    fn peek(&mut self) -> Option<char> {
        while self.text[self.pos..].starts_with("\\\n") {
            self.pos += 2;
        }
        self.text[self.pos..].chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// The next character as written, for the one a backslash escapes.
    fn next_as_written(&mut self) -> Option<char> {
        let c = self.text[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Consumes `c` when it is next.
    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += c.len_utf8();
        }
        found
    }

    /// Skips spaces and tabs.
    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    /// Skips to the end of the line, leaving its newline unread.
    fn skip_line(&mut self) {
        self.pos += self.text[self.pos..]
            .find('\n')
            .unwrap_or(self.text.len() - self.pos);
    }

    /// Reads the operator at `<` or `>`.
    fn operator(&mut self) -> Operator {
        if self.eat('>') {
            if self.eat('>') {
                Operator::Output { append: true }
            } else if self.eat('&') || self.eat('|') {
                Operator::Other
            } else {
                Operator::Output { append: false }
            }
        } else {
            self.pos += 1;
            if !self.eat('<') {
                let _ = self.eat('>') || self.eat('&');
                Operator::Other
            } else if self.eat('<') {
                Operator::Other
            } else {
                Operator::Heredoc {
                    strip_tabs: self.eat('-'),
                }
            }
        }
    }

    /// Reads the word a redirection operator applies to.
    fn target(&mut self) -> Result<Word, Stop> {
        self.skip_blanks();
        match self.peek() {
            None | Some('\n' | ';' | '&' | '|' | '<' | '>') => Err(Stop),
            Some(_) => self.word(),
        }
    }

    /// Reads one word, removing its quotes and escapes as the shell does.
    fn word(&mut self) -> Result<Word, Stop> {
        let mut text = String::new();
        // Runs of unquoted characters and quoted strings the word is made of.
        let mut segments = 0;
        let mut quoted = 0;
        let mut in_plain = false;
        let mut expands = false;
        let mut bare = true;
        while let Some(c) = self.peek() {
            if matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>') {
                break;
            }
            self.pos += c.len_utf8();
            match c {
                '(' | ')' | '`' => return Err(Stop),
                '\'' => {
                    let rest = &self.text[self.pos..];
                    let end = rest.find('\'').ok_or(Stop)?;
                    text.push_str(&rest[..end]);
                    self.pos += end + 1;
                    segments += 1;
                    quoted += 1;
                    in_plain = false;
                    bare = false;
                    continue;
                }
                '"' => {
                    expands |= self.double_quoted(&mut text)?;
                    segments += 1;
                    quoted += 1;
                    in_plain = false;
                    bare = false;
                    continue;
                }
                // Never before a newline: `peek` removed that pair.
                '\\' => {
                    text.push(self.next_as_written().ok_or(Stop)?);
                    expands = true;
                    bare = false;
                }
                // `$(…)`, `${…}`, `$[…]`, `$'…'`, and `$"…"` each have rules
                // of their own for what follows.
                '$' => {
                    if matches!(self.peek(), Some('(' | '{' | '[' | '\'' | '"')) {
                        return Err(Stop);
                    }
                    text.push(c);
                    expands = true;
                }
                '*' | '?' | '[' | ']' | '{' | '}' => {
                    text.push(c);
                    expands = true;
                }
                '~' if segments == 0 => {
                    text.push(c);
                    expands = true;
                }
                _ => text.push(c),
            }
            if !in_plain {
                segments += 1;
                in_plain = true;
            }
        }
        let kind = if expands || segments != 1 {
            WordKind::Other
        } else if quoted == 1 {
            WordKind::Quoted
        } else {
            WordKind::Plain
        };
        Ok(Word { text, kind, bare })
    }

    /// Reads a double-quoted string after its opening quote into `text`.
    /// Returns whether it holds a `$` or a backslash, either of which the
    /// shell may rewrite. Inside double quotes a backslash escapes only
    /// `$`, a backtick, `"`, and `\`; before any other character it stays.
    fn double_quoted(&mut self, text: &mut String) -> Result<bool, Stop> {
        let mut expands = false;
        loop {
            match self.next().ok_or(Stop)? {
                '"' => return Ok(expands),
                '`' => return Err(Stop),
                '$' => {
                    if matches!(self.peek(), Some('(' | '{' | '[')) {
                        return Err(Stop);
                    }
                    text.push('$');
                    expands = true;
                }
                // Never before a newline: `next` removed that pair.
                '\\' => {
                    expands = true;
                    match self.next_as_written().ok_or(Stop)? {
                        escaped @ ('$' | '`' | '"' | '\\') => text.push(escaped),
                        kept => {
                            text.push('\\');
                            text.push(kept);
                        }
                    }
                }
                c => text.push(c),
            }
        }
    }

    /// Reads a heredoc body from the start of a line up to and including its
    /// terminator line.
    ///
    /// Returns `None` when the terminator is missing. With `expands` (a
    /// bare delimiter), the shell processes the body. It joins a line
    /// ending in a backslash with the next one before it looks for the
    /// terminator, so the physical lines no longer say where the body ends;
    /// and it runs command substitutions and arithmetic in it, which can
    /// change the shell (`$((PATH=0))`). A body with either is not read:
    /// `None` is returned at that line.
    fn body(&mut self, delimiter: &str, strip_tabs: bool, expands: bool) -> Option<String> {
        let mut body = String::new();
        loop {
            if self.pos >= self.text.len() {
                return None;
            }
            let rest = &self.text[self.pos..];
            let (line, read) = match rest.find('\n') {
                Some(end) => (&rest[..end], end + 1),
                None => (rest, rest.len()),
            };
            self.pos += read;
            let compared = if strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if compared == delimiter {
                return Some(body);
            }
            let processed = line.ends_with('\\')
                || line.contains('`')
                || ["$(", "${", "$["].iter().any(|open| line.contains(open));
            if expands && processed {
                return None;
            }
            body.push_str(line);
            body.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writes of `command` run from `/work/repo`, as
    /// `(path, append, body)`.
    fn writes(command: &str) -> Vec<(String, bool, String)> {
        shell_writes(command, Path::new("/work/repo"))
            .into_iter()
            .map(|w| {
                let path = w.path.to_string_lossy().replace('\\', "/");
                (path, w.append, w.body)
            })
            .collect()
    }

    fn write(path: &str, body: &str) -> (String, bool, String) {
        (path.to_owned(), false, body.to_owned())
    }

    fn append(path: &str, body: &str) -> (String, bool, String) {
        (path.to_owned(), true, body.to_owned())
    }

    #[test]
    fn a_quoted_heredoc_redirected_to_a_file_is_a_write() {
        assert_eq!(
            writes("cat > src/a.rs <<'EOF'\nfn a() {}\n\n  $x `y` \\n\nEOF\n"),
            vec![write("/work/repo/src/a.rs", "fn a() {}\n\n  $x `y` \\n\n")]
        );
        // Double quotes around the delimiter quote the body as well.
        assert_eq!(
            writes("cat > a.rs <<\"EOF\"\n$x\nEOF"),
            vec![write("/work/repo/a.rs", "$x\n")]
        );
        // The heredoc may come before the redirection, with or without blanks.
        assert_eq!(
            writes("cat << 'EOF' >a.rs\nx\nEOF\n"),
            vec![write("/work/repo/a.rs", "x\n")]
        );
        assert_eq!(
            writes("cat >>a.rs <<'EOF'\nx\nEOF\n"),
            vec![append("/work/repo/a.rs", "x\n")]
        );
    }

    #[test]
    fn a_body_runs_to_the_first_line_that_is_exactly_the_delimiter() {
        assert_eq!(
            writes("cat > a <<'EOF'\n EOF\nEOF \nEOFX\nEOF\nlater\n"),
            vec![write("/work/repo/a", " EOF\nEOF \nEOFX\n")]
        );
        // An empty body is an empty file.
        assert_eq!(
            writes("cat > a <<'EOF'\nEOF\n"),
            vec![write("/work/repo/a", "")]
        );
        // A delimiter may hold spaces.
        assert_eq!(
            writes("cat > a <<'END OF FILE'\nx\nEND OF FILE\n"),
            vec![write("/work/repo/a", "x\n")]
        );
    }

    #[test]
    fn a_heredoc_without_its_terminator_is_no_write() {
        assert_eq!(writes("cat > a <<'EOF'\nx\n"), vec![]);
        assert_eq!(writes("cat > a <<'EOF'"), vec![]);
        // Writes read in full before it stand.
        assert_eq!(
            writes("cat > a <<'A'\n1\nA\ncat > b <<'B'\n2\n"),
            vec![write("/work/repo/a", "1\n")]
        );
    }

    #[test]
    fn a_body_the_shell_may_rewrite_is_no_write() {
        for command in [
            // Unquoted and partly quoted delimiters.
            "cat > a <<EOF\nx\nEOF\n",
            "cat > a <<\\EOF\nx\nEOF\n",
            "cat > a <<'E'OF\nx\nEOF\n",
            "cat > a <<E\"O\"F\nx\nEOF\n",
            // A double-quoted delimiter holding `$`: the shell takes it
            // literally, but this reader keeps to delimiters with nothing
            // in them that could be an expansion.
            "cat > a <<\"$EOF\"\nx\n$EOF\n",
            // `<<-` strips leading tabs.
            "cat > a <<-'EOF'\n\tx\n\tEOF\n",
            // A here-string is not a heredoc.
            "cat > a <<<'EOF'\n",
        ] {
            assert_eq!(writes(command), vec![], "{command}");
        }
    }

    #[test]
    fn only_a_bare_cat_with_one_output_and_one_heredoc_is_a_write() {
        for command in [
            "cat - > a <<'EOF'\nx\nEOF\n",
            "cat b > a <<'EOF'\nx\nEOF\n",
            "tee a <<'EOF'\nx\nEOF\n",
            "sudo cat > a <<'EOF'\nx\nEOF\n",
            "LC_ALL=C cat > a <<'EOF'\nx\nEOF\n",
            "$CAT > a <<'EOF'\nx\nEOF\n",
            "cat <<'EOF'\nx\nEOF\n",
            "cat > a\n",
            "cat > a > b <<'EOF'\nx\nEOF\n",
            "cat > a 2>/dev/null <<'EOF'\nx\nEOF\n",
            "cat > a <<'EOF' 2>&1\nx\nEOF\n",
            "cat 1> a <<'EOF'\nx\nEOF\n",
            "cat >| a <<'EOF'\nx\nEOF\n",
            "cat &> a <<'EOF'\nx\nEOF\n",
            "cat >& a <<'EOF'\nx\nEOF\n",
            "cat > a 3<<'EOF'\nx\nEOF\n",
            "cat > a <<'EOF' <<'TWO'\nx\nEOF\ny\nTWO\n",
        ] {
            assert_eq!(writes(command), vec![], "{command}");
        }
    }

    #[test]
    fn the_target_must_be_one_literal_word() {
        for target in [
            "$DIR/a",
            "\"$DIR/a\"",
            "~/a",
            "a*",
            "a?",
            "a[1]",
            "{a,b}",
            "a\\ b",
            "`pwd`/a",
            "'a'b",
            "\"a\"'b'",
            "''",
            "\"a\\\"b\"",
        ] {
            let command = format!("cat > {target} <<'EOF'\nx\nEOF\n");
            assert_eq!(writes(&command), vec![], "{command}");
        }
        for (target, path) in [
            ("'a b'", "/work/repo/a b"),
            ("\"a b\"", "/work/repo/a b"),
            ("'$a'", "/work/repo/$a"),
            ("a~", "/work/repo/a~"),
            ("a#b", "/work/repo/a#b"),
            ("/abs/a", "/abs/a"),
            ("./a", "/work/repo/a"),
            ("./b/./a", "/work/repo/b/a"),
            ("../a", "/work/repo/../a"),
        ] {
            let command = format!("cat > {target} <<'EOF'\nx\nEOF\n");
            assert_eq!(writes(&command), vec![write(path, "x\n")], "{command}");
        }
    }

    #[test]
    fn statements_are_separated_by_and_semicolon_and_newline() {
        let command = "mkdir -p src && cat > src/a <<'A'; cat >> b <<'B'\n1\nA\n2\nB\n\
                       # cat > c <<'C'\necho done\ncat > d <<'D'\n4\nD\n";
        assert_eq!(
            writes(command),
            vec![
                write("/work/repo/src/a", "1\n"),
                append("/work/repo/b", "2\n"),
                write("/work/repo/d", "4\n"),
            ]
        );
    }

    #[test]
    fn other_statements_heredoc_bodies_are_data() {
        let command = "python3 - <<EOF\ncat > x <<'X'\nnot a write\nX\nEOF\n\
                       cat > a <<'A'\n1\nA\n";
        assert_eq!(writes(command), vec![write("/work/repo/a", "1\n")]);
        // A stripped heredoc ends at a tab-indented terminator.
        let stripped = "python3 - <<-EOF\n\tcat > x <<'X'\n\tEOF\ncat > a <<'A'\n1\nA\n";
        assert_eq!(writes(stripped), vec![write("/work/repo/a", "1\n")]);
    }

    #[test]
    fn a_conditional_piped_or_background_statement_is_no_write() {
        for command in [
            "test -f a || cat > a <<'EOF'\nx\nEOF\n",
            "cat > a <<'EOF' | wc -l\nx\nEOF\n",
            "echo x | cat > a <<'EOF'\nx\nEOF\n",
            "echo x |& cat > a <<'EOF'\nx\nEOF\n",
            "cat > a <<'EOF' &\nx\nEOF\n",
        ] {
            assert_eq!(writes(command), vec![], "{command}");
        }
        // The statement after a background one runs in the foreground.
        assert_eq!(
            writes("sleep 1 & cat > a <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/a", "x\n")]
        );
    }

    #[test]
    fn recognition_stops_at_what_is_not_a_simple_command() {
        for stop in [
            "(cd sub)",
            "{ true; }",
            "if true; then true; fi",
            "for f in a b; do true; done",
            "while false; do true; done",
            "case x in x) true;; esac",
            "f() { true; }",
            "function f { true; }",
            "[[ -f a ]]",
            "! true",
            "time ls",
            "echo $(date)",
            "echo \"$(date)\"",
            "echo `date`",
            "echo \"`date`\"",
            "echo ${HOME}",
            "echo \"${HOME}\"",
            "echo $'a'",
            "echo $\"a\"",
            "echo 'unclosed",
            "echo \"unclosed",
            "diff <(true) a",
            "echo >",
        ] {
            let command = format!("cat > a <<'A'\n1\nA\n{stop}\ncat > b <<'B'\n2\nB\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
    }

    #[test]
    fn ordinary_statements_do_not_stop_recognition() {
        for ordinary in [
            "echo \"a $HOME b\" 'c' d\\ e",
            "RUST_LOG=debug cargo test 2>&1 | tail -5",
            "git commit -q -m \"x; y && z\"",
            "grep -n 'a|b' f > out.txt 2> /dev/null < in.txt",
            "echo done # (not a subshell",
            "wc -l <<< 'x'",
            "echo a \\\n  b",
            "X=1",
            "echo if then fi",
        ] {
            let command = format!("{ordinary}\ncat > a <<'A'\n1\nA\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
    }

    #[test]
    fn a_literal_cd_moves_the_directory_of_later_statements() {
        assert_eq!(
            writes("cd ./crates/a && cat > src/x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/crates/a/src/x", "x\n")]
        );
        assert_eq!(
            writes("cd ./crates && cd './a b' && true &&\ncat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/crates/a b/x", "x\n")]
        );
        assert_eq!(
            writes("cd /other && cat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/other/x", "x\n")]
        );
        // The path is joined as written; the caller rejects `..`.
        assert_eq!(
            writes("cd .. && cat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/../x", "x\n")]
        );
        // A write before the `cd` runs in the starting directory.
        assert_eq!(
            writes("cat > x <<'EOF' && cd sub\nx\nEOF\n"),
            vec![write("/work/repo/x", "x\n")]
        );
    }

    #[test]
    fn a_cd_counts_only_for_statements_that_run_when_it_succeeded() {
        // On the next line or after `;`, the statement also runs when the
        // `cd` failed, in the directory the shell was in.
        for command in [
            "cd ./sub\ncat > x <<'EOF'\nx\nEOF\n",
            "cd ./sub; cat > x <<'EOF'\nx\nEOF\n",
            "false && cd ./sub; cat > x <<'EOF'\nx\nEOF\n",
            "cd ./sub && true\ncat > x <<'EOF'\nx\nEOF\n",
            "cd ./sub || true && cat > x <<'EOF'\nx\nEOF\n",
            "cd ./sub && true || true && cat > x <<'EOF'\nx\nEOF\n",
        ] {
            assert_eq!(writes(command), vec![], "{command}");
        }
        // An `||` before the `cd` does not come between it and the write.
        assert_eq!(
            writes("false || true && cd ./sub && cat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/sub/x", "x\n")]
        );
        // A list that moves nothing leaves the directory known.
        assert_eq!(
            writes("true || false\ncat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/x", "x\n")]
        );
    }

    #[test]
    fn a_directory_change_that_cannot_be_read_leaves_only_absolute_targets() {
        for lost in [
            "cd",
            "cd -",
            "cd ~",
            "cd $DIR",
            "cd \"$DIR\"",
            "cd -P ./sub",
            "cd ./a ./b",
            "cd ./sub > /dev/null",
            "X=1 cd ./sub",
            "true || cd ./sub",
            "cd ./sub | true",
            // A plain relative name may be found through `CDPATH`.
            "cd sub",
            "cd crates/a",
            "pushd ./sub",
            "popd",
            "exec 2>&1",
            "builtin cd ./sub",
            "command cd ./sub",
        ] {
            let command = format!("{lost}\ncat > rel <<'A'\n1\nA\ncat > /abs/x <<'B'\n2\nB\n");
            assert_eq!(writes(&command), vec![write("/abs/x", "2\n")], "{command}");
            // Chained with `&&`, a relative write is still not resolved.
            let chained = format!("{lost} && cat > rel <<'A'\n1\nA\n");
            assert_eq!(writes(&chained), vec![], "{chained}");
        }
        // An absolute `cd` makes the directory known again.
        assert_eq!(
            writes("cd $DIR; cd /known && cat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/known/x", "x\n")]
        );
        // `.` and `..` are not looked up in `CDPATH` either.
        assert_eq!(
            writes("cd . && cd .. && cat > x <<'EOF'\nx\nEOF\n"),
            vec![write("/work/repo/../x", "x\n")]
        );
    }

    #[test]
    fn nothing_is_recognized_after_a_statement_that_may_redefine_cat() {
        for redefining in [
            "alias cat='tr a b'",
            "unalias -a",
            "shopt -s expand_aliases",
            "set -n",
            "set -o noclobber",
            "enable -n echo",
            "trap 'true' DEBUG",
            "export PATH=/x:$PATH",
            "declare -f",
            "typeset x",
            "local x",
            "readonly x",
            "unset -f cat",
            "hash -p /x/cat cat",
            "eval 'cat() { tr a b; }'",
            "source env.sh",
            ". env.sh",
            "PATH=/x",
            "PATH+=:/x",
            "X=1 PATH=/x",
            "$RUN it",
            "\"$RUN\" it",
            // Through a wrapper, the same builtins.
            "builtin alias cat='tr a b'",
            "command -p shopt -s expand_aliases",
            "builtin command builtin set -n",
            "command $RUN",
            "builtin -$X alias",
            // Builtins that set a variable, which may be `PATH`.
            "printf -v PATH '%s' /x",
            "printf -vPATH /x",
            "printf $OPTION PATH /x",
            "read PATH",
            "mapfile -t PATH",
            "readarray PATH",
            "getopts a PATH",
            "let PATH=0",
            "printf '%n' PATH",
            "printf 'a%5n b' PATH",
            "printf '%s%-3.2n' x PATH",
            "printf '%ln' PATH",
            "printf '%5hhn' PATH",
            // A limit on what a program may write.
            "ulimit -f 0",
            // Whether or not it ran, a later `cat` is in doubt.
            "false && alias cat=x",
            "true || set -n",
        ] {
            let command = format!("cat > a <<'A'\n1\nA\n{redefining}\ncat > /abs/b <<'B'\n2\nB\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
        for ordinary in [
            "X=1",
            "PATHS=/x",
            "PATH=/x cargo build",
            "echo alias cat=x",
            "echo export PATH=/x",
            "printf '%s' set",
            "printf '%s -v' x",
            "printf '100%%n'",
            "printf '%d names' 3",
            "printf '%ld lines' 3",
            // `command -v` only asks where a name comes from.
            "command -v alias > /dev/null",
            "command -V set",
            "command",
        ] {
            let command = format!("{ordinary}\ncat > a <<'A'\n1\nA\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
    }

    #[test]
    fn a_locale_quoted_delimiter_stops_recognition() {
        // The shell reads `$"EOF"` as the delimiter `EOF`: the line `$EOF`
        // is data, and so is the `cat > f` after it.
        let command = "cat >/dev/null <<$\"EOF\"\n$EOF\ncat > f <<'E'\nx\nE\nEOF\necho y > f\n";
        assert_eq!(writes(command), vec![]);
    }

    #[test]
    fn text_that_is_not_ascii_is_read_as_written() {
        assert_eq!(
            writes("echo 'häää' && cat > 'ä ö.txt' <<'LOPPU'\nyö — ☃\nLOPPU\n"),
            vec![write("/work/repo/ä ö.txt", "yö — ☃\n")]
        );
    }

    #[test]
    fn a_carriage_return_is_data_not_a_line_ending() {
        // The return joins the delimiter word, which is then not wholly quoted.
        assert_eq!(writes("cat > a <<'EOF'\r\nx\r\nEOF\r\n"), vec![]);
        assert_eq!(
            writes("cat > a <<'EOF'\nx\r\nEOF\n"),
            vec![write("/work/repo/a", "x\r\n")]
        );
    }

    #[test]
    fn a_long_command_is_read_in_time_proportional_to_its_length() {
        use std::fmt::Write as _;

        let count = 50_000;
        let mut command = String::new();
        for i in 0..count {
            write!(
                command,
                "echo {i} 'q' \"d\" > /dev/null 2>&1; true && cat > f{i} <<'E'\n{i}\nE\n"
            )
            .unwrap();
        }
        let started = std::time::Instant::now();
        let found = shell_writes(&command, Path::new("/work/repo"));
        assert_eq!(found.len(), count);
        assert_eq!(found[count - 1].body, format!("{}\n", count - 1));
        // Rescanning the text per statement would take minutes here.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn an_unquoted_heredoc_whose_body_joins_lines_stops_recognition() {
        // The shell joins `prefix\` with the next line, so the first `EOF`
        // is data and the `cat > f` after it is too.
        let joined = "cat >/dev/null <<EOF\nprefix\\\nEOF\ncat > f <<'E'\nx\nE\nEOF\necho y > f\n";
        assert_eq!(writes(joined), vec![]);
        // A terminator split over two lines ends the body for the shell;
        // the physical lines do not show it, so nothing after is read.
        let split = "cat >/dev/null <<EOF\nEO\\\nF\ncat > a <<'A'\n1\nA\n";
        assert_eq!(writes(split), vec![]);
        // What was read in full before such a body stands.
        let after = "cat > a <<'A'\n1\nA\ncat >/dev/null <<EOF\nx\\\nEOF\n";
        assert_eq!(writes(after), vec![write("/work/repo/a", "1\n")]);
        // A quoted delimiter joins nothing: the backslash is data.
        assert_eq!(
            writes("cat > a <<'EOF'\nx\\\nEOF\n"),
            vec![write("/work/repo/a", "x\\\n")]
        );
        let other = "python3 - <<'EOF'\nprefix\\\nEOF\ncat > a <<'A'\n1\nA\n";
        assert_eq!(writes(other), vec![write("/work/repo/a", "1\n")]);
    }

    #[test]
    fn a_double_quoted_delimiter_keeps_a_backslash_the_shell_keeps() {
        // The delimiter is `E\q`: the line `Eq` is data, and so is the
        // `cat > f` after it.
        let command =
            "cat >/dev/null <<\"E\\q\"\nEq\ncat > f <<'Z'\nx\nZ\nE\\q\ncat > a <<'A'\n1\nA\n";
        assert_eq!(writes(command), vec![write("/work/repo/a", "1\n")]);
        // An escaped quote or backslash loses its backslash.
        let escaped = "cat >/dev/null <<\"E\\\"\\\\\"\nE\"\\\ncat > a <<'A'\n1\nA\n";
        assert_eq!(writes(escaped), vec![write("/work/repo/a", "1\n")]);
    }

    #[test]
    fn an_operator_before_a_newline_still_joins_the_next_statement() {
        for command in [
            "true ||\ncat > f <<'E'\nx\nE\n",
            "true || # try the next line\n\n  cat > f <<'E'\nx\nE\n",
            "echo x |\ncat > f <<'E'\nx\nE\n",
            "echo x |&\ncat > f <<'E'\nx\nE\n",
        ] {
            assert_eq!(writes(command), vec![], "{command}");
        }
        // After `&&` the statement on the next line is still a write.
        assert_eq!(
            writes("mkdir -p d &&\ncat > d/f <<'E'\nx\nE\n"),
            vec![write("/work/repo/d/f", "x\n")]
        );
    }

    #[test]
    fn a_reserved_word_written_over_a_continuation_stops_recognition() {
        let command = "i\\\nf false; th\\\nen\ncat > f <<'E'\nx\nE\nfi\necho y > f\n";
        assert_eq!(writes(command), vec![]);
        // Quoted, the same letters are an ordinary word, as they are when
        // they do not start the statement.
        for ordinary in ["'if' x", "\"then\"", "echo time"] {
            let command = format!("{ordinary}\ncat > a <<'A'\n1\nA\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
        // Escaped, they name a program through a word that is not literal,
        // which stops recognition for another reason.
        assert_eq!(writes("\\if x\ncat > /abs/a <<'A'\n1\nA\n"), vec![]);
    }

    #[test]
    fn a_list_run_in_the_background_writes_and_moves_nothing() {
        // `&` applies to the whole list, not to its last statement.
        assert_eq!(writes("cat > f <<'E' && true &\nx\nE\nwait\n"), vec![]);
        assert_eq!(writes("true && cat > f <<'E' &\nx\nE\n"), vec![]);
        // A `cd` in a background list runs in a subshell.
        assert_eq!(
            writes("cd sub && true &\nwait\ncat > f <<'E'\nx\nE\n"),
            vec![write("/work/repo/f", "x\n")]
        );
        assert_eq!(
            writes("eval x &\ncat > f <<'E'\nx\nE\n"),
            vec![write("/work/repo/f", "x\n")]
        );
    }

    #[test]
    fn a_timed_statement_stops_recognition() {
        // `time` is a reserved word: `time cd sub` moves this shell.
        assert_eq!(writes("time cd sub; cat > f <<'E'\nx\nE\n"), vec![]);
        assert_eq!(writes("time -p cat > f <<'E'\nx\nE\n"), vec![]);
    }

    #[test]
    fn nothing_is_recognized_after_a_statement_that_leaves_the_shell() {
        for leaving in [
            "exit",
            "exit 0",
            "return 1",
            "logout",
            "exec bash",
            "exec ./run.sh a",
        ] {
            let command = format!("cat > a <<'A'\n1\nA\n{leaving}\ncat > /abs/b <<'B'\n2\nB\n");
            assert_eq!(
                writes(&command),
                vec![write("/work/repo/a", "1\n")],
                "{command}"
            );
        }
        // Reached only on a condition, or in a subshell, it leaves nothing:
        // a later statement that runs shows it did not happen.
        for staying in [
            "test -d src || exit 1",
            "true && exit",
            "exit | true",
            "exit &",
            "exec 2>&1",
            "echo exit",
        ] {
            let command = format!("{staying}\ncat > /abs/b <<'B'\n2\nB\n");
            assert_eq!(writes(&command), vec![write("/abs/b", "2\n")], "{command}");
        }
    }

    #[test]
    fn many_heredocs_on_one_line_are_read_in_time_proportional_to_their_number() {
        use std::fmt::Write as _;

        let count = 50_000;
        let mut command = String::from("cat");
        for i in 0..count {
            write!(command, " <<D{i}").unwrap();
        }
        command.push('\n');
        for i in 0..count {
            writeln!(command, "body\nD{i}").unwrap();
        }
        command.push_str("cat > a <<'A'\n1\nA\n");
        let started = std::time::Instant::now();
        assert_eq!(writes(&command), vec![write("/work/repo/a", "1\n")]);
        // Shifting the pending heredocs once per body would take minutes.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_line_continuation_is_removed_wherever_the_shell_removes_it() {
        // Inside `&&`: the `cd` is chained to the write, not backgrounded.
        assert_eq!(
            writes("cd ./sub &\\\n& cat > f <<'E'\nx\nE\n"),
            vec![write("/work/repo/sub/f", "x\n")]
        );
        // Inside `||` and a pipe, the statement stays conditional or piped.
        assert_eq!(writes("true |\\\n| cat > f <<'E'\nx\nE\n"), vec![]);
        // Inside the program name, the operator, and the target.
        assert_eq!(
            writes("c\\\nat >\\\n> a.\\\nrs <\\\n<'E'\nx\nE\n"),
            vec![append("/work/repo/a.rs", "x\n")]
        );
        // Between `$` and the quote of a locale string: still that string.
        let locale = "cat >/dev/null <<$\\\n\"EOF\"\n$EOF\ncat > f <<'E'\nx\nE\nEOF\necho y > f\n";
        assert_eq!(writes(locale), vec![]);
        assert_eq!(writes("echo $\\\n(date)\ncat > a <<'A'\n1\nA\n"), vec![]);
        // An escaped backslash before a newline is a backslash, then the
        // end of the statement.
        assert_eq!(
            writes("echo a\\\\\ncat > a <<'A'\n1\nA\n"),
            vec![write("/work/repo/a", "1\n")]
        );
        // In single quotes, a comment, and a quoted body it is text.
        assert_eq!(
            writes("echo 'a\\\nb' # c \\\ncat > a <<'A'\n1\\\nA\n"),
            vec![write("/work/repo/a", "1\\\n")]
        );
    }

    #[test]
    fn a_wrapped_program_is_read_as_the_program_it_runs() {
        // A wrapped `cat` is not the bare form, and a wrapped `cd` is not
        // resolved.
        assert_eq!(writes("command cat > a <<'A'\n1\nA\n"), vec![]);
        assert_eq!(writes("builtin cd ./sub && cat > x <<'A'\n1\nA\n"), vec![]);
        // After any wrapped program only absolute targets resolve.
        assert_eq!(
            writes("command ls\ncat > x <<'A'\n1\nA\ncat > /abs/y <<'B'\n2\nB\n"),
            vec![write("/abs/y", "2\n")]
        );
        // A wrapped `exit` leaves the shell like a bare one.
        assert_eq!(writes("builtin exit 0\ncat > /abs/y <<'B'\n2\nB\n"), vec![]);
    }

    #[test]
    fn an_unquoted_heredoc_that_runs_an_expansion_stops_recognition() {
        for body in [
            "$((PATH=0))",
            "${PATH:=/x}",
            "$[PATH=0]",
            "`alias cat=x`",
            "a $(date) b",
        ] {
            let command = format!(": <<EOF\n{body}\nEOF\ncat > /abs/a <<'A'\n1\nA\n");
            assert_eq!(writes(&command), vec![], "{command}");
        }
        // A plain variable changes nothing, and a quoted body runs nothing.
        for command in [
            ": <<EOF\n$HOME and $1\nEOF\ncat > /abs/a <<'A'\n1\nA\n",
            ": <<'EOF'\n$((PATH=0)) `x` ${y}\nEOF\ncat > /abs/a <<'A'\n1\nA\n",
        ] {
            assert_eq!(writes(command), vec![write("/abs/a", "1\n")], "{command}");
        }
        // The old arithmetic form stops in a word and in double quotes too.
        assert_eq!(writes("echo $[1+1]\ncat > /abs/a <<'A'\n1\nA\n"), vec![]);
        assert_eq!(
            writes("echo \"$[PATH=0]\"\ncat > /abs/a <<'A'\n1\nA\n"),
            vec![]
        );
    }
}
