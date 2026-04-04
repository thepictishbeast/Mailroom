//! Postfix configuration management.
//!
//! Reads and writes Postfix virtual mailbox maps, virtual alias maps,
//! and main.cf parameters. All writes go through validation before
//! touching disk.

use crate::{Domain, ConfigError, Result};
use std::path::Path;

/// Default path for the Postfix virtual mailbox map.
pub const VMAILBOX_PATH: &str = "/etc/postfix/vmailbox";

/// Default path for the Postfix virtual alias map.
pub const VALIAS_PATH: &str = "/etc/postfix/virtual";

/// Generate vmailbox entries for a domain's mailboxes.
///
/// Each line maps an address to its Maildir path:
/// `tim@sacred.vote  sacred.vote/tim/`
pub fn generate_vmailbox_entries(domain: &Domain) -> Vec<String> {
    domain
        .mailboxes
        .iter()
        .map(|mb| {
            format!(
                "{}@{}  {}/{}/",
                mb.local_part, domain.name, domain.name, mb.local_part
            )
        })
        .collect()
}

/// Validate that a mailbox local part is safe for Postfix.
/// Rejects empty strings, strings with spaces, and special characters
/// that could cause config injection.
pub fn validate_local_part(local: &str) -> Result<()> {
    if local.is_empty() {
        return Err(ConfigError::ValidationError {
            field: "local_part".into(),
            reason: "cannot be empty".into(),
        });
    }
    if local.contains(|c: char| c.is_whitespace() || c == '@' || c == '/' || c == '\\') {
        return Err(ConfigError::ValidationError {
            field: "local_part".into(),
            reason: format!("contains invalid characters: {}", local),
        });
    }
    Ok(())
}

/// Read existing vmailbox entries from disk.
pub fn read_vmailbox(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ConfigError::FileNotFound {
                path: path.display().to_string(),
            }
        } else {
            ConfigError::Io(e)
        }
    })?;
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mailbox, MailboxKind};

    #[test]
    fn generate_entries() {
        let domain = Domain {
            name: "sacred.vote".into(),
            dkim_enabled: true,
            dkim_selector: "default".into(),
            mailboxes: vec![
                Mailbox {
                    local_part: "tim".into(),
                    kind: MailboxKind::User,
                    display_name: None,
                },
                Mailbox {
                    local_part: "noreply".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
            ],
        };
        let entries = generate_vmailbox_entries(&domain);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], "tim@sacred.vote  sacred.vote/tim/");
        assert_eq!(entries[1], "noreply@sacred.vote  sacred.vote/noreply/");
    }

    #[test]
    fn validate_local_part_rejects_bad_input() {
        assert!(validate_local_part("").is_err());
        assert!(validate_local_part("has space").is_err());
        assert!(validate_local_part("has@at").is_err());
        assert!(validate_local_part("tim").is_ok());
        assert!(validate_local_part("no-reply").is_ok());
    }
}
