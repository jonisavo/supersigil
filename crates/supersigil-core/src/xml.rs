/// Escape a string for use as XML text content.
///
/// Replaces `&`, `<`, and `>` with their XML entity references so that
/// arbitrary text can safely appear inside XML element bodies.
#[must_use]
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_special_chars() {
        assert_eq!(xml_escape("<a>&b</a>"), "&lt;a&gt;&amp;b&lt;/a&gt;");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(xml_escape("hello world"), "hello world");
    }
}
