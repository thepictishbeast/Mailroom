//! Property-based tests for the `CategoryRules` evaluator.
//!
//! The evaluator is the load-bearing decision point in the mail
//! pipeline — every incoming message gets routed through it, and a
//! bug here either misroutes mail (annoyance) or routes mail to a
//! folder the user can't see (lost messages). Conventional unit
//! tests pin specific cases; proptest catches the cases we didn't
//! think to write.
//!
//! Properties enforced:
//!
//! 1. **Determinism.** The same `(rules, ctx)` always yields the
//!    same hits, in the same order. (Iterator-order or hash-set
//!    drift would break audit reproducibility.)
//! 2. **Score ordering.** Hits come back highest-score-first.
//! 3. **Tie-break by id.** Equal scores resolve alphabetically.
//! 4. **stop_on_match short-circuits.** No rule scoring lower than
//!    a `stop_on_match=true` hit appears in the output.
//! 5. **No empty-input panic.** An empty rule set + an arbitrary
//!    header bag returns an empty hit list, never panics.
//!
//! These properties are the audit-explainer's guarantees: a future
//! "why is this here?" UI can rely on the evaluator behaving
//! deterministically across versions.

use mail_config::categories::{Action, CategoryRule, CategoryRules, MatchExpr, MessageContext};
use proptest::prelude::*;

/// Generate a valid `MatchExpr`. We bias toward header-based
/// matchers because that's the realistic distribution of rules.
fn arb_match_expr() -> impl Strategy<Value = MatchExpr> {
    prop_oneof![
        Just(MatchExpr::Always),
        ("[a-z][a-z0-9-]{0,15}", "[a-zA-Z0-9 ._-]{1,30}")
            .prop_map(|(header, substring)| MatchExpr::HeaderContains { header, substring }),
        "[a-z][a-z0-9-]{0,15}".prop_map(|header| MatchExpr::HasHeader { header }),
        "@[a-z0-9][a-z0-9.-]{0,20}\\.[a-z]{2,5}"
            .prop_map(|d| MatchExpr::FromDomainIn { domains: vec![d] }),
        prop::collection::vec("[a-zA-Z]{1,15}", 1..3)
            .prop_map(|needles| MatchExpr::SubjectContainsAny { needles }),
    ]
}

/// Generate a valid `CategoryRule`.
fn arb_rule() -> impl Strategy<Value = CategoryRule> {
    (
        "[a-z][a-z0-9_]{2,20}",       // id
        "[A-Za-z][A-Za-z0-9 ]{2,30}", // display_name
        arb_match_expr(),
        any::<i32>().prop_map(i32::abs),
        any::<bool>(),
        "[A-Za-z][A-Za-z0-9._/-]{1,30}", // folder
    )
        .prop_map(
            |(id, display_name, when, score, stop, folder)| CategoryRule {
                id,
                display_name,
                when,
                action: Action::FileInto { folder },
                score,
                stop_on_match: stop,
            },
        )
}

/// Generate a `(headers, from, subject)` triple.
fn arb_message() -> impl Strategy<Value = (Vec<(String, String)>, String, String)> {
    (
        prop::collection::vec(("[a-z][a-z0-9-]{1,20}", "[a-zA-Z0-9 .,@_-]{0,80}"), 0..6),
        "[a-z][a-z0-9.+_-]{0,15}@[a-z][a-z0-9.-]{0,20}\\.[a-z]{2,5}",
        "[A-Za-z0-9 ]{0,60}",
    )
}

// Hits are deterministic across repeated evaluation.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn evaluate_is_deterministic(
        rules in prop::collection::vec(arb_rule(), 0..10),
        message in arb_message(),
    ) {
        let cr = CategoryRules { rules };
        let ctx = MessageContext {
            headers: &message.0,
            from_address: &message.1,
            subject: &message.2,
        };
        let first: Vec<String> = cr.evaluate(&ctx).iter().map(|r| r.id.clone()).collect();
        let second: Vec<String> = cr.evaluate(&ctx).iter().map(|r| r.id.clone()).collect();
        prop_assert_eq!(first, second);
    }
}

// Hits are ordered by score descending, then id ascending.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn hits_respect_score_then_id_order(
        rules in prop::collection::vec(arb_rule(), 0..10),
        message in arb_message(),
    ) {
        let cr = CategoryRules { rules };
        let ctx = MessageContext {
            headers: &message.0,
            from_address: &message.1,
            subject: &message.2,
        };
        let hits = cr.evaluate(&ctx);
        for window in hits.windows(2) {
            let a = window[0];
            let b = window[1];
            // Score is non-increasing; on equal scores, id is non-decreasing.
            prop_assert!(
                a.score > b.score || (a.score == b.score && a.id <= b.id),
                "ordering violated: ({}, score={}) before ({}, score={})",
                a.id, a.score, b.id, b.score
            );
        }
    }
}

// `stop_on_match` truncates the hit list at the first such match.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn stop_on_match_truncates(
        rules in prop::collection::vec(arb_rule(), 1..10),
        message in arb_message(),
    ) {
        let cr = CategoryRules { rules };
        let ctx = MessageContext {
            headers: &message.0,
            from_address: &message.1,
            subject: &message.2,
        };
        let hits = cr.evaluate(&ctx);
        for (i, h) in hits.iter().enumerate() {
            if h.stop_on_match {
                // Nothing must follow this hit.
                prop_assert_eq!(i, hits.len() - 1, "stop_on_match did not terminate the hit list");
                break;
            }
        }
    }
}

// An empty rule set never produces a hit, regardless of the message.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn empty_rules_means_empty_hits(message in arb_message()) {
        let cr = CategoryRules { rules: vec![] };
        let ctx = MessageContext {
            headers: &message.0,
            from_address: &message.1,
            subject: &message.2,
        };
        let hits = cr.evaluate(&ctx);
        prop_assert!(hits.is_empty());
    }
}
