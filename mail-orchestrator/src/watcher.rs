//! Maildir watcher using inotify (via the `notify` crate).
//!
//! Monitors Maildir/new/ directories for new email files.
//! When a file appears, it is parsed and dispatched to the
//! appropriate handler (router or notifier).

use crate::config::Config;
use crate::db::Database;
use crate::notifier;
use crate::parser;
use crate::router;
use crate::sender::Sender;
use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Start watching all configured Maildir/new/ directories.
pub async fn watch_maildirs(config: Arc<Config>, db: Arc<Database>, sender: Arc<Sender>) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<(String, PathBuf)>(256);

    // Set up filesystem watcher
    let tx_clone = tx.clone();
    let config_clone = config.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                // Dovecot uses Maildir convention: write to tmp/, rename to new/.
                // This generates MOVED_TO (inotify), which the notify crate maps to
                // either Create or Modify(Name(RenameMode::To)) depending on version.
                // Match both to ensure we catch all new mail deliveries.
                let dominated = matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(notify::event::ModifyKind::Name(
                            notify::event::RenameMode::To
                        ))
                        | EventKind::Modify(notify::event::ModifyKind::Name(
                            notify::event::RenameMode::Any
                        ))
                );
                if dominated {
                    for path in event.paths {
                        // Determine which mailbox this belongs to
                        if let Some(mailbox) = identify_mailbox(&path, &config_clone) {
                            let _ = tx_clone.blocking_send((mailbox, path));
                        }
                    }
                }
            }
            Err(e) => error!(error = %e, "Filesystem watch error"),
        }
    })?;

    // Watch the router maildir
    let router_path = &config.router.maildir;
    if router_path.exists() {
        watcher.watch(router_path.as_ref(), RecursiveMode::NonRecursive)?;
        info!(path = %router_path.display(), "Watching router maildir");
    } else {
        warn!(path = %router_path.display(), "Router maildir does not exist");
    }

    // Watch all notification maildirs
    for (name, notify_config) in &config.notify {
        let path = &notify_config.maildir;
        if path.exists() {
            watcher.watch(path.as_ref(), RecursiveMode::NonRecursive)?;
            info!(mailbox = %name, path = %path.display(), "Watching notification maildir");
        } else {
            warn!(mailbox = %name, path = %path.display(), "Notification maildir does not exist");
        }
    }

    // Process events
    info!("Maildir watcher running, waiting for new emails...");

    // Keep watcher alive
    let _watcher = watcher;

    while let Some((mailbox_name, path)) = rx.recv().await {
        // Small delay to ensure file is fully written
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Skip non-regular files and dotfiles
        if !path.is_file() {
            continue;
        }
        if path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true) {
            continue;
        }

        info!(mailbox = %mailbox_name, file = %path.display(), "New email detected");

        match parser::parse_email(&path) {
            Ok(email) => {
                if mailbox_name == "router" {
                    if let Err(e) = router::execute_command(&email, &config.router, &sender, &db) {
                        error!(error = %e, "Router command execution failed");
                    }
                } else if let Some(notify_config) = config.notify.get(&mailbox_name) {
                    let alerts_from = format!(
                        "alerts@{}",
                        config.domain.name
                    );
                    if let Err(e) = notifier::notify_subscribers(
                        notify_config,
                        &email,
                        &sender,
                        &db,
                        &alerts_from,
                    ) {
                        error!(mailbox = %mailbox_name, error = %e, "Notification dispatch failed");
                    }
                }

                // Move processed file from new/ to cur/ (standard Maildir convention)
                move_to_cur(&path);
            }
            Err(e) => {
                error!(
                    file = %path.display(),
                    error = %e,
                    "Failed to parse email"
                );
            }
        }
    }

    Ok(())
}

/// Identify which mailbox a file belongs to based on its path.
fn identify_mailbox(path: &Path, config: &Config) -> Option<String> {
    let path_str = path.display().to_string();

    if path_str.contains(&config.router.maildir.display().to_string()) {
        return Some("router".to_string());
    }

    for (name, notify_config) in &config.notify {
        if path_str.contains(&notify_config.maildir.display().to_string()) {
            return Some(name.clone());
        }
    }

    None
}

/// Move a processed email from Maildir/new/ to Maildir/cur/.
fn move_to_cur(path: &Path) {
    if let Some(parent) = path.parent() {
        if parent.ends_with("new") {
            let cur_dir = parent.with_file_name("cur");
            if let Some(filename) = path.file_name() {
                let dest = cur_dir.join(filename);
                if let Err(e) = std::fs::rename(path, &dest) {
                    warn!(
                        src = %path.display(),
                        dest = %dest.display(),
                        error = %e,
                        "Failed to move email to cur/"
                    );
                }
            }
        }
    }
}
