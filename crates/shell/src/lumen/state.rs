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
    /// Р’РµСЂСЃРёСЏ [`Self::display_list`] РґР»СЏ СЂРµРЅРґРµСЂРµСЂР° (BUG-405 СЃСЂРµР· 39).
    ///
    /// РњРµРЅСЏРµС‚СЃСЏ РїСЂРё РљРђР–Р”РћРњ РёР·РјРµРЅРµРЅРёРё СЃРїРёСЃРєР° вЂ” Рё Р·Р°РјРµРЅРµ С†РµР»РёРєРѕРј, Рё РїСЂР°РІРєРµ РЅР°
    /// РјРµСЃС‚Рµ, вЂ” РїРѕСЌС‚РѕРјСѓ СЃРїРёСЃРѕРє РјРµРЅСЏСЋС‚ С‚РѕР»СЊРєРѕ С‡РµСЂРµР· [`Self::set_display_list`]
    /// Рё [`Self::display_list_mut`], Р° РЅРµ РїСЂРёСЃРІР°РёРІР°РЅРёРµРј РїРѕР»СЋ. РџРѕРєР° РІРµСЂСЃРёСЏ С‚Р°
    /// Р¶Рµ, СЂРµРЅРґРµСЂРµСЂ РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµС‚ СЃРІС‘СЂС‚РєСѓ content-С‡Р°СЃС‚Рё РєР°РґСЂРѕРІС‹С… С…СЌС€РµР№ РІРјРµСЃС‚Рѕ
    /// РѕР±С…РѕРґР° РІСЃРµРіРѕ СЃРїРёСЃРєР°; РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№ Р±Р°РјРї РїРѕРєР°Р·Р°Р» Р±С‹ СѓСЃС‚Р°СЂРµРІС€РёРµ РїРёРєСЃРµР»Рё.
    /// РќРёРєРѕРіРґР° РЅРµ `0`: РЅРѕР»СЊ Р·Р°СЂРµР·РµСЂРІРёСЂРѕРІР°РЅ Р·Р° В«РІРµСЂСЃРёСЏ РЅРµРёР·РІРµСЃС‚РЅР°В».
    pub(crate) display_list_epoch: u64,
    /// Tile-based dirty-rect tracker. Updated on every display-list change via
    /// [`lumen_paint::TileGrid::update_from_diff`]. Dirty tiles are re-rendered
    /// on the next frame; clean tiles reuse the previous output (Phase 2).
    pub(crate) tile_grid: lumen_paint::TileGrid,
    /// Per-subtree display-list cache. Keyed by stacking-context root `NodeId`.
    /// Hit on a matching `content_hash` в†’ skip re-traversing the layout tree for
    /// that subtree. Registered with `cache_registry` so OS memory-pressure
    /// events evict it via `EvictableCache::on_memory_pressure` (EE-4).
    pub(crate) display_list_cache: lumen_paint::DisplayListCache,
    pub(crate) title: Option<String>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ `<img>` СЂРµСЃСѓСЂСЃС‹. Р”Рѕ СЃРѕР·РґР°РЅРёСЏ Renderer-Р° вЂ” С…СЂР°РЅСЏС‚СЃСЏ
    /// РІ Vec Рё Р·Р°Р»РёРІР°СЋС‚СЃСЏ РІ GPU РІ `resumed`; РїРѕСЃР»Рµ вЂ” register_image РёРґС‘С‚
    /// РЅР°РїСЂСЏРјСѓСЋ РІ `reload`. РќР° РїРµСЂРµС…РѕРґР°С… РјРµР¶РґСѓ СЃС‚СЂР°РЅРёС†Р°РјРё РѕС‡РёС‰Р°РµС‚СЃСЏ С‡РµСЂРµР·
    /// `Renderer::clear_images` + РїРµСЂРµСѓСЃС‚Р°РЅРѕРІРєР°. `Arc<Image>` (BUG-272 СЃСЂРµР· 17).
    pub(crate) pending_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// PH3-19: СЂРµРµСЃС‚СЂ С€СЂРёС„С‚РѕРІ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ (local() + web-С€СЂРёС„С‚С‹, РїСЂРёС€РµРґС€РёРµ
    /// С‡РµСЂРµР· `FontLoaded`). РҐСЂР°РЅРёС‚СЃСЏ РѕС‚РґРµР»СЊРЅРѕ РѕС‚ `Arc<dyn FontProvider>` РІ renderer-Рµ,
    /// С‡С‚РѕР±С‹ `user_event(FontLoaded)` РјРѕРі РґРѕСЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ С€СЂРёС„С‚ С‡РµСЂРµР·
    /// `register_from_bytes` Р±РµР· РґР°СѓРЅРєР°СЃС‚Р°, Р° Р·Р°С‚РµРј РѕР±РЅРѕРІРёС‚СЊ СЂРµРЅРґРµСЂРµСЂ РѕРґРЅРѕР№ СЃС‚СЂРѕРєРѕР№.
    /// РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РЅР° РєР°Р¶РґСѓСЋ РЅР°РІРёРіР°С†РёСЋ РІРјРµСЃС‚Рµ СЃ `web_fonts`.
    pub(crate) page_font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: web-С€СЂРёС„С‚С‹ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹, СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ РёР· @font-face url().
    /// РСЃРїРѕР»СЊР·СѓСЋС‚СЃСЏ РґР»СЏ РїРµСЂРµСЃР±РѕСЂРєРё `MultiFontMeasurer` РїСЂРё РєР°Р¶РґРѕРј relayout (resize,
    /// scroll, JS DOM mutation) вЂ” Р±РµР· С…СЂР°РЅРµРЅРёСЏ Р·РґРµСЃСЊ resize-relayout С‚РµСЂСЏР» Р±С‹
    /// web-РјРµС‚СЂРёРєРё Рё РѕС‚РєР°С‚С‹РІР°Р»СЃСЏ Рє Inter.  РћС‡РёС‰Р°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё.
    pub(crate) web_fonts: Vec<LoadedWebFont>,
    pub(crate) source: PageSource,
    pub(crate) event_sink: Arc<dyn EventSink>,
    pub(crate) modifiers: ModifiersState,
    pub(crate) window: Option<Arc<Window>>,
    /// Detected target `ColorSpace` for the active display.
    /// Populated at startup from the OS (Windows WCS/DXGI/EDID query).
    /// Defaults to `ColorSpace::Srgb` when the display profile is unknown or
    /// the OS query fails вЂ” making the whole wide-gamut pipeline a no-op on
    /// sRGB-only hardware.
    #[allow(dead_code)] // РїРѕС‚СЂРµР±РёС‚РµР»СЊ РїРѕСЏРІРёС‚СЃСЏ РїСЂРё P3 wiring (ph3-color-management Step 1)
    pub(crate) display_color_profile: platform::display_color_profile::PlatformDisplayColorProfile,
    pub(crate) renderer: Option<Box<dyn RenderBackend>>,
    /// CC-4: chrome document + stylesheet, parsed once at startup via
    /// [`lumen_chrome::parse_document`] from `chrome_preview::HTML` вЂ” the same
    /// bytes `build.rs` already CSS-gated. Only relaid out on resize
    /// ([`Lumen::relayout_chrome_host`]); the asset has no dynamic content yet
    /// (`ChromeModel` DOM mutation is CC-6), so nothing else invalidates it.
    ///
    /// CC-15-6: always `Some` since the `LUMEN_LEGACY_CHROME` rollback flag was
    /// deleted вЂ” the `Option` is now only the shape every accessor already reads
    /// through, not a live "no engine chrome" mode.
    pub(crate) chrome_doc: Option<(lumen_dom::Document, lumen_css_parser::Stylesheet)>,
    /// CC-4: `LayoutBox` + display list of the last `relayout_chrome_host` pass,
    /// painted at the front of `overlay_buf` every frame (legacy panels/tab-bar/
    /// toolbar still draw over it, painter's order). `None` until the first
    /// resize after startup provides a window size. `#contentArea` вЂ” the
    /// design reference's placeholder for tab content, doubling as the
    /// brief's "`#page-host`" вЂ” is pruned out of this tree entirely (not just
    /// its children) before painting, so neither its demo markup nor its own
    /// `background:var(--surface-0)` fill can end up on top of the real page
    /// painted separately at [`Self::chrome_page_host_rect`]'s rect.
    pub(crate) chrome_layout: Option<(lumen_layout::LayoutBox, lumen_paint::DisplayList)>,
    /// CC-4: `#contentArea`'s rect, captured from the layout tree right
    /// before [`Self::relayout_chrome_host`] prunes that node out вЂ” replaces
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
    /// interactive thread-locals rather than inheriting them вЂ” the two
    /// documents' hover state must never leak into each other's layout pass.
    pub(crate) chrome_hovered_nid: Option<NodeId>,
    /// CC-5: pressed node in `chrome_layout`'s tree вЂ” mirrors
    /// [`Self::chrome_hovered_nid`] but for `:active`, set from
    /// `WindowEvent::MouseInput` press.
    pub(crate) chrome_active_nid: Option<NodeId>,
    /// CC-7 (docs/tasks/p1-css-chrome.md): `#omniInput`'s post-layout rect
    /// from the last [`Self::relayout_chrome_host`] pass вЂ” the anchor for the
    /// hand-painted caret overlay (editing itself stays owned by the legacy
    /// `address_bar::AddressBarState`, no native caret exists for `<input>`
    /// yet). `None` off the flag or before the first chrome layout.
    pub(crate) chrome_omni_input_rect: Option<Rect>,
    /// CC-8 (docs/tasks/p1-css-chrome.md): `true` collapses the vertical
    /// sidebar to its icon rail (`#sidebar.collapsed`, `--sidebar-w-collapsed`
    /// in the asset). Toggled by `ChromeAction::ToggleSidebar`
    /// (`.sb-collapse` button). Independent of [`Self::vertical_tabs`]'s own
    /// `visible` flag вЂ” that one picks vertical vs. horizontal layout,
    /// this one narrows the vertical sidebar without hiding it.
    pub(crate) chrome_sidebar_collapsed: bool,
    /// CC-10b (docs/tasks/p1-css-chrome.md): `data-section` slug of the
    /// active `#view-settings` tab (`"general"`/`"privacy"`/`"appearance"`/
    /// `"sync"`/`"ext"`/`"qa"`). Engine-chrome-only UI state вЂ” the design's 6
    /// sections don't line up with `SettingsPanel::SettingsSection`'s 7 (see
    /// `lumen_chrome::ChromeSettingsModel` doc comment), so this is a
    /// separate field rather than a projection of the legacy enum. Set by
    /// `ChromeAction::SetSettingsSection`.
    pub(crate) chrome_settings_section: String,
    /// CC-11 (docs/tasks/p1-css-chrome.md): CSS Animations scheduler for the
    /// chrome document вЂ” a separate instance from [`Self::animation_scheduler`]
    /// because `chrome_doc` and the page `Document` number `NodeId`s
    /// independently (both start at 0), so a shared scheduler would collide
    /// entries between the two trees. Ticked on every `RedrawRequested`
    /// alongside the page scheduler.
    /// Unlike the page scheduler, never `.clear()`-ed: `chrome_doc`'s nodes
    /// persist for the process lifetime (no reload/navigation equivalent for
    /// chrome), so clearing on every [`Self::relayout_chrome_host`] call вЂ”
    /// which happens far more often than page relayouts (any hover/click) вЂ”
    /// would restart `infinite` animations (the spinner) on every interaction.
    pub(crate) chrome_animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CC-11: CSS Transitions scheduler for the chrome document вЂ” mirrors
    /// [`Self::transition_scheduler`] but keyed against `chrome_doc`'s own
    /// `NodeId` space (see [`Self::chrome_animation_scheduler`] doc comment).
    /// `sync()` runs at the end of [`Self::relayout_chrome_host`] (chrome's
    /// post-layout point, mirroring `apply_relayout_result`'s page-side
    /// sync); `tick()` runs on every `RedrawRequested`.
    pub(crate) chrome_transition_scheduler: TransitionScheduler,
    /// CC-11: computed styles from the previous [`Self::relayout_chrome_host`]
    /// pass вЂ” needed by [`Self::chrome_transition_scheduler`]'s `sync()` to
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
    /// cycle вЂ” the largest item left in a chrome interaction, and ~40 % of a
    /// hover cycle. S22 keeps the *difference* instead: the pruning is
    /// recorded here and undone by [`restore_content_area`] at the top of the
    /// next pass, so the live tree in [`Self::chrome_layout`] becomes the
    /// basis and no copy is made at all. Sound because nothing mutates
    /// `chrome_layout` between passes вЂ” it is read-only until
    /// [`Self::relayout_chrome_host`] replaces it wholesale.
    pub(crate) chrome_content_area_detached: Option<ContentAreaDetachment>,
    /// BUG-341 S5: the per-node `ComputedStyle` cascade cache
    /// ([`lumen_layout::CounterMap::styles`]) from the previous pass вЂ”
    /// `RestyleDelta::prev_styles` for the next incremental cascade. Distinct
    /// from [`Self::chrome_prev_styles`] (CC-11's transition-sync snapshot,
    /// collected from post-layout `LayoutBox`es *after* `font-size-adjust` has
    /// mutated them in place) вЂ” the cascade cache must be the pre-layout,
    /// pre-adjust styles the cascade itself produced, or the incremental
    /// cascade's `incr == full` correctness gate (BUG-341 brief В§4) would
    /// compare against the wrong reference.
    pub(crate) chrome_prev_cascade_styles: lumen_layout::CascadeStyles,
    /// BUG-341 S5: `(hover, focus, active)` node ids from the previous pass вЂ”
    /// `restyle_root_set_for_state_change`'s `prev` argument for each axis, so
    /// a hover/focus/active transition can compute its conservative dirty
    /// root-set (brief В§4).
    pub(crate) chrome_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// BUG-341 S5: viewport size the previous pass laid out at вЂ” a resize
    /// invalidates the previous tree's geometry for `graft_geometry` purposes,
    /// so a viewport change forces the full-layout path regardless of what
    /// `bind_model_tracked` reports touched.
    pub(crate) chrome_prev_viewport: Option<Size>,
    /// BUG-341 S5: Forced Colors Mode state ([`lumen_layout::forced_colors_active`])
    /// the previous pass ran under. Not part of `ChromeModel` (it's a
    /// thread-local accessibility preference, not shell UI state), but it does
    /// feed the cascade вЂ” a change here must force a full recompute (the
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
    /// transform, color, background-color) are applied вЂ” same limitation as
    /// [`Self::anim_frame`] for the page, since `width` transitions
    /// (`#sidebar`, `.dl-progress-fill`) aren't in the Phase-0 animatable
    /// property table (`TransitionScheduler::sync`) and stay unanimated.
    pub(crate) chrome_anim_frame: Option<lumen_layout::AnimationFrame>,
    /// HTML event loop runtime. РќР° РєР°Р¶РґРѕР№ РёС‚РµСЂР°С†РёРё winit-loop (AboutToWait)
    /// РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ РѕРґРЅР° task, РЅР° RedrawRequested вЂ” run_rendering_step
    /// (РІС‹Р·С‹РІР°РµС‚ rAF-callback-Рё), РЅР° WindowEvent::Resized вЂ”
    /// deliver_observer_records(Resize).
    pub(crate) runtime: runtime::EventLoop,
    /// CSS Animations timeline scheduler вЂ” С‚РёРєР°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕРј RedrawRequested.
    /// РҐСЂР°РЅРёС‚ start-time РґР»СЏ РєР°Р¶РґРѕР№ Р·Р°РїСѓС‰РµРЅРЅРѕР№ Р°РЅРёРјР°С†РёРё Рё РІС‹С‡РёСЃР»СЏРµС‚
    /// РёРЅС‚РµСЂРїРѕР»РёСЂРѕРІР°РЅРЅС‹Рµ Р·РЅР°С‡РµРЅРёСЏ. РћС‡РёС‰Р°РµС‚СЃСЏ РїСЂРё load/reload.
    pub(crate) animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CSS Transitions scheduler вЂ” reactive; РѕР±РЅР°СЂСѓР¶РёРІР°РµС‚ РёР·РјРµРЅРµРЅРёСЏ computed-style
    /// РјРµР¶РґСѓ РґРІСѓРјСЏ relayout-Р°РјРё Рё РёРЅС‚РµСЂРїРѕР»РёСЂСѓРµС‚ Р·РЅР°С‡РµРЅРёСЏ per-frame.
    /// `sync()` РІС‹Р·С‹РІР°РµС‚СЃСЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ layout-РѕР±РЅРѕРІР»РµРЅРёСЏ; `tick()` вЂ” РЅР° РєР°Р¶РґРѕРј
    /// RedrawRequested РІРјРµСЃС‚Рµ СЃ animation_scheduler. РћС‡РёС‰Р°РµС‚СЃСЏ РїСЂРё load/reload.
    pub(crate) transition_scheduler: TransitionScheduler,
    /// Tracks nodes that are "entering" the document (inserted or display:noneв†’visible)
    /// so that `@starting-style` rules can provide the before-change style for their
    /// entry transitions (CSS Transitions L2 В§3.4). Consumed in `relayout()`.
    pub(crate) starting_style_tracker: StartingStyleTracker,
    /// Computed styles РїСЂРµРґС‹РґСѓС‰РµРіРѕ layout-РґРµСЂРµРІР° вЂ” РЅСѓР¶РЅС‹ `transition_scheduler.sync()`
    /// РґР»СЏ РѕРїСЂРµРґРµР»РµРЅРёСЏ РёР·РјРµРЅРёРІС€РёС…СЃСЏ СЃРІРѕР№СЃС‚РІ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ layout.
    pub(crate) prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S7: `CounterMap::styles()` cascade cache from the last
    /// [`Self::try_relayout_raf_incremental`] call that took the restyle-aware
    /// path (`layout_mutation_incremental_restyle`) вЂ” the `RestyleDelta::prev_styles`
    /// basis for the *next* such call. `None` whenever `layout_box` was set by
    /// any other producer (`relayout()`, tab switch, page load, hibernate
    /// restore, streaming layout, вЂ¦), since a stale cache would silently derive
    /// the wrong dirty-root set against a `layout_box` it does not match вЂ”
    /// `try_relayout_raf_incremental` falls back to the existing
    /// full-cascade-plus-graft path (`layout_mutation_incremental`) whenever
    /// this is `None`.
    pub(crate) page_prev_cascade_styles: Option<lumen_layout::CascadeStyles>,
    /// Interactive state (`hovered_nid`/`focused_node`/`active_nid`) at the
    /// moment `page_prev_cascade_styles` was captured вЂ” the `prev` side of the
    /// next call's `restyle_root_set_for_state_change`. Only meaningful when
    /// `page_prev_cascade_styles` is `Some`.
    pub(crate) page_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// РџРѕСЃР»РµРґРЅРёР№ РІС‹С‡РёСЃР»РµРЅРЅС‹Р№ РєР°РґСЂ Р°РЅРёРјР°С†РёР№. `None` вЂ” СЃС‚СЂР°РЅРёС†Р° РЅРµ Р·Р°РіСЂСѓР¶РµРЅР°
    /// РёР»Рё РЅРµС‚ Р°РєС‚РёРІРЅС‹С… Р°РЅРёРјР°С†РёР№.
    pub(crate) anim_frame: Option<lumen_layout::AnimationFrame>,
    /// Layout-РґРµСЂРµРІРѕ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ вЂ” РЅСѓР¶РµРЅ scheduler-Сѓ РґР»СЏ РѕР±С…РѕРґР° СѓР·Р»РѕРІ
    /// Рё РёР·РІР»РµС‡РµРЅРёСЏ animation-longhands. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїСЂРё load/reload/relayout.
    pub(crate) layout_box: Option<lumen_layout::LayoutBox>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ (`<video>` в†’ cues).
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
    /// Р­РїРѕС…Р° РґР»СЏ rAF-timestamp-РѕРІ РІ РјРёР»Р»РёСЃРµРєСѓРЅРґР°С… РѕС‚ СЃС‚Р°СЂС‚Р° shell-Р°
    /// (DOMHighResTimeStamp вЂ” HTML В§8.1.5.1: В«timestamp passed to callback
    /// should be the current high resolution timeВ»).
    pub(crate) epoch: std::time::Instant,
    /// Timestamp (ms from `epoch`) of the last `requestAnimationFrame` batch fire.
    ///
    /// Used by the vsync gate: rAF callbacks fire at most once per `RAF_MIN_INTERVAL_MS`
    /// (~16.67 ms, 60 Hz). Initialized to `-RAF_MIN_INTERVAL_MS` so the first frame
    /// fires immediately.
    pub(crate) last_raf_batch_ms: f64,
    /// TEMP BUG-272 diagnostics: epoch seconds of the last memory report.
    pub(crate) last_mem_report_s: f64,
    /// РЎРµСЃСЃРёРѕРЅРЅС‹Р№ Р°РєРєСѓРјСѓР»СЏС‚РѕСЂ РІСЂРµРјС‘РЅ РєР°РґСЂРѕРІ (`LUMEN_FRAME_LOG`, M0.1 ADR-016).
    /// РќР°РїРѕР»РЅСЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё РІРєР»СЋС‡С‘РЅРЅРѕРј frame-log; СЃРІРѕРґРєР° p50/p95/p99
    /// РїРµС‡Р°С‚Р°РµС‚СЃСЏ РїРѕ РєР°РґР°РЅСЃСѓ `LUMEN_MEM_REPORT` Рё РѕРґРёРЅ СЂР°Р· РЅР° РІС‹С…РѕРґРµ.
    pub(crate) frame_stats: lumen_paint::FrameStats,
    /// ADR-016 M2.0: СЃРµСЃСЃРёРѕРЅРЅС‹Р№ Р°РєРєСѓРјСѓР»СЏС‚РѕСЂ РІСЂРµРјРµРЅРё `relayout()` РЅР° UI-РїРѕС‚РѕРєРµ
    /// (СЃС‚РёР»СЊ + layout + СЃР±РѕСЂРєР° display-list + РґРѕСЃС‚Р°РІРєР° JS-observer'РѕРІ). РљР°Р¶РґС‹Р№
    /// РёРЅС‚РµСЂР°РєС‚РёРІРЅС‹Р№ relayout (DOM-РјСѓС‚Р°С†РёСЏ РёР· JS, hover/focus, resize, С‚РёРє
    /// Р°РЅРёРјР°С†РёРё, content-visibility) СЃРµРіРѕРґРЅСЏ Р±Р»РѕРєРёСЂСѓРµС‚ UI-РїРѕС‚РѕРє вЂ” СЌС‚Рѕ Рё РµСЃС‚СЊ С‚Р°
    /// СЂР°Р±РѕС‚Р°, РєРѕС‚РѕСЂСѓСЋ M2 СѓРЅРѕСЃРёС‚ РЅР° РѕС‚РґРµР»СЊРЅС‹Р№ engine-РїРѕС‚РѕРє. РќР°РїРѕР»РЅСЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ
    /// РїСЂРё РІРєР»СЋС‡С‘РЅРЅРѕРј `LUMEN_FRAME_LOG` (РєР°Рє `frame_stats`), СЃРІРѕРґРєР°
    /// `ENGINE_SUMMARY` РїРµС‡Р°С‚Р°РµС‚СЃСЏ РїРѕ РєР°РґР°РЅСЃСѓ `LUMEN_MEM_REPORT` Рё РѕРґРёРЅ СЂР°Р· РЅР°
    /// РІС‹С…РѕРґРµ вЂ” РґР°С‘С‚ before/after С‡РёСЃР»Р°, РЅР° РєРѕС‚РѕСЂС‹Рµ СЃРѕС€Р»СЋС‚СЃСЏ СЃР»РµРґСѓСЋС‰РёРµ СЃСЂРµР·С‹ M2.
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
    /// instrumentation (M3.2.0 вЂ” measure the real-content band mix before the GL
    /// blit path acts on it); the femtovg backend does not yet own a content
    /// surface, so normal runs pay nothing. Invalidated on navigation
    /// ([`Lumen::reset_to_blank_tab`]); resize/nav content changes also fall out
    /// naturally because the content hash folds surface size.
    pub(crate) scroll_cache: lumen_paint::ScrollCache,
    /// РЎРѕСЃС‚РѕСЏРЅРёРµ Ctrl+F. РћС‚РєСЂС‹С‚ Р»Рё bar, С‚РµРєСѓС‰РёР№ query Рё РёРЅРґРµРєСЃ Р°РєС‚РёРІРЅРѕРіРѕ
    /// СЃРѕРІРїР°РґРµРЅРёСЏ. РЎРѕРґРµСЂР¶РёРјРѕРµ РїРѕРёСЃРєР° РЅРµ СЃРѕС…СЂР°РЅСЏРµС‚СЃСЏ РјРµР¶РґСѓ reload-Р°РјРё
    /// (close() РїРѕР»РЅРѕСЃС‚СЊСЋ РѕС‡РёС‰Р°РµС‚ state); СЌС‚Рѕ СЃРѕР·РЅР°С‚РµР»СЊРЅРѕ: РїРѕСЃР»Рµ reload
    /// display list РґСЂСѓРіРѕР№, Рё СЃС‚Р°СЂС‹Рµ РїРѕР·РёС†РёРё СЃРѕРІРїР°РґРµРЅРёР№ СѓР¶Рµ РЅРµРІР°Р»РёРґРЅС‹.
    pub(crate) find: find::FindState,
    /// РЎРѕСЃС‚РѕСЏРЅРёРµ Ctrl+L Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРё. РћС‚РєСЂС‹С‚ Р»Рё Р±Р°СЂ Рё С‚РµРєСѓС‰РёР№ РІРІРѕРґ.
    /// Р—Р°РєСЂС‹РІР°РµС‚СЃСЏ РїСЂРё РЅР°РІРёРіР°С†РёРё (commit) Рё РїСЂРё Esc.
    pub(crate) address_bar: address_bar::AddressBarState,
    /// Click-hint overlay: vimium-style kbd-РЅР°РІРёРіР°С†РёСЏ РїРѕ РєР»РёРєР°Р±РµР»СЊРЅС‹Рј СЌР»РµРјРµРЅС‚Р°Рј.
    /// РћС‚РєСЂС‹РІР°РµС‚СЃСЏ РєР»Р°РІРёС€РµР№ F; Р·Р°РєСЂС‹РІР°РµС‚СЃСЏ Escape, СѓСЃРїРµС€РЅРѕР№ Р°РєС‚РёРІР°С†РёРµР№,
    /// РѕС‚РєСЂС‹С‚РёРµРј find/address bar РёР»Рё РїРµСЂРµС…РѕРґРѕРј РЅР° РґСЂСѓРіСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    pub(crate) hint: hints::HintState,
    /// РўРµРєСѓС‰РµРµ РІРµСЂС‚РёРєР°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ СЃС‚СЂР°РЅРёС†С‹ (CSS px). 0 вЂ” РІРµСЂС… РґРѕРєСѓРјРµРЅС‚Р°.
    /// Р Р°СЃС‚С‘С‚ РІРЅРёР·, РєР»Р°РјРїРёС‚СЃСЏ РІ `[0, max(0, content_height в€’ viewport_height)]`.
    /// РќР° load/reload СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ 0.
    pub(crate) scroll_y: f32,
    /// РўРµРєСѓС‰РµРµ РіРѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ СЃС‚СЂР°РЅРёС†С‹ (CSS px). 0 вЂ” Р»РµРІС‹Р№ РєСЂР°Р№.
    /// Р Р°СЃС‚С‘С‚ РІРїСЂР°РІРѕ, РєР»Р°РјРїРёС‚СЃСЏ РІ `[0, max(0, content_width в€’ viewport_width)]`.
    /// РќР° load/reload СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ 0.
    pub(crate) scroll_x: f32,
    /// `scroll_y` РїСЂРµРґС‹РґСѓС‰РµРіРѕ `RedrawRequested` вЂ” РґР»СЏ РѕС†РµРЅРєРё СЃРєРѕСЂРѕСЃС‚Рё СЃРєСЂРѕР»Р»Р°
    /// (fast-scroll РґРµРіСЂР°РґР°С†РёСЏ, EXPERIMENT.md В§2 СЃСЂРµР· 2).
    pub(crate) last_frame_scroll_y: f32,
    /// EMA-СЃРєРѕСЂРѕСЃС‚СЊ СЃРєСЂРѕР»Р»Р° РІ CSS px/РєР°РґСЂ (СЃРіР»Р°Р¶РёРІР°РµС‚ СЂР°Р·РѕРІС‹Рµ wheel-СЂС‹РІРєРё).
    pub(crate) scroll_velocity: f32,
    /// Р РµР¶РёРј Р±С‹СЃС‚СЂРѕРіРѕ СЃРєСЂРѕР»Р»Р°: С‚РёРєРё CSS-Р°РЅРёРјР°С†РёР№/GIF/video-GIF Р·Р°РјРѕСЂРѕР¶РµРЅС‹,
    /// РєРѕРЅС‚РµРЅС‚ scroll-СЃС‚Р°Р±РёР»РµРЅ, РєР°РґСЂС‹ СѓС…РѕРґСЏС‚ РІ page-compose HIT.
    pub(crate) fast_scroll: bool,
    /// РџРѕР»РЅР°СЏ РІС‹СЃРѕС‚Р° РєРѕРЅС‚РµРЅС‚Р° РІ CSS px вЂ” `max(rect.y + rect.height)` РїРѕ
    /// С‚РµРєСѓС‰РµРјСѓ display list-Сѓ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ load/reload. 0 вЂ” РЅРµС‚ РєРѕРЅС‚РµРЅС‚Р°.
    pub(crate) content_height: f32,
    /// РџРѕР»РЅР°СЏ С€РёСЂРёРЅР° РєРѕРЅС‚РµРЅС‚Р° РІ CSS px вЂ” `max(rect.x + rect.width)` РїРѕ
    /// С‚РµРєСѓС‰РµРјСѓ display list-Сѓ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ load/reload. 0 вЂ” РЅРµС‚ РєРѕРЅС‚РµРЅС‚Р°.
    pub(crate) content_width: f32,
    /// CSS Containment L3 В§4.4 (BB-4): `(node, top_y)` РїРѕРґРґРµСЂРµРІСЊРµРІ, РїСЂРѕРїСѓС‰РµРЅРЅС‹С…
    /// РїРѕСЃР»РµРґРЅРёРј layout-РїСЂРѕС…РѕРґРѕРј РёР·-Р·Р° `content-visibility: auto` РІРЅРµ СЂР°СЃС€РёСЂРµРЅРЅРѕРіРѕ
    /// viewport. top_y вЂ” СЃС‚СЂР°РЅРёС†Р°-РєРѕРѕСЂРґРёРЅР°С‚С‹ (scroll 0) СЃС…Р»РѕРїРЅСѓС‚РѕРіРѕ Р±РѕРєСЃР°.
    /// РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РІ `refresh_cv_state` РїРѕСЃР»Рµ РєР°Р¶РґРѕР№ СЃРјРµРЅС‹ `layout_box`.
    pub(crate) cv_skipped: Vec<(NodeId, f32)>,
    /// Ratchet-РЅР°Р±РѕСЂ auto-СѓР·Р»РѕРІ, СЃС‚Р°РІС€РёС… relevant (РІРѕС€Р»Рё РІ СЂР°СЃС€РёСЂРµРЅРЅС‹Р№ viewport
    /// РїСЂРё СЃРєСЂРѕР»Р»Рµ): РїСЂРѕРєРёРґС‹РІР°РµС‚СЃСЏ РІ layout С‡РµСЂРµР· `set_cv_relevant`, С‚Р°РєРёРµ СѓР·Р»С‹
    /// Р±РѕР»СЊС€Рµ РЅРµ РїСЂРѕРїСѓСЃРєР°СЋС‚СЃСЏ. РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РїСЂРё Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) cv_relevant: std::collections::HashSet<NodeId>,
    /// Skipped-СЃРѕСЃС‚РѕСЏРЅРёРµ **РєР°Р¶РґРѕРіРѕ** `content-visibility: auto` СѓР·Р»Р° РїСЂРѕС€Р»РѕРіРѕ
    /// РїСЂРѕС…РѕРґР° вЂ” Р±Р°Р·Р° РґРёС„С„Р° (BUG-852). РћС‚РґРµР»СЊРЅРѕ РѕС‚ `cv_skipped`, РєРѕС‚РѕСЂС‹Р№ РґРµСЂР¶РёС‚
    /// С‚РѕР»СЊРєРѕ РїСЂРѕРїСѓС‰РµРЅРЅС‹Рµ Рё С‚РѕР»СЊРєРѕ СЂР°РґРё ratchet-Р°: В«СѓР·Р»Р° РІ РєР°СЂС‚Рµ РЅРµС‚В» Рё В«СѓР·РµР» РЅРµ
    /// РїСЂРѕРїСѓС‰РµРЅВ» вЂ” СЂР°Р·РЅС‹Рµ РІРµС‰Рё, Рё РёРјРµРЅРЅРѕ РЅР° РїРµСЂРІРѕРј РґРµСЂР¶РёС‚СЃСЏ СЃРѕР±С‹С‚РёРµ РїРµСЂРІРѕРіРѕ
    /// РЅР°Р±Р»СЋРґРµРЅРёСЏ.
    pub(crate) cv_auto_state: std::collections::HashMap<NodeId, bool>,
    /// РћС‡РµСЂРµРґСЊ shell-СЃРѕР±С‹С‚РёР№ `ContentVisibilityChange` вЂ” РґРёС„С„С‹ skipped-СЃРѕСЃС‚РѕСЏРЅРёСЏ
    /// РјРµР¶РґСѓ layout-РїСЂРѕС…РѕРґР°РјРё. Р”СЂРµРЅРёСЂСѓРµС‚СЃСЏ СЂР°Р· РІ РєР°РґСЂ РІ `RedrawRequested` Рё
    /// СѓС…РѕРґРёС‚ РІ JS РєР°Рє `contentvisibilityautostatechange`. РљР°Рї 256 Р·Р°РїРёСЃРµР№.
    pub(crate) cv_events: Vec<ContentVisibilityChange>,
    /// OS-level `prefers-color-scheme` preference. `true` вЂ” СЃРёСЃС‚РµРјР° РІ С‚С‘РјРЅРѕР№ С‚РµРјРµ.
    /// Р§РёС‚Р°РµС‚СЃСЏ РёР· winit `Window::theme()` РїСЂРё СЃРѕР·РґР°РЅРёРё РѕРєРЅР° Рё РѕР±РЅРѕРІР»СЏРµС‚СЃСЏ РЅР°
    /// `WindowEvent::ThemeChanged`. РџСЂРѕРєРёРґС‹РІР°РµС‚СЃСЏ РІ JS `matchMedia` С‡РµСЂРµР·
    /// `deliver_media_query_changes(.., self.dark_mode)`. Default `false` (light)
    /// РґРѕ СЃРѕР·РґР°РЅРёСЏ РѕРєРЅР° Рё РІ headless/deterministic-СЂРµР¶РёРјР°С… (СЃС‚Р°Р±РёР»СЊРЅРѕСЃС‚СЊ snapshot-РѕРІ).
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
    /// РџРѕСЃР»РµРґРЅСЏСЏ РёР·РІРµСЃС‚РЅР°СЏ РїРѕР·РёС†РёСЏ РєСѓСЂСЃРѕСЂР° РІ **physical** РїРёРєСЃРµР»СЏС… (РѕС‚ winit).
    /// `None` РїРѕРєР° РєСѓСЂСЃРѕСЂ РЅРµ РІРѕС€С‘Р» РІ РѕРєРЅРѕ. РљРѕРЅРІРµСЂС‚РёСЂСѓРµС‚СЃСЏ РІ CSS px С‡РµСЂРµР·
    /// `scale_factor()` РЅРµРїРѕСЃСЂРµРґСЃС‚РІРµРЅРЅРѕ РІ hit-test / drag callback-Р°С….
    pub(crate) cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Ph3 pointer-events-l3: CSS-pixel `(x, y)` samples from `CursorMoved`
    /// queued since the last flush, in chronological order. Pointer Events
    /// Level 3 В§4.1 "coalesced events" вЂ” multiple raw OS samples can arrive
    /// before the next paint; `flush_pointer_moves` turns the whole batch
    /// into one `pointermove` dispatch with the rest exposed via
    /// `getCoalescedEvents()`. Flushed once per `about_to_wait` tick, or
    /// earlier вЂ” right before a hover-boundary crossing or `pointerdown`/
    /// `pointerup` вЂ” so event order stays chronological.
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
    /// РђРєС‚РёРІРЅС‹Р№ drag scrollbar-thumb-Р°: `Some` РїРѕРєР° Р·Р°Р¶Р°С‚Р° Р»РµРІР°СЏ РєРЅРѕРїРєР° РїРѕСЃР»Рµ
    /// click-Р° РїРѕ thumb-Сѓ. `MouseInput Released` РёР»Рё `CursorLeft` СЃР±СЂР°СЃС‹РІР°СЋС‚
    /// РІ `None`. РЎРЅР°РїС€РѕС‚ `(start_scroll_y, start_mouse_y)` С„РёРєСЃРёСЂРѕРІР°РЅ РЅР° РјРѕРјРµРЅС‚
    /// РЅР°С‡Р°Р»Р° drag-Р° вЂ” СЌС‚Рѕ РґР°С‘С‚ В«Р·Р°РєСЂРµРїР»С‘РЅРЅС‹Р№ РїРѕРґ РїР°Р»СЊС†РµРјВ» thumb (СЃС‚Р°РЅРґР°СЂС‚РЅС‹Р№
    /// scrollbar UX).
    pub(crate) scroll_drag: Option<scrollbar::ScrollDrag>,
    /// РђРєС‚РёРІРЅР°СЏ smooth-scroll Р°РЅРёРјР°С†РёСЏ РґР»СЏ keyboard / wheel / page-jump /
    /// find-scroll-to-match. `None` вЂ” `scroll_y` СЃС‚Р°С†РёРѕРЅР°СЂРµРЅ РёР»Рё РјРµРЅСЏРµС‚СЃСЏ
    /// РёРЅСЃС‚Р°РЅС‚РЅРѕ (drag, reload). РџСЂРё live-Р°РЅРёРјР°С†РёРё `RedrawRequested` С‚РёРєР°РµС‚
    /// РµС‘ С‡РµСЂРµР· `advance_scroll_anim` Рё РїСЂРѕСЃРёС‚ РµС‰С‘ РѕРґРёРЅ redraw РґРѕ Р·Р°РІРµСЂС€РµРЅРёСЏ.
    pub(crate) scroll_anim: Option<scroll_anim::ScrollAnim>,
    /// Momentum (kinetic) scroll: Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ РїСЂРё `TouchPhase::Ended` СЃ
    /// РЅРµРЅСѓР»РµРІРѕР№ СЃРєРѕСЂРѕСЃС‚СЊСЋ РѕС‚ С‚Р°С‡РїР°РґР°. РўРёРєР°РµС‚СЃСЏ С‡РµСЂРµР· `advance_momentum`
    /// РІ `RedrawRequested`. `None` вЂ” РЅРµС‚ Р°РєС‚РёРІРЅРѕР№ РёРЅРµСЂС†РёРё.
    pub(crate) momentum_anim: Option<momentum_anim::MomentumAnim>,
    /// РњРіРЅРѕРІРµРЅРЅР°СЏ СЃРєРѕСЂРѕСЃС‚СЊ С‚Р°С‡РїР°РґР° РѕС‚ РїРѕСЃР»РµРґРЅРёС… `PixelDelta`-СЃРѕР±С‹С‚РёР№
    /// (CSS px / ms). РћР±РЅРѕРІР»СЏРµС‚СЃСЏ EWMA-С„РёР»СЊС‚СЂРѕРј. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РїСЂРё
    /// `TouchPhase::Ended` РґР»СЏ Р·Р°РїСѓСЃРєР° `momentum_anim`.
    pub(crate) touchpad_vel: (f32, f32),
    /// Timestamp РїРѕСЃР»РµРґРЅРµРіРѕ `PixelDelta`-СЃРѕР±С‹С‚РёСЏ РґР»СЏ СЂР°СЃС‡С‘С‚Р° dt РІ EWMA.
    pub(crate) touchpad_vel_time_ms: f64,
    /// РџРѕСЃР»РµРґРЅРёР№ РІС‹СЃС‚Р°РІР»РµРЅРЅС‹Р№ cursor icon вЂ” С‡С‚РѕР±С‹ РїСЂРё РєР°Р¶РґРѕРј CursorMoved (Р° СЌС‚Рѕ
    /// СЃРѕС‚РЅРё СЃРѕР±С‹С‚РёР№ РІ СЃРµРєСѓРЅРґСѓ РїСЂРё Р°РєС‚РёРІРЅРѕРј РґРІРёР¶РµРЅРёРё РјС‹С€Рё) РЅРµ РґС‘СЂРіР°С‚СЊ
    /// `Window::set_cursor` РЅР°РїСЂР°СЃРЅРѕ. `None` вЂ” РµС‰С‘ РЅРµ РІС‹СЃС‚Р°РІР»СЏР»Рё (init).
    pub(crate) last_cursor_icon: Option<CursorIcon>,
    /// DOM + stylesheet РґР»СЏ relayout Р±РµР· РїРѕРІС‚РѕСЂРЅРѕРіРѕ fetch/parse. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ
    /// РїСЂРё РєР°Р¶РґРѕРј load/reload. `None` вЂ” СЃС‚СЂР°РЅРёС†Р° РЅРµ Р·Р°РіСЂСѓР¶РµРЅР° (Empty source).
    pub(crate) layout_source: Option<LayoutSource>,
    /// Р¤Р»Р°Рі В«РЅСѓР¶РЅРѕ reload РїРѕСЃР»Рµ С‚РµРєСѓС‰РµРіРѕ about_to_waitВ». РЈСЃС‚Р°РЅР°РІР»РёРІР°РµС‚СЃСЏ
    /// closure-РѕРј РІРЅСѓС‚СЂРё queue_task вЂ” СЌС‚Рѕ РµРґРёРЅСЃС‚РІРµРЅРЅС‹Р№ СЃРїРѕСЃРѕР± СЃРѕРѕР±С‰РёС‚СЊ
    /// Lumen-Сѓ РёР· task-closure (РєРѕС‚РѕСЂР°СЏ `+ 'static` Рё РЅРµ РІР»Р°РґРµРµС‚ `&mut self`).
    pub(crate) pending_reload: Rc<Cell<bool>>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ РѕС‚ JS (location.href=, assign, replace, reload),
    /// Р·Р°С…РІР°С‡РµРЅРЅС‹Р№ РІРѕ РІСЂРµРјСЏ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ СЃС‚СЂР°РЅРёС†С‹. РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚СЃСЏ
    /// РІ `about_to_wait` РїРѕСЃР»Рµ РїРµСЂРІРѕРіРѕ СЂРµРЅРґРµСЂР° Р·Р°РіСЂСѓР¶РµРЅРЅРѕР№ СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) pending_js_navigate: Option<JsNavigateRequest>,
    /// Proxy РґР»СЏ РѕС‚РїСЂР°РІРєРё LoadEvent РёР· background-РїРѕС‚РѕРєР° Р·Р°РіСЂСѓР·РєРё РІ event loop.
    pub(crate) load_proxy: EventLoopProxy<LoadEvent>,
    /// РРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅС‹Р№ HTML-РїР°СЂСЃРµСЂ вЂ” Р°РєС‚РёРІРµРЅ РІРѕ РІСЂРµРјСЏ streaming load.
    /// `None` РґРѕ РїРµСЂРІРѕРіРѕ HtmlChunk РёР»Рё РїРѕСЃР»Рµ LoadDone/LoadError.
    pub(crate) stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    /// РњРѕРјРµРЅС‚ РїРѕСЃР»РµРґРЅРµРіРѕ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅРѕРіРѕ РєР°РґСЂР° РїСЂРё streaming вЂ” РґР»СЏ throttling.
    pub(crate) stream_last_paint: std::time::Instant,
    /// CSS-С‚Р°Р±Р»РёС†Р° РёР· РїР°СЂР°Р»Р»РµР»СЊРЅС‹С… РїРѕС‚РѕРєРѕРІ Р·Р°РіСЂСѓР·РєРё CSS (PH1-2). РџСЂРёРјРµРЅСЏРµС‚СЃСЏ
    /// РІ `paint_partial_dom` РІРјРµСЃС‚Рѕ РїСѓСЃС‚РѕР№ С‚Р°Р±Р»РёС†С‹. РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РЅР° РєР°Р¶РґС‹Р№
    /// РЅРѕРІС‹Р№ СЃС‚СЂР°РЅРёС‡РЅС‹Р№ load.
    pub(crate) stream_sheet: lumen_css_parser::Stylesheet,
    /// PH1-2b: `true` РєРѕРіРґР° `layout_box` СЃРѕРґРµСЂР¶РёС‚ РґРµСЂРµРІРѕ, РїРѕСЃС‚СЂРѕРµРЅРЅРѕРµ РёР· С‚РµРєСѓС‰РµРіРѕ
    /// streaming-DOM (РІР°Р»РёРґРЅС‹Р№ РёСЃС‚РѕС‡РЅРёРє РґР»СЏ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ graft). `false` РІ
    /// РЅР°С‡Р°Р»Рµ РЅРѕРІРѕР№ РЅР°РІРёРіР°С†РёРё вЂ” РїРµСЂРІС‹Р№ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹Р№ РєР°РґСЂ РґРµР»Р°РµС‚ РїРѕР»РЅС‹Р№ layout Рё
    /// В«Р·Р°СЃРµРІР°РµС‚В» РґРµСЂРµРІРѕ; РїРѕСЃР»РµРґСѓСЋС‰РёРµ РєР°РґСЂС‹ СЂРµР»РµР№Р°СѓС‚СЏС‚ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕ.
    pub(crate) stream_layout_seeded: bool,
    /// URL subresource-С…РёРЅС‚РѕРІ, СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… РІ sink РІРѕ РІСЂРµРјСЏ streaming
    /// (`EarlyPreloadHints`). Р¤РёРЅР°Р»СЊРЅС‹Р№ `dispatch_preload_hints` РІ `LoadDone`
    /// РїСЂРѕРїСѓСЃРєР°РµС‚ URL РёР· СЌС‚РѕРіРѕ РЅР°Р±РѕСЂР° вЂ” Р±РµР· РґСѓР±Р»РµР№ РІ stderr Рё Р±РµР· РїРѕРІС‚РѕСЂРЅС‹С…
    /// fetch-С‚СЂРёРіРіРµСЂРѕРІ РїСЂРё СЂРµР°Р»СЊРЅРѕРј РїР°СЂР°Р»Р»РµР»СЊРЅРѕРј prefetch. РћС‡РёС‰Р°РµС‚СЃСЏ РІ РЅР°С‡Р°Р»Рµ
    /// РєР°Р¶РґРѕРіРѕ РЅРѕРІРѕРіРѕ СЃС‚СЂР°РЅРёС‡РЅРѕРіРѕ load.
    pub(crate) preload_dispatched: std::collections::HashSet<String>,
    /// PH1-2c: РєР»СЋС‡Рё `src` РєР°СЂС‚РёРЅРѕРє, СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… РІ background-РїРѕС‚РѕРєРё
    /// РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ РІРѕ РІСЂРµРјСЏ С‚РµРєСѓС‰РµРіРѕ streaming-load. Р”РµРґСѓРї РјРµР¶РґСѓ
    /// РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹РјРё РєР°РґСЂР°РјРё `paint_partial_dom`, С‡С‚РѕР±С‹ РєР°Р¶РґС‹Р№ `<img>`
    /// Р·Р°РіСЂСѓР¶Р°Р»СЃСЏ РѕРґРёРЅ СЂР°Р·. РћС‡РёС‰Р°РµС‚СЃСЏ РІ РЅР°С‡Р°Р»Рµ РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё.
    pub(crate) stream_images_requested: std::collections::HashSet<String>,
    /// BUG-735: intrinsic-СЂР°Р·РјРµСЂС‹ `src` в†’ `(width, height)` РІСЃРµС… РєР°СЂС‚РёРЅРѕРє,
    /// РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹С… streaming/РґРёРЅР°РјРёС‡РµСЃРєРёРј РїСѓС‚С‘Рј РІ С‚РµРєСѓС‰РµР№ РЅР°РІРёРіР°С†РёРё.
    /// РљР°СЂС‚Р° Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° РЅР°РІРёРіР°С†РёРё (Р° РЅРµ РґСЂРµРЅРёСЂСѓРµС‚СЃСЏ Р·Р° РїСЂРѕС…РѕРґ), РїРѕС‚РѕРјСѓ С‡С‚Рѕ
    /// `stream_images_requested` РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚ Р·Р°РїСЂРѕСЃ РїРѕ URL: СѓР·РµР» СЃ С‚РµРј Р¶Рµ
    /// `src`, РґРѕР±Р°РІР»РµРЅРЅС‹Р№ СЃРєСЂРёРїС‚РѕРј РїРѕР·Р¶Рµ, СЃРІРѕРµРіРѕ `ImageDecoded` СѓР¶Рµ РЅРµ РїРѕР»СѓС‡РёС‚,
    /// Рё СЂР°Р·РјРµСЂ РµРјСѓ РјРѕР¶РµС‚ РґР°С‚СЊ С‚РѕР»СЊРєРѕ СЌС‚Р° РєР°СЂС‚Р°.
    pub(crate) stream_image_sizes: HashMap<String, (u32, u32)>,
    /// BUG-735: РІ РєР°СЂС‚Сѓ [`Self::stream_image_sizes`] РїРѕРїР°Р» РЅРѕРІС‹Р№ СЂР°Р·РјРµСЂ вЂ”
    /// РЅР° Р±Р»РёР¶Р°Р№С€РµРј РєР°РґСЂРµ РЅСѓР¶РЅРѕ СЂР°Р·РЅРµСЃС‚Рё РµРіРѕ РїРѕ `<img>` Рё, РµСЃР»Рё DOM РёР·РјРµРЅРёР»СЃСЏ,
    /// СЃРґРµР»Р°С‚СЊ СЂРµР»РµР№Р°СѓС‚. Р¤Р»Р°Рі РєРѕР°Р»РµСЃС†РёСЂСѓРµС‚ РїР°С‡РєСѓ РґРµРєРѕРґРѕРІ (СЃРѕС‚РЅСЏ РєР°СЂС‚РёРЅРѕРє = РѕРґРёРЅ
    /// РїСЂРѕС…РѕРґ, Р° РЅРµ СЃРѕС‚РЅСЏ СЂРµР»РµР№Р°СѓС‚РѕРІ).
    pub(crate) stream_image_sizes_dirty: bool,
    /// U-1: scroll offset to restore once the in-flight navigation completes.
    /// Set by back/forward navigation before kicking off an async (streaming)
    /// reload; consumed in `apply_loaded_page` (and the sync fallback in
    /// `reload`) after the page resets scroll to the top. `None` for ordinary
    /// navigations (they stay at 0,0). Needed because navigation is no longer
    /// synchronous вЂ” the old code set `scroll_x/y` right after `reload()`
    /// returned, but the scroll reset now happens later, at `LoadEvent::LoadDone`.
    pub(crate) pending_restore_scroll: Option<(f32, f32)>,
    /// Bfcache (HTML LS В§8.6): `.persisted` flag for the `pageshow` event fired
    /// after the next page load completes. Set `true` by `navigate_back`/
    /// `navigate_forward` when the destination is restored from bfcache,
    /// consumed (and reset to `false`) in `apply_loaded_page` right after
    /// `notify_window_loaded`. `false` for ordinary fresh loads.
    pub(crate) pending_pageshow_persisted: bool,
    /// Same-document (`pushState`) state JSON + display URL to apply once an
    /// in-flight reload completes. Set by `navigate_back`/`navigate_forward`
    /// when a multi-step `history.go(n)` traversal (`navigate_by`) silently
    /// shuttled through a full-document entry before landing on a
    /// same-document entry вЂ” the currently loaded document is not the one
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
    /// BUG-757: СЂРµР°Р»СЊРЅР°СЏ Р±Р°Р·Р° С‚РµРєСѓС‰РµРіРѕ РґРѕРєСѓРјРµРЅС‚Р° Рё generation РЅР°РІРёРіР°С†РёРё, РІ
    /// РєРѕС‚РѕСЂРѕР№ РѕРЅР° РїРѕР»СѓС‡РµРЅР°. Р—Р°РїРѕР»РЅСЏРµС‚СЃСЏ, РєРѕРіРґР° СЃРµСЂРІРµСЂ СѓРІС‘Р» Р·Р°РїСЂРѕСЃ СЂРµРґРёСЂРµРєС‚РѕРј:
    /// `self.source` С…СЂР°РЅРёС‚ Р—РђРџР РћРЁР•РќРќР«Р™ Р°РґСЂРµСЃ, Рё РїРѕРґСЂРµСЃСѓСЂСЃС‹ С‡Р°СЃС‚РёС‡РЅРѕРіРѕ DOM
    /// (РєР°СЂС‚РёРЅРєРё, `@font-face`) СѓС…РѕРґРёР»Рё Р±С‹ РѕС‚ РЅРµРіРѕ. РџР°СЂР° СЃ generation РІРјРµСЃС‚Рѕ
    /// СЃР±СЂРѕСЃР° РЅР° РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё вЂ” СѓСЃС‚Р°СЂРµРІС€Р°СЏ Р±Р°Р·Р° РїСЂРѕСЃС‚Рѕ РїРµСЂРµСЃС‚Р°С‘С‚
    /// РїРѕРґС…РѕРґРёС‚СЊ (СЃРј. [`Self::document_resource_base`]).
    pub(crate) document_base: Option<(ResourceBase, u64)>,
    /// ADR-016 M2.2: РґРѕР»РіРѕР¶РёРІСѓС‰РёР№ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. `Some` С‚РѕР»СЊРєРѕ РїСЂРё
    /// `LUMEN_ENGINE_THREAD=1`; РёРЅР°С‡Рµ `None` Рё РїРѕРІРµРґРµРЅРёРµ shell РЅРµРёР·РјРµРЅРЅРѕ (РІРµСЃСЊ
    /// relayout СЃРёРЅС…СЂРѕРЅРЅС‹Р№). Р§РµСЂРµР· РЅРµРіРѕ РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµС‚СЃСЏ off-thread layout
    /// async-С‚СЂРёРіРіРµСЂРѕРІ (РїРѕРєР° вЂ” debounce-Р·СѓРј): `submit_relayout_job` С€Р»С‘С‚ Р·Р°РґР°РЅРёРµ,
    /// `poll_engine_commit` Р·Р°Р±РёСЂР°РµС‚ РіРѕС‚РѕРІС‹Р№ [`EngineCommit`]. Р”СЂРѕРї РїСЂРё Р·Р°РІРµСЂС€РµРЅРёРё
    /// С€Р»С‘С‚ `Shutdown` Рё РґР¶РѕР№РЅРёС‚.
    ///
    /// ADR-016 M2.2c-2b: РїРѕС‚РѕРє С‚Р°РєР¶Рµ РІР»Р°РґРµРµС‚ РїРµСЂСЃРёСЃС‚РµРЅС‚РЅС‹Рј СЃРѕСЃС‚РѕСЏРЅРёРµРј
    /// [`EngineJsState`] (`Document` + С…СЌРЅРґР» `js_ctx`) вЂ” СЃРёРґРµРЅСЊРµ РґР»СЏ РїРµСЂРµРЅРѕСЃР° JS РЅР°
    /// РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. Р—Р°РїРѕР»РЅСЏРµС‚СЃСЏ С‡РµСЂРµР· `sync_engine_js_state` РїСЂРё СЃРјРµРЅРµ СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) engine_thread: Option<engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    /// ADR-016 M2.2: generation РїРѕСЃР»РµРґРЅРµРіРѕ **РїСЂРёРјРµРЅС‘РЅРЅРѕРіРѕ** relayout-СЂРµР·СѓР»СЊС‚Р°С‚Р°.
    /// Off-thread Р·Р°РґР°РЅРёРµ СЃС‡РёС‚Р°РµС‚СЃСЏ В«РІ РїРѕР»С‘С‚РµВ» (РЅСѓР¶РµРЅ poll-Р±СѓРґРёР»СЊРЅРёРє), РїРѕРєР°
    /// `engine_job_generation != engine_applied_generation`. РЎРёРЅС…СЂРѕРЅРЅС‹Р№
    /// `relayout()` РІС‹СЃС‚Р°РІР»СЏРµС‚ РёС… СЂР°РІРЅС‹РјРё (off-thread Р·Р°РґР°РЅРёРµ РЅРµ Р¶РґС‘С‚СЃСЏ);
    /// `poll_engine_commit` РїСЂРѕРґРІРёРіР°РµС‚ СЌС‚Рѕ РїРѕР»Рµ РїСЂРёРјРµРЅС‘РЅРЅС‹Рј `commit.generation`.
    pub(crate) engine_applied_generation: u64,
    /// ADR-016 M2.2: РјРѕРЅРѕС‚РѕРЅРЅС‹Р№ РЅРѕРјРµСЂ async-relayout Р·Р°РґР°РЅРёСЏ. Р Р°СЃС‚С‘С‚ РїСЂРё РєР°Р¶РґРѕР№
    /// РїРѕСЃС‚Р°РЅРѕРІРєРµ off-thread Р·Р°РґР°РЅРёСЏ (`submit_relayout_job`) **Рё** РїСЂРё РєР°Р¶РґРѕРј
    /// СЃРёРЅС…СЂРѕРЅРЅРѕРј `relayout()` вЂ” С‚Р°Рє СЂРµР·СѓР»СЊС‚Р°С‚ СѓР¶Рµ РїРѕСЃС‚Р°РІР»РµРЅРЅРѕРіРѕ, РЅРѕ РµС‰С‘ РЅРµ
    /// РїСЂРёРјРµРЅС‘РЅРЅРѕРіРѕ off-thread Р·Р°РґР°РЅРёСЏ РѕРїРѕР·РЅР°С‘С‚СЃСЏ РєР°Рє СѓСЃС‚Р°СЂРµРІС€РёР№
    /// (`commit.generation != engine_job_generation`) Рё СЂРѕРЅСЏРµС‚СЃСЏ РІ
    /// `poll_engine_commit`. Latest-wins/generation-guard РЅР° СЃС‚РѕСЂРѕРЅРµ РїРѕС‚РѕРєР° вЂ”
    /// [`engine_thread`].
    pub(crate) engine_job_generation: u64,
    /// РўРµРєСѓС‰РёР№ IME preedit-С‚РµРєСЃС‚. `Some` вЂ” composition-СЃРµСЃСЃРёСЏ Р°РєС‚РёРІРЅР°,
    /// `None` вЂ” РЅРµС‚ Р°РєС‚РёРІРЅРѕРіРѕ IME РІРІРѕРґР°.
    pub(crate) ime_composing: Option<String>,
    /// In-memory bfcache вЂ” HTML snapshots keyed by URL for instant back/forward
    /// restoration without a network round-trip (HTML Living Standard В§8.6).
    pub(crate) bfcache: BfCache,
    /// Parsed stylesheets of frozen bfcache pages, keyed by URL.
    /// Kept shell-side because `Stylesheet` is not serializable.
    /// Pruned lazily against `bfcache.has_frozen`.
    pub(crate) frozen_styles: HashMap<String, lumen_css_parser::Stylesheet>,
    /// Pages kept alive (JS runtime included) for back/forward restoration,
    /// keyed by URL вЂ” see [`ParkedPage`]. Capped at [`PARKED_PAGES_MAX`];
    /// a `Vec` rather than a map because eviction is oldest-first.
    pub(crate) parked_pages: Vec<(String, ParkedPage)>,
    /// Navigation history stack вЂ” pages the user navigated away from.
    /// Top = most recent previous page.
    pub(crate) nav_back: Vec<NavEntry>,
    /// Forward history stack вЂ” pages the user went back from.
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
    /// HTML LS В§12.2 binds session storage to the browsing context: it must
    /// survive every navigation of this tab and never reach another one, so the
    /// map travels in the tab snapshot and is emptied for a newly opened tab вЂ”
    /// unlike `ls_storage`, nothing here is ever persisted.
    pub(crate) ss_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// Directory for per-origin IndexedDB SQLite files (`{sha256(eTLD+1)[:16]}.db`).
    /// `None` в†’ ephemeral in-memory store per page (headless / tests).
    /// `Some(dir)` в†’ each origin gets its own SQLite file in `dir`; data persists
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
    /// except the ephemeral Anonymous one вЂ” see [`Self::anonymous_cookie_jar`]
    /// and [`Self::active_cookie_jar`] (DS-16).
    pub(crate) cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Anonymous profile's own cookie jar (DS-16, В§9.3 ADR-020) вЂ” kept out of
    /// [`Self::cookie_jar`] so cookies set while browsing as Anonymous never
    /// leak into Personal/Work/Guest and vice versa. Reset to a fresh
    /// in-memory instance every time Anonymous becomes the active profile
    /// (`ProfileMenuHit::SwitchTo`), so it never carries state from a
    /// previous Anonymous session either вЂ” true ephemerality within the
    /// running process, not just isolation.
    pub(crate) anonymous_cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Live JS context for the current page вЂ” keeps event listeners active after
    /// initial script execution. `None` when `v8` feature is disabled or
    /// no scripts were registered. Must be dropped before `layout_source` on
    /// navigation to release Arc clones held in JS closures.
    ///
    /// ADR-016 M2.2c-2d (21): `Arc` (РЅРµ `Box`), РїРѕС‚РѕРјСѓ С‡С‚Рѕ С…СЌРЅРґР»РѕРј С‚РµРїРµСЂСЊ РІР»Р°РґРµРµС‚
    /// **Р»РёР±Рѕ** UI-СЃС‚РѕСЂРѕРЅР°, **Р»РёР±Рѕ** РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. РџРѕРґ С„Р»Р°РіРѕРј
    /// (`LUMEN_ENGINE_THREAD=1`) `Arc` Р¶РёРІС‘С‚ РІ [`EngineJsState::js`], Р° СЌС‚Рѕ РїРѕР»Рµ вЂ”
    /// `None`; Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) `Arc` Р·РґРµСЃСЊ, РєР°Рє РїСЂРµР¶РґРµ. Р’Р»Р°РґРµРЅРёРµ Р·Р°РґР°С‘С‚
    /// [`Self::set_js_ctx`], СЃРЅРёРјР°РµС‚ вЂ” [`Self::take_js_ctx`]. В«Р•СЃС‚СЊ Р»Рё JS?В» С‡РёС‚Р°Р№С‚Рµ
    /// РёР· [`Self::js_present`], Р° РЅРµ РёР· `self.js_ctx.is_some()`.
    pub(crate) js_ctx: Option<Arc<dyn PersistentJs>>,
    /// ADR-016 M2.2c-2d: UI-СЃС‚РѕСЂРѕРЅРЅРёР№ С„Р»Р°Рі В«Р°РєС‚РёРІРЅР°СЏ РІРєР»Р°РґРєР° РёРјРµРµС‚ JS-СЂР°РЅС‚Р°Р№РјВ»,
    /// СЃРѕРїСЂРѕРІРѕР¶РґР°СЋС‰РёР№ РєР°Р¶РґРѕРµ РїСЂРёСЃРІР°РёРІР°РЅРёРµ С…СЌРЅРґР»Р° (С‡РµСЂРµР· [`Self::set_js_ctx`] Рё
    /// snapshot save/restore). РћС‚РґРµР»СЏРµС‚ СЂРµС€РµРЅРёРµ В«РµСЃС‚СЊ Р»Рё JS?В» РѕС‚ С‚РѕРіРѕ, РєР°РєР°СЏ
    /// СЃС‚РѕСЂРѕРЅР° РґРµСЂР¶РёС‚ `Arc`: РіРµР№С‚С‹ (`if self.js_present`) С‡РёС‚Р°СЋС‚ РµРіРѕ РІРјРµСЃС‚Рѕ
    /// `self.js_ctx.is_some()`, РїРѕСЌС‚РѕРјСѓ РѕСЃС‚Р°СЋС‚СЃСЏ РІРµСЂРЅС‹ Рё РєРѕРіРґР° РїРѕРґ С„Р»Р°РіРѕРј СЃР°Рј `Arc`
    /// СѓРµС…Р°Р» РЅР° РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє (`state.js`), РѕСЃС‚Р°РІРёРІ `self.js_ctx == None`.
    pub(crate) js_present: bool,
    /// ADR-016 M2.3: UI-side lock-free clone of the JS runtime's rAF-pending
    /// flag (`Some` only when the active tab has a `v8` handle **and** the
    /// engine thread is enabled вЂ” the only mode that needs it). Read directly on
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
    /// Guards for PerformancePaintTiming entries (W3C Paint Timing В§2).
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
    /// Human-readable reason for `load_failed` (BUG-438) вЂ” the `LoadError`
    /// message or the final-render `Err`'s `Display`. Surfaced to
    /// `AutomationCommand::Wait{DocumentReady|NetworkIdle}` callers (BiDi
    /// `browsingContext.navigate`, MCP `wait`) as an `AutomationReply::Error`
    /// instead of the settled-error `Ack` BUG-308 used to send вЂ” a failed
    /// load must not be reported as a successful navigation. `None` whenever
    /// `load_failed` is `false`; reset together with it.
    pub(crate) load_error_message: Option<String>,
    /// Instant at which the current navigation began (set in `reload()`).
    /// Used to compute `duration` for the W3C Navigation Timing entry.
    pub(crate) nav_start: Option<std::time::Instant>,
    /// FTS5-РёРЅРґРµРєСЃ РїРѕ С‚РµРєСЃС‚Сѓ РїРѕСЃРµС‰С‘РЅРЅС‹С… СЃС‚СЂР°РЅРёС† вЂ” РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ omnibox (@history).
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    pub(crate) history_fts: HistoryFts,
    /// РҐСЂР°РЅРёР»РёС‰Рµ РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёС… Р·Р°РјРµС‚РѕРє (В§12.2) вЂ” omnibox `@notes <query>`.
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    pub(crate) notes_store: lumen_knowledge::Notes,
    /// РСЃС‚РѕСЂРёСЏ РїРѕРёСЃРєРѕРІС‹С… Р·Р°РїСЂРѕСЃРѕРІ РґР»СЏ prefix-match autocomplete РІ omnibox.
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    pub(crate) search_history: SearchHistory,
    /// РЎС‡С‘С‚С‡РёРє РґР»СЏ РіРµРЅРµСЂРёСЂРѕРІР°РЅРёСЏ rowid РїСЂРё РёРЅРґРµРєСЃРёСЂРѕРІР°РЅРёРё РІ history_fts.
    /// РРЅРєСЂРµРјРµРЅС‚РёСЂСѓРµС‚СЃСЏ РїСЂРё РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё РЅР° РЅРѕРІСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    pub(crate) next_history_id: i64,
    /// KnuthвЂ“Liang hyphenation provider вЂ” СЂРµР°Р»РёР·СѓРµС‚ CSS `hyphens: auto`.
    /// Lazy-loads per-locale dictionaries on first use; cached for subsequent layouts.
    /// `Arc`, С‡С‚РѕР±С‹ С„РёРЅР°Р»СЊРЅС‹Р№ pipeline (BUG-171 СЌС‚Р°Рї 2) РјРѕРі СЂР°Р·РґРµР»РёС‚СЊ РїСЂРѕРІР°Р№РґРµСЂ СЃ
    /// С„РѕРЅРѕРІС‹Рј СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєРѕРј Р±РµР· РїРѕС‚РµСЂРё РїСЂРѕРіСЂРµС‚РѕРіРѕ РєСЌС€Р° СЃР»РѕРІР°СЂРµР№.
    pub(crate) hyp_provider: Arc<KnuthLiangHyphenation>,
    /// Multi-frame GIF animations keyed by the same src URL used in `DrawImage`.
    /// Populated at image-load time; cleared on page navigation.
    /// Single-frame GIFs are not stored here вЂ” handled as regular static images.
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
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹.
    /// Р”РµСЂР¶Р°С‚ DOM+JS РґРµС‚РµР№; Р·Р°РјРµРЅСЏРµС‚СЃСЏ С†РµР»РёРєРѕРј РІ [`Lumen::apply_loaded_page`].
    /// Р’ PageSnapshot РЅРµ РїРѕРїР°РґР°РµС‚ вЂ” РїРѕСЃР»Рµ bfcache-РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ С„СЂРµР№РјС‹ Р±РµР·
    /// СЃРєСЂРёРїС‚РѕРІ (РёР·РІРµСЃС‚РЅРѕРµ РѕРіСЂР°РЅРёС‡РµРЅРёРµ СЃСЂРµР·Р° 1, СЃРј. bugs/BUG-480-OPEN.md).
    pub(crate) frames: Vec<FrameHandle>,
    /// BUG-480 срез 19: провайдеры, которыми загружены фреймы ТЕКУЩЕЙ
    /// страницы, — ими же грузит под-документ навигация фрейма.
    ///
    /// Едет из `LoadedPage` вместе с самими фреймами и заменяется в том же
    /// месте: набор принадлежит документу (его origin, его хранилища, его
    /// база), поэтому пережить смену страницы не может.
    pub(crate) frame_env: Option<crate::frames::FrameLoadEnv>,
    /// Shared GIF-video store вЂ” same Arc used by JS native bindings (PH3-12).
    ///
    /// The shell owns the Arc; JS bindings hold clones captured at context
    /// creation time.  The shell's render tick drains `pending_loads`, decodes
    /// GIFs, and re-registers frames under `"video:{nid}"` image keys.
    pub(crate) video_gif_store: std::sync::Arc<lumen_js::VideoGifStore>,
    /// Shared TextTrack store вЂ” same Arc used by JS native bindings (P3-webvtt).
    ///
    /// Mirrors `page_tracks.tracks_by_video` so `video.textTracks` exposes the
    /// shell's parsed `<track>` cues. Re-synced on load, cleared on navigation.
    pub(crate) text_track_store: std::sync::Arc<lumen_js::TextTrackStore>,
    /// CPU-side decoded image cache (ADR-008 В§10E.4 scroll-discard).
    ///
    /// Stores one `ImageHandle` per image URL so far-away images can be evicted
    /// from RAM on scroll without discarding the GPU texture in the renderer.
    /// Cleared and repopulated on every page load; entries are dropped by
    /// `try_discard_offscreen_images` once an image leaves the
    /// `gate_image_requests` zone (viewport В± 2 screens).
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
    /// starve the very state вЂ” network completions, JS ticks вЂ” the condition
    /// depends on), so a wait is queued here and re-checked once per frame in
    /// `about_to_wait` until it is satisfied or its deadline passes.
    pub(crate) pending_waits: Vec<PendingWait>,
    /// Receiver side of the input injection channel (ADR-007 В§8C).
    ///
    /// Drained each `about_to_wait`; commands are processed through the same
    /// hit-test / JS-dispatch path as real OS events.
    pub(crate) input_rx: input::InputReceiver,
    /// Sender side of the input injection channel вЂ” cloned for external callers.
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
    /// Lightweight identity for hibernated (T3) tabs вЂ” keyed by `TabEntry::id`.
    ///
    /// When a background tab is promoted to Hibernated its full `PageSnapshot`
    /// is evicted from `bg_tabs` and stored in `tab_snapshots`; only this
    /// cheap struct remains in RAM.
    pub(crate) hibernated_tabs: HashMap<usize, tab_lifecycle::TabMetadata>,
    /// SQLite-backed blob store for T3 DOM snapshots (ADR-008 В§10J).
    pub(crate) tab_snapshots: lumen_storage::TabSnapshotStore,
    /// SQLite-backed checkpoint store for T2 (BackgroundOld) tabs (ADR-008 В§10I).
    ///
    /// Written on every T1в†’T2 transition so scroll + form state survive a crash.
    /// Restored on T2в†’T0 when `bg_tabs` is empty (crash-recovery path).
    pub(crate) t2_store: lumen_storage::SleepingTabStore,
    /// Monotonic timestamp (ms since epoch) when a T2 SQLite restore started.
    ///
    /// `None` when no restore is in progress.  The `sleep_hint` overlay is shown
    /// once this exceeds 100 ms.
    pub(crate) t2_restore_start_ms: Option<f64>,
    /// SQLite-backed store for the last session вЂ” all open tabs at window close
    /// (В§10I). Overwritten wholesale on `CloseRequested`, read back on launch to
    /// reopen the previous set of tabs. On-disk at `session_persist::SESSION_DB_PATH`.
    pub(crate) session_store: lumen_storage::SessionStore,
    /// Lifecycle tier manager вЂ” tracks T0в†’T4 transitions and LRU ordering.
    ///
    /// Synced with `tab_strip` on open/switch/close; `tick_idle` is polled
    /// from `about_to_wait` once per second to drive automatic hibernation.
    pub(crate) lifecycle_mgr: tab_lifecycle::TabLifecycleManager,
    /// Monotonic instant of the last `tick_lifecycle` call вЂ” used to throttle
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
    /// Persistent workspace storage вЂ” SQLite in-memory during testing; wired to
    /// a disk path in production via `Workspaces::open(path)`.
    pub(crate) workspaces: lumen_storage::Workspaces,
    /// Profile switcher dropdown state (DS-14), anchored below the toolbar
    /// avatar button (`toolbar::avatar_x()`).
    pub(crate) profile_menu: panels::profile_menu::ProfileMenuPanel,
    /// Persistent profile registry (В§9.3, DS-14): profile metadata + which
    /// one is active. Opened from the portable data dir
    /// (`<exe_dir>/data/profiles.db`); first run seeds 4 default profiles
    /// (Р›РёС‡РЅС‹Р№/Р Р°Р±РѕС‡РёР№/РђРЅРѕРЅРёРјРЅС‹Р№/Р“РѕСЃС‚СЊ вЂ” `panels::profile_menu::DEFAULT_PROFILES`).
    /// DS-14 scope: only the active pointer and visual signature (avatar,
    /// chrome accent) are wired вЂ” per-profile data isolation is DS-16.
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
    /// page origin.  Each row has a toggle button cycling Ask в†’ Allow в†’ Deny.
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
    /// AI assistant sidebar panel (В§12.8, GG-1).
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
    /// Floating overlay showing a single user annotation (В§12.2, GG-2).
    ///
    /// Opened when the user selects a `@notes`-search result from the omnibox
    /// dropdown and presses Enter. The committed value (`note-viewer:<id>`)
    /// is intercepted in `handle_omnibox_commit`. `Escape` closes the overlay.
    pub(crate) note_viewer: panels::note_viewer::NoteViewerPanel,
    /// AI inference backend for the AI sidebar (В§12.8).
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
    /// menu ("Р’ РЅРѕРІСѓСЋ РіСЂСѓРїРїСѓ"). Membership is session state on `TabStrip`.
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
    /// "РћС‡РёСЃС‚РёС‚СЊ РІСЃС‘" button.
    pub(crate) history_panel: panels::history_panel::HistoryPanel,
    /// Command palette modal state (task #23, В§7E.2).
    ///
    /// `Ctrl+K` toggles a centred modal that fuzzy-searches across commands,
    /// bookmarks and history. While visible it captures all keyboard and pointer
    /// input; `в†‘/в†“` move the selection, `Enter` activates, `Esc` closes.
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
    /// `Ctrl+Shift+V` opens a compact 320Г—180 card that keeps a tab's `<video>`
    /// element visible (poster placeholder) while the page scrolls or the user
    /// switches tabs. Implemented as an in-window overlay (the ad-hoc panel
    /// convention) вЂ” a true second OS window awaits multi-window support. The
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
    /// is no in-window overlay fallback вЂ” window/backend creation failure just
    /// leaves the request unfulfilled (the JS `PictureInPictureWindow` promise
    /// still resolves; `.document` stays a JS-only mock either way, see
    /// `document_pip.rs`).
    pub(crate) doc_pip_os: Option<DocPipOsWindow>,
    /// Right-button drag gesture recognizer (В§7B.3).
    ///
    /// Tracks right-button drags, classifies the trajectory into L/R/U/D/LD/RD,
    /// and maps each direction to a [`GestureAction`] via a configurable
    /// [`GestureMap`].  Default bindings: Left=Back, Right=Forward,
    /// LeftDown=CloseTab, RightDown=NewTab.
    pub(crate) gesture: input::gesture::GestureRecognizer,
    /// SQLite-backed omnibox bang-alias registry (В§7B.4).
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
    /// Displayed nowhere yet вЂ” UI is a future task.
    pub(crate) notes: Vec<String>,
    /// В§12.3 Read-later storage: persists HTML snapshots of saved pages.
    ///
    /// Populated by the `@read-later <url>` omnibox command: a background thread
    /// fetches the page HTML and calls `save()`. In-memory only (no SQLite path
    /// for the first ship вЂ” drop-in replacement once a `read_later.db` path is
    /// wired through the profile directory).
    pub(crate) read_later_store: lumen_knowledge::ReadLater,
    /// В§12.3 Read-later panel state (Ctrl+Shift+R).
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
    /// Throttled OS memory pressure poller (ADR-008 В§10H).
    ///
    /// Polled every 5 s in `about_to_wait`.  On `Medium` or `High` pressure,
    /// [`CacheRegistry::broadcast_pressure`] is called on `cache_registry`, and
    /// owned caches (`image_cache`, renderer `layer_cache`) are evicted directly.
    pub(crate) memory_poll: memory_poll::MemoryPollTick,
    /// Registry of cross-session shared caches (ADR-008 В§10D.3).
    ///
    /// Caches registered here receive `on_memory_pressure` broadcasts from the
    /// poll loop.  Owned per-page caches (`image_cache`, layer cache) are evicted
    /// directly rather than through the registry to avoid shared-ownership overhead.
    pub(crate) cache_registry: lumen_core::ext::CacheRegistry,
    /// Deterministic render mode (8F).
    ///
    /// When `enabled` (`--deterministic` CLI flag): window opens at 1280Г—800
    /// (unless overridden by `viewport_override`, DEVX-1), `Date.now()` is
    /// frozen at 0, `Math.random` uses a seeded PRNG, and
    /// `requestAnimationFrame` callbacks receive a 0 ms timestamp.
    /// `rng_seed`/`monotonic_clock` (DEVX-16, `--rng-seed`/`--monotonic-clock`)
    /// reach the JS runtime via `V8JsRuntime::set_deterministic_mode`.
    /// Intended for snapshot testing and reproducible output.
    pub(crate) deterministic: deterministic::DetConfig,
    /// `--viewport <W>x<H>` override (DEVX-1): pins the window's CSS content
    /// viewport size, taking priority over both the `deterministic` 1280Г—800
    /// default and the plain 1024Г—720 default (see `resumed()`). Lets
    /// automation combine `--deterministic` with `graphic_tests`'s fixed
    /// 1024Г—720 crop-calibration contract.
    pub(crate) viewport_override: Option<(f32, f32)>,
    /// DevTools JS console panel (В§7E.5).
    ///
    /// Captures `console.log/warn/error` output from the active page's JS runtime.
    /// Visible as a bottom overlay; toggled with `F12`.
    pub(crate) devtools_console: devtools::console_panel::ConsolePanel,
    /// DevTools DOM inspector panel (В§7E.1).
    ///
    /// While active, hovering highlights the box under the cursor with a
    /// box-model overlay and clicking pins a node, showing its computed style
    /// in a right-docked side panel. Toggled with `Ctrl+Shift+I`.
    pub(crate) dom_inspector: devtools::inspector::DomInspectorPanel,
    /// DevTools network log panel (В§7E.4).
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
    /// A centred 300Г—260 px modal. Holds a working draft; on close the draft
    /// is persisted to `a11y_store` and media changes are re-delivered to JS.
    pub(crate) a11y_panel: panels::a11y_panel::A11yPanel,
    /// Platform accessibility bridge (O-5).
    ///
    /// Receives `AXTree` updates after every page load and focus change.
    /// Routes them to the OS accessibility API (UIA / NSAccessibility / AT-SPI2).
    pub(crate) platform_bridge: Box<dyn lumen_a11y::platform::PlatformBridge>,
    /// Print dialog overlay (task E-1, `Ctrl+P`).
    ///
    /// A centred 560Г—400 px modal with paper size, orientation, margins,
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
    /// stores other than `settings_store` (HTTP/3 в†’ `fingerprint.toml`,
    /// ad-block subscriptions в†’ `AdblockStore`, spellcheck locale в†’ `SPELL_DICTS`).
    pub(crate) settings_panel: panels::settings_panel::SettingsPanel,
    /// Persistent ad-block filter-list store (`<exe_dir>/data/adblock/adblock.db`).
    ///
    /// Opened once at startup ([`config::init_adblock`]); shared with the
    /// background refresh thread and with the settings panel's Adblock
    /// section (enable/disable a subscription, trigger a manual refresh).
    pub(crate) adblock_store: std::sync::Arc<lumen_storage::adblock::AdblockStore>,
    /// Keyboard shortcuts panel (Ctrl+Shift+/, В§D-4).
    ///
    /// Shows all `KeyCommand` bindings with rebind-on-click support.
    pub(crate) shortcuts_panel: panels::shortcuts_panel::ShortcutsPanel,
    /// Certificate viewer panel (Ctrl+Shift+C, В§D-1).
    ///
    /// Centred 500Г—440 overlay showing X.509 cert data (subject CN/Org, issuer,
    /// validity dates, SHA-256 fingerprint, SAN list, TLS version).
    pub(crate) cert_panel: panels::cert_panel::CertPanel,
    /// Whether the curated system-font fallback chain has been preloaded into
    /// the renderer (CSS Fonts L4 В§5.3 codepoint cascade).
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
    /// `history.replaceState`.  `None` в†’ use `source.url_str()`.
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
    /// Active CSS View Transition (CSS View Transitions L1 В§4).
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
    /// time вЂ” they gate which of width/height is updated during CursorMoved via the
    /// JS binding, so `resize: vertical` no longer also changes width on a diagonal drag.
    pub(crate) resize_active: Option<(lumen_dom::NodeId, f32, f32, bool, bool)>,
    /// In-progress tab drag-and-drop (В§O-9).
    ///
    /// `Some` from the moment the user presses on a tab until they release.
    /// Transitions to `active = true` after the cursor crosses
    /// [`tabs::strip::DRAG_THRESHOLD`] px.  On release, calls
    /// `tab_strip.move_tab` if the drag was active.
    pub(crate) tab_drag: Option<tabs::strip::TabDragState>,
    /// In-progress HTML5 drag-and-drop gesture (PH3-9 / HTML LS В§9.3.3).
    ///
    /// `Some` from `mousedown` on a draggable element until `mouseup`.
    /// Transitions to `active = true` after the cursor travels в‰Ґ
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
    /// Words the user chose to ignore for this session ("РџСЂРѕРїСѓСЃС‚РёС‚СЊ"),
    /// lowercase. Cleared on restart.
    pub(crate) spell_ignored: std::collections::HashSet<String>,
    /// Shell UI theme: base brightness + accent colour (В§O-9).
    ///
    /// Initialised from `BrowserSettings` on startup.  Updated when the user
    /// changes the theme or accent in the settings panel (Appearance section).
    /// The accent drives the active-tab indicator colour passed to
    /// `build_tab_bar`.
    pub(crate) shell_theme: panels::themes::ShellTheme,
    /// Original page source stored when Reader View (В§D-3) is active.
    ///
    /// `Some` when the current page is showing the clean reader HTML (F9 toggle);
    /// `None` in normal browsing mode.  Toggling F9 again restores this source.
    pub(crate) reader_original_source: Option<PageSource>,
    /// TLS certificate information for the current tab (В§D-1).
    ///
    /// Populated when a page loads over HTTPS; cleared on tab switch / navigation.
    /// Phase 0: shell can set this to a stub value via `CertInfo::stub_for`.
    pub(crate) cert_info: Option<panels::cert_panel::PanelCertData>,
}
