//! `mail-templates` — branded HTML/plain transactional email rendering.
//!
//! The PlausiDen email stack sends a half-dozen kinds of automatic
//! mail: magic-link sign-ins, feedback notifications, contact-form
//! inquiries, NDR / bounce messages, DNS / config dispatches, alerts.
//! All of them want the same visual chrome (gradient hero, branded
//! footer, accent-stripe content cards) — this crate is the typed,
//! reusable substrate that produces it.
//!
//! ## Design
//!
//! - **Typed AST.** [`EmailDocument`] is a tree of [`Block`]s that
//!   know how to render themselves to HTML and plain text. The
//!   intermediate AST lets a renderer choose where to break, when to
//!   collapse, and what to escape.
//! - **No runtime template engine.** Hand-written Rust functions;
//!   data flows in as typed structs, HTML flows out as an owned
//!   `String`. No Tera / Handlebars / Liquid — every interpolation
//!   point is grep-visible.
//! - **Email-client-safe.** The HTML renderer emits `<table>`-based
//!   layouts with inline styles; no external CSS, no `<style>`
//!   blocks (Gmail strips them), no remote images, no JS. Only the
//!   system font stack.
//! - **Plain text alongside HTML.** Every document also renders to a
//!   `text/plain` alternative — Postfix bounces, terminal mail
//!   readers, and accessibility tools all consume that path.
//! - **Loom-token-aligned.** Color and spacing tokens mirror the
//!   PlausiDen-Loom palette so the email chrome feels like a
//!   continuation of the website.
//! - **Nested cards.** A [`GroupCard`] can hold either a flat list
//!   of [`Field`]s (label + value rows) OR a list of nested
//!   [`RecordCard`]s — each record gets its own bordered panel for
//!   maximum visual separation. DNS-records dispatches use the
//!   nested form.

#![doc(html_no_source)]
#![deny(missing_docs)]

mod escape;
mod render_html;
mod render_plain;
mod tokens;

pub use tokens::Theme;

pub mod prebuilt;

use serde::{Deserialize, Serialize};

/// Top-level email document. Renders to a complete `text/html` body
/// and a paired `text/plain` alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailDocument {
    /// `Subject:` header value (escaped at SMTP layer, not here).
    pub subject: String,
    /// Hidden preheader text — the snippet shown in the inbox list
    /// preview before the user opens the message. Keep ≤ 120 chars.
    pub preheader: String,
    /// Optional pill-shaped tag above the headline (e.g., "New feedback").
    pub eyebrow: Option<String>,
    /// Main headline displayed below the gradient hero.
    pub heading: String,
    /// Optional intro paragraph between heading and the first block.
    pub intro: Option<String>,
    /// Body blocks rendered in order.
    pub blocks: Vec<Block>,
    /// Lines printed in muted text just below the last block. Useful
    /// for "source of truth" / "this is automated" disclaimers.
    pub footer_lines: Vec<String>,
}

impl EmailDocument {
    /// Render to a complete HTML body using the default
    /// [`Theme::plausiden`] palette. Includes the `<!DOCTYPE>`,
    /// `<html>`, `<head>`, and `<body>` wrappers.
    #[must_use]
    pub fn render_html(&self) -> String {
        render_html::render(self, &Theme::plausiden())
    }

    /// Render to HTML with an explicit theme — the multi-tenant
    /// path. Use [`Theme::sacredvote`] (or a custom-built `Theme`)
    /// to render the same AST with a different brand chrome.
    #[must_use]
    pub fn render_html_with_theme(&self, theme: &Theme) -> String {
        render_html::render(self, theme)
    }

    /// Render to a `text/plain` alternative. Wraps lines at 78 cols
    /// where possible, preserves structure via blank lines and
    /// underline-style section headings.
    #[must_use]
    pub fn render_plain(&self) -> String {
        render_plain::render(self)
    }
}

/// One body element. Rendered in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    /// Group card — the main content unit. Eyebrow + title + sub +
    /// either a [`Field`] table or a list of nested [`RecordCard`]s
    /// + optional tinted how-to footer.
    Group(GroupCard),
    /// Mid-body section heading (h2-equivalent).
    ///
    /// Struct variant rather than tuple newtype because serde's
    /// `tag = "kind"` adjacency requires struct/map shapes — tuple
    /// newtypes holding plain strings refuse to serialize with an
    /// internal tag.
    SectionHeading {
        /// Heading text.
        text: String,
    },
    /// Plain prose paragraph. See [`Block::SectionHeading`] for the
    /// reason this is a struct variant rather than `Paragraph(String)`.
    Paragraph {
        /// Paragraph body.
        text: String,
    },
    /// Monospaced code/command block. Each entry is one line.
    Code(CodeBlock),
    /// Call-to-action button. Renders as a styled link with a
    /// shadow and subtle hover affordance (plain text falls back to
    /// `Label: <url>`).
    Cta(Cta),
}

/// Group card payload. Rendered as a bordered panel with a 3px
/// primary-tinted left rule, an eyebrow tag, a heavy title, an
/// optional subtitle, a body ([`Field`] table or nested
/// [`RecordCard`] list), and an optional tinted "how-to" footer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCard {
    /// Pill-shaped tag in primary color above the title.
    pub eyebrow: String,
    /// Card headline.
    pub title: String,
    /// Optional descriptive sub-line beneath the title.
    pub subtitle: Option<String>,
    /// Body shape — flat fields OR nested record cards.
    pub body: GroupBody,
    /// Optional concise instruction printed in a tinted footer.
    /// Plain HTML allowed for inline `<code>` snippets — escape at
    /// the call site if the source is user-controlled.
    pub how_to: Option<String>,
}

/// Body shape of a [`GroupCard`].
///
/// Struct variants rather than tuple newtypes for the same serde
/// reason as [`Block::Paragraph`] — `tag = "kind"` adjacency
/// requires struct/map shapes for round-trip JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupBody {
    /// Flat label/value table — used for sender summaries, settings
    /// dumps, etc.
    Fields {
        /// Field rows.
        fields: Vec<Field>,
    },
    /// Nested record cards — each record gets its own bordered
    /// panel for maximum visual separation. Use this when each row
    /// is meaningful in isolation (DNS records, audit-log entries,
    /// user accounts).
    Records {
        /// Records, in order.
        records: Vec<RecordCard>,
    },
}

/// One row inside a [`GroupCard::body`] = `Fields`. The HTML renderer
/// lays these out as a label-on-left, value-on-right table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// Short uppercase-presented label (e.g., `"Host"`).
    pub label: String,
    /// Field value.
    pub value: String,
    /// If `true`, render the value in a monospaced "code box" with a
    /// pale background and rounded corners. Useful for DNS values,
    /// commands, IDs.
    pub mono: bool,
}

/// One nested card inside a [`GroupCard::body`] = `Records`. Each
/// record renders as its own bordered panel with a numeric prefix,
/// a primary label, a type tag, the value, and an optional note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordCard {
    /// Number/eyebrow text rendered at the top-left of the card
    /// (e.g., `"Record 1 of 5"`, or `"#1"`).
    pub eyebrow: String,
    /// Primary identifier — the "host" or "name" of the record.
    pub primary_label: String,
    /// Short type tag rendered as a pill (e.g., `"TXT"`, `"AAAA"`,
    /// `"CNAME"`). Optional — pass `None` for non-typed records.
    pub type_tag: Option<String>,
    /// Main value — rendered in a monospaced code-box.
    pub value: String,
    /// Optional explanation note rendered in muted text below the
    /// value (e.g., "this string is one continuous TXT — do not
    /// split").
    pub note: Option<String>,
}

/// Code block payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeBlock {
    /// Optional eyebrow above the block (e.g., `"Verification"`).
    pub eyebrow: Option<String>,
    /// Lines, in order.
    pub lines: Vec<String>,
}

/// Call-to-action button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cta {
    /// Visible button label.
    pub label: String,
    /// `href` target — `https://` or `mailto:` URL.
    pub href: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> EmailDocument {
        EmailDocument {
            subject: "Sample".into(),
            preheader: "Sample preheader.".into(),
            eyebrow: Some("Group test".into()),
            heading: "Sample document".into(),
            intro: Some("Intro paragraph.".into()),
            blocks: vec![
                Block::Group(GroupCard {
                    eyebrow: "Group A".into(),
                    title: "First group".into(),
                    subtitle: Some("With a subtitle.".into()),
                    body: GroupBody::Fields {
                        fields: vec![
                            Field {
                                label: "Host".into(),
                                value: "outreach".into(),
                                mono: true,
                            },
                            Field {
                                label: "Type".into(),
                                value: "TXT".into(),
                                mono: true,
                            },
                        ],
                    },
                    how_to: Some("After publishing, run <code>dig +short</code>.".into()),
                }),
                Block::Group(GroupCard {
                    eyebrow: "Group B".into(),
                    title: "Records — nested".into(),
                    subtitle: None,
                    body: GroupBody::Records {
                        records: vec![
                            RecordCard {
                                eyebrow: "Record 1 of 2".into(),
                                primary_label: "outreach".into(),
                                type_tag: Some("A".into()),
                                value: "207.246.86.218".into(),
                                note: None,
                            },
                            RecordCard {
                                eyebrow: "Record 2 of 2".into(),
                                primary_label: "outreach._domainkey.outreach".into(),
                                type_tag: Some("TXT".into()),
                                value: "v=DKIM1; ...".into(),
                                note: Some("One continuous string — do not split.".into()),
                            },
                        ],
                    },
                    how_to: None,
                }),
                Block::SectionHeading {
                    text: "Verification".into(),
                },
                Block::Code(CodeBlock {
                    eyebrow: None,
                    lines: vec!["dig +short A example.com".into()],
                }),
                Block::Cta(Cta {
                    label: "View dashboard".into(),
                    href: "https://example.com/dashboard".into(),
                }),
            ],
            footer_lines: vec!["Source of truth: example.com/spec".into()],
        }
    }

    #[test]
    fn html_renders_doctype_and_heading() {
        let doc = sample_doc();
        let h = doc.render_html();
        assert!(h.contains("<!DOCTYPE html>"));
        assert!(h.contains("Sample document"));
        assert!(h.contains("First group"));
        assert!(h.contains("dig +short A example.com"));
    }

    #[test]
    fn html_no_style_blocks_or_external_assets() {
        let h = sample_doc().render_html();
        assert!(!h.contains("<style"));
        assert!(!h.contains("<link "));
        assert!(!h.contains("<script"));
    }

    #[test]
    fn html_renders_nested_records_as_individual_cards() {
        let h = sample_doc().render_html();
        // Each RecordCard's primary_label should appear once per record.
        assert!(h.contains("outreach._domainkey.outreach"));
        assert!(h.contains("Record 1 of 2"));
        assert!(h.contains("Record 2 of 2"));
        // The TYPE tag should render as a pill.
        assert!(h.contains(">TXT<"));
    }

    #[test]
    fn plain_renders_all_content() {
        let doc = sample_doc();
        let p = doc.render_plain();
        assert!(p.contains("Sample document"));
        assert!(p.contains("First group"));
        assert!(p.contains("Host: outreach"));
        assert!(p.contains("dig +short A example.com"));
        assert!(p.contains("Verification"));
        // Nested records render as numbered list with type-tag.
        assert!(p.contains("outreach._domainkey.outreach"));
    }

    #[test]
    fn html_escapes_user_content() {
        let mut doc = sample_doc();
        doc.heading = "<script>alert(1)</script>".into();
        let h = doc.render_html();
        assert!(!h.contains("<script>alert(1)"));
        assert!(h.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_includes_preheader_hidden_text() {
        let doc = sample_doc();
        let h = doc.render_html();
        assert!(h.contains("Sample preheader."));
        assert!(h.contains("mso-hide:all"));
    }

    #[test]
    fn cta_renders_as_anchor_button() {
        let h = sample_doc().render_html();
        assert!(h.contains("View dashboard"));
        assert!(h.contains("href=\"https://example.com/dashboard\""));
    }

    #[test]
    fn default_html_uses_plausiden_chrome() {
        let h = sample_doc().render_html();
        assert!(h.contains("PlausiDen"));
        assert!(h.contains("Plausible. Defensible."));
        assert!(h.contains("plausiden.com"));
        assert!(h.contains("team@plausiden.com"));
    }

    #[test]
    fn explicit_theme_renders_alternate_chrome() {
        let h = sample_doc().render_html_with_theme(&Theme::sacredvote());
        // SacredVote chrome instead of PlausiDen
        assert!(h.contains("SacredVote"));
        assert!(h.contains("Defending the ballot."));
        assert!(h.contains("sacred.vote"));
        // Should NOT contain PlausiDen footer
        assert!(!h.contains("PlausiDen LLC"));
    }

    #[test]
    fn custom_theme_propagates_brand_color() {
        let mut t = Theme::plausiden();
        t.brand_primary = "#ff00aa".into();
        let h = sample_doc().render_html_with_theme(&t);
        // Color shows up at multiple touch points (pill, button, links).
        assert!(
            h.matches("#ff00aa").count() >= 3,
            "expected brand_primary in multiple places, got {} occurrences",
            h.matches("#ff00aa").count()
        );
    }
}
