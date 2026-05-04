//! Server-wide mailbox layout — the set of folders every account gets.
//!
//! This is *not* the per-user mailbox database (that's `dovecot.rs`).
//! This is the Dovecot `namespace inbox { mailbox X { ... } }` block that
//! defines special-use folders (Drafts/Sent/Trash/Junk/Archive/Important)
//! plus Gmail-style category folders (Updates/Social/Promotions/Forums).
//!
//! Emits `15-mailboxes.conf` for Dovecot 2.4.

use serde::{Deserialize, Serialize};

/// IANA RFC 6154 special-use markers + the platform extension `\Important`.
///
/// These tell IMAP clients (Thunderbird, Outlook, Thundercrab) which folder
/// is which without manual user mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SpecialUse {
    Drafts,
    Sent,
    Trash,
    Junk,
    Archive,
    Important,
    /// No special-use marker — a category folder like Updates/Social.
    None,
}

impl SpecialUse {
    /// The Dovecot/IMAP marker string, including the leading backslash.
    /// Returns `None` for [`SpecialUse::None`].
    #[must_use]
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Self::Drafts => Some("\\Drafts"),
            Self::Sent => Some("\\Sent"),
            Self::Trash => Some("\\Trash"),
            Self::Junk => Some("\\Junk"),
            Self::Archive => Some("\\Archive"),
            Self::Important => Some("\\Important"),
            Self::None => None,
        }
    }
}

/// One folder entry in the server-wide layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutFolder {
    /// Folder name as the user sees it (`Drafts`, `Sent Messages`, `Updates`).
    pub name: String,
    /// Special-use marker, or [`SpecialUse::None`] for category folders.
    pub special_use: SpecialUse,
    /// Whether new accounts automatically subscribe (visible in clients
    /// without manual subscribe). `false` for legacy aliases like
    /// `Sent Messages` that exist only so old clients don't lose mail.
    pub auto_subscribe: bool,
}

impl LayoutFolder {
    /// Construct a folder with auto-subscribe on. The common case.
    #[must_use]
    pub fn subscribed(name: impl Into<String>, special_use: SpecialUse) -> Self {
        Self {
            name: name.into(),
            special_use,
            auto_subscribe: true,
        }
    }

    /// Construct an unsubscribed alias folder (e.g., `Sent Messages` →
    /// `\Sent`, kept for legacy clients).
    #[must_use]
    pub fn alias(name: impl Into<String>, special_use: SpecialUse) -> Self {
        Self {
            name: name.into(),
            special_use,
            auto_subscribe: false,
        }
    }
}

/// The full server-wide layout.
///
/// SECURITY: Layout drift between deployment and repo has caused
/// classifier silent-failure before — keep the test suite as the canonical
/// guard against text-vs-typed divergence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxLayout {
    /// Ordered folder list. Order is preserved in the emitted config so
    /// reviewers reading a diff see stable output.
    pub folders: Vec<LayoutFolder>,
}

impl Default for MailboxLayout {
    /// The PlausiDen default layout — IMAP special-use folders +
    /// Gmail-style categories.
    ///
    /// BUG ASSUMPTION: Adding a category here without a matching
    /// [`crate::categories::CategoryRule`] produces a folder that mail
    /// never gets sorted into — confusing, but not broken. Removing one
    /// without removing the rule produces a Sieve script that fileinto's
    /// to a missing folder; Sieve's `:create` flag mitigates this.
    fn default() -> Self {
        use SpecialUse::{Archive, Drafts, Important, Junk, None as N, Sent, Trash};
        Self {
            folders: vec![
                LayoutFolder::subscribed("Drafts", Drafts),
                LayoutFolder::subscribed("Sent", Sent),
                LayoutFolder::alias("Sent Messages", Sent),
                LayoutFolder::subscribed("Junk", Junk),
                LayoutFolder::alias("Spam", Junk),
                LayoutFolder::subscribed("Trash", Trash),
                LayoutFolder::subscribed("Archive", Archive),
                LayoutFolder::subscribed("Important", Important),
                LayoutFolder::subscribed("Updates", N),
                LayoutFolder::subscribed("Receipts", N),
                LayoutFolder::subscribed("Social", N),
                LayoutFolder::subscribed("Promotions", N),
                LayoutFolder::subscribed("Forums", N),
            ],
        }
    }
}

impl MailboxLayout {
    /// Emit the Dovecot 2.4 `15-mailboxes.conf` body.
    #[must_use]
    pub fn to_dovecot_conf(&self) -> String {
        let mut out = String::from("namespace inbox {\n");
        for f in &self.folders {
            // Quote names that contain whitespace; Dovecot accepts both
            // bareword and quoted forms but quoting keeps diffs uniform.
            let name = if f.name.contains(char::is_whitespace) {
                format!("\"{}\"", f.name)
            } else {
                f.name.clone()
            };
            out.push_str(&format!("  mailbox {name} {{\n"));
            if f.auto_subscribe {
                out.push_str("    auto = subscribe\n");
            }
            if let Some(marker) = f.special_use.marker() {
                out.push_str(&format!("    special_use = {marker}\n"));
            }
            out.push_str("  }\n");
        }
        out.push_str("}\n");
        out
    }

    /// All category folders (no special-use marker, auto-subscribed).
    /// Used by the Sieve generator to validate that every rule's
    /// destination exists.
    #[must_use]
    pub fn category_folders(&self) -> Vec<&str> {
        self.folders
            .iter()
            .filter(|f| f.special_use == SpecialUse::None && f.auto_subscribe)
            .map(|f| f.name.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_includes_all_categories() {
        let layout = MailboxLayout::default();
        let cats = layout.category_folders();
        for expected in &["Updates", "Receipts", "Social", "Promotions", "Forums"] {
            assert!(cats.contains(expected), "missing category {expected}");
        }
    }

    #[test]
    fn dovecot_output_has_special_use_markers() {
        let conf = MailboxLayout::default().to_dovecot_conf();
        assert!(conf.contains("special_use = \\Drafts"));
        assert!(conf.contains("special_use = \\Sent"));
        assert!(conf.contains("special_use = \\Junk"));
        assert!(conf.contains("special_use = \\Trash"));
        assert!(conf.contains("special_use = \\Archive"));
        assert!(conf.contains("special_use = \\Important"));
    }

    #[test]
    fn alias_folders_are_not_auto_subscribed() {
        let conf = MailboxLayout::default().to_dovecot_conf();
        // `Sent Messages` exists for legacy clients but should not
        // auto-subscribe — duplicate Sent listings confuse users.
        let sm_block_start = conf.find("\"Sent Messages\"").expect("sent messages folder");
        let next_close = conf[sm_block_start..].find("}").expect("close brace");
        let block = &conf[sm_block_start..sm_block_start + next_close];
        assert!(!block.contains("auto = subscribe"), "alias auto-subscribed");
        assert!(block.contains("special_use = \\Sent"));
    }

    #[test]
    fn quoted_names_for_whitespace() {
        let conf = MailboxLayout::default().to_dovecot_conf();
        assert!(conf.contains("mailbox \"Sent Messages\""));
        assert!(conf.contains("mailbox Drafts"));
    }

    #[test]
    fn round_trip_serde() {
        let layout = MailboxLayout::default();
        let json = serde_json::to_string(&layout).unwrap();
        let back: MailboxLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout.folders.len(), back.folders.len());
    }
}
