//! Data model, config loader, and built-in component definitions for supersigil.

mod component_defs;
mod config;
mod error;
mod graph;
mod rust_scope;
mod rust_validation_inputs;
mod types;

pub use component_defs::ComponentDefs;
pub use config::{
    AttributeDef, ComponentDef, Config, DocumentTypeDef, DocumentsConfig, EcosystemConfig,
    ExamplesConfig, HooksConfig, KNOWN_PLUGINS, KNOWN_RULES, ProjectConfig, RunnerConfig,
    RustEcosystemConfig, RustProjectScope, RustValidationPolicy, Severity, SkillsConfig,
    TestResultsConfig, VerifyConfig, load_config,
};
pub use error::{ComponentDefError, ConfigError, ListSplitError, ParseError, split_list_attribute};
pub use rust_scope::{RustProjectResolutionError, match_rust_project_scope, resolve_rust_project};
pub use rust_validation_inputs::{
    RustValidationInputResolutionError, RustValidationInputs, resolve_rust_validation_inputs,
};
pub use types::{
    CodeBlock, ExtractedComponent, Frontmatter, ParseResult, SourcePosition, SpecDocument,
};

// Graph module re-exports
pub use graph::{
    CRITERION, ContextOutput, DocRef, DocumentGraph, EXAMPLE, EXPECTED, GraphError,
    OutstandingTarget, PlanOutput, PlanQuery, QueryError, ResolvedRef, TargetContext, TaskInfo,
    VERIFIED_BY, build_graph, glob_prefix,
};
