//! SQLite database for audit logging and delivery tracking.
//!
//! All email events (inbound, outbound, notifications, router commands)
//! are logged here for auditing and debugging.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

/// Database handle wrapping a SQLite connection behind a Mutex for thread safety.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) the database and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let schema = include_str!("../../migrations/001_initial.sql");
        conn.execute_batch(schema)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Log an email event.
    pub fn log_email(
        &self,
        message_id: &str,
        tracking_id: &str,
        direction: &str,
        from: &str,
        to: &str,
        subject: Option<&str>,
        mailbox: &str,
        status: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO email_log (message_id, tracking_id, direction, from_addr, to_addr, subject, mailbox, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![message_id, tracking_id, direction, from, to, subject, mailbox, status],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update email log status.
    pub fn update_email_status(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE email_log SET status = ?1, error_message = ?2, processed_at = datetime('now') WHERE id = ?3",
            rusqlite::params![status, error, id],
        )?;
        Ok(())
    }

    /// Log a notification dispatch.
    pub fn log_notification(
        &self,
        source_mailbox: &str,
        source_message_id: &str,
        subscriber: &str,
        priority: &str,
        status: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO notification_log (source_mailbox, source_message_id, subscriber, priority, status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![source_mailbox, source_message_id, subscriber, priority, status],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update notification status.
    pub fn update_notification_status(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE notification_log SET status = ?1, error_message = ?2, sent_at = datetime('now') WHERE id = ?3",
            rusqlite::params![status, error, id],
        )?;
        Ok(())
    }

    /// Insert a scheduled email.
    pub fn insert_scheduled(
        &self,
        tracking_id: &str,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
        template: Option<&str>,
        template_vars: Option<&str>,
        scheduled_at: &str,
        cron_expr: Option<&str>,
        is_recurring: bool,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO scheduled_emails (tracking_id, from_addr, to_addr, subject, body, template, template_vars, scheduled_at, cron_expr, is_recurring)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![tracking_id, from, to, subject, body, template, template_vars, scheduled_at, cron_expr, is_recurring as i32],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all pending scheduled emails that are due.
    pub fn get_due_scheduled(&self) -> Result<Vec<ScheduledEmail>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tracking_id, from_addr, to_addr, subject, body, template, template_vars, scheduled_at, cron_expr, is_recurring
             FROM scheduled_emails
             WHERE status = 'pending' AND scheduled_at <= datetime('now')"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ScheduledEmail {
                id: row.get(0)?,
                tracking_id: row.get(1)?,
                from_addr: row.get(2)?,
                to_addr: row.get(3)?,
                subject: row.get(4)?,
                body: row.get(5)?,
                template: row.get(6)?,
                template_vars: row.get(7)?,
                scheduled_at: row.get(8)?,
                cron_expr: row.get(9)?,
                is_recurring: row.get::<_, i32>(10)? != 0,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Mark a scheduled email as sent.
    pub fn mark_scheduled_sent(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE scheduled_emails SET status = 'sent', last_run_at = datetime('now') WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// For recurring scheduled emails, reset to pending with next run time.
    pub fn reschedule(&self, id: i64, next_at: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE scheduled_emails SET scheduled_at = ?1, last_run_at = datetime('now') WHERE id = ?2",
            rusqlite::params![next_at, id],
        )?;
        Ok(())
    }
}

/// A scheduled email record from the database.
#[derive(Debug)]
pub struct ScheduledEmail {
    pub id: i64,
    pub tracking_id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub subject: String,
    pub body: String,
    pub template: Option<String>,
    pub template_vars: Option<String>,
    pub scheduled_at: String,
    pub cron_expr: Option<String>,
    pub is_recurring: bool,
}
