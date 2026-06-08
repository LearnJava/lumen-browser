//! T2 (BackgroundOld) sleep helpers: form-state JSON serialisation/deserialisation.
//!
//! When a tab enters T2, the shell checkpoints its mutable form state
//! (input values, checkbox state) and scroll offsets to SQLite via
//! `lumen_storage::SleepingTabStore`.  On the next T2→T0 restore the
//! form state is read back and applied on top of the DOM so the user sees
//! the same field values they left.
//!
//! JSON format: `{"<node_id>": {"value": "...", "checked": true}, ...}`
//! where `<node_id>` is the decimal representation of `NodeId.0`.
//! Uses `lumen_core::json` — already in the dependency graph — so no new dep.

use std::collections::BTreeMap;

use lumen_core::json::{parse as parse_json, JsonValue};
use lumen_dom::NodeId;

use crate::forms::{FormControlState, FormState};

/// Serialise a `FormState` map to a compact JSON string.
///
/// Output: `{"<u32>": {"value": "<string>", "checked": <bool>}, ...}`.
/// Keys are sorted for deterministic output (easier to diff / test).
pub fn serialize_form_state(state: &FormState) -> String {
    // Collect into BTreeMap for stable key order.
    let sorted: BTreeMap<usize, &FormControlState> =
        state.iter().map(|(id, v)| (id.index(), v)).collect();

    let mut out = String::from('{');
    for (i, (k, v)) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // Minimal JSON escaping for the value string.
        let escaped = escape_json_string(&v.value);
        let checked = v.checked;
        out.push_str(&format!(r#""{k}":{{"value":"{escaped}","checked":{checked}}}"#));
    }
    out.push('}');
    out
}

/// Deserialise a JSON string produced by [`serialize_form_state`] back into a `FormState`.
///
/// Unknown keys and malformed entries are silently skipped so a corrupt
/// snapshot never prevents a tab from opening.
pub fn deserialize_form_state(json: &str) -> FormState {
    let Ok(root) = parse_json(json) else {
        return FormState::default();
    };
    let Some(obj) = root.as_object() else {
        return FormState::default();
    };

    let mut out = FormState::new();
    for (key, entry) in obj {
        let Ok(id_usize) = key.parse::<usize>() else {
            continue;
        };
        let Some(entry_obj) = entry.as_object() else {
            continue;
        };
        let value = entry_obj
            .get("value")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_owned();
        let checked = entry_obj
            .get("checked")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        out.insert(NodeId::from_index(id_usize), FormControlState { value, checked });
    }
    out
}

/// Escape a string for embedding inside a JSON double-quoted value.
///
/// Handles the characters that must be escaped per RFC 8259 §7.
fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(entries: &[(usize, &str, bool)]) -> FormState {
        entries
            .iter()
            .map(|&(id, val, checked)| {
                (NodeId::from_index(id), FormControlState { value: val.to_owned(), checked })
            })
            .collect()
    }

    #[test]
    fn empty_state_roundtrips() {
        let s = FormState::default();
        let json = serialize_form_state(&s);
        assert_eq!(json, "{}");
        let back = deserialize_form_state(&json);
        assert!(back.is_empty());
    }

    #[test]
    fn single_entry_roundtrips() {
        let s = make_state(&[(42, "hello", false)]);
        let json = serialize_form_state(&s);
        let back = deserialize_form_state(&json);
        assert_eq!(back.len(), 1);
        let v = back.get(&NodeId::from_index(42)).unwrap();
        assert_eq!(v.value, "hello");
        assert!(!v.checked);
    }

    #[test]
    fn checked_true_roundtrips() {
        let s = make_state(&[(7, "", true)]);
        let json = serialize_form_state(&s);
        let back = deserialize_form_state(&json);
        assert!(back.get(&NodeId::from_index(7)).unwrap().checked);
    }

    #[test]
    fn multiple_entries_roundtrip() {
        let s = make_state(&[(1, "alpha", false), (2, "beta", true), (100, "gamma", false)]);
        let json = serialize_form_state(&s);
        let back = deserialize_form_state(&json);
        assert_eq!(back.len(), 3);
        assert_eq!(back[&NodeId::from_index(1)].value, "alpha");
        assert_eq!(back[&NodeId::from_index(2)].value, "beta");
        assert!(back[&NodeId::from_index(2)].checked);
        assert_eq!(back[&NodeId::from_index(100)].value, "gamma");
    }

    #[test]
    fn escape_quotes_and_backslash() {
        let s = make_state(&[(1, r#"say "hi" \n"#, false)]);
        let json = serialize_form_state(&s);
        // Must not crash and must round-trip cleanly.
        let back = deserialize_form_state(&json);
        assert_eq!(back[&NodeId::from_index(1)].value, r#"say "hi" \n"#);
    }

    #[test]
    fn escape_newlines_tabs() {
        let s = make_state(&[(1, "line1\nline2\ttab", false)]);
        let json = serialize_form_state(&s);
        let back = deserialize_form_state(&json);
        assert_eq!(back[&NodeId::from_index(1)].value, "line1\nline2\ttab");
    }

    #[test]
    fn cyrillic_value_roundtrips() {
        let s = make_state(&[(99, "Привет мир", false)]);
        let json = serialize_form_state(&s);
        let back = deserialize_form_state(&json);
        assert_eq!(back[&NodeId::from_index(99)].value, "Привет мир");
    }

    #[test]
    fn invalid_json_returns_empty() {
        let back = deserialize_form_state("not valid json{");
        assert!(back.is_empty());
    }

    #[test]
    fn non_object_json_returns_empty() {
        let back = deserialize_form_state("[1,2,3]");
        assert!(back.is_empty());
    }

    #[test]
    fn non_numeric_key_skipped() {
        let back = deserialize_form_state(r#"{"bad_key":{"value":"x","checked":false}}"#);
        assert!(back.is_empty());
    }
}
