//! BUG-1558: an empty plain inline element (`<span></span>`, `<br>`) owns no text fragment, so the
//! geometry collectors used to drop it (`[0,0,0,0]`, no `getClientRects` entry).

use lumen_core::geom::Size;

use super::super::layout_measured;

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

type Rects = std::collections::HashMap<u32, [f32; 4]>;
type ClientRects = std::collections::HashMap<u32, Vec<[f32; 4]>>;

fn build(html: &str, css: &str) -> (Rects, ClientRects, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Measurer);
    (crate::collect_layout_rects(&root, &doc), crate::collect_client_rects(&root, &doc), doc)
}

fn gbcr(r: &Rects, doc: &lumen_dom::Document, id: &str) -> Option<[f32; 4]> {
    r.get(&(doc.find_by_id(id).unwrap().index() as u32)).copied()
}

fn crs(r: &ClientRects, doc: &lumen_dom::Document, id: &str) -> Vec<[f32; 4]> {
    r.get(&(doc.find_by_id(id).unwrap().index() as u32)).cloned().unwrap_or_default()
}

/// Fixed `char_width = size / 2`, so "abc " at 20px is 4 * 10 = 40 wide.
#[test]
fn empty_span_between_words_sits_after_the_collapsed_space() {
    let (r, c, doc) = build(
        r#"<p style="font-size:20px;margin:0">abc <span id=s></span> def</p>"#,
        "",
    );
    let g = gbcr(&r, &doc, "s").expect("пустой span имеет rect");
    assert!((g[0] - 40.0).abs() < 0.5, "x = {g:?}");
    assert_eq!(g[2], 0.0);
    assert!(g[3] > 0.0, "высота строки: {g:?}");
    assert_eq!(crs(&c, &doc, "s"), vec![g]);
}

#[test]
fn empty_span_without_a_space_before_it_hugs_the_previous_word() {
    let (r, _, doc) = build(r#"<p style="font-size:20px;margin:0">abc<span id=s></span> def</p>"#, "");
    let g = gbcr(&r, &doc, "s").unwrap();
    assert!((g[0] - 30.0).abs() < 0.5, "x = {g:?}");
}

#[test]
fn empty_span_after_a_br_goes_to_the_start_of_the_next_line() {
    let (r, _, doc) = build(
        r#"<p style="font-size:20px;margin:0">abc<br><span id=t></span><span id=u>ref</span></p>"#,
        "",
    );
    let (t, u) = (gbcr(&r, &doc, "t").unwrap(), gbcr(&r, &doc, "u").unwrap());
    assert_eq!((t[0], t[1]), (u[0], u[1]), "{t:?} vs {u:?}");
    assert!(t[1] > 0.0);
}

#[test]
fn nested_empty_spans_share_one_place_and_an_ancestor_with_text_is_not_widened() {
    let (r, _, doc) = build(
        r#"<p style="font-size:20px;margin:0">ab<span id=o><span id=i></span></span>cd <b id=b>xy<i id=e></i></b></p>"#,
        "",
    );
    assert_eq!(gbcr(&r, &doc, "o"), gbcr(&r, &doc, "i"));
    let (b, e) = (gbcr(&r, &doc, "b").unwrap(), gbcr(&r, &doc, "e").unwrap());
    assert!((b[0] + b[2] - e[0]).abs() < 0.5, "{b:?} {e:?}");
    assert_eq!(b[2], 20.0);
}

#[test]
fn a_block_with_only_an_empty_span_stays_zero_high() {
    let (r, _, doc) = build(
        r#"<div id=d style="font-size:20px"><span></span></div><div id=e style="height:5px"></div>"#,
        "",
    );
    assert_eq!(gbcr(&r, &doc, "d").unwrap()[3], 0.0);
    assert_eq!(gbcr(&r, &doc, "e").unwrap()[1], 0.0);
}
