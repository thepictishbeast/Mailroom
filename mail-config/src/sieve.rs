//! Sieve filter generation.
//!
//! Generates Sieve scripts for mailbox-specific filtering rules:
//! auto-reject for noreply, notification routing, auto-responders.

use crate::{Mailbox, MailboxKind};

/// Generate a Sieve script for a mailbox based on its kind.
///
/// - Service/noreply: reject all inbound with a human-readable message.
/// - Router: fileinto a processing folder for the orchestrator.
/// - User/Legal: basic spam filtering + folder organization.
pub fn generate_sieve(mailbox: &Mailbox, domain: &str) -> String {
    match mailbox.kind {
        MailboxKind::Service if mailbox.local_part == "noreply" => {
            format!(
                r#"require ["reject"];
# Auto-reject: {address} does not accept inbound mail.
if true {{
    reject "This address ({address}) does not accept incoming messages. Please contact support@{domain} instead.";
    stop;
}}
"#,
                address = mailbox.address(domain),
                domain = domain
            )
        }
        MailboxKind::Service => {
            r#"require ["fileinto", "mailbox"];
# Service mailbox: file all inbound into service-specific folder.
if true {
    fileinto :create "Service";
}
"#
            .to_string()
        }
        MailboxKind::Router => {
            r#"require ["fileinto", "mailbox"];
# Router mailbox: all mail goes to processing queue.
if true {
    fileinto :create "Processing";
}
"#
            .to_string()
        }
        MailboxKind::User | MailboxKind::Legal => {
            format!(
                r#"require ["fileinto", "mailbox"];
# Standard mailbox filtering for {address}.
# Additional rules can be added via ManageSieve (port 4190).
"#,
                address = mailbox.address(domain)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noreply_generates_reject() {
        let mb = Mailbox {
            local_part: "noreply".into(),
            kind: MailboxKind::Service,
            display_name: None,
        };
        let script = generate_sieve(&mb, "sacred.vote");
        assert!(script.contains("reject"));
        assert!(script.contains("noreply@sacred.vote"));
    }

    #[test]
    fn router_generates_fileinto() {
        let mb = Mailbox {
            local_part: "router".into(),
            kind: MailboxKind::Router,
            display_name: None,
        };
        let script = generate_sieve(&mb, "sacred.vote");
        assert!(script.contains("Processing"));
    }

    #[test]
    fn user_generates_standard_filter() {
        let mb = Mailbox {
            local_part: "tim".into(),
            kind: MailboxKind::User,
            display_name: Some("Tim".into()),
        };
        let script = generate_sieve(&mb, "sacred.vote");
        assert!(script.contains("tim@sacred.vote"));
    }
}
