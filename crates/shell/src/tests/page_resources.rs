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
    // downcast — `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    assert_eq!(events.len(), 2);

    // CSS (High) сортируется перед JS (Medium) независимо от source-order
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
    // rel="preload stylesheet" создаёт два хинта на один href
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
    // downcast — `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    // style.css появляется дважды — должен emit-иться один раз
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
    // Второй вызов с тем же seen-набором не должен повторно эмитить.
    use lumen_html_parser::PreloadHint;
    use std::sync::{Arc, Mutex};

    struct CollectingSink(Mutex<Vec<Event>>);
    impl EventSink for CollectingSink {
        fn emit(&self, e: &Event) { self.0.lock().unwrap().push(e.clone()); }
    }

    let sink: Arc<dyn EventSink> = Arc::new(CollectingSink(Mutex::new(Vec::new())));
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let mut seen = std::collections::HashSet::new();

    // Первый вызов — ранний скан (streaming chunk)
    let early = vec![PreloadHint::Stylesheet { url: "reset.css".into(), media: None }];
    dispatch_preload_hints(&early, &base, &sink, &mut seen);

    // Второй вызов — финальный pipeline: те же хинты + новый
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
    // downcast — `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    // reset.css — один раз (из первого вызова), hero.png — один раз (из второго)
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
    // Source-order: img (Low) → script (Medium) → css (High)
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
    // downcast — `EventSink` has no `Any` supertrait to do it safely.
    let events = unsafe { (*sink_any).0.lock().unwrap() };
    assert_eq!(events.len(), 3);

    // После сортировки: css(High) → js(Medium) → img(Low)
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

/// BUG-804: исход каждого `<link rel=stylesheet>` возвращается по узлам, в
/// порядке объявления, и провал не выпадает из списка — иначе элементу
/// негде выстрелить `error`. `samples/` заведомо не содержит этих файлов,
/// так что оба листа тут «не пришли»; проверяется связь узел↔исход, а не
/// сеть.
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
    // Разные элементы — разные узлы: без этого страница не смогла бы
    // отличить, какой именно `<link>` отчитался.
    assert_ne!(outcomes[0].0, outcomes[1].0);
}

/// BUG-480 срез 11: проход подресурсов под-документа фрейма запрашивает
/// `<link rel=stylesheet>` (оба исхода) и `<img>` (оба исхода), не трогает
/// `rel=alternate`/`loading="lazy"` и возвращает исходы в порядке DOM.
#[test]
fn frame_subresources_fetch_links_and_imgs_with_outcomes() {
    let dir = std::env::temp_dir().join("lumen_frame_subresources_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ok.css"), "p { color: red; }").unwrap();
    // BUG-480 срез 15: картинка теперь ДЕКОДИРУЕТСЯ, а не только читается с
    // диска, поэтому фикстура обязана быть настоящим PNG — исход `<img>` стал
    // означать «пиксели есть», а не «байты прочитались».
    std::fs::write(dir.join("ok.png"), tiny_png(4, 2)).unwrap();
    // missing.css / missing.png не создаются.

    // URL-ы ОТНОСИТЕЛЬНЫЕ (разрешаются от базы ребёнка ниже): так пишет живая
    // разметка, и только на них видно, что ключ регистрации картинки — не
    // сырой `src` (BUG-480 срез 15).
    let doc = lumen_html_parser::parse(
        r#"<html><head>
                 <link rel="stylesheet" href="ok.css">
                 <link rel="alternate" href="ignored.css">
                 <link rel="stylesheet" href="missing.css">
               </head><body>
                 <img src="ok.png">
                 <img src="missing.png">
                 <img loading="lazy" src="lazy.png">
               </body></html>"#,
    );
    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }
    let base = ResourceBase::File(dir.join("index.html"));
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let mut doc = doc;
    let out = fetch_frame_subresources(
        &mut doc,
        &base,
        &sink,
        None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        Size::new(1024.0, 720.0),
        lumen_core::ColorSpace::Srgb,
    );

    assert_eq!(out.links.len(), 2, "rel=alternate is not a cascade sheet");
    assert!(out.links[0].1, "ok.css exists");
    assert!(!out.links[1].1, "missing.css must report error");
    assert_ne!(out.links[0].0, out.links[1].0);

    assert_eq!(out.images.len(), 2, "loading=lazy is not requested at all");
    assert!(out.images[0].1, "ok.png exists");
    assert!(!out.images[1].1, "missing.png must report error");

    // BUG-480 срез 15: пиксели уехали наружу под РАЗРЕШЁННЫМ ключом, а карта
    // ключей описывает ОБЕ картинки — битую тоже, иначе её сырой `src` в
    // display list совпал бы с чужим зарегистрированным ключом страницы.
    assert_eq!(out.decoded_images.len(), 1, "декодируется только существующая");
    assert!(
        out.decoded_images[0].0.ends_with("ok.png") && out.decoded_images[0].0 != "ok.png",
        "ключ — разрешённый адрес, а не сырой src: {}",
        out.decoded_images[0].0
    );
    assert_eq!(out.decoded_images[0].1.width, 4, "пиксели настоящие, 4x2");
    // FRAME-5 срез 2: lazy.png тоже получает ключ (не байты) — иначе его
    // placeholder-заглушка в display list ребёнка не была бы переписана
    // `rekey_frame_images`, и уже загруженная позже картинка не нашла бы
    // адресата.
    assert_eq!(out.image_keys.len(), 3, "в карте битая и ленивая картинки тоже");
    assert!(out.image_keys.iter().all(|(raw, key)| raw != key), "ключ отличается от src");
    assert_eq!(out.lazy_requests.len(), 1, "собран, но не загружен — сеть его не видела");
    assert_eq!(out.lazy_requests[0].url, "lazy.png");
    // Intrinsic-размеры дописаны в дерево ребёнка: без них `<img>` без
    // атрибутов лёг бы нулевым боксом внутри фрейма.
    let img = out.images[0].0;
    let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(img).data else {
        panic!("<img> is an element");
    };
    let w = attrs.iter().find(|a| a.name.local == "width").map(|a| a.value.clone());
    assert_eq!(w.as_deref(), Some("4"), "intrinsic width из декодированной картинки");
}

/// Настоящий PNG `w`×`h` (непрозрачный) для фикстур: `decode_image` обязан его
/// разобрать, поэтому строка «bytes» тут больше не годится.
///
/// `pub(crate)`, а не module-private: FRAME-5 срез 2 (`scripts_and_frames.rs`)
/// нужен тот же настоящий PNG для теста ленивой дозагрузки фрейма — заводить
/// вторую копию ради одного файла было бы просто дублированием.
pub(crate) fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    let img = lumen_image::Image {
        width: w,
        height: h,
        format: lumen_image::PixelFormat::Rgba8,
        data: vec![255u8; (w * h * 4) as usize],
        icc_profile: None,
    };
    lumen_image::encode_png_rgba8(&img).unwrap()
}

// ──────────────── @import file loading (CSS Cascade L4 §6.5) ─────────────

/// Создаёт уникальную временную директорию для CSS-фикстур `@import`-теста,
/// очищая прошлый прогон. Возвращает путь директории.
fn import_fixture_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lumen_import_test_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn null_sink() -> Arc<dyn EventSink> {
    Arc::new(StdoutEventSink)
}

/// `@import` предпосылает содержимое импортированного листа собственным
/// правилам импортирующего (импорт «раньше» в каскаде).
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

/// BUG-743: результат всегда **оканчивается** исходным текстом листа —
/// на этом инварианте держится вырезание `imports_prefix` в
/// `parse_and_layout` (префикс = всё, что длиннее исходного текста).
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
        assert!(out.ends_with(text), "результат не оканчивается исходником: {out:?}");
    }
}

// ──────────────── BUG-743: отпечаток инлайновых <style> ──────────────────

/// Вставка нового `<style>` меняет отпечаток — иначе поздний динамический
/// лист не пересоберёт каскад.
#[test]
fn inline_style_fingerprint_detects_added_block() {
    let a = lumen_html_parser::parse("<html><head><style>.a{color:red}</style></head><body></body></html>");
    let b = lumen_html_parser::parse(
        "<html><head><style>.a{color:red}</style><style>.b{color:blue}</style></head><body></body></html>",
    );
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// Правка текста блока без изменения длины тоже меняет отпечаток —
/// счётчика блоков или суммарной длины было бы недостаточно.
#[test]
fn inline_style_fingerprint_detects_same_length_edit() {
    let a = lumen_html_parser::parse("<html><head><style>.a{color:red}</style></head></html>");
    let b = lumen_html_parser::parse("<html><head><style>.a{color:RED}</style></head></html>");
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// Один и тот же документ даёт один и тот же отпечаток, а перестановка
/// текста между двумя блоками — разный (границы блоков учитываются).
#[test]
fn inline_style_fingerprint_is_stable_and_block_aware() {
    let src = "<html><head><style>.a{}</style><style>.b{}</style></head></html>";
    let a = lumen_html_parser::parse(src);
    let b = lumen_html_parser::parse(src);
    assert_eq!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
    let merged = lumen_html_parser::parse("<html><head><style>.a{}.b{}</style></head></html>");
    assert_ne!(inline_style_fingerprint(&a), inline_style_fingerprint(&merged));
}

/// Документ без единого `<style>` — отпечаток считается и не паникует.
#[test]
fn inline_style_fingerprint_handles_document_without_styles() {
    let a = lumen_html_parser::parse("<html><body><p>текст</p></body></html>");
    let b = lumen_html_parser::parse("<html><body><p>другой</p></body></html>");
    assert_eq!(inline_style_fingerprint(&a), inline_style_fingerprint(&b));
}

/// Вложенные `@import` (a → b → c) разворачиваются в порядке c, b, a.
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

/// Циклический `@import` (a → b → a) завершается без бесконечной рекурсии.
#[test]
fn inline_css_imports_cycle_guard() {
    let dir = import_fixture_dir("cycle");
    std::fs::write(dir.join("a.css"), "@import url(b.css);\n.a{}").unwrap();
    std::fs::write(dir.join("b.css"), "@import url(a.css);\n.b{}").unwrap();
    let base = ResourceBase::File(dir.join("a.css"));
    // Начинаем с содержимого a.css — тот же файл будет импортирован из b.
    let out = inline_css_imports(
        "@import url(b.css);\n.a{}", &base, &null_sink(), None,
        &screen_media_context(Size::new(1024.0, 720.0), false),
        &mut std::collections::HashSet::new(), 0,
    );
    // Каждый лист загружен максимум один раз (guard по `seen`).
    assert_eq!(out.matches(".b{}").count(), 1);
}

/// `@import url(x) print;` не загружается под экранным контекстом.
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

/// Отсутствующий импортируемый файл не валит рендер — текст возвращается,
/// собственные правила сохранены.
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

/// Текст без `@import` возвращается без изменений (быстрый путь).
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
    // print.css отсеян; huge.css отсеян (viewport 1024px < 5000px); остальные — да.
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
    // RCDATA-режим декодирует &amp; → '&' прямо в tokenizer-е.
    let doc = lumen_html_parser::parse(
        r#"<html><head><title>Дом &amp; Сад</title></head><body></body></html>"#,
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("Дом & Сад"));
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
    // Lenient: если страница объявила <title> дважды, берём первый.
    let doc = lumen_html_parser::parse(
        "<html><head><title>A</title><title>B</title></head><body></body></html>",
    );
    assert_eq!(extract_title(&doc).as_deref(), Some("A"));
}

#[test]
fn window_title_with_page() {
    assert_eq!(window_title(Some("Foo")), "Foo — Lumen");
}

#[test]
fn window_title_fallback() {
    // Fallback содержит версию пакета — проверяем префикс.
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
    // Без Ctrl — обычная буква, не команда. Защита от перехвата ввода
    // в омнибокс (когда он появится).
    assert_eq!(keybinding_for(KeyCode::KeyR, ModifiersState::empty()), None);
}

#[test]
fn keybinding_ctrl_shift_r_is_read_later() {
    // Ctrl+Shift+R → toggle Read-later panel (§12.3).
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
    // Esc + любые модификаторы — не наша команда (рамп для будущего).
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
    // F без модификаторов открывает hint-режим kbd-навигации.
    assert_eq!(
        keybinding_for(KeyCode::KeyF, ModifiersState::empty()),
        Some(KeyCommand::HintModeOpen)
    );
}
