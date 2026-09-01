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
    /// CSS Containment L3 §4.4 (BB-4): обновить skipped-состояние
    /// `content-visibility: auto` после смены `layout_box` — пересканировать
    /// дерево, задиффать с предыдущим проходом, добавить события в `cv_events`.
    /// Дренирует thread-local layout-крейта, чтобы записи не пережили проход.
    pub(crate) fn refresh_cv_state(&mut self) {
        let _ = lumen_layout::take_cv_skipped();
        let mut auto_boxes = Vec::new();
        if let Some(lb) = self.layout_box.as_ref() {
            collect_cv_auto(lb, &mut auto_boxes);
        }
        // BUG-852: состояние считается тем же правилом релевантности, что и в
        // layout (`cv_is_skipped`), а не выводится из «дети пусты» — иначе
        // пустой auto-элемент неотличим от пропущенного.
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
        // Кап очереди: доставка идёт раз в кадр, но кадра может и не быть
        // (фоновая вкладка) — храним только хвост.
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

    /// Доставить накопленные `contentvisibilityautostatechange` в JS.
    ///
    /// Зовётся раз в кадр из `RedrawRequested` — шага «update the rendering»,
    /// внутри которого CSS Contain L2 §4.1 и определяет релевантность. Точка
    /// одна на все источники состояния (загрузка страницы, релейаут, ratchet
    /// при скролле), потому что `refresh_cv_state` вызывается из четырёх мест,
    /// и в двух из них JS-контекст ещё не установлен.
    #[cfg(feature = "v8")]
    pub(crate) fn deliver_cv_state_changes(&mut self) {
        if self.cv_events.is_empty() || !self.js_present {
            // Пока JS-контекста нет, события копятся: страница, объявившая
            // `content-visibility: auto` в разметке, должна получить первое
            // наблюдение, когда её скрипты уже могут слушать.
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

    /// Шаг 1.6 «Update the rendering»: если при скролле пропущенный
    /// `content-visibility: auto` узел вошёл в расширенный viewport —
    /// ratchet в `cv_relevant` + relayout (его содержимое выкладывается).
    ///
    /// BUG-286: routed through [`Self::relayout_raf_dirty`] (not the direct
    /// synchronous [`Self::relayout`]) so this scroll-time trigger gets the
    /// same off-UI-thread treatment as the other `RedrawRequested` relayout
    /// sites once `LUMEN_ENGINE_THREAD=1` — this was the one caller still
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

    /// Drop CPU-decoded images that have scrolled outside the gate zone (ADR-008 §10E.4).
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
