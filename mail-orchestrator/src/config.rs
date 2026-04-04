//! TOML configuration deserialization for the mail orchestrator.
//!
//! Loads daemon settings, domain info, router config, notification
//! subscribers, scheduled sends, and template paths from a single TOML file.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level orchestrator configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub domain: DomainConfig,
    pub router: RouterConfig,
    pub templates: TemplateConfig,
    #[serde(default)]
    pub notify: HashMap<String, NotifyConfig>,
    #[serde(default)]
    pub schedule: Vec<ScheduleConfig>,
}

/// Daemon runtime settings.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
    pub pid_file: PathBuf,
    pub db_path: PathBuf,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

/// Mail domain and server settings.
#[derive(Debug, Clone, Deserialize)]
pub struct DomainConfig {
    pub name: String,
    pub mail_base: PathBuf,
    #[serde(default = "default_smtp_host")]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
}

/// Router mailbox configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    pub mailbox: String,
    pub maildir: PathBuf,
    pub authorized_senders: Vec<String>,
    pub allowed_from: Vec<String>,
}

/// Template engine configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateConfig {
    pub dir: PathBuf,
}

/// Per-mailbox notification routing.
#[derive(Debug, Clone, Deserialize)]
pub struct NotifyConfig {
    pub mailbox: String,
    pub maildir: PathBuf,
    pub subscribers: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Scheduled email definition.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleConfig {
    pub name: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub cron: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get all Maildir paths that should be watched (router + all notify mailboxes).
    pub fn watch_paths(&self) -> Vec<(String, PathBuf)> {
        let mut paths = vec![("router".to_string(), self.router.maildir.clone())];
        for (name, notify) in &self.notify {
            paths.push((name.clone(), notify.maildir.clone()));
        }
        paths
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_smtp_host() -> String {
    "127.0.0.1".to_string()
}

fn default_smtp_port() -> u16 {
    25
}

fn default_priority() -> String {
    "normal".to_string()
}

fn default_true() -> bool {
    true
}
