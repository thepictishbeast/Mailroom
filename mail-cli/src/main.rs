//! mail-admin — CLI tool for managing the secure email server.
//!
//! Provides subcommands for account management, DKIM operations,
//! health checks, and configuration validation.

use clap::{Parser, Subcommand};
use mail_config::{Domain, Mailbox, MailboxKind};

#[derive(Parser)]
#[command(name = "mail-admin", about = "Manage the secure email server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all configured domains and their mailboxes.
    List,
    /// Validate all configuration files for consistency.
    Validate,
    /// Generate Sieve scripts for all mailboxes.
    GenerateSieve {
        /// Domain to generate scripts for.
        #[arg(long)]
        domain: String,
    },
    /// Show DKIM configuration for a domain.
    DkimStatus {
        /// Domain to check.
        #[arg(long)]
        domain: String,
    },
    /// Generate Postfix vmailbox entries for a domain.
    Vmailbox {
        /// Domain to generate entries for.
        #[arg(long)]
        domain: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            println!("Configured domains:");
            println!("  (read from orchestrator.toml — not yet wired)");
        }
        Commands::Validate => {
            println!("Validating configuration files...");
            println!("  Postfix vmailbox:  (check pending)");
            println!("  Dovecot users:     (check pending)");
            println!("  OpenDKIM keys:     (check pending)");
            println!("  Sieve scripts:     (check pending)");
        }
        Commands::GenerateSieve { domain } => {
            let example = Mailbox {
                local_part: "noreply".into(),
                kind: MailboxKind::Service,
                display_name: None,
            };
            let script = mail_config::sieve::generate_sieve(&example, &domain);
            println!("{}", script);
        }
        Commands::DkimStatus { domain } => {
            let d = Domain {
                name: domain.clone(),
                dkim_enabled: true,
                dkim_selector: "default".into(),
                mailboxes: vec![],
            };
            println!("KeyTable:    {}", mail_config::dkim::key_table_entry(&d));
            println!("SigningTable: {}", mail_config::dkim::signing_table_entry(&d));
            println!("Key path:    {}", mail_config::dkim::key_path(&domain, "default"));
        }
        Commands::Vmailbox { domain } => {
            println!("(Reads domain config from orchestrator.toml — not yet wired)");
            println!("Domain: {}", domain);
        }
    }

    Ok(())
}
