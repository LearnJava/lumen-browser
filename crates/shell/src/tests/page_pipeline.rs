//! Конвейер страницы: пригодность к bfcache, разрешение JS-навигации,
//! вьюпорт контента, размеры картинок, маршрутизация в движковый поток,
//! полноэкранный опрос и состояние `content-visibility`.

use super::*;

// ── Ph3 P3-bfcache: Cache-Control: no-store eligibility filter ──────────

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

// ── BUG-293: JS-navigation URL resolution (window.open / location.href) ──

#[test]
fn resolve_js_nav_file_from_local_page_loads_from_disk() {
    // file→file: a local page opening a local file resolves to PageSource::File.
    let opener = PageSource::File(PathBuf::from("/home/x/index.html"));
    let src = resolve_js_navigation("file:///home/x/page.html", &opener)
        .expect("file→file must be allowed");
    match src {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("/home/x/page.html")),
        other => panic!("expected PageSource::File, got {other:?}"),
    }
}

#[test]
fn resolve_js_nav_file_strips_windows_drive_slash() {
    // file:///D:/… → D:/… (leading slash before the drive letter dropped).
    let opener = PageSource::File(PathBuf::from("D:/proj/index.html"));
    let src = resolve_js_navigation("file:///D:/proj/page.html", &opener)
        .expect("file→file must be allowed");
    match src {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("D:/proj/page.html")),
        other => panic!("expected PageSource::File, got {other:?}"),
    }
}

#[test]
fn resolve_js_nav_web_to_file_is_blocked() {
    // web→file: an http(s) page must not open a local file:// resource.
    let opener = PageSource::Url("https://example.com/".to_owned());
    let err = resolve_js_navigation("file:///etc/passwd", &opener)
        .expect_err("web→file must be blocked");
    assert!(err.contains("политикой безопасности"), "reason: {err}");
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
        .expect("non-web opener → file is allowed");
    assert!(matches!(src, PageSource::File(_)));
}

// ── RP-2: live layout viewport tracks window size, minus chrome ─────────

#[test]
fn content_layout_viewport_subtracts_tab_strip() {
    // Interactive window at 1280×800 → page content area excludes the tab
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
    // The interactive window opens at 1024 × (720 + toolbar::CHROME_H) so the
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
    // Headless (--screenshot/--dump/--ipc): no chrome → full surface,
    // keeping those paths deterministic at 1024×720.
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

// ── BUG-269: replaced-element intrinsic aspect-ratio sizing ─────────────

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
    // `<img width="240">` with intrinsic 120×80 → height = 240·80/120 = 160,
    // not the raw intrinsic 80 (and never a collapsed `height: auto` = 0).
    let (w, h) = img_dims(r#"<img src="p.png" width="240">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("240"));
    assert_eq!(h.as_deref(), Some("160"));
}

#[test]
fn bug269_fixed_height_derives_width_from_ratio() {
    // Symmetric case: author height only → width from the intrinsic ratio.
    let (w, h) = img_dims(r#"<img src="p.png" height="160">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("240"));
    assert_eq!(h.as_deref(), Some("160"));
}

#[test]
fn bug269_no_attrs_uses_raw_intrinsic() {
    // Neither dimension set → both filled with the raw intrinsic values.
    let (w, h) = img_dims(r#"<img src="p.png">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("120"));
    assert_eq!(h.as_deref(), Some("80"));
}

#[test]
fn bug269_both_attrs_unchanged() {
    // Both author dimensions set → intrinsic size never overrides them.
    let (w, h) = img_dims(r#"<img src="p.png" width="10" height="20">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("10"));
    assert_eq!(h.as_deref(), Some("20"));
}

#[test]
fn bug269_percentage_width_falls_back_to_intrinsic_height() {
    // A non-integer (percentage) width has no shell-resolvable px value to
    // drive the ratio, but the image must still be visible → fill the height
    // with the raw intrinsic value and leave the percentage width intact.
    let (w, h) = img_dims(r#"<img src="p.png" width="50%">"#, 120, 80);
    assert_eq!(w.as_deref(), Some("50%"));
    assert_eq!(h.as_deref(), Some("80"));
}

// ── BUG-735: сходимость повторного прохода intrinsic-размеров ───────────
//
// Streaming/динамический путь раздаёт размеры проходом
// `Lumen::apply_stream_intrinsic_sizes`, который просит релейаут ровно
// тогда, когда `apply_intrinsic_size` сообщил об изменении DOM. Проход сам
// заказывается после каждого релейаута (новый `<img>` с уже декодированным
// `src` своего `ImageDecoded` не получит — запрос дедуплицирован по URL),
// поэтому «ничего не дописал → false» — это то, на чём держится отсутствие
// петли «релейаут → проход → релейаут».

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
    assert!(first, "первый вызов дописывает width/height");
    assert!(!second, "повторный вызов не меняет DOM — релейаут не нужен");
}

#[test]
fn bug735_author_dimensions_report_no_change() {
    // Автор задал оба размера — дописывать нечего с самого начала.
    let (first, second) =
        img_apply_twice(r#"<img src="p.png" width="10" height="20">"#, 120, 80);
    assert!(!first);
    assert!(!second);
}

#[test]
fn bug735_half_filled_reports_change_once() {
    // Задана только ширина → дописывается высота из соотношения (изменение),
    // второй проход уже видит обе и молчит.
    let (first, second) = img_apply_twice(r#"<img src="p.png" width="240">"#, 120, 80);
    assert!(first);
    assert!(!second);
}

// ── BUG-171 этап 2: off-UI-thread финальный pipeline ────────────────────
//
// Финальный render (`render_bytes`) уезжает на фоновый поток, а готовый
// результат пересылается назад через `LoadEvent::RenderDone`. Это работает
// только если весь груз — `Send`: `RenderOutcome` (включая JS-хэндл за
// `Arc<dyn PersistentJs>`), сам `LoadEvent` и proxy. Регрессионная защита:
// если кто-то добавит `!Send`-поле в `LoadedPage`/`LayoutSource` или снимет
// `Send`/`Sync` с `PersistentJs`, эти ассерты перестанут компилироваться.

fn _assert_send<T: Send>() {}
fn _assert_sync<T: Sync>() {}

#[test]
fn render_pipeline_payload_is_send() {
    // Груз, пересекающий границу рендер-поток → UI-поток.
    _assert_send::<RenderOutcome>();
    _assert_send::<LoadEvent>();
    _assert_send::<Arc<dyn PersistentJs>>();
    _assert_send::<EventLoopProxy<LoadEvent>>();
    // Аргументы, уезжающие в рендер-поток.
    _assert_send::<Arc<KnuthLiangHyphenation>>();
    _assert_send::<RawPage>();
    // ADR-016 M2.2c-2b: хэндл разделяется между UI- и движковым потоком за
    // `Arc<dyn PersistentJs>`, поэтому обязан быть `Send + Sync`. Если кто-то
    // снимет `Sync` с `PersistentJs`, эта строка перестанет компилироваться.
    _assert_sync::<Arc<dyn PersistentJs>>();
}

// ── ADR-016 M2.2c-2b: engine-thread owns EngineJsState ──────────────────

#[test]
fn engine_js_state_default_is_empty() {
    // Свежее состояние движкового потока — без хэндла и без DOM (заполняется
    // `sync_engine_js_state` при первой загрузке страницы).
    let state = EngineJsState::default();
    assert!(state.js.is_none());
    assert!(state.document.is_none());
}

#[test]
fn engine_thread_carries_and_mutates_js_state() {
    // Реальный тип состояния shell (`EngineJsState`) живёт на движковом потоке:
    // `task` кладёт разделяемый `Document` (как это делает `sync_engine_js_state`),
    // `query` читает его обратно — end-to-end проверка, что зеркалирование
    // хэндла/DOM в состояние работает через настоящий `EngineThread<_, S>`.
    let engine = engine_thread::EngineThread::<u64, EngineJsState>::spawn()
        .expect("spawn engine thread");
    // Пусто до первого task (аналог движкового потока сразу после старта).
    assert_eq!(engine.query(|s| s.document.is_some()), Some(false));
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>hi</p>")));
    engine.task(move |s| s.document = Some(doc));
    // query исполняется после task (упорядоченный канал) → DOM на месте.
    assert_eq!(engine.query(|s| s.document.is_some()), Some(true));
}

#[test]
fn engine_thread_query_take_extracts_and_clears_state() {
    // ADR-016 M2.2c-2d (21): механизм, на котором стоит `Lumen::take_js_ctx` —
    // `save_page_snapshot` вынимает хэндл из движкового состояния блокирующим
    // `query`, `take`-ающим поле (state → snapshot). Проверяем на `document`
    // (тот же generic-путь, что и `state.js.take()`, но без mock-хэндла): после
    // депозита `task`-ом `query`-take возвращает значение и оставляет состояние
    // пустым, а повторный take — `None` (как `js_ctx` уехал в снапшот).
    let engine = engine_thread::EngineThread::<u64, EngineJsState>::spawn()
        .expect("spawn engine thread");
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>hi</p>")));
    engine.task(move |s| s.document = Some(doc));
    // Первый take извлекает депонированный Arc, состояние очищается.
    assert_eq!(
        engine.query(|s| s.document.take().map(|d| Arc::strong_count(&d))),
        Some(Some(1))
    );
    // Состояние теперь пусто — повторный take даёт `None` (`flatten` в
    // `take_js_ctx` схлопнет `Some(None)` → `None`).
    assert_eq!(engine.query(|s| s.document.take().is_some()), Some(false));
}

#[test]
fn route_eval_js_without_handle_is_noop() {
    // Флаг выключен и хэндла нет: маршрутизатор не должен паниковать и просто
    // ничего не делает (байт-идентично прежнему `if let Some(js) = … {}`).
    route_eval_js(None, None, "_lumen_run_navigate_handler()".to_owned());
}

#[test]
fn route_query_js_without_handle_is_none() {
    // Флаг выключен (`engine = None`) и хэндла нет (`js = None`): value-returning
    // маршрутизатор возвращает `None` → вызывающая сторона подставит ветку
    // «без JS» (напр. `unwrap_or(false)`), байт-идентично `js_ctx == None`.
    let r: Option<bool> = route_query_js(None, None, |j| j.take_dom_dirty());
    assert_eq!(r, None);
}

#[test]
fn route_query_js_flag_on_without_synced_handle_is_none() {
    // Флаг включён (движковый поток есть), но `EngineJsState.js` ещё не
    // зеркалирован (`sync_engine_js_state` не вызывался) → внутренний
    // `state.js.map(read)` даёт `None`, `flatten` схлопывает до `None`.
    // Замыкание `read` при этом НЕ исполняется — хэндла нет.
    let engine = engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn()
        .expect("spawn engine thread");
    let r: Option<bool> =
        route_query_js(Some(&engine), None, |j| j.take_dom_dirty());
    assert_eq!(r, None);
}

#[test]
fn route_query_js_nav_reads_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2c (остаток): nav/timer/nav-update чтения используют тот же
    // `route_query_js`, но с более богатыми типами возврата (`Option<_>` и `Vec<_>`).
    // Без хэндла (`engine = None`, `js = None`) внешний `Option` = `None`, поэтому
    // `flatten`/`unwrap_or_default` в вызывающих сайтах дают ту же ветку «без JS»,
    // что и прежние прямые вызовы: нет навигации, нет wakeup, пустой дренаж.
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
    // ADR-016 M2.2c-2d: canvas/history per-tick дренажи в `about_to_wait`
    // маршрутизируются тем же `route_query_js`. Без хэндла (`engine = None`,
    // `js = None`) внешний `Option` = `None`, поэтому `unwrap_or_default` в
    // вызывающих сайтах даёт пустой `Vec` — та же ветка «без JS», что и прежние
    // прямые `js_ctx.map(<drain>).unwrap_or_default()`.
    // Тип `R` выводится из возвращаемого значения замыкания — явную аннотацию
    // не пишем (сложный кортеж `flush_canvas_updates` иначе триггерит
    // clippy::type_complexity); цепляем `unwrap_or_default` сразу.
    let canvas = route_query_js(None, None, |j| j.flush_canvas_updates()).unwrap_or_default();
    assert!(canvas.is_empty());
    let hist_url = route_query_js(None, None, |j| j.take_history_url_updates()).unwrap_or_default();
    assert!(hist_url.is_empty());
    let hist_go = route_query_js(None, None, |j| j.take_history_traversals()).unwrap_or_default();
    assert!(hist_go.is_empty());
}

#[test]
fn bug428_canvas_updates_keyed_as_display_list_expects() {
    // BUG-428: headless CPU-рендер получил тот же дренаж канваса, что живой цикл.
    // Оба сайта строят ключ через `canvas_updates_as_images`, и он обязан совпасть
    // с тем, что кладёт в `DrawImage.src` эмиттер (`display_list.rs`:
    // `format!("canvas:{}", b.node.index())`) — иначе картинка не найдётся и
    // канвас снова нарисуется прозрачным.
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
    // Страница без канваса (или без JS-контекста) не должна подмешивать записи
    // в набор картинок CPU-растеризатора — дренаж пуст, набор не меняется.
    assert!(canvas_updates_as_images(Vec::new()).is_empty());
}

#[test]
fn route_query_js_nav_intercept_without_handle_defaults_to_no_op() {
    // ADR-016 M2.2c-2d: последнее синхронное read-after-eval чтение —
    // `take_nav_intercept_result` в nav-методах (`navigate_to`/`_replace`/
    // `_back`/`_forward`) — маршрутизируется тем же `route_query_js`. Без хэндла
    // (`engine = None`, `js = None`) внешний `Option` = `None`, поэтому вызывающий
    // `if let Some(intercept) = …` пропускает весь intercept-блок — та же ветка
    // «без JS», что и прежний `if let Some(js) = &self.js_ctx { … }`.
    let intercept: Option<Vec<(bool, bool)>> =
        route_query_js(None, None, |j| j.take_nav_intercept_result());
    assert!(intercept.is_none());
}

#[test]
fn route_query_js_pointer_capture_and_raf_reads_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2d: последние синхронные value-returning UI→JS чтения —
    // pre-dispatch pointer-capture (`pointer_capture_nid`/`take_pointer_capture`
    // в mouseup/pointermove) и wait-poll `has_raf_pending` (`WaitCondition::JsIdle`)
    // — маршрутизируются тем же `route_query_js`. Без хэндла (`engine = None`,
    // `js = None`) внешний `Option` = `None`, поэтому `flatten` в capture-сайтах
    // даёт `hit_nid`/пропуск lostpointercapture, а `unwrap_or(false)` + отрицание
    // в JsIdle даёт `idle = true` — та же ветка «без JS», что и прежние прямые
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
    // ADR-016 M2.2c-2d: layout-geometry push (`update_layout_rects` и Co.). The
    // relayout observer-delivery site wraps its whole ordered void+read sequence
    // in one `route_query_js` returning the drained lazy-image requests. Без хэндла
    // (`engine = None`, `js = None`) внешний `Option` = `None`, поэтому
    // `unwrap_or_default` даёт пустой `Vec` — та же ветка «без JS», что и прежний
    // `if let Some(js) = &self.js_ctx { … }` (оставлявший `lazy_reqs` пустым, а
    // seed-сайты — не диспатчившими push).
    let lazy: Vec<(u32, String)> =
        route_query_js(None, None, |j| j.take_lazy_image_requests()).unwrap_or_default();
    assert!(lazy.is_empty());
}

#[test]
fn route_lazy_pageshow_resize_without_handle_default_to_no_op() {
    // ADR-016 M2.2c-2d (17): lazy-image setup + pageshow lifecycle + resize-eval.
    // The lazy setup wraps register→push→deliver→drain in one `route_query_js`
    // returning the requests; pageshow (`notify_window_loaded` +
    // `fire_page_lifecycle`) and resize-eval (`_lumen_apply_resize`) are routed
    // fire-and-forget. Без хэндла (`engine = None`, `js = None`) все три — no-op:
    // `unwrap_or_default` даёт пустой `Vec`, void-действия не исполняются — та же
    // ветка «без JS», что и прежние прямые `if let Some(js) = &self.js_ctx { … }`.
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
    // ADR-016 M2.2d (18): tab park/unpark в `switch_tab`. Park
    // (`pause_event_loop` перед `save_page_snapshot`) и unpark
    // (`unpause_event_loop` + `run_gc_pass(0)` после `restore_page_snapshot`) —
    // последние прямые `self.js_ctx`-обращения, переведённые на `route_task_js`.
    // Без хэндла (`engine = None`, `js = None`) оба — no-op: void-действия не
    // исполняются, паники нет — та же ветка «без JS», что и прежние прямые
    // `if let Some(js) = &self.js_ctx { … }`.
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
    // heap probe (`debug_js_heap`, value read) through `route_query_js`. Без хэндла
    // (`engine = None`, `js = None`) delivery — no-op, а heap-чтение возвращает
    // `None` → `unwrap_or((-1, -1))` = прежний `map_or((-1, -1), …)`; та же ветка
    // «без JS», что и прежние прямые обращения к полю.
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

// ── ADR-016 M2.2c-2d: generic void-action router (pump/tick batch) ──────

#[test]
fn route_task_js_without_handle_is_noop() {
    // Флаг выключен и хэндла нет: обобщённый void-маршрутизатор не паникует и
    // действие не исполняется (байт-идентично прежнему `if let Some(js) = … {}`).
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);
    route_task_js(None, None, move |_j| {
        ran2.store(true, std::sync::atomic::Ordering::SeqCst)
    });
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn route_task_js_flag_on_without_synced_handle_skips_action() {
    // Флаг включён (движковый поток есть), но `EngineJsState.js` ещё не
    // зеркалирован → задача исполняется на потоке, но `state.js == None`, поэтому
    // само действие (pump-батч) пропускается. Барьер-`query` упорядочен после
    // `task`, так что к моменту проверки задача гарантированно отработала.
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

// ── Fullscreen viewport reconciliation (BUG-167) ────────────────────────

#[test]
fn fullscreen_poll_applies_when_size_changed() {
    // OS resized the window to the fullscreen area → apply resize + relayout.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1920, 1080), 240),
        FullscreenPoll::Apply(1920, 1080)
    );
}

#[test]
fn fullscreen_poll_waits_while_size_unchanged() {
    // OS has not applied the new size yet → keep polling, one attempt spent.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (1024, 756), 240),
        FullscreenPoll::Wait(1024, 756, 239)
    );
}

#[test]
fn fullscreen_poll_waits_on_zero_size() {
    // Minimized / not-yet-mapped window reports 0×0 → treat as not applied.
    assert_eq!(
        decide_fullscreen_poll((1024, 756), (0, 0), 5),
        FullscreenPoll::Wait(1024, 756, 4)
    );
}

#[test]
fn fullscreen_poll_gives_up_when_budget_exhausted() {
    // Last attempt with no size change → stop polling, do not spin the loop.
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
    // Exiting fullscreen shrinks back to a windowed size — also a change.
    assert_eq!(
        decide_fullscreen_poll((1920, 1080), (1024, 756), 240),
        FullscreenPoll::Apply(1024, 756)
    );
}

// ── content-visibility: auto — shell state (BB-4) ───────────────────────

fn cv_nid(n: usize) -> NodeId {
    NodeId::from_index(n)
}

fn cv_state(pairs: &[(usize, bool)]) -> std::collections::HashMap<NodeId, bool> {
    pairs.iter().map(|&(n, s)| (cv_nid(n), s)).collect()
}

#[test]
fn diff_cv_state_emits_on_first_observation_both_ways() {
    // CSS Contain L2 §4.1: the first observation of an element fires in
    // BOTH directions — this is what
    // `content-visibility-auto-state-changed-first-observation.html`
    // asserts, and what the old "new node ⇒ skipped: true" diff could not
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
    assert_eq!(found.len(), 2, "оба auto-бокса, а не только пропущенный");
    let states: Vec<bool> = found
        .iter()
        .map(|&(_, top)| lumen_layout::cv_is_skipped(false, top, 0.0, 300.0))
        .collect();
    assert_eq!(states, vec![false, true], "первый во вьюпорте, второй под ним");
}

#[test]
fn collect_cv_auto_reports_an_empty_auto_box_by_position() {
    // BUG-852: an empty `content-visibility: auto` element used to be
    // indistinguishable from a skipped one ("no children ⇒ skipped"), so
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
    assert_eq!(found.len(), 1, "пустой auto-бокс тоже наблюдается");
    assert!(
        !lumen_layout::cv_is_skipped(false, found[0].1, 0.0, 300.0),
        "он в начале страницы — значит relevant, а не пропущен"
    );
}

#[test]
fn collect_cv_auto_reports_an_element_with_inline_content_once() {
    // BUG-852: an anonymous box carries the parent's style, so an auto
    // element with inline content produces TWO boxes reading
    // `content-visibility: auto` — the element and its `InlineRun`. Without
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
    assert_eq!(found.len(), 1, "один элемент — одна запись, анонимный бокс не в счёт");
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
    // Абсолютный href с http/https-схемой ловится в начале ResourceBase::resolve
    // до Url::resolve — это позволяет href с другим scheme быть видимым как Url,
    // даже если base — File.
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

// -- BUG-443: parse-time getComputedStyle / getBoundingClientRect ------------

/// Run one page through the whole [`parse_and_layout`] pipeline with defaults.
#[cfg(feature = "v8")]
fn parse_and_layout_for_test(html: &str) -> crate::page_pipeline::ParsedPage {
    parse_and_layout(
        html.as_bytes(),
        Some("text/html"),
        &ResourceBase::File(std::env::temp_dir().join("bug443.html")),
        &(Arc::new(StdoutEventSink) as Arc<dyn EventSink>),
        Size::new(1024.0, 720.0),
        &mut std::collections::HashSet::new(),
        None, None, None, None,
        &NullHyphenationProvider,
        false,
        deterministic::DetConfig::default(),
        false,
        None,
        false,
        None, None,
        lumen_core::ColorSpace::Srgb,
        false,
    )
    .expect("pipeline must not fail on a well-formed page")
}

/// Read back an attribute a probe script wrote onto `<html>`.
#[cfg(feature = "v8")]
fn probe_attr(page: &crate::page_pipeline::ParsedPage, name: &str) -> String {
    let doc = page.document.lock().expect("document lock");
    let mut found = None;
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let NodeData::Element { attrs, .. } = &node.data
            && let Some(a) = attrs.iter().find(|a| a.name.local == name)
        {
            found = Some(a.value.clone());
            break;
        }
        stack.extend(node.children.iter().copied());
    }
    found.unwrap_or_default()
}

/// An inline `<script>` running during parsing reads the real computed style
/// and the real border box, not `""` and a zero rect. Before BUG-443 the
/// cascade and the first layout both ran *after* the page's scripts, so every
/// such read answered empty.
#[cfg(feature = "v8")]
#[test]
fn parse_time_script_reads_computed_style_and_rect() {
    let page = parse_and_layout_for_test(
        "<html><head><style>#t{width:300px;height:120px;color:rgb(1,2,3)}</style></head>\
         <body><div id=t>t</div>\
         <script>var e=document.getElementById('t');var r=e.getBoundingClientRect();\
         document.documentElement.setAttribute('data-w',getComputedStyle(e).getPropertyValue('width'));\
         document.documentElement.setAttribute('data-c',getComputedStyle(e).getPropertyValue('color'));\
         document.documentElement.setAttribute('data-r',r.width+'x'+r.height);</script>\
         </body></html>",
    );
    assert_eq!(probe_attr(&page, "data-w"), "300px");
    assert_eq!(probe_attr(&page, "data-c"), "rgb(1, 2, 3)");
    assert_eq!(probe_attr(&page, "data-r"), "300x120");
}

/// A `DOMContentLoaded` handler sees geometry that includes what the scripts
/// themselves changed — the snapshot is re-derived after they run, not reused
/// from before them.
#[cfg(feature = "v8")]
#[test]
fn dom_content_loaded_handler_sees_post_script_geometry() {
    let page = parse_and_layout_for_test(
        "<html><head><style>#t{width:100px;height:50px}</style></head>\
         <body><div id=t>t</div>\
         <script>var s=document.createElement('style');\
         s.textContent='#t{width:250px;height:70px}';document.head.appendChild(s);\
         document.addEventListener('DOMContentLoaded',function(){\
         var e=document.getElementById('t');var r=e.getBoundingClientRect();\
         document.documentElement.setAttribute('data-r',r.width+'x'+r.height);});</script>\
         </body></html>",
    );
    assert_eq!(probe_attr(&page, "data-r"), "250x70");
}

/// BUG-470: `prop in getComputedStyle(el)` used to be `false` for every
/// property, not just `float`/`clear` — the returned `Proxy({}, handler)`
/// had no `has` trap, so `in` fell through to `Reflect.has` on the empty
/// target object regardless of what `get` would have answered. `float`/
/// `clear` themselves were already in `computed_style_to_map` (since before
/// this bug was even filed); the gap was entirely in the `in` check WPT's
/// `assert_not_inherited()` starts with.
#[cfg(feature = "v8")]
#[test]
fn computed_style_in_operator_reports_known_properties() {
    let page = parse_and_layout_for_test(
        "<html><body><div id=t style='float:left'>t</div>\
         <script>var e=document.getElementById('t');var cs=getComputedStyle(e);\
         document.documentElement.setAttribute('data-float','float' in cs);\
         document.documentElement.setAttribute('data-clear','clear' in cs);\
         document.documentElement.setAttribute('data-bogus','__lumenBogusProp__' in cs);\
         document.documentElement.setAttribute('data-float-val',cs.getPropertyValue('float'));\
         </script></body></html>",
    );
    assert_eq!(probe_attr(&page, "data-float"), "true");
    assert_eq!(probe_attr(&page, "data-clear"), "true");
    assert_eq!(probe_attr(&page, "data-bogus"), "false");
    assert_eq!(probe_attr(&page, "data-float-val"), "left");
}

/// BUG-473: `CSSStyleDeclaration.cssText`/`getPropertyValue` used to
/// serialize declarations by naive `k+': '+v` concatenation — no trailing
/// `;`, no TRBL-longhand-to-shorthand collapsing, and (via the old raw-text
/// `cssText` getter) no normalization of a `var()` value's surrounding
/// whitespace. All three are exercised through one inline `style` attribute.
#[cfg(feature = "v8")]
#[test]
fn inline_style_serialization_collapses_shorthand_and_normalizes_text() {
    let page = parse_and_layout_for_test(
        "<html><body><div id=t style='margin-top:10px;margin-right:10px;\
         margin-bottom:10px;margin-left:10px;left:10px;font-size:var( --a )'>t</div>\
         <script>var e=document.getElementById('t');\
         document.documentElement.setAttribute('data-css-text',e.style.cssText);\
         document.documentElement.setAttribute('data-margin',e.style.getPropertyValue('margin'));\
         </script></body></html>",
    );
    let css_text = probe_attr(&page, "data-css-text");
    assert!(
        css_text.contains("margin: 10px;"),
        "shorthand not collapsed: {css_text}"
    );
    assert!(
        css_text.ends_with("font-size: var( --a );"),
        "trailing `;` missing or var() whitespace not preserved: {css_text}"
    );
    assert_eq!(probe_attr(&page, "data-margin"), "10px");
}

/// A `<style>` a script inserts still reaches the FIRST cascade: the CSS is
/// collected before the scripts now, so the pipeline has to notice the change
/// and rebuild. Asserted on the final layout, not on JS.
#[cfg(feature = "v8")]
#[test]
fn script_inserted_style_still_reaches_the_first_layout() {
    let page = parse_and_layout_for_test(
        "<html><head><style>#t{width:100px;height:50px}</style></head>\
         <body><div id=t>t</div>\
         <script>var s=document.createElement('style');\
         s.textContent='#t{width:250px;height:70px}';document.head.appendChild(s);</script>\
         </body></html>",
    );
    let mut found = None;
    let mut stack = vec![&page.layout];
    while let Some(b) = stack.pop() {
        if b.rect.width == 250.0 && b.rect.height == 70.0 {
            found = Some(b.rect);
            break;
        }
        stack.extend(b.children.iter());
    }
    assert!(found.is_some(), "script-inserted <style> did not reach the layout");
}
