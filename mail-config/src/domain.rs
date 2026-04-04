//! Domain and mailbox model types.
//!
//! These types represent the logical structure of the mail server:
//! which domains we serve, which mailboxes exist, and their roles.

use serde::{Deserialize, Serialize};

/// A domain served by this mail server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    /// The domain name (e.g., "sacred.vote").
    pub name: String,
    /// Whether DKIM signing is enabled for this domain.
    pub dkim_enabled: bool,
    /// The DKIM selector (e.g., "default").
    pub dkim_selector: String,
    /// Mailboxes belonging to this domain.
    pub mailboxes: Vec<Mailbox>,
}

/// A mailbox on the mail server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mailbox {
    /// Local part of the address (e.g., "tim" for tim@sacred.vote).
    pub local_part: String,
    /// What kind of mailbox this is — affects Sieve rules and routing.
    pub kind: MailboxKind,
    /// Human-readable display name (optional).
    pub display_name: Option<String>,
}

impl Mailbox {
    /// Returns the full email address for a given domain.
    pub fn address(&self, domain: &str) -> String {
        format!("{}@{}", self.local_part, domain)
    }
}

/// Classification of mailbox function — drives Sieve filter generation
/// and access control decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind {
    /// Human user mailbox (tim@, admin@).
    User,
    /// Service/application mailbox (noreply@, alerts@, vault@).
    Service,
    /// Legal/compliance mailbox (legal@, privacy@, security@).
    Legal,
    /// Routing/orchestration mailbox (router@).
    Router,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_address_format() {
        let mb = Mailbox {
            local_part: "tim".to_string(),
            kind: MailboxKind::User,
            display_name: Some("Tim".to_string()),
        };
        assert_eq!(mb.address("sacred.vote"), "tim@sacred.vote");
    }

    #[test]
    fn mailbox_kind_serialization() {
        let json = serde_json::to_string(&MailboxKind::Service).unwrap();
        assert_eq!(json, "\"service\"");
        let parsed: MailboxKind = serde_json::from_str("\"legal\"").unwrap();
        assert_eq!(parsed, MailboxKind::Legal);
    }
}
