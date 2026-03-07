use clap::Parser;
use std::path::PathBuf;
use supersigil_cli::Cli;

#[test]
fn parse_lint() {
    let cli = Cli::parse_from(["supersigil", "lint"]);
    assert!(matches!(cli.command, supersigil_cli::Command::Lint));
}

#[test]
fn parse_ls() {
    let cli = Cli::parse_from(["supersigil", "ls"]);
    assert!(matches!(cli.command, supersigil_cli::Command::Ls(_)));
}

#[test]
fn parse_list_alias() {
    let cli = Cli::parse_from(["supersigil", "list"]);
    assert!(matches!(cli.command, supersigil_cli::Command::Ls(_)));
}

#[test]
fn parse_ls_with_filters() {
    let cli = Cli::parse_from([
        "supersigil",
        "ls",
        "--type",
        "requirement",
        "--status",
        "draft",
        "--format",
        "json",
    ]);
    if let supersigil_cli::Command::Ls(args) = cli.command {
        assert_eq!(args.doc_type, Some("requirement".into()));
        assert_eq!(args.status, Some("draft".into()));
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Ls");
    }
}

#[test]
fn parse_schema_default_format() {
    let cli = Cli::parse_from(["supersigil", "schema"]);
    if let supersigil_cli::Command::Schema(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::SchemaFormat::Yaml));
    } else {
        panic!("expected Schema");
    }
}

#[test]
fn parse_schema_yaml_format() {
    let cli = Cli::parse_from(["supersigil", "schema", "--format", "yaml"]);
    if let supersigil_cli::Command::Schema(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::SchemaFormat::Yaml));
    } else {
        panic!("expected Schema");
    }
}

#[test]
fn parse_context() {
    let cli = Cli::parse_from(["supersigil", "context", "auth/req/login"]);
    if let supersigil_cli::Command::Context(args) = cli.command {
        assert_eq!(args.id, "auth/req/login");
    } else {
        panic!("expected Context");
    }
}

#[test]
fn parse_plan_no_args() {
    let cli = Cli::parse_from(["supersigil", "plan"]);
    if let supersigil_cli::Command::Plan(args) = cli.command {
        assert!(args.id_or_prefix.is_none());
    } else {
        panic!("expected Plan");
    }
}

#[test]
fn parse_plan_with_id() {
    let cli = Cli::parse_from(["supersigil", "plan", "auth/", "--format", "json"]);
    if let supersigil_cli::Command::Plan(args) = cli.command {
        assert_eq!(args.id_or_prefix, Some("auth/".into()));
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Plan");
    }
}

#[test]
fn parse_import_dry_run() {
    let cli = Cli::parse_from([
        "supersigil",
        "import",
        "--from",
        "kiro",
        "--dry-run",
        "--source-dir",
        "kiro/specs",
        "--output-dir",
        "out/",
        "--prefix",
        "myproject",
    ]);
    if let supersigil_cli::Command::Import(args) = cli.command {
        assert!(matches!(args.from, supersigil_cli::ImportSource::Kiro));
        assert!(args.dry_run);
        assert_eq!(args.source_dir, Some(PathBuf::from("kiro/specs")));
        assert_eq!(args.output_dir, Some(PathBuf::from("out/")));
        assert_eq!(args.prefix, Some("myproject".into()));
        assert!(!args.force);
    } else {
        panic!("expected Import");
    }
}

#[test]
fn parse_import_source_dir_not_provided() {
    let cli = Cli::parse_from(["supersigil", "import", "--from", "kiro"]);
    if let supersigil_cli::Command::Import(args) = cli.command {
        assert!(args.source_dir.is_none());
        assert!(args.output_dir.is_none());
    } else {
        panic!("expected Import");
    }
}

#[test]
fn parse_import_force() {
    let cli = Cli::parse_from(["supersigil", "import", "--from", "kiro", "--force"]);
    if let supersigil_cli::Command::Import(args) = cli.command {
        assert!(args.force);
        assert!(!args.dry_run);
    } else {
        panic!("expected Import");
    }
}

#[test]
fn parse_import_prefix_with_trailing_slash_rejected() {
    Cli::try_parse_from([
        "supersigil",
        "import",
        "--from",
        "kiro",
        "--prefix",
        "myproject/",
    ])
    .unwrap_err();
}

#[test]
fn parse_schema_invalid_format_rejected() {
    Cli::try_parse_from(["supersigil", "schema", "--format", "toml"]).unwrap_err();
}

// -----------------------------------------------------------------------
// New commands: verify, status, affected, graph, init, new
// -----------------------------------------------------------------------

#[test]
fn parse_verify_defaults() {
    let cli = Cli::parse_from(["supersigil", "verify"]);
    if let supersigil_cli::Command::Verify(args) = cli.command {
        assert!(args.project.is_none());
        assert!(args.since.is_none());
        assert!(!args.committed_only);
        assert!(!args.merge_base);
        assert!(matches!(
            args.format,
            supersigil_cli::VerifyFormat::Terminal
        ));
    } else {
        panic!("expected Verify");
    }
}

#[test]
fn parse_verify_with_all_flags() {
    let cli = Cli::parse_from([
        "supersigil",
        "verify",
        "--project",
        "core",
        "--since",
        "main",
        "--committed-only",
        "--merge-base",
        "--format",
        "json",
    ]);
    if let supersigil_cli::Command::Verify(args) = cli.command {
        assert_eq!(args.project, Some("core".into()));
        assert_eq!(args.since, Some("main".into()));
        assert!(args.committed_only);
        assert!(args.merge_base);
        assert!(matches!(args.format, supersigil_cli::VerifyFormat::Json));
    } else {
        panic!("expected Verify");
    }
}

#[test]
fn parse_verify_markdown_format() {
    let cli = Cli::parse_from(["supersigil", "verify", "--format", "markdown"]);
    if let supersigil_cli::Command::Verify(args) = cli.command {
        assert!(matches!(
            args.format,
            supersigil_cli::VerifyFormat::Markdown
        ));
    } else {
        panic!("expected Verify");
    }
}

#[test]
fn parse_status_no_args() {
    let cli = Cli::parse_from(["supersigil", "status"]);
    if let supersigil_cli::Command::Status(args) = cli.command {
        assert!(args.id.is_none());
    } else {
        panic!("expected Status");
    }
}

#[test]
fn parse_status_with_id() {
    let cli = Cli::parse_from(["supersigil", "status", "auth/req"]);
    if let supersigil_cli::Command::Status(args) = cli.command {
        assert_eq!(args.id, Some("auth/req".into()));
    } else {
        panic!("expected Status");
    }
}

#[test]
fn parse_affected() {
    let cli = Cli::parse_from([
        "supersigil",
        "affected",
        "--since",
        "main",
        "--committed-only",
    ]);
    if let supersigil_cli::Command::Affected(args) = cli.command {
        assert_eq!(args.since, "main");
        assert!(args.committed_only);
        assert!(!args.merge_base);
    } else {
        panic!("expected Affected");
    }
}

#[test]
fn parse_graph_default() {
    let cli = Cli::parse_from(["supersigil", "graph"]);
    if let supersigil_cli::Command::Graph(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::GraphFormat::Mermaid));
    } else {
        panic!("expected Graph");
    }
}

#[test]
fn parse_graph_dot() {
    let cli = Cli::parse_from(["supersigil", "graph", "--format", "dot"]);
    if let supersigil_cli::Command::Graph(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::GraphFormat::Dot));
    } else {
        panic!("expected Graph");
    }
}

#[test]
fn parse_init() {
    let cli = Cli::parse_from(["supersigil", "init"]);
    assert!(matches!(cli.command, supersigil_cli::Command::Init));
}

#[test]
fn parse_new() {
    let cli = Cli::parse_from(["supersigil", "new", "requirement", "auth"]);
    if let supersigil_cli::Command::New(args) = cli.command {
        assert_eq!(args.doc_type, "requirement");
        assert_eq!(args.id, "auth");
    } else {
        panic!("expected New");
    }
}

#[test]
fn parse_color_flag() {
    let cli = Cli::parse_from(["supersigil", "--color", "never", "lint"]);
    assert!(matches!(cli.color, supersigil_cli::ColorChoice::Never));
}

#[test]
fn parse_color_always() {
    let cli = Cli::parse_from(["supersigil", "--color", "always", "lint"]);
    assert!(matches!(cli.color, supersigil_cli::ColorChoice::Always));
}

#[test]
fn parse_color_default_auto() {
    let cli = Cli::parse_from(["supersigil", "lint"]);
    assert!(matches!(cli.color, supersigil_cli::ColorChoice::Auto));
}
