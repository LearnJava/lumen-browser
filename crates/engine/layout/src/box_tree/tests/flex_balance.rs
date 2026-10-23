use lumen_core::geom::Size;

// ── flex-wrap: balance / flex-line-count (CSS Flexbox L2 #algo-balance) ──

/// Top-left `(x, y)` of the items `i0..i{n-1}` of `#flex` in `css` laid out at 800×600.
fn item_pos(items: &str, css: &str, n: usize) -> Vec<(f32, f32)> {
    let html = format!(r#"<div id="flex">{items}</div>"#);
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (0..n)
        .map(|i| {
            let b = super::find_by_id_all(&root, &doc, &format!("i{i}")).expect("item");
            (b.rect.x, b.rect.y)
        })
        .collect()
}

#[test]
fn balance_splits_three_one_into_two_two() {
    // Greedy wrap gives 3+1 (25·3 + 2·10 = 95 ≤ 100); balance spreads them 2+2.
    let items = r#"<div id="i0"></div><div id="i1"></div><div id="i2"></div><div id="i3"></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:balance;width:100px;gap:10px} #flex>div{width:25px;height:25px}";
    let p = item_pos(items, css, 4);
    assert_eq!(p, vec![(0.0, 0.0), (35.0, 0.0), (0.0, 35.0), (35.0, 35.0)], "{p:?}");
    // `wrap` keeps the greedy 3+1.
    let p = item_pos(items, &css.replace("balance", "wrap"), 4);
    assert_eq!(p[3], (0.0, 35.0), "{p:?}");
}

#[test]
fn flex_line_count_raises_the_line_count_and_start_bias_wins_ties() {
    // Nine 20px items, gap 20, container wide: one greedy line, minimum 4 → 3+2+2+2.
    let items: String = (0..9).map(|i| format!(r#"<div id="i{i}"></div>"#)).collect();
    let css = "body{margin:0} #flex{display:flex;flex-wrap:balance;flex-line-count:4;gap:20px;width:400px} #flex>div{width:20px;height:20px}";
    let p = item_pos(&items, css, 9);
    let ys: Vec<f32> = p.iter().map(|q| q.1).collect();
    assert_eq!(ys, vec![0.0, 0.0, 0.0, 40.0, 40.0, 80.0, 80.0, 120.0, 120.0], "{p:?}");
}

#[test]
fn flex_line_count_is_ignored_without_balance() {
    let items: String = (0..4).map(|i| format!(r#"<div id="i{i}"></div>"#)).collect();
    let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;flex-line-count:10000;width:400px} #flex>div{width:20px;height:20px}";
    let p = item_pos(&items, css, 4);
    assert!(p.iter().all(|q| q.1 == 0.0), "{p:?}");
}

#[test]
fn balance_floors_negative_outer_size_at_zero_for_line_breaking() {
    // 150 + (-100) would "fit" the 100px line; floored at zero the second item wraps.
    let items = r#"<div id="i0" style="width:150px"></div><div id="i1" style="width:0;margin:0 -50px"></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-wrap:balance;width:100px} #flex>div{height:50px}";
    let p = item_pos(items, css, 2);
    assert_eq!(p[1].1, 50.0, "{p:?}");
}

#[test]
fn balanced_inline_flex_max_content_uses_the_line_count() {
    // 80,40,40 with gap 20 on two lines: [80] and [40,40] → widest line 100.
    let html = r#"<div id="flex"><div style="width:80px"></div><div style="width:40px"></div><div style="width:40px"></div></div>"#;
    let css = "body{margin:0} #flex{display:inline-flex;flex-wrap:balance;flex-line-count:2;gap:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let f = super::find_by_id_all(&root, &doc, "flex").expect("flex");
    assert_eq!(f.rect.width, 100.0, "{}", f.rect.width);
}

#[test]
fn balanced_auto_height_column_is_cut_by_flex_line_count() {
    // The column has no definite height, so only `flex-line-count: 2` makes lines: 2 + 1.
    let items = r#"<div id="i0"></div><div id="i1"></div><div id="i2"></div>"#;
    let css = "body{margin:0} #flex{display:flex;flex-direction:column;flex-wrap:balance;flex-line-count:2;gap:20px;width:220px} #flex>div{width:100px;height:100px}";
    let p = item_pos(items, css, 3);
    assert_eq!(p, vec![(0.0, 0.0), (0.0, 120.0), (120.0, 0.0)], "{p:?}");
}

#[test]
fn balanced_inline_flex_column_is_as_wide_as_its_lines() {
    // 40px items, gap 20, `flex-line-count: 2`: lines of 2 + 1 items → 40 + 20 + 40 wide, 100 tall.
    let html = r#"<div id="flex"><div style="width:40px;height:40px"></div><div style="width:40px;height:40px"></div><div style="width:40px;height:40px"></div></div>"#;
    let css = "body{margin:0} #flex{display:inline-flex;flex-direction:column;flex-wrap:balance;flex-line-count:2;gap:20px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let f = super::find_by_id_all(&root, &doc, "flex").expect("flex");
    assert_eq!((f.rect.width, f.rect.height), (100.0, 100.0), "{:?}", f.rect);
}
