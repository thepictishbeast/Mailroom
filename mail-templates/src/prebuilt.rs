//! Pre-built [`EmailDocument`] builders for the recurring email
//! shapes we send: DNS records dispatches, NDR / bounce notices,
//! magic-link sign-ins, feedback / inquiry notifications.
//!
//! Each builder takes typed inputs and produces a fully-populated
//! [`crate::EmailDocument`] ready to render. Call sites stay
//! grep-clear: `prebuilt::dns_records(...)` is unambiguous.

use crate::{
    Block, CodeBlock, Cta, EmailDocument, Field, GroupBody, GroupCard, RecordCard,
};

/// One DNS record group (e.g., "outreach.plausiden.com — Salesman
/// Path B"). Renders as a [`GroupCard`] with each record as a
/// nested [`RecordCard`] for maximum visual separation.
#[derive(Debug, Clone)]
pub struct DnsGroup {
    /// Pill eyebrow (e.g., "Group 1 · Salesman").
    pub eyebrow: String,
    /// Group title (e.g., "outreach.plausiden.com sender domain").
    pub title: String,
    /// Optional descriptive sub-line.
    pub subtitle: Option<String>,
    /// Records in this group.
    pub records: Vec<DnsRecord>,
    /// Optional concise instruction for after publishing.
    pub how_to: Option<String>,
}

/// One DNS record line.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    /// `Host` value (e.g., `outreach`).
    pub host: String,
    /// Record type (`A`, `AAAA`, `TXT`, `CNAME`, …).
    pub record_type: String,
    /// Record value.
    pub value: String,
    /// Optional note rendered beneath the value (e.g., "do not split
    /// across multiple TXT chunks — one continuous string").
    pub note: Option<String>,
}

/// Build the "DNS records to publish" email used for outreach /
/// MTA-STS / TLS-RPT / cms-subdomain dispatches.
#[must_use]
pub fn dns_records(groups: Vec<DnsGroup>, dig_commands: Vec<String>) -> EmailDocument {
    let mut blocks: Vec<Block> = Vec::new();

    for g in &groups {
        let total = g.records.len();
        let records = g
            .records
            .iter()
            .enumerate()
            .map(|(i, r)| RecordCard {
                eyebrow: format!("Record {} of {}", i + 1, total),
                primary_label: r.host.clone(),
                type_tag: Some(r.record_type.clone()),
                value: r.value.clone(),
                note: r.note.clone(),
            })
            .collect();
        blocks.push(Block::Group(GroupCard {
            eyebrow: g.eyebrow.clone(),
            title: g.title.clone(),
            subtitle: g.subtitle.clone(),
            body: GroupBody::Records { records },
            how_to: g.how_to.clone(),
        }));
    }

    if !dig_commands.is_empty() {
        blocks.push(Block::Code(CodeBlock {
            eyebrow: Some("Verification".into()),
            lines: dig_commands,
        }));
        blocks.push(Block::Paragraph {
            text: "Each command should print at least one line once propagated \
                   (typically 5–30 min at the registrar). Empty output = not \
                   yet visible."
                .into(),
        });
    }

    EmailDocument {
        subject: "DNS records to publish — plausiden.com".into(),
        preheader: "Hostnames, types, values — paste at your registrar.".into(),
        eyebrow: Some("Pending DNS · plausiden.com".into()),
        heading: "Records to add at your registrar".into(),
        intro: Some(
            "Records are grouped by purpose; you can publish a group at a \
             time. Each card below is a single record — paste host, type, \
             and value verbatim."
                .into(),
        ),
        blocks,
        footer_lines: vec![
            "Source of truth: https://github.com/thepictishbeast/PlausiDen-Email-Config/blob/main/docs/DNS-RECORDS.md".into(),
        ],
    }
}

/// Reason a message couldn't be delivered. Shapes the explanatory
/// copy in [`bounce`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BounceReason {
    /// 5.1.1 — recipient does not exist.
    UserUnknown,
    /// 5.7.1 — sender / message rejected by content policy or auth.
    PolicyReject,
    /// 4.x.x — temporary failure (queued > delay_threshold).
    Temporary,
    /// Catch-all for unexpected SMTP responses.
    Other,
}

/// Build the NDR (non-delivery report) bounce notification.
///
/// `to` is the original sender (the one receiving the bounce).
/// `failed_recipient` is who the message failed to reach.
/// `diagnostic` is the SMTP diagnostic text (`"550 5.1.1 user
/// unknown"`).
/// `original_subject` is the subject of the message that bounced.
#[must_use]
pub fn bounce(
    to: &str,
    failed_recipient: &str,
    reason: BounceReason,
    diagnostic: &str,
    original_subject: &str,
) -> EmailDocument {
    let (eyebrow_text, heading_text, why) = match reason {
        BounceReason::UserUnknown => (
            "Undeliverable · 5.1.1",
            "Recipient address doesn't exist",
            "The mailbox you sent to isn't configured at the destination \
             server. Check the address for typos — capitalization is OK \
             (we lowercase before lookup), but a wrong local part won't \
             route. If you copied this address from somewhere, the most \
             likely cause is a stale reference; the recipient may have \
             deactivated or never had a mailbox here.",
        ),
        BounceReason::PolicyReject => (
            "Undeliverable · 5.7.x",
            "Message rejected by policy",
            "The destination server refused this message. Common reasons: \
             SPF / DKIM / DMARC alignment failure, content matched a \
             filter, or your sender IP is on a blocklist the recipient \
             trusts. The diagnostic line below is the verbatim SMTP \
             response.",
        ),
        BounceReason::Temporary => (
            "Delayed · 4.x.x",
            "Message delayed — still trying",
            "The destination server returned a temporary failure. We'll \
             keep retrying for up to five days. No action needed unless \
             the message is time-sensitive — in which case contact the \
             recipient over a different channel and let us know.",
        ),
        BounceReason::Other => (
            "Undeliverable",
            "Message couldn't be delivered",
            "An unexpected SMTP response prevented delivery. The verbatim \
             diagnostic from the destination server is below — feel free \
             to forward this email to team@plausiden.com if you'd like \
             help interpreting it.",
        ),
    };

    let blocks: Vec<Block> = vec![
        Block::Paragraph { text: why.into() },
        Block::Group(GroupCard {
            eyebrow: "Failed delivery".into(),
            title: "What we tried to deliver".into(),
            subtitle: None,
            body: GroupBody::Fields { fields: vec![
                Field {
                    label: "To".into(),
                    value: failed_recipient.into(),
                    mono: true,
                },
                Field {
                    label: "Subject".into(),
                    value: original_subject.into(),
                    mono: false,
                },
                Field {
                    label: "Server reply".into(),
                    value: diagnostic.into(),
                    mono: true,
                },
            ] },
            how_to: None,
        }),
        Block::Paragraph {
            text: "Your original message is attached as a copy at the end of \
                   this email — feel free to copy the body and re-send to a \
                   corrected address."
                .into(),
        },
        Block::Cta(Cta {
            label: "Email postmaster".into(),
            href: "mailto:postmaster@plausiden.com".into(),
        }),
    ];

    EmailDocument {
        subject: format!("Undeliverable: {original_subject}"),
        preheader: format!("We couldn't deliver to {failed_recipient}."),
        eyebrow: Some(eyebrow_text.into()),
        heading: heading_text.into(),
        intro: Some(format!(
            "We tried to deliver your message to {failed_recipient}, but \
             the destination server returned an error."
        )),
        blocks,
        footer_lines: vec![
            format!("Bounce notice for {to} · sent by mail.plausiden.com"),
            "If you think this is a mistake, forward this email to team@plausiden.com.".into(),
        ],
    }
}

/// Build the "Sign in to PlausiDen admin" magic-link email.
#[must_use]
pub fn magic_link(link: &str) -> EmailDocument {
    let blocks = vec![
        Block::Paragraph {
            text: "Click the button below within 15 minutes to sign in. The \
                   link is single-use — once you click it, it can't be reused."
                .into(),
        },
        Block::Cta(Cta {
            label: "Sign in →".into(),
            href: link.into(),
        }),
        Block::Group(GroupCard {
            eyebrow: "Fallback".into(),
            title: "If the button doesn't work".into(),
            subtitle: Some("Paste this URL directly into your browser.".into()),
            body: GroupBody::Fields { fields: vec![Field {
                label: "URL".into(),
                value: link.into(),
                mono: true,
            }] },
            how_to: None,
        }),
        Block::Paragraph {
            text: "Didn't request this? You can safely ignore this email — \
                   the link will expire on its own."
                .into(),
        },
    ];

    EmailDocument {
        subject: "Sign in to PlausiDen admin".into(),
        preheader: "Single-use sign-in link, valid for 15 minutes.".into(),
        eyebrow: Some("Sign-in link".into()),
        heading: "Sign in to PlausiDen admin".into(),
        intro: None,
        blocks,
        footer_lines: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_records_renders_each_record_as_individual_card() {
        let groups = vec![DnsGroup {
            eyebrow: "Group 1 · Salesman".into(),
            title: "outreach.plausiden.com sender domain".into(),
            subtitle: None,
            records: vec![
                DnsRecord {
                    host: "outreach".into(),
                    record_type: "A".into(),
                    value: "207.246.86.218".into(),
                    note: None,
                },
                DnsRecord {
                    host: "outreach._domainkey.outreach".into(),
                    record_type: "TXT".into(),
                    value: "v=DKIM1; ...".into(),
                    note: Some("One continuous string.".into()),
                },
            ],
            how_to: Some("Then run <code>dig +short</code>.".into()),
        }];
        let doc = dns_records(groups, vec!["dig +short A example.com".into()]);
        let html = doc.render_html();
        // Each record is its own bordered card.
        assert!(html.contains("Record 1 of 2"));
        assert!(html.contains("Record 2 of 2"));
        // Type tag pill
        assert!(html.contains(">TXT<"));
        // Note rendered
        assert!(html.contains("One continuous string"));
        // Verification block
        assert!(html.contains("dig +short A example.com"));
        // Plain text mirrors structure
        let plain = doc.render_plain();
        assert!(plain.contains("Record 1 of 2"));
        assert!(plain.contains("[A]"));
    }

    #[test]
    fn bounce_user_unknown_explains_clearly() {
        let doc = bounce(
            "team@plausiden.com",
            "William@plausiden.com",
            BounceReason::UserUnknown,
            "550 5.1.1 user unknown",
            "Test mail",
        );
        let html = doc.render_html();
        assert!(html.contains("Recipient address doesn"));
        assert!(html.contains("550 5.1.1 user unknown"));
        assert!(html.contains("William@plausiden.com"));
    }

    #[test]
    fn magic_link_includes_link_in_button_and_fallback() {
        let url = "https://plausiden.com/admin/login/verify?token=abc";
        let doc = magic_link(url);
        let html = doc.render_html();
        // Once in the CTA href, once in the fallback Field
        assert!(html.matches(url).count() >= 2);
    }

    #[test]
    fn magic_link_escapes_angle_brackets_in_url() {
        // Pathological URL: should not break out of the href attribute.
        let url = "https://example.com/?x=<script>";
        let doc = magic_link(url);
        let html = doc.render_html();
        assert!(!html.contains("?x=<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
