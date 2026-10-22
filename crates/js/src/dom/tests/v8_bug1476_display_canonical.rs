//! BUG-1476: `element.style.display` читается в каноническом виде CSS Display 3 §2
//! (`inline flow-root` → `inline-block`), недопустимое значение отбрасывается.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn display_reads_back_canonical_form() {
    let rt = rt();
    let cases = [
        ("flow", "block"),
        ("inline flow-root", "inline-block"),
        ("flow-root inline", "inline-block"),
        ("block flow-root", "flow-root"),
        ("list-item block", "list-item"),
        ("flow-root list-item", "flow-root list-item"),
        ("list-item inline", "inline list-item"),
        ("list-item inline flow-root", "inline flow-root list-item"),
        ("inline flex", "inline-flex"),
        ("block grid", "grid"),
        ("table inline", "inline-table"),
        ("ruby", "ruby"),
        ("inline ruby", "ruby"),
        ("ruby block", "block ruby"),
        ("run-in", "run-in"),
        ("flow run-in", "run-in"),
        ("run-in flow-root", "run-in flow-root"),
        ("flow-root run-in list-item", "run-in flow-root list-item"),
        ("inline-grid-lanes", "inline-grid-lanes"),
        ("grid-lanes inline", "inline grid-lanes"),
        ("contents", "contents"),
        ("table-cell", "table-cell"),
    ];
    for (input, want) in cases {
        let got = eval_str(
            &rt,
            &format!("var d = document.createElement('div'); d.style.display = '{input}'; d.style.display"),
        );
        assert_eq!(got, want, "display: {input}");
    }
}

#[test]
fn invalid_display_is_dropped() {
    let rt = rt();
    for bad in ["bogus", "block block", "flex grid", "list-item flex", "inline-block block", "block ruby-text"] {
        let got = eval_str(
            &rt,
            &format!("var d = document.createElement('div'); d.style.display = 'flex'; d.style.display = '{bad}'; d.style.display"),
        );
        assert_eq!(got, "flex", "display: {bad}");
    }
}
