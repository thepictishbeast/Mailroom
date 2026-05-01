//! `bounce-pretty` — content-filter binary that wraps Postfix bounce
//! messages in branded HTML.
//!
//! ## Pipeline
//!
//! 1. Postfix's cleanup daemon emits a multipart/report bounce when
//!    a delivery fails (failure / delay / etc).
//! 2. A non-smtpd `header_checks` rule matches the bounce's
//!    `From: MAILER-DAEMON@…` and routes it through this filter via
//!    `FILTER bounce_pretty:`.
//! 3. This binary reads the message from stdin, parses out
//!    failed_recipient + diagnostic + original_subject, generates
//!    an HTML alternative via `mail_templates::prebuilt::bounce`,
//!    and re-emits a new `multipart/alternative` body that holds
//!    both the original plain text and the new HTML side-by-side.
//! 4. The original `message/delivery-status` and
//!    `message/rfc822` parts of the bounce are preserved
//!    structurally — RFC 3464 receivers can still parse the DSN.
//! 5. The rewritten message is written to stdout. Postfix's pipe
//!    transport re-injects it via `sendmail`.
//!
//! ## Failure mode
//!
//! If parsing fails or the input doesn't look like a bounce, the
//! binary writes the input through unchanged and exits 0. We never
//! want a broken filter to block bounce delivery — better to ship
//! the original than nothing.

use mail_parser::{HeaderValue, MessageParser};
use mail_templates::prebuilt::{bounce, BounceReason};
use std::io::{Read, Write};

fn main() {
    let mut buf = Vec::with_capacity(8192);
    if std::io::stdin().read_to_end(&mut buf).is_err() {
        std::process::exit(0);
    }

    let rewritten = rewrite_bounce(&buf).unwrap_or_else(|| buf.clone());

    if std::io::stdout().write_all(&rewritten).is_err() {
        std::process::exit(0);
    }
}

/// Try to parse `raw` as a multipart/report bounce and rewrite it.
/// Returns `None` when the input doesn't look like a recognized
/// bounce; the caller passes through unchanged.
fn rewrite_bounce(raw: &[u8]) -> Option<Vec<u8>> {
    let parser = MessageParser::default();
    let msg = parser.parse(raw)?;

    let ct = top_level_content_type(&msg)?;
    if !ct.to_ascii_lowercase().contains("multipart/report") {
        return None;
    }

    // Walk parts: find message/delivery-status and message/rfc822.
    let mut diagnostic: Option<String> = None;
    let mut status: Option<String> = None;
    let mut failed_recipient: Option<String> = None;
    let mut original_subject: Option<String> = None;
    let mut original_from: Option<String> = None;

    let mut delivery_status_block: Option<Vec<u8>> = None;
    let mut rfc822_block: Option<Vec<u8>> = None;

    for part in msg.parts.iter() {
        let part_ct = part_content_type(part).unwrap_or_default().to_ascii_lowercase();

        let body = part_body_bytes(raw, part);

        if part_ct.contains("message/delivery-status") {
            delivery_status_block = Some(body.to_vec());
            for (name, value) in unfold_header_lines(body) {
                if name.eq_ignore_ascii_case("Diagnostic-Code") {
                    diagnostic = Some(value);
                } else if name.eq_ignore_ascii_case("Status") {
                    status = Some(value);
                } else if name.eq_ignore_ascii_case("Final-Recipient") {
                    if let Some(addr) = value.split(';').nth(1) {
                        failed_recipient = Some(addr.trim().to_string());
                    }
                }
            }
        } else if part_ct.contains("message/rfc822") || part_ct.contains("text/rfc822-headers") {
            if part_ct.contains("message/rfc822") {
                rfc822_block = Some(body.to_vec());
            }
            for (name, value) in unfold_header_lines(body) {
                if name.eq_ignore_ascii_case("Subject") {
                    original_subject = Some(value);
                } else if name.eq_ignore_ascii_case("From") {
                    original_from = Some(value);
                }
            }
        }
    }

    let failed_recipient = failed_recipient.unwrap_or_else(|| "(unknown)".into());
    let diagnostic = diagnostic.unwrap_or_else(|| "(no SMTP diagnostic)".into());
    let original_subject = original_subject.unwrap_or_else(|| "(no subject)".into());

    let reason = classify_status(status.as_deref().unwrap_or(""), &diagnostic);

    let to = original_from.unwrap_or_else(|| "(unknown)".into());
    let doc = bounce(
        &to,
        &failed_recipient,
        reason,
        &diagnostic,
        &original_subject,
    );

    let html = doc.render_html();
    let plain = doc.render_plain();

    Some(emit_multipart_report(
        raw,
        &msg,
        &plain,
        &html,
        delivery_status_block.as_deref(),
        rfc822_block.as_deref(),
    ))
}

fn top_level_content_type(msg: &mail_parser::Message<'_>) -> Option<String> {
    for h in msg.headers() {
        if h.name().eq_ignore_ascii_case("Content-Type") {
            if let HeaderValue::Text(v) = h.value() {
                return Some(v.to_string());
            }
            if let HeaderValue::ContentType(ct) = h.value() {
                return Some(format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")));
            }
        }
    }
    None
}

fn part_content_type(part: &mail_parser::MessagePart<'_>) -> Option<String> {
    for h in part.headers.iter() {
        if h.name().eq_ignore_ascii_case("Content-Type") {
            if let HeaderValue::Text(v) = h.value() {
                return Some(v.to_string());
            }
            if let HeaderValue::ContentType(ct) = h.value() {
                return Some(format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")));
            }
        }
    }
    None
}

fn part_body_bytes<'a>(raw: &'a [u8], part: &mail_parser::MessagePart<'_>) -> &'a [u8] {
    let start = part.raw_body_offset();
    let end = part.raw_end_offset();
    if end <= raw.len() && start <= end {
        &raw[start..end]
    } else {
        &[]
    }
}

fn classify_status(status: &str, diagnostic: &str) -> BounceReason {
    let trimmed = status.trim();
    if trimmed.starts_with("4.") {
        return BounceReason::Temporary;
    }
    if trimmed.starts_with("5.1.1") || diagnostic.contains("5.1.1") {
        return BounceReason::UserUnknown;
    }
    if trimmed.starts_with("5.7") || diagnostic.contains("5.7.") {
        return BounceReason::PolicyReject;
    }
    BounceReason::Other
}

/// Case-insensitive `str::strip_prefix`.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// RFC 5322 §2.2.3 header unfolding, plus RFC 3464-friendly
/// "blank lines separate blocks but don't end the header set"
/// behavior. Iterates a bytes block as (header-name, header-value)
/// pairs.
///
/// Continuation lines (those starting with whitespace) join to the
/// preceding header, separated by a single space.
///
/// Blank lines do NOT terminate parsing here — RFC 3464
/// `message/delivery-status` content is structured as multiple
/// header-like blocks (per-message + per-recipient), separated by
/// blank lines but with all of them being properties we want.
/// Callers that need a stop-at-blank semantic should slice the
/// input first.
///
/// `max_lines` caps work to keep pathological input from running
/// away; pass a generous value (>=200) for normal use.
fn unfold_header_lines(body: &[u8]) -> Vec<(String, String)> {
    let s = String::from_utf8_lossy(body);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in s.lines().take(200) {
        if line.is_empty() {
            // Block separator — flush current; keep iterating.
            if let Some(prev) = current.take() {
                out.push(prev);
            }
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation — append (with single space) to previous.
            if let Some((_, v)) = current.as_mut() {
                v.push(' ');
                v.push_str(line.trim_start());
            }
            continue;
        }
        if let Some(prev) = current.take() {
            out.push(prev);
        }
        if let Some(colon) = line.find(':') {
            let name = line[..colon].trim().to_string();
            let value = line[colon + 1..].trim().to_string();
            current = Some((name, value));
        }
    }
    if let Some(prev) = current {
        out.push(prev);
    }
    out
}

/// Emit a new multipart message that wraps the original bounce body
/// in a multipart/alternative (the new HTML alongside the new
/// branded plain text), preserving the report-type and the
/// delivery-status / rfc822 sub-parts so RFC 3464 readers still
/// work.
fn emit_multipart_report(
    raw: &[u8],
    msg: &mail_parser::Message<'_>,
    plain: &str,
    html: &str,
    delivery_status: Option<&[u8]>,
    rfc822: Option<&[u8]>,
) -> Vec<u8> {
    let alt_boundary = format!("alt-{}", random_token());
    let outer_boundary = format!("rep-{}", random_token());

    let mut out: Vec<u8> = Vec::with_capacity(raw.len() + html.len() + 1024);

    // Pass through every original top-level header verbatim (raw
    // bytes from the source message) except Content-Type, MIME-Version,
    // and Content-Transfer-Encoding — we own those because the body
    // shape changes. headers_raw() preserves Address / DateTime /
    // structured-value headers that wouldn't survive a `HeaderValue::Text`
    // match alone.
    for (name, value) in msg.headers_raw() {
        let trimmed_name = name.trim_end_matches(':').trim();
        if trimmed_name.eq_ignore_ascii_case("Content-Type")
            || trimmed_name.eq_ignore_ascii_case("Content-Transfer-Encoding")
            || trimmed_name.eq_ignore_ascii_case("MIME-Version")
        {
            continue;
        }
        // header_raw returns the value with the trailing CRLF; strip
        // and re-add a clean LF for line-ending consistency.
        let _ = writeln!(out, "{trimmed_name}:{}", value.trim_end_matches(['\r', '\n']));
    }
    let _ = writeln!(
        out,
        "Content-Type: multipart/report; report-type=delivery-status;\r\n boundary=\"{outer_boundary}\""
    );
    let _ = writeln!(out, "MIME-Version: 1.0");
    let _ = writeln!(out);

    // Part 1 — multipart/alternative (plain + html)
    let _ = writeln!(out, "--{outer_boundary}");
    let _ = writeln!(
        out,
        "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\""
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "--{alt_boundary}");
    let _ = writeln!(out, "Content-Type: text/plain; charset=\"utf-8\"");
    let _ = writeln!(out, "Content-Transfer-Encoding: 8bit");
    let _ = writeln!(out);
    let _ = writeln!(out, "{plain}");

    let _ = writeln!(out, "--{alt_boundary}");
    let _ = writeln!(out, "Content-Type: text/html; charset=\"utf-8\"");
    let _ = writeln!(out, "Content-Transfer-Encoding: 8bit");
    let _ = writeln!(out);
    let _ = writeln!(out, "{html}");

    let _ = writeln!(out, "--{alt_boundary}--");

    // Part 2 — message/delivery-status (preserved)
    if let Some(ds) = delivery_status {
        let _ = writeln!(out, "--{outer_boundary}");
        let _ = writeln!(out, "Content-Type: message/delivery-status");
        let _ = writeln!(out);
        out.extend_from_slice(ds);
        if !ds.ends_with(b"\n") {
            let _ = writeln!(out);
        }
    }

    // Part 3 — message/rfc822 (preserved)
    if let Some(orig) = rfc822 {
        let _ = writeln!(out, "--{outer_boundary}");
        let _ = writeln!(out, "Content-Type: message/rfc822");
        let _ = writeln!(out);
        out.extend_from_slice(orig);
        if !orig.ends_with(b"\n") {
            let _ = writeln!(out);
        }
    }

    let _ = writeln!(out, "--{outer_boundary}--");

    out
}

/// 16-char hex token for MIME boundaries — pid + nanos. Unique
/// enough for the use case; the boundary only needs to not appear
/// in the body.
fn random_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{nanos:08x}{pid:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BOUNCE: &str = "Return-Path: <>\r
From: postmaster@plausiden.com\r
To: william@plausiden.com\r
Subject: Mail couldn't be delivered\r
Date: Fri, 01 May 2026 15:13:26 +0000 (UTC)\r
MIME-Version: 1.0\r
Auto-Submitted: auto-replied\r
Content-Type: multipart/report; report-type=delivery-status;\r
\tboundary=\"BOUNDARY1\"\r
\r
--BOUNDARY1\r
Content-Type: text/plain; charset=\"us-ascii\"\r
\r
This is the mail system. Could not deliver your message.\r
\r
--BOUNDARY1\r
Content-Type: message/delivery-status\r
\r
Reporting-MTA: dns; mail.plausiden.com\r
\r
Final-Recipient: rfc822; nobody@plausiden.com\r
Action: failed\r
Status: 5.1.1\r
Diagnostic-Code: smtp; 550 5.1.1 user unknown\r
\r
--BOUNDARY1\r
Content-Type: message/rfc822\r
\r
From: william@plausiden.com\r
To: nobody@plausiden.com\r
Subject: hi there\r
\r
the body of the original\r
\r
--BOUNDARY1--\r
";

    #[test]
    fn rewrites_recognized_bounce() {
        let result = rewrite_bounce(SAMPLE_BOUNCE.as_bytes());
        assert!(
            result.is_some(),
            "expected the sample bounce to be recognized"
        );
        let out = String::from_utf8(result.unwrap()).unwrap();

        // New chrome inserted
        assert!(out.contains("multipart/alternative"));
        assert!(out.contains("text/html"));
        assert!(out.contains("<!DOCTYPE html>"));
        // Failed recipient extracted
        assert!(out.contains("nobody@plausiden.com"));
        // Diagnostic surfaced in HTML body
        assert!(out.contains("550 5.1.1 user unknown"));
        // Original subject surfaced
        assert!(out.contains("hi there"));
        // RFC 3464 sub-parts preserved
        assert!(out.contains("Final-Recipient: rfc822; nobody@plausiden.com"));
    }

    #[test]
    fn passes_through_non_bounce() {
        let plain = b"Subject: just a message\r\n\r\nbody";
        assert!(rewrite_bounce(plain).is_none());
    }

    #[test]
    fn classify_status_branches() {
        assert!(matches!(
            classify_status("4.4.1", ""),
            BounceReason::Temporary
        ));
        assert!(matches!(
            classify_status("5.1.1", ""),
            BounceReason::UserUnknown
        ));
        assert!(matches!(
            classify_status("5.7.1", ""),
            BounceReason::PolicyReject
        ));
        assert!(matches!(classify_status("5.0.0", ""), BounceReason::Other));
        // Diagnostic-only fallback
        assert!(matches!(
            classify_status("", "550 5.1.1 unknown"),
            BounceReason::UserUnknown
        ));
        assert!(matches!(
            classify_status("", "554 5.7.26"),
            BounceReason::PolicyReject
        ));
    }

    #[test]
    fn strip_prefix_ci_works() {
        assert_eq!(strip_prefix_ci("Status: 5.1.1", "status:"), Some(" 5.1.1"));
        assert_eq!(strip_prefix_ci("STATUS: x", "Status:"), Some(" x"));
        assert_eq!(strip_prefix_ci("nope", "Status:"), None);
    }

    #[test]
    fn unfold_joins_continuation_lines() {
        let raw = b"Diagnostic-Code: smtp; 550 5.1.1 <nobody@x> User\r\n  doesn't exist: nobody@x\r\nStatus: 5.1.1\r\n\r\nbody\r\n";
        let pairs = unfold_header_lines(raw);
        let diag = pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Diagnostic-Code"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(
            diag.contains("User doesn't exist: nobody@x"),
            "unfold should join multi-line diagnostic: {diag:?}"
        );
        let status = pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Status"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(status, "5.1.1");
    }

    #[test]
    fn rewrites_multi_line_diagnostic() {
        // Same as SAMPLE_BOUNCE but with Diagnostic-Code spanning two lines.
        let raw = SAMPLE_BOUNCE.replace(
            "Diagnostic-Code: smtp; 550 5.1.1 user unknown",
            "Diagnostic-Code: smtp; 550 5.1.1 user\r\n  unknown — try a different address",
        );
        let result = rewrite_bounce(raw.as_bytes());
        assert!(result.is_some());
        let out = String::from_utf8(result.unwrap()).unwrap();
        // Both lines of the diagnostic survive into the rendered body.
        assert!(
            out.contains("user unknown — try a different address"),
            "multi-line diagnostic was truncated"
        );
    }
}
