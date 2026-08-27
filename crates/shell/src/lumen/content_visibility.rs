//! What the shell recomputes when the page has moved: which
//! `content-visibility: auto` subtrees are relevant to the user now
//! (CSS Contain L2 4.1) and which decoded images are far enough off screen
//! to drop.
//!
//! Both are scroll-position consumers rather than scroll mechanics, which is
//! why they are not in `crate::lumen::scrolling`: the relevance rule itself is
//! `lumen_layout::cv_is_skipped`, called from here once per frame and from
//! layout for the skip decision, and `contentvisibilityautostatechange` is
//! delivered from `RedrawRequested` because two of the four refresh sites run
//! before a JS context exists (BUG-852).

use crate::*;

impl Lumen {
    /// CSS Containment L3 В§4.4 (BB-4): РѕР±РЅРѕРІРёС‚СЊ skipped-СЃРѕСЃС‚РѕСЏРЅРёРµ
    /// `content-visibility: auto` РїРѕСЃР»Рµ СЃРјРµРЅС‹ `layout_box` вЂ” РїРµСЂРµСЃРєР°РЅРёСЂРѕРІР°С‚СЊ
    /// РґРµСЂРµРІРѕ, Р·Р°РґРёС„С„Р°С‚СЊ СЃ РїСЂРµРґС‹РґСѓС‰РёРј РїСЂРѕС…РѕРґРѕРј, РґРѕР±Р°РІРёС‚СЊ СЃРѕР±С‹С‚РёСЏ РІ `cv_events`.
    /// Р”СЂРµРЅРёСЂСѓРµС‚ thread-local layout-РєСЂРµР№С‚Р°, С‡С‚РѕР±С‹ Р·Р°РїРёСЃРё РЅРµ РїРµСЂРµР¶РёР»Рё РїСЂРѕС…РѕРґ.
    pub(crate) fn refresh_cv_state(&mut self) {
        let _ = lumen_layout::take_cv_skipped();
        let mut auto_boxes = Vec::new();
        if let Some(lb) = self.layout_box.as_ref() {
            collect_cv_auto(lb, &mut auto_boxes);
        }
        // BUG-852: СЃРѕСЃС‚РѕСЏРЅРёРµ СЃС‡РёС‚Р°РµС‚СЃСЏ С‚РµРј Р¶Рµ РїСЂР°РІРёР»РѕРј СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚Рё, С‡С‚Рѕ Рё РІ
        // layout (`cv_is_skipped`), Р° РЅРµ РІС‹РІРѕРґРёС‚СЃСЏ РёР· В«РґРµС‚Рё РїСѓСЃС‚С‹В» вЂ” РёРЅР°С‡Рµ
        // РїСѓСЃС‚РѕР№ auto-СЌР»РµРјРµРЅС‚ РЅРµРѕС‚Р»РёС‡РёРј РѕС‚ РїСЂРѕРїСѓС‰РµРЅРЅРѕРіРѕ.
        let scroll_y = self.scroll_y;
        let viewport_h = self.viewport_height_css();
        let next: Vec<(NodeId, bool)> = auto_boxes
            .iter()
            .map(|&(n, top)| {
                let relevant = self.cv_relevant.contains(&n);
                (n, lumen_layout::cv_is_skipped(relevant, top, scroll_y, viewport_h))
            })
            .collect();
        self.cv_events.extend(diff_cv_state(&self.cv_auto_state, &next));
        // РљР°Рї РѕС‡РµСЂРµРґРё: РґРѕСЃС‚Р°РІРєР° РёРґС‘С‚ СЂР°Р· РІ РєР°РґСЂ, РЅРѕ РєР°РґСЂР° РјРѕР¶РµС‚ Рё РЅРµ Р±С‹С‚СЊ
        // (С„РѕРЅРѕРІР°СЏ РІРєР»Р°РґРєР°) вЂ” С…СЂР°РЅРёРј С‚РѕР»СЊРєРѕ С…РІРѕСЃС‚.
        if self.cv_events.len() > 256 {
            let drop_n = self.cv_events.len() - 256;
            self.cv_events.drain(..drop_n);
        }
        self.cv_auto_state = next.iter().copied().collect();
        self.cv_skipped = auto_boxes
            .into_iter()
            .zip(next)
            .filter_map(|((n, top), (_, skipped))| skipped.then_some((n, top)))
            .collect();
    }

    /// Р”РѕСЃС‚Р°РІРёС‚СЊ РЅР°РєРѕРїР»РµРЅРЅС‹Рµ `contentvisibilityautostatechange` РІ JS.
    ///
    /// Р—РѕРІС‘С‚СЃСЏ СЂР°Р· РІ РєР°РґСЂ РёР· `RedrawRequested` вЂ” С€Р°РіР° В«update the renderingВ»,
    /// РІРЅСѓС‚СЂРё РєРѕС‚РѕСЂРѕРіРѕ CSS Contain L2 В§4.1 Рё РѕРїСЂРµРґРµР»СЏРµС‚ СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚СЊ. РўРѕС‡РєР°
    /// РѕРґРЅР° РЅР° РІСЃРµ РёСЃС‚РѕС‡РЅРёРєРё СЃРѕСЃС‚РѕСЏРЅРёСЏ (Р·Р°РіСЂСѓР·РєР° СЃС‚СЂР°РЅРёС†С‹, СЂРµР»РµР№Р°СѓС‚, ratchet
    /// РїСЂРё СЃРєСЂРѕР»Р»Рµ), РїРѕС‚РѕРјСѓ С‡С‚Рѕ `refresh_cv_state` РІС‹Р·С‹РІР°РµС‚СЃСЏ РёР· С‡РµС‚С‹СЂС‘С… РјРµСЃС‚,
    /// Рё РІ РґРІСѓС… РёР· РЅРёС… JS-РєРѕРЅС‚РµРєСЃС‚ РµС‰С‘ РЅРµ СѓСЃС‚Р°РЅРѕРІР»РµРЅ.
    #[cfg(feature = "v8")]
    pub(crate) fn deliver_cv_state_changes(&mut self) {
        if self.cv_events.is_empty() || !self.js_present {
            // РџРѕРєР° JS-РєРѕРЅС‚РµРєСЃС‚Р° РЅРµС‚, СЃРѕР±С‹С‚РёСЏ РєРѕРїСЏС‚СЃСЏ: СЃС‚СЂР°РЅРёС†Р°, РѕР±СЉСЏРІРёРІС€Р°СЏ
            // `content-visibility: auto` РІ СЂР°Р·РјРµС‚РєРµ, РґРѕР»Р¶РЅР° РїРѕР»СѓС‡РёС‚СЊ РїРµСЂРІРѕРµ
            // РЅР°Р±Р»СЋРґРµРЅРёРµ, РєРѕРіРґР° РµС‘ СЃРєСЂРёРїС‚С‹ СѓР¶Рµ РјРѕРіСѓС‚ СЃР»СѓС€Р°С‚СЊ.
            return;
        }
        let payload: String = {
            let mut s = String::from("[");
            for (i, ev) in self.cv_events.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&format!("[{},{}]", ev.node.index(), ev.skipped));
            }
            s.push(']');
            s
        };
        self.cv_events.clear();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.deliver_cv_state_changes(&payload);
        });
    }

    /// РЁР°Рі 1.6 В«Update the renderingВ»: РµСЃР»Рё РїСЂРё СЃРєСЂРѕР»Р»Рµ РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№
    /// `content-visibility: auto` СѓР·РµР» РІРѕС€С‘Р» РІ СЂР°СЃС€РёСЂРµРЅРЅС‹Р№ viewport вЂ”
    /// ratchet РІ `cv_relevant` + relayout (РµРіРѕ СЃРѕРґРµСЂР¶РёРјРѕРµ РІС‹РєР»Р°РґС‹РІР°РµС‚СЃСЏ).
    ///
    /// BUG-286: routed through [`Self::relayout_raf_dirty`] (not the direct
    /// synchronous [`Self::relayout`]) so this scroll-time trigger gets the
    /// same off-UI-thread treatment as the other `RedrawRequested` relayout
    /// sites once `LUMEN_ENGINE_THREAD=1` вЂ” this was the one caller still
    /// calling `relayout()` directly. No behavior change on the default
    /// (flag-off) build: `relayout_raf_dirty()` falls back to the same
    /// incremental-then-full sequence.
    pub(crate) fn maybe_expand_cv_relevant(&mut self) {
        if self.cv_skipped.is_empty() {
            return;
        }
        let bound = self.scroll_y
            + self.viewport_height_css() * (1.0 + lumen_layout::CV_SLACK_FACTOR);
        let newly: Vec<NodeId> = self
            .cv_skipped
            .iter()
            .filter(|(n, top)| *top <= bound && !self.cv_relevant.contains(n))
            .map(|&(n, _)| n)
            .collect();
        if newly.is_empty() {
            return;
        }
        self.cv_relevant.extend(newly);
        self.relayout_raf_dirty();
    }

    /// Drop CPU-decoded images that have scrolled outside the gate zone (ADR-008 В§10E.4).
    ///
    /// Called once per rendered frame (in `RedrawRequested`) after scroll advancement.
    /// No-op when the cache is empty or the layout tree or renderer is unavailable.
    pub(crate) fn try_discard_offscreen_images(&mut self) {
        let (Some(root), Some(renderer)) = (self.layout_box.as_ref(), self.renderer.as_ref()) else {
            return;
        };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width, vp_size.height);
        scroll::decode_gating::discard_offscreen_images(
            &mut self.image_cache,
            root,
            viewport,
            self.scroll_x,
            self.scroll_y,
        );
    }
}
