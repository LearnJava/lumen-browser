//! Tab / Shift+Tab sequential focus navigation on the page (FRAME-7 срез 2:
//! Tab-driven focus change did not exist at all before this slice), WITHIN a
//! frame's own document (срез 4), and — срез 5 — the page→frame ENTRY point:
//! Tab landing on an `<iframe>` host now continues into the frame's own
//! first/last focusable field instead of stopping on the host element.
//!
//! The order itself is pure DOM logic in [`crate::focus_nav`]; this module
//! is the side-effecting glue that mirrors what a click does to
//! `focused_node`/`focused_frame` (`click.rs::handle_click_at_inner`) so
//! `:focus`/`:focus-within` restyle, the platform accessibility bridge and
//! `document.activeElement` all stay in sync however focus moved.
//!
//! [`Lumen::advance_frame_focus`] (срез 4) walks the frame's OWN document —
//! `set_interactive_state`/relayout-on-focus-change (BUG-480 срез 23) already
//! makes that restyle visible, so this slice only needed the traversal glue,
//! not a new painting path. The remaining FRAME-7 gap is the CARET bar
//! itself inside a frame field — a separate, larger slice (the
//! `CompositorOverride` channel a page caret rides is not wired to a frame's
//! `content_dl` at all, see `STATUS-P1.md`'s FRAME-7 row).

use crate::*;

/// CSS Overflow L5 §3.1.10/§3.3.3: one entry of the merged sequential focus
/// order — a DOM node (with its radio-collapse stop flag) or a
/// scroll-control pseudo-element stop (`::scroll-marker`/`::scroll-button()`).
#[derive(Clone, Copy)]
enum FocusItem {
    Dom { node: lumen_dom::NodeId, stop: bool },
    Pseudo { origin: lumen_dom::NodeId, kind: u8, container: lumen_dom::NodeId },
}

impl Lumen {
    /// CSS UI L4 §3.3: record the shell's keyboard/pointer input modality for
    /// `:focus-visible` and mirror it to the engine thread's same-tick style
    /// flush (`getComputedStyle`/`matches`), which never sees this field
    /// directly — the painted relayout reads it via `set_keyboard_focus`.
    pub(crate) fn set_focus_modality(&mut self, keyboard: bool) {
        self.focus_from_keyboard = keyboard;
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_keyboard_focus(keyboard);
        });
    }

    /// Move `focused_node` to the next (`forward`) or previous focusable
    /// element in the page's Tab order, wrapping around, and apply the same
    /// side effects a click does when it changes focus. Returns `true` iff
    /// focus actually moved (a redraw is warranted) — `false` for a page
    /// with no focusable elements, or one whose single focusable element was
    /// already focused.
    pub(crate) fn advance_page_focus(&mut self, forward: bool) -> bool {
        // A queued `element.focus()` is OLDER than this Tab press — apply it
        // before computing `current`, or the stale `focused_node` both picks
        // the wrong successor AND gets overwritten back by the late drain
        // inside `inject_key_events` (WPT radio-focus-navigation-checked).
        #[cfg(feature = "v8")]
        self.drain_js_focus_requests_for_input();
        let Some(src) = self.layout_source.as_ref() else {
            return false;
        };
        // CSS Overflow L5 §3.1.10: the Tab press right after scroll-marker
        // ACTIVATION does not continue the pseudo-element order — it focuses
        // the scroll target when that is a tab stop, else the next tab stop
        // after it in tree order (WPT `scroll-marker-next-focus`).
        if self.pseudo_nav_pending
            && let Some((origin, ..)) = self.focused_pseudo
        {
            self.clear_pseudo_focus();
            return self.focus_after_marker_activation(origin, forward);
        }
        let target = {
            let doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            let items = self.merged_focus_items(&doc, &flat_tree);
            Self::next_focus_item(&items, self.focused_node, self.focused_pseudo, forward)
        };
        let Some(target) = target else {
            return false;
        };
        match target {
            FocusItem::Pseudo { origin, kind, container } => {
                self.focus_pseudo_item(origin, kind, container)
            }
            FocusItem::Dom { node, .. } => {
                // Landing on a real element ends pseudo-element focus (§3.3.2:
                // `:focus*` return to the element mirrors).
                self.clear_pseudo_focus();
                self.focus_page_node(node, forward)
            }
        }
    }

    /// §3.1.10 continuation: focus `target` (the activated marker's scroll
    /// target) when it is itself a tab stop, otherwise the next/previous tab
    /// stop in flat-tree order relative to it.
    fn focus_after_marker_activation(
        &mut self,
        target: lumen_dom::NodeId,
        forward: bool,
    ) -> bool {
        let next = {
            let Some(src) = self.layout_source.as_ref() else {
                return false;
            };
            let doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            if focus_nav::is_sequential_tab_stop(doc.get(target)) {
                Some(target)
            } else {
                Self::next_tab_stop_in_tree_order(&doc, &flat_tree, doc.root(), target, forward)
            }
        };
        match next {
            Some(n) => self.focus_page_node(n, forward),
            None => false,
        }
    }

    /// First node in flat-tree (pre-order) order strictly after `from` that
    /// is a sequential tab stop. `from`'s own descendants are scanned too —
    /// they follow it in tree order, exactly where sequential navigation
    /// would look "as though it were focused" (WPT `scroll-marker-next-focus`:
    /// a `tabindex="-1"` section's inner `<button>` is the next stop).
    fn next_tab_stop_in_tree_order(
        doc: &lumen_dom::Document,
        flat_tree: &lumen_dom::FlatTree,
        root: lumen_dom::NodeId,
        from: lumen_dom::NodeId,
        forward: bool,
    ) -> Option<lumen_dom::NodeId> {
        let mut order: Vec<lumen_dom::NodeId> = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            order.push(id);
            for &c in flat_tree.children_of(doc, id).iter().rev() {
                stack.push(c);
            }
        }
        let pos = order.iter().position(|&n| n == from)?;
        let iter: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(pos + 1..order.len())
        } else {
            Box::new((0..pos).rev())
        };
        iter.map(|i| order[i])
            .find(|&n| focus_nav::is_sequential_tab_stop(doc.get(n)))
    }

    /// CSS Overflow L5 §3.3.3 — the merged sequential focus order: DOM tab
    /// stops (positive-`tabindex` section first, then tree order) with the
    /// scroll-control pseudo-element stops spliced in at their box-tree
    /// positions: a `before` marker-group's markers and the scroll buttons
    /// precede the container's own position, an `after` group's markers
    /// follow its whole subtree.
    fn merged_focus_items(
        &self,
        doc: &lumen_dom::Document,
        flat_tree: &lumen_dom::FlatTree,
    ) -> Vec<FocusItem> {
        let (seq, stops, positive_count) = focus_nav::build_focus_seq(doc, flat_tree, doc.root());
        let (pre, size) = Self::tree_ranks(doc, flat_tree);
        let n_ids = doc.len() as f64;
        // Pseudo stops share the zero-tabindex section's rank space (lifted
        // by `n_ids` past the positive section): before-group markers and
        // buttons land in (pre-1, pre), after-group markers in (exit-1, exit)
        // — the fractional slot right before the container's own rank / the
        // rank of the node following its subtree.
        let mut ranked: Vec<(f64, FocusItem)> = Vec::new();
        for (i, &n) in seq.iter().enumerate() {
            let idx = n.index();
            let rank = if i < positive_count {
                // Positive-tabindex section: keep it ahead of everything,
                // ordered exactly as `build_focus_seq` produced.
                i as f64
            } else {
                pre.get(idx).copied().unwrap_or(u32::MAX) as f64 + n_ids
            };
            ranked.push((rank, FocusItem::Dom { node: n, stop: stops[i] }));
        }
        for (rank, origin, kind, container) in self.collect_pseudo_stops(doc, &pre, &size) {
            ranked.push((
                rank + n_ids,
                FocusItem::Pseudo { origin, kind, container },
            ));
        }
        ranked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        ranked.into_iter().map(|(_, it)| it).collect()
    }

    /// Pre-order index and subtree size per node index — the anchors pseudo
    /// stops splice against (§3.3.3 positions are relative to the container
    /// element's own tree slot, not to any focusable's).
    fn tree_ranks(
        doc: &lumen_dom::Document,
        flat_tree: &lumen_dom::FlatTree,
    ) -> (Vec<u32>, Vec<u32>) {
        let n_ids = doc.len();
        let mut pre = vec![u32::MAX; n_ids];
        let mut size = vec![0u32; n_ids];
        let mut order: Vec<lumen_dom::NodeId> = Vec::new();
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            let idx = id.index();
            if idx < n_ids {
                pre[idx] = order.len() as u32;
            }
            order.push(id);
            for &c in flat_tree.children_of(doc, id).iter().rev() {
                stack.push(c);
            }
        }
        for &id in order.iter().rev() {
            let idx = id.index();
            if idx >= n_ids {
                continue;
            }
            let mut s = 1u32;
            for &c in flat_tree.children_of(doc, id) {
                s = s.saturating_add(size[c.index().min(n_ids - 1)]);
            }
            size[idx] = s;
        }
        (pre, size)
    }

    /// Collect `(fractional rank in tree-index space, origin, kind,
    /// container)` pseudo stops from the laid-out box tree. Marker order
    /// inside a group is already tree order (CSS Overflow L5 §3.1.5);
    /// buttons sort by their fixed creation order (block-start,
    /// inline-start, inline-end, block-end — the `BUTTONS` table in
    /// `box_tree::scroll_pseudos`).
    fn collect_pseudo_stops(
        &self,
        doc: &lumen_dom::Document,
        pre: &[u32],
        size: &[u32],
    ) -> Vec<(f64, lumen_dom::NodeId, u8, lumen_dom::NodeId)> {
        let Some(lb) = self.layout_box.as_ref() else {
            return Vec::new();
        };
        let rank_of = |nid: lumen_dom::NodeId| -> Option<f64> {
            let i = nid.index();
            (i < pre.len() && pre[i] != u32::MAX).then(|| pre[i] as f64)
        };
        let exit_of = |nid: lumen_dom::NodeId| -> Option<f64> {
            let i = nid.index();
            (i < pre.len() && pre[i] != u32::MAX).then(|| (pre[i] + size[i]) as f64)
        };
        let _ = doc;

        // Walk the box tree once: groups carry their marker targets inside;
        // buttons are siblings collected per originating element.
        struct Group {
            container: lumen_dom::NodeId,
            markers: Vec<lumen_dom::NodeId>,
            placement_after: bool,
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut buttons: Vec<(lumen_dom::NodeId, u8)> = Vec::new();
        // CSS Overflow L5 §3.2: `:disabled` `::scroll-button()` — не tab-стоп
        // (WPT `scroll-button-disabled-no-focus`). Края прокрутки считаются
        // один раз по текущему дереву.
        let edges = self
            .relayout_viewport()
            .map(|vp| lumen_layout::scroll_state_edges(lb, vp))
            .unwrap_or_default();
        fn walk(
            b: &lumen_layout::LayoutBox,
            groups: &mut Vec<Group>,
            buttons: &mut Vec<(lumen_dom::NodeId, u8)>,
            lb_root: &lumen_layout::LayoutBox,
            edges: &std::collections::HashMap<u32, lumen_layout::EdgeFlags>,
        ) {
            use lumen_layout::{BoxRole, PseudoKind};
            match b.origin.role {
                BoxRole::Pseudo(PseudoKind::ScrollMarkerGroup) => {
                    let container = b.origin.node.unwrap_or(b.node);
                    let mut markers = Vec::new();
                    fn collect_markers(
                        b: &lumen_layout::LayoutBox,
                        out: &mut Vec<lumen_dom::NodeId>,
                    ) {
                        if matches!(b.origin.role, BoxRole::Pseudo(PseudoKind::ScrollMarker))
                            && let Some(t) = b.origin.node
                        {
                            out.push(t);
                        }
                        for c in &b.children {
                            collect_markers(c, out);
                        }
                    }
                    collect_markers(b, &mut markers);
                    let placement_after = crate::forms::find_layout_box(lb_root, container)
                        .and_then(|cb| {
                            use lumen_layout::style::ScrollMarkerGroupPlacement as P;
                            cb.style.scroll_marker_group.map(|g| g.placement == P::After)
                        })
                        .unwrap_or(false);
                    groups.push(Group { container, markers, placement_after });
                }
                BoxRole::Pseudo(
                    k @ (PseudoKind::ScrollButtonBlockStart
                    | PseudoKind::ScrollButtonInlineStart
                    | PseudoKind::ScrollButtonInlineEnd
                    | PseudoKind::ScrollButtonBlockEnd),
                ) if lumen_layout::scroll_button_box_enabled(edges, b) => {
                    buttons.push((b.origin.node.unwrap_or(b.node), k as u8));
                }
                _ => {}
            }
            for c in &b.children {
                walk(c, groups, buttons, lb_root, edges);
            }
        }
        walk(lb, &mut groups, &mut buttons, lb, &edges);

        let mut out = Vec::new();
        // Per container: before-group markers + buttons slot into
        // (pre-1, pre); after-group markers into (exit-1, exit) — both in
        // tree-index space, `merged_focus_items` lifts them into the
        // zero-tabindex rank space.
        let mut before_by_container: std::collections::HashMap<
            lumen_dom::NodeId,
            Vec<(lumen_dom::NodeId, u8)>,
        > = std::collections::HashMap::new();
        for g in &groups {
            if g.placement_after {
                let Some(exit) = exit_of(g.container) else { continue };
                let n = g.markers.len().max(1) as f64;
                for (i, &t) in g.markers.iter().enumerate() {
                    let rank = exit - 1.0 + (i as f64 + 1.0) / (n + 1.0);
                    out.push((rank, t, lumen_layout::PseudoKind::ScrollMarker as u8, g.container));
                }
            } else {
                let v = before_by_container.entry(g.container).or_default();
                for &t in &g.markers {
                    v.push((t, lumen_layout::PseudoKind::ScrollMarker as u8));
                }
            }
        }
        buttons.sort_by_key(|&(_, k)| k);
        for (container, kind) in buttons {
            before_by_container
                .entry(container)
                .or_default()
                .push((container, kind));
        }
        for (container, items) in before_by_container {
            let Some(pre_c) = rank_of(container) else { continue };
            let n = items.len() as f64;
            for (i, (origin, kind)) in items.iter().enumerate() {
                let rank = pre_c - 1.0 + (i as f64 + 1.0) / (n + 1.0);
                out.push((rank, *origin, *kind, container));
            }
        }
        out
    }

    /// Next item in `items` after the current focus position, skipping
    /// non-stop DOM entries (collapsed radio members). `current_pseudo`
    /// addresses a pseudo stop directly; a `focused_node` that is not in the
    /// order starts from the boundary, same as `next_focus_target`.
    fn next_focus_item(
        items: &[FocusItem],
        current_node: Option<lumen_dom::NodeId>,
        current_pseudo: Option<(lumen_dom::NodeId, u8, lumen_dom::NodeId)>,
        forward: bool,
    ) -> Option<FocusItem> {
        let n = items.len();
        if n == 0 {
            return None;
        }
        let cur = if let Some((o, k, c)) = current_pseudo {
            items.iter().position(|it| {
                matches!(it, FocusItem::Pseudo { origin, kind, container }
                    if *origin == o && *kind == k && *container == c)
            })
        } else {
            current_node.and_then(|cn| {
                items.iter().position(|it| matches!(it, FocusItem::Dom { node, .. } if *node == cn))
            })
        };
        let step: isize = if forward { 1 } else { -1 };
        let mut i = match cur {
            Some(i) => i as isize,
            None => {
                if forward { -1 } else { n as isize }
            }
        };
        for _ in 0..n {
            i += step;
            if i < 0 {
                i = n as isize - 1;
            } else if i >= n as isize {
                i = 0;
            }
            let stop = match &items[i as usize] {
                FocusItem::Pseudo { .. } => true,
                FocusItem::Dom { stop, .. } => *stop,
            };
            if stop {
                return Some(items[i as usize]);
            }
        }
        None
    }

    /// Focus a pseudo-element tab stop WITHOUT activation — sets the
    /// pseudo-focus mirrors (`:focus`/`:focus-visible` resolve on the pseudo,
    /// `document.activeElement` reads the scroll container) and records it as
    /// keyboard-modality focus.
    fn focus_pseudo_item(
        &mut self,
        origin: lumen_dom::NodeId,
        kind: u8,
        container: lumen_dom::NodeId,
    ) -> bool {
        self.pseudo_nav_pending = false;
        self.set_pseudo_focus_state(origin, kind, container);
        self.set_focus_modality(true);
        self.status_bubble_sync(false);
        self.relayout_chrome();
        self.platform_bridge.focused_node_changed(self.focused_node);
        self.request_redraw();
        true
    }

    /// Record pseudo-element focus: `focused_node` becomes the scroll
    /// container (§3.3.1 `document.activeElement`), `focused_pseudo` carries
    /// the pseudo identity for `set_pseudo_focus`/`:focus*` matching, and the
    /// JS mirrors are notified. Shared by Tab stops and marker activation.
    pub(crate) fn set_pseudo_focus_state(
        &mut self,
        origin: lumen_dom::NodeId,
        kind: u8,
        container: lumen_dom::NodeId,
    ) {
        self.focused_pseudo = Some((origin, kind, container));
        self.focused_node = Some(container);
        self.notify_pseudo_focus_change();
        let focus_idx = Some(container.index() as u32);
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_focus_changed(focus_idx);
        });
    }

    /// Drop pseudo-element focus state without assigning a new
    /// `focused_node` — callers that move element focus themselves (click,
    /// `element.focus()`, navigation, Tab onto a DOM node) invoke this so
    /// `:focus*` mirrors and the JS side stop resolving the pseudo-element.
    pub(crate) fn clear_pseudo_focus(&mut self) {
        if self.focused_pseudo.take().is_none() && !self.pseudo_nav_pending {
            return;
        }
        self.pseudo_nav_pending = false;
        self.notify_pseudo_focus_change();
    }

    /// Push the current `focused_pseudo` to the engine thread's same-tick
    /// style mirror (`::scroll-marker:focus` in `getComputedStyle`/`matches`).
    pub(crate) fn notify_pseudo_focus_change(&self) {
        let pf = self.focused_pseudo.map(|(o, k, c)| {
            (o.index() as u32, k, c.index() as u32)
        });
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_pseudo_focus(pf);
        });
    }

    /// Put page focus on `next` with the side effects a click has (restyle,
    /// accessibility bridge, `document.activeElement`, `focus` events), and
    /// continue into an `<iframe>` host's own order. Shared by Tab navigation
    /// and `accesskey` activation. `false` iff `next` already had focus.
    pub(crate) fn focus_page_node(&mut self, next: lumen_dom::NodeId, forward: bool) -> bool {
        // Any element-level focus move ends pseudo-element focus first —
        // covers accesskey and other non-Tab callers of this helper.
        self.clear_pseudo_focus();
        if Some(next) == self.focused_node && self.focused_frame.is_none() {
            return false;
        }
        // WPT `active-onblur`: a keyboard-held `:active` belongs to the element
        // that had focus — moving focus releases it even while the key is down.
        if self.active_from_keyboard {
            self.active_from_keyboard = false;
            self.active_nid = None;
            self.sync_active_mirror();
        }
        self.focused_node = Some(next);
        self.set_focus_modality(true);
        self.status_bubble_sync(false);
        let before_frame = self.focused_frame.take();
        if before_frame.is_some() {
            self.notify_frame_focus(before_frame, None);
            self.refresh_frames(None);
        }
        // Same reasoning as `click.rs`'s focus-change branch: a pure restyle,
        // so it goes off-thread under `LUMEN_ENGINE_THREAD=1` and stays a
        // synchronous UI-handle call otherwise.
        self.relayout_chrome();
        self.platform_bridge.focused_node_changed(self.focused_node);
        let focus_idx = self.focused_node.map(|n| n.index() as u32);
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_focus_changed(focus_idx);
        });
        // FRAME-7 срез 5: `next` may itself be a top-level `<iframe>` host —
        // per HTML Standard §6.6.6 the frame's OWN order is spliced in right
        // there, so Tab must continue into it instead of stopping on the
        // container. `before_frame` was cleared above (`None`), so
        // `advance_frame_focus`'s `current = None` lookup lands on the
        // frame's first (`forward`) or last (`!forward`) entry — the same
        // "no current position → start of order" branch
        // `focus_nav::next_focus_target_no_wrap` uses when a click hands it a
        // frame with no prior focus. A frame with no focusable field at all
        // leaves focus on the host, same as before this slice. Nested frames
        // (a frame's own child `<iframe>`) are out of scope here — only the
        // page's own `self.frames` entries with `parent_doc.is_none()` are a
        // page-level host.
        if let Some(idx) = self
            .frames
            .iter()
            .position(|f| f.host == next && f.parent_doc.is_none())
        {
            self.advance_frame_focus(idx, forward);
        }
        true
    }

    /// Move focus to the next (`forward`) or previous focusable node WITHIN
    /// frame `idx`'s own document (FRAME-7 срез 4) — the nested browsing
    /// context's own composed-order walk, HTML Standard §6.6.6.
    ///
    /// Deliberately non-wrapping ([`focus_nav::next_focus_target_no_wrap`]):
    /// `false` once the frame's own order is exhausted (or it has no
    /// focusable node at all), so the caller (`keyboard.rs`'s Tab handler)
    /// falls back to [`Self::advance_page_focus`] to leave the frame —
    /// `self.focused_node` still addresses the `<iframe>` host at that point
    /// (set by the click that entered the frame, or — срез 5 — by
    /// `advance_page_focus` landing Tab on the host directly), so that call
    /// lands on exactly the page-level sibling before/after it, same as it
    /// always did for a frame with zero focusable fields.
    pub(crate) fn advance_frame_focus(&mut self, idx: usize, forward: bool) -> bool {
        // Same stale-queue rule as `advance_page_focus`: a pending
        // `element.focus()` predates this Tab press.
        #[cfg(feature = "v8")]
        self.drain_js_focus_requests_for_input();
        let Some(handle) = self.frames.get(idx) else {
            return false;
        };
        let current = self.focused_frame.and_then(|(i, n)| (i == idx).then_some(n));
        let next = {
            let doc = handle.doc.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            focus_nav::next_focus_target_no_wrap(&doc, &flat_tree, doc.root(), current, forward)
        };
        let Some(next) = next else {
            return false;
        };
        let before = self.focused_frame;
        self.focused_frame = Some((idx, next));
        self.notify_frame_focus(before, self.focused_frame);
        self.refresh_frames(None);
        true
    }
}
