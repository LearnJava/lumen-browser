//! Подресурсы документа: preload-подсказки, `<link>`-таблицы стилей,
//! `@import`, отпечаток инлайновых стилей, заголовок окна и клавиатурные
//! сокращения.

use super::*;

#[test]
fn dispatch_preload_hints_emits_events() {
    use lumen_core::event::SubresourceKind;
    use lumen_html_parser::PreloadHint;
    use std::sync::{Arc, Mutex};

    struct CollectingSink(Mutex<Vec<Event>>);
    impl EventSink for CollectingSink {
        fn emit(&self, e: &Event) {
            self.0.lock().unwrap().push(e.clone());
        }
    }

    let sink: Arc<dyn EventSink> =
        Arc::new(CollectingSink(Mutex::new(Vec::new())));
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let hints = vec![
        PreloadHint::Stylesheet { url: "reset.css".into(), media: None },
        PreloadHint::Script { url: "https://cdn.example.com/lib.js".into() },
    ];

    dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

    let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
    // SAFETY: `sink` was created two statements above as
    // `Arc::new(CollectingSink(..))` and never reassigned, so the erased
    // `dyn EventSink` really points at a `CollectingSink`; the pointer is
    // derived from a live `Arc` that outlives the borrow. A test-only
    // downcast вЂ” `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    assert_eq!(events.len(), 2);

    // CSS (High) СЃРѕСЂС‚РёСЂСѓРµС‚СЃСЏ РїРµСЂРµРґ JS (Medium) РЅРµР·Р°РІРёСЃРёРјРѕ РѕС‚ source-order
    let Event::SubresourceHintFound { url, kind, priority } = &events[0] else { panic!() };
    assert_eq!(url, "https://example.com/reset.css");
    assert_eq!(*kind, SubresourceKind::Stylesheet);
    assert_eq!(*priority, FetchPriority::High);

    let Event::SubresourceHintFound { url: url2, kind: kind2, priority: p2 } = &events[1] else { panic!() };
    assert_eq!(url2, "https://cdn.example.com/lib.js");
    assert_eq!(*kind2, SubresourceKind::Script);
    assert_eq!(*p2, FetchPriority::Medium);
}

#[test]
fn dispatch_preload_hints_deduplicates_same_url() {
    use lumen_html_parser::PreloadHint;
    use std::sync::{Arc, Mutex};

    struct CollectingSink(Mutex<Vec<Event>>);
    impl EventSink for CollectingSink {
        fn emit(&self, e: &Event) {
            self.0.lock().unwrap().push(e.clone());
        }
    }

    let sink: Arc<dyn EventSink> =
        Arc::new(CollectingSink(Mutex::new(Vec::new())));
    let base = ResourceBase::Url("https://example.com/".to_owned());
    // rel="preload stylesheet" СЃРѕР·РґР°С‘С‚ РґРІР° С…РёРЅС‚Р° РЅР° РѕРґРёРЅ href
    let hints = vec![
        PreloadHint::Preload { url: "style.css".into(), as_kind: Some("style".into()) },
        PreloadHint::Stylesheet { url: "style.css".into(), media: None },
        PreloadHint::Stylesheet { url: "other.css".into(), media: None },
    ];

    dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

    let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
    // SAFETY: `sink` was created two statements above as
    // `Arc::new(CollectingSink(..))` and never reassigned, so the erased
    // `dyn EventSink` really points at a `CollectingSink`; the pointer is
    // derived from a live `Arc` that outlives the borrow. A test-only
    // downcast вЂ” `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    // style.css РїРѕСЏРІР»СЏРµС‚СЃСЏ РґРІР°Р¶РґС‹ вЂ” РґРѕР»Р¶РµРЅ emit-РёС‚СЊСЃСЏ РѕРґРёРЅ СЂР°Р·
    assert_eq!(events.len(), 2, "expected 2 unique urls, got {}", events.len());
    let urls: Vec<_> = events.iter().map(|e| {
        let Event::SubresourceHintFound { url, .. } = e else { panic!() };
        url.as_str()
    }).collect();
    assert!(urls.contains(&"https://example.com/style.css"));
    assert!(urls.contains(&"https://example.com/other.css"));
}

#[test]
fn dispatch_preload_hints_cross_call_dedup() {
    // Р’С‚РѕСЂРѕР№ РІС‹Р·РѕРІ СЃ С‚РµРј Р¶Рµ seen-РЅР°Р±РѕСЂРѕРј РЅРµ РґРѕР»Р¶РµРЅ РїРѕРІС‚РѕСЂРЅРѕ СЌРјРёС‚РёС‚СЊ.
    use lumen_html_parser::PreloadHint;
    use std::sync::{Arc, Mutex};

    struct CollectingSink(Mutex<Vec<Event>>);
    impl EventSink for CollectingSink {
        fn emit(&self, e: &Event) { self.0.lock().unwrap().push(e.clone()); }
    }

    let sink: Arc<dyn EventSink> = Arc::new(CollectingSink(Mutex::new(Vec::new())));
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let mut seen = std::collections::HashSet::new();

    // РџРµСЂРІС‹Р№ РІС‹Р·РѕРІ вЂ” СЂР°РЅРЅРёР№ СЃРєР°РЅ (streaming chunk)
    let early = vec![PreloadHint::Stylesheet { url: "reset.css".into(), media: None }];
    dispatch_preload_hints(&early, &base, &sink, &mut seen);

    // Р’С‚РѕСЂРѕР№ РІС‹Р·РѕРІ вЂ” С„РёРЅР°Р»СЊРЅС‹Р№ pipeline: С‚Рµ Р¶Рµ С…РёРЅС‚С‹ + РЅРѕРІС‹Р№
    let full = vec![
        PreloadHint::Stylesheet { url: "reset.css".into(), media: None },
        PreloadHint::Image { url: Some("hero.png".into()), srcset: None, sizes: None },
    ];
    dispatch_preload_hints(&full, &base, &sink, &mut seen);

    let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
    // SAFETY: `sink` was created two statements above as
    // `Arc::new(CollectingSink(..))` and never reassigned, so the erased
    // `dyn EventSink` really points at a `CollectingSink`; the pointer is
    // derived from a live `Arc` that outlives the borrow. A test-only
    // downcast вЂ” `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    // reset.css вЂ” РѕРґРёРЅ СЂР°Р· (РёР· РїРµСЂРІРѕРіРѕ РІС‹Р·РѕРІР°), hero.png вЂ” РѕРґРёРЅ СЂР°Р· (РёР· РІС‚РѕСЂРѕРіРѕ)
    assert_eq!(events.len(), 2);
    let urls: Vec<_> = events.iter().map(|e| {
        let Event::SubresourceHintFound { url, .. } = e else { panic!() };
        url.as_str()
    }).collect();
    assert!(urls.contains(&"https://example.com/reset.css"));
    assert!(urls.contains(&"https://example.com/hero.png"));
}

#[test]
fn dispatch_preload_hints_sorts_by_priority() {
    use lumen_html_parser::PreloadHint;
    use std::sync::{Arc, Mutex};

    struct CollectingSink(Mutex<Vec<Event>>);
    impl EventSink for CollectingSink {
        fn emit(&self, e: &Event) { self.0.lock().unwrap().push(e.clone()); }
    }

    let sink: Arc<dyn EventSink> = Arc::new(CollectingSink(Mutex::new(Vec::new())));
    let base = ResourceBase::Url("https://example.com/".to_owned());
    // Source-order: img (Low) в†’ script (Medium) в†’ css (High)
    let hints = vec![
        PreloadHint::Image { url: Some("hero.png".into()), srcset: None, sizes: None },
        PreloadHint::Script { url: "app.js".into() },
        PreloadHint::Stylesheet { url: "main.css".into(), media: None },
    ];

    dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

    let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
    // SAFETY: `sink` was created two statements above as
    // `Arc::new(CollectingSink(..))` and never reassigned, so the erased
    // `dyn EventSink` really points at a `CollectingSink`; the pointer is
    // derived from a live `Arc` that outlives the borrow. A test-only
    // downcast вЂ” `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    assert_eq!(events.len(), 3);

    // РџРѕСЃР»Рµ СЃРѕСЂС‚РёСЂРѕРІРєРё: css(High) в†’ js(Medium) в†’ img(Low)
    let priorities: Vec<_> = events.iter().map(|e| {
        let Event::SubresourceHintFound { priority, .. } = e else { panic!() };
        *priority
    }).collect();
    assert_eq!(priorities, vec![FetchPriority::High, FetchPriority::Medium, FetchPriority::Low]);
}

#[test]
fn collect_link_hrefs_finds_stylesheet() {
    let doc = lumen_html_parser::parse(
        r#"<html><head><link rel="stylesheet" href="style.css"></head><body></body></html>"#,
    );
    let mut hrefs = Vec::new();
    collect_link_hrefs(&doc, doc.root(), &mut hrefs, &screen_media_context(Size::new(1024.0, 720.0), false));
    let only_hrefs: Vec<&str> = hrefs.iter().map(|(_, h)| h.as_str()).collect();
    assert_eq!(only_hrefs, vec!["style.css"]);
}

/// BUG-804: РёСЃС…РѕРґ РєР°Р¶РґРѕРіРѕ `<link rel=stylesheet>` РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ РїРѕ СѓР·Р»Р°Рј, РІ
/// РїРѕСЂСЏРґРєРµ РѕР±СЉСЏРІР»РµРЅРёСЏ, Рё РїСЂРѕРІР°Р» РЅРµ РІС‹РїР°РґР°РµС‚ РёР· СЃРїРёСЃРєР° вЂ” РёРЅР°С‡Рµ СЌР»РµРјРµРЅС‚Сѓ
/// РЅРµРіРґРµ РІС‹СЃС‚СЂРµР»РёС‚СЊ `error`. `samples/` Р·Р°РІРµРґРѕРјРѕ РЅРµ СЃРѕРґРµСЂР¶РёС‚ СЌС‚РёС… С„Р°Р№Р»РѕРІ,
/// С‚Р°Рє С‡С‚Рѕ РѕР±Р° Р»РёСЃС‚Р° С‚СѓС‚ В«РЅРµ РїСЂРёС€Р»РёВ»; РїСЂРѕРІРµСЂСЏРµС‚СЃСЏ СЃРІСЏР·СЊ СѓР·РµР»в†”РёСЃС…РѕРґ, Р° РЅРµ
/// СЃРµС‚СЊ.
#[test]
fn load_linked_stylesheets_reports_one_outcome_per_element() {
    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }
    let doc = lumen_html_parser::parse(
        r#"<html><head>
                 <link rel="stylesheet" href="b804-no-such-a.css">
                 <link rel="alternate" href="ignored.css">
                 <link rel="stylesheet" href="b804-no-such-b.css">
               </head><body></body></html>"#,
    );
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let ctx = screen_media_context(Size::new(1024.0, 720.0), false);
    let (css, outcomes) = load_linked_stylesheets(&doc, &base, &sink, None, &ctx);
    assert!(css.is_empty(), "neither sheet exists on disk");
    assert_eq!(outcomes.len(), 2, "rel=alternate is not a cascade sheet");
    assert!(outcomes.iter().all(|(_, ok)| !ok));
    // Р Р°Р·РЅС‹Рµ СЌР»РµРјРµРЅС‚С‹ вЂ” СЂР°Р·РЅС‹Рµ СѓР·Р»С‹: Р±РµР· СЌС‚РѕРіРѕ СЃС‚СЂР°РЅРёС†Р° РЅРµ СЃРјРѕРіР»Р° Р±С‹
    // РѕС‚Р»РёС‡РёС‚СЊ, РєР°РєРѕР№ РёРјРµРЅРЅРѕ `<link>` РѕС‚С‡РёС‚Р°Р»СЃСЏ.
    assert_ne!(outcomes[0].0, outcomes[1].0);
}

/// BUG-480 СЃСЂРµР· 11: РїСЂРѕС…РѕРґ РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р° С„СЂРµР№РјР° Р·Р°РїСЂР°С€РёРІР°РµС‚
/// `<link rel=stylesheet>` (РѕР±Р° РёСЃС…РѕРґР°) Рё `<img>` (РѕР±Р° РёСЃС…РѕРґР°), РЅРµ С‚СЂРѕРіР°РµС‚
/// `rel=alternate`/`loading="lazy"` Рё РІРѕР·РІСЂР°С‰Р°РµС‚ РёСЃС…РѕРґС‹ РІ РїРѕСЂСЏРґРєРµ DOM.
#[test]
fn frame_subresources_fetch_links_and_imgs_with_outcomes() {
    let dir = std::env::temp_dir().join("lumen_frame_subresources_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ok.css"), "p { color: red; }").unwrap();
    // Р”Р»СЏ file-Р±Р°Р·С‹ `fetch_image_bytes` С‡РёС‚Р°РµС‚ Р±Р°Р№С‚С‹ СЃ РґРёСЃРєР° Р±РµР· РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ:
    // СЃРѕРґРµСЂР¶РёРјРѕРµ РЅРµ РІР°Р¶РЅРѕ, РІР°Р¶РµРЅ С„Р°РєС‚ С‡С‚РµРЅРёСЏ.
    std::fs::write(dir.join("ok.png"), "bytes").unwrap();
    // missing.css / missing.png РЅРµ СЃРѕР·РґР°СЋС‚СЃСЏ.

    let doc = lumen_html_parser::parse(&format!(
        r#"<html><head>
                 <link rel="stylesheet" href="{}/ok.css">
                 <link rel="alternate" href="{}/ignored.css">
                 <link rel="stylesheet" href="{}/missing.css">
               </head><body>
                 <img src="{}/ok.png">
                 <img src="{}/missing.png">
                 <img loading="lazy" src="{}/lazy.png">
               </body></html>"#,
        dir.display(),
        dir.display(),
        dir.display(),
        dir.display(),
        dir.display(),
        dir.display(),
    ));
    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }
    let base = ResourceBase::File(dir.join("index.html"));
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let out = fetch_frame_subresources(
        &doc,
        &base,
        &sink,
        None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        Size::new(1024.0, 720.0),
    );

    assert_eq!(out.links.len(), 2, "rel=alternate is not a cascade sheet");
    assert!(out.links[0].1, "ok.css exists");
    assert!(!out.links[1].1, "missing.css must report error");
    assert_ne!(out.links[0].0, out.links[1].0);

    assert_eq!(out.images.len(), 2, "loading=lazy is not requested at all");
    assert!(out.images[0].1, "ok.png exists");
    assert!(!out.images[1].1, "missing.png must report error");
}

// в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ @import file loading (CSS Cascade L4 В§6.5) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// РЎРѕР·РґР°С‘С‚ СѓРЅРёРєР°Р»СЊРЅСѓСЋ РІСЂРµРјРµРЅРЅСѓСЋ РґРёСЂРµРєС‚РѕСЂРёСЋ РґР»СЏ CSS-С„РёРєСЃС‚СѓСЂ `@import`-С‚РµСЃС‚Р°,
/// РѕС‡РёС‰Р°СЏ РїСЂРѕС€Р»С‹Р№ РїСЂРѕРіРѕРЅ. Р’РѕР·РІСЂР°С‰Р°РµС‚ РїСѓС‚СЊ РґРёСЂРµРєС‚РѕСЂРёРё.
fn import_fixture_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lumen_import_test_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn null_sink() -> Arc<dyn EventSink> {
    Arc::new(StdoutEventSink)
}

/// `@import` РїСЂРµРґРїРѕСЃС‹Р»Р°РµС‚ СЃРѕРґРµСЂР¶РёРјРѕРµ РёРјРїРѕСЂС‚РёСЂРѕРІР°РЅРЅРѕРіРѕ Р»РёСЃС‚Р° СЃРѕР±СЃС‚РІРµРЅРЅС‹Рј
/// РїСЂР°РІРёР»Р°Рј РёРјРїРѕСЂС‚РёСЂСѓСЋС‰РµРіРѕ (РёРјРїРѕСЂС‚ В«СЂР°РЅСЊС€РµВ» РІ РєР°СЃРєР°РґРµ).
#[test]
fn inline_css_imports_prepends_imported_content() {
    let dir = import_fixture_dir("prepend");
    std::fs::write(dir.join("b.css"), "b { color: blue; }").unwrap();
    let entry = dir.join("a.css");
    let base = ResourceBase::File(entry.clone());
    let text = "@import url(\"b.css\");\na { color: red; }";
    let out = inline_css_imports(
        text, &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    let b_pos = out.find("color: blue").expect("imported content present");
    let a_pos = out.find("color: red").expect("own content present");
    assert!(b_pos < a_pos, "imported rules must precede importing sheet's own rules");
}

/// BUG-743: СЂРµР·СѓР»СЊС‚Р°С‚ РІСЃРµРіРґР° **РѕРєР°РЅС‡РёРІР°РµС‚СЃСЏ** РёСЃС…РѕРґРЅС‹Рј С‚РµРєСЃС‚РѕРј Р»РёСЃС‚Р° вЂ”
/// РЅР° СЌС‚РѕРј РёРЅРІР°СЂРёР°РЅС‚Рµ РґРµСЂР¶РёС‚СЃСЏ РІС‹СЂРµР·Р°РЅРёРµ `imports_prefix` РІ
/// `parse_and_layout` (РїСЂРµС„РёРєСЃ = РІСЃС‘, С‡С‚Рѕ РґР»РёРЅРЅРµРµ РёСЃС…РѕРґРЅРѕРіРѕ С‚РµРєСЃС‚Р°).
#[test]
fn inline_css_imports_result_ends_with_source_text() {
    let dir = import_fixture_dir("suffix");
    std::fs::write(dir.join("b.css"), "b { color: blue; }").unwrap();
    let base = ResourceBase::File(dir.join("a.css"));
    let ctx = screen_media_context(Size::new(1024.0, 720.0), false);
    for text in ["@import url(\"b.css\");\na { color: red; }", "a { color: red; }", ""] {
        let out = inline_css_imports(
            text, &base, &null_sink(), None, &ctx,
            &mut std::collections::HashSet::new(), 0,
        );
        assert!(out.ends_with(text), "СЂРµР·СѓР»СЊС‚Р°С‚ РЅРµ РѕРєР°РЅС‡РёРІР°РµС‚СЃСЏ РёСЃС…РѕРґРЅРёРєРѕРј: {out:?}");
    }
}

// в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ BUG-743: РѕС‚РїРµС‡Р°С‚РѕРє РёРЅР»Р°Р№РЅРѕРІС‹С… <style> в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// Р’СЃС‚Р°РІРєР° РЅРѕРІРѕРіРѕ `<style>` РјРµРЅСЏРµС‚ РѕС‚РїРµС‡Р°С‚РѕРє вЂ” РёРЅР°С‡Рµ РїРѕР·РґРЅРёР№ РґРёРЅР°РјРёС‡РµСЃРєРёР№
/// Р»РёСЃС‚ РЅРµ РїРµСЂРµСЃРѕР±РµСЂС‘С‚ РєР°СЃРєР°Рґ.
#[test]
fn inline_style_fingerprint_detects_added_block() {
    let a = lumen_html_parser::parse("<html><head><style>.a{color:red}</style></head><body></body></html>");
    let b = lumen_html_parser::parse(
        "<html><head><style>.a{color:red}</style><style>.b{color:blue}</style></head><body></body></html>",
    );
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// РџСЂР°РІРєР° С‚РµРєСЃС‚Р° Р±Р»РѕРєР° Р±РµР· РёР·РјРµРЅРµРЅРёСЏ РґР»РёРЅС‹ С‚РѕР¶Рµ РјРµРЅСЏРµС‚ РѕС‚РїРµС‡Р°С‚РѕРє вЂ”
/// СЃС‡С‘С‚С‡РёРєР° Р±Р»РѕРєРѕРІ РёР»Рё СЃСѓРјРјР°СЂРЅРѕР№ РґР»РёРЅС‹ Р±С‹Р»Рѕ Р±С‹ РЅРµРґРѕСЃС‚Р°С‚РѕС‡РЅРѕ.
#[test]
fn inline_style_fingerprint_detects_same_length_edit() {
    let a = lumen_html_parser::parse("<html><head><style>.a{color:red}</style></head></html>");
    let b = lumen_html_parser::parse("<html><head><style>.a{color:RED}</style></head></html>");
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// РћРґРёРЅ Рё С‚РѕС‚ Р¶Рµ РґРѕРєСѓРјРµРЅС‚ РґР°С‘С‚ РѕРґРёРЅ Рё С‚РѕС‚ Р¶Рµ РѕС‚РїРµС‡Р°С‚РѕРє, Р° РїРµСЂРµСЃС‚Р°РЅРѕРІРєР°
/// С‚РµРєСЃС‚Р° РјРµР¶РґСѓ РґРІСѓРјСЏ Р±Р»РѕРєР°РјРё вЂ” СЂР°Р·РЅС‹Р№ (РіСЂР°РЅРёС†С‹ Р±Р»РѕРєРѕРІ СѓС‡РёС‚С‹РІР°СЋС‚СЃСЏ).
#[test]
fn inline_style_fingerprint_is_stable_and_block_aware() {
    let src = "<html><head><style>.a{}</style><style>.b{}</style></head></html>";
    let a = lumen_html_parser::parse(src);
    let b = lumen_html_parser::parse(src);
    assert_eq!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
    let merged = lumen_html_parser::parse("<html><head><style>.a{}.b{}</style></head></html>");
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&merged));
}

/// Р”РѕРєСѓРјРµРЅС‚ Р±РµР· РµРґРёРЅРѕРіРѕ `<style>` вЂ” РѕС‚РїРµС‡Р°С‚РѕРє СЃС‡РёС‚Р°РµС‚СЃСЏ Рё РЅРµ РїР°РЅРёРєСѓРµС‚.
#[test]
fn inline_style_fingerprint_handles_document_without_styles() {
    let a = lumen_html_parser::parse("<html><body><p>С‚РµРєСЃС‚</p></body></html>");
    let b = lumen_html_parser::parse("<html><body><p>РґСЂСѓРіРѕР№</p></body></html>");
    assert_eq!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// Р’Р»РѕР¶РµРЅРЅС‹Рµ `@import` (a в†’ b в†’ c) СЂР°Р·РІРѕСЂР°С‡РёРІР°СЋС‚СЃСЏ РІ РїРѕСЂСЏРґРєРµ c, b, a.
#[test]
fn inline_css_imports_nested_order() {
    let dir = import_fixture_dir("nested");
    std::fs::write(dir.join("c.css"), ".c{}").unwrap();
    std::fs::write(dir.join("b.css"), "@import url(c.css);\n.b{}").unwrap();
    let base = ResourceBase::File(dir.join("a.css"));
    let out = inline_css_imports(
        "@import url(b.css);\n.a{}", &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    let c = out.find(".c{}").unwrap();
    let b = out.find(".b{}").unwrap();
    let a = out.find(".a{}").unwrap();
    assert!(c < b && b < a, "expected c < b < a, got c={c} b={b} a={a}");
}

/// Р¦РёРєР»РёС‡РµСЃРєРёР№ `@import` (a в†’ b в†’ a) Р·Р°РІРµСЂС€Р°РµС‚СЃСЏ Р±РµР· Р±РµСЃРєРѕРЅРµС‡РЅРѕР№ СЂРµРєСѓСЂСЃРёРё.
#[test]
fn inline_css_imports_cycle_guard() {
    let dir = import_fixture_dir("cycle");
    std::fs::write(dir.join("a.css"), "@import url(b.css);\n.a{}").unwrap();
    std::fs::write(dir.join("b.css"), "@import url(a.css);\n.b{}").unwrap();
    let base = ResourceBase::File(dir.join("a.css"));
    // РќР°С‡РёРЅР°РµРј СЃ СЃРѕРґРµСЂР¶РёРјРѕРіРѕ a.css вЂ” С‚РѕС‚ Р¶Рµ С„Р°Р№Р» Р±СѓРґРµС‚ РёРјРїРѕСЂС‚РёСЂРѕРІР°РЅ РёР· b.
    let out = inline_css_imports(
        "@import url(b.css);\n.a{}", &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    // РљР°Р¶РґС‹Р№ Р»РёСЃС‚ Р·Р°РіСЂСѓР¶РµРЅ РјР°РєСЃРёРјСѓРј РѕРґРёРЅ СЂР°Р· (guard РїРѕ `seen`).
    assert_eq!(out.matches(".b{}").count(), 1);
}

/// `@import url(x) print;` РЅРµ Р·Р°РіСЂСѓР¶Р°РµС‚СЃСЏ РїРѕРґ СЌРєСЂР°РЅРЅС‹Рј РєРѕРЅС‚РµРєСЃС‚РѕРј.
#[test]
fn inline_css_imports_media_gate() {
    let dir = import_fixture_dir("media");
    std::fs::write(dir.join("p.css"), ".print-only{}").unwrap();
    let base = ResourceBase::File(dir.join("a.css"));
    let out = inline_css_imports(
        "@import url(p.css) print;\n.a{}", &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    assert!(!out.contains(".print-only{}"), "print-only @import must be skipped for screen");
    assert!(out.contains(".a{}"));
}

/// РћС‚СЃСѓС‚СЃС‚РІСѓСЋС‰РёР№ РёРјРїРѕСЂС‚РёСЂСѓРµРјС‹Р№ С„Р°Р№Р» РЅРµ РІР°Р»РёС‚ СЂРµРЅРґРµСЂ вЂ” С‚РµРєСЃС‚ РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ,
/// СЃРѕР±СЃС‚РІРµРЅРЅС‹Рµ РїСЂР°РІРёР»Р° СЃРѕС…СЂР°РЅРµРЅС‹.
#[test]
fn inline_css_imports_missing_file_is_skipped() {
    let dir = import_fixture_dir("missing");
    let base = ResourceBase::File(dir.join("a.css"));
    let out = inline_css_imports(
        "@import url(nope.css);\n.a{}", &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    assert!(out.contains(".a{}"));
}

/// РўРµРєСЃС‚ Р±РµР· `@import` РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ Р±РµР· РёР·РјРµРЅРµРЅРёР№ (Р±С‹СЃС‚СЂС‹Р№ РїСѓС‚СЊ).
#[test]
fn inline_css_imports_no_import_passthrough() {
    let base = ResourceBase::File(std::path::PathBuf::from("x/a.css"));
    let text = ".a { color: red; }";
    let out = inline_css_imports(
        text, &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    assert_eq!(out, text);
}

#[test]
fn contains_ignore_ascii_case_matches() {
    assert!(contains_ignore_ascii_case(b"body { } @IMPORT url(x);", b"@import"));
    assert!(contains_ignore_ascii_case(b"@import", b"@import"));
    assert!(!contains_ignore_ascii_case(b"body { color: red }", b"@import"));
    assert!(!contains_ignore_ascii_case(b"@imp", b"@import"));
}

/// BUG-268: `<link rel=stylesheet media=print>` must NOT enter the screen
/// cascade, while `media=screen`/`all`/matching `@media`-features do.
#[test]
fn collect_link_hrefs_media_gate() {
    let doc = lumen_html_parser::parse(
        r#"<html><head>
                <link rel="stylesheet" media="print" href="print.css">
                <link rel="stylesheet" media="screen" href="screen.css">
                <link rel="stylesheet" media="all" href="all.css">
                <link rel="stylesheet" href="plain.css">
                <link rel="stylesheet" media="(min-width: 500px)" href="wide.css">
                <link rel="stylesheet" media="(min-width: 5000px)" href="huge.css">
            </head><body></body></html>"#,
    );
    let mut hrefs = Vec::new();
    collect_link_hrefs(&doc, doc.root(), &mut hrefs, &screen_media_context(Size::new(1024.0, 720.0), false));
    // print.css РѕС‚СЃРµСЏРЅ; huge.css РѕС‚СЃРµСЏРЅ (viewport 1024px < 5000px); РѕСЃС‚Р°Р»СЊРЅС‹Рµ вЂ” РґР°.
    let only_hrefs: Vec<&str> = hrefs.iter().map(|(_, h)| h.as_str()).collect();
    assert_eq!(only_hrefs, vec!["screen.css", "all.css", "plain.css", "wide.css"]);
}

#[test]
fn collect_link_hrefs_ignores_non_stylesheet() {
    let doc = lumen_html_parser::parse(
        r#"<html><head><link rel="icon" href="favicon.ico"></head><body></body></html>"#,
    );
    let mut hrefs = Vec::new();
    collect_link_hrefs(&doc, doc.root(), &mut hrefs, &screen_media_context(Size::new(1024.0, 720.0), false));
    assert!(hrefs.is_empty());
}

#[test]
fn extract_title_basic() {
    let doc = lumen_html_parser::parse(
        r#"<html><head><title>Hello</title></head><body></body></html>"#,
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("Hello"));
}

#[test]
fn extract_title_cyrillic_and_entities() {
    // RCDATA-СЂРµР¶РёРј РґРµРєРѕРґРёСЂСѓРµС‚ &amp; в†’ '&' РїСЂСЏРјРѕ РІ tokenizer-Рµ.
    let doc = lumen_html_parser::parse(
        r#"<html><head><title>Р”РѕРј &amp; РЎР°Рґ</title></head><body></body></html>"#,
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("Р”РѕРј & РЎР°Рґ"));
}

#[test]
fn extract_title_collapses_whitespace() {
    let doc = lumen_html_parser::parse(
        "<html><head><title>  foo\n\t  bar  </title></head><body></body></html>",
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("foo bar"));
}

#[test]
fn extract_title_missing_is_none() {
    let doc = lumen_html_parser::parse("<html><body><p>x</p></body></html>");
    assert!(extract_title(&doc).is_none());
}

#[test]
fn extract_title_empty_is_none() {
    let doc = lumen_html_parser::parse(
        "<html><head><title>   </title></head><body></body></html>",
    );
    assert!(extract_title(&doc).is_none());
}

#[test]
fn extract_title_first_wins() {
    // Lenient: РµСЃР»Рё СЃС‚СЂР°РЅРёС†Р° РѕР±СЉСЏРІРёР»Р° <title> РґРІР°Р¶РґС‹, Р±РµСЂС‘Рј РїРµСЂРІС‹Р№.
    let doc = lumen_html_parser::parse(
        "<html><head><title>A</title><title>B</title></head><body></body></html>",
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("A"));
}

#[test]
fn window_title_with_page() {
    assert_eq!(window_title(Some("Foo")), "Foo вЂ” Lumen");
}

#[test]
fn window_title_fallback() {
    // Fallback СЃРѕРґРµСЂР¶РёС‚ РІРµСЂСЃРёСЋ РїР°РєРµС‚Р° вЂ” РїСЂРѕРІРµСЂСЏРµРј РїСЂРµС„РёРєСЃ.
    let t = window_title(None);
    assert!(t.starts_with("Lumen "));
}

#[test]
fn keybinding_f5_reload() {
    assert_eq!(
        keybinding_for(KeyCode::F5, ModifiersState::empty()),
        Some(KeyCommand::Reload),
    );
}

#[test]
fn keybinding_ctrl_r_reload() {
    assert_eq!(
        keybinding_for(KeyCode::KeyR, ModifiersState::CONTROL),
        Some(KeyCommand::Reload),
    );
}

#[test]
fn keybinding_plain_r_is_none() {
    // Р‘РµР· Ctrl вЂ” РѕР±С‹С‡РЅР°СЏ Р±СѓРєРІР°, РЅРµ РєРѕРјР°РЅРґР°. Р—Р°С‰РёС‚Р° РѕС‚ РїРµСЂРµС…РІР°С‚Р° РІРІРѕРґР°
    // РІ РѕРјРЅРёР±РѕРєСЃ (РєРѕРіРґР° РѕРЅ РїРѕСЏРІРёС‚СЃСЏ).
    assert_eq!(keybinding_for(KeyCode::KeyR, ModifiersState::empty()), None);
}

#[test]
fn keybinding_ctrl_shift_r_is_read_later() {
    // Ctrl+Shift+R в†’ toggle Read-later panel (В§12.3).
    assert_eq!(
        keybinding_for(KeyCode::KeyR, ModifiersState::CONTROL | ModifiersState::SHIFT),
        Some(KeyCommand::ToggleReadLater),
    );
}

#[test]
fn keybinding_escape_exit() {
    assert_eq!(
        keybinding_for(KeyCode::Escape, ModifiersState::empty()),
        Some(KeyCommand::Exit),
    );
}

#[test]
fn keybinding_ctrl_w_close_tab() {
    assert_eq!(
        keybinding_for(KeyCode::KeyW, ModifiersState::CONTROL),
        Some(KeyCommand::CloseTab),
    );
}

#[test]
fn keybinding_ctrl_escape_is_none() {
    // Esc + Р»СЋР±С‹Рµ РјРѕРґРёС„РёРєР°С‚РѕСЂС‹ вЂ” РЅРµ РЅР°С€Р° РєРѕРјР°РЅРґР° (СЂР°РјРї РґР»СЏ Р±СѓРґСѓС‰РµРіРѕ).
    assert_eq!(
        keybinding_for(KeyCode::Escape, ModifiersState::CONTROL),
        None,
    );
}

#[test]
fn keybinding_unknown_key_is_none() {
    assert_eq!(keybinding_for(KeyCode::KeyA, ModifiersState::empty()), None);
    assert_eq!(keybinding_for(KeyCode::F1, ModifiersState::empty()), None);
}

#[test]
fn keybinding_ctrl_f_opens_find() {
    assert_eq!(
        keybinding_for(KeyCode::KeyF, ModifiersState::CONTROL),
        Some(KeyCommand::FindOpen),
    );
}

#[test]
fn keybinding_ctrl_l_opens_address_bar() {
    assert_eq!(
        keybinding_for(KeyCode::KeyL, ModifiersState::CONTROL),
        Some(KeyCommand::OpenAddressBar),
    );
}

#[test]
fn keybinding_f6_opens_address_bar() {
    assert_eq!(
        keybinding_for(KeyCode::F6, ModifiersState::empty()),
        Some(KeyCommand::OpenAddressBar),
    );
}

#[test]
fn keybinding_plain_f_opens_hints() {
    // F Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ РѕС‚РєСЂС‹РІР°РµС‚ hint-СЂРµР¶РёРј kbd-РЅР°РІРёРіР°С†РёРё.
    assert_eq!(
        keybinding_for(KeyCode::KeyF, ModifiersState::empty()),
        Some(KeyCommand::HintModeOpen)
    );
}
