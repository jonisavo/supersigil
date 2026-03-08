use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use supersigil_core::{AttributeDef, ComponentDef, ComponentDefs, DocumentTypeDef, load_config};

use crate::commands::{BUILTIN_DOC_TYPES, SchemaArgs, SchemaFormat};
use crate::error::CliError;
use crate::format::{ColorConfig, write_json, write_yaml};

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if helpers must accept references"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Serialize)]
struct SchemaOutput {
    components: BTreeMap<String, SchemaComponentDef>,
    document_types: BTreeMap<String, SchemaDocumentTypeDef>,
}

#[derive(Debug, Serialize)]
struct SchemaComponentDef {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    attributes: BTreeMap<String, SchemaAttributeDef>,
    #[serde(skip_serializing_if = "is_false")]
    referenceable: bool,
    #[serde(skip_serializing_if = "is_false")]
    verifiable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_component: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    examples: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SchemaAttributeDef {
    required: bool,
    #[serde(skip_serializing_if = "is_false")]
    list: bool,
}

#[derive(Debug, Serialize)]
struct SchemaDocumentTypeDef {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    status: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    required_components: Vec<String>,
}

fn default_document_types() -> BTreeMap<String, SchemaDocumentTypeDef> {
    fn doc_type(description: &str, status: &[&str]) -> SchemaDocumentTypeDef {
        SchemaDocumentTypeDef {
            description: Some(description.into()),
            status: status.iter().map(ToString::to_string).collect(),
            required_components: Vec::new(),
        }
    }

    debug_assert_eq!(
        BUILTIN_DOC_TYPES,
        &["requirements", "design", "tasks"],
        "update descriptions below when BUILTIN_DOC_TYPES changes"
    );

    [
        (
            "requirements".to_string(),
            doc_type(
                "Captures what the system must do. Contains acceptance criteria that can be traced to designs, tasks, and tests.",
                &["draft", "review", "approved", "implemented"],
            ),
        ),
        (
            "design".to_string(),
            doc_type(
                "Describes how a requirement will be implemented. Links back to requirements via Validates.",
                &["draft", "review", "approved"],
            ),
        ),
        (
            "tasks".to_string(),
            doc_type(
                "A plan of work items (Task components) that implement criteria from requirements or designs.",
                &["draft", "ready", "in-progress", "done"],
            ),
        ),
    ]
    .into_iter()
    .collect()
}

impl From<&AttributeDef> for SchemaAttributeDef {
    fn from(value: &AttributeDef) -> Self {
        Self {
            required: value.required,
            list: value.list,
        }
    }
}

impl From<&ComponentDef> for SchemaComponentDef {
    fn from(value: &ComponentDef) -> Self {
        Self {
            description: value.description.clone(),
            attributes: value
                .attributes
                .iter()
                .map(|(name, attr)| (name.clone(), SchemaAttributeDef::from(attr)))
                .collect(),
            referenceable: value.referenceable,
            verifiable: value.verifiable,
            target_component: value.target_component.clone(),
            examples: value.examples.clone(),
        }
    }
}

impl From<&DocumentTypeDef> for SchemaDocumentTypeDef {
    fn from(value: &DocumentTypeDef) -> Self {
        Self {
            description: value.description.clone(),
            status: value.status.clone(),
            required_components: value.required_components.clone(),
        }
    }
}

/// Run the `schema` command: output merged component definitions and
/// configured document types.
///
/// # Errors
///
/// Returns `CliError` if config loading or output serialization fails.
pub fn run(args: &SchemaArgs, config_path: &Path, _color: ColorConfig) -> Result<(), CliError> {
    let config = load_config(config_path).map_err(CliError::Config)?;
    let merged_components = ComponentDefs::merge(ComponentDefs::defaults(), config.components)
        .map_err(CliError::ComponentDef)?;
    let mut document_types = default_document_types();
    document_types.extend(
        config
            .documents
            .types
            .iter()
            .map(|(name, def)| (name.clone(), SchemaDocumentTypeDef::from(def))),
    );

    let output = SchemaOutput {
        components: merged_components
            .iter()
            .map(|(name, def)| (name.to_owned(), SchemaComponentDef::from(def)))
            .collect(),
        document_types,
    };

    match args.format {
        SchemaFormat::Json => write_json(&output)?,
        SchemaFormat::Yaml => write_yaml(&output)?,
    }

    Ok(())
}
