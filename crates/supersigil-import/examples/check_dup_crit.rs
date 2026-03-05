use supersigil_import::{ImportConfig, plan_kiro_import};

fn main() {
    let base = std::env::temp_dir().join(format!("checkdupcrit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let specs_dir = base.join("specs");
    let feat = specs_dir.join("dup");
    std::fs::create_dir_all(&feat).unwrap();

    std::fs::write(
        feat.join("requirements.md"),
        "# Requirements Document\n\n### Requirement 1: R\n\n#### Acceptance Criteria\n\n1. first\n1. second\n",
    )
    .unwrap();

    std::fs::write(
        feat.join("design.md"),
        "# Design Document: D\n\n## Correctness\n\n**Validates: Requirements 1.1**\n",
    )
    .unwrap();

    let cfg = ImportConfig {
        kiro_specs_dir: specs_dir,
        output_dir: base.join("out"),
        id_prefix: None,
        force: false,
    };

    let plan = plan_kiro_import(&cfg).unwrap();
    for doc in &plan.documents {
        println!("=== {} ===\n{}", doc.document_id, doc.content);
    }

    let _ = std::fs::remove_dir_all(&base);
}
