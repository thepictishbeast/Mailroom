//! Mail Orchestrator — Standalone email orchestration daemon.
//!
//! Watches Maildir directories for new emails, dispatches notifications
//! to subscribers, processes router commands for multi-identity sending,
//! and handles scheduled email delivery.
//!
//! Designed for Postfix + Dovecot mail servers. Domain-agnostic — all
//! configuration is in a TOML file.

mod config;
mod db;
mod notifier;
mod parser;
mod router;
mod scheduler;
mod sender;
mod watcher;

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

/// Mail Orchestrator — email routing, notifications, and scheduling daemon.
#[derive(Parser, Debug)]
#[command(name = "mail-orchestrator", version, about)]
struct Cli {
    /// Path to the orchestrator TOML config file.
    #[arg(short, long, default_value = "/etc/mail-orchestrator/orchestrator.toml")]
    config: PathBuf,

    /// Validate config and exit without starting the daemon.
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load configuration
    let config = config::Config::load(&cli.config)?;

    // Initialize logging
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.daemon.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    info!(
        config = %cli.config.display(),
        domain = %config.domain.name,
        "Mail Orchestrator starting"
    );

    // Config check mode
    if cli.check {
        info!("Configuration valid");
        info!("  Domain: {}", config.domain.name);
        info!("  SMTP: {}:{}", config.domain.smtp_host, config.domain.smtp_port);
        info!("  Router maildir: {}", config.router.maildir.display());
        info!("  Authorized senders: {:?}", config.router.authorized_senders);
        info!("  Notification mailboxes: {}", config.notify.len());
        info!("  Scheduled emails: {}", config.schedule.len());
        return Ok(());
    }

    // Ensure PID file directory exists
    if let Some(parent) = config.daemon.pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write PID file
    std::fs::write(&config.daemon.pid_file, std::process::id().to_string())?;

    // Open database
    if let Some(parent) = config.daemon.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Arc::new(db::Database::open(&config.daemon.db_path)?);
    info!(path = %config.daemon.db_path.display(), "Database opened");

    // Create SMTP sender
    let smtp_sender = Arc::new(sender::Sender::new(
        &config.domain.smtp_host,
        config.domain.smtp_port,
    )?);
    info!(
        host = %config.domain.smtp_host,
        port = config.domain.smtp_port,
        "SMTP sender initialized"
    );

    let config = Arc::new(config);

    // Spawn scheduler task
    let sched_db = db.clone();
    let sched_sender = smtp_sender.clone();
    let scheduler_handle = tokio::spawn(async move {
        if let Err(e) = scheduler::run_scheduler(sched_db, sched_sender).await {
            error!(error = %e, "Scheduler task failed");
        }
    });

    // Spawn Maildir watcher (this is the main event loop)
    let watch_config = config.clone();
    let watch_db = db.clone();
    let watch_sender = smtp_sender.clone();
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = watcher::watch_maildirs(watch_config, watch_db, watch_sender).await {
            error!(error = %e, "Maildir watcher failed");
        }
    });

    info!("Mail Orchestrator running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, cleaning up...");

    // Clean up PID file
    let _ = std::fs::remove_file(&Arc::try_unwrap(config).unwrap_or_else(|c| (*c).clone()).daemon.pid_file);

    // Abort tasks
    scheduler_handle.abort();
    watcher_handle.abort();

    info!("Mail Orchestrator stopped");
    Ok(())
}
