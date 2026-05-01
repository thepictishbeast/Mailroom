//! Outbound email sender via SMTP to local Postfix.
//!
//! Connects to localhost:25 (trusted loopback in Postfix mynetworks)
//! and sends email with arbitrary From: identity. Postfix + OpenDKIM
//! handles signing and relay automatically.

use anyhow::{Context, Result};
use lettre::message::{header::ContentType, Mailbox, MultiPart};
use lettre::transport::smtp::client::Tls;
use lettre::{Message, SmtpTransport, Transport};
use mail_templates::{Block, EmailDocument, Field, GroupBody, GroupCard};
use tracing::{debug, error};

/// SMTP sender that connects to local Postfix.
pub struct Sender {
    transport: SmtpTransport,
}

impl Sender {
    /// Create a new sender connecting to the given SMTP host:port.
    pub fn new(host: &str, port: u16) -> Result<Self> {
        let transport = SmtpTransport::builder_dangerous(host)
            .port(port)
            .tls(Tls::None)
            .build();
        Ok(Self { transport })
    }

    /// Send an email with the specified identity.
    pub fn send_email(
        &self,
        from: &str,
        from_name: Option<&str>,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<String> {
        let from_mailbox: Mailbox = if let Some(name) = from_name {
            format!("{} <{}>", name, from)
                .parse()
                .context("Invalid from address")?
        } else {
            from.parse().context("Invalid from address")?
        };

        let to_mailbox: Mailbox = to.parse().context("Invalid to address")?;

        let message = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .context("Failed to build email message")?;

        match self.transport.send(&message) {
            Ok(response) => {
                let msg = response.message().collect::<Vec<_>>().join(" ");
                debug!(from, to, subject, "Email sent: {}", msg);
                Ok(msg)
            }
            Err(e) => {
                error!(from, to, subject, error = %e, "Failed to send email");
                Err(e).context("SMTP send failed")
            }
        }
    }

    /// Send a notification email about a new message in a service
    /// mailbox. Sends as `multipart/alternative` (plain + branded HTML)
    /// so receiving clients pick the polished version when available;
    /// terminal mail readers and accessibility tools still get the
    /// plain side.
    #[allow(clippy::too_many_arguments)]
    pub fn send_notification(
        &self,
        from_identity: &str,
        subscriber: &str,
        mailbox_name: &str,
        priority: &str,
        original_from: &str,
        original_subject: &str,
        body_preview: &str,
        tracking_id: &str,
    ) -> Result<String> {
        let subject = format!(
            "[{}] {} — New email in {}@",
            priority.to_uppercase(),
            original_subject,
            mailbox_name
        );

        let plain = format!(
            "New email in {mailbox}\nPriority: {priority}\n\n\
             From: {from}\nSubject: {subj}\n\n\
             Preview:\n{preview}\n\n---\n\
             Automated notification from Mail Orchestrator\n\
             Tracking ID: {tid}",
            mailbox = mailbox_name,
            priority = priority,
            from = original_from,
            subj = original_subject,
            preview = body_preview,
            tid = tracking_id,
        );

        let html = build_notification_html(
            mailbox_name,
            priority,
            original_from,
            original_subject,
            body_preview,
            tracking_id,
        );

        self.send_multipart(
            from_identity,
            Some("Mail Orchestrator"),
            subscriber,
            &subject,
            &plain,
            &html,
        )
    }

    /// Send a `multipart/alternative` message with both plain-text
    /// and HTML alternatives. Receiving clients pick whichever they
    /// prefer; terminal readers see the plain side, GUI clients see
    /// the branded HTML.
    fn send_multipart(
        &self,
        from: &str,
        from_name: Option<&str>,
        to: &str,
        subject: &str,
        plain: &str,
        html: &str,
    ) -> Result<String> {
        let from_mailbox: Mailbox = if let Some(name) = from_name {
            format!("{} <{}>", name, from)
                .parse()
                .context("Invalid from address")?
        } else {
            from.parse().context("Invalid from address")?
        };

        let to_mailbox: Mailbox = to.parse().context("Invalid to address")?;

        let message = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .multipart(MultiPart::alternative_plain_html(
                plain.to_string(),
                html.to_string(),
            ))
            .context("Failed to build multipart email message")?;

        match self.transport.send(&message) {
            Ok(response) => {
                let msg = response.message().collect::<Vec<_>>().join(" ");
                debug!(from, to, subject, "Multipart email sent: {}", msg);
                Ok(msg)
            }
            Err(e) => {
                error!(from, to, subject, error = %e, "Failed to send multipart email");
                Err(e).context("SMTP send failed")
            }
        }
    }
}

/// Build the polished HTML alternative for a notification email.
/// Mirrors the plain-text body's information density (mailbox,
/// priority, sender, subject, preview, tracking ID) but laid out
/// with the mail-templates chrome.
fn build_notification_html(
    mailbox_name: &str,
    priority: &str,
    original_from: &str,
    original_subject: &str,
    body_preview: &str,
    tracking_id: &str,
) -> String {
    let priority_pretty = match priority.to_ascii_lowercase().as_str() {
        "high" | "urgent" | "p1" => "High priority",
        "low" | "p4" => "Low priority",
        _ => "New message",
    };
    let preview_truncated: String = if body_preview.chars().count() > 480 {
        let truncated: String = body_preview.chars().take(480).collect();
        format!("{truncated}…")
    } else {
        body_preview.to_string()
    };
    let doc = EmailDocument {
        subject: original_subject.to_string(),
        preheader: format!("From {original_from}: {original_subject}"),
        eyebrow: Some(priority_pretty.into()),
        heading: original_subject.to_string(),
        intro: Some(format!(
            "A new message arrived in {mailbox_name}. Preview below — open the \
             mailbox to read the full message."
        )),
        blocks: vec![
            Block::Group(GroupCard {
                eyebrow: "Sender".into(),
                title: original_from.to_string(),
                subtitle: None,
                body: GroupBody::Fields {
                    fields: vec![
                        Field {
                            label: "Mailbox".into(),
                            value: mailbox_name.into(),
                            mono: true,
                        },
                        Field {
                            label: "Priority".into(),
                            value: priority.into(),
                            mono: false,
                        },
                    ],
                },
                how_to: None,
            }),
            Block::Paragraph {
                text: preview_truncated,
            },
        ],
        footer_lines: vec![format!("Tracking ID: {tracking_id}")],
    };
    doc.render_html()
}
