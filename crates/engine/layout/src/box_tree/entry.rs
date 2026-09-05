use super::*;

/// CSS Scoping L1 — collect each shadow tree's author stylesheet, keyed by its
/// shadow-host `NodeId`, from the `<style>` elements inside every shadow root.
///
/// These sheets are installed via [`crate::style::set_shadow_sheets`] at the start
/// of every layout pass so the cascade can apply `:host`/`:host()`/`::slotted()`
/// rules in their proper scope. The page's document `<style>` is collected
/// separately by the shell (`extract_style_blocks`), which does NOT descend into
/// shadow roots — so the two collections never overlap.
fn build_shadow_sheets(doc: &Document) -> std::collections::HashMap<NodeId, Stylesheet> {
    let mut map = std::collections::HashMap::new();
    if doc.is_empty() {
        return map;
    }
    for i in 0..doc.len() {
        let host = NodeId::from_index(i);
        if !doc.is_shadow_host(host) {
            continue;
        }
        let Some(sr) = doc.shadow_root_of(host) else { continue };
        let mut css = String::new();
        collect_shadow_style_css(doc, sr, &mut css);
        if !css.trim().is_empty() {
            map.insert(host, lumen_css_parser::parse(&css));
        }
    }
    map
}

/// Concatenate the text of all `<style>` elements within a shadow subtree.
/// Walks DOM children only; nested shadow roots are not DOM children, so a nested
/// host's own `<style>` stays in its own scope (collected by the outer loop).
fn collect_shadow_style_css(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        collect_shadow_style_css(doc, child, out);
    }
}

/// Lay out a document without a text measurer. For tests and headless dump modes.
/// Invalidates the rule-index cache before the cascade so stale hits are impossible.
pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    // Prevent stale RULE_IDX_CACHE hits when a new sheet lands at the same address as a freed one.
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = precompute_counters(doc, sheet, viewport, &flat, false);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, false, None);
    propagate_canvas_background(doc, &mut root);
    let (gw, gx, gh, gy) = propagate_viewport_scrollbar_gutter(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let null_hp = NullHyphenationProvider;
    lay_out(
        &mut root,
        gx,
        gy,
        viewport.width - gw,
        Some(viewport.height - gh),
        None,
        viewport,
        init_pcb,
        &null_hp,
        false,
    );
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, false);
    // CSS Container Queries L1: second pass applies @container rules + re-layout.
    apply_container_styles(&mut root, doc, sheet, viewport, None, &null_hp, false);
    // CSS Anchor Positioning L1: post-layout pass repositions anchored elements.
    apply_anchor_positions(&mut root, viewport);
    // CSS Pseudo-elements L4 §3.1: split first formatted lines into own boxes (BB-1).
    split_first_line_boxes(&mut root);
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(&root);
    root
}

/// Layout without a text measurer. For tests and headless modes; uses `layout_measured_hyp` with `dark_mode=false`.
pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let null_hp = NullHyphenationProvider;
    layout_measured_hyp(doc, sheet, viewport, measurer, &null_hp, false)
}

/// Like [`layout_measured`], but also returns the [`CounterMap`] (BUG-489) —
/// a caller that feeds the result to [`collect_computed_styles`] needs it so
/// that collector can publish the resolved style of `display: contents`
/// elements, whose own `LayoutBox` `flatten_contents` eliminates from the tree.
pub fn layout_measured_with_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> (LayoutBox, CounterMap) {
    let null_hp = NullHyphenationProvider;
    layout_measured_hyp_with_counters(doc, sheet, viewport, measurer, &null_hp, false)
}

/// Layout with a real hyphenation provider (for `hyphens: auto`).
/// `dark_mode` drives `@media (prefers-color-scheme: dark)` matching throughout
/// the cascade — shell reads the value from `Lumen.dark_mode` (OS preference via winit).
pub fn layout_measured_hyp(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) -> LayoutBox {
    layout_measured_hyp_with_counters(doc, sheet, viewport, measurer, hp, dark_mode).0
}

/// Like [`layout_measured_hyp`], but also returns the [`CounterMap`] the cascade
/// pass produced (BUG-341 S2).
///
/// The `CounterMap` carries the full per-node `ComputedStyle` cascade cache (its
/// `styles` field — see [`CounterMap::styles`]) that `build_box` reused. Persisting
/// it across interaction cycles is the foundation of the incremental cascade
/// (BUG-341 S3+): the incremental path must reproduce this exact map for the same
/// final state, and the `incr == full` differential tests assert that.
///
/// [`layout_measured_hyp`] is a thin wrapper that discards the map, so this
/// function carries the real body and there is no behavioural difference between
/// them.
pub fn layout_measured_hyp_with_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) -> (LayoutBox, CounterMap) {
    let _prof = lumen_core::profile::scope("layout_measured_hyp");
    lumen_core::tracy_zone!("layout_measured_hyp");
    // Invalidate the rule-index cache before each layout pass to prevent
    // stale hits when a new stylesheet lands at the same pointer as a freed one.
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = {
        let _prof = lumen_core::profile::scope("precompute_counters");
        lumen_core::tracy_zone!("precompute_counters");
        precompute_counters(doc, sheet, viewport, &flat, dark_mode)
    };
    let registry = build_counter_style_registry(sheet);
    let mut root = {
        let _prof = lumen_core::profile::scope("build_box");
        lumen_core::tracy_zone!("build_box");
        build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None)
    };
    propagate_canvas_background(doc, &mut root);
    let (gw, gx, gh, gy) = propagate_viewport_scrollbar_gutter(doc, &mut root);
    // CSS Fonts L5 §4 — resolve `font-size-adjust` against the real font x-height
    // before measurement, so both line wrapping and paint use the scaled size.
    apply_font_size_adjust(&mut root, measurer);
    // FONTLOAD-14 (BUG-467): resolve `line-height: normal` from real font
    // metrics — after font-size-adjust (so it sees the adjusted size), before
    // `lay_out` (paint/hit-test/selection read `LayoutBox::used_line_height`
    // without a measurer of their own).
    resolve_used_line_height(&mut root, measurer);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    {
        let _prof = lumen_core::profile::scope("lay_out");
        lumen_core::tracy_zone!("lay_out");
        lay_out(
            &mut root,
            gx,
            gy,
            viewport.width - gw,
            Some(viewport.height - gh),
            Some(measurer),
            viewport,
            init_pcb,
            hp,
            false,
        );
    }
    {
        let _prof = lumen_core::profile::scope("post_layout_passes");
        lumen_core::tracy_zone!("post_layout_passes");
        apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
        apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
        // CSS Anchor Positioning L1: post-layout pass repositions anchored elements.
        apply_anchor_positions(&mut root, viewport);
        // CSS Pseudo-elements L4 §3.1: split first formatted lines into own boxes (BB-1).
        split_first_line_boxes(&mut root);
    }
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(&root);
    (root, counters)
}

/// Incremental re-layout pass: skips clean subtrees, re-lays out only dirty ones.
///
/// `root` must be a previously laid-out `LayoutBox` (from `layout_measured_hyp`).
/// Call [`crate::incremental::mark_dirty`] on changed nodes first.
///
/// Internally enables [`INCREMENTAL_LAYOUT_MODE`] so that `lay_out` returns early
/// (translating the subtree to its new position) for any node with
/// [`crate::incremental::DirtyBits::CLEAN`]. After this call all dirty bits are
/// cleared automatically via [`crate::incremental::clear_dirty`].
///
/// Parameters match `lay_out` / `layout_measured_hyp`. Phase 0 limitation:
/// container-query re-evaluation and anchor positioning are not re-run here
/// (they rely on a full layout pass); add a full `layout_measured_hyp` call when
/// those features are required.
#[allow(clippy::too_many_arguments)]
pub fn lay_out_incremental(
    root: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    INCREMENTAL_LAYOUT_MODE.with(|m| m.set(true));
    lay_out(root, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp, false);
    INCREMENTAL_LAYOUT_MODE.with(|m| m.set(false));
    crate::incremental::clear_dirty(root);
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(root);
}

/// Streaming incremental layout (PH1-2b).
///
/// Builds a fresh box tree from `doc` + `sheet` — which, during a streaming load,
/// grows by nodes appended at the end each tick — then reuses laid-out geometry
/// from `prev` (the previous tick's result) for every subtree whose node id, box
/// kind payload and computed style are unchanged. Only new or changed subtrees
/// are re-laid-out; unchanged prefix siblings are repositioned in O(1) by the
/// `lay_out` incremental fast path (a zero-delta translate when content is merely
/// appended below them).
///
/// `prev` must be a tree produced by an earlier `layout_streaming_incremental`
/// or `layout_measured*` call on an ancestor DOM of `doc` (same, stable node ids
/// — the incremental tree builder only appends new ids). When the stylesheet
/// changed since `prev` was built, the per-box style comparison naturally marks
/// the affected boxes dirty and re-lays them out.
///
/// Post-layout passes (container queries, anchor positioning, first-line split)
/// are NOT re-run here — same Phase 0 limitation as [`lay_out_incremental`]. The
/// final `LoadDone` pipeline applies them via a full `layout_measured_hyp`.
#[allow(clippy::too_many_arguments)]
pub fn layout_streaming_incremental(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    prev: &LayoutBox,
) -> LayoutBox {
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = precompute_counters(doc, sheet, viewport, &flat, dark_mode);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None);
    propagate_canvas_background(doc, &mut root);
    apply_font_size_adjust(&mut root, measurer);
    // FONTLOAD-14 (BUG-467): see `layout_measured_hyp` — runs on the fresh
    // tree, before `graft_geometry`, so grafted-clean subtrees keep this
    // pass's freshly-resolved value (graft never copies `used_line_height`
    // from `prev`).
    resolve_used_line_height(&mut root, measurer);
    // Every freshly-built box needs layout; graft clears the bit on reusable
    // subtrees so the incremental pass only re-lays-out new/changed content.
    crate::incremental::mark_subtree_dirty(&mut root);
    crate::incremental::graft_geometry(&mut root, prev);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out_incremental(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp);
    root
}

/// Incremental re-layout for JS DOM mutations (ADR-016 M4).
///
/// Functionally equivalent to [`layout_measured_hyp`] but avoids re-computing
/// geometry for subtrees whose [`crate::style::ComputedStyle`] did not change:
/// the cascade runs in full (same as [`layout_measured_hyp`]), then
/// [`crate::incremental::graft_geometry`] copies laid-out rects from `prev` for
/// unchanged subtrees (marking them [`crate::incremental::DirtyBits::CLEAN`]),
/// and only dirty subtrees are re-laid-out by [`lay_out_incremental`]. All
/// post-layout passes (container queries, anchor positioning, `::first-line`
/// split) run afterwards, matching [`layout_measured_hyp`] semantics exactly.
///
/// Typical speedup: ~10× on a single-node class toggle on a large page (the
/// unchanged siblings are translated in O(k), not re-laid-out). For mutations
/// where every node's style changes (e.g. a viewport-wide media query flip) the
/// overhead of `graft_geometry` is small compared to the full geometry pass.
///
/// `prev` must be a tree produced by an earlier [`layout_measured_hyp`] or
/// `layout_mutation_incremental` call on a compatible DOM (same stable node ids).
/// When `prev` is unavailable (first load) call [`layout_measured_hyp`] instead.
#[allow(clippy::too_many_arguments)]
pub fn layout_mutation_incremental(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    prev: &LayoutBox,
) -> LayoutBox {
    // Full cascade + graft: unchanged-style subtrees become CLEAN.
    let mut root = layout_streaming_incremental(doc, sheet, viewport, measurer, hp, dark_mode, prev);
    // Post-layout passes — same set as layout_measured_hyp, same order.
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
    apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
    apply_anchor_positions(&mut root, viewport);
    split_first_line_boxes(&mut root);
    root
}

/// BUG-341 S5: incremental re-layout for a pure interactive-state transition
/// (`:hover`/`:focus`/`:active`), combining the S3 incremental cascade with
/// [`layout_mutation_incremental`]'s existing geometry-graft reuse.
///
/// Like [`layout_mutation_incremental`], but the cascade itself only
/// re-derives `delta.dirty_roots` and their subtrees
/// ([`crate::counters::incremental_precompute_counters`]) instead of every
/// node — the dominant cost S1's profiling found (brief §1,
/// `precompute_counters` at 53% of the cycle). BUG-341 S15: box-build is
/// skipped too when [`set_incremental_box_build`] is on — whole `LayoutBox`
/// subtrees are cloned from `prev` wherever [`CounterMap::clean_subtrees`]
/// licenses it ([`incremental_build_box`]) — and [`crate::incremental::
/// graft_geometry`] then reuses the layout geometry the same way it already
/// did. With the flag off this builds fresh boxes with the plain `build_box`,
/// exactly like [`layout_streaming_incremental`] does.
///
/// `delta.prev_styles` must be the [`CounterMap::styles`] this same document
/// produced on the previous cycle (this function's own returned `CounterMap`,
/// or [`layout_measured_hyp_with_counters`] for the first cycle). The caller
/// is responsible for only using this entry point when nothing besides
/// interactive state changed since `prev` — `delta.dom_content_stable` must be
/// `true` (a DOM/attribute mutation can change content `build_box` reads,
/// e.g. text or attribute values, in ways a style-only comparison does not
/// catch) and `delta.dirty_roots` should come from
/// [`crate::style::restyle_root_set_for_state_change`]. See
/// [`crate::counters::RestyleDelta`]'s own doc comment for the full
/// correctness precondition.
///
/// BUG-341 S19: `prev` is taken **by value** — this pass moves the reusable
/// subtrees out of it into the tree it returns rather than copying them, so the
/// previous tree does not survive the call. Callers persist the *returned* tree
/// as the next cycle's `prev`; one that also needs the old tree afterwards must
/// clone it before handing it over.
///
/// Returns the fresh `CounterMap` alongside the tree so the caller can carry
/// its `styles()` forward as the next cycle's `prev_styles`.
#[allow(clippy::too_many_arguments)]
pub fn layout_mutation_incremental_restyle(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    mut prev: LayoutBox,
    delta: crate::counters::RestyleDelta<'_>,
) -> (LayoutBox, CounterMap) {
    // Stage scopes deliberately reuse `layout_measured_hyp_with_counters`'
    // names so `LUMEN_PROFILE_TREE=1` yields a directly comparable split of
    // the *incremental* path — BUG-341 §1's profile only ever described the
    // full pass, and every slice since S3 has been reasoning from it.
    let _prof = lumen_core::profile::scope("layout_mutation_incremental_restyle");
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = {
        let _prof = lumen_core::profile::scope("build_flat_tree");
        build_flat_tree(doc)
    };
    {
        // BUG-341 S26: scoped because it is per-pass whole-document work that
        // the delta cannot shrink — it walks every node asking `is_shadow_host`,
        // with no `shadow_roots.is_empty()` fast path of its own (unlike
        // `build_flat_tree`). Unscoped, it was invisible to every stage profile
        // this track has taken.
        let _prof = lumen_core::profile::scope("build_shadow_sheets");
        crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    }
    let counters = {
        let _prof = lumen_core::profile::scope("precompute_counters");
        crate::counters::incremental_precompute_counters(doc, sheet, viewport, &flat, dark_mode, delta)
    };
    let registry = {
        // BUG-341 S26: likewise per-pass, sheet-wide and delta-independent.
        let _prof = lumen_core::profile::scope("counter_style_registry");
        build_counter_style_registry(sheet)
    };
    let mut root = {
        let _prof = lumen_core::profile::scope("build_box");
        // BUG-341 S15: reuse whole `LayoutBox` subtrees from `prev` wherever
        // `CounterMap::clean_subtrees` licenses it (the S4 mechanism), instead
        // of rebuilding a tree that `graft_geometry` is about to graft straight
        // back onto the previous geometry. S4's own measurement rejected this
        // because `index_by_node`'s whole-prev-tree hash outweighed the ~8%
        // `build_box` share it saved; both halves of that trade have since
        // moved — `build_box` is now ~60% of the incremental cycle (S14's
        // profile) and, after S13/S14, the dirty set on a chrome interaction is
        // empty, so `clean_subtrees` licenses nearly the whole tree.
        // BUG-341 S19: this is where `prev` is consumed — the reusable subtrees
        // are moved into the tree being built, not copied out of it.
        if incremental_box_build_enabled() {
            incremental_build_box(
                doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, &mut prev,
            )
        } else {
            build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None)
        }
    };
    {
        // BUG-341 S26: two whole-tree walks between the build and graft stages,
        // both previously unscoped.
        let _prof = lumen_core::profile::scope("post_build_tree_walks");
        propagate_canvas_background(doc, &mut root);
        apply_font_size_adjust(&mut root, measurer);
        // FONTLOAD-14 (BUG-467): see `layout_measured_hyp` / `layout_streaming_incremental`.
        resolve_used_line_height(&mut root, measurer);
    }
    {
        let _prof = lumen_core::profile::scope("graft_geometry");
        // Every freshly-built box needs layout; graft clears the bit on reusable
        // subtrees so the incremental pass only re-lays-out new/changed content.
        crate::incremental::mark_subtree_dirty(&mut root);
        // BUG-341 S13: `prev` is a laid-out tree, so its styles carry the used
        // values `lay_out` wrote back into them; `delta.prev_styles` is the
        // unpolluted cascade those boxes were built from, and lets the graft
        // tell "the author's style changed" from "layout wrote its own output
        // here last cycle". Without it, 81 of the chrome document's 318 boxes
        // were rejected on style every single hover flip — every one of them
        // differing only in those used-value fields — plus 41 ancestors
        // dragged along by the reject propagation.
        // BUG-341 S19: `prev` is a husked tree by now — the reusable subtrees
        // are in `root`, and every position they came from carries
        // `DirtyBits::MOVED_OUT`. The graft skips those on the S18 claim before
        // it ever looks at the husk, and rejects any it reaches without one.
        // BUG-341 S24: the delta's cache was moved into `counters` and rewritten
        // in place, so the "what was `prev` built from" view now comes from
        // there — live entries for everything this pass reused, displaced ones
        // for everything it recomputed.
        crate::incremental::graft_geometry_with_cascade(&mut root, &prev, Some(counters.prev_cascade()));
    }
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    {
        let _prof = lumen_core::profile::scope("lay_out");
        lay_out_incremental(
            &mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp,
        );
    }
    {
        let _prof = lumen_core::profile::scope("post_layout_passes");
        // Post-layout passes — same set as layout_measured_hyp/layout_mutation_incremental, same order.
        apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
        apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
        apply_anchor_positions(&mut root, viewport);
        split_first_line_boxes(&mut root);
    }
    (root, counters)
}

/// CSS Fonts L5 §4 — used `font-size` after applying `font-size-adjust`.
///
/// The aspect value of the rendered font is `x_height_px(size) / size`. To make
/// the text's x-height equal `adjust × size`, the size is scaled by
/// `adjust / aspect`. `None` (initial) and `Auto` (use the first available
/// font's own aspect — a no-op for a single font) leave the size unchanged.
pub(crate) fn font_size_adjust_used(style: &ComputedStyle, m: &dyn TextMeasurer) -> f32 {
    use crate::style::FontSizeAdjust;
    let size = style.font_size;
    match style.font_size_adjust {
        FontSizeAdjust::None | FontSizeAdjust::Auto => size,
        FontSizeAdjust::Value(z) => {
            let xh = m.x_height_px(size);
            if size > 0.0 && xh > 0.0 {
                let aspect = xh / size;
                size * z / aspect
            } else {
                size
            }
        }
    }
}

/// Apply `font-size-adjust` to a single style in place (CSS Fonts L5 §4).
///
/// Mutates `font_size` to the x-height-normalised used size. Because an absolute
/// `line-height` (`<length>`/`<percentage>`/`em`/`rem`) computes to a fixed line
/// box that must NOT rescale with the used font-size, the ratio-encoded
/// `line_height` is corrected inversely so the absolute line box stays constant
/// (CSS2 §10.8.1). Relative line-heights (`normal`/`<number>`) keep their ratio
/// and scale with the new size, as the spec requires.
fn apply_font_size_adjust_to_style(style: &mut ComputedStyle, m: &dyn TextMeasurer) {
    use crate::style::FontSizeAdjust;
    if matches!(style.font_size_adjust, FontSizeAdjust::None) {
        return;
    }
    let old_size = style.font_size;
    let new_size = font_size_adjust_used(style, m);
    style.font_size = new_size;
    if !style.line_height_is_relative && new_size > 0.0 {
        style.line_height = style.line_height * old_size / new_size;
    }
}

/// CSS Fonts L5 §4 — post-build pass rewriting `font_size` wherever
/// `font-size-adjust` is a number, using the measurer's real x-height.
///
/// Runs after `build_box` and before `lay_out`: mutating `style.font_size` here
/// makes both inline measurement and the display list (which reads
/// `frag.style.font_size`) pick up the scaled size from a single source. Inline
/// text segments carry their own cloned style, so they are adjusted too.
pub(crate) fn apply_font_size_adjust(b: &mut LayoutBox, m: &dyn TextMeasurer) {
    // BUG-341 S12: the `None` test lives here rather than only inside
    // `apply_font_size_adjust_to_style`, because reaching for `Arc::make_mut`
    // on a style shared with the cascade cache would deep-copy it — on every
    // box of the document, for a property almost no box sets.
    if !matches!(b.style.font_size_adjust, crate::style::FontSizeAdjust::None) {
        apply_font_size_adjust_to_style(Arc::make_mut(&mut b.style), m);
    }
    if let BoxKind::InlineRun { segments, .. } = &mut b.kind {
        for seg in segments.iter_mut() {
            apply_font_size_adjust_to_style(&mut seg.style, m);
        }
    }
    for child in &mut b.children {
        apply_font_size_adjust(child, m);
    }
}

/// CSS2 §10.8.1 — used line-height in px for `style`. Single choke point for
/// what was previously the `font_size * line_height` computation duplicated
/// across ~10 call sites (`layout_dispatch.rs`, `pseudo_text.rs`,
/// `selection.rs`, `text_iter.rs`, `lib.rs`, `vertical.rs`, paint's
/// `text_run.rs`/`hit_test.rs`, shell's `forms.rs`) — all of them now read
/// [`LayoutBox::used_line_height`], written once per layout pass by
/// [`resolve_used_line_height`].
///
/// FONTLOAD-14 (BUG-467) built this choke point specifically to let
/// `line-height: normal` resolve against real font metrics (`ascent +
/// descent [+ lineGap]`, `m` is unused for that reason right now — kept as a
/// parameter so the next slice doesn't have to re-thread it through every
/// caller) instead of the flat `1.2` approximation, but measurement against
/// Edge (`graphic_tests/run.py --continue-on-fail`, 2026-09-05) showed the
/// opposite of what CSS Fonts L4 §14.3's line-gap accessor (FONTLOAD-13)
/// suggested: `ascent_px + descent_px` alone regresses TEST-02/04/18/21/56/
/// 83/150/151/155 past the 0.5% threshold (TEST-02 at 0.68%, matching the
/// InlineBlockRow strut's own IFC-1 finding almost exactly), and adding
/// `line_gap_px` on top changes nothing (these fonts declare `line_gap = 0`).
/// `OwnedFontMetrics::ascent_px` (`crates/engine/paint/src/lib.rs`) also
/// normalises ascent against `ascent+descent` while `descent_px`/
/// `line_gap_px` normalise against `units_per_em` — inconsistent denominators
/// that make even the sum's intent murky. The strut comparison this formula
/// was modeled on (`BoxKind::InlineBlockRow` in this file's `layout_dispatch`
/// sibling) tests *relative* baseline alignment of an empty box, which
/// tolerates the mismatch; general text line spacing is an *absolute* height
/// that does not. Not included in this slice: matching Edge/DirectWrite's
/// actual `normal` algorithm (Windows text likely uses `OS/2.usWinAscent`/
/// `usWinDescent`, which run taller than the `sTypoAscender`/`sTypoDescender`
/// pair `OwnedFontMetrics` reads, or a UA-side floor) — needs its own
/// investigation. `line_height_is_normal` and this function stay as the
/// architecture for that slice to land in without re-touching every
/// consumer. `<number>`/`<length>` values are unaffected either way — the
/// ratio already carries the used value (see `style::apply_line_height_value`
/// and `apply_font_size_adjust_to_style`'s inverse correction for absolute
/// line-heights).
pub(crate) fn used_line_height_px(style: &ComputedStyle, m: &dyn TextMeasurer) -> f32 {
    let _ = m;
    style.font_size * style.line_height
}

/// Whole-tree pass writing [`LayoutBox::used_line_height`] from real font
/// metrics (FONTLOAD-14, BUG-467) — see [`used_line_height_px`]. Runs
/// alongside [`apply_font_size_adjust`] (same call sites, same ordering
/// requirement: after `build_box`/`apply_font_size_adjust` so it reads the
/// post-adjustment `font_size`, before `lay_out`/`graft_geometry` so every
/// box — reused or freshly laid out — already carries the resolved value).
///
/// Deliberately does NOT touch `b.style` (see `LayoutBox::used_line_height`'s
/// doc comment for why: `style` is `Arc`-shared with the cascade cache, and
/// `normal` is the default `line-height` — writing into it would force
/// `Arc::make_mut` to deep-copy nearly every box in the document).
pub(crate) fn resolve_used_line_height(b: &mut LayoutBox, m: &dyn TextMeasurer) {
    b.used_line_height = used_line_height_px(&b.style, m);
    for child in &mut b.children {
        resolve_used_line_height(child, m);
    }
}

/// Parse inline HTML from an `<iframe srcdoc="...">` attribute (HTML spec §4.8.5).
///
/// Returns the parsed `Document` ready for sub-document layout. The document
/// has no base URL — relative resource references inside `srcdoc` HTML are
/// interpreted as `about:blank`-relative (effectively unresolvable until
/// Phase 1 navigation wiring).
pub fn build_iframe_document(srcdoc: &str) -> Document {
    lumen_html_parser::parse(srcdoc)
}

/// CSS Backgrounds L3 §2.11.2 — «The Canvas Background and the Root Element»:
/// если у root-элемента (`<html>`) нет собственного фона
/// (`background-color: transparent` И `background-image: none`), фон
/// `<body>` пропагируется на root box, а у `<body>` обнуляется. Это
/// покрывает legacy-страницы `body { background: red }`, где иначе фон
/// рисуется только в пределах body box-а и не достигает viewport-а
/// сверху / снизу.
///
/// Phase 0: переносим только два longhand-а — `background-color` и
/// `background-image`. Остальные `background-*` longhand-ы у body без
/// image не имеют визуального эффекта и сейчас не propagated; при
/// добавлении реального paint pattern fill-а их тоже нужно будет
/// перенести.
///
/// Structure: `doc.root()` — Document-узел; его ребёнок — `<html>`
/// element. Body — прямой ребёнок `<html>`. SVG / MathML root-ы пока не
/// учитываются (spec упоминает их отдельно).
fn propagate_canvas_background(doc: &Document, root: &mut LayoutBox) {
    let html_idx = root
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "html"));
    let Some(html_idx) = html_idx else {
        return;
    };

    let html_box = &mut root.children[html_idx];
    let html_has_bg = html_box.style.background_color.is_some()
        || !html_box.style.background_layers.is_empty();
    if html_has_bg {
        return;
    }

    let body_idx = html_box
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "body"));
    let Some(body_idx) = body_idx else {
        return;
    };

    let body = &mut html_box.children[body_idx];
    let body_has_bg = body.style.background_color.is_some()
        || !body.style.background_layers.is_empty();
    if !body_has_bg {
        return;
    }

    let body_style = Arc::make_mut(&mut body.style);
    let bg_color = body_style.background_color.take();
    let bg_layers = std::mem::take(&mut body_style.background_layers);
    let html_style = Arc::make_mut(&mut html_box.style);
    html_style.background_color = bg_color;
    html_style.background_layers = bg_layers;
}

/// CSS Backgrounds §3.11.1 — the canvas background color.
///
/// Returns the opaque background color of the root element box (the color
/// `propagate_canvas_background` moved onto `<html>`, originally the root's or
/// `<body>`'s background). The renderer clears the **entire** surface to this
/// color so the page background covers the whole viewport even when the root
/// element's box is shorter or narrower than the window — e.g. a fixed 1024×720
/// page in a maximized window, where painting only the root box's rect would
/// leave the rest of the canvas the UA-default white (and the root's own
/// `background-color` shows only as a band the size of the box, not the canvas).
///
/// Returns `None` (→ UA-default white clear) when the root element has no
/// background color or the color is not fully opaque: a translucent root
/// background must composite over the UA canvas, which the root box's own
/// background `FillRect` already handles within its rect.
pub fn canvas_background_color(root: &LayoutBox) -> Option<crate::style::Color> {
    let html = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block | BoxKind::FlowRoot))?;
    let color = html.style.background_color?.to_color_opt()?;
    (color.a == 255).then_some(color)
}

/// CSS Overflow L4 §"scrollbar-gutter propagation" — `scrollbar-gutter` on the
/// root element (`:root`, i.e. `<html>`) reserves its gutter against the
/// **viewport**, unlike the same property on any other element
/// (`scrollbar_gutter_inline`/`_block`, which reserve space between a box and
/// its own children). Concretely: `document.documentElement.offsetWidth` must
/// come out narrower than `window.innerWidth` by the gutter unit, while
/// `<body>` (and everything under it) keeps exactly `<html>`'s own
/// `offsetWidth` — no *second*, interior reservation between `<html>` and
/// `<body>` the way a plain scrolling element would apply to its children.
/// Only `:root`'s own declared value counts: a `scrollbar-gutter` on `<body>`
/// (or deeper) must NOT propagate to the viewport (WPT
/// `scrollbar-gutter-propagation-006.html`), so this only ever reads the
/// `<html>` box's own style.
///
/// Because the initial value of `overflow` is `visible` and the CSS Overflow
/// spec's UA note has `visible` on the root propagate to the viewport as
/// `auto` (a page is always scrollable regardless), eligibility here maps
/// `Visible` to `Auto` on a scratch copy of `<html>`'s style before deferring
/// to the same `scrollbar_gutter_inline`/`_block` eligibility gate every other
/// element uses — unlike a plain element, `:root` does not need an explicit
/// `overflow: auto/scroll/hidden` to reserve its gutter (WPT
/// `-propagation-001`/`-002` declare no `overflow` at all).
///
/// Returns `(inline_reserve, inline_start_shift, block_reserve,
/// block_start_shift)` — CSS px to subtract from the top-level `lay_out`
/// call's available width/height and to add to its start `x`/`y`, mirroring
/// exactly what `content_x`/`content_width`/`children_available_height` do
/// for any other scrolling element. As a side effect, `<html>`'s own
/// `scrollbar_gutter` is reset to `Auto` whenever a non-zero reservation is
/// returned, so the normal per-element machinery does not reserve the *same*
/// gutter a second time, between `<html>` and `<body>`.
///
/// Deliberately wired only into the non-incremental entry points ([`layout`],
/// [`layout_measured_hyp_with_counters`]) — every navigation goes through one
/// of these for its first layout pass. The incremental restyle/mutation paths
/// ([`layout_streaming_incremental`], [`layout_mutation_incremental_restyle`])
/// are left untouched: `Arc::make_mut`-ing `<html>`'s style here would give it
/// a fresh `Arc` on every call regardless of whether the author's declaration
/// changed, which risks defeating the cascade-cache/graft style-pointer reuse
/// BUG-341's incremental machinery depends on for *any* page with a
/// `scrollbar-gutter` on `:root` — a perf regression out of proportion to
/// this fix, and untested by any of these WPT files (all one-shot `test()`,
/// not `async_test` mutating `:root` after load).
fn propagate_viewport_scrollbar_gutter(doc: &Document, root: &mut LayoutBox) -> (f32, f32, f32, f32) {
    let Some(html_idx) = root.children.iter().position(|c| is_html_element_named(doc, c.node, "html")) else {
        return (0.0, 0.0, 0.0, 0.0);
    };
    let html_box = &mut root.children[html_idx];
    if matches!(html_box.style.scrollbar_gutter, ScrollbarGutter::Auto) {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut viewport_style = (*html_box.style).clone();
    if viewport_style.overflow_x == Overflow::Visible {
        viewport_style.overflow_x = Overflow::Auto;
    }
    if viewport_style.overflow_y == Overflow::Visible {
        viewport_style.overflow_y = Overflow::Auto;
    }
    let w_reserve = scrollbar_gutter_inline(&viewport_style);
    let x_shift = scrollbar_gutter_inline_start(&viewport_style);
    let h_reserve = scrollbar_gutter_block(&viewport_style);
    let y_shift = scrollbar_gutter_block_start(&viewport_style);
    if w_reserve > 0.0 || h_reserve > 0.0 {
        Arc::make_mut(&mut html_box.style).scrollbar_gutter = ScrollbarGutter::Auto;
    }
    (w_reserve, x_shift, h_reserve, y_shift)
}

fn is_html_element_named(doc: &Document, id: NodeId, want: &str) -> bool {
    matches!(
        doc.get(id).element_name(),
        Some(q) if q.local.eq_ignore_ascii_case(want)
    )
}

/// Является ли DOM-узел inline-контентом (non-whitespace текст или inline-элемент).
///
/// True for Unicode control characters (Cc: C0, DEL, C1) that browsers render as
/// invisible zero-advance — EXCEPT tab/LF/CR, which carry white-space semantics
/// (CSS Text L3 §4.1). Such characters are stripped at the inline-item level so a
/// stray control byte never produces a visible line box (BUG-120: Edge renders
/// U+0001 invisible, Lumen drew a 19.2px text line shifting content below).
pub(crate) fn is_invisible_control(c: char) -> bool {
    c.is_control() && c != '\t' && c != '\n' && c != '\r'
}

/// Removes invisible control characters (see [`is_invisible_control`]) from `s`.
/// Borrows the input unchanged when no such characters are present (common case).
pub(crate) fn strip_invisible_controls(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains(is_invisible_control) {
        std::borrow::Cow::Owned(s.chars().filter(|&c| !is_invisible_control(c)).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

