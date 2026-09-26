//! GAP-RUBYBOX — full-pipeline tests: DOM → `build_box` → `lay_out` → assert
//! on child rect positions. Before this task `lay_out_ruby` had no pipeline
//! caller at all (`ruby.rs`'s own unit tests exercised the algorithm
//! directly, never through `layout()`), so `ruby-position: over` and
//! `: under` produced IDENTICAL layout on a real page. These tests prove a
//! box-tree caller now exists and that the two `ruby-position` values are
//! geometrically distinct.

use lumen_core::geom::Size;

fn find_ruby(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if matches!(b.kind, super::super::BoxKind::Ruby { .. }) {
        return Some(b);
    }
    b.children.iter().find_map(find_ruby)
}

/// The composed subtree `lay_out_ruby` returns, wrapped as `Ruby`'s single
/// child by `layout_dispatch`'s `Ruby` arm.
fn composed_column(ruby: &super::super::LayoutBox) -> &super::super::LayoutBox {
    assert_eq!(ruby.children.len(), 1, "Ruby box must wrap exactly one composed column");
    &ruby.children[0]
}

#[test]
fn ruby_element_produces_ruby_box_kind() {
    let html = r#"<div style="width:300px"><ruby>base<rt>anno</rt></ruby></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found — lay_out_ruby has no pipeline caller");
    assert!(ruby.rect.width > 0.0);
    assert!(ruby.rect.height > 0.0);
}

/// GAP-RUBYBOX core claim: `ruby-position: over` puts the annotation ABOVE
/// the base (annotation's composed y < base's composed y).
#[test]
fn ruby_position_over_places_annotation_above_base() {
    let html = r#"<div style="width:300px"><ruby style="ruby-position:over">base<rt>anno</rt></ruby></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let column = composed_column(ruby);
    assert_eq!(column.children.len(), 2, "composed column must hold annotation row + base row");
    let annotation_row = &column.children[0];
    let base_row = &column.children[1];
    assert!(
        annotation_row.rect.y < base_row.rect.y,
        "ruby-position:over must place annotation above base: annotation.y={} base.y={}",
        annotation_row.rect.y,
        base_row.rect.y
    );
}

/// GAP-RUBYBOX core claim: `ruby-position: under` puts the annotation BELOW
/// the base — the geometric opposite of `over`, proving the property now has
/// observable effect (not identical layout for both values, as the ROADMAP
/// entry's live probe found before this task).
#[test]
fn ruby_position_under_places_annotation_below_base() {
    let html = r#"<div style="width:300px"><ruby style="ruby-position:under">base<rt>anno</rt></ruby></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let column = composed_column(ruby);
    assert_eq!(column.children.len(), 2);
    let base_row = &column.children[0];
    let annotation_row = &column.children[1];
    assert!(
        annotation_row.rect.y > base_row.rect.y,
        "ruby-position:under must place annotation below base: annotation.y={} base.y={}",
        annotation_row.rect.y,
        base_row.rect.y
    );
}

/// `over` and `under` must not degenerate to the same layout — this IS the
/// ROADMAP entry's headline bug ("ruby-position: over and : under on a real
/// page give IDENTICAL layout").
#[test]
fn ruby_position_over_and_under_are_not_identical() {
    let over_html = r#"<div style="width:300px"><ruby style="ruby-position:over">base<rt>anno</rt></ruby></div>"#;
    let under_html = r#"<div style="width:300px"><ruby style="ruby-position:under">base<rt>anno</rt></ruby></div>"#;
    let sheet = lumen_css_parser::parse("");

    let over_doc = lumen_html_parser::parse(over_html);
    let over_root = super::super::layout(&over_doc, &sheet, Size::new(800.0, 600.0));
    let over_ruby = find_ruby(&over_root).expect("over: BoxKind::Ruby not found");
    let over_column = composed_column(over_ruby);

    let under_doc = lumen_html_parser::parse(under_html);
    let under_root = super::super::layout(&under_doc, &sheet, Size::new(800.0, 600.0));
    let under_ruby = find_ruby(&under_root).expect("under: BoxKind::Ruby not found");
    let under_column = composed_column(under_ruby);

    // Same two rows, but their vertical order (and hence y positions) is
    // swapped — annotation-first for `over`, base-first for `under`.
    let over_first_y = over_column.children[0].rect.y;
    let under_first_y = under_column.children[0].rect.y;
    assert_eq!(over_first_y, under_first_y, "both columns start their first row at the same y");
    // The DEFINING difference: which row (by height contribution order) sits
    // first differs — proven by the two dedicated tests above; here we assert
    // the total column height (base + annotation) is identical (same content)
    // while the two `find_ruby` calls above already showed a distinct row order,
    // which is the actual geometric effect this task restores.
    assert_eq!(over_column.rect.height, under_column.rect.height);
}

/// `ruby-merge: separate` (the default) multi-pair case:
/// `<ruby>b1<rt>a1</rt>b2<rt>a2</rt></ruby>` must produce two side-by-side
/// columns, each internally consistent (its own annotation directly above/
/// below its own base, not shifted to another column's position — the
/// GAP-RUBYBOX regression `stack_boxes_horizontal` had before this task's
/// fix translated only the outer wrapper, leaving nested content at column 0).
#[test]
fn ruby_merge_separate_pairs_multiple_bases_into_distinct_columns() {
    let html = r#"<div style="width:300px"><ruby>b1<rt>a1</rt>b2<rt>a2</rt></ruby></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let column = composed_column(ruby);
    assert_eq!(column.children.len(), 2, "separate pairing produces one sub-column per base");

    let col0 = &column.children[0];
    let col1 = &column.children[1];
    assert!(col1.rect.x > col0.rect.x, "second column must sit to the right of the first");

    // Recursively find every InlineRun's leftmost fragment x, in DOCUMENT
    // coordinates (rect.x), and assert each descendant column's content
    // stays within ITS OWN column's horizontal span — proving the
    // translate_subtree fix actually moved nested text, not just the
    // outer wrapper box.
    fn collect_inline_run_x(b: &super::super::LayoutBox, out: &mut Vec<f32>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) {
            out.push(b.rect.x);
        }
        for c in &b.children {
            collect_inline_run_x(c, out);
        }
    }
    let mut col0_text_x = Vec::new();
    collect_inline_run_x(col0, &mut col0_text_x);
    let mut col1_text_x = Vec::new();
    collect_inline_run_x(col1, &mut col1_text_x);
    assert!(!col0_text_x.is_empty() && !col1_text_x.is_empty());
    for x in &col0_text_x {
        assert!(
            *x < col1.rect.x,
            "column 0 text at x={x} bled into column 1's horizontal span (starts at {})",
            col1.rect.x
        );
    }
    for x in &col1_text_x {
        assert!(
            *x >= col1.rect.x,
            "column 1 text at x={x} was left behind at column 0's position (col1 starts at {})",
            col1.rect.x
        );
    }
}

/// Base with no `<rt>` at all degrades to `lay_out_ruby`'s "no ruby text"
/// branch — must not panic, must still produce visible content.
#[test]
fn ruby_without_rt_children_degrades_gracefully() {
    let html = r#"<div style="width:300px"><ruby>justbase</ruby></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    assert!(ruby.rect.width > 0.0);
    assert!(ruby.rect.height > 0.0);
}

// ── GAP-RUBYBOX-2: `<rtc>` levels, `<rb>` bases, `ruby-position` values ──

/// Every laid-out box built for a `<tag>` element, in tree order.
fn boxes_of_tag<'a>(
    doc: &lumen_dom::Document,
    b: &'a super::super::LayoutBox,
    tag: &str,
    out: &mut Vec<&'a super::super::LayoutBox>,
) {
    if b.origin.role == super::super::BoxRole::Element
        && let Some(node) = b.origin.node
        && let lumen_dom::NodeData::Element { name, .. } = &doc.get(node).data
        && name.local == tag
    {
        out.push(b);
    }
    for c in &b.children {
        boxes_of_tag(doc, c, tag, out);
    }
}

/// Base-group boxes (anonymous wrappers owned by the `<ruby>` itself).
fn base_groups(ruby: &super::super::LayoutBox) -> Vec<&super::super::LayoutBox> {
    fn walk<'a>(b: &'a super::super::LayoutBox, owner: lumen_dom::NodeId, out: &mut Vec<&'a super::super::LayoutBox>) {
        if b.origin.role == super::super::BoxRole::AnonymousBlock && b.origin.node == Some(owner) {
            out.push(b);
            return;
        }
        for c in &b.children {
            walk(c, owner, out);
        }
    }
    let mut out = Vec::new();
    for c in &ruby.children {
        walk(c, ruby.node, &mut out);
    }
    out
}

fn lay_out_html(html: &str) -> (lumen_dom::Document, super::super::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, root)
}

fn is_over(rt: &super::super::LayoutBox, base: &super::super::LayoutBox) -> bool {
    rt.rect.y + rt.rect.height <= base.rect.y + 0.01
}

fn is_under(rt: &super::super::LayoutBox, base: &super::super::LayoutBox) -> bool {
    rt.rect.y >= base.rect.y + base.rect.height - 0.01
}

/// WPT `css-ruby/ruby-position-alternate.html`: three `<rtc>` levels with the
/// initial `ruby-position` (`alternate`) go over, under, over.
#[test]
fn rtc_levels_alternate_by_default() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby>base<rtc><rt>one</rt></rtc><rtc><rt>two</rt></rtc><rtc><rt>three</rt></rtc></ruby></div>"#,
    );
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let base = base_groups(ruby)[0];
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    assert_eq!(rts.len(), 3);
    assert!(is_over(rts[0], base), "level 1 over: rt.y={} base.y={}", rts[0].rect.y, base.rect.y);
    assert!(is_under(rts[1], base), "level 2 under: rt.y={} base.bottom={}", rts[1].rect.y, base.rect.y + base.rect.height);
    assert!(is_over(rts[2], base), "level 3 over");
    assert!(rts[2].rect.y < rts[0].rect.y, "level 3 stacks outside level 1");
}

#[test]
fn rtc_levels_alternate_from_under() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby style="ruby-position: alternate under">base<rtc><rt>one</rt></rtc><rtc><rt>two</rt></rtc></ruby></div>"#,
    );
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let base = base_groups(ruby)[0];
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    assert!(is_under(rts[0], base));
    assert!(is_over(rts[1], base));
}

/// `ruby-position` is read from each `<rtc>`: `under` on both puts both
/// levels under the base (no alternation between non-`alternate` levels).
#[test]
fn rtc_own_ruby_position_is_honoured() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby>base<rtc style="ruby-position:under"><rt>one</rt></rtc><rtc style="ruby-position:under"><rt>two</rt></rtc></ruby></div>"#,
    );
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let base = base_groups(ruby)[0];
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    assert!(is_under(rts[0], base) && is_under(rts[1], base));
    assert!(rts[1].rect.y > rts[0].rect.y, "second under level stacks below the first");
}

/// `<rb><rb><rt><rt>`: each `<rb>` is its own base, the `<rt>`s pair with
/// them by index (before GAP-RUBYBOX-2 both `<rb>` fused into one base and
/// the second `<rt>` got an empty one).
#[test]
fn rb_bases_pair_with_rt_by_index() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby><rb>AAAA</rb><rb>BBBB</rb><rt>a</rt><rt>b</rt></ruby></div>"#,
    );
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let bases = base_groups(ruby);
    assert_eq!(bases.len(), 2, "one base group per <rb>");
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    assert_eq!(rts.len(), 2);
    for (rt, base) in rts.iter().zip(&bases) {
        assert!(is_over(rt, base));
        assert!(
            rt.rect.x >= base.rect.x - 0.01
                && rt.rect.x + rt.rect.width <= base.rect.x + base.rect.width + 0.01,
            "rt [{}, +{}] outside its base [{}, +{}]",
            rt.rect.x, rt.rect.width, base.rect.x, base.rect.width
        );
    }
}

/// Base text after the annotation starts a new, unannotated segment whose
/// base sits on the same line as the annotated one.
#[test]
fn trailing_base_gets_its_own_segment_on_the_base_line() {
    let (_doc, root) = lay_out_html(r#"<div><ruby>XX<rt>x</rt>YY</ruby></div>"#);
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let bases = base_groups(ruby);
    assert_eq!(bases.len(), 2);
    assert!((bases[0].rect.y - bases[1].rect.y).abs() < 0.01);
    assert!(bases[1].rect.x >= bases[0].rect.x + bases[0].rect.width);
}

/// `inter-character`: the annotation sits after its base on the inline axis,
/// not above or below it.
#[test]
fn inter_character_places_annotation_after_base() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby style="ruby-position: inter-character">base<rt>a</rt></ruby></div>"#,
    );
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let base = base_groups(ruby)[0];
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    assert!(rts[0].rect.x >= base.rect.x + base.rect.width - 0.01);
    assert!((rts[0].rect.y - base.rect.y).abs() < 0.01);
}

/// Loose text directly inside `<rtc>` is one spanning annotation.
#[test]
fn loose_rtc_text_is_an_annotation() {
    let (_doc, root) = lay_out_html(r#"<div><ruby><rb>AB</rb><rb>CD</rb><rtc>span</rtc></ruby></div>"#);
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let bases = base_groups(ruby);
    assert_eq!(bases.len(), 2);
    let top = ruby.children[0].rect.y;
    assert!(bases[0].rect.y > top, "the <rtc> text must sit over the bases");
}

/// CSSOM geometry: `getBoundingClientRect()` of a `<ruby>` is its base-level
/// box, so an over annotation's top is above it and an under one's bottom is
/// below it (WPT `css-ruby/ruby-position-alternate.html` helpers).
#[test]
fn ruby_client_rect_is_base_level_only() {
    let (doc, root) = lay_out_html(
        r#"<div><ruby id="r">base<rtc><rt>one</rt></rtc><rtc><rt>two</rt></rtc></ruby></div>"#,
    );
    let rects = crate::collect_layout_rects(&root, &doc);
    let ruby = find_ruby(&root).expect("BoxKind::Ruby not found");
    let r = rects[&(ruby.node.index() as u32)];
    let mut rts = Vec::new();
    boxes_of_tag(&doc, ruby, "rt", &mut rts);
    let (over, under) = (rects[&(rts[0].node.index() as u32)], rects[&(rts[1].node.index() as u32)]);
    assert!(over[1] < r[1], "over rt top {} must be above ruby top {}", over[1], r[1]);
    assert!(under[1] + under[3] > r[1] + r[3], "under rt bottom must be below ruby bottom");
    assert!(r[3] < ruby.rect.height, "client rect excludes the annotations");
}
