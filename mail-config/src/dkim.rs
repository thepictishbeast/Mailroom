//! OpenDKIM configuration management.
//!
//! Manages DKIM key tables, signing tables, and trusted hosts
//! for multi-domain OpenDKIM setups.

use crate::Domain;

/// Default paths for OpenDKIM configuration files.
pub const KEY_TABLE_PATH: &str = "/etc/opendkim/KeyTable";
pub const SIGNING_TABLE_PATH: &str = "/etc/opendkim/SigningTable";
pub const TRUSTED_HOSTS_PATH: &str = "/etc/opendkim/TrustedHosts";
pub const KEYS_DIR: &str = "/etc/opendkim/keys";

/// Generate a KeyTable entry for a domain.
///
/// Format: `selector._domainkey.domain  domain:selector:/path/to/key`
pub fn key_table_entry(domain: &Domain) -> String {
    format!(
        "{}._domainkey.{}  {}:{}:{}/{}/{}.private",
        domain.dkim_selector,
        domain.name,
        domain.name,
        domain.dkim_selector,
        KEYS_DIR,
        domain.name,
        domain.dkim_selector
    )
}

/// Generate a SigningTable entry for a domain.
///
/// Format: `*@domain  selector._domainkey.domain`
pub fn signing_table_entry(domain: &Domain) -> String {
    format!(
        "*@{}  {}._domainkey.{}",
        domain.name, domain.dkim_selector, domain.name
    )
}

/// Generate TrustedHosts content for multiple domains.
pub fn trusted_hosts(domains: &[Domain]) -> String {
    let mut lines = vec![
        "127.0.0.1".to_string(),
        "localhost".to_string(),
    ];
    for d in domains {
        lines.push(format!("*.{}", d.name));
    }
    lines.join("\n") + "\n"
}

/// Returns the expected filesystem path for a domain's DKIM private key.
pub fn key_path(domain: &str, selector: &str) -> String {
    format!("{}/{}/{}.private", KEYS_DIR, domain, selector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Domain, Mailbox, MailboxKind};

    fn test_domain() -> Domain {
        Domain {
            name: "sacred.vote".into(),
            dkim_enabled: true,
            dkim_selector: "default".into(),
            mailboxes: vec![Mailbox {
                local_part: "tim".into(),
                kind: MailboxKind::User,
                display_name: None,
            }],
        }
    }

    #[test]
    fn key_table_format() {
        let entry = key_table_entry(&test_domain());
        assert!(entry.contains("default._domainkey.sacred.vote"));
        assert!(entry.contains("/etc/opendkim/keys/sacred.vote/default.private"));
    }

    #[test]
    fn signing_table_format() {
        let entry = signing_table_entry(&test_domain());
        assert_eq!(entry, "*@sacred.vote  default._domainkey.sacred.vote");
    }

    #[test]
    fn trusted_hosts_includes_localhost() {
        let hosts = trusted_hosts(&[test_domain()]);
        assert!(hosts.contains("127.0.0.1"));
        assert!(hosts.contains("*.sacred.vote"));
    }
}
