//! BUG-1408 срез 8: шорткод `font` в `element.style` — значение проверяется по грамматике
//! CSS Fonts L4 §6.10, раскладывается по семи группам лонгхендов (`_lumen_css_expand_font`) и
//! ставится обратно из них (`_lumen_css_font_shorthand`).

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

/// `style.font = val` и чтение `style.font`; невалидное значение оставляет пустую строку.
fn assign(rt: &V8JsRuntime, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style.font = {val:?}; d.style.font"),
    )
}

#[test]
fn font_shorthand_is_canonical() {
    let rt = rt();
    for (val, want) in [
        ("medium serif", "medium serif"),
        ("normal medium/normal sans-serif", "medium sans-serif"),
        ("normal normal normal italic 10px/normal Menu", "italic 10px Menu"),
        ("italic small-caps bold condensed 10px / 1.2 serif", "italic small-caps bold condensed 10px / 1.2 serif"),
        ("ultra-expanded 20%/1.2 FB Armada", "ultra-expanded 20% / 1.2 FB Armada"),
        ("bolder calc(30% - 40px)/calc(120% + 1.2em) monospace", "bolder calc(30% - 40px) / calc(120% + 1.2em) monospace"),
        ("oblique xx-large fantasy", "oblique xx-large fantasy"),
        ("900 xx-small serif", "900 xx-small serif"),
    ] {
        assert_eq!(assign(&rt, val), want, "font = {val:?}");
    }
}

#[test]
fn invalid_font_shorthand_is_rejected() {
    let rt = rt();
    for val in [
        "menu icon",
        "italic oblique xx-small serif",
        "small-caps small-caps medium/normal sans-serif",
        "bold bolder xx-large/1.2 cursive",
        "italic small-caps lighter condensed normal smaller monospace",
        "normal normal normal normal normal 20%/1.2 \"FB Armada\"",
        "oldstyle-nums medium serif",
        "normal full-width medium serif",
        "100% larger / 2 fantasy",
        "bogus",
    ] {
        assert_eq!(assign(&rt, val), "", "font = {val:?} должен быть отброшен");
    }
}

#[test]
fn font_shorthand_sets_its_longhands() {
    let rt = rt();
    let read = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.font = 'italic small-caps bold condensed 10px / 1.2 serif'; \
         ['font-style','font-variant-caps','font-variant-ligatures','font-weight','font-stretch','font-size', \
          'line-height','font-family'] \
           .map(function(p) { return d.style.getPropertyValue(p); }).join('|')",
    );
    assert_eq!(read, "italic|small-caps|normal|bold|condensed|10px|1.2|serif");
}

#[test]
fn font_shorthand_follows_css2_variant_only() {
    let rt = rt();
    // WPT `font-shorthand-variant.html`: шорткод возвращается только для CSS2 `small-caps`.
    let read = eval_str(
        &rt,
        "var d = document.createElement('div'); var out = []; \
         d.style.font = 'medium serif'; out.push(d.style.font); \
         d.style.fontVariant = 'small-caps'; out.push(d.style.font); \
         d.style.fontVariant = 'titling-caps'; out.push(d.style.font); \
         d.style.fontVariant = 'normal'; out.push(d.style.font); \
         d.style.fontVariant = 'full-width'; out.push(d.style.font); out.join('|')",
    );
    assert_eq!(read, "medium serif|small-caps medium serif||medium serif|");
}

#[test]
fn removing_font_drops_its_longhands() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.font = 'medium serif'; d.style.font = ''; \
             ['font-size','font-family','font-style'].map(function(p) { return d.style.getPropertyValue(p); }).join('|')",
        ),
        "||"
    );
    assert_eq!(
        eval_str(&rt, "var d = document.createElement('div'); d.style.font = 'medium serif'; d.style.removeProperty('font')"),
        "medium serif"
    );
}

#[test]
fn system_font_keyword_round_trips() {
    let rt = rt();
    let read = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.font = 'italic xx-large/0px fantasy'; \
         d.style.font = 'caption'; var first = d.style.font; d.style.font = first; \
         [first, d.style.font].join('|')",
    );
    assert_eq!(read, "caption|caption");
}

#[test]
fn css_text_uses_the_font_shorthand() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.font = 'normal normal normal italic 10px/normal Menu'; d.style.cssText",
        ),
        "font: italic 10px Menu;"
    );
}
