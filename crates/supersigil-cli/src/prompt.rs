//! Interactive prompt helpers for CLI commands.

use std::io::{self, BufRead, Write};

/// Prompt the user with a yes/no question. Returns `default` when input is
/// empty or when `is_tty` is false.
///
/// Prompts are written to `output` (typically stderr). The hint shows `[Y/n]`
/// when `default_yes` is true, `[y/N]` otherwise.
///
/// # Errors
///
/// Returns `io::Error` on read/write failure.
pub fn confirm(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    message: &str,
    default_yes: bool,
    is_tty: bool,
) -> io::Result<bool> {
    if !is_tty {
        return Ok(default_yes);
    }

    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    write!(output, "{message} {hint} ")?;
    output.flush()?;

    let mut line = String::new();
    input.read_line(&mut line)?;
    let trimmed = line.trim().to_lowercase();

    if trimmed.is_empty() {
        return Ok(default_yes);
    }

    Ok(trimmed == "y" || trimmed == "yes")
}

/// Prompt the user for a string value with a default. Returns `default` when
/// input is empty or when `is_tty` is false.
///
/// Prompts are written to `output` (typically stderr). The default value is
/// shown in brackets.
///
/// # Errors
///
/// Returns `io::Error` on read/write failure.
pub fn input_with_default(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    message: &str,
    default: &str,
    is_tty: bool,
) -> io::Result<String> {
    if !is_tty {
        return Ok(default.to_owned());
    }

    write!(output, "{message} [{default}]: ")?;
    output.flush()?;

    let mut line = String::new();
    input.read_line(&mut line)?;
    let trimmed = line.trim();

    if trimmed.is_empty() {
        Ok(default.to_owned())
    } else {
        Ok(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use supersigil_rust::verifies;

    #[test]
    fn confirm_empty_input_returns_default_yes() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", true, true).unwrap();
        assert!(result);
    }

    #[test]
    fn confirm_empty_input_returns_default_no() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", false, true).unwrap();
        assert!(!result);
    }

    #[test]
    fn confirm_y_returns_true() {
        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", false, true).unwrap();
        assert!(result);
    }

    #[test]
    fn confirm_yes_returns_true() {
        let mut input = Cursor::new(b"yes\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", false, true).unwrap();
        assert!(result);
    }

    #[test]
    fn confirm_capital_y_returns_true() {
        let mut input = Cursor::new(b"Y\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", false, true).unwrap();
        assert!(result);
    }

    #[test]
    fn confirm_n_returns_false() {
        let mut input = Cursor::new(b"n\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", true, true).unwrap();
        assert!(!result);
    }

    #[test]
    fn confirm_no_returns_false() {
        let mut input = Cursor::new(b"no\n");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", true, true).unwrap();
        assert!(!result);
    }

    #[test]
    fn confirm_not_tty_returns_default_without_prompt() {
        let mut input = Cursor::new(b"");
        let mut output = Vec::new();
        let result = confirm(&mut input, &mut output, "Install?", true, false).unwrap();
        assert!(result);
        assert!(output.is_empty());
    }

    #[verifies("skills-install/req#req-2-1")]
    #[test]
    fn confirm_writes_prompt_with_yes_default() {
        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();
        confirm(&mut input, &mut output, "Install?", true, true).unwrap();
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("Install?"), "prompt: {prompt}");
        assert!(prompt.contains("[Y/n]"), "prompt: {prompt}");
    }

    #[test]
    fn confirm_writes_prompt_with_no_default() {
        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();
        confirm(&mut input, &mut output, "Install?", false, true).unwrap();
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("[y/N]"), "prompt: {prompt}");
    }

    #[test]
    fn input_with_default_empty_returns_default() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result =
            input_with_default(&mut input, &mut output, "Directory", ".agents/skills", true)
                .unwrap();
        assert_eq!(result, ".agents/skills");
    }

    #[test]
    fn input_with_default_user_value() {
        let mut input = Cursor::new(b"custom/path\n");
        let mut output = Vec::new();
        let result =
            input_with_default(&mut input, &mut output, "Directory", ".agents/skills", true)
                .unwrap();
        assert_eq!(result, "custom/path");
    }

    #[test]
    fn input_with_default_not_tty_returns_default_without_prompt() {
        let mut input = Cursor::new(b"");
        let mut output = Vec::new();
        let result = input_with_default(
            &mut input,
            &mut output,
            "Directory",
            ".agents/skills",
            false,
        )
        .unwrap();
        assert_eq!(result, ".agents/skills");
        assert!(output.is_empty());
    }

    #[verifies("skills-install/req#req-2-2")]
    #[test]
    fn input_with_default_shows_default_in_brackets() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        input_with_default(&mut input, &mut output, "Directory", ".agents/skills", true).unwrap();
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("[.agents/skills]"), "prompt: {prompt}");
    }
}
