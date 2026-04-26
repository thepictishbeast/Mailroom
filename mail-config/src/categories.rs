//! Gmail-style category rules — typed AST + Sieve emitter + in-process
//! evaluator.
//!
//! This module is the **canonical wire format** for category rules. The
//! same `CategoryRule` is:
//!   1. Evaluated server-side (compiled to Sieve and run by Pigeonhole at
//!      LMTP delivery).
//!   2. Evaluated client-side (Thundercrab runs [`Self::evaluate`] over
//!      cached message headers when sorting offline).
//!   3. Round-tripped through ManageSieve and the federated suggestion
//!      ledger.
//!
//! SECURITY: The evaluator only looks at headers and subject — never
//! body or full sender address — so no rule can leak message content
//! into a routing decision logged in the audit header.
//!
//! BUG ASSUMPTION: Sieve's `:contains` is case-insensitive per RFC 5228;
//! the in-process evaluator matches that.

use serde::{Deserialize, Serialize};

/// AST of a header-only match expression.
///
/// Keep variants serde-stable — this is the wire format consumed by
/// Thundercrab and the federated suggestion ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchExpr {
    /// Always true. Useful for the trailing default route.
    Always,
    /// Header `name` contains `substring` (case-insensitive).
    HeaderContains { header: String, substring: String },
    /// Header `name` is present (any value).
    HasHeader { header: String },
    /// `From:` address ends with one of the listed domains
    /// (e.g., `["@github.com", "@stripe.com"]`). Leading `@` required.
    FromDomainIn { domains: Vec<String> },
    /// `Subject:` contains any of the listed substrings (case-insensitive).
    SubjectContainsAny { needles: Vec<String> },
    /// All sub-expressions match.
    All { exprs: Vec<MatchExpr> },
    /// At least one sub-expression matches.
    Any { exprs: Vec<MatchExpr> },
    /// Sub-expression does not match.
    Not { expr: Box<MatchExpr> },
}

/// What to do when a rule fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Move the message to the named folder. Folder is created if missing
    /// (Sieve `:create` flag).
    FileInto { folder: String },
    /// Set an IMAP flag (e.g., `\Flagged`, `$Important`) without moving.
    SetFlag { flag: String },
    /// Multiple actions in order.
    Sequence { actions: Vec<Action> },
}

/// One category rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryRule {
    /// Stable identifier — used in audit headers and federated suggestions.
    /// Snake_case, no whitespace, e.g., `promotions_listunsub`.
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// Match condition.
    pub when: MatchExpr,
    /// Action(s) to take.
    pub action: Action,
    /// Higher score wins ordering; ties resolved by `id` for determinism.
    /// Suggested ranges: 100+ explicit user rules, 50–99 platform defaults,
    /// 1–49 federated suggestions.
    pub score: i32,
    /// If true, no further rules evaluate after this one matches.
    /// Server-side: emitted as Sieve `stop;`. Client-side: short-circuits
    /// the evaluator loop.
    pub stop_on_match: bool,
}

/// Audit record produced when a rule fires. The Sieve emitter writes one
/// `X-PlausiDen-Category` header per matched rule via the `editheader`
/// extension; clients can read these to explain "why is this here?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTag {
    pub rule_id: String,
    pub score: i32,
}

impl AuditTag {
    /// Format as a single header value.
    /// Format: `id=promotions_listunsub; score=80`
    #[must_use]
    pub fn to_header_value(&self) -> String {
        format!("id={}; score={}", self.rule_id, self.score)
    }
}

/// Sieve emission options.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SieveEmitOptions {
    /// Emit `X-PlausiDen-Category` headers via the `editheader` extension.
    /// Requires Pigeonhole to be configured with `sieve_extensions =
    /// +editheader` (see [`crate::dovecot::generate_sieve_runtime_conf`]).
    /// Default off — the deployed VPS does not currently enable it.
    pub audit_header: bool,
}

/// Collection of rules, evaluated in score order (highest first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRules {
    pub rules: Vec<CategoryRule>,
}

impl Default for CategoryRules {
    /// PlausiDen default rules — match the categories.sieve we deployed.
    fn default() -> Self {
        Self {
            rules: vec![
                rule_important(),
                rule_promotions_listunsub(),
                rule_promotions_senders(),
                rule_social_senders(),
                rule_forums_listid(),
                rule_forums_googlegroups(),
                rule_updates_senders(),
                rule_updates_subject_keywords(),
            ],
        }
    }
}

impl CategoryRules {
    /// Emit a complete Sieve script with default options (no audit
    /// header). Equivalent to [`Self::to_sieve_with`] with
    /// [`SieveEmitOptions::default`].
    #[must_use]
    pub fn to_sieve(&self) -> String {
        self.to_sieve_with(SieveEmitOptions::default())
    }

    /// Emit a complete Sieve script with explicit options.
    ///
    /// SECURITY: When `audit_header` is set, the emitted `addheader`
    /// includes the rule id and score only — never message content — so
    /// the audit trail is safe to expose to recipients and to the
    /// Thundercrab "why is this here?" UI.
    #[must_use]
    pub fn to_sieve_with(&self, opts: SieveEmitOptions) -> String {
        let mut out = String::new();
        out.push_str(
            "# AUTO-GENERATED by mail-config::categories. Do not hand-edit.\n\
             # Edit the typed CategoryRules and re-emit.\n\n",
        );
        let mut requires: Vec<&str> = vec!["fileinto", "mailbox", "imap4flags", "envelope"];
        if opts.audit_header {
            requires.push("editheader");
        }
        let req_list = requires
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("require [{req_list}];\n\n"));

        // Sort by score desc, then id asc, for deterministic output.
        let mut ordered: Vec<&CategoryRule> = self.rules.iter().collect();
        ordered.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));

        for r in ordered {
            out.push_str(&format!(
                "# rule {id} (score={score})\n",
                id = r.id,
                score = r.score
            ));
            out.push_str(&format!("if {} {{\n", emit_match(&r.when)));
            if opts.audit_header {
                out.push_str(&format!(
                    "    addheader \"X-PlausiDen-Category\" \"{}\";\n",
                    AuditTag {
                        rule_id: r.id.clone(),
                        score: r.score,
                    }
                    .to_header_value()
                ));
            }
            emit_action(&r.action, &mut out, "    ");
            if r.stop_on_match {
                out.push_str("    stop;\n");
            }
            out.push_str("}\n\n");
        }
        out
    }

    /// Evaluate rules in-process against parsed headers. Returns the
    /// matched rules in firing order (score desc, id asc), stopping at
    /// the first `stop_on_match` rule.
    ///
    /// `from_address` is the lowercased `From:` address (e.g.,
    /// `noreply@github.com`); the evaluator extracts the domain itself.
    /// Pass an empty string if unknown.
    #[must_use]
    pub fn evaluate(&self, ctx: &MessageContext) -> Vec<&CategoryRule> {
        let mut ordered: Vec<&CategoryRule> = self.rules.iter().collect();
        ordered.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));

        let mut hits = Vec::new();
        for r in ordered {
            if eval_match(&r.when, ctx) {
                hits.push(r);
                if r.stop_on_match {
                    break;
                }
            }
        }
        hits
    }
}

/// Read-only view of a message used by the in-process evaluator.
///
/// Headers are stored lowercased on the key side; values keep their
/// original case (we case-insensitive-match on substrings).
pub struct MessageContext<'a> {
    /// Lowercased header name → original-case header value, in arrival
    /// order. Multiple values per header are stored as separate entries.
    pub headers: &'a [(String, String)],
    /// `From:` address (full, lowercased), e.g., `noreply@github.com`.
    /// Empty if not parseable.
    pub from_address: &'a str,
    /// `Subject:` value (original case).
    pub subject: &'a str,
}

impl<'a> MessageContext<'a> {
    fn header_values<'b>(
        &'b self,
        name_lower: &'b str,
    ) -> impl Iterator<Item = &'b str> + 'b {
        self.headers
            .iter()
            .filter(move |(k, _)| k == name_lower)
            .map(|(_, v)| v.as_str())
    }

    fn has_header(&self, name_lower: &str) -> bool {
        self.header_values(name_lower).next().is_some()
    }

    fn extract_from_domain(&self) -> Option<String> {
        let at = self.from_address.rfind('@')?;
        Some(self.from_address[at..].to_string())
    }
}

fn eval_match(expr: &MatchExpr, ctx: &MessageContext) -> bool {
    match expr {
        MatchExpr::Always => true,
        MatchExpr::HeaderContains { header, substring } => {
            let needle = substring.to_lowercase();
            ctx.header_values(&header.to_lowercase())
                .any(|v| v.to_lowercase().contains(&needle))
        }
        MatchExpr::HasHeader { header } => ctx.has_header(&header.to_lowercase()),
        MatchExpr::FromDomainIn { domains } => {
            let Some(dom) = ctx.extract_from_domain() else {
                return false;
            };
            domains.iter().any(|d| d.eq_ignore_ascii_case(&dom))
        }
        MatchExpr::SubjectContainsAny { needles } => {
            let s = ctx.subject.to_lowercase();
            needles.iter().any(|n| s.contains(&n.to_lowercase()))
        }
        MatchExpr::All { exprs } => exprs.iter().all(|e| eval_match(e, ctx)),
        MatchExpr::Any { exprs } => exprs.iter().any(|e| eval_match(e, ctx)),
        MatchExpr::Not { expr } => !eval_match(expr, ctx),
    }
}

fn emit_match(expr: &MatchExpr) -> String {
    match expr {
        MatchExpr::Always => "true".into(),
        MatchExpr::HeaderContains { header, substring } => {
            format!(
                "header :contains \"{}\" \"{}\"",
                sieve_escape(header),
                sieve_escape(substring)
            )
        }
        MatchExpr::HasHeader { header } => {
            format!("exists \"{}\"", sieve_escape(header))
        }
        MatchExpr::FromDomainIn { domains } => {
            let list = domains
                .iter()
                .map(|d| format!("\"*{}\"", sieve_escape(d)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("address :matches \"From\" [{list}]")
        }
        MatchExpr::SubjectContainsAny { needles } => {
            let list = needles
                .iter()
                .map(|n| format!("\"{}\"", sieve_escape(n)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("header :contains \"Subject\" [{list}]")
        }
        MatchExpr::All { exprs } => {
            let list = exprs.iter().map(emit_match).collect::<Vec<_>>().join(", ");
            format!("allof ({list})")
        }
        MatchExpr::Any { exprs } => {
            let list = exprs.iter().map(emit_match).collect::<Vec<_>>().join(", ");
            format!("anyof ({list})")
        }
        MatchExpr::Not { expr } => format!("not {}", emit_match(expr)),
    }
}

fn emit_action(action: &Action, out: &mut String, indent: &str) {
    match action {
        Action::FileInto { folder } => {
            out.push_str(&format!(
                "{indent}fileinto :create \"{}\";\n",
                sieve_escape(folder)
            ));
        }
        Action::SetFlag { flag } => {
            out.push_str(&format!(
                "{indent}addflag \"{}\";\n",
                sieve_escape(flag)
            ));
        }
        Action::Sequence { actions } => {
            for a in actions {
                emit_action(a, out, indent);
            }
        }
    }
}

/// Escape a string for safe inclusion in a Sieve double-quoted literal.
/// Sieve quoted strings are MIME-style: backslash and double-quote need
/// escaping; other bytes pass through.
fn sieve_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            _ => out.push(c),
        }
    }
    out
}

// --- default rules ---------------------------------------------------

fn rule_important() -> CategoryRule {
    CategoryRule {
        id: "important_priority".into(),
        display_name: "Important (X-Priority 1/2)".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HeaderContains {
                    header: "X-Priority".into(),
                    substring: "1".into(),
                },
                MatchExpr::HeaderContains {
                    header: "X-Priority".into(),
                    substring: "2".into(),
                },
            ],
        },
        action: Action::SetFlag {
            flag: "\\Flagged".into(),
        },
        score: 90,
        // Important leaves the message in INBOX, so we DO want subsequent
        // rules to consider it (it might also be Updates etc. — flagging
        // doesn't preclude further routing).
        stop_on_match: false,
    }
}

fn rule_promotions_listunsub() -> CategoryRule {
    CategoryRule {
        id: "promotions_listunsub".into(),
        display_name: "Promotions — List-Unsubscribe present".into(),
        when: MatchExpr::HasHeader {
            header: "List-Unsubscribe".into(),
        },
        action: Action::FileInto {
            folder: "Promotions".into(),
        },
        score: 80,
        stop_on_match: true,
    }
}

fn rule_promotions_senders() -> CategoryRule {
    CategoryRule {
        id: "promotions_senders".into(),
        display_name: "Promotions — known marketing platforms".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                "@mailchimp.com".into(),
                "@sendgrid.net".into(),
                "@mailgun.org".into(),
                "@constantcontact.com".into(),
                "@hubspot.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Promotions".into(),
        },
        score: 70,
        stop_on_match: true,
    }
}

fn rule_social_senders() -> CategoryRule {
    CategoryRule {
        id: "social_senders".into(),
        display_name: "Social — major platforms".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                "@facebook.com".into(),
                "@facebookmail.com".into(),
                "@twitter.com".into(),
                "@x.com".into(),
                "@linkedin.com".into(),
                "@discord.com".into(),
                "@discordapp.com".into(),
                "@instagram.com".into(),
                "@reddit.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Social".into(),
        },
        score: 70,
        stop_on_match: true,
    }
}

fn rule_forums_listid() -> CategoryRule {
    CategoryRule {
        id: "forums_listid".into(),
        display_name: "Forums — List-Id present".into(),
        when: MatchExpr::HasHeader {
            header: "List-Id".into(),
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        score: 75,
        stop_on_match: true,
    }
}

fn rule_forums_googlegroups() -> CategoryRule {
    CategoryRule {
        id: "forums_googlegroups".into(),
        display_name: "Forums — Google Groups".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec!["@googlegroups.com".into()],
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        score: 70,
        stop_on_match: true,
    }
}

fn rule_updates_senders() -> CategoryRule {
    CategoryRule {
        id: "updates_senders".into(),
        display_name: "Updates — service providers".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                "@github.com".into(),
                "@gitlab.com".into(),
                "@stripe.com".into(),
                "@paypal.com".into(),
                "@amazon.com".into(),
                "@amazonaws.com".into(),
                "@apple.com".into(),
                "@dropbox.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Updates".into(),
        },
        score: 60,
        stop_on_match: true,
    }
}

fn rule_updates_subject_keywords() -> CategoryRule {
    CategoryRule {
        id: "updates_subject_keywords".into(),
        display_name: "Updates — receipt/invoice/OTP keywords".into(),
        when: MatchExpr::SubjectContainsAny {
            needles: vec![
                "receipt".into(),
                "invoice".into(),
                "your order".into(),
                "verification code".into(),
                "one-time".into(),
                "otp".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Updates".into(),
        },
        score: 50,
        stop_on_match: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        headers: &'a [(String, String)],
        from: &'a str,
        subj: &'a str,
    ) -> MessageContext<'a> {
        MessageContext {
            headers,
            from_address: from,
            subject: subj,
        }
    }

    #[test]
    fn github_email_routes_to_updates() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "noreply@github.com", "PR opened"));
        assert!(!hits.is_empty(), "no rule fired");
        let names: Vec<_> = hits.iter().map(|r| r.id.as_str()).collect();
        assert!(names.contains(&"updates_senders"), "got {names:?}");
    }

    #[test]
    fn list_unsubscribe_overrides_subject_keywords() {
        // Mailchimp "your order" promo — List-Unsubscribe wins over subject
        // keywords because score 80 > 50.
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@x>".into())];
        let hits = rules.evaluate(&ctx(&h, "noreply@mailchimp.com", "Your order is confirmed"));
        assert_eq!(hits.first().unwrap().id, "promotions_listunsub");
    }

    #[test]
    fn googlegroups_routes_to_forums() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "thread@googlegroups.com", "Re: design"));
        assert_eq!(
            hits.first().unwrap().action,
            Action::FileInto {
                folder: "Forums".into(),
            }
        );
    }

    #[test]
    fn linkedin_routes_to_social() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "invitations@linkedin.com", "John wants to connect"));
        assert_eq!(hits.first().unwrap().id, "social_senders");
    }

    #[test]
    fn x_priority_flags_but_does_not_stop() {
        // High-priority GitHub email should be flagged AND filed to Updates.
        let rules = CategoryRules::default();
        let h = vec![("x-priority".into(), "1".into())];
        let hits = rules.evaluate(&ctx(&h, "alerts@github.com", "Build failure"));
        let ids: Vec<_> = hits.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"important_priority"));
        assert!(ids.contains(&"updates_senders"), "got {ids:?}");
    }

    #[test]
    fn unmatched_message_yields_empty() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alice@example.org", "lunch tomorrow?"));
        assert!(hits.is_empty(), "got {hits:?}");
    }

    #[test]
    fn default_sieve_output_does_not_require_editheader() {
        // Default emission must compile on stock Pigeonhole — i.e., not
        // require an opt-in extension.
        let s = CategoryRules::default().to_sieve();
        assert!(s.contains("require ["));
        for ext in &["fileinto", "mailbox", "imap4flags"] {
            assert!(s.contains(ext), "missing extension {ext}");
        }
        assert!(!s.contains("editheader"), "default must not require editheader");
        assert!(!s.contains("addheader"), "default must not emit addheader");
    }

    #[test]
    fn audit_header_opt_in_requires_editheader() {
        let s = CategoryRules::default().to_sieve_with(SieveEmitOptions {
            audit_header: true,
        });
        assert!(s.contains("editheader"));
        assert!(s.contains("addheader \"X-PlausiDen-Category\""));
        assert!(s.contains("id=promotions_listunsub"));
    }

    #[test]
    fn sieve_output_orders_by_score_desc() {
        let s = CategoryRules::default().to_sieve();
        let imp_pos = s.find("important_priority").expect("important present");
        let promo_pos = s.find("promotions_listunsub").expect("promo present");
        let updates_pos = s.find("updates_senders").expect("updates present");
        assert!(imp_pos < promo_pos, "score 90 should precede score 80");
        assert!(promo_pos < updates_pos, "score 80 should precede score 60");
    }

    #[test]
    fn round_trip_serde() {
        let rules = CategoryRules::default();
        let json = serde_json::to_string(&rules).unwrap();
        let back: CategoryRules = serde_json::from_str(&json).unwrap();
        assert_eq!(rules.rules.len(), back.rules.len());
        // Spot-check the AST round-tripped.
        let original_first = &rules.rules[0];
        let back_first = back
            .rules
            .iter()
            .find(|r| r.id == original_first.id)
            .unwrap();
        assert_eq!(original_first.when, back_first.when);
        assert_eq!(original_first.action, back_first.action);
    }

    #[test]
    fn sieve_escape_handles_quotes_and_backslashes() {
        assert_eq!(sieve_escape(r#"a"b"#), r#"a\"b"#);
        assert_eq!(sieve_escape(r"a\b"), r"a\\b");
        assert_eq!(sieve_escape("plain"), "plain");
    }
}
