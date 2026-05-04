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
                rule_internal_source(),
                rule_2fa_inbox(),
                rule_important_security(),
                rule_important_signature(),
                rule_important_billing_critical(),
                rule_receipts(),
                rule_travel(),
                rule_banking(),
                rule_important(),
                rule_promotions_listunsub(),
                rule_promotions_senders(),
                rule_social_senders(),
                rule_forums_listid(),
                rule_forums_listid_substring(),
                rule_forums_discourse_subdomain(),
                rule_forums_dev_communities(),
                rule_forums_googlegroups(),
                rule_updates_senders(),
                rule_updates_subject_keywords(),
                rule_updates_noreply_sender(),
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

fn rule_internal_source() -> CategoryRule {
    CategoryRule {
        id: "internal_source".into(),
        display_name: "Internal infrastructure → INBOX".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::FromDomainIn {
                    domains: vec![
                        // PlausiDen-owned mail (replies, alerts, reports).
                        "@plausiden.com".into(),
                        "@plausiden.internal".into(),
                        // Vultr default hostnames — varies per instance
                        // image (vultr.guest, vultr.vultr, vultr.local
                        // are all observed in the wild). Sub-VPS
                        // operational alerts go through these before
                        // they're given a real sender identity.
                        "@vultr.guest".into(),
                        "@vultr.vultr".into(),
                        "@vultr.local".into(),
                        "@web-01.plausiden.internal".into(),
                    ],
                },
                // Belt-and-braces: any From containing ".vultr." catches
                // future hostname variants we haven't seen yet (Vultr
                // sometimes spawns instances with hostnames like
                // `web-02.plausiden.vultr.guest` or similar).
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: ".vultr.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@vultr".into(),
                },
            ],
        },
        // INBOX is the implicit default if no rule fires; explicitly
        // filing here lets `stop_on_match` short-circuit lower-priority
        // rules (especially List-Unsubscribe) that would otherwise
        // misroute these direct messages.
        action: Action::FileInto {
            folder: "INBOX".into(),
        },
        // Score 100 — beats `important_priority` (90) so any direct
        // message from internal infrastructure sits in INBOX without
        // being flagged. Adding a flag is the user's choice via the
        // mail client.
        score: 100,
        stop_on_match: true,
    }
}

fn rule_2fa_inbox() -> CategoryRule {
    CategoryRule {
        id: "2fa_inbox".into(),
        display_name: "Auth codes (2FA / verification / sign-in) → INBOX + flagged".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::SubjectContainsAny {
                    needles: vec![
                        "2FA".into(),
                        "verification code".into(),
                        "verify your email".into(),
                        "sign-in code".into(),
                        "sign in code".into(),
                        "security code".into(),
                        "one-time code".into(),
                        "auth code".into(),
                        "login code".into(),
                        "passcode".into(),
                        "your code is".into(),
                        " code is ".into(),
                        "OTP".into(),
                    ],
                },
                MatchExpr::HeaderContains {
                    header: "Subject".into(),
                    substring: "is your code".into(),
                },
            ],
        },
        // Two-step action: flag for visibility in the IMAP client, then
        // file to INBOX explicitly so `stop_on_match` can short-circuit
        // the lower-priority Updates / List-Unsubscribe rules. Without
        // this, "Sacred Vote Admin 2FA Code" matches
        // updates_subject_keywords and gets buried in Updates.
        action: Action::Sequence {
            actions: vec![
                Action::SetFlag {
                    flag: "\\Flagged".into(),
                },
                Action::FileInto {
                    folder: "INBOX".into(),
                },
            ],
        },
        // Score 95 — runs after internal_source (100) and before
        // important_priority (90). Authentication codes are time-sensitive,
        // so we want them at the top of INBOX regardless of bulk-mail
        // markers.
        score: 95,
        stop_on_match: true,
    }
}

/// Banking → Banking folder. Bank statements, credit-card alerts,
/// brokerage notices, retirement-account statements. These are
/// reference material reviewed monthly (statements) or at tax time
/// (1099s, donation receipts), and they're worth pulling out of the
/// generic Updates bucket where they currently go via the
/// updates_senders domain list.
///
/// Score 86 (same band as travel): below receipts (87, "your
/// receipt from Chase" → financial record first) and below
/// important_billing_critical (89, "card declined" → page).
///
/// IMPORTANT: bank security alerts ("new sign-in detected") still
/// route to Important via important_security (92), since "did
/// someone log into my bank account?" is page-worthy.
fn rule_banking() -> CategoryRule {
    CategoryRule {
        id: "banking".into(),
        display_name: "Banking — bank / credit card / brokerage senders".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                // Major US retail banks.
                "@chase.com".into(),
                "@bankofamerica.com".into(),
                "@wellsfargo.com".into(),
                "@citi.com".into(),
                "@citibank.com".into(),
                "@usbank.com".into(),
                "@pnc.com".into(),
                "@truist.com".into(),
                "@capitalone.com".into(),
                "@allyinvest.com".into(),
                "@ally.com".into(),
                "@discover.com".into(),
                "@hsbc.com".into(),
                "@tdbank.com".into(),
                // Credit-card issuers (often distinct from the parent bank).
                "@americanexpress.com".into(),
                "@amex.com".into(),
                "@citicards.com".into(),
                "@chase.email.chase.com".into(),
                // Brokerage / investment.
                "@schwab.com".into(),
                "@fidelity.com".into(),
                "@vanguard.com".into(),
                "@etrade.com".into(),
                "@tdameritrade.com".into(),
                "@robinhood.com".into(),
                "@coinbase.com".into(),
                // Tax / accounting.
                "@intuit.com".into(),
                "@turbotax.com".into(),
                "@quickbooks.com".into(),
                "@hrblock.com".into(),
                // Money-movement services.
                "@venmo.com".into(),
                "@zelle.com".into(),
                "@wise.com".into(),
                "@transferwise.com".into(),
                "@cash.app".into(),
                "@squareup.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Banking".into(),
        },
        // 86 mirrors travel — both are reference-document senders.
        // receipts (87) wins for "your receipt" subjects so that a
        // Chase/Stripe/etc. monthly receipt files as a receipt;
        // banking gets the rest (statements, alerts, notices).
        score: 86,
        stop_on_match: true,
    }
}

/// Travel → Travel folder. Airline / hotel / rental-car / travel-
/// agency mail. These are reference documents (boarding passes,
/// reservation confirmations) the recipient typically wants
/// findable as a group, not interleaved with GitHub PRs and Stripe
/// receipts in a generic Updates bucket.
///
/// Score 86 sits between receipts (87) and updates_senders (85).
/// So a "Your receipt from United" matches receipts first (87),
/// while a "Your flight to LAX on May 12" hits travel (86) — both
/// are sensible because the receipts rule is for the financial
/// record, the travel rule is for the itinerary.
///
/// IMPORTANT-class travel mail (cancellation, schedule change,
/// "action required") still beats this via important_billing_critical
/// (89, "action required" pattern) — flight delays and cancellations
/// are time-sensitive enough to warrant pager-level routing.
fn rule_travel() -> CategoryRule {
    CategoryRule {
        id: "travel".into(),
        display_name: "Travel — airline / hotel / rental car / travel agency senders".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                // US major + low-cost airlines.
                "@united.com".into(),
                "@aa.com".into(),
                "@delta.com".into(),
                "@southwest.com".into(),
                "@jetblue.com".into(),
                "@alaskaair.com".into(),
                "@spirit.com".into(),
                "@frontierairlines.com".into(),
                "@hawaiianair.com".into(),
                // International majors that route US-based travelers.
                "@britishairways.com".into(),
                "@aircanada.ca".into(),
                "@aircanada.com".into(),
                "@lufthansa.com".into(),
                "@airfrance.com".into(),
                "@klm.com".into(),
                "@emirates.com".into(),
                "@qatarairways.com".into(),
                "@singaporeair.com".into(),
                "@cathaypacific.com".into(),
                "@ana.co.jp".into(),
                "@jal.com".into(),
                // Hotels.
                "@marriott.com".into(),
                "@hilton.com".into(),
                "@hyatt.com".into(),
                "@ihg.com".into(),
                "@accorhotels.com".into(),
                "@choicehotels.com".into(),
                "@bestwestern.com".into(),
                "@wyndhamhotels.com".into(),
                "@radisson.com".into(),
                // Booking platforms.
                "@booking.com".into(),
                "@expedia.com".into(),
                "@hotels.com".into(),
                "@kayak.com".into(),
                "@priceline.com".into(),
                "@orbitz.com".into(),
                "@travelocity.com".into(),
                "@tripadvisor.com".into(),
                "@trivago.com".into(),
                "@agoda.com".into(),
                "@airbnb.com".into(),
                "@vrbo.com".into(),
                // Rental cars.
                "@hertz.com".into(),
                "@enterprise.com".into(),
                "@avis.com".into(),
                "@budget.com".into(),
                "@nationalcar.com".into(),
                "@alamo.com".into(),
                "@sixt.com".into(),
                // Train + ground transport.
                "@amtrak.com".into(),
                "@eurostar.com".into(),
                "@trainline.com".into(),
                "@greyhound.com".into(),
                "@megabus.com".into(),
                // Travel-related cards / loyalty programs that send
                // booking confirmations and itinerary updates.
                "@tripit.com".into(),
                "@triplog.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Travel".into(),
        },
        score: 86,
        stop_on_match: true,
    }
}

/// Receipts → Receipts folder. Transactional payment confirmations
/// (Stripe charges, PayPal receipts, bank notifications, on-demand
/// service receipts). These are valuable to keep but they swamp the
/// Updates folder when they're mixed with operational alerts.
///
/// Score 87 sits BETWEEN important_billing_critical (89, "payment
/// FAILED" → still Important) and updates_senders (85, generic
/// transactional). So:
///   - "Payment received from Stripe" → Receipts (matches here)
///   - "Payment FAILED at Stripe"     → Important (89 wins, paged)
///   - "GitHub PR opened"             → Updates (85, no receipt match)
fn rule_receipts() -> CategoryRule {
    CategoryRule {
        id: "receipts".into(),
        display_name: "Receipts — payment confirmations + order receipts".into(),
        when: MatchExpr::SubjectContainsAny {
            needles: vec![
                // Payment receipts.
                "your receipt".into(),
                "payment received".into(),
                "payment from".into(),
                "you paid".into(),
                "you've been paid".into(),
                "thanks for your payment".into(),
                "thank you for your payment".into(),
                "we received your payment".into(),
                "payment confirmed".into(),
                "payment successful".into(),
                // Order / shipping receipts.
                "your order".into(),
                "order received".into(),
                "order confirmed".into(),
                "order confirmation".into(),
                "thanks for your order".into(),
                "thank you for your order".into(),
                "shipped".into(),
                "delivery confirmation".into(),
                // Subscription / renewal receipts (success only — failures
                // hit important_billing_critical at score 89).
                "subscription renewed".into(),
                "renewal confirmation".into(),
                "your invoice".into(),
                "invoice receipt".into(),
                "donation receipt".into(),
                // Generic transactional receipts.
                "receipt for".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Receipts".into(),
        },
        // 87: above updates_senders (85) so Stripe/PayPal/etc. transactional
        // mail with receipt-shaped subjects routes here, but below
        // important_billing_critical (89) so payment-FAILURE alerts still
        // page to Important.
        score: 87,
        stop_on_match: true,
    }
}

/// Security alerts → Important. New-device sign-ins, password changes,
/// suspicious activity warnings, account-lockout notices. Score 92 sits
/// above `important_priority` (90) so we file *into* Important rather
/// than just flagging.
///
/// BUG ASSUMPTION: marketing mail occasionally borrows this language
/// ("Did you mean to sign in?"). Subject keywords are paired with
/// generic auth language so a campaign won't trip a single keyword.
fn rule_important_security() -> CategoryRule {
    CategoryRule {
        id: "important_security".into(),
        display_name: "Important — security alerts (sign-in, password, suspicious activity)".into(),
        when: MatchExpr::SubjectContainsAny {
            needles: vec![
                "new sign-in".into(),
                "new sign in".into(),
                "new device".into(),
                "new login".into(),
                "suspicious sign-in".into(),
                "suspicious activity".into(),
                "unusual sign-in".into(),
                "unusual activity".into(),
                "we noticed".into(),
                "security alert".into(),
                "password was changed".into(),
                "password changed".into(),
                "password reset".into(),
                "account locked".into(),
                "account suspended".into(),
                "compromised".into(),
                "data breach".into(),
                "have i been pwned".into(),
                "haveibeenpwned".into(),
            ],
        },
        action: Action::Sequence {
            actions: vec![
                Action::SetFlag {
                    flag: "\\Flagged".into(),
                },
                Action::FileInto {
                    folder: "Important".into(),
                },
            ],
        },
        score: 92,
        stop_on_match: true,
    }
}

/// Document-signature requests → Important. DocuSign / HelloSign /
/// Adobe Sign / Dropbox Sign envelopes plus generic "please sign"
/// patterns. These are time-sensitive and shouldn't be buried in
/// Updates next to receipts.
fn rule_important_signature() -> CategoryRule {
    CategoryRule {
        id: "important_signature".into(),
        display_name: "Important — signature requests (DocuSign / HelloSign / Adobe Sign)".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::FromDomainIn {
                    domains: vec![
                        "@docusign.com".into(),
                        "@docusign.net".into(),
                        "@hellosign.com".into(),
                        "@dropboxsign.com".into(),
                        "@adobesign.com".into(),
                        "@echosign.com".into(),
                        "@pandadoc.com".into(),
                        "@signnow.com".into(),
                    ],
                },
                MatchExpr::SubjectContainsAny {
                    needles: vec![
                        "please sign".into(),
                        "signature requested".into(),
                        "request for signature".into(),
                        "complete with docusign".into(),
                        "ready to sign".into(),
                        "agreement to sign".into(),
                        "contract to sign".into(),
                        "nda".into(),
                    ],
                },
            ],
        },
        action: Action::Sequence {
            actions: vec![
                Action::SetFlag {
                    flag: "\\Flagged".into(),
                },
                Action::FileInto {
                    folder: "Important".into(),
                },
            ],
        },
        score: 91,
        stop_on_match: true,
    }
}

/// Billing-critical → Important. Failed payments, expiring domains,
/// final notices. Sits above generic transactional Updates so a
/// "Your domain expires in 7 days" doesn't get lost next to delivery
/// confirmations.
///
/// BUG ASSUMPTION: pure receipts ("invoice for $X") still land in
/// Updates — only failure / expiry / final-notice language hits here.
fn rule_important_billing_critical() -> CategoryRule {
    CategoryRule {
        id: "important_billing_critical".into(),
        display_name: "Important — billing failure / expiry / final notice".into(),
        when: MatchExpr::SubjectContainsAny {
            needles: vec![
                "payment failed".into(),
                "card declined".into(),
                "payment declined".into(),
                "could not charge".into(),
                "unable to process".into(),
                "subscription canceled".into(),
                "subscription cancelled".into(),
                "subscription expir".into(),
                "domain expir".into(),
                "renewal failed".into(),
                "final notice".into(),
                "past due".into(),
                "overdue".into(),
                "action required".into(),
                "trial ending".into(),
                "trial expir".into(),
            ],
        },
        action: Action::Sequence {
            actions: vec![
                Action::SetFlag {
                    flag: "\\Flagged".into(),
                },
                Action::FileInto {
                    folder: "Important".into(),
                },
            ],
        },
        score: 89,
        stop_on_match: true,
    }
}

fn rule_important() -> CategoryRule {
    CategoryRule {
        id: "important_priority".into(),
        display_name: "Important (X-Priority high / Importance high / Outlook attach hint)".into(),
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
                MatchExpr::HeaderContains {
                    header: "Importance".into(),
                    substring: "High".into(),
                },
                MatchExpr::HeaderContains {
                    header: "X-MS-Has-Attach".into(),
                    substring: "yes".into(),
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
        // ID kept stable for federated-ledger continuity; the rule body
        // now matches multiple bulk-marketing markers, not just
        // List-Unsubscribe.
        id: "promotions_listunsub".into(),
        display_name: "Promotions — bulk-mail markers (List-Unsubscribe / X-Mailer / X-Campaign / Precedence)".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HasHeader {
                    header: "List-Unsubscribe".into(),
                },
                MatchExpr::HasHeader {
                    header: "X-Mailer".into(),
                },
                MatchExpr::HasHeader {
                    header: "X-Campaign".into(),
                },
                MatchExpr::HasHeader {
                    header: "X-Mailgun-Tag".into(),
                },
                MatchExpr::HeaderContains {
                    header: "Precedence".into(),
                    substring: "bulk".into(),
                },
                MatchExpr::HeaderContains {
                    header: "Precedence".into(),
                    substring: "junk".into(),
                },
                MatchExpr::HeaderContains {
                    header: "Precedence".into(),
                    substring: "list".into(),
                },
            ],
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
                "@sparkpostmail.com".into(),
                "@amazonses.com".into(),
                "@klaviyomail.com".into(),
                "@rsgsv.net".into(),
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
                // Facebook family.
                "@facebook.com".into(),
                "@facebookmail.com".into(),
                "@mail.facebook.com".into(),
                "@messenger.com".into(),
                "@instagram.com".into(),
                "@mail.instagram.com".into(),
                // Twitter / X.
                "@twitter.com".into(),
                "@x.com".into(),
                // LinkedIn.
                "@linkedin.com".into(),
                "@linkedinmail.com".into(),
                // Discord.
                "@discord.com".into(),
                "@discordapp.com".into(),
                // Slack.
                "@slack.com".into(),
                // Other social platforms.
                "@tiktok.com".into(),
                "@youtube.com".into(),
                "@reddit.com".into(),
                "@redditmail.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Social".into(),
        },
        // Score 85 — same trick as `updates_senders`: known social
        // platforms always include a List-Unsubscribe header, so we
        // need to beat `promotions_listunsub` (80) to keep "Tanya
        // tagged you on Instagram" mail in Social, not Promotions.
        score: 85,
        stop_on_match: true,
    }
}

fn rule_forums_listid() -> CategoryRule {
    CategoryRule {
        id: "forums_listid".into(),
        display_name: "Forums — mailing-list headers (List-Id / Mailing-List / List-Post)".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HasHeader {
                    header: "List-Id".into(),
                },
                MatchExpr::HasHeader {
                    header: "Mailing-List".into(),
                },
                MatchExpr::HasHeader {
                    header: "List-Post".into(),
                },
            ],
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        score: 75,
        stop_on_match: true,
    }
}

/// Forums — list-server identity hints in List-Id (when present).
/// `forums_listid` only requires the header to exist. Real-world
/// List-Id values almost always contain "list", "users", "discuss",
/// "forum", or "discourse" — but the bare `exists` check already
/// catches that case. This rule is here for completeness so that
/// senders advertising their list ID via a non-standard header
/// (e.g., `X-List-Id`) still route correctly.
fn rule_forums_listid_substring() -> CategoryRule {
    CategoryRule {
        id: "forums_listid_substring".into(),
        display_name: "Forums — list-id substring on alternate headers".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HeaderContains {
                    header: "X-List-Id".into(),
                    substring: "list".into(),
                },
                MatchExpr::HeaderContains {
                    header: "X-Mailing-List".into(),
                    substring: "list".into(),
                },
                MatchExpr::HeaderContains {
                    header: "X-Loop".into(),
                    substring: "list".into(),
                },
                MatchExpr::HeaderContains {
                    header: "Sender".into(),
                    substring: "owner-".into(),
                },
                MatchExpr::HeaderContains {
                    header: "Sender".into(),
                    substring: "-bounces@".into(),
                },
            ],
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        score: 74,
        stop_on_match: true,
    }
}

/// Forums — Discourse / phpBB / Vanilla / forum subdomain senders.
/// Discourse-style forums use `notifications@<host>` and the
/// host typically starts with `discourse.` or `forum.` or
/// `community.` Catch the From substring so we don't have to
/// enumerate hostnames.
fn rule_forums_discourse_subdomain() -> CategoryRule {
    CategoryRule {
        id: "forums_discourse_subdomain".into(),
        display_name: "Forums — Discourse / forum / community subdomain senders".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@discourse.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@forum.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@forums.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@community.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "@boards.".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "noreply@discourse".into(),
                },
            ],
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        score: 72,
        stop_on_match: true,
    }
}

/// Forums — developer Q&A and community platforms. These look like
/// service providers (so `updates_senders` would catch them) but
/// they're discussion threads — Forums is the right home.
fn rule_forums_dev_communities() -> CategoryRule {
    CategoryRule {
        id: "forums_dev_communities".into(),
        display_name: "Forums — developer Q&A and community platforms".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                "@stackexchange.com".into(),
                "@stackoverflow.email".into(),
                "@stackoverflow.com".into(),
                "@users.rust-lang.org".into(),
                "@discuss.kotlinlang.org".into(),
                "@meta.discourse.org".into(),
                "@discourse.org".into(),
                "@quora.com".into(),
                "@medium.com".into(),
                "@substack.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Forums".into(),
        },
        // Score 86 beats updates_senders (85) and promotions_listunsub
        // (80) — these platforms are forums, even though they look
        // service-shaped at the SMTP layer.
        score: 86,
        stop_on_match: true,
    }
}

fn rule_forums_googlegroups() -> CategoryRule {
    CategoryRule {
        id: "forums_googlegroups".into(),
        display_name: "Forums — list-server domains (Google Groups, groups.io, Yahoo Groups)".into(),
        when: MatchExpr::FromDomainIn {
            domains: vec![
                "@googlegroups.com".into(),
                "@groups.io".into(),
                "@yahoogroups.com".into(),
            ],
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
                // Developer infrastructure + code hosting.
                "@github.com".into(),
                "@noreply.github.com".into(),
                "@gitlab.com".into(),
                "@bitbucket.org".into(),
                // Cloud / SaaS infrastructure.
                "@amazon.com".into(),
                "@amazonaws.com".into(),
                "@apple.com".into(),
                "@icloud.com".into(),
                "@google.com".into(),
                "@microsoft.com".into(),
                "@dropbox.com".into(),
                "@cloudflare.com".into(),
                "@vercel.com".into(),
                "@netlify.com".into(),
                "@docker.com".into(),
                "@npmjs.com".into(),
                "@pypi.org".into(),
                "@vultr.com".into(),
                "@digitalocean.com".into(),
                "@letsencrypt.org".into(),
                // Domain registrars.
                "@namesilo.com".into(),
                "@godaddy.com".into(),
                "@networksolutions.com".into(),
                // Payment / commerce. (Banking-specific senders moved to
                // the dedicated banking rule at score 86 so they file
                // into Banking instead of Updates.)
                "@stripe.com".into(),
                "@paypal.com".into(),
                "@shopify.com".into(),
                // On-demand services with order/receipt confirmations.
                "@doordash.com".into(),
                "@uber.com".into(),
                "@lyft.com".into(),
            ],
        },
        action: Action::FileInto {
            folder: "Updates".into(),
        },
        // Score 85 beats `promotions_listunsub` (80) so transactional /
        // operational notifications from known service providers go to
        // Updates even when they include a List-Unsubscribe header
        // (which most modern senders do for compliance).
        // BUG ASSUMPTION: This rule fires before list-unsubscribe routing.
        // Re-rank with care — moving service-provider mail into Promotions
        // is a worse default than the noise of an unrelated newsletter
        // landing in Updates.
        score: 85,
        stop_on_match: true,
    }
}

fn rule_updates_subject_keywords() -> CategoryRule {
    CategoryRule {
        id: "updates_subject_keywords".into(),
        display_name: "Updates — transactional subject keywords".into(),
        when: MatchExpr::SubjectContainsAny {
            needles: vec![
                "receipt".into(),
                "invoice".into(),
                "your order".into(),
                "shipping".into(),
                "delivery".into(),
                "confirmation".into(),
                "verify".into(),
                "verification".into(),
                "verification code".into(),
                "password reset".into(),
                "two-factor".into(),
                "2FA".into(),
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

/// Catch transactional / no-reply senders by From-header substring.
/// Lower priority than the explicit domain list so a known service
/// provider still wins, but high enough to file generic
/// `noreply@example.org` mail into Updates rather than letting it
/// fall through to (none).
fn rule_updates_noreply_sender() -> CategoryRule {
    CategoryRule {
        id: "updates_noreply_sender".into(),
        display_name: "Updates — From contains noreply / no-reply / donotreply".into(),
        when: MatchExpr::Any {
            exprs: vec![
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "noreply".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "no-reply".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "donotreply".into(),
                },
                MatchExpr::HeaderContains {
                    header: "From".into(),
                    substring: "do-not-reply".into(),
                },
            ],
        },
        action: Action::FileInto {
            folder: "Updates".into(),
        },
        // Score 45 — sits below subject_keywords (50) so an order
        // confirmation from `noreply@…` still routes via the more
        // semantic keyword rule. Both end in Updates anyway.
        score: 45,
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

    /// REGRESSION-GUARD: GitHub notifications include a List-Unsubscribe
    /// header for compliance, but they're operational alerts (CI failures,
    /// PR reviews, security advisories) that belong in Updates, not
    /// Promotions. Service-provider domains must win against the generic
    /// List-Unsubscribe rule.
    #[test]
    fn github_with_list_unsubscribe_still_routes_to_updates() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:unsub@github.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "notifications@github.com", "[repo] Run failed: ci"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(
            first.id, "updates_senders",
            "GitHub mail with List-Unsubscribe must still go to Updates, got {}",
            first.id
        );
    }

    /// Stripe receipts are the same shape — transactional, with
    /// List-Unsubscribe — and must stay in Updates.
    /// Stripe receipts WERE routed to updates_senders (85) before the
    /// receipts rule (87) shipped. Now they go to Receipts. Promoted-
    /// regression guard: still must NOT hit promotions_listunsub even
    /// though they carry List-Unsubscribe.
    #[test]
    fn stripe_receipt_with_list_unsubscribe_routes_to_receipts() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:unsub@stripe.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "receipts@stripe.com", "Your receipt from Stripe"));
        assert_eq!(hits.first().unwrap().id, "receipts");
    }

    /// Facebook + Instagram notifications carry List-Unsubscribe but
    /// belong in Social, not Promotions.
    #[test]
    fn facebook_notification_with_list_unsubscribe_routes_to_social() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@facebookmail.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "friendsuggestion@facebookmail.com", "Friend suggestions"));
        assert_eq!(hits.first().unwrap().id, "social_senders");
    }

    #[test]
    fn instagram_subdomain_with_list_unsubscribe_routes_to_social() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@mail.instagram.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "follow@mail.instagram.com", "New followers"));
        assert_eq!(hits.first().unwrap().id, "social_senders");
    }

    /// Internal-infrastructure messages — PlausiDen Salesman, daily
    /// summary daemons, oncall alerts from the same fleet — must land
    /// in INBOX, never in a category folder. Even with a
    /// List-Unsubscribe header (which most SMTP libraries auto-add).
    #[test]
    fn internal_vultr_guest_sender_lands_in_inbox() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:x@vultr.guest>".into())];
        let hits = rules.evaluate(&ctx(&h, "salesman@vultr.guest", "daily summary"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(first.id, "internal_source");
        assert_eq!(
            first.action,
            Action::FileInto {
                folder: "INBOX".into()
            }
        );
    }

    #[test]
    fn internal_plausiden_com_sender_lands_in_inbox() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@plausiden.com", "Disk usage 87%"));
        assert_eq!(hits.first().unwrap().id, "internal_source");
    }

    /// Vultr default hostnames vary per image (.guest, .vultr, .local).
    /// All of them must hit internal_source.
    #[test]
    fn internal_vultr_vultr_hostname_lands_in_inbox() {
        let rules = CategoryRules::default();
        let h = vec![("from".into(), "claude-code@vultr.vultr".into())];
        let hits = rules.evaluate(&ctx(&h, "claude-code@vultr.vultr", "task ack"));
        assert_eq!(hits.first().unwrap().id, "internal_source");
    }

    /// Future Vultr hostname variants (e.g. web-02.plausiden.vultr.guest)
    /// are caught by the From-substring fallback.
    #[test]
    fn internal_unknown_vultr_subdomain_caught_by_substring() {
        let rules = CategoryRules::default();
        let h = vec![("from".into(), "robot@web-02.plausiden.vultr.guest".into())];
        let hits = rules.evaluate(&ctx(&h, "robot@web-02.plausiden.vultr.guest", "test"));
        assert_eq!(hits.first().unwrap().id, "internal_source");
    }

    /// Internal-source rule beats important_priority — direct internal
    /// messages don't need the X-Priority flag treatment.
    #[test]
    fn internal_beats_x_priority_flag() {
        let rules = CategoryRules::default();
        let h = vec![("x-priority".into(), "1".into())];
        let hits = rules.evaluate(&ctx(&h, "salesman@vultr.guest", "URGENT: lead"));
        assert_eq!(
            hits.first().unwrap().id,
            "internal_source",
            "internal_source (100) must outscore important_priority (90)"
        );
    }

    #[test]
    fn list_unsubscribe_overrides_subject_keywords() {
        // Mailchimp "your order" promo. Order: receipts (87) > listunsub
        // (80) > subject_keywords (50). The receipts rule wins now that
        // it ships — these mailings DO look like receipts. Promotions-
        // shaped marketing without "order" / "receipt" subject still
        // hits promotions_listunsub.
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@x>".into())];
        let hits = rules.evaluate(&ctx(&h, "noreply@mailchimp.com", "Your order is confirmed"));
        assert_eq!(hits.first().unwrap().id, "receipts");

        // Pure-promo subject still routes via list-unsubscribe.
        let hits = rules.evaluate(&ctx(&h, "noreply@mailchimp.com", "50% off this weekend!"));
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
    fn security_alert_subject_routes_to_important() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "noreply@accounts.google.com", "Security alert: new sign-in on Linux"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(first.id, "important_security");
        // important_security must beat updates_senders (which @google.com would otherwise hit).
    }

    #[test]
    fn docusign_routes_to_important() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "dse@docusign.net", "Please sign: Master Services Agreement"));
        assert_eq!(hits.first().unwrap().id, "important_signature");
    }

    #[test]
    fn chase_statement_routes_to_banking() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@chase.com", "Your December statement is ready"));
        assert_eq!(hits.first().unwrap().id, "banking");
    }

    #[test]
    fn fidelity_brokerage_alert_routes_to_banking() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "no-reply@fidelity.com", "Trade confirmation"));
        assert_eq!(hits.first().unwrap().id, "banking");
    }

    #[test]
    fn amex_charge_alert_routes_to_banking() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@americanexpress.com", "Large purchase notification"));
        assert_eq!(hits.first().unwrap().id, "banking");
    }

    #[test]
    fn bank_security_alert_still_pages_to_important() {
        // important_security (92) > banking (86): "new sign-in to your
        // bank account" must page, not be filed quietly into Banking.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@chase.com", "New sign-in detected on your Chase account"));
        assert_eq!(hits.first().unwrap().id, "important_security");
    }

    #[test]
    fn bank_card_declined_still_pages_to_important() {
        // important_billing_critical (89) > banking (86): "card
        // declined" must page.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@chase.com", "Action required: card declined"));
        assert_eq!(hits.first().unwrap().id, "important_billing_critical");
    }

    #[test]
    fn bank_receipt_routes_to_receipts_not_banking() {
        // receipts (87) > banking (86): "your receipt" wins because
        // it's a financial-record axis. Both are reasonable; receipts
        // is the more specific signal.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "no-reply@chase.com", "Your receipt for the $50 transfer"));
        assert_eq!(hits.first().unwrap().id, "receipts");
    }

    #[test]
    fn united_flight_confirmation_routes_to_travel() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "no-reply@united.com", "Your flight on May 12 to LAX"));
        assert_eq!(hits.first().unwrap().id, "travel");
    }

    #[test]
    fn marriott_reservation_routes_to_travel() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@marriott.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "reservations@marriott.com", "Confirmation 12345 — see you May 14"));
        assert_eq!(hits.first().unwrap().id, "travel");
    }

    #[test]
    fn airbnb_booking_routes_to_travel() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "automated@airbnb.com", "Your trip to Lisbon"));
        assert_eq!(hits.first().unwrap().id, "travel");
    }

    #[test]
    fn airline_receipt_routes_to_receipts_not_travel() {
        // receipts (87) beats travel (86): a "Your receipt from United"
        // is a financial record first, an itinerary second. Both
        // sensible — but receipt-shape wins on score.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "receipts@united.com", "Your receipt from United Airlines — $429.50"));
        assert_eq!(hits.first().unwrap().id, "receipts");
    }

    #[test]
    fn flight_action_required_routes_to_important_not_travel() {
        // Cancellation / "action required" must page to Important (89),
        // not be filed quietly into Travel (86).
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "alerts@united.com", "Action required: your flight has been canceled"));
        assert_eq!(hits.first().unwrap().id, "important_billing_critical");
    }

    #[test]
    fn stripe_receipt_routes_to_receipts_folder() {
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@stripe.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "receipts@stripe.com", "Your receipt from Stripe — $14.00"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(first.id, "receipts", "Stripe receipt should hit receipts rule first");
        assert_eq!(
            first.action,
            Action::FileInto {
                folder: "Receipts".into()
            }
        );
    }

    #[test]
    fn paypal_payment_received_routes_to_receipts() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "service@paypal.com", "You've been paid $50.00"));
        assert_eq!(hits.first().unwrap().id, "receipts");
    }

    #[test]
    fn order_confirmation_routes_to_receipts_not_updates() {
        // "Your order" matched the existing updates_subject_keywords rule
        // at score 50; the new receipts rule at 87 should win.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "orders@shopify.com", "Your order has shipped"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(first.id, "receipts", "order confirmations belong in Receipts");
    }

    #[test]
    fn payment_failure_still_routes_to_important_not_receipts() {
        // Score-ordering regression guard: payment-failure copy must beat
        // the receipts rule (87) and hit important_billing_critical (89).
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "billing@stripe.com", "Action required: payment failed"));
        let first = hits.first().expect("a rule fires");
        assert_eq!(
            first.id, "important_billing_critical",
            "payment FAILURE must page to Important, not file as a receipt"
        );
    }

    #[test]
    fn generic_github_notification_does_not_hit_receipts() {
        // A non-receipt GitHub email should still go to Updates.
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "noreply@github.com", "[repo] PR opened: fix bug"));
        assert_eq!(hits.first().unwrap().id, "updates_senders");
    }

    #[test]
    fn billing_failure_routes_to_important() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "billing@stripe.com", "Action required: payment failed"));
        // Beats updates_senders(@stripe.com, 85) because important_billing_critical is 89.
        assert_eq!(hits.first().unwrap().id, "important_billing_critical");
    }

    #[test]
    fn ordinary_stripe_receipt_routes_to_receipts() {
        // After the receipts rule shipped, "your receipt" subjects route
        // to Receipts (score 87) rather than updates_senders (85). They
        // must still NOT trigger important_billing_critical (89; needs
        // failure-shaped subject).
        let rules = CategoryRules::default();
        let h = vec![("list-unsubscribe".into(), "<mailto:u@stripe.com>".into())];
        let hits = rules.evaluate(&ctx(&h, "receipts@stripe.com", "Your receipt from Stripe — $14.00"));
        let first = hits.first().unwrap();
        assert_eq!(first.id, "receipts");
        assert_ne!(first.id, "important_billing_critical");
    }

    #[test]
    fn discourse_subdomain_routes_to_forums() {
        // forums_discourse_subdomain matches the "From" header substring, so
        // tests must populate it explicitly (the in-process evaluator does
        // not synthesize a From header from from_address).
        let rules = CategoryRules::default();
        let h = vec![("from".into(), "noreply@discourse.example.org".into())];
        let hits = rules.evaluate(&ctx(&h, "noreply@discourse.example.org", "[Discussion] new topic"));
        assert_eq!(hits.first().unwrap().id, "forums_discourse_subdomain");
    }

    #[test]
    fn rust_users_forum_routes_to_forums_not_updates() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "noreply@users.rust-lang.org", "Re: trait object lifetimes"));
        // forums_dev_communities (86) beats updates_senders (85) and listunsub (80).
        assert_eq!(hits.first().unwrap().id, "forums_dev_communities");
    }

    #[test]
    fn stackexchange_routes_to_forums() {
        let rules = CategoryRules::default();
        let h = vec![];
        let hits = rules.evaluate(&ctx(&h, "do-not-reply@stackexchange.com", "Your weekly digest"));
        assert_eq!(hits.first().unwrap().id, "forums_dev_communities");
    }

    #[test]
    fn alternate_listid_header_routes_to_forums() {
        let rules = CategoryRules::default();
        let h = vec![("x-list-id".into(), "<example-list.example.org>".into())];
        let hits = rules.evaluate(&ctx(&h, "alice@example.org", "thread reply"));
        assert_eq!(hits.first().unwrap().id, "forums_listid_substring");
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
        let updates_pos = s.find("updates_senders").expect("updates present");
        let promo_pos = s.find("promotions_listunsub").expect("promo present");
        let promo_senders_pos = s.find("promotions_senders").expect("promo senders present");
        // important (90) → updates_senders (85) → promotions_listunsub (80)
        // → forums_listid (75) → promotions_senders (70).
        assert!(imp_pos < updates_pos, "score 90 should precede score 85");
        assert!(updates_pos < promo_pos, "score 85 should precede score 80");
        assert!(promo_pos < promo_senders_pos, "score 80 should precede score 70");
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
