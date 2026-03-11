//! Merged runtime view of built-in + user component definitions.

use std::collections::HashMap;

use crate::{AttributeDef, ComponentDef, ComponentDefError};

/// The merged set of component definitions: built-in defaults + user overrides.
/// This is the runtime type passed to the parser for lint-time validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDefs {
    defs: HashMap<String, ComponentDef>,
}

impl ComponentDefs {
    /// Returns the 8 built-in default component definitions.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "declarative definition of all 8 built-in components"
    )]
    pub fn defaults() -> Self {
        /// Insert a component with a single required list attribute `refs`.
        fn refs_only(
            name: &str,
            description: &str,
            example: &str,
            defs: &mut HashMap<String, ComponentDef>,
        ) {
            defs.insert(
                name.into(),
                ComponentDef {
                    attributes: HashMap::from([(
                        "refs".into(),
                        AttributeDef {
                            required: true,
                            list: true,
                        },
                    )]),
                    referenceable: false,
                    verifiable: false,
                    target_component: None,
                    description: Some(description.into()),
                    examples: vec![example.into()],
                },
            );
        }

        let mut defs = HashMap::new();

        // AcceptanceCriteria — no attributes, not referenceable
        defs.insert(
            "AcceptanceCriteria".into(),
            ComponentDef {
                attributes: HashMap::new(),
                referenceable: false,
                verifiable: false,
                target_component: None,
                description: Some("Free-text acceptance criteria block. Use when criteria don't need individual IDs or cross-referencing.".into()),
                examples: vec!["<AcceptanceCriteria>\n- User can log in with valid credentials\n- Invalid credentials show an error message\n</AcceptanceCriteria>".into()],
            },
        );

        // Criterion — id: required; referenceable; verifiable
        defs.insert(
            "Criterion".into(),
            ComponentDef {
                attributes: HashMap::from([(
                    "id".into(),
                    AttributeDef {
                        required: true,
                        list: false,
                    },
                )]),
                referenceable: true,
                verifiable: true,
                target_component: None,
                description: Some("A single verifiable criterion with a unique ID. Use for fine-grained traceability — each Criterion can be referenced by References, VerifiedBy, and Task components.".into()),
                examples: vec!["<Criterion id=\"login-success\">User sees the dashboard after entering valid credentials</Criterion>".into()],
            },
        );

        // References — refs: required, list; informational traceability link
        refs_only(
            "References",
            "Declares that this document references one or more other documents or criteria. Creates informational traceability links with no verification semantics.",
            "<References refs=\"auth/req/login#login-success, auth/req/login#login-failure\" />",
            &mut defs,
        );

        // VerifiedBy — strategy: required; tag: optional; paths: optional, list
        defs.insert(
            "VerifiedBy".into(),
            ComponentDef {
                attributes: HashMap::from([
                    (
                        "strategy".into(),
                        AttributeDef {
                            required: true,
                            list: false,
                        },
                    ),
                    (
                        "tag".into(),
                        AttributeDef {
                            required: false,
                            list: false,
                        },
                    ),
                    (
                        "paths".into(),
                        AttributeDef {
                            required: false,
                            list: true,
                        },
                    ),
                ]),
                referenceable: false,
                verifiable: false,
                target_component: None,
                description: Some("Specifies how a criterion is verified: by tag-based test matching or by file glob patterns.".into()),
                examples: vec![
                    "<VerifiedBy strategy=\"tag\" tag=\"test_login_success\" />".into(),
                    "<VerifiedBy strategy=\"file-glob\" paths=\"path/to/test-file.rs\" />"
                        .into(),
                ],
            },
        );

        // Implements, DependsOn — refs: required, list
        refs_only(
            "Implements",
            "Declares that this document implements one or more criteria from another document.",
            "<Implements refs=\"auth/req/login#login-success\" />",
            &mut defs,
        );
        refs_only(
            "DependsOn",
            "Declares that this document depends on one or more other documents.",
            "<DependsOn refs=\"auth/design/session-mgmt\" />",
            &mut defs,
        );

        // Task — id: required; status: optional; implements: optional, list; depends: optional, list; referenceable
        defs.insert(
            "Task".into(),
            ComponentDef {
                attributes: HashMap::from([
                    (
                        "id".into(),
                        AttributeDef {
                            required: true,
                            list: false,
                        },
                    ),
                    (
                        "status".into(),
                        AttributeDef {
                            required: false,
                            list: false,
                        },
                    ),
                    (
                        "implements".into(),
                        AttributeDef {
                            required: false,
                            list: true,
                        },
                    ),
                    (
                        "depends".into(),
                        AttributeDef {
                            required: false,
                            list: true,
                        },
                    ),
                ]),
                referenceable: true,
                verifiable: false,
                target_component: None,
                description: Some("A trackable work item with status. Tasks can implement criteria and depend on other tasks. Referenceable by ID.".into()),
                examples: vec![
                    "<Task id=\"task-1-1\" status=\"done\" implements=\"auth/req/login#login-success\">\nImplement login endpoint\n</Task>".into(),
                    "<Task id=\"task-1-2\" status=\"in-progress\" depends=\"task-1-1\">\nAdd rate limiting to login\n</Task>".into(),
                ],
            },
        );

        // TrackedFiles — paths: required, list
        defs.insert(
            "TrackedFiles".into(),
            ComponentDef {
                attributes: HashMap::from([(
                    "paths".into(),
                    AttributeDef {
                        required: true,
                        list: true,
                    },
                )]),
                referenceable: false,
                verifiable: false,
                target_component: None,
                description: Some("Declares file paths (globs) that are tracked as part of this document. Used to detect stale references.".into()),
                examples: vec![
                    "<TrackedFiles paths=\"src/auth/**/*.rs, tests/auth/**/*.rs\" />".into(),
                ],
            },
        );

        Self { defs }
    }

    /// Merge user-defined components over defaults. User defs with the same
    /// name override; new names are added; unmentioned built-ins remain.
    ///
    /// # Errors
    ///
    /// Returns [`ComponentDefError`] if any component definition is invalid
    /// (e.g. verifiable but not referenceable, or verifiable without a
    /// required `id` attribute).
    pub fn merge(
        mut defaults: Self,
        user: HashMap<String, ComponentDef>,
    ) -> Result<Self, Vec<ComponentDefError>> {
        defaults.defs.extend(user);
        Self::validate(&defaults.defs)?;
        Ok(defaults)
    }

    /// Validate all component definitions in the map.
    fn validate(defs: &HashMap<String, ComponentDef>) -> Result<(), Vec<ComponentDefError>> {
        let mut errors = Vec::new();

        for (name, def) in defs {
            if def.verifiable {
                if !def.referenceable {
                    errors.push(ComponentDefError::VerifiableNotReferenceable {
                        component: name.clone(),
                    });
                }

                let has_required_id = def.attributes.get("id").is_some_and(|attr| attr.required);

                if !has_required_id {
                    errors.push(ComponentDefError::VerifiableMissingId {
                        component: name.clone(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Returns the number of component definitions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Returns `true` if there are no component definitions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Iterates over all component definitions.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ComponentDef)> {
        self.defs.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Returns all component names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.defs.keys().map(String::as_str)
    }

    /// Check if a component name is known.
    #[must_use]
    pub fn is_known(&self, name: &str) -> bool {
        self.defs.contains_key(name)
    }

    /// Get the definition for a component, if known.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ComponentDef> {
        self.defs.get(name)
    }
}
