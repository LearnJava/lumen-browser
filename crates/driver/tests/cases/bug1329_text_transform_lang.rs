//! BUG-1329 — `text-transform` и язык: `lang`/`xml:lang` выбирает правила регистра (tr/az, lt,
//! nl, ga), а `innerText` и `Selection.toString()` дают тот же текст, что нарисован
//! раскладкой, а не свою приближённую копию.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><head><style>
  .cap { text-transform: capitalize }
  .up  { text-transform: uppercase }
  .low { text-transform: lowercase }
</style></head><body>
<div id="word" class="cap">john's apple foo_bar</div>
<div id="hyphen" class="cap">foo-bar (hello)</div>
<div id="nested" class="cap">
  hello <span>world</span>
</div>
<div id="nl" class="cap" lang="nl">ijsland</div>
<div id="en" class="cap" lang="en">ijsland</div>
<div id="tr" class="up" lang="tr">i&#x131;</div>
<div id="trlow" class="low" lang="tr-TR">&#x130;I</div>
<div id="inherit" lang="tr"><p class="up" id="child">istanbul</p></div>
<div id="reset" lang="tr"><p class="up" lang="en" id="child_en">istanbul</p></div>
<div id="xml" class="up" xml:lang="az">i</div>
<div id="lt" class="up" lang="lt">i&#x307;&#x300;</div>
<div id="ga" class="up" lang="ga">tAthair nathair</div>
<span id="sharp" class="up" lang="de">&#xDF;</span>
</body></html>"#;

fn text(s: &mut InProcessSession, id: &str) -> String {
    s.eval(&format!("document.getElementById('{id}').innerText"))
        .expect("eval")
        .trim_matches('"')
        .to_owned()
}

#[test]
fn inner_text_capitalize_follows_the_layout_word_rule() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    // Слово — до пробела: апостроф и `_` не начинают новое (раньше `John'S`, `Foo_Bar`).
    assert_eq!(text(&mut s, "word"), "John's Apple Foo_bar");
    // Дефис и скобка границу слова дают, `'` и `_` — нет (WPT `capitalize-036`).
    assert_eq!(text(&mut s, "hyphen"), "Foo-Bar (Hello)");
    assert_eq!(text(&mut s, "nested"), "Hello World");
}

#[test]
fn lang_tailors_case_mapping() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(text(&mut s, "nl"), "IJsland");
    assert_eq!(text(&mut s, "en"), "Ijsland");
    assert_eq!(text(&mut s, "tr"), "\u{130}I");
    assert_eq!(text(&mut s, "trlow"), "i\u{131}");
    assert_eq!(text(&mut s, "xml"), "\u{130}");
    assert_eq!(text(&mut s, "lt"), "I\u{300}");
    assert_eq!(text(&mut s, "ga"), "tATHAIR NATHAIR");
}

#[test]
fn lang_is_inherited_and_overridable() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(text(&mut s, "child"), "\u{130}STANBUL");
    assert_eq!(text(&mut s, "child_en"), "ISTANBUL");
}

#[test]
fn selection_to_string_applies_text_transform() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let got = s
        .eval(
            "(function () { var t = document.getElementById('sharp'); \
               getSelection().setBaseAndExtent(t, 0, t, 1); \
               return getSelection().toString(); })()",
        )
        .expect("eval selection");
    assert_eq!(got.trim_matches('"'), "SS");
}
