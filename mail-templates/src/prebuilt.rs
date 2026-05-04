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

/// Severity of an operations alert.
///
/// Maps to the eyebrow color + heading tone of the rendered email.
/// Receivers triage on the eyebrow at a glance — `Critical` should
/// page; `Warning` should be looked at within the day; `Info` is
/// observability noise that's not worth waking up for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    /// Page-worthy. Service down, data loss in progress, security
    /// breach in flight.
    Critical,
    /// Investigate within the day. Disk filling, certs expiring soon,
    /// queue backing up.
    Warning,
    /// Informational. Cron job ran, daily digest, build promoted.
    Info,
}

impl AlertSeverity {
    fn eyebrow(self) -> &'static str {
        match self {
            Self::Critical => "Critical · page",
            Self::Warning => "Warning · investigate",
            Self::Info => "Info",
        }
    }
}

/// Build a polished ops alert / monitoring notification email.
///
/// `title` is the one-line summary that becomes the headline (and
/// subject prefix). `summary` is the longer "what happened" prose
/// that opens the body. `fields` are the typed key/value pairs the
/// receiver scans for context (host, service, since-when, error
/// rate, etc.). `runbook_url` is optional — when present, renders
/// as a CTA button so the on-call can jump straight to the right
/// runbook page. `on_call` is the addressable contact line printed
/// in the footer (an email or a paging URL); a tenant-specific
/// default is used if `None`.
#[must_use]
pub fn alert(
    severity: AlertSeverity,
    title: &str,
    summary: &str,
    fields: Vec<Field>,
    runbook_url: Option<&str>,
    on_call: Option<&str>,
) -> EmailDocument {
    let subject_tag = match severity {
        AlertSeverity::Critical => "[CRITICAL]",
        AlertSeverity::Warning => "[WARN]",
        AlertSeverity::Info => "[INFO]",
    };
    let mut blocks: Vec<Block> = vec![
        Block::Paragraph {
            text: summary.to_string(),
        },
        Block::Group(GroupCard {
            eyebrow: "Context".into(),
            title: "Signal at a glance".into(),
            subtitle: None,
            body: GroupBody::Fields { fields },
            how_to: None,
        }),
    ];
    if let Some(url) = runbook_url {
        blocks.push(Block::Cta(Cta {
            label: "Open runbook →".into(),
            href: url.into(),
        }));
    }

    let on_call_line = on_call.unwrap_or("Reply to this thread or page team@plausiden.com.");

    EmailDocument {
        subject: format!("{subject_tag} {title}"),
        preheader: summary.chars().take(120).collect(),
        eyebrow: Some(severity.eyebrow().into()),
        heading: title.to_string(),
        intro: None,
        blocks,
        footer_lines: vec![
            on_call_line.to_string(),
            "Generated by mail-orchestrator alerts.".into(),
        ],
    }
}

/// One row of a weekly digest — a category with a count and a
/// short caption. Renders as a [`RecordCard`] inside the digest
/// summary group.
#[derive(Debug, Clone)]
pub struct DigestRow {
    /// Category name (e.g., `"Inbox"`, `"Updates"`, `"Important"`).
    pub category: String,
    /// Count of items this week.
    pub count: u32,
    /// Optional caption (e.g., `"3 unread"`, `"+12% vs last week"`).
    pub caption: Option<String>,
}

/// Build a weekly digest / summary email. Useful for "what arrived
/// in the service mailboxes this week" reports, recap newsletters,
/// any periodic summary the operator wants visible.
///
/// `period_label` is the human-readable timeframe (e.g.,
/// `"Week of 2026-04-28"`). `headline` is the at-a-glance summary
/// (e.g., `"42 new messages, 3 require attention"`). `rows` are
/// the per-category breakdowns. `extras` are optional free-form
/// paragraphs after the summary card (e.g., commentary, what's
/// next).
#[must_use]
pub fn weekly_digest(
    period_label: &str,
    headline: &str,
    rows: Vec<DigestRow>,
    extras: Vec<String>,
) -> EmailDocument {
    let total = rows.len();
    let records = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| RecordCard {
            eyebrow: format!("Row {} of {}", i + 1, total),
            primary_label: r.category,
            type_tag: Some(format!("{}", r.count)),
            value: r.caption.unwrap_or_default(),
            note: None,
        })
        .collect();

    let mut blocks: Vec<Block> = vec![Block::Group(GroupCard {
        eyebrow: "Digest".into(),
        title: headline.to_string(),
        subtitle: Some(period_label.to_string()),
        body: GroupBody::Records { records },
        how_to: None,
    })];

    for extra in extras {
        blocks.push(Block::Paragraph { text: extra });
    }

    EmailDocument {
        subject: format!("Digest · {period_label}"),
        preheader: headline.chars().take(120).collect(),
        eyebrow: Some("Weekly digest".into()),
        heading: format!("Digest · {period_label}"),
        intro: None,
        blocks,
        footer_lines: vec!["Generated by mail-orchestrator weekly summary.".into()],
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

    #[test]
    fn alert_critical_includes_severity_eyebrow_and_runbook_cta() {
        let doc = alert(
            AlertSeverity::Critical,
            "Disk usage 95% on web-01",
            "The /var partition has crossed the 95% high-water mark. \
             Mail delivery will stall once it hits 100%.",
            vec![
                Field {
                    label: "Host".into(),
                    value: "web-01".into(),
                    mono: true,
                },
                Field {
                    label: "Mountpoint".into(),
                    value: "/var".into(),
                    mono: true,
                },
                Field {
                    label: "Used".into(),
                    value: "95%".into(),
                    mono: false,
                },
            ],
            Some("https://runbooks.plausiden.com/disk-full"),
            Some("oncall@plausiden.com"),
        );
        let html = doc.render_html();
        // Severity eyebrow
        assert!(html.contains("Critical · page"));
        // Subject prefix
        assert_eq!(doc.subject, "[CRITICAL] Disk usage 95% on web-01");
        // Fields rendered
        assert!(html.contains("web-01"));
        assert!(html.contains("/var"));
        // Runbook CTA
        assert!(html.contains("Open runbook"));
        assert!(html.contains("https://runbooks.plausiden.com/disk-full"));
        // On-call footer
        assert!(html.contains("oncall@plausiden.com"));
    }

    #[test]
    fn alert_info_skips_runbook_cta_when_not_provided() {
        let doc = alert(
            AlertSeverity::Info,
            "Daily backup completed",
            "Nightly rsync of /var/mail/vhosts to off-site target completed cleanly.",
            vec![Field {
                label: "Duration".into(),
                value: "8 min".into(),
                mono: false,
            }],
            None,
            None,
        );
        let html = doc.render_html();
        assert_eq!(doc.subject, "[INFO] Daily backup completed");
        assert!(html.contains("Daily backup completed"));
        // No runbook CTA when URL is absent.
        assert!(!html.contains("Open runbook"));
        // Default on-call line falls back to the team@ contact.
        assert!(html.contains("team@plausiden.com"));
    }

    #[test]
    fn weekly_digest_renders_each_row_as_record_card() {
        let doc = weekly_digest(
            "Week of 2026-04-28",
            "42 new messages, 3 require attention",
            vec![
                DigestRow {
                    category: "Inbox".into(),
                    count: 12,
                    caption: Some("3 unread".into()),
                },
                DigestRow {
                    category: "Important".into(),
                    count: 3,
                    caption: Some("all replied".into()),
                },
                DigestRow {
                    category: "Updates".into(),
                    count: 27,
                    caption: None,
                },
            ],
            vec!["Top sender this week: GitHub.".into()],
        );
        let html = doc.render_html();
        assert_eq!(doc.subject, "Digest · Week of 2026-04-28");
        // Period in subtitle, headline in title
        assert!(html.contains("Week of 2026-04-28"));
        assert!(html.contains("42 new messages"));
        // Each row is its own bordered card
        assert!(html.contains("Row 1 of 3"));
        assert!(html.contains("Row 3 of 3"));
        // Type-tag pill carries the count
        assert!(html.contains(">12<"));
        assert!(html.contains(">27<"));
        // Caption for absent → empty value, not crash
        // Extra paragraph appears below
        assert!(html.contains("Top sender this week"));
    }
}
