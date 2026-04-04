-- Mail Orchestrator — Initial Schema
-- Tracks all email events, notifications, and scheduled sends

CREATE TABLE IF NOT EXISTS email_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id      TEXT NOT NULL,
    tracking_id     TEXT NOT NULL,
    direction       TEXT NOT NULL CHECK (direction IN ('inbound','outbound','internal','notification')),
    from_addr       TEXT NOT NULL,
    to_addr         TEXT NOT NULL,
    subject         TEXT,
    mailbox         TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'received'
                    CHECK (status IN ('received','processing','sent','delivered','bounced','rejected','failed')),
    router_command  TEXT,
    template_used   TEXT,
    scheduled_at    TEXT,
    processed_at    TEXT,
    error_message   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_email_log_mailbox ON email_log(mailbox, created_at);
CREATE INDEX IF NOT EXISTS idx_email_log_status ON email_log(status);
CREATE INDEX IF NOT EXISTS idx_email_log_from ON email_log(from_addr);
CREATE INDEX IF NOT EXISTS idx_email_log_tracking ON email_log(tracking_id);

CREATE TABLE IF NOT EXISTS notification_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source_mailbox  TEXT NOT NULL,
    source_message_id TEXT NOT NULL,
    subscriber      TEXT NOT NULL,
    priority        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','sent','failed')),
    sent_at         TEXT,
    error_message   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_notif_source ON notification_log(source_mailbox, created_at);
CREATE INDEX IF NOT EXISTS idx_notif_status ON notification_log(status);

CREATE TABLE IF NOT EXISTS scheduled_emails (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    tracking_id     TEXT NOT NULL,
    from_addr       TEXT NOT NULL,
    to_addr         TEXT NOT NULL,
    subject         TEXT NOT NULL,
    body            TEXT NOT NULL,
    template        TEXT,
    template_vars   TEXT,
    scheduled_at    TEXT NOT NULL,
    cron_expr       TEXT,
    is_recurring    INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','sent','cancelled','failed')),
    last_run_at     TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sched_at ON scheduled_emails(scheduled_at, status);
CREATE INDEX IF NOT EXISTS idx_sched_tracking ON scheduled_emails(tracking_id);
