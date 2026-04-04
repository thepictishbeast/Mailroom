//! Error types for mail-config operations.

use thiserror::Error;

/// All errors that can occur during mail configuration operations.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// A required configuration file was not found at the expected path.
    #[error("config file not found: {path}")]
    FileNotFound { path: String },

    /// Failed to parse a configuration file.
    #[error("failed to parse {component} config at {path}: {reason}")]
    ParseError {
        component: String,
        path: String,
        reason: String,
    },

    /// A configuration value failed validation.
    #[error("invalid {field}: {reason}")]
    ValidationError { field: String, reason: String },

    /// Failed to write a configuration file.
    #[error("failed to write {path}: {source}")]
    WriteError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to reload a service after config change.
    #[error("failed to reload {service}: {reason}")]
    ReloadError { service: String, reason: String },

    /// I/O error wrapper.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
