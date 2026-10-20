use lumen_core::geom::{Rect, Size};

// ── внутренний размер grid-контейнера: fr/fit-content-столбцы и aspect-ratio (BUG-1309) ──
//
// Каждый случай — разметка WPT `css/css-grid`, где reftest падал на внутренней ширине.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn sizes(html: &str, css: &str, ids: &[&str]) -> Vec<(f32, f32)> {
    rects(html, css, ids).iter().map(|r| (r.width, r.height)).collect()
}

const TWO_ITEMS: &str = r#"<div id="g"><div id="a" class="item"><div class="content"></div></div><div id="b" class="item"><div class="content"></div></div></div>"#;
const ITEM_CSS: &str = ".item{max-width:max-content;box-sizing:border-box;padding:10px 20px}.content{width:50px;height:50px}";
const ONE_ITEM: &str = r#"<div id="g"><div id="a"></div></div>"#;

#[test]
fn max_content_grid_sums_implicit_fr_columns() {
    // `grid-auto-columns: 1fr` + `grid-auto-flow: column`: каждый item — свой столбец по 90.
    let css = format!(
        "{ITEM_CSS}#g{{display:grid;grid-auto-columns:1fr;grid-auto-flow:column;border:1px solid red;width:max-content}}"
    );
    let r = rects(TWO_ITEMS, &css, &["g", "a", "b"]);
    assert_eq!((r[0].width, r[1].width, r[2].x), (182.0, 90.0, 91.0), "{r:?}");
}

#[test]
fn fit_content_tracks_are_as_wide_as_their_items() {
    // `fit-content(30px)` не уже минимального вклада item'а (90), поэтому оба столбца по 90.
    let css = format!(
        "{ITEM_CSS}#g{{display:grid;grid-template-columns:fit-content(30px) fit-content(80px);border:1px solid red;width:max-content}}"
    );
    let r = rects(TWO_ITEMS, &css, &["g", "a", "b"]);
    assert_eq!((r[0].width, r[1].width, r[2].width), (182.0, 90.0, 90.0), "{r:?}");
}

#[test]
fn inline_grid_takes_width_from_percent_height_aspect_ratio_item() {
    // Строка 100px: `height: 100%` item'а = 100, `aspect-ratio: 1/1` даёт ему ширину 100.
    let r = sizes(ONE_ITEM, "#g{display:inline-grid;grid-template-rows:100px}#a{aspect-ratio:1/1;height:100%}", &["g", "a"]);
    assert_eq!(r, [(100.0, 100.0), (100.0, 100.0)]);
}

#[test]
fn item_spanning_two_rows_resolves_percent_height_against_both() {
    let r = sizes(
        ONE_ITEM,
        "#g{display:inline-grid;grid-template-rows:50px 50px}#a{aspect-ratio:1/1;height:100%;grid-row-start:1;grid-row-end:3}",
        &["g", "a"],
    );
    assert_eq!(r, [(100.0, 100.0), (100.0, 100.0)]);
}

#[test]
fn half_height_ratio_item_sets_inline_grid_width() {
    let r = sizes(ONE_ITEM, "#g{display:inline-grid;grid-template-rows:100px}#a{aspect-ratio:2/1;height:50%}", &["g", "a"]);
    assert_eq!(r, [(100.0, 100.0), (100.0, 50.0)]);
}

#[test]
fn auto_fill_repeat_runs_enough_times_for_transferred_min_height() {
    // `min-height: 60px` через `aspect-ratio: 1/1` — минимальная ширина 60: auto-fill по 50px
    // повторяется дважды (Grid L1 §7.2.3.2), контейнер 100×100.
    let r = sizes(
        r#"<div id="g"></div>"#,
        "#g{display:inline-grid;aspect-ratio:1/1;min-height:60px;grid-template-columns:repeat(auto-fill,50px)}",
        &["g"],
    );
    assert_eq!(r, [(100.0, 100.0)]);
}

#[test]
fn auto_fill_repeat_without_minimum_runs_once() {
    let r = sizes(r#"<div id="g"></div>"#, "#g{display:inline-grid;grid-template-columns:repeat(auto-fill,50px)}", &["g"]);
    assert_eq!(r[0].0, 50.0, "{r:?}");
}

#[test]
fn min_height_transfers_to_width_of_ratio_box() {
    let r = sizes(r#"<div id="g"></div>"#, "#g{display:inline-block;aspect-ratio:1/1;min-height:60px}", &["g"]);
    assert_eq!(r, [(60.0, 60.0)]);
}

#[test]
fn min_content_container_sees_percent_min_height_of_ratio_grid() {
    let r = sizes(
        r#"<div id="w"><div id="h"><div id="g"></div></div></div>"#,
        "#w{width:min-content;height:100px}#h{height:100%}\
         #g{display:grid;aspect-ratio:1/1;min-height:60%;grid-template-columns:repeat(auto-fill,50px)}",
        &["w", "g"],
    );
    assert_eq!(r, [(100.0, 100.0), (100.0, 100.0)]);
}

#[test]
fn canvas_percent_height_in_grid_item_uses_the_row() {
    // WPT `grid-*-width-percent-height-aspect-ratio-001`: холст 20×10, высота 100% от item'а,
    // а тот — 100% от строки 50px → холст 100×50, контейнер по ширине тоже 100.
    let cv = r#"<div id="w"><div id="g"><div id="i"><canvas id="c" width="20" height="10" style="height:100%"></canvas></div></div></div>"#;
    for display in [
        "display:grid;width:fit-content",
        "display:grid;float:left",
        "display:inline-grid",
        "display:grid;width:max-content",
        "display:grid;width:min-content",
    ] {
        let css = format!("#w{{width:400px}}#g{{{display};height:100px;grid-template-rows:50px}}#i{{height:100%}}");
        let r = sizes(cv, &css, &["g", "c"]);
        assert_eq!(r, [(100.0, 100.0), (100.0, 50.0)], "{display}");
    }
}
