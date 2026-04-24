//! Data model, config loader, and built-in component definitions for supersigil.

mod component_defs;
mod config;
mod error;
mod glob_util;
mod graph;
mod locate;
mod refs;
mod rust_scope;
mod rust_validation_inputs;
pub mod scaffold;
mod types;
mod xml;

pub use component_defs::ComponentDefs;
pub use config::{
    AttributeDef, ComponentDef, Config, DocumentTypeDef, DocumentationConfig, DocumentsConfig,
    EcosystemConfig, KNOWN_PLUGINS, KNOWN_RULES, ProjectConfig, RepositoryConfig,
    RepositoryProvider, RustEcosystemConfig, RustProjectScope, RustValidationPolicy, Severity,
    SkillsConfig, TestDiscoveryConfig, TestDiscoveryIgnoreMode, TestResultsConfig, VerifyConfig,
    load_config,
};
pub use error::{
    ComponentDefError, ConfigError, ListSplitError, ParseError, split_list_attribute,
    suggest_similar,
};
pub use glob_util::{expand_glob, expand_globs, expand_globs_checked};
pub use locate::{CONFIG_FILENAME, find_config};
pub use refs::{is_valid_criterion_ref, split_criterion_ref};
pub use rust_scope::{RustProjectResolutionError, match_rust_project_scope, resolve_rust_project};
pub use rust_validation_inputs::{
    RustValidationInputResolutionError, RustValidationInputs, resolve_project_validation_inputs,
    resolve_workspace_validation_inputs,
};
pub use types::{
    CodeBlock, ExtractedComponent, Frontmatter, ParseResult, SourcePosition, SpanKind, SpecDocument,
};
pub use xml::xml_escape;

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

// Graph module re-exports
pub use graph::{
    ACCEPTANCE_CRITERIA, ALTERNATIVE, AlternativeContext, CRITERION, ContextOutput, DECISION,
    DEPENDS_ON, DecisionContext, DocRef, DocumentGraph, EdgeKind, GraphError, IMPLEMENTS,
    LinkedDecision, OutstandingTarget, PlanOutput, PlanQuery, QueryError, RATIONALE, REFERENCES,
    ResolvedRef, SUPERSIGIL_XML_LANG, TASK, TRACKED_FILES, TargetContext, TaskInfo, VERIFIED_BY,
    build_graph, decision_references_target, glob_prefix,
};
