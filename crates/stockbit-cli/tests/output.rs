//! JSON output formatting.

use serde_json::json;
use stockbit_cli::output::{write_json, Format};

#[test]
fn compact_is_single_line() {
    let mut buf = Vec::new();
    write_json(&mut buf, &json!({"a": 1, "b": [1, 2]}), Format::Compact).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s, "{\"a\":1,\"b\":[1,2]}\n");
}

#[test]
fn pretty_has_indent_and_newlines() {
    let mut buf = Vec::new();
    write_json(&mut buf, &json!({"a": 1}), Format::Pretty).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("\n  \"a\""));
    assert!(s.ends_with("}\n"));
}
