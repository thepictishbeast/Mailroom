# Secure-Email-Server-and-UI

Run your own email server with military-grade encryption and zero dependence on Big Tech. This project provides a complete, self-hosted email stack with a Rust orchestration daemon, proven open-source mail components, and a webmail interface -- giving you full sovereignty over your communications.

## The Problem

Email remains the backbone of digital communication, yet most organizations surrender their email to providers who scan message contents for advertising, comply with mass surveillance requests without meaningful pushback, and can lock you out of your own communications at any time. For organizations handling sensitive data -- legal communications, activist coordination, journalistic sources -- trusting a third-party email provider is an unacceptable risk. Self-hosting email has historically been so complex that even technical teams avoid it. This project aims to change that.

## How It Works

The stack combines battle-tested open-source mail components with a custom Rust daemon that handles orchestration, notification routing, and health monitoring.

```
Inbound Mail                              Outbound Mail
     |                                         ^
     v                                         |
+----------+    +---------+    +-----------+   |
| Postfix  |--->| Dovecot |--->| Roundcube |   |
| (MTA)    |    | (IMAP/  |    | (Webmail) |   |
|          |    |  LMTP)  |    |           |   |
+----------+    +---------+    +-----------+   |
     |               |                         |
     |          +----------+                   |
     |          | Sieve    |                   |
     |          | Filters  |                   |
     |          +----------+                   |
     |                                         |
     v                                         |
+------------------+                    +----------+
| mail-orchestrator|    (Rust daemon)   | OpenDKIM |
| - Routing        |                    | (DKIM    |
| - Notifications  |                    |  signing)|
| - Health checks  |                    +----------+
| - Audit logging  |
+------------------+
     |
     v
+------------------+
| Security Layer   |
| - Fail2Ban (IDS) |
| - ClamAV (AV)   |
| - TLS everywhere |
+------------------+
```

**Components:**

- **Postfix** -- Mail Transfer Agent. Handles SMTP, relay, virtual mailbox routing.
- **Dovecot** -- IMAP server with LMTP delivery. Manages mailboxes, authentication, and shared namespaces.
- **OpenDKIM** -- DKIM signing for outbound mail. Ensures recipient servers can verify message authenticity.
- **mail-orchestrator** -- Custom Rust daemon that manages notification routing to subscribers, health monitoring, and audit logging.
- **Roundcube** -- Webmail UI for browser-based access.
- **Sieve/ManageSieve** -- Server-side mail filtering. Auto-responders, notification rules, spam routing.
- **Fail2Ban** -- Intrusion detection. Blocks brute-force authentication attempts.
- **ClamAV** -- Antivirus scanning for attachments.

**Security posture:**

- DKIM, SPF, and DMARC enforced on all domains.
- TLS required for all connections (SMTP, IMAP, webmail).
- Per-user Maildir storage with filesystem-level access control.
- No aliases -- every address is a real mailbox with its own credentials.

## Current Status

| Component | Status |
|-----------|--------|
| Postfix (MTA) | ✅ Deployed, virtual mailboxes configured |
| Dovecot (IMAP/LMTP) | ✅ Deployed, Sieve filters active |
| OpenDKIM | ✅ Deployed, signing verified |
| DKIM/SPF/DMARC | ✅ All passing |
| TLS (Let's Encrypt) | ✅ Configured |
| Fail2Ban | ✅ Active |
| ClamAV | ✅ Active |
| Roundcube (webmail) | ✅ Deployed |
| Sieve filters | ✅ Deployed (noreply reject, notifications, auto-responders) |
| mail-orchestrator (Rust) | ✅ Scaffold complete, core daemon implemented |
| Deployment guide | 🚧 In progress |
| Configuration templates | 🚧 In progress |

## Quick Start

> **Note:** This repository is being assembled from a production deployment. Full automated setup is in progress.

```bash
git clone https://github.com/PlausiDen/Secure-Email-Server-and-UI.git
cd Secure-Email-Server-and-UI

# The mail-orchestrator Rust daemon:
cd mail-orchestrator
cargo build --release

# Deployment requires:
# - A VPS with ports 25, 587, 993, 443 open
# - A domain with DNS control (MX, SPF, DKIM, DMARC records)
# - TLS certificates (Let's Encrypt recommended)
# See docs/ for full deployment guide
```

## The PlausiDen Ecosystem

Secure-Email-Server-and-UI provides the email infrastructure for the PlausiDen ecosystem. Sacred.Vote uses it for voter notifications, ballot receipts, and administrative alerts. The mail-orchestrator daemon handles notification routing across all services, ensuring that alerts from monitoring, voting deadlines, and security events reach the right people through the right mailboxes.

Related repositories:
- [Sacred.Vote](https://github.com/PlausiDen/Sacred.Vote) -- Voting platform that sends notifications via this email stack
- [Shield](https://github.com/PlausiDen/Shield) -- Server admin panel with email monitoring integration
- [Vulnerability-Scanner](https://github.com/PlausiDen/Vulnerability-Scanner) -- Sends vulnerability alerts via email

## License

Licensed under Apache-2.0. See [LICENSE](LICENSE) for details.
