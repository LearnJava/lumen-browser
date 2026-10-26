//! Highlight resolution: range → text node spans, the `::selection` chain, the cut of a
//! fragment into pieces. Font — `Fixed8`: 8 px per character.

use super::*;
use crate::{layout_measured, BoxKind, LayoutBox};
use lumen_dom::DomPosition;

struct Fixed8;

impl TextMeasurer for Fixed8 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        8.0
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn setup(html: &str, css: &str) -> (Document, Stylesheet, LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    (doc, sheet, root)
}

fn text_node(doc: &Document, content: &str) -> NodeId {
    (0..doc.len())
        .map(NodeId::from_index)
        .find(|&n| matches!(&doc.get(n).data, NodeData::Text(t) if &**t == content))
        .unwrap_or_else(|| panic!("text node {content:?}"))
}

fn element(doc: &Document, tag: &str) -> NodeId {
    (0..doc.len())
        .map(NodeId::from_index)
        .find(|&n| doc.get(n).element_name().is_some_and(|q| q.local.as_str() == tag))
        .unwrap_or_else(|| panic!("<{tag}>"))
}

fn pos(container: NodeId, offset: u32) -> DomPosition {
    DomPosition { container, offset }
}

fn selection_layer(ranges: &[Range]) -> HighlightLayer<'_> {
    HighlightLayer { pseudo: "selection", ranges, ua_background: Some(UA_SELECTION_BACKGROUND) }
}

fn resolve(doc: &Document, sheet: &Stylesheet, root: &LayoutBox, ranges: &[Range]) -> Option<Highlights> {
    let env = HighlightEnv { doc, sheet, viewport: Size::new(800.0, 600.0), dark_mode: false, measurer: &Fixed8 };
    resolve_highlights(root, &env, &[selection_layer(ranges)])
}

fn first_frag(b: &LayoutBox) -> Option<&InlineFrag> {
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        return lines.iter().flatten().next();
    }
    b.children.iter().find_map(first_frag)
}

/// A range inside one text node cuts its fragment into before / selected / after.
#[test]
fn partial_range_cuts_the_fragment() {
    let (doc, sheet, root) = setup(
        "<body><p>quick</p></body>",
        "body{margin:0} p{margin:0} p::selection{background:#c0c0c0}",
    );
    let t = text_node(&doc, "quick");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 1), end: pos(t, 4) }]).expect("highlights");
    let frag = first_frag(&root).expect("frag");
    let pieces = hl.pieces(frag).expect("pieces");
    let cuts: Vec<_> = pieces.iter().map(|p| (p.bytes.clone(), p.x, p.width, p.looks.len())).collect();
    assert_eq!(cuts, vec![(0..1, 0.0, 8.0, 0), (1..4, 8.0, 24.0, 1), (4..5, 32.0, 8.0, 0)]);
    let look = &pieces[1].looks[0];
    assert_eq!(look.background, Some(Color { r: 0xc0, g: 0xc0, b: 0xc0, a: 255 }));
    assert_eq!(look.color, None, "no declared colour — the text keeps its own");
}

/// An author `::selection { color }` alone paints no UA background (§3.2).
#[test]
fn author_rule_replaces_the_ua_background() {
    let (doc, sheet, root) = setup("<body><p>quick</p></body>", "p::selection{color:green}");
    let t = text_node(&doc, "quick");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 5) }]).expect("highlights");
    let look = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks[0];
    assert_eq!(look.background, None);
    assert_eq!(look.color, Some(Color { r: 0, g: 128, b: 0, a: 255 }));
}

/// No `::selection` rule: the UA background, the text colour untouched.
#[test]
fn no_rule_gives_the_ua_default() {
    let (doc, sheet, root) = setup("<body><p>quick</p></body>", "p{color:red}");
    let t = text_node(&doc, "quick");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 5) }]).expect("highlights");
    let look = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks[0];
    assert_eq!(look.background, Some(UA_SELECTION_BACKGROUND));
    assert_eq!(look.color, None);
}

/// Highlight inheritance (§3.3): the child takes `color` from the parent's `::selection`.
#[test]
fn child_inherits_the_parent_highlight() {
    let (doc, sheet, root) = setup(
        "<body><div><span>inner</span></div></body>",
        "div::selection{color:green} span{color:red}",
    );
    let t = text_node(&doc, "inner");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 5) }]).expect("highlights");
    let look = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks[0];
    assert_eq!(look.color, Some(Color { r: 0, g: 128, b: 0, a: 255 }));
    assert_eq!(look.background, None, "the background does not inherit");
}

/// `selectNodeContents(div)`: both endpoints are on the element, between its children.
#[test]
fn element_container_range_covers_the_text() {
    let (doc, sheet, root) = setup("<body><div>Selected Text</div></body>", "div::selection{color:green}");
    let d = element(&doc, "div");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(d, 0), end: pos(d, 1) }]).expect("highlights");
    let pieces = hl.pieces(first_frag(&root).unwrap()).unwrap();
    assert!(pieces.iter().all(|p| p.looks.len() == 1), "{pieces:?}");
}

/// A range that ends on an element boundary before a node leaves that node out.
#[test]
fn range_end_before_a_node_excludes_it() {
    let (doc, sheet, root) = setup("<body><div>one</div><div>two</div></body>", "");
    let body = element(&doc, "body");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(body, 0), end: pos(body, 1) }]).expect("highlights");
    let one = first_frag(&root).unwrap();
    assert!(hl.pieces(one).is_some());
    let two = text_node(&doc, "two");
    assert!(hl.frags.keys().all(|(n, _)| *n != two), "second div is not selected");
}

/// A range spanning two text nodes covers the tail of the first and the head of the second.
#[test]
fn multi_node_range_covers_both_ends() {
    let (doc, sheet, root) = setup("<body><p>one<b>two</b></p></body>", "");
    let a = text_node(&doc, "one");
    let b = text_node(&doc, "two");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(a, 1), end: pos(b, 2) }]).expect("highlights");
    let sum: usize = hl
        .frags
        .values()
        .flatten()
        .filter(|p| !p.looks.is_empty())
        .map(|p| p.bytes.len())
        .sum();
    assert_eq!(sum, 2 + 2, "\"ne\" and \"tw\"");
}

/// A collapsed range paints nothing and costs nothing.
#[test]
fn collapsed_range_resolves_to_none() {
    let (doc, sheet, root) = setup("<body><p>quick</p></body>", "");
    let t = text_node(&doc, "quick");
    assert!(resolve(&doc, &sheet, &root, &[Range::collapsed(pos(t, 2))]).is_none());
}

/// `user-select: none` text is never highlighted by `::selection`.
#[test]
fn user_select_none_is_skipped() {
    let (doc, sheet, root) = setup("<body><p>quick</p></body>", "p{user-select:none}");
    let t = text_node(&doc, "quick");
    assert!(resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 5) }]).is_none());
}

/// A later layer sits above an earlier one on the same stretch.
#[test]
fn layers_stack_in_order() {
    let (doc, sheet, root) = setup(
        "<body><p>quick</p></body>",
        "p::selection{background:#111111} p::highlight(x){background:#222222}",
    );
    let t = text_node(&doc, "quick");
    let r = [Range { start: pos(t, 0), end: pos(t, 5) }];
    let env = HighlightEnv { doc: &doc, sheet: &sheet, viewport: Size::new(800.0, 600.0), dark_mode: false, measurer: &Fixed8 };
    let layers = [
        HighlightLayer { pseudo: "highlight(x)", ranges: &r, ua_background: None },
        selection_layer(&r),
    ];
    let hl = resolve_highlights(&root, &env, &layers).expect("highlights");
    let looks = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks;
    let bgs: Vec<_> = looks.iter().map(|l| l.background.map(|c| c.r)).collect();
    assert_eq!(bgs, vec![Some(0x22), Some(0x11)]);
}

/// `getComputedStyle(el, "::selection")` reads the cascaded values; `::highlight(name)`
/// matches by name, `::spelling-error` and `::grammar-error` are known pseudo-elements.
#[test]
fn computed_highlight_style_per_pseudo() {
    let (doc, sheet, _) = setup(
        "<body><div id=target></div></body>",
        "#target::selection{background-color:green;color:lime}          #target::spelling-error{color:red} #target::grammar-error{color:blue}          #target::highlight(foo){color:yellow} #target::highlight(bar){background-color:blue}",
    );
    let el = element(&doc, "div");
    let vp = Size::new(800.0, 600.0);
    let base = originating_base(Some("rgb(0, 128, 0)"), Some("20px"));
    let get = |pseudo: &str| compute_highlight_style(&doc, &sheet, vp, false, el, pseudo, &base);
    let sel = get("selection").expect("selection");
    assert_eq!(sel.color, Color { r: 0, g: 255, b: 0, a: 255 });
    assert!(sel.background_color.is_some());
    // What `getComputedStyle(el, "::selection")` serialises.
    let map = crate::computed_style_to_map(&sel);
    assert_eq!(map.get("background-color").map(String::as_str), Some("rgb(0, 128, 0)"));
    assert_eq!(map.get("color").map(String::as_str), Some("rgb(0, 255, 0)"));
    assert_eq!(get("spelling-error").expect("spelling").color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(get("grammar-error").expect("grammar").color, Color { r: 0, g: 0, b: 255, a: 255 });
    assert_eq!(get("highlight(foo)").expect("foo").color, Color { r: 255, g: 255, b: 0, a: 255 });
    assert!(get("highlight(bar)").expect("bar").background_color.is_some());
    assert!(get("highlight(baz)").is_none(), "no rule for that name");
    assert!(get("target-text").is_none(), "no ::target-text rule");
}

/// Collapsed whitespace: DOM offsets of a pretty-printed text land on the right glyphs.
#[test]
fn collapsed_whitespace_is_aligned() {
    let (doc, sheet, root) = setup("<body><p>foo\n      bar baz</p></body>", "");
    let t = text_node(&doc, "foo\n      bar baz");
    // "bar" is bytes 10..13 of the DOM text, 4..7 of the laid-out "foo bar baz".
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 10), end: pos(t, 13) }]).expect("highlights");
    let pieces = hl.pieces(first_frag(&root).unwrap()).unwrap();
    let cuts: Vec<_> = pieces.iter().map(|p| (p.bytes.clone(), p.looks.len())).collect();
    assert_eq!(cuts, vec![(0..4, 0), (4..7, 1), (7..11, 0)]);
    assert_eq!(pieces[1].x, 32.0, "after \"foo \" at 8 px a character");
}

/// Preserved whitespace maps one to one.
#[test]
fn preserved_whitespace_is_not_collapsed() {
    let (doc, sheet, root) = setup("<body><p>a  b</p></body>", "p{white-space:pre}");
    let t = text_node(&doc, "a  b");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 3), end: pos(t, 4) }]).expect("highlights");
    let pieces = hl.pieces(first_frag(&root).unwrap()).unwrap();
    assert_eq!(pieces.iter().find(|p| !p.looks.is_empty()).map(|p| p.bytes.clone()), Some(3..4));
}

/// Adjacent text nodes of one fragment: the space that joins them is highlighted with them,
/// and the pieces abut.
#[test]
fn join_space_between_nodes_is_highlighted() {
    let (doc, sheet, root) = setup("<body><span>One </span><span>two </span><span>three</span></body>", "");
    let body = element(&doc, "body");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(body, 0), end: pos(body, 2) }]).expect("highlights");
    let pieces = hl.pieces(first_frag(&root).unwrap()).unwrap();
    let lit: Vec<_> = pieces.iter().filter(|p| !p.looks.is_empty()).collect();
    assert_eq!(lit.iter().map(|p| p.bytes.len()).sum::<usize>(), "One two ".len(), "{pieces:?}");
    for w in lit.windows(2) {
        assert!((w[0].x + w[0].width - w[1].x).abs() < 0.01, "a hole between {:?} and {:?}", w[0], w[1]);
    }
    assert!(pieces.last().unwrap().looks.is_empty(), "\"three\" is not selected");
}

/// A `::selection` rule with no valid declaration is ignored: the UA background stays.
#[test]
fn invalid_declaration_block_keeps_the_ua_default() {
    let (doc, sheet, root) = setup("<body><p>quick</p></body>", "p::selection{foo:bar}");
    let t = text_node(&doc, "quick");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 5) }]).expect("highlights");
    let look = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks[0];
    assert_eq!(look.background, Some(UA_SELECTION_BACKGROUND));
}

/// A highlight that declares no `color` reports the originating element's own.
#[test]
fn undeclared_colour_is_the_originating_one() {
    let (doc, sheet, _) = setup("<body><main id=m>x</main></body>", ":root::selection{background-color:lime}");
    let el = element(&doc, "main");
    let base = originating_base(Some("rgb(0, 128, 0)"), None);
    let s = compute_highlight_style(&doc, &sheet, Size::new(800.0, 600.0), false, el, "selection", &base).expect("selection");
    assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
}

/// `var()` in a highlight reads the originating element's custom properties, not the ones a
/// parent highlight declared (highlight-cascade-008).
#[test]
fn var_resolves_against_the_originating_element() {
    let (doc, sheet, root) = setup(
        "<body><div>text</div></body>",
        ":root{--c:green} :root::selection{--c:cyan} div::selection{background-color:var(--c,red)}",
    );
    let t = text_node(&doc, "text");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(t, 0), end: pos(t, 4) }]).expect("highlights");
    let look = &hl.pieces(first_frag(&root).unwrap()).unwrap()[0].looks[0];
    assert_eq!(look.background, Some(Color { r: 0, g: 128, b: 0, a: 255 }));
}

/// `<body>Outer<div id=host></div></body>` with the shadow text `inner` in `host`'s shadow
/// root; returns the laid-out tree, the shadow root and its text node.
fn shadow_setup(css: &str) -> (Document, Stylesheet, LayoutBox, NodeId, NodeId) {
    let mut doc = lumen_html_parser::parse("<body>Outer<div id=host></div></body>");
    let host = element(&doc, "div");
    let sr = doc.attach_shadow(host, lumen_dom::ShadowRootMode::Open);
    let inner = doc.create_text("inner");
    doc.append_child(sr, inner);
    let sheet = lumen_css_parser::parse(css);
    let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    (doc, sheet, root, sr, inner)
}

fn highlighted_text(b: &LayoutBox, hl: &Highlights, out: &mut Vec<String>) {
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        for frag in lines.iter().flatten() {
            for p in hl.pieces(frag).unwrap_or_default().iter().filter(|p| !p.looks.is_empty()) {
                out.push(frag.text[p.bytes.clone()].to_string());
            }
        }
    }
    b.children.iter().for_each(|c| highlighted_text(c, hl, out));
}

/// A range over `<body>` reaches the text of a shadow root inside it (flat-tree order).
#[test]
fn range_over_body_covers_shadow_text() {
    let (doc, sheet, root, _, _) = shadow_setup("body{margin:0}");
    let body = element(&doc, "body");
    let n = doc.get(body).children.len() as u32;
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(body, 0), end: pos(body, n) }]).expect("highlights");
    let mut got = Vec::new();
    highlighted_text(&root, &hl, &mut got);
    assert_eq!(got, ["Outer", "inner"]);
}

/// Endpoints inside a shadow tree: from the light text into the shadow text.
#[test]
fn range_from_light_text_into_shadow_text() {
    let (doc, sheet, root, _, inner) = shadow_setup("body{margin:0}");
    let outer = text_node(&doc, "Outer");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(outer, 2), end: pos(inner, 3) }]).expect("highlights");
    let mut got = Vec::new();
    highlighted_text(&root, &hl, &mut got);
    assert_eq!(got, ["ter", "inn"]);
}

/// A range ending at the shadow root's own end covers its text.
#[test]
fn range_to_the_end_of_a_shadow_root() {
    let (doc, sheet, root, sr, _) = shadow_setup("body{margin:0}");
    let outer = text_node(&doc, "Outer");
    let hl = resolve(&doc, &sheet, &root, &[Range { start: pos(outer, 0), end: pos(sr, 1) }]).expect("highlights");
    let mut got = Vec::new();
    highlighted_text(&root, &hl, &mut got);
    assert_eq!(got, ["Outer", "inner"]);
}
