//! RFC 5545 (iCalendar) parser → typed [`CalendarItem`].
//!
//! This is a focused, hand-rolled parser. RFC 5545 is line-oriented
//! and scope-bounded enough that a third-party generic library
//! mostly hurts more than helps; we only need VEVENT, VTODO, VALARM
//! out of the seven defined components, with a strict subset of
//! their properties. Hand-rolling is ~250 lines, no API guessing,
//! every behavior is in this file.
//!
//! What's supported:
//!
//! - VCALENDAR wrapper (multiple top-level components)
//! - VEVENT (with nested VALARMs as reminders)
//! - VTODO (with nested VALARMs as reminders)
//! - VALARM (standalone — rare, but RFC 5545 allows)
//! - Continuation lines (RFC 5545 §3.1 line folding)
//! - Property parameters (PROPERTY;PARAM=VALUE:VALUE)
//! - DATE-TIME (with Z suffix = UTC, or floating treated as UTC)
//! - DATE (date-only, treated as midnight UTC)
//! - Relative TRIGGER (-PT15M / PT1H30M)
//!
//! What's not supported in v0:
//!
//! - VTIMEZONE (full TZ-aware time conversion). Floating times are
//!   treated as UTC; any TZID parameter is ignored. Most invitation
//!   flows from major providers emit UTC explicitly anyway.
//! - VJOURNAL (rare, no clear use case)
//! - VFREEBUSY (out of v0 scope)
//! - RECURRENCE (RRULE, EXDATE) — events are treated as single
//!   occurrences. Recurring-event support is a planned follow-up.

use crate::item::{
    CalendarAlarm, CalendarEvent, CalendarItem, CalendarTodo, EventClass, EventStatus, Person,
    Reminder, TodoStatus,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use std::collections::HashMap;

/// Errors during ICS parsing.
#[derive(Debug, thiserror::Error)]
pub enum IcsError {
    /// Input wasn't valid iCalendar syntax.
    #[error("malformed iCalendar input: {0}")]
    Malformed(String),
    /// Required RFC 5545 property was missing.
    #[error("missing required property `{property}` in {component}")]
    MissingProperty {
        /// Component the property is missing from.
        component: &'static str,
        /// Property name.
        property: &'static str,
    },
    /// Date/time field couldn't be parsed.
    #[error("invalid date/time `{value}` in {field}: {reason}")]
    BadDateTime {
        /// Field that contained the bad value.
        field: &'static str,
        /// The raw value.
        value: String,
        /// Why parsing failed.
        reason: String,
    },
}

/// Parsed property: parameters + value.
#[derive(Debug, Clone)]
struct Property {
    /// Parameters, e.g. `CN=Paul` → `{"CN": "Paul"}`.
    params: HashMap<String, String>,
    /// The value after the `:`.
    value: String,
}

/// One component (VEVENT, VTODO, VALARM, …).
#[derive(Debug, Clone, Default)]
struct Component {
    name: String,
    /// Single-valued properties (UID, SUMMARY, DTSTART, …). The
    /// last value wins if a property appears twice.
    props: HashMap<String, Property>,
    /// Multi-valued properties (ATTENDEE — RFC 5545 allows
    /// repeats). Order is preserved.
    multi: Vec<(String, Property)>,
    /// Nested components (VALARM inside VEVENT).
    nested: Vec<Component>,
}

impl Component {
    fn prop_value(&self, name: &str) -> Option<&str> {
        self.props.get(name).map(|p| p.value.as_str())
    }

    fn prop(&self, name: &str) -> Option<&Property> {
        self.props.get(name)
    }
}

/// Parse a raw ICS string into a list of typed [`CalendarItem`]s.
///
/// One ICS body can contain multiple VEVENT / VTODO / VALARM
/// components — RFC 5545 calls this a VCALENDAR. We return them in
/// document order; consumers exhaustive-match on the variant.
///
/// # Errors
/// `Malformed` if the syntax doesn't parse; `MissingProperty` if a
/// required field (UID, DTSTART, SUMMARY) is absent; `BadDateTime`
/// if a temporal field can't be coerced to a UTC instant.
pub fn parse_ics(ics: &str) -> Result<Vec<CalendarItem>, IcsError> {
    let unfolded = unfold(ics);
    let lines: Vec<&str> = unfolded
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    let (root, consumed) = parse_component(&lines, 0)?;
    if consumed != lines.len() {
        return Err(IcsError::Malformed(format!(
            "trailing content after VCALENDAR (line {consumed} of {})",
            lines.len()
        )));
    }
    if root.name != "VCALENDAR" {
        return Err(IcsError::Malformed(format!(
            "expected root VCALENDAR, found {}",
            root.name
        )));
    }
    let mut out = Vec::new();
    for nested in &root.nested {
        match nested.name.as_str() {
            "VEVENT" => out.push(CalendarItem::Event(parse_event(nested)?)),
            "VTODO" => out.push(CalendarItem::Todo(parse_todo(nested)?)),
            "VALARM" => out.push(CalendarItem::Alarm(parse_standalone_alarm(nested)?)),
            // Skip VTIMEZONE / VJOURNAL / VFREEBUSY in v0.
            _ => continue,
        }
    }
    Ok(out)
}

/// Unfold RFC 5545 continuation lines: a line starting with WSP is
/// concatenated to the previous line with the WSP removed.
fn unfold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut first = true;
    for line in input.split_inclusive('\n') {
        let stripped = line.strip_suffix('\n').unwrap_or(line);
        let stripped = stripped.strip_suffix('\r').unwrap_or(stripped);
        if !first && stripped.starts_with(' ') {
            out.push_str(&stripped[1..]);
        } else if !first && stripped.starts_with('\t') {
            out.push_str(&stripped[1..]);
        } else {
            if !first {
                out.push('\n');
            }
            out.push_str(stripped);
            first = false;
        }
    }
    out
}

/// Recursive-descent parse of a component starting at `lines[start]`.
/// Returns the parsed component and the index just past its END line.
fn parse_component(lines: &[&str], start: usize) -> Result<(Component, usize), IcsError> {
    let begin_line = lines.get(start).ok_or_else(|| {
        IcsError::Malformed(format!("expected BEGIN line at index {start}, hit EOF"))
    })?;
    let name = begin_line
        .strip_prefix("BEGIN:")
        .ok_or_else(|| {
            IcsError::Malformed(format!("expected BEGIN:, found `{begin_line}`"))
        })?
        .trim()
        .to_string();
    let end_marker = format!("END:{name}");

    let mut comp = Component {
        name: name.clone(),
        ..Default::default()
    };
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.trim() == end_marker {
            return Ok((comp, i + 1));
        }
        if line.starts_with("BEGIN:") {
            let (nested, next) = parse_component(lines, i)?;
            comp.nested.push(nested);
            i = next;
            continue;
        }
        if let Some((prop_key, prop)) = parse_property_line(line) {
            // RFC 5545 ATTENDEE/COMMENT/etc. can repeat. Keep
            // multi-valued properties in `multi`; everything else
            // is single-valued (last-write-wins).
            if matches!(prop_key.as_str(), "ATTENDEE" | "COMMENT" | "CATEGORIES" | "RELATED-TO") {
                comp.multi.push((prop_key, prop));
            } else {
                comp.props.insert(prop_key, prop);
            }
        }
        i += 1;
    }
    Err(IcsError::Malformed(format!(
        "missing {end_marker} for BEGIN:{name}"
    )))
}

/// Parse `PROPNAME[;PARAM=VAL[;PARAM=VAL]]:VALUE`.
fn parse_property_line(line: &str) -> Option<(String, Property)> {
    // Find the first ':' that is NOT inside a quoted parameter value.
    let mut in_quotes = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (lhs, rhs) = line.split_at(colon);
    let value = rhs[1..].to_string();
    let mut parts = lhs.split(';');
    let name = parts.next()?.trim().to_string();
    let mut params = HashMap::new();
    for part in parts {
        if let Some((k, v)) = part.split_once('=') {
            params.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    Some((name, Property { params, value }))
}

fn parse_event(c: &Component) -> Result<CalendarEvent, IcsError> {
    let uid = c
        .prop_value("UID")
        .ok_or(IcsError::MissingProperty {
            component: "VEVENT",
            property: "UID",
        })?
        .to_string();
    let summary = c.prop_value("SUMMARY").unwrap_or("").to_string();
    let description = c.prop_value("DESCRIPTION").unwrap_or("").to_string();
    let location = c.prop_value("LOCATION").unwrap_or("").to_string();
    let dtstart_raw = c.prop_value("DTSTART").ok_or(IcsError::MissingProperty {
        component: "VEVENT",
        property: "DTSTART",
    })?;
    let dtend_raw = c.prop_value("DTEND").ok_or(IcsError::MissingProperty {
        component: "VEVENT",
        property: "DTEND",
    })?;
    let (start, all_day_start) = parse_datetime(dtstart_raw, "DTSTART")?;
    let (end, all_day_end) = parse_datetime(dtend_raw, "DTEND")?;
    let all_day = all_day_start && all_day_end;
    let status = match c.prop_value("STATUS").map(str::to_uppercase).as_deref() {
        Some("TENTATIVE") => EventStatus::Tentative,
        Some("CANCELLED") => EventStatus::Cancelled,
        _ => EventStatus::Confirmed,
    };
    let class = match c.prop_value("CLASS").map(str::to_uppercase).as_deref() {
        Some("PRIVATE") => EventClass::Private,
        Some("CONFIDENTIAL") => EventClass::Confidential,
        _ => EventClass::Public,
    };
    let organizer = c.prop("ORGANIZER").map(parse_cal_address);
    let attendees: Vec<Person> = c
        .multi
        .iter()
        .filter(|(k, _)| k == "ATTENDEE")
        .map(|(_, p)| parse_cal_address(p))
        .collect();
    let mut reminders = Vec::new();
    for nested in &c.nested {
        if nested.name == "VALARM" {
            if let Some(r) = parse_nested_alarm(nested, start)? {
                reminders.push(r);
            }
        }
    }
    Ok(CalendarEvent {
        uid,
        summary,
        description,
        start,
        end,
        all_day,
        location,
        status,
        class,
        organizer,
        attendees,
        reminders,
    })
}

fn parse_todo(c: &Component) -> Result<CalendarTodo, IcsError> {
    let uid = c
        .prop_value("UID")
        .ok_or(IcsError::MissingProperty {
            component: "VTODO",
            property: "UID",
        })?
        .to_string();
    let summary = c.prop_value("SUMMARY").unwrap_or("").to_string();
    let description = c.prop_value("DESCRIPTION").unwrap_or("").to_string();
    let due = c
        .prop_value("DUE")
        .map(|raw| parse_datetime(raw, "DUE"))
        .transpose()?
        .map(|(dt, _)| dt);
    let status = match c.prop_value("STATUS").map(str::to_uppercase).as_deref() {
        Some("IN-PROCESS") => TodoStatus::InProcess,
        Some("COMPLETED") => TodoStatus::Completed,
        Some("CANCELLED") => TodoStatus::Cancelled,
        _ => TodoStatus::NeedsAction,
    };
    let priority = c
        .prop_value("PRIORITY")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let mut reminders = Vec::new();
    for nested in &c.nested {
        if nested.name == "VALARM" {
            // Relative triggers in a VTODO resolve against DUE; if
            // there's no DUE we drop the reminder rather than guess.
            if let Some(due_anchor) = due {
                if let Some(r) = parse_nested_alarm(nested, due_anchor)? {
                    reminders.push(r);
                }
            }
        }
    }
    Ok(CalendarTodo {
        uid,
        summary,
        description,
        due,
        status,
        priority,
        reminders,
    })
}

fn parse_nested_alarm(
    c: &Component,
    anchor: DateTime<Utc>,
) -> Result<Option<Reminder>, IcsError> {
    let trigger_raw = match c.prop_value("TRIGGER") {
        Some(t) => t,
        None => return Ok(None),
    };
    let trigger = if trigger_raw.starts_with("-PT") || trigger_raw.starts_with("PT") {
        resolve_relative_trigger(trigger_raw, anchor)?
    } else {
        parse_datetime(trigger_raw, "TRIGGER")?.0
    };
    Ok(Some(Reminder {
        trigger,
        description: c.prop_value("DESCRIPTION").unwrap_or("").to_string(),
    }))
}

fn parse_standalone_alarm(c: &Component) -> Result<CalendarAlarm, IcsError> {
    let trigger_raw = c.prop_value("TRIGGER").ok_or(IcsError::MissingProperty {
        component: "VALARM",
        property: "TRIGGER",
    })?;
    if trigger_raw.starts_with("-PT") || trigger_raw.starts_with("PT") {
        return Err(IcsError::BadDateTime {
            field: "TRIGGER",
            value: trigger_raw.to_string(),
            reason: "standalone VALARM requires an absolute trigger".into(),
        });
    }
    let (trigger, _) = parse_datetime(trigger_raw, "TRIGGER")?;
    Ok(CalendarAlarm {
        trigger,
        description: c.prop_value("DESCRIPTION").unwrap_or("").to_string(),
    })
}

/// Parse one of the RFC 5545 date-time forms. Returns the instant
/// in UTC + a flag for whether the input was a date-only value.
fn parse_datetime(raw: &str, field: &'static str) -> Result<(DateTime<Utc>, bool), IcsError> {
    let s = raw.trim();
    // UTC form: trailing 'Z' is a literal, not a timezone-format token,
    // so we strip it manually before parsing the naive datetime.
    if let Some(rest) = s.strip_suffix('Z') {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(rest, "%Y%m%dT%H%M%S") {
            return Ok((Utc.from_utc_datetime(&ndt), false));
        }
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S") {
        // Floating local time → treat as UTC.
        return Ok((Utc.from_utc_datetime(&ndt), false));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y%m%d") {
        let nt = NaiveTime::from_hms_opt(0, 0, 0).expect("midnight");
        return Ok((Utc.from_utc_datetime(&NaiveDateTime::new(d, nt)), true));
    }
    Err(IcsError::BadDateTime {
        field,
        value: s.to_string(),
        reason: "unrecognized format (expected YYYYMMDDTHHMMSSZ, YYYYMMDDTHHMMSS, or YYYYMMDD)".into(),
    })
}

/// Resolve a relative TRIGGER (`-PT15M`, `PT1H30M`) against an anchor.
fn resolve_relative_trigger(raw: &str, anchor: DateTime<Utc>) -> Result<DateTime<Utc>, IcsError> {
    let s = raw.trim();
    let (sign, body) = if let Some(rest) = s.strip_prefix('-') {
        (-1i64, rest)
    } else {
        (1i64, s)
    };
    let body = body.strip_prefix("PT").ok_or_else(|| IcsError::BadDateTime {
        field: "TRIGGER",
        value: raw.to_string(),
        reason: "expected PT-prefixed duration".into(),
    })?;
    let mut total_seconds: i64 = 0;
    let mut buf = String::new();
    for c in body.chars() {
        if c.is_ascii_digit() {
            buf.push(c);
        } else {
            let n: i64 = buf.parse().map_err(|_| IcsError::BadDateTime {
                field: "TRIGGER",
                value: raw.to_string(),
                reason: "non-numeric duration component".into(),
            })?;
            buf.clear();
            total_seconds += match c {
                'H' => n * 3600,
                'M' => n * 60,
                'S' => n,
                _ => {
                    return Err(IcsError::BadDateTime {
                        field: "TRIGGER",
                        value: raw.to_string(),
                        reason: format!("unknown duration unit '{c}'"),
                    });
                }
            };
        }
    }
    Ok(anchor + chrono::Duration::seconds(sign * total_seconds))
}

/// Parse `mailto:foo@bar` style RFC 5545 CAL-ADDRESS into a [`Person`].
fn parse_cal_address(prop: &Property) -> Person {
    let email = prop.value.trim_start_matches("mailto:").to_string();
    let name = prop.params.get("CN").cloned().unwrap_or_default();
    Person { email, name }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_INVITATION: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Mailroom//mail-calendar//EN\r\n\
BEGIN:VEVENT\r\n\
UID:abc-123@plausiden.com\r\n\
DTSTAMP:20260501T120000Z\r\n\
DTSTART:20260501T143000Z\r\n\
DTEND:20260501T153000Z\r\n\
SUMMARY:Quarterly review\r\n\
DESCRIPTION:Discuss roadmap for Q3\r\n\
LOCATION:Boardroom A\r\n\
STATUS:CONFIRMED\r\n\
ORGANIZER;CN=Paul:mailto:paul@plausiden.com\r\n\
ATTENDEE;CN=William:mailto:william@plausiden.com\r\n\
ATTENDEE:mailto:team@plausiden.com\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
DESCRIPTION:Reminder\r\n\
TRIGGER:-PT15M\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    const SAMPLE_TODO: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Mailroom//mail-calendar//EN\r\n\
BEGIN:VTODO\r\n\
UID:task-456@plausiden.com\r\n\
DTSTAMP:20260501T120000Z\r\n\
DUE:20260510T170000Z\r\n\
SUMMARY:Submit Q2 report\r\n\
STATUS:NEEDS-ACTION\r\n\
PRIORITY:1\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn parse_meeting_invitation() {
        let items = parse_ics(SAMPLE_INVITATION).unwrap();
        assert_eq!(items.len(), 1);
        let CalendarItem::Event(ev) = &items[0] else {
            panic!("expected an event");
        };
        assert_eq!(ev.uid, "abc-123@plausiden.com");
        assert_eq!(ev.summary, "Quarterly review");
        assert_eq!(ev.location, "Boardroom A");
        assert_eq!(ev.status, EventStatus::Confirmed);
        assert!(!ev.all_day);
        let org = ev.organizer.as_ref().unwrap();
        assert_eq!(org.email, "paul@plausiden.com");
        assert_eq!(org.name, "Paul");
        assert_eq!(ev.attendees.len(), 2);
        assert_eq!(ev.attendees[0].email, "william@plausiden.com");
        assert_eq!(ev.attendees[0].name, "William");
        assert_eq!(ev.attendees[1].email, "team@plausiden.com");
        assert_eq!(ev.reminders.len(), 1);
        assert_eq!(ev.reminders[0].trigger.format("%H:%M").to_string(), "14:15");
    }

    #[test]
    fn parse_todo_with_priority() {
        let items = parse_ics(SAMPLE_TODO).unwrap();
        assert_eq!(items.len(), 1);
        let CalendarItem::Todo(t) = &items[0] else {
            panic!("expected a todo");
        };
        assert_eq!(t.uid, "task-456@plausiden.com");
        assert_eq!(t.summary, "Submit Q2 report");
        assert_eq!(t.priority, 1);
        assert_eq!(t.status, TodoStatus::NeedsAction);
        assert!(t.due.is_some());
    }

    #[test]
    fn malformed_input_rejected() {
        assert!(parse_ics("not an ics").is_err());
    }

    #[test]
    fn missing_uid_rejected() {
        let bad = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20260501T143000Z\r\nDTEND:20260501T153000Z\r\nSUMMARY:no uid\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let err = parse_ics(bad).unwrap_err();
        assert!(matches!(err, IcsError::MissingProperty { property: "UID", .. }));
    }

    #[test]
    fn relative_trigger_minutes_before() {
        let anchor = Utc.with_ymd_and_hms(2026, 5, 1, 14, 30, 0).unwrap();
        let resolved = resolve_relative_trigger("-PT15M", anchor).unwrap();
        assert_eq!(resolved.format("%H:%M").to_string(), "14:15");
    }

    #[test]
    fn relative_trigger_hours_after() {
        let anchor = Utc.with_ymd_and_hms(2026, 5, 1, 14, 30, 0).unwrap();
        let resolved = resolve_relative_trigger("PT1H30M", anchor).unwrap();
        assert_eq!(resolved.format("%H:%M").to_string(), "16:00");
    }

    #[test]
    fn parse_datetime_utc() {
        let (dt, all_day) = parse_datetime("20260501T143000Z", "DTSTART").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-05-01 14:30:00");
        assert!(!all_day);
    }

    #[test]
    fn parse_datetime_floating() {
        let (dt, all_day) = parse_datetime("20260501T143000", "DTSTART").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-05-01 14:30:00");
        assert!(!all_day);
    }

    #[test]
    fn parse_datetime_date_only() {
        let (dt, all_day) = parse_datetime("20260501", "DTSTART").unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-05-01");
        assert!(all_day);
    }

    #[test]
    fn parse_datetime_invalid() {
        assert!(parse_datetime("bogus", "DTSTART").is_err());
    }

    #[test]
    fn standalone_alarm_with_relative_trigger_rejected() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:bare\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VCALENDAR\r\n";
        assert!(parse_ics(ics).is_err());
    }

    #[test]
    fn unfolds_continuation_lines() {
        // RFC 5545 §3.1: the inserted CRLF+WSP is removed entirely.
        // "DESCRIPTION:lo\r\n ng" → "DESCRIPTION:long" (the WSP at
        // the start of the continuation line is consumed, not kept).
        let raw = "DESCRIPTION:lo\r\n ng description\r\n";
        let unfolded = unfold(raw);
        assert!(
            unfolded.contains("long description"),
            "unfold should join across CRLF+WSP: got {unfolded:?}"
        );
    }

    #[test]
    fn parses_property_with_params() {
        let line = "ATTENDEE;CN=Alice;ROLE=REQ-PARTICIPANT:mailto:alice@x";
        let (name, prop) = parse_property_line(line).unwrap();
        assert_eq!(name, "ATTENDEE");
        assert_eq!(prop.params.get("CN").unwrap(), "Alice");
        assert_eq!(prop.params.get("ROLE").unwrap(), "REQ-PARTICIPANT");
        assert_eq!(prop.value, "mailto:alice@x");
    }

    #[test]
    fn parses_property_with_quoted_param() {
        let line = r#"ORGANIZER;CN="O'Brien, Pat":mailto:pat@x"#;
        let (name, prop) = parse_property_line(line).unwrap();
        assert_eq!(name, "ORGANIZER");
        assert_eq!(prop.params.get("CN").unwrap(), "O'Brien, Pat");
    }

    #[test]
    fn multiple_components_in_one_calendar() {
        let ics = format!(
            "BEGIN:VCALENDAR\r\n{}{}END:VCALENDAR\r\n",
            "BEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20260501T143000Z\r\nDTEND:20260501T153000Z\r\nSUMMARY:e1\r\nEND:VEVENT\r\n",
            "BEGIN:VTODO\r\nUID:t1\r\nDUE:20260510T170000Z\r\nSUMMARY:t1\r\nEND:VTODO\r\n"
        );
        let items = parse_ics(&ics).unwrap();
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], CalendarItem::Event(_)));
        assert!(matches!(items[1], CalendarItem::Todo(_)));
    }

    #[test]
    fn unknown_component_skipped() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:UTC\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20260501T143000Z\r\nDTEND:20260501T153000Z\r\nSUMMARY:e1\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let items = parse_ics(ics).unwrap();
        assert_eq!(items.len(), 1);
    }
}
