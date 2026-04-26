//! Wire-format compatibility test against the Thundercrab client.
//!
//! `mail_config::CategoryRule` and `thundercrab_core::CrabRule` share a
//! serde shape modulo a single `origin` field. This test asserts the
//! shape from the server side; the symmetric test in
//! `thundercrab/thundercrab-suggestions/tests/schema_compat.rs` asserts
//! it from the client side. If you change one, change both.

use mail_config::{Action, CategoryRule, MatchExpr};
use serde_json::Value;

fn snake_eq(v: &Value, key: &str, expected: &str) -> bool {
    v.get(key).and_then(Value::as_str) == Some(expected)
}

#[test]
fn category_rule_uses_kind_tag_with_snake_case() {
    let r = CategoryRule {
        id: "promotions_listunsub".into(),
        display_name: "Promotions".into(),
        when: MatchExpr::HasHeader {
            header: "List-Unsubscribe".into(),
        },
        action: Action::FileInto {
            folder: "Promotions".into(),
        },
        score: 80,
        stop_on_match: true,
    };
    let v: Value = serde_json::to_value(&r).unwrap();

    // Top-level fields the client expects.
    assert!(v.get("id").is_some());
    assert!(v.get("display_name").is_some());
    assert!(v.get("score").is_some());
    assert!(v.get("stop_on_match").is_some());

    // The discriminant must be `kind`, the value snake_case.
    assert!(snake_eq(&v["when"], "kind", "has_header"));
    assert!(snake_eq(&v["action"], "kind", "file_into"));

    // No `origin` here — client adds it on its side.
    assert!(v.get("origin").is_none());
}

#[test]
fn nested_any_round_trips() {
    let r = CategoryRule {
        id: "important_priority".into(),
        display_name: "Important".into(),
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
        stop_on_match: false,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CategoryRule = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}
