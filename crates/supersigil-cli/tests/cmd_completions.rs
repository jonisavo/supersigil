//! Tests for the `completions` command.

use clap::Parser;
use clap_complete::Shell;
use supersigil_cli::{Cli, Command};

#[test]
fn parse_completions_accepts_every_supported_shell() {
    for (name, shell) in [
        ("bash", Shell::Bash),
        ("zsh", Shell::Zsh),
        ("fish", Shell::Fish),
        ("elvish", Shell::Elvish),
        ("powershell", Shell::PowerShell),
    ] {
        let cli = Cli::parse_from(["supersigil", "completions", name]);
        let Command::Completions(args) = cli.command;
        assert_eq!(args.shell, shell, "parsing {name}");
    }
}

#[test]
fn parse_completions_invalid_shell_rejected() {
    Cli::try_parse_from(["supersigil", "completions", "nushell"]).unwrap_err();
}

#[test]
fn completions_generate_all_shells() {
    use clap::CommandFactory;

    for shell in [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::Elvish,
        Shell::PowerShell,
    ] {
        let mut buf = Vec::new();
        let mut cmd = Cli::command();
        clap_complete::generate(shell, &mut cmd, "supersigil", &mut buf);
        let output = String::from_utf8(buf).expect("completions should be valid UTF-8");
        assert!(
            !output.is_empty(),
            "completions for {shell:?} should not be empty"
        );
    }
}
