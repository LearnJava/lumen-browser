use lumen_core::geom::Size;

// ── Flex align-content (multi-line flex wrap) ───────────────────────────
//
// Setup: 200px wide × 300px tall flex container with 3 × 90px wide items.
// Lines: [a, b] on line 1, [c] on line 2. Each line cross-size = 50px.
// used_cross = 100px; free_cross = 200px.

#[test]
fn flex_align_content_flex_start() {
    // flex-start: lines packed at cross-start → line1 y=0, line2 y=50.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:flex-start} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
    assert_eq!(c.rect.y, 50.0, "c.y {}", c.rect.y);
}

#[test]
fn flex_align_content_flex_end() {
    // flex-end: offset=200 → line1 y=200, line2 y=250.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:flex-end} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 200.0, "a.y {}", a.rect.y);
    assert_eq!(c.rect.y, 250.0, "c.y {}", c.rect.y);
}

#[test]
fn flex_row_item_margin_left_applied_once() {
    // BUG-294: a row flex item's `margin-left` must move it 1× the margin past
    // the preceding item's border-box edge, not 2×. item-a occupies [0,60);
    // item-b (margin-left:10) starts at 60+10=70. The main-axis position is not
    // rewritten by any later cross-alignment pass, so a double-add would surface
    // directly as rect.x=80.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:300px;height:100px} #a{width:60px;height:40px} #b{width:60px;height:40px;margin-left:10px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.x, 0.0, "a.x {}", a.rect.x);
    assert_eq!(b.rect.x, 70.0, "b.x {} (expected 70 = 0 + 60 + 10)", b.rect.x);
}

#[test]
fn flex_row_item_padding_applied_once() {
    // BUG-427: a row flex item's own padding must be counted once. The flex base
    // size handed back to the item's re-layout is already a border-box width, so
    // the re-layout has to be forced to `border-box` (as the column arm is);
    // otherwise a content-box item got padding+border added on top and its rect
    // came out `padding_x + border_x` too wide, while the main-axis cursor still
    // advanced by the correct border-box size — adjacent items then overlapped by
    // exactly that amount (dzen.ru topic tabs).
    // a: 100 content + 2×12 padding + 2×3 border = 130 → b starts at 130.
    let html = r#"<div id="flex"><div id="a"><div class="in"></div></div><div id="b"><div class="in"></div></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:600px;height:100px}\
                   #a,#b{padding:0 12px;border:3px solid #000}\
                   .in{width:100px;height:40px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.width, 130.0, "a.width {} (expected 100+24+6)", a.rect.width);
    assert_eq!(b.rect.x, 130.0, "b.x {} (must abut a's border-box edge)", b.rect.x);
    assert_eq!(b.rect.width, 130.0, "b.width {}", b.rect.width);
}

#[test]
fn min_content_width_of_nowrap_text_is_max_content() {
    // BUG-427: whitespace is a soft-wrap opportunity only where wrapping is
    // allowed. Under `white-space: nowrap` the text cannot break at all, so its
    // min-content width equals its max-content width — reporting the widest
    // single word instead let a row of nowrap flex items shrink far below their
    // text and paint on top of each other.
    /// Fixed 8px per character, so the expected widths are exact.
    struct Fixed8;
    impl crate::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }
    let html = r#"<div id="wrapy">aaaa bbbb cccc</div><div id="nowrapy">aaaa bbbb cccc</div><div id="maxy">aaaa bbbb cccc</div>"#;
    let css = "body{margin:0} div{font-size:16px}\
                   #wrapy{width:min-content}\
                   #nowrapy{width:min-content;white-space:nowrap}\
                   #maxy{width:max-content}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    let wrapy = super::find_by_id_all(&root, &doc, "wrapy").expect("wrapy").rect.width;
    let nowrapy = super::find_by_id_all(&root, &doc, "nowrapy").expect("nowrapy").rect.width;
    let maxy = super::find_by_id_all(&root, &doc, "maxy").expect("maxy").rect.width;
    // 14 characters × 8px = 112 on one line; the widest word is 4 × 8 = 32.
    assert_eq!(maxy, 112.0, "max-content {maxy}");
    assert_eq!(nowrapy, 112.0, "nowrap min-content {nowrapy} must equal max-content");
    assert_eq!(wrapy, 32.0, "wrappable min-content {wrapy} must be the widest word");
}

#[test]
fn flex_column_item_margin_top_applied_once() {
    // BUG-294: a column flex item's `margin-top` must offset it 1× along the
    // main (block) axis. item-a occupies [0,40); item-b (margin-top:10) starts at
    // 40+10=50. Column containers skip the cross-alignment pass, so the main-axis
    // rect.y is exactly what the layout call produced — a double-add would show
    // as rect.y=60.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;width:100px;height:300px} #a{width:60px;height:40px} #b{width:60px;height:40px;margin-top:10px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
    assert_eq!(b.rect.y, 50.0, "b.y {} (expected 50 = 0 + 40 + 10)", b.rect.y);
}

#[test]
fn nested_column_flex_costs_one_layout_per_level() {
    // BUG-802: `flex-basis: auto` (the default) made the Step-1 probe
    // unconditional for column items, and the final placement pass laid the
    // same item out a second time — two full recursive layouts per level,
    // multiplying down the tree. A chain of nested `flex-direction: column`
    // boxes therefore cost ×2 per level (4.91 s at depth 20, ~76 s at 24).
    // Gated on the layout-call counter, not wall-clock: the exponent is what
    // the fix removes, and a counter says so without a timing threshold that
    // a slow machine would trip (`docs/perf-method.md`).
    fn layout_calls_at_depth(n: usize) -> u32 {
        let html = format!("{}{}", r#"<div class="f">"#.repeat(n), "</div>".repeat(n));
        let css = "body{margin:0} div.f{height:200px;display:flex;flex-direction:column}";
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse(css);
        super::super::set_layout_key_census(true);
        let _root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let census = super::super::take_layout_key_census();
        super::super::set_layout_key_census(false);
        census.calls
    }
    let shallow = layout_calls_at_depth(4);
    let deep = layout_calls_at_depth(12);
    // Eight more levels are eight more boxes, so a linear pass adds a small
    // constant per level; the pre-fix code multiplied by 2^8 = 256 instead.
    assert!(
        deep < shallow * 4,
        "nested column flex is superlinear: depth 4 = {shallow} layout calls, depth 12 = {deep}"
    );
}

#[test]
fn nested_column_flex_that_shrinks_costs_one_probe_per_level() {
    // BUG-802, second half: when the container has a definite height and its
    // item overflows it, flex-shrink changes the item's used main size, so
    // the final pass genuinely has to lay it out again and the probe cannot
    // be replayed — the ×2 per level survived there (3.7 s at depth 20,
    // still doubling). The probe-height memo removes it from the other side:
    // an item's probe runs once per (node, width) instead of once under its
    // parent's probe and again under its parent's final pass. Shape copied
    // from `tests/wpt/verify_layout_hangs.py`'s `flex-nesting-20` repro:
    // `height:200px` + a 1px border makes every level overflow by 2px.
    fn layout_calls_at_depth(n: usize) -> u32 {
        let open = r#"<div style="height:200px;display:flex;flex-direction:column;border:1px dotted black">"#;
        let leaf = r#"<div style="width:10px;flex:0 10px;border:solid 1px purple;padding:2px">x</div>"#;
        let html = format!("{}{leaf}{}", open.repeat(n), "</div>".repeat(n));
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse("body{margin:0}");
        super::super::set_layout_key_census(true);
        let _root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let census = super::super::take_layout_key_census();
        super::super::set_layout_key_census(false);
        census.calls
    }
    let shallow = layout_calls_at_depth(4);
    let deep = layout_calls_at_depth(12);
    // Polynomial, not exponential: the memo leaves each level's *final* pass
    // recursing into the level below (the shrink is real work), so eight
    // more levels cost ×5 here — against the ×2^8 = ×256 they cost before.
    assert!(
        deep < shallow * 16,
        "shrinking nested column flex is exponential: depth 4 = {shallow} layout calls, depth 12 = {deep}"
    );
}

#[test]
fn flex_column_item_margin_left_applied_once() {
    // BUG-294: a column flex item's `margin-left` (a cross-axis margin) must
    // offset it 1× along the inline axis. The column arm never re-runs cross
    // alignment, so rect.x is the layout call's output — a double-add would show
    // as rect.x=30.
    let html = r#"<div id="flex"><div id="a"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;width:100px;height:300px} #a{width:60px;height:40px;margin-left:15px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    assert_eq!(a.rect.x, 15.0, "a.x {} (expected 15 = margin-left applied once)", a.rect.x);
}

/// Fixed 8px per character, so the expected intrinsic widths are exact.
struct FixedCharWidth8;
impl crate::TextMeasurer for FixedCharWidth8 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        8.0
    }
}

#[test]
fn flex_column_align_items_center_shrinks_to_fit_and_centers() {
    // BUG-460: a column container's cross axis is width. `align-items: center`
    // on a non-replaced item with no explicit width used to be a no-op — the
    // item filled the whole container width regardless (ordinary block
    // auto-width already fills available space, and the column arm had no
    // shrink-to-fit/alignment logic for anything but the default `stretch`).
    // Live case: `.newtab .nt-restore` in `assets/chrome/chrome.html`.
    let html = r#"<div id="flex"><div id="item">aaaa</div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;align-items:center;width:300px;height:200px} #item{font-size:16px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &FixedCharWidth8);
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.width, 32.0, "shrink-to-fit width (4 chars × 8px), w={}", item.rect.width);
    assert_eq!(item.rect.x, 134.0, "centered: (300 − 32) / 2, x={}", item.rect.x);
}

#[test]
fn flex_column_align_items_flex_end_shrinks_to_fit_and_pushes_to_end() {
    // Same gap as above, `flex-end` arm: item must shrink to content width and
    // sit flush against the cross-end, not stretch full width then align.
    let html = r#"<div id="flex"><div id="item">aaaa</div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;align-items:flex-end;width:300px;height:200px} #item{font-size:16px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &FixedCharWidth8);
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.width, 32.0, "shrink-to-fit width, w={}", item.rect.width);
    assert_eq!(item.rect.x, 268.0, "flush with cross-end: 300 − 32, x={}", item.rect.x);
}

#[test]
fn flex_column_align_self_center_overrides_container_align_items() {
    // `align-self` on the item must win over the container's `align-items`
    // (here `flex-start`, itself a non-stretch value exercising the same
    // shrink-to-fit path) for the column cross axis, mirroring the row arm.
    let html = r#"<div id="flex"><div id="item">aaaa</div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;align-items:flex-start;width:300px;height:200px} #item{font-size:16px;align-self:center}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &FixedCharWidth8);
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.width, 32.0, "shrink-to-fit width, w={}", item.rect.width);
    assert_eq!(item.rect.x, 134.0, "align-self:center wins over container's flex-start, x={}", item.rect.x);
}

#[test]
fn flex_align_content_center() {
    // center: offset=100 → line1 y=100, line2 y=150.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:center} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 100.0, "a.y {}", a.rect.y);
    assert_eq!(c.rect.y, 150.0, "c.y {}", c.rect.y);
}

#[test]
fn flex_align_content_space_between() {
    // space-between (n=2): line1 offset=0, line2 offset=200 → y=0 and y=250.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-between} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
    assert_eq!(c.rect.y, 250.0, "c.y {}", c.rect.y);
}

#[test]
fn flex_align_content_space_around() {
    // space-around (n=2): per=100; line1 offset=50, line2 offset=150 → y=50 and y=200.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-around} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 50.0, "a.y {}", a.rect.y);
    assert_eq!(c.rect.y, 200.0, "c.y {}", c.rect.y);
}

#[test]
fn flex_align_content_space_evenly() {
    // space-evenly (n=2): per=200/3≈66.67; line1 offset=per, line2 offset=2*per.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-evenly} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    let per = 200.0_f32 / 3.0;
    assert!((a.rect.y - per).abs() < 0.5, "a.y expected ≈{per:.2}, got {}", a.rect.y);
    assert!((c.rect.y - (50.0 + 2.0 * per)).abs() < 0.5, "c.y expected ≈{:.2}, got {}", 50.0 + 2.0 * per, c.rect.y);
}

#[test]
fn flex_align_content_default_stretches_lines() {
    // BUG-107: align-content defaults to `auto`, which CSS Box Alignment L3 §5.4
    // says behaves as `stretch` for flex containers. With free cross-space the
    // lines must grow equally and later lines shift toward the cross-end by the
    // cumulative growth of preceding lines (CSS Flexbox §8.3).
    //
    // 200×300 container, 3×90px items → line1 [a,b] (cross 50), line2 [c] (cross 50).
    // used_cross=100, free_cross=200, per=200/2=100. line1 offset 0, line2 offset 100.
    // c was at cross_cursor 50 → ends at 50+100 = 150.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 0.0, "a.y default-stretch line1 {}", a.rect.y);
    assert_eq!(c.rect.y, 150.0, "c.y default-stretch line2 {}", c.rect.y);
}

#[test]
fn flex_align_content_single_line_flex_end() {
    // Single-line flex container (all items fit in one row) with align-content: flex-end.
    // CSS Box Alignment L3: align-content applies even when n_lines == 1.
    // Container 300×200, items 80×50 — all 3 fit in one row (240px < 300px, single line).
    // free_cross = 200 - 50 = 150; flex-end offset = 150 → items at y=150.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:300px;height:200px;align-content:flex-end} #a,#b,#c{width:80px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 150.0, "a.y with single-line flex-end {}", a.rect.y);
    assert_eq!(b.rect.y, 150.0, "b.y with single-line flex-end {}", b.rect.y);
    assert_eq!(c.rect.y, 150.0, "c.y with single-line flex-end {}", c.rect.y);
}

#[test]
fn flex_align_content_single_line_center() {
    // Single-line flex with align-content: center → items centered vertically.
    // Container 300×200, items 80×50 (all fit one row). free_cross=150, offset=75.
    let html = r#"<div id="flex"><div id="a"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:300px;height:200px;align-content:center} #a{width:80px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    assert_eq!(a.rect.y, 75.0, "a.y with single-line center {}", a.rect.y);
}

#[test]
fn flex_text_child_is_wrapped_and_centered() {
    // BUG-194: raw text directly inside a flex item must be wrapped in an
    // anonymous (blockified) flex item (CSS Flexbox §4) and rendered. With
    // align-items:center the item — and its InlineRun — must be centered on the
    // cross axis: the cross-alignment shift moves the whole subtree, not just
    // the item's own rect (previously the InlineRun stayed at the box top).
    fn find_inline_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = find_inline_run(c) {
                return Some(f);
            }
        }
        None
    }
    let html = r#"<div id="box">1</div>"#;
    let css = "body{margin:0} #box{display:flex;align-items:center;justify-content:center;width:50px;height:50px;font-size:12px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let box_b = super::find_by_id_all(&root, &doc, "box").expect("box");
    let run = find_inline_run(box_b).expect("anonymous InlineRun with text was dropped");
    // One line of 12px text (~14.4px line-box) centered in a 50px box → center ≈ 25.
    assert!(run.rect.height > 0.0, "run has a non-zero line box");
    let center = run.rect.y + run.rect.height / 2.0;
    assert!(
        (center - 25.0).abs() < 3.0,
        "text not vertically centered: center={center}, run.y={}, h={}",
        run.rect.y, run.rect.height
    );
}

#[test]
fn flex_align_content_stretch_repositions_lines() {
    // BUG-107: explicit align-content:stretch grows lines AND shifts later lines
    // down by the cumulative growth of preceding lines (previously the growth was
    // computed but items were never repositioned). Same geometry as the default
    // test: a.y=0, c.y=150.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:stretch} #a,#b,#c{width:90px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let c = super::find_by_id_all(&root, &doc, "c").expect("c");
    assert_eq!(a.rect.y, 0.0, "a.y (line1) {}", a.rect.y);
    assert_eq!(c.rect.y, 150.0, "c.y (line2 shifted) {}", c.rect.y);
}

#[test]
fn flex_align_content_shifts_item_subtree() {
    // BUG-165: when align-content offsets a flex line, the item's descendants
    // (already laid out in absolute coordinates) must move in lockstep with the
    // item box. Previously only `item.rect.y` was bumped, leaving nested content
    // behind — most visible when a wrapped flex container with the default
    // (stretch) align-content held flex items that were themselves containers.
    //
    // Single-line flex-end: container 300×200, item #a 80×80 fits one row →
    // free_cross = 120, flex-end offset = 120. #a's block child #inner sits at
    // #a's content origin, so it must end at y=120, not be stranded at y=0.
    let html = r#"<div id="flex"><div id="a"><div id="inner"></div></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:300px;height:200px;align-content:flex-end} #a{width:80px;height:80px} #inner{width:40px;height:40px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let inner = super::find_by_id_all(&root, &doc, "inner").expect("inner");
    assert_eq!(a.rect.y, 120.0, "a.y flex-end {}", a.rect.y);
    assert_eq!(inner.rect.y, 120.0, "inner.y must follow #a (subtree shift) {}", inner.rect.y);
}

#[test]
fn flex_abs_child_does_not_advance_main_axis() {
    // CSS Flexbox L1 §4.1: an absolutely-positioned child of a flex container is
    // not a flex item — it must not consume main-axis space. Regression for the
    // lenta.ru bug where position:fixed/absolute children advanced the flex-column
    // cursor and pushed real content ~700px below the fold.
    // Column flex: in-flow #a (h=90) then abs #x (h=380) then in-flow #b (h=250).
    // #b must sit directly after #a (y=90), not after the abs box (y=470).
    let html = r#"<div id="flex"><div id="a"></div><div id="x"></div><div id="b"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;width:400px} \
                   #a{height:90px} #x{position:absolute;width:380px;height:380px} #b{height:250px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
    assert_eq!(b.rect.y, 90.0, "abs sibling must not advance flow; b.y {}", b.rect.y);
}

#[test]
fn flex_nowrap_align_items_center_uses_container_cross_size() {
    // BUG-141: in a non-wrapping flex container with an explicit height,
    // align-items: center must center items relative to the full container
    // height, not the tallest item height (line_cross).
    // Container 500×400, item 100×100 → y = (400 - 100) / 2 = 150.
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:500px;height:400px;align-items:center} #item{width:100px;height:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.y, 150.0, "align-items:center nowrap should be at (400-100)/2=150, got {}", item.rect.y);
}

#[test]
fn flex_nowrap_align_items_end_uses_container_cross_size() {
    // align-items: flex-end in a non-wrapping container → item at bottom of container.
    // Container 500×400, item 100×100 → y = 400 - 100 = 300.
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:500px;height:400px;align-items:flex-end} #item{width:100px;height:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.y, 300.0, "align-items:flex-end nowrap should be 300, got {}", item.rect.y);
}

#[test]
fn flex_nowrap_align_items_stretch_auto_height_fills_container() {
    // CSS Flexbox §9.5: stretch with height:auto stretches item to container cross size.
    // Container 500×400, item width:100px height:auto → item should fill 400px.
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:500px;height:400px;align-items:stretch} #item{width:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.height, 400.0, "align-items:stretch auto-height item should fill container (400), got {}", item.rect.height);
}

#[test]
fn flex_nowrap_align_items_stretch_explicit_height_not_grown() {
    // CSS Flexbox §9.5: stretch must NOT grow items with explicit cross sizes.
    // Container 500×400, item 100×100 → item stays at 100px (not stretched to 400).
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:500px;height:400px;align-items:stretch} #item{width:100px;height:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.height, 100.0, "explicit height should not be stretched by align-items:stretch, got {}", item.rect.height);
}

#[test]
fn flex_item_height_percentage_resolves_against_container() {
    // BUG-074: height:100% on a row flex item must resolve against the container's
    // definite cross size, not fall back to auto (height=0).
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;height:60px;width:400px} #item{height:100%;width:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.height, 60.0, "height:100% flex item should be container height, got {}", item.rect.height);
}

#[test]
fn flex_item_half_height_percentage_resolves_against_container() {
    // CSS Flexbox §9.8: percentage cross sizes resolve against definite container cross size.
    let html = r#"<div id="flex"><div id="item"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;height:80px;width:400px} #item{height:50%;width:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let item = super::find_by_id_all(&root, &doc, "item").expect("item");
    assert_eq!(item.rect.height, 40.0, "height:50% flex item should be 40px, got {}", item.rect.height);
}

#[test]
fn flex_column_explicit_height_grows_items() {
    // BUG-104: a column flex container with a definite main (block) size must
    // distribute free space to `flex:1` children. Container height 300px, two
    // `flex:1` items → each grows to 150px. Previously column main size was
    // hardcoded to 0, so flex-grow had no free space and items collapsed.
    let html = r#"<div id="col"><div id="a"></div><div id="b"></div></div>"#;
    let css = "body{margin:0} \
                   #col{display:flex;flex-direction:column;height:300px;width:100px} \
                   #a{flex:1} #b{flex:1}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.height, 150.0, "first flex:1 column item should grow to 150, got {}", a.rect.height);
    assert_eq!(b.rect.height, 150.0, "second flex:1 column item should grow to 150, got {}", b.rect.height);
    assert_eq!(b.rect.y, 150.0, "second item starts after first; b.y={}", b.rect.y);
}

#[test]
fn flex_stretched_column_child_grows_its_items() {
    // BUG-104 (TEST-62): a column flex container with NO explicit height that is
    // stretched by a row parent (align-items:stretch) gains a definite main size.
    // Its `flex:1` children must then grow to fill it. This is the `.right-col`
    // scenario from TEST-62 — the right column collapsed to ~0 height before.
    // Row 400px tall, #col stretched to 400, its two flex:1 items → 200 each.
    let html = r#"<div id="row"><div id="col"><div id="a"></div><div id="b"></div></div></div>"#;
    let css = "body{margin:0} \
                   #row{display:flex;height:400px;width:500px} \
                   #col{display:flex;flex-direction:column;flex:1} \
                   #a{flex:1} #b{flex:1}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let col = super::find_by_id_all(&root, &doc, "col").expect("col");
    assert_eq!(col.rect.height, 400.0, "stretched column container should be 400 tall, got {}", col.rect.height);
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.height, 200.0, "first item in stretched column should grow to 200, got {}", a.rect.height);
    assert_eq!(b.rect.height, 200.0, "second item in stretched column should grow to 200, got {}", b.rect.height);
    assert_eq!(b.rect.y, 200.0, "second item starts after first; b.y={}", b.rect.y);
}

#[test]
fn flex_nested_stretch_after_indefinite_pass_fills_row() {
    // BUG-209 (TEST-90): a column-flex item (#col) nested inside a row-flex cell
    // (#cell) that is itself a flex item of an outer column (#outer). The outer
    // column lays #cell out twice: first an indefinite preliminary pass (the row's
    // cross size is unknown), then a real pass with #cell stretched to its grown
    // height. On the preliminary pass the stretch fell back to the line's own
    // height and wrote a px `style.height` back onto #col, clobbering its
    // `height:auto`; the real pass then skipped the genuine stretch and #col
    // collapsed to content height. Here #col must fill the row's full cross size.
    let html = r#"<div id="outer"><div id="cell"><div id="col">x</div></div></div>"#;
    let css = "body{margin:0} \
                   #outer{display:flex;flex-direction:column;height:300px;width:400px} \
                   #cell{flex:1;display:flex} \
                   #col{flex:1;display:flex;flex-direction:column}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let cell = super::find_by_id_all(&root, &doc, "cell").expect("cell");
    assert_eq!(cell.rect.height, 300.0, "row cell should fill the outer column (300), got {}", cell.rect.height);
    let col = super::find_by_id_all(&root, &doc, "col").expect("col");
    assert_eq!(
        col.rect.height, 300.0,
        "nested column item must stretch to the cell's cross size (300), not collapse to content; got {}",
        col.rect.height
    );
}

#[test]
fn flex_probe_pass_does_not_burn_item_height_into_style() {
    // BUG-333: the chrome sidebar's tab rows all rendered with `h=0`.
    // #sidebar is a row-flex item with an explicit width and `flex-basis:auto`,
    // so the outer row runs a Step-1 probe on it with an *indefinite* height.
    // In that probe #tabs (`flex:1` → basis 0, `overflow-y:auto` so no automatic
    // minimum size) resolves to height 0 and is laid out with a definite main
    // size of 0 — which shrinks its own rows to 0 and, before the fix, wrote
    // `height:0px` permanently into their style. The outer row's real pass then
    // had nothing left to recompute from.
    let html = r#"<div id="app"><div id="sidebar"><div id="tabs">
            <div class="row" id="r1"></div><div class="row" id="r2"></div>
            <div class="row" id="r3"></div></div></div></div>"#;
    let css = "body{margin:0} \
                   #app{display:flex;height:300px;width:600px} \
                   #sidebar{width:240px;flex:none;display:flex;flex-direction:column;overflow:hidden} \
                   #tabs{flex:1;overflow-y:auto;display:flex;flex-direction:column} \
                   .row{height:28px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    for id in ["r1", "r2", "r3"] {
        let row = super::find_by_id_all(&root, &doc, id).expect(id);
        assert_eq!(
            row.rect.height, 28.0,
            "{id}: explicit height:28px must survive the ancestor's indefinite probe pass, got {}",
            row.rect.height
        );
    }
}

#[test]
fn flex_probe_pass_does_not_burn_percentage_width_into_style() {
    // BUG-343: #x is probed (row, `flex-basis:auto` + explicit width) at its
    // *unshrunk* 300px, so its `width:100%` child resolved to 300px — and that
    // px value replaced the percentage declaration. #x is then shrunk to 200px
    // by the real pass, but the child (`flex-shrink:0`) stayed at the stale
    // 300px because the percentage was gone.
    let html = r#"<div id="outer"><div id="a"></div><div id="x"><div id="p"></div></div></div>"#;
    let css = "body{margin:0} \
                   #outer{display:flex;width:400px} \
                   #a{width:300px;height:20px} \
                   #x{width:300px;display:flex} \
                   #p{flex-shrink:0;width:100%;height:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let x = super::find_by_id_all(&root, &doc, "x").expect("x");
    assert_eq!(x.rect.width, 200.0, "#x should shrink to 200, got {}", x.rect.width);
    let p = super::find_by_id_all(&root, &doc, "p").expect("p");
    assert_eq!(
        p.rect.width, 200.0,
        "width:100% must re-resolve against the shrunk container (200), not stay at the probe's 300; got {}",
        p.rect.width
    );
}

#[test]
fn flex_single_line_row_gap_excluded_from_cross_size() {
    // BUG-113: a single-line row flex container must NOT add the row-gap
    // (`gap`/`row-gap`) to its own cross size — there is no second line to
    // separate. Previously the per-line trailing cross-gap leaked into the
    // container height (e.g. TEST-53 rows drifted ~24px vertically).
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div></div>"#;
    // gap:24px sets both row- and column-gap; in a row flex the row-gap is the cross gap.
    let css = "body{margin:0} #flex{display:flex;gap:24px;width:400px} \
                   #a{width:100px;height:120px} #b{width:100px;height:120px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let flex = super::find_by_id_all(&root, &doc, "flex").expect("flex");
    assert_eq!(
        flex.rect.height, 120.0,
        "single-line flex height must equal the tallest item (120), not 120+gap; got {}",
        flex.rect.height
    );
}

#[test]
fn flex_auto_basis_item_with_min_width_uses_min_not_container_width() {
    // BUG-179: a row flex item with no explicit `width` and `min-width` set was
    // reporting the full container width as its flex base size. In the preliminary
    // layout pass a block-level item stretches to fill the container, so
    // item.rect.width == container_width. The old code fell back to that value when
    // no child had an explicit width, inflating the total_hyp beyond the container
    // and triggering erroneous shrink. After the fix, flex_auto_base_main_width
    // computes max-content (= 0 for an empty div) and clamps by min-width → 200px.
    //
    // 600px container, A (no width, min-width:200px), B (width:100px).
    // total_hyp = 200+100 = 300 < 600 → no shrink → A stays at 200px, B at 100px.
    // Old behaviour: A.base = 600px, total_hyp = 700px → shrink, A ≈ 514px.
    let html = r#"<div id="flex"><div id="a"></div><div id="b"></div></div>"#;
    let css = "body{margin:0} #flex{display:flex;width:600px} #a{min-width:200px} #b{width:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.width, 200.0, "A must stay at min-width (200), not shrink from inflated base; a.w={}", a.rect.width);
    assert_eq!(b.rect.width, 100.0, "B explicit width stays; b.w={}", b.rect.width);
    assert_eq!(b.rect.x, 200.0, "B starts after A; b.x={}", b.rect.x);
}

#[test]
fn bug433_flex_item_not_shrunk_below_automatic_minimum_size() {
    // BUG-433 / CSS Flexbox §4.5: `min-width: auto` on a flex item means its
    // content-based minimum size, so a row of fixed-width children must overflow
    // its container instead of being shrunk to an equal share of it.
    //
    // 300px container, two items each holding a 200px child. Old behaviour:
    // deficit -100 split evenly → 150 + 150 (content sticking out of both items).
    // Expected (Edge/Chromium): 200 + 200, second at x=200, row overflows by 100.
    let html = r#"<div id="row"><div id="a"><div class="fixed"></div></div><div id="b"><div class="fixed"></div></div></div>"#;
    let css = "body{margin:0} #row{display:flex;width:300px} .fixed{width:200px;height:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.width, 200.0, "A must stay at its min-content width; a.w={}", a.rect.width);
    assert_eq!(b.rect.width, 200.0, "B must stay at its min-content width; b.w={}", b.rect.width);
    assert_eq!(b.rect.x, 200.0, "B starts right after A, row overflows; b.x={}", b.rect.x);
}

#[test]
fn bug433_shrink_redistributes_deficit_onto_the_still_flexible_item() {
    // BUG-433 §9.7 step 4 is a *loop*: freezing the item that hit its minimum must
    // push the deficit it could not absorb onto the remaining flexible items.
    //
    // 300px container. A holds a 200px child (floor 200), B is empty text-less
    // with flex-basis 200px and no content (floor 0). Deficit = -100.
    // One proportional pass would give 150/150; after clamping A back to 200 the
    // remaining -100 must land entirely on B → 200 + 100 = 300, no overflow.
    let html = r#"<div id="row"><div id="a"><div class="fixed"></div></div><div id="b"></div></div>"#;
    let css = "body{margin:0} #row{display:flex;width:300px} \
                   #a{flex:0 1 200px} #b{flex:0 1 200px} .fixed{width:200px;height:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.width, 200.0, "A frozen at its automatic minimum; a.w={}", a.rect.width);
    assert_eq!(b.rect.width, 100.0, "B absorbs the whole deficit; b.w={}", b.rect.width);
    assert_eq!(b.rect.x, 200.0, "row exactly fills the container; b.x={}", b.rect.x);
}

#[test]
fn bug433_item_with_collapsible_contents_still_shrinks_below_its_width() {
    // BUG-433 / §4.5: the content-based minimum is the *smaller* of the specified
    // size suggestion (the item's own `width`) and the content size suggestion (the
    // min-content width of its contents). An item whose contents can collapse to
    // nothing — here a single `width:100%` child, which contributes 0 to
    // min-content — therefore has a floor of 0 and stays fully shrinkable.
    // Reading the item's own `width` as its min-content would freeze both items at
    // 300px and wrongly overflow the 400px container.
    let html = r#"<div id="outer"><div id="a"></div><div id="x"><div id="p"></div></div></div>"#;
    let css = "body{margin:0} #outer{display:flex;width:400px} \
                   #a{width:300px;height:20px} #x{width:300px} #p{width:100%;height:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let x = super::find_by_id_all(&root, &doc, "x").expect("x");
    assert_eq!(a.rect.width, 200.0, "A has no contents to floor it; a.w={}", a.rect.width);
    assert_eq!(x.rect.width, 200.0, "X's `width:100%` child floors nothing; x.w={}", x.rect.width);
}

#[test]
fn bug433_scroll_container_flex_item_has_no_automatic_minimum() {
    // CSS Flexbox §4.5: the content-based minimum applies only while the main-axis
    // overflow is `visible`. A scroll container may still be shrunk to zero, so the
    // BUG-433 floor must not freeze it — otherwise `overflow:hidden` rows (the
    // standard "truncating flex child" idiom) would start overflowing.
    let html = r#"<div id="row"><div id="a"><div class="fixed"></div></div><div id="b"><div class="fixed"></div></div></div>"#;
    let css = "body{margin:0} #row{display:flex;width:300px} \
                   #a{overflow:hidden} #b{overflow:hidden} .fixed{width:200px;height:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("a");
    let b = super::find_by_id_all(&root, &doc, "b").expect("b");
    assert_eq!(a.rect.width, 150.0, "scroll container shrinks freely; a.w={}", a.rect.width);
    assert_eq!(b.rect.x, 150.0, "no overflow for scroll containers; b.x={}", b.rect.x);
}

/// Returns the first `InlineRun` box in `b`'s subtree (depth-first).
fn find_inline_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) {
        return Some(b);
    }
    b.children.iter().find_map(find_inline_run)
}

#[test]
fn control_only_text_node_creates_no_line_box() {
    // BUG-120: a text node consisting only of a C0 control char must not
    // open an inline run — Edge renders U+0001 invisible with no line box.
    let html = "<div id=\"wrap\">\u{0001}</div>";
    let css = "body{margin:0} #wrap{width:100px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let wrap = super::find_by_id_all(&root, &doc, "wrap").expect("wrap");
    assert_eq!(
        wrap.rect.height, 0.0,
        "control-only text must not produce a line box, got height {}",
        wrap.rect.height
    );
}

#[test]
fn control_char_text_does_not_shift_following_block() {
    // BUG-120 / BUG-119 scenario: a stray U+0001 in body text shifted all
    // following content down by one line height (~19.2px) in Lumen,
    // while Edge keeps it at y=0.
    let html = "\u{0001}<div id=\"x\"></div>";
    let css = "body{margin:0} #x{width:100px;height:50px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let x = super::find_by_id_all(&root, &doc, "x").expect("x");
    assert_eq!(
        x.rect.y, 0.0,
        "block after control-only text must stay at y=0, got {}",
        x.rect.y
    );
}

#[test]
fn control_chars_stripped_from_inline_text() {
    // BUG-120: embedded C0 controls are zero-advance in Edge — strip them
    // from inline segments so they contribute no glyphs or width.
    let html = "<div id=\"t\">a\u{0001}b\u{0002}c</div>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("t");
    let run = find_inline_run(t).expect("inline run");
    let super::super::BoxKind::InlineRun { segments, .. } = &run.kind else {
        unreachable!()
    };
    assert_eq!(segments[0].text, "abc", "controls must be stripped, got {:?}", segments[0].text);
}

#[test]
fn control_chars_stripped_in_preserved_whitespace() {
    // BUG-120: white-space:pre keeps tab/LF (CSS Text L3 §4.1) but other
    // Cc controls are still invisible and must be stripped.
    let html = "<div id=\"p\">a\u{0001}b\tc</div>";
    let css = "#p{white-space:pre}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let p = super::find_by_id_all(&root, &doc, "p").expect("p");
    let run = find_inline_run(p).expect("inline run");
    let super::super::BoxKind::InlineRun { segments, .. } = &run.kind else {
        unreachable!()
    };
    assert_eq!(segments[0].text, "ab\tc", "tab preserved, U+0001 stripped; got {:?}", segments[0].text);
}

#[test]
fn svg_defs_element_is_skipped() {
    // <defs> container should be invisible (Skip).
    let html = r#"<svg><defs><rect id="r"/></defs><circle/></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    // SVG should have only <circle> as visible child, <defs> should be skipped.
    assert!(!root.children.is_empty(), "svg should have children");
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { .. } = &svg.kind
    {
        assert!(!svg.children.is_empty(), "svg should have visible children");
        // Should contain circle, not defs.
    }
}

#[test]
fn svg_intrinsic_ratio_from_viewbox() {
    // SVG with viewBox="0 0 200 100" should have intrinsic ratio of 2:1.
    let html = r#"<svg viewBox="0 0 200 100"></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    // Find SVG root.
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { view_box, .. } = &svg.kind
    {
        let ratio = super::super::svg_intrinsic_ratio(view_box);
        assert_eq!(ratio, Some(2.0), "viewBox 200x100 should give ratio 2.0");
    }
}

#[test]
fn svg_intrinsic_ratio_none_without_viewbox() {
    // SVG without viewBox should return None for intrinsic ratio.
    let html = r#"<svg></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { view_box, .. } = &svg.kind
    {
        let ratio = super::super::svg_intrinsic_ratio(view_box);
        assert_eq!(ratio, None, "svg without viewBox should have no intrinsic ratio");
    }
}

#[test]
fn svg_preserve_aspect_ratio_meet() {
    // preserveAspectRatio="xMidYMid meet" (default) should parse correctly.
    let html = r#"<svg viewBox="0 0 100 100" width="200" height="100"></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { preserve_aspect_ratio, .. } = &svg.kind
    {
        assert_eq!(preserve_aspect_ratio.meet_or_slice, super::super::SvgMeetOrSlice::Meet);
        assert_eq!(preserve_aspect_ratio.align_x, super::super::SvgAlignX::Mid);
        assert_eq!(preserve_aspect_ratio.align_y, super::super::SvgAlignY::Mid);
    }
}

#[test]
fn svg_preserve_aspect_ratio_slice() {
    // preserveAspectRatio="xMinYMin slice" should parse correctly.
    let html = r#"<svg preserveAspectRatio="xMinYMin slice"></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { preserve_aspect_ratio, .. } = &svg.kind
    {
        assert_eq!(preserve_aspect_ratio.meet_or_slice, super::super::SvgMeetOrSlice::Slice);
        assert_eq!(preserve_aspect_ratio.align_x, super::super::SvgAlignX::Min);
        assert_eq!(preserve_aspect_ratio.align_y, super::super::SvgAlignY::Min);
    }
}

#[test]
fn svg_use_element_references_target() {
    // <use href="#target"/> should reference element with id="target".
    // SVG 1.1 § 5.6 — <use> creates a reference to another element.
    let html = "<svg><defs><rect id=\"r1\" x=\"10\" y=\"10\" width=\"50\" height=\"50\"/></defs><use href=\"#r1\" x=\"100\" y=\"100\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    // SVG should have at least the <use> element (which should create referenced content).
    if let Some(svg) = root.children.first()
        && let super::super::BoxKind::SvgRoot { .. } = &svg.kind
    {
        // <use> should have been processed and added to the layout.
        // The exact structure depends on implementation, but we verify no panic.
        assert!(!svg.children.is_empty(), "svg should have layout children from <use>");
    }
}

#[test]
fn svg_use_translate_x_y() {
    // <use x="10" y="20"> should apply a translate transform.
    // The clone group's svg_group_transform should encode translate(10, 20).
    let html = "<svg><circle id=\"c1\" cx=\"0\" cy=\"0\" r=\"5\"/><use href=\"#c1\" x=\"10\" y=\"20\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_use_group(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::Block)
                && child.svg_group_transform.is_some()
                && !child.children.is_empty()
            {
                return Some(child);
            }
            if let Some(f) = find_use_group(child) { return Some(f); }
        }
        None
    }

    let group = find_use_group(&root);
    assert!(group.is_some(), "<use> should produce a Block group with svg_group_transform");
    if let Some(g) = group {
        let m = g.svg_group_transform.as_ref().unwrap().matrix;
        // translate(10, 20): [1, 0, 0, 1, 10, 20]
        assert!((m[4] - 10.0).abs() < 0.1, "expected tx=10, got {}", m[4]);
        assert!((m[5] - 20.0).abs() < 0.1, "expected ty=20, got {}", m[5]);
    }
}

#[test]
fn svg_use_references_shape_in_defs() {
    // <use href="#r"> where <rect id="r"> is inside <defs> should produce a clone.
    let html = "<svg><defs><rect id=\"r\" x=\"5\" y=\"5\" width=\"30\" height=\"20\"/></defs>\
                    <use href=\"#r\" x=\"50\" y=\"60\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_any_rect(b: &super::super::LayoutBox) -> bool {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) {
            return true;
        }
        b.children.iter().any(find_any_rect)
    }

    assert!(find_any_rect(&root), "cloned <rect> should appear in layout tree");
}

#[test]
fn svg_use_references_group() {
    // <use href="#g1"> where <g id="g1"> contains shapes clones the whole group.
    let html = "<svg><g id=\"g1\"><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\"/>\
                    <circle cx=\"5\" cy=\"5\" r=\"3\"/></g>\
                    <use href=\"#g1\" x=\"100\" y=\"0\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn count_rects(b: &super::super::LayoutBox) -> usize {
        let self_count = if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) { 1 } else { 0 };
        self_count + b.children.iter().map(count_rects).sum::<usize>()
    }

    let rect_count = count_rects(&root);
    assert!(rect_count >= 2, "both original and cloned <rect> should be in layout; found {rect_count}");
}

#[test]
fn svg_use_cycle_does_not_panic() {
    // Self-referential <use> must not cause infinite recursion.
    // <g id="a"> <use href="#a"/> </g> — cycle via self
    let html = "<svg><g id=\"a\"><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\"/>\
                    <use href=\"#a\"/></g><use href=\"#a\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    // Must not panic/hang.
    let _root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
}

#[test]
fn svg_use_xlink_href() {
    // xlink:href should also work for legacy SVG 1.1 references.
    let html = "<svg><circle id=\"c2\" cx=\"20\" cy=\"20\" r=\"8\"/>\
                    <use xlink:href=\"#c2\" x=\"5\" y=\"5\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn has_any_shape(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::SvgShape { .. }) { return true; }
        b.children.iter().any(has_any_shape)
    }
    assert!(has_any_shape(&root), "xlink:href <use> should produce shape in layout");
}

#[test]
fn svg_use_symbol_element() {
    // <symbol id="s"> acts as a group; <use href="#s"> clones its children.
    let html = "<svg><symbol id=\"s\"><rect x=\"0\" y=\"0\" width=\"20\" height=\"20\"/></symbol>\
                    <use href=\"#s\" x=\"10\" y=\"30\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn has_any_shape(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::SvgShape { .. }) { return true; }
        b.children.iter().any(has_any_shape)
    }
    assert!(has_any_shape(&root), "<symbol> target via <use> should produce shape in layout");
}

#[test]
fn svg_use_symbol_viewbox_scales_to_use_size() {
    // BUG-246: a <use> referencing a <symbol viewBox="0 0 40 40"> is sized by
    // the <use>'s width/height — the symbol's viewBox maps into that viewport.
    // Two instances of differing size must render at differing scales, not at
    // the viewBox's intrinsic 40×40 size.
    let html = "<svg width=\"400\" height=\"400\">\
                    <symbol id=\"s\" viewBox=\"0 0 40 40\"><rect x=\"0\" y=\"0\" width=\"40\" height=\"40\"/></symbol>\
                    <use href=\"#s\" x=\"0\" y=\"0\" width=\"40\" height=\"40\"/>\
                    <use href=\"#s\" x=\"0\" y=\"0\" width=\"80\" height=\"80\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn collect_rects(b: &super::super::LayoutBox, acc: &mut Vec<super::super::Rect>) {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) {
            acc.push(b.rect);
        }
        b.children.iter().for_each(|c| collect_rects(c, acc));
    }
    let mut rects = Vec::new();
    collect_rects(&root, &mut rects);
    assert_eq!(rects.len(), 2, "both <use> instances should clone the symbol rect; got {rects:?}");
    assert!(rects.iter().any(|r| (r.width - 40.0).abs() < 0.5), "width=40 instance should render at 40px; got {rects:?}");
    assert!(rects.iter().any(|r| (r.width - 80.0).abs() < 0.5), "width=80 instance should scale to 80px; got {rects:?}");
}

#[test]
fn svg_use_symbol_no_explicit_size_scales_to_css_icon_size() {
    // BUG-334: an icon sprite pattern — `<use href="#s">` with no width/height
    // attributes at all, sized purely via CSS on the enclosing `<svg>` (e.g.
    // `.icon { width: 14px; height: 14px }`). Per SVG 2 §5.7/§7.10 the used
    // viewport for the reference is 100% of the *enclosing* svg's own
    // (CSS-resolved) viewport, not the symbol's viewBox dims (that was the
    // BUG-246-era identity-scale regression this test guards against).
    let html = "<svg class=\"icon\"><symbol id=\"s\" viewBox=\"0 0 24 24\">\
                    <rect x=\"0\" y=\"0\" width=\"24\" height=\"24\"/></symbol>\
                    <use href=\"#s\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0} .icon{width:14px;height:14px}");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn collect_rects(b: &super::super::LayoutBox, acc: &mut Vec<super::super::Rect>) {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) {
            acc.push(b.rect);
        }
        b.children.iter().for_each(|c| collect_rects(c, acc));
    }
    let mut rects = Vec::new();
    collect_rects(&root, &mut rects);
    assert_eq!(rects.len(), 1, "use should clone the symbol rect; got {rects:?}");
    assert!(
        (rects[0].width - 14.0).abs() < 0.5,
        "icon should scale to the CSS-sized 14px viewport, not the viewBox's 24px; got {rects:?}"
    );
}

#[test]
fn parse_svg_points_handles_commas_and_spaces() {
    // SVG `points` accepts commas and/or whitespace between numbers.
    assert_eq!(super::super::parse_svg_points("20,5 25,17 38,17"), vec![(20.0, 5.0), (25.0, 17.0), (38.0, 17.0)]);
    assert_eq!(super::super::parse_svg_points("0 0 10 10"), vec![(0.0, 0.0), (10.0, 10.0)]);
    // A trailing lone coordinate is dropped (no half-pair).
    assert_eq!(super::super::parse_svg_points("1 2 3"), vec![(1.0, 2.0)]);
}

#[test]
fn points_to_path_d_closes_polygon_but_not_polyline() {
    let pts = vec![(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)];
    assert_eq!(super::super::points_to_path_d(&pts, true).unwrap(), "M 0 0 L 10 0 L 5 8 Z");
    assert_eq!(super::super::points_to_path_d(&pts, false).unwrap(), "M 0 0 L 10 0 L 5 8");
    // Fewer than two points → nothing to render.
    assert!(super::super::points_to_path_d(&[(1.0, 1.0)], true).is_none());
}

#[test]
fn svg_polygon_renders_as_path_shape() {
    // BUG-201: <polygon> had no case and rendered nothing. It must now appear
    // as a Path-kind SVG shape (closed contour).
    let html = "<svg><polygon points=\"20,5 25,17 38,17 27,25 20,30\" style=\"fill:#f39c12;\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn has_path(b: &super::super::LayoutBox) -> bool {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Path { .. }, .. }) { return true; }
        b.children.iter().any(has_path)
    }
    assert!(has_path(&root), "<polygon> should produce a Path-kind SVG shape");
}

#[test]
fn svg_polygon_inside_symbol_renders_via_use() {
    // BUG-201 row 2b: <symbol> containing a <polygon>, instantiated by <use>.
    // The symbol must not render directly, but <use> must clone its polygon.
    let html = "<svg width=\"200\" height=\"120\">\
                    <symbol id=\"star\"><polygon points=\"20,5 25,17 38,17 27,25 20,30\" style=\"fill:#f39c12;\"/></symbol>\
                    <use href=\"#star\" x=\"10\" y=\"10\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn count_paths(b: &super::super::LayoutBox) -> usize {
        let self_count = usize::from(matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Path { .. }, .. }));
        self_count + b.children.iter().map(count_paths).sum::<usize>()
    }
    // Exactly one path: the <use> clone. The <symbol> itself must NOT render.
    assert_eq!(count_paths(&root), 1, "polygon should render once (via <use>), not directly from <symbol>");
}

#[test]
fn svg_use_multiple_siblings_all_clone() {
    // BUG-201: the HTML5 parser does not honour `<use/>` self-closing, so
    // sibling `<use>` elements nest as DOM children. Both must still clone.
    let html = "<svg width=\"300\" height=\"120\">\
                    <defs><rect id=\"r1\" x=\"0\" y=\"0\" width=\"50\" height=\"35\"/></defs>\
                    <use href=\"#r1\" x=\"20\" y=\"20\"/>\
                    <use href=\"#r1\" x=\"100\" y=\"60\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn collect_rects(b: &super::super::LayoutBox, acc: &mut Vec<super::super::Rect>) {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) {
            acc.push(b.rect);
        }
        b.children.iter().for_each(|c| collect_rects(c, acc));
    }
    let mut rects = Vec::new();
    collect_rects(&root, &mut rects);
    // Two clones at distinct positions; original in <defs> stays hidden.
    assert_eq!(rects.len(), 2, "both <use> siblings should clone; got {rects:?}");
    assert!(rects.iter().any(|r| (r.x - 20.0).abs() < 0.1 && (r.y - 20.0).abs() < 0.1), "clone @ (20,20) missing: {rects:?}");
    assert!(rects.iter().any(|r| (r.x - 100.0).abs() < 0.1 && (r.y - 60.0).abs() < 0.1), "clone @ (100,60) missing: {rects:?}");
}

#[test]
fn svg_use_scale_transform_does_not_scale_viewport_origin() {
    // BUG-201 row 3: an element `transform="scale(k)"` must operate in SVG
    // user space, NOT scale the document-space viewport origin. The svg sits
    // below the page origin so the bug (scaling oy) is observable.
    let html = "<div style=\"height:300px\"></div>\
                    <svg width=\"460\" height=\"130\">\
                    <defs><rect id=\"tile\" x=\"0\" y=\"0\" width=\"40\" height=\"40\"/></defs>\
                    <use href=\"#tile\" x=\"270\" y=\"20\" transform=\"scale(0.75)\"/></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 800.0));

    fn find_svg_root(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::SvgRoot { .. }) { return Some(b); }
        b.children.iter().find_map(find_svg_root)
    }
    fn first_rect(b: &super::super::LayoutBox) -> Option<super::super::Rect> {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Rect { .. }, .. }) {
            return Some(b.rect);
        }
        b.children.iter().find_map(first_rect)
    }
    let svg = find_svg_root(&root).expect("SvgRoot");
    let rect = first_rect(&root).expect("scaled tile rect");
    // scale(0.75) ∘ translate(270,20) applied to (0,0) = (202.5, 15) in the
    // SVG local frame, then offset by the viewport doc-origin (svg.rect.x/y).
    let expected_x = svg.rect.x + 202.5;
    let expected_y = svg.rect.y + 15.0;
    assert!((rect.x - expected_x).abs() < 0.2, "scaled tile x: got {}, expected {}", rect.x, expected_x);
    assert!((rect.y - expected_y).abs() < 0.2, "scaled tile y (origin must not be scaled): got {}, expected {}", rect.y, expected_y);
    assert!((rect.width - 30.0).abs() < 0.2 && (rect.height - 30.0).abs() < 0.2, "scaled tile size 30×30: got {}×{}", rect.width, rect.height);
}

