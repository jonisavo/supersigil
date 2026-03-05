use regex::Regex;
use std::sync::LazyLock;

/// Parsed requirements.md
#[derive(Debug, Clone)]
pub struct ParsedRequirements {
    pub title: Option<String>,
    pub introduction: String,
    pub glossary: Option<String>,
    pub requirements: Vec<ParsedRequirement>,
}

#[derive(Debug, Clone)]
pub struct ParsedRequirement {
    pub number: String,
    pub title: Option<String>,
    pub user_story: Option<String>,
    pub criteria: Vec<ParsedCriterion>,
    /// Prose between user story and criteria, or after criteria.
    pub extra_prose: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedCriterion {
    pub index: String,
    pub text: String,
}

// Regex patterns per the design document's parsing strategy table.
static DOC_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^# Requirements Document(?:: (.+))?$").expect("valid regex"));

static REQ_HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^### Requirement (\w+)(?:: (.+))?$").expect("valid regex"));

static USER_STORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*User Story:\*\*\s*(.+)$").expect("valid regex"));

static CRITERION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+[a-zA-Z]?)\.\s+(.+)$").expect("valid regex"));

/// Parser state machine for tracking which section we're in.
#[derive(Debug, PartialEq)]
enum Section {
    /// Before any recognized heading — collecting introduction text.
    Introduction,
    /// Inside `## Glossary` section.
    Glossary,
    /// Inside a `### Requirement N` section, before acceptance criteria.
    RequirementBody,
    /// Inside `#### Acceptance Criteria` subsection.
    AcceptanceCriteria,
}

/// Parse a Kiro `requirements.md` file into a structured IR.
///
/// Uses line-by-line processing with regex patterns. Handles:
/// - Document title from `# Requirements Document: Title`
/// - Introduction text before the first requirement
/// - Optional glossary section
/// - Requirement sections with number, title, user story, criteria
/// - Alphanumeric criterion indices (e.g., `8a`)
#[must_use]
pub fn parse_requirements(content: &str) -> ParsedRequirements {
    let mut title: Option<String> = None;
    let mut introduction_lines: Vec<String> = Vec::new();
    let mut glossary_lines: Vec<String> = Vec::new();
    let mut requirements: Vec<ParsedRequirement> = Vec::new();

    let mut section = Section::Introduction;
    let mut current_req: Option<RequirementBuilder> = None;
    let mut extra_prose_buf: Vec<String> = Vec::new();

    for line in content.lines() {
        // Check for document title heading
        if let Some(caps) = DOC_TITLE_RE.captures(line) {
            title = caps.get(1).map(|m| m.as_str().trim().to_string());
            continue;
        }

        // Check for glossary heading
        if line.starts_with("## Glossary") {
            // Flush any current requirement
            flush_requirement(&mut current_req, &mut requirements, &mut extra_prose_buf);
            section = Section::Glossary;
            continue;
        }

        // Check for requirement heading
        if let Some(caps) = REQ_HEADING_RE.captures(line) {
            // Flush previous requirement if any
            flush_requirement(&mut current_req, &mut requirements, &mut extra_prose_buf);

            let number = caps[1].to_string();
            let req_title = caps.get(2).map(|m| m.as_str().trim().to_string());

            current_req = Some(RequirementBuilder {
                number,
                title: req_title,
                user_story: None,
                criteria: Vec::new(),
            });
            section = Section::RequirementBody;
            continue;
        }

        // Check for acceptance criteria heading
        if line.trim() == "#### Acceptance Criteria" {
            section = Section::AcceptanceCriteria;
            continue;
        }

        // Process line based on current section
        match section {
            Section::Introduction => {
                introduction_lines.push(line.to_string());
            }
            Section::Glossary => {
                // Stop glossary collection if we hit a non-glossary ## heading
                if line.starts_with("## ") && !line.starts_with("## Glossary") {
                    section = Section::Introduction;
                    // This line is a different section heading — treat as intro prose
                    introduction_lines.push(line.to_string());
                } else {
                    glossary_lines.push(line.to_string());
                }
            }
            Section::RequirementBody => {
                if let Some(ref mut req) = current_req {
                    if let Some(caps) = USER_STORY_RE.captures(line) {
                        req.user_story = Some(caps[1].trim().to_string());
                    } else if !line.trim().is_empty() {
                        extra_prose_buf.push(line.to_string());
                    }
                }
            }
            Section::AcceptanceCriteria => {
                if let Some(ref mut req) = current_req {
                    if let Some(caps) = CRITERION_RE.captures(line) {
                        let index = caps[1].to_string();
                        let text = caps[2].trim().to_string();
                        req.criteria.push(ParsedCriterion { index, text });
                    } else if !line.trim().is_empty() {
                        // Non-criterion line after criteria — collect as extra prose
                        extra_prose_buf.push(line.to_string());
                    }
                }
            }
        }
    }

    // Flush the last requirement
    flush_requirement(&mut current_req, &mut requirements, &mut extra_prose_buf);

    let introduction = super::join_trimmed(&introduction_lines);
    let glossary = if glossary_lines.is_empty() {
        None
    } else {
        Some(super::join_trimmed(&glossary_lines))
    };

    ParsedRequirements {
        title,
        introduction,
        glossary,
        requirements,
    }
}

/// Temporary builder for accumulating requirement fields during parsing.
struct RequirementBuilder {
    number: String,
    title: Option<String>,
    user_story: Option<String>,
    criteria: Vec<ParsedCriterion>,
}

/// Flush the current requirement builder into the requirements list.
fn flush_requirement(
    current: &mut Option<RequirementBuilder>,
    requirements: &mut Vec<ParsedRequirement>,
    extra_prose_buf: &mut Vec<String>,
) {
    if let Some(builder) = current.take() {
        let extra_prose = if extra_prose_buf.is_empty() {
            Vec::new()
        } else {
            // Group consecutive non-empty lines into prose blocks
            let joined = super::join_trimmed(extra_prose_buf);
            if joined.is_empty() {
                Vec::new()
            } else {
                vec![joined]
            }
        };
        extra_prose_buf.clear();

        requirements.push(ParsedRequirement {
            number: builder.number,
            title: builder.title,
            user_story: builder.user_story,
            criteria: builder.criteria,
            extra_prose,
        });
    }
}
