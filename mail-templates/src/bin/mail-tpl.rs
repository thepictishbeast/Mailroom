//! `mail-tpl` — render an `EmailDocument` JSON to HTML + plain.
//!
//! Reads JSON on stdin, writes the rendered output on stdout. The
//! shape of the JSON matches [`mail_templates::EmailDocument`]'s
//! serde representation. Optional flags select the theme and the
//! output side.
//!
//! ## Usage
//!
//! ```sh
//! # HTML side, default PlausiDen theme
//! cat doc.json | mail-tpl > out.html
//!
//! # Plain side
//! cat doc.json | mail-tpl --plain > out.txt
//!
//! # Multipart envelope (HTML and plain together, MIME-shaped)
//! cat doc.json | mail-tpl --mime > out.eml
//!
//! # SacredVote tenant chrome
//! cat doc.json | mail-tpl --theme sacredvote > out.html
//! ```
//!
//! ## Authoring tips
//!
//! Build the JSON with `jq -n` for one-shots, or write a small
//! shell wrapper that interpolates env vars. The
//! `mail_templates::prebuilt` helpers (`dns_records`, `bounce`,
//! `magic_link`) are easier to call from Rust; this CLI is meant
//! for ad-hoc operator scripting where adding a Rust dep would be
//! overkill.

use mail_templates::{EmailDocument, Theme};
use std::io::{Read as _, Write as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Html,
    Plain,
    Mime,
}

#[derive(Debug)]
enum Args {
    Help,
    Render {
        mode: OutputMode,
        theme: Option<String>,
    },
}

fn parse_args(argv: &[String]) -> Args {
    let mut mode = OutputMode::Html;
    let mut theme: Option<String> = None;
    let mut iter = argv.iter().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--help" | "-h" => return Args::Help,
            "--plain" => mode = OutputMode::Plain,
            "--html" => mode = OutputMode::Html,
            "--mime" => mode = OutputMode::Mime,
            "--theme" => {
                if let Some(v) = iter.next() {
                    theme = Some(v.clone());
                } else {
                    eprintln!("--theme requires a value");
                    std::process::exit(2);
                }
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }
    Args::Render { mode, theme }
}

fn select_theme(name: Option<&str>) -> Theme {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("sacredvote") | Some("sacred") => Theme::sacredvote(),
        Some("plausiden") | None => Theme::plausiden(),
        Some(other) => {
            eprintln!("unknown theme: {other}; falling back to plausiden");
            Theme::plausiden()
        }
    }
}

fn print_help() {
    let bin = std::env::args().next().unwrap_or_else(|| "mail-tpl".into());
    eprintln!(
        "usage: {bin} [--html|--plain|--mime] [--theme plausiden|sacredvote]

Reads an EmailDocument JSON on stdin; writes the rendered output
on stdout.

  --html        (default) Render the HTML alternative.
  --plain       Render the plain-text alternative.
  --mime        Render a multipart/alternative envelope (HTML + plain).
  --theme NAME  Pick the brand chrome — \"plausiden\" (default) or
                \"sacredvote\" for the alt tenant.
  -h, --help    Print this help."
    );
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let args = parse_args(&argv);
    let (mode, theme_name) = match args {
        Args::Help => {
            print_help();
            return;
        }
        Args::Render { mode, theme } => (mode, theme),
    };

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("read stdin: {e}");
        std::process::exit(1);
    }

    let doc: EmailDocument = match serde_json::from_str(&input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse EmailDocument JSON: {e}");
            std::process::exit(1);
        }
    };

    let theme = select_theme(theme_name.as_deref());

    match mode {
        OutputMode::Html => {
            let _ = write!(std::io::stdout(), "{}", doc.render_html_with_theme(&theme));
        }
        OutputMode::Plain => {
            let _ = write!(std::io::stdout(), "{}", doc.render_plain());
        }
        OutputMode::Mime => emit_mime(&doc, &theme),
    }
}

/// Print a `multipart/alternative` MIME envelope with both plain and
/// HTML alternatives. Convenience for piping straight to `sendmail`:
///
/// ```sh
/// cat doc.json | mail-tpl --mime | sendmail -t
/// ```
///
/// The caller is responsible for setting From / To / Subject in the
/// JSON's `subject` (becomes the `Subject:`); the Mime envelope's
/// other headers are deliberately omitted so the SMTP submission
/// path supplies them.
fn emit_mime(doc: &EmailDocument, theme: &Theme) {
    let boundary = format!("alt-{}", boundary_token());
    let plain = doc.render_plain();
    let html = doc.render_html_with_theme(theme);
    let mut stdout = std::io::stdout();
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
    fn parse_args_defaults_to_html_no_theme() {
        let argv = vec!["mail-tpl".into()];
        match parse_args(&argv) {
            Args::Render { mode, theme } => {
                assert_eq!(mode, OutputMode::Html);
                assert!(theme.is_none());
            }
            _ => panic!("expected Render"),
        }
    }

    #[test]
    fn parse_args_handles_plain_flag() {
        let argv = vec!["mail-tpl".into(), "--plain".into()];
        match parse_args(&argv) {
            Args::Render { mode, .. } => assert_eq!(mode, OutputMode::Plain),
            _ => panic!(),
        }
    }

    #[test]
    fn parse_args_handles_theme_arg() {
        let argv = vec!["mail-tpl".into(), "--theme".into(), "sacredvote".into()];
        match parse_args(&argv) {
            Args::Render { theme, .. } => {
                assert_eq!(theme.as_deref(), Some("sacredvote"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_args_help_short_and_long() {
        for flag in ["-h", "--help"] {
            let argv = vec!["mail-tpl".into(), flag.into()];
            assert!(matches!(parse_args(&argv), Args::Help));
        }
    }

    #[test]
    fn select_theme_resolves_known_names() {
        assert_eq!(
            select_theme(Some("plausiden")).brand_name,
            Theme::plausiden().brand_name
        );
        assert_eq!(
            select_theme(Some("sacredvote")).brand_name,
            Theme::sacredvote().brand_name
        );
        // Default (None) falls back to plausiden.
        assert_eq!(select_theme(None).brand_name, Theme::plausiden().brand_name);
    }

    #[test]
    fn select_theme_unknown_falls_back_to_plausiden() {
        assert_eq!(
            select_theme(Some("nonexistent")).brand_name,
            Theme::plausiden().brand_name
        );
    }
}
