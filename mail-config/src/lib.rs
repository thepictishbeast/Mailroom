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
pub mod mailbox_layout;
pub mod categories;

pub use categories::{Action, AuditTag, CategoryRule, CategoryRules, MatchExpr, MessageContext};
pub use domain::{Domain, Mailbox, MailboxKind};
pub use error::ConfigError;
pub use mailbox_layout::{LayoutFolder, MailboxLayout, SpecialUse};

/// Result type alias for mail-config operations.
pub type Result<T> = std::result::Result<T, ConfigError>;
