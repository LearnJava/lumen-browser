//! BUG-1402: `element.style.{column-count,column-width,column-height,column-fill,column-span,
//! column-wrap,columns}` — грамматика и каноническая запись; значения multicol в
//! `getComputedStyle` (список ключей и вычисленные значения).

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

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn columns_shorthand_is_canonical() {
    let rt = rt();
    for (val, want) in [
        ("auto 3", "3"),
        ("auto 10em", "10em"),
        ("3 auto", "3"),
        ("10em auto", "10em"),
        ("2 10px", "10px 2"),
        ("10px 2", "10px 2"),
        ("auto auto", "auto"),
        ("0 1", "0px 1"),
        ("1 0", "0px 1"),
        ("2 10px / 100px", "10px 2 / 100px"),
        ("auto / 100px", "auto / 100px"),
        ("10px 2 / auto", "10px 2"),
        ("auto / auto", "auto"),
        ("initial", "initial"),
    ] {
        assert_eq!(assign(&rt, "columns", val), want, "columns = {val:?}");
    }
}

#[test]
fn columns_shorthand_rejects_bad_grammar() {
    let rt = rt();
    for val in [
        "none", "10px 20px", "10 20", "0 0", "0 7px", "auto auto auto", "10em auto auto", "initial initial",
        "/ 100px", "/ auto", "/", "auto /", "100px /", "2 /", "100px 2 /", "10% 2",
    ] {
        assert_eq!(assign(&rt, "columns", val), "", "columns = {val:?} должно отклоняться");
    }
}

#[test]
fn longhands_validate_and_canonicalize() {
    let rt = rt();
    assert_eq!(assign(&rt, "columnWidth", "0"), "0px");
    assert_eq!(assign(&rt, "columnWidth", "auto"), "auto");
    // Заданное значение остаётся математической функцией; 235 — уже вычисленное.
    assert_eq!(assign(&rt, "columnCount", "calc(1 + 234)"), "calc(235)");
    assert_eq!(assign(&rt, "columnCount", "3"), "3");
    assert_eq!(assign(&rt, "columnFill", "balance-all"), "balance-all");
    assert_eq!(assign(&rt, "columnSpan", "all"), "all");
    for (prop, val) in [
        ("columnCount", "none"), ("columnCount", "2.5"), ("columnCount", "-1"), ("columnCount", "0"),
        ("columnCount", "auto 1"), ("columnWidth", "none"), ("columnWidth", "10"), ("columnWidth", "-20px"),
        ("columnWidth", "30%"), ("columnWidth", "auto 40px"), ("columnFill", "none"), ("columnFill", "auto balance"),
        ("columnSpan", "auto"), ("columnSpan", "none all"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}
