//! Data model, config loader, and built-in component definitions for supersigil.

mod component_defs;
mod config;
mod error;
mod types;

pub use component_defs::ComponentDefs;
pub use config::{
    load_config, AttributeDef, ComponentDef, Config, DocumentTypeDef, DocumentsConfig,
    EcosystemConfig, HooksConfig, ProjectConfig, Severity, TestResultsConfig, VerifyConfig,
    KNOWN_RULES,
};
pub use error::{split_list_attribute, ConfigError, ListSplitError, ParseError};
pub use types::{ExtractedComponent, Frontmatter, ParseResult, SourcePosition, SpecDocument};
