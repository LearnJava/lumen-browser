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
    push_backend: Option<Arc<dyn lumen_core::ext::PushBackend>>,
    target: lumen_core::ColorSpace,
    cache_control_no_store: bool,
    // BUG-640: real facts about the top-level HTTP response, carried straight
    // through into `LoadedPage::nav` for `PerformanceNavigationTiming` — see
    // `nav_timing`'s doc comment for what these two can and can't express.
    response_status: u16,
    redirected: bool,
    // GAP-CSPENF срез 5: raw `Content-Security-Policy` response header(s),
    // stamped onto the parsed document so every enforcement point sees them
    // next to the document's `<meta>` policies. срез 41: one entry per header
    // occurrence, not joined — CSP3 §3.4 treats each as an independent policy.
    csp_header: &[String],
    // GAP-CSPENF срез 59: `{group name -> endpoint URLs}` resolved from the
    // response's `Report-To` header(s) — a CSP policy's `report-to <group>`
    // directive resolves against this map. Same "raw fact about the
    // response" status as `csp_header`; see `page_source::report_to_endpoints`.
    report_to_endpoints: &std::collections::HashMap<String, Vec<String>>,
    // GAP-POLICYREPORT (BUG-953): `sync-xhr` disposition resolved from the
    // response's `Document-Policy`/`Permissions-Policy` (+ `-Report-Only`)
    // headers — see `page_source::document_policy_sync_xhr_disposition`/
    // `permissions_policy_sync_xhr_disposition`. Neither policy has a `<meta>`
    // form, so unlike `csp_header` these arrive already resolved, not raw.
    sync_xhr_document_policy: Option<lumen_core::ext::PolicyDisposition>,
    sync_xhr_permissions_policy: Option<lumen_core::ext::PolicyDisposition>,
    // GAP-REFERRER срез 3: raw `Referrer-Policy` response header text, stamped
    // onto the parsed document next to `csp_header` — see
    // `page_source::referrer_policy_header`.
    referrer_policy_header: Option<&str>,
) -> Result<RenderedPage, Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport, preload_seen, ls_store, ss_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic, dark_mode, cookie_jar, cross_origin_isolated, sw_worker_store, cache_backend, push_backend, target, false, csp_header, report_to_endpoints, sync_xhr_document_policy, sync_xhr_permissions_policy, referrer_policy_header)?;
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
        stylesheet_nodes: Arc::new(parsed.stylesheet_nodes),
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
            nav: crate::nav_timing::NavResponseMeta {
                status: response_status,
                redirected,
                decoded_body_size: bytes.len() as u64,
            },
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

    // Первый проход: резолв URL + вычисление kind + author fetchpriority
    // (HTML LS §2.5.7, срез 4) — override эвристики `FetchPriority::for_kind`,
    // когда `<link>`/`<img>`/`<script>` несёт явный `fetchpriority="high|low"`.
    let mut resolved: Vec<(String, SubresourceKind, Option<String>)> = Vec::with_capacity(hints.len());
    for hint in hints {
        let triple = match hint {
            PreloadHint::Stylesheet { url, fetch_priority, .. } =>
                (base.resolve_str(url), SubresourceKind::Stylesheet, fetch_priority.clone()),
            PreloadHint::Script { url, fetch_priority } =>
                (base.resolve_str(url), SubresourceKind::Script, fetch_priority.clone()),
            PreloadHint::Image { url: Some(url), fetch_priority, .. } =>
                (base.resolve_str(url), SubresourceKind::Image, fetch_priority.clone()),
            // srcset содержит список URL — резолвинг каждого кандидата
            // откладывается до picker-а; эмитим srcset-строку как-есть.
            PreloadHint::Image { url: None, srcset: Some(s), fetch_priority, .. } =>
                (s.clone(), SubresourceKind::Image, fetch_priority.clone()),
            PreloadHint::SourceSet { srcset, .. } =>
                (srcset.clone(), SubresourceKind::Image, None),
            PreloadHint::Preload { url, as_kind, fetch_priority } => {
                let kind = match as_kind.as_deref() {
                    Some("font") => SubresourceKind::Font,
                    Some("image") => SubresourceKind::Image,
                    Some("script") => SubresourceKind::Script,
                    Some("style") => SubresourceKind::Stylesheet,
                    _ => SubresourceKind::Other { as_kind: as_kind.clone() },
                };
                (base.resolve_str(url), kind, fetch_priority.clone())
            }
            // BUG-826: остальные два вида author-хинта. Реальный fetch и
            // события `load`/`error` для них делает JS-шим на самом элементе
            // (`_lumen_link_hint_prepare`), здесь — только строка сетевого лога.
            PreloadHint::ModulePreload { url } =>
                (base.resolve_str(url), SubresourceKind::Script, None),
            PreloadHint::Prefetch { url } =>
                (base.resolve_str(url), SubresourceKind::Other { as_kind: Some("prefetch".into()) }, None),
            // Preconnect URL — origin, не содержит path — резолвинг тривиален.
            PreloadHint::Preconnect { url, dns_only } =>
                (base.resolve_str(url), SubresourceKind::Preconnect { dns_only: *dns_only }, None),
            PreloadHint::Image { url: None, srcset: None, .. } => continue,
        };
        resolved.push(triple);
    }

    // Stable-sort по приоритету: High первыми. Stable сохраняет source-order
    // внутри одного уровня приоритета (важно для HTTP/2 multiplexing).
    resolved.sort_by_key(|(_, k, fp)| {
        FetchPriority::from_attr(fp.as_deref()).unwrap_or_else(|| FetchPriority::for_kind(k))
    });

    // Дедупликация + emit: пропускаем URL, уже отправленные в предыдущих вызовах
    // (cross-call dedup для streaming + финального pipeline).
    for (url, kind, fp) in resolved {
        if seen.insert(url.clone()) {
            let priority =
                FetchPriority::from_attr(fp.as_deref()).unwrap_or_else(|| FetchPriority::for_kind(&kind));
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
    /// BUG-640: real facts about the top-level HTTP response, needed to build
    /// the `PerformanceNavigationTiming` detail payload at the
    /// `deliver_nav_timing` call site (both of which read `page.nav` before
    /// the rest of this struct's fields are moved out).
    pub(crate) nav: crate::nav_timing::NavResponseMeta,
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
                used_line_height: 16.0 * 1.2,
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
            nav: crate::nav_timing::NavResponseMeta::default(),
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
    /// CSSOM-1 срез 2: см. [`PageCascade::stylesheet_nodes`].
    pub(crate) stylesheet_nodes: Vec<StylesheetNodeEntry>,
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
    /// CSSOM-1 срез 2: см. [`PageCascade::stylesheet_nodes`]. Ещё не читается
    /// нигде — срез 3 подключит JS-байндинги `document.styleSheets` поверх
    /// него.
    #[allow(dead_code)]
    pub(crate) stylesheet_nodes: Arc<Vec<StylesheetNodeEntry>>,
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

/// CSS View Transitions Module Level 2 §3: whether `stylesheet` opts a
/// document in to cross-document (MPA) view transitions, i.e. its cascade
/// declared `@view-transition { navigation: auto; }`. Mirrors CSS's
/// last-declaration-wins for a document-level descriptor — with more than
/// one `@view-transition` block, the last one in document order decides.
/// See `docs/tasks/ph3-view-transitions-mpa.md` срез 2. Not called from the
/// navigation pipeline yet — wiring lands in срезы 3-4; until then only the
/// tests below and [`mpa_view_transition_allowed`] use it.
#[allow(dead_code)]
pub(crate) fn view_transition_navigation_opted_in(
    stylesheet: &lumen_css_parser::Stylesheet,
) -> bool {
    stylesheet
        .view_transition_rules
        .last()
        .is_some_and(|r| r.navigation == lumen_css_parser::ViewTransitionNavigation::Auto)
}

/// Whether a navigation from `from` to `to` qualifies for a cross-document
/// view transition: same origin (spec §navigation — cross-origin MPA
/// transitions are out of scope) and **both** the departing and arriving
/// document opt in via `@view-transition { navigation: auto; }`.
#[allow(dead_code)] // wired in срезы 3-4, see the comment above `view_transition_navigation_opted_in`
pub(crate) fn mpa_view_transition_allowed(
    from_origin: &lumen_network::Origin,
    from_stylesheet: &lumen_css_parser::Stylesheet,
    to_origin: &lumen_network::Origin,
    to_stylesheet: &lumen_css_parser::Stylesheet,
) -> bool {
    from_origin.same_origin(to_origin)
        && view_transition_navigation_opted_in(from_stylesheet)
        && view_transition_navigation_opted_in(to_stylesheet)
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
    /// GAP-CSPENF срез 7: resolved URL of every `<link rel=stylesheet>`
    /// `style-src`/`default-src` blocked — mirrors `blocked_by_img_src`
    /// (срез 4), fired as `securitypolicyviolation` once a JS runtime exists.
    pub(crate) blocked_by_style_src: Vec<String>,
    /// GAP-CSPENF срез 21: text of the `style-src`/`default-src` policy that
    /// blocked each inline `<style>` block, one entry per blocked block — no
    /// URL to report (`blockedURI` is always `"inline"`, same as inline
    /// `<script>`). Срез 57 turned this from a bare count into the violated
    /// policy's own text (CSP3 §7.8's `originalPolicy`, not the document's
    /// combined text).
    pub(crate) blocked_inline_style_policies: Vec<String>,
    /// GAP-CSPENF срез 23: nodes whose `style=""` attribute `style-src-attr`/
    /// `style-src`/`default-src` blocked — handed to
    /// [`lumen_dom::Document::set_style_attr_csp_blocked`] before the first
    /// layout.
    pub(crate) blocked_style_attr_nodes: std::collections::HashSet<NodeId>,
    /// GAP-CSPENF срез 57: text of the policy that blocked each node in
    /// [`Self::blocked_style_attr_nodes`], same document order — was a bare
    /// length used to fire one `securitypolicyviolation` per node; now each
    /// dispatch carries the violated policy's own text instead of the
    /// document's combined text.
    pub(crate) blocked_style_attr_policies: Vec<String>,
    /// Parsed cascade.
    pub(crate) sheet: lumen_css_parser::Stylesheet,
    /// CSSOM-1 срез 2: один [`StylesheetNodeEntry`] на `<style>`/`<link
    /// rel=stylesheet>`, в порядке документа — параллельно [`Self::sheet`],
    /// не участвует в каскаде/layout.
    pub(crate) stylesheet_nodes: Vec<StylesheetNodeEntry>,
    /// `@font-face local()` faces plus the system font index.
    pub(crate) font_registry: lumen_font::FontRegistry,
    /// `@font-face url()` sources not fetched yet (loaded in the background).
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// Text measurer wired to the two above.
    pub(crate) measurer: lumen_paint::MultiFontMeasurer,
}

/// BUG-752: the base every subresource (`<link>`, `<script src>`, `<img>`,
/// `<iframe>`, `<track>`, CSS `url()`) resolves against — `base` adjusted by
/// the document's current `<base href>`, if any. Recomputed at each call site
/// from the live `doc` rather than cached once: a script can insert or change
/// `<base>` after parsing, and the JS-side `_lumen_document_base_url()` this
/// must agree with recomputes on every call too (a snapshot here would drift
/// from it the moment a page does that). `base` itself — Origin/CORS/mixed
/// content checks, `window.location`, cross-origin frame gates — is
/// unaffected by `<base>` and must keep using the un-adjusted value.
pub(crate) fn effective_base(doc: &Document, base: &ResourceBase) -> ResourceBase {
    match doc.base_href() {
        // GAP-CSPENF срез 32: an href the document's `base-uri` directive
        // forbids is discarded — HTML LS §4.2.3 step 6 already discards a
        // `<base>` whose href fails to *parse*; CSP3 §6.4.1 adds a second,
        // policy-based reason to discard it, and both leave the document's
        // original base in effect, not merely fail the one relative-URL
        // resolution that happened to trigger this call.
        Some(href) if base_uri_href_blocked(doc, base, href).is_none() => {
            base.resolve_as_base(href)
        }
        _ => base.clone(),
    }
}

/// `Some((resolved href, every violated policy's text))` if this document's
/// `base-uri` directive (CSP3 §6.4.1) forbids `href` (relative to `base`,
/// the document's un-adjusted base) — `None` if there is no policy, no
/// `base-uri` directive, or the href is allowed. `self_origin` deliberately
/// comes from the un-adjusted
/// `base`, never from an already-`<base>`-adjusted one — `base-uri` gates
/// what `<base>` may become, so checking it against its own candidate value
/// would make `'self'` degenerate into "always true".
///
/// Shared by [`effective_base`] (the actual block) and the one-shot
/// `securitypolicyviolation` report fired once per document in
/// `parse_and_layout`, the same one-shot-push shape every other GAP-CSPENF
/// срез already uses for a directive whose choke point is a pure function
/// with no `js_ctx`. Returns `(resolved href, every violated policy's raw
/// text)` — срез 56 made the second element the SPECIFIC policy/policies
/// `base-uri` violated, not `document_csp_policy`'s combined text of every
/// policy the document declared (`SecurityPolicyViolationEvent.
/// originalPolicy`, CSP3 §7.8); срез 58 made it every violated policy, not
/// just the first, when several independent policies (CSP3 §3.4) forbid the
/// same `<base href>` at once. `effective_base` only checks `.is_none()` —
/// the block itself stays a single decision regardless of how many texts
/// come back.
fn base_uri_href_blocked(doc: &Document, base: &ResourceBase, href: &str) -> Option<(String, Vec<String>)> {
    let root = doc.root();
    let (policy, _original) = crate::csp_enforce::document_csp_policy(doc, root)?;
    let resolved = base.resolve_str(href);
    let self_origin = base.origin();
    let violated = crate::csp_enforce::violating_base_uri_policy(&policy, &resolved, self_origin.as_ref());
    if violated.is_empty() {
        return None;
    }
    Some((resolved, violated.into_iter().map(str::to_owned).collect()))
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
    let (css, dynamic_css, link_outcomes, blocked_by_style_src, blocked_inline_style_policies, blocked_style_attr_nodes, blocked_style_attr_policies) = {
        let _s = lumen_core::trace::span("fetch-css", "net");
        let link_media_ctx = if media_print {
            print_media_context(viewport, dark_mode)
        } else {
            screen_media_context(viewport, dark_mode)
        };
        // GAP-CSPENF срез 21: та же одноразовая точка чтения политики, что
        // остальные срезы уже применяют — посчитана здесь, а не передана
        // параметром, чтобы не менять сигнатуру `build_page_cascade` ради
        // одного гейта.
        let root = doc.root();
        let csp_policy = crate::csp_enforce::document_csp_policy(doc, root);
        // Инлайновые <style>: их `@import` резолвятся относительно базы
        // документа (CSS-SPECS §@import). Внешние <link> резолвят собственные
        // `@import` внутри load_linked_stylesheets.
        let (inline, blocked_inline_style_policies) =
            extract_style_blocks(doc, csp_policy.as_ref().map(|(p, _)| p.as_slice()));
        // GAP-CSPENF срез 23: `style=""` attribute — same one-shot policy read
        // as above, separate walk (attributes live on arbitrary elements, not
        // only `<style>` nodes).
        let (blocked_style_attr_nodes, blocked_style_attr_policies) =
            collect_style_attr_csp_blocked(doc, csp_policy.as_ref().map(|(p, _)| p.as_slice()));
        // GAP-CSPENF срез 38: `style-src` теперь также gates `@import`
        // targets, not only the `<style>`/`<link>` themselves — same
        // one-shot `csp_policy` read as the two gates above.
        let self_origin = base.origin();
        let (mut css, blocked_by_style_src_imports) = inline_css_imports(
            &inline,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
            &mut std::collections::HashSet::new(),
            0,
            crate::stylesheets::document_encoding(doc),
            csp_policy.as_ref().map(|(p, _)| (p.as_slice(), self_origin.as_ref())),
            crate::resource_base::document_referrer_policy(doc),
        );
        // BUG-743: всё, что не пришло из инлайновых <style>, откладывается
        // отдельно — так поздний динамический <style> пересобирает каскад без
        // единого сетевого запроса. `inline_css_imports` возвращает
        // `<импорты> + <исходный текст>`, поэтому префикс = всё до хвоста.
        let imports_prefix = css[..css.len() - inline.len()].to_owned();
        let (linked, link_outcomes, mut blocked_by_style_src) = load_linked_stylesheets(
            doc,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
        );
        blocked_by_style_src.extend(blocked_by_style_src_imports);
        css.push_str(&linked);
        let dyn_css = DynamicCssBase {
            imports_prefix,
            linked,
            inline_fp: inline_style_fingerprint(doc),
            // CSSOM-5 срез 2: placeholder — see the field's doc comment.
            adopted_fp: 0,
        };
        (css, dyn_css, link_outcomes, blocked_by_style_src, blocked_inline_style_policies, blocked_style_attr_nodes, blocked_style_attr_policies)
    };

    let sheet = {
        let _s = lumen_core::trace::span("parse-css", "parse");
        lumen_css_parser::parse(&css)
    };

    // CSSOM-1 срез 2: параллельный per-элементный реестр — не участвует в
    // каскаде выше, читает те же `<link>`-байты из уже прогретого
    // PREFETCH_CACHE (см. doc-комментарий build_stylesheet_node_registry).
    let stylesheet_nodes = {
        let _s = lumen_core::trace::span("cssom-node-sheets", "parse");
        build_stylesheet_node_registry(doc, base, sink, cookie_jar.clone())
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
            // CSS Fonts L4 §14 (FONTLOAD-11/12/13, BUG-467): ascent/descent/line-gap-override, size-adjust.
            let ascent_override = rule.ascent_override.as_deref()
                .and_then(lumen_font::parse_metric_override_percent);
            let descent_override = rule.descent_override.as_deref()
                .and_then(lumen_font::parse_metric_override_percent);
            let size_adjust = rule.size_adjust.as_deref()
                .and_then(lumen_font::parse_metric_override_percent);
            let line_gap_override = rule.line_gap_override.as_deref()
                .and_then(lumen_font::parse_metric_override_percent);
            measurer.register_family_with_overrides(
                &rule.family, bytes, ranges, ascent_override, descent_override, size_adjust,
                line_gap_override,
            );
        }
    }

    Ok(PageCascade {
        dynamic_css, link_outcomes, blocked_by_style_src, blocked_inline_style_policies,
        blocked_style_attr_nodes, blocked_style_attr_policies, sheet,
        stylesheet_nodes, font_registry, pending_web_fonts, measurer,
    })
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
    /// `node index -> per-fragment rects` (BUG-1007), for `getClientRects()`/
    /// `getBoxQuads()` — same tree `rects` was collected from.
    pub(crate) client_rects: std::collections::HashMap<u32, Vec<[f32; 4]>>,
    /// `LayoutBox` tree for `document.elementFromPoint`/`elementsFromPoint`
    /// (BUG-464/BUG-477) — same tree `rects` was collected from.
    pub(crate) tree: Arc<LayoutBox>,
    /// `node index -> property -> serialized computed value`.
    pub(crate) styles: std::collections::HashMap<u32, std::collections::HashMap<String, String>>,
    /// CSSOM-6 (BUG-490): `(node index, pseudo name) -> property -> serialized
    /// computed value` — backs `getComputedStyle(el, pseudoElt)`.
    pub(crate) pseudo_styles:
        std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>>,
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
        client_rects: lumen_layout::collect_client_rects(root, doc),
        tree: Arc::new(root.clone()),
        styles: lumen_layout::collect_computed_styles(root, doc, None),
        pseudo_styles: lumen_layout::collect_pseudo_computed_styles(root),
        customs: lumen_layout::collect_custom_properties(root, viewport),
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

/// Документ XML-flavoured (GAP-XMLDOC/BUG-786) — по MIME-типу или, если тот
/// отсутствует/generic (например, локальный `file://` без заголовков),
/// по расширению адреса. Единственное текущее следствие — CDATA-обёртка
/// `<style>`/`<script>` снимается перед CSS/JS ([`lumen_html_parser::parse_xml_flavoured`]);
/// остальные грани GAP-XMLDOC (foreign content, self-closing non-void tags)
/// этим не покрыты.
pub(crate) fn is_xml_flavoured_document(content_type: Option<&str>, base: &ResourceBase) -> bool {
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or(ct).trim().to_ascii_lowercase();
        if mime == "application/xhtml+xml"
            || mime == "image/svg+xml"
            || mime == "application/xml"
            || mime == "text/xml"
            || mime.ends_with("+xml")
        {
            return true;
        }
    }
    let path = match base {
        ResourceBase::File(p) => p.to_string_lossy().to_ascii_lowercase(),
        ResourceBase::Url(u) => u.to_ascii_lowercase(),
    };
    path.ends_with(".xhtml") || path.ends_with(".xht") || path.ends_with(".svg")
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
    push_backend: Option<Arc<dyn lumen_core::ext::PushBackend>>,
    target: lumen_core::ColorSpace,
    media_print: bool,
    // GAP-CSPENF срез 5: the response's `Content-Security-Policy` header(s),
    // empty for a non-network source. Stamped onto the document right after
    // parsing — before any script runs — so that the enforcement points,
    // which only ever receive a `&Document`, can combine them with the
    // document's `<meta>` policies. срез 41: one entry per header occurrence.
    csp_header: &[String],
    // GAP-CSPENF срез 59: see `render_bytes`'s doc comment on this parameter —
    // stamped onto the document right next to `csp_header`.
    report_to_endpoints: &std::collections::HashMap<String, Vec<String>>,
    // GAP-POLICYREPORT (BUG-953): see `render_bytes`'s doc comment on these
    // same two parameters — passed straight through to the `HttpClient` built
    // below, no per-document merge needed.
    sync_xhr_document_policy: Option<lumen_core::ext::PolicyDisposition>,
    sync_xhr_permissions_policy: Option<lumen_core::ext::PolicyDisposition>,
    // GAP-REFERRER срез 3: see `render_bytes`'s doc comment on this same
    // parameter — stamped onto the document right next to `csp_header`.
    referrer_policy_header: Option<&str>,
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
        if is_xml_flavoured_document(content_type, base) {
            lumen_html_parser::parse_xml_flavoured(&source)
        } else {
            lumen_html_parser::parse(&source)
        }
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
    // GAP-CSPENF срез 5: the response header(s) travel on the document, the
    // same way `character_set`/`content_type` above do, because CSP is
    // enforced from several places that hold nothing but a `&Document`.
    doc.set_csp_header(csp_header.to_vec());
    // GAP-CSPENF срез 59: same point, same reasoning as `csp_header` above —
    // a `report-to <group>` directive resolves against this map from
    // whichever call site fires `securitypolicyviolation`.
    doc.set_report_to_endpoints(report_to_endpoints.clone());
    // GAP-REFERRER срез 3: same point, same reasoning — combined with the
    // document's own `<meta name=referrer>` (already on `doc` from parsing)
    // by `resource_base::document_referrer_policy` at each point that needs
    // the resolved policy.
    doc.set_referrer_policy_header(referrer_policy_header.map(str::to_owned));
    let title = extract_title(&doc);

    // Гейт выполнения скриптов: top-level документ не sandboxed.
    // QuickJS + install_dom дают скриптам полный доступ к DOM-дереву.
    // fetch_provider пробрасывается в window.fetch(); ws_provider — в new WebSocket();
    // sse_provider — в new EventSource(). Все три используют один HttpClient.
    let (fetch_provider, ws_provider, sse_provider) = match base {
        ResourceBase::Url(_) => {
            // GAP-REFERRER срез 3: this client's `Referer` respects the
            // document's own `<meta name=referrer>`/`Referrer-Policy` header
            // instead of always the project default — see
            // `resource_base::document_referrer_policy`'s doc comment.
            let mut client = base.http_client_for_subresource_with_policy(
                Arc::clone(sink),
                cookie_jar.clone(),
                crate::resource_base::document_referrer_policy(&doc),
            );
            // GAP-CSPENF срез 10: gate JS-issued fetch()/XMLHttpRequest against
            // `connect-src` (or `default-src`) — WebSocket/EventSource share this
            // `HttpClient` but are not gated here, that's a separate directive
            // (`connect-src` covers them too per CSP3 §6.7.2, left for a later срез).
            // GAP-CSPENF срез 13: same document policies also gate
            // `new Worker(url)`/`new SharedWorker(url)`'s classic script fetch
            // against `worker-src` (or `default-src`) — `Worker`/`SharedWorker`
            // share this `HttpClient` the same way WebSocket/EventSource/
            // sendBeacon do (срезы 11/12), each checked against its own directive.
            // GAP-CSPENF срез 16: same document policies also gate
            // `<embed src>`/`<object data>` against `object-src` (or
            // `default-src`) — the JS shim's `_lumen_check_object_src` reads
            // this via `check_object_src`, same one-`HttpClient`-per-document
            // approach as connect-src/worker-src above.
            let root = doc.root();
            if let Some((policies, original_policy)) = crate::csp_enforce::document_csp_policy(&doc, root) {
                let self_origin = base.origin();
                client = client
                    .with_connect_src_policy(policies.clone(), self_origin.clone(), original_policy.clone())
                    .with_worker_src_policy(policies.clone(), self_origin.clone(), original_policy.clone())
                    .with_object_src_policy(policies.clone(), self_origin.clone(), original_policy.clone())
                    // GAP-CSPENF срез 17: and the same policies gate
                    // `<video src>`/`<audio src>`/`<track src>` against
                    // `media-src` (or `default-src`) — the JS media shims'
                    // `_lumen_check_media_src` reads this via `check_media_src`,
                    // same one-`HttpClient`-per-document approach as the three
                    // gates above.
                    .with_media_src_policy(policies, self_origin, original_policy);
            }
            // GAP-POLICYREPORT (BUG-953): attach the precomputed sync-xhr
            // disposition regardless of whether either header was present —
            // `with_sync_xhr_policy(None, None)` is the same as never calling it.
            client = client.with_sync_xhr_policy(sync_xhr_document_policy, sync_xhr_permissions_policy);
            // GAP-REFERRER: `Referer`/`Origin` on `fetch()`/`XMLHttpRequest`/
            // `sendBeacon` — `with_document_context` is attached by
            // `http_client_for_subresource_with_policy` above, using this
            // document's resolved policy (срез 3) rather than always the
            // project default (срез 2).
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
        let eff_base = effective_base(&doc, base);
        (
            resolve_script_sources(&classic_items, &eff_base, sink, cookie_jar.clone(), &doc),
            resolve_script_sources(&module_items, &eff_base, sink, cookie_jar.clone(), &doc),
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
    // STTF-1: a `:~:text=...` scroll-to-text directive is not part of the
    // element-id fragment — `text_fragment::parse_fragment` strips it so
    // `:target` never tries to match a raw directive string against an `id`.
    let page_fragment = if let ResourceBase::Url(u) = base {
        lumen_core::url::Url::parse(u)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
            .and_then(|f| text_fragment::parse_fragment(&f).element_id)
    } else {
        None
    };
    doc.set_target(page_fragment.as_deref());
    let mut cascade = build_page_cascade(
        &doc, &effective_base(&doc, base), sink, cookie_jar.clone(), viewport, dark_mode, media_print,
    )?;
    // GAP-CSPENF срез 23: hand the decision down to `Document` before the
    // first layout below reads any `style=""` attribute — see
    // `Document::style_attr_csp_blocked`'s doc comment for why this travels
    // as bare node ids rather than the `CspPolicy` itself.
    doc.set_style_attr_csp_blocked(cascade.blocked_style_attr_nodes.clone());
    // Fingerprints of the two stylesheet sources, so the rebuild below can tell
    // whether the scripts touched either. Cheap: two tree walks, no fetching.
    let css_sources_before = (inline_style_fingerprint(&doc), stylesheet_link_fingerprint(&doc));

    // FONTLOAD-4: populate `document.fonts` from the STATIC (pre-script)
    // cascade's `@font-face` rules, BEFORE scripts run — must happen ahead of
    // `run_scripts_with_dom` below, not after it (the ordering this block had
    // since FONTLOAD-1/2): `document.fonts`'s JS wrapper caches its
    // `FontFaceSet` snapshot on first touch (`_lumen_wrapper_slot(this,
    // '__fonts__', ...)`, `crates/js/src/shim/web_api_shim_mid.js`), so a
    // synchronous top-level script read — the exact
    // `document.fonts.ready.then(...)`-before-any-`test()` WPT pattern
    // `bugs/BUG-467-OPEN.md` "gap 0" describes — froze the set empty for the
    // page's whole life, same class of defect FONTLOAD-3 fixed for
    // `InProcessSession` (`crates/driver/src/font_faces.rs`), reproduced here
    // for the shell/`run_report.py` path by a probe script reading
    // `document.fonts.size` synchronously (always `0`, even for a resolvable
    // `local()` face). Uses the pre-script cascade already built above, not a
    // separate early parse like driver's — shell already has one; a script
    // that inserts its own `<style>`/`@font-face` before layout still reaches
    // the layout cascade exactly as before, it just isn't reflected in
    // `document.fonts` (same one-shot-snapshot limitation already documented
    // for CSS-connected reactivity, not a new regression).
    for rule in &cascade.sheet.font_faces {
        let mut font_face = rule_to_font_face(rule);
        // local() rules already resolved — mark Loaded; url() rules queued
        // for the background fetch `apply_loaded_page` spawns — mark
        // Loading, not left at the constructor's `Unloaded` default
        // (FONTLOAD-5, `bugs/BUG-467-OPEN.md`): a synchronous top-level
        // `document.fonts.ready.then(...)` read (this whole track's target
        // pattern) needs to see this face as pending, not as "nothing to
        // wait for", or `ready` resolves before the fetch even starts. A
        // rule with neither (unresolved local() and no url() fallback at
        // all) stays `Unloaded` — nothing will ever load it, so marking it
        // `Loading` would leave `ready` pending forever instead.
        let has_local = rule.sources.iter().any(|s| {
            s.kind == lumen_css_parser::FontFaceSourceKind::Local
                && cascade.font_registry.face_bytes_for_family(&rule.family).is_some()
        });
        if has_local {
            font_face.status = lumen_dom::FontFaceStatus::Loaded;
        } else if rule.sources.iter().any(|s| s.kind == lumen_css_parser::FontFaceSourceKind::Url) {
            font_face.status = lumen_dom::FontFaceStatus::Loading;
        }
        doc.fonts_mut().add(font_face);
    }

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
    // CSSOM-7 (BUG-977): same gate as `parse_time_snapshot` — nothing to flush
    // against if there is no script that could read it. `Stylesheet::clone()`
    // is a real (hand-written, revision-minting) deep copy, not an `Arc`
    // handle — paid once per navigation with scripts, not per relayout.
    let parse_time_stylesheet = parse_time_snapshot
        .is_some()
        .then(|| Arc::new(cascade.sheet.clone()));

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
        push_backend: push_backend.clone(),
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
        push_backend,
        cookie_banner_dismiss,
        deterministic,
        cross_origin_isolated,
        &ext_scripts,
        classic_scripts,
        module_scripts,
        false,
        parse_time_snapshot,
        cascade.stylesheet_nodes.clone(),
        parse_time_stylesheet,
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
        let mut d = doc_arc.lock().unwrap();
        cascade = build_page_cascade(
            &d, &effective_base(&d, base), sink, cookie_jar.clone(), viewport, dark_mode, media_print,
        )?;
        // GAP-CSPENF срез 23: re-derived alongside the rest of `cascade` —
        // scripts may have inserted new elements with a `style=""` attribute
        // before touching `<style>`/`<link>` (the trigger for this branch).
        d.set_style_attr_csp_blocked(cascade.blocked_style_attr_nodes.clone());
    }
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        // CSSOM-5 срез 2 (BUG-897): a parse-time synchronous script may
        // already have assigned `document.adoptedStyleSheets` — fold its
        // content into the cascade before this block's snapshot (and the
        // real layout further down) read `cascade.sheet`. Assigning the
        // property doesn't mutate any DOM node, so `dom_dirty` never sees
        // it — this rides its own fingerprint instead.
        let adopted_fp = js.document_adopted_fingerprint();
        let adopted_changed = adopted_fp != cascade.dynamic_css.adopted_fp;
        if adopted_changed {
            if let Some(adopted) = js.document_adopted_stylesheet() {
                cascade.sheet.merge_from(adopted);
            }
            cascade.dynamic_css.adopted_fp = adopted_fp;
        }
        // Nothing to re-derive if the scripts changed neither the cascade nor
        // the tree: the snapshot pushed before they ran is still current, and
        // this is the one place a whole layout pass can be skipped. The flag is
        // only *read* here — `take_dom_dirty` would swallow the relayout the
        // shell schedules for itself after the load.
        let dom_touched = js
            .dom_dirty_flag()
            .is_none_or(|f| f.load(std::sync::atomic::Ordering::Relaxed));
        // GAP-CSPENF срез 37: `scripts_changed_css` above only re-derives
        // `style_attr_csp_blocked` when a script touched `<style>`/`<link>`
        // (срез 23's own trigger, `build_page_cascade`'s one-shot read) — a
        // script that only sets `style=""` on an existing or newly created
        // element (`setAttribute`/`style.cssText`/`style.setProperty`,
        // without adding/removing a `<style>`/`<link>`) left this list at its
        // parse-time snapshot even though `dom_touched` already says the tree
        // moved. Re-walking here is cheap (no cascade/layout rebuild, just
        // the attribute scan `build_page_cascade` already runs once) and
        // widens the same срез 23 gate from "stylesheet set changed" to
        // "the DOM changed at all" — still only this one post-script
        // checkpoint, not a live per-mutation hook: a later async mutation
        // (event handler, timer) after this point is unchanged and remains
        // unenforced, same limitation срез 23 already documented.
        if dom_touched && !scripts_changed_css {
            let mut d = doc_arc.lock().unwrap();
            let root = d.root();
            let csp_policy = crate::csp_enforce::document_csp_policy(&d, root);
            let (blocked_style_attr_nodes, _) =
                collect_style_attr_csp_blocked(&d, csp_policy.as_ref().map(|(p, _)| p.as_slice()));
            d.set_style_attr_csp_blocked(blocked_style_attr_nodes);
        }
        if scripts_changed_css || dom_touched || adopted_changed {
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
            js.update_client_rects(snapshot.client_rects);
            js.update_computed_styles(snapshot.styles);
            js.update_pseudo_computed_styles(snapshot.pseudo_styles);
            js.update_custom_properties(snapshot.customs);
            // CSSOM-7 (BUG-977): re-push alongside the snapshot above so a
            // `DOMContentLoaded` handler (about to fire right after this
            // block) has an up-to-date flush target too — covers the sheet
            // that was just rebuilt (`scripts_changed_css`) as well as the
            // unchanged one (`dom_touched`/`adopted_changed` alone), same
            // "cheap enough, simpler than gating separately" call as
            // `update_stylesheet_nodes` below.
            js.update_stylesheet(Arc::new(cascade.sheet.clone()));
            // CSSOM-1 срез 3: re-push whenever this block runs, even though
            // `cascade.stylesheet_nodes` only actually changed when
            // `scripts_changed_css` triggered the rebuild above — cheap
            // (`Vec` of `Arc` clones) and simpler than gating separately.
            js.update_stylesheet_nodes(cascade.stylesheet_nodes.clone());
            js.update_viewport_size(snapshot.viewport.0, snapshot.viewport.1);
        }
    }

    // HTML LS §8.2.3 — after HTML parse + inline scripts: readyState → "interactive"
    // + DOMContentLoaded event. Fires before images/fonts are decoded.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        // BUG-640: bracket the real dispatch — `domInteractive`/
        // `domContentLoadedEventStart` share the "before" instant,
        // `domContentLoadedEventEnd` is the "after" one.
        crate::nav_timing::record_dom_content_loaded_start();
        js.notify_dom_content_loaded();
        crate::nav_timing::record_dom_content_loaded_end();
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
        let eff_base = effective_base(&doc_arc.lock().unwrap(), base);
        load_frame_sub_documents(&doc_arc, 0, &eff_base, &doc_arc, &frame_env, js_ctx.as_ref())
    };

    // Fetch + decode <img src>. Должно идти ДО layout, потому что intrinsic
    // dimensions из декодированного изображения проставляются как HTML
    // presentational hints (width/height attribute) и потом подхватываются
    // style cascade. Errors silently пропускаются — битая картинка не валит
    // всю страницу, layout нарисует серый placeholder.
    // loading="lazy" изображения возвращаются в lazy_pairs и не загружаются сейчас.
    let (images, animated_gifs, lazy_pairs, blocked_by_img_src, cross_origin_img_urls) = {
        let _s = lumen_core::trace::span("fetch-images", "net");
        let mut d = doc_arc.lock().unwrap();
        let eff_base = effective_base(&d, base);
        fetch_and_decode_images(&mut d, &eff_base, sink, viewport, cookie_jar.clone(), target)
    };
    // GAP-CSPENF срез 4: `securitypolicyviolation` for every `img-src`-blocked
    // URL. `blocked_by_img_src` came back from a fetch pass that ran before
    // this runtime existed (parallel, off-thread), so this is the first point
    // able to dispatch it — same one-shot-push shape as the `script-src` push
    // in `scripts.rs`. Re-parsing the `<meta>` CSP here is cheap (a handful of
    // attributes) and keeps `fetch_and_decode_images`'s return shape from
    // having to carry the raw policy text just for this.
    #[cfg(feature = "v8")]
    if !blocked_by_img_src.is_empty()
        && let Some(js) = &js_ctx
    {
        let img_src_policy = {
            let d = doc_arc.lock().unwrap();
            let root = d.root();
            crate::csp_enforce::document_csp_policy(&d, root)
        };
        if let Some((policy, original_policy)) = &img_src_policy {
            let self_origin = base.origin();
            // Срез 56/58: `originalPolicy` — текст КАЖДОЙ нарушенной политики
            // (CSP3 §7.8/§3.4), не только первой.
            for url in &blocked_by_img_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::ImgSrc, url, self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("img-src", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("img-src", url, text);
                    }
                }
            }
        }
    }

    // P3-webvtt срез 3: загрузка WebVTT-субтитров из <track> каждого <video>.
    // Ошибки фетча/парсинга не валят страницу — видео просто остаётся без cues.
    // GAP-CSPENF срез 17: this is the *second* place a `<track src>` body is
    // fetched — the JS shim's `readTrackBody` is the other, gated by the native
    // `_lumen_check_media_src` binding. This one runs before any JS exists, has
    // a `&Document`, and so is gated here in the shell instead, the same way
    // img-src/style-src are (срезы 4/7). A live probe with `media-src 'none'`
    // showed the shim's gate alone still let `GET /cap.vtt` onto the wire from
    // here, so both halves are needed for the "not a single outgoing byte"
    // invariant to actually hold for `<track>`.
    let (page_tracks, blocked_by_media_src) = {
        let d = doc_arc.lock().unwrap();
        let eff_base = effective_base(&d, base);
        let root = d.root();
        let media_policy = crate::csp_enforce::document_csp_policy(&d, root);
        // GAP-REFERRER срез 5: same one-shot read the other producers in
        // this function already use.
        let referrer_policy = crate::resource_base::document_referrer_policy(&d);
        let self_origin = base.origin();
        let blocked = std::cell::RefCell::new(Vec::new());
        let tracks = tracks::load_video_tracks(&d, &|src| {
            if let Some((policy, _)) = &media_policy
                && let ResolvedResource::Url(abs) = eff_base.resolve(src)
            {
                // GAP-CSPENF срез 50: upgrade-insecure-requests для `<track
                // src>` — тот же порядок Fetch §4.1 (шаг 5 апгрейда раньше
                // шага 6 блокировки), что срезы 43-49 уже применяют к
                // остальным parser-driven подресурсам (`gate_url`, а не
                // `abs`, идёт и в `media_src_blocked`, и в `fetch_vtt_text`).
                let gate_url = crate::csp_enforce::upgrade_insecure_url(policy, &abs).unwrap_or(abs);
                // Срез 56/58: захватываем текст КАЖДОЙ нарушенной политики
                // (CSP3 §7.8/§3.4) здесь же, пока `policy`/`self_origin` в
                // скоупе — дешевле и точнее, чем пересчитывать
                // `document_csp_policy` заново в точке диспатча ниже; фетч
                // блокируется однократно, событий — по одному на политику.
                let violated = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::MediaSrc, &gate_url, self_origin.as_ref(),
                );
                if !violated.is_empty() {
                    let mut blocked = blocked.borrow_mut();
                    for policy_text in &violated {
                        blocked.push((gate_url.clone(), (*policy_text).to_owned()));
                    }
                    return None;
                }
                return fetch_vtt_text(&gate_url, &eff_base, sink, cookie_jar.clone(), referrer_policy);
            }
            fetch_vtt_text(src, &eff_base, sink, cookie_jar.clone(), referrer_policy)
        });
        (tracks, blocked.into_inner())
    };

    // Same deferred-dispatch shape as the `img-src` push above
    // (`blocked_by_img_src`, срез 4): the violation is reported once the JS
    // runtime exists, because the block itself happened before it did.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        for (url, policy_text) in &blocked_by_media_src {
            js.fire_csp_violation("media-src", url, policy_text);
        }
    }
    #[cfg(not(feature = "v8"))]
    let _ = &blocked_by_media_src;

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
        // GAP-CANVASORIGIN (BUG-941): `cross_origin_img_urls` came back from
        // the same fetch pass, keyed by the same raw `req.url` — carry the
        // taint bit into `img_bitmap_store` alongside the pixels.
        let cross_origin_set: std::collections::HashSet<&str> =
            cross_origin_img_urls.iter().map(String::as_str).collect();
        let bitmaps: Vec<(u32, std::sync::Arc<lumen_image::Image>, bool)> = img_reqs
            .iter()
            .filter_map(|req| {
                let img = url_to_img.get(req.url.as_str())?;
                let tainted = cross_origin_set.contains(req.url.as_str());
                Some((req.node_id.index() as u32, std::sync::Arc::clone(img), tainted))
            })
            .collect();
        if !bitmaps.is_empty() {
            js.register_img_bitmaps(bitmaps);
        }
        // BUG-630 (GAP-LOADEV срез 1): a request that isn't `loading="lazy"`
        // (those are deferred to `fetch_and_register_lazy_images` and fire
        // their own events there) and made it out of `fetch_and_decode_images`
        // either found its URL decoded in `images` (success — same join
        // `url_to_img` above already does) or was silently dropped, as a
        // fetch/decode failure (`ImgOutcome::Skip`) or a CSP block
        // (`ImgOutcome::Blocked`, GAP-CSPENF срез 4 — `securitypolicyviolation`
        // for those already fired above) — either way "not in `url_to_img`"
        // is exactly the failure case HTML LS §4.8.4.3 wants an `error` for.
        for req in &img_reqs {
            if req.is_lazy {
                continue;
            }
            let nid = req.node_id.index() as u32;
            match url_to_img.get(req.url.as_str()) {
                Some(img) => js.fire_image_load(nid, img.width, img.height),
                None => js.fire_image_error(nid),
            }
        }
    }

    // BUG-443: the cascade was collected before the scripts ran (and rebuilt
    // right after them if they touched `<style>`/`<link>`), so there is nothing
    // left to fetch or parse here — only to hand out.
    let PageCascade {
        dynamic_css, link_outcomes, blocked_by_style_src, blocked_inline_style_policies,
        blocked_style_attr_nodes: _, blocked_style_attr_policies, sheet,
        stylesheet_nodes, font_registry, pending_web_fonts, measurer,
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

    // GAP-CSPENF срез 7: `securitypolicyviolation` for every `style-src`-
    // blocked `<link rel=stylesheet>` — same one-shot-push shape as the
    // `img-src` push above (`blocked_by_img_src`, срез 4).
    #[cfg(feature = "v8")]
    if !blocked_by_style_src.is_empty()
        && let Some(js) = &js_ctx
    {
        let style_src_policy = {
            let d = doc_arc.lock().unwrap();
            let root = d.root();
            crate::csp_enforce::document_csp_policy(&d, root)
        };
        if let Some((policy, original_policy)) = &style_src_policy {
            let self_origin = base.origin();
            // Срез 58: одно событие на каждую нарушенную политику (CSP3
            // §7.8/§3.4), не только на первую.
            for url in &blocked_by_style_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::StyleSrc, url, self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("style-src", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("style-src", url, text);
                    }
                }
            }
        }
    }

    // GAP-CSPENF срез 21/57: `securitypolicyviolation` for every `style-src`-
    // blocked inline `<style>` — no URL, `blockedURI` is always `"inline"`,
    // same one-shot-push shape as `blocked_by_style_src` above. Срез 57:
    // each dispatch now carries the text of the policy `extract_style_blocks`
    // found actually violated for that node, not the document's combined text.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        for text in &blocked_inline_style_policies {
            js.fire_csp_violation("style-src", "inline", text);
        }
    }

    // GAP-CSPENF срез 23/57: `securitypolicyviolation` for every
    // `style-src-attr`-blocked `style=""` attribute — `blockedURI` is
    // `"inline"`, same as the inline `<style>` block above; `violatedDirective`
    // is `style-src-attr` (CSP3 §6.4's granular directive for the attribute
    // form), not `style-src`. Срез 57: same per-node policy text switch as
    // the inline `<style>` dispatch above.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        for text in &blocked_style_attr_policies {
            js.fire_csp_violation("style-src-attr", "inline", text);
        }
    }

    // GAP-CSPENF срез 32: `securitypolicyviolation` for a `base-uri`-blocked
    // `<base href>` — the actual block already happened, silently, inside
    // every `effective_base` call above (`base_uri_href_blocked`); this is
    // only the one-shot report, read fresh from the post-script document the
    // same way the style-src-attr block above does, since a script can
    // insert/change `<base>` after the initial parse.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        let blocked = {
            let d = doc_arc.lock().unwrap();
            d.base_href()
                .and_then(|href| base_uri_href_blocked(&d, base, href))
        };
        if let Some((blocked_href, policy_texts)) = blocked {
            // Срез 58: одно событие на каждую нарушенную политику.
            for policy_text in &policy_texts {
                js.fire_csp_violation("base-uri", &blocked_href, policy_text);
            }
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
    //
    // GAP-CSPENF срез 18: `img-src`/`default-src` теперь гейтит и этот
    // производитель — до этого среза `fetch_and_decode_background_images`
    // фетчила байты фона безусловно, в обход гейта, который срез 4 уже дал
    // `<img src>`. Политика документа считается один раз (тот же
    // `document_csp_policy`, что и остальные срезы), заблокированные URL
    // диспатчат `securitypolicyviolation` после фетча, тем же отложенным
    // one-shot-push, что срез 4 уже применяет к `blocked_by_img_src` — здесь
    // это не "до JS-рантайма", а просто "после параллельного фетча".
    let mut images = images;
    let blocked_by_bg_img_src = {
        let _s = lumen_core::trace::span("fetch-bg-images", "net");
        let d = doc_arc.lock().unwrap();
        let eff_base = effective_base(&d, base);
        let root = d.root();
        let bg_policy = crate::csp_enforce::document_csp_policy(&d, root);
        // GAP-REFERRER срез 5: same one-shot read as `bg_policy` above,
        // before `d` drops — this producer has no other point with a live
        // `&Document`.
        let referrer_policy = crate::resource_base::document_referrer_policy(&d);
        let self_origin = base.origin();
        drop(d);
        let csp_gate = bg_policy
            .as_ref()
            .map(|(policy, _)| (policy.as_slice(), self_origin.as_ref()));
        let (decoded, blocked) = fetch_and_decode_background_images(
            &layout, &eff_base, sink, cookie_jar.clone(), target, csp_gate, referrer_policy,
        );
        for (src, image) in decoded {
            images.push((src, image));
        }
        blocked
    };
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        for (url, policy_text) in &blocked_by_bg_img_src {
            js.fire_csp_violation("img-src", url, policy_text);
        }
    }
    #[cfg(not(feature = "v8"))]
    let _ = &blocked_by_bg_img_src;
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
        stylesheet_nodes,
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
