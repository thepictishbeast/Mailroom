//! HTML renderer. Walks an [`EmailDocument`] and emits a complete
//! `text/html` body. Every output byte is hand-written; no template
//! engine, no external assets, no `<style>` blocks.

use std::fmt::Write as _;

use crate::escape::html as esc;
use crate::tokens::{
    BRAND_ACCENT, BRAND_NAVY, BRAND_PRIMARY, FONT_MONO, FONT_SANS, HAIRLINE, SURFACE,
    SURFACE_RAISED, TEXT_BODY, TEXT_HEADING, TEXT_MUTED,
};
use crate::{Block, CodeBlock, Cta, EmailDocument, Field, GroupBody, GroupCard, RecordCard};

pub(crate) fn render(doc: &EmailDocument) -> String {
    let title = esc(&doc.subject);
    let preheader = esc(&doc.preheader);
    let mut inner = String::new();

    if let Some(eyebrow) = &doc.eyebrow {
        let _ = write!(
            inner,
            r#"<div style="display:inline-block;font-size:11px;font-weight:600;letter-spacing:0.1em;text-transform:uppercase;color:{BRAND_PRIMARY};background:rgba(13,72,138,0.08);padding:5px 10px;border-radius:999px;margin:0 0 14px;">{}</div>"#,
            esc(eyebrow)
        );
    }
    let _ = write!(
        inner,
        r#"<h1 style="margin:0 0 12px;font-size:26px;line-height:1.25;color:{TEXT_HEADING};font-weight:700;letter-spacing:-0.02em;font-family:{FONT_SANS};">{}</h1>"#,
        esc(&doc.heading)
    );
    if let Some(intro) = &doc.intro {
        let _ = write!(
            inner,
            r#"<p style="margin:0 0 24px;font-size:14.5px;line-height:1.65;color:{TEXT_BODY};font-family:{FONT_SANS};">{}</p>"#,
            esc(intro)
        );
    }

    for block in &doc.blocks {
        render_block(block, &mut inner);
    }

    if !doc.footer_lines.is_empty() {
        inner.push_str(r#"<div style="margin:28px 0 0;padding-top:20px;border-top:1px solid "#);
        inner.push_str(HAIRLINE);
        inner.push_str(r#";">"#);
        for line in &doc.footer_lines {
            let _ = write!(
                inner,
                r#"<p style="margin:0 0 6px;font-size:13px;line-height:1.6;color:{TEXT_MUTED};font-family:{FONT_SANS};">{}</p>"#,
                esc(line)
            );
        }
        inner.push_str("</div>");
    }

    let mut out = String::with_capacity(8192);
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
<body style="margin:0;padding:0;background:{SURFACE};font-family:{FONT_SANS};color:{TEXT_BODY};-webkit-font-smoothing:antialiased;line-height:1.5;">
<div style="display:none;max-height:0;overflow:hidden;mso-hide:all;font-size:1px;color:{SURFACE};">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:{SURFACE};">
  <tr><td align="center" style="padding:40px 16px;">
    <table role="presentation" width="640" cellpadding="0" cellspacing="0" border="0" style="max-width:640px;background:{SURFACE_RAISED};border:1px solid {HAIRLINE};border-radius:14px;overflow:hidden;box-shadow:0 1px 3px rgba(15,23,42,0.04),0 8px 24px rgba(15,23,42,0.06);">
      <tr><td style="background:linear-gradient(135deg,{BRAND_NAVY} 0%,{BRAND_PRIMARY} 60%,{BRAND_ACCENT} 100%);padding:28px 36px;color:#ffffff;">
        <table cellpadding="0" cellspacing="0" border="0">
          <tr>
            <td style="vertical-align:middle;padding-right:14px;">
              <span style="display:inline-block;width:36px;height:36px;background:rgba(255,255,255,0.18);border:1px solid rgba(255,255,255,0.35);border-radius:9px;text-align:center;line-height:36px;font-size:16px;font-weight:800;color:#ffffff;letter-spacing:-0.02em;font-family:{FONT_SANS};">P</span>
            </td>
            <td style="vertical-align:middle;font-family:{FONT_SANS};">
              <div style="font-size:18px;font-weight:700;letter-spacing:-0.01em;color:#ffffff;">PlausiDen <span style="color:rgba(255,255,255,0.78);font-weight:500;">LLC</span></div>
              <div style="font-size:12px;font-weight:500;color:rgba(255,255,255,0.72);letter-spacing:0.04em;text-transform:uppercase;margin-top:2px;">Plausible. Defensible.</div>
            </td>
          </tr>
        </table>
      </td></tr>
      <tr><td style="padding:32px 36px 28px;">
        {inner}
      </td></tr>
      <tr><td style="background:{SURFACE};padding:22px 36px;border-top:1px solid {HAIRLINE};">
        <p style="margin:0;font-size:12px;line-height:1.7;color:{TEXT_MUTED};font-family:{FONT_SANS};">
          <strong style="color:{TEXT_BODY};font-weight:600;">PlausiDen LLC</strong> &nbsp;·&nbsp; Massachusetts, USA<br>
          <a href="https://plausiden.com" style="color:{BRAND_PRIMARY};text-decoration:none;font-weight:600;">plausiden.com</a> &nbsp;·&nbsp; <a href="mailto:team@plausiden.com" style="color:{BRAND_PRIMARY};text-decoration:none;font-weight:600;">team@plausiden.com</a>
        </p>
      </td></tr>
    </table>
    <p style="margin:16px 0 0;font-size:11px;color:{TEXT_MUTED};text-align:center;letter-spacing:0.02em;font-family:{FONT_SANS};">
      Automated message — replies go to a real human.
    </p>
  </td></tr>
</table>
</body>
</html>
"#
    );

    out
}

fn render_block(block: &Block, out: &mut String) {
    match block {
        Block::Group(g) => render_group(g, out),
        Block::SectionHeading(h) => {
            let _ = write!(
                out,
                r#"<h2 style="margin:30px 0 14px;font-size:18px;font-weight:700;color:{TEXT_HEADING};letter-spacing:-0.01em;font-family:{FONT_SANS};">{}</h2>"#,
                esc(h)
            );
        }
        Block::Paragraph(p) => {
            let _ = write!(
                out,
                r#"<p style="margin:0 0 18px;font-size:14.5px;line-height:1.65;color:{TEXT_BODY};font-family:{FONT_SANS};">{}</p>"#,
                esc(p)
            );
        }
        Block::Code(c) => render_code(c, out),
        Block::Cta(c) => render_cta(c, out),
    }
}

fn render_group(g: &GroupCard, out: &mut String) {
    let _ = write!(
        out,
        r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:{SURFACE_RAISED};border:1px solid {HAIRLINE};border-radius:12px;border-left:3px solid {BRAND_ACCENT};margin:0 0 18px;">
  <tr><td style="padding:18px 20px 16px;">
    <div style="display:inline-block;font-size:10.5px;font-weight:700;letter-spacing:0.1em;text-transform:uppercase;color:{BRAND_PRIMARY};background:rgba(13,72,138,0.08);padding:4px 9px;border-radius:999px;margin:0 0 10px;font-family:{FONT_SANS};">{eyebrow}</div>
    <div style="font-size:18px;font-weight:700;color:{TEXT_HEADING};letter-spacing:-0.01em;margin:0 0 6px;font-family:{FONT_SANS};">{title}</div>"#,
        eyebrow = esc(&g.eyebrow),
        title = esc(&g.title),
    );
    if let Some(sub) = &g.subtitle {
        let _ = write!(
            out,
            r#"<div style="font-size:13px;color:{TEXT_MUTED};margin:0 0 14px;line-height:1.55;font-family:{FONT_SANS};">{}</div>"#,
            esc(sub)
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
                render_field_row(f, out);
            }
            out.push_str("</table>");
        }
        GroupBody::Records(records) => {
            for r in records {
                render_record_card(r, out);
            }
        }
    }

    if let Some(how_to) = &g.how_to {
        let _ = write!(
            out,
            r#"<div style="margin-top:14px;padding:10px 12px;background:#f0f9ff;border-radius:8px;border:1px solid #bae6fd;font-size:12.5px;color:#075985;line-height:1.55;font-family:{FONT_SANS};">{how_to}</div>"#,
            // intentional: how_to allows trusted inline HTML for inline <code>
            how_to = how_to
        );
    }

    out.push_str("</td></tr></table>");
}

fn render_field_row(f: &Field, out: &mut String) {
    let value_block = if f.mono {
        format!(
            r#"<div style="font-family:{FONT_MONO};font-size:12.5px;line-height:1.55;color:{TEXT_HEADING};word-break:break-all;background:{SURFACE};padding:8px 10px;border-radius:6px;border:1px solid {HAIRLINE};">{}</div>"#,
            esc(&f.value)
        )
    } else {
        format!(
            r#"<div style="font-size:13px;color:{TEXT_BODY};font-family:{FONT_SANS};">{}</div>"#,
            esc(&f.value)
        )
    };
    let _ = write!(
        out,
        r#"<tr>
      <td style="padding:6px 0;width:30%;font-size:11px;color:{TEXT_MUTED};font-weight:700;letter-spacing:0.06em;text-transform:uppercase;vertical-align:top;font-family:{FONT_SANS};">{label}</td>
      <td style="padding:6px 0 14px;">{value}</td>
    </tr>"#,
        label = esc(&f.label),
        value = value_block
    );
}

fn render_record_card(r: &RecordCard, out: &mut String) {
    let type_pill = match &r.type_tag {
        Some(t) => format!(
            r#"<span style="display:inline-block;font-size:10.5px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:{BRAND_PRIMARY};background:rgba(13,72,138,0.1);padding:3px 8px;border-radius:5px;margin-left:10px;font-family:{FONT_MONO};vertical-align:middle;">{}</span>"#,
            esc(t)
        ),
        None => String::new(),
    };
    let note_html = match &r.note {
        Some(n) => format!(
            r#"<div style="margin-top:8px;font-size:12px;color:{TEXT_MUTED};line-height:1.55;font-family:{FONT_SANS};font-style:italic;">{}</div>"#,
            esc(n)
        ),
        None => String::new(),
    };
    let _ = write!(
        out,
        r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:0 0 12px;background:{SURFACE};border:1px solid {HAIRLINE};border-radius:10px;">
        <tr><td style="padding:12px 14px 14px;">
          <div style="font-size:10px;font-weight:700;letter-spacing:0.14em;text-transform:uppercase;color:{TEXT_MUTED};margin:0 0 6px;font-family:{FONT_SANS};">{eyebrow}</div>
          <div style="font-size:14px;font-weight:700;color:{TEXT_HEADING};font-family:{FONT_MONO};word-break:break-all;line-height:1.4;">
            {primary}{type_pill}
          </div>
          <div style="margin-top:8px;font-family:{FONT_MONO};font-size:12.5px;line-height:1.55;color:{TEXT_HEADING};word-break:break-all;background:{SURFACE_RAISED};padding:9px 11px;border-radius:6px;border:1px solid {HAIRLINE};">{value}</div>
          {note_html}
        </td></tr>
      </table>"#,
        eyebrow = esc(&r.eyebrow),
        primary = esc(&r.primary_label),
        value = esc(&r.value),
    );
}

fn render_code(c: &CodeBlock, out: &mut String) {
    if let Some(eyebrow) = &c.eyebrow {
        let _ = write!(
            out,
            r#"<h2 style="margin:24px 0 10px;font-size:14px;font-weight:700;color:{TEXT_HEADING};letter-spacing:0.04em;text-transform:uppercase;font-family:{FONT_SANS};">{}</h2>"#,
            esc(eyebrow)
        );
    }
    out.push_str(&format!(
        r#"<div style="font-family:{FONT_MONO};font-size:12.5px;line-height:1.7;color:{TEXT_HEADING};background:{SURFACE};padding:14px 16px;border-radius:8px;border:1px solid {HAIRLINE};margin:0 0 18px;white-space:pre-wrap;">"#
    ));
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

fn render_cta(c: &Cta, out: &mut String) {
    let _ = write!(
        out,
        r#"<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:8px 0 18px;">
  <tr><td>
    <a href="{href}" style="display:inline-block;padding:13px 28px;background:{BRAND_PRIMARY};color:#ffffff;font-size:14.5px;font-weight:600;text-decoration:none;border-radius:10px;letter-spacing:0.01em;box-shadow:0 1px 2px rgba(13,72,138,0.25),0 4px 14px rgba(13,72,138,0.18);font-family:{FONT_SANS};">{label}</a>
  </td></tr>
</table>"#,
        href = esc(&c.href),
        label = esc(&c.label),
    );
}
