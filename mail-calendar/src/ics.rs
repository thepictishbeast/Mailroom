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
    nested: Vec<Self>,
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
            _ => {}
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
        if !first && stripped.starts_with([' ', '\t']) {
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
    let summary = unescape_text(c.prop_value("SUMMARY").unwrap_or(""));
    let description = unescape_text(c.prop_value("DESCRIPTION").unwrap_or(""));
    let location = unescape_text(c.prop_value("LOCATION").unwrap_or(""));
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
    let summary = unescape_text(c.prop_value("SUMMARY").unwrap_or(""));
    let description = unescape_text(c.prop_value("DESCRIPTION").unwrap_or(""));
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
    let Some(trigger_raw) = c.prop_value("TRIGGER") else {
        return Ok(None);
    };
    let trigger = if trigger_raw.starts_with("-PT") || trigger_raw.starts_with("PT") {
        resolve_relative_trigger(trigger_raw, anchor)?
    } else {
        parse_datetime(trigger_raw, "TRIGGER")?.0
    };
    Ok(Some(Reminder {
        trigger,
        description: unescape_text(c.prop_value("DESCRIPTION").unwrap_or("")),
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
        description: unescape_text(c.prop_value("DESCRIPTION").unwrap_or("")),
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
    let (sign, body) = s.strip_prefix('-').map_or((1i64, s), |rest| (-1i64, rest));
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

/// RFC 5545 §3.3.11 unescape: inverse of [`escape_text`]. Invalid
/// trailing backslash (one without a following char) is preserved
/// literally — the spec is silent on bad input and round-tripping
/// invalid escapes lossy is worse than lossless.
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(c @ ('\\' | ',' | ';')) => out.push(c),
                Some('n' | 'N') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ----- Writer: typed → ICS bytes -----------------------------------
//
// The inverse of [`parse_ics`]. Re-emits a `Vec<CalendarItem>` as a
// single VCALENDAR document the consumer can PUT to a CalDAV
// endpoint, attach to an outgoing email, or save to disk.
//
// The writer is intentionally minimal: pure structured output, no
// ML-shaped fields (RRULE, RECURRENCE-ID, EXDATE), no timezone
// definitions (everything is UTC-stamped with the trailing Z).
// Adding those is a v0.1 / v1.0 concern after we have a downstream
// adapter that needs them.

/// Serialize a list of calendar items to a single VCALENDAR ICS
/// document.
///
/// The output is RFC 5545 line-folded (CRLF + leading space at
/// 75 octets), TEXT-escaped (`,;\n\\`), and UTC-only. All-day
/// events emit `DTSTART;VALUE=DATE:YYYYMMDD` so receivers don't
/// double-shift across timezones.
///
/// PRODID identifies the producer for receiver-side debugging.
#[must_use]
pub fn write_ics(items: &[CalendarItem]) -> String {
    let mut out = String::with_capacity(256 + items.len() * 256);
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str("VERSION:2.0\r\n");
    out.push_str("PRODID:-//PlausiDen//mail-calendar 0.1//EN\r\n");
    out.push_str("CALSCALE:GREGORIAN\r\n");

    let now = Utc::now();
    for item in items {
        match item {
            CalendarItem::Event(e) => write_event(e, now, &mut out),
            CalendarItem::Todo(t) => write_todo(t, now, &mut out),
            CalendarItem::Alarm(a) => write_standalone_alarm(a, &mut out),
        }
    }

    out.push_str("END:VCALENDAR\r\n");
    out
}

fn write_event(e: &CalendarEvent, now: DateTime<Utc>, out: &mut String) {
    out.push_str("BEGIN:VEVENT\r\n");
    write_line(out, "UID", &e.uid);
    write_line_raw(out, "DTSTAMP", &fmt_datetime_z(now));
    if e.all_day {
        // For all-day events: DATE-only form, no time, no Z.
        write_line_raw(
            out,
            "DTSTART;VALUE=DATE",
            &e.start.format("%Y%m%d").to_string(),
        );
        write_line_raw(
            out,
            "DTEND;VALUE=DATE",
            &e.end.format("%Y%m%d").to_string(),
        );
    } else {
        write_line_raw(out, "DTSTART", &fmt_datetime_z(e.start));
        write_line_raw(out, "DTEND", &fmt_datetime_z(e.end));
    }
    write_line(out, "SUMMARY", &e.summary);
    if !e.description.is_empty() {
        write_line(out, "DESCRIPTION", &e.description);
    }
    if !e.location.is_empty() {
        write_line(out, "LOCATION", &e.location);
    }
    write_line_raw(out, "STATUS", event_status_str(e.status));
    write_line_raw(out, "CLASS", event_class_str(e.class));
    if let Some(org) = &e.organizer {
        write_cal_address(out, "ORGANIZER", org);
    }
    for att in &e.attendees {
        write_cal_address(out, "ATTENDEE", att);
    }
    for r in &e.reminders {
        write_reminder(r, out);
    }
    out.push_str("END:VEVENT\r\n");
}

fn write_todo(t: &CalendarTodo, now: DateTime<Utc>, out: &mut String) {
    out.push_str("BEGIN:VTODO\r\n");
    write_line(out, "UID", &t.uid);
    write_line_raw(out, "DTSTAMP", &fmt_datetime_z(now));
    write_line(out, "SUMMARY", &t.summary);
    if !t.description.is_empty() {
        write_line(out, "DESCRIPTION", &t.description);
    }
    if let Some(due) = t.due {
        write_line_raw(out, "DUE", &fmt_datetime_z(due));
    }
    write_line_raw(out, "STATUS", todo_status_str(t.status));
    if t.priority > 0 {
        write_line_raw(out, "PRIORITY", &t.priority.to_string());
    }
    for r in &t.reminders {
        write_reminder(r, out);
    }
    out.push_str("END:VTODO\r\n");
}

fn write_reminder(r: &Reminder, out: &mut String) {
    out.push_str("BEGIN:VALARM\r\n");
    out.push_str("ACTION:DISPLAY\r\n");
    let desc = if r.description.is_empty() {
        "Reminder"
    } else {
        &r.description
    };
    write_line(out, "DESCRIPTION", desc);
    write_line_raw(out, "TRIGGER;VALUE=DATE-TIME", &fmt_datetime_z(r.trigger));
    out.push_str("END:VALARM\r\n");
}

fn write_standalone_alarm(a: &CalendarAlarm, out: &mut String) {
    out.push_str("BEGIN:VALARM\r\n");
    out.push_str("ACTION:DISPLAY\r\n");
    let desc = if a.description.is_empty() {
        "Alarm"
    } else {
        &a.description
    };
    write_line(out, "DESCRIPTION", desc);
    write_line_raw(out, "TRIGGER;VALUE=DATE-TIME", &fmt_datetime_z(a.trigger));
    out.push_str("END:VALARM\r\n");
}

fn write_cal_address(out: &mut String, prop_name: &str, p: &Person) {
    let mut header = prop_name.to_string();
    if !p.name.is_empty() {
        // Quote CN if it contains characters that require it (`,;:`).
        let needs_quote = p.name.chars().any(|c| matches!(c, ',' | ';' | ':'));
        let cn_value = if needs_quote {
            format!("\"{}\"", p.name.replace('"', ""))
        } else {
            p.name.clone()
        };
        header.push_str(";CN=");
        header.push_str(&cn_value);
    }
    let value = format!("mailto:{}", p.email);
    let line = format!("{header}:{value}");
    out.push_str(&fold_line(&line));
    out.push_str("\r\n");
}

/// Write a TEXT property — escapes special chars per RFC 5545 and
/// applies line folding.
fn write_line(out: &mut String, name: &str, value: &str) {
    let line = format!("{name}:{}", escape_text(value));
    out.push_str(&fold_line(&line));
    out.push_str("\r\n");
}

/// Write a property whose value is already RFC 5545–shaped (e.g.,
/// pre-formatted DATE-TIME, enum string). Skips TEXT escaping but
/// still folds.
fn write_line_raw(out: &mut String, name: &str, value: &str) {
    let line = format!("{name}:{value}");
    out.push_str(&fold_line(&line));
    out.push_str("\r\n");
}

/// RFC 5545 §3.3.11 escape: `\\` for backslash, `\,` for comma,
/// `\;` for semicolon, `\n` for newline (literal `\n`, not actual
/// LF). Colon does NOT get escaped — colons are common in URLs.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            ',' => out.push_str(r"\,"),
            ';' => out.push_str(r"\;"),
            '\n' => out.push_str(r"\n"),
            '\r' => {} // strip — folding handles wrap
            _ => out.push(c),
        }
    }
    out
}

/// RFC 5545 §3.1 line folding: any line longer than 75 octets is
/// broken with CRLF + a single space. Returns the folded result,
/// without the trailing CRLF (caller appends).
///
/// The 75-octet limit is conservative — the spec mentions 75
/// octets, with 76 the absolute limit; we pick 75 to stay safely
/// under across UTF-8 multi-byte char boundaries.
fn fold_line(line: &str) -> String {
    const LIMIT: usize = 75;
    if line.len() <= LIMIT {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let bytes = line.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = (start + LIMIT).min(bytes.len());
        // Don't split inside a multi-byte UTF-8 char.
        while end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        if start > 0 {
            out.push_str("\r\n ");
        }
        out.push_str(std::str::from_utf8(&bytes[start..end]).unwrap_or(""));
        start = end;
    }
    out
}

fn fmt_datetime_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

const fn event_status_str(s: EventStatus) -> &'static str {
    match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    }
}

const fn event_class_str(c: EventClass) -> &'static str {
    match c {
        EventClass::Public => "PUBLIC",
        EventClass::Private => "PRIVATE",
        EventClass::Confidential => "CONFIDENTIAL",
    }
}

const fn todo_status_str(s: TodoStatus) -> &'static str {
    match s {
        TodoStatus::NeedsAction => "NEEDS-ACTION",
        TodoStatus::InProcess => "IN-PROCESS",
        TodoStatus::Completed => "COMPLETED",
        TodoStatus::Cancelled => "CANCELLED",
    }
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

    // ----- Writer tests --------------------------------------------

    fn sample_event() -> CalendarEvent {
        CalendarEvent {
            uid: "evt-1@example.com".into(),
            summary: "Standup".into(),
            description: "Daily, all hands".into(),
            start: "2026-05-02T14:00:00Z".parse().unwrap(),
            end: "2026-05-02T14:30:00Z".parse().unwrap(),
            all_day: false,
            location: "Zoom: https://zoom.us/j/123".into(),
            status: EventStatus::Confirmed,
            class: EventClass::Public,
            organizer: Some(Person {
                email: "tim@example.com".into(),
                name: "Tim Porter".into(),
            }),
            attendees: vec![Person {
                email: "alice@example.com".into(),
                name: "Alice".into(),
            }],
            reminders: vec![Reminder {
                trigger: "2026-05-02T13:45:00Z".parse().unwrap(),
                description: String::new(),
            }],
        }
    }

    #[test]
    fn write_event_emits_required_props() {
        let ics = write_ics(&[CalendarItem::Event(sample_event())]);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.contains("BEGIN:VEVENT\r\n"));
        assert!(ics.contains("UID:evt-1@example.com\r\n"));
        assert!(ics.contains("DTSTART:20260502T140000Z\r\n"));
        assert!(ics.contains("DTEND:20260502T143000Z\r\n"));
        assert!(ics.contains("SUMMARY:Standup\r\n"));
        assert!(ics.contains("STATUS:CONFIRMED\r\n"));
        assert!(ics.contains("CLASS:PUBLIC\r\n"));
        assert!(ics.contains("ORGANIZER;CN=Tim Porter:mailto:tim@example.com\r\n"));
        assert!(ics.contains("ATTENDEE;CN=Alice:mailto:alice@example.com\r\n"));
        assert!(ics.contains("END:VEVENT\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
    }

    #[test]
    fn write_event_escapes_text_fields() {
        let mut e = sample_event();
        e.summary = "needs, escaping; and\\here".into();
        e.description = "line1\nline2".into();
        let ics = write_ics(&[CalendarItem::Event(e)]);
        assert!(ics.contains(r"SUMMARY:needs\, escaping\; and\\here"));
        assert!(ics.contains(r"DESCRIPTION:line1\nline2"));
    }

    #[test]
    fn write_event_round_trips_through_parse() {
        // Round-trip: parse(write(x)) should produce a CalendarItem
        // equal to x for all fields except DTSTAMP (which is regenerated
        // at write time).
        let original = sample_event();
        let ics = write_ics(&[CalendarItem::Event(original.clone())]);
        let parsed = parse_ics(&ics).expect("parse should succeed");
        assert_eq!(parsed.len(), 1);
        let CalendarItem::Event(round_tripped) = &parsed[0] else {
            panic!("expected event, got {:?}", parsed[0]);
        };
        assert_eq!(round_tripped.uid, original.uid);
        assert_eq!(round_tripped.summary, original.summary);
        assert_eq!(round_tripped.description, original.description);
        assert_eq!(round_tripped.start, original.start);
        assert_eq!(round_tripped.end, original.end);
        assert_eq!(round_tripped.location, original.location);
        assert_eq!(round_tripped.status, original.status);
        assert_eq!(round_tripped.class, original.class);
        assert_eq!(round_tripped.organizer, original.organizer);
        assert_eq!(round_tripped.attendees, original.attendees);
        assert_eq!(round_tripped.reminders.len(), original.reminders.len());
        assert_eq!(round_tripped.reminders[0].trigger, original.reminders[0].trigger);
    }

    #[test]
    fn write_all_day_event_uses_date_form() {
        let mut e = sample_event();
        e.all_day = true;
        e.start = "2026-05-04T00:00:00Z".parse().unwrap();
        e.end = "2026-05-05T00:00:00Z".parse().unwrap();
        let ics = write_ics(&[CalendarItem::Event(e)]);
        assert!(ics.contains("DTSTART;VALUE=DATE:20260504\r\n"));
        assert!(ics.contains("DTEND;VALUE=DATE:20260505\r\n"));
        // No time-portion form should appear for the all-day case.
        assert!(!ics.contains("DTSTART:20260504T"));
    }

    #[test]
    fn write_todo_round_trips() {
        let original = CalendarTodo {
            uid: "todo-1@example.com".into(),
            summary: "Buy groceries".into(),
            description: String::new(),
            due: Some("2026-05-03T17:00:00Z".parse().unwrap()),
            status: TodoStatus::NeedsAction,
            priority: 5,
            reminders: vec![],
        };
        let ics = write_ics(&[CalendarItem::Todo(original.clone())]);
        assert!(ics.contains("BEGIN:VTODO\r\n"));
        assert!(ics.contains("UID:todo-1@example.com"));
        assert!(ics.contains("DUE:20260503T170000Z"));
        assert!(ics.contains("STATUS:NEEDS-ACTION"));
        assert!(ics.contains("PRIORITY:5"));

        let parsed = parse_ics(&ics).expect("parse");
        let CalendarItem::Todo(rt) = &parsed[0] else {
            panic!("expected todo, got {:?}", parsed[0]);
        };
        assert_eq!(rt.uid, original.uid);
        assert_eq!(rt.summary, original.summary);
        assert_eq!(rt.due, original.due);
        assert_eq!(rt.status, original.status);
        assert_eq!(rt.priority, original.priority);
    }

    #[test]
    fn write_emits_calendar_chrome() {
        let ics = write_ics(&[]);
        assert!(ics.contains("VERSION:2.0\r\n"));
        assert!(ics.contains("PRODID:-//PlausiDen//mail-calendar 0.1//EN\r\n"));
        assert!(ics.contains("CALSCALE:GREGORIAN\r\n"));
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
    }

    #[test]
    fn fold_long_lines_at_75_octets() {
        let line = format!("DESCRIPTION:{}", "x".repeat(200));
        let folded = fold_line(&line);
        for chunk in folded.split("\r\n") {
            // Each segment fits in 75 octets (continuation chunks
            // include the leading space which counts).
            assert!(chunk.len() <= 76, "chunk too long: {}", chunk.len());
        }
        // Continuation lines start with a single space.
        assert!(folded.contains("\r\n "));
    }

    #[test]
    fn round_trip_with_long_description() {
        let mut e = sample_event();
        e.description = "x".repeat(300);
        let ics = write_ics(&[CalendarItem::Event(e.clone())]);
        let parsed = parse_ics(&ics).expect("parse");
        let CalendarItem::Event(rt) = &parsed[0] else {
            panic!()
        };
        // After fold + unfold, the body comes back identical.
        assert_eq!(rt.description, e.description);
    }

    #[test]
    fn cn_with_special_chars_is_quoted() {
        let mut e = sample_event();
        e.organizer = Some(Person {
            email: "pat@x".into(),
            name: "O'Brien, Pat".into(),
        });
        let ics = write_ics(&[CalendarItem::Event(e)]);
        assert!(ics.contains(r#"ORGANIZER;CN="O'Brien, Pat":mailto:pat@x"#));
    }
}
