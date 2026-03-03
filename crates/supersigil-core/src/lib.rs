//! Data model, config loader, and built-in component definitions for supersigil.

mod component_defs;
mod config;
mod error;
mod graph;
mod types;

pub use component_defs::ComponentDefs;
pub use config::{
    AttributeDef, ComponentDef, Config, DocumentTypeDef, DocumentsConfig, EcosystemConfig,
    HooksConfig, KNOWN_RULES, ProjectConfig, Severity, TestResultsConfig, VerifyConfig,
    load_config,
};
pub use error::{ConfigError, ListSplitError, ParseError, split_list_attribute};
pub use types::{ExtractedComponent, Frontmatter, ParseResult, SourcePosition, SpecDocument};

// Graph module re-exports
pub use graph::{
    ContextOutput, CriterionContext, DocRef, DocumentGraph, GraphError, IllustrationRef,
    OutstandingCriterion, PlanOutput, PlanQuery, QueryError, ResolvedRef, TaskInfo, build_graph,
};
