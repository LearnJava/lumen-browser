//! Relayout pipeline of the shell: page reflow, the rAF turn, the off-thread
//! layout job (ADR-016) and ownership of the page's JS context handle.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.

use crate::*;

impl Lumen {
    /// Р—Р°РјРµРЅСЏРµС‚ display list СЃС‚СЂР°РЅРёС†С‹, Р±Р°РјРїР°СЏ РµРіРѕ РІРµСЂСЃРёСЋ (BUG-405 СЃСЂРµР· 39).
    ///
    /// Р•РґРёРЅСЃС‚РІРµРЅРЅС‹Р№ СЃРїРѕСЃРѕР± РїСЂРёСЃРІРѕРёС‚СЊ [`Self::display_list`]: СЂРµРЅРґРµСЂРµСЂ СЂРµС€Р°РµС‚ РїРѕ
    /// РІРµСЂСЃРёРё, РјРѕР¶РЅРѕ Р»Рё РїРµСЂРµРёСЃРїРѕР»СЊР·РѕРІР°С‚СЊ СЃРІС‘СЂС‚РєСѓ РєР°РґСЂРѕРІС‹С… С…СЌС€РµР№, РїРѕСЌС‚РѕРјСѓ Р·Р°РїРёСЃСЊ
    /// РјРёРјРѕ СЌС‚РѕРіРѕ РјРµС‚РѕРґР° РїРѕРєР°Р·Р°Р»Р° Р±С‹ СѓСЃС‚Р°СЂРµРІС€РёРµ РїРёРєСЃРµР»Рё.
    pub(crate) fn set_display_list(&mut self, mut dl: DisplayList) {
        // BUG-480 срез 14: содержимое под-документов фреймов вклеивается на
        // КАЖДОЙ записи списка, а не один раз на загрузку — список страницы
        // пересобирается из layout при любом relayout и о фреймах не знает.
        // Метрики (`content_height_of`/`content_width_of`) считаются
        // вызывающей стороной ДО этого места и остаются метриками СТРАНИЦЫ:
        // они складываются по плоскому списку прямоугольников, без клипов, а
        // содержимое фрейма выше его бокса прокручивать страницу не должно.
        crate::frames::splice_frame_content(&mut dl, &self.frames);
        self.display_list = dl;
        self.bump_display_list_epoch();
    }

    /// Р‘Р°РјРїР°РµС‚ РІРµСЂСЃРёСЋ [`Self::display_list`] (BUG-405 СЃСЂРµР· 39).
    ///
    /// РћС‚РґРµР»СЊРЅРѕ РѕС‚ [`Self::set_display_list`] РґР»СЏ С‚СЂС‘С… РјРµСЃС‚, РіРґРµ Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёСЏ
    /// РЅРµ РґР°СЋС‚ РІР·СЏС‚СЊ `&mut self` С†РµР»РёРєРѕРј: РїСЂР°РІРєР° СЃРїРёСЃРєР° РЅР° РјРµСЃС‚Рµ Рё РґРІР° РјРµСЃС‚Р°,
    /// РіРґРµ `self.layout_source`/`self.layout_box` СѓР¶Рµ Р·Р°РЅСЏС‚С‹ вЂ” С‚Р°Рј РїРѕР»Рµ РїРёС€РµС‚СЃСЏ
    /// РЅР°РїСЂСЏРјСѓСЋ, Р° РІРµСЂСЃРёСЏ Р±Р°РјРїР°РµС‚СЃСЏ СЌС‚РёРј РІС‹Р·РѕРІРѕРј СЂСЏРґРѕРј.
    pub(crate) fn bump_display_list_epoch(&mut self) {
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
    }

    /// BUG-743: РїРµСЂРµСЃРѕР±СЂР°С‚СЊ РєР°СЃРєР°Рґ, РµСЃР»Рё РЅР°Р±РѕСЂ РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>` РёР·РјРµРЅРёР»СЃСЏ
    /// СЃ РїРѕСЃР»РµРґРЅРµР№ СЃР±РѕСЂРєРё. Р’РѕР·РІСЂР°С‰Р°РµС‚ `true`, РµСЃР»Рё Р»РёСЃС‚ Р·Р°РјРµРЅС‘РЅ.
    ///
    /// РўР°Р±Р»РёС†Р° СЃС‚РёР»РµР№ СЃС‚СЂР°РЅРёС†С‹ СЃРѕР±РёСЂР°РµС‚СЃСЏ РѕРґРёРЅ СЂР°Р· Р·Р° РЅР°РІРёРіР°С†РёСЋ вЂ” РЅР° СЌС‚Р°РїРµ
    /// СЂР°Р·Р±РѕСЂР°, СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРёРЅС…СЂРѕРЅРЅС‹С… СЃРєСЂРёРїС‚РѕРІ. Р’СЃС‘, С‡С‚Рѕ РІСЃС‚Р°РІР»СЏРµС‚
    /// `<style>` РїРѕР·Р¶Рµ (РѕР±СЂР°Р±РѕС‚С‡РёРє `load`, `setTimeout`, rAF, РїСЂРѕРјРёСЃ вЂ” С‚Рѕ РµСЃС‚СЊ
    /// Р»СЋР±РѕР№ CSS-in-JS), РґРѕ СЌС‚РѕРіРѕ РѕСЃС‚Р°РІР°Р»РѕСЃСЊ РІРЅРµ РєР°СЃРєР°РґР° РЅР°РІСЃРµРіРґР°. Р—РґРµСЃСЊ
    /// РґРµС€С‘РІС‹Р№ РѕС‚РїРµС‡Р°С‚РѕРє ([`inline_style_fingerprint`]) СЃРІРµСЂСЏРµС‚СЃСЏ РЅР° РєР°Р¶РґРѕРј
    /// СЂРµР»РµР№Р°СѓС‚Рµ, Р° РїРѕР»РЅР°СЏ РїРµСЂРµСЃР±РѕСЂРєР° (СЃРєР»РµР№РєР° РёР· [`DynamicCssBase`] + РїР°СЂСЃ)
    /// РїСЂРѕРёСЃС…РѕРґРёС‚ С‚РѕР»СЊРєРѕ РєРѕРіРґР° Р±Р»РѕРєРё РґРµР№СЃС‚РІРёС‚РµР»СЊРЅРѕ РёР·РјРµРЅРёР»РёСЃСЊ.
    ///
    /// РЎРµС‚СЊ РЅРµ С‚СЂРѕРіР°РµС‚СЃСЏ: `@import` РІРЅСѓС‚СЂРё *РЅРѕРІРѕРіРѕ* Р»РёСЃС‚Р° РѕСЃС‚Р°РЅРµС‚СЃСЏ
    /// РЅРµСЂР°Р·СЂРµС€С‘РЅРЅС‹Рј, `@font-face` РёР· РЅРµРіРѕ РЅРµ РїРѕРґРіСЂСѓР·РёС‚СЃСЏ вЂ” СЂРµР»РµР№Р°СѓС‚ РЅРµ РјРµСЃС‚Рѕ
    /// РґР»СЏ Р·Р°РіСЂСѓР·РѕРє. РћР±С‹С‡РЅС‹Р№ CSS-in-JS РЅРё С‚РѕРіРѕ, РЅРё РґСЂСѓРіРѕРіРѕ РЅРµ РёСЃРїРѕР»СЊР·СѓРµС‚.
    pub(crate) fn refresh_dynamic_css(&mut self) -> bool {
        let Some(src) = self.layout_source.as_mut() else {
            return false;
        };
        // Р Р°Р·РґРµР»СЊРЅС‹Рµ Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёСЏ РїРѕР»РµР№: `document` С‡РёС‚Р°РµС‚СЃСЏ, РїРѕРєР° `stylesheet`
        // Рё `dynamic_css` РґРµСЂР¶Р°С‚СЃСЏ РЅР° Р·Р°РїРёСЃСЊ.
        let LayoutSource { document, stylesheet, dynamic_css, .. } = src;
        let Some(base) = dynamic_css.as_mut() else {
            return false;
        };
        let Ok(doc) = document.lock() else {
            return false;
        };
        let fp = inline_style_fingerprint(&doc);
        if fp == base.inline_fp {
            return false;
        }
        let inline = extract_style_blocks(&doc);
        drop(doc);
        let mut css =
            String::with_capacity(base.imports_prefix.len() + inline.len() + base.linked.len());
        css.push_str(&base.imports_prefix);
        css.push_str(&inline);
        css.push_str(&base.linked);
        let sheet = lumen_css_parser::parse(&css);
        eprintln!(
            "CSS РїРµСЂРµСЃРѕР±СЂР°РЅ РїРѕСЃР»Рµ РїСЂР°РІРєРё <style>: {} РїСЂР°РІРёР»",
            sheet.rules.len()
        );
        *stylesheet = Arc::new(sheet);
        base.inline_fp = fp;
        // РРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅС‹Р№ СЂРµСЃС‚Р°Р№Р» (BUG-341 S7) РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµС‚ СЃС‚РёР»Рё РїСЂРѕС€Р»РѕРіРѕ
        // РїСЂРѕС…РѕРґР° вЂ” РїСЂРѕС‚РёРІ РЅРѕРІРѕРіРѕ Р»РёСЃС‚Р° РѕРЅРё РЅРµРґРµР№СЃС‚РІРёС‚РµР»СЊРЅС‹.
        self.page_prev_cascade_styles = None;
        true
    }

    /// РџРѕРІС‚РѕСЂРЅС‹Р№ layout+paint РїСЂРё РёР·РјРµРЅРµРЅРёРё СЂР°Р·РјРµСЂР° viewport.
    /// РСЃРїРѕР»СЊР·СѓРµС‚ СЃРѕС…СЂР°РЅС‘РЅРЅС‹Р№ `LayoutSource`; РїР°СЂСЃРёРЅРі РЅРµ РїРѕРІС‚РѕСЂСЏРµС‚СЃСЏ.
    pub(crate) fn relayout(&mut self) {
        self.refresh_dynamic_css();
        let Some(viewport) = self.relayout_viewport() else { return };
        // ADR-016 M2.2: a synchronous relayout is authoritative вЂ” advance the
        // applied generation to `job_generation` so any off-thread commit still
        // in flight (older generation) is dropped by `poll_engine_commit`'s
        // guard, and no poll-wakeup is armed for a job that no longer matters.
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        self.engine_applied_generation = self.engine_job_generation;
        // ADR-016 M2.0: time the whole UI-thread relayout (style + layout +
        // display-list build + JS-observer delivery) вЂ” the work M2 moves to an
        // engine thread. Only under `LUMEN_FRAME_LOG`, so a normal run pays
        // nothing. Recorded after `apply_relayout_result` so the state it reports
        // (display list / styled nodes) is the freshly-applied one.
        let engine_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
        let Some(src) = self.layout_source.as_ref() else { return };
        // Set interactive hover/focus/active state for this layout pass so that
        // :hover / :focus / :active / :focus-within CSS rules evaluate correctly.
        lumen_layout::set_interactive_state(self.hovered_nid, self.focused_node, self.active_nid);
        // Forced Colors Mode (CSS Color Adjust L1 В§3) вЂ” a11y preference drives
        // the forced system palette and the `(forced-colors: active)` media
        // feature for this layout pass.
        lumen_layout::set_forced_colors(self.a11y_store.forced_colors());
        // content-visibility: auto (BB-4) вЂ” relevance-РїСЂРѕРІРµСЂРєР° РїСЂРѕС‚РёРІ С‚РµРєСѓС‰РµРіРѕ
        // scroll-РїРѕР»РѕР¶РµРЅРёСЏ + ratchet-РЅР°Р±РѕСЂР°. РЎР±СЂРѕСЃ Рє РґРµС„РѕР»С‚Р°Рј РїРѕСЃР»Рµ РїСЂРѕС…РѕРґР°,
        // С‡С‚РѕР±С‹ layout РґСЂСѓРіРёС… РґРѕРєСѓРјРµРЅС‚РѕРІ (sidebar, С„РѕРЅРѕРІС‹Р№ РїР°СЂСЃ) РЅРµ СѓРЅР°СЃР»РµРґРѕРІР°Р»
        // С‡СѓР¶РѕР№ scroll/relevant.
        lumen_layout::set_cv_scroll(self.scroll_x, self.scroll_y);
        lumen_layout::set_cv_relevant(self.cv_relevant.clone());
        let (new_dl, lb) = relayout_page(src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        lumen_layout::clear_interactive_state();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        self.apply_relayout_result(new_dl, lb, viewport);
        if let Some(t0) = engine_t0 {
            let engine_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.engine_stats.record(engine_ms);
            eprintln!(
                "[engine] relayout {engine_ms:.2}ms dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
    }

    /// ADR-016 M2.2b: route an **async-safe chrome-inset relayout** off the UI
    /// thread when the engine thread is enabled, falling back to the synchronous
    /// [`Self::relayout`] otherwise (the default, so behavior is byte-identical
    /// unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// "Async-safe" means the caller changed only *chrome* geometry вЂ” a docked
    /// panel's side/width, the workspace bar, vertical/tree tabs, sidebar
    /// visibility, the AI / accessibility side panels (M2.2b-3), or a mouse-click
    /// *close* of the AI / sidebar / accessibility panels (M2.2b-6) вЂ” or triggered a
    /// whole-page *restyle* with no geometry read of its own (an OS/settings theme
    /// flip, M2.2b-4; an interactive `:hover`/`:active` pseudo-class flip, M2.2b-5,
    /// including the `:hover` clear on cursor-leave, M2.2b-8; a `:focus`/`:focus-within`
    /// change from a JS focus request or a click, M2.2b-7; a web-font FOUTв†’FOIT swap,
    /// M2.2b-8) вЂ” or opened the web sidebar's error-placeholder panel (M2.2b-8) вЂ”
    /// and is in either case **not** followed by a synchronous read
    /// of page layout geometry. The reflowed content may
    /// therefore land a few frames later via [`Self::poll_engine_commit`], the
    /// same contract as the debounced zoom (M2.2a). The chrome itself is drawn
    /// from its own state, so it updates on the immediately-requested redraw; only
    /// the page reflow underneath it is deferred.
    pub(crate) fn relayout_chrome(&mut self) {
        if !self.submit_relayout_job() {
            self.relayout();
        }
    }

    /// ADR-016 M2.2c-3: route an **async-safe form-control DOM-mutation relayout**
    /// off the UI thread when the engine thread is enabled, falling back to the
    /// synchronous [`Self::relayout`] otherwise (the default, so behavior is
    /// byte-identical unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// "Async-safe" here means the caller already mutated the shared layout
    /// `Document` (a checkbox/radio `checked` flip, a `<details>` open toggle, a
    /// range-slider value change, вЂ¦) directly on the UI thread and is **not**
    /// followed by a synchronous read of page layout geometry. The mutation is
    /// therefore visible in the immutable `Arc<Mutex<Document>>` snapshot the
    /// off-thread job captures, and the reflowed content lands a few frames later
    /// via [`Self::poll_engine_commit`] вЂ” the same contract as the debounced zoom
    /// (M2.2a) and the chrome-inset toggles ([`Self::relayout_chrome`], M2.2b).
    ///
    /// Sites that read geometry synchronously right after the mutation (caret
    /// placement, `scrollIntoView`, hit-test) cannot use this вЂ” they belong to the
    /// blocking-readback path (`EngineThread::readback`, M2.2c-1) instead.
    pub(crate) fn relayout_form(&mut self) {
        if !self.submit_relayout_job() {
            self.relayout();
        }
    }

    /// ADR-016 M4: incremental re-layout for rAF JS DOM mutations.
    ///
    /// Runs [`layout_mutation_incremental`] (full cascade + `graft_geometry` +
    /// incremental geometry pass + post-layout passes) reusing the retained
    /// `self.layout_box` as `prev`. Returns `true` on success and calls
    /// [`Self::apply_relayout_result`] (updates `self.display_list` /
    /// `self.layout_box` / scroll clamps). Returns `false` when no previous
    /// layout is available (first load) or when `layout_source` / viewport are
    /// not ready вЂ” the caller falls back to [`Self::relayout`].
    ///
    /// BUG-341 S7: when [`Self::page_prev_cascade_styles`] is `Some` (the last
    /// cycle to touch `self.layout_box` was this same restyle path) *and* the
    /// page-side JS DOM-mutation tracker ([`PersistentJs::take_dom_touched`])
    /// reports an attributed summary, this takes the incremental-cascade path
    /// ([`lumen_layout::box_tree::layout_mutation_incremental_restyle`])
    /// instead of the plain graft-only one вЂ” mirroring
    /// `Lumen::relayout_chrome_host`'s BUG-341 S6 wiring. `dirty_roots` unions
    /// the interactive-state delta (hover/focus/active, vs.
    /// `self.page_prev_interactive`) with the DOM-mutation delta
    /// (`touched.nodes`); `content_dirty` is `Nothing` only when `touched.nodes`
    /// is empty (a pure interactive-state cycle) and `Untracked` otherwise, the
    /// same precondition `RestyleDelta::content_dirty` documents. An `unattributed` summary
    /// (untracked mutation primitive вЂ” Shadow DOM attach, `execCommand`, вЂ¦) or a
    /// missing/invalidated cache falls back to today's `layout_mutation_incremental`
    /// (full cascade, still correct, just without the cascade-skip win).
    ///
    /// `self.layout_box` is **moved out** (not cloned) to avoid copying the
    /// potentially large tree; `apply_relayout_result` moves the fresh tree
    /// back in, so field is always `Some` after a successful call.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn try_relayout_raf_incremental(&mut self) -> bool {
        let Some(viewport) = self.relayout_viewport() else {
            return false;
        };
        // BUG-743: СЃРјРµРЅР° С‚Р°Р±Р»РёС†С‹ СЃС‚РёР»РµР№ РјРѕР¶РµС‚ Р·Р°РґРµС‚СЊ Р»СЋР±РѕР№ СѓР·РµР» РґРµСЂРµРІР° вЂ”
        // РіРµРѕРјРµС‚СЂРёСЋ РїСЂРѕС€Р»РѕРіРѕ РїСЂРѕС…РѕРґР° РїРµСЂРµРёСЃРїРѕР»СЊР·РѕРІР°С‚СЊ РЅРµР»СЊР·СЏ, РїСѓСЃС‚СЊ РІС‹Р·С‹РІР°СЋС‰РёР№
        // СЃРґРµР»Р°РµС‚ РїРѕР»РЅС‹Р№ [`Self::relayout`].
        if self.refresh_dynamic_css() {
            return false;
        }
        let Some(prev_lb) = self.layout_box.take() else {
            return false;
        };
        let Some(src) = self.layout_source.as_ref() else {
            self.layout_box = Some(prev_lb);
            return false;
        };
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        self.engine_applied_generation = self.engine_job_generation;
        lumen_layout::set_interactive_state(self.hovered_nid, self.focused_node, self.active_nid);
        lumen_layout::set_forced_colors(self.a11y_store.forced_colors());
        lumen_layout::set_cv_scroll(self.scroll_x, self.scroll_y);
        lumen_layout::set_cv_relevant(self.cv_relevant.clone());
        let new_interactive = (self.hovered_nid, self.focused_node, self.active_nid);
        let touched = self.js_ctx.as_ref().map(|js| js.take_dom_touched()).unwrap_or_default();
        // BUG-341 S19: the two paths are one `if`/`else` rather than an
        // `Option` plus a `match` because the restyle path now *consumes*
        // `prev_lb` (it moves the reusable subtrees straight into the fresh
        // tree instead of copying them), and only this shape lets the compiler
        // see that the fallback below runs exactly when the move did not.
        let (new_dl, new_lb, fresh_cascade_styles) = if !touched.unattributed
            && let Some(prev_styles) = self.page_prev_cascade_styles.take()
        {
            let (prev_hover, prev_focus, prev_active) = self.page_prev_interactive;
            let doc = src.document.lock().unwrap();
            // BUG-341 S7: computed once per pass, reused across all three axes.
            let state_index = lumen_layout::style::restyle_state_index(&doc, &src.stylesheet);
            let mut dirty_roots = std::collections::HashSet::new();
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_hover, new_interactive.0, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_focus, new_interactive.1, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_active, new_interactive.2, &state_index,
            ));
            // BUG-341 S17: `DomTouched` records node ids without attribute
            // names, so every page-side mutation stays `Unattributed` вЂ” the
            // pre-S17 widen-to-parent behaviour, unchanged.
            let node_index = lumen_layout::style::restyle_node_index(&doc, &src.stylesheet);
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                &doc,
                touched.nodes.iter().map(|&n| (n, lumen_layout::style::NodeChange::Unattributed)),
                &node_index,
            ));
            drop(doc);
            // BUG-341 S16: the page-side tracker reports *selector-relevant*
            // nodes only (`DomTouched` deliberately says nothing about text
            // writes) and has an `unattributed` escape hatch, so it cannot
            // claim a complete per-node content record the way
            // `bind_model_tracked` can. Anything but "nothing touched at all"
            // must therefore stay `Untracked` вЂ” this is exactly S4's
            // `dom_content_stable` semantics, unchanged. Giving the page path a
            // real content set means completing `DomTouched` for content first.
            let content_dirty = if touched.nodes.is_empty() {
                lumen_layout::counters::ContentDirty::Nothing
            } else {
                lumen_layout::counters::ContentDirty::Untracked
            };
            let delta = lumen_layout::counters::RestyleDelta { prev_styles, dirty_roots, content_dirty };
            lumen_layout::counters::set_incremental_restyle(true);
            // BUG-341 S15 вЂ” see the twin call in `relayout_chrome_host`: the
            // box-build reuse rides on the same content precondition computed
            // just above.
            lumen_layout::box_tree::set_incremental_box_build(true);
            let (dl, lb, counters) = relayout_page_incremental_restyle(
                src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts, prev_lb, delta,
            );
            lumen_layout::box_tree::set_incremental_box_build(false);
            lumen_layout::counters::set_incremental_restyle(false);
            (dl, lb, Some(counters.into_styles()))
        } else {
            let (dl, lb) = relayout_page_incremental(
                src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts, &prev_lb,
            );
            (dl, lb, None)
        };
        lumen_layout::clear_interactive_state();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        self.apply_relayout_result(new_dl, new_lb, viewport);
        // `apply_relayout_result` unconditionally clears the cache вЂ” restore it
        // here, after `lb` has already landed in `self.layout_box`, only when
        // this cycle actually produced a matching one.
        if let Some(styles) = fresh_cascade_styles {
            self.page_prev_cascade_styles = Some(styles);
            self.page_prev_interactive = new_interactive;
        }
        true
    }

    /// ADR-016 M2.2c-3: route the **async-safe rAF DOM-dirty flush** off the UI
    /// thread when the engine thread is enabled, falling back to the synchronous
    /// [`Self::relayout`] otherwise (the default, so behavior is byte-identical
    /// unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// This is the `about_to_wait` rAF pump: a `requestAnimationFrame` callback ran
    /// (engine-side under the flag) and mutated the DOM, so the shared
    /// `Arc<Mutex<Document>>` already carries the mutation the off-thread job's
    /// snapshot will observe (invariant 1). The caller only requests a redraw
    /// afterwards вЂ” it does **not** read page geometry synchronously вЂ” so the
    /// reflow may land a few frames later via [`Self::poll_engine_commit`], the same
    /// async contract as the debounced zoom (M2.2a) and the form-input toggles
    /// ([`Self::relayout_form`]). The `RedrawRequested` counterpart *does* read a
    /// layout product synchronously (Step 5 PerformancePaintTiming) and therefore
    /// uses the blocking [`Self::readback_relayout_job`] path instead.
    ///
    /// ADR-016 M4: when the engine thread is present (default since ADR-023),
    /// [`Self::submit_relayout_job`] (full, off-thread) wins and the incremental
    /// path below is never reached (BUG-935). In the single-thread fallback path,
    /// tries the incremental layout ([`Self::try_relayout_raf_incremental`])
    /// before the full [`Self::relayout`].
    pub(crate) fn relayout_raf_dirty(&mut self) {
        if !self.submit_relayout_job() && !self.try_relayout_raf_incremental() {
            self.relayout();
        }
    }

    /// ADR-016 M2.3: `true` while a `run_animation_frame` batch dispatched to the
    /// engine thread has not yet completed (engine thread present + inflight flag
    /// set). While inflight the UI thread must not enqueue new blocking JS work вЂ”
    /// it would serialize the winit thread behind the (possibly 200 ms) turn,
    /// freezing scroll. Always `false` off the flag (no engine thread).
    pub(crate) fn raf_turn_inflight(&self) -> bool {
        self.engine_thread.is_some()
            && self
                .raf_task_inflight
                .load(std::sync::atomic::Ordering::Acquire)
    }

    /// ADR-016 M2.3: consume (clear + return) the rAF-pending flag lock-free via
    /// the cached UI-side atomic. `false` when no flag is cached (JS-less tab /
    /// off the flag). No engine `query`, so it never blocks behind an in-flight
    /// turn вЂ” unlike [`route_query_js`]`(вЂ¦ take_raf_pending)`.
    pub(crate) fn take_raf_pending_lockfree(&self) -> bool {
        self.raf_pending_flag
            .as_ref()
            .is_some_and(|f| f.swap(false, std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: value-returning JS drain that is **deferred** (returns
    /// `None`) while a rAF turn is in flight on the engine thread. The parked
    /// `about_to_wait` loop issues several blocking `route_query_js` drains each
    /// pass (canvas bitmaps, history/pushState, traversals, navigation updates);
    /// under the flag every one of them would otherwise FIFO-serialize behind the
    /// in-flight (up to ~200 ms) `run_animation_frame` task and freeze the loop вЂ”
    /// exactly the stall M2.3 removes. Skipping a drain merely defers it to the
    /// next pass after the turn finishes (the short rAF wakeup keeps the loop
    /// warm). Off the flag `raf_turn_inflight()` is always `false`, so this is
    /// byte-identical to calling [`route_query_js`] directly.
    pub(crate) fn drain_query_js<R: Send + 'static>(
        &self,
        read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
    ) -> Option<R> {
        if self.raf_turn_inflight() {
            return None;
        }
        route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), read)
    }

    /// ADR-016 M2.3: non-consuming peek at the rAF-pending flag lock-free (the
    /// [`Self::take_raf_pending_lockfree`] counterpart of `has_raf_pending`).
    /// Used to decide the next parked-loop wakeup without clearing the signal.
    pub(crate) fn raf_pending_lockfree(&self) -> bool {
        self.raf_pending_flag
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: consume (clear + return) the DOM-dirty flag lock-free via the
    /// cached UI-side atomic (companion to [`Self::take_raf_pending_lockfree`]).
    pub(crate) fn take_dom_dirty_lockfree(&self) -> bool {
        self.dom_dirty_flag
            .as_ref()
            .is_some_and(|f| f.swap(false, std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: dispatch one `run_animation_frame(raf_ts)` batch to the
    /// engine thread as a **non-blocking** `task`, marking `raf_task_inflight`
    /// for its whole duration so the scroll/redraw path presents the retained
    /// display list (and skips the JS pump) until it finishes. The caller must
    /// have already consumed the pending flag and updated `last_raf_batch_ms`.
    /// Only reached under `LUMEN_ENGINE_THREAD=1` (engine thread present).
    pub(crate) fn fire_raf_turn_async(&self, raf_ts: f64) {
        let Some(engine) = self.engine_thread.as_ref() else {
            return;
        };
        let inflight = Arc::clone(&self.raf_task_inflight);
        inflight.store(true, std::sync::atomic::Ordering::Release);
        engine.task(move |state| {
            if let Some(js) = &state.js {
                js.run_animation_frame(raf_ts);
            }
            inflight.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// ADR-016 M2.3: engine-thread rAF pump step shared by `RedrawRequested`
    /// Step 3.1/4 and the `about_to_wait` parked pump. Runs **only** under the
    /// flag (`self.engine_thread.is_some()`); the single-thread path keeps its
    /// original synchronous sequence for byte-identical behavior.
    ///
    /// Non-blocking by construction: (1) when a rAF batch is due and none is
    /// already running, consume the pending flag lock-free and fire the turn
    /// async ([`Self::fire_raf_turn_async`]); (2) when no turn is running,
    /// consume the DOM-dirty flag lock-free and, if a completed turn mutated the
    /// DOM, submit an **async** relayout ([`Self::relayout_raf_dirty`]) whose
    /// result lands via [`Self::poll_engine_commit`]. Neither step issues a
    /// blocking engine `query`, so the winit thread never stalls behind the JS
    /// turn and scroll stays smooth. Returns `true` if a relayout was submitted
    /// (caller requests a redraw).
    pub(crate) fn pump_raf_engine_thread(&mut self, raf_due: bool, timestamp_ms: f64) -> bool {
        // A turn still running hasn't finished its DOM mutations and holds the
        // engine FIFO вЂ” leave both the dirty check and the next fire to a later
        // pass (the flag is not cleared, so the pending signal survives).
        if self.raf_turn_inflight() {
            return false;
        }
        // Consume a completed turn's DOM mutations first (before any re-fire) so a
        // continuous rAF-DOM loop still relayouts each cycle.
        let mut submitted = false;
        if self.take_dom_dirty_lockfree() {
            self.relayout_raf_dirty();
            submitted = true;
        }
        // Drain gate: the first non-inflight pass after a turn completes is
        // reserved for the deferred `drain_query_js` queues (which run this pass,
        // engine now free) вЂ” hold off firing the next turn until the following
        // pass so a continuous rAF loop can't starve notifications/popups/console.
        if self.raf_drain_gate {
            self.raf_drain_gate = false;
            return submitted;
        }
        if raf_due && self.take_raf_pending_lockfree() {
            self.last_raf_batch_ms = timestamp_ms;
            let raf_ts = if self.deterministic.enabled { 0.0 } else { -1.0 };
            self.fire_raf_turn_async(raf_ts);
            self.raf_drain_gate = true;
        }
        submitted
    }

    /// ADR-016 M2.2c-3: route the **rAF DOM-dirty flush that is followed by a
    /// synchronous read of a layout product** off the UI thread via the blocking
    /// request/reply [`engine_thread::EngineThread::readback`], falling back to the
    /// synchronous [`Self::relayout`] otherwise (the default, so behavior is
    /// byte-identical unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// This is the `RedrawRequested` Step 4 site: a `requestAnimationFrame` callback
    /// mutated the DOM and the very next Step 5 reads `self.display_list.is_empty()`
    /// to latch PerformancePaintTiming (W3C Paint Timing В§2). That read must see the
    /// freshly-reflowed display list, so вЂ” unlike the async [`Self::relayout_form`] /
    /// [`Self::relayout_raf_dirty`] вЂ” the relayout cannot be deferred to a later
    /// commit. [`Self::readback_relayout_job`] computes the layout **on the engine
    /// thread** (which owns the mutable `Document` + `js_ctx` under the flag) and
    /// blocks for exactly that one commit, applying it synchronously so Step 5 sees
    /// the current display list.
    ///
    /// ADR-016 M4: in the single-thread fallback path, tries the incremental layout
    /// ([`Self::try_relayout_raf_incremental`]) before the full [`Self::relayout`].
    pub(crate) fn relayout_raf_dirty_readback(&mut self) {
        if !self.readback_relayout_job() && !self.try_relayout_raf_incremental() {
            self.relayout();
        }
    }

    /// Derive the CSS layout viewport for a relayout (shared by the synchronous
    /// [`Self::relayout`] and the off-thread [`Self::submit_relayout_job`]).
    ///
    /// Returns `None` вЂ” skip relayout вЂ” when there is no `LayoutSource`/renderer
    /// yet or the content region is degenerate (minimized window). Applies the
    /// live chrome inset (RP-2), `<meta viewport initial-scale>` and the user
    /// zoom, matching scroll clamping and the content `PushTransform`.
    pub(crate) fn relayout_viewport(&self) -> Option<Size> {
        let src = self.layout_source.as_ref()?;
        let r = self.renderer.as_ref()?;
        let vp_size = r.viewport_size();
        // RP-2: lay out against the live page content region, not the full
        // window. In an interactive window the page sits below the tab strip
        // (+ workspace switcher), so the layout viewport must exclude that
        // chrome to match scroll clamping (`viewport_height_css`) and the
        // PushTransform that shifts content down. Headless surfaces have no
        // chrome and use the full surface. Tracks live `inner_size` because
        // `viewport_size()` reflects the last `r.resize()` on `Resized`.
        let (vp_w, vp_h) =
            content_layout_viewport(vp_size, self.window.is_some(), self.workspace_panel.visible);
        // Guard against degenerate viewport (renderer not yet configured or minimized).
        if vp_w <= 0.0 || vp_h <= 0.0 {
            return None;
        }
        // Apply <meta viewport initial-scale> + user zoom to derive the CSS layout viewport.
        let meta_scale = meta_initial_scale(src);
        let (css_w, css_h) = zoom::effective_viewport(vp_w, vp_h, meta_scale, self.zoom_factor);
        Some(Size::new(css_w, css_h))
    }

    /// ADR-016 M2.2: post-layout UI-thread work shared by the synchronous
    /// [`Self::relayout`] and the off-thread commit path
    /// ([`Self::poll_engine_commit`]). Takes an already-computed
    /// `(DisplayList, LayoutBox)` (built either inline or on the engine thread)
    /// and applies everything that touches `&mut self`: caches, transitions /
    /// `@starting-style` sync, `will-change` layer promotion, zoom-preview reset,
    /// scroll clamping and JS-observer delivery. Kept identical for both callers
    /// so an off-thread relayout is byte-for-byte equivalent to a synchronous one.
    pub(crate) fn apply_relayout_result(&mut self, mut new_dl: DisplayList, lb: lumen_layout::LayoutBox, viewport: Size) {
        // BUG-480 СЃСЂРµР· 13: РєРѕРЅС‚РµРЅС‚РЅС‹Р№ РІСЊСЋРїРѕСЂС‚ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚РѕРІ СЃР»РµРґСѓРµС‚ Р·Р°
        // СЂР°Р·РјРµСЂРѕРј РёС… host-Р±РѕРєСЃР° вЂ” Р·РЅР°С‡РёС‚ Р·Р° РєР°Р¶РґС‹Рј relayout (СЂРµСЃР°Р№Р·, Р·СѓРј,
        // Р»СЋР±РѕРµ РґРІРёР¶РµРЅРёРµ РІС‘СЂСЃС‚РєРё РЅР°Рґ С„СЂРµР№РјРѕРј). РџСЂРѕС…РѕРґ СЃР°Рј РіРµР№С‚РёС‚СЃСЏ РЅР°
        // В«СЂР°Р·РјРµСЂ РЅРµ РјРµРЅСЏР»СЃСЏВ» Рё РЅР° РїСѓСЃС‚РѕРј СЃРїРёСЃРєРµ С„СЂРµР№РјРѕРІ СЃС‚РѕРёС‚ РЅРѕР»СЊ. Р”Рћ
        // Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёСЏ `layout_source`: С‚Р°Рј Р±РµСЂС‘С‚СЃСЏ `&self` РЅР° РІСЃСЋ С„СѓРЅРєС†РёСЋ.
        let frame_state = self.frame_interactive();
        crate::frames::sync_frame_viewports(&mut self.frames, &lb, frame_state);
        let Some(src) = self.layout_source.as_ref() else { return };
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        // BUG-480 срез 14: вклейка ПОСЛЕ метрик (содержимое фрейма не должно
        // растягивать прокрутку страницы — обе функции складывают плоский
        // список прямоугольников и клипов не видят) и ДО diff/кэша, чтобы обе
        // стороны сравнения были одинаково склеенными.
        crate::frames::splice_frame_content(&mut new_dl, &self.frames);
        self.tile_grid.update_from_diff(&self.display_list, &new_dl);
        // Cache display list directly (avoid &mut self while layout_source is borrowed).
        let _dl_hash = lumen_paint::hash_commands(&new_dl);
        self.display_list_cache.insert(lb.node.index() as u32, new_dl.clone(), _dl_hash, None);
        // РџРѕР»СЏ РїРёС€СѓС‚СЃСЏ РЅР°РїСЂСЏРјСѓСЋ (РЅРµ С‡РµСЂРµР· `set_display_list`): `layout_source`
        // Р·РґРµСЃСЊ Р·Р°РёРјСЃС‚РІРѕРІР°РЅ, `&mut self` С†РµР»РёРєРѕРј РІР·СЏС‚СЊ РЅРµР»СЊР·СЏ.
        self.display_list = new_dl;
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
        // Sync transitions: compare prev styles with new layout before replacing.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = HashMap::new();
        collect_box_styles(&lb, &mut new_styles);
        for (node, new_style) in &new_styles {
            if let Some(old_style) = self.prev_styles.get(node) {
                self.transition_scheduler.sync(*node, old_style, new_style, now_s);
            }
        }
        // @starting-style (CSS Transitions L2 В§3.4): newly visible nodes (not in
        // prev_styles) use @starting-style rules as the before-change style so that
        // entry transitions start from the declared starting values.
        if !src.stylesheet.starting_style_rules.is_empty() {
            let entering: Vec<NodeId> = new_styles
                .keys()
                .filter(|n| !self.prev_styles.contains_key(*n))
                .copied()
                .collect();
            if !entering.is_empty() {
                let mut entry_styles: Vec<(NodeId, ComputedStyle)> = Vec::new();
                if let Ok(doc) = src.document.lock() {
                    for node in &entering {
                        if let Some(decls) =
                            resolve_starting_style(*node, &doc, &src.stylesheet)
                        {
                            entry_styles.push((
                                *node,
                                compute_style_from_declarations(&decls, viewport),
                            ));
                        }
                    }
                }
                // MutexGuard dropped вЂ” apply entry transitions outside the lock.
                for (node, starting_style) in &entry_styles {
                    if let Some(new_style) = new_styles.get(node) {
                        self.transition_scheduler.sync(
                            *node,
                            starting_style,
                            new_style,
                            now_s,
                        );
                    }
                }
            }
        }
        self.prev_styles = new_styles;
        // BUG-341 S7: invalidate the restyle-cascade cache by default вЂ” every
        // producer routes through here, but only `try_relayout_raf_incremental`'s
        // restyle sub-path knows how to recompute a cache that actually matches
        // `lb`, and re-validates it right after this call returns. Every other
        // producer (full `relayout()`, `readback_relayout_job`,
        // `poll_engine_commit`) leaves it `None`, forcing the next incremental
        // attempt onto the safe full-cascade-plus-graft fallback for one cycle.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(lb);
        self.refresh_cv_state();
        // Promote nodes with will-change: transform/opacity/filter to GPU layers so
        // animation ticks can update only the layer matrix, bypassing relayout.
        // CSS: will-change вЂ” P4 wires ComputedStyle.will_change to promote_layer calls here.
        if let (Some(lb_ref), Some(r)) = (self.layout_box.as_ref(), self.renderer.as_mut()) {
            promote_will_change_layers(lb_ref, r.as_mut());
        }
        // ADR-016 M0.3: the fresh display list is now laid out at the current
        // zoom, so any transform-first zoom preview is complete вЂ” clear the
        // debounce and reset the backend to 1:1. Done for every relayout
        // (resize, DOM mutation, tab switch), not just the debounced zoom one,
        // so a relayout from another source also lands the pending zoom.
        self.laid_out_zoom_factor = self.zoom_factor;
        self.pending_zoom_relayout = None;
        if let Some(r) = self.renderer.as_mut() {
            r.set_preview_scale(1.0);
        }
        self.update_snap_containers();
        self.update_scroll_containers();
        self.animation_scheduler.clear();
        // Do NOT reset transition_scheduler here: active transitions must survive
        // relayout (viewport resize, DOM mutations) so that in-flight animations
        // continue smoothly. reset happens only on page load (apply_loaded_page).
        self.anim_frame = None;
        self.scroll_y = clamp_scroll(self.scroll_y, self.max_scroll());
        self.scroll_x = clamp_scroll(self.scroll_x, self.max_scroll_x());
        // Notify JS observers about the new layout geometry (ResizeObserver /
        // IntersectionObserver / getBoundingClientRect).
        #[cfg(feature = "v8")]
        {
            // Lazy-load requests drained while `self` is borrowed immutably;
            // fetched after the borrow ends (fetch needs `&mut self`).
            let mut lazy_reqs: Vec<(u32, String)> = Vec::new();
            // ADR-016 M2.2c-2d: layout-geometry push (`update_layout_rects` Рё Co.)
            // is the last mixed read+write UIв†’JS site in the relayout path. The
            // whole ordered sequence вЂ” rects/styles/viewport push в†’ observer &
            // matchMedia & lazy-image delivery в†’ `take_lazy_image_requests` read в†’
            // scroll-state push вЂ” moves into ONE `route_query_js` closure returning
            // `lazy_reqs`, so under the flag it runs atomically **in order** on the
            // engine thread (the value read after the void pushes keeps its
            // read-after-write ordering) and blocks only for that one result. The
            // `self.js_present` gate mirrors the old `if let Some(js)` вЂ” the
            // (side-effect-free) geometry collection runs only when a JS context
            // exists, byte-identical with the flag off. All captured data is owned
            // (`HashMap`/`Vec`) в†’ the closure is `Send + 'static`.
            if self.js_present
                && let Some(lb_ref) = self.layout_box.as_ref()
            {
                let rects = collect_layout_rects(lb_ref);
                let styles = collect_computed_styles(lb_ref);
                let customs = collect_custom_properties(lb_ref);
                let (vw, vh) = (viewport.width, viewport.height);
                let dark_mode = self.dark_mode;
                let reduced_motion = self.a11y_store.reduced_motion();
                // Keep JS scroll-state cache in sync so scrollTop/scrollLeft reads
                // immediately after relayout return the correct clamped values.
                let scroll_states: HashMap<u32, [f32; 4]> = collect_scroll_containers(lb_ref)
                    .iter()
                    .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                    .collect();
                lazy_reqs = route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                    js.update_layout_rects(rects);
                    js.update_computed_styles(styles);
                    js.update_custom_properties(customs);
                    js.update_viewport_size(vw, vh);
                    js.deliver_layout_observers();
                    // CSS MQ L4 В§4.2: re-evaluate matchMedia() lists against the new
                    // viewport. `dark_mode` mirrors the OS `prefers-color-scheme`,
                    // read from winit at window creation / refreshed on ThemeChanged.
                    js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion);
                    // After fresh rects are in JS: fire lazy-load proximity check.
                    // Images that entered the viewport+margin are queued by JS via
                    // _lumen_request_lazy_image_load; we drain and fetch them below.
                    js.deliver_lazy_images();
                    let reqs = js.take_lazy_image_requests();
                    js.update_scroll_states(scroll_states);
                    reqs
                })
                .unwrap_or_default();
            }
            if !lazy_reqs.is_empty() {
                self.fetch_and_register_lazy_images(lazy_reqs);
            }
        }
        // BUG-730: images the page added after load land here вЂ” this is the one
        // post-layout point every relayout producer routes through, so a
        // script-appended `<img>` is picked up whichever path relaid it out.
        self.spawn_dynamic_image_loads(viewport);
        // BUG-735: Рё РїРѕ С‚РѕР№ Р¶Рµ РїСЂРёС‡РёРЅРµ вЂ” СЃРІРµР¶РµРїРµСЂРµСЃС‚СЂРѕРµРЅРЅРѕРµ РїРѕРґРґРµСЂРµРІРѕ РјРѕРіР»Рѕ
        // РїСЂРёРЅРµСЃС‚Рё РќРћР’Р«Р™ `<img>` СЃ СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рј `src` (React РїРµСЂРµСЂРёСЃРѕРІР°Р»
        // Р±Р»РѕРє: СѓР·РµР» РґСЂСѓРіРѕР№, РєР°СЂС‚РёРЅРєР° С‚Р° Р¶Рµ). Р’С‚РѕСЂРѕРіРѕ `ImageDecoded` РґР»СЏ РЅРµРіРѕ РЅРµ
        // Р±СѓРґРµС‚ вЂ” Р·Р°РїСЂРѕСЃ РґРµРґСѓРїР»РёС†РёСЂРѕРІР°РЅ РїРѕ URL, вЂ” РїРѕСЌС‚РѕРјСѓ СЂР°Р·РјРµСЂС‹ РµРјСѓ СЂР°Р·РґР°С‘С‚
        // РїСЂРѕС…РѕРґ `apply_stream_intrinsic_sizes`, Рё Р·РґРµСЃСЊ РјС‹ РµРіРѕ Р·Р°РєР°Р·С‹РІР°РµРј.
        // РџСѓСЃС‚РѕР№ РєР°СЂС‚Рµ Р·Р°РєР°Р·С‹РІР°С‚СЊ РЅРµС‡РµРіРѕ; СЃР°Рј РїСЂРѕС…РѕРґ no-op, РµСЃР»Рё РґРѕРїРёСЃС‹РІР°С‚СЊ
        // РЅРµС‡РµРіРѕ, С‚Р°Рє С‡С‚Рѕ В«СЂРµР»РµР№Р°СѓС‚ в†’ РїСЂРѕС…РѕРґ в†’ СЂРµР»РµР№Р°СѓС‚В» РЅРµ Р·Р°С†РёРєР»РёРІР°РµС‚СЃСЏ.
        self.stream_image_sizes_dirty |= !self.stream_image_sizes.is_empty();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// ADR-016 M2.2: build the immutable-snapshot relayout job that the engine
    /// thread runs off the UI thread вЂ” shared by the fire-and-forget
    /// [`Self::submit_relayout_job`] (latest-wins) and the blocking
    /// [`Self::readback_relayout_job`] (request/reply), so both produce a
    /// byte-identical [`EngineCommit`] for the same DOM state.
    ///
    /// Returns `None` вЂ” nothing to lay out вЂ” when there is no `LayoutSource`/renderer
    /// or the viewport is degenerate. On success bumps `engine_job_generation` and
    /// returns `(generation, job)`; the caller decides whether to `submit` it
    /// (deferred, latest-wins) or `readback` it (blocking). Because the generation is
    /// bumped here, callers must gate on the engine thread being present **before**
    /// calling this (both wrappers do) so a flag-off run never advances the counter.
    ///
    /// The job captures immutable `Arc` snapshots of the document + stylesheet +
    /// web-fonts (invariant 1) and re-establishes the interactive/forced-colors/
    /// content-visibility thread-local state **on the engine thread** before
    /// computing layout.
    pub(crate) fn make_relayout_job(
        &mut self,
    ) -> Option<(u64, impl FnOnce() -> EngineCommit + Send + 'static)> {
        let viewport = self.relayout_viewport()?;
        // BUG-743: СЃРЅРёРјРѕРє Р»РёСЃС‚Р° РґР»СЏ РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° Р±РµСЂС‘С‚СЃСЏ Р·РґРµСЃСЊ, РїРѕСЌС‚РѕРјСѓ
        // РїРѕР·РґРЅРёР№ РґРёРЅР°РјРёС‡РµСЃРєРёР№ `<style>` РґРѕР»Р¶РµРЅ РїРѕРїР°СЃС‚СЊ РІ РєР°СЃРєР°Рґ РґРѕ РєР»РѕРЅРёСЂРѕРІР°РЅРёСЏ.
        self.refresh_dynamic_css();
        let src = self.layout_source.as_ref()?;
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        let generation = self.engine_job_generation;
        // Immutable snapshots captured by the job (ADR-016 invariant 1). The
        // stylesheet is now an `Arc` in `LayoutSource` (M2.2b), so the job clones
        // only the handle вЂ” no per-submit deep clone of the whole `Stylesheet`.
        let document = Arc::clone(&src.document);
        let stylesheet = Arc::clone(&src.stylesheet);
        let hp = Arc::clone(&self.hyp_provider);
        let web_fonts = self.web_fonts.clone();
        let dark_mode = self.dark_mode;
        let hovered = self.hovered_nid;
        let focused = self.focused_node;
        let active = self.active_nid;
        let forced_colors = self.a11y_store.forced_colors();
        let (cv_x, cv_y) = (self.scroll_x, self.scroll_y);
        let cv_relevant = self.cv_relevant.clone();
        let job = move || {
            let t0 = std::time::Instant::now();
            // Interactive state is thread-local вЂ” set it on THIS (engine) thread.
            lumen_layout::set_interactive_state(hovered, focused, active);
            lumen_layout::set_forced_colors(forced_colors);
            lumen_layout::set_cv_scroll(cv_x, cv_y);
            lumen_layout::set_cv_relevant(cv_relevant);
            let (content, layout_box) =
                compute_layout(&document, &stylesheet, viewport, &*hp, dark_mode, &web_fonts);
            lumen_layout::clear_interactive_state();
            lumen_layout::set_cv_scroll(0.0, 0.0);
            lumen_layout::set_cv_relevant(std::collections::HashSet::new());
            EngineCommit {
                content,
                layout_box,
                viewport,
                generation,
                compute_ms: t0.elapsed().as_secs_f32() * 1000.0,
            }
        };
        Some((generation, job))
    }

    /// ADR-016 M2.2: route a relayout to the persistent engine thread (off the
    /// UI thread). Returns `true` if a job was submitted; `false` when the engine
    /// thread is absent (`LUMEN_ENGINE_THREAD` off) or there is nothing to lay out
    /// вЂ” the caller then falls back to the synchronous [`Self::relayout`].
    ///
    /// Only for **async-safe** triggers: no caller may read layout geometry
    /// synchronously after this returns, because the commit lands a few frames
    /// later via [`Self::poll_engine_commit`]. Callers are the debounced
    /// transform-first zoom (M0.3), the chrome-inset toggles ([`Self::relayout_chrome`],
    /// M2.2b), the form-input toggles ([`Self::relayout_form`], M2.2c-3) and the
    /// `about_to_wait` rAF DOM-dirty flush ([`Self::relayout_raf_dirty`], M2.2c-3) вЂ”
    /// none reads geometry synchronously afterward.
    pub(crate) fn submit_relayout_job(&mut self) -> bool {
        if self.engine_thread.is_none() {
            return false;
        }
        let Some((generation, job)) = self.make_relayout_job() else { return false };
        let Some(engine) = self.engine_thread.as_ref() else { return false };
        engine.submit(generation, job);
        true
    }

    /// ADR-016 M2.2c-3: run a relayout **on the engine thread but block** for its
    /// commit (request/reply via [`engine_thread::EngineThread::readback`]), then
    /// apply it synchronously вЂ” for sites that read a layout product in the same
    /// tick. Returns `true` if the readback ran and was applied; `false` when the
    /// engine thread is absent (`LUMEN_ENGINE_THREAD` off), there is nothing to lay
    /// out, or the thread was shutting down (`readback` в†’ `None`) вЂ” the caller then
    /// falls back to the synchronous [`Self::relayout`].
    ///
    /// The sole caller today is the `RedrawRequested` rAF DOM-dirty flush
    /// ([`Self::relayout_raf_dirty_readback`]), whose next step reads
    /// `self.display_list.is_empty()` for PerformancePaintTiming. Unlike
    /// [`Self::submit_relayout_job`] the commit is **not** deposited in the
    /// latest-wins slot; it comes straight back and is applied here, so like the
    /// synchronous [`Self::relayout`] this is authoritative вЂ” `engine_applied_generation`
    /// advances to the just-bumped `engine_job_generation`, dropping any older
    /// in-flight async commit in [`Self::poll_engine_commit`]'s guard.
    pub(crate) fn readback_relayout_job(&mut self) -> bool {
        if self.engine_thread.is_none() {
            return false;
        }
        let Some((_generation, job)) = self.make_relayout_job() else { return false };
        let Some(engine) = self.engine_thread.as_ref() else { return false };
        let Some(commit) = engine.readback(job) else { return false };
        // Authoritative like `relayout()`: mark the just-bumped generation applied so
        // a stale in-flight async commit is dropped by `poll_engine_commit`.
        self.engine_applied_generation = self.engine_job_generation;
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
        self.apply_relayout_result(content, layout_box, viewport);
        if lumen_paint::frame_log_enabled() {
            self.engine_stats.record(compute_ms);
            eprintln!(
                "[engine] relayout {compute_ms:.2}ms (readback) dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
        true
    }

    /// ADR-016 M2.2: consume the newest off-thread layout result, if the engine
    /// thread produced one, and apply it on the UI thread. A no-op when the engine
    /// thread is off or nothing is ready. The commit is dropped when its
    /// `generation` no longer matches `engine_job_generation` вЂ” a newer job or a
    /// synchronous `relayout()` has superseded it (generation-guard, invariant 2).
    pub(crate) fn poll_engine_commit(&mut self) {
        // Take the commit and release the `engine_thread` borrow before the
        // `&mut self` apply below.
        let Some(commit) = self.engine_thread.as_ref().and_then(|e| e.take_committed()) else {
            return;
        };
        if commit.generation != self.engine_job_generation {
            return; // superseded вЂ” drop the stale result.
        }
        self.engine_applied_generation = commit.generation;
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
        self.apply_relayout_result(content, layout_box, viewport);
        // ADR-016 M2.0/M2.2: record the off-thread compute cost. Unlike the
        // synchronous path this excludes the UI-thread apply (observers etc.),
        // and is tagged `(off-thread)` so the summary reflects the work moved off
        // the UI thread.
        self.engine_stats.record(compute_ms);
        if lumen_paint::frame_log_enabled() {
            eprintln!(
                "[engine] relayout {compute_ms:.2}ms (off-thread) dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
    }

    /// ADR-016 M2.2c-2d (21): РЅР°Р·РЅР°С‡РёС‚СЊ JS-С…СЌРЅРґР» Р°РєС‚РёРІРЅРѕР№ РІРєР»Р°РґРєРё, РґРµСЂР¶Р°
    /// [`Self::js_present`] РІ СЃРІСЏР·РєРµ СЃ С„Р°РєС‚РёС‡РµСЃРєРёРј РІР»Р°РґРµР»СЊС†РµРј `Arc`.
    ///
    /// **Р­С‚Рѕ РµРґРёРЅСЃС‚РІРµРЅРЅР°СЏ С‚РѕС‡РєР° РІР»Р°РґРµРЅРёСЏ С…СЌРЅРґР»РѕРј.** РљСѓРґР° СЃР°РґРёС‚СЃСЏ `Arc` Р·Р°РІРёСЃРёС‚ РѕС‚
    /// С‚РѕРіРѕ, РїРѕРґРЅСЏС‚ Р»Рё РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє:
    /// - РїРѕС‚РѕРє РµСЃС‚СЊ (`LUMEN_ENGINE_THREAD=1`) в†’ `Arc` **РїРµСЂРµРµР·Р¶Р°РµС‚ РЅР° РґРІРёР¶РєРѕРІС‹Р№
    ///   РїРѕС‚РѕРє** РІ [`EngineJsState::js`] С‡РµСЂРµР· [`engine_thread::EngineThread::task`],
    ///   Р° UI-СЃС‚РѕСЂРѕРЅРЅРёР№ [`Self::js_ctx`] РѕСЃС‚Р°С‘С‚СЃСЏ `None`. РњР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂС‹
    ///   ([`route_task_js`]/[`route_query_js`]/[`route_eval_js`]) РїРѕРґ С„Р»Р°РіРѕРј Рё С‚Р°Рє
    ///   РёРіРЅРѕСЂРёСЂСѓСЋС‚ РїРµСЂРµРґР°РЅРЅС‹Р№ UI-РєР»РѕРЅ Рё С‡РёС‚Р°СЋС‚ `state.js`, РїРѕСЌС‚РѕРјСѓ РІСЃРµ call-site'С‹
    ///   РѕСЃС‚Р°СЋС‚СЃСЏ РєРѕСЂСЂРµРєС‚РЅС‹, Р° СЃР°Рј СЂР°РЅС‚Р°Р№Рј РІСЃС‘ СЂР°РІРЅРѕ Р¶РёРІС‘С‚ РЅР° СЃРІРѕС‘Рј `lumen-js`-РїРѕС‚РѕРєРµ
    ///   (ADR-014) вЂ” СЌС‚Рѕ РїРµСЂРµРЅРѕСЃ РІР»Р°РґРµРЅРёСЏ С…СЌРЅРґР»РѕРј, Р° РЅРµ СЂР°Р·РґРµР»РµРЅРёРµ РјСѓС‚Р°Р±РµР»СЊРЅРѕРіРѕ
    ///   СЃРѕСЃС‚РѕСЏРЅРёСЏ (РёРЅРІР°СЂРёР°РЅС‚ 1);
    /// - РїРѕС‚РѕРєР° РЅРµС‚ (С„Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ, РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ, Р»РёР±Рѕ spawn РЅРµ СѓРґР°Р»СЃСЏ) в†’ `Arc`
    ///   С…СЂР°РЅРёС‚СЃСЏ РІ UI-СЃС‚РѕСЂРѕРЅРЅРµРј [`Self::js_ctx`] РєР°Рє РїСЂРµР¶РґРµ вЂ” **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ**.
    ///
    /// [`Self::js_present`] РѕС‚РґРµР»СЏРµС‚ СЂРµС€РµРЅРёРµ В«РµСЃС‚СЊ Р»Рё JS?В» РѕС‚ С‚РѕРіРѕ, РєР°РєР°СЏ СЃС‚РѕСЂРѕРЅР°
    /// РґРµСЂР¶РёС‚ `Arc`: РІСЃРµ РіРµР№С‚С‹ (`if self.js_present`) С‡РёС‚Р°СЋС‚ РµРіРѕ, РїРѕСЌС‚РѕРјСѓ РѕСЃС‚Р°СЋС‚СЃСЏ
    /// РІРµСЂРЅС‹ РІ РѕР±РѕРёС… СЂРµР¶РёРјР°С… С„Р»Р°РіР°.
    pub(crate) fn set_js_ctx(&mut self, handle: Option<Arc<dyn PersistentJs>>) {
        // BUG-839: the document is committed at this point, so per-step
        // Resource Timing delivery may resume вЂ” whatever is still queued, and
        // everything that arrives from here on, belongs to this runtime. The
        // *clear* is deliberately not here (it runs where the load starts): by
        // the time this is reached, `source.load` has already fetched the
        // page's stylesheets, scripts and images.
        resource_timing::resume();
        self.js_present = handle.is_some();
        match self.engine_thread.as_ref() {
            // Flag on: the handle lives engine-side; deposit it into
            // `EngineJsState.js` and leave the UI field empty.
            Some(engine) => {
                // ADR-016 M2.3: before the handle moves engine-side, cache
                // lock-free clones of its rAF-pending / DOM-dirty flags so the
                // UI thread can schedule + consume rAF turns without a blocking
                // engine `query`. `None` clears them (blank/JS-less tab).
                self.raf_pending_flag = handle.as_ref().and_then(|h| h.raf_pending_flag());
                self.dom_dirty_flag = handle.as_ref().and_then(|h| h.dom_dirty_flag());
                self.js_ctx = None;
                engine.task(move |state| state.js = handle);
            }
            // Flag off (default): the UI thread owns the handle, exactly as before.
            None => self.js_ctx = handle,
        }
    }

    /// ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёС‚ СЂР°Р·РґРµР»СЏРµРјС‹Р№ `Document` Р°РєС‚РёРІРЅРѕР№ РІРєР»Р°РґРєРё РІ
    /// РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРµ СЃРѕСЃС‚РѕСЏРЅРёРµ [`EngineJsState`] РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР°.
    ///
    /// No-op, РєРѕРіРґР° РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° РЅРµС‚ (`LUMEN_ENGINE_THREAD` РІС‹РєР»СЋС‡РµРЅ, РїРѕ
    /// СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” С‚РѕРіРґР° РїРѕРІРµРґРµРЅРёРµ shell Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ. Р’С‹Р·С‹РІР°РµС‚СЃСЏ РїСЂРё РєР°Р¶РґРѕР№
    /// СЃРјРµРЅРµ СЃС‚СЂР°РЅРёС†С‹ (РїРѕСЃР»Рµ [`Self::set_js_ctx`] + СѓСЃС‚Р°РЅРѕРІРєРё `layout_source`),
    /// С‡С‚РѕР±С‹ `task`/`query`-РІС‹Р·РѕРІС‹ РІРёРґРµР»Рё Р°РєС‚СѓР°Р»СЊРЅС‹Р№ DOM. `Arc`-РєР»РѕРЅ РґС‘С€РµРІ.
    ///
    /// Р’Р»Р°РґРµРЅРёРµ JS-С…СЌРЅРґР»РѕРј СЃСЋРґР° Р±РѕР»СЊС€Рµ РЅРµ РІС…РѕРґРёС‚ вЂ” СЃ M2.2c-2d (21) РµРіРѕ РїРµСЂРµРЅРѕСЃРёС‚
    /// СЃР°Рј [`Self::set_js_ctx`]; Р·РґРµСЃСЊ РѕСЃС‚Р°С‘С‚СЃСЏ С‚РѕР»СЊРєРѕ Р·РµСЂРєР°Р»Рѕ `document`
    /// (В«СЃРёРґРµРЅСЊРµВ» Р±СѓРґСѓС‰РµРіРѕ РІР»Р°РґРµРЅРёСЏ DOM РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј, M2.2c-3).
    pub(crate) fn sync_engine_js_state(&self) {
        let Some(engine) = self.engine_thread.as_ref() else { return };
        let document = self.layout_source.as_ref().map(|ls| Arc::clone(&ls.document));
        engine.task(move |state| state.document = document);
    }

    /// ADR-016 M2.2c-2d (21): РёР·РІР»РµС‡СЊ JS-С…СЌРЅРґР» Р°РєС‚РёРІРЅРѕР№ РІРєР»Р°РґРєРё РґР»СЏ СЃРЅР°РїС€РѕС‚Р°
    /// (`save_page_snapshot`).
    ///
    /// РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) `Arc` Р¶РёРІС‘С‚ РІ [`EngineJsState::js`] РЅР°
    /// РґРІРёР¶РєРѕРІРѕРј РїРѕС‚РѕРєРµ, РїРѕСЌС‚РѕРјСѓ РµРіРѕ РІС‹РЅРёРјР°РµС‚ Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query`, `take`-Р°СЋС‰РёР№ РµРіРѕ
    /// РёР· СЃРѕСЃС‚РѕСЏРЅРёСЏ (РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ РїРѕСЃР»Рµ СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… `task`, С‚Р°Рє С‡С‚Рѕ РІРёРґРёС‚
    /// РїРѕСЃР»РµРґРЅРёР№ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРЅС‹Р№ С…СЌРЅРґР»); Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” `take` РїСЂСЏРјРѕ РёР·
    /// UI-СЃС‚РѕСЂРѕРЅРЅРµРіРѕ [`Self::js_ctx`], **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ** РїСЂРµР¶РЅРµРјСѓ `self.js_ctx.take()`.
    /// Р’РѕР·РІСЂР°С‰С‘РЅРЅС‹Р№ `Arc` РєР»Р°РґС‘С‚СЃСЏ РІ [`PageSnapshot::js_ctx`] Рё РѕСЃС‚Р°С‘С‚СЃСЏ СЂРµР°Р»СЊРЅС‹Рј
    /// С…СЌРЅРґР»РѕРј РґР°Р¶Рµ РїРѕРґ С„Р»Р°РіРѕРј (bg-tab GC Рё restore С‡РёС‚Р°СЋС‚ РµРіРѕ РЅР°РїСЂСЏРјСѓСЋ).
    pub(crate) fn take_js_ctx(&mut self) -> Option<Arc<dyn PersistentJs>> {
        match self.engine_thread.as_ref() {
            Some(engine) => engine.query(|state| state.js.take()).flatten(),
            None => self.js_ctx.take(),
        }
    }

    /// BUG-480 срез 19: КЛОН JS-хэндла страницы, где бы он ни лежал.
    ///
    /// [`Self::js_ctx`] в живом окне почти всегда `None`: с ADR-023 движковый
    /// поток включён ПО УМОЛЧАНИЮ, и [`Self::set_js_ctx`] кладёт хэндл в его
    /// состояние, оставляя UI-поле пустым. Код, который берёт `self.js_ctx`
    /// напрямую вместо `route_*`, поэтому молча ничего не делает — навигация
    /// фрейма так не зарегистрировала под-документ у родителя и не отправила
    /// `load` на хосте, пока проба не измерила это на живом окне.
    ///
    /// Нужен именно хэндл, а не `route_task_js`: под-документ грузится
    /// синхронно и зовёт у родителя четыре разных метода, а маршрутизатор
    /// умеет только «выполнить одно замыкание и забыть». Вызов самого хэндла с
    /// UI-потока безопасен — каждый `V8JsRuntime` владеет своим потоком и
    /// изолятом и сам переправляет работу туда (это и есть путь без
    /// движкового потока).
    pub(crate) fn clone_js_ctx(&self) -> Option<Arc<dyn PersistentJs>> {
        match self.engine_thread.as_ref() {
            Some(engine) => engine.query(|state| state.js.clone()).flatten(),
            None => self.js_ctx.clone(),
        }
    }
}

/// РџРѕРІС‚РѕСЂРЅС‹Р№ layout+paint РїРѕ СЃРѕС…СЂР°РЅС‘РЅРЅРѕРјСѓ `LayoutSource` СЃ РЅРѕРІС‹Рј viewport.
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(DisplayList, LayoutBox)` вЂ” LayoutBox РЅСѓР¶РµРЅ РґР»СЏ animation scheduler.
/// `dark_mode` is forwarded to `layout_measured_hyp` so `@media (prefers-color-scheme: dark)`
/// rules take effect on relayout (e.g. after OS theme change or window resize).
pub(crate) fn relayout_page(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
) -> (DisplayList, lumen_layout::LayoutBox) {
    compute_layout(&src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts)
}

/// РџСЂРѕС†РµСЃСЃ-РіР»РѕР±Р°Р»СЊРЅС‹Рµ РјРµС‚СЂРёРєРё СЃРёСЃС‚РµРјРЅС‹С… С€СЂРёС„С‚РѕРІ РґР»СЏ РёР·РјРµСЂРёС‚РµР»СЏ: CSS
/// generic-СЃРµРјРµР№СЃС‚РІР° + РєРѕРЅРєСЂРµС‚РЅС‹Рµ СЃРёСЃС‚РµРјРЅС‹Рµ СЃРµРјРµР№СЃС‚РІР° РїРѕ РёРјРµРЅРё (BUG-128).
///
/// РЎС‚СЂРѕРёС‚СЃСЏ РѕРґРёРЅ СЂР°Р· РїРѕРІРµСЂС… РѕР±С‰РµРіРѕ СЃРёСЃС‚РµРјРЅРѕРіРѕ РёРЅРґРµРєСЃР°
/// ([`lumen_font::shared_system_index`]) Рё РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РІСЃРµРјРё
/// РїРµСЂРµСЃР±РѕСЂРєР°РјРё РёР·РјРµСЂРёС‚РµР»СЏ: СЃР°Рј СЃРєР°РЅ РґРёСЂРµРєС‚РѕСЂРёР№ С€СЂРёС„С‚РѕРІ СЃС‚СЂР°РЅРёС†Р° РґРµР»Р°РµС‚ РІ
/// Р»СЋР±РѕРј СЃР»СѓС‡Р°Рµ (СЂРµРЅРґРµСЂ СЂРµР·РѕР»РІРёС‚ face-С‹ С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ РёРЅРґРµРєСЃ), Р° С‡С‚РµРЅРёРµ Рё
/// РїР°СЂСЃРёРЅРі РІС‹Р±СЂР°РЅРЅС‹С… С„Р°Р№Р»РѕРІ РЅРµ РґРѕР»Р¶РЅРѕ РїРѕРІС‚РѕСЂСЏС‚СЊСЃСЏ РЅР° РєР°Р¶РґС‹Р№ relayout.
/// Р›РµРЅРёРІС‹Р№ РєСЌС€ РєРѕРЅРєСЂРµС‚РЅС‹С… СЃРµРјРµР№СЃС‚РІ Р¶РёРІС‘С‚ Р·РґРµСЃСЊ Р¶Рµ, РїРѕСЌС‚РѕРјСѓ `font-family:
/// Arial` С‡РёС‚Р°РµС‚СЃСЏ СЃ РґРёСЃРєР° РѕРґРёРЅ СЂР°Р· РЅР° РїСЂРѕС†РµСЃСЃ, Р° РЅРµ РЅР° РєР°Р¶РґС‹Р№ СЂРµР»СЌР№Р°СѓС‚.
pub(crate) fn system_font_faces() -> Arc<lumen_paint::SystemFaceSet> {
    static SHARED: std::sync::OnceLock<Arc<lumen_paint::SystemFaceSet>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            Arc::new(lumen_paint::SystemFaceSet::from_provider(
                lumen_font::shared_system_index().clone(),
            ))
        })
        .clone()
}

/// РР·РјРµСЂРёС‚РµР»СЊ РґР»СЏ СЃС‚СЂР°РЅРёС†С‹: bundled Inter + @font-face-СЃРµРјСЊРё + СЃРёСЃС‚РµРјРЅС‹Рµ
/// face-С‹ (generic-СЃРµРјРµР№СЃС‚РІР° Рё РєРѕРЅРєСЂРµС‚РЅС‹Рµ СЃРµРјРµР№СЃС‚РІР° РїРѕ РёРјРµРЅРё).
///
/// Р•РґРёРЅР°СЏ С‚РѕС‡РєР° СЃР±РѕСЂРєРё РґР»СЏ РІСЃРµС… layout-РїСѓС‚РµР№ (РїРѕР»РЅС‹Р№ / РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅС‹Р№ /
/// restyle) вЂ” РёРЅР°С‡Рµ СЃРёСЃС‚РµРјРЅС‹Рµ СЃРµРјРµР№СЃС‚РІР° РјРµСЂСЏСЋС‚СЃСЏ РїРѕ-СЂР°Р·РЅРѕРјСѓ РІ Р·Р°РІРёСЃРёРјРѕСЃС‚Рё РѕС‚
/// С‚РѕРіРѕ, РµСЃС‚СЊ Р»Рё РЅР° СЃС‚СЂР°РЅРёС†Рµ web-С€СЂРёС„С‚С‹.
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn page_measurer(
    font: &lumen_font::Font<'static>,
    web_fonts: &[LoadedWebFont],
) -> lumen_paint::MultiFontMeasurer {
    let mut measurer = lumen_paint::MultiFontMeasurer::new(font)
        .expect("MultiFontMeasurer РёР· bundled Inter");
    for wf in web_fonts {
        measurer.register_family_with_ranges(
            &wf.family,
            wf.bytes.clone(),
            wf.unicode_range.clone(),
        );
    }
    measurer.set_system_faces(system_font_faces());
    measurer
}

/// РЇРґСЂРѕ style+layout+display-list РїРѕ immutable-СЃРЅР°РїС€РѕС‚Сѓ РґРѕРєСѓРјРµРЅС‚Р° Рё СЃС‚РёР»РµР№.
///
/// Р’С‹РЅРµСЃРµРЅРѕ РёР· [`relayout_page`], С‡С‚РѕР±С‹ РѕРґРЅСѓ Рё С‚Сѓ Р¶Рµ СЂР°Р±РѕС‚Сѓ РјРѕР¶РЅРѕ Р±С‹Р»Рѕ РІС‹Р·РІР°С‚СЊ Рё
/// РЅР° UI-РїРѕС‚РѕРєРµ (СЃРёРЅС…СЂРѕРЅРЅС‹Р№ `relayout()`), Рё РЅР° РґРІРёР¶РєРѕРІРѕРј РїРѕС‚РѕРєРµ (ADR-016 M2.2,
/// [`Lumen::submit_relayout_job`]) вЂ” РІС‚РѕСЂРѕРјСѓ `LayoutSource` РЅРµРґРѕСЃС‚СѓРїРµРЅ, Сѓ РЅРµРіРѕ РЅР°
/// СЂСѓРєР°С… С‚РѕР»СЊРєРѕ `Arc`-СЃРЅРёРјРєРё `document`/`stylesheet`. РРЅС‚РµСЂР°РєС‚РёРІРЅРѕРµ СЃРѕСЃС‚РѕСЏРЅРёРµ
/// (`:hover`/`:focus`/`forced-colors`/`content-visibility` scroll) вЂ” thread-local
/// (`lumen_layout::set_*`), РїРѕСЌС‚РѕРјСѓ РІС‹Р·С‹РІР°СЋС‰Р°СЏ СЃС‚РѕСЂРѕРЅР° РѕР±СЏР·Р°РЅР° РІС‹СЃС‚Р°РІРёС‚СЊ РµРіРѕ РЅР°
/// **С‚РѕРј Р¶Рµ** РїРѕС‚РѕРєРµ РґРѕ РІС‹Р·РѕРІР° Рё СЃР±СЂРѕСЃРёС‚СЊ РїРѕСЃР»Рµ.
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn compute_layout(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    // PH3-19: РёР·РјРµСЂРёС‚РµР»СЊ РІРєР»СЋС‡Р°РµС‚ РЅР°РєРѕРїР»РµРЅРЅС‹Рµ web-С€СЂРёС„С‚С‹ (FOUT relayout);
    // BUG-128: Рё СЃРёСЃС‚РµРјРЅС‹Рµ face-С‹.
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let layout = lumen_layout::layout_measured_hyp(&doc, stylesheet, viewport, &measurer, hp, dark_mode);
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// ADR-016 M4: incremental variant of [`relayout_page`] вЂ” uses
/// [`lumen_layout::layout_mutation_incremental`] to skip geometry re-computation
/// for subtrees whose [`lumen_layout::ComputedStyle`] is unchanged, while
/// preserving full cascade and post-layout passes. `prev` is the previously
/// laid-out tree stored in `Lumen::layout_box`.
pub(crate) fn relayout_page_incremental(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    prev: &lumen_layout::LayoutBox,
) -> (DisplayList, lumen_layout::LayoutBox) {
    compute_layout_incremental(&src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts, prev)
}

/// ADR-016 M4: incremental variant of [`compute_layout`] вЂ” runs the full
/// cascade but reuses geometry from `prev` for unchanged subtrees.
///
/// Same caller contract as [`compute_layout`]: thread-local interactive state
/// must be set before the call and cleared afterwards.
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn compute_layout_incremental(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    prev: &lumen_layout::LayoutBox,
) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let layout = lumen_layout::layout_mutation_incremental(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev,
    );
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// BUG-341 S7: restyle-aware variant of [`relayout_page_incremental`] вЂ” uses
/// [`lumen_layout::box_tree::layout_mutation_incremental_restyle`] instead of
/// [`lumen_layout::layout_mutation_incremental`], skipping cascade work (not
/// just geometry) for subtrees `delta.dirty_roots` proves untouched. Only
/// safe when `delta.prev_styles` is the exact `CounterMap::styles()` the
/// previous cycle over this same document produced вЂ” see
/// `layout_mutation_incremental_restyle`'s own doc comment for the full
/// precondition; [`Lumen::page_prev_cascade_styles`] being `Some` is the
/// caller-side half of that contract. Returns the fresh `CounterMap` so the
/// caller can persist its `styles()` as the next cycle's `delta.prev_styles`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn relayout_page_incremental_restyle(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    // BUG-341 S19: consumed вЂ” the reusable subtrees are moved out of it into
    // the tree returned. See `layout_mutation_incremental_restyle`.
    prev: lumen_layout::LayoutBox,
    delta: lumen_layout::counters::RestyleDelta<'_>,
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    compute_layout_incremental_restyle(
        &src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts, prev, delta,
    )
}

/// BUG-341 S7: restyle-aware variant of [`compute_layout_incremental`] вЂ” see
/// [`relayout_page_incremental_restyle`].
#[allow(clippy::too_many_arguments)]
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn compute_layout_incremental_restyle(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    // BUG-341 S19: consumed вЂ” the reusable subtrees are moved out of it into
    // the tree returned. See `layout_mutation_incremental_restyle`.
    prev: lumen_layout::LayoutBox,
    delta: lumen_layout::counters::RestyleDelta<'_>,
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let (layout, counters) = lumen_layout::box_tree::layout_mutation_incremental_restyle(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev, delta,
    );
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout, counters)
}

/// CSS Containment L3 В§4.4 (BB-4) вЂ” shell-СЃРѕР±С‹С‚РёРµ: СЌР»РµРјРµРЅС‚ СЃ
/// `content-visibility: auto` СЃРјРµРЅРёР» skipped-СЃРѕСЃС‚РѕСЏРЅРёРµ РјРµР¶РґСѓ layout-РїСЂРѕС…РѕРґР°РјРё.
/// `skipped == true` вЂ” РїРѕРґРґРµСЂРµРІРѕ РІС‹РїР°Р»Рѕ РёР· СЂР°СЃС€РёСЂРµРЅРЅРѕРіРѕ viewport Рё РїСЂРѕРїСѓС‰РµРЅРѕ;
/// `false` вЂ” СѓР·РµР» СЃС‚Р°Р» relevant Рё РµРіРѕ СЃРѕРґРµСЂР¶РёРјРѕРµ СЃРЅРѕРІР° РІС‹Р»РѕР¶РµРЅРѕ.
/// Phase 2: P3 РґРѕСЃС‚Р°РІР»СЏРµС‚ РєР°Рє `contentvisibilityautostatechange` РІ JS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContentVisibilityChange {
    /// DOM-СѓР·РµР» СЌР»РµРјРµРЅС‚Р° СЃ `content-visibility: auto`.
    pub(crate) node: NodeId,
    /// РќРѕРІРѕРµ СЃРѕСЃС‚РѕСЏРЅРёРµ: `true` вЂ” СЃРѕРґРµСЂР¶РёРјРѕРµ РїСЂРѕРїСѓС‰РµРЅРѕ, `false` вЂ” РІС‹Р»РѕР¶РµРЅРѕ.
    pub(crate) skipped: bool,
}

/// РЎРѕР±СЂР°С‚СЊ `(node, top_y)` **РІСЃРµС…** `content-visibility: auto` Р±РѕРєСЃРѕРІ РІ РїРѕСЂСЏРґРєРµ
/// РґРµСЂРµРІР°. top_y вЂ” СЃС‚СЂР°РЅРёС†Р°-РєРѕРѕСЂРґРёРЅР°С‚С‹ Р±РѕРєСЃР°. РЎРєР°РЅ РїРѕ РґРµСЂРµРІСѓ (Р° РЅРµ thread-local)
/// вЂ” СЂР°Р±РѕС‚Р°РµС‚ Рё РґР»СЏ layout-Р°, РІС‹РїРѕР»РЅРµРЅРЅРѕРіРѕ РІ С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹.
///
/// BUG-852: СЂР°РЅСЊС€Рµ СЌС‚Р° С„СѓРЅРєС†РёСЏ СЃРѕР±РёСЂР°Р»Р° С‚РѕР»СЊРєРѕ Р±РѕРєСЃС‹ СЃ РїСѓСЃС‚С‹Рј СЃРїРёСЃРєРѕРј РґРµС‚РµР№ Рё
/// Р·РІР°Р»Р° РёС… В«РїСЂРѕРїСѓС‰РµРЅРЅС‹РјРёВ». РЎРѕРІРїР°РґРµРЅРёРµ РЅРµС‚РѕС‡РЅРѕРµ РІ РѕР±Рµ СЃС‚РѕСЂРѕРЅС‹: РїСѓСЃС‚РѕР№
/// `<div style="content-visibility:auto">` вЂ” Р° РёРјРµРЅРЅРѕ С‚Р°РєРѕР№ СЃС‚СЂРѕРёС‚
/// `content-visibility-auto-state-changed-first-observation.html` вЂ” РІС‹РіР»СЏРґРµР»
/// РїСЂРѕРїСѓС‰РµРЅРЅС‹Рј, РіРґРµ Р±С‹ РѕРЅ РЅРё СЃС‚РѕСЏР», Р° layout РїСЂРѕ РЅРµРіРѕ РІРѕРѕР±С‰Рµ РЅРµ СЃРїСЂР°С€РёРІР°Р»
/// (`cv_should_skip` РІС‹Р·С‹РІР°РµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё `!children.is_empty()`). РЎРѕСЃС‚РѕСЏРЅРёРµ
/// С‚РµРїРµСЂСЊ СЃС‡РёС‚Р°РµС‚ [`Lumen::refresh_cv_state`] РїРѕ СЃР°РјРѕРјСѓ РїСЂР°РІРёР»Сѓ СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚Рё.
///
/// **Р”РµРґСѓРїР»РёРєР°С†РёСЏ РїРѕ СѓР·Р»Сѓ РѕР±СЏР·Р°С‚РµР»СЊРЅР°, Рё РµС‘ РѕС‚СЃСѓС‚СЃС‚РІРёРµ вЂ” РЅРµ РјРµР»РѕС‡СЊ.** РђРЅРѕРЅРёРјРЅС‹Р№
/// Р±РѕРєСЃ (`InlineRun` РґР»СЏ inline-СЃРѕРґРµСЂР¶РёРјРѕРіРѕ, `InlineBlockRow`, РѕР±С‘СЂС‚РєРё С‚Р°Р±Р»РёС†)
/// РЅРµ РёРјРµРµС‚ СЃРІРѕРµРіРѕ СЌР»РµРјРµРЅС‚Р° Рё РЅРµСЃС‘С‚ СЃС‚РёР»СЊ СЂРѕРґРёС‚РµР»СЏ, РІРєР»СЋС‡Р°СЏ
/// `content-visibility: auto`, вЂ” С‚Рѕ РµСЃС‚СЊ `<div style="content-visibility:auto">
/// <span>x</span></div>` РґР°С‘С‚ Р”Р’Рђ Р±РѕРєСЃР° СЃ СЌС‚РёРј Р·РЅР°С‡РµРЅРёРµРј. Р‘РµР· РґРµРґСѓРїР»РёРєР°С†РёРё
/// `diff_cv_state` СЃСЂР°РІРЅРёР» Р±С‹ РІС‚РѕСЂРѕР№ РёР· РЅРёС… СЃ РµС‰С‘ РЅРµ РѕР±РЅРѕРІР»С‘РЅРЅС‹Рј `prev` Рё
/// РІС‹РґР°Р» Р±С‹ СЃС‚СЂР°РЅРёС†Сѓ **РґРІР°** СЃРѕР±С‹С‚РёСЏ РЅР° РѕРґРЅРѕ РёР·РјРµРЅРµРЅРёРµ, СЂРѕРІРЅРѕ С‚Рѕ, С‡С‚Рѕ
/// `content-visibility-auto-state-changed-first-observation.html` Р·Р°РїСЂРµС‰Р°РµС‚
/// (В«already observedВ»). РџРµСЂРІС‹Р№ Р±РѕРєСЃ РІ РїРѕСЂСЏРґРєРµ РґРµСЂРµРІР° вЂ” СЃР°Рј СЌР»РµРјРµРЅС‚, Р°РЅРѕРЅРёРјРЅС‹Р№
/// РІСЃРµРіРґР° РµРіРѕ РїРѕС‚РѕРјРѕРє. Layout СЂРµС€Р°РµС‚ С‚Сѓ Р¶Рµ Р·Р°РґР°С‡Сѓ С‚РµРј Р¶Рµ СЃРїРѕСЃРѕР±РѕРј:
/// `CV_SKIPPED` РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚СЃСЏ РїРѕ СѓР·Р»Сѓ.
pub(crate) fn collect_cv_auto(b: &lumen_layout::LayoutBox, out: &mut Vec<(NodeId, f32)>) {
    fn walk(
        b: &lumen_layout::LayoutBox,
        seen: &mut std::collections::HashSet<NodeId>,
        out: &mut Vec<(NodeId, f32)>,
    ) {
        if b.style.content_visibility == lumen_layout::style::ContentVisibility::Auto
            && seen.insert(b.node)
        {
            out.push((b.node, b.rect.y));
        }
        for c in &b.children {
            walk(c, seen, out);
        }
    }
    walk(b, &mut std::collections::HashSet::new(), out);
}

/// Р”РёС„С„ skipped-СЃРѕСЃС‚РѕСЏРЅРёСЏ РјРµР¶РґСѓ РґРІСѓРјСЏ РїСЂРѕС…РѕРґР°РјРё в†’ СЃРѕР±С‹С‚РёСЏ
/// [`ContentVisibilityChange`].
///
/// CSS Contain L2 В§4.1: СЃРѕР±С‹С‚РёРµ РґРѕР»Р¶РЅРѕ РїСЂРёС…РѕРґРёС‚СЊ Рё РЅР° **РїРµСЂРІРѕРµ** РЅР°Р±Р»СЋРґРµРЅРёРµ
/// СЌР»РµРјРµРЅС‚Р°, РІ РѕР±Рµ СЃС‚РѕСЂРѕРЅС‹ вЂ” `skipped: false` РґР»СЏ СЌР»РµРјРµРЅС‚Р° РІРѕ РІСЊСЋРїРѕСЂС‚Рµ РЅРµ РјРµРЅРµРµ
/// РѕР±СЏР·Р°С‚РµР»РµРЅ, С‡РµРј `skipped: true` РґР»СЏ СЌР»РµРјРµРЅС‚Р° РїРѕРґ РЅРёРј. РџРѕСЌС‚РѕРјСѓ СѓР·РµР», РєРѕС‚РѕСЂРѕРіРѕ
/// РІ `prev` РЅРµС‚ РІРѕРІСЃРµ, РІСЃРµРіРґР° РїРѕСЂРѕР¶РґР°РµС‚ СЃРѕР±С‹С‚РёРµ СЃРѕ СЃРІРѕРёРј С‚РµРєСѓС‰РёРј СЃРѕСЃС‚РѕСЏРЅРёРµРј, Р°
/// СѓР·РµР», РєРѕС‚РѕСЂС‹Р№ РёР· РґРµСЂРµРІР° РёСЃС‡РµР·, вЂ” РЅРёРєР°РєРѕРіРѕ: РѕС‚СЃРѕРµРґРёРЅС‘РЅРЅС‹Р№ СЌР»РµРјРµРЅС‚ РјРѕР»С‡РёС‚
/// (`content-visibility-auto-state-changed-removed.html`).
///
/// `next` вЂ” РІ РїРѕСЂСЏРґРєРµ РґРµСЂРµРІР°, С‡С‚РѕР±С‹ РїРѕСЂСЏРґРѕРє СЃРѕР±С‹С‚РёР№ РЅРµ Р·Р°РІРёСЃРµР» РѕС‚ РѕР±С…РѕРґР° С…РµС€Р°.
pub(crate) fn diff_cv_state(
    prev: &std::collections::HashMap<NodeId, bool>,
    next: &[(NodeId, bool)],
) -> Vec<ContentVisibilityChange> {
    let mut out = Vec::new();
    for &(node, skipped) in next {
        if prev.get(&node) != Some(&skipped) {
            out.push(ContentVisibilityChange { node, skipped });
        }
    }
    out
}

/// Extract `initial-scale` from the `<meta name=viewport>` of a page's document.
///
/// Returns `1.0` when the page has no viewport meta or omits `initial-scale`.
pub(crate) fn meta_initial_scale(src: &LayoutSource) -> f32 {
    src.document
        .lock()
        .ok()
        .and_then(|doc| doc.viewport_meta().map(|m| m.initial_scale))
        .unwrap_or(1.0)
}
