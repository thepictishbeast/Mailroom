//! Print the *deployed* (deliberately minimal) category Sieve script to
//! stdout — the ruleset actually running on the live server
//! (`CategoryRules::deployed()`): spam → Junk, 2FA flagged-but-kept, and
//! everything else falling through to INBOX.
//!
//! Use this (not `dump_categories_sieve`, which emits the full `default()`
//! catalogue) to regenerate `/etc/dovecot/sieve/categories.sieve`:
//!
//!   cargo run -q -p mail-config --example dump_deployed_sieve > categories.sieve
//!   sievec categories.sieve   # compile-check before installing

fn main() {
    print!("{}", mail_config::CategoryRules::deployed().to_sieve());
}
