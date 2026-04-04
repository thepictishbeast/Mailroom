//! Outbound email sender via SMTP to local Postfix.
//!
//! Connects to localhost:25 (trusted loopback in Postfix mynetworks)
//! and sends email with arbitrary From: identity. Postfix + OpenDKIM
//! handles signing and relay automatically.

use anyhow::{Context, Result};
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::client::Tls;
use lettre::{Message, SmtpTransport, Transport};
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

    /// Send a notification email about a new message in a service mailbox.
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

        let body = format!(
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

        self.send_email(from_identity, Some("Mail Orchestrator"), subscriber, &subject, &body)
    }
}
