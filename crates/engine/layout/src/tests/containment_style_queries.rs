use super::*;

    // ── CSS Containment L3 enforcement ──────────────────────────────────────

    /// contain:size → auto height = 0 (children don't contribute).
    #[test]
    fn contain_size_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; } p { height: 50px; }",
            200.0,
        );
        let div = first_element_child(&root);
        // Explicit p height = 50px, but div has contain:size → div height = 0
        // (only padding+border, which are both 0 here).
        assert_eq!(div.rect.height, 0.0, "contain:size → auto height must be 0, got {}", div.rect.height);
    }

    /// contain:size with explicit height — explicit wins, children still don't contribute.
    #[test]
    fn contain_size_explicit_height_wins() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; height: 80px; } p { height: 100px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert!((div.rect.height - 80.0).abs() < 0.5, "contain:size with explicit height=80, got {}", div.rect.height);
    }

    /// contain:layout parses and stores correctly.
    #[test]
    fn contain_layout_stores_flag() {
        let root = lay("<div></div>", "div { contain: layout; }");
        let div = first_element_child(&root);
        assert!(
            div.style.contain.0 & ContainFlags::LAYOUT.0 != 0,
            "contain:layout flag not set"
        );
    }

    /// contain:strict = size + layout + style + paint → auto height = 0.
    #[test]
    fn contain_strict_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>text</p></div>",
            "div { contain: strict; } p { height: 60px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert_eq!(div.rect.height, 0.0, "contain:strict → auto height must be 0, got {}", div.rect.height);
    }

    // ── CSS Container Style Queries (Phase 0) ──────────────────────────────

    pub(crate) fn style_ctx(props: &[(&str, &str)]) -> crate::ContainerContext {
        style_ctx_with_style_props(props, &[])
    }

    pub(crate) fn style_ctx_with_style_props(
        custom_props: &[(&str, &str)],
        style_props: &[(&str, &str)],
    ) -> crate::ContainerContext {
        let mut custom = std::collections::HashMap::new();
        for (k, v) in custom_props {
            custom.insert(k.to_string(), v.to_string());
        }
        let mut style = std::collections::HashMap::new();
        for (k, v) in style_props {
            style.insert(k.to_string(), v.to_string());
        }
        crate::ContainerContext {
            width: 200.0,
            height: Some(100.0),
            names: vec![],
            custom_props: custom.into(),
            style_props: style,
            font_size: 16.0,
            viewport: lumen_core::Size::new(1024.0, 768.0),
            own_containing_block_height: 100.0,
            kinds: crate::ContainerKinds::SIZE,
            scroll_state: crate::ScrollStateInfo::default(),
            parent: None,
            rule_hits: Vec::new(),
        }
    }

    #[test]
    fn style_query_with_value_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_missing() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_boolean_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_boolean_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_with_extra_spaces() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme:  dark )", &ctx));
    }

    #[test]
    fn style_query_not() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(crate::evaluate_container_condition("not style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_unset_returns_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(width: 100px)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_matches() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: flex)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: block)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_keyword_case_insensitive() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: FLEX)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_true_when_set() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_false_when_unset() {
        let ctx = style_ctx_with_style_props(&[], &[]);
        assert!(!crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_color_keyword_matches_computed_rgb() {
        // Container's computed style is already serialized as `rgb(...)`
        // (getComputedStyle form); the query uses the author's keyword.
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(crate::evaluate_container_condition("style(color: red)", &ctx));
    }

    #[test]
    fn style_query_color_hex_matches_computed_rgb() {
        let ctx = style_ctx_with_style_props(&[], &[("background-color", "rgb(0, 0, 255)")]);
        assert!(crate::evaluate_container_condition(
            "style(background-color: #0000ff)",
            &ctx
        ));
    }

    #[test]
    fn style_query_color_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(!crate::evaluate_container_condition("style(color: blue)", &ctx));
    }

    #[test]
    fn style_query_non_color_value_mismatch_still_returns_false() {
        // A non-color, non-matching value must not be coerced into matching.
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: grid)", &ctx));
    }

    #[test]
    fn style_query_length_pt_matches_computed_px() {
        // Container's computed style is serialized in px; the query uses `pt`.
        let ctx = style_ctx_with_style_props(&[], &[("border-top-width", "2.6667px")]);
        assert!(crate::evaluate_container_condition(
            "style(border-top-width: 2pt)",
            &ctx
        ));
    }

    #[test]
    fn style_query_length_in_matches_computed_px() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(crate::evaluate_container_condition("style(width: 1in)", &ctx));
    }

    #[test]
    fn style_query_length_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2in)", &ctx));
    }

    #[test]
    fn style_query_em_matches_computed_px() {
        // `style_ctx` has font_size: 16.0 → `1em` resolves to 16px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(crate::evaluate_container_condition("style(width: 1em)", &ctx));
    }

    #[test]
    fn style_query_em_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2em)", &ctx));
    }

    #[test]
    fn style_query_percent_matches_computed_px() {
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "100px")]);
        assert!(crate::evaluate_container_condition("style(width: 50%)", &ctx));
    }

    #[test]
    fn style_query_line_height_percent_uses_font_size_basis_not_width() {
        // `style_ctx` has font_size: 16.0, width: 200.0. `line-height: 150%`
        // must resolve against font-size (24px), not width (300px).
        let ctx = style_ctx_with_style_props(&[], &[("line-height", "24px")]);
        assert!(crate::evaluate_container_condition("style(line-height: 150%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(line-height: 50%)", &ctx));
    }

    #[test]
    fn style_query_height_percent_uses_height_basis_not_width() {
        // `style_ctx` has height: Some(100.0), width: 200.0. `height: 50%`
        // must resolve against height (50px), not width (100px).
        let ctx = style_ctx_with_style_props(&[], &[("height", "50px")]);
        assert!(crate::evaluate_container_condition("style(height: 50%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 100%)", &ctx));
    }

    #[test]
    fn style_query_top_percent_uses_height_basis() {
        let ctx = style_ctx_with_style_props(&[], &[("top", "25px")]);
        assert!(crate::evaluate_container_condition("style(top: 25%)", &ctx));
    }

    #[test]
    fn style_query_height_basis_is_own_containing_block_not_own_height() {
        // Even when the container is `container-type: inline-size` (its own
        // `height` is unknown, mirroring `ctx.height: None`), `%` in a
        // `height`/`top` style() query must resolve against the container's
        // *own* containing block height — not fall back to width like the
        // Phase 0 gap used to (see CSS-SPECS.md T3 Container Queries).
        let mut ctx = style_ctx_with_style_props(&[], &[("height", "60px")]);
        ctx.height = None;
        ctx.own_containing_block_height = 300.0;
        assert!(crate::evaluate_container_condition("style(height: 20%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 30%)", &ctx),
            "30% of the containing block (300px) is 90px, not 60px — must not fall back to width (200px)");
    }

    #[test]
    fn style_query_margin_top_percent_still_uses_width_basis() {
        // CSS2.1 §8.3: vertical margin/padding percentages resolve against
        // the containing block *width*, not height — unlike `height`/`top`.
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("margin-top", "100px")]);
        assert!(crate::evaluate_container_condition("style(margin-top: 50%)", &ctx));
    }

    #[test]
    fn style_query_viewport_unit_matches_computed_px() {
        // `style_ctx` has viewport: 1024x768 → `10vw` resolves to 102.4px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "102.4px")]);
        assert!(crate::evaluate_container_condition("style(width: 10vw)", &ctx));
    }

    #[test]
    fn style_query_value_internal_whitespace_normalized() {
        // Container declares `1px  2px` (two spaces); query uses a single space.
        let ctx = style_ctx(&[("--gap", "1px  2px")]);
        assert!(crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_no_space_matches_spaced() {
        // Query without a space after the colon matches a spaced container value.
        let ctx = style_ctx(&[("--gap", "1px 2px")]);
        assert!(crate::evaluate_container_condition("style(--gap:1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_comma_whitespace_normalized() {
        // `a, b` (container) equals `a,b` (query) after comma-space normalization.
        let ctx = style_ctx(&[("--list", "a, b")]);
        assert!(crate::evaluate_container_condition("style(--list: a,b)", &ctx));
    }

    #[test]
    fn style_query_value_whitespace_difference_still_distinguishes_tokens() {
        // Normalization must not merge distinct tokens: `1px2px` != `1px 2px`.
        let ctx = style_ctx(&[("--gap", "1px2px")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_resolves() {
        // Container's `--gap` references `--base` via var() — resolved before compare.
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_mismatch() {
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 4px)", &ctx));
    }

    #[test]
    fn style_query_var_unresolved_reference_is_false() {
        // `--gap` references an undeclared custom property with no fallback.
        let ctx = style_ctx(&[("--gap", "var(--missing)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_boolean_form_resolves() {
        let ctx = style_ctx(&[("--base", "dark"), ("--theme", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_var_fallback_used() {
        // `--gap` references an undeclared property, but with a fallback value.
        let ctx = style_ctx(&[("--gap", "var(--missing, 8px)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }
