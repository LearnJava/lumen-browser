use super::*;

/// True when `node` is a `<select>`/`<selectlist>` host that opts into the
/// HTML/CSS «Customizable Select» rendering (`appearance: base-select`).
fn is_base_select_host(doc: &Document, node: NodeId) -> bool {
    matches!(
        &doc.get(node).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "select" | "selectlist")
    )
}

/// Build the author-styleable box subtree for a `<select>`/`<selectlist>` with
/// `appearance: base-select` (HTML/CSS «Customizable Select»).
///
/// Structure (Phase 0 — closed state):
/// ```text
/// FlowRoot (the <select> box, styled by author rules on `select`)
/// └── Block  trigger button — holds the `<selectedcontent>` label text
/// ```
/// Unlike the opaque native `FormControlKind::Select`, this is a real box tree,
/// so author CSS on the `<select>` (and, later, on `option`/`::picker(select)`)
/// cascades into it. The pop-up option list (`::picker(select)`) is revealed by
/// the shell as a popover on click — see `forms.rs`.
#[allow(clippy::too_many_arguments)]
fn build_base_select_box(
    doc: &Document,
    style: &ComputedStyle,
    id: NodeId,
) -> LayoutBox {
    // The trigger button shows the currently-selected option's label, mirroring
    // the `<selectedcontent>` element of the Customizable Select spec.
    let label = if is_selectlist(doc, id) {
        collect_selectlist_label(doc, id)
    } else {
        collect_select_label(doc, id)
    };

    let mut trigger_children = Vec::new();
    if !label.is_empty() {
        let seg = InlineSegment {
            text: label,
            style: anon_style(style),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: id,
            source_char_offset: 0,
            bidi_level: 0,
        };
        trigger_children.push(anon_inline_run(id, style, vec![seg], BoxRole::AnonymousInlineRun));
    }

    let mut trigger_style = anon_style(style);
    trigger_style.display = Display::Block;
    let trigger = LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(trigger_style),
        kind: BoxKind::Block,
        children: trigger_children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        // Not GeneratedContent: this mirrors the `<select>`'s own selected-option
        // label, not `content:` — closest is the anonymous UA-scaffolding wrapper.
        origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousBlock },
    };

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(style.clone()),
        // FlowRoot: establishes a BFC and lays out the trigger as a block child,
        // regardless of the select's own (inline-block) UA display.
        kind: BoxKind::FlowRoot,
        children: vec![trigger],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
    }
}

/// BUG-341 S4 — per-node reuse decision point for incremental box-build.
///
/// Called wherever `build_box` recurses into a DOM child. When incremental
/// box-build is enabled and `prev_index` still holds `id`'s subtree, **takes**
/// that previous [`LayoutBox`] subtree instead of rebuilding it. Otherwise
/// falls through to a normal `build_box` call (which itself threads
/// `prev_index` down, so a dirty ancestor's clean descendants still get reused
/// at their own level).
///
/// BUG-341 S19: membership in `prev_index` *is* the reuse licence — the index
/// is built by [`crate::incremental::extract_clean_subtrees`] from exactly
/// [`CounterMap::clean_subtrees`], so the separate `clean_subtrees` test S4-S18
/// did here would be asking the same question twice. Each entry can be taken
/// only once, which is also what keeps the previous tree's boxes from ending up
/// in two places at once.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_box_or_reuse(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    // BUG-341 S15: the gate is `prev_index.is_some()`, NOT the
    // `INCREMENTAL_BOX_BUILD` thread-local — `build_box` fans flex/grid
    // containers out over rayon workers, whose thread-locals start at their
    // defaults (the same trap `StyleEnvSnapshot` exists for), so a thread-local
    // check here silently disabled reuse for every child of a container with 8+
    // items. Chrome is built out of exactly such containers. The flag is
    // consulted once, at `incremental_build_box`, which is what decides whether
    // an index exists at all.
    if let Some(idx) = prev_index
        && let Some(cell) = idx.get(&id)
    {
        let taken = if box_build_diagnostics_on() {
            use std::sync::atomic::Ordering::Relaxed;
            let t = std::time::Instant::now();
            let taken = cell.lock().ok().and_then(|mut slot| slot.take());
            BOX_CLONE_NS.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            if let Some(b) = taken.as_ref() {
                BOX_CLONE_BOXES.fetch_add(count_boxes(b), Relaxed);
            }
            taken
        } else {
            cell.lock().ok().and_then(|mut slot| slot.take())
        };
        if let Some(mut subtree) = taken {
            BOX_BUILD_STATS.with(|s| {
                let mut v = s.get();
                v.reused += 1;
                s.set(v);
            });
            // BUG-341 S18: tell the two stages that follow — `mark_subtree_dirty`
            // and `graft_geometry` — that this subtree came out of `prev` itself.
            // Both of them exist to answer "may this subtree keep the previous
            // pass's geometry", and here the answer is known by construction, so
            // both can honour it at the root instead of walking the copy against
            // its own original. Only the root carries the flag: the move stops the
            // recursion, so nothing inside it can hold a claim of its own.
            subtree.dirty = crate::incremental::DirtyBits::REUSED_SUBTREE;
            return subtree;
        }
    }
    build_box(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index)
}

/// BUG-341 S4 — incremental box-build entry point.
///
/// Builds the `LayoutBox` tree rooted at `id`, **moving** whole subtrees out of
/// `prev` wherever [`CounterMap::clean_subtrees`] says it is safe to (see
/// [`build_box_or_reuse`]) instead of calling `build_box` for them. Must
/// reproduce a full `build_box` pass bit-for-bit for the same final state —
/// the `incr == full` differential tests in `incremental.rs` guard this.
///
/// BUG-341 S19: `prev` is taken by unique reference and is **gutted** by the
/// call — every reusable subtree ends up in the returned tree, and its old
/// position holds a husk (see [`crate::incremental::DirtyBits::MOVED_OUT`]).
/// The only thing a caller may still do with `prev` afterwards is hand it to
/// [`crate::incremental::graft_geometry_with_cascade`], which recognises the
/// husks; anything else must clone `prev` first.
///
/// Gated behind [`set_incremental_box_build`]: flag off (the default) makes
/// this behave exactly like `build_box(..., None)` and leaves `prev` untouched.
///
/// BUG-341 S15 wired this into [`layout_mutation_incremental_restyle`], the
/// chrome and page incremental pipelines' entry point. The full-layout entry
/// points (`layout_measured_hyp_with_counters`, `layout_streaming_incremental`)
/// still call `build_box` directly — they have no `RestyleDelta`, hence no
/// `clean_subtrees`, hence nothing to reuse.
#[allow(clippy::too_many_arguments)]
pub fn incremental_build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev: &mut LayoutBox,
) -> LayoutBox {
    if !incremental_box_build_enabled() {
        return build_box(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, None);
    }
    let t = std::time::Instant::now();
    let (prev_index, visited) = crate::incremental::extract_clean_subtrees(prev, counters.clean_subtrees());
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.prev_index_visited += visited as u32;
        s.set(v);
    });
    if box_build_diagnostics_on() {
        note_prev_index(t.elapsed().as_nanos() as u64, visited);
    }
    build_box_or_reuse(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, Some(&prev_index))
}

/// BUG-341 S20 — timing shim around [`build_box_inner`].
///
/// Off the census path (the overwhelmingly common case) this is one relaxed
/// atomic load and a direct call. With [`set_box_build_diagnostics`] on it
/// records the call's inclusive wall-clock into [`BOX_BUILD_TIME_LOG`]; the
/// timer lives here rather than inside the body so it covers every one of the
/// body's exit paths (`build_base_select_box`'s early return among them).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    if !BOX_TIME_LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return build_box_inner(
            doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index,
        );
    }
    let t = std::time::Instant::now();
    let out = build_box_inner(
        doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index,
    );
    let ns = t.elapsed().as_nanos() as u64;
    if let Ok(mut log) = BOX_BUILD_TIME_LOG.lock() {
        log.push((id, ns));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn build_box_inner(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    // BUG-341 S4/S19: `Some` when an incremental box-build pass is in progress —
    // an id→owned-subtree index carved out of the previous pass's tree,
    // consulted by `build_box_or_reuse` at every recursive call site below.
    // `None` for the full/legacy build path (all current pipeline entry points).
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.built += 1;
        s.set(v);
    });
    note_box_built(id);
    // BUG-284: `precompute_counters` already ran a full document-order cascade
    // pass over this exact tree (same `inherited` chain, same sheet/viewport/
    // dark_mode) to resolve counter-reset/increment/set — reuse its cached
    // result instead of paying for an identical `compute_style` call again.
    let mut style = counters.style_arc(id).unwrap_or_else(|| {
        note_style_miss(|| {
            Arc::new(compute_style(doc, id, sheet, inherited, viewport, dark_mode))
        })
    });

    // HTML/CSS «Customizable Select»: a `<select appearance:base-select>` renders
    // as an author-styleable widget tree instead of the opaque native control.
    if style.appearance == crate::style::Appearance::BaseSelect
        && style.display != Display::None
        && is_base_select_host(doc, id)
    {
        return build_base_select_box(doc, &style, id);
    }

    let kind = match &doc.get(id).data {
        // Shadow root nodes are infrastructure — never rendered directly.
        // The flat tree already maps host children to shadow root's children.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } | NodeData::ShadowRoot { .. } | NodeData::DocumentFragment => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None || is_closed_popover(doc, id) || is_svg_defs(doc, id) {
                BoxKind::Skip
            } else if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                // Intrinsic dimensions у выбранного `<source>` (а для голого
                // `<img>` — его собственные `width`/`height` атрибуты, куда
                // shell кладёт размер декодированной картинки) действуют как
                // presentational hint: заполняют только пустые слоты, не
                // перекрывают ни CSS-каскад, ни собственные `<img width|
                // height>` атрибуты (последние уже легли в style через
                // `apply_image_presentational_hints`). HTML5 §10 «mapped
                // attributes»: hint = UA-rule с specificity 0.
                //
                // BUG-734: когда известны ОБЕ стороны, это не два независимых
                // hint-а, а intrinsic **соотношение** — CSS Sizing L4 §4.1
                // («`aspect-ratio: auto` на замещаемом элементе = intrinsic
                // ratio») и CSS 2.1 §10.6.2. Подставлять сырое значение в
                // пустой слот нельзя: `width: 100px; height: auto` дало бы
                // 100×<intrinsic h> вместо 100×(100/ratio), а самый частый в
                // вебе `max-width: 100%` не пересчитывал бы высоту после
                // клампа ширины. Поэтому ratio уезжает в `style.aspect_ratio`
                // (если author его не задал), а сырой размер подставляется
                // ровно в одном случае «обе стороны auto» — и только в
                // ширину: высоту из неё выведет ratio-ветка, и она же
                // отработает после `min-`/`max-width`.
                let intrinsic_ratio = match (src.intrinsic_width, src.intrinsic_height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as f32, h as f32)),
                    _ => None,
                };
                if let Some((iw, ih)) = intrinsic_ratio {
                    let st = Arc::make_mut(&mut style);
                    if st.aspect_ratio.is_none() {
                        st.aspect_ratio = Some((iw, ih));
                    }
                    if st.width.is_none() && st.height.is_none() {
                        st.width = Some(Length::Px(iw));
                    }
                } else {
                    // Известна одна сторона — ratio не построить, поведение
                    // прежнее: hint заполняет пустой слот.
                    if style.width.is_none()
                        && let Some(w) = src.intrinsic_width
                    {
                        Arc::make_mut(&mut style).width = Some(Length::Px(w as f32));
                    }
                    if style.height.is_none()
                        && let Some(h) = src.intrinsic_height
                    {
                        Arc::make_mut(&mut style).height = Some(Length::Px(h as f32));
                    }
                }
                let is_lazy = doc.get(id).get_attr("loading")
                    .is_some_and(|v| v.eq_ignore_ascii_case("lazy"));
                BoxKind::Image { src: src.url, alt, is_lazy }
            } else if is_video_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let poster = node.get_attr("poster").unwrap_or("").to_string();
                // HTML spec §14.1: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(150.0));
                }
                BoxKind::Video { src, poster }
            } else if is_canvas_element(doc, id) {
                let node = doc.get(id);
                // HTML LS §4.12.4: width/height content attributes reflect as
                // `unsigned long`; defaults are 300×150 CSS px.
                //
                // BUG-452: this was `v.trim().parse::<u32>()`, whose rules are
                // neither the spec's nor `parseInt`'s — it rejected `"100.999"`,
                // `"100em"` and `"0x100"` (§2.4.4.1 gives 100/100/**0**), so the
                // box was laid out at the 300×150 default while `canvas.width`
                // from script answered 100 off the JS mirror of the same rule.
                let cw = lumen_dom::attr_int::reflect_unsigned_long(node.get_attr("width"), 300);
                let ch = lumen_dom::attr_int::reflect_unsigned_long(node.get_attr("height"), 150);
                // The bitmap dimensions act as intrinsic size; explicit CSS
                // width/height (or presentational hints) win if already set.
                //
                // BUG-099: unlike `<img>`/`<video>`, HTML Rendering §15.4.1 does
                // NOT map the `<canvas>` dimension attributes to the `width`/
                // `height` properties — they are the element's *intrinsic* size,
                // i.e. a content-box size. Feeding them through `style.width`
                // makes `box-sizing: border-box` subtract borders and padding
                // from the bitmap, shrinking the element (TEST-57 c3: 180×150
                // instead of Edge's 186×156 border box). Add the border+padding
                // back so that the resulting *content* box stays the bitmap size.
                // % padding resolves against the containing block, unknown here —
                // it degrades to 0, same limitation as the `<img>` hint above.
                let (fill_extra_w, fill_extra_h) = match style.box_sizing {
                    BoxSizing::ContentBox => (0.0, 0.0),
                    BoxSizing::BorderBox => {
                        let em = style.font_size;
                        (
                            style.border_left_width
                                + style.border_right_width
                                + style.padding_left.resolve_or_zero(em, 0.0, viewport)
                                + style.padding_right.resolve_or_zero(em, 0.0, viewport),
                            style.border_top_width
                                + style.border_bottom_width
                                + style.padding_top.resolve_or_zero(em, 0.0, viewport)
                                + style.padding_bottom.resolve_or_zero(em, 0.0, viewport),
                        )
                    }
                };
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(cw as f32 + fill_extra_w));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(ch as f32 + fill_extra_h));
                }
                BoxKind::Canvas { width: cw, height: ch }
            } else if is_audio_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let controls = node.get_attr("controls").is_some();
                // HTML spec §4.8.10: without controls, <audio> has no box (0×0).
                // With controls, UA must render a control interface; we use 40px height.
                if controls {
                    if style.height.is_none() {
                        Arc::make_mut(&mut style).height = Some(Length::Px(40.0));
                    }
                } else {
                    Arc::make_mut(&mut style).width = Some(Length::Px(0.0));
                    Arc::make_mut(&mut style).height = Some(Length::Px(0.0));
                }
                BoxKind::Audio { src, controls }
            } else if is_iframe_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let srcdoc = node.get_attr("srcdoc").filter(|s| !s.is_empty()).map(str::to_owned);
                // HTML spec §4.8.5: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(150.0));
                }
                BoxKind::Iframe { src, srcdoc }
            } else if is_form_control_element(doc, id) {
                let kind = {
                    let node = doc.get(id);
                    let tag = node.element_name()
                        .map(|q| q.local.as_str())
                        .unwrap_or("")
                        .to_owned();
                    match tag.as_str() {
                        "button"   => FormControlKind::Button,
                        "select"   => {
                            let selected_text = collect_select_label(doc, id);
                            FormControlKind::Select { selected_text }
                        }
                        // <selectlist> (Customizable Select, Phase 0) renders as a
                        // native-select widget. P4 wires ::picker(select) appearance.
                        // CSS: appearance: base-select
                        "selectlist" => {
                            let selected_text = collect_selectlist_label(doc, id);
                            FormControlKind::Select { selected_text }
                        }
                        "textarea" => {
                            let value_text = collect_textarea_content(doc, id);
                            FormControlKind::Textarea { value_text }
                        }
                        "progress" => {
                            let max = node.get_attr("max")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0)
                                .max(f32::EPSILON);
                            let value = node.get_attr("value")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .map(|v| v.clamp(0.0, max));
                            FormControlKind::Progress { value, max }
                        }
                        "meter" => {
                            let raw_min = node.get_attr("min")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(0.0);
                            let raw_max = node.get_attr("max")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0);
                            // Spec §4.10.14: if min ≥ max, reset to defaults 0/1.
                            let (min, max) = if raw_min < raw_max {
                                (raw_min, raw_max)
                            } else {
                                (0.0, 1.0)
                            };
                            let low = node.get_attr("low")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(min)
                                .clamp(min, max);
                            let high = node.get_attr("high")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(max)
                                .clamp(min, max);
                            let optimum = node.get_attr("optimum")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or((min + max) / 2.0)
                                .clamp(min, max);
                            let value = node.get_attr("value")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(0.0)
                                .clamp(min, max);
                            FormControlKind::Meter { value, min, max, low, high, optimum }
                        }
                        _ => {
                            let input_type = node.input_type()
                                .unwrap_or(lumen_dom::InputType::Text);
                            if input_type == lumen_dom::InputType::Range {
                                let min = node.get_attr("min")
                                    .and_then(|v| v.trim().parse::<f32>().ok())
                                    .unwrap_or(0.0);
                                let max = node.get_attr("max")
                                    .and_then(|v| v.trim().parse::<f32>().ok())
                                    .unwrap_or(100.0);
                                let default_val = (min + max) / 2.0;
                                let value = doc.control_value(id)
                                    .trim()
                                    .parse::<f32>()
                                    .ok()
                                    .unwrap_or(default_val)
                                    .clamp(min, max);
                                FormControlKind::Range { value, min, max }
                            } else {
                                // BUG-444: the painted mark is the control's
                                // current checkedness; `checked=` is only its
                                // default.
                                let checked = doc.control_checked(id);
                                // BUG-441: the painted text is the control's
                                // current value; `value=` is only its default.
                                let value_text = doc.control_value(id).into_owned();
                                let placeholder = node.get_attr("placeholder")
                                    .unwrap_or("")
                                    .to_owned();
                                let placeholder_style = compute_pseudo_element_style(
                                    doc, id, "placeholder", sheet, &style, viewport, dark_mode,
                                ).map(Box::new);
                                FormControlKind::Input { input_type, checked, value_text, placeholder, placeholder_style }
                            }
                        }
                    }
                };
                BoxKind::FormControl { kind }
            } else if matches!(style.display, Display::TableRow) {
                BoxKind::TableRow
            } else if matches!(style.display, Display::Table | Display::InlineTable) {
                BoxKind::Table
            } else if matches!(
                style.display,
                Display::TableRowGroup
                    | Display::TableHeaderGroup
                    | Display::TableFooterGroup
            ) {
                BoxKind::TableRowGroup
            } else if matches!(style.display, Display::FlowRoot) {
                BoxKind::FlowRoot
            } else if matches!(style.display, Display::Contents) {
                BoxKind::Contents
            } else if is_svg_root(doc, id) {
                // SVG root: apply width/height attributes as presentational hints.
                // CSS: width, height — if author CSS is absent, attribute values are used.
                // CSS: object-fit, object-position — P4 can override viewBox scaling (Phase 2)
                // CSS: intrinsic aspect-ratio from viewBox for replaced element sizing
                if style.width.is_none()
                    && let Some(w) = doc.get(id).get_attr("width").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    Arc::make_mut(&mut style).width = Some(crate::style::Length::Px(w));
                }
                if style.height.is_none()
                    && let Some(h) = doc.get(id).get_attr("height").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    Arc::make_mut(&mut style).height = Some(crate::style::Length::Px(h));
                }
                BoxKind::SvgRoot {
                    view_box: parse_view_box(doc, id),
                    preserve_aspect_ratio: parse_preserve_aspect_ratio(doc, id),
                }
            } else {
                BoxKind::Block
            }
        }
    };

    // CSS Containment L3 §4 — content-visibility: hidden suppresses the subtree.
    // Phase 1: element keeps its own box but contributes 0×0 (no contain-intrinsic-size yet).
    // content-visibility: auto (off-viewport skip) is deferred to Phase 2.
    if style.content_visibility == crate::style::ContentVisibility::Hidden {
        return LayoutBox {
            node: id,
            rect: Rect::ZERO,
            style,
            kind,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
        };
    }

    let mut children = Vec::new();
    // BUG-441: a `<textarea>` renders its *current* value. Unlike `<input>`,
    // whose text the form-control painter draws from `FormControlKind`, a
    // textarea's text is ordinary inline content laid out from its DOM
    // children — and those children are only its *default* value (HTML LS
    // §4.10.11). Once the control has a runtime value, that value is laid out
    // in their place, so typing and `el.value = …` reach the screen without
    // rewriting the markup they came from.
    let textarea_runtime_value: Option<String> = match &kind {
        BoxKind::FormControl { kind: FormControlKind::Textarea { value_text } }
            if doc.dirty_value(id).is_some() =>
        {
            Some(value_text.clone())
        }
        _ => None,
    };
    if let Some(value_text) = &textarea_runtime_value {
        children.push(anon_inline_run(
            id,
            &style,
            control_value_segments(id, value_text, &style),
            BoxRole::AnonymousInlineRun,
        ));
    }
    if matches!(kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Contents | BoxKind::FormControl { .. } | BoxKind::TableRow | BoxKind::Table | BoxKind::TableRowGroup | BoxKind::SvgRoot { .. }) {
        // CSS: :host, ::slotted — P4 wires shadow-scoped styles here
        // HTML5 §4.11.1 — <details>: when `open` attribute absent, only <summary> is rendered.
        // P3 wires: clicking <summary> should toggle `open` attribute + relayout.
        let dom_children: Vec<NodeId> = if textarea_runtime_value.is_some() {
            // The default value's text nodes are replaced by the run above.
            Vec::new()
        } else if is_details_element(doc, id)
            && doc.get(id).get_attr("open").is_none()
        {
            flat.children_of(doc, id)
                .iter()
                .copied()
                .filter(|&cid| is_summary_element(doc, cid))
                .collect()
        } else {
            flat.children_of(doc, id).to_vec()
        };
        // CSS Grid L1 §6: all direct children of a grid/flex container are
        // "blockified" — they participate as individual items, not wrapped in
        // InlineRun. Skip the inline-collection logic for these containers.
        let is_item_container = matches!(
            style.display,
            Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
                | Display::TableRow
                | Display::Table | Display::InlineTable
                | Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        );
        if is_item_container {
            // CSS Flexbox §4 / Grid §6: text runs directly inside a flex/grid
            // container become anonymous items. Tables keep their own
            // anonymous-box rules (text → anonymous cell), so wrap only for
            // flex/grid here.
            let wrap_text_items = matches!(
                style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            );

            // ADR-016 M4.1 — parallel selector matching for large item containers.
            // Siblings in a flex/grid/table container share only the immutable parent
            // style; their `compute_style` calls are fully independent. rayon worker
            // threads start with default thread-locals (no interactive state, no shadow
            // sheets), so we capture a `StyleEnvSnapshot` on the layout thread and
            // install it at the top of each closure before any style work runs.
            // The parallel path produces identical results to the sequential path;
            // item order is preserved by rayon's par_iter + collect guarantee.
            //
            // Threshold: only parallelize when the item count justifies the rayon
            // spawn overhead (~1–2 µs per closure on a warm thread pool).
            const RAYON_MIN_FLEX_CHILDREN: usize = 8;
            // BUG-341 S20: the threshold counts the children this pass will
            // really **build**, not the ones the container happens to have.
            //
            // M4.1 sized the threshold against a full pass, where every child
            // costs a cascade plus a box build and eight of them comfortably
            // outweigh the fan-out. On the incremental path since S15/S19 a
            // child that is in `prev_index` costs a `Mutex` lock and a move —
            // and on a chrome interaction nearly all of them are. Counting DOM
            // children there dispatched a worker per reused subtree to do
            // nothing: measured at ~1 ms of a 2.5 ms keystroke cycle, spread
            // over `body`/`.main-col`/`.omnibox-wrap`, each of whose *own* work
            // is ~4 µs (BUG-341 "S20" census). Same shape as S18 — the stage
            // was deciding for itself something the reuse mechanism had already
            // established.
            //
            // Non-element children are excluded from the estimate for the same
            // reason: whitespace between pretty-printed markup never enters the
            // reuse index (it holds elements only), yet it costs a `Skip` box or
            // one small anonymous item — never the cascade the threshold was
            // sized against. Counting it kept `body` above the threshold on a
            // cycle where every one of its element children was a move.
            //
            // `None` (every full-layout entry point) leaves the decision at
            // `dom_children.len()`, i.e. M4.1's behaviour byte for byte.
            let children_to_build = match prev_index {
                None => dom_children.len(),
                Some(idx) => dom_children
                    .iter()
                    .filter(|&&c| {
                        !idx.contains_key(&c)
                            && matches!(doc.get(c).data, NodeData::Element { .. })
                    })
                    .count(),
            };
            if children_to_build >= RAYON_MIN_FLEX_CHILDREN {
                use rayon::prelude::*;
                let snap = crate::style::StyleEnvSnapshot::capture();
                // BUG-341 S15: each closure drains the tally of whatever thread
                // ran it into this shared counter, which is folded back into the
                // parent's thread below. Draining is exact even when rayon
                // work-steals a closure onto the calling thread — whatever it
                // takes from that thread's tally comes straight back in the
                // fold. Without it every box built under a container with 8+
                // items was invisible to the reuse gates.
                let par_built = std::sync::atomic::AtomicU32::new(0);
                let par_reused = std::sync::atomic::AtomicU32::new(0);
                // Nested containers a worker fans out again are folded back
                // through the same drain as `built`/`reused` below.
                let par_fanouts = std::sync::atomic::AtomicU32::new(0);
                // BUG-341 S25: same drain for the display-probe / style-miss
                // tallies, for the same reason as `built`/`reused` above.
                let par_probes = std::sync::atomic::AtomicU32::new(0);
                let par_probe_cascades = std::sync::atomic::AtomicU32::new(0);
                let par_misses = std::sync::atomic::AtomicU32::new(0);
                // BUG-341 S21: the cascade's rule index is per-thread too, so a
                // worker that has not seen this sheet builds its own. Drained
                // through the same fold as the box tallies, for the same reason
                // — otherwise the gate that asserts a pass rebuilds no index
                // would be blind to every rebuild a worker made.
                let par_index_stats = std::sync::Mutex::new(crate::style::CascadeIndexStats::default());
                // BUG-341 S23: same drain for the pseudo-element cascade census.
                // Without it the census undercounts exactly the containers this
                // branch exists for — every flex/grid container with 8+ items.
                let par_pseudo_stats =
                    std::sync::Mutex::new(crate::style::PseudoCascadeStats::default());
                let par_pseudo_sites: std::sync::Mutex<
                    std::collections::HashMap<String, crate::style::PseudoCascadeStats>,
                > = std::sync::Mutex::new(std::collections::HashMap::new());
                children = dom_children.par_iter().filter_map(|&child_id| {
                    snap.install();
                    let out = if wrap_text_items && matches!(doc.get(child_id).data, NodeData::Text(_)) {
                        build_anon_text_item(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode,
                        )
                    } else {
                        let b = build_box_or_reuse(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index,
                        );
                        if matches!(b.kind, BoxKind::Skip) { None } else { Some(b) }
                    };
                    let d = take_box_build_stats();
                    use std::sync::atomic::Ordering::Relaxed;
                    par_built.fetch_add(d.built, Relaxed);
                    par_reused.fetch_add(d.reused, Relaxed);
                    par_fanouts.fetch_add(d.fanouts, Relaxed);
                    par_probes.fetch_add(d.display_probes, Relaxed);
                    par_probe_cascades.fetch_add(d.display_probe_cascades, Relaxed);
                    par_misses.fetch_add(d.style_misses, Relaxed);
                    let idx_stats = crate::style::take_cascade_index_stats();
                    par_index_stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add(idx_stats);
                    let ps_stats = crate::style::take_pseudo_cascade_stats();
                    par_pseudo_stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add(ps_stats);
                    let ps_sites = crate::style::take_pseudo_cascade_sites();
                    if !ps_sites.is_empty() {
                        let mut acc = par_pseudo_sites.lock().unwrap_or_else(|e| e.into_inner());
                        for (k, v) in ps_sites {
                            acc.entry(k).or_default().add(v);
                        }
                    }
                    out
                }).collect();
                crate::style::add_cascade_index_stats(
                    *par_index_stats.lock().unwrap_or_else(|e| e.into_inner()),
                );
                crate::style::add_pseudo_cascade_stats(
                    *par_pseudo_stats.lock().unwrap_or_else(|e| e.into_inner()),
                );
                crate::style::add_pseudo_cascade_sites(std::mem::take(
                    &mut *par_pseudo_sites.lock().unwrap_or_else(|e| e.into_inner()),
                ));
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    add_box_build_stats(BoxBuildStats {
                        built: par_built.load(Relaxed),
                        reused: par_reused.load(Relaxed),
                        // Extraction runs once, on the thread that owns `prev`
                        // — a worker never adds to this.
                        prev_index_visited: 0,
                        // This container's own dispatch, plus any a worker made.
                        fanouts: par_fanouts.load(Relaxed) + 1,
                        display_probes: par_probes.load(Relaxed),
                        display_probe_cascades: par_probe_cascades.load(Relaxed),
                        style_misses: par_misses.load(Relaxed),
                    });
                }
            } else {
                for child_id in dom_children {
                    if wrap_text_items
                        && matches!(doc.get(child_id).data, NodeData::Text(_))
                    {
                        if let Some(item) = build_anon_text_item(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode,
                        ) {
                            children.push(item);
                        }
                        continue;
                    }
                    let child_box = build_box_or_reuse(
                        doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index,
                    );
                    if !matches!(child_box.kind, BoxKind::Skip) {
                        children.push(child_box);
                    }
                }
            }
            // CSS Flexbox §4 / Grid §6 — ::before / ::after on a flex or grid
            // container generate blockified flex/grid items (first and last,
            // respectively). Tables have their own anonymous-box rules, so they
            // are excluded here.
            if matches!(
                style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            ) {
                let before_ps = compute_pseudo_element_style(
                    doc, id, "before", sheet, &style, viewport, dark_mode,
                );
                let after_ps = compute_pseudo_element_style(
                    doc, id, "after", sheet, &style, viewport, dark_mode,
                );
                inject_pseudo(id, &mut children, before_ps, true, doc, viewport, counters, registry, true);
                inject_pseudo(id, &mut children, after_ps, false, doc, viewport, counters, registry, true);
            }
        } else {
        let mut i = 0;
        while i < dom_children.len() {
            let child_id = dom_children[i];
            let is_inl =
                is_inline_content(doc, sheet, child_id, &style, viewport, dark_mode, counters);
            let is_ib = !is_inl
                && is_atomic_inline_level(doc, sheet, child_id, &style, viewport, dark_mode, counters);

            if is_inl || is_ib {
                // Унифицированный сбор inline-уровневого контента: inline-элементы
                // и atomic inline-level (`inline-block`/`inline-flex`/
                // `inline-grid`) участвуют в ОДНОМ inline-контексте.
                // Межэлементный whitespace не прерывает поток.
                // Результат: InlineRun (чистый текст) или InlineBlockRow (смешанный).
                let mut row_items: Vec<LayoutBox> = Vec::new();
                let mut pending: Vec<InlineSegment> = Vec::new();
                // BUG-728: потомки inline-элементов, которым нужен собственный
                // бокс. Индексы `at` считаются по общему `pending`, поэтому
                // вектор один на весь цикл, как и `pending`.
                let mut pending_escapes: Vec<InlineEscape> = Vec::new();
                // CSS §4.1.2 white-space collapsing: whitespace between
                // inline-level siblings collapses to a single space.
                let mut had_ws = false;
                // CSS Pseudo-elements L4 §5.1: first letter of this inline run hasn't been
                // split out yet. Passed through all collect_inline_segments calls in this loop.
                let mut need_first_letter = true;
                // CSS Pseudo-elements L4 §5.3: pre-compute ::first-line style once for this block.
                // BUG-341 S23: skipped outright on a sheet that never uses
                // `::first-line` as a selector subject — the cascade could only
                // return `None` there, and this runs per inline-content block.
                let first_line_style = if crate::style::sheet_targets_pseudo(sheet, viewport, dark_mode, "first-line") {
                    crate::style::compute_pseudo_element_style(doc, id, "first-line", sheet, &style, viewport, dark_mode)
                        .map(Box::new)
                } else {
                    None
                };
                // Track whether first_line_style has been assigned to the first InlineRun.
                let mut first_line_assigned = false;

                loop {
                    if i >= dom_children.len() {
                        break;
                    }
                    let cid = dom_children[i];
                    match &doc.get(cid).data {
                        // BUG-120: control-only text is skipped like whitespace-only,
                        // but contributes an inter-segment space only if it actually
                        // contains whitespace (a bare U+0001 is zero-advance in Edge).
                        NodeData::Text(s)
                            if s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) =>
                        {
                            had_ws |= s.chars().any(char::is_whitespace);
                            i += 1;
                            continue;
                        }
                        NodeData::Comment(_) | NodeData::Doctype { .. } => {
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    if is_inline_content(doc, sheet, cid, &style, viewport, dark_mode, counters) {
                        // CSS §4.1.1 — collapsed whitespace between inline-level
                        // siblings becomes a single inter-word gap. Record it as a
                        // trailing space on the previous segment so wrap_inline_run
                        // inserts exactly one space at the boundary; without it,
                        // `<span>a</span> <span>b</span>` would join tightly.
                        if had_ws
                            && let Some(last) = pending.last_mut()
                            && !last.forced_break
                            && !last.style.white_space.preserves_whitespace()
                            && !last.text.ends_with(|c: char| c.is_whitespace())
                        {
                            last.text.push(' ');
                        }
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending, &mut pending_escapes, flat, counters, registry, &mut need_first_letter, dark_mode);
                        had_ws = false;
                        i += 1;
                    } else if is_atomic_inline_level(doc, sheet, cid, &style, viewport, dark_mode, counters)
                    {
                        if !pending.is_empty() || !pending_escapes.is_empty() {
                            let from = row_items.len();
                            split_inline_pieces(
                                doc, sheet, id, &style, viewport, flat, counters, registry,
                                dark_mode, prev_index,
                                std::mem::take(&mut pending),
                                std::mem::take(&mut pending_escapes),
                                &mut row_items,
                            );
                            assign_first_line_style(
                                &mut row_items[from..], &first_line_style, &mut first_line_assigned,
                            );
                        }
                        // Whitespace between inline-blocks → collapsed space gap.
                        if had_ws && !row_items.is_empty() {
                            row_items.push(LayoutBox {
                                node: id,
                                rect: Rect::ZERO,
                                style: Arc::new(anon_style(&style)),
                                kind: BoxKind::InlineSpace,
                                children: vec![],
                                col_span: 1,
                                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                                origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousInlineRun },
                            });
                        }
                        row_items.push(build_box_or_reuse(doc, sheet, cid, &style, viewport, flat, counters, registry, dark_mode, prev_index));
                        had_ws = false;
                        i += 1;
                    } else if matches!(doc.get(cid).data, NodeData::Element { .. })
                        && probe_display(doc, sheet, cid, &style, viewport, dark_mode, counters)
                            == Display::None
                    {
                        // display:none не прерывает inline-контекст — CSS §9.2.4.
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !pending.is_empty() || !pending_escapes.is_empty() {
                    let from = row_items.len();
                    split_inline_pieces(
                        doc, sheet, id, &style, viewport, flat, counters, registry,
                        dark_mode, prev_index,
                        std::mem::take(&mut pending),
                        std::mem::take(&mut pending_escapes),
                        &mut row_items,
                    );
                    assign_first_line_style(
                        &mut row_items[from..], &first_line_style, &mut first_line_assigned,
                    );
                }

                // CSS Pseudo-elements L4 §5.1 — apply ::first-letter style.
                // collect_inline_segments marks the first non-whitespace text segment
                // with PseudoKind::FirstLetter; split it here so wrap_inline_run uses
                // the override font metrics for both the letter and the remainder.
                let fl_pseudo = compute_pseudo_element_style(
                    doc, id, "first-letter", sheet, &style, viewport, dark_mode,
                );
                // CSS Inline Layout L3 §5 — `initial-letter`. Effective value:
                // a ::first-letter pseudo `initial-letter` wins over the element's
                // own; `size > 1` activates the drop cap and supersedes the legacy
                // float-::first-letter path.
                let initial_letter = fl_pseudo
                    .as_ref()
                    .map(|p| (p.initial_letter_size, p.initial_letter_sink))
                    .filter(|(s, _)| *s > 1.0)
                    .or_else(|| {
                        (style.initial_letter_size > 1.0)
                            .then_some((style.initial_letter_size, style.initial_letter_sink))
                    });
                // ::first-letter / initial-letter target the block's first formatted
                // line, so only the inline group that opens the block qualifies.
                let first_group = children
                    .iter()
                    .all(|c| matches!(c.kind, BoxKind::Marker { .. }));
                if let Some((size, sink)) = initial_letter {
                    if first_group
                        && let Some(letter) = extract_initial_letter(
                            &mut row_items, &style, fl_pseudo.as_ref(), size, sink,
                        )
                    {
                        children.push(letter);
                    }
                } else if let Some(fl_style) = fl_pseudo {
                    // CSS Pseudo-elements L4 §5.2 — float ::first-letter → drop cap
                    // (BB-2): promote the letter to a block-level float sibling placed
                    // before the run.
                    if fl_style.float_side != FloatSide::None && first_group {
                        if let Some(letter) = extract_first_letter_float(&mut row_items, &fl_style) {
                            children.push(letter);
                        }
                    } else {
                        apply_first_letter_style(&mut row_items, fl_style, &style);
                    }
                }

                // BUG-728 / CSS 2.1 §9.2.1.1: блочно-уровневый бокс, всплывший
                // из inline-элемента, разрывает inline-контекст — контент до
                // него и после него образуют РАЗНЫЕ анонимные группы, а сам он
                // становится блочным сиблингом. Без escape-ов цикл вырождается
                // в прежнюю одну группу на весь ряд.
                let mut group: Vec<LayoutBox> = Vec::new();
                let flush_group = |group: &mut Vec<LayoutBox>, children: &mut Vec<LayoutBox>| {
                    match group.len() {
                        0 => {}
                        // Единственный чисто-текстовый run — без лишней обёртки.
                        1 if matches!(group[0].kind, BoxKind::InlineRun { .. }) => {
                            children.push(group.remove(0));
                        }
                        // Несколько элементов или inline-block → InlineBlockRow.
                        _ => {
                            children.push(anon_inline_block_row(id, &style, std::mem::take(group)));
                        }
                    }
                };
                for item in row_items.drain(..) {
                    if breaks_inline_row(&item) {
                        flush_group(&mut group, &mut children);
                        children.push(item);
                    } else {
                        group.push(item);
                    }
                }
                flush_group(&mut group, &mut children);
            } else {
                children.push(build_box_or_reuse(doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index));
                i += 1;
            }
        }
        // CSS Pseudo-elements L4 §4 — inject ::before / ::after for block-flow.
        // Only for Block / FlowRoot (not FormControl, not flex/grid item containers).
        if matches!(kind, BoxKind::Block | BoxKind::FlowRoot) {
            let before_ps =
                compute_pseudo_element_style(doc, id, "before", sheet, &style, viewport, dark_mode);
            let after_ps =
                compute_pseudo_element_style(doc, id, "after", sheet, &style, viewport, dark_mode);
            inject_pseudo(id, &mut children, before_ps, true, doc, viewport, counters, registry, false);
            inject_pseudo(id, &mut children, after_ps, false, doc, viewport, counters, registry, false);
            // CSS Lists L3 §2.1 — inject ::marker for list items.
            // ::marker comes before ::before in document order.
            if style.display == Display::ListItem {
                let ordinal = li_ordinal(doc, id);
                inject_marker(id, &mut children, &style, ordinal,
                              doc, sheet, viewport, dark_mode, counters, registry);
            }
        }
        } // end else (non-item-container)
        // CSS Display L3 §7.2 — flatten display:contents boxes into this context.
        // Must run for ALL child-building paths (item-container and non-item-container)
        // because flex/grid/table children may include display:contents elements whose
        // Contents boxes must be unpacked before lay_out sees them.
        flatten_contents(&mut children);
    }

    // SVG root: build SVG shape children (separate from HTML box-tree flow).
    if let BoxKind::SvgRoot { view_box, .. } = &kind {
        let own_svg_size = svg_root_own_size(&style, view_box.as_ref(), viewport);
        children = build_svg_children(doc, sheet, id, &style, viewport, own_svg_size, flat, dark_mode);
    }

    // Read HTML colspan/rowspan attributes for table-cell elements.
    let (col_span, row_span) = if style.display == Display::TableCell {
        let cs = doc
            .get(id)
            .get_attr("colspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        let rs = doc
            .get(id)
            .get_attr("rowspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        (cs, rs)
    } else {
        (1, 1)
    };

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style,
        kind,
        children,
        col_span,
        row_span,
        svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
    }
}

