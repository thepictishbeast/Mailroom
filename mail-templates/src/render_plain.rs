//! Plain-text renderer. Walks the same AST as the HTML path and
//! emits a `text/plain` alternative. Wraps prose at 78 cols, keeps
//! code blocks unwrapped (the operator may need to copy verbatim),
//! preserves visual hierarchy via blank lines and rule strings.

use std::fmt::Write as _;

use crate::{Block, CodeBlock, Cta, EmailDocument, Field, GroupBody, GroupCard, RecordCard};

const RULE_HEAVY: &str = "============================================================";
const RULE_LIGHT: &str = "------------------------------------------------------------";
const WRAP: usize = 78;

pub(crate) fn render(doc: &EmailDocument) -> String {
    let mut out = String::new();

    // Hero / heading
    if let Some(eyebrow) = &doc.eyebrow {
        let _ = writeln!(out, "[{}]", eyebrow);
    }
    let _ = writeln!(out, "{}", doc.heading);
    let _ = writeln!(out, "{}", &RULE_HEAVY[..doc.heading.chars().count().min(60).max(8)]);
    out.push('\n');

    if let Some(intro) = &doc.intro {
        out.push_str(&wrap(intro, WRAP));
        out.push_str("\n\n");
    }

    for block in &doc.blocks {
        render_block(block, &mut out);
    }

    if !doc.footer_lines.is_empty() {
        out.push_str(RULE_LIGHT);
        out.push('\n');
        for line in &doc.footer_lines {
            out.push_str(&wrap(line, WRAP));
            out.push('\n');
        }
    }

    out
}

fn render_block(block: &Block, out: &mut String) {
    match block {
        Block::Group(g) => render_group(g, out),
        Block::SectionHeading { text: h } => {
            let _ = writeln!(out, "{}", h);
            let _ = writeln!(out, "{}", &RULE_LIGHT[..h.chars().count().min(60).max(4)]);
            out.push('\n');
        }
        Block::Paragraph { text: p } => {
            out.push_str(&wrap(p, WRAP));
            out.push_str("\n\n");
        }
        Block::Code(c) => render_code(c, out),
        Block::Cta(c) => render_cta(c, out),
    }
}

fn render_group(g: &GroupCard, out: &mut String) {
    let _ = writeln!(out, "[{}] {}", g.eyebrow, g.title);
    if let Some(sub) = &g.subtitle {
        for line in wrap(sub, WRAP - 2).lines() {
            let _ = writeln!(out, "  {}", line);
        }
    }
    let _ = writeln!(out, "{}", &RULE_LIGHT[..(g.title.chars().count() + g.eyebrow.chars().count() + 3).min(60).max(8)]);

    match &g.body {
        GroupBody::Fields { fields } => {
            for f in fields {
                render_field(f, out);
            }
        }
        GroupBody::Records { records } => {
            for (i, r) in records.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                render_record(r, out);
            }
        }
    }
    if let Some(how_to) = &g.how_to {
        out.push('\n');
        // Strip basic HTML tags from how_to (we permit inline <code>).
        let stripped = strip_tags(how_to);
        for line in wrap(&stripped, WRAP - 2).lines() {
            let _ = writeln!(out, "→ {}", line);
        }
    }
    out.push('\n');
}

fn render_field(f: &Field, out: &mut String) {
    let _ = writeln!(out, "  {}: {}", f.label, f.value);
}

fn render_record(r: &RecordCard, out: &mut String) {
    let type_str = match &r.type_tag {
        Some(t) => format!(" [{}]", t),
        None => String::new(),
    };
    let _ = writeln!(out, "  {} · {}{}", r.eyebrow, r.primary_label, type_str);
    for line in r.value.lines() {
        let _ = writeln!(out, "    {}", line);
    }
    if let Some(note) = &r.note {
        for line in wrap(note, WRAP - 6).lines() {
            let _ = writeln!(out, "      ↳ {}", line);
        }
    }
}

fn render_code(c: &CodeBlock, out: &mut String) {
    if let Some(eyebrow) = &c.eyebrow {
        let _ = writeln!(out, "[{}]", eyebrow);
    }
    for line in &c.lines {
        let _ = writeln!(out, "  $ {}", line);
    }
    out.push('\n');
}

fn render_cta(c: &Cta, out: &mut String) {
    let _ = writeln!(out, "→ {}: {}", c.label, c.href);
    out.push('\n');
}

/// Hard-wrap to width, breaking on whitespace. Long unbreakable
/// tokens (URLs, base64) are kept intact even if they exceed the
/// width — splitting them would change meaning.
fn wrap(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for paragraph in s.split('\n') {
        let mut line_len = 0usize;
        let mut first_word = true;
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if !first_word {
                if line_len + 1 + word_len > width {
                    out.push('\n');
                    line_len = 0;
                } else {
                    out.push(' ');
                    line_len += 1;
                }
            }
            out.push_str(word);
            line_len += word_len;
            first_word = false;
        }
        out.push('\n');
    }
    // Strip the trailing newline we always add.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Strip a small set of inline HTML tags from how-to strings. We
/// permit `<code>` in the source for HTML readers; the plain
/// renderer just removes the angle-brackets.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_on_word_boundary() {
        let s = wrap("the quick brown fox jumped over the lazy dog", 20);
        for line in s.lines() {
            assert!(line.chars().count() <= 20, "line too long: {line:?}");
        }
    }

    #[test]
    fn wrap_keeps_long_tokens() {
        // URLs / base64 are not split.
        let url = "https://example.com/very/long/path/that/exceeds/twenty/columns";
        let s = wrap(url, 20);
        assert!(s.contains(url), "long token should pass through: {s:?}");
    }

    #[test]
    fn strip_tags_drops_inline_code() {
        assert_eq!(strip_tags("run <code>dig</code> first"), "run dig first");
    }
}
