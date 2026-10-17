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
    fn explicit_formats_resolve_to_themselves() {
        assert!(matches!(AutoFormat::Json.resolve(), OutputFormat::Json));
        assert!(matches!(
            AutoFormat::Terminal.resolve(),
            OutputFormat::Terminal
        ));
    }
}
