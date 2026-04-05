//! mail-admin — CLI tool for managing the secure email server.
//!
//! Provides subcommands for configuration validation, Sieve generation,
//! DKIM status, health checks, and Postfix vmailbox management.
//! Reads domain configuration from the orchestrator TOML file.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mail_config::{Domain, Mailbox, MailboxKind};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    }
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

    // Check DKIM config files
    for (name, path) in [
        ("KeyTable", mail_config::dkim::KEY_TABLE_PATH),
        ("SigningTable", mail_config::dkim::SIGNING_TABLE_PATH),
        ("TrustedHosts", mail_config::dkim::TRUSTED_HOSTS_PATH),
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
