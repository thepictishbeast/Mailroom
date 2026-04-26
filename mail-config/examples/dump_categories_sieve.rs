//! Print the default category Sieve script to stdout. Useful for
//! sievec-roundtrip verification:
//!
//!   cargo run -q -p mail-config --example dump_categories_sieve > /tmp/cats.sieve
//!   sievec /tmp/cats.sieve

fn main() {
    print!("{}", mail_config::CategoryRules::default().to_sieve());
}
