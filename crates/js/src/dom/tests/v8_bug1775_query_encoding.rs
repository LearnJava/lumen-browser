//! BUG-1775 — query кодируется кодировкой ДОКУМЕНТА при резолве URL-атрибутов
//! (HTML LS §2.1 «encoding parse a URL»). После BUG-1713 (`99c91124b`)
//! `document.characterSet` стал честно отвечать `windows-1252`, но резолв
//! остался чисто UTF-8 — и WPT `query-encoding/attributes.sub.html?…`
//! схлопнулся с 17/26 до 2/26. Эти тесты пинуют три звена фикса: отражение
//! URL-атрибутов (`_lumen_reflect_url` → `_url_resolve_doc`), одинокие
//! суррогаты (numeric character reference) и computed `background-image`
//! presentational-атрибута `background`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// Runtime с живым документом кодировки `charset` и базой
/// `https://example.com/dir/page.html` — той же формой, что
/// `v8_bug552_document_metadata::runtime_with`, плюс фоновый stylesheet и
/// viewport, чтобы read-time flush `getComputedStyle` пересобрал каскад.
fn runtime_charset(charset: &str) -> V8JsRuntime {
    let doc = make_doc();
    doc.lock().unwrap().set_character_set(charset.to_string());
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(
        doc,
        "https://example.com/dir/page.html",
        None, None, None, None, None, None, None, None, None, false, None,
    )
    .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body { margin: 0; }")));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

fn text_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

/// `Getting <a> href` из attributes.sub.html: `?ÿ` отражается как
/// `?%FF`/`?%26%23255%3B`/`?%C3%BF` по кодировке документа.
#[test]
fn url_attribute_reflects_query_in_document_encoding() {
    let rt = runtime_charset("windows-1252");
    assert_eq!(
        text_of(&rt, "(function(){ var a = document.createElement('a'); a.setAttribute('href', '?ÿ'); return a.href; })()"),
        "https://example.com/dir/page.html?%FF",
    );
    let rt = runtime_charset("windows-1251");
    assert_eq!(
        text_of(&rt, "(function(){ var a = document.createElement('a'); a.setAttribute('href', '?ÿ'); return a.href; })()"),
        "https://example.com/dir/page.html?%26%23255%3B",
    );
    // UTF-8 документ — `new URL()`-семантика, ничего не поменялось.
    let rt = runtime_charset("utf-8");
    assert_eq!(
        text_of(&rt, "(function(){ var a = document.createElement('a'); a.setAttribute('href', '?ÿ'); return a.href; })()"),
        "https://example.com/dir/page.html?%C3%BF",
    );
}

/// `Getting <base> href`: резолвится против document URL (не определяемого им
/// самим base URL) и тоже кодируется кодировкой документа — отдельный геттер,
/// не строка `url` в таблице отражений.
#[test]
fn base_href_reflects_query_in_document_encoding() {
    let rt = runtime_charset("windows-1252");
    assert_eq!(
        text_of(&rt, "(function(){ var b = document.createElement('base'); b.setAttribute('href', '?ÿ'); return b.href; })()"),
        "https://example.com/dir/page.html?%FF",
    );
}

/// lone-surrogates.sub.html: одинокий суррогат доходит до атрибута как U+FFFD и
/// в windows-1252 идёт numeric character reference `&#65533;`.
#[test]
fn lone_surrogate_query_goes_through_numeric_charref() {
    let rt = runtime_charset("windows-1252");
    assert_eq!(
        text_of(&rt, "(function(){ var a = document.createElement('a'); a.setAttribute('href', '?\\uD800'); return a.href; })()"),
        "https://example.com/dir/page.html?%26%2365533%3B",
    );
}

/// `getComputedStyle <body> background` из attributes.sub.html: computed
/// `background-image` сериализует РАЗРЕШЁННЫЙ url с query в кодировке
/// документа (`url("…?%FF")`), а не сырую строку атрибута. Как и WPT,
/// элемент вставляется в документ — каскад считает стили только дерева.
#[test]
fn computed_background_image_resolves_url_in_document_encoding() {
    let rt = runtime_charset("windows-1252");
    assert_eq!(
        text_of(&rt, "(function(){ var b = document.createElement('body'); document.body.appendChild(b); b.setAttribute('background', '?ÿ'); return getComputedStyle(b).backgroundImage; })()"),
        "url(\"https://example.com/dir/page.html?%FF\")",
    );
    // В итерируемом виде (entries) то же значение — read и итерация не
    // расходятся.
    assert_eq!(
        text_of(&rt, "(function(){ var b = document.createElement('td'); document.body.appendChild(b); b.setAttribute('background', '?ÿ'); return getComputedStyle(b).getPropertyValue('background-image'); })()"),
        "url(\"https://example.com/dir/page.html?%FF\")",
    );
}

/// `new URL()` — единственный, кто НЕ наследует кодировку документа: конструктор
/// по спецификации всегда UTF-8.
#[test]
fn url_constructor_stays_utf8() {
    let rt = runtime_charset("windows-1252");
    assert_eq!(
        text_of(&rt, "new URL('?ÿ', location.href).href"),
        "https://example.com/dir/page.html?%C3%BF",
    );
}
