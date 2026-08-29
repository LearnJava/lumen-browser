//! Конвейер страницы: пригодность к bfcache, разрешение JS-навигации,
//! вьюпорт контента, размеры картинок, маршрутизация в движковый поток,
//! полноэкранный опрос и состояние `content-visibility`.

use super::*;

// в”Ђв”Ђ Ph3 P3-bfcache: Cache-Control: no-store eligibility filter в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn cache_control_no_store_detects_directive() {
    let headers = vec![("Cache-Control".to_owned(), "no-store".to_owned())];
    assert!(cache_control_no_store(&headers));
}

#[test]
fn cache_control_no_store_case_insensitive_header_name() {
    let headers = vec![("cache-control".to_owned(), "no-store, max-age=0".to_owned())];
    assert!(cache_control_no_store(&headers));
}

#[test]
fn cache_control_no_store_false_when_absent() {
    let headers = vec![("Cache-Control".to_owned(), "max-age=3600, public".to_owned())];
    assert!(!cache_control_no_store(&headers));
}

#[test]
fn cache_control_no_store_false_when_header_missing() {
    let headers = vec![("Content-Type".to_owned(), "text/html".to_owned())];
    assert!(!cache_control_no_store(&headers));
}

// в”Ђв”Ђ BUG-293: JS-navigation URL resolution (window.open / location.href) в”Ђв”Ђ

#[test]
fn resolve_js_nav_file_from_local_page_loads_from_disk() {
    // fileв†’file: a local page opening a local file resolves to PageSource::File.
    let opener = PageSource::File(PathBuf::from("/home/x/index.html"));
    let src = resolve_js_navigation("file:///home/x/page.html", &opener)
        .expect("fileв†’file must be allowed");
    match src {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("/home/x/page.html")),
        other => panic!("expected PageSource::File, got {other:?}"),
    }
}

#[test]
fn resolve_js_nav_file_strips_windows_drive_slash() {
    // file:///D:/вЂ¦ в†’ D:/вЂ¦ (leading slash before the drive letter dropped).
    let opener = PageSource::File(PathBuf::from("D:/proj/index.html"));
    let src = resolve_js_navigation("file:///D:/proj/page.html", &opener)
        .expect("fileв†’file must be allowed");
    match src {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("D:/proj/page.html")),
        other => panic!("expected PageSource::File, got {other:?}"),
    }
}

#[test]
fn resolve_js_nav_web_to_file_is_blocked() {
    // webв†’file: an http(s) page must not open a local file:// resource.
    let opener = PageSource::Url("https://example.com/".to_owned());
    let err = resolve_js_navigation("file:///etc/passwd", &opener)
        .expect_err("webв†’file must be blocked");
    assert!(err.contains("РїРѕР»РёС‚РёРєРѕР№ Р±РµР·РѕРїР°СЃРЅРѕСЃС‚Рё"), "reason: {err}");
}

#[test]
fn resolve_js_nav_http_url_untouched() {
    // Non-file URLs keep the existing PageSource::Url path regardless of opener.
    let opener = PageSource::Url("https://example.com/".to_owned());
    let src = resolve_js_navigation("https://example.org/next", &opener)
        .expect("http navigation stays on the network path");
    match src {
        PageSource::Url(u) => assert_eq!(u, "https://example.org/next"),
        other => panic!("expected PageSource::Url, got {other:?}"),
    }
}

#[test]
fn resolve_js_nav_file_from_about_blank_allowed() {
    // A non-web opener (about:blank / Empty) may open a file:// resource.
    let src = resolve_js_navigation("file:///home/x/page.html", &PageSource::AboutBlank)
        .expect("non-web opener в†’ file is allowed");
    assert!(matches!(src, PageSource::File(_)));
}

// в”Ђв”Ђ RP-2: live layout viewport tracks window size, minus chrome в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn content_layout_viewport_subtracts_tab_strip() {
    // Interactive window at 1280Г—800 в†’ page content area excludes the tab
    // strip + toolbar (toolbar::CHROME_H) but keeps the full width.
    let (w, h) = content_layout_viewport(Size::new(1280.0, 800.0), true, false);
    assert!((w - 1280.0).abs() < 1e-3);
    assert!((h - (800.0 - toolbar::CHROME_H)).abs() < 1e-3);
}

#[test]
fn content_layout_viewport_tracks_resized_window() {
    // After a resize the surface (= r.viewport_size()) follows inner_size;
    // the content height follows it too (no hardcoded 720).
    let (w, h) = content_layout_viewport(Size::new(640.0, 480.0), true, false);
    assert!((w - 640.0).abs() < 1e-3);
    assert!((h - (480.0 - toolbar::CHROME_H)).abs() < 1e-3);
}

#[test]
fn content_layout_viewport_default_window_yields_720() {
    // The interactive window opens at 1024 Г— (720 + toolbar::CHROME_H) so the
    // page gets exactly 720 CSS px, as graphic tests expect.
    let surface = Size::new(1024.0, 720.0 + toolbar::CHROME_H);
    let (w, h) = content_layout_viewport(surface, true, false);
    assert!((w - 1024.0).abs() < 1e-3);
    assert!((h - 720.0).abs() < 1e-3);
}

#[test]
fn content_layout_viewport_subtracts_workspace_switcher() {
    // With the workspace switcher visible the page loses both bars.
    let surface = Size::new(1024.0, 800.0);
    let (_w, h) = content_layout_viewport(surface, true, true);
    let expected =
        800.0 - toolbar::CHROME_H - panels::workspace_panel::SWITCHER_HEIGHT;
    assert!((h - expected).abs() < 1e-3);
}

#[test]
fn content_layout_viewport_headless_uses_full_surface() {
    // Headless (--screenshot/--dump/--ipc): no chrome в†’ full surface,
    // keeping those paths deterministic at 1024Г—720.
    let (w, h) = content_layout_viewport(Size::new(1024.0, 720.0), false, false);
    assert!((w - 1024.0).abs() < 1e-3);
    assert!((h - 720.0).abs() < 1e-3);
}

#[test]
fn content_layout_viewport_clamps_tiny_window_to_zero() {
    // A window shorter than the chrome must not yield a negative height.
    let (_w, h) = content_layout_viewport(Size::new(800.0, 10.0), true, false);
    assert!(h >= 0.0);
}

// в”Ђв”Ђ BUG-269: replaced-element intrinsic aspect-ratio sizing в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// Depth-first search for the first `<img>` node in a parsed document.
fn find_img(doc: &Document, id: NodeId) -> Option<NodeId> {
    if let NodeData::Element { name, .. } = &doc.get(id).data
        && name.local == "img"
    {
        return Some(id);
    }
    for &child in &doc.get(id).children {
        if let Some(found) = find_img(doc, child) {
            return Some(found);
        }
    }
    None
}

/// Parse `html`, run `apply_intrinsic_size` on its `<img>` with the given
/// intrinsic size, and return the resulting `(width, height)` attributes.
fn img_dims(html: &str, iw: u32, ih: u32) -> (Option<String>, Option<String>) {
    let mut doc = lumen_html_parser::parse(html);
    let img = find_img(&doc, doc.root()).expect("img present");
    apply_intrinsic_size(&mut doc, img, iw, ih);
    let NodeData::Element { attrs, .. } = &doc.get(img).data else {
        unreachable!()
    };
    let pick = |name: &str| {
        attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
            .map(|a| a.value.to_string())
    };
    (pick("width"), pick("height"))
}

#[test]
fn bug269_fixed_width_derives_height_from_ratio() {
    // `<img width="240">` with intrinsic 120Г—80 в†’ height = 240В·80/120 = 160,
    // not the raw intrinsic 80 (and never a collapsed `height: auto` = 0).
    let (w, h) = img_dims(r#"<img src="p.png" width="240">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("240"));
    assert_eq!(h.as_deref(), Some("160"));
}

#[test]
fn bug269_fixed_height_derives_width_from_ratio() {
    // Symmetric case: author height only в†’ width from the intrinsic ratio.
    let (w, h) = img_dims(r#"<img src="p.png" height="160">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("240"));
    assert_eq!(h.as_deref(), Some("160"));
}

#[test]
fn bug269_no_attrs_uses_raw_intrinsic() {
    // Neither dimension set в†’ both filled with the raw intrinsic values.
    let (w, h) = img_dims(r#"<img src="p.png">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("120"));
    assert_eq!(h.as_deref(), Some("80"));
}

#[test]
fn bug269_both_attrs_unchanged() {
    // Both author dimensions set в†’ intrinsic size never overrides them.
    let (w, h) = img_dims(r#"<img src="p.png" width="10" height="20">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("10"));
    assert_eq!(h.as_deref(), Some("20"));
}

#[test]
fn bug269_percentage_width_falls_back_to_intrinsic_height() {
    // A non-integer (percentage) width has no shell-resolvable px value to
    // drive the ratio, but the image must still be visible в†’ fill the height
    // with the raw intrinsic value and leave the percentage width intact.
    let (w, h) = img_dims(r#"<img src="p.png" width="50%">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("50%"));
    assert_eq!(h.as_deref(), Some("80"));
}

// в”Ђв”Ђ BUG-735: СЃС…РѕРґРёРјРѕСЃС‚СЊ РїРѕРІС‚РѕСЂРЅРѕРіРѕ РїСЂРѕС…РѕРґР° intrinsic-СЂР°Р·РјРµСЂРѕРІ в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
//
// Streaming/РґРёРЅР°РјРёС‡РµСЃРєРёР№ РїСѓС‚СЊ СЂР°Р·РґР°С‘С‚ СЂР°Р·РјРµСЂС‹ РїСЂРѕС…РѕРґРѕРј
// `Lumen::apply_stream_intrinsic_sizes`, РєРѕС‚РѕСЂС‹Р№ РїСЂРѕСЃРёС‚ СЂРµР»РµР№Р°СѓС‚ СЂРѕРІРЅРѕ
// С‚РѕРіРґР°, РєРѕРіРґР° `apply_intrinsic_size` СЃРѕРѕР±С‰РёР» РѕР± РёР·РјРµРЅРµРЅРёРё DOM. РџСЂРѕС…РѕРґ СЃР°Рј
// Р·Р°РєР°Р·С‹РІР°РµС‚СЃСЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ СЂРµР»РµР№Р°СѓС‚Р° (РЅРѕРІС‹Р№ `<img>` СЃ СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рј
// `src` СЃРІРѕРµРіРѕ `ImageDecoded` РЅРµ РїРѕР»СѓС‡РёС‚ вЂ” Р·Р°РїСЂРѕСЃ РґРµРґСѓРїР»РёС†РёСЂРѕРІР°РЅ РїРѕ URL),
// РїРѕСЌС‚РѕРјСѓ В«РЅРёС‡РµРіРѕ РЅРµ РґРѕРїРёСЃР°Р» в†’ falseВ» вЂ” СЌС‚Рѕ С‚Рѕ, РЅР° С‡С‘Рј РґРµСЂР¶РёС‚СЃСЏ РѕС‚СЃСѓС‚СЃС‚РІРёРµ
// РїРµС‚Р»Рё В«СЂРµР»РµР№Р°СѓС‚ в†’ РїСЂРѕС…РѕРґ в†’ СЂРµР»РµР№Р°СѓС‚В».

/// Parse `html`, run `apply_intrinsic_size` twice with the same intrinsic
/// size, and return both calls' change flags.
fn img_apply_twice(html: &str, iw: u32, ih: u32) -> (bool, bool) {
    let mut doc = lumen_html_parser::parse(html);
    let img = find_img(&doc, doc.root()).expect("img present");
    let first = apply_intrinsic_size(&mut doc, img, iw, ih);
    let second = apply_intrinsic_size(&mut doc, img, iw, ih);
    (first, second)
}

#[test]
fn bug735_first_apply_changes_dom_second_does_not() {
    let (first, second) = img_apply_twice(r#"<img src="p.png">"#, 120, 80);
    assert!(first, "РїРµСЂРІС‹Р№ РІС‹Р·РѕРІ РґРѕРїРёСЃС‹РІР°РµС‚ width/height");
    assert!(!second, "РїРѕРІС‚РѕСЂРЅС‹Р№ РІС‹Р·РѕРІ РЅРµ РјРµРЅСЏРµС‚ DOM вЂ” СЂРµР»РµР№Р°СѓС‚ РЅРµ РЅСѓР¶РµРЅ");
}

#[test]
fn bug735_author_dimensions_report_no_change() {
    // РђРІС‚РѕСЂ Р·Р°РґР°Р» РѕР±Р° СЂР°Р·РјРµСЂР° вЂ” РґРѕРїРёСЃС‹РІР°С‚СЊ РЅРµС‡РµРіРѕ СЃ СЃР°РјРѕРіРѕ РЅР°С‡Р°Р»Р°.
    let (first, second) =
        img_apply_twice(r#"<img src="p.png" width="10" height="20">"#, 120, 80);
    assert!(!first);
    assert!(!second);
}

#[test]
fn bug735_half_filled_reports_change_once() {
    // Р—Р°РґР°РЅР° С‚РѕР»СЊРєРѕ С€РёСЂРёРЅР° в†’ РґРѕРїРёСЃС‹РІР°РµС‚СЃСЏ РІС‹СЃРѕС‚Р° РёР· СЃРѕРѕС‚РЅРѕС€РµРЅРёСЏ (РёР·РјРµРЅРµРЅРёРµ),
    // РІС‚РѕСЂРѕР№ РїСЂРѕС…РѕРґ СѓР¶Рµ РІРёРґРёС‚ РѕР±Рµ Рё РјРѕР»С‡РёС‚.
    let (first, second) = img_apply_twice(r#"<img src="p.png" width="240">"#, 120, 80);
    assert!(first);
    assert!(!second);
}

// в”Ђв”Ђ BUG-171 СЌС‚Р°Рї 2: off-UI-thread С„РёРЅР°Р»СЊРЅС‹Р№ pipeline в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
//
// Р¤РёРЅР°Р»СЊРЅС‹Р№ render (`render_bytes`) СѓРµР·Р¶Р°РµС‚ РЅР° С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє, Р° РіРѕС‚РѕРІС‹Р№
// СЂРµР·СѓР»СЊС‚Р°С‚ РїРµСЂРµСЃС‹Р»Р°РµС‚СЃСЏ РЅР°Р·Р°Рґ С‡РµСЂРµР· `LoadEvent::RenderDone`. Р­С‚Рѕ СЂР°Р±РѕС‚Р°РµС‚
// С‚РѕР»СЊРєРѕ РµСЃР»Рё РІРµСЃСЊ РіСЂСѓР· вЂ” `Send`: `RenderOutcome` (РІРєР»СЋС‡Р°СЏ JS-С…СЌРЅРґР» Р·Р°
// `Arc<dyn PersistentJs>`), СЃР°Рј `LoadEvent` Рё proxy. Р РµРіСЂРµСЃСЃРёРѕРЅРЅР°СЏ Р·Р°С‰РёС‚Р°:
// РµСЃР»Рё РєС‚Рѕ-С‚Рѕ РґРѕР±Р°РІРёС‚ `!Send`-РїРѕР»Рµ РІ `LoadedPage`/`LayoutSource` РёР»Рё СЃРЅРёРјРµС‚
// `Send`/`Sync` СЃ `PersistentJs`, СЌС‚Рё Р°СЃСЃРµСЂС‚С‹ РїРµСЂРµСЃС‚Р°РЅСѓС‚ РєРѕРјРїРёР»РёСЂРѕРІР°С‚СЊСЃСЏ.

fn _assert_send<T: Send>() {}
fn _assert_sync<T: Sync>() {}

#[test]
fn render_pipeline_payload_is_send() {
    // Р“СЂСѓР·, РїРµСЂРµСЃРµРєР°СЋС‰РёР№ РіСЂР°РЅРёС†Сѓ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРє в†’ UI-РїРѕС‚РѕРє.
    _assert_send::<RenderOutcome>();
    _assert_send::<LoadEvent>();
    _assert_send::<Arc<dyn PersistentJs>>();
    _assert_send::<EventLoopProxy<LoadEvent>>();
    // РђСЂРіСѓРјРµРЅС‚С‹, СѓРµР·Р¶Р°СЋС‰РёРµ РІ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРє.
    _assert_send::<Arc<KnuthLiangHyphenation>>();
    _assert_send::<RawPage>();
    // ADR-016 M2.2c-2b: С…СЌРЅРґР» СЂР°Р·РґРµР»СЏРµС‚СЃСЏ РјРµР¶РґСѓ UI- Рё РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј Р·Р°
    // `Arc<dyn PersistentJs>`, РїРѕСЌС‚РѕРјСѓ РѕР±СЏР·Р°РЅ Р±С‹С‚СЊ `Send + Sync`. Р•СЃР»Рё РєС‚Рѕ-С‚Рѕ
    // СЃРЅРёРјРµС‚ `Sync` СЃ `PersistentJs`, СЌС‚Р° СЃС‚СЂРѕРєР° РїРµСЂРµСЃС‚Р°РЅРµС‚ РєРѕРјРїРёР»РёСЂРѕРІР°С‚СЊСЃСЏ.
    _assert_sync::<Arc<dyn PersistentJs>>();
}

// в”Ђв”Ђ ADR-016 M2.2c-2b: engine-thread owns EngineJsState в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn engine_js_state_default_is_empty() {
    // РЎРІРµР¶РµРµ СЃРѕСЃС‚РѕСЏРЅРёРµ РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° вЂ” Р±РµР· С…СЌРЅРґР»Р° Рё Р±РµР· DOM (Р·Р°РїРѕР»РЅСЏРµС‚СЃСЏ
    // `sync_engine_js_state` РїСЂРё РїРµСЂРІРѕР№ Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹).
    let state = EngineJsState::default();
    assert!(state.js.is_none());
    assert!(state.document.is_none());
}

#[test]
fn engine_thread_carries_and_mutates_js_state() {
    // Р РµР°Р»СЊРЅС‹Р№ С‚РёРї СЃРѕСЃС‚РѕСЏРЅРёСЏ shell (`EngineJsState`) Р¶РёРІС‘С‚ РЅР° РґРІРёР¶РєРѕРІРѕРј РїРѕС‚РѕРєРµ:
    // `task` РєР»Р°РґС‘С‚ СЂР°Р·РґРµР»СЏРµРјС‹Р№ `Document` (РєР°Рє СЌС‚Рѕ РґРµР»Р°РµС‚ `sync_engine_js_state`),
    // `query` С‡РёС‚Р°РµС‚ РµРіРѕ РѕР±СЂР°С‚РЅРѕ вЂ” end-to-end РїСЂРѕРІРµСЂРєР°, С‡С‚Рѕ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРёРµ
    // С…СЌРЅРґР»Р°/DOM РІ СЃРѕСЃС‚РѕСЏРЅРёРµ СЂР°Р±РѕС‚Р°РµС‚ С‡РµСЂРµР· РЅР°СЃС‚РѕСЏС‰РёР№ `EngineThread<_, S>`.
    let engine = engine_thread::EngineThread::<u64, EngineJsState>::spawn()
        .expect("spawn engine thread");
    // РџСѓСЃС‚Рѕ РґРѕ РїРµСЂРІРѕРіРѕ task (Р°РЅР°Р»РѕРі РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ СЃС‚Р°СЂС‚Р°).
    assert_eq!(engine.query(|s| s.document.is_some()), Some(false));
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>hi</p>")));
    engine.task(move |s| s.document = Some(doc));
    // query РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РїРѕСЃР»Рµ task (СѓРїРѕСЂСЏРґРѕС‡РµРЅРЅС‹Р№ РєР°РЅР°Р») в†’ DOM РЅР° РјРµСЃС‚Рµ.
    assert_eq!(engine.query(|s| s.document.is_some()), Some(true));
}

#[test]
fn engine_thread_query_take_extracts_and_clears_state() {
    // ADR-016 M2.2c-2d (21): РјРµС…Р°РЅРёР·Рј, РЅР° РєРѕС‚РѕСЂРѕРј СЃС‚РѕРёС‚ `Lumen::take_js_ctx` вЂ”
    // `save_page_snapshot` РІС‹РЅРёРјР°РµС‚ С…СЌРЅРґР» РёР· РґРІРёР¶РєРѕРІРѕРіРѕ СЃРѕСЃС‚РѕСЏРЅРёСЏ Р±Р»РѕРєРёСЂСѓСЋС‰РёРј
    // `query`, `take`-Р°СЋС‰РёРј РїРѕР»Рµ (state в†’ snapshot). РџСЂРѕРІРµСЂСЏРµРј РЅР° `document`
    // (С‚РѕС‚ Р¶Рµ generic-РїСѓС‚СЊ, С‡С‚Рѕ Рё `state.js.take()`, РЅРѕ Р±РµР· mock-С…СЌРЅРґР»Р°): РїРѕСЃР»Рµ
    // РґРµРїРѕР·РёС‚Р° `task`-РѕРј `query`-take РІРѕР·РІСЂР°С‰Р°РµС‚ Р·РЅР°С‡РµРЅРёРµ Рё РѕСЃС‚Р°РІР»СЏРµС‚ СЃРѕСЃС‚РѕСЏРЅРёРµ
    // РїСѓСЃС‚С‹Рј, Р° РїРѕРІС‚РѕСЂРЅС‹Р№ take вЂ” `None` (РєР°Рє `js_ctx` СѓРµС…Р°Р» РІ СЃРЅР°РїС€РѕС‚).
    let engine = engine_thread::EngineThread::<u64, EngineJsState>::spawn()
        .expect("spawn engine thread");
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>hi</p>")));
    engine.task(move |s| s.document = Some(doc));
    // РџРµСЂРІС‹Р№ take РёР·РІР»РµРєР°РµС‚ РґРµРїРѕРЅРёСЂРѕРІР°РЅРЅС‹Р№ Arc, СЃРѕСЃС‚РѕСЏРЅРёРµ РѕС‡РёС‰Р°РµС‚СЃСЏ.
    assert_eq!(
        engine.query(|s| s.document.take().map(|d| Arc::strong_count(&d))),
        Some(Some(1))
    );
    // РЎРѕСЃС‚РѕСЏРЅРёРµ С‚РµРїРµСЂСЊ РїСѓСЃС‚Рѕ вЂ” РїРѕРІС‚РѕСЂРЅС‹Р№ take РґР°С‘С‚ `None` (`flatten` РІ
    // `take_js_ctx` СЃС…Р»РѕРїРЅРµС‚ `Some(None)` в†’ `None`).
    assert_eq!(engine.query(|s| s.document.take().is_some()), Some(false));
}

#[test]
fn route_eval_js_without_handle_is_noop() {
    // Р¤Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ Рё С…СЌРЅРґР»Р° РЅРµС‚: РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ РЅРµ РґРѕР»Р¶РµРЅ РїР°РЅРёРєРѕРІР°С‚СЊ Рё РїСЂРѕСЃС‚Рѕ
    // РЅРёС‡РµРіРѕ РЅРµ РґРµР»Р°РµС‚ (Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `if let Some(js) = вЂ¦ {}`).
    route_eval_js(None, None, "_lumen_run_navigate_handler()".to_owned());
}

#[test]
fn route_query_js_without_handle_is_none() {
    // Р¤Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ (`engine = None`) Рё С…СЌРЅРґР»Р° РЅРµС‚ (`js = None`): value-returning
    // РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ РІРѕР·РІСЂР°С‰Р°РµС‚ `None` в†’ РІС‹Р·С‹РІР°СЋС‰Р°СЏ СЃС‚РѕСЂРѕРЅР° РїРѕРґСЃС‚Р°РІРёС‚ РІРµС‚РєСѓ
    // В«Р±РµР· JSВ» (РЅР°РїСЂ. `unwrap_or(false)`), Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ `js_ctx == None`.
    let r: Option<bool> = route_query_js(None, None, |j| j.take_dom_dirty());
    assert_eq!(r, None);
}

#[test]
fn route_query_js_flag_on_without_synced_handle_is_none() {
    // Р¤Р»Р°Рі РІРєР»СЋС‡С‘РЅ (РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РµСЃС‚СЊ), РЅРѕ `EngineJsState.js` РµС‰С‘ РЅРµ
    // Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅ (`sync_engine_js_state` РЅРµ РІС‹Р·С‹РІР°Р»СЃСЏ) в†’ РІРЅСѓС‚СЂРµРЅРЅРёР№
    // `state.js.map(read)` РґР°С‘С‚ `None`, `flatten` СЃС…Р»РѕРїС‹РІР°РµС‚ РґРѕ `None`.
    // Р—Р°РјС‹РєР°РЅРёРµ `read` РїСЂРё СЌС‚РѕРј РќР• РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ вЂ” С…СЌРЅРґР»Р° РЅРµС‚.
    let engine = engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn()
        .expect("spawn engine thread");
    let r: Option<bool> =
        route_query_js(Some(&engine), None, |j| j.take_dom_dirty());
    assert_eq!(r, None);
}

#[test]
fn route_query_js_nav_reads_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2c (РѕСЃС‚Р°С‚РѕРє): nav/timer/nav-update С‡С‚РµРЅРёСЏ РёСЃРїРѕР»СЊР·СѓСЋС‚ С‚РѕС‚ Р¶Рµ
    // `route_query_js`, РЅРѕ СЃ Р±РѕР»РµРµ Р±РѕРіР°С‚С‹РјРё С‚РёРїР°РјРё РІРѕР·РІСЂР°С‚Р° (`Option<_>` Рё `Vec<_>`).
    // Р‘РµР· С…СЌРЅРґР»Р° (`engine = None`, `js = None`) РІРЅРµС€РЅРёР№ `Option` = `None`, РїРѕСЌС‚РѕРјСѓ
    // `flatten`/`unwrap_or_default` РІ РІС‹Р·С‹РІР°СЋС‰РёС… СЃР°Р№С‚Р°С… РґР°СЋС‚ С‚Сѓ Р¶Рµ РІРµС‚РєСѓ В«Р±РµР· JSВ»,
    // С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ РїСЂСЏРјС‹Рµ РІС‹Р·РѕРІС‹: РЅРµС‚ РЅР°РІРёРіР°С†РёРё, РЅРµС‚ wakeup, РїСѓСЃС‚РѕР№ РґСЂРµРЅР°Р¶.
    let nav: Option<Option<JsNavigateRequest>> =
        route_query_js(None, None, |j| j.take_navigate_request());
    assert!(nav.flatten().is_none());
    let wakeup: Option<Option<f64>> =
        route_query_js(None, None, |j| j.take_timer_wakeup());
    assert!(wakeup.flatten().is_none());
    let navs: Option<Vec<(u8, String, String, String)>> =
        route_query_js(None, None, |j| j.take_nav_updates());
    assert!(navs.unwrap_or_default().is_empty());
}

#[test]
fn route_query_js_canvas_history_drains_without_handle_default_to_empty() {
    // ADR-016 M2.2c-2d: canvas/history per-tick РґСЂРµРЅР°Р¶Рё РІ `about_to_wait`
    // РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓСЋС‚СЃСЏ С‚РµРј Р¶Рµ `route_query_js`. Р‘РµР· С…СЌРЅРґР»Р° (`engine = None`,
    // `js = None`) РІРЅРµС€РЅРёР№ `Option` = `None`, РїРѕСЌС‚РѕРјСѓ `unwrap_or_default` РІ
    // РІС‹Р·С‹РІР°СЋС‰РёС… СЃР°Р№С‚Р°С… РґР°С‘С‚ РїСѓСЃС‚РѕР№ `Vec` вЂ” С‚Р° Р¶Рµ РІРµС‚РєР° В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ
    // РїСЂСЏРјС‹Рµ `js_ctx.map(<drain>).unwrap_or_default()`.
    // РўРёРї `R` РІС‹РІРѕРґРёС‚СЃСЏ РёР· РІРѕР·РІСЂР°С‰Р°РµРјРѕРіРѕ Р·РЅР°С‡РµРЅРёСЏ Р·Р°РјС‹РєР°РЅРёСЏ вЂ” СЏРІРЅСѓСЋ Р°РЅРЅРѕС‚Р°С†РёСЋ
    // РЅРµ РїРёС€РµРј (СЃР»РѕР¶РЅС‹Р№ РєРѕСЂС‚РµР¶ `flush_canvas_updates` РёРЅР°С‡Рµ С‚СЂРёРіРіРµСЂРёС‚
    // clippy::type_complexity); С†РµРїР»СЏРµРј `unwrap_or_default` СЃСЂР°Р·Сѓ.
    let canvas = route_query_js(None, None, |j| j.flush_canvas_updates()).unwrap_or_default();
    assert!(canvas.is_empty());
    let hist_url = route_query_js(None, None, |j| j.take_history_url_updates()).unwrap_or_default();
    assert!(hist_url.is_empty());
    let hist_go = route_query_js(None, None, |j| j.take_history_traversals()).unwrap_or_default();
    assert!(hist_go.is_empty());
}

#[test]
fn bug428_canvas_updates_keyed_as_display_list_expects() {
    // BUG-428: headless CPU-СЂРµРЅРґРµСЂ РїРѕР»СѓС‡РёР» С‚РѕС‚ Р¶Рµ РґСЂРµРЅР°Р¶ РєР°РЅРІР°СЃР°, С‡С‚Рѕ Р¶РёРІРѕР№ С†РёРєР».
    // РћР±Р° СЃР°Р№С‚Р° СЃС‚СЂРѕСЏС‚ РєР»СЋС‡ С‡РµСЂРµР· `canvas_updates_as_images`, Рё РѕРЅ РѕР±СЏР·Р°РЅ СЃРѕРІРїР°СЃС‚СЊ
    // СЃ С‚РµРј, С‡С‚Рѕ РєР»Р°РґС‘С‚ РІ `DrawImage.src` СЌРјРёС‚С‚РµСЂ (`display_list.rs`:
    // `format!("canvas:{}", b.node.index())`) вЂ” РёРЅР°С‡Рµ РєР°СЂС‚РёРЅРєР° РЅРµ РЅР°Р№РґС‘С‚СЃСЏ Рё
    // РєР°РЅРІР°СЃ СЃРЅРѕРІР° РЅР°СЂРёСЃСѓРµС‚СЃСЏ РїСЂРѕР·СЂР°С‡РЅС‹Рј.
    let updates = vec![
        (7_u32, 2_u32, 1_u32, vec![1, 2, 3, 4, 5, 6, 7, 8]),
        (12_u32, 1_u32, 1_u32, vec![9, 9, 9, 9]),
    ];
    let images = canvas_updates_as_images(updates);
    let keys: Vec<&str> = images.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["canvas:7", "canvas:12"]);
    let (_, first) = &images[0];
    assert_eq!((first.width, first.height), (2, 1));
    assert_eq!(first.format, lumen_image::PixelFormat::Rgba8);
    assert_eq!(first.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn bug428_empty_canvas_drain_adds_no_images() {
    // РЎС‚СЂР°РЅРёС†Р° Р±РµР· РєР°РЅРІР°СЃР° (РёР»Рё Р±РµР· JS-РєРѕРЅС‚РµРєСЃС‚Р°) РЅРµ РґРѕР»Р¶РЅР° РїРѕРґРјРµС€РёРІР°С‚СЊ Р·Р°РїРёСЃРё
    // РІ РЅР°Р±РѕСЂ РєР°СЂС‚РёРЅРѕРє CPU-СЂР°СЃС‚РµСЂРёР·Р°С‚РѕСЂР° вЂ” РґСЂРµРЅР°Р¶ РїСѓСЃС‚, РЅР°Р±РѕСЂ РЅРµ РјРµРЅСЏРµС‚СЃСЏ.
    assert!(canvas_updates_as_images(Vec::new()).is_empty());
}

#[test]
fn route_query_js_nav_intercept_without_handle_defaults_to_no_op() {
    // ADR-016 M2.2c-2d: РїРѕСЃР»РµРґРЅРµРµ СЃРёРЅС…СЂРѕРЅРЅРѕРµ read-after-eval С‡С‚РµРЅРёРµ вЂ”
    // `take_nav_intercept_result` РІ nav-РјРµС‚РѕРґР°С… (`navigate_to`/`_replace`/
    // `_back`/`_forward`) вЂ” РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµС‚СЃСЏ С‚РµРј Р¶Рµ `route_query_js`. Р‘РµР· С…СЌРЅРґР»Р°
    // (`engine = None`, `js = None`) РІРЅРµС€РЅРёР№ `Option` = `None`, РїРѕСЌС‚РѕРјСѓ РІС‹Р·С‹РІР°СЋС‰РёР№
    // `if let Some(intercept) = вЂ¦` РїСЂРѕРїСѓСЃРєР°РµС‚ РІРµСЃСЊ intercept-Р±Р»РѕРє вЂ” С‚Р° Р¶Рµ РІРµС‚РєР°
    // В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёР№ `if let Some(js) = &self.js_ctx { вЂ¦ }`.
    let intercept: Option<Vec<(bool, bool)>> =
        route_query_js(None, None, |j| j.take_nav_intercept_result());
    assert!(intercept.is_none());
}

#[test]
fn route_query_js_pointer_capture_and_raf_reads_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2d: РїРѕСЃР»РµРґРЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ value-returning UIв†’JS С‡С‚РµРЅРёСЏ вЂ”
    // pre-dispatch pointer-capture (`pointer_capture_nid`/`take_pointer_capture`
    // РІ mouseup/pointermove) Рё wait-poll `has_raf_pending` (`WaitCondition::JsIdle`)
    // вЂ” РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓСЋС‚СЃСЏ С‚РµРј Р¶Рµ `route_query_js`. Р‘РµР· С…СЌРЅРґР»Р° (`engine = None`,
    // `js = None`) РІРЅРµС€РЅРёР№ `Option` = `None`, РїРѕСЌС‚РѕРјСѓ `flatten` РІ capture-СЃР°Р№С‚Р°С…
    // РґР°С‘С‚ `hit_nid`/РїСЂРѕРїСѓСЃРє lostpointercapture, Р° `unwrap_or(false)` + РѕС‚СЂРёС†Р°РЅРёРµ
    // РІ JsIdle РґР°С‘С‚ `idle = true` вЂ” С‚Р° Р¶Рµ РІРµС‚РєР° В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ РїСЂСЏРјС‹Рµ
    // `self.js_ctx.as_ref().and_then(...)` / `is_none_or(...)`.
    let cap_nid: Option<Option<u32>> =
        route_query_js(None, None, |j| j.pointer_capture_nid());
    assert!(cap_nid.flatten().is_none());
    let taken: Option<Option<u32>> =
        route_query_js(None, None, |j| j.take_pointer_capture());
    assert!(taken.flatten().is_none());
    let raf: Option<bool> = route_query_js(None, None, |j| j.has_raf_pending());
    assert!(!raf.unwrap_or(false));
}

#[test]
fn route_query_js_layout_geometry_push_without_handle_defaults_to_empty() {
    // ADR-016 M2.2c-2d: layout-geometry push (`update_layout_rects` Рё Co.). The
    // relayout observer-delivery site wraps its whole ordered void+read sequence
    // in one `route_query_js` returning the drained lazy-image requests. Р‘РµР· С…СЌРЅРґР»Р°
    // (`engine = None`, `js = None`) РІРЅРµС€РЅРёР№ `Option` = `None`, РїРѕСЌС‚РѕРјСѓ
    // `unwrap_or_default` РґР°С‘С‚ РїСѓСЃС‚РѕР№ `Vec` вЂ” С‚Р° Р¶Рµ РІРµС‚РєР° В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёР№
    // `if let Some(js) = &self.js_ctx { вЂ¦ }` (РѕСЃС‚Р°РІР»СЏРІС€РёР№ `lazy_reqs` РїСѓСЃС‚С‹Рј, Р°
    // seed-СЃР°Р№С‚С‹ вЂ” РЅРµ РґРёСЃРїР°С‚С‡РёРІС€РёРјРё push).
    let lazy: Vec<(u32, String)> =
        route_query_js(None, None, |j| j.take_lazy_image_requests()).unwrap_or_default();
    assert!(lazy.is_empty());
}

#[test]
fn route_lazy_pageshow_resize_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2d (17): lazy-image setup + pageshow lifecycle + resize-eval.
    // The lazy setup wraps registerв†’pushв†’deliverв†’drain in one `route_query_js`
    // returning the requests; pageshow (`notify_window_loaded` +
    // `fire_page_lifecycle`) and resize-eval (`_lumen_apply_resize`) are routed
    // fire-and-forget. Р‘РµР· С…СЌРЅРґР»Р° (`engine = None`, `js = None`) РІСЃРµ С‚СЂРё вЂ” no-op:
    // `unwrap_or_default` РґР°С‘С‚ РїСѓСЃС‚РѕР№ `Vec`, void-РґРµР№СЃС‚РІРёСЏ РЅРµ РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ вЂ” С‚Р° Р¶Рµ
    // РІРµС‚РєР° В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ РїСЂСЏРјС‹Рµ `if let Some(js) = &self.js_ctx { вЂ¦ }`.
    let lazy: Vec<(u32, String)> = route_query_js(None, None, |j| {
        j.register_lazy_images(&[]);
        j.take_lazy_image_requests()
    })
    .unwrap_or_default();
    assert!(lazy.is_empty());
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);
    route_task_js(None, None, move |j| {
        j.notify_window_loaded();
        ran2.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    route_eval_js(None, None, "_lumen_apply_resize(0, 0, 0);".to_string());
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn route_tab_park_unpark_without_handle_default_to_no_op() {
    // ADR-016 M2.2d (18): tab park/unpark РІ `switch_tab`. Park
    // (`pause_event_loop` РїРµСЂРµРґ `save_page_snapshot`) Рё unpark
    // (`unpause_event_loop` + `run_gc_pass(0)` РїРѕСЃР»Рµ `restore_page_snapshot`) вЂ”
    // РїРѕСЃР»РµРґРЅРёРµ РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ, РїРµСЂРµРІРµРґС‘РЅРЅС‹Рµ РЅР° `route_task_js`.
    // Р‘РµР· С…СЌРЅРґР»Р° (`engine = None`, `js = None`) РѕР±Р° вЂ” no-op: void-РґРµР№СЃС‚РІРёСЏ РЅРµ
    // РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ, РїР°РЅРёРєРё РЅРµС‚ вЂ” С‚Р° Р¶Рµ РІРµС‚РєР° В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ РїСЂСЏРјС‹Рµ
    // `if let Some(js) = &self.js_ctx { вЂ¦ }`.
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let park = Arc::clone(&ran);
    route_task_js(None, None, move |j| {
        j.pause_event_loop();
        park.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let unpark = Arc::clone(&ran);
    route_task_js(None, None, move |j| {
        j.unpause_event_loop();
        j.run_gc_pass(0);
        unpark.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn route_nav_timing_and_js_heap_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2d (20): the last direct `self.js_ctx` reads on the UI thread
    // were decoupled from `Arc` ownership: nav-timing delivery (`deliver_nav_timing`,
    // fire-and-forget void) now routes through `route_task_js`, and the MEM_REPORT
    // heap probe (`debug_js_heap`, value read) through `route_query_js`. Р‘РµР· С…СЌРЅРґР»Р°
    // (`engine = None`, `js = None`) delivery вЂ” no-op, Р° heap-С‡С‚РµРЅРёРµ РІРѕР·РІСЂР°С‰Р°РµС‚
    // `None` в†’ `unwrap_or((-1, -1))` = РїСЂРµР¶РЅРёР№ `map_or((-1, -1), вЂ¦)`; С‚Р° Р¶Рµ РІРµС‚РєР°
    // В«Р±РµР· JSВ», С‡С‚Рѕ Рё РїСЂРµР¶РЅРёРµ РїСЂСЏРјС‹Рµ РѕР±СЂР°С‰РµРЅРёСЏ Рє РїРѕР»СЋ.
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);
    route_task_js(None, None, move |j| {
        j.deliver_nav_timing("https://example.test/", 1.0);
        ran2.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    let js_heap = route_query_js(None, None, |j| j.debug_js_heap()).unwrap_or((-1, -1));
    assert_eq!(js_heap, (-1, -1));
}

// в”Ђв”Ђ ADR-016 M2.2c-2d: generic void-action router (pump/tick batch) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn route_task_js_without_handle_is_noop() {
    // Р¤Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ Рё С…СЌРЅРґР»Р° РЅРµС‚: РѕР±РѕР±С‰С‘РЅРЅС‹Р№ void-РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ РЅРµ РїР°РЅРёРєСѓРµС‚ Рё
    // РґРµР№СЃС‚РІРёРµ РЅРµ РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ (Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `if let Some(js) = вЂ¦ {}`).
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);
    route_task_js(None, None, move |_j| {
        ran2.store(true, std::sync::atomic::Ordering::SeqCst)
    });
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn route_task_js_flag_on_without_synced_handle_skips_action() {
    // Р¤Р»Р°Рі РІРєР»СЋС‡С‘РЅ (РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РµСЃС‚СЊ), РЅРѕ `EngineJsState.js` РµС‰С‘ РЅРµ
    // Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅ в†’ Р·Р°РґР°С‡Р° РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РЅР° РїРѕС‚РѕРєРµ, РЅРѕ `state.js == None`, РїРѕСЌС‚РѕРјСѓ
    // СЃР°РјРѕ РґРµР№СЃС‚РІРёРµ (pump-Р±Р°С‚С‡) РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ. Р‘Р°СЂСЊРµСЂ-`query` СѓРїРѕСЂСЏРґРѕС‡РµРЅ РїРѕСЃР»Рµ
    // `task`, С‚Р°Рє С‡С‚Рѕ Рє РјРѕРјРµРЅС‚Сѓ РїСЂРѕРІРµСЂРєРё Р·Р°РґР°С‡Р° РіР°СЂР°РЅС‚РёСЂРѕРІР°РЅРЅРѕ РѕС‚СЂР°Р±РѕС‚Р°Р»Р°.
    let engine = engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn()
        .expect("spawn engine thread");
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);
    route_task_js(Some(&engine), None, move |_j| {
        ran2.store(true, std::sync::atomic::Ordering::SeqCst)
    });
    let _ = engine.query(|_s| ());
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
}

// в”Ђв”Ђ Fullscreen viewport reconciliation (BUG-167) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn fullscreen_poll_applies_when_size_changed() {
    // OS resized the window to the fullscreen area в†’ apply resize + relayout.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1920, 1080), 240),
        FullscreenPoll::Apply(1920, 1080)
    );
}

#[test]
fn fullscreen_poll_waits_while_size_unchanged() {
    // OS has not applied the new size yet в†’ keep polling, one attempt spent.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1024, 756), 240),
        FullscreenPoll::Wait(1024, 756, 239)
    );
}

#[test]
fn fullscreen_poll_waits_on_zero_size() {
    // Minimized / not-yet-mapped window reports 0Г—0 в†’ treat as not applied.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (0, 0), 5),
        FullscreenPoll::Wait(1024, 756, 4)
    );
}

#[test]
fn fullscreen_poll_gives_up_when_budget_exhausted() {
    // Last attempt with no size change в†’ stop polling, do not spin the loop.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1024, 756), 1),
        FullscreenPoll::Done
    );
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1024, 756), 0),
        FullscreenPoll::Done
    );
}

#[test]
fn fullscreen_poll_applies_even_on_last_attempt() {
    // A real size change is honoured regardless of the remaining budget.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1920, 1080), 1),
        FullscreenPoll::Apply(1920, 1080)
    );
}

#[test]
fn fullscreen_poll_applies_on_exit_shrink() {
    // Exiting fullscreen shrinks back to a windowed size вЂ” also a change.
    assert_eq!(
        decide_fullscreen_poll((1920, 1080), (1024, 756), 240),
        FullscreenPoll::Apply(1024, 756)
    );
}

// в”Ђв”Ђ content-visibility: auto вЂ” shell state (BB-4) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

fn cv_nid(n: usize) -> NodeId {
    NodeId::from_index(n)
}

fn cv_state(pairs: &[(usize, bool)]) -> std::collections::HashMap<NodeId, bool> {
    pairs.iter().map(|&(n, s)| (cv_nid(n), s)).collect()
}

#[test]
fn diff_cv_state_emits_on_first_observation_both_ways() {
    // CSS Contain L2 В§4.1: the first observation of an element fires in
    // BOTH directions вЂ” this is what
    // `content-visibility-auto-state-changed-first-observation.html`
    // asserts, and what the old "new node в‡’ skipped: true" diff could not
    // express at all.
    let events = diff_cv_state(
        &cv_state(&[]),
        &[(cv_nid(1), false), (cv_nid(2), true)],
    );
    assert_eq!(events, vec![
        ContentVisibilityChange { node: cv_nid(1), skipped: false },
        ContentVisibilityChange { node: cv_nid(2), skipped: true },
    ]);
}

#[test]
fn diff_cv_state_emits_on_transition_only() {
    let prev = cv_state(&[(1, false), (2, true)]);
    let events = diff_cv_state(&prev, &[(cv_nid(1), true), (cv_nid(2), true)]);
    assert_eq!(events, vec![ContentVisibilityChange { node: cv_nid(1), skipped: true }]);
}

#[test]
fn diff_cv_state_is_silent_for_a_removed_node() {
    // `content-visibility-auto-state-changed-removed.html`: a disconnected
    // element must not be told anything, so a node that left the tree is
    // not a state change.
    let prev = cv_state(&[(1, true), (2, false)]);
    assert!(diff_cv_state(&prev, &[(cv_nid(2), false)]).is_empty());
    assert!(diff_cv_state(&cv_state(&[]), &[]).is_empty());
}

#[test]
fn collect_cv_auto_reports_every_auto_box_with_its_state() {
    lumen_layout::set_cv_scroll(0.0, 0.0);
    lumen_layout::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="cv"><span>on</span></div>
                      <div class="spacer"></div><div class="cv"><span>off</span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".spacer { height: 2000px; } .cv { content-visibility: auto; }",
    );
    let lb = lumen_layout::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let _ = lumen_layout::take_cv_skipped();
    let mut found = Vec::new();
    collect_cv_auto(&lb, &mut found);
    assert_eq!(found.len(), 2, "РѕР±Р° auto-Р±РѕРєСЃР°, Р° РЅРµ С‚РѕР»СЊРєРѕ РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№");
    let states: Vec<bool> = found
        .iter()
        .map(|&(_, top)| lumen_layout::cv_is_skipped(false, top, 0.0, 300.0))
        .collect();
    assert_eq!(states, vec![false, true], "РїРµСЂРІС‹Р№ РІРѕ РІСЊСЋРїРѕСЂС‚Рµ, РІС‚РѕСЂРѕР№ РїРѕРґ РЅРёРј");
}

#[test]
fn collect_cv_auto_reports_an_empty_auto_box_by_position() {
    // BUG-852: an empty `content-visibility: auto` element used to be
    // indistinguishable from a skipped one ("no children в‡’ skipped"), so
    // one in the viewport reported the wrong state and layout never even
    // consulted the rule for it. Position decides now, emptiness does not.
    lumen_layout::set_cv_scroll(0.0, 0.0);
    lumen_layout::set_cv_relevant(std::collections::HashSet::new());
    let doc = lumen_html_parser::parse(r#"<div class="cv"></div>"#);
    let sheet = lumen_css_parser::parse(".cv { content-visibility: auto; }");
    let lb = lumen_layout::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let _ = lumen_layout::take_cv_skipped();
    let mut found = Vec::new();
    collect_cv_auto(&lb, &mut found);
    assert_eq!(found.len(), 1, "РїСѓСЃС‚РѕР№ auto-Р±РѕРєСЃ С‚РѕР¶Рµ РЅР°Р±Р»СЋРґР°РµС‚СЃСЏ");
    assert!(
        !lumen_layout::cv_is_skipped(false, found[0].1, 0.0, 300.0),
        "РѕРЅ РІ РЅР°С‡Р°Р»Рµ СЃС‚СЂР°РЅРёС†С‹ вЂ” Р·РЅР°С‡РёС‚ relevant, Р° РЅРµ РїСЂРѕРїСѓС‰РµРЅ"
    );
}

#[test]
fn collect_cv_auto_reports_an_element_with_inline_content_once() {
    // BUG-852: an anonymous box carries the parent's style, so an auto
    // element with inline content produces TWO boxes reading
    // `content-visibility: auto` вЂ” the element and its `InlineRun`. Without
    // dedup by node `diff_cv_state` compares the second against a `prev`
    // it has not updated yet and hands the page a second event for one
    // change, which is the "already observed" rejection of
    // `content-visibility-auto-state-changed-first-observation.html`.
    lumen_layout::set_cv_scroll(0.0, 0.0);
    lumen_layout::set_cv_relevant(std::collections::HashSet::new());
    let doc = lumen_html_parser::parse(r#"<div class="cv"><span>on</span></div>"#);
    let sheet = lumen_css_parser::parse(".cv { content-visibility: auto; }");
    let lb = lumen_layout::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let _ = lumen_layout::take_cv_skipped();
    let mut found = Vec::new();
    collect_cv_auto(&lb, &mut found);
    assert_eq!(found.len(), 1, "РѕРґРёРЅ СЌР»РµРјРµРЅС‚ вЂ” РѕРґРЅР° Р·Р°РїРёСЃСЊ, Р°РЅРѕРЅРёРјРЅС‹Р№ Р±РѕРєСЃ РЅРµ РІ СЃС‡С‘С‚");
}

fn expect_resolved_url(base: &str, href: &str) -> String {
    match ResourceBase::Url(base.to_owned()).resolve(href) {
        ResolvedResource::Url(u) => u,
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resource_base_url_absolute_path() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "/style.css"),
        "https://example.com/style.css",
    );
}

#[test]
fn resource_base_url_relative_same_dir() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "style.css"),
        "https://example.com/path/style.css",
    );
}

#[test]
fn resource_base_url_relative_subdirectory() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "css/main.css"),
        "https://example.com/path/css/main.css",
    );
}

#[test]
fn resource_base_url_root_base() {
    assert_eq!(
        expect_resolved_url("https://example.com/", "style.css"),
        "https://example.com/style.css",
    );
}

#[test]
fn resource_base_url_http_scheme_with_port() {
    assert_eq!(
        expect_resolved_url("http://localhost:8080/index.html", "/css/app.css"),
        "http://localhost:8080/css/app.css",
    );
}

#[test]
fn resource_base_url_absolute_href_passthrough() {
    // РђР±СЃРѕР»СЋС‚РЅС‹Р№ href СЃ http/https-СЃС…РµРјРѕР№ Р»РѕРІРёС‚СЃСЏ РІ РЅР°С‡Р°Р»Рµ ResourceBase::resolve
    // РґРѕ Url::resolve вЂ” СЌС‚Рѕ РїРѕР·РІРѕР»СЏРµС‚ href СЃ РґСЂСѓРіРёРј scheme Р±С‹С‚СЊ РІРёРґРёРјС‹Рј РєР°Рє Url,
    // РґР°Р¶Рµ РµСЃР»Рё base вЂ” File.
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let res = base.resolve("https://cdn.example.com/style.css");
    match res {
        ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resource_base_file_resolves_relative() {
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let res = base.resolve("style.css");
    match res {
        ResolvedResource::File(p) => {
            assert_eq!(p, PathBuf::from("samples/style.css"));
        }
        ResolvedResource::Url(_) => panic!("expected File"),
    }
}

#[test]
fn resource_base_file_absolute_url_passthrough() {
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let res = base.resolve("https://cdn.example.com/style.css");
    match res {
        ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resolve_str_url_base_relative() {
    let base = ResourceBase::Url("https://example.com/path/page.html".to_owned());
    assert_eq!(
        base.resolve_str("style.css"),
        "https://example.com/path/style.css"
    );
}

#[test]
fn resolve_str_url_base_absolute_passthrough() {
    let base = ResourceBase::Url("https://example.com/page.html".to_owned());
    assert_eq!(
        base.resolve_str("https://cdn.example.com/lib.js"),
        "https://cdn.example.com/lib.js"
    );
}

#[test]
fn resolve_str_file_base_yields_path_string() {
    let base = ResourceBase::File(PathBuf::from("/home/user/page.html"));
    let result = base.resolve_str("style.css");
    assert!(result.ends_with("style.css"), "got: {result}");
}

// ── BUG-440: File base must not smuggle a query/fragment into the filename,
// nor join an href that carries its own scheme onto its directory ─────────

#[test]
fn resolve_file_base_get_query_is_not_part_of_filename() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("target.html?q=hello") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_strips_fragment() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("target.html#sec") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_query_only_href_reloads_same_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("?q=hello") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/form.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_fragment_only_href_reloads_same_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("#sec") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/form.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_about_blank_is_a_url_not_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("about:blank") {
        ResolvedResource::Url(u) => assert_eq!(u, "about:blank"),
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_data_url_is_a_url_not_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("data:text/html,x") {
        ResolvedResource::Url(u) => assert_eq!(u, "data:text/html,x"),
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_plain_relative_href_unaffected() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("sub/../target.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/sub/../target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// GET form submission end-to-end through `make_get_url` + `resolve_str` —
/// the exact sequence BUG-440 was filed against (`action="target.html"`,
/// field `q=hello`, page opened as `file://`).
#[test]
fn bug440_get_form_submission_resolves_to_the_target_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form437target.html"));
    let get_url = forms::make_get_url("target.html", "q=hello");
    assert_eq!(base.resolve_str(&get_url), "D:/tmp\\target.html");
}

/// The other BUG-440 symptom: `action="about:blank"` must resolve to a
/// literal `"about:blank"` string so `PageSource::from_arg` recognizes it,
/// not a path like `"D:/tmp\about:blank"`.
#[test]
fn bug440_get_form_submission_with_scheme_action_resolves_to_url() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form437target.html"));
    let get_url = forms::make_get_url("about:blank", "");
    assert_eq!(base.resolve_str(&get_url), "about:blank");
}

/// A `file:` href on a local page is still a local file: resolving it to a
/// `Url` would only move the defect one caller along, where `PathBuf` gets
/// the whole `file://...` string verbatim (the BUG-651 shape).
#[test]
fn resolve_file_base_file_scheme_href_yields_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("file:///D:/other/x.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/other/x.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// The path component of a URL reference is percent-encoded, so a file whose
/// name holds a space arrives as `%20` and has to be decoded before the
/// filesystem is asked for it.
#[test]
fn resolve_file_base_decodes_percent_escapes() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("my%20file.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/my file.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// A `%` that is not a valid escape is a legal filename character and must
/// survive verbatim — decoding it away would name a different file.
#[test]
fn resolve_file_base_keeps_a_lone_percent() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("100%done.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/100%done.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// A Windows drive letter is not a URI scheme: one character cannot be one,
/// and reading `D:/docs/x.html` as a `d:` URL would send a perfectly good
/// local href to the network layer instead of to disk.
#[test]
fn resolve_file_base_drive_letter_is_not_a_scheme() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("D:/docs/x.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/docs/x.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// An automation `file://` URL goes through the same rule as a `file:` href
/// (BUG-440 folded the two onto `file_url_to_path`), so a percent-escaped
/// name navigates as well from BiDi/MCP as it resolves inside a page.
#[test]
fn automation_file_url_decodes_percent_escapes() {
    match page_source_for_automation_url("file:///D:/tmp/my%20page.html") {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/my page.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// A bare CLI path is not a URL: its `%` is a literal character of the
/// filename, so the automation fallback must not decode it.
#[test]
fn automation_bare_path_is_not_percent_decoded() {
    match page_source_for_automation_url("D:/tmp/100%done.html") {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/100%done.html")),
        other => panic!("expected File, got {other:?}"),
    }
}
