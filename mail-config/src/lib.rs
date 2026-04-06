//! mail-config — Programmatic management of mail server configurations.
//!
//! Provides a typed interface for reading, validating, and writing
//! Postfix, Dovecot, OpenDKIM, and Sieve configurations. Eliminates
//! manual config file editing and ensures consistency across components.

#![forbid(unsafe_code)]

pub mod postfix;
pub mod dovecot;
pub mod dkim;
pub mod sieve;
pub mod domain;
pub mod error;

pub use domain::{Domain, Mailbox, MailboxKind};
pub use error::ConfigError;

/// Result type alias for mail-config operations.
pub type Result<T> = std::result::Result<T, ConfigError>;
