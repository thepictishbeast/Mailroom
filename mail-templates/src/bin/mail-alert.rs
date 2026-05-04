//! `mail-alert` — operator-facing CLI for `prebuilt::alert`.
//!
//! Wraps the typed alert builder with command-line args so cron
//! jobs, systemd timers, and monitoring scripts can fire polished
//! alerts without authoring JSON or Rust.
//!
//! ## Usage
//!
//! ```sh
//! # Dry-run: print HTML to stdout
//! mail-alert --severity critical --title "Disk 95%" \
//!     --summary "/var has crossed 95% high-water." \
//!     --field Host=web-01 --field Mountpoint=/var --field Used=95% \
//!     --runbook https://runbooks/disk-full \
//!     --on-call oncall@plausiden.com
//!
//! # Pipe a multipart envelope to sendmail (which sets From/Date/etc.)
//! mail-alert --severity warning --title "Cert renewal failing" \
//!     --to ops@plausiden.com --mime | sendmail -t
//! ```
//!
//! ## Why this exists
//!
//! `mail-tpl` already renders an `EmailDocument` JSON to MIME — but
//! authoring the JSON for every cron alert is friction. This is the
//! same render path with a tighter, alert-specific arg surface so a
//! one-line shell pipeline becomes feasible.

use mail_templates::prebuilt::{alert, AlertSeverity};
use mail_templates::{Field, Theme};
use std::io::Write as _;

#[derive(Debug)]
enum Args {
    Help,
    Render(RenderArgs),
}

#[derive(Debug)]
struct RenderArgs {
    severity: AlertSeverity,
    title: String,
    summary: String,
    fields: Vec<Field>,
    runbook: Option<String>,
    on_call: Option<String>,
    to: Option<String>,
    output: OutputMode,
    theme: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Html,
    Plain,
    Mime,
}

fn parse_severity(s: &str) -> Option<AlertSeverity> {
    match s.to_ascii_lowercase().as_str() {
        "critical" | "crit" | "page" => Some(AlertSeverity::Critical),
        "warning" | "warn" => Some(AlertSeverity::Warning),
        "info" | "informational" => Some(AlertSeverity::Info),
        _ => None,
    }
}

fn parse_field(arg: &str) -> Option<Field> {
    let (label, value) = arg.split_once('=')?;
    let mono = label.starts_with('@'); // --field @Host=web-01 → mono
    let label = label.trim_start_matches('@').trim().to_string();
    if label.is_empty() {
        return None;
    }
    Some(Field {
        label,
        value: value.trim().to_string(),
        mono,
    })
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut iter = argv.iter().skip(1);
    let mut severity: Option<AlertSeverity> = None;
    let mut title: Option<String> = None;
    let mut summary = String::new();
    let mut fields: Vec<Field> = Vec::new();
    let mut runbook: Option<String> = None;
    let mut on_call: Option<String> = None;
    let mut to: Option<String> = None;
    let mut output = OutputMode::Html;
    let mut theme: Option<String> = None;

    while let Some(a) = iter.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Args::Help),
            "--severity" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--severity requires a value".to_string())?;
                severity = Some(
                    parse_severity(v).ok_or_else(|| {
                        format!("--severity must be critical|warning|info, got {v:?}")
                    })?,
                );
            }
            "--title" => {
                title = Some(
                    iter.next()
                        .ok_or_else(|| "--title requires a value".to_string())?
                        .clone(),
                );
            }
            "--summary" => {
                summary = iter
                    .next()
                    .ok_or_else(|| "--summary requires a value".to_string())?
                    .clone();
            }
            "--field" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--field requires a NAME=VALUE arg".to_string())?;
                fields.push(parse_field(v).ok_or_else(|| {
                    format!("--field expects NAME=VALUE, got {v:?}")
                })?);
            }
            "--runbook" => {
                runbook = Some(
                    iter.next()
                        .ok_or_else(|| "--runbook requires a URL".to_string())?
                        .clone(),
                );
            }
            "--on-call" => {
                on_call = Some(
                    iter.next()
                        .ok_or_else(|| "--on-call requires a contact".to_string())?
                        .clone(),
                );
            }
            "--to" => {
                to = Some(
                    iter.next()
                        .ok_or_else(|| "--to requires a recipient".to_string())?
                        .clone(),
                );
            }
            "--html" => output = OutputMode::Html,
            "--plain" => output = OutputMode::Plain,
            "--mime" => output = OutputMode::Mime,
            "--theme" => {
                theme = Some(
                    iter.next()
                        .ok_or_else(|| "--theme requires a name".to_string())?
                        .clone(),
                );
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }

    let severity = severity.ok_or_else(|| "--severity is required".to_string())?;
    let title = title.ok_or_else(|| "--title is required".to_string())?;

    if matches!(output, OutputMode::Mime) && to.is_none() {
        return Err("--mime requires --to <recipient>".into());
    }

    Ok(Args::Render(RenderArgs {
        severity,
        title,
        summary,
        fields,
        runbook,
        on_call,
        to,
        output,
        theme,
    }))
}

fn select_theme(name: Option<&str>) -> Theme {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("sacredvote") | Some("sacred") => Theme::sacredvote(),
        _ => Theme::plausiden(),
    }
}

fn print_help() {
    eprintln!(
        "usage: mail-alert --severity {{critical|warning|info}} --title TEXT [opts]

Required:
  --severity NAME       critical | warning | info
  --title TEXT          one-line headline (becomes Subject prefix too)

Optional:
  --summary TEXT        body copy below the headline
  --field NAME=VALUE    repeatable; prefix NAME with @ for monospace value
                        (e.g. --field @Host=web-01 --field Used=95%)
  --runbook URL         renders as 'Open runbook →' CTA button
  --on-call CONTACT     footer line; defaults to 'page team@plausiden.com'
  --to RECIPIENT        required when --mime; sets To: header

Output (pick one; --html is default):
  --html                HTML alternative only (default)
  --plain               text/plain alternative only
  --mime                multipart/alternative envelope ready for `sendmail -t`

Theme:
  --theme NAME          plausiden (default) | sacredvote

Example:
  mail-alert --severity critical --title 'Disk 95% on web-01' \\
      --summary 'High-water crossed; mail will stall at 100%.' \\
      --field @Host=web-01 --field Mountpoint=/var --field Used=95% \\
      --runbook https://runbooks.plausiden.com/disk-full \\
      --to oncall@plausiden.com --mime | sendmail -t"
    );
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n");
            print_help();
            std::process::exit(2);
        }
    };
    let r = match args {
        Args::Help => {
            print_help();
            return;
        }
        Args::Render(r) => r,
    };

    let theme = select_theme(r.theme.as_deref());
    let doc = alert(
        r.severity,
        &r.title,
        &r.summary,
        r.fields,
        r.runbook.as_deref(),
        r.on_call.as_deref(),
    );

    let mut stdout = std::io::stdout();
    match r.output {
        OutputMode::Html => {
            let _ = write!(stdout, "{}", doc.render_html_with_theme(&theme));
        }
        OutputMode::Plain => {
            let _ = write!(stdout, "{}", doc.render_plain());
        }
        OutputMode::Mime => {
            let to = r.to.as_deref().unwrap_or("");
            emit_mime(&doc, &theme, to);
        }
    }
}

fn emit_mime(doc: &mail_templates::EmailDocument, theme: &Theme, to: &str) {
    let boundary = format!("alt-{}", boundary_token());
    let plain = doc.render_plain();
    let html = doc.render_html_with_theme(theme);
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "To: {to}");
    let _ = writeln!(stdout, "Subject: {}", doc.subject);
    let _ = writeln!(stdout, "MIME-Version: 1.0");
    let _ = writeln!(
        stdout,
        "Content-Type: multipart/alternative; boundary=\"{boundary}\""
    );
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "--{boundary}");
    let _ = writeln!(stdout, "Content-Type: text/plain; charset=\"utf-8\"");
    let _ = writeln!(stdout, "Content-Transfer-Encoding: 8bit");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "{plain}");
    let _ = writeln!(stdout, "--{boundary}");
    let _ = writeln!(stdout, "Content-Type: text/html; charset=\"utf-8\"");
    let _ = writeln!(stdout, "Content-Transfer-Encoding: 8bit");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "{html}");
    let _ = writeln!(stdout, "--{boundary}--");
}

fn boundary_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{nanos:08x}{pid:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_severity_accepts_aliases() {
        assert!(matches!(parse_severity("critical"), Some(AlertSeverity::Critical)));
        assert!(matches!(parse_severity("CRIT"), Some(AlertSeverity::Critical)));
        assert!(matches!(parse_severity("warn"), Some(AlertSeverity::Warning)));
        assert!(matches!(parse_severity("info"), Some(AlertSeverity::Info)));
        assert!(parse_severity("nonsense").is_none());
    }

    #[test]
    fn parse_field_handles_mono_prefix() {
        let f = parse_field("@Host=web-01").unwrap();
        assert_eq!(f.label, "Host");
        assert_eq!(f.value, "web-01");
        assert!(f.mono);

        let f = parse_field("Used=95%").unwrap();
        assert_eq!(f.label, "Used");
        assert_eq!(f.value, "95%");
        assert!(!f.mono);
    }

    #[test]
    fn parse_field_rejects_missing_equals() {
        assert!(parse_field("Host").is_none());
    }

    #[test]
    fn parse_args_requires_severity_and_title() {
        let argv: Vec<String> = vec!["mail-alert".into(), "--summary".into(), "x".into()];
        let r = parse_args(&argv);
        assert!(r.is_err());
        let msg = format!("{:?}", r.err());
        // Either severity or title is missing — error should name one of them.
        assert!(
            msg.contains("severity") || msg.contains("title"),
            "expected severity/title in error, got {msg}"
        );
    }

    #[test]
    fn parse_args_full_happy_path() {
        let argv: Vec<String> = vec![
            "mail-alert".into(),
            "--severity".into(),
            "critical".into(),
            "--title".into(),
            "Disk full".into(),
            "--summary".into(),
            "/var crossed 95%".into(),
            "--field".into(),
            "@Host=web-01".into(),
            "--field".into(),
            "Used=95%".into(),
            "--runbook".into(),
            "https://x/d".into(),
            "--on-call".into(),
            "oncall@x".into(),
            "--to".into(),
            "ops@x".into(),
            "--mime".into(),
        ];
        let r = parse_args(&argv).expect("parses");
        match r {
            Args::Render(rr) => {
                assert!(matches!(rr.severity, AlertSeverity::Critical));
                assert_eq!(rr.title, "Disk full");
                assert_eq!(rr.fields.len(), 2);
                assert!(rr.fields[0].mono);
                assert!(!rr.fields[1].mono);
                assert_eq!(rr.runbook.as_deref(), Some("https://x/d"));
                assert_eq!(rr.to.as_deref(), Some("ops@x"));
                assert_eq!(rr.output, OutputMode::Mime);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_args_mime_requires_to() {
        let argv: Vec<String> = vec![
            "mail-alert".into(),
            "--severity".into(),
            "warning".into(),
            "--title".into(),
            "x".into(),
            "--mime".into(),
        ];
        let r = parse_args(&argv);
        let msg = format!("{:?}", r.err());
        assert!(msg.contains("--to"), "expected --to error, got {msg}");
    }
}
