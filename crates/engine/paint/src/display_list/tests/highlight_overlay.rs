//! Highlight overlays on text (CSS Pseudo-Elements L4 §3.6): how a fragment cut by a
//! `::selection` range is painted. Font — `Fixed8`: 8 px per character.

use super::text_and_images::{Fixed8, BODY_RESET};
use super::*;

/// One command of the text of a display list, abridged.
#[derive(Debug, PartialEq)]
enum Op {
    Fill { x: f32, w: f32, rgb: (u8, u8, u8) },
    Text { text: String, x: f32, rgb: (u8, u8, u8) },
}

fn rgb(c: Color) -> (u8, u8, u8) {
    (c.r, c.g, c.b)
}

/// The fills and texts of `html` with `range` (byte offsets in the first text node whose
/// content is `node_text`) selected, in paint order.
fn paint(html: &str, css: &str, node_text: &str, from: u32, to: u32) -> Vec<Op> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
    let vp = Size::new(800.0, 600.0);
    let tree = lumen_layout::layout_measured(&doc, &sheet, vp, &Fixed8);
    let node = (0..doc.len())
        .map(lumen_dom::NodeId::from_index)
        .find(|&n| matches!(&doc.get(n).data, lumen_dom::NodeData::Text(t) if &**t == node_text))
        .expect("text node");
    let ranges = [lumen_dom::Range {
        start: lumen_dom::DomPosition { container: node, offset: from },
        end: lumen_dom::DomPosition { container: node, offset: to },
    }];
    let env = lumen_layout::HighlightEnv { doc: &doc, sheet: &sheet, viewport: vp, dark_mode: false, measurer: &Fixed8 };
    let layer = lumen_layout::HighlightLayer {
        pseudo: "selection",
        ranges: &ranges,
        ua_background: Some(lumen_layout::UA_SELECTION_BACKGROUND),
    };
    let highlights = lumen_layout::resolve_highlights(&tree, &env, &[layer]).map(std::sync::Arc::new);
    let dl = with_highlights(highlights, || build_display_list(&tree));
    dl.iter()
        .filter_map(|c| match c {
            DisplayCommand::FillRect { rect, color } => Some(Op::Fill { x: rect.x, w: rect.width, rgb: rgb(*color) }),
            DisplayCommand::DrawText { rect, text, color, .. } => {
                Some(Op::Text { text: text.clone(), x: rect.x, rgb: rgb(*color) })
            }
            _ => None,
        })
        .collect()
}

fn text(t: &str, x: f32, rgb: (u8, u8, u8)) -> Op {
    Op::Text { text: t.to_string(), x, rgb }
}

/// `::selection { color }` recolours only the selected glyphs, which are not painted
/// twice: the originating colour stays on the unselected pieces alone.
#[test]
fn selected_glyphs_take_the_highlight_colour() {
    let ops = paint("<p>quick</p>", "p{color:red} p::selection{color:lime}", "quick", 1, 4);
    assert_eq!(
        ops,
        vec![text("q", 0.0, (255, 0, 0)), text("uic", 8.0, (0, 255, 0)), text("k", 32.0, (255, 0, 0))]
    );
}

/// The UA background paints behind the selected glyphs only, over the exact stretch.
#[test]
fn ua_background_covers_the_selected_stretch() {
    let ops = paint("<p>quick</p>", "", "quick", 1, 4);
    assert_eq!(
        ops,
        vec![
            text("q", 0.0, (0, 0, 0)),
            Op::Fill { x: 8.0, w: 24.0, rgb: (0x30, 0x8a, 0xff) },
            text("uic", 8.0, (0, 0, 0)),
            text("k", 32.0, (0, 0, 0)),
        ]
    );
}

/// highlight-painting-001: the element's shadows go first for every piece, then the
/// highlight's background, its shadows and its glyphs over the selected one.
#[test]
fn shadows_then_background_then_highlight_shadows_then_glyphs() {
    let ops = paint(
        "<p>quick</p>",
        "p{text-shadow:4px 4px #c0c000} p::selection{background:#c0c0c0;text-shadow:8px 8px #3838e0}",
        "quick",
        1,
        4,
    );
    let yellow = (0xc0, 0xc0, 0x00);
    let blue = (0x38, 0x38, 0xe0);
    assert_eq!(
        ops,
        vec![
            // 1. the originating shadows of all three pieces
            text("q", 4.0, yellow),
            text("uic", 12.0, yellow),
            text("k", 36.0, yellow),
            // 2. the unselected glyph before the selection
            text("q", 0.0, (0, 0, 0)),
            // 3. the selection: background, its shadow, its glyphs
            Op::Fill { x: 8.0, w: 24.0, rgb: (0xc0, 0xc0, 0xc0) },
            text("uic", 16.0, blue),
            text("uic", 8.0, (0, 0, 0)),
            // 4. the unselected glyph after it
            text("k", 32.0, (0, 0, 0)),
        ]
    );
}

/// Without a selection (or with no highlights installed) nothing changes.
#[test]
fn no_highlights_no_change() {
    let doc = lumen_html_parser::parse("<p>quick</p>");
    let sheet = lumen_css_parser::parse(BODY_RESET);
    let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    assert_eq!(with_highlights(None, || build_display_list(&tree)), build_display_list(&tree));
}

#[cfg(feature = "cpu-render")]
mod reftests {
    use super::*;

    // ── Reftests: the WPT css-pseudo highlight pair, painted by the CPU rasterizer ─────────

    /// `html` laid out with the bundled Inter metrics, the selection `select` made by the page
    /// applied, rasterized to RGBA — what `--screenshot` produces for it.
    fn screenshot(css: &str, html: &str, select: impl FnOnce(&lumen_dom::Document) -> Option<lumen_dom::Range>) -> lumen_image::Image {
        let font = lumen_font::Font::parse(include_bytes!("../../../../../../assets/fonts/Inter-Regular.ttf"))
            .expect("bundled Inter");
        let measurer = crate::FontMeasurer::new(&font).expect("measurer");
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let vp = Size::new(600.0, 300.0);
        let tree = lumen_layout::layout_measured(&doc, &sheet, vp, &measurer);
        let ranges: Vec<_> = select(&doc).into_iter().collect();
        let env = lumen_layout::HighlightEnv { doc: &doc, sheet: &sheet, viewport: vp, dark_mode: false, measurer: &measurer };
        let layer = lumen_layout::HighlightLayer {
            pseudo: "selection",
            ranges: &ranges,
            ua_background: Some(lumen_layout::UA_SELECTION_BACKGROUND),
        };
        let highlights = lumen_layout::resolve_highlights(&tree, &env, &[layer]).map(std::sync::Arc::new);
        let dl = with_highlights(highlights, || {
            let stacking = StackingTree::build(&tree);
            let order = PaintOrder::from_tree(&stacking);
            build_display_list_ordered(&tree, &stacking, &order).0
        });
        crate::cpu_raster::rasterize_cpu(600, 300, &dl, &[], 0.0, 0.0).expect("rasterize")
    }

    fn element_by_id(doc: &lumen_dom::Document, id: &str) -> lumen_dom::NodeId {
        (0..doc.len())
            .map(lumen_dom::NodeId::from_index)
            .find(|&n| doc.get(n).get_attr("id") == Some(id))
            .expect("element")
    }

    fn select_contents(id: &'static str) -> impl FnOnce(&lumen_dom::Document) -> Option<lumen_dom::Range> {
        move |doc| {
            let el = element_by_id(doc, id);
            let n = doc.get(el).children.len() as u32;
            Some(lumen_dom::Range {
                start: lumen_dom::DomPosition { container: el, offset: 0 },
                end: lumen_dom::DomPosition { container: el, offset: n },
            })
        }
    }

    /// css-pseudo/active-selection-011: `div::selection { color: green }` over red text paints
    /// green glyphs on a white (not UA-blue) background — the same pixels as green text.
    #[test]
    fn reftest_active_selection_colour() {
        let css = "body{margin:8px} div{color:red;font-size:300%} div::selection{color:green}";
        let html = "<div id=test>Selected Text</div>";
        let a = screenshot(css, html, select_contents("test"));
        assert_ne!(a.data, screenshot(css, html, |_| None).data, "the selection must change the pixels");
        let b = screenshot(
            "body{margin:8px} div{color:green;font-size:300%}",
            "<div>Selected Text</div>",
            |_| None,
        );
        assert_eq!(a.data, b.data);
    }

    /// css-pseudo/highlight-painting-001: the selected "uic" of a shadowed word paints the
    /// element's shadow, the highlight's background, its shadow and then its glyphs.
    #[test]
    fn reftest_highlight_painting_order() {
        let css = "body{margin:8px} p{font-size:7em;text-shadow:.25em .25em #C0C000;line-height:1;margin:0}                    p::selection{background:#C0C0C0;text-shadow:.5em .5em #3838E0}";
        let html = "<p id=p>quick</p>";
        let select = |doc: &lumen_dom::Document| {
            let p = element_by_id(doc, "p");
            let t = doc.get(p).children[0];
            Some(lumen_dom::Range {
                start: lumen_dom::DomPosition { container: t, offset: 1 },
                end: lumen_dom::DomPosition { container: t, offset: 4 },
            })
        };
        let a = screenshot(css, html, select);
        assert_ne!(a.data, screenshot(css, html, |_| None).data, "the selection must change the pixels");
        let b = screenshot(
            "body{margin:8px} p{font-size:7em;text-shadow:.25em .25em #C0C000;line-height:1;margin:0;             position:relative;color:transparent} p>span{color:initial;text-shadow:none;position:absolute}              p>span>span{background:#C0C0C0;text-shadow:.5em .5em #3838E0;position:relative}",
            "<p><span>q<span>uic</span>k</span>quick</p>",
            |_| None,
        );
        assert_eq!(a.data, b.data);
    }
}
