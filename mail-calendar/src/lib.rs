//! `mail-calendar` — typed calendar items extracted from email.
//!
//! ## What this crate does
//!
//! When mail comes in, it often carries calendar payloads — either
//! as `.ics` (iCalendar / RFC 5545) attachments from a meeting
//! invitation, or as inline structured data (Schema.org JSON-LD,
//! `text/calendar` parts in multipart messages). This crate parses
//! those into a typed [`CalendarItem`] tree:
//!
//! ```text
//! enum CalendarItem {
//!     Event(CalendarEvent),  // VEVENT — meetings, appointments
//!     Todo(CalendarTodo),    // VTODO  — tasks, reminders
//!     Alarm(CalendarAlarm),  // VALARM — notifications
//! }
//! ```
//!
//! Mail clients (Thundercrab on the desktop, Thunderbird Android,
//! the future Patina/PlausiDen mobile suite) consume this typed
//! tree and publish to the user's calendar — Google Calendar via
//! the REST API, Microsoft 365 via Microsoft Graph, Apple iCloud
//! via CalDAV (RFC 4791), Nextcloud / generic CalDAV likewise.
//!
//! ## What this crate doesn't do
//!
//! - **Network I/O.** No HTTP client, no OAuth, no Google API keys
//!   in here. The publish side is a separate adapter, lives next to
//!   the mail client. This crate is pure parsing → typed events.
//! - **Natural-language date detection.** Catching "Tuesday at 3pm"
//!   in plain prose is a separate, ML-shaped problem; this crate
//!   covers the structured paths (RFC 5545) that don't need ML.
//! - **Calendar storage.** Once you have a typed [`CalendarItem`],
//!   what to do with it is the consumer's call.
//!
//! ## Provider compatibility
//!
//! The output types are deliberately the union of what every major
//! provider needs. Each [`CalendarEvent`] carries enough info that
//! a downstream adapter can map it to:
//!
//! | Provider | Adapter shape |
//! |---|---|
//! | Google Calendar | POST events.insert with `start`, `end`, `summary`, `location`, `attendees`, `reminders` |
//! | Microsoft Graph | POST /me/events with `subject`, `start`, `end`, `attendees`, `bodyPreview` |
//! | Apple iCloud / Nextcloud / generic CalDAV | PUT an .ics body (re-emitted from `to_ics()`) |
//! | Android Calendar Provider (via intent) | content URI insert with EVENTS table fields |

#![doc(html_no_source)]

pub mod ics;
pub mod item;
pub mod merge;

pub use ics::{parse_ics, write_ics, IcsError};
pub use merge::{merge_by_uid, MergeOutcome, MergeReport};
pub use item::{
    CalendarAlarm, CalendarEvent, CalendarItem, CalendarTodo, EventClass, EventStatus, Person,
    Reminder, TodoStatus,
};
