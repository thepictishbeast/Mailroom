//! Integration tests for the mail-orchestrator database module.
//!
//! Tests use in-memory SQLite databases — no disk I/O needed.

// We can't directly import private modules, so we test through the binary.
// Instead, we test the SQL schema directly with rusqlite.

#[test]
fn schema_creates_all_tables() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let schema = include_str!("../../migrations/001_initial.sql");
    conn.execute_batch(schema).unwrap();

    // Verify all tables exist
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(tables.contains(&"email_log".to_string()));
    assert!(tables.contains(&"notification_log".to_string()));
    assert!(tables.contains(&"scheduled_emails".to_string()));
}

#[test]
fn email_log_insert_and_query() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_initial.sql")).unwrap();

    conn.execute(
        "INSERT INTO email_log (message_id, tracking_id, direction, from_addr, to_addr, subject, mailbox, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params!["msg-001", "trk-001", "inbound", "sender@example.com", "admin@sacred.vote", "Test", "admin", "received"],
    ).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM email_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    // Verify status constraint
    let result = conn.execute(
        "INSERT INTO email_log (message_id, tracking_id, direction, from_addr, to_addr, mailbox, status)
         VALUES ('m', 't', 'inbound', 'a', 'b', 'x', 'invalid_status')",
        [],
    );
    assert!(result.is_err(), "Should reject invalid status");
}

#[test]
fn email_log_direction_constraint() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_initial.sql")).unwrap();

    let result = conn.execute(
        "INSERT INTO email_log (message_id, tracking_id, direction, from_addr, to_addr, mailbox, status)
         VALUES ('m', 't', 'invalid_direction', 'a', 'b', 'x', 'received')",
        [],
    );
    assert!(result.is_err(), "Should reject invalid direction");
}

#[test]
fn scheduled_email_lifecycle() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_initial.sql")).unwrap();

    // Insert a scheduled email
    conn.execute(
        "INSERT INTO scheduled_emails (tracking_id, from_addr, to_addr, subject, body, scheduled_at)
         VALUES ('trk-sched', 'admin@sacred.vote', 'user@example.com', 'Reminder', 'Body text', '2026-01-01T00:00:00')",
        [],
    ).unwrap();

    // Verify it shows as pending
    let status: String = conn
        .query_row("SELECT status FROM scheduled_emails WHERE tracking_id = 'trk-sched'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "pending");

    // Mark as sent
    conn.execute(
        "UPDATE scheduled_emails SET status = 'sent', last_run_at = datetime('now') WHERE tracking_id = 'trk-sched'",
        [],
    ).unwrap();

    let status: String = conn
        .query_row("SELECT status FROM scheduled_emails WHERE tracking_id = 'trk-sched'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "sent");
}

#[test]
fn notification_log_insert() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_initial.sql")).unwrap();

    conn.execute(
        "INSERT INTO notification_log (source_mailbox, source_message_id, subscriber, priority, status)
         VALUES ('support@sacred.vote', 'msg-123', 'tim@sacred.vote', 'high', 'pending')",
        [],
    ).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM notification_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn schema_is_idempotent() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let schema = include_str!("../../migrations/001_initial.sql");
    // Running the schema twice should not fail (IF NOT EXISTS)
    conn.execute_batch(schema).unwrap();
    conn.execute_batch(schema).unwrap();
}

#[test]
fn indexes_exist() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../migrations/001_initial.sql")).unwrap();

    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(indexes.contains(&"idx_email_log_mailbox".to_string()));
    assert!(indexes.contains(&"idx_email_log_status".to_string()));
    assert!(indexes.contains(&"idx_email_log_tracking".to_string()));
    assert!(indexes.contains(&"idx_sched_at".to_string()));
    assert!(indexes.contains(&"idx_notif_source".to_string()));
}
