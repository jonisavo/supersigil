use std::fmt;
use std::io::{self, IsTerminal, Write};

use anstyle::{AnsiColor, Effects, Style};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Semantic color tokens
// ---------------------------------------------------------------------------

/// Semantic color token for terminal output styling.
#[derive(Debug, Clone, Copy)]
pub enum Token {
    /// Section header (bold).
    Header,
    /// Field label (bold).
    Label,
    /// Document ID (cyan).
    DocId,
    /// Document type (blue).
    DocType,
    /// Default status (yellow).
    Status,
    /// Good/passing status (green).
    StatusGood,
    /// Bad/failing status (red).
    StatusBad,
    /// Informational status (yellow).
    StatusInfo,
    /// Superseded status (dimmed).
    StatusSuperseded,
    /// Numeric count (bold).
    Count,
    /// File path (dimmed).
    Path,
    /// Success indicator (bold green).
    Success,
    /// Error indicator (bold red).
    Error,
    /// Warning indicator (bold yellow).
    Warning,
    /// Hint text (dimmed).
    Hint,
}

impl Token {
    fn style(self) -> Style {
        match self {
            Token::Header | Token::Label | Token::Count => Style::new().effects(Effects::BOLD),
            Token::DocId => Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            Token::DocType => Style::new().fg_color(Some(AnsiColor::Blue.into())),
            Token::Status | Token::StatusInfo => {
                Style::new().fg_color(Some(AnsiColor::Yellow.into()))
            }
            Token::StatusGood => Style::new().fg_color(Some(AnsiColor::Green.into())),
            Token::StatusBad => Style::new().fg_color(Some(AnsiColor::Red.into())),
            Token::StatusSuperseded | Token::Path | Token::Hint => {
                Style::new().effects(Effects::DIMMED)
            }
            Token::Success => Style::new()
                .fg_color(Some(AnsiColor::Green.into()))
                .effects(Effects::BOLD),
            Token::Error => Style::new()
                .fg_color(Some(AnsiColor::Red.into()))
                .effects(Effects::BOLD),
            Token::Warning => Style::new()
                .fg_color(Some(AnsiColor::Yellow.into()))
                .effects(Effects::BOLD),
        }
    }
}

// ---------------------------------------------------------------------------
// Painted wrapper
// ---------------------------------------------------------------------------

/// A string annotated with an ANSI style for display.
#[derive(Debug)]
pub struct Painted<'a> {
    text: &'a str,
    style: Style,
}

impl fmt::Display for Painted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.text,
            self.style.render_reset()
        )
    }
}

// ---------------------------------------------------------------------------
// ColorChoice / ColorConfig
// ---------------------------------------------------------------------------

/// Raw clap enum for `--color` flag.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Always emit ANSI colors.
    Always,
    /// Never emit ANSI colors.
    Never,
    /// Detect from terminal and environment.
    #[default]
    Auto,
}

/// Resolved runtime color/unicode configuration.
#[derive(Debug, Clone, Copy)]
pub struct ColorConfig {
    color: bool,
    unicode: bool,
}

impl ColorConfig {
    /// Resolve the final color configuration.
    ///
    /// Priority: `--color` flag > `FORCE_COLOR` > `NO_COLOR` > TTY detection.
    /// Unicode mirrors the color decision (per design spec symbol table).
    #[must_use]
    pub fn resolve(choice: ColorChoice) -> Self {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => Self::detect_from_env(),
        };
        Self {
            color,
            unicode: color,
        }
    }

    fn detect_from_env() -> bool {
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        std::io::stdout().is_terminal()
    }

    /// Whether color output is enabled.
    #[must_use]
    pub fn use_color(self) -> bool {
        self.color
    }

    /// Whether unicode symbols are enabled.
    #[must_use]
    pub fn use_unicode(self) -> bool {
        self.unicode
    }

    /// Wrap `text` with the ANSI style for `token`.
    #[must_use]
    pub fn paint(self, token: Token, text: &str) -> Painted<'_> {
        let style = if self.color {
            token.style()
        } else {
            Style::new()
        };
        Painted { text, style }
    }
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

impl ColorConfig {
    /// Success symbol (checkmark or `[ok]`).
    #[must_use]
    pub fn ok(self) -> Painted<'static> {
        let text = if self.unicode { "✔" } else { "[ok]" };
        self.paint(Token::Success, text)
    }

    /// Error symbol (cross or `[err]`).
    #[must_use]
    pub fn err(self) -> Painted<'static> {
        let text = if self.unicode { "✖" } else { "[err]" };
        self.paint(Token::Error, text)
    }

    /// Warning symbol (triangle or `[warn]`).
    #[must_use]
    pub fn warn(self) -> Painted<'static> {
        let text = if self.unicode { "⚠" } else { "[warn]" };
        self.paint(Token::Warning, text)
    }

    /// Info symbol (info icon or `[info]`).
    #[must_use]
    pub fn info(self) -> Painted<'static> {
        let text = if self.unicode { "ℹ" } else { "[info]" };
        self.paint(Token::Header, text)
    }
}

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

/// Print a next-step hint to stderr.
pub fn hint(color: ColorConfig, msg: &str) {
    let prefix = color.paint(Token::Hint, "hint:");
    let _ = writeln!(io::stderr(), "{prefix} {msg}");
}

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// Output format for commands that support `--format`.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    /// Colored terminal output.
    Terminal,
    /// JSON output.
    Json,
}

/// JSON detail level for commands that support `--detail`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Detail {
    /// Omit redundant or debug-level data from JSON output.
    #[default]
    Compact,
    /// Include all data in JSON output.
    Full,
}

/// Write a value as pretty-printed JSON to stdout.
///
/// # Errors
///
/// Returns an I/O error if serialization or writing fails.
pub fn write_json<T: Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value).map_err(io::Error::other)?;
    writeln!(handle)?;
    Ok(())
}
