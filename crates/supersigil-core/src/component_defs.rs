//! Merged runtime view of built-in + user component definitions.

use std::collections::HashMap;

use crate::{AttributeDef, ComponentDef};

/// The merged set of component definitions: built-in defaults + user overrides.
/// This is the runtime type passed to the parser for lint-time validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDefs {
    defs: HashMap<String, ComponentDef>,
}

impl ComponentDefs {
    /// Returns the 9 built-in default component definitions.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "declarative definition of all 9 built-in components"
    )]
    pub fn defaults() -> Self {
        /// Insert a component with a single required list attribute `refs`.
        fn refs_only(name: &str, defs: &mut HashMap<String, ComponentDef>) {
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
                    target_component: None,
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
                target_component: None,
            },
        );

        // Criterion — id: required; referenceable
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
                target_component: None,
            },
        );

        // Validates — refs: required, list; targets Criterion
        defs.insert(
            "Validates".into(),
            ComponentDef {
                attributes: HashMap::from([(
                    "refs".into(),
                    AttributeDef {
                        required: true,
                        list: true,
                    },
                )]),
                referenceable: false,
                target_component: Some("Criterion".into()),
            },
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
                target_component: None,
            },
        );

        // Implements, Illustrates, DependsOn — refs: required, list
        refs_only("Implements", &mut defs);
        refs_only("Illustrates", &mut defs);
        refs_only("DependsOn", &mut defs);

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
                target_component: None,
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
                target_component: None,
            },
        );

        Self { defs }
    }

    /// Merge user-defined components over defaults. User defs with the same
    /// name override; new names are added; unmentioned built-ins remain.
    #[must_use]
    pub fn merge(mut defaults: Self, user: HashMap<String, ComponentDef>) -> Self {
        defaults.defs.extend(user);
        defaults
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
