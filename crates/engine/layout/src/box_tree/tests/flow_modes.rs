use lumen_core::geom::Size;

// ── display: flow-root (BFC) ──────────────────────────────────────────────

#[test]
fn flow_root_produces_flow_root_kind() {
    let html = r#"<div id="bfc"></div>"#;
    let css = "#bfc { display: flow-root; width: 200px; height: 50px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_flow_root(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::FlowRoot) {
            return Some(b);
        }
        for child in &b.children {
            if let Some(found) = find_flow_root(child) {
                return Some(found);
            }
        }
        None
    }
    let bfc = find_flow_root(&root).expect("FlowRoot box not found");
    assert_eq!(bfc.rect.width, 200.0);
    assert_eq!(bfc.rect.height, 50.0);
}

#[test]
fn flow_root_lays_out_children_like_block() {
    // A flow-root containing two block children should stack them vertically.
    let html = r#"<div class="bfc"><div class="a"></div><div class="b"></div></div>"#;
    let css = ".bfc { display: flow-root; width: 200px; } .a { height: 30px; } .b { height: 20px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_flow_root(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::FlowRoot) { return Some(b); }
        for c in &b.children { if let Some(f) = find_flow_root(c) { return Some(f); } }
        None
    }
    let bfc = find_flow_root(&root).expect("FlowRoot box not found");
    // Height auto → sum of children (30 + 20 = 50).
    assert_eq!(bfc.rect.height, 50.0, "flow-root auto height wrong: {}", bfc.rect.height);
    // Children stacked vertically.
    let blocks: Vec<_> = bfc.children.iter()
        .filter(|c| matches!(c.kind, super::super::BoxKind::Block))
        .collect();
    assert_eq!(blocks.len(), 2);
    assert!(blocks[1].rect.y > blocks[0].rect.y, "children not stacked vertically");
}

// ── display: contents (box elimination) ──────────────────────────────────

#[test]
fn contents_box_is_eliminated_from_layout_tree() {
    // The display:contents wrapper should not appear as a box; its child
    // block should be a direct child of the outer div.
    let html = r#"<div id="outer"><div id="wrap"><div id="inner"></div></div></div>"#;
    let css = "#outer { width: 400px; } #wrap { display: contents; } #inner { height: 40px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_by_id<'a>(b: &'a super::super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::super::LayoutBox> {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
        {
            return Some(b);
        }
        for child in &b.children { if let Some(f) = find_by_id(child, doc, id) { return Some(f); } }
        None
    }
    // display:contents wrapper must not appear as a Contents box in the tree.
    fn find_contents(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::Contents) { return true; }
        b.children.iter().any(find_contents)
    }
    assert!(!find_contents(&root), "Contents box must be flattened out of layout tree");
    // Inner block must exist with correct height.
    let inner = find_by_id(&root, &doc, "inner").expect("inner div not found");
    assert_eq!(inner.rect.height, 40.0, "inner height wrong: {}", inner.rect.height);
}

#[test]
fn nested_contents_flattened() {
    // Two nested display:contents wrappers — both should be eliminated.
    let html = r#"<div id="root"><div id="a"><div id="b"><div id="leaf"></div></div></div></div>"#;
    let css = "#a, #b { display: contents; } #leaf { height: 25px; width: 100px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_contents(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::Contents) { return true; }
        b.children.iter().any(find_contents)
    }
    assert!(!find_contents(&root), "nested Contents boxes must be fully flattened");
}

#[test]
fn contents_in_flex_container_no_panic() {
    // BUG-058: display:contents child inside a flex container caused a panic
    // because flatten_contents was only called in the non-item-container path.
    let html = r#"<div id="flex"><div id="wrap"><div id="item"></div></div></div>"#;
    let css = "#flex { display: flex; width: 400px; } #wrap { display: contents; } #item { width: 100px; height: 50px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    // Must not panic.
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_contents(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::Contents) { return true; }
        b.children.iter().any(find_contents)
    }
    assert!(!find_contents(&root), "Contents box must be flattened inside flex container");
}

#[test]
fn contents_in_grid_container_no_panic() {
    // BUG-058: same panic reproducible with display:grid container.
    let html = r#"<div id="grid"><div id="wrap"><div id="item"></div></div></div>"#;
    let css = "#grid { display: grid; width: 400px; } #wrap { display: contents; } #item { width: 100px; height: 50px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    // Must not panic.
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_contents(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::Contents) { return true; }
        b.children.iter().any(find_contents)
    }
    assert!(!find_contents(&root), "Contents box must be flattened inside grid container");
}

// ── CSS 2.1 §10.3.3 — auto horizontal-margin centering ───────────────────

#[test]
fn margin_auto_both_centers_block() {
    // margin: 0 auto on a 200px block inside an 800px viewport → x = 300.
    let html = r#"<div id="box"></div>"#;
    let css = "#box { width: 200px; height: 50px; margin: 0 auto; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "box").expect("box not found");
    // (800 - 200) / 2 = 300
    assert_eq!(b.rect.x, 300.0, "centered x expected 300, got {}", b.rect.x);
    assert_eq!(b.rect.width, 200.0, "width must stay 200px");
}

#[test]
fn margin_auto_left_only_pushes_to_right() {
    // margin-left: auto, margin-right: 0 → element flush-right.
    let html = r#"<div id="box"></div>"#;
    let css = "body{margin:0}#box { width: 200px; height: 50px; margin-left: auto; margin-right: 0; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "box").expect("box not found");
    // available=800, width=200, mr=0 → remaining=600 → ml_computed=600 → x=600
    assert_eq!(b.rect.x, 600.0, "flush-right x expected 600, got {}", b.rect.x);
}

#[test]
fn margin_auto_right_only_no_x_shift() {
    // margin-right: auto, margin-left: 20px → element at x=20.
    let html = r#"<div id="box"></div>"#;
    let css = "body{margin:0}#box { width: 200px; height: 50px; margin-left: 20px; margin-right: auto; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "box").expect("box not found");
    // margin-left is fixed at 20px → x=20
    assert_eq!(b.rect.x, 20.0, "x with fixed left margin expected 20, got {}", b.rect.x);
}

#[test]
fn margin_auto_no_explicit_width_fills_container() {
    // Without explicit width, auto margins resolve to 0 (width takes remaining).
    let html = r#"<div id="box"></div>"#;
    let css = "body{margin:0}#box { height: 50px; margin: 0 auto; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "box").expect("box not found");
    // No explicit width → margin auto resolves to 0 → element fills 800px, x=0.
    assert_eq!(b.rect.x, 0.0, "x without explicit width must be 0, got {}", b.rect.x);
    assert_eq!(b.rect.width, 800.0, "width without explicit must fill 800px, got {}", b.rect.width);
}

// ── CSS Box Alignment L3 §5.2 — block-level justify-self ──────────────────

fn layout_box_x(css: &str) -> f32 {
    let html = r#"<div id="box"></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "box").expect("box not found").rect.x
}

#[test]
fn justify_self_end_flushes_block_to_inline_end() {
    // width 200 in an 800px CB, justify-self:end → right edge at 800 → x=600.
    let x = layout_box_x("body{margin:0}#box { width: 200px; height: 50px; justify-self: end; }");
    assert_eq!(x, 600.0, "justify-self:end x expected 600, got {x}");
}

#[test]
fn justify_self_center_centers_block() {
    // (800 - 200) / 2 = 300.
    let x = layout_box_x("body{margin:0}#box { width: 200px; height: 50px; justify-self: center; }");
    assert_eq!(x, 300.0, "justify-self:center x expected 300, got {x}");
}

#[test]
fn justify_self_start_leaves_box_at_inline_start() {
    // start = current behaviour, no shift → x=0.
    let x = layout_box_x("body{margin:0}#box { width: 200px; height: 50px; justify-self: start; }");
    assert_eq!(x, 0.0, "justify-self:start x expected 0, got {x}");
}

#[test]
fn justify_self_default_no_shift() {
    // Without justify-self (auto) the block stays flush-start → x=0.
    let x = layout_box_x("body{margin:0}#box { width: 200px; height: 50px; }");
    assert_eq!(x, 0.0, "default justify-self x expected 0, got {x}");
}

#[test]
fn justify_self_ignored_without_definite_width() {
    // No width → block stretches to fill; justify-self has no free space → x=0.
    let x = layout_box_x("body{margin:0}#box { height: 50px; justify-self: end; }");
    assert_eq!(x, 0.0, "auto-width justify-self:end must not shift, got {x}");
}

#[test]
fn justify_self_end_flushes_right_edge_past_fixed_left_margin() {
    // margin-left:40 + width:200, justify-self:end → right edge still at 800.
    let x = layout_box_x(
        "body{margin:0}#box { width: 200px; height: 50px; margin-left: 40px; justify-self: end; }",
    );
    assert_eq!(x, 600.0, "justify-self:end with left margin x expected 600, got {x}");
}

// ── CSS Box Alignment L3 §6.3 — block container `justify-items` default ───

/// Lays out `<div id="wrap" style="{wrap_css}"><div id="box" style="{box_css}">`
/// in an 800×600 viewport (body margin reset) and returns the inner box's x.
/// The wrap has no width, so it fills the 800px content area (content_x = 0),
/// making the child's absolute x directly comparable to the shift.
fn nested_box_x(wrap_css: &str, box_css: &str) -> f32 {
    let html = r#"<div id="wrap"><div id="box"></div></div>"#;
    let css = format!(
        "body{{margin:0}} #wrap {{ {wrap_css} }} #box {{ {box_css} }}"
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "box").expect("box not found").rect.x
}

#[test]
fn justify_items_center_centers_block_child() {
    // Parent justify-items:center + child with definite width and default
    // (auto) justify-self → child centres: (800 - 200) / 2 = 300.
    let x = nested_box_x("justify-items: center;", "width: 200px; height: 50px;");
    assert_eq!(x, 300.0, "justify-items:center child x expected 300, got {x}");
}

#[test]
fn justify_items_end_flushes_block_child_to_inline_end() {
    // Parent justify-items:end → child right edge at 800 → x = 600.
    let x = nested_box_x("justify-items: end;", "width: 200px; height: 50px;");
    assert_eq!(x, 600.0, "justify-items:end child x expected 600, got {x}");
}

#[test]
fn justify_self_overrides_parent_justify_items() {
    // Child justify-self:start is a specified (non-auto) value → it wins over
    // the parent's justify-items:center, leaving the child flush-start (x=0).
    let x = nested_box_x(
        "justify-items: center;",
        "width: 200px; height: 50px; justify-self: start;",
    );
    assert_eq!(x, 0.0, "explicit justify-self:start must override parent, got {x}");
}

#[test]
fn justify_items_start_leaves_block_child_at_inline_start() {
    // Parent justify-items:start (explicit) → no shift, x=0.
    let x = nested_box_x("justify-items: start;", "width: 200px; height: 50px;");
    assert_eq!(x, 0.0, "justify-items:start child x expected 0, got {x}");
}

#[test]
fn justify_items_ignored_without_definite_child_width() {
    // Auto-width child fills the container → no free inline space for the
    // parent's justify-items:end to distribute, so x stays 0.
    let x = nested_box_x("justify-items: end;", "height: 50px;");
    assert_eq!(x, 0.0, "auto-width child must not shift, got {x}");
}

#[test]
fn margin_auto_position_sticky_centers() {
    // position:sticky element with margin: 20px auto 0 in 1022px container.
    // Static view: sticky behaves like normal flow → centering applies.
    let html = r#"<div id="wrap"><div id="sticky"></div></div>"#;
    let css = "body{margin:0} #wrap { width: 1022px; position: relative; } \
               #sticky { position: sticky; top: 10px; width: 600px; height: 60px; margin: 20px auto 0; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let s = super::find_by_id_all(&root, &doc, "sticky").expect("sticky not found");
    // (1022 - 600) / 2 = 211 → x = wrap.content_x + 211
    assert_eq!(s.rect.width, 600.0, "width must be 600, got {}", s.rect.width);
    let centered_x = s.rect.x;
    // Should be (1022-600)/2 = 211 relative to wrap's content_x (0).
    assert!((centered_x - 211.0).abs() < 1.0, "centered x expected ~211, got {centered_x}");
    assert_eq!(s.rect.y, 20.0, "top margin 20px must be respected, got {}", s.rect.y);
}

#[test]
fn abs_pos_inset_resolves_width_and_height() {
    // CSS Position L3 §6: position:absolute with inset:0 (top/right/bottom/left
    // all 0) and no explicit width/height must fill the relatively-positioned
    // containing block on both axes. Regression for BUG-051 — height-from-insets
    // was missing, so the box collapsed to height 0.
    let html = r#"<div id="cb"><div id="bg"></div></div>"#;
    let css = "#cb { position: relative; width: 660px; height: 120px; } \
               #bg { position: absolute; top: 0; right: 0; bottom: 0; left: 0; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let bg = super::find_by_id_all(&root, &doc, "bg").expect("bg not found");
    assert_eq!(bg.rect.width, 660.0, "inset:0 width must fill cb, got {}", bg.rect.width);
    assert_eq!(bg.rect.height, 120.0, "inset:0 height must fill cb, got {}", bg.rect.height);
}

#[test]
fn abs_pos_explicit_height_overrides_insets() {
    // An explicit height wins over top+bottom insets (height is not auto), so the
    // §6 gap-fill rule does not apply — guards the `cs.height.is_none()` guard.
    let html = r#"<div id="cb"><div id="bg"></div></div>"#;
    let css = "#cb { position: relative; width: 660px; height: 120px; } \
               #bg { position: absolute; top: 0; bottom: 0; left: 0; height: 40px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let bg = super::find_by_id_all(&root, &doc, "bg").expect("bg not found");
    assert_eq!(bg.rect.height, 40.0, "explicit height must win, got {}", bg.rect.height);
}

/// Lays out `#card` (abs-positioned, styled by `card_css`) inside a 400×200
/// relatively-positioned containing block holding one `inner_w`-wide child,
/// and returns the card's `(x, width)`. The child carries the whole
/// max-content contribution, so the result does not depend on a text
/// measurer being installed.
fn abs_card_x_w(card_css: &str, inner_w: &str) -> (f32, f32) {
    let html = r#"<div id="cb"><div id="card"><div id="inner"></div></div></div>"#;
    let css = format!(
        "body {{ margin: 0 }} #cb {{ position: relative; width: 400px; height: 200px; }} \
         #card {{ position: absolute; {card_css} }} \
         #inner {{ width: {inner_w}; height: 20px; }}"
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let card = super::find_by_id_all(&root, &doc, "card").expect("card not found");
    (card.rect.x, card.rect.width)
}

#[test]
fn bug745_abs_auto_width_shrinks_to_fit() {
    // CSS 2.1 §10.3.7: `width: auto` + at least one `auto` inset → shrink-to-fit,
    // not the containing block's width. With `right` given, the stretched width
    // also pushed the box off the left edge, since x counts back from the cb's
    // right edge by the used width.
    let (x, w) = abs_card_x_w("right: 16px; top: 8px;", "60px");
    assert_eq!(w, 60.0, "shrink-to-fit width must be 60, got {w}");
    assert_eq!(x, 324.0, "x must be 400 - 16 - 60 = 324, got {x}");

    let (x, w) = abs_card_x_w("left: 16px; top: 8px;", "60px");
    assert_eq!(w, 60.0, "shrink-to-fit width must be 60, got {w}");
    assert_eq!(x, 16.0, "x must be the left inset, got {x}");

    // No insets at all: still shrink-to-fit (both insets are `auto`), the box
    // just stays at its static position.
    let (_, w) = abs_card_x_w("", "60px");
    assert_eq!(w, 60.0, "static-position abs box must shrink to fit, got {w}");
}

#[test]
fn bug745_shrink_to_fit_capped_by_available_space() {
    // shrink-to-fit = min(max(min-content, available), max-content). Three 200px
    // inline-blocks give max-content 600 (one line) and min-content 200 (one per
    // line), so the used width is the free space 400 − 16 = 384 — neither the
    // 600px max-content nor the full 400px containing block.
    let html = r#"<div id="cb"><div id="card"><span class="i"></span><span class="i"></span><span class="i"></span></div></div>"#;
    let css = "body { margin: 0 } #cb { position: relative; width: 400px; height: 200px; } \
               #card { position: absolute; right: 16px; top: 8px; } \
               .i { display: inline-block; width: 200px; height: 20px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let card = super::find_by_id_all(&root, &doc, "card").expect("card not found");
    assert_eq!(card.rect.width, 384.0, "width must clamp to the free space 384, got {}", card.rect.width);
    assert_eq!(card.rect.x, 0.0, "x must be 400 - 16 - 384 = 0, got {}", card.rect.x);
}

#[test]
fn bug745_shrink_to_fit_never_below_min_content() {
    // The `max(min-content, available)` half of the formula: an unbreakable 1000px
    // child overflows the 384px free space rather than being squeezed into it.
    let (_, w) = abs_card_x_w("right: 16px; top: 8px;", "1000px");
    assert_eq!(w, 1000.0, "min-content must not be squeezed, got {w}");
}

#[test]
fn bug745_explicit_width_and_both_insets_unaffected() {
    // Both "anchored" branches keep their pre-BUG-745 behaviour: an explicit
    // width wins over shrink-to-fit, and both insets given resolve the width
    // from the gap between them.
    let (x, w) = abs_card_x_w("right: 16px; top: 8px; width: 200px;", "60px");
    assert_eq!(w, 200.0, "explicit width must win, got {w}");
    assert_eq!(x, 184.0, "x must be 400 - 16 - 200 = 184, got {x}");

    let (x, w) = abs_card_x_w("left: 10px; right: 10px; top: 8px;", "60px");
    assert_eq!(w, 380.0, "both insets must fill the gap, got {w}");
    assert_eq!(x, 10.0, "x must be the left inset, got {x}");
}

// ── auto-поля флекс-элемента (CSS Flexbox §8.1) ────────────────────────

/// Разложить один флекс-элемент со стилем `item_css` в контейнере со
/// стилем `container_css` (800×600) и вернуть его прямоугольник.
fn flex_item_rect(container_css: &str, item_css: &str) -> lumen_core::geom::Rect {
    let html = r#"<div id="c"><div id="i"></div></div>"#;
    let css = format!(
        "body{{margin:0}}#c {{ display: flex; width: 400px; height: 200px; {container_css} }}\
         #i {{ width: 100px; height: 50px; {item_css} }}"
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "i").expect("item not found").rect
}

#[test]
fn flex_item_auto_main_margins_center() {
    // Оба auto-поля главной оси делят свободное место пополам: элемент по
    // центру строки. Живой случай — карточка формы входа `tbank.ru/login/`,
    // которая стояла слева, пока auto резолвился в ноль.
    let r = flex_item_rect("", "margin-left: auto; margin-right: auto;");
    assert_eq!(r.x, 150.0, "элемент должен встать по центру, x={}", r.x);
}

#[test]
fn flex_item_auto_main_margin_beats_justify_content() {
    // Auto-поля съедают свободное место ДО `justify-content`, поэтому
    // `space-between` с одним элементом ничего не меняет.
    let r = flex_item_rect(
        "justify-content: space-between;",
        "margin-left: auto; margin-right: auto;",
    );
    assert_eq!(r.x, 150.0, "justify-content не должен перебивать auto, x={}", r.x);
}

#[test]
fn flex_item_single_auto_main_margin_pushes_to_end() {
    // Одно auto слева — элемент прижат к концу строки.
    let r = flex_item_rect("", "margin-left: auto;");
    assert_eq!(r.x, 300.0, "элемент должен уехать вправо, x={}", r.x);
}

#[test]
fn flex_item_auto_cross_margins_center() {
    // Поперечная ось: два auto центрируют и отменяют растяжение.
    let r = flex_item_rect("", "margin-top: auto; margin-bottom: auto;");
    assert_eq!(r.y, 75.0, "элемент должен встать по центру по вертикали, y={}", r.y);
    assert_eq!(r.height, 50.0, "auto-поля отменяют stretch, h={}", r.height);
}

#[test]
fn flex_item_single_auto_cross_margin_pushes_to_end() {
    let r = flex_item_rect("", "margin-top: auto;");
    assert_eq!(r.y, 150.0, "элемент должен уехать вниз, y={}", r.y);
}

#[test]
fn flex_item_grow_stops_at_max_width() {
    // §9.7 шаг 4: растущий элемент замирает на своём `max-width`, а не
    // забирает всю строку. Без этого раскладка «выдавала» ему 400px,
    // свободного места не оставалось, а нарисован он всё равно был на
    // 150px — и любое выравнивание строки становилось бессмысленным.
    let r = flex_item_rect("", "flex-grow: 1; max-width: 150px;");
    assert_eq!(r.width, 150.0, "рост обязан упереться в max-width, w={}", r.width);
}

#[test]
fn flex_item_grow_with_max_width_leaves_room_for_auto_margin() {
    // Связка обоих правил — ровно случай карточки формы входа: элемент
    // растёт, упирается в `max-width`, остаток строки достаётся auto-полям.
    let r = flex_item_rect("", "flex-grow: 1; max-width: 200px; margin-left: auto; margin-right: auto;");
    assert_eq!(r.width, 200.0, "w={}", r.width);
    assert_eq!(r.x, 100.0, "остаток строки делится поровну, x={}", r.x);
}

#[test]
fn flex_item_grow_without_max_still_fills_line() {
    // Плечо сравнения: без потолка рост работает как раньше.
    let r = flex_item_rect("", "flex-grow: 1;");
    assert_eq!(r.width, 400.0, "без max-width элемент занимает строку, w={}", r.width);
}

#[test]
fn flex_item_without_auto_margins_unchanged() {
    // Плечо сравнения: без auto-полей ничего не поменялось.
    let r = flex_item_rect("justify-content: center;", "");
    assert_eq!(r.x, 150.0, "justify-content: center по-прежнему работает, x={}", r.x);
    let r = flex_item_rect("", "");
    assert_eq!(r.x, 0.0, "по умолчанию элемент у начала строки, x={}", r.x);
}

#[test]
fn margin_auto_float_not_centered() {
    // float:left with margin: 0 auto must NOT be centered — floats ignore auto margins.
    let html = r#"<div id="box"></div>"#;
    let css = "body{margin:0}#box { float: left; width: 100px; height: 50px; margin: 0 auto; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let b = super::find_by_id_all(&root, &doc, "box").expect("box not found");
    // Float placed at left edge (auto = 0).
    assert_eq!(b.rect.x, 0.0, "float with auto margins must be at x=0, got {}", b.rect.x);
}

// ── loading="lazy" image deferral (HTML LS §2.6.6.9) ────────────────────

#[test]
fn loading_lazy_marks_image_as_lazy() {
    let doc = lumen_html_parser::parse(r#"<img src="a.png" loading="lazy">"#);
    let viewport = Size::new(800.0, 600.0);
    let reqs = super::super::collect_image_requests(&doc, viewport);
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].is_lazy, "loading=lazy must set is_lazy=true");
    assert_eq!(reqs[0].url, "a.png");
}

#[test]
fn loading_eager_not_lazy() {
    let doc = lumen_html_parser::parse(r#"<img src="b.png" loading="eager">"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert!(!reqs[0].is_lazy, "loading=eager must not set is_lazy");
}

#[test]
fn loading_absent_not_lazy() {
    let doc = lumen_html_parser::parse(r#"<img src="c.png">"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert!(!reqs[0].is_lazy, "absent loading attr must not set is_lazy");
}

#[test]
fn loading_lazy_case_insensitive() {
    let doc = lumen_html_parser::parse(r#"<img src="d.png" loading="LAZY">"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].is_lazy, "loading=LAZY (uppercase) must set is_lazy=true");
}

#[test]
fn loading_lazy_mixed_with_eager() {
    let html = r#"<img src="e.png"><img src="f.png" loading="lazy"><img src="g.png">"#;
    let doc = lumen_html_parser::parse(html);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 3);
    assert!(!reqs[0].is_lazy, "first img (no attr) must not be lazy");
    assert!(reqs[1].is_lazy, "second img (loading=lazy) must be lazy");
    assert!(!reqs[2].is_lazy, "third img (no attr) must not be lazy");
}

#[test]
fn fetchpriority_variants() {
    let html = r#"<img src="a.png" fetchpriority="high"><img src="b.png" fetchpriority="LOW"><img src="c.png" fetchpriority="auto"><img src="d.png">"#;
    let doc = lumen_html_parser::parse(html);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 4);
    assert_eq!(reqs[0].fetch_priority, Some("high".to_string()));
    assert_eq!(reqs[1].fetch_priority, Some("low".to_string()), "LOW must normalize to low");
    assert_eq!(reqs[2].fetch_priority, None, "auto must map to None");
    assert_eq!(reqs[3].fetch_priority, None, "absent attr must map to None");
}

// ── BUG-848: image requests for non-`<img>` elements ──────────────────────

#[test]
fn video_poster_produces_an_image_request() {
    let doc = lumen_html_parser::parse(
        r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#,
    );
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "thumb.jpg");
}

#[test]
fn video_without_poster_produces_no_request() {
    let doc = lumen_html_parser::parse(r#"<video src="clip.mp4"></video>"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert!(reqs.is_empty(), "no poster attribute must not request anything");
}

#[test]
fn input_type_image_src_produces_an_image_request() {
    let doc = lumen_html_parser::parse(r#"<input type="image" src="btn.png">"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "btn.png");
}

#[test]
fn input_type_text_with_src_produces_no_request() {
    // `src` on a non-image input carries no meaning — must not be
    // misread as an image URL.
    let doc = lumen_html_parser::parse(r#"<input type="text" src="btn.png">"#);
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert!(reqs.is_empty(), "type=text must not produce an image request");
}

#[test]
fn svg_image_href_produces_an_image_request() {
    let doc = lumen_html_parser::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="pic.svg" width="8" height="8"/></svg>"#,
    );
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "pic.svg");
}

#[test]
fn svg_image_xlink_href_produces_an_image_request() {
    // Legacy SVG 1.1 form, same fallback `<use>` already relies on.
    let doc = lumen_html_parser::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image xlink:href="pic.svg" width="8" height="8"/></svg>"#,
    );
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "pic.svg");
}

#[test]
fn svg_image_without_href_produces_no_request() {
    let doc = lumen_html_parser::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image width="8" height="8"/></svg>"#,
    );
    let reqs = super::super::collect_image_requests(&doc, Size::new(800.0, 600.0));
    assert!(reqs.is_empty(), "no href must not produce a request");
}

