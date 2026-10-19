//! The findings in `report.rs` and the studio's badges in `web/src/badges.ts` must say the same things. Both are
//! checked against the same cases (`tests/fixtures/findings.json`); `web/e2e/badges.spec.ts` runs the TS side.

use serde_json::{json, Value};
use stencil_core::report::findings;

#[test]
fn findings_match_the_shared_cases() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/findings.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let got: Vec<Value> = findings(
            c["type"].as_str().unwrap(),
            &c["params"],
            std::slice::from_ref(&c["info"]),
        )
        .iter()
        .map(|f| json!({"level": f.level.name(), "text": f.text}))
        .collect();
        assert_eq!(
            Value::Array(got),
            c["expected"],
            "case: {}",
            c["name"].as_str().unwrap()
        );
    }
}
