//! Tests for the `completions` command.

use clap::Parser;
use clap_complete::Shell;
use supersigil_cli::{Cli, Command};

#[test]
fn parse_completions_bash() {
    let cli = Cli::parse_from(["supersigil", "completions", "bash"]);
    let Command::Completions(args) = cli.command;
    assert_eq!(args.shell, Shell::Bash);
}

#[test]
fn parse_completions_zsh() {
    let cli = Cli::parse_from(["supersigil", "completions", "zsh"]);
    let Command::Completions(args) = cli.command;
    assert_eq!(args.shell, Shell::Zsh);
}

#[test]
fn parse_completions_fish() {
    let cli = Cli::parse_from(["supersigil", "completions", "fish"]);
    let Command::Completions(args) = cli.command;
    assert_eq!(args.shell, Shell::Fish);
}

#[test]
fn parse_completions_elvish() {
    let cli = Cli::parse_from(["supersigil", "completions", "elvish"]);
    let Command::Completions(args) = cli.command;
    assert_eq!(args.shell, Shell::Elvish);
}

#[test]
fn parse_completions_powershell() {
    let cli = Cli::parse_from(["supersigil", "completions", "powershell"]);
    let Command::Completions(args) = cli.command;
    assert_eq!(args.shell, Shell::PowerShell);
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
