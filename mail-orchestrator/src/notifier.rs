//! Notification dispatcher for service mailbox events.
//!
//! When a new email arrives in a monitored service mailbox,
//! this module sends notification alerts to all configured subscribers.

use crate::config::NotifyConfig;
use crate::db::Database;
use crate::parser::ParsedEmail;
use crate::sender::Sender;
use anyhow::Result;
use tracing::{info, warn};
use uuid::Uuid;

/// Dispatch notifications for a new email in a service mailbox.
pub fn notify_subscribers(
    config: &NotifyConfig,
    email: &ParsedEmail,
    sender: &Sender,
    db: &Database,
    alerts_from: &str,
) -> Result<()> {
    let tracking_id = Uuid::new_v4().to_string();
    let mailbox_name = config.mailbox.split('@').next().unwrap_or(&config.mailbox);

    // Log the inbound email
    db.log_email(
        &email.message_id,
        &tracking_id,
        "inbound",
        &email.from,
        &email.to,
        Some(&email.subject),
        &config.mailbox,
        "received",
    )?;

    // Check if this mailbox is log_only (e.g., noreply bounce capture)
    if config.actions.contains(&"log_only".to_string()) {
        info!(mailbox = %config.mailbox, "Log-only mailbox, skipping notifications");
        return Ok(());
    }

    for subscriber in &config.subscribers {
        let notif_id = db.log_notification(
            &config.mailbox,
            &email.message_id,
            subscriber,
            &config.priority,
            "pending",
        )?;

        match sender.send_notification(
            alerts_from,
            subscriber,
            mailbox_name,
            &config.priority,
            &email.from,
            &email.subject,
            &email.body_preview,
            &tracking_id,
        ) {
            Ok(_) => {
                db.update_notification_status(notif_id, "sent", None)?;
                info!(
                    subscriber,
                    mailbox = %config.mailbox,
                    priority = %config.priority,
                    "Notification sent"
                );
            }
            Err(e) => {
                let err_msg = format!("{:#}", e);
                db.update_notification_status(notif_id, "failed", Some(&err_msg))?;
                warn!(
                    subscriber,
                    mailbox = %config.mailbox,
                    error = %e,
                    "Failed to send notification"
                );
            }
        }
    }

    Ok(())
}
