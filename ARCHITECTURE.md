# Architecture

## Overview

A self-hosted email stack combining Postfix, Dovecot, and security tooling with a custom Rust workspace (mail-orchestrator, mail-config, mail-cli) that handles notification routing, configuration management, and administrative operations.

## System Diagram

```
               Inbound SMTP              Outbound SMTP
                    |                          ^
                    v                          |
+---------------------------------------------------+
|  +----------+   +---------+   +----------+        |
|  | Postfix  |-->| Dovecot |-->| Roundcube|        |
|  | (MTA)    |   | (IMAP/  |   | (Webmail)|        |
|  |          |   |  LMTP)  |   +----------+        |
|  +----------+   +---------+                       |
|       |              |             +----------+   |
|       |         +--------+        | OpenDKIM  |   |
|       |         | Sieve  |        | (signing) |   |
|       |         +--------+        +----------+   |
+---------------------------------------------------+
               |
               v
+---------------------------------------------------+
|          Rust Workspace                            |
|                                                    |
|  +------------------+  +-----------------+         |
|  | mail-orchestrator|  | mail-config     |         |
|  | (daemon)         |  | (library)       |         |
|  | - watcher        |  | - postfix.rs    |         |
|  | - parser         |  | - dovecot.rs    |         |
|  | - router         |  | - dkim.rs       |         |
|  | - notifier       |  | - sieve.rs      |         |
|  | - sender         |  | - domain.rs     |         |
|  | - scheduler      |  +-----------------+         |
|  | - db (SQLite)    |                              |
|  +------------------+  +-----------------+         |
|                        | mail-cli        |         |
|  +------------------+  | (binary)        |         |
|  | Security Layer   |  | - admin commands|         |
|  | - Fail2Ban (IDS) |  +-----------------+         |
|  | - ClamAV (AV)    |                              |
|  | - TLS everywhere |                              |
|  +------------------+                              |
+---------------------------------------------------+
```

## Data Flow

1. **Inbound mail:** Postfix receives SMTP, validates SPF/DKIM/DMARC, delivers to Dovecot via LMTP. Dovecot writes to per-user Maildir. Sieve filters apply server-side rules.
2. **Orchestrator watch:** mail-orchestrator watches Maildir directories via inotify. On new arrival, it parses headers (mail-parser), matches against notification routes, and dispatches alerts to subscribers.
3. **Router commands:** Authorized senders email the router mailbox with send-as commands. The router validates authorization, constructs the message with the requested From identity, and sends via lettre.
4. **Scheduled delivery:** The scheduler fires on cron expressions, renders MiniJinja templates, and sends via SMTP. Digests, reports, and recurring notifications.
5. **Audit trail:** Every action (receive, notify, route, send, schedule) is logged to SQLite with timestamps and message IDs.
6. **Configuration:** mail-config provides typed Rust structs for Postfix, Dovecot, DKIM, and Sieve configuration. mail-cli uses these for administrative tasks.

## Key Design Decisions

- **Three-crate workspace.** mail-orchestrator (daemon), mail-config (config library), mail-cli (admin tool). Clear separation of runtime, configuration, and operations.
- **No aliases.** Every address is a real mailbox with its own Maildir and credentials.
- **inotify over polling.** Instant detection with zero CPU overhead during idle.
- **Domain-agnostic.** Single TOML config defines all behavior. Reusable for any Postfix+Dovecot deployment.

## Threat Model

**Defends against:** brute-force auth (Fail2Ban), email spoofing (DKIM+SPF+DMARC), malware (ClamAV), unauthorized send-as (router allowlist), audit gaps (SQLite log).

**Out of scope:** E2E message encryption (planned), Postfix/Dovecot zero-days, physical server compromise.

## Future Directions

- Sequoia-PGP integration for automatic encryption/decryption
- Shield integration for web-based admin dashboard
- Migration from Roundcube to custom Rust webmail UI
