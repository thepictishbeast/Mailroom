//! Dovecot configuration management.
//!
//! Manages the Dovecot passwd-file user database and Sieve script paths.
//! Generates properly formatted entries for Dovecot's userdb/passdb.

use crate::{Domain, ConfigError, Result};

/// Default path for the Dovecot passwd-file.
pub const PASSWD_FILE_PATH: &str = "/etc/dovecot/users";

/// Generate a Dovecot passwd-file entry for a mailbox.
///
/// Format: `user@domain:{scheme}password:vmail_uid:vmail_gid::/var/mail/vhosts/domain/user`
///
/// The password should already be hashed (SHA512-CRYPT recommended).
pub fn generate_passwd_entry(
    address: &str,
    password_hash: &str,
    vmail_uid: u32,
    vmail_gid: u32,
    mail_base: &str,
    domain: &str,
    local_part: &str,
) -> Result<String> {
    if address.is_empty() || !address.contains('@') {
        return Err(ConfigError::ValidationError {
            field: "address".into(),
            reason: "must be a valid email address".into(),
        });
    }
    Ok(format!(
        "{}:{}:{}:{}::{}/{}/{}/",
        address, password_hash, vmail_uid, vmail_gid, mail_base, domain, local_part
    ))
}

/// Parse existing Dovecot passwd-file entries.
/// Returns a list of (address, hash, uid, gid, home_path) tuples.
pub fn parse_passwd_file(content: &str) -> Vec<(String, String, String)> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(6, ':').collect();
            if parts.len() >= 2 {
                Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts.get(5).unwrap_or(&"").to_string(),
                ))
            } else {
                None
            }
        })
        .collect()
}

/// Validate that a domain has at least one mailbox defined.
pub fn validate_domain(domain: &Domain) -> Result<()> {
    if domain.mailboxes.is_empty() {
        return Err(ConfigError::ValidationError {
            field: "mailboxes".into(),
            reason: format!("domain {} has no mailboxes", domain.name),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_valid_passwd_entry() {
        let entry = generate_passwd_entry(
            "tim@sacred.vote",
            "{SHA512-CRYPT}$6$abc$xyz",
            5000,
            5000,
            "/var/mail/vhosts",
            "sacred.vote",
            "tim",
        )
        .unwrap();
        assert!(entry.starts_with("tim@sacred.vote:"));
        assert!(entry.contains("/var/mail/vhosts/sacred.vote/tim/"));
    }

    #[test]
    fn reject_invalid_address() {
        assert!(generate_passwd_entry("noatsign", "hash", 5000, 5000, "/var/mail", "d", "u").is_err());
        assert!(generate_passwd_entry("", "hash", 5000, 5000, "/var/mail", "d", "u").is_err());
    }

    #[test]
    fn parse_passwd_entries() {
        let content = "tim@sacred.vote:{SHA512-CRYPT}hash:5000:5000::/var/mail/vhosts/sacred.vote/tim/\n\
                        admin@sacred.vote:{SHA512-CRYPT}hash2:5000:5000::/var/mail/vhosts/sacred.vote/admin/\n";
        let entries = parse_passwd_file(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "tim@sacred.vote");
    }
}
