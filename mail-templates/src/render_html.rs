//! HTML renderer. Walks an [`EmailDocument`] and emits a complete
//! `text/html` body using a [`Theme`] for the brand chrome. Every
//! output byte is hand-written; no template engine, no external
//! assets, no `<style>` blocks.

use std::fmt::Write as _;

use crate::escape::html as esc;
use crate::tokens::{Theme, FONT_MONO, FONT_SANS};
use crate::{Block, CodeBlock, Cta, EmailDocument, Field, GroupBody, GroupCard, RecordCard};

pub(crate) fn render(doc: &EmailDocument, theme: &Theme) -> String {
    let title = esc(&doc.subject);
    let preheader = esc(&doc.preheader);
    let mut inner = String::new();

    if let Some(eyebrow) = &doc.eyebrow {
        let _ = write!(
            inner,
            r#"<div style="display:inline-block;font-size:11px;font-weight:600;letter-spacing:0.1em;text-transform:uppercase;color:{primary};background:{primary_tint};padding:5px 10px;border-radius:999px;margin:0 0 14px;">{eyebrow_safe}</div>"#,
            primary = theme.brand_primary,
            primary_tint = tint(&theme.brand_primary, 0.08),
            eyebrow_safe = esc(eyebrow)
        );
    }
    let _ = write!(
        inner,
        r#"<h1 style="margin:0 0 12px;font-size:26px;line-height:1.25;color:{heading};font-weight:700;letter-spacing:-0.02em;font-family:{FONT_SANS};">{}</h1>"#,
        esc(&doc.heading),
        heading = theme.text_heading,
    );
    if let Some(intro) = &doc.intro {
        let _ = write!(
            inner,
            r#"<p style="margin:0 0 24px;font-size:14.5px;line-height:1.65;color:{body};font-family:{FONT_SANS};">{}</p>"#,
            esc(intro),
            body = theme.text_body,
        );
    }

    for block in &doc.blocks {
        render_block(block, theme, &mut inner);
    }

    if !doc.footer_lines.is_empty() {
        let _ = write!(
            inner,
            r#"<div style="margin:28px 0 0;padding-top:20px;border-top:1px solid {hairline};">"#,
            hairline = theme.hairline,
        );
        for line in &doc.footer_lines {
            let _ = write!(
                inner,
                r#"<p style="margin:0 0 6px;font-size:13px;line-height:1.6;color:{muted};font-family:{FONT_SANS};">{}</p>"#,
                esc(line),
                muted = theme.text_muted,
            );
        }
        inner.push_str("</div>");
    }

    let mut out = String::with_capacity(8192);
    let logo_letter = esc(&theme.logo_letter);
    let brand_name = esc(&theme.brand_name);
    let brand_suffix = esc(&theme.brand_suffix);
    let tagline = esc(&theme.tagline);
    let footer_org = esc(&theme.footer_org);
    let footer_address = esc(&theme.footer_address);
    let footer_website = esc(&theme.footer_website);
    let footer_email = esc(&theme.footer_email);
    let _ = write!(
        out,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light only">
<meta name="supported-color-schemes" content="light only">
<title>{title}</title>
</head>
<body style="margin:0;padding:0;background:{surface};font-family:{FONT_SANS};color:{body};-webkit-font-smoothing:antialiased;line-height:1.5;">
<div style="display:none;max-height:0;overflow:hidden;mso-hide:all;font-size:1px;color:{surface};">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:{surface};">
  <tr><td align="center" style="padding:40px 16px;">
    <table role="presentation" width="640" cellpadding="0" cellspacing="0" border="0" style="max-width:640px;background:{surface_raised};border:1px solid {hairline};border-radius:14px;overflow:hidden;box-shadow:0 1px 3px rgba(15,23,42,0.04),0 8px 24px rgba(15,23,42,0.06);">
      <tr><td style="background:linear-gradient(135deg,{navy} 0%,{primary} 60%,{accent} 100%);padding:28px 36px;color:#ffffff;">
        <table cellpadding="0" cellspacing="0" border="0">
          <tr>
            <td style="vertical-align:middle;padding-right:14px;">
              <span style="display:inline-block;width:36px;height:36px;background:rgba(255,255,255,0.18);border:1px solid rgba(255,255,255,0.35);border-radius:9px;text-align:center;line-height:36px;font-size:16px;font-weight:800;color:#ffffff;letter-spacing:-0.02em;font-family:{FONT_SANS};">{logo_letter}</span>
            </td>
            <td style="vertical-align:middle;font-family:{FONT_SANS};">
              <div style="font-size:18px;font-weight:700;letter-spacing:-0.01em;color:#ffffff;">{brand_name}{brand_suffix_html}</div>
              <div style="font-size:12px;font-weight:500;color:rgba(255,255,255,0.72);letter-spacing:0.04em;text-transform:uppercase;margin-top:2px;">{tagline}</div>
            </td>
          </tr>
        </table>
      </td></tr>
      <tr><td style="padding:32px 36px 28px;">
        {inner}
      </td></tr>
      <tr><td style="background:{surface};padding:22px 36px;border-top:1px solid {hairline};">
        <p style="margin:0;font-size:12px;line-height:1.7;color:{muted};font-family:{FONT_SANS};">
          <strong style="color:{body};font-weight:600;">{footer_org}</strong> &nbsp;·&nbsp; {footer_address}<br>
          <a href="https://{footer_website}" style="color:{primary};text-decoration:none;font-weight:600;">{footer_website}</a> &nbsp;·&nbsp; <a href="mailto:{footer_email}" style="color:{primary};text-decoration:none;font-weight:600;">{footer_email}</a>
        </p>
      </td></tr>
    </table>
    <p style="margin:16px 0 0;font-size:11px;color:{muted};text-align:center;letter-spacing:0.02em;font-family:{FONT_SANS};">
      Automated message — replies go to a real human.
    </p>
  </td></tr>
</table>
</body>
</html>
"#,
        navy = theme.brand_navy,
        primary = theme.brand_primary,
        accent = theme.brand_accent,
        body = theme.text_body,
        muted = theme.text_muted,
        surface = theme.surface,
        surface_raised = theme.surface_raised,
        hairline = theme.hairline,
        brand_suffix_html = if theme.brand_suffix.is_empty() {
            String::new()
        } else {
            format!(
                r#" <span style="color:rgba(255,255,255,0.78);font-weight:500;">{brand_suffix}</span>"#
            )
        },
    );

    out
}

fn render_block(block: &Block, theme: &Theme, out: &mut String) {
    match block {
        Block::Group(g) => render_group(g, theme, out),
        Block::SectionHeading(h) => {
            let _ = write!(
                out,
                r#"<h2 style="margin:30px 0 14px;font-size:18px;font-weight:700;color:{heading};letter-spacing:-0.01em;font-family:{FONT_SANS};">{}</h2>"#,
                esc(h),
                heading = theme.text_heading,
            );
        }
        Block::Paragraph(p) => {
            let _ = write!(
                out,
                r#"<p style="margin:0 0 18px;font-size:14.5px;line-height:1.65;color:{body};font-family:{FONT_SANS};">{}</p>"#,
                esc(p),
                body = theme.text_body,
            );
        }
        Block::Code(c) => render_code(c, theme, out),
        Block::Cta(c) => render_cta(c, theme, out),
    }
}

fn render_group(g: &GroupCard, theme: &Theme, out: &mut String) {
    let _ = write!(
        out,
        r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:{surface_raised};border:1px solid {hairline};border-radius:12px;border-left:3px solid {accent};margin:0 0 18px;">
  <tr><td style="padding:18px 20px 16px;">
    <div style="display:inline-block;font-size:10.5px;font-weight:700;letter-spacing:0.1em;text-transform:uppercase;color:{primary};background:{primary_tint};padding:4px 9px;border-radius:999px;margin:0 0 10px;font-family:{FONT_SANS};">{eyebrow}</div>
    <div style="font-size:18px;font-weight:700;color:{heading};letter-spacing:-0.01em;margin:0 0 6px;font-family:{FONT_SANS};">{title}</div>"#,
        eyebrow = esc(&g.eyebrow),
        title = esc(&g.title),
        accent = theme.brand_accent,
        primary = theme.brand_primary,
        primary_tint = tint(&theme.brand_primary, 0.08),
        heading = theme.text_heading,
        surface_raised = theme.surface_raised,
        hairline = theme.hairline,
    );
    if let Some(sub) = &g.subtitle {
        let _ = write!(
            out,
            r#"<div style="font-size:13px;color:{muted};margin:0 0 14px;line-height:1.55;font-family:{FONT_SANS};">{}</div>"#,
            esc(sub),
            muted = theme.text_muted,
        );
    } else {
        out.push_str(r#"<div style="margin:0 0 14px;"></div>"#);
    }

    match &g.body {
        GroupBody::Fields(fields) => {
            out.push_str(
                r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">"#,
            );
            for f in fields {
                render_field_row(f, theme, out);
            }
            out.push_str("</table>");
        }
        GroupBody::Records(records) => {
            for r in records {
                render_record_card(r, theme, out);
            }
        }
    }

    if let Some(how_to) = &g.how_to {
        let _ = write!(
            out,
            r#"<div style="margin-top:14px;padding:10px 12px;background:{tint_strong};border-radius:8px;border:1px solid {tint_border};font-size:12.5px;color:{primary_dark};line-height:1.55;font-family:{FONT_SANS};">{how_to}</div>"#,
            tint_strong = tint(&theme.brand_primary, 0.05),
            tint_border = tint(&theme.brand_primary, 0.20),
            primary_dark = darken(&theme.brand_primary, 0.20),
            // intentional: how_to allows trusted inline HTML for <code>
            how_to = how_to,
        );
    }

    out.push_str("</td></tr></table>");
}

fn render_field_row(f: &Field, theme: &Theme, out: &mut String) {
    let value_block = if f.mono {
        format!(
            r#"<div style="font-family:{FONT_MONO};font-size:12.5px;line-height:1.55;color:{heading};word-break:break-all;background:{surface};padding:8px 10px;border-radius:6px;border:1px solid {hairline};">{}</div>"#,
            esc(&f.value),
            heading = theme.text_heading,
            surface = theme.surface,
            hairline = theme.hairline,
        )
    } else {
        format!(
            r#"<div style="font-size:13px;color:{body};font-family:{FONT_SANS};">{}</div>"#,
            esc(&f.value),
            body = theme.text_body,
        )
    };
    let _ = write!(
        out,
        r#"<tr>
      <td style="padding:6px 0;width:30%;font-size:11px;color:{muted};font-weight:700;letter-spacing:0.06em;text-transform:uppercase;vertical-align:top;font-family:{FONT_SANS};">{label}</td>
      <td style="padding:6px 0 14px;">{value}</td>
    </tr>"#,
        label = esc(&f.label),
        value = value_block,
        muted = theme.text_muted,
    );
}

fn render_record_card(r: &RecordCard, theme: &Theme, out: &mut String) {
    let type_pill = match &r.type_tag {
        Some(t) => format!(
            r#"<span style="display:inline-block;font-size:10.5px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:{primary};background:{primary_tint};padding:3px 8px;border-radius:5px;margin-left:10px;font-family:{FONT_MONO};vertical-align:middle;">{}</span>"#,
            esc(t),
            primary = theme.brand_primary,
            primary_tint = tint(&theme.brand_primary, 0.10),
        ),
        None => String::new(),
    };
    let note_html = match &r.note {
        Some(n) => format!(
            r#"<div style="margin-top:8px;font-size:12px;color:{muted};line-height:1.55;font-family:{FONT_SANS};font-style:italic;">{}</div>"#,
            esc(n),
            muted = theme.text_muted,
        ),
        None => String::new(),
    };
    let _ = write!(
        out,
        r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:0 0 12px;background:{surface};border:1px solid {hairline};border-radius:10px;">
        <tr><td style="padding:12px 14px 14px;">
          <div style="font-size:10px;font-weight:700;letter-spacing:0.14em;text-transform:uppercase;color:{muted};margin:0 0 6px;font-family:{FONT_SANS};">{eyebrow}</div>
          <div style="font-size:14px;font-weight:700;color:{heading};font-family:{FONT_MONO};word-break:break-all;line-height:1.4;">
            {primary}{type_pill}
          </div>
          <div style="margin-top:8px;font-family:{FONT_MONO};font-size:12.5px;line-height:1.55;color:{heading};word-break:break-all;background:{surface_raised};padding:9px 11px;border-radius:6px;border:1px solid {hairline};">{value}</div>
          {note_html}
        </td></tr>
      </table>"#,
        eyebrow = esc(&r.eyebrow),
        primary = esc(&r.primary_label),
        value = esc(&r.value),
        muted = theme.text_muted,
        heading = theme.text_heading,
        surface = theme.surface,
        surface_raised = theme.surface_raised,
        hairline = theme.hairline,
    );
}

fn render_code(c: &CodeBlock, theme: &Theme, out: &mut String) {
    if let Some(eyebrow) = &c.eyebrow {
        let _ = write!(
            out,
            r#"<h2 style="margin:24px 0 10px;font-size:14px;font-weight:700;color:{heading};letter-spacing:0.04em;text-transform:uppercase;font-family:{FONT_SANS};">{}</h2>"#,
            esc(eyebrow),
            heading = theme.text_heading,
        );
    }
    let _ = write!(
        out,
        r#"<div style="font-family:{FONT_MONO};font-size:12.5px;line-height:1.7;color:{heading};background:{surface};padding:14px 16px;border-radius:8px;border:1px solid {hairline};margin:0 0 18px;white-space:pre-wrap;">"#,
        heading = theme.text_heading,
        surface = theme.surface,
        hairline = theme.hairline,
    );
    let mut first = true;
    for line in &c.lines {
        if !first {
            out.push_str("<br>");
        }
        first = false;
        out.push_str(&esc(line));
    }
    out.push_str("</div>");
}

fn render_cta(c: &Cta, theme: &Theme, out: &mut String) {
    let _ = write!(
        out,
        r#"<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:8px 0 18px;">
  <tr><td>
    <a href="{href}" style="display:inline-block;padding:13px 28px;background:{primary};color:#ffffff;font-size:14.5px;font-weight:600;text-decoration:none;border-radius:10px;letter-spacing:0.01em;box-shadow:0 1px 2px {primary_shadow_a},0 4px 14px {primary_shadow_b};font-family:{FONT_SANS};">{label}</a>
  </td></tr>
</table>"#,
        href = esc(&c.href),
        label = esc(&c.label),
        primary = theme.brand_primary,
        primary_shadow_a = rgba_from_hex(&theme.brand_primary, 0.25),
        primary_shadow_b = rgba_from_hex(&theme.brand_primary, 0.18),
    );
}

/// Convert a `#rrggbb` hex token to `rgba(r,g,b,alpha)` for tints.
/// On parse failure, returns the input verbatim — better a slightly
/// wrong color than a panic in a renderer that runs in production.
fn rgba_from_hex(hex: &str, alpha: f32) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return hex.to_string();
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    format!("rgba({r},{g},{b},{alpha:.2})")
}

/// Same as [`rgba_from_hex`] but typed as a "tint" — call site reads
/// clearer when the value is meant to be a low-opacity wash.
fn tint(hex: &str, alpha: f32) -> String {
    rgba_from_hex(hex, alpha)
}

/// Darken a hex color by mixing toward black. Used for "how-to"
/// callouts where we want the text to be readable on a tinted bg.
/// `factor` of 0.0 returns the original, 1.0 returns black.
fn darken(hex: &str, factor: f32) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return hex.to_string();
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    let mix = |c: u8| ((f32::from(c) * (1.0 - factor)).clamp(0.0, 255.0)) as u8;
    format!("#{:02x}{:02x}{:02x}", mix(r), mix(g), mix(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_handles_well_formed_hex() {
        assert_eq!(rgba_from_hex("#0d488a", 0.5), "rgba(13,72,138,0.50)");
        assert_eq!(rgba_from_hex("0d488a", 0.10), "rgba(13,72,138,0.10)");
    }

    #[test]
    fn rgba_passes_through_malformed_hex() {
        assert_eq!(rgba_from_hex("not-a-color", 0.5), "not-a-color");
    }

    #[test]
    fn darken_moves_toward_black() {
        let d = darken("#ff0000", 0.5);
        assert_eq!(d, "#7f0000");
    }
}
