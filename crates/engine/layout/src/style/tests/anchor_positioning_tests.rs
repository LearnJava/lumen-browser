//! Тесты `style.rs`: CSS Anchor Positioning L1.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─────────────────────────────────────────────────────────────────────────────
// CSS Anchor Positioning L1 tests
// ─────────────────────────────────────────────────────────────────────────────

    use super::*;
    use crate::anchor::InsetAreaKeyword;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn first_div_style(html: &str, css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children[0];
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    #[test]
    fn anchor_name_parsed() {
        let s = first_div_style("<div></div>", "div { anchor-name: --btn; }");
        assert_eq!(s.anchor_name.as_deref(), Some("--btn"));
    }

    #[test]
    fn anchor_name_none_clears() {
        let s = first_div_style("<div></div>", "div { anchor-name: none; }");
        assert!(s.anchor_name.is_none());
    }

    #[test]
    fn position_anchor_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-anchor: --tooltip-anchor; }",
        );
        assert_eq!(s.position_anchor.as_deref(), Some("--tooltip-anchor"));
    }

    #[test]
    fn inset_area_two_keywords() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset-area: end start; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::End);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::Start);
    }

    #[test]
    fn inset_area_single_keyword_sets_both_axes() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset-area: center; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::Center);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::Center);
    }

    #[test]
    fn position_area_alias_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-area: start end; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::Start);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::End);
    }

    // ── CSS Anchor Positioning L1 §3.1 — `anchor()` function ─────────────────

    #[test]
    fn anchor_func_side_only_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-anchor: --btn; top: anchor(bottom); }",
        );
        let f = s.anchor_top.as_ref().expect("anchor() not parsed");
        assert!(f.anchor_name.is_none());
        assert_eq!(f.side, crate::anchor::AnchorSide::Bottom);
        assert!(f.fallback.is_none());
        assert!(s.top.is_auto(), "plain top must stay auto when anchor() is used");
    }

    #[test]
    fn anchor_func_explicit_name_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; left: anchor(--btn right); }",
        );
        let f = s.anchor_left.as_ref().expect("anchor() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--btn"));
        assert_eq!(f.side, crate::anchor::AnchorSide::Right);
    }

    #[test]
    fn anchor_func_percentage_side_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; left: anchor(25%); }",
        );
        let f = s.anchor_left.as_ref().expect("anchor() not parsed");
        assert_eq!(f.side, crate::anchor::AnchorSide::Percentage(25.0));
    }

    #[test]
    fn anchor_func_fallback_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; bottom: anchor(top, 10px); }",
        );
        let f = s.anchor_bottom.as_ref().expect("anchor() not parsed");
        assert_eq!(f.side, crate::anchor::AnchorSide::Top);
        assert_eq!(f.fallback, Some(Length::Px(10.0)));
    }

    #[test]
    fn anchor_func_cleared_by_plain_length() {
        // A later declaration without anchor() must clear the previously
        // parsed AnchorFunc (mirrors width/anchor-size() interception).
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; top: anchor(bottom); top: 5px; }",
        );
        assert!(s.anchor_top.is_none());
        assert_eq!(s.top, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn anchor_func_inset_shorthand_applies_to_all_sides() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset: anchor(top); }",
        );
        assert!(s.anchor_top.is_some());
        assert!(s.anchor_right.is_some());
        assert!(s.anchor_bottom.is_some());
        assert!(s.anchor_left.is_some());
    }

    #[test]
    fn anchor_name_not_inherited() {
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { anchor-name: --parent; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert_eq!(div_style.anchor_name.as_deref(), Some("--parent"));
        assert!(span_style.anchor_name.is_none(), "anchor-name must not be inherited");
    }

    // ── CSS View Transitions L1 ───────────────────────────────────────────────

    #[test]
    fn view_transition_name_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: hero; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_transition_name.as_deref(), Some("hero"));
    }

    #[test]
    fn view_transition_name_none_clears() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: none; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert!(s.view_transition_name.is_none());
    }

    #[test]
    fn view_transition_name_default_is_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert!(s.view_transition_name.is_none());
    }

    #[test]
    fn view_transition_name_not_inherited() {
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { view-transition-name: parent-el; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert_eq!(div_style.view_transition_name.as_deref(), Some("parent-el"));
        assert!(span_style.view_transition_name.is_none(), "view-transition-name must not be inherited");
    }

    #[test]
    fn view_transition_name_dashed_ident() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: --my-element; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_transition_name.as_deref(), Some("--my-element"));
    }

    // ── CSS Scroll-Driven Animations ─────────────────────────────────────────

    #[test]
    fn scroll_timeline_name_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { scroll-timeline-name: --my-tl; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_name.as_deref(), Some("--my-tl"));
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::Block);
    }

    #[test]
    fn scroll_timeline_axis_inline() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "div { scroll-timeline-name: --t; scroll-timeline-axis: inline; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::Inline);
    }

    #[test]
    fn scroll_timeline_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { scroll-timeline: --tl x; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_name.as_deref(), Some("--tl"));
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::X);
    }

    #[test]
    fn view_timeline_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-timeline: --vt y; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_timeline_name.as_deref(), Some("--vt"));
        assert_eq!(s.view_timeline_axis, ScrollAxis::Y);
    }

    #[test]
    fn animation_timeline_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: auto; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.animation_timelines, vec![AnimationTimeline::Auto]);
    }

    #[test]
    fn animation_timeline_scroll_fn() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: scroll(inline root); }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::Scroll { axis: ScrollAxis::Inline, nearest: false }]
        );
    }

    #[test]
    fn animation_timeline_view_fn() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: view(inline); }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::View { axis: ScrollAxis::Inline }]
        );
    }

    #[test]
    fn animation_timeline_named() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: --my-scroll; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::Named("--my-scroll".into())]
        );
    }

    // ── border-collapse ────────────────────────────────────────────────────────

    #[test]
    fn p4_border_collapse_default_is_separate() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate);
    }

    #[test]
    fn p4_border_collapse_parse_collapse() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: collapse; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Collapse);
    }

    #[test]
    fn p4_border_collapse_parse_separate_explicit() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: separate; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate);
    }

    #[test]
    fn p4_border_collapse_inherited_by_cells() {
        // border-collapse is inherited: td should see the table's collapse value.
        // Must walk the ancestor chain: root → body → table → (tbody) → tr → td.
        let doc = lumen_html_parser::parse("<table><tr><td>x</td></tr></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: collapse; }");
        let root_style = ComputedStyle::root();
        let body_node = doc.body().expect("body");
        let body_style = compute_style(&doc, body_node, &sheet, &root_style, VP, false);
        // Walk children to find the table, then tbody/tr, then td.
        fn find_tag(doc: &lumen_dom::Document, parent: lumen_dom::NodeId, tag_name: &str) -> Option<lumen_dom::NodeId> {
            for &c in &doc.get(parent).children {
                if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c).data
                    && name.local == tag_name
                {
                    return Some(c);
                }
                if let Some(found) = find_tag(doc, c, tag_name) { return Some(found); }
            }
            None
        }
        let table = find_tag(&doc, body_node, "table").expect("table");
        let table_style = compute_style(&doc, table, &sheet, &body_style, VP, false);
        assert_eq!(table_style.border_collapse, BorderCollapse::Collapse, "table should have collapse");
        // TD inherits via tr; tr uses table_style (or intermediate row-group).
        let tr = find_tag(&doc, table, "tr").expect("tr");
        let tr_style = compute_style(&doc, tr, &sheet, &table_style, VP, false);
        let td = find_tag(&doc, tr, "td").expect("td");
        let td_style = compute_style(&doc, td, &sheet, &tr_style, VP, false);
        assert_eq!(td_style.border_collapse, BorderCollapse::Collapse, "td inherits collapse from table");
    }

    #[test]
    fn p4_border_collapse_initial_via_keyword() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse(
            "table { border-collapse: collapse; } table { border-collapse: initial; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate, "initial resets to Separate");
    }

    // ── empty-cells ────────────────────────────────────────────────────────────

    #[test]
    fn p4_empty_cells_default_is_show() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show);
    }

    #[test]
    fn p4_empty_cells_parse_hide() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: hide; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Hide);
    }

    #[test]
    fn p4_empty_cells_parse_show_explicit() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: show; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show);
    }

    #[test]
    fn p4_empty_cells_inherited_by_cells() {
        // empty-cells is inherited: td should see the table's hide value.
        let doc = lumen_html_parser::parse("<table><tr><td>x</td></tr></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: hide; }");
        let root_style = ComputedStyle::root();
        let body_node = doc.body().expect("body");
        let body_style = compute_style(&doc, body_node, &sheet, &root_style, VP, false);
        fn find_tag(doc: &lumen_dom::Document, parent: lumen_dom::NodeId, tag_name: &str) -> Option<lumen_dom::NodeId> {
            for &c in &doc.get(parent).children {
                if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c).data
                    && name.local == tag_name
                {
                    return Some(c);
                }
                if let Some(found) = find_tag(doc, c, tag_name) { return Some(found); }
            }
            None
        }
        let table = find_tag(&doc, body_node, "table").expect("table");
        let table_style = compute_style(&doc, table, &sheet, &body_style, VP, false);
        let tr = find_tag(&doc, table, "tr").expect("tr");
        let tr_style = compute_style(&doc, tr, &sheet, &table_style, VP, false);
        let td = find_tag(&doc, tr, "td").expect("td");
        let td_style = compute_style(&doc, td, &sheet, &tr_style, VP, false);
        assert_eq!(td_style.empty_cells, EmptyCells::Hide, "td inherits hide from table");
    }

    #[test]
    fn p4_empty_cells_initial_via_keyword() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse(
            "table { empty-cells: hide; } table { empty-cells: initial; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show, "initial resets to Show");
    }

    #[test]
    fn p4_empty_cells_keyword_parse() {
        assert_eq!(EmptyCells::parse("show"), Some(EmptyCells::Show));
        assert_eq!(EmptyCells::parse("hide"), Some(EmptyCells::Hide));
        assert_eq!(EmptyCells::parse("bogus"), None);
    }

    // ── StyleEnvSnapshot (ADR-016 M4.1) ──────────────────────────────────────

    #[test]
    fn style_env_snapshot_captures_and_restores_interactive_state() {
        // Set a non-default interactive state, capture it, reset, then install
        // the snapshot and verify the state is restored correctly.
        use lumen_dom::NodeId;

        let hover  = NodeId::from_index(42);
        let focus  = NodeId::from_index(7);
        let active = NodeId::from_index(99);

        set_interactive_state(Some(hover), Some(focus), Some(active));
        set_forced_colors(true);
        set_print_media(true);

        let snap = StyleEnvSnapshot::capture();

        // Reset to defaults on this thread.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);

        assert!(!forced_colors_active());
        assert!(!print_media_active());

        // Install snapshot — must restore the captured values.
        snap.install();

        assert!(forced_colors_active(), "forced colors must be restored");
        assert!(print_media_active(), "print media must be restored");
        assert_eq!(
            HOVER_NID.with(Cell::get),
            hover.index() as u32,
            "hover nid must be restored",
        );
        assert_eq!(
            FOCUS_NID.with(Cell::get),
            focus.index() as u32,
            "focus nid must be restored",
        );
        assert_eq!(
            ACTIVE_NID.with(Cell::get),
            active.index() as u32,
            "active nid must be restored",
        );

        // Cleanup so later tests on this thread see the screen default.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);
    }

    #[test]
    fn style_env_snapshot_default_state_is_clean() {
        // A snapshot captured in the default (no interactive state) condition
        // must reinstall u32::MAX for all nids and false for bool flags.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);

        let snap = StyleEnvSnapshot::capture();

        // Corrupt thread state briefly, then restore via install.
        HOVER_NID.with(|h| h.set(1));
        FORCED_COLORS.with(|f| f.set(true));

        snap.install();

        assert_eq!(HOVER_NID.with(Cell::get), u32::MAX, "hover must be MAX");
        assert!(!forced_colors_active(), "forced colors must be false");
    }

