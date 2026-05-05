//! Integration tests for multi-domain config generation.
//!
//! Tests that the mail-config crate produces valid Postfix, Dovecot,
//! DKIM, and Sieve configurations for the Sacred.Vote two-domain setup.

use mail_config::dkim;
use mail_config::dovecot;
use mail_config::postfix;
use mail_config::sieve;
use mail_config::{Domain, Mailbox, MailboxKind};

fn sacred_vote_domains() -> Vec<Domain> {
    vec![
        Domain {
            name: "sacred.vote".into(),
            dkim_enabled: true,
            dkim_selector: "default".into(),
            mailboxes: vec![
                Mailbox {
                    local_part: "tim".into(),
                    kind: MailboxKind::User,
                    display_name: Some("Tim".into()),
                },
                Mailbox {
                    local_part: "admin".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
                Mailbox {
                    local_part: "noreply".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
                Mailbox {
                    local_part: "support".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
                Mailbox {
                    local_part: "router".into(),
                    kind: MailboxKind::Router,
                    display_name: None,
                },
            ],
        },
        Domain {
            name: "sacredvote.org".into(),
            dkim_enabled: true,
            dkim_selector: "default".into(),
            mailboxes: vec![
                Mailbox {
                    local_part: "legal".into(),
                    kind: MailboxKind::Legal,
                    display_name: None,
                },
                Mailbox {
                    local_part: "security".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
                Mailbox {
                    local_part: "privacy".into(),
                    kind: MailboxKind::Service,
                    display_name: None,
                },
            ],
        },
    ]
}

#[test]
fn vmailbox_entries_all_present() {
    let domains = sacred_vote_domains();
    for domain in &domains {
        let entries = postfix::generate_vmailbox_entries(domain);
        assert_eq!(entries.len(), domain.mailboxes.len());
        for entry in &entries {
            assert!(
                entry.contains(&domain.name),
                "entry missing domain: {}",
                entry
            );
            assert!(
                entry.contains("  "),
                "missing double-space separator: {}",
                entry
            );
            assert!(
                entry.ends_with('/'),
                "maildir path should end with /: {}",
                entry
            );
        }
    }
}

#[test]
fn vmailbox_no_duplicates() {
    let domains = sacred_vote_domains();
    let mut all_entries = Vec::new();
    for domain in &domains {
        all_entries.extend(postfix::generate_vmailbox_entries(domain));
    }
    let count = all_entries.len();
    all_entries.sort();
    all_entries.dedup();
    assert_eq!(count, all_entries.len(), "duplicate vmailbox entries found");
}

#[test]
fn dovecot_passwd_entries_valid() {
    let domains = sacred_vote_domains();
    for domain in &domains {
        for mb in &domain.mailboxes {
            let addr = mb.address(&domain.name);
            let entry = dovecot::generate_passwd_entry(
                &addr,
                "{SHA512-CRYPT}$6$test$hash",
                5000,
                5000,
                "/var/mail/vhosts",
                &domain.name,
                &mb.local_part,
            )
            .unwrap();
            assert!(entry.starts_with(&addr), "entry should start with address");
            assert!(entry.contains("5000:5000"), "entry should contain uid:gid");
        }
    }
}

#[test]
fn dovecot_validates_domain_has_mailboxes() {
    let empty_domain = Domain {
        name: "empty.com".into(),
        dkim_enabled: false,
        dkim_selector: "default".into(),
        mailboxes: vec![],
    };
    assert!(dovecot::validate_domain(&empty_domain).is_err());
    assert!(dovecot::validate_domain(&sacred_vote_domains()[0]).is_ok());
}

#[test]
fn dkim_key_and_signing_tables_consistent() {
    let domains = sacred_vote_domains();
    for domain in &domains {
        let kt = dkim::key_table_entry(domain);
        let st = dkim::signing_table_entry(domain);
        // Both reference the same selector._domainkey.domain
        let domainkey = format!("{}._domainkey.{}", domain.dkim_selector, domain.name);
        assert!(kt.contains(&domainkey), "KeyTable missing domainkey ref");
        assert!(
            st.contains(&domainkey),
            "SigningTable missing domainkey ref"
        );
        // Signing table maps *@domain
        assert!(st.starts_with(&format!("*@{}", domain.name)));
    }
}

#[test]
fn dkim_trusted_hosts_includes_all_domains() {
    let domains = sacred_vote_domains();
    let hosts = dkim::trusted_hosts(&domains);
    assert!(hosts.contains("127.0.0.1"));
    assert!(hosts.contains("localhost"));
    for domain in &domains {
        assert!(hosts.contains(&format!("*.{}", domain.name)));
    }
}

#[test]
fn dkim_key_paths_correct() {
    let path = dkim::key_path("sacred.vote", "default");
    assert_eq!(path, "/etc/opendkim/keys/sacred.vote/default.private");
}

#[test]
fn sieve_noreply_rejects() {
    let mb = Mailbox {
        local_part: "noreply".into(),
        kind: MailboxKind::Service,
        display_name: None,
    };
    let script = sieve::generate_sieve(&mb, "sacred.vote");
    assert!(script.contains("reject"));
    assert!(script.contains("noreply@sacred.vote"));
    assert!(!script.contains("fileinto"));
}

#[test]
fn sieve_router_files_into_processing() {
    let mb = Mailbox {
        local_part: "router".into(),
        kind: MailboxKind::Router,
        display_name: None,
    };
    let script = sieve::generate_sieve(&mb, "sacred.vote");
    assert!(script.contains("fileinto"));
    assert!(script.contains("Processing"));
}

#[test]
fn sieve_service_non_noreply_files_into_service() {
    let mb = Mailbox {
        local_part: "admin".into(),
        kind: MailboxKind::Service,
        display_name: None,
    };
    let script = sieve::generate_sieve(&mb, "sacred.vote");
    assert!(script.contains("fileinto"));
    assert!(script.contains("Service"));
}

#[test]
fn sieve_legal_gets_standard_filter() {
    let mb = Mailbox {
        local_part: "legal".into(),
        kind: MailboxKind::Legal,
        display_name: None,
    };
    let script = sieve::generate_sieve(&mb, "sacredvote.org");
    assert!(script.contains("legal@sacredvote.org"));
    assert!(!script.contains("reject"));
}

#[test]
fn postfix_validate_rejects_injection() {
    assert!(postfix::validate_local_part("valid").is_ok());
    assert!(postfix::validate_local_part("no-reply").is_ok());
    assert!(postfix::validate_local_part("").is_err());
    assert!(postfix::validate_local_part("has space").is_err());
    assert!(postfix::validate_local_part("user@domain").is_err());
    assert!(postfix::validate_local_part("../escape").is_err());
    assert!(postfix::validate_local_part("back\\slash").is_err());
}

#[test]
fn all_mailbox_kinds_produce_sieve() {
    let kinds = [
        ("noreply", MailboxKind::Service),
        ("admin", MailboxKind::Service),
        ("router", MailboxKind::Router),
        ("tim", MailboxKind::User),
        ("legal", MailboxKind::Legal),
    ];
    for (local, kind) in &kinds {
        let mb = Mailbox {
            local_part: local.to_string(),
            kind: *kind,
            display_name: None,
        };
        let script = sieve::generate_sieve(&mb, "test.com");
        assert!(!script.is_empty(), "Sieve script empty for {}", local);
        assert!(
            script.contains("require"),
            "Sieve missing require for {}",
            local
        );
    }
}
