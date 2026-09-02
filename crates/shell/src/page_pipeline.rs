//! The page-load pipeline's entry point: [`render_bytes`] turns fetched bytes
//! into a laid-out, painted page, and [`dispatch_preload_hints`] emits the
//! preload-scanner hints the parser found along the way.
//!
//! Below them (batch SH-3d) is the phase `render_bytes` is a wrapper around —
//! [`parse_and_layout`], shared with the headless dump modes — and the three
//! shapes a page takes on the way through: [`ParsedPage`] (what `decode →
//! parse → layout` produced), [`LoadedPage`] (what the window is to draw and
//! be titled with) and [`LayoutSource`] (what a later reflow re-runs from
//! without touching the network).
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-3c, SH-3d); behaviour
//! and signatures are unchanged.

use crate::*;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    cross_origin_isolated: bool,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    target: lumen_core::ColorSpace,
    cache_control_no_store: bool,
) -> Result<RenderedPage, Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport, preload_seen, ls_store, ss_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic, dark_mode, cookie_jar, cross_origin_isolated, sw_worker_store, cache_backend, target, false)?;
    let display_list = paint_ordered(&parsed.layout);
    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд, {} картинок, {} preload-хинтов",
        parsed.document.lock().unwrap().len(),
        parsed.rule_count,
        display_list.len(),
        parsed.images.len(),
        parsed.preload_hints.len(),
    );
    let layout_box = parsed.layout;
    let layout_source = LayoutSource {
        document: Arc::clone(&parsed.document),
        stylesheet: Arc::new(parsed.stylesheet),
        html_source: Some(parsed.html_source),
        cache_control_no_store,
        dynamic_css: Some(parsed.dynamic_css),
    };
    Ok((
        LoadedPage {
            display_list,
            title: parsed.title,
            images: parsed.images,
            animated_gifs: parsed.animated_gifs,
            lazy_pairs: parsed.lazy_pairs,
            layout_box,
            font_registry: parsed.font_registry,
            pending_web_fonts: parsed.pending_web_fonts,
            js_navigate: parsed.js_navigate,
            page_tracks: parsed.page_tracks,
            frames: parsed.frames,
            frame_env: Some(parsed.frame_env),
        },
        layout_source,
        parsed.js_ctx,
    ))
}

/// Отправить preload-хинты в EventSink.
///
/// Каждый `PreloadHint` резолвится относительно `base` (4B.3) и
/// преобразуется в `Event::SubresourceHintFound { url, kind, priority }`.
/// Хинты сортируются по убыванию приоритета (High → Medium → Low), чтобы
/// самые критичные ресурсы стартовали первыми (полезно при HTTP/2).
/// `srcset`-строки эмитятся как-есть (multi-URL формат — задача picker-а).
/// `seen` — набор уже отправленных URL (cross-call дедупликация); caller
/// передаёт `&mut HashSet::new()` для одноразового вызова или persistent-сет
/// для дедупа между streaming-сканом и финальным pipeline.
/// Sink логирует хинт в stderr. Сам fetch по хинту делает JS-шим на элементе
/// `<link>` (BUG-826) — там же, где живут его события `load`/`error`; здесь
/// сетевого запроса по-прежнему нет, поэтому строка лога говорит «хинт найден»,
/// а не «ресурс запрошен».
pub(crate) fn dispatch_preload_hints(
    hints: &[lumen_html_parser::PreloadHint],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    seen: &mut std::collections::HashSet<String>,
) {
    use lumen_html_parser::PreloadHint;

    // Первый проход: резолв URL + вычисление kind.
    let mut resolved: Vec<(String, SubresourceKind)> = Vec::with_capacity(hints.len());
    for hint in hints {
        let pair = match hint {
            PreloadHint::Stylesheet { url, .. } =>
                (base.resolve_str(url), SubresourceKind::Stylesheet),
            PreloadHint::Script { url } =>
                (base.resolve_str(url), SubresourceKind::Script),
            PreloadHint::Image { url: Some(url), .. } =>
                (base.resolve_str(url), SubresourceKind::Image),
            // srcset содержит список URL — резолвинг каждого кандидата
            // откладывается до picker-а; эмитим srcset-строку как-есть.
            PreloadHint::Image { url: None, srcset: Some(s), .. } =>
                (s.clone(), SubresourceKind::Image),
            PreloadHint::SourceSet { srcset, .. } =>
                (srcset.clone(), SubresourceKind::Image),
            PreloadHint::Preload { url, as_kind } => {
                let kind = match as_kind.as_deref() {
                    Some("font") => SubresourceKind::Font,
                    Some("image") => SubresourceKind::Image,
                    Some("script") => SubresourceKind::Script,
                    Some("style") => SubresourceKind::Stylesheet,
                    _ => SubresourceKind::Other { as_kind: as_kind.clone() },
                };
                (base.resolve_str(url), kind)
            }
            // BUG-826: остальные два вида author-хинта. Реальный fetch и
            // события `load`/`error` для них делает JS-шим на самом элементе
            // (`_lumen_link_hint_prepare`), здесь — только строка сетевого лога.
            PreloadHint::ModulePreload { url } =>
                (base.resolve_str(url), SubresourceKind::Script),
            PreloadHint::Prefetch { url } =>
                (base.resolve_str(url), SubresourceKind::Other { as_kind: Some("prefetch".into()) }),
            // Preconnect URL — origin, не содержит path — резолвинг тривиален.
            PreloadHint::Preconnect { url, dns_only } =>
                (base.resolve_str(url), SubresourceKind::Preconnect { dns_only: *dns_only }),
            PreloadHint::Image { url: None, srcset: None, .. } => continue,
        };
        resolved.push(pair);
    }

    // Stable-sort по приоритету: High первыми. Stable сохраняет source-order
    // внутри одного уровня приоритета (важно для HTTP/2 multiplexing).
    resolved.sort_by_key(|(_, k)| FetchPriority::for_kind(k));

    // Дедупликация + emit: пропускаем URL, уже отправленные в предыдущих вызовах
    // (cross-call dedup для streaming + финального pipeline).
    for (url, kind) in resolved {
        if seen.insert(url.clone()) {
            let priority = FetchPriority::for_kind(&kind);
            sink.emit(&Event::SubresourceHintFound { url, kind, priority });
        }
    }
}

/// Результат загрузки страницы: что рисовать и как назвать окно.
/// Расширяется: favicon, current URL, scroll state — позже.
pub(crate) struct LoadedPage {
    pub(crate) display_list: DisplayList,
    pub(crate) title: Option<String>,
    /// Декодированные `<img src="…">` для GPU upload через
    /// `Renderer::register_image`. Ключ — raw src attribute value (тот же,
    /// что попадает в `DisplayCommand::DrawImage.src`), чтобы render-side
    /// мог сделать lookup без отдельной нормализации URL. `Arc<Image>` (BUG-272
    /// срез 17): разделяет пиксели с `IMAGE_CACHE`/`register_image`, не копирует.
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations decoded at load time. Keyed by the same src URL
    /// as `DrawImage.src`. Frame 0 of each entry is already in `images` so the
    /// renderer has a valid texture on first paint; subsequent frames are uploaded
    /// on each `RedrawRequested` tick via `Lumen::animated_gifs`.
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` — registered with JS
    /// after page load via `_lumen_init_lazy_images` for proximity-based loading.
    #[allow(dead_code)] // read only inside #[cfg(feature = "v8")] blocks
    pub(crate) lazy_pairs: Vec<(u32, String)>,
    /// Layout-дерево страницы — используется animation scheduler-ом.
    pub(crate) layout_box: lumen_layout::LayoutBox,
    /// Провайдер шрифтов с @font-face local()-источниками страницы.
    /// Передаётся рендеру через `set_font_provider` при apply_loaded_page.
    /// PH3-19: конкретный тип (не трейт-объект), чтобы `apply_loaded_page`
    /// мог динамически дорегистрировать web-шрифты через `register_from_bytes`.
    pub(crate) font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-источники, ещё не загруженные в момент первого
    /// layout-а. `apply_loaded_page` спавнит фоновый поток для каждого;
    /// результат приходит как `LoadEvent::FontLoaded` → relayout с FOUT.
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// Навигационный запрос от JS (location.href= и т.п.), выполненный
    /// в процессе загрузки. Обрабатывается в `about_to_wait`.
    pub(crate) js_navigate: Option<JsNavigateRequest>,
    /// P3-webvtt срез 3: WebVTT-cues по каждому `<video>` страницы.
    pub(crate) page_tracks: tracks::PageTracks,
    /// BUG-480 срез 1: живые sub-документы `<iframe>` — держат JS-контексты
    /// и DOM детей до замены страницы.
    pub(crate) frames: Vec<FrameHandle>,
    /// BUG-480 срез 19: набор провайдеров, которым загружались фреймы этой
    /// страницы, — им же грузит их навигация фрейма из живого окна.
    ///
    /// `None` — путь, где фреймов нет вовсе (headless-рендер `lumen-driver`,
    /// пустая страница): загружать в живом окне будет нечего.
    pub(crate) frame_env: Option<frames::FrameLoadEnv>,
}

impl LoadedPage {
    pub(crate) fn empty() -> Self {
        Self {
            display_list: DisplayList::new(),
            title: None,
            images: Vec::new(),
            animated_gifs: Vec::new(),
            lazy_pairs: Vec::new(),
            layout_box: lumen_layout::LayoutBox {
                node: NodeId::from_index(0),
                rect: Rect::ZERO,
                style: std::sync::Arc::new(lumen_layout::style::ComputedStyle::root()),
                kind: lumen_layout::BoxKind::Block,
                children: Vec::new(),
                col_span: 1,
                row_span: 1,
                svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                dirty: lumen_layout::DirtyBits::CLEAN,
                origin: lumen_layout::BoxOrigin { node: None, role: lumen_layout::BoxRole::Placeholder },
            },
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate: None,
            page_tracks: tracks::PageTracks::default(),
            frames: Vec::new(),
            frame_env: None,
        }
    }
}

/// Результат фаз `decode → parse → layout` — общая часть для оконного и
/// dump-режимов. Поля владеют своими данными — нет ссылок наружу.
pub(crate) struct ParsedPage {
    /// Parsed DOM — shared with JS closures via Arc so event handlers can
    /// mutate the document without rebuilding the entire page.
    pub(crate) document: Arc<Mutex<Document>>,
    pub(crate) stylesheet: lumen_css_parser::Stylesheet,
    pub(crate) layout: LayoutBox,
    pub(crate) title: Option<String>,
    pub(crate) rule_count: usize,
    /// Декодированные изображения, найденные при обходе DOM. См. [`LoadedPage::images`].
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations found in the DOM. See [`LoadedPage::animated_gifs`].
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` elements — skipped by
    /// the eager fetch pass; registered with JS `_lumen_init_lazy_images` after load.
    pub(crate) lazy_pairs: Vec<(u32, String)>,
    /// Subresource-хинты, найденные preload-сканером ДО DOM-парсинга.
    /// Source-order: первые хинты важнее (их fetch стартует первым).
    pub(crate) preload_hints: Vec<lumen_html_parser::PreloadHint>,
    /// Decoded UTF-8 HTML source — stored for bfcache snapshot.
    pub(crate) html_source: String,
    /// @font-face local()-шрифты + системные шрифты. Передаётся рендеру.
    /// PH3-19: конкретный `FontRegistry` (не трейт-объект) для дорегистрации
    /// web-шрифтов после `FontLoaded` без даункаста.
    pub(crate) font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-источники, ещё не загруженные; передаются в
    /// `LoadedPage` и далее в фоновые потоки через `apply_loaded_page`.
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// Навигационный запрос, выставленный JS во время выполнения скриптов.
    pub(crate) js_navigate: Option<JsNavigateRequest>,
    /// Persistent JS context (V8) kept alive after page load so that
    /// event handlers registered via `addEventListener` continue to work.
    /// `None` when the v8 feature is disabled or script init failed.
    ///
    /// ADR-016 M2.2c-2b: `Arc` (не `Box`), чтобы хэндл можно было разделить с
    /// движковым потоком (`EngineJsState`) на время миграции `js_ctx` на него.
    pub(crate) js_ctx: Option<Arc<dyn PersistentJs>>,
    /// P3-webvtt срез 3: WebVTT-cues, загруженные из `<track>` каждого `<video>`.
    pub(crate) page_tracks: tracks::PageTracks,
    /// BUG-743: неизменяемая часть CSS + отпечаток инлайновых `<style>`,
    /// чтобы поздняя вставка листа пересобрала каскад без сети.
    pub(crate) dynamic_css: DynamicCssBase,
    /// BUG-480 срез 19: набор провайдеров, которым загружены фреймы этой
    /// страницы (см. [`LoadedPage::frame_env`]).
    pub(crate) frame_env: frames::FrameLoadEnv,
    /// BUG-480 срез 1: живые sub-документы `<iframe>` этой страницы.
    pub(crate) frames: Vec<FrameHandle>,
}

/// Источник для повторного layout без повторной загрузки/парсинга.
/// Хранится в `Lumen`; обновляется только при reload/load новой страницы.
pub(crate) struct LayoutSource {
    /// DOM — shared with the persistent JS runtime via Arc<Mutex> so that
    /// JS event handlers can mutate it between repaints.
    pub(crate) document: Arc<Mutex<Document>>,
    /// Parsed stylesheet, shared as an immutable `Arc` snapshot (ADR-016 M2.2b):
    /// off-thread relayout jobs clone the handle (`Arc::clone`) instead of deep-
    /// cloning the whole `Stylesheet` on every submit. Replaced wholesale on
    /// reload/thaw, never mutated in place.
    pub(crate) stylesheet: Arc<lumen_css_parser::Stylesheet>,
    /// Decoded HTML source captured after encoding detection. Used by bfcache
    /// to restore the page without a network round-trip.
    #[allow(dead_code)]
    pub(crate) html_source: Option<String>,
    /// `Cache-Control: no-store` on the response that produced this page.
    /// Checked by [`Lumen::bfcache_eligible`] on navigate-away; `true` routes
    /// the page to the HTML-snapshot bfcache fallback instead of a full
    /// freeze. `false` for non-network sources (file/thaw/sidebar/hibernate
    /// restore) — no header to check, so the page is treated as cacheable.
    pub(crate) cache_control_no_store: bool,
    /// BUG-743: часть CSS, не зависящая от инлайновых `<style>`, плюс отпечаток
    /// тех блоков, из которых собран текущий [`Self::stylesheet`]. `Some` на
    /// обычном пути загрузки; `None` на путях восстановления (bfcache-thaw,
    /// разморозка вкладки, sidebar), где исходные части CSS не сохранены — там
    /// каскад ведёт себя как до BUG-743 и поздний `<style>` не подхватывается.
    pub(crate) dynamic_css: Option<DynamicCssBase>,
}

/// Everything one page load's cascade is built from: the collected CSS text,
/// its parsed form and the font stack the text measurer needs.
///
/// BUG-443: these four stretches used to sit inline in [`parse_and_layout`],
/// *after* the document's scripts had run. They are a function now because the
/// cascade has to exist **before** those scripts (a parse-time
/// `getComputedStyle` must have something to read) and be rebuilt after them
/// only if a script touched `<style>`/`<link>`.
pub(crate) struct PageCascade {
    /// BUG-743 rebuild base: the part of the CSS a later `<style>` cannot change.
    pub(crate) dynamic_css: DynamicCssBase,
    /// Per-`<link rel=stylesheet>` load outcome, for BUG-804's `load`/`error`.
    pub(crate) link_outcomes: Vec<(NodeId, bool)>,
    /// Parsed cascade.
    pub(crate) sheet: lumen_css_parser::Stylesheet,
    /// `@font-face local()` faces plus the system font index.
    pub(crate) font_registry: lumen_font::FontRegistry,
    /// `@font-face url()` sources not fetched yet (loaded in the background).
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// Text measurer wired to the two above.
    pub(crate) measurer: lumen_paint::MultiFontMeasurer,
}

/// Fetch + parse the page CSS and build the matching font stack (BUG-443).
///
/// Verbatim the code `parse_and_layout` used to run inline; the only change is
/// that it can now run twice for one load (once before the scripts, once after
/// them if they changed the stylesheet set).
#[allow(clippy::too_many_arguments)]
fn build_page_cascade(
    doc: &Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    viewport: Size,
    dark_mode: bool,
    media_print: bool,
) -> Result<PageCascade, Box<dyn Error>> {
    let (css, dynamic_css, link_outcomes) = {
        let _s = lumen_core::trace::span("fetch-css", "net");
        let link_media_ctx = if media_print {
            print_media_context(viewport, dark_mode)
        } else {
            screen_media_context(viewport, dark_mode)
        };
        // Инлайновые <style>: их `@import` резолвятся относительно базы
        // документа (CSS-SPECS §@import). Внешние <link> резолвят собственные
        // `@import` относительно своего URL внутри load_linked_stylesheets.
        let inline = extract_style_blocks(doc);
        let mut css = inline_css_imports(
            &inline,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
            &mut std::collections::HashSet::new(),
            0,
        );
        // BUG-743: всё, что не пришло из инлайновых <style>, откладывается
        // отдельно — так поздний динамический <style> пересобирает каскад без
        // единого сетевого запроса. `inline_css_imports` возвращает
        // `<импорты> + <исходный текст>`, поэтому префикс = всё до хвоста.
        let imports_prefix = css[..css.len() - inline.len()].to_owned();
        let (linked, link_outcomes) = load_linked_stylesheets(
            doc,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
        );
        css.push_str(&linked);
        let dyn_css = DynamicCssBase {
            imports_prefix,
            linked,
            inline_fp: inline_style_fingerprint(doc),
        };
        (css, dyn_css, link_outcomes)
    };

    let sheet = {
        let _s = lumen_core::trace::span("parse-css", "parse");
        lumen_css_parser::parse(&css)
    };

    // PH3-19: @font-face загрузка разделена на два прохода.
    // local()-источники загружаются синхронно (из системного индекса, быстро).
    // url()-источники — только собираем в pending_web_fonts; фоновый поток
    // fetch+decode спавнится в apply_loaded_page → первый paint не ждёт сети.
    let (font_registry, pending_web_fonts) = {
        // PERF-12: this stretch — @font-face resolution through to the measurer's
        // system faces below — was the single largest unnamed hole in the
        // `--trace-nav` waterfall (114 ms of a 128 ms `navigation` on
        // samples/page.html, against a `layout` span of 0.6 ms). It is dominated
        // by the lazy system-font index build that PERF-11 caches.
        let _s = lumen_core::trace::span("font-faces", "font");
        load_font_faces(&sheet.font_faces, base, sink, cookie_jar)
    };

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    // Многошрифтовый измеритель: Inter как fallback + уже загруженные local()-семьи.
    // url()-семьи добавятся позже через FontLoaded + relayout_with_web_fonts.
    let mut measurer = lumen_paint::MultiFontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;
    // BUG-128: системные face-ы — те же, что выберет рендер.
    {
        // PERF-11/PERF-12: `system_font_faces()` is where the lazy system font
        // index is built on first use — hundreds of files parsed, once per
        // process. Named separately from `font-faces` so the trace attributes
        // the cost to the index rather than to @font-face handling.
        let _s = lumen_core::trace::span("system-fonts", "font");
        measurer.set_system_faces(system_font_faces());
    }
    for rule in &sheet.font_faces {
        if !rule.family.is_empty()
            && let Some(bytes) = font_registry.face_bytes_for_family(&rule.family)
        {
            // CSS Fonts L4 §5.1: передаём unicode-range из @font-face дескриптора.
            let ranges = rule.unicode_range.as_deref()
                .map(lumen_font::parse_unicode_ranges)
                .unwrap_or_default();
            measurer.register_family_with_ranges(&rule.family, bytes, ranges);
        }
    }

    Ok(PageCascade { dynamic_css, link_outcomes, sheet, font_registry, pending_web_fonts, measurer })
}

/// What a JS runtime has to be handed before it can answer `getComputedStyle`,
/// `getBoundingClientRect`, `offsetWidth` or `window.innerHeight` (BUG-443).
///
/// The same four tables `apply_loaded_page`/`relayout_page` push after every
/// layout; collected here so the *first* one can be pushed before the page's
/// own scripts run instead of long after them.
pub(crate) struct JsLayoutSnapshot {
    /// `node index -> [x, y, w, h]` border boxes.
    pub(crate) rects: std::collections::HashMap<u32, [f32; 4]>,
    /// `LayoutBox` tree for `document.elementFromPoint`/`elementsFromPoint`
    /// (BUG-464/BUG-477) — same tree `rects` was collected from.
    pub(crate) tree: Arc<LayoutBox>,
    /// `node index -> property -> serialized computed value`.
    pub(crate) styles: std::collections::HashMap<u32, std::collections::HashMap<String, String>>,
    /// `node index -> custom property -> value`.
    pub(crate) customs:
        std::collections::HashMap<u32, Arc<std::collections::HashMap<String, String>>>,
    /// Viewport the layout was run at, for `window.innerWidth`/`innerHeight`.
    pub(crate) viewport: (f32, f32),
}

/// Snapshot a laid-out tree into the tables the JS runtime reads (BUG-443).
pub(crate) fn collect_js_layout_snapshot(
    root: &LayoutBox,
    doc: &Document,
    viewport: Size,
) -> JsLayoutSnapshot {
    JsLayoutSnapshot {
        rects: lumen_layout::collect_layout_rects(root, doc),
        tree: Arc::new(root.clone()),
        styles: lumen_layout::collect_computed_styles(root, doc),
        customs: lumen_layout::collect_custom_properties(root),
        viewport: (viewport.width, viewport.height),
    }
}

/// One layout pass with BUG-270's per-pass `print` media flag bracketed.
fn layout_page(
    doc: &Document,
    sheet: &lumen_css_parser::Stylesheet,
    measurer: &lumen_paint::MultiFontMeasurer,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    media_print: bool,
) -> LayoutBox {
    // BUG-270: печать в PDF фильтрует каскад по media_type="print" через
    // sticky thread-local. Флаг per-pass, поэтому сбрасываем сразу после layout,
    // чтобы последующие экранные проходы на этом же потоке не наследовали print.
    lumen_layout::set_print_media(media_print);
    let out = {
        let _s = lumen_core::trace::span("layout", "layout");
        lumen_layout::layout_measured_hyp(doc, sheet, viewport, measurer, hp, dark_mode)
    };
    lumen_layout::set_print_media(false);
    out
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    cross_origin_isolated: bool,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    target: lumen_core::ColorSpace,
    media_print: bool,
) -> Result<ParsedPage, Box<dyn Error>> {
    // Кодировку определяем по BOM -> <meta charset> -> эвристике. Это покрывает
    // и UTF-8 (большинство), и старые cp1251 / koi8-r / cp866 файлы.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("Кодировка: {}", encoding.name());

    // Preload-сканер запускается ДО DOM-парсинга (HTML LS §13.2.6.4.7).
    // `preload_seen` — cross-call dedup: если streaming уже отправил <head>-хинты
    // через EarlyPreloadHints, финальный scan пропустит их и добавит только новые
    // (body-images, lazy-loaded resources и т.п.).
    let preload_hints = lumen_html_parser::scan_preload_hints(&source);
    dispatch_preload_hints(&preload_hints, base, sink, preload_seen);

    let mut doc = {
        let _s = lumen_core::trace::span("parse-html", "parse");
        lumen_html_parser::parse(&source)
    };
    // BUG-358: stamp the document with what it was actually decoded as / served
    // as, so `document.characterSet`/`charset`/`inputEncoding`/`contentType`
    // read real per-load state instead of `undefined`.
    doc.set_character_set(encoding.canonical_name().to_string());
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or(ct).trim();
        if !mime.is_empty() {
            doc.set_content_type(mime.to_string());
        }
    }
    let title = extract_title(&doc);

    // Гейт выполнения скриптов: top-level документ не sandboxed.
    // QuickJS + install_dom дают скриптам полный доступ к DOM-дереву.
    // fetch_provider пробрасывается в window.fetch(); ws_provider — в new WebSocket();
    // sse_provider — в new EventSource(). Все три используют один HttpClient.
    let (fetch_provider, ws_provider, sse_provider) = match base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar.clone());
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            let sp: Option<Arc<dyn lumen_core::ext::JsSseProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsSseProvider>);
            (fp, wp, sp)
        }
        ResourceBase::File(_) => (None, None, None),
    };
    // URL страницы для инициализации window.location в JS.
    let page_url = base_url_string(base);
    // Extension content scripts: collect JS sources that match the page URL.
    let ext_registry = extensions::ExtensionRegistry::load();
    let ext_scripts = ext_registry.content_scripts_for_url(&page_url);
    // BUG-164: collect classic + module scripts in document order and fetch
    // external `<script src>` bodies via the subresource fetcher, so SPA
    // bundles execute (lenta.ru owlBundle.js etc.), not just inline scripts.
    let (classic_scripts, module_scripts) = {
        let _s = lumen_core::trace::span("fetch-scripts", "net");
        let mut classic_items = Vec::new();
        let mut module_items = Vec::new();
        collect_scripts_ordered(&doc, doc.root(), &mut classic_items, &mut module_items);
        (
            resolve_script_sources(&classic_items, base, sink, cookie_jar.clone()),
            resolve_script_sources(&module_items, base, sink, cookie_jar.clone()),
        )
    };
    // BUG-443: the cascade is built BEFORE the page's scripts run, and so is the
    // first layout, because code executing during parsing — an inline
    // `<script>`, a `DOMContentLoaded` handler — is entitled to read geometry
    // and computed style. Until now those phases sat after `run_scripts_with_dom`,
    // so every such read answered `""` / a zero rect. HTML LS §4.12.1 makes a
    // classic script wait for pending stylesheets anyway, so building the
    // cascade first is the spec order, not just a convenience.
    //
    // CSS Selectors L4 §9.6 `:target`: set current target from the URL fragment
    // so the matcher has the correct target_id before that first cascade.
    let page_fragment = if let ResourceBase::Url(u) = base {
        lumen_core::url::Url::parse(u)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
    } else {
        None
    };
    doc.set_target(page_fragment.as_deref());
    let mut cascade = build_page_cascade(
        &doc, base, sink, cookie_jar.clone(), viewport, dark_mode, media_print,
    )?;
    // Fingerprints of the two stylesheet sources, so the rebuild below can tell
    // whether the scripts touched either. Cheap: two tree walks, no fetching.
    let css_sources_before = (inline_style_fingerprint(&doc), stylesheet_link_fingerprint(&doc));

    // The pre-script layout exists only to be read by scripts, so a page with
    // none pays nothing for it. Its geometry is what the document has *now*:
    // no images decoded yet, no web fonts registered — exactly what a real
    // browser answers for a forced layout at this point.
    let parse_time_snapshot = if classic_scripts.is_empty()
        && module_scripts.is_empty()
        && ext_scripts.is_empty()
    {
        None
    } else {
        Some(collect_js_layout_snapshot(
            &layout_page(
                &doc, &cascade.sheet, &cascade.measurer, viewport, hp, dark_mode, media_print,
            ),
            &doc,
            viewport,
        ))
    };

    let run_scripts_span = lumen_core::trace::span("run-scripts", "script");
    // BUG-480 срез 1: клоны провайдеров/хранилищ для sub-документов <iframe> —
    // основные уходят в run_scripts_with_dom по значению.
    // BUG-480 срез 19: те же клоны, но одним значением [`FrameLoadEnv`] —
    // оно переживает загрузку и уезжает в `Lumen`, чтобы навигация фрейма
    // повторила загрузку под-документа тем же набором провайдеров.
    let frame_env = frames::FrameLoadEnv {
        sink: Arc::clone(sink),
        cookie_jar: cookie_jar.clone(),
        fetch_provider: fetch_provider.clone(),
        ws_provider: ws_provider.clone(),
        sse_provider: sse_provider.clone(),
        ls_store: ls_store.clone(),
        ss_store: ss_store.clone(),
        idb_backend: idb_backend.clone(),
        sw_backend: sw_backend.clone(),
        sw_worker_store: sw_worker_store.clone(),
        cache_backend: cache_backend.clone(),
        media_ctx: screen_media_context(viewport, dark_mode),
        viewport,
        cookie_banner_dismiss,
        deterministic,
        cross_origin_isolated,
        target,
        page_base: base.clone(),
    };
    let (doc_arc, js_nav, js_ctx) = run_scripts_with_dom(
        doc,
        lumen_core::SandboxFlags::empty(),
        &page_url,
        fetch_provider,
        ws_provider,
        sse_provider,
        ls_store,
        ss_store,
        idb_backend,
        sw_backend,
        sw_worker_store,
        cache_backend,
        cookie_banner_dismiss,
        deterministic,
        cross_origin_isolated,
        &ext_scripts,
        classic_scripts,
        module_scripts,
        false,
        parse_time_snapshot,
    );
    drop(run_scripts_span);

    // BUG-443: the scripts have had their turn at the DOM, so the cascade and
    // the geometry a `DOMContentLoaded` handler is about to read are re-derived
    // here. The CSS is only refetched if a script actually touched `<style>` or
    // `<link>` — otherwise the pre-script cascade is reused verbatim, which is
    // what keeps this one network pass per load, as before.
    let scripts_changed_css = {
        let d = doc_arc.lock().unwrap();
        (inline_style_fingerprint(&d), stylesheet_link_fingerprint(&d)) != css_sources_before
    };
    if scripts_changed_css {
        let d = doc_arc.lock().unwrap();
        cascade = build_page_cascade(
            &d, base, sink, cookie_jar.clone(), viewport, dark_mode, media_print,
        )?;
    }
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        // Nothing to re-derive if the scripts changed neither the cascade nor
        // the tree: the snapshot pushed before they ran is still current, and
        // this is the one place a whole layout pass can be skipped. The flag is
        // only *read* here — `take_dom_dirty` would swallow the relayout the
        // shell schedules for itself after the load.
        let dom_touched = js
            .dom_dirty_flag()
            .is_none_or(|f| f.load(std::sync::atomic::Ordering::Relaxed));
        if scripts_changed_css || dom_touched {
            let snapshot = {
                let d = doc_arc.lock().unwrap();
                collect_js_layout_snapshot(
                    &layout_page(
                        &d, &cascade.sheet, &cascade.measurer, viewport, hp, dark_mode, media_print,
                    ),
                    &d,
                    viewport,
                )
            };
            js.update_layout_rects(snapshot.rects);
            js.update_computed_styles(snapshot.styles);
            js.update_custom_properties(snapshot.customs);
            js.update_viewport_size(snapshot.viewport.0, snapshot.viewport.1);
        }
    }

    // HTML LS §8.2.3 — after HTML parse + inline scripts: readyState → "interactive"
    // + DOMContentLoaded event. Fires before images/fonts are decoded.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        js.notify_dom_content_loaded();
    }

    {
        let d = doc_arc.lock().unwrap();
        // Гейт отправки форм: Phase 0 — top-level документ не sandboxed.
        check_form_gate(&d, lumen_core::SandboxFlags::empty());
        // Гейт навигации: Phase 0 — top-level документ не sandboxed.
        check_navigation_gate(&d, lumen_core::SandboxFlags::empty());
        // Применяем sandbox-ограничения из <iframe sandbox> элементов.
        // Phase 0: iframe sub-документы не загружаются — применяем гейты
        // к самому iframe-элементу, логируем ограничения для будущего Phase 1.
        apply_iframe_sandbox_gates(&d);
    }

    // BUG-480 срез 1: загрузка sub-документов <iframe>. Локи внутри функции
    // короткие — скрипты детей и `load` хоста идут без удержания дерева.
    // Срез 3: документ/база страницы передаются и как top — у фреймов
    // первого уровня parent === top, глубже top всегда корень.
    // Срез 11: экранный media-гейт `<link>` и вьюпорт picker-а картинок —
    // те же, с какими страница грузит свои подресурсы (print-гейт
    // фреймам не нужен — печать PDF под-документов вне среза).
    let mut frames = {
        let _s = lumen_core::trace::span("fetch-iframes", "net");
        load_frame_sub_documents(&doc_arc, 0, base, &doc_arc, &frame_env, js_ctx.as_ref())
    };

    // Fetch + decode <img src>. Должно идти ДО layout, потому что intrinsic
    // dimensions из декодированного изображения проставляются как HTML
    // presentational hints (width/height attribute) и потом подхватываются
    // style cascade. Errors silently пропускаются — битая картинка не валит
    // всю страницу, layout нарисует серый placeholder.
    // loading="lazy" изображения возвращаются в lazy_pairs и не загружаются сейчас.
    let (images, animated_gifs, lazy_pairs) = {
        let _s = lumen_core::trace::span("fetch-images", "net");
        let mut d = doc_arc.lock().unwrap();
        fetch_and_decode_images(&mut d, base, sink, viewport, cookie_jar.clone(), target)
    };

    // P3-webvtt срез 3: загрузка WebVTT-субтитров из <track> каждого <video>.
    // Ошибки фетча/парсинга не валят страницу — видео просто остаётся без cues.
    let page_tracks = {
        let d = doc_arc.lock().unwrap();
        tracks::load_video_tracks(&d, &|src| {
            fetch_vtt_text(src, base, sink, cookie_jar.clone())
        })
    };

    // Register decoded <img> bitmaps with the JS runtime so Canvas 2D
    // drawImage(imgElement, …) can read the pixels. Collect nid→url from DOM
    // (same traversal fetch_and_decode_images used), join with decoded images by
    // URL, and share the decoded `Arc<Image>` into img_bitmap_store on the JS thread.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        let img_reqs = {
            let d = doc_arc.lock().unwrap();
            lumen_layout::collect_image_requests(&d, viewport)
        };
        // BUG-272 срез 20: share the decoded `Arc<Image>` with the JS canvas
        // drawImage store instead of eagerly copying an RGBA8 buffer per image.
        // The store converts to RGBA8 lazily, only for images a canvas actually
        // draws — images never used as a drawImage source cost zero extra bytes.
        let url_to_img: std::collections::HashMap<&str, &std::sync::Arc<lumen_image::Image>> =
            images.iter().map(|(url, img)| (url.as_str(), img)).collect();
        let bitmaps: Vec<(u32, std::sync::Arc<lumen_image::Image>)> = img_reqs
            .iter()
            .filter_map(|req| {
                let img = url_to_img.get(req.url.as_str())?;
                Some((req.node_id.index() as u32, std::sync::Arc::clone(img)))
            })
            .collect();
        if !bitmaps.is_empty() {
            js.register_img_bitmaps(bitmaps);
        }
    }

    // BUG-443: the cascade was collected before the scripts ran (and rebuilt
    // right after them if they touched `<style>`/`<link>`), so there is nothing
    // left to fetch or parse here — only to hand out.
    let PageCascade {
        dynamic_css, link_outcomes, sheet, font_registry, pending_web_fonts, measurer,
    } = cascade;

    // BUG-804: HTML LS §4.6.7 «process the linked resource» — каждый
    // `<link rel=stylesheet>` обязан сообщить странице `load` или `error`.
    // Отчёт уходит отсюда, а не из шима: лист грузит проход выше, и только он
    // знает исход — повторный фетч из JS дал бы второй запрос и всё равно не
    // отличил бы «лист в каскаде» от «байты пришли». Элемент, который уже
    // отчитался сам (вставленный скриптом — он проходит через
    // `_lumen_link_prepare` ЕЩЁ ДО этого прохода, скрипты выполняются раньше),
    // отсекается общим пер-узловым флагом на JS-стороне.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx
        && !link_outcomes.is_empty()
    {
        use std::fmt::Write as _;
        let mut arg = String::with_capacity(link_outcomes.len() * 8 + 40);
        arg.push_str("_lumen_deliver_parser_link_events([");
        for (i, (node, ok)) in link_outcomes.iter().enumerate() {
            if i > 0 {
                arg.push(',');
            }
            let _ = write!(arg, "{},{}", node.index(), u8::from(*ok));
        }
        arg.push_str("]);");
        js.eval_js(&arg);
    }


    // Populate document.fonts with FontFace objects from @font-face rules.
    // local() — immediately Loaded; url() — Loading (будет Loaded по FontLoaded).
    {
        let mut d = doc_arc.lock().unwrap();
        for rule in &sheet.font_faces {
            let mut font_face = rule_to_font_face(rule);
            // local() rules already resolved — mark Loaded; url() rules stay Loading.
            let has_local = rule.sources.iter().any(|s| {
                s.kind == lumen_css_parser::FontFaceSourceKind::Local
                    && font_registry.face_bytes_for_family(&rule.family).is_some()
            });
            if has_local {
                font_face.status = lumen_dom::FontFaceStatus::Loaded;
            }
            d.fonts_mut().add(font_face);
        }
    }

    let font_provider = Arc::new(font_registry);

    // BUG-443: same helper the pre-script layout uses, so the print-media
    // bracket (BUG-270) cannot drift between the two passes.
    let layout = {
        let d = doc_arc.lock().unwrap();
        layout_page(&d, &sheet, &measurer, viewport, hp, dark_mode, media_print)
    };

    // BUG-480 срез 13: размер host-бокса каждого `<iframe>` известен только
    // теперь — пересчитываем layout под-документов под него (срез 12 считал их
    // на UA-дефолтных 300×150, потому что шёл до этой строки).
    // Интерактивное состояние здесь заведомо пустое (BUG-480 срез 23): фреймы
    // только что созданы, ни курсора над ними, ни фокуса в них ещё не было.
    crate::frames::sync_frame_viewports(&mut frames, &layout, Default::default());

    // FRAME-5 срез 2: above-the-fold `<img loading="lazy">` inside a frame —
    // `sync_frame_viewports` just harvested proximity hits from each frame's
    // own `IntersectionObserver` into `FrameHandle::pending_lazy`; turn them
    // into pixels now (network+decode only, safe off the UI thread — nothing
    // here touches `Lumen::renderer`). Folded straight into `h.images`/
    // `h.animated_gifs`, so the merge loop below picks them up like any other
    // frame image — no separate registration path for the initial load.
    for h in &mut frames {
        crate::frame_lazy::fetch_frame_lazy_images(h, sink, cookie_jar.clone(), target);
    }

    // CSS Backgrounds L3 §3.10 — собираем `background-image: url(...)` уже
    // после layout-а (картинки фона не влияют на расчёт коробок). Декодируем
    // и добавляем к `images` тем же ключом, что эмиттер кладёт в
    // `DisplayCommand::DrawBackgroundImage.src`.
    let mut images = images;
    {
        let _s = lumen_core::trace::span("fetch-bg-images", "net");
        for (src, image) in fetch_and_decode_background_images(&layout, base, sink, cookie_jar.clone(), target) {
            images.push((src, image));
        }
    }
    // BUG-480 срез 15: картинки под-документов фреймов едут в ОБЩИЙ список
    // страницы. Их ключи разрешены относительно базы ребёнка
    // (`frames::frame_image_key`), поэтому со своими ключами страницы они не
    // сталкиваются, а все существующие точки регистрации (`apply_loaded_page`,
    // `reload`, `pending_images`, CPU-кэш снимков) подхватывают их без правок.
    let mut animated_gifs = animated_gifs;
    for h in &frames {
        images.extend(h.images.iter().map(|(k, i)| (k.clone(), Arc::clone(i))));
        // FRAME-5: то же слияние, что у `images` выше — под-документов GIF
        // тикает в `RedrawRequested` через `Lumen::animated_gifs` (карта
        // СТРАНИЦЫ), ключи уже уникальны на всю страницу (`frame_image_key`).
        animated_gifs.extend(h.animated_gifs.iter().cloned());
    }

    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc_arc,
        stylesheet: sheet,
        layout,
        title,
        rule_count,
        images,
        animated_gifs,
        lazy_pairs,
        preload_hints,
        html_source: source,
        font_registry: font_provider,
        pending_web_fonts,
        js_navigate: js_nav,
        js_ctx,
        page_tracks,
        dynamic_css,
        frames,
        frame_env,
    })
}

/// Готовый результат финального pipeline: display-list-страница, источник для
/// relayout и живой JS-хэндл (если включён QuickJS). Тип-алиас, чтобы вынести
/// сложную тройку из сигнатур (`render_bytes`, `RenderOutcome`).
pub(crate) type RenderedPage = (LoadedPage, LayoutSource, Option<Arc<dyn PersistentJs>>);

/// BUG-171 этап 2: результат финального off-UI-thread рендера (`render_bytes`),
/// пересылаемый назад на UI-поток через `LoadEvent::RenderDone`.
///
/// Все поля `Send`: `LoadedPage`/`LayoutSource` — обычные данные; `js_ctx` —
/// хэндл QuickJS (`Send + Sync` по ADR-014, создан на рендер-потоке);
/// `preload_dispatched` временно забран из `Lumen` на время рендера (он его
/// дедуплицирует) и возвращается для восстановления.
pub(crate) struct RenderOutcome {
    /// Готовая страница + источник layout + живой JS-хэндл; либо текст ошибки
    /// (`Box<dyn Error>` не `Send`, поэтому конвертируется в `String`).
    pub(crate) result: Result<RenderedPage, String>,
    /// Набор уже разосланных preload-хинтов, забранный из
    /// `Lumen::preload_dispatched` на время рендера.
    pub(crate) preload_dispatched: std::collections::HashSet<String>,
}
