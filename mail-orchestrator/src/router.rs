//! Router command parser and executor.
//!
//! Processes structured command emails sent to the router@ mailbox.
//! Authorized senders can instruct the orchestrator to send emails
//! as any allowed identity, with optional scheduling and templates.
//!
//! Command format (email body):
//! ```text
//! TO: someone@example.com
//! FROM: admin@sacredvote.org
//! SUBJECT: Your account has been verified
//! TEMPLATE: account_verified
//! VARS: name=John, date=2026-04-02
//! SCHEDULE: 2026-04-03T09:00:00Z
//! ---
//! Optional raw body here if no TEMPLATE specified.
//! ```

use crate::config::RouterConfig;
use crate::db::Database;
use crate::parser::{extract_address, ParsedEmail};
use crate::sender::Sender;
use anyhow::{bail, Result};
use regex::Regex;
use std::collections::HashMap;
use tracing::{info, warn};
use uuid::Uuid;

/// A parsed router command extracted from an email body.
#[derive(Debug)]
pub struct RouterCommand {
    pub to: String,
    pub from: String,
    pub subject: String,
    pub template: Option<String>,
    pub vars: HashMap<String, String>,
    pub schedule: Option<String>,
    pub body: String,
}

/// Parse a router command from the email body text.
pub fn parse_command(body: &str) -> Result<RouterCommand> {
    let mut to = String::new();
    let mut from = String::new();
    let mut subject = String::new();
    let mut template = None;
    let mut vars = HashMap::new();
    let mut schedule = None;
    let mut raw_body = String::new();
    let mut in_body = false;

    let key_re = Regex::new(r"^(TO|FROM|SUBJECT|TEMPLATE|VARS|SCHEDULE|ATTACH):\s*(.+)$")?;

    for line in body.lines() {
        if in_body {
            raw_body.push_str(line);
            raw_body.push('\n');
            continue;
        }

        if line.trim() == "---" {
            in_body = true;
            continue;
        }

        if let Some(caps) = key_re.captures(line.trim()) {
            let key = caps.get(1).unwrap().as_str();
            let value = caps.get(2).unwrap().as_str().trim().to_string();

            match key {
                "TO" => to = value,
                "FROM" => from = value,
                "SUBJECT" => subject = value,
                "TEMPLATE" => template = Some(value),
                "VARS" => {
                    for pair in value.split(',') {
                        let pair = pair.trim();
                        if let Some((k, v)) = pair.split_once('=') {
                            vars.insert(k.trim().to_string(), v.trim().to_string());
                        }
                    }
                }
                "SCHEDULE" => schedule = Some(value),
                _ => {}
            }
        }
    }

    if to.is_empty() {
        bail!("Router command missing TO: field");
    }
    if from.is_empty() {
        bail!("Router command missing FROM: field");
    }
    if subject.is_empty() {
        bail!("Router command missing SUBJECT: field");
    }

    Ok(RouterCommand {
        to,
        from,
        subject,
        template,
        vars,
        schedule,
        body: raw_body.trim_end().to_string(),
    })
}

/// Execute a router command: validate authorization, then send or schedule.
pub fn execute_command(
    email: &ParsedEmail,
    config: &RouterConfig,
    sender: &Sender,
    db: &Database,
) -> Result<()> {
    let sender_addr = extract_address(&email.from).to_lowercase();
    let tracking_id = Uuid::new_v4().to_string();

    // Authorization check
    let authorized = config
        .authorized_senders
        .iter()
        .any(|a| a.to_lowercase() == sender_addr);

    if !authorized {
        warn!(
            sender = %sender_addr,
            "Unauthorized router command attempt"
        );
        db.log_email(
            &email.message_id,
            &tracking_id,
            "inbound",
            &email.from,
            &email.to,
            Some(&email.subject),
            "router",
            "rejected",
        )?;
        return Ok(());
    }

    // Parse the command from the email body
    let cmd = match parse_command(&email.body_text) {
        Ok(cmd) => cmd,
        Err(e) => {
            warn!(error = %e, "Failed to parse router command");
            db.log_email(
                &email.message_id,
                &tracking_id,
                "inbound",
                &email.from,
                &email.to,
                Some(&email.subject),
                "router",
                "failed",
            )?;
            // Send error reply to sender
            let _ = sender.send_email(
                "router@sacredvote.org",
                Some("Mail Orchestrator"),
                &sender_addr,
                "Router Command Failed",
                &format!("Your router command could not be parsed.\n\nError: {}\n\nTracking ID: {}", e, tracking_id),
            );
            return Ok(());
        }
    };

    // Validate FROM identity is in allowed list
    let from_allowed = config
        .allowed_from
        .iter()
        .any(|a| a.to_lowercase() == cmd.from.to_lowercase());

    if !from_allowed {
        warn!(
            requested_from = %cmd.from,
            "Router command requested unauthorized FROM identity"
        );
        let _ = sender.send_email(
            "router@sacredvote.org",
            Some("Mail Orchestrator"),
            &sender_addr,
            "Router Command Rejected",
            &format!("Identity '{}' is not in the allowed sender list.\n\nTracking ID: {}", cmd.from, tracking_id),
        );
        return Ok(());
    }

    // Log the inbound router command
    let log_id = db.log_email(
        &email.message_id,
        &tracking_id,
        "inbound",
        &email.from,
        &email.to,
        Some(&email.subject),
        "router",
        "processing",
    )?;

    // Handle scheduled sends
    if let Some(ref schedule_at) = cmd.schedule {
        let body = if cmd.body.is_empty() {
            "(template-based email)".to_string()
        } else {
            cmd.body.clone()
        };

        db.insert_scheduled(
            &tracking_id,
            &cmd.from,
            &cmd.to,
            &cmd.subject,
            &body,
            cmd.template.as_deref(),
            Some(&serde_json::to_string(&cmd.vars).unwrap_or_default()),
            schedule_at,
            None,
            false,
        )?;

        db.update_email_status(log_id, "sent", None)?;

        info!(
            from = %cmd.from,
            to = %cmd.to,
            scheduled_at = %schedule_at,
            tracking_id,
            "Router command: email scheduled"
        );

        // Send acknowledgment
        let _ = sender.send_email(
            "router@sacredvote.org",
            Some("Mail Orchestrator"),
            &sender_addr,
            &format!("Scheduled: {}", cmd.subject),
            &format!(
                "Your email has been scheduled.\n\nFrom: {}\nTo: {}\nSubject: {}\nScheduled: {}\n\nTracking ID: {}",
                cmd.from, cmd.to, cmd.subject, schedule_at, tracking_id
            ),
        );

        return Ok(());
    }

    // Immediate send
    let body = if cmd.body.is_empty() {
        "(no body provided)".to_string()
    } else {
        cmd.body.clone()
    };

    match sender.send_email(&cmd.from, None, &cmd.to, &cmd.subject, &body) {
        Ok(_) => {
            db.update_email_status(log_id, "sent", None)?;

            // Log the outbound email
            db.log_email(
                &format!("router-{}", tracking_id),
                &tracking_id,
                "outbound",
                &cmd.from,
                &cmd.to,
                Some(&cmd.subject),
                "router",
                "sent",
            )?;

            info!(
                from = %cmd.from,
                to = %cmd.to,
                subject = %cmd.subject,
                tracking_id,
                "Router command: email sent"
            );

            // Send acknowledgment
            let _ = sender.send_email(
                "router@sacredvote.org",
                Some("Mail Orchestrator"),
                &sender_addr,
                &format!("Sent: {}", cmd.subject),
                &format!(
                    "Your email has been sent.\n\nFrom: {}\nTo: {}\nSubject: {}\n\nTracking ID: {}",
                    cmd.from, cmd.to, cmd.subject, tracking_id
                ),
            );
        }
        Err(e) => {
            let err_msg = format!("{:#}", e);
            db.update_email_status(log_id, "failed", Some(&err_msg))?;
            warn!(error = %e, tracking_id, "Router command: send failed");
        }
    }

    Ok(())
}
