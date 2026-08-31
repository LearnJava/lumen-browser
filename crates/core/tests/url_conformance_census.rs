//! LIB-0 conformance census (docs/conformance-method.md §3.1): measures
//! `lumen_core::url` against the WHATWG URL test corpus so ADR-027/LIB-6 can
//! decide "fix точечно or replace with `url`" from a number, not a guess.
//! Not run by default — `cargo test` skips `#[ignore]`d tests. Invoke with:
//! `cargo test -p lumen-core --test url_conformance_census -- --ignored --nocapture`
//! and paste the printed table into `docs/conformance/<date>.md`. Re-run the
//! same way after LIB-6 to compare against the "before" numbers below.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lumen_core::url::Url;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/wpt/url/resources/urltestdata.json")
}

#[test]
#[ignore]
fn url_conformance_census() {
    let text = fs::read_to_string(corpus_path()).expect("read urltestdata.json");
    let data: Vec<Value> = serde_json::from_str(&text).expect("parse urltestdata.json");

    let mut total_records = 0usize;
    let mut excluded_setter = 0usize;
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut examples: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();

    for entry in &data {
        let Some(obj) = entry.as_object() else {
            continue; // top-level comment string, not a test record
        };
        total_records += 1;

        // Records carrying `searchParams` test URLSearchParams stringification
        // (an API-setter-adjacent surface we don't implement at all — the Url
        // type is immutable) rather than the parser itself; excluded from the
        // denominator per docs/conformance-method.md §2.
        if obj.contains_key("searchParams") {
            excluded_setter += 1;
            continue;
        }

        let input = obj.get("input").and_then(Value::as_str).unwrap_or("");
        let base = obj.get("base").and_then(Value::as_str);
        let expect_failure = obj.get("failure").and_then(Value::as_bool).unwrap_or(false);

        let our = match base {
            Some(b) => Url::parse(b).and_then(|base_url| base_url.resolve(input)),
            None => Url::parse(input),
        };

        if expect_failure {
            match our {
                Err(_) => pass += 1,
                Ok(_) => {
                    fail += 1;
                    bump(
                        &mut reasons,
                        &mut examples,
                        "too-permissive: parsed input the spec rejects",
                        input,
                    );
                }
            }
            continue;
        }

        let u = match our {
            Ok(u) => u,
            Err(e) => {
                fail += 1;
                bump(&mut reasons, &mut examples, classify_parse_error(&e.to_string()), input);
                continue;
            }
        };

        let expected_protocol = obj.get("protocol").and_then(Value::as_str).unwrap_or("");
        let expected_host = obj.get("host").and_then(Value::as_str).unwrap_or("");
        let expected_hostname = obj.get("hostname").and_then(Value::as_str).unwrap_or("");
        let expected_port = obj.get("port").and_then(Value::as_str).unwrap_or("");
        let expected_pathname = obj.get("pathname").and_then(Value::as_str).unwrap_or("");
        let expected_search = obj.get("search").and_then(Value::as_str).unwrap_or("");
        let expected_hash = obj.get("hash").and_then(Value::as_str).unwrap_or("");

        let our_protocol = format!("{}:", u.scheme());
        let our_port = u.port().map(|p| p.to_string()).unwrap_or_default();
        let our_host = match u.port() {
            Some(p) => format!("{}:{}", u.host(), p),
            None => u.host().to_string(),
        };
        let our_hostname = u.host().to_string();
        let our_pathname = u.path().to_string();
        let our_search = u.query().map(|q| format!("?{q}")).unwrap_or_default();
        let our_hash = u.fragment().map(|f| format!("#{f}")).unwrap_or_default();

        let mut mismatches = Vec::new();
        if our_protocol != expected_protocol {
            mismatches.push("protocol");
        }
        if our_host != expected_host {
            mismatches.push("host");
        }
        if our_hostname != expected_hostname {
            mismatches.push("hostname");
        }
        if our_port != expected_port {
            mismatches.push("port");
        }
        if our_pathname != expected_pathname {
            mismatches.push("pathname");
        }
        if our_search != expected_search {
            mismatches.push("search");
        }
        if our_hash != expected_hash {
            mismatches.push("hash");
        }

        if mismatches.is_empty() {
            pass += 1;
        } else {
            fail += 1;
            bump(&mut reasons, &mut examples, classify_field_mismatch(&mismatches, input), input);
        }
    }

    let denom = total_records - excluded_setter;
    println!("=== LIB-0 URL census ===");
    println!("total records (incl. excluded): {total_records}");
    println!("excluded (API-setter/searchParams): {excluded_setter}");
    println!("denominator: {denom}");
    println!("pass: {pass}");
    println!("fail: {fail}");
    println!("pass rate: {:.2}%", 100.0 * pass as f64 / denom as f64);
    println!("--- failure reasons ---");
    for (reason, count) in &reasons {
        println!("{count:5}  {reason}");
        if let Some(exs) = examples.get(reason) {
            for e in exs.iter().take(3) {
                println!("         e.g. {e:?}");
            }
        }
    }

    assert_eq!(pass + fail, denom, "every non-excluded record must be counted exactly once");
}

fn bump(
    reasons: &mut BTreeMap<&'static str, usize>,
    examples: &mut BTreeMap<&'static str, Vec<String>>,
    reason: &'static str,
    input: &str,
) {
    *reasons.entry(reason).or_insert(0) += 1;
    let list = examples.entry(reason).or_default();
    if list.len() < 3 {
        list.push(input.to_string());
    }
}

fn classify_parse_error(msg: &str) -> &'static str {
    if msg.contains("missing scheme") {
        "unexpected-failure: missing scheme (relative reference, e.g. bad/invalid base)"
    } else if msg.contains("empty host") {
        "unexpected-failure: empty host rejected (special scheme requires host)"
    } else if msg.contains("invalid port") {
        "unexpected-failure: port"
    } else {
        "unexpected-failure: other"
    }
}

fn classify_field_mismatch(mismatches: &[&str], input: &str) -> &'static str {
    let has = |f: &str| mismatches.contains(&f);
    let looks_percent = input.contains('%');
    let looks_idn = !input.is_ascii() || input.contains("xn--");
    let looks_dotseg = input.contains("/.") || input.contains("/..");
    let looks_empty_query = input.ends_with('?');

    if (has("host") || has("hostname")) && looks_idn {
        "field-mismatch: IDN / non-ASCII host (no IDNA normalization)"
    } else if (has("pathname") || has("search")) && looks_percent {
        "field-mismatch: percent-encoding not normalized"
    } else if has("pathname") && looks_dotseg {
        "field-mismatch: dot-segment normalization"
    } else if has("search") && looks_empty_query {
        "field-mismatch: empty query"
    } else if has("host") || has("hostname") {
        "field-mismatch: host (other)"
    } else if has("pathname") {
        "field-mismatch: pathname (other)"
    } else if has("search") {
        "field-mismatch: search (other)"
    } else if has("hash") {
        "field-mismatch: hash (other)"
    } else if has("port") {
        "field-mismatch: port (other)"
    } else if has("protocol") {
        "field-mismatch: protocol (other)"
    } else {
        "field-mismatch: other"
    }
}
