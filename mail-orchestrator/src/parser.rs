//! RFC 5322 email parser for reading Maildir files.
//!
//! Extracts headers, body text, and attachment info from raw email files
//! delivered to Maildir/new/ directories.

use anyhow::{Context, Result};
use mail_parser::{MessageParser, MimeHeaders};
use std::path::Path;

/// Parsed email with extracted fields.
#[derive(Debug, Clone)]
pub struct ParsedEmail {
    pub message_id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub body_text: String,
    pub body_preview: String,
    pub has_attachments: bool,
    pub attachment_names: Vec<String>,
    pub raw_path: String,
}

/// Parse an email file from a Maildir/new/ directory.
pub fn parse_email(path: &Path) -> Result<ParsedEmail> {
    let raw = std::fs::read(path)
        .with_context(|| format!("Failed to read email file: {}", path.display()))?;

    let message = MessageParser::default()
        .parse(&raw)
        .context("Failed to parse email")?;

    let message_id = message
        .message_id()
        .unwrap_or("unknown")
        .to_string();

    let from = message
        .from()
        .and_then(|addrs| addrs.first())
        .map(|a| {
            if let Some(name) = a.name() {
                format!("{} <{}>", name, a.address().unwrap_or_default())
            } else {
                a.address().unwrap_or_default().to_string()
            }
        })
        .unwrap_or_default();

    let to = message
        .to()
        .and_then(|addrs| addrs.first())
        .and_then(|a| a.address())
        .unwrap_or_default()
        .to_string();

    let subject = message
        .subject()
        .unwrap_or("(no subject)")
        .to_string();

    let date = message
        .date()
        .map(|d| d.to_rfc3339())
        .unwrap_or_default();

    let body_text = message
        .body_text(0)
        .unwrap_or_default()
        .to_string();

    let body_preview = body_text
        .chars()
        .take(200)
        .collect::<String>();

    let mut attachment_names = Vec::new();
    let has_attachments = message.attachment_count() > 0;
    for i in 0..message.attachment_count() {
        if let Some(part) = message.attachment(i) {
            let name = part
                .attachment_name()
                .unwrap_or("unnamed")
                .to_string();
            attachment_names.push(name);
        }
    }

    Ok(ParsedEmail {
        message_id,
        from,
        to,
        subject,
        date,
        body_text,
        body_preview,
        has_attachments,
        attachment_names,
        raw_path: path.display().to_string(),
    })
}

/// Extract just the email address from a "Name <addr>" string.
pub fn extract_address(addr_str: &str) -> &str {
    if let Some(start) = addr_str.find('<') {
        if let Some(end) = addr_str.find('>') {
            return &addr_str[start + 1..end];
        }
    }
    addr_str.trim()
}
