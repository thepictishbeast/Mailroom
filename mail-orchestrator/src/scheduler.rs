//! Cron-based scheduled email sender.
//!
//! Periodically checks the database for emails that are due
//! and sends them via the SMTP sender. Supports one-time and
//! recurring (cron-expression) schedules.

use crate::db::Database;
use crate::sender::Sender;
use anyhow::Result;
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Run the scheduler loop. Checks for due emails every 60 seconds.
pub async fn run_scheduler(db: Arc<Database>, sender: Arc<Sender>) -> Result<()> {
    info!("Scheduler started, checking every 60 seconds");

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;

        match process_due_emails(&db, &sender) {
            Ok(count) => {
                if count > 0 {
                    info!(count, "Processed scheduled emails");
                }
            }
            Err(e) => {
                error!(error = %e, "Scheduler error");
            }
        }
    }
}

/// Process all emails that are due for sending.
fn process_due_emails(db: &Database, sender: &Sender) -> Result<usize> {
    let due = db.get_due_scheduled()?;
    let count = due.len();

    for email in due {
        match sender.send_email(&email.from_addr, None, &email.to_addr, &email.subject, &email.body) {
            Ok(_) => {
                if email.is_recurring {
                    if let Some(ref cron_expr) = email.cron_expr {
                        match next_cron_time(cron_expr) {
                            Some(next) => {
                                db.reschedule(email.id, &next)?;
                                info!(
                                    tracking_id = %email.tracking_id,
                                    next_run = %next,
                                    "Recurring email sent, rescheduled"
                                );
                            }
                            None => {
                                db.mark_scheduled_sent(email.id)?;
                                warn!(
                                    tracking_id = %email.tracking_id,
                                    "Recurring email sent but no next cron time found"
                                );
                            }
                        }
                    } else {
                        db.mark_scheduled_sent(email.id)?;
                    }
                } else {
                    db.mark_scheduled_sent(email.id)?;
                    info!(
                        tracking_id = %email.tracking_id,
                        "Scheduled email sent"
                    );
                }
            }
            Err(e) => {
                error!(
                    tracking_id = %email.tracking_id,
                    error = %e,
                    "Failed to send scheduled email"
                );
            }
        }
    }

    Ok(count)
}

/// Calculate the next run time from a cron expression.
fn next_cron_time(cron_expr: &str) -> Option<String> {
    // The cron crate expects 7-field expressions (sec min hour dom mon dow year)
    // Pad with "0" seconds if the expression has 5 fields
    let padded = if cron_expr.split_whitespace().count() == 5 {
        format!("0 {}", cron_expr)
    } else {
        cron_expr.to_string()
    };

    match Schedule::from_str(&padded) {
        Ok(schedule) => {
            schedule.upcoming(Utc).next().map(|dt| dt.to_rfc3339())
        }
        Err(e) => {
            warn!(cron = %cron_expr, error = %e, "Invalid cron expression");
            None
        }
    }
}
