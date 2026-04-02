// Front matter extraction and deserialization tests

mod common;
use common::dummy_path;

// ── Stage 1: Front matter extraction ────────────────────────────────────────

mod extract_front_matter {
    use super::*;
    use supersigil_parser::extract_front_matter;

    #[test]
    fn valid_front_matter_extracts_yaml_and_body() {
        let content = "---\nsupersigil:\n  id: test\n---\nbody";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "supersigil:\n  id: test\n");
        assert_eq!(body, "body");
    }

    #[test]
    fn delimiter_with_trailing_whitespace_accepted() {
        let content = "---  \nsupersigil:\n  id: test\n---  \nbody text";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "supersigil:\n  id: test\n");
        assert_eq!(body, "body text");
    }

    #[test]
    fn no_opening_delimiter_returns_none() {
        let content = "no front matter here\njust content";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none(), "expected None for no opening ---");
    }

    #[test]
    fn unclosed_front_matter_returns_error() {
        let content = "---\nsupersigil:\n  id: test\nno closing delimiter";
        let result = extract_front_matter(content, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::UnclosedFrontMatter { .. }),
            "expected UnclosedFrontMatter, got {err:?}"
        );
    }

    #[test]
    fn empty_yaml_between_delimiters() {
        let content = "---\n---\nbody";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "");
        assert_eq!(body, "body");
    }

    #[test]
    fn triple_dash_inside_yaml_terminates_front_matter() {
        // The first `---` on its own line after the opening closes the front matter.
        // Multi-document YAML separators are not supported.
        let content = "---\nkey: value\n---\nmore content\n---\nfinal";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "key: value\n");
        assert_eq!(body, "more content\n---\nfinal");
    }

    #[test]
    fn opening_delimiter_not_on_first_line_returns_none() {
        let content = "some text\n---\nsupersigil:\n  id: test\n---\n";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn empty_content_returns_none() {
        let content = "";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn only_opening_delimiter_returns_error() {
        let content = "---\n";
        let result = extract_front_matter(content, &dummy_path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            supersigil_core::ParseError::UnclosedFrontMatter { .. }
        ));
    }

    #[test]
    fn body_after_closing_delimiter_with_no_trailing_newline() {
        let content = "---\nid: x\n---";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "id: x\n");
        assert_eq!(body, "");
    }
}

// ── Stage 1: Front matter deserialization ───────────────────────────────────

mod deserialize_front_matter {
    use super::*;
    use supersigil_parser::{FrontMatterResult, deserialize_front_matter};

    #[test]
    fn valid_supersigil_with_all_fields() {
        let yaml = "supersigil:\n  id: my-doc\n  type: requirement\n  status: draft\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "my-doc");
                assert_eq!(frontmatter.doc_type.as_deref(), Some("requirement"));
                assert_eq!(frontmatter.status.as_deref(), Some("draft"));
                assert!(extra.is_empty());
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil, got NotSupersigil"),
        }
    }

    #[test]
    fn supersigil_with_only_id() {
        let yaml = "supersigil:\n  id: minimal\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "minimal");
                assert!(frontmatter.doc_type.is_none());
                assert!(frontmatter.status.is_none());
                assert!(extra.is_empty());
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn missing_id_returns_error() {
        let yaml = "supersigil:\n  type: requirement\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::MissingId { .. }),
            "expected MissingId, got {err:?}"
        );
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let yaml = ":\n  - :\n    bad: [yaml\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::InvalidYaml { .. }),
            "expected InvalidYaml, got {err:?}"
        );
    }

    #[test]
    fn no_supersigil_key_returns_not_supersigil() {
        let yaml = "title: My Document\nauthor: someone\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        assert!(
            matches!(result, FrontMatterResult::NotSupersigil),
            "expected NotSupersigil"
        );
    }

    #[test]
    fn extra_metadata_keys_preserved() {
        let yaml = "supersigil:\n  id: doc-1\ntitle: My Doc\nauthor: dev\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "doc-1");
                assert_eq!(extra.len(), 2);
                assert_eq!(extra.get("title").and_then(|v| v.as_str()), Some("My Doc"));
                assert_eq!(extra.get("author").and_then(|v| v.as_str()), Some("dev"));
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn supersigil_inline_syntax_with_extra_keys() {
        let yaml = "supersigil: { id: x }\nversion: 2\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "x");
                assert!(frontmatter.doc_type.is_none());
                assert!(frontmatter.status.is_none());
                assert_eq!(extra.len(), 1);
                assert!(extra.contains_key("version"));
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn empty_yaml_returns_not_supersigil() {
        let yaml = "";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        assert!(matches!(result, FrontMatterResult::NotSupersigil));
    }

    #[test]
    fn supersigil_empty_object_missing_id_returns_error() {
        let yaml = "supersigil: {}\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            supersigil_core::ParseError::MissingId { .. }
        ));
    }
}
