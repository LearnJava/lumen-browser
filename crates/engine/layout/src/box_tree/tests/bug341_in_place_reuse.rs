use lumen_core::geom::Size;

// ---------------------------------------------------------------------------
// BUG-341 S40 — in-place layout reuse.
// ---------------------------------------------------------------------------

/// Builds a fresh box tree for `html`/`css` plus everything `lay_out` needs.
/// Returned separately from the tree so callers can lay out repeatedly.
fn s40_fixture(
    html: &str,
    css: &str,
    vp: Size,
) -> (super::super::LayoutBox, super::super::Rect, lumen_core::ext::NullHyphenationProvider) {
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    use lumen_dom::build_flat_tree;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    (tree, super::super::Rect::new(0.0, 0.0, vp.width, vp.height), lumen_core::ext::NullHyphenationProvider)
}

fn s40_collect_rects(b: &super::super::LayoutBox, out: &mut Vec<(lumen_dom::NodeId, super::super::Rect)>) {
    out.push((b.node, b.rect));
    for c in &b.children {
        s40_collect_rects(c, out);
    }
}

/// Lays `html`/`css` out twice — once with in-place reuse on, once with it
/// off — and asserts every box's rect agrees. Returns the reusing pass's
/// stats. Reuse is invisible in output by construction (a pass that reuses
/// nothing produces the same tree, only slower — the S8 lesson), so geometry
/// parity plus a counter is the honest gate, not either one alone.
fn s40_reuse_vs_disabled(html: &str, css: &str, vp: Size) -> super::super::LayoutInPlaceStats {
    let (mut off_tree, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(false);
    super::super::lay_out(&mut off_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp, false);

    let (mut on_tree, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(true);
    let _ = super::super::take_layout_in_place_stats();
    super::super::lay_out(&mut on_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp, false);
    let stats = super::super::take_layout_in_place_stats();

    let mut ra = Vec::new();
    let mut rb = Vec::new();
    s40_collect_rects(&on_tree, &mut ra);
    s40_collect_rects(&off_tree, &mut rb);
    assert_eq!(ra.len(), rb.len(), "box count must match the non-reusing pass");
    for ((na, xa), (nb, xb)) in ra.iter().zip(rb.iter()) {
        assert_eq!(na, nb, "node order must match the non-reusing pass");
        assert!(
            (xa.x - xb.x).abs() < 0.5 && (xa.y - xb.y).abs() < 0.5
                && (xa.width - xb.width).abs() < 0.5 && (xa.height - xb.height).abs() < 0.5,
            "rect mismatch for {na:?}: reuse-on {xa:?} vs reuse-off {xb:?}",
        );
    }
    stats
}

#[test]
fn in_place_reuse_matches_disabled_on_nested_column_flex() {
    let html = r#"<div class="outer"><div class="mid"><div class="inner">
            some reasonably long text content so intrinsic sizing has real work to do
        </div></div></div>"#;
    let css = r#"
            .outer { display: flex; flex-direction: column; width: 400px; }
            .mid { display: flex; flex-direction: column; }
            .inner { display: flex; flex-direction: column; }
        "#;
    let stats = s40_reuse_vs_disabled(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.refused, 0, "no content-visibility:auto in this fixture, nothing to refuse");
}

#[test]
fn in_place_reuse_matches_disabled_on_nested_row_flex_with_wrap() {
    let html = r#"<div class="bar">
            <div class="group"><span class="btn">one</span><span class="btn">two</span></div>
            <div class="group"><span class="btn">three</span><span class="btn">four</span></div>
        </div>"#;
    let css = r#"
            .bar { display: flex; flex-wrap: wrap; width: 300px; gap: 4px; align-items: center; }
            .group { display: flex; gap: 2px; }
            .btn { display: inline-flex; padding: 2px 6px; }
        "#;
    let stats = s40_reuse_vs_disabled(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.refused, 0, "no content-visibility:auto in this fixture, nothing to refuse");
}

#[test]
fn in_place_reuse_matches_disabled_on_grid_and_table() {
    let html = r#"<div class="grid"><div class="cell">cell text</div><div class="cell">more</div></div>
        <table><tr><td>a</td><td>b b b</td></tr><tr><td>c</td><td>d</td></tr></table>"#;
    let css = r#"
            .grid { display: grid; grid-template-columns: 1fr 2fr; width: 400px; }
            td { padding: 3px; }
        "#;
    let stats = s40_reuse_vs_disabled(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.refused, 0, "no content-visibility:auto in this fixture, nothing to refuse");
}

/// The hit path itself, proven not to be dead code. Two verbatim-identical
/// calls on the very same box must produce exactly one hit — and the tree
/// must still equal what a single uncached call produces.
///
/// The explicit [`LayoutPassGuard`] is load-bearing, not scaffolding: the memo
/// is cleared at both ends of an *outermost* pass, so two top-level `lay_out`
/// calls can never see each other's entries. Holding a guard open makes both
/// calls nested, which is the only situation this mechanism ever fires in —
/// the redundant recursive layout *within* one pass that BUG-341 is about.
#[test]
fn in_place_reuse_hits_on_verbatim_repeat_within_one_pass() {
    let vp = Size::new(800.0, 600.0);
    let html = "<div>hello there</div>";
    let css = "div { width: 200px; }";

    let (mut baseline, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(false);
    super::super::lay_out(&mut baseline, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp, false);

    let (mut tree, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(true);
    let _ = super::super::take_layout_in_place_stats();
    {
        let _pass = super::super::LayoutPassGuard::enter();
        super::super::lay_out(&mut tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp, false);
        super::super::lay_out(&mut tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp, false);
    }
    let stats = super::super::take_layout_in_place_stats();
    assert_eq!(stats.hits, 1, "the second identical root call must be served in place");

    let mut ra = Vec::new();
    let mut rb = Vec::new();
    s40_collect_rects(&tree, &mut ra);
    s40_collect_rects(&baseline, &mut rb);
    assert_eq!(ra, rb, "a hit must leave the tree bit-identical to a plain single pass");
}

/// S39's whole finding, as a regression guard: a result computed at one origin
/// is **not** valid at another, because `LayoutBox` rects are absolute. The
/// second call must miss and recompute, and land where a fresh call at that
/// origin lands — never where the first call left the box.
#[test]
fn in_place_reuse_misses_when_the_origin_differs() {
    let vp = Size::new(800.0, 600.0);
    let html = "<div>hello there</div>";
    let css = "div { width: 200px; }";

    let (mut baseline, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(false);
    let baseline_div = baseline.children.last_mut().expect("html > body");
    let baseline_div = baseline_div.children.last_mut().expect("body > div");
    super::super::lay_out(baseline_div, 30.0, 40.0, 200.0, None, None, vp, pcb, &hp, false);
    let baseline_rect = baseline_div.rect;

    let (mut tree, pcb, hp) = s40_fixture(html, css, vp);
    super::super::set_layout_in_place_reuse(true);
    let _ = super::super::take_layout_in_place_stats();
    let div = tree.children.last_mut().expect("html > body");
    let div = div.children.last_mut().expect("body > div");
    {
        let _pass = super::super::LayoutPassGuard::enter();
        super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, pcb, &hp, false);
        super::super::lay_out(div, 30.0, 40.0, 200.0, None, None, vp, pcb, &hp, false);
    }
    let stats = super::super::take_layout_in_place_stats();
    assert_eq!(stats.hits, 0, "a different origin is a different result — must not be reused");
    assert!(
        (div.rect.x - baseline_rect.x).abs() < 0.01 && (div.rect.y - baseline_rect.y).abs() < 0.01,
        "recomputed box must land at the second call's origin: {:?} vs baseline {baseline_rect:?}",
        div.rect,
    );
}

/// The `out_rect` half of the witness. S39's audit found eight production
/// sites that reposition an already-laid-out subtree — two of them inside
/// `lay_out_flex` itself — so an entry whose box has been moved since must not
/// be served. Proven directly rather than inferred from the audit.
#[test]
fn in_place_reuse_misses_after_the_box_has_been_shifted() {
    let vp = Size::new(800.0, 600.0);
    let (mut tree, pcb, hp) = s40_fixture("<div>hello there</div>", "div { width: 200px; }", vp);
    super::super::set_layout_in_place_reuse(true);
    let _ = super::super::take_layout_in_place_stats();
    let div = tree.children.last_mut().expect("html > body");
    let div = div.children.last_mut().expect("body > div");
    {
        let _pass = super::super::LayoutPassGuard::enter();
        super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, pcb, &hp, false);
        let before_shift = div.rect;
        // Exactly what `lay_out_flex`'s cross-axis alignment does to an item
        // after its final placement pass.
        super::super::shift_tree(div, 0.0, 17.0);
        assert_ne!(div.rect, before_shift, "the shift must actually move the box");
        super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, pcb, &hp, false);
        assert_eq!(
            div.rect, before_shift,
            "the recomputed box must be back where this call's own origin puts it, \
             not left where the shift moved it",
        );
    }
    let stats = super::super::take_layout_in_place_stats();
    assert_eq!(stats.hits, 0, "a shifted box no longer holds the recorded result");
}

/// The `style` half of the witness. `Arc::make_mut` writers still exist
/// (`apply_font_size_adjust_to_style`, `apply_container_rules`,
/// `lay_out_table_row`'s width dance — S39's audit item 4), and a diverged
/// style means the recorded result is not what this call would produce.
#[test]
fn in_place_reuse_misses_when_the_style_arc_diverges() {
    use std::sync::Arc;
    let vp = Size::new(800.0, 600.0);
    let (mut tree, pcb, hp) = s40_fixture("<div>hello there</div>", "div { width: 200px; }", vp);
    super::super::set_layout_in_place_reuse(true);
    let _ = super::super::take_layout_in_place_stats();
    let div = tree.children.last_mut().expect("html > body");
    let div = div.children.last_mut().expect("body > div");
    {
        let _pass = super::super::LayoutPassGuard::enter();
        super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, pcb, &hp, false);
        // Same shape as the surviving in-place style writers: take a unique
        // copy of the style behind the `Arc`, leaving the pointer different
        // from the one recorded, without changing the box's geometry.
        let _ = Arc::make_mut(&mut div.style);
        super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, pcb, &hp, false);
    }
    let stats = super::super::take_layout_in_place_stats();
    assert_eq!(stats.hits, 0, "a diverged style `Arc` must invalidate the recorded result");
}

/// `content-visibility: auto` resolves its skip from the scroll offset and a
/// cross-frame ratchet — state that is in neither the key nor the style — so
/// such a subtree must be refused a recording, exactly as S32 established for
/// the cache.
#[test]
fn in_place_reuse_refuses_a_content_visibility_auto_subtree() {
    let html = r#"<div class="cv"><div class="inner">text content here</div></div>"#;
    let css = r#"
            .cv { content-visibility: auto; contain-intrinsic-height: 40px; }
            .inner { width: 100px; }
        "#;
    let stats = s40_reuse_vs_disabled(html, css, Size::new(800.0, 600.0));
    assert!(
        stats.refused > 0,
        "a content-visibility:auto subtree must be refused a recording, got {stats:?}",
    );
}
