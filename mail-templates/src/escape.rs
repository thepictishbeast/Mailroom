//! HTML / plain-text escape helpers.
//!
//! Conservative — covers the five XML metacharacters and nothing
//! else. Intentional: the renderer wraps every interpolation in this
//! call, so over-eager normalization (smart-quote rewrites, NBSP
//! substitutions) would be invisible at the call site and surprise
//! the operator down the line.

/// HTML-escape arbitrary text for inclusion in an email body.
#[must_use]
pub(crate) fn html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_metacharacters() {
        assert_eq!(html("<a>"), "&lt;a&gt;");
        assert_eq!(html("a&b"), "a&amp;b");
        assert_eq!(html("\"x\""), "&quot;x&quot;");
        assert_eq!(html("'x'"), "&#39;x&#39;");
    }

    #[test]
    fn passes_unicode_through() {
        // Smart-quotes, dashes, em-dash should not be normalized.
        let s = "“PlausiDen” · café — résumé";
        assert_eq!(html(s), s);
    }
}
