//! The `Lumen` struct itself: every piece of state one browser window holds
//! between two frames.
//!
//! The methods that read and write these fields live in the sibling modules of
//! `crate::lumen`; the declaration sits here because a type and its `impl`
//! blocks belong together, and `main.rs` is meant to be a bootstrap.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Field order, types,
//! `#[cfg]` gates and doc comments are unchanged; the only edit is visibility
//! (`pub(crate)` on the struct and on each field), which every other module of
//! the crate needs now that the fields no longer sit in the crate root.

use crate::*;

pub(crate) struct Lumen {
    pub(crate) display_list: DisplayList,
    /// Версия [`Self::display_list`] для рендерера (BUG-405 срез 39).
    ///
    /// Меняется при КАЖДОМ изменении списка — и замене целиком, и правке на
    /// месте, — поэтому список меняют только через [`Self::set_display_list`]
    /// и [`Self::display_list_mut`], а не присваиванием полю. Пока версия та
    /// же, рендерер переиспользует свёртку content-части кадровых хэшей вместо
    /// обхода всего списка; пропущенный бамп показал бы устаревшие пиксели.
    /// Никогда не `0`: ноль зарезервирован за «версия неизвестна».
    pub(crate) display_list_epoch: u64,
    /// Tile-based dirty-rect tracker. Updated on every display-list change via
    /// [`lumen_paint::TileGrid::update_from_diff`]. Dirty tiles are re-rendered
    /// on the next frame; clean tiles reuse the previous output (Phase 2).
    pub(crate) tile_grid: lumen_paint::TileGrid,
    /// Per-subtree display-list cache. Keyed by stacking-context root `NodeId`.
    /// Hit on a matching `content_hash` → skip re-traversing the layout tree for
    /// that subtree. Registered with `cache_registry` so OS memory-pressure
    /// events evict it via `EvictableCache::on_memory_pressure` (EE-4).
    pub(crate) display_list_cache: lumen_paint::DisplayListCache,
    pub(crate) title: Option<String>,
    /// Декодированные `<img>` ресурсы. До создания Renderer-а — хранятся
    /// в Vec и заливаются в GPU в `resumed`; после — register_image идёт
    /// напрямую в `reload`. На переходах между страницами очищается через
    /// `Renderer::clear_images` + переустановка. `Arc<Image>` (BUG-272 срез 17).
    pub(crate) pending_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// PH3-19: реестр шрифтов текущей страницы (local() + web-шрифты, пришедшие
    /// через `FontLoaded`). Хранится отдельно от `Arc<dyn FontProvider>` в renderer-е,
    /// чтобы `user_event(FontLoaded)` мог дорегистрировать шрифт через
    /// `register_from_bytes` без даункаста, а затем обновить рендерер одной строкой.
    /// Сбрасывается на каждую навигацию вместе с `web_fonts`.
    pub(crate) page_font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: web-шрифты текущей страницы, уже декодированные из @font-face url().
    /// Используются для пересборки `MultiFontMeasurer` при каждом relayout (resize,
    /// scroll, JS DOM mutation) — без хранения здесь resize-relayout терял бы
    /// web-метрики и откатывался к Inter.  Очищается на каждой навигации.
    pub(crate) web_fonts: Vec<LoadedWebFont>,
    pub(crate) source: PageSource,
    pub(crate) event_sink: Arc<dyn EventSink>,
    pub(crate) modifiers: ModifiersState,
    pub(crate) window: Option<Arc<Window>>,
    /// Detected target `ColorSpace` for the active display.
    /// Populated at startup from the OS (Windows WCS/DXGI/EDID query).
    /// Defaults to `ColorSpace::Srgb` when the display profile is unknown or
    /// the OS query fails — making the whole wide-gamut pipeline a no-op on
    /// sRGB-only hardware.
    #[allow(dead_code)] // потребитель появится при P3 wiring (ph3-color-management Step 1)
    pub(crate) display_color_profile: platform::display_color_profile::PlatformDisplayColorProfile,
    pub(crate) renderer: Option<Box<dyn RenderBackend>>,
    /// CC-4: chrome document + stylesheet, parsed once at startup via
    /// [`lumen_chrome::parse_document`] from `chrome_preview::HTML` — the same
    /// bytes `build.rs` already CSS-gated. Only relaid out on resize
    /// ([`Lumen::relayout_chrome_host`]); the asset has no dynamic content yet
    /// (`ChromeModel` DOM mutation is CC-6), so nothing else invalidates it.
    ///
    /// CC-15-6: always `Some` since the `LUMEN_LEGACY_CHROME` rollback flag was
    /// deleted — the `Option` is now only the shape every accessor already reads
    /// through, not a live "no engine chrome" mode.
    pub(crate) chrome_doc: Option<(lumen_dom::Document, lumen_css_parser::Stylesheet)>,
    /// CC-4: `LayoutBox` + display list of the last `relayout_chrome_host` pass,
    /// painted at the front of `overlay_buf` every frame (legacy panels/tab-bar/
    /// toolbar still draw over it, painter's order). `None` until the first
    /// resize after startup provides a window size. `#contentArea` — the
    /// design reference's placeholder for tab content, doubling as the
    /// brief's "`#page-host`" — is pruned out of this tree entirely (not just
    /// its children) before painting, so neither its demo markup nor its own
    /// `background:var(--surface-0)` fill can end up on top of the real page
    /// painted separately at [`Self::chrome_page_host_rect`]'s rect.
    pub(crate) chrome_layout: Option<(lumen_layout::LayoutBox, lumen_paint::DisplayList)>,
    /// CC-4: `#contentArea`'s rect, captured from the layout tree right
    /// before [`Self::relayout_chrome_host`] prunes that node out — replaces
    /// the legacy `left_dock()` width / `toolbar::CHROME_H` pair at the two
    /// render-time page-offset call sites. `None` until the first chrome
    /// layout exists (mirrors `chrome_layout`).
    pub(crate) chrome_page_host_rect: Option<Rect>,
    /// CC-5 (docs/tasks/p1-css-chrome.md): hovered node in `chrome_layout`'s
    /// tree, or `None` when the pointer isn't over the chrome's own opaque
    /// area (or off the flag). Set from `WindowEvent::CursorMoved`; feeds
    /// `:hover` into the next [`Self::relayout_chrome_host`] pass. Kept
    /// separate from [`Self::hovered_nid`] (the page's own hover state) for
    /// the same reason `relayout_chrome_host` explicitly resets the
    /// interactive thread-locals rather than inheriting them — the two
    /// documents' hover state must never leak into each other's layout pass.
    pub(crate) chrome_hovered_nid: Option<NodeId>,
    /// CC-5: pressed node in `chrome_layout`'s tree — mirrors
    /// [`Self::chrome_hovered_nid`] but for `:active`, set from
    /// `WindowEvent::MouseInput` press.
    pub(crate) chrome_active_nid: Option<NodeId>,
    /// CC-7 (docs/tasks/p1-css-chrome.md): `#omniInput`'s post-layout rect
    /// from the last [`Self::relayout_chrome_host`] pass — the anchor for the
    /// hand-painted caret overlay (editing itself stays owned by the legacy
    /// `address_bar::AddressBarState`, no native caret exists for `<input>`
    /// yet). `None` off the flag or before the first chrome layout.
    pub(crate) chrome_omni_input_rect: Option<Rect>,
    /// CC-8 (docs/tasks/p1-css-chrome.md): `true` collapses the vertical
    /// sidebar to its icon rail (`#sidebar.collapsed`, `--sidebar-w-collapsed`
    /// in the asset). Toggled by `ChromeAction::ToggleSidebar`
    /// (`.sb-collapse` button). Independent of [`Self::vertical_tabs`]'s own
    /// `visible` flag — that one picks vertical vs. horizontal layout,
    /// this one narrows the vertical sidebar without hiding it.
    pub(crate) chrome_sidebar_collapsed: bool,
    /// CC-10b (docs/tasks/p1-css-chrome.md): `data-section` slug of the
    /// active `#view-settings` tab (`"general"`/`"privacy"`/`"appearance"`/
    /// `"sync"`/`"ext"`/`"qa"`). Engine-chrome-only UI state — the design's 6
    /// sections don't line up with `SettingsPanel::SettingsSection`'s 7 (see
    /// `lumen_chrome::ChromeSettingsModel` doc comment), so this is a
    /// separate field rather than a projection of the legacy enum. Set by
    /// `ChromeAction::SetSettingsSection`.
    pub(crate) chrome_settings_section: String,
    /// CC-11 (docs/tasks/p1-css-chrome.md): CSS Animations scheduler for the
    /// chrome document — a separate instance from [`Self::animation_scheduler`]
    /// because `chrome_doc` and the page `Document` number `NodeId`s
    /// independently (both start at 0), so a shared scheduler would collide
    /// entries between the two trees. Ticked on every `RedrawRequested`
    /// alongside the page scheduler.
    /// Unlike the page scheduler, never `.clear()`-ed: `chrome_doc`'s nodes
    /// persist for the process lifetime (no reload/navigation equivalent for
    /// chrome), so clearing on every [`Self::relayout_chrome_host`] call —
    /// which happens far more often than page relayouts (any hover/click) —
    /// would restart `infinite` animations (the spinner) on every interaction.
    pub(crate) chrome_animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CC-11: CSS Transitions scheduler for the chrome document — mirrors
    /// [`Self::transition_scheduler`] but keyed against `chrome_doc`'s own
    /// `NodeId` space (see [`Self::chrome_animation_scheduler`] doc comment).
    /// `sync()` runs at the end of [`Self::relayout_chrome_host`] (chrome's
    /// post-layout point, mirroring `apply_relayout_result`'s page-side
    /// sync); `tick()` runs on every `RedrawRequested`.
    pub(crate) chrome_transition_scheduler: TransitionScheduler,
    /// CC-11: computed styles from the previous [`Self::relayout_chrome_host`]
    /// pass — needed by [`Self::chrome_transition_scheduler`]'s `sync()` to
    /// detect which properties changed. Mirrors [`Self::prev_styles`] for the
    /// chrome tree.
    pub(crate) chrome_prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S5/S22: what the previous pass's [`take_content_area`] removed
    /// from [`Self::chrome_layout`].
    ///
    /// The incremental graft needs the *pristine* (pre-pruning) tree of the
    /// previous pass: it matches children **by index**, and pruning
    /// `#contentArea` shifts every sibling after it (see BUG-341's "attempted
    /// mitigation" note, which hit exactly that). S5 met this by keeping a
    /// whole second copy of the tree (`chrome_prev_pristine_layout =
    /// layout.clone()`), which the S22 census priced at 0.16-0.40 ms per
    /// cycle — the largest item left in a chrome interaction, and ~40 % of a
    /// hover cycle. S22 keeps the *difference* instead: the pruning is
    /// recorded here and undone by [`restore_content_area`] at the top of the
    /// next pass, so the live tree in [`Self::chrome_layout`] becomes the
    /// basis and no copy is made at all. Sound because nothing mutates
    /// `chrome_layout` between passes — it is read-only until
    /// [`Self::relayout_chrome_host`] replaces it wholesale.
    pub(crate) chrome_content_area_detached: Option<ContentAreaDetachment>,
    /// BUG-341 S5: the per-node `ComputedStyle` cascade cache
    /// ([`lumen_layout::CounterMap::styles`]) from the previous pass —
    /// `RestyleDelta::prev_styles` for the next incremental cascade. Distinct
    /// from [`Self::chrome_prev_styles`] (CC-11's transition-sync snapshot,
    /// collected from post-layout `LayoutBox`es *after* `font-size-adjust` has
    /// mutated them in place) — the cascade cache must be the pre-layout,
    /// pre-adjust styles the cascade itself produced, or the incremental
    /// cascade's `incr == full` correctness gate (BUG-341 brief §4) would
    /// compare against the wrong reference.
    pub(crate) chrome_prev_cascade_styles: lumen_layout::CascadeStyles,
    /// BUG-341 S5: `(hover, focus, active)` node ids from the previous pass —
    /// `restyle_root_set_for_state_change`'s `prev` argument for each axis, so
    /// a hover/focus/active transition can compute its conservative dirty
    /// root-set (brief §4).
    pub(crate) chrome_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// BUG-341 S5: viewport size the previous pass laid out at — a resize
    /// invalidates the previous tree's geometry for `graft_geometry` purposes,
    /// so a viewport change forces the full-layout path regardless of what
    /// `bind_model_tracked` reports touched.
    pub(crate) chrome_prev_viewport: Option<Size>,
    /// BUG-341 S5: Forced Colors Mode state ([`lumen_layout::forced_colors_active`])
    /// the previous pass ran under. Not part of `ChromeModel` (it's a
    /// thread-local accessibility preference, not shell UI state), but it does
    /// feed the cascade — a change here must force a full recompute (the
    /// `bind_model_tracked` diff cannot see it, since it never touches `doc`),
    /// or the incremental path would reuse `chrome_prev_cascade_styles`
    /// computed under the wrong Forced-Colors state.
    pub(crate) chrome_prev_forced_colors: bool,
    /// BUG-405 срез 48 (диагностика, п.85): total-хэш ([`lumen_paint::hash_display_list`]
    /// с пустым content-лейном) предыдущего [`Self::relayout_chrome_host`]-прохода —
    /// только сравнение с текущим под `LUMEN_FRAME_LOG=2`, ни на что не влияет.
    /// Нужен, чтобы измерить, насколько надёжно `touched.is_empty()` +
    /// стабильность interactive/viewport/forced-colors предсказывают, что байты
    /// `chrome_dl` не изменились — предпосылка content_epoch-архитектуры overlay
    /// (срез 46), которую этот срез проверяет БЕЗ правки самого relayout-пути.
    pub(crate) chrome_dl_content_hash: Option<u64>,
    /// BUG-405 срез 50: monotonic version of [`Self::chrome_layout`], bumped
    /// unconditionally by every [`Self::relayout_chrome_host`] pass — see
    /// [`ChromeOverlayFrameCache`]'s doc comment for why "unconditional" (not
    /// "only when bytes changed") is the correct, safe choice here.
    pub(crate) chrome_layout_generation: u64,
    /// BUG-405 срез 50: the last `RedrawRequested`'s assembled chrome overlay
    /// segment, reused verbatim on a later frame when nothing that shapes it
    /// has changed — see [`ChromeOverlayFrameCache`]'s own doc comment.
    pub(crate) chrome_overlay_frame_cache: Option<ChromeOverlayFrameCache>,
    /// CC-11: last computed animation/transition frame for the chrome
    /// document. `None` when the chrome flag is off or nothing is currently
    /// animating. Only the compositor-offloadable properties (opacity,
    /// transform, color, background-color) are applied — same limitation as
    /// [`Self::anim_frame`] for the page, since `width` transitions
    /// (`#sidebar`, `.dl-progress-fill`) aren't in the Phase-0 animatable
    /// property table (`TransitionScheduler::sync`) and stay unanimated.
    pub(crate) chrome_anim_frame: Option<lumen_layout::AnimationFrame>,
    /// HTML event loop runtime. На каждой итерации winit-loop (AboutToWait)
    /// выполняется одна task, на RedrawRequested — run_rendering_step
    /// (вызывает rAF-callback-и), на WindowEvent::Resized —
    /// deliver_observer_records(Resize).
    pub(crate) runtime: runtime::EventLoop,
    /// CSS Animations timeline scheduler — тикается на каждом RedrawRequested.
    /// Хранит start-time для каждой запущенной анимации и вычисляет
    /// интерполированные значения. Очищается при load/reload.
    pub(crate) animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CSS Transitions scheduler — reactive; обнаруживает изменения computed-style
    /// между двумя relayout-ами и интерполирует значения per-frame.
    /// `sync()` вызывается после каждого layout-обновления; `tick()` — на каждом
    /// RedrawRequested вместе с animation_scheduler. Очищается при load/reload.
    pub(crate) transition_scheduler: TransitionScheduler,
    /// Tracks nodes that are "entering" the document (inserted or display:none→visible)
    /// so that `@starting-style` rules can provide the before-change style for their
    /// entry transitions (CSS Transitions L2 §3.4). Consumed in `relayout()`.
    pub(crate) starting_style_tracker: StartingStyleTracker,
    /// Computed styles предыдущего layout-дерева — нужны `transition_scheduler.sync()`
    /// для определения изменившихся свойств. Обновляется после каждого layout.
    pub(crate) prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S7: `CounterMap::styles()` cascade cache from the last
    /// [`Self::try_relayout_raf_incremental`] call that took the restyle-aware
    /// path (`layout_mutation_incremental_restyle`) — the `RestyleDelta::prev_styles`
    /// basis for the *next* such call. `None` whenever `layout_box` was set by
    /// any other producer (`relayout()`, tab switch, page load, hibernate
    /// restore, streaming layout, …), since a stale cache would silently derive
    /// the wrong dirty-root set against a `layout_box` it does not match —
    /// `try_relayout_raf_incremental` falls back to the existing
    /// full-cascade-plus-graft path (`layout_mutation_incremental`) whenever
    /// this is `None`.
    pub(crate) page_prev_cascade_styles: Option<lumen_layout::CascadeStyles>,
    /// Interactive state (`hovered_nid`/`focused_node`/`active_nid`) at the
    /// moment `page_prev_cascade_styles` was captured — the `prev` side of the
    /// next call's `restyle_root_set_for_state_change`. Only meaningful when
    /// `page_prev_cascade_styles` is `Some`.
    pub(crate) page_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// Последний вычисленный кадр анимаций. `None` — страница не загружена
    /// или нет активных анимаций.
    pub(crate) anim_frame: Option<lumen_layout::AnimationFrame>,
    /// Layout-дерево текущей страницы — нужен scheduler-у для обхода узлов
    /// и извлечения animation-longhands. Обновляется при load/reload/relayout.
    pub(crate) layout_box: Option<lumen_layout::LayoutBox>,
    /// P3-webvtt срез 3: WebVTT-cues текущей страницы (`<video>` → cues).
    pub(crate) page_tracks: tracks::PageTracks,
    /// CSS Scroll Snap L1 containers collected from `layout_box` after every
    /// layout update. Used by `start_smooth_scroll` / `scroll_x_by` to apply
    /// snap positions. Empty when `layout_box` is `None` or the page has no
    /// `scroll-snap-type` declarations. Cleared on navigation, recomputed on
    /// relayout / tab switch.
    pub(crate) snap_containers: Vec<SnapContainer>,
    /// Overflow scroll containers collected from `layout_box` after every layout
    /// update. Used by `MouseWheel` handler to route wheel events into the correct
    /// overflow container instead of always scrolling the page. Also used to fire
    /// `scroll` events after position changes. Cleared on navigation, recomputed on
    /// relayout / tab switch.
    pub(crate) scroll_containers: Vec<lumen_layout::ScrollContainer>,
    /// Эпоха для rAF-timestamp-ов в миллисекундах от старта shell-а
    /// (DOMHighResTimeStamp — HTML §8.1.5.1: «timestamp passed to callback
    /// should be the current high resolution time»).
    pub(crate) epoch: std::time::Instant,
    /// Timestamp (ms from `epoch`) of the last `requestAnimationFrame` batch fire.
    ///
    /// Used by the vsync gate: rAF callbacks fire at most once per `RAF_MIN_INTERVAL_MS`
    /// (~16.67 ms, 60 Hz). Initialized to `-RAF_MIN_INTERVAL_MS` so the first frame
    /// fires immediately.
    pub(crate) last_raf_batch_ms: f64,
    /// TEMP BUG-272 diagnostics: epoch seconds of the last memory report.
    pub(crate) last_mem_report_s: f64,
    /// Сессионный аккумулятор времён кадров (`LUMEN_FRAME_LOG`, M0.1 ADR-016).
    /// Наполняется только при включённом frame-log; сводка p50/p95/p99
    /// печатается по кадансу `LUMEN_MEM_REPORT` и один раз на выходе.
    pub(crate) frame_stats: lumen_paint::FrameStats,
    /// ADR-016 M2.0: сессионный аккумулятор времени `relayout()` на UI-потоке
    /// (стиль + layout + сборка display-list + доставка JS-observer'ов). Каждый
    /// интерактивный relayout (DOM-мутация из JS, hover/focus, resize, тик
    /// анимации, content-visibility) сегодня блокирует UI-поток — это и есть та
    /// работа, которую M2 уносит на отдельный engine-поток. Наполняется только
    /// при включённом `LUMEN_FRAME_LOG` (как `frame_stats`), сводка
    /// `ENGINE_SUMMARY` печатается по кадансу `LUMEN_MEM_REPORT` и один раз на
    /// выходе — даёт before/after числа, на которые сошлются следующие срезы M2.
    pub(crate) engine_stats: lumen_paint::FrameStats,
    /// ADR-016 M0.5: split fingerprint (content-hash + scroll/page offset) of the
    /// previously presented frame. Used only when `LUMEN_FRAME_LOG` is on: each
    /// frame is classified against it (`Identical`/`OffsetOnly`/`ContentChanged`)
    /// and the delta is logged, so the scroll-vs-content frame mix is measurable
    /// before M3 turns `OffsetOnly` into an actual blit fast path. `None` until
    /// the first logged frame.
    pub(crate) last_frame_fp: Option<lumen_paint::FrameFingerprint>,
    /// ADR-016 M3.2: retained scroll-band bookkeeping (the pure decision brain
    /// from M3.0/M3.1). Fed the layout content hash + scroll offset + viewport
    /// each frame to classify it as blit / blit+expose / repaint against the
    /// cached overscan band. Currently drives only the `LUMEN_FRAME_LOG`
    /// instrumentation (M3.2.0 — measure the real-content band mix before the GL
    /// blit path acts on it); the femtovg backend does not yet own a content
    /// surface, so normal runs pay nothing. Invalidated on navigation
    /// ([`Lumen::reset_to_blank_tab`]); resize/nav content changes also fall out
    /// naturally because the content hash folds surface size.
    pub(crate) scroll_cache: lumen_paint::ScrollCache,
    /// Состояние Ctrl+F. Открыт ли bar, текущий query и индекс активного
    /// совпадения. Содержимое поиска не сохраняется между reload-ами
    /// (close() полностью очищает state); это сознательно: после reload
    /// display list другой, и старые позиции совпадений уже невалидны.
    pub(crate) find: find::FindState,
    /// Состояние Ctrl+L адресной строки. Открыт ли бар и текущий ввод.
    /// Закрывается при навигации (commit) и при Esc.
    pub(crate) address_bar: address_bar::AddressBarState,
    /// Click-hint overlay: vimium-style kbd-навигация по кликабельным элементам.
    /// Открывается клавишей F; закрывается Escape, успешной активацией,
    /// открытием find/address bar или переходом на другую страницу.
    pub(crate) hint: hints::HintState,
    /// Текущее вертикальное смещение страницы (CSS px). 0 — верх документа.
    /// Растёт вниз, клампится в `[0, max(0, content_height − viewport_height)]`.
    /// На load/reload сбрасывается в 0.
    pub(crate) scroll_y: f32,
    /// Текущее горизонтальное смещение страницы (CSS px). 0 — левый край.
    /// Растёт вправо, клампится в `[0, max(0, content_width − viewport_width)]`.
    /// На load/reload сбрасывается в 0.
    pub(crate) scroll_x: f32,
    /// `scroll_y` предыдущего `RedrawRequested` — для оценки скорости скролла
    /// (fast-scroll деградация, EXPERIMENT.md §2 срез 2).
    pub(crate) last_frame_scroll_y: f32,
    /// EMA-скорость скролла в CSS px/кадр (сглаживает разовые wheel-рывки).
    pub(crate) scroll_velocity: f32,
    /// Режим быстрого скролла: тики CSS-анимаций/GIF/video-GIF заморожены,
    /// контент scroll-стабилен, кадры уходят в page-compose HIT.
    pub(crate) fast_scroll: bool,
    /// Полная высота контента в CSS px — `max(rect.y + rect.height)` по
    /// текущему display list-у. Обновляется после load/reload. 0 — нет контента.
    pub(crate) content_height: f32,
    /// Полная ширина контента в CSS px — `max(rect.x + rect.width)` по
    /// текущему display list-у. Обновляется после load/reload. 0 — нет контента.
    pub(crate) content_width: f32,
    /// CSS Containment L3 §4.4 (BB-4): `(node, top_y)` поддеревьев, пропущенных
    /// последним layout-проходом из-за `content-visibility: auto` вне расширенного
    /// viewport. top_y — страница-координаты (scroll 0) схлопнутого бокса.
    /// Обновляется в `refresh_cv_state` после каждой смены `layout_box`.
    pub(crate) cv_skipped: Vec<(NodeId, f32)>,
    /// Ratchet-набор auto-узлов, ставших relevant (вошли в расширенный viewport
    /// при скролле): прокидывается в layout через `set_cv_relevant`, такие узлы
    /// больше не пропускаются. Сбрасывается при загрузке страницы.
    pub(crate) cv_relevant: std::collections::HashSet<NodeId>,
    /// Skipped-состояние **каждого** `content-visibility: auto` узла прошлого
    /// прохода — база диффа (BUG-852). Отдельно от `cv_skipped`, который держит
    /// только пропущенные и только ради ratchet-а: «узла в карте нет» и «узел не
    /// пропущен» — разные вещи, и именно на первом держится событие первого
    /// наблюдения.
    pub(crate) cv_auto_state: std::collections::HashMap<NodeId, bool>,
    /// Очередь shell-событий `ContentVisibilityChange` — диффы skipped-состояния
    /// между layout-проходами. Дренируется раз в кадр в `RedrawRequested` и
    /// уходит в JS как `contentvisibilityautostatechange`. Кап 256 записей.
    pub(crate) cv_events: Vec<ContentVisibilityChange>,
    /// OS-level `prefers-color-scheme` preference. `true` — система в тёмной теме.
    /// Читается из winit `Window::theme()` при создании окна и обновляется на
    /// `WindowEvent::ThemeChanged`. Прокидывается в JS `matchMedia` через
    /// `deliver_media_query_changes(.., self.dark_mode)`. Default `false` (light)
    /// до создания окна и в headless/deterministic-режимах (стабильность snapshot-ов).
    pub(crate) dark_mode: bool,
    /// Per-tab user zoom factor (100% = 1.0). Changed via Ctrl+= / Ctrl+- / Ctrl+0.
    ///
    /// Combined with `<meta viewport initial-scale>` to compute the effective CSS
    /// layout viewport: `effective = physical / (meta_scale * zoom_factor)`.
    /// Resets to 1.0 on tab switch (stored in `PageSnapshot` for background tabs).
    pub(crate) zoom_factor: f32,
    /// Zoom factor the current display list was laid out at (ADR-016 M0.3).
    ///
    /// Transform-first zoom lets `zoom_factor` diverge from this between a
    /// Ctrl+/-/0 press and the debounced relayout; the backend previews the gap
    /// via `set_preview_scale(zoom_factor / laid_out_zoom_factor)`. Every
    /// `relayout()` re-syncs it to `zoom_factor` (the display list then matches
    /// the requested zoom, so no preview scale is needed).
    pub(crate) laid_out_zoom_factor: f32,
    /// Pending debounced relayout deadline for transform-first zoom (M0.3).
    ///
    /// Set on each Ctrl+/-/0 press to `now + ZOOM_RELAYOUT_DEBOUNCE_MS`; a fresh
    /// press pushes it later so a burst reflows only once. `about_to_wait`
    /// folds it into the `WaitUntil` deadline and runs `relayout()` when it
    /// elapses. `None` when no zoom preview is in flight.
    pub(crate) pending_zoom_relayout: Option<std::time::Instant>,
    /// Последняя известная позиция курсора в **physical** пикселях (от winit).
    /// `None` пока курсор не вошёл в окно. Конвертируется в CSS px через
    /// `scale_factor()` непосредственно в hit-test / drag callback-ах.
    pub(crate) cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Ph3 pointer-events-l3: CSS-pixel `(x, y)` samples from `CursorMoved`
    /// queued since the last flush, in chronological order. Pointer Events
    /// Level 3 §4.1 "coalesced events" — multiple raw OS samples can arrive
    /// before the next paint; `flush_pointer_moves` turns the whole batch
    /// into one `pointermove` dispatch with the rest exposed via
    /// `getCoalescedEvents()`. Flushed once per `about_to_wait` tick, or
    /// earlier — right before a hover-boundary crossing or `pointerdown`/
    /// `pointerup` — so event order stays chronological.
    pub(crate) pending_pointer_moves: Vec<(f32, f32)>,
    /// DOM node currently under the mouse pointer (CSS `:hover` target).
    /// Updated on every `CursorMoved`; triggers relayout when it changes so
    /// `:hover` rules re-evaluate. `None` when cursor is outside the content area.
    pub(crate) hovered_nid: Option<NodeId>,
    /// `(индекс фрейма, узел ЕГО документа)` под курсором — когда курсор стоит
    /// над содержимым `<iframe>` (BUG-480 срез 16).
    ///
    /// Пара с [`Self::hovered_nid`], а не часть его: `NodeId` уникален лишь
    /// внутри своего документа, поэтому узел ребёнка нельзя положить в поле,
    /// которое читают `:hover`-рестайл страницы и её же `mousedown`/`mouseup`
    /// — они нашли бы чужой бокс с совпавшим индексом. Ровно одно из двух
    /// полей непусто: пока курсор во фрейме, страница считает, что курсор не
    /// над её содержимым.
    pub(crate) hovered_frame: Option<(usize, NodeId)>,
    /// DOM node whose mouse button is currently held down (CSS `:active` target).
    /// Set on `MouseInput(Pressed)`, cleared on `MouseInput(Released)`.
    pub(crate) active_nid: Option<NodeId>,
    /// Активный drag scrollbar-thumb-а: `Some` пока зажата левая кнопка после
    /// click-а по thumb-у. `MouseInput Released` или `CursorLeft` сбрасывают
    /// в `None`. Снапшот `(start_scroll_y, start_mouse_y)` фиксирован на момент
    /// начала drag-а — это даёт «закреплённый под пальцем» thumb (стандартный
    /// scrollbar UX).
    pub(crate) scroll_drag: Option<scrollbar::ScrollDrag>,
    /// Активный drag СОБСТВЕННОГО scrollbar-thumb-а фрейма (FRAME-3
    /// remainder) — зеркало [`Self::scroll_drag`], но должен помнить ЕЩЁ и
    /// КАКОЙ фрейм: несколько фреймов на странице держат независимые
    /// scroll-полосы. `MouseInput Released` сбрасывает в `None`, как и
    /// навигация (frames пересобирается целиком — старый индекс адресовал
    /// бы чужой или отсутствующий хэндл).
    pub(crate) frame_scroll_drag: Option<(usize, scrollbar::ScrollDrag)>,
    /// Активная smooth-scroll анимация для keyboard / wheel / page-jump /
    /// find-scroll-to-match. `None` — `scroll_y` стационарен или меняется
    /// инстантно (drag, reload). При live-анимации `RedrawRequested` тикает
    /// её через `advance_scroll_anim` и просит ещё один redraw до завершения.
    pub(crate) scroll_anim: Option<scroll_anim::ScrollAnim>,
    /// Momentum (kinetic) scroll: запускается при `TouchPhase::Ended` с
    /// ненулевой скоростью от тачпада. Тикается через `advance_momentum`
    /// в `RedrawRequested`. `None` — нет активной инерции.
    pub(crate) momentum_anim: Option<momentum_anim::MomentumAnim>,
    /// Мгновенная скорость тачпада от последних `PixelDelta`-событий
    /// (CSS px / ms). Обновляется EWMA-фильтром. Используется при
    /// `TouchPhase::Ended` для запуска `momentum_anim`.
    pub(crate) touchpad_vel: (f32, f32),
    /// Timestamp последнего `PixelDelta`-события для расчёта dt в EWMA.
    pub(crate) touchpad_vel_time_ms: f64,
    /// Последний выставленный cursor icon — чтобы при каждом CursorMoved (а это
    /// сотни событий в секунду при активном движении мыши) не дёргать
    /// `Window::set_cursor` напрасно. `None` — ещё не выставляли (init).
    pub(crate) last_cursor_icon: Option<CursorIcon>,
    /// DOM + stylesheet для relayout без повторного fetch/parse. Обновляется
    /// при каждом load/reload. `None` — страница не загружена (Empty source).
    pub(crate) layout_source: Option<LayoutSource>,
    /// Флаг «нужно reload после текущего about_to_wait». Устанавливается
    /// closure-ом внутри queue_task — это единственный способ сообщить
    /// Lumen-у из task-closure (которая `+ 'static` и не владеет `&mut self`).
    pub(crate) pending_reload: Rc<Cell<bool>>,
    /// Навигационный запрос от JS (location.href=, assign, replace, reload),
    /// захваченный во время выполнения скриптов страницы. Обрабатывается
    /// в `about_to_wait` после первого рендера загруженной страницы.
    pub(crate) pending_js_navigate: Option<JsNavigateRequest>,
    /// Proxy для отправки LoadEvent из background-потока загрузки в event loop.
    pub(crate) load_proxy: EventLoopProxy<LoadEvent>,
    /// Инкрементальный HTML-парсер — активен во время streaming load.
    /// `None` до первого HtmlChunk или после LoadDone/LoadError.
    pub(crate) stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    /// Момент последнего промежуточного кадра при streaming — для throttling.
    pub(crate) stream_last_paint: std::time::Instant,
    /// CSS-таблица из параллельных потоков загрузки CSS (PH1-2). Применяется
    /// в `paint_partial_dom` вместо пустой таблицы. Сбрасывается на каждый
    /// новый страничный load.
    pub(crate) stream_sheet: lumen_css_parser::Stylesheet,
    /// PH1-2b: `true` когда `layout_box` содержит дерево, построенное из текущего
    /// streaming-DOM (валидный источник для инкрементального graft). `false` в
    /// начале новой навигации — первый промежуточный кадр делает полный layout и
    /// «засевает» дерево; последующие кадры релейаутят инкрементально.
    pub(crate) stream_layout_seeded: bool,
    /// URL subresource-хинтов, уже отправленных в sink во время streaming
    /// (`EarlyPreloadHints`). Финальный `dispatch_preload_hints` в `LoadDone`
    /// пропускает URL из этого набора — без дублей в stderr и без повторных
    /// fetch-триггеров при реальном параллельном prefetch. Очищается в начале
    /// каждого нового страничного load.
    pub(crate) preload_dispatched: std::collections::HashSet<String>,
    /// PH1-2c: ключи `src` картинок, уже отправленных в background-потоки
    /// декодирования во время текущего streaming-load. Дедуп между
    /// промежуточными кадрами `paint_partial_dom`, чтобы каждый `<img>`
    /// загружался один раз. Очищается в начале каждой навигации.
    pub(crate) stream_images_requested: std::collections::HashSet<String>,
    /// BUG-735: intrinsic-размеры `src` → `(width, height)` всех картинок,
    /// декодированных streaming/динамическим путём в текущей навигации.
    /// Карта живёт до конца навигации (а не дренируется за проход), потому что
    /// `stream_images_requested` дедуплицирует запрос по URL: узел с тем же
    /// `src`, добавленный скриптом позже, своего `ImageDecoded` уже не получит,
    /// и размер ему может дать только эта карта.
    pub(crate) stream_image_sizes: HashMap<String, (u32, u32)>,
    /// BUG-735: в карту [`Self::stream_image_sizes`] попал новый размер —
    /// на ближайшем кадре нужно разнести его по `<img>` и, если DOM изменился,
    /// сделать релейаут. Флаг коалесцирует пачку декодов (сотня картинок = один
    /// проход, а не сотня релейаутов).
    pub(crate) stream_image_sizes_dirty: bool,
    /// U-1: scroll offset to restore once the in-flight navigation completes.
    /// Set by back/forward navigation before kicking off an async (streaming)
    /// reload; consumed in `apply_loaded_page` (and the sync fallback in
    /// `reload`) after the page resets scroll to the top. `None` for ordinary
    /// navigations (they stay at 0,0). Needed because navigation is no longer
    /// synchronous — the old code set `scroll_x/y` right after `reload()`
    /// returned, but the scroll reset now happens later, at `LoadEvent::LoadDone`.
    pub(crate) pending_restore_scroll: Option<(f32, f32)>,
    /// Bfcache (HTML LS §8.6): `.persisted` flag for the `pageshow` event fired
    /// after the next page load completes. Set `true` by `navigate_back`/
    /// `navigate_forward` when the destination is restored from bfcache,
    /// consumed (and reset to `false`) in `apply_loaded_page` right after
    /// `notify_window_loaded`. `false` for ordinary fresh loads.
    pub(crate) pending_pageshow_persisted: bool,
    /// Same-document (`pushState`) state JSON + display URL to apply once an
    /// in-flight reload completes. Set by `navigate_back`/`navigate_forward`
    /// when a multi-step `history.go(n)` traversal (`navigate_by`) silently
    /// shuttled through a full-document entry before landing on a
    /// same-document entry — the currently loaded document is not the one
    /// that entry belongs to, so `popstate`/the URL update must wait for the
    /// correct document to actually finish loading. `None` for the
    /// overwhelmingly common case (destination belongs to the already-loaded
    /// document); consumed in `apply_loaded_page`.
    pub(crate) pending_post_reload_traversal: Option<(String, Option<String>)>,
    /// Set by `navigate_by` immediately before calling `navigate_back`/
    /// `navigate_forward` when the multi-step shuffle passed through a
    /// full-document entry en route to the destination. Consumed (reset to
    /// `false`) at the top of both functions; direct callers (single-step
    /// Alt+Left/Right, not routed through `navigate_by`) always see `false`,
    /// matching their existing single-hop behavior.
    pub(crate) traversal_crossed_document: bool,
    /// U-1: monotonic navigation generation. Bumped on every async navigation
    /// (`reload` when a window exists) and on the initial streaming load. Each
    /// streaming `LoadEvent` carries the generation it was spawned under;
    /// `user_event` drops events whose generation is stale (a superseded
    /// navigation), so a slow earlier load can't paint over a newer page.
    pub(crate) load_generation: u64,
    /// BUG-757: реальная база текущего документа и generation навигации, в
    /// которой она получена. Заполняется, когда сервер увёл запрос редиректом:
    /// `self.source` хранит ЗАПРОШЕННЫЙ адрес, и подресурсы частичного DOM
    /// (картинки, `@font-face`) уходили бы от него. Пара с generation вместо
    /// сброса на каждой навигации — устаревшая база просто перестаёт
    /// подходить (см. [`Self::document_resource_base`]).
    pub(crate) document_base: Option<(ResourceBase, u64)>,
    /// ADR-016 M2.2: долгоживущий движковый поток. `Some` только при
    /// `LUMEN_ENGINE_THREAD=1`; иначе `None` и поведение shell неизменно (весь
    /// relayout синхронный). Через него маршрутизируется off-thread layout
    /// async-триггеров (пока — debounce-зум): `submit_relayout_job` шлёт задание,
    /// `poll_engine_commit` забирает готовый [`EngineCommit`]. Дроп при завершении
    /// шлёт `Shutdown` и джойнит.
    ///
    /// ADR-016 M2.2c-2b: поток также владеет персистентным состоянием
    /// [`EngineJsState`] (`Document` + хэндл `js_ctx`) — сиденье для переноса JS на
    /// движковый поток. Заполняется через `sync_engine_js_state` при смене страницы.
    pub(crate) engine_thread: Option<engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    /// ADR-016 M2.2: generation последнего **применённого** relayout-результата.
    /// Off-thread задание считается «в полёте» (нужен poll-будильник), пока
    /// `engine_job_generation != engine_applied_generation`. Синхронный
    /// `relayout()` выставляет их равными (off-thread задание не ждётся);
    /// `poll_engine_commit` продвигает это поле применённым `commit.generation`.
    pub(crate) engine_applied_generation: u64,
    /// ADR-016 M2.2: монотонный номер async-relayout задания. Растёт при каждой
    /// постановке off-thread задания (`submit_relayout_job`) **и** при каждом
    /// синхронном `relayout()` — так результат уже поставленного, но ещё не
    /// применённого off-thread задания опознаётся как устаревший
    /// (`commit.generation != engine_job_generation`) и роняется в
    /// `poll_engine_commit`. Latest-wins/generation-guard на стороне потока —
    /// [`engine_thread`].
    pub(crate) engine_job_generation: u64,
    /// Текущий IME preedit-текст. `Some` — composition-сессия активна,
    /// `None` — нет активного IME ввода.
    pub(crate) ime_composing: Option<String>,
    /// In-memory bfcache — HTML snapshots keyed by URL for instant back/forward
    /// restoration without a network round-trip (HTML Living Standard §8.6).
    pub(crate) bfcache: BfCache,
    /// Parsed stylesheets of frozen bfcache pages, keyed by URL.
    /// Kept shell-side because `Stylesheet` is not serializable.
    /// Pruned lazily against `bfcache.has_frozen`.
    pub(crate) frozen_styles: HashMap<String, lumen_css_parser::Stylesheet>,
    /// Pages kept alive (JS runtime included) for back/forward restoration,
    /// keyed by URL — see [`ParkedPage`]. Capped at [`PARKED_PAGES_MAX`];
    /// a `Vec` rather than a map because eviction is oldest-first.
    pub(crate) parked_pages: Vec<(String, ParkedPage)>,
    /// Navigation history stack — pages the user navigated away from.
    /// Top = most recent previous page.
    pub(crate) nav_back: Vec<NavEntry>,
    /// Forward history stack — pages the user went back from.
    /// Top = most recently visited "forward" page.
    pub(crate) nav_fwd: Vec<NavEntry>,
    /// Monotonic counter for Navigation API entry keys.
    /// Incremented on each new entry so `key` is unique across the session.
    pub(crate) nav_key_counter: u64,
    /// Key of the page currently displayed. Assigned when the page becomes
    /// current; preserved across back/forward navigation so `commit_nav_state`
    /// emits a stable key for the current entry (BUG-256 uniqueness invariant).
    pub(crate) current_nav_key: String,
    /// Pending intercepted navigation awaiting handler completion.
    pub(crate) pending_intercepted: Option<PendingIntercepted>,
    /// Runtime form control state (value, checked) keyed by NodeId.
    /// Persists for the lifetime of the current page; cleared on load/reload.
    pub(crate) form_state: forms::FormState,
    /// Active validation tooltip: (anchor_rect_in_doc_space, message).
    /// Displayed as a viewport-locked overlay. Dismissed on next click.
    pub(crate) validation_tooltip: Option<(Rect, String)>,
    /// NodeId of the `<input type="color">` whose picker is currently open.
    /// The picker overlay is viewport-locked; clicking a swatch closes it.
    pub(crate) color_picker_node: Option<NodeId>,
    /// NodeId of the `<input type="date/datetime-local/time/month/week">` whose
    /// calendar picker overlay is open. `None` when no picker is visible.
    pub(crate) date_picker_node: Option<NodeId>,
    /// Calendar year currently displayed in the open date picker (1-based).
    pub(crate) date_picker_year: i32,
    /// Calendar month currently displayed in the open date picker (1-based, 1=January).
    pub(crate) date_picker_month: u8,
    /// NodeId of the `<select>` whose dropdown is currently open.
    /// The dropdown overlay is viewport-locked; clicking an option closes it.
    pub(crate) select_dropdown_node: Option<NodeId>,
    /// Persistent `localStorage` partitions keyed by origin (scheme+host+port).
    /// Each entry survives page reloads within the same session.
    /// Partitioned by origin to enforce Same-Origin Policy for storage access.
    pub(crate) ls_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// `sessionStorage` partitions of the *active tab*, keyed by the same origin
    /// string as [`Self::ls_storage`] (BUG-836).
    ///
    /// HTML LS §12.2 binds session storage to the browsing context: it must
    /// survive every navigation of this tab and never reach another one, so the
    /// map travels in the tab snapshot and is emptied for a newly opened tab —
    /// unlike `ls_storage`, nothing here is ever persisted.
    pub(crate) ss_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// Directory for per-origin IndexedDB SQLite files (`{sha256(eTLD+1)[:16]}.db`).
    /// `None` → ephemeral in-memory store per page (headless / tests).
    /// `Some(dir)` → each origin gets its own SQLite file in `dir`; data persists
    /// across page reloads and is shared across tabs of the same origin.
    pub(crate) idb_dir: Option<std::path::PathBuf>,
    /// Shared backend for Service Worker registration persistence. A per-origin
    /// `SwStore` is built over this for each page load so SW registrations survive
    /// page navigations within the session (same pattern as `idb_backend`).
    pub(crate) sw_backend: Arc<std::sync::Mutex<dyn lumen_core::ext::StorageBackend>>,
    /// Live SW execution thread registry (PH3-20: SW fetch interception).
    ///
    /// Shared between `QuickJsRuntime` (populates via `_lumen_sw_activate_script`)
    /// and `ServiceWorkerInterceptor` (reads when routing network fetch requests).
    pub(crate) sw_worker_store: lumen_core::ext::SwWorkerStore,
    /// Session-scoped Cache API store (PH3-20). Shared between the page's `caches`
    /// API and activating SW execution threads: the SW serves cache-first
    /// responses from entries the page previously cached into this store. Also the
    /// fallback cache consulted by `ServiceWorkerInterceptor`. In-memory SQLite.
    pub(crate) cache_store: Arc<lumen_storage::CacheStorage>,
    /// Session-scoped cookie jar. Shared across all `HttpClient` instances so
    /// `Set-Cookie` headers received on one hop (including 3xx redirects) are
    /// sent back on subsequent requests to the same domain. In-memory in Phase 0;
    /// wired to a per-profile SQLite file in Phase 2. Used for every profile
    /// except the ephemeral Anonymous one — see [`Self::anonymous_cookie_jar`]
    /// and [`Self::active_cookie_jar`] (DS-16).
    pub(crate) cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Anonymous profile's own cookie jar (DS-16, §9.3 ADR-020) — kept out of
    /// [`Self::cookie_jar`] so cookies set while browsing as Anonymous never
    /// leak into Personal/Work/Guest and vice versa. Reset to a fresh
    /// in-memory instance every time Anonymous becomes the active profile
    /// (`ProfileMenuHit::SwitchTo`), so it never carries state from a
    /// previous Anonymous session either — true ephemerality within the
    /// running process, not just isolation.
    pub(crate) anonymous_cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Live JS context for the current page — keeps event listeners active after
    /// initial script execution. `None` when `v8` feature is disabled or
    /// no scripts were registered. Must be dropped before `layout_source` on
    /// navigation to release Arc clones held in JS closures.
    ///
    /// ADR-016 M2.2c-2d (21): `Arc` (не `Box`), потому что хэндлом теперь владеет
    /// **либо** UI-сторона, **либо** движковый поток. Под флагом
    /// (`LUMEN_ENGINE_THREAD=1`) `Arc` живёт в [`EngineJsState::js`], а это поле —
    /// `None`; без флага (по умолчанию) `Arc` здесь, как прежде. Владение задаёт
    /// [`Self::set_js_ctx`], снимает — [`Self::take_js_ctx`]. «Есть ли JS?» читайте
    /// из [`Self::js_present`], а не из `self.js_ctx.is_some()`.
    pub(crate) js_ctx: Option<Arc<dyn PersistentJs>>,
    /// ADR-016 M2.2c-2d: UI-сторонний флаг «активная вкладка имеет JS-рантайм»,
    /// сопровождающий каждое присваивание хэндла (через [`Self::set_js_ctx`] и
    /// snapshot save/restore). Отделяет решение «есть ли JS?» от того, какая
    /// сторона держит `Arc`: гейты (`if self.js_present`) читают его вместо
    /// `self.js_ctx.is_some()`, поэтому остаются верны и когда под флагом сам `Arc`
    /// уехал на движковый поток (`state.js`), оставив `self.js_ctx == None`.
    pub(crate) js_present: bool,
    /// ADR-016 M2.3: UI-side lock-free clone of the JS runtime's rAF-pending
    /// flag (`Some` only when the active tab has a `v8` handle **and** the
    /// engine thread is enabled — the only mode that needs it). Read directly on
    /// the UI thread to schedule rAF turns without a blocking engine `query`
    /// that would serialize the winit thread behind an in-flight JS turn.
    /// Kept in lockstep with the handle by [`Self::set_js_ctx`]; `None` off the
    /// flag, so the byte-identical single-thread path never consults it.
    pub(crate) raf_pending_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// ADR-016 M2.3: UI-side lock-free clone of the JS runtime's DOM-dirty flag
    /// (companion to [`Self::raf_pending_flag`]). Consumed on the UI thread to
    /// trigger an asynchronous relayout after an off-thread rAF turn mutated the
    /// DOM, instead of a synchronous read blocked behind that turn.
    pub(crate) dom_dirty_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// ADR-016 M2.3: `true` while a `run_animation_frame` batch dispatched to the
    /// engine thread is still executing. Set by the UI thread before firing the
    /// (fire-and-forget) rAF `task`, cleared by that task on completion. Guards
    /// against piling a fresh 200 ms rAF turn onto the engine FIFO every 16 ms
    /// scroll frame: while set, the scroll/redraw path presents the retained
    /// display list and skips the JS pump, keeping the UI thread responsive.
    /// Only ever set under `LUMEN_ENGINE_THREAD=1`; stays `false` off the flag.
    pub(crate) raf_task_inflight: Arc<std::sync::atomic::AtomicBool>,
    /// ADR-016 M2.3: reserve one `about_to_wait` pass for draining the deferred
    /// value-returning JS queues after each rAF turn completes, before firing the
    /// next one. Set when a turn is fired, consumed on the first non-inflight pass
    /// afterwards (which then runs the [`Self::drain_query_js`] drains with the
    /// engine free). Without it a continuous rAF loop would re-fire every pass and
    /// permanently starve notifications/popups/console. UI-thread only, flag-on
    /// only (stays `false` off the flag).
    pub(crate) raf_drain_gate: bool,
    /// When true the vertical scrollbar overlay is suppressed entirely.
    /// Set by `--no-scrollbar` CLI flag; used by graphic test pipeline to
    /// avoid scrollbar pixels contaminating the diff against Edge headless.
    pub(crate) no_scrollbar: bool,
    /// When true the window is created maximized (`--maximized` CLI flag;
    /// live perf audit runs full-screen so the user can watch rendering).
    pub(crate) maximized: bool,
    /// Guards for PerformancePaintTiming entries (W3C Paint Timing §2).
    /// `true` once the entry has been delivered to JS so we don't double-fire.
    pub(crate) first_paint_delivered: bool,
    /// `true` once `first-contentful-paint` has been delivered to JS.
    pub(crate) first_contentful_paint_delivered: bool,
    /// `true` when the current navigation finished in a network/HTTP error
    /// (`LoadError` / final-render `Err`) rather than a loaded document. A
    /// settled error IS "done loading": `check_wait_condition` treats it as
    /// `DocumentReady` so a `wait{document_ready}` (MCP/BiDi) resolves at once
    /// instead of hanging until its deadline when there is no JS context and no
    /// prior `layout_box` to fall back on (BUG-308). Reset to `false` at the
    /// start of every navigation; per-tab (saved/restored via `PageSnapshot`).
    pub(crate) load_failed: bool,
    /// Human-readable reason for `load_failed` (BUG-438) — the `LoadError`
    /// message or the final-render `Err`'s `Display`. Surfaced to
    /// `AutomationCommand::Wait{DocumentReady|NetworkIdle}` callers (BiDi
    /// `browsingContext.navigate`, MCP `wait`) as an `AutomationReply::Error`
    /// instead of the settled-error `Ack` BUG-308 used to send — a failed
    /// load must not be reported as a successful navigation. `None` whenever
    /// `load_failed` is `false`; reset together with it.
    pub(crate) load_error_message: Option<String>,
    /// Instant at which the current navigation began (set in `reload()`).
    /// Used to compute `duration` for the W3C Navigation Timing entry.
    pub(crate) nav_start: Option<std::time::Instant>,
    /// FTS5-индекс по тексту посещённых страниц — используется omnibox (@history).
    /// In-memory в Phase 0; в Phase 2 открывается из профильной БД.
    pub(crate) history_fts: HistoryFts,
    /// Хранилище пользовательских заметок (§12.2) — omnibox `@notes <query>`.
    /// In-memory в Phase 0; в Phase 2 открывается из профильной БД.
    pub(crate) notes_store: lumen_knowledge::Notes,
    /// История поисковых запросов для prefix-match autocomplete в omnibox.
    /// In-memory в Phase 0; в Phase 2 открывается из профильной БД.
    pub(crate) search_history: SearchHistory,
    /// Счётчик для генерирования rowid при индексировании в history_fts.
    /// Инкрементируется при каждой навигации на новую страницу.
    pub(crate) next_history_id: i64,
    /// Knuth–Liang hyphenation provider — реализует CSS `hyphens: auto`.
    /// Lazy-loads per-locale dictionaries on first use; cached for subsequent layouts.
    /// `Arc`, чтобы финальный pipeline (BUG-171 этап 2) мог разделить провайдер с
    /// фоновым рендер-потоком без потери прогретого кэша словарей.
    pub(crate) hyp_provider: Arc<KnuthLiangHyphenation>,
    /// Multi-frame GIF animations keyed by the same src URL used in `DrawImage`.
    /// Populated at image-load time; cleared on page navigation.
    /// Single-frame GIFs are not stored here — handled as regular static images.
    pub(crate) animated_gifs: HashMap<String, lumen_image::AnimatedGif>,
    /// Last rendered frame index per animated GIF URL. Avoids redundant GPU texture
    /// re-uploads when `frame_index_at(elapsed_ms)` returns the same frame as the
    /// previous tick. Cleared together with `animated_gifs` on navigation.
    pub(crate) gif_last_frame: HashMap<String, usize>,
    /// Last rendered frame index per GIF-backed `<video>` node (keyed by nid).
    /// Cleared together with the VideoGifStore entries on navigation.
    pub(crate) video_gif_last_frame: HashMap<u32, usize>,
    /// Decoded animated GIF frames for `<video>` nodes (keyed by nid).
    /// Stored separately from `VideoGifStore` (which has no `lumen_image` dep).
    pub(crate) video_gif_frames: HashMap<u32, lumen_image::AnimatedGif>,
    /// BUG-480 срез 1: живые sub-документы `<iframe>` текущей страницы.
    /// Держат DOM+JS детей; заменяется целиком в [`Lumen::apply_loaded_page`].
    /// В PageSnapshot не попадает — после bfcache-восстановления фреймы без
    /// скриптов (известное ограничение среза 1, см. bugs/BUG-480-OPEN.md).
    pub(crate) frames: Vec<FrameHandle>,
    /// BUG-480 срез 19: провайдеры, которыми загружены фреймы ТЕКУЩЕЙ
    /// страницы, — ими же грузит под-документ навигация фрейма.
    ///
    /// Едет из `LoadedPage` вместе с самими фреймами и заменяется в том же
    /// месте: набор принадлежит документу (его origin, его хранилища, его
    /// база), поэтому пережить смену страницы не может.
    pub(crate) frame_env: Option<crate::frames::FrameLoadEnv>,
    /// FRAME-4 срез 3: generation-слоты навигаций фреймов текущей страницы,
    /// ещё не завершившихся или уже применённых (см.
    /// `crate::frames::FrameNavRequest`) — по одному на живой `(host_doc,
    /// host)`, куда сверяется ответ фонового потока
    /// [`Lumen::on_frame_nav_done`]. Заменяется вместе с [`Self::frames`]:
    /// `host_doc` внутри слота адресует документ, которого больше не будет.
    pub(crate) frame_nav_requests: Vec<crate::frames::FrameNavRequest>,
    /// Shared GIF-video store — same Arc used by JS native bindings (PH3-12).
    ///
    /// The shell owns the Arc; JS bindings hold clones captured at context
    /// creation time.  The shell's render tick drains `pending_loads`, decodes
    /// GIFs, and re-registers frames under `"video:{nid}"` image keys.
    pub(crate) video_gif_store: std::sync::Arc<lumen_js::VideoGifStore>,
    /// Shared TextTrack store — same Arc used by JS native bindings (P3-webvtt).
    ///
    /// Mirrors `page_tracks.tracks_by_video` so `video.textTracks` exposes the
    /// shell's parsed `<track>` cues. Re-synced on load, cleared on navigation.
    pub(crate) text_track_store: std::sync::Arc<lumen_js::TextTrackStore>,
    /// CPU-side decoded image cache (ADR-008 §10E.4 scroll-discard).
    ///
    /// Stores one `ImageHandle` per image URL so far-away images can be evicted
    /// from RAM on scroll without discarding the GPU texture in the renderer.
    /// Cleared and repopulated on every page load; entries are dropped by
    /// `try_discard_offscreen_images` once an image leaves the
    /// `gate_image_requests` zone (viewport ± 2 screens).
    pub(crate) image_cache: lumen_image::ImageDecodeCache,
    /// Receiver side of the automation command channel (SDC-1b/SDC-2).
    ///
    /// Connected to an external sender for BiDi/MCP/graphic_tests control.
    /// Each request carries its own reply sender (see [`AutomationRequest`]),
    /// so replies reach the specific caller that issued the command instead
    /// of a shared, unread channel. Commands are drained in `about_to_wait`.
    pub(crate) automation_rx: std::sync::mpsc::Receiver<AutomationRequest>,
    /// Sender side of the automation command channel - cloned for external callers.
    #[allow(dead_code)]
    pub(crate) automation_cmd_tx: std::sync::mpsc::Sender<AutomationRequest>,
    /// `AutomationCommand::Wait` requests not yet satisfied (SDC-1b).
    ///
    /// The event loop cannot block on `Wait` (that would freeze rendering and
    /// starve the very state — network completions, JS ticks — the condition
    /// depends on), so a wait is queued here and re-checked once per frame in
    /// `about_to_wait` until it is satisfied or its deadline passes.
    pub(crate) pending_waits: Vec<PendingWait>,
    /// Receiver side of the input injection channel (ADR-007 §8C).
    ///
    /// Drained each `about_to_wait`; commands are processed through the same
    /// hit-test / JS-dispatch path as real OS events.
    pub(crate) input_rx: input::InputReceiver,
    /// Sender side of the input injection channel — cloned for external callers.
    #[allow(dead_code)]
    pub(crate) input_tx: input::InputSender,
    /// The DOM node that received the last click (used as target for TypeText injection).
    ///
    /// `None` until the first click is processed.  Updated by `handle_click_at`.
    pub(crate) focused_node: Option<lumen_dom::NodeId>,
    /// `(индекс фрейма, узел ЕГО документа)`, если последний клик пришёлся
    /// ВНУТРЬ содержимого фрейма (BUG-480 срезы 22/23).
    ///
    /// Пара с [`Self::focused_node`], той же формы, что [`Self::hovered_frame`]
    /// у [`Self::hovered_nid`]: `NodeId` уникален лишь внутри своего документа.
    /// `focused_node` в этот момент указывает на host-элемент `<iframe>` (клик
    /// внутрь фрейма фокусирует контейнер с точки зрения страницы, срез 16) —
    /// это поле не заменяет его, а адресует фокус ВНУТРИ под-документа.
    ///
    /// Срез 22 записывал сюда только TYPEABLE-поле, потому что читал это поле
    /// один ввод текста. Срез 23 хранит ЛЮБОЙ узел под точкой клика — ровно
    /// как `focused_node` у страницы, которому тоже всё равно, что за узел, —
    /// иначе `:focus` внутри фрейма работал бы на полях и не работал на всём
    /// остальном, то есть расходился бы со страницей. Ввод текста от этого не
    /// пострадал: обе его точки (`keyboard.rs`, `about_to_wait.rs`)
    /// перепроверяют typeable-ность на месте использования.
    ///
    /// `None`, если последний клик не был по содержимому фрейма.
    pub(crate) focused_frame: Option<(usize, NodeId)>,
    /// Char-index text cursor for a typeable field ВНУТРИ содержимого фрейма
    /// (FRAME-2 п.1), keyed by `(индекс фрейма, узел ЕГО документа)` — the
    /// same pairing [`Self::focused_frame`] uses, needed for the same reason:
    /// `NodeId` is unique only within its own document. The page-side
    /// counterpart lives on `FormControlState::cursor` (page controls already
    /// have a per-`NodeId` state map in [`Self::form_state`]; a frame's
    /// sub-document does not, so this is a standalone map rather than a new
    /// field threaded through one). Cleared alongside `focused_frame`'s
    /// document on navigation ([`crate::page_load`]) and on tab reset
    /// ([`crate::lumen::tabs_cmd`]).
    pub(crate) frame_text_cursor: HashMap<(usize, NodeId), usize>,
    /// Char-index selection anchor for a typeable field ВНУТРИ содержимого
    /// фрейма (FRAME-7 remainder 2) — mirror of `FormControlState::selection_anchor`
    /// (page), keyed the same way [`Self::frame_text_cursor`] is (a frame's
    /// sub-document has no per-`NodeId` state map of its own). Cleared
    /// alongside `frame_text_cursor` on navigation/tab reset — see that
    /// field's doc comment.
    pub(crate) frame_text_selection_anchor: HashMap<(usize, NodeId), usize>,
    /// Mouse-drag text selection in progress (FRAME-7 остаток) — armed by
    /// [`super::text_drag_select::Lumen::begin_text_drag_select`] right after
    /// a left-button press lands on a typeable field's focus, updated by
    /// [`super::text_drag_select::Lumen::update_text_drag_select`] on every
    /// `CursorMoved` while held, and disarmed on `ElementState::Released`
    /// (`mouse_input.rs`) — the button-release path every other drag field on
    /// this struct (`scroll_drag`, `panel_resize`, `dnd_state`) already
    /// follows.
    pub(crate) text_drag: Option<super::text_drag_select::TextDragTarget>,
    /// `(индекс фрейма, узел ЕГО документа)` под НАЖАТОЙ кнопкой мыши внутри
    /// содержимого фрейма — `:active` под-документа (BUG-480 срез 23).
    ///
    /// Пара с [`Self::active_nid`] ровно так же, как [`Self::focused_frame`] —
    /// с [`Self::focused_node`]. Ставится на press, снимается на release, из
    /// тех же двух веток `mouse_input.rs`, что уже адресуют `hovered_frame`.
    pub(crate) active_frame: Option<(usize, NodeId)>,
    /// Download manager: background download threads, progress channel, and
    /// panel visibility state. Panel toggled via Ctrl+Shift+J.
    pub(crate) downloads: download::DownloadManager,
    /// Tab strip state: open tabs (title, id) and active index.
    ///
    /// The ACTIVE tab's page state lives directly in the `Lumen` fields.
    /// Background tabs have their page state in `bg_tabs` keyed by `TabEntry::id`.
    pub(crate) tab_strip: tabs::strip::TabStrip,
    /// Per-`(origin, ContainerKind)` cookie/storage store ids (7D.2).
    ///
    /// Allocated lazily on first access; the actual cookie jar / storage
    /// dispatch picks up the store id as a partitioning key. Stored on the
    /// shell so isolation survives tab open/close/restore.
    pub(crate) container_store: tabs::containers::ContainerStore,
    /// Frozen page state for each background tab, keyed by `TabEntry::id`.
    ///
    /// `None` entry means the tab was opened but never loaded (blank new tab).
    pub(crate) bg_tabs: HashMap<usize, PageSnapshot>,
    /// Lightweight identity for hibernated (T3) tabs — keyed by `TabEntry::id`.
    ///
    /// When a background tab is promoted to Hibernated its full `PageSnapshot`
    /// is evicted from `bg_tabs` and stored in `tab_snapshots`; only this
    /// cheap struct remains in RAM.
    pub(crate) hibernated_tabs: HashMap<usize, tab_lifecycle::TabMetadata>,
    /// SQLite-backed blob store for T3 DOM snapshots (ADR-008 §10J).
    pub(crate) tab_snapshots: lumen_storage::TabSnapshotStore,
    /// SQLite-backed checkpoint store for T2 (BackgroundOld) tabs (ADR-008 §10I).
    ///
    /// Written on every T1→T2 transition so scroll + form state survive a crash.
    /// Restored on T2→T0 when `bg_tabs` is empty (crash-recovery path).
    pub(crate) t2_store: lumen_storage::SleepingTabStore,
    /// Monotonic timestamp (ms since epoch) when a T2 SQLite restore started.
    ///
    /// `None` when no restore is in progress.  The `sleep_hint` overlay is shown
    /// once this exceeds 100 ms.
    pub(crate) t2_restore_start_ms: Option<f64>,
    /// SQLite-backed store for the last session — all open tabs at window close
    /// (§10I). Overwritten wholesale on `CloseRequested`, read back on launch to
    /// reopen the previous set of tabs. On-disk at `session_persist::SESSION_DB_PATH`.
    pub(crate) session_store: lumen_storage::SessionStore,
    /// Lifecycle tier manager — tracks T0→T4 transitions and LRU ordering.
    ///
    /// Synced with `tab_strip` on open/switch/close; `tick_idle` is polled
    /// from `about_to_wait` once per second to drive automatic hibernation.
    pub(crate) lifecycle_mgr: tab_lifecycle::TabLifecycleManager,
    /// Monotonic instant of the last `tick_lifecycle` call — used to throttle
    /// polling to approximately once per second.
    pub(crate) lifecycle_last_tick: std::time::Instant,
    /// Active split-view state. `None` = single-pane mode (normal).
    ///
    /// When `Some`, the window is divided into two side-by-side panes:
    /// left = active tab (live `Lumen` state), right = `SplitView::right`
    /// (frozen snapshot of another tab). `Ctrl+\` toggles; `Ctrl+M` switches focus.
    pub(crate) split_view: Option<panels::split_view::SplitView>,
    /// Vim keybinding mode state.  `None` = vim mode is off (default).
    ///
    /// Activated via `Ctrl+Alt+V`; deactivated via `Ctrl+Alt+V` again.
    /// When `Some`, [`VimMode::feed`] intercepts navigation keys before the
    /// global keybinding table.  [`VimState::Insert`] passes keys through.
    pub(crate) vim_mode: Option<input::vim::VimMode>,
    /// Vertical tab panel state. Toggled via Ctrl+B.
    ///
    /// When visible, the left `PANEL_WIDTH` CSS px of the window are occupied by
    /// the tab list and the page viewport shifts right accordingly.
    pub(crate) vertical_tabs: panels::vertical_tabs::VerticalTabsPanel,
    /// Tree-style tab panel state (7A.2): collapse/expand subtrees.
    ///
    /// Stores which subtrees are collapsed. Rendering delegate: see
    /// `panels::tree_tabs::build_panel`. Currently initialised alongside
    /// `vertical_tabs`; future toggle key will switch between flat/tree views.
    pub(crate) tree_tabs: panels::tree_tabs::TreeTabsPanel,
    /// Workspace switcher panel state (7A.3).
    ///
    /// Bottom-docked 32px bar showing named workspaces as coloured chips.
    /// `Ctrl+Shift+W` toggles.  When visible, `viewport_height_css()` subtracts
    /// `SWITCHER_HEIGHT` so the page layout does not overlap the bar.
    pub(crate) workspace_panel: panels::workspace_panel::WorkspacePanel,
    /// Persistent workspace storage — SQLite in-memory during testing; wired to
    /// a disk path in production via `Workspaces::open(path)`.
    pub(crate) workspaces: lumen_storage::Workspaces,
    /// Profile switcher dropdown state (DS-14), anchored below the toolbar
    /// avatar button (`toolbar::avatar_x()`).
    pub(crate) profile_menu: panels::profile_menu::ProfileMenuPanel,
    /// Persistent profile registry (§9.3, DS-14): profile metadata + which
    /// one is active. Opened from the portable data dir
    /// (`<exe_dir>/data/profiles.db`); first run seeds 4 default profiles
    /// (Личный/Рабочий/Анонимный/Гость — `panels::profile_menu::DEFAULT_PROFILES`).
    /// DS-14 scope: only the active pointer and visual signature (avatar,
    /// chrome accent) are wired — per-profile data isolation is DS-16.
    pub(crate) profiles: lumen_storage::ProfileRegistry,
    /// Shields floating panel state (7C.4).
    ///
    /// Shows blocked-request counts per domain, and lets the user toggle
    /// request filtering on/off for the current site.  `Ctrl+Shift+S` toggles
    /// visibility.  Backed by a shared [`BlockedLog`] updated from the network
    /// thread via [`ShieldCountSink`].
    pub(crate) shields: panels::shields_panel::ShieldsPanel,
    /// Per-site permission popover state (7C.2).
    ///
    /// Shows camera/mic/notifications/clipboard grant state for the current
    /// page origin.  Each row has a toggle button cycling Ask → Allow → Deny.
    /// `Ctrl+Shift+P` toggles visibility.  State is in-memory only (no
    /// persistence across sessions).
    pub(crate) permission: panels::permission_panel::PermissionPanel,
    /// Right-docked sidebar web panel state (7D.3).
    ///
    /// Shows a secondary web viewport in a 300 CSS px slot at the right edge.
    /// `Lumen::open_sidebar_page` supplies the page display list.
    /// When visible, `page_content_width_css()` subtracts
    /// [`panels::sidebar_panel::PANEL_WIDTH`] and `relayout()` fires.
    pub(crate) sidebar: panels::sidebar_panel::SidebarPanel,
    /// Re-layoutable source of the web sidebar page (parsed DOM + stylesheet).
    ///
    /// Kept so a drag-resize of the sidebar can reflow its content to the new
    /// width instead of stretching a frozen display list. `None` until a page
    /// is opened via [`Self::open_sidebar_page`].
    pub(crate) sidebar_source: Option<LayoutSource>,
    /// AI assistant sidebar panel (§12.8, GG-1).
    ///
    /// Right-docked 200 CSS px panel with a prompt input field and response area.
    /// `Ctrl+Shift+A` toggles visibility. When visible, `page_content_width_css()`
    /// subtracts [`panels::ai_panel::PANEL_WIDTH`] and `relayout()` fires.
    /// Queries are dispatched to [`Self::ai_backend`] synchronously (Phase 0).
    pub(crate) ai_panel: panels::ai_panel::AiPanel,
    /// Persisted, drag-resizable widths of the docked sidebars (F2-6).
    ///
    /// Replaces the panels' compiled `PANEL_WIDTH` constants: `width_for(id,
    /// default)` supplies the active width, dragging a panel's inner edge calls
    /// `set_width` + `relayout` + `save`. Loaded at startup, so the layout
    /// survives a restart.
    pub(crate) panel_layout: panel_layout::PanelLayout,
    /// In-flight docked-panel resize drag: `(dock side, panel id)` of the edge
    /// currently being dragged, or `None` when no resize is active.
    pub(crate) panel_resize: Option<(panel_layout::Dock, &'static str)>,
    /// Floating overlay showing a single user annotation (§12.2, GG-2).
    ///
    /// Opened when the user selects a `@notes`-search result from the omnibox
    /// dropdown and presses Enter. The committed value (`note-viewer:<id>`)
    /// is intercepted in `handle_omnibox_commit`. `Escape` closes the overlay.
    pub(crate) note_viewer: panels::note_viewer::NoteViewerPanel,
    /// AI inference backend for the AI sidebar (§12.8).
    ///
    /// Defaults to [`lumen_core::NullAiBackend`] (returns a stub message).
    /// Replace with a real implementation to enable AI functionality.
    pub(crate) ai_backend: Box<dyn lumen_core::AiBackend>,
    /// SQLite-backed bookmark store (in-memory for the session).
    ///
    /// Backs the bookmark manager panel. `@read-later <url>` omnibox commands and
    /// `Ctrl+D` (bookmark current page) write here; the panel reads via
    /// `Bookmarks::list_all` on every refresh.
    pub(crate) bookmarks: lumen_storage::Bookmarks,
    /// Bookmark manager panel state (task #22).
    ///
    /// Floating overlay anchored under the toolbar. `Ctrl+Shift+O` toggles
    /// visibility. Folder tree + bookmark list + search + drag-and-drop re-file
    /// (move bookmark to folder, persisted via `Bookmarks::set_folder`).
    pub(crate) bookmark_panel: panels::bookmark_panel::BookmarkPanel,
    /// SQLite-backed tab-group metadata store (CC-6, in-memory for the session).
    ///
    /// Persists group label/colour/collapsed state created via the tab context
    /// menu ("В новую группу"). Membership is session state on `TabStrip`.
    pub(crate) tab_groups: lumen_storage::TabGroups,
    /// SQLite-backed browsing history store (in-memory for the session, task D-5).
    ///
    /// Records each page visit. The history panel reads via `History::recent`
    /// (50 entries, grouped by date). `History::delete` / `History::clear` are
    /// called from the panel's delete and clear-all buttons.
    pub(crate) history_store: History,
    /// Browser history panel state (task D-5).
    ///
    /// Centred floating overlay. `Ctrl+H` toggles visibility. Shows recent pages
    /// grouped by date with search (via `HistoryFts`), delete per-entry, and a
    /// "Очистить всё" button.
    pub(crate) history_panel: panels::history_panel::HistoryPanel,
    /// Command palette modal state (task #23, §7E.2).
    ///
    /// `Ctrl+K` toggles a centred modal that fuzzy-searches across commands,
    /// bookmarks and history. While visible it captures all keyboard and pointer
    /// input; `↑/↓` move the selection, `Enter` activates, `Esc` closes.
    pub(crate) command_palette: panels::command_palette::CommandPalette,
    /// Focus mode + Pomodoro timer panel (task #25, V4).
    ///
    /// `Ctrl+Shift+F` enters a distraction-free focus mode: the tab bar is
    /// hidden and a compact Pomodoro countdown widget with an arc progress ring
    /// floats in the top-right corner. `Esc` exits focus mode (instead of
    /// quitting). The embedded `PomodoroTimer` is ticked from `about_to_wait`.
    pub(crate) focus: panels::focus_panel::FocusModePanel,
    /// Picture-in-picture floating video window (task #21).
    ///
    /// `Ctrl+Shift+V` opens a compact 320×180 card that keeps a tab's `<video>`
    /// element visible (poster placeholder) while the page scrolls or the user
    /// switches tabs. Implemented as an in-window overlay (the ad-hoc panel
    /// convention) — a true second OS window awaits multi-window support. The
    /// card can be dragged by its title bar.
    pub(crate) pip: panels::pip_window::PipWindow,
    /// CC-7 enter/exit state machine for the real OS-level PiP window, driven by
    /// the JS `_lumen_pip_enter` / `_lumen_pip_exit` requests. Pure data; the
    /// live window + backend it tracks live in [`Self::pip_os`].
    pub(crate) pip_controller: panels::pip_os_window::PipController,
    /// The live always-on-top OS window backing video Picture-in-Picture
    /// (CC-7), with its own render backend, or `None` when no `<video>` is in
    /// OS PiP. Created on `_lumen_pip_enter`; dropped on exit / close button.
    /// Falls back to the in-window [`Self::pip`] overlay when a second GPU
    /// surface cannot be created.
    pub(crate) pip_os: Option<PipOsWindow>,
    /// Document Picture-in-Picture open/closed state machine, driven by the JS
    /// `_lumen_docpip_request_window` / `_lumen_docpip_close` requests. Pure
    /// data; the live window + backend it tracks live in [`Self::doc_pip_os`].
    pub(crate) doc_pip_controller: panels::doc_pip_os_window::DocPipController,
    /// The live always-on-top OS window backing `documentPictureInPicture`
    /// (Document PiP slice 1), with its own render backend, or `None` when no
    /// Document PiP window is open. Created on `_lumen_docpip_request_window`;
    /// dropped on `.close()` / OS close button. Unlike [`Self::pip_os`] there
    /// is no in-window overlay fallback — window/backend creation failure just
    /// leaves the request unfulfilled (the JS `PictureInPictureWindow` promise
    /// still resolves; `.document` stays a JS-only mock either way, see
    /// `document_pip.rs`).
    pub(crate) doc_pip_os: Option<DocPipOsWindow>,
    /// Right-button drag gesture recognizer (§7B.3).
    ///
    /// Tracks right-button drags, classifies the trajectory into L/R/U/D/LD/RD,
    /// and maps each direction to a [`GestureAction`] via a configurable
    /// [`GestureMap`].  Default bindings: Left=Back, Right=Forward,
    /// LeftDown=CloseTab, RightDown=NewTab.
    pub(crate) gesture: input::gesture::GestureRecognizer,
    /// SQLite-backed omnibox bang-alias registry (§7B.4).
    ///
    /// Seeded with `!g` (Google) and `!gh` (GitHub) on startup.  Custom aliases
    /// are addable via `set(trigger, expansion)`.
    pub(crate) omnibox_aliases: lumen_storage::OmniboxAliases,
    /// SQLite-backed pinned `about:newtab` speed-dial tiles (DS-11).
    ///
    /// Portable-data store (`<exe_dir>/data/newtab_tiles.db`); falls back to
    /// in-memory if the path cannot be opened.
    pub(crate) newtab_tiles: lumen_storage::NewtabTiles,
    /// In-session notes created via `@notes <text>` in the omnibox.
    ///
    /// Persisted in-memory for the session; each entry is a raw text string.
    /// Displayed nowhere yet — UI is a future task.
    pub(crate) notes: Vec<String>,
    /// §12.3 Read-later storage: persists HTML snapshots of saved pages.
    ///
    /// Populated by the `@read-later <url>` omnibox command: a background thread
    /// fetches the page HTML and calls `save()`. In-memory only (no SQLite path
    /// for the first ship — drop-in replacement once a `read_later.db` path is
    /// wired through the profile directory).
    pub(crate) read_later_store: lumen_knowledge::ReadLater,
    /// §12.3 Read-later panel state (Ctrl+Shift+R).
    pub(crate) read_later_panel: panels::read_later_panel::ReadLaterPanel,
    /// Channel receiver for completed background read-later fetches.
    ///
    /// Background threads send `(url, title, html_bytes)` here when done.
    /// Drained in `about_to_wait` to call `read_later_store.save()`.
    pub(crate) read_later_rx: std::sync::mpsc::Receiver<(String, String, Vec<u8>)>,
    /// Sender half of the read-later fetch channel (cloned into each background thread).
    pub(crate) read_later_tx: std::sync::mpsc::Sender<(String, String, Vec<u8>)>,
    /// Cookie-banner auto-dismiss preference (7C.3).
    ///
    /// When `true` (default) the JS shim in `lumen-js` auto-clicks consent-banner
    /// accept buttons on every page load. When `false` banners are shown normally.
    /// Toggle via `Ctrl+Shift+K` or a future settings UI.
    pub(crate) cookie_banner_dismiss: bool,
    /// Idle GC tick: drains dead DOM node IDs every 30 s and purges JS-side
    /// per-node caches (`_lumen_listeners`, `_input_values`) via `_lumen_gc_collect`.
    pub(crate) gc_tick: gc_tick::GcTick,
    /// Throttled OS memory pressure poller (ADR-008 §10H).
    ///
    /// Polled every 5 s in `about_to_wait`.  On `Medium` or `High` pressure,
    /// [`CacheRegistry::broadcast_pressure`] is called on `cache_registry`, and
    /// owned caches (`image_cache`, renderer `layer_cache`) are evicted directly.
    pub(crate) memory_poll: memory_poll::MemoryPollTick,
    /// Registry of cross-session shared caches (ADR-008 §10D.3).
    ///
    /// Caches registered here receive `on_memory_pressure` broadcasts from the
    /// poll loop.  Owned per-page caches (`image_cache`, layer cache) are evicted
    /// directly rather than through the registry to avoid shared-ownership overhead.
    pub(crate) cache_registry: lumen_core::ext::CacheRegistry,
    /// Deterministic render mode (8F).
    ///
    /// When `enabled` (`--deterministic` CLI flag): window opens at 1280×800
    /// (unless overridden by `viewport_override`, DEVX-1), `Date.now()` is
    /// frozen at 0, `Math.random` uses a seeded PRNG, and
    /// `requestAnimationFrame` callbacks receive a 0 ms timestamp.
    /// `rng_seed`/`monotonic_clock` (DEVX-16, `--rng-seed`/`--monotonic-clock`)
    /// reach the JS runtime via `V8JsRuntime::set_deterministic_mode`.
    /// Intended for snapshot testing and reproducible output.
    pub(crate) deterministic: deterministic::DetConfig,
    /// `--viewport <W>x<H>` override (DEVX-1): pins the window's CSS content
    /// viewport size, taking priority over both the `deterministic` 1280×800
    /// default and the plain 1024×720 default (see `resumed()`). Lets
    /// automation combine `--deterministic` with `graphic_tests`'s fixed
    /// 1024×720 crop-calibration contract.
    pub(crate) viewport_override: Option<(f32, f32)>,
    /// DevTools JS console panel (§7E.5).
    ///
    /// Captures `console.log/warn/error` output from the active page's JS runtime.
    /// Visible as a bottom overlay; toggled with `F12`.
    pub(crate) devtools_console: devtools::console_panel::ConsolePanel,
    /// DevTools DOM inspector panel (§7E.1).
    ///
    /// While active, hovering highlights the box under the cursor with a
    /// box-model overlay and clicking pins a node, showing its computed style
    /// in a right-docked side panel. Toggled with `Ctrl+Shift+I`.
    pub(crate) dom_inspector: devtools::inspector::DomInspectorPanel,
    /// DevTools network log panel (§7E.4).
    ///
    /// Shows a live log of HTTP requests (method / status / timing / URL),
    /// fed by `NetworkLogSink` from the engine's `EventSink`. Bottom overlay,
    /// toggled with `Ctrl+Shift+E`.
    pub(crate) network_panel: devtools::network_panel::NetworkPanel,
    /// Privacy network panel (V5).
    ///
    /// A privacy-focused, right-docked overlay sharing the same `NetworkLog` as
    /// [`network_panel`]: it presents the request stream as a newest-first log of
    /// tracker domains with blocked/allowed status and the matched filter rule,
    /// plus a blocked/allowed summary. Toggled with `Ctrl+Shift+Y`.
    ///
    /// [`network_panel`]: Lumen::network_panel
    pub(crate) privacy: panels::privacy_panel::PrivacyPanel,
    /// Persistent accessibility preferences store (task E-2).
    ///
    /// Backed by SQLite (in-memory for the session). Stores font-size
    /// multiplier, prefers-reduced-motion, forced-colors, and cursor size.
    /// Read on panel open; written when panel closes.
    pub(crate) a11y_store: lumen_storage::A11yPrefs,
    /// Accessibility settings panel overlay (task E-2, `Ctrl+Shift+Q`).
    ///
    /// A centred 300×260 px modal. Holds a working draft; on close the draft
    /// is persisted to `a11y_store` and media changes are re-delivered to JS.
    pub(crate) a11y_panel: panels::a11y_panel::A11yPanel,
    /// Platform accessibility bridge (O-5).
    ///
    /// Receives `AXTree` updates after every page load and focus change.
    /// Routes them to the OS accessibility API (UIA / NSAccessibility / AT-SPI2).
    pub(crate) platform_bridge: Box<dyn lumen_a11y::platform::PlatformBridge>,
    /// Print dialog overlay (task E-1, `Ctrl+P`).
    ///
    /// A centred 560×400 px modal with paper size, orientation, margins,
    /// page range, colour mode, and output-file fields. Clicking **Print**
    /// calls `do_print_to_pdf()` with the configured settings.
    pub(crate) print_panel: panels::print_panel::PrintPanel,
    /// Persistent browser settings store (task D-7).
    ///
    /// Backed by SQLite at `<exe_dir>/data/settings.db` (survives restarts;
    /// falls back to an in-memory store if the file cannot be opened). Stores
    /// homepage, search engine ID, shields, fingerprint mode, DoH, font size,
    /// theme, download path, tab layout, and panel layout. Read on panel open;
    /// written when panel closes.
    pub(crate) settings_store: lumen_storage::BrowserSettings,
    /// Settings page overlay state (task D-7, `about:settings`).
    ///
    /// `Ctrl+,`, the settings gear button in the tab strip, or navigating to
    /// `about:settings` toggles a centred overlay with seven tabbed sections:
    /// General, Privacy, Appearance, Downloads, Network, Adblock, Language.
    /// Opened/closed via [`Lumen::open_settings_panel`] /
    /// [`Lumen::close_settings_panel`], which also sync the sections backed by
    /// stores other than `settings_store` (HTTP/3 → `fingerprint.toml`,
    /// ad-block subscriptions → `AdblockStore`, spellcheck locale → `SPELL_DICTS`).
    pub(crate) settings_panel: panels::settings_panel::SettingsPanel,
    /// Persistent ad-block filter-list store (`<exe_dir>/data/adblock/adblock.db`).
    ///
    /// Opened once at startup ([`config::init_adblock`]); shared with the
    /// background refresh thread and with the settings panel's Adblock
    /// section (enable/disable a subscription, trigger a manual refresh).
    pub(crate) adblock_store: std::sync::Arc<lumen_storage::adblock::AdblockStore>,
    /// Keyboard shortcuts panel (Ctrl+Shift+/, §D-4).
    ///
    /// Shows all `KeyCommand` bindings with rebind-on-click support.
    pub(crate) shortcuts_panel: panels::shortcuts_panel::ShortcutsPanel,
    /// Certificate viewer panel (Ctrl+Shift+C, §D-1).
    ///
    /// Centred 500×440 overlay showing X.509 cert data (subject CN/Org, issuer,
    /// validity dates, SHA-256 fingerprint, SAN list, TLS version).
    pub(crate) cert_panel: panels::cert_panel::CertPanel,
    /// Whether the curated system-font fallback chain has been preloaded into
    /// the renderer (CSS Fonts L4 §5.3 codepoint cascade).
    ///
    /// The renderer can fall back per-glyph across loaded faces, but those
    /// faces must first be loaded via `Renderer::preload_curated_fallbacks`.
    /// Without it, CJK / emoji / RTL / Indic codepoints on pages with no
    /// explicit `font-family` for that script render as `.notdef`. Preloading
    /// is a one-time, idempotent operation (the curated families are system
    /// fonts, identical across pages), so this guard runs it once after the
    /// first page provides a `FontProvider`.
    pub(crate) fallbacks_preloaded: bool,
    /// Virtual URL shown in the address bar after `history.pushState` /
    /// `history.replaceState`.  `None` → use `source.url_str()`.
    /// Reset to `None` on any full navigation.
    pub(crate) display_url: Option<String>,
    /// Serialised JS state JSON for the current history entry, mirrored from JS
    /// so the shell can populate `NavEntry::same_doc_state_json` on pushState.
    /// `"null"` until a `pushState`/`replaceState` call updates it.
    pub(crate) current_history_state_json: String,
    /// Node ID of the currently fullscreen element, or `None` if not fullscreen.
    ///
    /// Set when `requestFullscreen()` is called in JS and cleared when
    /// `document.exitFullscreen()` or `Escape` exits fullscreen.  Used to deliver
    /// `_lumen_notify_fullscreen_exit()` when the OS exits fullscreen externally.
    pub(crate) fullscreen_nid: Option<u32>,
    /// Pending viewport reconciliation after an OS fullscreen toggle (BUG-167).
    ///
    /// `Some((prev_w, prev_h, attempts_left))` is armed right after
    /// `window.set_fullscreen(..)` is called: `prev_w`/`prev_h` are the window's
    /// **physical** inner size *before* the OS applied the new mode. The OS
    /// resizes the window asynchronously, so `about_to_wait` polls each loop
    /// iteration until `inner_size()` differs from `(prev_w, prev_h)`, then runs
    /// the same resize + relayout path as `WindowEvent::Resized` so the page
    /// viewport (`vw`/`vh`, `innerWidth`/`innerHeight`) follows the fullscreen
    /// area. `attempts_left` bounds the poll so a no-op toggle can't spin the
    /// loop; it is cleared once the size changes or the budget runs out.
    pub(crate) fullscreen_resize_pending: Option<(u32, u32, u8)>,
    /// Active CSS View Transition (CSS View Transitions L1 §4).
    ///
    /// Set when `document.startViewTransition(callback)` fires `_lumen_vt_end`.
    /// The `old_dl` snapshot fades out over the new display list for `duration_ms`.
    /// `None` when no transition is active.
    pub(crate) view_transition: Option<ViewTransitionState>,
    /// Tab auto-archive state (7A.5).
    ///
    /// Background tabs idle for more than `ARCHIVE_AFTER_MS` are moved here from
    /// the visible tab strip.  Only a title + URL string is retained; restoring
    /// opens a fresh navigation to that URL.  The archive button (rightmost 36 px
    /// of the tab bar) shows a count badge and toggles the archive panel.
    pub(crate) archive: tabs::archive::TabArchive,
    /// Timestamp (wall ms) when restore of a hibernated tab began.
    ///
    /// `Some(ms)` = spinner overlay is active; `None` = no restoration in progress.
    /// Set at the start of `restore_hibernated_tab` and cleared when restore completes.
    pub(crate) restore_spinner_start_ms: Option<f64>,
    /// Active element resize: `Some((node_id, start_x, start_y, allow_width, allow_height))`
    /// when user is dragging the resize grip. `None` when no resize is active.
    /// Set on MouseInput Pressed over a resize grip, cleared on MouseInput Released.
    /// `allow_width`/`allow_height` are the grip node's `Resize` CSS value resolved to
    /// physical axes (CC-CSS-4: `Resize::allowed_axes`, writing-mode aware) at press
    /// time — they gate which of width/height is updated during CursorMoved via the
    /// JS binding, so `resize: vertical` no longer also changes width on a diagonal drag.
    pub(crate) resize_active: Option<(lumen_dom::NodeId, f32, f32, bool, bool)>,
    /// In-progress tab drag-and-drop (§O-9).
    ///
    /// `Some` from the moment the user presses on a tab until they release.
    /// Transitions to `active = true` after the cursor crosses
    /// [`tabs::strip::DRAG_THRESHOLD`] px.  On release, calls
    /// `tab_strip.move_tab` if the drag was active.
    pub(crate) tab_drag: Option<tabs::strip::TabDragState>,
    /// In-progress HTML5 drag-and-drop gesture (PH3-9 / HTML LS §9.3.3).
    ///
    /// `Some` from `mousedown` on a draggable element until `mouseup`.
    /// Transitions to `active = true` after the cursor travels ≥
    /// `DND_THRESHOLD` px, at which point `dragstart` is fired on `src_nid`.
    /// On `mouseup`: fires `drop` on the current target, `dragend` on `src_nid`,
    /// then clears this field.
    pub(crate) dnd_state: Option<DndState>,
    /// Right-click tab context menu (CC-4): Duplicate / Pin / Move to new
    /// window / Close others / Close to the right. Hidden unless `open`.
    pub(crate) tab_context_menu: tabs::context_menu::TabContextMenu,
    /// Page-level spell-check suggestion menu (P3-spell slice 3): opened by
    /// right-clicking a misspelled word in a focused text `<input>`. Hidden
    /// unless open.
    pub(crate) page_context_menu: page_context_menu::PageContextMenu,
    /// Words the user added to the persistent dictionary
    /// (`data/spell/user_words.txt`), lowercase. Treated as correct spellings.
    pub(crate) spell_user_words: std::collections::HashSet<String>,
    /// Words the user chose to ignore for this session ("Пропустить"),
    /// lowercase. Cleared on restart.
    pub(crate) spell_ignored: std::collections::HashSet<String>,
    /// Shell UI theme: base brightness + accent colour (§O-9).
    ///
    /// Initialised from `BrowserSettings` on startup.  Updated when the user
    /// changes the theme or accent in the settings panel (Appearance section).
    /// The accent drives the active-tab indicator colour passed to
    /// `build_tab_bar`.
    pub(crate) shell_theme: panels::themes::ShellTheme,
    /// Original page source stored when Reader View (§D-3) is active.
    ///
    /// `Some` when the current page is showing the clean reader HTML (F9 toggle);
    /// `None` in normal browsing mode.  Toggling F9 again restores this source.
    pub(crate) reader_original_source: Option<PageSource>,
    /// TLS certificate information for the current tab (§D-1).
    ///
    /// Populated when a page loads over HTTPS; cleared on tab switch / navigation.
    /// Phase 0: shell can set this to a stub value via `CertInfo::stub_for`.
    pub(crate) cert_info: Option<panels::cert_panel::PanelCertData>,
}
