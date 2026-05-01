//! mail-admin — CLI tool for managing the secure email server.
//!
//! Provides subcommands for configuration validation, Sieve generation,
//! DKIM status, health checks, and Postfix vmailbox management.
//! Reads domain configuration from the orchestrator TOML file.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mail_config::{
    CategoryRules, Domain, Mailbox, MailboxKind, MailboxLayout,
    categories::SieveEmitOptions, dovecot::generate_sieve_runtime_conf,
};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Deployed paths the new emit-* subcommands write to by default.
/// Match the live VPS layout.
const DEFAULT_CATEGORIES_SIEVE: &str = "/etc/dovecot/sieve/categories.sieve";
const DEFAULT_MAILBOXES_CONF: &str = "/etc/dovecot/conf.d/15-mailboxes.conf";
const DEFAULT_SIEVE_RUNTIME_CONF: &str = "/etc/dovecot/conf.d/90-sieve.conf";

#[derive(Parser)]
#[command(name = "mail-admin", version, about = "Manage the secure email server")]
struct Cli {
    /// Path to the orchestrator TOML config file.
    #[arg(short, long, default_value = "/etc/mail-orchestrator/orchestrator.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate all mail server configuration files for consistency.
    Validate,

    /// Check the health of all mail services (Postfix, Dovecot, OpenDKIM).
    Health,

    /// Generate Sieve scripts for all mailbox kinds.
    GenerateSieve {
        /// Domain name.
        #[arg(long)]
        domain: String,
        /// Write scripts to disk (default: print to stdout).
        #[arg(long)]
        write: bool,
    },

    /// Show DKIM configuration entries for a domain.
    DkimStatus {
        /// Domain to check.
        #[arg(long)]
        domain: String,
    },

    /// Generate Postfix vmailbox entries for a domain.
    Vmailbox {
        /// Domain name.
        #[arg(long)]
        domain: String,
    },

    /// Show delivery log from the orchestrator database.
    Log {
        /// Maximum number of entries to show.
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Filter by mailbox name.
        #[arg(short, long)]
        mailbox: Option<String>,
    },

    /// Test SMTP connectivity to the local Postfix instance.
    TestSmtp {
        /// SMTP host to test.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// SMTP port.
        #[arg(long, default_value = "25")]
        port: u16,
    },

    /// Emit the platform-wide categories Sieve script from typed
    /// `CategoryRules` and (by default) write it to the deployed path.
    /// Diffs against the live file before overwriting.
    EmitCategories {
        /// Output file. Defaults to the deployed path.
        #[arg(long, default_value = DEFAULT_CATEGORIES_SIEVE)]
        output: PathBuf,
        /// Print to stdout instead of writing to disk.
        #[arg(long)]
        stdout: bool,
        /// Emit the `editheader` audit-tag variant. Requires Dovecot to
        /// have `sieve_extensions = +editheader` (use `emit-sieve-runtime
        /// --audit-headers` to generate that config).
        #[arg(long)]
        audit_headers: bool,
        /// Overwrite the live file even if it differs from the rendered
        /// output. Without this flag, a difference produces a diff
        /// preview and a non-zero exit so the operator can review.
        #[arg(long)]
        force: bool,
    },

    /// Emit the Dovecot mailbox-layout config (`15-mailboxes.conf`) from
    /// typed `MailboxLayout` and (by default) write it to the deployed
    /// path. Diff/force semantics mirror `emit-categories`.
    EmitMailboxes {
        /// Output file.
        #[arg(long, default_value = DEFAULT_MAILBOXES_CONF)]
        output: PathBuf,
        /// Print to stdout instead of writing to disk.
        #[arg(long)]
        stdout: bool,
        /// Overwrite a differing live file.
        #[arg(long)]
        force: bool,
    },

    /// Emit the Dovecot Sieve-runtime config (`90-sieve.conf`).
    EmitSieveRuntime {
        /// Output file.
        #[arg(long, default_value = DEFAULT_SIEVE_RUNTIME_CONF)]
        output: PathBuf,
        /// Path the categories.sieve will live at — must match what
        /// `emit-categories --output` wrote.
        #[arg(long, default_value = DEFAULT_CATEGORIES_SIEVE)]
        categories_path: PathBuf,
        /// Print to stdout instead of writing to disk.
        #[arg(long)]
        stdout: bool,
        /// Enable `sieve_extensions = +editheader` so audit-tagged
        /// scripts work.
        #[arg(long)]
        audit_headers: bool,
        /// Overwrite a differing live file.
        #[arg(long)]
        force: bool,
    },

    /// Render all three deployed configs in one shot. Useful pre-deploy
    /// gate: with no `--force`, exits non-zero if anything differs from
    /// the live state, printing the diffs.
    EmitAll {
        /// Print all three to stdout instead of writing.
        #[arg(long)]
        stdout: bool,
        /// Use audit headers (passes through to categories + runtime).
        #[arg(long)]
        audit_headers: bool,
        /// Overwrite any differing live files.
        #[arg(long)]
        force: bool,
    },

    /// Classify a single RFC822 message read from stdin against the
    /// typed `CategoryRules`. Prints which rules would fire, in the
    /// same order Pigeonhole would fire them.
    ///
    /// Demo of the round-trip: pipe `doveadm fetch -u user text uid N`
    /// through this and you'll see exactly what the deployed Sieve
    /// would do — same code path the Thundercrab client uses offline.
    Classify,

    /// Parse an ICS payload from stdin and print typed CalendarItems.
    /// Useful for inspecting `.ics` attachments — pipe `doveadm fetch
    /// ... part 2.0` (or any source) through this and confirm what
    /// `mail-calendar::parse_ics` extracts.
    CalendarParse {
        /// Output format: `summary` (one-line per item), `json`
        /// (full typed JSON), or `ics` (round-trip — re-emit ICS).
        #[arg(long, default_value = "summary")]
        format: String,
    },

    /// Merge an ICS payload from stdin into a calendar file at `path`,
    /// keyed by UID. Items in the incoming payload that match an
    /// existing UID replace it in place; new UIDs are appended;
    /// existing items not mentioned in the incoming payload are
    /// preserved (partial sync).
    ///
    /// Atomic — writes to a tempfile in the same directory and
    /// renames, so a reader never sees a half-written calendar.
    CalendarMerge {
        /// Path to the calendar file. Created if missing.
        #[arg(long)]
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate => cmd_validate(),
        Commands::Health => cmd_health(),
        Commands::GenerateSieve { domain, write } => cmd_generate_sieve(&domain, write),
        Commands::DkimStatus { domain } => cmd_dkim_status(&domain),
        Commands::Vmailbox { domain } => cmd_vmailbox(&domain),
        Commands::Log { limit, mailbox } => cmd_log(limit, mailbox.as_deref()),
        Commands::TestSmtp { host, port } => cmd_test_smtp(&host, port),
        Commands::EmitCategories { output, stdout, audit_headers, force } => {
            cmd_emit_categories(&output, stdout, audit_headers, force)
        }
        Commands::EmitMailboxes { output, stdout, force } => {
            cmd_emit_mailboxes(&output, stdout, force)
        }
        Commands::EmitSieveRuntime { output, categories_path, stdout, audit_headers, force } => {
            cmd_emit_sieve_runtime(&output, &categories_path, stdout, audit_headers, force)
        }
        Commands::EmitAll { stdout, audit_headers, force } => {
            cmd_emit_all(stdout, audit_headers, force)
        }
        Commands::Classify => cmd_classify(),
        Commands::CalendarParse { format } => cmd_calendar_parse(&format),
        Commands::CalendarMerge { path } => cmd_calendar_merge(&path),
    }
}

// ---------------------------------------------------------------------
// calendar — parse + merge
// ---------------------------------------------------------------------

fn cmd_calendar_parse(format: &str) -> Result<()> {
    use std::io::Read as _;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let items = mail_calendar::parse_ics(&buf)
        .map_err(|e| anyhow::anyhow!("parse_ics: {e}"))?;

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&items)?;
            println!("{json}");
        }
        "ics" => {
            print!("{}", mail_calendar::write_ics(&items));
        }
        "summary" | _ => {
            println!("{} item(s):", items.len());
            for item in &items {
                match item {
                    mail_calendar::CalendarItem::Event(e) => {
                        println!(
                            "  EVENT  {uid}  {start} → {end}  {summary}",
                            uid = e.uid,
                            start = e.start.format("%Y-%m-%d %H:%M"),
                            end = e.end.format("%H:%M"),
                            summary = e.summary,
                        );
                    }
                    mail_calendar::CalendarItem::Todo(t) => {
                        let due = t
                            .due
                            .map_or_else(|| "(no due)".to_string(), |d| d.format("%Y-%m-%d").to_string());
                        println!(
                            "  TODO   {uid}  due {due}  {summary}",
                            uid = t.uid,
                            summary = t.summary,
                        );
                    }
                    mail_calendar::CalendarItem::Alarm(a) => {
                        println!(
                            "  ALARM  {trigger}  {desc}",
                            trigger = a.trigger.format("%Y-%m-%d %H:%M"),
                            desc = a.description,
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn cmd_calendar_merge(path: &Path) -> Result<()> {
    use std::io::Read as _;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let incoming = mail_calendar::parse_ics(&buf)
        .map_err(|e| anyhow::anyhow!("parse_ics on stdin: {e}"))?;
    let report = mail_calendar::merge::merge_to_file(path, &incoming)
        .map_err(|e| anyhow::anyhow!("merge_to_file: {e}"))?;

    eprintln!(
        "merged: {added} added · {updated} updated · {alarms} alarms · {total} total in {p}",
        added = report.added_count(),
        updated = report.updated_count(),
        alarms = report.alarms_count(),
        total = report.outcomes.len(),
        p = path.display(),
    );
    for outcome in &report.outcomes {
        match outcome {
            mail_calendar::MergeOutcome::Added(uid) => eprintln!("  + {uid}"),
            mail_calendar::MergeOutcome::Updated(uid) => eprintln!("  ~ {uid}"),
            mail_calendar::MergeOutcome::AppendedAlarm => eprintln!("  ! standalone alarm"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------
// classify
// ---------------------------------------------------------------------

/// Read RFC822 from stdin, parse headers, run the typed evaluator, and
/// print the result.
///
/// SECURITY: The body is read from stdin but never inspected — only
/// headers up to the first blank line. This keeps the tool safe to use
/// on untrusted messages.
///
/// BUG ASSUMPTION: Header continuation lines (RFC 5322 §2.2.3) start
/// with SP or HTAB and are joined into the previous header. We don't
/// implement encoded-word decoding (`=?utf-8?...?=`) — Sieve doesn't
/// either, so matching parity is preserved.
fn cmd_classify() -> Result<()> {
    use mail_config::MessageContext;
    use std::io::Read;

    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read stdin")?;

    let parsed = parse_rfc822_headers(&buf);
    let from_addr = extract_address(parsed.from.as_deref().unwrap_or("")).to_lowercase();
    let subject = parsed.subject.unwrap_or_default();

    // Build the lowercased-key headers for MessageContext.
    let mut hdr_pairs: Vec<(String, String)> = Vec::with_capacity(parsed.other.len());
    for (k, v) in &parsed.other {
        hdr_pairs.push((k.to_lowercase(), v.clone()));
    }
    let ctx = MessageContext {
        headers: &hdr_pairs,
        from_address: &from_addr,
        subject: &subject,
    };

    let rules = CategoryRules::default();
    let hits = rules.evaluate(&ctx);

    println!("From:    {from_addr}");
    println!("Subject: {subject}");
    println!();
    if hits.is_empty() {
        println!("No category rule fired — message stays in INBOX with no flag.");
    } else {
        println!("Rules that would fire (in order):");
        for r in hits {
            println!("  - {} (score={}) → {}", r.id, r.score, action_summary(&r.action));
            if r.stop_on_match {
                println!("    [stop_on_match: subsequent rules skipped]");
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct ParsedHeaders {
    from: Option<String>,
    subject: Option<String>,
    /// Every other header as `(name, value)` in order. May contain
    /// duplicate names (e.g., `Received:`).
    other: Vec<(String, String)>,
}

/// Parse RFC822 headers up to the first blank line. Handles continuation
/// lines (lines starting with whitespace are joined to the previous
/// header). Returns headers split into From/Subject (most-recent wins
/// per RFC 5322 §3.6, but in practice messages have one each) plus
/// everything else.
fn parse_rfc822_headers(raw: &str) -> ParsedHeaders {
    let mut out = ParsedHeaders::default();
    let mut current: Option<(String, String)> = None;

    for line in raw.lines() {
        if line.is_empty() {
            // End of headers.
            if let Some((k, v)) = current.take() {
                stash_header(&mut out, k, v);
            }
            return out;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line. Append (with single space, per
            // unfolding rules) to the current header value.
            if let Some((_, v)) = current.as_mut() {
                v.push(' ');
                v.push_str(line.trim_start());
            }
            continue;
        }
        if let Some((k, v)) = current.take() {
            stash_header(&mut out, k, v);
        }
        if let Some(idx) = line.find(':') {
            let name = line[..idx].trim().to_string();
            let value = line[idx + 1..].trim().to_string();
            current = Some((name, value));
        }
    }
    if let Some((k, v)) = current.take() {
        stash_header(&mut out, k, v);
    }
    out
}

fn stash_header(out: &mut ParsedHeaders, name: String, value: String) {
    match name.to_ascii_lowercase().as_str() {
        "from" if out.from.is_none() => out.from = Some(value),
        "subject" if out.subject.is_none() => out.subject = Some(value),
        _ => out.other.push((name, value)),
    }
}

/// Pull the email address out of a `From:` header value. Handles both
/// `addr@dom` and `Name <addr@dom>` forms. Falls back to the raw value.
fn extract_address(raw: &str) -> String {
    let s = raw.trim();
    if let Some(start) = s.rfind('<') {
        if let Some(end) = s[start + 1..].find('>') {
            return s[start + 1..start + 1 + end].trim().to_string();
        }
    }
    s.to_string()
}

fn action_summary(action: &mail_config::Action) -> String {
    match action {
        mail_config::Action::FileInto { folder } => format!("FileInto({folder})"),
        mail_config::Action::SetFlag { flag } => format!("SetFlag({flag})"),
        mail_config::Action::Sequence { actions } => {
            let parts: Vec<_> = actions.iter().map(action_summary).collect();
            format!("Sequence[{}]", parts.join(", "))
        }
    }
}

// ---------------------------------------------------------------------
// emit-* helpers
// ---------------------------------------------------------------------

/// Render a piece of config and either print it, diff against live, or
/// atomically write it. Returns `Ok(true)` if the live file changed,
/// `Ok(false)` if it was already current.
///
/// SECURITY: Writes via tempfile + rename for atomicity — readers
/// (Dovecot reading the categories.sieve at LMTP-time) never see a
/// half-written file.
fn render_and_write(
    label: &str,
    output: &Path,
    rendered: &str,
    stdout_only: bool,
    force: bool,
) -> Result<bool> {
    if stdout_only {
        print!("{rendered}");
        return Ok(false);
    }

    let existing = match std::fs::read_to_string(output) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(anyhow::Error::from(e).context(format!("read {label}"))),
    };

    if existing.as_deref() == Some(rendered) {
        println!("[{label}] OK — live file already matches typed config");
        return Ok(false);
    }

    if let Some(prev) = &existing {
        println!("[{label}] differs from live file:");
        print_unified_diff(prev, rendered);
    } else {
        println!("[{label}] live file does not exist; would create:");
        for line in rendered.lines().take(40) {
            println!("    + {line}");
        }
    }

    if !force {
        bail!("[{label}] refusing to write without --force; review the diff above");
    }

    write_atomic(output, rendered).with_context(|| format!("write {label}"))?;
    println!("[{label}] WROTE {} ({} bytes)", output.display(), rendered.len());
    println!("    → run `systemctl reload dovecot` to pick up changes");
    Ok(true)
}

/// Atomic write: render to `<output>.tmp.<pid>` then rename. Same
/// filesystem only, so the rename is guaranteed atomic on Linux.
fn write_atomic(output: &Path, contents: &str) -> Result<()> {
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent: {}", output.display()))?;
    let tmp = parent.join(format!(
        ".{}.tmp.{}",
        output
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("emit"),
        std::process::id()
    ));
    std::fs::write(&tmp, contents).with_context(|| format!("write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, output).with_context(|| {
        format!(
            "rename {} → {}",
            tmp.display(),
            output.display()
        )
    })?;
    Ok(())
}

/// Tiny unified-diff printer — line-by-line, no fanciness, no extra
/// crate dep. Prefixes: ` ` context (when adjacent to changes), `-`
/// removed, `+` added.
fn print_unified_diff(old: &str, new: &str) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut i = 0;
    let mut j = 0;
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
            continue;
        }
        // Find the next sync point.
        let mut sync_old = None;
        for (k, old_line) in old_lines.iter().enumerate().skip(i) {
            if let Some(p) = new_lines[j..].iter().position(|x| x == old_line) {
                sync_old = Some((k, j + p));
                break;
            }
        }
        let (next_i, next_j) = sync_old.unwrap_or((old_lines.len(), new_lines.len()));
        for line in &old_lines[i..next_i] {
            println!("    - {line}");
        }
        for line in &new_lines[j..next_j] {
            println!("    + {line}");
        }
        i = next_i;
        j = next_j;
    }
}

fn cmd_emit_categories(
    output: &Path,
    stdout: bool,
    audit_headers: bool,
    force: bool,
) -> Result<()> {
    let opts = SieveEmitOptions { audit_header: audit_headers };
    let rendered = CategoryRules::default().to_sieve_with(opts);
    render_and_write("categories.sieve", output, &rendered, stdout, force).map(|_| ())
}

fn cmd_emit_mailboxes(output: &Path, stdout: bool, force: bool) -> Result<()> {
    let rendered = MailboxLayout::default().to_dovecot_conf();
    render_and_write("15-mailboxes.conf", output, &rendered, stdout, force).map(|_| ())
}

fn cmd_emit_sieve_runtime(
    output: &Path,
    categories_path: &Path,
    stdout: bool,
    audit_headers: bool,
    force: bool,
) -> Result<()> {
    let cat_str = categories_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("categories_path is not valid UTF-8"))?;
    let rendered = generate_sieve_runtime_conf(cat_str, audit_headers);
    render_and_write("90-sieve.conf", output, &rendered, stdout, force).map(|_| ())
}

/// Render all three. With `--force`, writes everything and reports a
/// summary. Without, prints diffs and returns non-zero if any file
/// would change — useful as a pre-deploy gate (CI).
fn cmd_emit_all(stdout: bool, audit_headers: bool, force: bool) -> Result<()> {
    let cats_path = PathBuf::from(DEFAULT_CATEGORIES_SIEVE);
    let mb_path = PathBuf::from(DEFAULT_MAILBOXES_CONF);
    let runtime_path = PathBuf::from(DEFAULT_SIEVE_RUNTIME_CONF);

    let cats_rendered =
        CategoryRules::default().to_sieve_with(SieveEmitOptions { audit_header: audit_headers });
    let mb_rendered = MailboxLayout::default().to_dovecot_conf();
    let runtime_rendered =
        generate_sieve_runtime_conf(cats_path.to_str().unwrap(), audit_headers);

    let mut any_changed = false;
    for (label, path, rendered) in [
        ("categories.sieve", &cats_path, &cats_rendered),
        ("15-mailboxes.conf", &mb_path, &mb_rendered),
        ("90-sieve.conf", &runtime_path, &runtime_rendered),
    ] {
        // Run each with `force = false` first to surface diffs even
        // when stdout-only or when not forcing — but if force is on,
        // forward it so writes happen.
        match render_and_write(label, path, rendered, stdout, force) {
            Ok(changed) => any_changed |= changed,
            Err(e) => {
                println!("{e}");
                any_changed = true;
            }
        }
    }

    if !force && any_changed && !stdout {
        bail!("emit-all detected drift; pass --force after reviewing");
    }
    Ok(())
}

fn cmd_validate() -> Result<()> {
    println!("Validating mail server configuration...\n");
    let mut errors = 0;

    // Check Postfix vmailbox
    print!("  Postfix vmailbox ({})... ", mail_config::postfix::VMAILBOX_PATH);
    match mail_config::postfix::read_vmailbox(Path::new(mail_config::postfix::VMAILBOX_PATH)) {
        Ok(entries) => println!("OK ({} entries)", entries.len()),
        Err(e) => { println!("FAIL: {}", e); errors += 1; }
    }

    // Check Dovecot passwd file
    let passwd_path = mail_config::dovecot::PASSWD_FILE_PATH;
    print!("  Dovecot users ({})... ", passwd_path);
    match std::fs::read_to_string(passwd_path) {
        Ok(content) => {
            let entries = mail_config::dovecot::parse_passwd_file(&content);
            println!("OK ({} users)", entries.len());
        }
        Err(e) => { println!("FAIL: {}", e); errors += 1; }
    }

    // Check DKIM keys directory
    print!("  OpenDKIM keys ({})... ", mail_config::dkim::KEYS_DIR);
    match std::fs::read_dir(mail_config::dkim::KEYS_DIR) {
        Ok(entries) => {
            let count = entries.filter_map(|e| e.ok()).count();
            println!("OK ({} domain dirs)", count);
        }
        Err(e) => { println!("FAIL: {}", e); errors += 1; }
    }

    // Check DKIM config files. Use the resolver functions so verify-config
    // works on Debian (lowercase key.table / signing.table / trusted.hosts)
    // as well as RHEL/Fedora (CamelCase). The resolver falls back to the
    // canonical path on a fresh box where neither variant exists yet.
    for (name, path) in [
        ("KeyTable", mail_config::dkim::resolve_key_table_path()),
        ("SigningTable", mail_config::dkim::resolve_signing_table_path()),
        ("TrustedHosts", mail_config::dkim::resolve_trusted_hosts_path()),
    ] {
        print!("  OpenDKIM {} ({})... ", name, path);
        match std::fs::metadata(path) {
            Ok(m) => println!("OK ({} bytes)", m.len()),
            Err(e) => { println!("FAIL: {}", e); errors += 1; }
        }
    }

    println!();
    if errors > 0 {
        println!("RESULT: {} error(s) found", errors);
        std::process::exit(1);
    } else {
        println!("RESULT: All configuration files valid");
    }
    Ok(())
}

fn cmd_health() -> Result<()> {
    println!("Mail server health check...\n");

    for (service, check_cmd) in [
        ("Postfix", "postfix status"),
        ("Dovecot", "doveadm service status"),
        ("OpenDKIM", "systemctl is-active opendkim"),
    ] {
        print!("  {}... ", service);
        let parts: Vec<&str> = check_cmd.split_whitespace().collect();
        match Command::new(parts[0]).args(&parts[1..]).output() {
            Ok(output) => {
                if output.status.success() {
                    println!("RUNNING");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    println!("DOWN ({})", stderr.trim());
                }
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // Check ports
    println!();
    for (port, desc) in [(25, "SMTP"), (587, "Submission"), (993, "IMAPS"), (4190, "ManageSieve")] {
        print!("  Port {} ({})... ", port, desc);
        match std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", port).parse().unwrap(),
            std::time::Duration::from_secs(2),
        ) {
            Ok(_) => println!("OPEN"),
            Err(_) => println!("CLOSED"),
        }
    }

    // Check mail queue
    println!();
    print!("  Postfix queue... ");
    match Command::new("postqueue").arg("-p").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("Mail queue is empty") {
                println!("EMPTY");
            } else {
                let lines: Vec<&str> = stdout.lines().collect();
                println!("{} message(s)", lines.len().saturating_sub(1));
            }
        }
        Err(e) => println!("ERROR: {}", e),
    }

    Ok(())
}

fn cmd_generate_sieve(domain: &str, write: bool) -> Result<()> {
    let mailbox_defs = [
        ("noreply", MailboxKind::Service),
        ("alerts", MailboxKind::Service),
        ("support", MailboxKind::Service),
        ("router", MailboxKind::Router),
        ("tim", MailboxKind::User),
        ("admin", MailboxKind::User),
        ("legal", MailboxKind::Legal),
        ("privacy", MailboxKind::Legal),
        ("security", MailboxKind::Legal),
    ];

    for (local, kind) in &mailbox_defs {
        let mb = Mailbox {
            local_part: local.to_string(),
            kind: *kind,
            display_name: None,
        };
        let script = mail_config::sieve::generate_sieve(&mb, domain);

        if write {
            let sieve_dir = format!("/var/mail/vhosts/{}/{}/sieve", domain, local);
            std::fs::create_dir_all(&sieve_dir)
                .with_context(|| format!("Failed to create {}", sieve_dir))?;
            let path = format!("{}/default.sieve", sieve_dir);
            std::fs::write(&path, &script)
                .with_context(|| format!("Failed to write {}", path))?;
            println!("  Wrote {}", path);
        } else {
            println!("--- {}@{} ({:?}) ---", local, domain, kind);
            println!("{}", script);
        }
    }

    Ok(())
}

fn cmd_dkim_status(domain: &str) -> Result<()> {
    let d = Domain {
        name: domain.to_string(),
        dkim_enabled: true,
        dkim_selector: "default".to_string(),
        mailboxes: vec![],
    };

    println!("DKIM Configuration for {}\n", domain);
    println!("  KeyTable entry:     {}", mail_config::dkim::key_table_entry(&d));
    println!("  SigningTable entry:  {}", mail_config::dkim::signing_table_entry(&d));

    let key_path = mail_config::dkim::key_path(domain, "default");
    print!("  Private key:        {} ", key_path);
    if Path::new(&key_path).exists() {
        println!("[EXISTS]");
    } else {
        println!("[MISSING]");
    }

    // Check DNS TXT record
    print!("  DNS TXT record:     ");
    match Command::new("dig")
        .args(["+short", "TXT", &format!("default._domainkey.{}", domain)])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                println!("[NOT FOUND]");
            } else {
                println!("{}", stdout.trim());
            }
        }
        Err(_) => println!("[dig not available]"),
    }

    Ok(())
}

fn cmd_vmailbox(domain: &str) -> Result<()> {
    println!("Postfix vmailbox entries for {}\n", domain);
    match mail_config::postfix::read_vmailbox(Path::new(mail_config::postfix::VMAILBOX_PATH)) {
        Ok(entries) => {
            let domain_entries: Vec<_> = entries.iter().filter(|e| e.contains(domain)).collect();
            if domain_entries.is_empty() {
                println!("  No entries found for {}", domain);
            } else {
                for entry in &domain_entries {
                    println!("  {}", entry);
                }
                println!("\n  Total: {} mailboxes", domain_entries.len());
            }
        }
        Err(e) => println!("  Error reading vmailbox: {}", e),
    }
    Ok(())
}

fn cmd_log(limit: usize, mailbox: Option<&str>) -> Result<()> {
    let db_path = "/var/lib/mail-orchestrator/orchestrator.db";
    let conn = rusqlite::Connection::open(db_path)
        .with_context(|| format!("Cannot open database at {}", db_path))?;

    let query = if let Some(mb) = mailbox {
        format!(
            "SELECT created_at, direction, from_addr, to_addr, subject, mailbox, status FROM email_log WHERE mailbox = '{}' ORDER BY created_at DESC LIMIT {}",
            mb.replace('\'', "''"), limit
        )
    } else {
        format!(
            "SELECT created_at, direction, from_addr, to_addr, subject, mailbox, status FROM email_log ORDER BY created_at DESC LIMIT {}",
            limit
        )
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;

    println!("{:<20} {:<8} {:<30} {:<30} {:<10} {:<40}", "Time", "Dir", "From", "To", "Status", "Subject");
    println!("{}", "-".repeat(140));

    for row in rows {
        let (time, dir, from, to, subject, _mailbox, status) = row?;
        let subj = subject.unwrap_or_default();
        let subj_short: String = subj.chars().take(40).collect();
        println!("{:<20} {:<8} {:<30} {:<30} {:<10} {}", time, dir, from, to, status, subj_short);
    }

    Ok(())
}

fn cmd_test_smtp(host: &str, port: u16) -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    println!("Testing SMTP connection to {}:{}...\n", host, port);

    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().context("Invalid address")?,
        std::time::Duration::from_secs(5),
    ).context("Connection failed")?;

    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf)?;
    let banner = String::from_utf8_lossy(&buf[..n]);
    println!("  Banner: {}", banner.trim());

    stream.write_all(b"EHLO localhost\r\n")?;
    let n = stream.read(&mut buf)?;
    let ehlo_response = String::from_utf8_lossy(&buf[..n]);
    println!("  EHLO response:");
    for line in ehlo_response.lines() {
        println!("    {}", line);
    }

    stream.write_all(b"QUIT\r\n")?;
    println!("\n  SMTP connection test: PASSED");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_headers() {
        let raw = "From: alice@example.com\r\nSubject: hi\r\nX-Custom: yes\r\n\r\nbody";
        let p = parse_rfc822_headers(raw);
        assert_eq!(p.from.as_deref(), Some("alice@example.com"));
        assert_eq!(p.subject.as_deref(), Some("hi"));
        assert_eq!(p.other, vec![("X-Custom".into(), "yes".into())]);
    }

    #[test]
    fn parse_continuation_lines() {
        let raw = "Subject: line one\r\n\tline two\r\n line three\r\n\r\n";
        let p = parse_rfc822_headers(raw);
        assert_eq!(p.subject.as_deref(), Some("line one line two line three"));
    }

    #[test]
    fn parse_named_from_extracts_address() {
        let raw = "From: \"Alice\" <alice@example.com>\r\n\r\n";
        let p = parse_rfc822_headers(raw);
        let addr = extract_address(p.from.as_deref().unwrap());
        assert_eq!(addr, "alice@example.com");
    }

    #[test]
    fn parse_duplicate_received_headers_preserved() {
        let raw = "Received: from a\r\nReceived: from b\r\n\r\n";
        let p = parse_rfc822_headers(raw);
        let count = p
            .other
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("received"))
            .count();
        assert_eq!(count, 2);
    }

    #[test]
    fn parse_no_blank_line_still_works() {
        // Some IMAP fetches return only headers without a body separator.
        let raw = "From: a@b.com\r\nSubject: x\r\n";
        let p = parse_rfc822_headers(raw);
        assert_eq!(p.from.as_deref(), Some("a@b.com"));
        assert_eq!(p.subject.as_deref(), Some("x"));
    }
}
