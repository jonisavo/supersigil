//! A builder of Claude Code transcripts for generated sessions.

use std::path::Path;

use serde_json::{Value, json};

/// Builds the records of one generated session, each chained to the last.
pub struct Session<'a> {
    id: &'a str,
    cwd: &'a Path,
    text: String,
    count: u32,
    parent: Option<String>,
    /// The agent whose transcript this is; `None` for the main transcript.
    agent: Option<&'a str>,
}

impl<'a> Session<'a> {
    /// The main transcript of session `id`, working in `cwd`.
    pub fn new(id: &'a str, cwd: &'a Path) -> Self {
        Self {
            id,
            cwd,
            text: String::new(),
            count: 0,
            parent: None,
            agent: None,
        }
    }

    /// The transcript of agent `agent` of session `id`.
    pub fn agent(id: &'a str, cwd: &'a Path, agent: &'a str) -> Self {
        Self {
            agent: Some(agent),
            ..Self::new(id, cwd)
        }
    }

    /// Works in `cwd` from the next record on, as a session that changes
    /// directory does.
    pub fn cd(&mut self, cwd: &'a Path) -> &mut Self {
        self.cwd = cwd;
        self
    }

    /// A record of `kind` with message `content`, and `result` as its
    /// `toolUseResult` when present.
    pub fn push(&mut self, kind: &str, content: &Value, result: Option<Value>) -> &mut Self {
        self.push_with(kind, content, result, &json!({}))
    }

    /// [`Self::push`] with the fields of `extra` added to the record.
    pub fn push_with(
        &mut self,
        kind: &str,
        content: &Value,
        result: Option<Value>,
        extra: &Value,
    ) -> &mut Self {
        let uuid = format!("{}-{}", self.id, self.count);
        let mut value = json!({
            "type": kind, "uuid": uuid, "parentUuid": self.parent, "sessionId": self.id,
            "cwd": self.cwd.to_string_lossy(),
            "timestamp": format!("2026-09-29T10:00:{:02}.000Z", self.count),
            "isSidechain": false, "promptId": format!("prompt-{}", self.id),
            "message": {"role": kind, "content": content},
        });
        if let Some(result) = result {
            value["toolUseResult"] = result;
        }
        if let Some(agent) = self.agent {
            value["isSidechain"] = json!(true);
            value["agentId"] = json!(agent);
        }
        for (key, field) in extra.as_object().into_iter().flatten() {
            value[key] = field.clone();
        }
        self.text.push_str(&value.to_string());
        self.text.push('\n');
        self.parent = Some(uuid);
        self.count += 1;
        self
    }

    /// A person's message `text`.
    pub fn prompt(&mut self, text: &str) -> &mut Self {
        self.push("user", &json!(text), None)
    }

    /// An Edit call on a file over 10 KB and its result, as Claude Code
    /// records it: no `originalFile`, and the patch of replacing `old` with
    /// `new` in `before`, the file as the edit found it.
    pub fn edit(
        &mut self,
        tool: &str,
        file: &Path,
        before: &str,
        old: &str,
        new: &str,
    ) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Edit",
            "input": {"file_path": file, "old_string": old, "new_string": new}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "updated"}]);
        let result = json!({"filePath": file, "oldString": old, "newString": new,
            "originalFile": null, "structuredPatch": structured_patch(before, old, new),
            "userModified": false, "replaceAll": false});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A Write call that creates `file`, and its result.
    pub fn create(&mut self, tool: &str, file: &Path, content: &str) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Write",
            "input": {"file_path": file, "content": content}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "File created"}]);
        let result =
            json!({"type": "create", "filePath": file, "content": content, "structuredPatch": []});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A Bash call running `command` and its result in Claude Code's
    /// format, with `report` as `bashEditDiff` when present.
    pub fn bash(&mut self, tool: &str, command: &str, report: Option<Value>) -> &mut Self {
        let call = json!([{"type": "tool_use", "id": tool, "name": "Bash",
            "input": {"command": command}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": ""}]);
        let mut result = json!({"stdout": "", "stderr": "", "interrupted": false,
            "isImage": false, "noOutputExpected": false});
        if let Some(report) = report {
            result["bashEditDiff"] = report;
        }
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// An Agent call `tool` dispatching `agent` in the background, and its
    /// result.
    pub fn dispatch(&mut self, tool: &str, agent: &str) -> &mut Self {
        let call = json!([{"type": "tool_use", "id": tool, "name": "Agent",
            "input": {"description": agent, "prompt": "Edit big.txt.",
                "subagent_type": "general-purpose"}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool,
            "content": format!("Async agent launched successfully. agentId: {agent}")}]);
        self.push("assistant", &call, None).push(
            "user",
            &done,
            Some(json!({"status": "async_launched", "agentId": agent})),
        )
    }

    /// The harness delivering the notification that `agent`, started by
    /// call `tool`, completed, as it does to an idle issuer.
    pub fn stopped(&mut self, tool: &str, agent: &str) -> &mut Self {
        let text = format!(
            "<task-notification>\n<task-id>{agent}</task-id>\n<tool-use-id>{tool}</tool-use-id>\n\
             <status>completed</status>\n</task-notification>"
        );
        self.push_with(
            "user",
            &json!(text),
            None,
            &json!({"origin": {"kind": "task-notification"}}),
        )
    }

    /// A `NotebookEdit` call, which the capture cannot read, and its result.
    pub fn notebook_edit(&mut self, tool: &str) -> &mut Self {
        let notebook = self.cwd.join("nb.ipynb");
        let call = json!([{"type": "tool_use", "id": tool, "name": "NotebookEdit",
            "input": {"notebook_path": notebook.to_string_lossy(), "new_source": "x"}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "ok"}]);
        self.push("assistant", &call, None)
            .push("user", &done, None)
    }

    /// The transcript's text so far.
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

/// Claude Code's `structuredPatch` for replacing `old` with `new` once in
/// `before`: one hunk with three lines of context, as jsdiff builds it, with
/// tabs shown as two spaces.
fn structured_patch(before: &str, old: &str, new: &str) -> Value {
    let after = before.replacen(old, new, 1);
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_changed, new_changed) = (old_lines.len() - suffix, new_lines.len() - suffix);
    let start = prefix.saturating_sub(3);
    let old_end = (old_changed + 3).min(old_lines.len());
    let shown = |marker: &str, lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .map(|line| format!("{marker}{}", line.replace('\t', "  ")))
            .collect()
    };
    let mut lines = shown(" ", &old_lines[start..prefix]);
    lines.extend(shown("-", &old_lines[prefix..old_changed]));
    lines.extend(shown("+", &new_lines[prefix..new_changed]));
    lines.extend(shown(" ", &old_lines[old_changed..old_end]));
    let after_context = old_end - old_changed;
    json!([{"oldStart": start + 1, "oldLines": old_end - start,
        "newStart": start + 1, "newLines": new_changed + after_context - start,
        "lines": lines}])
}
