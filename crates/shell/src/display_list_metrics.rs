//! Building a display list in paint order, measuring one, and the split-view
//! placeholder list.
//!
//! `paint_ordered` is the crate's single entry point for turning a laid-out
//! tree into a display list with the CSS 2.1 Appendix E painting order;
//! `content_height_of` / `content_width_of` derive the painted extent from
//! the emitted display list and `page_content_extent` widens it by the
//! layout's scrollable overflow (a spacer that paints nothing still scrolls),
//! `next_dl_epoch` versions it, and `build_split_placeholder` synthesises a
//! minimal list for a hibernated tab whose real one was evicted. Moved out of
//! `main.rs` by the SPLIT track (batches SH-5 and SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Полная высота контента в CSS px — `max(rect.y + rect.height)` по всем
/// rect-несущим командам display list-а. Используется для clamping-а scroll_y.
pub(crate) fn content_height_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_y = 0.0_f32;
    for cmd in dl {
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawTextStroke { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::LazyImageSlot { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushClipRoundedRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::DrawBorderImage { area, .. } => area,
            DisplayCommand::PopClip
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::BeginFixedLayer
            | DisplayCommand::EndFixedLayer
            | DisplayCommand::BeginFixedBackground
            | DisplayCommand::EndFixedBackground
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawSvgFill { .. }
            | DisplayCommand::DrawSvgStroke { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
        };
        let bottom = r.y + r.height;
        if bottom > max_y {
            max_y = bottom;
        }
    }
    max_y
}

/// Scroll range of the page — `(content_height, content_width)` in CSS px:
/// what the display list paints, extended by the layout's scrollable overflow
/// ([`lumen_layout::page_scrollable_extent`]) so an invisible spacer or an
/// empty sized block still gives the page room to scroll (SMOOTH-SCROLL: WPT
/// `scrollIntoView-smooth.html` scrolls to a box inside a 2500x2500 container
/// that paints nothing itself).
pub(crate) fn page_content_extent(
    dl: &lumen_paint::DisplayList,
    lb: &lumen_layout::LayoutBox,
    viewport_width: f32,
) -> (f32, f32) {
    let (lw, lh) = lumen_layout::page_scrollable_extent(lb);
    let w = content_width_in(dl, lb, viewport_width);
    // `overflow-x: hidden|clip` на viewport-е (BUG-1642) отсекает и scrollable overflow раскладки.
    let w = if viewport_clips_x(lb) { w } else { w.max(lw) };
    (content_height_of(dl).max(lh), w)
}

/// Полная ширина контента в CSS px — `max(rect.x + rect.width)` по всем
/// rect-несущим командам display list-а, обрезанным клипами предков: содержимое под
/// `overflow: hidden` не прокручивается за край клипа (BUG-1642 — подложка
/// `left: -50vw; width: 150vw` под `overflow: hidden` давала лишнюю h-полосу).
/// Используется для clamping-а scroll_x.
pub(crate) fn content_width_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_x = 0.0_f32;
    // Правые края открытых клипов; общий `PopClip` закрывает и `PushClipPath`, чья форма
    // ширину не ограничивает (наследует край внешнего клипа).
    let mut clip_right: Vec<f32> = Vec::new();
    for cmd in dl {
        let outer = clip_right.last().copied().unwrap_or(f32::INFINITY);
        match cmd {
            DisplayCommand::PushClipRect { rect } | DisplayCommand::PushClipRoundedRect { rect, .. } => {
                clip_right.push(outer.min(rect.x + rect.width));
            }
            DisplayCommand::PushScrollLayer { clip_rect, .. } => {
                clip_right.push(outer.min(clip_rect.x + clip_rect.width));
            }
            DisplayCommand::PushClipPath { .. } => clip_right.push(outer),
            DisplayCommand::PopClip | DisplayCommand::PopScrollLayer => {
                clip_right.pop();
                continue;
            }
            _ => {}
        }
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawTextStroke { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::LazyImageSlot { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushClipRoundedRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::DrawBorderImage { area, .. } => area,
            DisplayCommand::PopClip
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::BeginFixedLayer
            | DisplayCommand::EndFixedLayer
            | DisplayCommand::BeginFixedBackground
            | DisplayCommand::EndFixedBackground
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawSvgFill { .. }
            | DisplayCommand::DrawSvgStroke { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
        };
        // Клип, открытый самой командой, уже на вершине стека — `outer` его не знает.
        let limit = clip_right.last().copied().unwrap_or(f32::INFINITY);
        let right = (r.x + r.width).min(limit);
        if right > max_x {
            max_x = right;
        }
    }
    max_x
}

/// Горизонтально ли viewport не прокручивается пользователем: `overflow-x: hidden | clip` у
/// `<html>` (или у `<body>`, когда у `<html>` `visible`) уходит на viewport (CSS Overflow L3
/// §3.5) — страница не растягивается вбок, что бы ни торчало за правым краем.
fn viewport_clips_x(root: &lumen_layout::LayoutBox) -> bool {
    use lumen_layout::Overflow;
    // `<html>` помечен всегда, `<body>` — только при `visible` у `<html>`: источник — первый
    // помеченный с не-`visible` значением (иначе `<html>` с `visible` заслонил бы `<body>`).
    fn source(b: &lumen_layout::LayoutBox, depth: u8) -> Option<&lumen_layout::LayoutBox> {
        let own = b.style.overflow_to_viewport
            && !(b.style.overflow_x == Overflow::Visible && b.style.overflow_y == Overflow::Visible);
        if own {
            return Some(b);
        }
        if depth == 0 {
            return None;
        }
        b.children.iter().find_map(|c| source(c, depth - 1))
    }
    source(root, 3).is_some_and(|b| matches!(b.style.overflow_x, Overflow::Hidden | Overflow::Clip))
}

/// [`content_width_of`] с учётом `overflow-x` корня: при `hidden`/`clip` на viewport ширина
/// контента не превышает `viewport_width` (BUG-1642 — `html { overflow-x: hidden }` рисовал
/// h-полосу, хотя в Chrome её нет).
pub(crate) fn content_width_in(
    dl: &lumen_paint::DisplayList,
    root: &lumen_layout::LayoutBox,
    viewport_width: f32,
) -> f32 {
    let w = content_width_of(dl);
    if viewport_clips_x(root) { w.min(viewport_width) } else { w }
}

/// Build a minimal placeholder display list for a hibernated tab in split view.
///
/// Shows a dark grey background with the URL text — used when the hibernated
/// tab's full display list has been evicted from memory.
pub(crate) fn build_split_placeholder(url: &str) -> lumen_paint::DisplayList {
    use lumen_layout::{Color, FontStyle, FontWeight};
    use lumen_paint::DisplayCommand;

    let bg = Color { r: 30, g: 30, b: 35, a: 255 };
    let fg = Color { r: 180, g: 180, b: 190, a: 255 };
    vec![
        // Background fill — large enough to cover any viewport half.
        DisplayCommand::FillRect {
            rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 4096.0, height: 4096.0 },
            color: bg,
        },
        // URL label near vertical centre of a typical viewport half.
        DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: lumen_core::geom::Rect { x: 16.0, y: 300.0, width: 480.0, height: 20.0 },
            text: url.to_owned(),
            font_size: 13.0,
            color: fg,
            font_family: vec![],
            font_weight: FontWeight::new(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        },
    ]
}

/// Строит display list с правильным painting order (CSS 2.1 Appendix E, z-index stacking).
pub(crate) fn paint_ordered(layout: &lumen_layout::LayoutBox) -> DisplayList {
    let tree = StackingTree::build(layout);
    let order = PaintOrder::from_tree(&tree);
    build_display_list_ordered(layout, &tree, &order).0
}


/// [`paint_ordered`] с кэшем emit по поддереву, который держит вызывающий между кадрами
/// (PERF-16 срез 2). Результат тот же; `LUMEN_NO_EMIT_CACHE=1` возвращает сборку без кэша.
pub(crate) fn paint_ordered_cached(
    layout: &lumen_layout::LayoutBox,
    cache: &mut lumen_paint::SubtreeEmitCache,
) -> DisplayList {
    if !lumen_paint::emit_cache_enabled() {
        return paint_ordered(layout);
    }
    let tree = StackingTree::build(layout);
    let order = PaintOrder::from_tree(&tree);
    lumen_paint::build_display_list_ordered_dpr_cached(layout, &tree, &order, 1.0, cache).0
}
/// [`paint_ordered`] для документа, разложенного под `viewport`: его
/// `background-attachment: fixed` слои позиционируются от этого вьюпорта
/// (CSS Backgrounds L3 §3.6). Корневой бокс — `max(вьюпорт, документ)`, так
/// что без вьюпорта фиксированный фон длинной страницы растянулся бы на весь
/// документ.
pub(crate) fn paint_ordered_in(layout: &lumen_layout::LayoutBox, viewport: lumen_core::geom::Size) -> DisplayList {
    lumen_paint::with_fixed_background_viewport(viewport, || paint_ordered(layout))
}

/// One entry of `CSS.highlights` (CSS Custom Highlight API L1): the ranges of a `Highlight`
/// object under the name `::highlight(<name>)` styles it by.
pub(crate) struct CustomHighlight {
    pub(crate) name: String,
    /// Stacking rank among the highlights: `spelling-error` below `grammar-error` below a
    /// plain `highlight` (§3.1), then `Highlight.priority`, then registration order.
    sort_key: (u8, i32, usize),
    pub(crate) ranges: Vec<lumen_dom::Range>,
}

/// Reads the page's `CSS.highlights` registry. Empty when the page has no JS, never
/// touched the Highlight API or the read fails — a highlight is cosmetic, never an error.
pub(crate) fn custom_highlights(js: Option<&dyn crate::persistent_js::PersistentJs>) -> Vec<CustomHighlight> {
    // `Range.__start_nid__`/`__start_off__`/… are what `getSelection().addRange` hands to
    // `_lumen_set_selection`, i.e. already the DOM's own positions.
    const READ: &str = "(function(){var out=[];try{if(typeof CSS==='undefined'||!CSS.highlights)return out;        var i=0;CSS.highlights.forEach(function(h,name){var rs=[];h.forEach(function(r){        var sn=r.__start_nid__,en=r.__end_nid__;if(sn==null||en==null)return;        rs.push([sn,r.__start_off__,en,r.__end_off__]);});        out.push({name:String(name),priority:h.priority|0,type:String(h.type),order:i++,ranges:rs});});}        catch(e){}return out;})()";
    let Some(json) = js.and_then(|j| j.eval_js_value(READ).ok()) else { return Vec::new() };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&json) else { return Vec::new() };
    let pos = |v: &serde_json::Value, i: usize| v.get(i).and_then(serde_json::Value::as_u64).map(|n| n as u32);
    entries
        .iter()
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?.to_owned();
            let rank = match e.get("type").and_then(serde_json::Value::as_str) {
                Some("spelling-error") => 0,
                Some("grammar-error") => 1,
                _ => 2,
            };
            let priority = e.get("priority").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32;
            let order = e.get("order").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
            let ranges = e
                .get("ranges")?
                .as_array()?
                .iter()
                .filter_map(|r| {
                    let at = |c: usize, o: usize| {
                        Some(lumen_dom::DomPosition {
                            container: lumen_dom::NodeId::from_index(pos(r, c)? as usize),
                            offset: pos(r, o)?,
                        })
                    };
                    Some(lumen_dom::Range { start: at(0, 1)?, end: at(2, 3)? })
                })
                .collect();
            Some(CustomHighlight { name, sort_key: (rank, priority, order), ranges })
        })
        .collect()
}

/// Highlights of the page — `CSS.highlights` entries under the document's selection —
/// resolved against `layout`. `None` when nothing is highlighted, the common case, which
/// costs one range read. The DOM selection is the JS `getSelection()` one and the mouse one.
pub(crate) fn page_highlights(
    layout: &lumen_layout::LayoutBox,
    doc: &lumen_dom::Document,
    sheet: &lumen_css_parser::Stylesheet,
    viewport: lumen_core::geom::Size,
    dark_mode: bool,
    measurer: &dyn lumen_layout::TextMeasurer,
    custom: &[CustomHighlight],
) -> Option<Arc<lumen_layout::Highlights>> {
    let selection: Vec<lumen_dom::Range> =
        doc.get_selection().get_range().filter(|r| !r.is_collapsed()).into_iter().collect();
    if selection.is_empty() && custom.iter().all(|c| c.ranges.iter().all(lumen_dom::Range::is_collapsed)) {
        return None;
    }
    // Lowest layer first (CSS Pseudo-Elements L4 §3.6): custom highlights by their rank,
    // then `::selection` on top.
    let mut sorted: Vec<&CustomHighlight> = custom.iter().collect();
    sorted.sort_by_key(|c| c.sort_key);
    let pseudos: Vec<String> = sorted.iter().map(|c| format!("highlight({})", c.name)).collect();
    let mut layers: Vec<lumen_layout::HighlightLayer<'_>> = sorted
        .iter()
        .zip(&pseudos)
        .map(|(c, pseudo)| lumen_layout::HighlightLayer { pseudo, ranges: &c.ranges, ua_background: None })
        .collect();
    layers.push(lumen_layout::HighlightLayer {
        pseudo: "selection",
        ranges: &selection,
        ua_background: Some(lumen_layout::UA_SELECTION_BACKGROUND),
    });
    let env = lumen_layout::HighlightEnv { doc, sheet, viewport, dark_mode, measurer };
    lumen_layout::resolve_highlights(layout, &env, &layers).map(Arc::new)
}

/// [`paint_ordered_in`] with `highlights` painted over the text they cover.
pub(crate) fn paint_ordered_in_highlighted(
    layout: &lumen_layout::LayoutBox,
    viewport: lumen_core::geom::Size,
    highlights: Option<Arc<lumen_layout::Highlights>>,
) -> DisplayList {
    lumen_paint::with_highlights(highlights, || paint_ordered_in(layout, viewport))
}

/// Следующая версия display list-а; `0` пропускается — он зарезервирован за
/// «версия неизвестна» (BUG-405 срез 39).
pub(crate) fn next_dl_epoch(cur: u64) -> u64 {
    match cur.wrapping_add(1) {
        0 => 1,
        n => n,
    }
}
