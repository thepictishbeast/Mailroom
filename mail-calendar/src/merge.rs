//! UID-keyed merge of calendar items.
//!
//! Calendar publishers (CalDAV, ICS-file, Google API, Microsoft
//! Graph) all need the same "have I seen this UID before?" semantic.
//! RFC 5545 mandates that an organizer re-sending an updated
//! invitation reuses the same UID; the receiver's job is to update
//! rather than duplicate. This module implements that merge once.
//!
//! ## Semantics
//!
//! For each incoming item:
//! - If its UID matches an existing item, the existing entry is
//!   **replaced** in place (same position in the list — preserves
//!   ordering for downstream renderers).
//! - If no match, the incoming item is **appended**.
//!
//! Items in `existing` whose UID does NOT appear in `incoming` are
//! preserved untouched — this is a partial sync, not a replace-all.
//! Use the writer (`write_ics`) directly if you want full
//! replacement.
//!
//! Standalone `VALARM`s (no UID) are always appended; we don't try
//! to dedupe them since the spec doesn't give us a stable identity.

use crate::ics::{parse_ics, write_ics, IcsError};
use crate::item::CalendarItem;
use std::collections::HashSet;

/// Per-incoming-item outcome of a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// New UID — added to the output.
    Added(String),
    /// Existing UID — replaced in place.
    Updated(String),
    /// Standalone VALARM with no UID — always appended.
    AppendedAlarm,
}

/// Summary returned by [`merge_by_uid`].
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    /// One entry per incoming item, in input order.
    pub outcomes: Vec<MergeOutcome>,
    /// The merged ICS document, ready to write to disk or PUT to a
    /// CalDAV endpoint.
    pub ics: String,
}

impl MergeReport {
    /// Number of UIDs added (new).
    #[must_use]
    pub fn added_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, MergeOutcome::Added(_)))
            .count()
    }

    /// Number of UIDs updated (replacing existing entries).
    #[must_use]
    pub fn updated_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, MergeOutcome::Updated(_)))
            .count()
    }

    /// Number of standalone alarms appended.
    #[must_use]
    pub fn alarms_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, MergeOutcome::AppendedAlarm))
            .count()
    }
}

/// Merge `incoming` into the calendar represented by the existing
/// ICS document `existing_ics`, keyed by UID.
///
/// Returns a [`MergeReport`] holding the per-item outcomes plus the
/// rewritten ICS document.
///
/// # Errors
///
/// Propagates any [`IcsError`] from parsing `existing_ics`. The
/// incoming items are not parsed (they're already typed); writing
/// them back doesn't fail.
pub fn merge_by_uid(
    existing_ics: &str,
    incoming: &[CalendarItem],
) -> Result<MergeReport, IcsError> {
    let mut existing = if existing_ics.trim().is_empty() {
        Vec::new()
    } else {
        parse_ics(existing_ics)?
    };

    let mut outcomes = Vec::with_capacity(incoming.len());
    let mut seen_existing_uids: HashSet<String> = existing
        .iter()
        .filter_map(item_uid)
        .map(str::to_string)
        .collect();

    for inc in incoming {
        let Some(uid) = item_uid(inc) else {
            // Standalone VALARMs: no UID, always append.
            existing.push(inc.clone());
            outcomes.push(MergeOutcome::AppendedAlarm);
            continue;
        };
        if seen_existing_uids.contains(uid) {
            // Replace the matching item in place.
            for slot in &mut existing {
                if item_uid(slot) == Some(uid) {
                    *slot = inc.clone();
                    break;
                }
            }
            outcomes.push(MergeOutcome::Updated(uid.to_string()));
        } else {
            seen_existing_uids.insert(uid.to_string());
            existing.push(inc.clone());
            outcomes.push(MergeOutcome::Added(uid.to_string()));
        }
    }

    Ok(MergeReport {
        outcomes,
        ics: write_ics(&existing),
    })
}

/// UID accessor that abstracts over the variants. Standalone alarms
/// have no UID per RFC 5545 (it's an event/todo property); returns
/// `None` for those.
fn item_uid(item: &CalendarItem) -> Option<&str> {
    match item {
        CalendarItem::Event(e) => Some(&e.uid),
        CalendarItem::Todo(t) => Some(&t.uid),
        CalendarItem::Alarm(_) => None,
    }
}

/// File-backed UID-keyed merge — read the file (or treat as empty if
/// missing), merge `incoming`, write atomically (tempfile + rename).
///
/// The atomic write guarantees that a reader can never see a half-
/// written calendar — the file either holds the prior content or
/// the new content, never a partial blend.
///
/// # Errors
///
/// I/O errors reading or writing the file; ICS parse errors on the
/// existing content.
pub fn merge_to_file(
    path: &std::path::Path,
    incoming: &[CalendarItem],
) -> Result<MergeReport, MergeFileError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(MergeFileError::Io(e)),
    };
    let report = merge_by_uid(&existing, incoming).map_err(MergeFileError::Ics)?;

    // Atomic write — same dir as the target so the rename is atomic
    // (cross-fs renames are not).
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("calendar.ics"),
        std::process::id(),
    ));
    std::fs::write(&tmp, &report.ics).map_err(MergeFileError::Io)?;
    std::fs::rename(&tmp, path).map_err(MergeFileError::Io)?;
    Ok(report)
}

/// Errors from [`merge_to_file`].
#[derive(Debug, thiserror::Error)]
pub enum MergeFileError {
    /// I/O reading or writing the calendar file.
    #[error("calendar file io: {0}")]
    Io(#[from] std::io::Error),
    /// ICS parse error on the existing file content.
    #[error("calendar ics: {0}")]
    Ics(IcsError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{CalendarEvent, EventClass, EventStatus};
    use chrono::{DateTime, Utc};

    fn evt(uid: &str, summary: &str) -> CalendarItem {
        CalendarItem::Event(CalendarEvent {
            uid: uid.into(),
            summary: summary.into(),
            description: String::new(),
            start: "2026-05-01T14:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            end: "2026-05-01T15:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            all_day: false,
            location: String::new(),
            status: EventStatus::Confirmed,
            class: EventClass::Public,
            organizer: None,
            attendees: vec![],
            reminders: vec![],
        })
    }

    #[test]
    fn merge_into_empty_appends_all() {
        let r = merge_by_uid("", &[evt("a@x", "A"), evt("b@x", "B")]).unwrap();
        assert_eq!(r.added_count(), 2);
        assert_eq!(r.updated_count(), 0);
        assert!(r.ics.contains("UID:a@x"));
        assert!(r.ics.contains("UID:b@x"));
    }

    #[test]
    fn merge_replaces_matching_uid_in_place() {
        // Seed an ICS doc with two events, then update one.
        let seed = write_ics(&[evt("a@x", "old A"), evt("b@x", "B")]);
        let r = merge_by_uid(&seed, &[evt("a@x", "new A")]).unwrap();
        assert_eq!(r.updated_count(), 1);
        assert_eq!(r.added_count(), 0);
        assert!(r.ics.contains("SUMMARY:new A"));
        assert!(!r.ics.contains("SUMMARY:old A"));
        // B is preserved untouched.
        assert!(r.ics.contains("SUMMARY:B"));
    }

    #[test]
    fn merge_appends_unknown_uid() {
        let seed = write_ics(&[evt("a@x", "A")]);
        let r = merge_by_uid(&seed, &[evt("c@x", "C")]).unwrap();
        assert_eq!(r.added_count(), 1);
        assert!(r.ics.contains("SUMMARY:A"));
        assert!(r.ics.contains("SUMMARY:C"));
    }

    #[test]
    fn merge_preserves_unmatched_existing_items() {
        // Existing has 3, incoming has 1 (matching one of them) —
        // result keeps all three, with the match updated.
        let seed = write_ics(&[evt("a@x", "A"), evt("b@x", "B"), evt("c@x", "C")]);
        let r = merge_by_uid(&seed, &[evt("b@x", "B-updated")]).unwrap();
        assert_eq!(r.updated_count(), 1);
        assert!(r.ics.contains("SUMMARY:A"));
        assert!(r.ics.contains("SUMMARY:B-updated"));
        assert!(r.ics.contains("SUMMARY:C"));
        assert!(!r.ics.contains("SUMMARY:B\r\n"));
    }

    #[test]
    fn merge_outcomes_in_input_order() {
        let seed = write_ics(&[evt("a@x", "A")]);
        let r = merge_by_uid(
            &seed,
            &[evt("a@x", "A2"), evt("b@x", "B"), evt("a@x", "A3")],
        )
        .unwrap();
        // Input is [update, add, update-again].
        assert_eq!(
            r.outcomes,
            vec![
                MergeOutcome::Updated("a@x".into()),
                MergeOutcome::Added("b@x".into()),
                MergeOutcome::Updated("a@x".into()),
            ]
        );
        // Final state: a@x has the latest summary "A3"; b@x present.
        assert!(r.ics.contains("SUMMARY:A3"));
        assert!(!r.ics.contains("SUMMARY:A2"));
        assert!(r.ics.contains("SUMMARY:B"));
    }

    #[test]
    fn merge_alarm_has_no_uid_and_appends() {
        use crate::item::CalendarAlarm;
        let alarm = CalendarItem::Alarm(CalendarAlarm {
            trigger: "2026-05-02T10:00:00Z".parse().unwrap(),
            description: "Reminder".into(),
        });
        let r = merge_by_uid("", &[alarm]).unwrap();
        assert_eq!(r.alarms_count(), 1);
        assert_eq!(r.added_count(), 0);
    }

    #[test]
    fn merge_empty_existing_handles_whitespace_only() {
        // A whitespace-only or empty existing should not error.
        let r = merge_by_uid("   \n", &[evt("a@x", "A")]).unwrap();
        assert_eq!(r.added_count(), 1);
    }

    #[test]
    fn merge_to_file_creates_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cal.ics");
        let r = merge_to_file(&path, &[evt("a@x", "A")]).unwrap();
        assert_eq!(r.added_count(), 1);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("UID:a@x"));
    }

    #[test]
    fn merge_to_file_atomic_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cal.ics");
        // First write
        merge_to_file(&path, &[evt("a@x", "old")]).unwrap();
        // Second write updates the same UID
        let r = merge_to_file(&path, &[evt("a@x", "new")]).unwrap();
        assert_eq!(r.updated_count(), 1);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("SUMMARY:new"));
        assert!(!body.contains("SUMMARY:old"));
    }

    #[test]
    fn merge_idempotent_when_incoming_unchanged() {
        // Merging the same item twice should produce the same final
        // ICS body (modulo regenerated DTSTAMP).
        let seed = write_ics(&[evt("a@x", "A")]);
        let pass1 = merge_by_uid(&seed, &[evt("a@x", "A2")]).unwrap();
        let pass2 = merge_by_uid(&pass1.ics, &[evt("a@x", "A2")]).unwrap();
        // Body content equal modulo DTSTAMP timing.
        let strip_dtstamp = |s: &str| {
            s.lines()
                .filter(|l| !l.starts_with("DTSTAMP:"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(strip_dtstamp(&pass1.ics), strip_dtstamp(&pass2.ics));
    }
}
