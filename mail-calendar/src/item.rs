//! Typed calendar items.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level enum — one of the three kinds of calendar payload we
/// extract from email today. New variants need a doctrine review;
/// the assumption is downstream adapters do exhaustive `match`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalendarItem {
    /// VEVENT — a meeting, appointment, or scheduled occurrence.
    Event(CalendarEvent),
    /// VTODO — a task or reminder with no fixed time.
    Todo(CalendarTodo),
    /// VALARM — a standalone notification (rare; usually nested
    /// inside an Event or Todo, but RFC 5545 permits standalone).
    Alarm(CalendarAlarm),
}

/// A meeting / appointment / scheduled occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Stable identifier (RFC 5545 UID). Carries across edits;
    /// a re-sent invitation with the same UID updates rather than
    /// duplicates.
    pub uid: String,
    /// One-line title — what shows up on the calendar grid.
    pub summary: String,
    /// Longer description; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Start of the event in UTC.
    pub start: DateTime<Utc>,
    /// End of the event in UTC. For all-day events, end is exclusive
    /// per RFC 5545 (an event on a single day has end = start + 24h).
    pub end: DateTime<Utc>,
    /// Whether this is an all-day occurrence. When true, `start`
    /// and `end` are at 00:00 UTC and the time portion should be
    /// ignored by renderers.
    #[serde(default)]
    pub all_day: bool,
    /// Free-form location string. Adapters that need lat/lon
    /// resolve later via geocoding.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub location: String,
    /// Confirmed / tentative / cancelled.
    pub status: EventStatus,
    /// Public / private / confidential. Maps to per-provider
    /// privacy fields.
    pub class: EventClass,
    /// Organizer (the inviter). `None` for events you create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organizer: Option<Person>,
    /// Attendees — every invited address.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attendees: Vec<Person>,
    /// Embedded reminders (VALARM). Most providers map these to
    /// per-event notification settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reminders: Vec<Reminder>,
}

/// VTODO — a task without a fixed start/end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarTodo {
    /// Stable identifier.
    pub uid: String,
    /// Task title.
    pub summary: String,
    /// Longer description; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Optional due date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DateTime<Utc>>,
    /// Status: todo / in-progress / completed / cancelled.
    pub status: TodoStatus,
    /// 0–9. 0 = undefined, 1 = highest, 9 = lowest, per RFC 5545.
    #[serde(default)]
    pub priority: u8,
    /// Reminders attached to this task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reminders: Vec<Reminder>,
}

/// Standalone VALARM — a notification not bound to a specific
/// event/todo. Rare; mostly we see VALARMs nested inside the
/// containing item's `reminders`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAlarm {
    /// When the alarm should fire.
    pub trigger: DateTime<Utc>,
    /// Optional summary / message.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// A person — organizer or attendee. RFC 5545 stores these as
/// CAL-ADDRESS URIs (`mailto:foo@bar`); we strip to the address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    /// Email address.
    pub email: String,
    /// Display name; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// VEVENT STATUS — RFC 5545 §3.8.1.11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventStatus {
    /// Confirmed event.
    Confirmed,
    /// Tentative — not yet confirmed.
    Tentative,
    /// Cancelled.
    Cancelled,
}

/// VEVENT CLASS — RFC 5545 §3.8.1.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventClass {
    /// Visible to anyone with calendar access.
    Public,
    /// Visible only to owner.
    Private,
    /// Visible only to owner; details obscured even from the
    /// owner's other calendars.
    Confidential,
}

/// VTODO STATUS — RFC 5545 §3.8.1.11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TodoStatus {
    /// Not yet started.
    NeedsAction,
    /// Worked on.
    InProcess,
    /// Done.
    Completed,
    /// Won't do.
    Cancelled,
}

/// VALARM nested inside an Event or Todo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    /// When the reminder fires. For relative triggers ("15 min
    /// before start"), this is pre-computed against the event.
    pub trigger: DateTime<Utc>,
    /// Free-form message; some providers display this verbatim.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}
