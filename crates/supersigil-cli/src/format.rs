use std::fmt::{self, Write as _};
use std::io::{self, IsTerminal, Write};

use anstyle::{Effects, Style};
use serde::Serialize;

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

/// Resolved runtime color configuration.
#[derive(Debug, Clone, Copy)]
pub struct ColorConfig {
    color: bool,
}

impl ColorConfig {
    /// Resolve the final color configuration.
    ///
    /// Priority: `--color` flag > `FORCE_COLOR` > `NO_COLOR` > TTY detection.
    #[must_use]
    pub fn resolve(choice: ColorChoice) -> Self {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => Self::detect_from_env(),
        };
        Self { color }
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
}

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

/// Print a next-step hint to stderr, its `hint:` prefix dimmed when color
/// is on.
pub fn hint(color: ColorConfig, msg: &str) {
    let style = if color.color {
        Style::new().effects(Effects::DIMMED)
    } else {
        Style::new()
    };
    let _ = writeln!(io::stderr(), "{style}hint:{style:#} {msg}");
}

// ---------------------------------------------------------------------------
// Escaping
// ---------------------------------------------------------------------------

/// Makes control characters in `text` visible for terminal output.
///
/// Every character in `U+0000..=U+001F` except tab, plus `U+007F` and the C1
/// range `U+0080..=U+009F`, becomes `\x` and two lowercase hex digits, for
/// example `\x1b`. Everything else passes through. See [`Untrusted`] for
/// which values need it.
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

/// Output format for `review` and `why`, which default to `auto`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum AutoFormat {
    /// Terminal text when stdout is a terminal, JSON otherwise.
    Auto,
    /// JSON output.
    Json,
    /// Plain terminal text.
    Terminal,
}

impl AutoFormat {
    /// The concrete format: `auto` is terminal text when stdout is a
    /// terminal and JSON otherwise.
    #[must_use]
    pub fn resolve(self) -> OutputFormat {
        match self {
            Self::Auto if io::stdout().is_terminal() => OutputFormat::Terminal,
            Self::Auto | Self::Json => OutputFormat::Json,
            Self::Terminal => OutputFormat::Terminal,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_control_makes_control_characters_visible() {
        assert_eq!(escape_control("a\u{1b}[2Kb"), r"a\x1b[2Kb");
        assert_eq!(escape_control("bell\u{7}"), r"bell\x07");
        assert_eq!(escape_control("line\nbreak\r"), r"line\x0abreak\x0d");
        assert_eq!(escape_control("nul\u{0}del\u{7f}"), r"nul\x00del\x7f");
        assert_eq!(escape_control("c1\u{9b}31m"), r"c1\x9b31m");
        assert_eq!(escape_control("tab\tstays"), "tab\tstays");
        assert_eq!(escape_control("ünïcode ✔ \u{a0}"), "ünïcode ✔ \u{a0}");
    }

    #[test]
    fn explicit_formats_resolve_to_themselves() {
        assert!(matches!(AutoFormat::Json.resolve(), OutputFormat::Json));
        assert!(matches!(
            AutoFormat::Terminal.resolve(),
            OutputFormat::Terminal
        ));
    }
}
