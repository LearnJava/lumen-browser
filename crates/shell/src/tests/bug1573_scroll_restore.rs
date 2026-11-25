//! BUG-1573: каждый продюсер `LayoutBox` обязан переносить смещения
//! прокрутки — иначе `scroll_states` для JS публикует нули и `scrollTop`
//! обнуляется после мутации стиля (~50–150 мс, когда доходит relayout).
//!
//! Здесь покрыты обёртки `compute_layout_*` на UI-потоке. Engine-thread
//! `relayout_job` и стриминговый релэйаут `page_load` делят то же правило
//! (`scroll_offsets` → `restore_scroll_positions`), но их job-замыкания
//! не поднимаются в юнит-тесте — проверены живым стендом
//! `.tmp/scroll_repro.html` (см. карточку бага).

use super::*;

const BUG1573_CSS: &str = ".c{width:200px;height:200px;overflow:auto}\n.c div{height:150px;width:150px;background:#dde}";
const BUG1573_CSS_RESTYLED: &str = ".c{width:200px;height:200px;overflow:auto;color:red}\n.c div{height:150px;width:150px;background:#dde}";

fn bug1573_doc() -> (lumen_dom::Document, lumen_dom::NodeId) {
    let html = concat!(
        "<!doctype html><html><body>",
        "<div class=\"c\" id=\"c\"><div></div><div></div><div></div><div></div><div></div></div>",
        "</body></html>",
    );
    let doc = lumen_html_parser::parse(html);
    let nid = doc.find_by_id("c").expect("fixture has #c");
    (doc, nid)
}

fn bug1573_scroll_y(layout: &lumen_layout::LayoutBox, nid: lumen_dom::NodeId) -> f32 {
    lumen_layout::find_box_by_node(layout, nid)
        .map(|b| b.scroll_y)
        .unwrap_or_else(|| panic!("scroller box missing"))
}

/// Полный проход → скролл → инкрементальный graft-проход с ИЗМЕНЁННЫМ
/// листом (бокс скроллера пересобирается — чистый reuse перенёс бы
/// offset и без restore). Без `restore_scroll_positions` получилось бы 0.
#[test]
fn compute_layout_incremental_restores_scroll_offsets() {
    let (doc, nid) = bug1573_doc();
    let document = Mutex::new(doc);
    let sheet = lumen_css_parser::parse(BUG1573_CSS);
    let sheet2 = lumen_css_parser::parse(BUG1573_CSS_RESTYLED);
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(800.0, 600.0);

    let (_dl, mut layout, _c) = crate::relayout::compute_layout_with_counters(
        &document, &sheet, viewport, &hyp, false, &[],
    );
    assert!(lumen_layout::set_scroll_position(&mut layout, nid, 0.0, 100.0));
    assert_eq!(bug1573_scroll_y(&layout, nid), 100.0);

    let (_dl2, layout2, _c2) = crate::relayout::compute_layout_incremental(
        &document, &sheet2, viewport, &hyp, false, &[], &layout,
    );
    assert_eq!(bug1573_scroll_y(&layout2, nid), 100.0, "incremental graft lost scroll offset");
}

/// Тот же сценарий для restyle-ветки (`layout_mutation_incremental_restyle`)
/// — основной путь DOM-мутаций на rAF-тиках.
#[test]
fn compute_layout_incremental_restyle_restores_scroll_offsets() {
    let (doc, nid) = bug1573_doc();
    let document = Mutex::new(doc);
    let sheet = lumen_css_parser::parse(BUG1573_CSS);
    let sheet2 = lumen_css_parser::parse(BUG1573_CSS_RESTYLED);
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(800.0, 600.0);

    let (_dl, mut layout, counters) = crate::relayout::compute_layout_with_counters(
        &document, &sheet, viewport, &hyp, false, &[],
    );
    let prev_styles = counters.into_styles();
    assert!(lumen_layout::set_scroll_position(&mut layout, nid, 0.0, 100.0));

    // Скроллер — dirty root: его поддерево пересчитывается заново.
    let mut dirty_roots = std::collections::HashSet::new();
    dirty_roots.insert(nid);
    let delta = lumen_layout::counters::RestyleDelta {
        invalidation: None,
        attr_changed: None,
        prev_styles,
        dirty_roots,
        shallow_roots: Default::default(),
        point_roots: Default::default(),
        content_dirty: lumen_layout::counters::ContentDirty::Untracked,
    };
    let (_dl2, layout2, _c2) = crate::relayout::compute_layout_incremental_restyle(
        &document, &sheet2, viewport, &hyp, false, &[], layout, delta,
    );
    assert_eq!(bug1573_scroll_y(&layout2, nid), 100.0, "restyle incremental lost scroll offset");
}

/// Негативный контроль: сам `layout_mutation_incremental_with_counters`
/// offset НЕ переносит (graft держит его только в чистых переиспользованных
/// поддеревьях) — именно поэтому обёртки `compute_layout_*` обязаны звать
/// `restore_scroll_positions`. Если этот тест вдруг начнёт отдавать 100,
/// контракт изменился и обёртки можно пересмотреть.
#[test]
fn raw_incremental_layout_loses_scroll_offset_without_restore() {
    let (doc, nid) = bug1573_doc();
    let sheet = lumen_css_parser::parse(BUG1573_CSS);
    let sheet2 = lumen_css_parser::parse(BUG1573_CSS_RESTYLED);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(800.0, 600.0);

    let (mut layout, _c) = lumen_layout::layout_measured_hyp_with_counters(
        &doc, &sheet, viewport, &measurer, &hyp, false,
    );
    assert!(lumen_layout::set_scroll_position(&mut layout, nid, 0.0, 100.0));

    let (layout2, _c2) = lumen_layout::layout_mutation_incremental_with_counters(
        &doc, &sheet2, viewport, &measurer, &hyp, false, &layout,
    );
    assert_eq!(
        bug1573_scroll_y(&layout2, nid),
        0.0,
        "raw incremental unexpectedly preserved the offset — wrapper restore may be redundant"
    );
}

/// Скролл, выставленный ПОСЛЕ первого инкрементального дерева, тоже
/// переживает следующий — offsets снимаются с `prev` каждого прохода,
/// а не только из исходного layout.
#[test]
fn scroll_offsets_survive_a_second_incremental_pass() {
    let (doc, nid) = bug1573_doc();
    let document = Mutex::new(doc);
    let sheet = lumen_css_parser::parse(BUG1573_CSS);
    let sheet2 = lumen_css_parser::parse(BUG1573_CSS_RESTYLED);
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(800.0, 600.0);

    let (_dl, mut layout, _c) = crate::relayout::compute_layout_with_counters(
        &document, &sheet, viewport, &hyp, false, &[],
    );
    assert!(lumen_layout::set_scroll_position(&mut layout, nid, 0.0, 60.0));
    let (_dl2, mut layout2, _c2) = crate::relayout::compute_layout_incremental(
        &document, &sheet2, viewport, &hyp, false, &[], &layout,
    );
    assert_eq!(bug1573_scroll_y(&layout2, nid), 60.0);
    // Второй скролл поверх уже инкрементального дерева.
    assert!(lumen_layout::set_scroll_position(&mut layout2, nid, 0.0, 120.0));
    let (_dl3, layout3, _c3) = crate::relayout::compute_layout_incremental(
        &document, &sheet, viewport, &hyp, false, &[], &layout2,
    );
    assert_eq!(bug1573_scroll_y(&layout3, nid), 120.0, "second incremental pass lost scroll offset");
}
