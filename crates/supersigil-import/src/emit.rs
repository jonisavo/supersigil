pub mod design;
pub mod requirements;
pub mod tasks;

use std::fmt::Write;

/// Escape a string for use as XML text content.
///
/// Replaces `&`, `<`, and `>` with their XML entity references so that
/// arbitrary prose can safely appear inside XML element bodies.
///
/// Note: `supersigil-core` exports an identical `xml_escape` function but
/// `supersigil-import` does not depend on it at runtime, so we keep a local
/// copy to avoid adding a dependency edge.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape a string for use inside a double-quoted YAML scalar.
///
/// Escapes backslashes and double quotes so the resulting value is valid YAML
/// when interpolated between `"..."`.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Emit the standard supersigil front matter block.
pub(crate) fn emit_front_matter(out: &mut String, doc_id: &str, doc_type: &str, title: &str) {
    let escaped_title = yaml_escape(title);
    let _ = writeln!(out, "---");
    let _ = writeln!(out, "supersigil:");
    let _ = writeln!(out, "  id: {doc_id}");
    let _ = writeln!(out, "  type: {doc_type}");
    let _ = writeln!(out, "  status: draft");
    let _ = writeln!(out, "title: \"{escaped_title}\"");
    let _ = writeln!(out, "---");
    out.push('\n');
}
