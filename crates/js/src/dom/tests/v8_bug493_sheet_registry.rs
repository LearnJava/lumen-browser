//! BUG-493 (живое окно): реестр `document.styleSheets`/`element.sheet`
//! сверяется с DOM на стороне JS — `<style>`, вставленный скриптом после
//! загрузки, получает лист сразу, а CSSOM-правки его листа доходят и до
//! синхронного флаша, и до каскада шелла (`V8JsRuntime::patch_cascade`).
//! См. `crates/js/src/v8_runtime/sheet_sync.rs`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name + ':' + e.message; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

/// Сам сценарий бага: styled-components создаёт пустой `<style>`,
/// вставляет его в `<head>` и тут же читает `tag.sheet` — до правки `null`
/// и ошибка #17.
#[test]
fn appended_style_has_a_sheet_at_once() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var before = document.styleSheets.length; \
              var t = document.createElement('style'); \
              var detached = t.sheet; \
              document.head.appendChild(t); \
              var sh = t.sheet; \
              return [before, detached, sh !== null, sh instanceof CSSStyleSheet, \
                      sh.ownerNode === t, document.styleSheets.length, \
                      document.styleSheets[before] === undefined ? 'u' : 'ok', \
                      sh.cssRules.length].join(); })()"
        ),
        "0,,true,true,true,1,ok,0"
    );
}

/// `tag.sheet` — один и тот же объект между чтениями: библиотеки сравнивают
/// его по ссылке.
#[test]
fn element_sheet_is_the_same_object_across_reads() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var t = document.createElement('style'); \
              document.head.appendChild(t); return t.sheet === t.sheet; })()"
        ),
        "true"
    );
}

/// Speedy-режим CSS-in-JS: правила идут только через `insertRule`, текст
/// `<style>` остаётся пустым. Правила видны в `cssRules` и переживают
/// следующие мутации DOM (повторная сверка не должна перечитать пустой текст
/// и потерять их).
#[test]
fn insert_rule_into_appended_style_survives_later_dom_mutations() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var t = document.createElement('style'); \
              document.head.appendChild(t); \
              t.sheet.insertRule('.a { color: red; }', 0); \
              t.sheet.insertRule('.b { color: blue; }', 1); \
              document.body.appendChild(document.createElement('p')); \
              return [t.sheet.cssRules.length, t.sheet.cssRules[1].selectorText, \
                      document.styleSheets[0].cssRules.length].join(); })()"
        ),
        "2,.b,2"
    );
}

/// CSSOM §6.1: узел вне дерева листа не имеет.
#[test]
fn removed_style_loses_its_sheet() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var t = document.createElement('style'); \
              document.head.appendChild(t); var sh = t.sheet; \
              t.remove(); \
              return [t.sheet, document.styleSheets.length, sh.ownerNode, sh.cssRules.length].join(); })()"
        ),
        ",0,,0"
    );
}

/// Обёртка листа привязана к своему `<style>`, а не к позиции в реестре:
/// `<style>`, вставленный раньше по документу, сдвигает индексы.
#[test]
fn sheet_wrapper_follows_its_owner_when_indices_shift() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var late = document.createElement('style'); \
              document.body.appendChild(late); var sh = late.sheet; \
              var early = document.createElement('style'); \
              early.textContent = 'p { color: green; }'; \
              document.head.appendChild(early); \
              sh.insertRule('.late { color: red; }', 0); \
              return [document.styleSheets.length, document.styleSheets[0].ownerNode === early, \
                      early.sheet.cssRules[0].selectorText, late.sheet.cssRules[0].selectorText].join(); })()"
        ),
        "2,true,p,.late"
    );
}

/// Смена текста `<style>` — новый лист из нового текста.
#[test]
fn style_text_change_reparses_the_sheet() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var t = document.createElement('style'); \
              t.textContent = 'a { color: red; }'; document.head.appendChild(t); \
              var n1 = t.sheet.cssRules.length; \
              t.textContent = 'a { color: red; } b { color: blue; }'; \
              return [n1, t.sheet.cssRules.length].join(); })()"
        ),
        "1,2"
    );
}

/// `<style>` в теневом дереве относится к своему `ShadowRoot`, а не к документу.
#[test]
fn shadow_style_is_listed_by_its_shadow_root_only() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var host = document.createElement('div'); document.body.appendChild(host); \
              var sr = host.attachShadow({mode:'open'}); \
              var ss = document.createElement('style'); sr.appendChild(ss); \
              return [ss.sheet !== null, document.styleSheets.length, sr.styleSheets.length, \
                      sr.styleSheets[0].ownerNode === ss].join(); })()"
        ),
        "true,0,1,true"
    );
}

/// Правило, вставленное через `insertRule` в пустой `<style>`, действует на
/// `getComputedStyle` в том же тике — его текста в каскаде нет, лист
/// вливается целиком.
#[test]
fn insert_rule_into_empty_style_reaches_same_tick_computed_style() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("#main { color: red; }")));
    rt.update_viewport_size(800.0, 600.0);
    assert_eq!(
        s(
            &rt,
            "(function(){ var t = document.createElement('style'); \
              document.head.appendChild(t); \
              t.sheet.insertRule('#main { width: 55px; display: block; }', 0); \
              return getComputedStyle(document.getElementById('main')).width; })()"
        ),
        "55px"
    );
}

/// Каскад шелла: `patch_cascade` вливает лист, наполненный через
/// `insertRule`, и поднимает `cssom_epoch`, по которому шелл решает
/// пересобрать каскад. Без правок — `None`.
#[test]
fn patch_cascade_lays_in_insert_rule_only_sheets() {
    let rt = v8_runtime_with_dom(make_doc());
    let base = lumen_css_parser::parse("p { color: red; }");
    assert!(rt.patch_cascade(&base).is_none());
    let epoch0 = rt.cssom_epoch();
    rt.eval(
        "var t = document.createElement('style'); document.head.appendChild(t); \
         t.sheet.insertRule('.x { color: blue; }', 0);",
    )
    .unwrap();
    assert_ne!(rt.cssom_epoch(), epoch0);
    let patched = rt.patch_cascade(&base).expect("insertRule must reach the cascade");
    assert_eq!(patched.cssom_rules().len(), 2);
}

/// Шелл пересобрал реестр после загрузки (`update_stylesheet_nodes`) — правки
/// `insertRule`, сделанные скриптом раньше, из `cssRules` не пропадают, пока
/// текст `<style>` тот же.
#[test]
fn shell_registry_push_keeps_earlier_insert_rule_edits() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = match rt
        .eval(
            "var t = document.createElement('style'); document.head.appendChild(t); \
             t.sheet.insertRule('.x { color: blue; }', 0); t.__nid__",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("{other:?}"),
    };
    rt.update_stylesheet_nodes(vec![lumen_css_parser::StylesheetNodeEntry {
        node: nid,
        sheet: Arc::new(lumen_css_parser::parse("")),
        disabled: false,
    }]);
    assert_eq!(s(&rt, "t.sheet.cssRules.length"), "1");
}
