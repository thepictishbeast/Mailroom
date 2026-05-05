//! OpenDKIM configuration management.
//!
//! Manages DKIM key tables, signing tables, and trusted hosts
//! for multi-domain OpenDKIM setups.

use crate::Domain;

/// Canonical (CamelCase) OpenDKIM config paths.
///
/// The OpenDKIM upstream sources, the manpages, and the RHEL/Fedora
/// packages all use CamelCase: `KeyTable`, `SigningTable`, `TrustedHosts`.
/// Debian ships the lowercase variants `key.table`, `signing.table`,
/// `trusted.hosts`. Both are accepted by `opendkim.conf` directives, but
/// we have to read the right file when `verify-config` checks the live
/// state on a host. Use [`resolve_key_table_path`] /
/// [`resolve_signing_table_path`] / [`resolve_trusted_hosts_path`] to
/// pick whichever convention the installed distribution uses; fall back
/// to these constants if the resolver can't find either variant (so a
/// fresh Mailroom install still produces the canonical layout).
pub const KEY_TABLE_PATH: &str = "/etc/opendkim/KeyTable";
pub const SIGNING_TABLE_PATH: &str = "/etc/opendkim/SigningTable";
pub const TRUSTED_HOSTS_PATH: &str = "/etc/opendkim/TrustedHosts";
pub const KEYS_DIR: &str = "/etc/opendkim/keys";

/// Debian-style lowercase variants. Kept private — callers go through
/// the resolver functions below so we have one source of truth for the
/// candidate ordering.
const KEY_TABLE_PATH_DEBIAN: &str = "/etc/opendkim/key.table";
const SIGNING_TABLE_PATH_DEBIAN: &str = "/etc/opendkim/signing.table";
const TRUSTED_HOSTS_PATH_DEBIAN: &str = "/etc/opendkim/trusted.hosts";

/// Probe the filesystem for whichever case-convention the installed
/// OpenDKIM uses. Returns the canonical (CamelCase) path if neither
/// variant exists yet — that's the right default for a freshly
/// `mail-cli setup`-ed host.
pub fn resolve_key_table_path() -> &'static str {
    pick_existing(&[KEY_TABLE_PATH, KEY_TABLE_PATH_DEBIAN])
}

pub fn resolve_signing_table_path() -> &'static str {
    pick_existing(&[SIGNING_TABLE_PATH, SIGNING_TABLE_PATH_DEBIAN])
}

pub fn resolve_trusted_hosts_path() -> &'static str {
    pick_existing(&[TRUSTED_HOSTS_PATH, TRUSTED_HOSTS_PATH_DEBIAN])
}

/// Probe a list of candidate paths and return the first that exists,
/// falling back to the first candidate (the canonical path) if none do.
/// Public for tests; callers should use the typed wrappers above.
pub fn pick_existing(candidates: &[&'static str]) -> &'static str {
    for p in candidates {
        if std::path::Path::new(p).exists() {
            return p;
        }
    }
    candidates[0]
}

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
    let mut lines = vec!["127.0.0.1".to_string(), "localhost".to_string()];
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

    #[test]
    fn pick_existing_returns_first_match() {
        // /etc/passwd exists everywhere we run CI; /no/such/file shouldn't.
        let pick = pick_existing(&["/no/such/file", "/etc/passwd"]);
        assert_eq!(pick, "/etc/passwd");
    }

    #[test]
    fn pick_existing_prefers_first_candidate() {
        // When multiple candidates exist, we take the first listed —
        // canonical (CamelCase) paths are always first in the resolver
        // wrappers, so a host with BOTH conventions present (rare,
        // usually a botched cross-distro upgrade) gets the canonical one.
        let pick = pick_existing(&["/etc/passwd", "/etc/hostname"]);
        assert_eq!(pick, "/etc/passwd");
    }

    #[test]
    fn pick_existing_falls_back_to_canonical() {
        // Neither candidate exists — we want the canonical (first) path
        // back so a fresh `mail-cli setup` writes to the canonical
        // location rather than refusing to act.
        let pick = pick_existing(&["/no/such/file", "/also/missing"]);
        assert_eq!(pick, "/no/such/file");
    }

    #[test]
    fn resolver_wrappers_compile_and_return_static_strs() {
        // Smoke test — the actual filesystem behavior is exercised by
        // pick_existing tests above; here we just confirm the public
        // wrappers stay wired.
        let kt = resolve_key_table_path();
        let st = resolve_signing_table_path();
        let th = resolve_trusted_hosts_path();
        assert!(kt.starts_with("/etc/opendkim/"));
        assert!(st.starts_with("/etc/opendkim/"));
        assert!(th.starts_with("/etc/opendkim/"));
    }
}
