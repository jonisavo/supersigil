use clap::Parser;
use std::path::PathBuf;
use supersigil_cli::Cli;
use supersigil_rust::verifies;

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
        "requirements",
        "--status",
        "draft",
        "--format",
        "json",
    ]);
    if let supersigil_cli::Command::Ls(args) = cli.command {
        assert_eq!(args.doc_type, Some("requirements".into()));
        assert_eq!(args.status, Some("draft".into()));
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Ls");
    }
}

#[test]
fn parse_ls_with_short_project() {
    let cli = Cli::parse_from(["supersigil", "ls", "-p", "frontend"]);
    if let supersigil_cli::Command::Ls(args) = cli.command {
        assert_eq!(args.project.as_deref(), Some("frontend"));
    } else {
        panic!("expected Ls");
    }
}

#[verifies("inventory-queries/req#req-2-2")]
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
        assert!(!args.verbose);
    } else {
        panic!("expected Plan");
    }
}

#[test]
fn parse_plan_verbose() {
    let cli = Cli::parse_from(["supersigil", "plan", "--verbose"]);
    if let supersigil_cli::Command::Plan(args) = cli.command {
        assert!(args.verbose);
        assert!(args.id_or_prefix.is_none());
    } else {
        panic!("expected Plan");
    }
}

#[test]
fn parse_plan_verbose_with_id() {
    let cli = Cli::parse_from(["supersigil", "plan", "auth/", "--verbose"]);
    if let supersigil_cli::Command::Plan(args) = cli.command {
        assert_eq!(args.id_or_prefix, Some("auth/".into()));
        assert!(args.verbose);
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
fn parse_verify_with_short_project() {
    let cli = Cli::parse_from(["supersigil", "verify", "-p", "core"]);
    if let supersigil_cli::Command::Verify(args) = cli.command {
        assert_eq!(args.project.as_deref(), Some("core"));
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

#[verifies("inventory-queries/req#req-3-2")]
#[test]
fn parse_graph_default() {
    let cli = Cli::parse_from(["supersigil", "graph"]);
    if let supersigil_cli::Command::Graph(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::GraphFormat::Mermaid));
    } else {
        panic!("expected Graph");
    }
}

#[verifies("inventory-queries/req#req-3-2")]
#[test]
fn parse_graph_dot() {
    let cli = Cli::parse_from(["supersigil", "graph", "--format", "dot"]);
    if let supersigil_cli::Command::Graph(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::GraphFormat::Dot));
    } else {
        panic!("expected Graph");
    }
}

#[verifies("graph-explorer/req#req-1-1")]
#[test]
fn parse_graph_json() {
    let cli = Cli::parse_from(["supersigil", "graph", "--format", "json"]);
    if let supersigil_cli::Command::Graph(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::GraphFormat::Json));
    } else {
        panic!("expected Graph");
    }
}

#[test]
fn parse_init() {
    let cli = Cli::parse_from(["supersigil", "init"]);
    assert!(matches!(cli.command, supersigil_cli::Command::Init(_)));
}

#[test]
fn parse_init_yes() {
    let cli = Cli::parse_from(["supersigil", "init", "-y"]);
    if let supersigil_cli::Command::Init(args) = cli.command {
        assert!(args.yes);
        assert!(!args.skills);
        assert!(!args.no_skills);
        assert!(args.skills_path.is_none());
    } else {
        panic!("expected Init");
    }
}

#[test]
fn parse_init_skills_flags() {
    let cli = Cli::parse_from(["supersigil", "init", "--skills"]);
    if let supersigil_cli::Command::Init(args) = cli.command {
        assert!(args.skills);
        assert!(!args.no_skills);
    } else {
        panic!("expected Init");
    }
}

#[test]
fn parse_init_no_skills_flag() {
    let cli = Cli::parse_from(["supersigil", "init", "--no-skills"]);
    if let supersigil_cli::Command::Init(args) = cli.command {
        assert!(!args.skills);
        assert!(args.no_skills);
    } else {
        panic!("expected Init");
    }
}

#[test]
fn parse_init_skills_path() {
    let cli = Cli::parse_from(["supersigil", "init", "--skills-path", "custom/dir"]);
    if let supersigil_cli::Command::Init(args) = cli.command {
        assert_eq!(args.skills_path, Some(PathBuf::from("custom/dir")));
    } else {
        panic!("expected Init");
    }
}

#[verifies("skills-install/req#req-3-6")]
#[test]
fn parse_init_skills_and_no_skills_conflict() {
    Cli::try_parse_from(["supersigil", "init", "--skills", "--no-skills"]).unwrap_err();
}

#[test]
fn parse_skills_install() {
    let cli = Cli::parse_from(["supersigil", "skills", "install"]);
    if let supersigil_cli::Command::Skills(args) = cli.command {
        assert!(matches!(
            args.command,
            supersigil_cli::SkillsCommand::Install(_)
        ));
    } else {
        panic!("expected Skills");
    }
}

#[test]
fn parse_skills_install_with_path() {
    let cli = Cli::parse_from(["supersigil", "skills", "install", "--path", "my/skills"]);
    if let supersigil_cli::Command::Skills(args) = cli.command {
        let supersigil_cli::SkillsCommand::Install(install_args) = args.command;
        assert_eq!(install_args.path, Some(PathBuf::from("my/skills")));
    } else {
        panic!("expected Skills");
    }
}

#[test]
fn parse_new() {
    let cli = Cli::parse_from(["supersigil", "new", "requirements", "auth"]);
    if let supersigil_cli::Command::New(args) = cli.command {
        assert_eq!(args.doc_type, "requirements");
        assert_eq!(args.id, "auth");
        assert!(args.project.is_none());
    } else {
        panic!("expected New");
    }
}

#[test]
fn parse_new_with_project() {
    let cli = Cli::parse_from([
        "supersigil",
        "new",
        "--project",
        "cli",
        "requirements",
        "auth",
    ]);
    if let supersigil_cli::Command::New(args) = cli.command {
        assert_eq!(args.doc_type, "requirements");
        assert_eq!(args.id, "auth");
        assert_eq!(args.project.as_deref(), Some("cli"));
    } else {
        panic!("expected New");
    }
}

#[test]
fn parse_new_with_short_project() {
    let cli = Cli::parse_from(["supersigil", "new", "-p", "cli", "design", "auth"]);
    if let supersigil_cli::Command::New(args) = cli.command {
        assert_eq!(args.doc_type, "design");
        assert_eq!(args.id, "auth");
        assert_eq!(args.project.as_deref(), Some("cli"));
    } else {
        panic!("expected New");
    }
}

// -----------------------------------------------------------------------
// refs command
// -----------------------------------------------------------------------

#[verifies("ref-discovery/req#req-2-3")]
#[test]
fn parse_refs_defaults() {
    let cli = Cli::parse_from(["supersigil", "refs"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert!(!args.all);
        assert!(matches!(
            args.format,
            supersigil_cli::OutputFormat::Terminal
        ));
    } else {
        panic!("expected Refs");
    }
}

#[verifies("ref-discovery/req#req-3-3")]
#[test]
fn parse_refs_all_flag() {
    let cli = Cli::parse_from(["supersigil", "refs", "--all"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert!(args.all);
    } else {
        panic!("expected Refs");
    }
}

#[verifies("ref-discovery/req#req-2-3")]
#[test]
fn parse_refs_json_format() {
    let cli = Cli::parse_from(["supersigil", "refs", "--format", "json"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Refs");
    }
}

#[test]
fn parse_refs_all_with_json() {
    let cli = Cli::parse_from(["supersigil", "refs", "--all", "--format", "json"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert!(args.all);
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Refs");
    }
}

#[verifies("ref-discovery/req#req-2-2")]
#[test]
fn parse_refs_with_prefix() {
    let cli = Cli::parse_from(["supersigil", "refs", "auth/"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert_eq!(args.prefix, Some("auth/".into()));
        assert!(!args.all);
        assert!(matches!(
            args.format,
            supersigil_cli::OutputFormat::Terminal
        ));
    } else {
        panic!("expected Refs");
    }
}

#[test]
fn parse_refs_no_prefix() {
    let cli = Cli::parse_from(["supersigil", "refs"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert!(args.prefix.is_none());
    } else {
        panic!("expected Refs");
    }
}

#[test]
fn parse_refs_prefix_with_flags() {
    let cli = Cli::parse_from(["supersigil", "refs", "auth/", "--all", "--format", "json"]);
    if let supersigil_cli::Command::Refs(args) = cli.command {
        assert_eq!(args.prefix, Some("auth/".into()));
        assert!(args.all);
        assert!(matches!(args.format, supersigil_cli::OutputFormat::Json));
    } else {
        panic!("expected Refs");
    }
}

// -----------------------------------------------------------------------
// explore command
// -----------------------------------------------------------------------

#[verifies("graph-explorer/req#req-2-1")]
#[test]
fn parse_explore_default() {
    let cli = Cli::parse_from(["supersigil", "explore"]);
    if let supersigil_cli::Command::Explore(args) = cli.command {
        assert!(args.output.is_none());
    } else {
        panic!("expected Explore");
    }
}

#[verifies("graph-explorer/req#req-2-2")]
#[test]
fn parse_explore_with_output() {
    let cli = Cli::parse_from(["supersigil", "explore", "--output", "graph.html"]);
    if let supersigil_cli::Command::Explore(args) = cli.command {
        assert_eq!(args.output, Some(PathBuf::from("graph.html")));
    } else {
        panic!("expected Explore");
    }
}

// -----------------------------------------------------------------------
// color flag
// -----------------------------------------------------------------------

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
