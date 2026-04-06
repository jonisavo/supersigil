// Property-based tests for supersigil-core
// Task 2.3: Frontmatter YAML round-trip (Property 1)
// Validates: Requirements 22.1, 4.1

use proptest::prelude::*;
use supersigil_core::Frontmatter;

/// Generator for valid Frontmatter values.
/// `id` is non-empty alphanumeric + `/` + `-`.
/// `doc_type` and `status` are optional strings.
fn arb_frontmatter() -> impl Strategy<Value = Frontmatter> {
    let id_strategy = "[a-zA-Z0-9/\\-]{1,30}";
    let opt_string = prop::option::of("[a-zA-Z0-9_\\-]{1,20}");

    (id_strategy, opt_string.clone(), opt_string).prop_map(|(id, doc_type, status)| Frontmatter {
        id,
        doc_type,
        status,
    })
}

// Feature: parser-and-config, Property 1: Frontmatter YAML round-trip
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn frontmatter_yaml_round_trip(fm in arb_frontmatter()) {
        let yaml = yaml_serde::to_string(&fm).unwrap();
        let deserialized: Frontmatter = yaml_serde::from_str(&yaml).unwrap();
        prop_assert_eq!(&fm, &deserialized);
    }

    /// Verify the `type` ↔ `doc_type` rename survives round-trip:
    /// when `doc_type` is Some, the serialized YAML must contain `type:` not `doc_type:`.
    #[test]
    fn frontmatter_type_field_rename(fm in arb_frontmatter()) {
        let yaml = yaml_serde::to_string(&fm).unwrap();
        prop_assert!(!yaml.contains("doc_type"), "YAML should use 'type' not 'doc_type'");
        if fm.doc_type.is_some() {
            prop_assert!(yaml.contains("type:"), "YAML should contain 'type:' when doc_type is Some");
        }
    }
}

// ---------------------------------------------------------------------------
// Task 3.3: Config TOML round-trip (Property 2)
// Validates: Requirements 23.1, 11.1
// ---------------------------------------------------------------------------

use supersigil_core::{
    AttributeDef, ComponentDef, Config, DocumentTypeDef, DocumentationConfig, DocumentsConfig,
    EcosystemConfig, ProjectConfig, Severity, SkillsConfig, TestResultsConfig, VerifyConfig,
};

/// Generator for a non-empty identifier string (safe for TOML keys).
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}"
}

/// Generator for a glob-like path string.
fn arb_glob() -> impl Strategy<Value = String> {
    "[a-z]{1,8}/\\*\\*/\\*\\.[a-z]{1,4}".prop_map(|s| s.replace('\\', ""))
}

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Off),
        Just(Severity::Warning),
        Just(Severity::Error),
    ]
}

fn arb_attribute_def() -> impl Strategy<Value = AttributeDef> {
    (any::<bool>(), any::<bool>()).prop_map(|(required, list)| AttributeDef { required, list })
}

fn arb_component_def() -> impl Strategy<Value = ComponentDef> {
    (
        prop::collection::hash_map(arb_ident(), arb_attribute_def(), 0..3),
        any::<bool>(),
        prop::option::of(arb_ident()),
        prop::option::of(arb_ident()),
        prop::collection::vec(arb_ident(), 0..3),
    )
        .prop_map(
            |(attributes, referenceable, target_component, description, examples)| ComponentDef {
                attributes,
                referenceable,
                verifiable: false,
                target_component,
                description,
                examples,
            },
        )
}

fn arb_document_type_def() -> impl Strategy<Value = DocumentTypeDef> {
    (
        prop::collection::vec(arb_ident(), 0..4),
        prop::collection::vec("[A-Z][a-z]{2,8}", 0..3),
    )
        .prop_map(|(status, required_components)| DocumentTypeDef {
            status,
            required_components,
            description: None,
        })
}

fn arb_project_config() -> impl Strategy<Value = ProjectConfig> {
    (
        prop::collection::vec(arb_glob(), 1..3),
        prop::collection::vec(arb_glob(), 0..2),
        any::<bool>(),
    )
        .prop_map(|(paths, tests, isolated)| ProjectConfig {
            paths,
            tests,
            isolated,
        })
}

fn arb_verify_config() -> impl Strategy<Value = VerifyConfig> {
    (
        prop::option::of(arb_severity()),
        prop::collection::hash_map(arb_ident(), arb_severity(), 0..3),
    )
        .prop_map(|(strictness, rules)| VerifyConfig { strictness, rules })
}

fn arb_ecosystem_config() -> impl Strategy<Value = EcosystemConfig> {
    prop::collection::vec(arb_ident(), 0..4).prop_map(|plugins| EcosystemConfig {
        plugins,
        rust: None,
        js: None,
    })
}

fn arb_test_results_config() -> impl Strategy<Value = TestResultsConfig> {
    (
        prop::collection::vec(arb_ident(), 0..3),
        prop::collection::vec(arb_glob(), 0..2),
    )
        .prop_map(|(formats, paths)| TestResultsConfig { formats, paths })
}

/// Generator for valid Config values. Ensures mutual exclusivity:
/// either single-project (paths + optional tests) or multi-project (projects only).
fn arb_config() -> impl Strategy<Value = Config> {
    let single_project = (
        prop::collection::vec(arb_glob(), 1..3),
        prop::option::of(prop::collection::vec(arb_glob(), 1..3)),
    )
        .prop_map(|(paths, tests)| (Some(paths), tests, None));

    let multi_project = prop::collection::hash_map(arb_ident(), arb_project_config(), 1..3)
        .prop_map(|projects| (None, None, Some(projects)));

    (
        prop_oneof![single_project, multi_project],
        prop::option::of("[a-z\\-]{1,10}"),
        prop::collection::hash_map(arb_ident(), arb_document_type_def(), 0..3),
        prop::collection::hash_map("[A-Z][a-z]{2,8}", arb_component_def(), 0..3),
        arb_verify_config(),
        arb_ecosystem_config(),
        arb_test_results_config(),
    )
        .prop_map(
            |(
                (paths, tests, projects),
                id_pattern,
                types,
                components,
                verify,
                ecosystem,
                test_results,
            )| {
                Config {
                    paths,
                    tests,
                    projects,
                    id_pattern,
                    documents: DocumentsConfig { types },
                    components,
                    verify,
                    ecosystem,
                    test_results,
                    skills: SkillsConfig::default(),
                    documentation: DocumentationConfig::default(),
                }
            },
        )
}

// Feature: parser-and-config, Property 2: Config TOML round-trip
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn config_toml_round_trip(config in arb_config()) {
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(&config, &deserialized);
    }
}

// ---------------------------------------------------------------------------
// Task 4.3: Component definition merge (Property 13)
// Validates: Requirements 14.5
// ---------------------------------------------------------------------------

use supersigil_core::ComponentDefs;

/// Generator for a `PascalCase` component name (starts with uppercase).
fn arb_pascal_name() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{2,10}"
}

// Feature: parser-and-config, Property 13: Component definition merge is additive over built-in defaults
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn component_def_merge_additive(
        user_defs in prop::collection::hash_map(arb_pascal_name(), arb_component_def(), 0..6)
    ) {
        let defaults = ComponentDefs::defaults();
        let default_names: std::collections::HashSet<String> =
            defaults.names().map(str::to_owned).collect();

        let merged = ComponentDefs::merge(defaults, user_defs.clone())
            .expect("merge with verifiable=false should not fail");

        // (a) User-defined components with the same name as a built-in override it
        for (name, user_def) in &user_defs {
            let merged_def = merged.get(name).unwrap();
            prop_assert_eq!(merged_def, user_def,
                "user override for '{}' should replace built-in", name);
        }

        // (b) User-defined components with new names are added
        for name in user_defs.keys() {
            prop_assert!(merged.is_known(name),
                "user component '{}' should be in merged set", name);
        }

        // (c) Built-in components not mentioned by the user remain unchanged
        let original_defaults = ComponentDefs::defaults();
        for name in &default_names {
            if !user_defs.contains_key(name) {
                let merged_def = merged.get(name).unwrap();
                let default_def = original_defaults.get(name).unwrap();
                prop_assert_eq!(merged_def, default_def,
                    "unmentioned built-in '{}' should be unchanged", name);
            }
        }

        // Total count = built-ins + new user names (not already built-in)
        let new_user_names = user_defs.keys()
            .filter(|n| !default_names.contains(n.as_str()))
            .count();
        prop_assert_eq!(merged.len(), 11 + new_user_names,
            "merged count should be 11 defaults + new user components");
    }
}

// ===========================================================================
// Task 5.3: Unknown TOML keys rejected (Property 11)
// Validates: Requirements 11.3
// ===========================================================================

use supersigil_core::{ConfigError, load_config};

mod common;
use common::write_temp_toml;

/// Generator for an unknown key name that won't collide with real Config fields.
fn arb_unknown_key() -> impl Strategy<Value = String> {
    "zzz_unknown_[a-z]{3,8}"
}

/// The nesting levels where we can inject an unknown key.
#[derive(Debug, Clone)]
enum NestingLevel {
    TopLevel,
    Documents,
    Verify,
    Ecosystem,
    TestResults,
}

fn arb_nesting_level() -> impl Strategy<Value = NestingLevel> {
    prop_oneof![
        Just(NestingLevel::TopLevel),
        Just(NestingLevel::Documents),
        Just(NestingLevel::Verify),
        Just(NestingLevel::Ecosystem),
        Just(NestingLevel::TestResults),
    ]
}

/// Build a valid Config TOML with an unknown key injected at the given nesting level.
fn inject_unknown_key(level: &NestingLevel, key: &str) -> String {
    match level {
        NestingLevel::TopLevel => format!("paths = [\"specs/**/*.md\"]\n{key} = \"bad\"\n"),
        NestingLevel::Documents => {
            format!("paths = [\"specs/**/*.md\"]\n\n[documents]\n{key} = \"bad\"\n")
        }
        NestingLevel::Verify => {
            format!("paths = [\"specs/**/*.md\"]\n\n[verify]\n{key} = \"bad\"\n")
        }
        NestingLevel::Ecosystem => {
            format!("paths = [\"specs/**/*.md\"]\n\n[ecosystem]\n{key} = \"bad\"\n")
        }
        NestingLevel::TestResults => {
            format!("paths = [\"specs/**/*.md\"]\n\n[test_results]\n{key} = \"bad\"\n")
        }
    }
}

// Feature: parser-and-config, Property 11: Unknown TOML keys are rejected at all nesting levels
// **Validates: Requirements 11.3**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unknown_toml_keys_rejected(
        level in arb_nesting_level(),
        key in arb_unknown_key()
    ) {
        let toml_content = inject_unknown_key(&level, &key);
        let path = write_temp_toml(&toml_content);
        let result = load_config(std::path::Path::new(&path));
        prop_assert!(result.is_err(),
            "expected error for unknown key '{}' at {:?}, got Ok", key, level);
        let errs = result.unwrap_err();
        // The error should be a TomlSyntax (from deny_unknown_fields)
        prop_assert!(
            errs.iter().any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
            "expected TomlSyntax error for unknown key '{}' at {:?}, got: {:?}", key, level, errs
        );
    }
}
