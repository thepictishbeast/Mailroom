# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project scaffold with workspace structure
- Core library with tests
- CI/CD pipeline
- `mail-config::mailbox_layout` — typed `MailboxLayout` emits Dovecot
  `15-mailboxes.conf` with IMAP special-use folders (Drafts/Sent/Trash/
  Junk/Archive/Important) plus Gmail-style category folders (Updates/
  Social/Promotions/Forums). Codifies the deployed VPS layout.
- `mail-config::categories` — typed `CategoryRule` AST with score-based
  ordering, optional `editheader` audit-tagging, in-process evaluator
  (`CategoryRules::evaluate`), and Sieve emitter (`to_sieve` /
  `to_sieve_with`). Same struct is the wire format consumed by the
  Thundercrab desktop client and the federated suggestion ledger.
- `mail-config::dovecot::generate_sieve_runtime_conf` — emits Dovecot
  2.4 `90-sieve.conf` (sieve_script blocks + protocol lmtp/imap
  mail_plugins). Optional `enable_editheader` opts into audit headers.
- `examples/dump_categories_sieve.rs` — round-trip dev aid; emitted
  script is verified to compile under Pigeonhole's `sievec`.

## [0.1.0] - 2026-04-04

### Added
- Initial release
