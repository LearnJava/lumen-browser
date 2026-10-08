//! BUG-1325: CSSOM свойств CSS Text в `element.style` — грамматика ключевых слов и длин
//! (`_LUMEN_KEYWORD_PROPERTIES`, `_lumen_css_canonical_text`), шорткоды `white-space`/`text-wrap`
//! (раскладка на лонгхенды и обратная сборка), алиас `word-wrap` → `overflow-wrap`.

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
fn invalid_text_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("tabSize", "-10px"),
        ("tabSize", "30%"),
        ("hyphens", "normal"),
        ("lineBreak", "none"),
        ("overflowWrap", "auto"),
        ("wordWrap", "normal break-word"),
        ("wordBreak", "auto"),
        ("textAlignLast", "none"),
        ("textWrapMode", "balance"),
        ("textWrapStyle", "wrap"),
        ("textWrap", "wrap nowrap"),
        ("whiteSpace", "balance"),
        ("whiteSpaceCollapse", "collapse preserve"),
        ("letterSpacing", "auto"),
        ("wordSpacing", "normal 10px"),
        ("textIndent", "hanging"),
        ("textTransform", "uppercase lowercase"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_text_values_are_canonical() {
    let rt = rt();
    for (prop, val, want) in [
        ("tabSize", "2.5", "2.5"),
        ("letterSpacing", "0", "0px"),
        ("wordSpacing", "calc(2ch - 30%)", "calc(-30% + 2ch)"),
        ("textIndent", "each-line hanging 10px", "10px hanging each-line"),
        ("textTransform", "full-size-kana full-width capitalize", "capitalize full-width full-size-kana"),
        ("textAlign", "justify", "justify"),
        ("textAlign", "match-parent", "match-parent"),
        ("wordBreak", "auto-phrase", "auto-phrase"),
        ("textWrap", "auto", "wrap"),
        ("textWrap", "balance nowrap", "nowrap balance"),
        ("textWrap", "stable wrap", "stable"),
        ("whiteSpace", "preserve nowrap", "pre"),
        ("whiteSpace", "wrap", "normal"),
        ("whiteSpace", "preserve-breaks nowrap", "preserve-breaks nowrap"),
        ("whiteSpace", "pre-line", "pre-line"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}

#[test]
fn word_wrap_is_an_alias_of_overflow_wrap() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.wordWrap = 'anywhere'; \
             d.style.overflowWrap + '|' + d.style.cssText"
        ),
        "anywhere|overflow-wrap: anywhere;"
    );
}

#[test]
fn white_space_and_text_wrap_share_text_wrap_mode() {
    let rt = rt();
    // `white-space` пишет `text-wrap-mode`, но не трогает `text-wrap-style`.
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.textWrap = 'balance'; \
             d.style.whiteSpace = 'pre'; \
             [d.style.textWrapMode, d.style.textWrapStyle, d.style.whiteSpaceCollapse, d.style.textWrap].join('|')"
        ),
        "nowrap|balance|preserve|nowrap balance"
    );
    // Пустое присваивание шорткоду снимает оба лонгхенда.
    assert_eq!(
        eval_str(
            &rt,
            "var e = document.createElement('div'); e.style.whiteSpace = 'pre'; e.style.whiteSpace = ''; \
             e.style.cssText + '|' + e.style.whiteSpace"
        ),
        "|"
    );
}
