// ── ::first-letter / ::first-line structural markers ─────────────────────

#[test]
fn first_letter_segment_marked_on_plain_paragraph() {
    // The first text segment in a block should be marked as FirstLetter.
    let root = super::super::layout(
        &lumen_html_parser::parse("<p>Hello world</p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        assert!(!segments.is_empty(), "expected at least one segment");
        assert_eq!(
            segments[0].pseudo_kind,
            super::super::PseudoKind::FirstLetter,
            "first segment must be PseudoKind::FirstLetter"
        );
        // Remaining segments have no pseudo kind.
        for seg in segments.iter().skip(1) {
            assert_eq!(seg.pseudo_kind, super::super::PseudoKind::None, "only first seg is FirstLetter");
        }
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_letter_not_marked_on_second_paragraph() {
    // Each block creates its own inline run; each run's first seg is marked.
    let root = super::super::layout(
        &lumen_html_parser::parse("<p>One</p><p>Two</p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    fn collect_runs<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { collect_runs(c, out); }
    }
    let mut runs = Vec::new();
    collect_runs(&root, &mut runs);
    assert!(runs.len() >= 2, "expected at least 2 inline runs");
    for run in &runs {
        if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind
            && !segments.is_empty()
        {
            assert_eq!(
                segments[0].pseudo_kind,
                super::super::PseudoKind::FirstLetter,
                "each run's first seg should be FirstLetter"
            );
        }
    }
}

/// Shared helpers for the ::first-letter drop-cap tests (BB-2).
mod first_letter_drop_cap {
    struct Fixed8;
    impl super::super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }

    fn layout(html: &str, css: &str) -> super::super::super::LayoutBox {
        super::super::super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
            &Fixed8,
        )
    }

    /// Depth-first search for the synthesized drop-cap box.
    fn find_drop_cap(b: &super::super::super::LayoutBox) -> Option<&super::super::super::LayoutBox> {
        if super::super::super::is_first_letter_box(b) {
            return Some(b);
        }
        b.children.iter().find_map(find_drop_cap)
    }

    /// First non-drop-cap InlineRun in the tree (the paragraph remainder).
    fn find_rest_run(b: &super::super::super::LayoutBox) -> Option<&super::super::super::LayoutBox> {
        if super::super::super::is_first_letter_box(b) {
            return None;
        }
        if matches!(b.kind, super::super::super::BoxKind::InlineRun { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find_rest_run)
    }

    fn letter_seg(b: &super::super::super::LayoutBox) -> &super::super::super::InlineSegment {
        let super::super::super::BoxKind::InlineRun { segments, .. } = &b.children[0].kind else {
            panic!("drop-cap inner box must be InlineRun");
        };
        &segments[0]
    }

    #[test]
    fn float_extracts_drop_cap_box() {
        let root = layout(
            "<p>Hello world</p>",
            "p::first-letter { float: left; font-size: 32px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        assert_eq!(letter_seg(cap).text, "H");
        assert_eq!(letter_seg(cap).style.font_size, 32.0);
        let rest = find_rest_run(&root).expect("rest run missing");
        let super::super::super::BoxKind::InlineRun { segments, .. } = &rest.kind else {
            unreachable!();
        };
        assert_eq!(segments[0].text, "ello world");
    }

    #[test]
    fn float_narrows_text_beside_drop_cap() {
        let root = layout(
            "<p>Hello world</p>",
            "p::first-letter { float: left; font-size: 32px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        let rest = find_rest_run(&root).expect("rest run missing");
        assert!(
            rest.rect.x >= cap.rect.x + cap.rect.width - 0.01,
            "rest run x={} must start after drop cap right edge {}",
            rest.rect.x,
            cap.rect.x + cap.rect.width,
        );
    }

    #[test]
    fn float_drop_cap_shrinks_to_letter_width() {
        // Fixed8: every char 8px. padding 6px each side → 8 + 12 = 20px outer,
        // height = 32px × line-height 1 + 12 = 44px.
        let root = layout(
            "<p>Hello world</p>",
            "p::first-letter { float: left; font-size: 32px; line-height: 1; padding: 6px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        assert!(
            (cap.rect.width - 20.0).abs() < 0.1,
            "drop cap width {} ≠ 20",
            cap.rect.width,
        );
        assert!(
            (cap.rect.height - 44.0).abs() < 0.1,
            "drop cap height {} ≠ 44",
            cap.rect.height,
        );
    }

    #[test]
    fn float_single_char_paragraph_drops_empty_run() {
        let root = layout("<p>X</p>", "p::first-letter { float: left; font-size: 32px; line-height: 1; }");
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        assert_eq!(letter_seg(cap).text, "X");
        assert!(find_rest_run(&root).is_none(), "emptied InlineRun must be dropped");
        // CSS 2.1 §9.5: the paragraph height still encloses the float.
        fn find_p<'a>(b: &'a super::super::super::LayoutBox, cap: &super::super::super::LayoutBox) -> Option<&'a super::super::super::LayoutBox> {
            if b.children.iter().any(|c| std::ptr::eq(c, cap)) {
                return Some(b);
            }
            b.children.iter().find_map(|c| find_p(c, cap))
        }
        let p = find_p(&root, cap).expect("paragraph not found");
        assert!(
            p.rect.height >= cap.rect.height - 0.01,
            "paragraph height {} must enclose float {}",
            p.rect.height,
            cap.rect.height,
        );
    }

    #[test]
    fn float_right_places_drop_cap_at_right_edge() {
        let root = layout(
            "<p>Hello world</p>",
            "p::first-letter { float: right; font-size: 32px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        let rest = find_rest_run(&root).expect("rest run missing");
        assert!(
            cap.rect.x > rest.rect.x,
            "right-floated drop cap x={} must sit right of the text x={}",
            cap.rect.x,
            rest.rect.x,
        );
        assert!(
            cap.rect.x + cap.rect.width <= 800.0 + 0.01,
            "drop cap must not overflow the container",
        );
    }

    #[test]
    fn non_float_first_letter_stays_inline() {
        let root = layout("<p>Hello world</p>", "p::first-letter { font-size: 32px; }");
        assert!(find_drop_cap(&root).is_none(), "no drop-cap box without float");
        let run = find_rest_run(&root).expect("run missing");
        let super::super::super::BoxKind::InlineRun { segments, .. } = &run.kind else {
            unreachable!();
        };
        assert_eq!(segments[0].text, "H");
        assert_eq!(segments[0].style.font_size, 32.0);
        assert_eq!(segments[1].text, "ello world");
    }

    #[test]
    fn leading_punctuation_joins_first_letter() {
        // CSS Pseudo-elements L4 §5.1: leading punctuation is part of the unit.
        let root = layout("<p>\u{201C}Hello world\u{201D}</p>", "p::first-letter { font-size: 32px; }");
        let run = find_rest_run(&root).expect("run missing");
        let super::super::super::BoxKind::InlineRun { segments, .. } = &run.kind else {
            unreachable!();
        };
        assert_eq!(segments[0].text, "\u{201C}H");
        assert_eq!(segments[0].style.font_size, 32.0);
    }

    #[test]
    fn float_extraction_skips_leading_whitespace() {
        // Pretty-printed HTML: raw segment text starts with "\n  " — the
        // first-letter unit must be the first non-whitespace character,
        // not the newline (regression: TEST-58 drop cap rendered "\n").
        let root = layout(
            "<p>\n          Once upon a time</p>",
            "p::first-letter { float: left; font-size: 48px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        assert_eq!(letter_seg(cap).text.trim(), "O");
        let rest = find_rest_run(&root).expect("rest run missing");
        let super::super::super::BoxKind::InlineRun { lines, .. } = &rest.kind else {
            unreachable!();
        };
        assert!(
            lines[0][0].text.starts_with("nce"),
            "rest must start with 'nce', got {:?}",
            lines[0][0].text,
        );
    }

    #[test]
    fn first_line_does_not_override_drop_cap() {
        // ::first-letter wins over ::first-line where they conflict.
        let root = layout(
            "<p>Hello world and more words here</p>",
            "p::first-letter { float: left; font-size: 32px; } p::first-line { font-size: 20px; }",
        );
        let cap = find_drop_cap(&root).expect("drop-cap box not created");
        let super::super::super::BoxKind::InlineRun { lines, .. } = &cap.children[0].kind else {
            unreachable!();
        };
        assert!(
            lines[0].iter().all(|f| f.style.font_size == 32.0),
            "drop-cap frags must keep the ::first-letter font, not ::first-line",
        );
    }
}

/// CSS Inline Layout L3 §5 — `initial-letter` drop cap (Phase 0).
mod initial_letter {
    struct Fixed8;
    impl super::super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }

    fn layout(html: &str, css: &str) -> super::super::super::LayoutBox {
        super::super::super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
            &Fixed8,
        )
    }

    /// The synthesized cap reuses the first-letter-box shape (float + single
    /// FirstLetter segment), so `is_first_letter_box` locates it.
    fn find_cap(b: &super::super::super::LayoutBox) -> Option<&super::super::super::LayoutBox> {
        if super::super::super::is_first_letter_box(b) {
            return Some(b);
        }
        b.children.iter().find_map(find_cap)
    }

    fn find_rest_run(b: &super::super::super::LayoutBox) -> Option<&super::super::super::LayoutBox> {
        if super::super::super::is_first_letter_box(b) {
            return None;
        }
        if matches!(b.kind, super::super::super::BoxKind::InlineRun { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find_rest_run)
    }

    fn letter_seg(b: &super::super::super::LayoutBox) -> &super::super::super::InlineSegment {
        let super::super::super::BoxKind::InlineRun { segments, .. } = &b.children[0].kind else {
            panic!("cap inner box must be InlineRun");
        };
        &segments[0]
    }

    fn cap_height(b: &super::super::super::LayoutBox) -> f32 {
        match &b.style.height {
            Some(crate::style::Length::Px(p)) => *p,
            other => panic!("cap must reserve a fixed px height, got {other:?}"),
        }
    }

    #[test]
    fn element_initial_letter_extracts_cap() {
        // initial-letter on the element itself (no ::first-letter rule).
        let root = layout("<p>Hello world</p>", "p { initial-letter: 3; }");
        let cap = find_cap(&root).expect("initial-letter cap not created");
        assert_eq!(letter_seg(cap).text, "H");
        // Glyph enlarged to ~3 lines: font-size > base 16px.
        assert!(
            letter_seg(cap).style.font_size > 16.0,
            "cap font {} must be enlarged",
            letter_seg(cap).style.font_size,
        );
        assert!(cap_height(cap) > 0.0, "cap must reserve sink height");
        let rest = find_rest_run(&root).expect("rest run missing");
        let super::super::super::BoxKind::InlineRun { segments, .. } = &rest.kind else {
            unreachable!();
        };
        assert_eq!(segments[0].text, "ello world");
    }

    #[test]
    fn pseudo_initial_letter_extracts_cap() {
        // initial-letter via the ::first-letter pseudo-element.
        let root = layout("<p>Hello</p>", "p::first-letter { initial-letter: 2; }");
        let cap = find_cap(&root).expect("pseudo initial-letter cap not created");
        assert_eq!(letter_seg(cap).text, "H");
    }

    #[test]
    fn cap_narrows_following_text() {
        let root = layout("<p>Hello world</p>", "p { initial-letter: 3; }");
        let cap = find_cap(&root).expect("cap not created");
        let rest = find_rest_run(&root).expect("rest run missing");
        assert!(
            rest.rect.x >= cap.rect.x + cap.rect.width - 0.01,
            "rest x={} must start past cap right edge {}",
            rest.rect.x,
            cap.rect.x + cap.rect.width,
        );
    }

    #[test]
    fn explicit_sink_reserves_fewer_lines() {
        // `4 2`: glyph spans 4 lines but only 2 in-flow lines are reserved,
        // so the reserved height is smaller than the default sink=floor(4).
        let four = layout("<p>Hello world here we go again</p>", "p { initial-letter: 4; }");
        let four_two =
            layout("<p>Hello world here we go again</p>", "p { initial-letter: 4 2; }");
        let h4 = cap_height(find_cap(&four).expect("cap"));
        let h2 = cap_height(find_cap(&four_two).expect("cap"));
        assert!(h2 < h4, "sink 2 height {h2} must be < default sink height {h4}");
    }

    #[test]
    fn normal_value_leaves_no_cap() {
        let root = layout("<p>Hello world</p>", "p { initial-letter: normal; }");
        assert!(find_cap(&root).is_none(), "initial-letter:normal must not create a cap");
    }
}

#[test]
fn first_line_frags_marked_after_wrap() {
    // After lay_out, frags on lines[0] must have is_first_line = true;
    // frags on subsequent lines must have is_first_line = false.
    // Uses Fixed8 measurer (8px/char): "one two" = 7×8=56 ≤ 60px; "three" = 5×8=40,
    // 56+8+40=104 > 60 → wraps. 60px viewport ensures at least 2 lines.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let html = "<p>one two three four five</p>";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(60.0, 600.0),
        &Fixed8,
    );
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { lines, .. } = &run.kind {
        assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
        for frag in &lines[0] {
            assert!(frag.is_first_line, "line 0 frag must be is_first_line=true");
        }
        for line in lines.iter().skip(1) {
            for frag in line {
                assert!(!frag.is_first_line, "lines 1+ frags must be is_first_line=false");
            }
        }
    } else {
        panic!("expected InlineRun");
    }
}

// ::first-letter / ::first-line style application

#[test]
fn first_letter_style_applied_when_rule_present() {
    // ::first-letter { color: red } must change only the first segment's style.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let html = "<p>Hello world</p>";
    let css  = "p::first-letter { color: red; }";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(800.0, 600.0),
        &Fixed8,
    );
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        assert!(!segments.is_empty());
        // First segment (the single 'H' letter) must have red color.
        let red = crate::style::Color { r: 255, g: 0, b: 0, a: 255 };
        assert_eq!(
            segments[0].style.color, red,
            "::first-letter segment must have red color"
        );
        assert_eq!(segments[0].text, "H", "first-letter segment should be exactly 'H'");
        // Remaining segment keeps original (black) color.
        if segments.len() > 1 {
            assert_ne!(
                segments[1].style.color, red,
                "remainder segment must keep original color"
            );
        }
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_letter_no_rule_leaves_segment_unchanged() {
    // Without a ::first-letter rule the segment style must be unchanged.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let html = "<p>Hello</p>";
    let css  = "p { color: blue; }";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(800.0, 600.0),
        &Fixed8,
    );
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        // No split: single segment still contains full text.
        assert_eq!(segments[0].text, "Hello");
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_line_style_applied_to_first_line_frags() {
    // ::first-line { color: green } must change the style of frags on line 0 only.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // 60px wide container forces wrap: "one two" (56px) fits on line 0, rest wraps.
    let html = "<p>one two three four</p>";
    let css  = "p::first-line { color: green; }";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(60.0, 600.0),
        &Fixed8,
    );
    // After the ::first-line layout split (BB-1) the paragraph holds two
    // InlineRun boxes: the first formatted line and the remainder.
    fn collect_runs<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { collect_runs(c, out); }
    }
    let mut runs = Vec::new();
    collect_runs(&root, &mut runs);
    assert!(runs.len() >= 2, "expected split into 2 runs, got {}", runs.len());
    let green = crate::style::Color { r: 0, g: 128, b: 0, a: 255 };
    if let super::super::BoxKind::InlineRun { lines, .. } = &runs[0].kind {
        assert_eq!(lines.len(), 1, "first-line run must hold exactly one line");
        for frag in &lines[0] {
            assert_eq!(frag.style.color, green, "line 0 frag must have green color");
        }
    } else {
        panic!("expected InlineRun");
    }
    if let super::super::BoxKind::InlineRun { lines, .. } = &runs[1].kind {
        assert!(!lines.is_empty(), "remainder run must hold the wrapped lines");
        for line in lines {
            for frag in line {
                assert_ne!(frag.style.color, green, "remainder frags keep original color");
            }
        }
    } else {
        panic!("expected InlineRun");
    }
}

/// BUG-341 S23 gate — **by counter, on both arms**.
///
/// `apply_first_line_pseudo_styles` used to probe `::first-line` on every
/// block box of the document whether or not the sheet contained such a
/// rule; on `chrome.html` that is 123 probes and zero hits per interaction
/// cycle. The fix skips the walk, so the load-bearing assertion is a
/// **count of probes**, which no output diff can see.
///
/// The second arm is what makes the first one mean anything: the cheapest
/// way to drive the probe count to zero is to stop applying `::first-line`
/// altogether, so the same fixture with a rule must still probe *and* still
/// paint the first line green.
#[test]
fn bug341_s23_first_line_is_probed_only_by_a_sheet_that_uses_it() {
    let html = lumen_html_parser::parse("<p>one two three four</p>");
    let vp = lumen_core::geom::Size::new(60.0, 600.0);

    fn probes(css: &str, html: &lumen_dom::Document, vp: lumen_core::geom::Size)
        -> (u32, super::super::LayoutBox)
    {
        struct Fixed8;
        impl super::super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        crate::style::set_pseudo_cascade_diagnostics(true);
        let _ = crate::style::take_pseudo_cascade_sites();
        let root = super::super::layout_measured(html, &lumen_css_parser::parse(css), vp, &Fixed8);
        let sites = crate::style::take_pseudo_cascade_sites();
        crate::style::set_pseudo_cascade_diagnostics(false);
        (sites.get("first-line").map_or(0, |s| s.calls), root)
    }

    // Arm 1 — no `::first-line` anywhere in the sheet: not one probe.
    let (calls, _) = probes("p { color: blue; }", &html, vp);
    assert_eq!(calls, 0, "sheet without ::first-line must not probe for it");

    // Arm 2 — the rule is there: probed, and it still lands on line 0.
    let (calls, root) = probes("p::first-line { color: green; }", &html, vp);
    assert!(calls > 0, "sheet with ::first-line must still probe for it");
    fn collect_runs<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { collect_runs(c, out); }
    }
    let mut runs = Vec::new();
    collect_runs(&root, &mut runs);
    let green = crate::style::Color { r: 0, g: 128, b: 0, a: 255 };
    let first = runs.first().expect("expected an InlineRun");
    let super::super::BoxKind::InlineRun { lines, .. } = &first.kind else {
        panic!("expected InlineRun")
    };
    assert!(
        lines[0].iter().all(|f| f.style.color == green),
        "::first-line style must still reach the first line's frags",
    );
}

/// BUG-341 S25 gate — **by counter, on both arms**.
///
/// Deciding which formatting context a child joins used to run a full
/// `compute_style` per element child — up to three of them, since
/// `is_inline_content`, `is_inline_block` and the `display:none` re-probe
/// each cascaded the node again. `precompute_counters` had already cascaded
/// every one of them, and `build_box_inner` builds the child's box out of
/// *that* entry, so the probes were pure re-derivation: invisible in the
/// tree, and 0.21-0.25 ms of a 0.63 ms chrome keystroke. Arm 1 is therefore
/// a count, which no output diff can see.
///
/// Arm 2 is what makes arm 1 mean anything: the cheapest way to drive the
/// probe count to zero is to stop asking about `display` at all, and each
/// of the three questions decides the shape of the tree — so the fixture
/// exercises all three at once. An inline and an inline-block child share
/// one inline context (probes 1 and 2), a `display:none` child between them
/// does **not** close it (probe 3, CSS 2.1 §9.2.4), and a block child does.
#[test]
fn bug341_s25_display_probes_read_the_cascade_instead_of_re_running_it() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let doc = lumen_html_parser::parse(
        "<div id=host>text <span>inline</span> <b>ib</b> <i>gone</i> <span>after</span>\
         <p>block</p></div>",
    );
    let sheet = lumen_css_parser::parse(
        "span { display: inline; } b { display: inline-block; } \
         i { display: none; } p { display: block; }",
    );
    let vp = lumen_core::geom::Size::new(600.0, 600.0);

    let _ = super::super::take_box_build_stats();
    let root = super::super::layout_measured(&doc, &sheet, vp, &Fixed8);
    let stats = super::super::take_box_build_stats();

    // Arm 1 — every element the probes asked about is in the cascade cache,
    // so not one of them re-ran the cascade...
    assert_eq!(
        stats.display_probe_cascades, 0,
        "the box-build stage must read `display` off the cascade cache, \
         not re-run `compute_style` for it. Census: {stats:?}",
    );
    // ...and the questions are still being asked, which is the only reason
    // arm 1 can be satisfied honestly.
    assert!(
        stats.display_probes > 0,
        "the stage must still ask what each element child's `display` is.              Census: {stats:?}",
    );
    // The non-elements still miss (the cascade records elements only) —
    // asserted so a change that empties the cache entirely cannot pass arm 1
    // by making `style_arc` return `Some` for everything.
    assert!(
        stats.style_misses > 0,
        "whitespace text nodes have no cascade entry and must still fall back",
    );

    // Arm 2 — the three decisions those probes make, on the same document.
    fn kind_name(k: &super::super::BoxKind) -> &'static str {
        match k {
            super::super::BoxKind::InlineBlockRow => "InlineBlockRow",
            super::super::BoxKind::InlineRun { .. } => "InlineRun",
            super::super::BoxKind::Block => "Block",
            super::super::BoxKind::Skip => "Skip",
            _ => "other",
        }
    }
    fn find<'a>(b: &'a super::super::LayoutBox, doc: &lumen_dom::Document, id: &str)
        -> Option<&'a super::super::LayoutBox>
    {
        if doc.get(b.node).get_attr("id") == Some(id) {
            return Some(b);
        }
        b.children.iter().find_map(|c| find(c, doc, id))
    }
    let host = find(&root, &doc, "host").expect("#host must have a box");
    // Discriminant names only: a `BoxKind` `Debug` carries a full
    // `ComputedStyle` per inline fragment, which buries the assertion.
    let kinds: Vec<&'static str> = host.children.iter().map(|c| kind_name(&c.kind)).collect();
    assert_eq!(
        kinds.len(),
        2,
        "one inline context (probes 1-3) plus the block child, got {kinds:?}",
    );
    assert_eq!(
        kinds[0], "InlineBlockRow",
        "the inline and inline-block children share one row, got {kinds:?}",
    );
    assert_eq!(
        kinds[1], "Block",
        "the block child opens its own box, got {kinds:?}",
    );
    // `display:none` did not close the inline context: the text after it is
    // in the same row as the text before it.
    let texts = {
        fn runs(b: &super::super::LayoutBox, out: &mut Vec<String>) {
            if let super::super::BoxKind::InlineRun { segments, .. } = &b.kind {
                out.extend(segments.iter().map(|s| s.text.clone()));
            }
            for c in &b.children {
                runs(c, out);
            }
        }
        let mut out = Vec::new();
        runs(&host.children[0], &mut out);
        out.concat()
    };
    assert!(
        texts.contains("inline") && texts.contains("after"),
        "a display:none sibling must not split the inline context, got {texts:?}",
    );
    assert!(
        !texts.contains("gone"),
        "the display:none child must not contribute text, got {texts:?}",
    );
}

/// BUG-341 S25 — the `compute_style` fallback in `probe_display` still
/// answers when the cascade cache has no entry for the node.
///
/// The cache is populated by `precompute_counters`, which every entry point
/// runs; a miss means a caller holding a map that predates the node. That
/// path has no fixture in the pipeline, so it gets one here — otherwise
/// deleting it would be invisible until a real document hit it.
#[test]
fn bug341_s25_display_probe_falls_back_to_the_cascade_on_a_cache_miss() {
    let doc = lumen_html_parser::parse("<span id=s>x</span>");
    let sheet = lumen_css_parser::parse("span { display: inline-block; }");
    let vp = lumen_core::geom::Size::new(600.0, 600.0);
    let span = doc.find_by_id("s").expect("fixture has a <span id=s>");
    let empty = crate::counters::CounterMap::default();
    let inherited = crate::style::ComputedStyle::root();

    let _ = super::super::take_box_build_stats();
    assert_eq!(
        super::super::probe_display(&doc, &sheet, span, &inherited, vp, false, &empty),
        super::super::Display::InlineBlock,
        "an empty cache must fall back to a real cascade",
    );
    assert_eq!(
        {
            let st = super::super::take_box_build_stats();
            (st.display_probes, st.display_probe_cascades)
        },
        (1, 1),
        "one question asked, and with an empty cache it cost one cascade",
    );
}

#[test]
fn first_line_no_rule_frags_unchanged() {
    // Without a ::first-line rule, frag styles must be unchanged.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let html = "<p>one two three four</p>";
    let css  = "p { color: blue; }";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(60.0, 600.0),
        &Fixed8,
    );
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let run = find_run(&root).expect("InlineRun not found");
    let blue = crate::style::Color { r: 0, g: 0, b: 255, a: 255 };
    if let super::super::BoxKind::InlineRun { lines, .. } = &run.kind {
        // All frags across all lines must be blue (from `p { color: blue }`).
        for line in lines {
            for frag in line {
                assert_eq!(frag.style.color, blue, "all frags must keep blue color");
            }
        }
    } else {
        panic!("expected InlineRun");
    }
}

// ── BB-1: ::first-line layout split ──────────────────────────────────

/// 8px per char regardless of font size.
struct FixedW8;
impl super::super::super::TextMeasurer for FixedW8 {
    fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
}
/// Char width scales with font size (half the font size per char):
/// 16px font → 8px/char, 32px font → 16px/char.
struct HalfEm;
impl super::super::super::TextMeasurer for HalfEm {
    fn char_width(&self, _: char, size: f32) -> f32 { size / 2.0 }
}

/// All InlineRun boxes in tree order.
fn runs_of(root: &super::super::LayoutBox) -> Vec<&super::super::LayoutBox> {
    fn rec<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { rec(c, out); }
    }
    let mut v = Vec::new();
    rec(root, &mut v);
    v
}

/// All words of a run's positioned lines, in order.
fn run_words(run: &super::super::LayoutBox) -> Vec<String> {
    let super::super::BoxKind::InlineRun { lines, .. } = &run.kind else { return vec![] };
    lines
        .iter()
        .flatten()
        .flat_map(|f| f.text.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

/// All words of a run's source segments, in order.
fn segment_words(run: &super::super::LayoutBox) -> Vec<String> {
    let super::super::BoxKind::InlineRun { segments, .. } = &run.kind else { return vec![] };
    segments
        .iter()
        .flat_map(|s| s.text.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

#[test]
fn first_line_split_creates_two_runs() {
    // Wrapping paragraph with a ::first-line rule → two InlineRun boxes:
    // first formatted line + remainder; first_line_style cleared on both.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse("p::first-line { color: green; }"),
        lumen_core::geom::Size::new(60.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2, "expected exactly 2 runs after split");
    let super::super::BoxKind::InlineRun { lines, first_line_style, .. } = &runs[0].kind else {
        panic!("expected InlineRun");
    };
    assert_eq!(lines.len(), 1, "first-line run holds exactly one line");
    assert!(first_line_style.is_none(), "first_line_style must be cleared");
    let super::super::BoxKind::InlineRun { lines, first_line_style, .. } = &runs[1].kind else {
        panic!("expected InlineRun");
    };
    assert!(!lines.is_empty(), "remainder run holds the wrapped rest");
    assert!(first_line_style.is_none(), "first_line_style must be cleared");
}

#[test]
fn first_line_split_no_word_loss() {
    // Words across both runs must equal the source text exactly (no loss, no dupes).
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse("p::first-line { color: green; }"),
        lumen_core::geom::Size::new(60.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2);
    let mut words = run_words(runs[0]);
    words.extend(run_words(runs[1]));
    assert_eq!(words, ["one", "two", "three", "four"]);
}

#[test]
fn first_line_split_heights_and_positions() {
    // ::first-line { font-size: 32px } → first-line box is 32px-based tall,
    // remainder box starts right below it and uses the base 16px metrics.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse("p::first-line { font-size: 32px; }"),
        lumen_core::geom::Size::new(60.0, 600.0),
        &HalfEm,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2);
    assert!((runs[0].style.font_size - 32.0).abs() < 0.01, "first-line box gets fls font");
    let fl_h = 32.0 * runs[0].style.line_height;
    assert!(
        (runs[0].rect.height - fl_h).abs() < 0.01,
        "first-line box height {} != {}", runs[0].rect.height, fl_h,
    );
    assert!(
        (runs[1].rect.y - (runs[0].rect.y + fl_h)).abs() < 0.01,
        "remainder box must start right below the first-line box",
    );
    let base_h = runs[1].style.font_size * runs[1].style.line_height;
    let super::super::BoxKind::InlineRun { lines, .. } = &runs[1].kind else { panic!() };
    assert!(
        (runs[1].rect.height - lines.len() as f32 * base_h).abs() < 0.01,
        "remainder box height = lines × base line height",
    );
}

#[test]
fn first_line_bigger_font_wraps_earlier() {
    // Base 16px (8px/char): "one two" = 56px fits in 60px.
    // ::first-line 32px (16px/char): "one two" = 112px > 60px → only "one" fits.
    // The first line must be measured with the ::first-line font, not the base one.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse("p::first-line { font-size: 32px; }"),
        lumen_core::geom::Size::new(60.0, 600.0),
        &HalfEm,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2);
    assert_eq!(run_words(runs[0]), ["one"], "32px first line fits only one word");
    assert_eq!(run_words(runs[1]), ["two", "three", "four"]);
}

#[test]
fn first_line_single_line_restyled_in_place() {
    // Everything fits the first formatted line → no split; the run box itself
    // takes the ::first-line style so paint uses its font metrics.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two</p>"),
        &lumen_css_parser::parse("p::first-line { color: green; font-size: 32px; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 1, "single-line run must not be split");
    let green = crate::style::Color { r: 0, g: 128, b: 0, a: 255 };
    assert_eq!(runs[0].style.color, green, "run box restyled with ::first-line style");
    assert!((runs[0].style.font_size - 32.0).abs() < 0.01);
    let fl_h = 32.0 * runs[0].style.line_height;
    assert!((runs[0].rect.height - fl_h).abs() < 0.01, "height uses ::first-line metrics");
    let super::super::BoxKind::InlineRun { first_line_style, .. } = &runs[0].kind else { panic!() };
    assert!(first_line_style.is_none(), "first_line_style must be cleared");
}

#[test]
fn first_line_no_rule_no_split() {
    // Without a ::first-line rule the wrapping run stays a single box.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(60.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 1, "no rule → no split");
    let super::super::BoxKind::InlineRun { lines, .. } = &runs[0].kind else { panic!() };
    assert!(lines.len() >= 2, "text still wraps into multiple lines");
}

#[test]
fn first_line_split_segments_partitioned() {
    // Source segments are partitioned between the two boxes at the word
    // boundary; the remainder's first segment must not re-apply pre_space.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse("p::first-line { color: green; }"),
        lumen_core::geom::Size::new(60.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2);
    let mut words = segment_words(runs[0]);
    words.extend(segment_words(runs[1]));
    assert_eq!(words, ["one", "two", "three", "four"], "segments partitioned losslessly");
    let super::super::BoxKind::InlineRun { segments, .. } = &runs[1].kind else { panic!() };
    assert!(!segments.is_empty());
    assert_eq!(segments[0].pre_space, 0.0, "tail segment must not repeat pre_space");
}

#[test]
fn first_line_remainder_no_indent() {
    // text-indent applies to the first formatted line only: the first-line box
    // keeps the indent, the re-wrapped remainder starts at x = 0.
    let css = "p { text-indent: 16px; } p::first-line { color: green; }";
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(60.0, 600.0),
        &FixedW8,
    );
    let runs = runs_of(&root);
    assert_eq!(runs.len(), 2);
    let super::super::BoxKind::InlineRun { lines, .. } = &runs[0].kind else { panic!() };
    assert!((lines[0][0].x - 16.0).abs() < 0.01, "first line keeps text-indent");
    let super::super::BoxKind::InlineRun { lines, .. } = &runs[1].kind else { panic!() };
    assert!((lines[0][0].x).abs() < 0.01, "remainder lines start without indent");
}

// Phase 3: Nested SVG layout tests

#[test]
fn nested_svg_viewbox_scaling() {
    let html = r#"
        <svg viewBox="0 0 100 100" width="100" height="100">
            <rect x="0" y="0" width="50" height="50" />
            <svg viewBox="0 0 50 50" width="50" height="50" x="50" y="50">
                <rect x="0" y="0" width="25" height="25" />
            </svg>
        </svg>
    "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
    assert!(!root.children.is_empty());
}

#[test]
fn nested_svg_transform_composition() {
    let html = r#"
        <svg viewBox="0 0 100 100" width="100" height="100" transform="scale(2)">
            <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0" transform="translate(10, 10)">
                <rect x="0" y="0" width="25" height="25" />
            </svg>
        </svg>
    "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
    assert!(!root.children.is_empty());
}

#[test]
fn nested_svg_preserve_aspect_ratio() {
    let html = r#"
        <svg viewBox="0 0 100 100" width="100" height="100">
            <svg viewBox="0 0 100 50" width="100" height="100" preserveAspectRatio="xMidYMid meet" x="0" y="0">
                <rect x="0" y="0" width="100" height="50" />
            </svg>
        </svg>
    "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
    assert!(!root.children.is_empty());
}

#[test]
fn deeply_nested_svg_viewbox_cascade() {
    let html = r#"
        <svg viewBox="0 0 200 200" width="200" height="200">
            <svg viewBox="0 0 100 100" width="100" height="100" x="0" y="0">
                <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0">
                    <rect x="0" y="0" width="50" height="50" />
                </svg>
            </svg>
        </svg>
    "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(400.0, 400.0));
    assert!(!root.children.is_empty());
}

#[test]
fn nested_svg_group_with_transform() {
    let html = r#"
        <svg viewBox="0 0 100 100" width="100" height="100">
            <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0">
                <g transform="scale(2)">
                    <rect x="0" y="0" width="10" height="10" />
                </g>
            </svg>
        </svg>
    "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
    assert!(!root.children.is_empty());
}

#[test]
fn bug424_flex_centered_svg_use_path_ctm_tracks_alignment() {
    // BUG-424 (в): a flex-centered SVG icon (toolbar button, `align-items:
    // center`) whose content is a `<use>` onto a `<symbol viewBox>`. Before
    // the fix, `AlignValue::Center`'s `shift_y_box` moved `rect.y` to the
    // centered position but left the `Path` shape's `svg_paint_matrix`
    // (the CTM paint uses for rotated/skewed shapes, BUG-244) pinned to the
    // pre-alignment origin — drifting the two representations of the same
    // box out of sync by the alignment offset.
    let css = ".tb-btn{width:26px;height:26px;display:flex;align-items:center;justify-content:center;}";
    let html = r##"
        <div class="tb-btn"><svg width="14" height="14"><symbol id="i-back" viewBox="0 0 24 24"><polyline points="15 18 9 12 15 6"/></symbol><use href="#i-back"/></svg></div>
    "##;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, lumen_core::geom::Size::new(400.0, 400.0));
    fn find_svg_root(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::SvgRoot { .. }) { return Some(b); }
        b.children.iter().find_map(find_svg_root)
    }
    fn find_path_shape(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Path { .. }, .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find_path_shape)
    }
    let svg_root = find_svg_root(&root).expect("svg root box");
    // 26px button, 14px icon, align-items:center → icon top = (26-14)/2 = 6px
    // below the button's own top; confirms flex actually centered this box
    // (not just an assumption about layout details this test doesn't own).
    assert!((svg_root.rect.y - 14.0).abs() < 0.01, "expected centered svg root y=14, got {}", svg_root.rect.y);
    let path = find_path_shape(&root).expect("path shape box");
    let super::super::BoxKind::SvgShape { svg_paint_matrix, .. } = &path.kind else { unreachable!() };
    // The icon's own viewBox→viewport scale is centered (no letterboxing, tx=ty=0),
    // so the CTM's translation must equal the svg root's own document-space origin.
    assert!((svg_paint_matrix.matrix[4] - svg_root.rect.x).abs() < 0.01,
        "svg_paint_matrix tx should track svg root x={}, got {}", svg_root.rect.x, svg_paint_matrix.matrix[4]);
    assert!((svg_paint_matrix.matrix[5] - svg_root.rect.y).abs() < 0.01,
        "svg_paint_matrix ty should track svg root y={} (flex-centered), got {} — BUG-424 (в): shift_y_box didn't move the CTM",
        svg_root.rect.y, svg_paint_matrix.matrix[5]);
}

// ── ::first-letter / ::first-line CSS wiring ─────────────────────────────

fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
    for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
    None
}

#[test]
fn first_letter_style_override_splits_segment() {
    // p::first-letter { font-size: 3em } → segment "H" gets overridden style,
    // "ello world" becomes a separate segment with normal style.
    let css = "p::first-letter { font-size: 3em; }";
    let root = super::super::layout(
        &lumen_html_parser::parse("<p>Hello world</p>"),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        assert!(segments.len() >= 2, "expected split: got {} segment(s)", segments.len());
        assert_eq!(segments[0].text, "H", "first segment must be the first letter");
        assert_eq!(segments[0].pseudo_kind, super::super::PseudoKind::FirstLetter);
        // font-size 3em on the root = 3 × 16px = 48px.
        assert!(
            (segments[0].style.font_size - 48.0).abs() < 1.0,
            "first-letter font-size must be 3em, got {}", segments[0].style.font_size,
        );
        assert_eq!(segments[1].text, "ello world");
        assert_eq!(segments[1].pseudo_kind, super::super::PseudoKind::None);
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_letter_no_rule_leaves_segment_unsplit() {
    // No ::first-letter rule → segment stays marked but style is unchanged.
    let root = super::super::layout(
        &lumen_html_parser::parse("<p>Hello</p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        assert_eq!(segments.len(), 1, "no split without ::first-letter rule");
        assert_eq!(segments[0].pseudo_kind, super::super::PseudoKind::FirstLetter);
        assert_eq!(segments[0].text, "Hello");
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_letter_single_char_no_split() {
    // Single character: style override without splitting.
    let css = "p::first-letter { font-weight: bold; }";
    let root = super::super::layout(
        &lumen_html_parser::parse("<p>X</p>"),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { segments, .. } = &run.kind {
        assert_eq!(segments.len(), 1, "single char: no extra segment");
        assert_eq!(segments[0].text, "X");
        assert_eq!(segments[0].pseudo_kind, super::super::PseudoKind::FirstLetter);
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_line_style_override_applied_to_first_line_frags() {
    // p::first-line { color: #ff0000 } → frags on lines[0] get red color.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let css = "p::first-line { color: #ff0000; }";
    // 60px wide → "one two" (56px) on line 0, "three" wraps to line 1.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(60.0, 600.0),
        &Fixed8,
    );
    // After the ::first-line layout split (BB-1): runs[0] = first line (red),
    // runs[1] = remainder (original color).
    fn collect_runs<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { collect_runs(c, out); }
    }
    let mut runs = Vec::new();
    collect_runs(&root, &mut runs);
    assert!(runs.len() >= 2, "expected split into 2 runs, got {}", runs.len());
    if let super::super::BoxKind::InlineRun { lines, .. } = &runs[0].kind {
        assert_eq!(lines.len(), 1, "first-line run must hold exactly one line");
        for frag in &lines[0] {
            assert!(
                frag.style.color.r > 200,
                "first-line frags must have red color (r={})", frag.style.color.r,
            );
        }
    } else {
        panic!("expected InlineRun");
    }
    if let super::super::BoxKind::InlineRun { lines, .. } = &runs[1].kind {
        for line in lines {
            for frag in line {
                assert!(
                    frag.style.color.r < 50,
                    "non-first-line frags must NOT be red (r={})", frag.style.color.r,
                );
            }
        }
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn first_letter_keeps_enclosing_inline_style() {
    // BUG-100: `<p><em>Bravo</em>…</p>` with `p::first-letter { color }` —
    // the letter inherits the `<em>`'s italic (§3.4) and takes only the
    // pseudo-element's own declarations.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p><em>Bravo</em> beta gamma</p>"),
        &lumen_css_parser::parse("p::first-letter { color: #ff0000; }"),
        lumen_core::geom::Size::new(400.0, 600.0),
        &Fixed8,
    );
    let run = find_run(&root).expect("InlineRun not found");
    let super::super::BoxKind::InlineRun { segments, .. } = &run.kind else {
        panic!("expected InlineRun");
    };
    let fl = segments
        .iter()
        .find(|s| s.pseudo_kind == super::super::PseudoKind::FirstLetter)
        .expect("::first-letter segment");
    assert_eq!(fl.text, "B");
    assert_eq!(
        fl.style.font_style,
        crate::style::FontStyle::Italic,
        "::first-letter must keep the enclosing <em>'s font-style",
    );
    assert!(fl.style.color.r > 200, "…while taking ::first-letter's own color");
    let rest = &segments[1];
    assert_eq!(rest.text, "ravo");
    assert_eq!(
        rest.style.font_style,
        crate::style::FontStyle::Italic,
        "the tail of the split segment stays inside the <em>",
    );
}

#[test]
fn first_line_does_not_clobber_inner_bold() {
    // BUG-100: CSS Pseudo-elements L4 §3.4 — ::first-line is the *parent* of
    // the first line's content, so it supplies only what the fragment
    // inherited. `<b>`'s own font-weight must survive; the color must not.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one <b>two</b> three four</p>"),
        &lumen_css_parser::parse("p::first-line { color: #ff0000; }"),
        lumen_core::geom::Size::new(80.0, 600.0),
        &Fixed8,
    );
    fn collect_runs<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { out.push(b); }
        for c in &b.children { collect_runs(c, out); }
    }
    let mut runs = Vec::new();
    collect_runs(&root, &mut runs);
    let frags: Vec<&super::super::InlineFrag> =
        runs.iter().flat_map(|r| match &r.kind {
            super::super::BoxKind::InlineRun { lines, .. } => lines.iter().flatten(),
            _ => unreachable!(),
        }).filter(|f| f.is_first_line).collect();
    let bold = frags.iter().find(|f| f.text.contains("two")).expect("`two` frag on first line");
    assert_eq!(
        bold.style.font_weight,
        crate::style::FontWeight::BOLD,
        "<b> inside ::first-line must keep its own font-weight",
    );
    assert!(bold.style.color.r > 200, "…while still inheriting ::first-line's color");
}

#[test]
fn first_line_no_rule_leaves_frags_unstyled() {
    // No ::first-line rule → is_first_line is true but style is unchanged.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<p>one two three four</p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(60.0, 600.0),
        &Fixed8,
    );
    let run = find_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { lines, .. } = &run.kind {
        assert!(lines.len() >= 2, "expected wrapping");
        // Verify is_first_line is still set (layout infrastructure works).
        assert!(lines[0].iter().all(|f| f.is_first_line), "first line must be marked");
        assert!(lines[1..].iter().flatten().all(|f| !f.is_first_line), "rest not marked");
    } else {
        panic!("expected InlineRun");
    }
}

// ── CSS Pseudo-elements L4 §14.2 — ::marker tests ────────────────────────

// ── CSS Generated Content L3 §3.2 — open-quote / close-quote ─────────────

#[test]
fn quotes_nested_q_uses_primary_then_secondary() {
    // Nested <q> → outer uses primary “ ”, inner uses secondary ‘ ’.
    let root = super::super::layout(
        &lumen_html_parser::parse("<p><q>outer <q>inner</q> end</q></p>"),
        &lumen_css_parser::parse(
            "q::before{content:open-quote} q::after{content:close-quote}",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    super::collect_seg_text(&root, &mut text);
    let po = text.find('\u{201C}').expect("primary open quote");
    let so = text.find('\u{2018}').expect("secondary open quote");
    let sc = text.find('\u{2019}').expect("secondary close quote");
    let pc = text.find('\u{201D}').expect("primary close quote");
    assert!(po < so && so < sc && sc < pc, "quote nesting order wrong: {text:?}");
}

#[test]
fn quotes_custom_pairs_applied() {
    let root = super::super::layout(
        &lumen_html_parser::parse("<q>hi</q>"),
        &lumen_css_parser::parse(
            "q{quotes:\"\u{ab}\" \"\u{bb}\"} q::before{content:open-quote} q::after{content:close-quote}",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    super::collect_seg_text(&root, &mut text);
    assert!(text.contains('\u{ab}') && text.contains('\u{bb}'), "custom quotes: {text:?}");
}

#[test]
fn quotes_none_suppresses_marks() {
    let root = super::super::layout(
        &lumen_html_parser::parse("<q>hi</q>"),
        &lumen_css_parser::parse(
            "q{quotes:none} q::before{content:open-quote} q::after{content:close-quote}",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    super::collect_seg_text(&root, &mut text);
    assert!(
        !text.contains('\u{201C}') && !text.contains('\u{201D}'),
        "quotes:none must emit no marks: {text:?}",
    );
}

/// Concatenates the post-`wrap_inline_run` fragment text of every InlineRun
/// in document order. Unlike `collect_seg_text` (pre-wrap segments), this
/// reflects how adjacent inline boxes were joined — tightly or with a single
/// collapsed space (CSS Text L3 §4.1.1).
fn collect_frag_text(b: &super::super::LayoutBox, out: &mut String) {
    if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind {
        for line in lines {
            for f in line {
                out.push_str(&f.text);
            }
        }
    }
    for c in &b.children {
        collect_frag_text(c, out);
    }
}

#[test]
fn bug216_open_close_quote_abut_quoted_text() {
    // BUG-216: open-quote / close-quote glue to the quoted text with no
    // inter-word space (the ::before/::after content and the text share the
    // <q> inline box; no source whitespace separates them).
    let root = super::super::layout(
        &lumen_html_parser::parse("<p><q>auto quotes</q></p>"),
        &lumen_css_parser::parse(
            "q::before{content:open-quote} q::after{content:close-quote}",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    collect_frag_text(&root, &mut text);
    assert_eq!(
        text, "\u{201C}auto quotes\u{201D}",
        "quotes must abut the quoted text: {text:?}",
    );
}

#[test]
fn bug216_adjacent_inline_boxes_join_tight() {
    // BUG-216: no source whitespace between inline boxes → no spurious space.
    let root = super::super::layout(
        &lumen_html_parser::parse("<p><span>foo</span><span>bar</span></p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    collect_frag_text(&root, &mut text);
    assert_eq!(text, "foobar", "adjacent inline boxes must not gain a space: {text:?}");
}

#[test]
fn bug216_inter_box_whitespace_collapses_to_one_space() {
    // The regression guard's complement: a whitespace-only text node between
    // inline boxes must still collapse to exactly one space (CSS Text L3 §4.1.1).
    let root = super::super::layout(
        &lumen_html_parser::parse("<p><span>foo</span> <span>bar</span></p>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut text = String::new();
    collect_frag_text(&root, &mut text);
    assert_eq!(text, "foo bar", "collapsed inter-box whitespace must remain one space: {text:?}");
}

