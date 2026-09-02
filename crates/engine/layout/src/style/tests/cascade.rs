//! Тесты `style.rs`: каскад: `@layer`, `revert`, `@supports`, инлайновый `style`, `zoom`,
//! псевдоэлементы.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ──────────────── CSS Cascade L4 §6.4.3: inline style attribute ────────────────

    #[test]
    fn inline_style_background_applies() {
        // Базовая проверка BUG-003 fix — inline `style="background: ..."`
        // должен подключаться к каскаду и давать цветной фон.
        let s = cascade_at(
            r#"<div style="background: red;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.background_color, Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })));
    }

    #[test]
    fn inline_style_overrides_class_rule() {
        // CSS Cascade L4 §6.4.3: inline побеждает любой селектор в author origin.
        let s = cascade_at(
            r#"<div class="k" style="color: blue;">x</div>"#,
            ".k { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn inline_style_overrides_id_rule() {
        // Inline побеждает даже ID-селектор, чья specificity (1,0,0) выше
        // class (0,1,0): inline-tier приоритетнее specificity.
        let s = cascade_at(
            r#"<div id="x" style="color: blue;">x</div>"#,
            "#x { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn inline_style_important_beats_class_important() {
        // Inline !important побеждает class !important (равная Importance,
        // разные тиры — Element-Attached Styles побеждает в author!important).
        let s = cascade_at(
            r#"<div class="k" style="color: blue !important;">x</div>"#,
            ".k { color: red !important; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn class_important_beats_inline_normal() {
        // Author !important побеждает author normal (включая inline normal),
        // потому что Importance — главный sort-критерий.
        let s = cascade_at(
            r#"<div class="k" style="color: blue;">x</div>"#,
            ".k { color: red !important; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn inline_style_multiple_properties() {
        let s = cascade_at(
            r#"<div style="background: green; color: yellow; padding: 5px;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.background_color, Some(CssColor::Rgba(Color { r: 0, g: 128, b: 0, a: 255 })));
        assert_eq!(s.color, Color { r: 255, g: 255, b: 0, a: 255 });
        assert_eq!(s.padding_top, Length::Px(5.0));
        assert_eq!(s.padding_right, Length::Px(5.0));
        assert_eq!(s.padding_bottom, Length::Px(5.0));
        assert_eq!(s.padding_left, Length::Px(5.0));
    }

    #[test]
    fn inline_style_display_none_hides_element() {
        // BUG-001 manifestation: `style="display:none"` через inline должен
        // ставить display = None.
        let s = cascade_at(
            r#"<div style="display: none;">hidden</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.display, Display::None);
    }

    #[test]
    fn inline_style_empty_attribute_is_noop() {
        // Пустой `style=""` не ломает каскад; class-rule остаётся в силе.
        let s = cascade_at(
            r#"<div class="k" style="">x</div>"#,
            ".k { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn inline_style_invalid_declaration_skipped() {
        // Невалидное declaration пропускается (recovery в parse_inline_style),
        // валидные применяются.
        let s = cascade_at(
            r#"<div style="garbage no colon; color: blue;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    // ─── @layer cascade ordering (CSS Cascade L5 §6.4.5) ─────────────────────

    #[test]
    fn at_layer_unlayered_beats_layered() {
        // Unlayered rule wins over layered rule with equal specificity.
        let s = cascade_at(
            "<p>x</p>",
            "@layer base { p { color: red; } } p { color: blue; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_layer_later_layer_beats_earlier_layer() {
        // layer `components` declared after `base` → higher priority.
        let s = cascade_at(
            "<p>x</p>",
            "@layer base { p { color: red; } } \
             @layer components { p { color: blue; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_layer_order_statement_respected() {
        // Statement-form `@layer base, components;` fixes order;
        // later block definitions don't change priority.
        let s = cascade_at(
            "<p>x</p>",
            "@layer base, components; \
             @layer components { p { color: blue; } } \
             @layer base { p { color: red; } }",
            &[0],
        );
        // components has higher priority than base → blue wins.
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_layer_important_reversal() {
        // CSS Cascade L5 §6.4.5: for !important, earlier layer's !important
        // wins (inversion of normal ordering). Layer `base` declared first
        // → base !important wins over components !important.
        let s = cascade_at(
            "<p>x</p>",
            "@layer base { p { color: red !important; } } \
             @layer components { p { color: blue !important; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_layer_unlayered_important_loses_to_layered_important() {
        // Unlayered !important loses to any layer's !important
        // (per CSS Cascade L5 §6.4.5 inversion: unlayered has lowest
        // !important priority, i.e., layer[0]'s !important beats it).
        let s = cascade_at(
            "<p>x</p>",
            "p { color: blue !important; } \
             @layer base { p { color: red !important; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_layer_specificity_within_same_layer() {
        // Within a single layer, normal specificity rules apply.
        let s = cascade_at(
            r#"<p class="k">x</p>"#,
            "@layer base { .k { color: blue; } p { color: red; } }",
            &[0],
        );
        // .k has higher specificity → blue.
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    // ─── revert-layer (CSS Cascade L5 §6.4.6) ────────────────────────────────

    #[test]
    fn revert_layer_falls_back_to_lower_layer() {
        // `.r` wins layer `b` with `revert-layer` → layer `b` is rolled back for
        // `color`, falling to layer `a` (green). Plain `p` stays red.
        let s = cascade_at(
            r#"<p class="r">x</p>"#,
            "@layer a { p { color: green; } } \
             @layer b { p { color: red; } .r { color: revert-layer; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn revert_layer_unlayered_falls_to_highest_layer() {
        // Unlayered `revert-layer` reverts the unlayered declarations, so the
        // value falls to the highest-priority layer `b` (red), not back to `a`.
        let s = cascade_at(
            "<p>x</p>",
            "@layer a { p { color: green; } } \
             @layer b { p { color: red; } } \
             p { color: revert-layer; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn revert_layer_with_no_lower_decl_keeps_inherited() {
        // No lower-priority author declaration for `color` → reverting the only
        // layer leaves the inherited value (blue from <body>).
        let s = cascade_at(
            "<p>x</p>",
            "body { color: blue; } \
             @layer a { p { color: revert-layer; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn revert_layer_overridden_has_no_effect() {
        // A `revert-layer` declaration overridden by a higher (unlayered) rule
        // has no effect — red still wins.
        let s = cascade_at(
            "<p>x</p>",
            "@layer a { p { color: green; } } \
             @layer b { p { color: revert-layer; } } \
             p { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn revert_layer_only_affects_its_own_property() {
        // `revert-layer` on `color` must not touch `background-color`:
        // color reverts to green (layer a), background-color stays blue (layer b).
        let s = cascade_at(
            r#"<p class="r">x</p>"#,
            "@layer a { p { color: green; background-color: yellow; } } \
             @layer b { p { color: red; background-color: blue; } \
                        .r { color: revert-layer; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
        assert_eq!(
            s.background_color.map(|c| c.resolve(s.color)),
            Some(Color { r: 0, g: 0, b: 255, a: 255 })
        );
    }

    // ─── revert-rule (CSS Cascade L5 §revert-rule-keyword, BUG-487) ──────────

    #[test]
    fn revert_rule_rolls_back_to_previous_rule() {
        // Second `p` rule's own `color: red` is overridden by its own
        // `color: revert-rule` — rolls back to the FIRST rule (green), not to
        // the `red` declared earlier within the same rule.
        let s = cascade_at(
            "<p>x</p>",
            "p { color: green; } \
             p { color: red; color: revert-rule; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn revert_rule_uses_cascade_order_not_appearance_order() {
        // The highest-specificity rule (`.r.r.r`) wins and reverts; removing
        // its own `color` declarations exposes the next-highest-specificity
        // rule in CASCADE order (`.r.r`, green), even though `.r.r` appears
        // textually AFTER the reverting rule.
        let s = cascade_at(
            r#"<p class="r">x</p>"#,
            ".r { color: red; } \
             .r.r.r { color: red; color: revert-rule; } \
             .r.r { color: green; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn revert_rule_chain_walks_back_multiple_rules() {
        // Three consecutive rules each declare `z-index: -1;
        // z-index: revert-rule` — each must fully unwind (both its own
        // declarations dropped) before the next is checked, landing on the
        // second rule's `2`, not the first rule's `1`.
        let s = cascade_at(
            "<p>x</p>",
            "p { z-index: 1; } \
             p { z-index: 2; } \
             p { z-index: -1; z-index: revert-rule; } \
             p { z-index: -1; z-index: revert-rule; } \
             p { z-index: -1; z-index: revert-rule; }",
            &[0],
        );
        assert_eq!(s.z_index, Some(2));
    }

    #[test]
    fn revert_rule_in_custom_property() {
        // Same rollback mechanism applies to custom properties: `--a` unwinds
        // one rule (to the middle rule's `green`), `--b` unwinds two (to the
        // first rule's `green`).
        let s = cascade_at(
            "<p>x</p>",
            "p { --a: red; --b: green; } \
             p { --a: green; --b: revert-rule; } \
             p { --a: revert-rule; --b: revert-rule; }",
            &[0],
        );
        assert_eq!(s.custom_props.get("--a").map(String::as_str), Some("green"));
        assert_eq!(s.custom_props.get("--b").map(String::as_str), Some("green"));
    }

    #[test]
    fn revert_rule_important_reverts_to_earlier_normal_rule() {
        // An `!important revert-rule` still wins the cascade over a later
        // normal rule (importance sorts first); reverting it falls back to
        // that later normal rule regardless of source order.
        let s = cascade_at(
            "<p>x</p>",
            "p { color: revert-rule !important; } \
             p { color: green; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn revert_rule_can_chain_into_revert_layer() {
        // `revert-rule` in the highest layer unwinds to reveal a
        // `revert-layer` in a lower layer, which must itself be resolved
        // (not left as a literal stuck value) — falls all the way back to
        // the lowest layer's plain green.
        let s = cascade_at(
            "<p>x</p>",
            "@layer a { p { color: green; } } \
             @layer b { p { color: red; } p { color: revert-layer; } } \
             @layer c { p { color: revert-rule; } }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    // ─── revert (CSS Cascade L4 §7.4) ────────────────────────────────────────

    #[test]
    fn revert_non_inherited_without_ua_hint_falls_to_initial() {
        // No UA rule for `background-color` on `<p>` → `revert` behaves like
        // `unset`/`initial`: falls to the property's initial value (None).
        let s = cascade_at(
            "<p>x</p>",
            "p { background-color: yellow; background-color: revert; }",
            &[0],
        );
        assert_eq!(s.background_color, None);
    }

    #[test]
    fn revert_inherited_without_ua_hint_falls_to_inherited() {
        // No UA rule for `color` on `<p>` → `revert` behaves like `unset`:
        // falls to the inherited value (blue from <body>).
        let s = cascade_at(
            "<p>x</p>",
            "body { color: blue; } p { color: red; color: revert; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn revert_non_inherited_ua_hint_restores_ua_value() {
        // `display` has a real UA default per element (`default_display`):
        // `<span>` → inline. `revert` must roll back to that UA value, NOT to
        // the CSS initial value (`Block` in this engine's `ComputedStyle::root()`)
        // the way `unset`/`initial` would.
        let s = cascade_at(
            "<span>x</span>",
            "span { display: block; display: revert; }",
            &[0],
        );
        assert_eq!(s.display, Display::Inline);
    }

    #[test]
    fn revert_non_inherited_no_ua_hint_matches_unset() {
        // Control case: `unset` on the same property/element stays at the
        // engine's non-inherited fallback (`Block`), confirming `revert` above
        // is genuinely different, not an artifact of the test setup.
        let s = cascade_at("<span>x</span>", "span { display: unset; }", &[0]);
        assert_eq!(s.display, Display::Block);
    }

    #[test]
    fn revert_inherited_ua_hint_restores_ua_value() {
        // `font-style` has a UA hint for `<em>` (`ua_font_style` → Italic).
        // An author rule sets it to `normal`, then `revert` — must roll back to
        // Italic (the UA value), not to the parent's inherited `Normal`.
        let s = cascade_at(
            "<em>x</em>",
            "em { font-style: normal; font-style: revert; }",
            &[0],
        );
        assert_eq!(s.font_style, FontStyle::Italic);
    }

    #[test]
    fn revert_inherited_ua_hint_no_hint_element_falls_to_inherited() {
        // Control case: on an element with no `font-style` UA hint (`<p>`),
        // `revert` behaves like `unset` — falls to the parent's inherited value.
        let s = cascade_at(
            "<p>x</p>",
            "p { font-style: italic; font-style: revert; }",
            &[0],
        );
        assert_eq!(s.font_style, FontStyle::Normal);
    }

    #[test]
    fn revert_font_size_restores_ua_hinted_factor() {
        // `<small>` gets a UA hint of 0.83× parent font-size
        // (`ua_font_size_factor`). `font-size: revert` must roll back to that
        // scaled value, not to the raw (unscaled) parent font-size the way
        // `unset` would — exercises the `apply_font_size` pre-pass fix, not
        // just the generic `apply_css_wide_keyword` path.
        let s = cascade_at(
            "<small>x</small>",
            "body { font-size: 20px; } small { font-size: 12px; font-size: revert; }",
            &[0],
        );
        assert!((s.font_size - 20.0 * 0.83).abs() < 1e-4, "font_size={}", s.font_size);
    }

    // ── matches_defined (CSS Selectors L4 §6.4.1 / HTML LS §4.13.5) ──────

    fn first_child_of_root(doc: &lumen_dom::Document) -> lumen_dom::NodeId {
        doc.get(doc.body().unwrap()).children[0]
    }

    #[test]
    fn defined_matches_builtin_html_element() {
        // `<div>` — built-in, defined.
        let doc = lumen_html_parser::parse("<div></div>");
        let node = first_child_of_root(&doc);
        assert!(matches_defined(&doc, node));
    }

    #[test]
    fn defined_matches_arbitrary_unknown_no_hyphen() {
        // `<foo>` без дефиса не может быть валидным custom-element-именем
        // (HTML LS §4.13.2 требует дефис), значит трактуется как built-in
        // unknown — defined.
        let doc = lumen_html_parser::parse("<foo></foo>");
        let node = first_child_of_root(&doc);
        assert!(matches_defined(&doc, node));
    }

    #[test]
    fn defined_does_not_match_custom_element_name() {
        // `<my-button>` — валидное custom-element-имя, в Phase 0 без
        // registry никогда не defined.
        let doc = lumen_html_parser::parse("<my-button></my-button>");
        let node = first_child_of_root(&doc);
        assert!(!matches_defined(&doc, node));
    }

    #[test]
    fn defined_does_not_match_deep_custom_element_name() {
        // Имя с несколькими дефисами — тоже custom (`<x-y-z>` валидно).
        let doc = lumen_html_parser::parse("<x-y-z></x-y-z>");
        let node = first_child_of_root(&doc);
        assert!(!matches_defined(&doc, node));
    }

    #[test]
    fn defined_selector_filters_custom_elements_in_cascade() {
        // E2E: `:not(:defined) { display: none }` скрывает custom-element
        // (FOUC-protection idiom). Built-in остаётся видимым.
        let doc =
            lumen_html_parser::parse("<my-card></my-card><div></div>");
        let sheet =
            lumen_css_parser::parse(":not(:defined) { display: none; }");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let my_card = doc.get(body).children[0];
        let div = doc.get(body).children[1];
        let my_card_style =
            compute_style(&doc, my_card, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let div_style =
            compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(my_card_style.display, Display::None);
        assert_ne!(div_style.display, Display::None);
    }

    // --- CSS Viewport L1 §5 — `zoom` ---

    /// Style one element from an inline `style` attribute.
    fn zoom_test_style(html: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false)
    }

    #[test]
    fn zoom_parses_numbers_percentages_and_keywords() {
        let approx = |got: Option<f32>, want: f32| {
            assert!(got.is_some_and(|g| (g - want).abs() < 1e-6), "got {got:?}, want {want}");
        };
        approx(parse_zoom("0.8"), 0.8);
        approx(parse_zoom(".8"), 0.8);
        approx(parse_zoom("80%"), 0.8);
        approx(parse_zoom("  1.5  "), 1.5);
        // `normal`/`reset` contribute no scaling of their own.
        approx(parse_zoom("normal"), 1.0);
        approx(parse_zoom("RESET"), 1.0);
        // Invalid values yield None so the caller drops the declaration rather
        // than resetting an already-cascaded value to 1.
        assert_eq!(parse_zoom("-0.5"), None);
        assert_eq!(parse_zoom("0"), None);
        assert_eq!(parse_zoom("auto"), None);
        assert_eq!(parse_zoom(""), None);
    }

    /// The core of the property: `zoom` shrinks the box itself, not just its
    /// painted appearance, so the computed lengths come out already scaled.
    #[test]
    fn zoom_scales_own_lengths_and_font_size() {
        let s = zoom_test_style(
            "<div style=\"zoom: 0.5; width: 100px; padding-left: 20px; \
             margin-top: 8px; font-size: 40px; border-left: 4px solid red\"></div>",
        );
        assert_eq!(s.width, Some(Length::Px(50.0)));
        assert_eq!(s.padding_left, Length::Px(10.0));
        assert_eq!(s.margin_top, LengthOrAuto::Length(Length::Px(4.0)));
        assert!((s.font_size - 20.0).abs() < 0.01, "font_size = {}", s.font_size);
        assert!((s.border_left_width - 2.0).abs() < 0.01);
        assert!((s.effective_zoom - 0.5).abs() < 1e-6);
    }

    /// This is the shape tbank.ru relies on: a fixed-width container that only
    /// fits the viewport once `zoom` is applied.
    #[test]
    fn zoom_shrinks_fixed_min_and_max_width_container() {
        let s = zoom_test_style(
            "<div style=\"zoom: 0.8; min-width: 1104px; max-width: 1104px\"></div>",
        );
        assert_eq!(s.min_width, Some(Length::Px(1104.0 * 0.8)));
        assert_eq!(s.max_width, Some(Length::Px(1104.0 * 0.8)));
    }

    /// `zoom` multiplies down the tree, and an *inherited* font-size must take
    /// only the element's own factor — the ancestors' is already baked into the
    /// value it inherited.
    #[test]
    fn zoom_compounds_through_the_tree() {
        let doc = lumen_html_parser::parse(
            "<div style=\"zoom: 0.5; font-size: 40px\">\
             <span style=\"zoom: 0.5; width: 100px\"></span></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let vp = Size::new(800.0, 600.0);
        let outer = doc.get(doc.body().unwrap()).children[0];
        let outer_style = compute_style(&doc, outer, &sheet, &root, vp, false);
        let inner = doc.get(outer).children[0];
        let inner_style = compute_style(&doc, inner, &sheet, &outer_style, vp, false);

        assert!((outer_style.font_size - 20.0).abs() < 0.01);
        // 0.5 × 0.5 — the ancestor's factor and its own.
        assert!((inner_style.effective_zoom - 0.25).abs() < 1e-6);
        assert_eq!(inner_style.width, Some(Length::Px(25.0)));
        // Inherited 20px, scaled once by the inner element's own 0.5 — not by
        // the compounded 0.25, which would re-apply the parent's zoom.
        assert!(
            (inner_style.font_size - 10.0).abs() < 0.01,
            "font_size = {}",
            inner_style.font_size
        );
    }

    /// A font-size *specified* on a descendant is not automatically "unzoomed":
    /// what matters is the basis it resolved against. `em`/`%` resolve against
    /// the parent's already-zoomed size and must take only the element's own
    /// factor, while `px` comes from nowhere and takes the compounded one.
    /// Charging both to `effective_zoom` re-applies the ancestors' factor once
    /// per level, so a tree of `em` sizes under a zoomed container shrinks
    /// geometrically (0.8 → 0.64 → 0.512 …).
    #[test]
    fn zoom_font_size_takes_the_factor_its_basis_lacks() {
        let doc = lumen_html_parser::parse(
            "<div style=\"zoom: 0.5; font-size: 40px\">\
             <span style=\"font-size: 1.5em\"></span>\
             <span style=\"font-size: 150%\"></span>\
             <span style=\"font-size: 20px\"></span>\
             <span style=\"zoom: 0.5; font-size: 1.5em\"></span></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let vp = Size::new(800.0, 600.0);
        let outer = doc.get(doc.body().unwrap()).children[0];
        let outer_style = compute_style(&doc, outer, &sheet, &root, vp, false);
        assert!((outer_style.font_size - 20.0).abs() < 0.01);

        let kids = doc.get(outer).children.clone();
        let fs = |i: usize| compute_style(&doc, kids[i], &sheet, &outer_style, vp, false).font_size;

        // 1.5 × the parent's zoomed 20px. The parent's 0.5 is already in the
        // basis; applying it again would give 15.
        assert!((fs(0) - 30.0).abs() < 0.01, "em font-size = {}", fs(0));
        // `%` resolves against the same basis as `em`.
        assert!((fs(1) - 30.0).abs() < 0.01, "% font-size = {}", fs(1));
        // A `px` size knows nothing of the ancestors' zoom, so it takes all of it.
        assert!((fs(2) - 10.0).abs() < 0.01, "px font-size = {}", fs(2));
        // Own zoom still applies on top of a parent-relative basis: 20 × 1.5 × 0.5.
        assert!((fs(3) - 15.0).abs() < 0.01, "em + own zoom = {}", fs(3));
    }

    /// Relative units must not be scaled here: they resolve later against a
    /// basis that is itself already zoomed, so touching them would apply the
    /// factor twice. `10em` against the zoomed 10px font-size is the intended
    /// 100px, whereas a pre-scaled `5em` would collapse to 50px.
    #[test]
    fn zoom_leaves_relative_units_to_their_zoomed_basis() {
        let s = zoom_test_style(
            "<div style=\"zoom: 0.5; font-size: 20px; width: 10em; padding-left: 50%\"></div>",
        );
        assert!((s.font_size - 10.0).abs() < 0.01);
        assert_eq!(s.width, Some(Length::Em(10.0)));
        assert_eq!(s.padding_left, Length::Percent(50.0));
    }

    /// The initial value is 1.0, so a page that never mentions `zoom` computes
    /// exactly as before — this is what keeps the change neutral for every
    /// existing test page.
    #[test]
    fn absent_zoom_leaves_lengths_untouched() {
        let s = zoom_test_style(
            "<div style=\"width: 100px; padding-left: 20px; font-size: 40px\"></div>",
        );
        assert!((s.effective_zoom - 1.0).abs() < f32::EPSILON);
        assert_eq!(s.width, Some(Length::Px(100.0)));
        assert_eq!(s.padding_left, Length::Px(20.0));
        assert!((s.font_size - 40.0).abs() < 0.01);
    }

    /// An unparseable `zoom` is ignored (CSS Syntax): the element keeps the
    /// neutral factor instead of being scaled by a garbage value.
    #[test]
    fn invalid_zoom_declaration_is_ignored() {
        let s = zoom_test_style("<div style=\"zoom: banana; width: 100px\"></div>");
        assert!((s.effective_zoom - 1.0).abs() < f32::EPSILON);
        assert_eq!(s.width, Some(Length::Px(100.0)));
    }

    // ── @supports cascade wiring ──────────────────────────────────────────────

    #[test]
    fn at_supports_known_property_applies_rules() {
        let doc = lumen_html_parser::parse("<div>x</div>");
        let sheet = lumen_css_parser::parse("@supports (color: red) { div { color: blue; } }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_supports_unknown_property_skips_rules() {
        let doc = lumen_html_parser::parse("<div>x</div>");
        let sheet = lumen_css_parser::parse(
            "@supports (unknown-xyz-prop: 1) { div { color: blue; } }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        // Color stays default (black) because rule was filtered out.
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn scope_rule_applies_to_descendant() {
        // @scope (.wrapper) { color: blue; } applies to .child inside .wrapper.
        let doc = lumen_html_parser::parse(
            r#"<style>@scope (.wrapper) { .child { color: blue; } }</style>
            <div class="wrapper"><span class="child">x</span></div>"#
        );
        let sheet = lumen_css_parser::parse(r#"@scope (.wrapper) { .child { color: blue; } }"#);
        let root = ComputedStyle::root();
        // Find .child
        let wrapper = doc.get(doc.body().unwrap()).children[0];
        let child = doc.get(wrapper).children[0];
        let style = compute_style(&doc, child, &sheet, &root, Size::new(400.0, 400.0), false);
        assert_eq!(style.color.b, 255, "scope rule should apply to .child inside .wrapper");
    }

    #[test]
    fn scope_rule_does_not_apply_outside() {
        // @scope (.wrapper) { color: blue; } does NOT apply to .child outside .wrapper.
        let doc = lumen_html_parser::parse(r#"<div class="child">x</div>"#);
        let sheet = lumen_css_parser::parse(r#"@scope (.wrapper) { .child { color: blue; } }"#);
        let root = ComputedStyle::root();
        let child = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, child, &sheet, &root, Size::new(400.0, 400.0), false);
        assert_eq!(style.color.b, 0, "scope rule should NOT apply outside .wrapper");
    }

    #[test]
    fn scope_rule_empty_root_applies_everywhere() {
        // @scope { color: red; } (no root) applies to any element.
        let doc = lumen_html_parser::parse(r#"<span>x</span>"#);
        let sheet = lumen_css_parser::parse(r#"@scope { span { color: red; } }"#);
        let root = ComputedStyle::root();
        let span = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, span, &sheet, &root, Size::new(400.0, 400.0), false);
        assert_eq!(style.color.r, 255, "empty-root scope should apply everywhere");
    }

    #[test]
    fn scope_limit_excludes_donut_hole() {
        // CSS Cascade L6 §3.2 — `to (<limit>)` carves a donut hole: elements
        // that are inclusive descendants of a limit *within* the scope are out.
        let html = r#"<div class="card"><p class="a">A</p><section class="content"><p class="b">B</p></section></div>"#;
        let sheet = lumen_css_parser::parse(
            r#"@scope (.card) to (.content) {
                .a { color: blue; }
                .b { color: blue; }
                .content { color: blue; }
            }"#,
        );
        let doc = lumen_html_parser::parse(html);
        let root = ComputedStyle::root();
        let card = doc.get(doc.body().unwrap()).children[0];
        let a = doc.get(card).children[0];
        let content = doc.get(card).children[1];
        let b = doc.get(content).children[0];
        let vp = Size::new(400.0, 400.0);
        // .a is above the limit → in scope.
        assert_eq!(
            compute_style(&doc, a, &sheet, &root, vp, false).color.b, 255,
            ".a above the limit should be in scope"
        );
        // The limit element itself is inclusive → out of scope.
        assert_eq!(
            compute_style(&doc, content, &sheet, &root, vp, false).color.b, 0,
            "the limit element itself should be out of scope"
        );
        // .b is a descendant of the limit → out of scope (the donut hole).
        assert_eq!(
            compute_style(&doc, b, &sheet, &root, vp, false).color.b, 0,
            ".b inside the limit should be out of scope"
        );
    }

    #[test]
    fn scope_limit_above_root_does_not_exclude() {
        // A limit-matching element that sits *above* the scope root must not
        // remove a node from scope — walking up, the root is reached first.
        let html = r#"<section class="content"><div class="card"><p class="a">A</p></div></section>"#;
        let sheet = lumen_css_parser::parse(
            r#"@scope (.card) to (.content) { .a { color: blue; } }"#,
        );
        let doc = lumen_html_parser::parse(html);
        let root = ComputedStyle::root();
        let content = doc.get(doc.body().unwrap()).children[0];
        let card = doc.get(content).children[0];
        let a = doc.get(card).children[0];
        let style = compute_style(&doc, a, &sheet, &root, Size::new(400.0, 400.0), false);
        assert_eq!(
            style.color.b, 255,
            ".a should stay in scope: the .content limit is above the .card root"
        );
    }

    #[test]
    fn scope_empty_root_with_limit_carves_hole() {
        // `@scope { … } to (<limit>)` — implicit document-root scope still
        // honours the limit: descendants of the limit are excluded.
        let html = r#"<div class="wrap"><p class="a">A</p><section class="stop"><p class="b">B</p></section></div>"#;
        let sheet = lumen_css_parser::parse(
            r#"@scope to (.stop) { .a { color: red; } .b { color: red; } }"#,
        );
        let doc = lumen_html_parser::parse(html);
        let root = ComputedStyle::root();
        let wrap = doc.get(doc.body().unwrap()).children[0];
        let a = doc.get(wrap).children[0];
        let stop = doc.get(wrap).children[1];
        let b = doc.get(stop).children[0];
        let vp = Size::new(400.0, 400.0);
        assert_eq!(
            compute_style(&doc, a, &sheet, &root, vp, false).color.r, 255,
            ".a should be in the implicit document scope"
        );
        assert_eq!(
            compute_style(&doc, b, &sheet, &root, vp, false).color.r, 0,
            ".b under the limit should be excluded even with an empty root"
        );
    }

    #[test]
    fn fullscreen_pseudo_matches_sentinel_attr() {
        let html = r#"<div id="el" data-lumen-fullscreen="">x</div>"#;
        let css = r#":fullscreen { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let el = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, el, &sheet, &root, Size::new(200.0, 200.0), false);
        assert_eq!(style.color.r, 255, ":fullscreen rule should apply when sentinel attr present");
    }

    #[test]
    fn popover_open_pseudo_matches_sentinel_attr() {
        let html = r#"<div id="p" data-lumen-popover-open="">x</div>"#;
        let css = r#":popover-open { color: blue; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let el = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, el, &sheet, &root, Size::new(200.0, 200.0), false);
        assert_eq!(style.color.b, 255, ":popover-open rule should apply when sentinel attr present");
        assert_eq!(style.color.r, 0);
    }

    #[test]
    fn modal_pseudo_matches_data_lumen_modal_attr() {
        // :modal matches only when showModal() sets `data-lumen-modal` sentinel.
        let html = r#"<dialog id="d" data-lumen-modal="" open>content</dialog>"#;
        let css = r#":modal { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let dlg = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, dlg, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color.r, 255, ":modal rule should apply when sentinel attr present");
    }

    #[test]
    fn modal_pseudo_does_not_match_show_dialog() {
        // Non-modal dialog (show() — no data-lumen-modal attr) must NOT match :modal.
        let html = r#"<dialog id="d" open>content</dialog>"#;
        let css = r#":modal { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let dlg = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, dlg, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_ne!(style.color.r, 255, ":modal rule must NOT apply without sentinel attr");
    }

    #[test]
    fn state_pseudo_matches_sentinel_attr() {
        // :state(open) matches when the JS CustomStateSet reflects the
        // active state into `data-lumen-state-open` on the host element.
        let html = r#"<my-el id="el" data-lumen-state-open="">x</my-el>"#;
        let css = r#":state(open) { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let el = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, el, &sheet, &root, Size::new(200.0, 200.0), false);
        assert_eq!(style.color.r, 255, ":state(open) rule should apply when sentinel attr present");
    }

    #[test]
    fn state_pseudo_does_not_match_without_sentinel_attr() {
        let html = r#"<my-el id="el">x</my-el>"#;
        let css = r#":state(open) { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let el = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, el, &sheet, &root, Size::new(200.0, 200.0), false);
        assert_ne!(style.color.r, 255, ":state(open) must NOT apply without sentinel attr");
    }

    #[test]
    fn state_pseudo_distinguishes_state_names() {
        // Sentinel attr для одного state-имени не должен матчить другое.
        let html = r#"<my-el id="el" data-lumen-state-collapsed="">x</my-el>"#;
        let css = r#":state(open) { color: red; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let el = doc.get(doc.body().unwrap()).children[0];
        let root = ComputedStyle::root();
        let style = compute_style(&doc, el, &sheet, &root, Size::new(200.0, 200.0), false);
        assert_ne!(style.color.r, 255, ":state(open) must not match a differently-named state attr");
    }

    #[test]
    fn parse_paint_function_basic() {
        // CSS Paint API (Houdini) — parse paint(name) function.
        assert_eq!(parse_paint_function("paint(my-paint)"), Some("my-paint".to_string()));
        assert_eq!(parse_paint_function("paint('my-paint')"), Some("my-paint".to_string()));
        assert_eq!(parse_paint_function("paint(\"my-paint\")"), Some("my-paint".to_string()));
    }

    #[test]
    fn parse_paint_function_with_whitespace() {
        // paint() name trimmed; outer whitespace ignored, inner whitespace preserved.
        assert_eq!(parse_paint_function("  paint(test)  "), Some("test".to_string()));
        // Interior whitespace is trimmed during parsing (inner trim).
        assert_eq!(parse_paint_function("paint( test )"), Some("test".to_string()));
    }

    #[test]
    fn parse_paint_function_invalid() {
        // Invalid: missing parentheses, wrong function name, or no closing paren.
        assert_eq!(parse_paint_function("paint"), None);
        assert_eq!(parse_paint_function("gradient(test)"), None);
        assert_eq!(parse_paint_function("paint(test"), None);
        assert_eq!(parse_paint_function("paint(test))"), None);
    }

    #[test]
    fn background_image_paint_function_parsed_to_paint_variant() {
        // CSS Paint API (Houdini) — `background-image: paint(name)` must produce BackgroundImage::Paint.
        let s = cascade_at("<div></div>", "div { background-image: paint(my-worklet); }", &[0]);
        assert_eq!(s.background_layers.len(), 1);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Paint("my-worklet".to_string()));
    }

    #[test]
    fn background_image_paint_function_with_quotes_parsed() {
        // paint("name") with double-quotes must strip quotes and produce Paint("name").
        let s = cascade_at("<div></div>", r#"div { background-image: paint("checker"); }"#, &[0]);
        assert_eq!(s.background_layers.len(), 1);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Paint("checker".to_string()));
    }

    #[test]
    fn css_properties_values_api_parse_property_rule() {
        // CSS Properties and Values L1 — @property at-rule parsing
        let sheet = lumen_css_parser::parse(
            "@property --my-color { syntax: \"<color>\"; inherits: true; initial-value: blue; }"
        );
        assert_eq!(sheet.properties.len(), 1);
        let prop = &sheet.properties[0];
        assert_eq!(prop.name, "--my-color");
        assert_eq!(prop.syntax, "<color>");
        assert!(prop.inherits);
        assert_eq!(prop.initial_value, Some("blue".to_string()));
    }

    #[test]
    fn css_properties_values_api_initial_value_fallback() {
        // CSS Properties and Values L1 §1.1 — initial-value fallback when property not set
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --size { syntax: \"<length>\"; inherits: false; initial-value: 10px; }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.custom_props.get("--size").map(String::as_str), Some("10px"));
    }

    #[test]
    fn svg_presentation_attributes_applied() {
        // BUG-096: SVG presentation attributes (fill / stroke / stroke-width as
        // plain XML attributes) must map onto the SVG paint properties. Without
        // this, `<path fill="none" stroke="#e94560">` kept the default black fill
        // and no stroke, so every <path> painted as a black blob.
        let doc = lumen_html_parser::parse(
            "<svg><path d='M 0 0 L 10 10' fill='none' stroke='#e94560' stroke-width='8'/></svg>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();

        // Locate the <path> element wherever the HTML parser placed it.
        fn find_path(doc: &Document, id: NodeId) -> Option<NodeId> {
            if doc.get(id).element_name().is_some_and(|n| n.local.as_str() == "path") {
                return Some(id);
            }
            for &c in &doc.get(id).children {
                if let Some(found) = find_path(doc, c) {
                    return Some(found);
                }
            }
            None
        }
        let path = find_path(&doc, doc.root()).expect("path element present");

        let style = compute_style(&doc, path, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(matches!(style.svg_fill, SvgPaint::None), "fill=none must map to SvgPaint::None");
        assert!(
            matches!(style.svg_stroke, SvgPaint::Color(c) if c.r == 233 && c.g == 69 && c.b == 96),
            "stroke=#e94560 must map to its colour, got {:?}",
            style.svg_stroke,
        );
        assert_eq!(style.svg_stroke_width, 8.0, "stroke-width attribute must apply");
    }

    #[test]
    fn svg_stroke_geometry_unitless_in_standards_mode() {
        // BUG-102: SVG stroke geometry attributes use unitless **user units**
        // (SVG 2 §7.10). `parse_length_q` rejects unitless non-zero numbers in
        // standards mode, so on a `<!DOCTYPE html>` page `stroke-width="20"`,
        // `stroke-dasharray="20 8"` and `stroke-dashoffset="14"` were silently
        // dropped — every <path> painted at the inherited default width of 1px.
        let doc = lumen_html_parser::parse(
            "<!DOCTYPE html><svg width='240' height='120'>\
             <path d='M 30 40 H 210' fill='none' stroke='#58a6ff' \
             stroke-width='20' stroke-dasharray='20 8' stroke-dashoffset='14'/></svg>",
        );
        assert_eq!(doc.mode(), DocumentMode::NoQuirks, "doctype must select standards mode");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        fn find_path(doc: &Document, id: NodeId) -> Option<NodeId> {
            if doc.get(id).element_name().is_some_and(|n| n.local.as_str() == "path") {
                return Some(id);
            }
            doc.get(id).children.iter().find_map(|&c| find_path(doc, c))
        }
        let path = find_path(&doc, doc.root()).expect("path element present");
        let style = compute_style(&doc, path, &sheet, &root, Size::new(1024.0, 720.0), false);
        assert_eq!(style.svg_stroke_width, 20.0, "unitless stroke-width must apply in standards mode");
        assert_eq!(style.svg_stroke_dasharray, vec![20.0, 8.0], "unitless dasharray must apply");
        assert_eq!(style.svg_stroke_dashoffset, 14.0, "unitless dashoffset must apply");
    }

    #[test]
    fn svg_presentation_attribute_overridden_by_css() {
        // SVG 2 §6.4: a presentation attribute has the lowest author priority —
        // any matching CSS rule wins. `style="stroke:#00ff00"` overrides stroke="red".
        let doc = lumen_html_parser::parse(
            "<svg><path d='M 0 0 L 10 10' stroke='red' style='stroke:#00ff00'/></svg>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        fn find_path(doc: &Document, id: NodeId) -> Option<NodeId> {
            if doc.get(id).element_name().is_some_and(|n| n.local.as_str() == "path") {
                return Some(id);
            }
            doc.get(id).children.iter().find_map(|&c| find_path(doc, c))
        }
        let path = find_path(&doc, doc.root()).expect("path element present");
        let style = compute_style(&doc, path, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(
            matches!(style.svg_stroke, SvgPaint::Color(c) if c.r == 0 && c.g == 255 && c.b == 0),
            "inline CSS stroke must override the stroke presentation attribute, got {:?}",
            style.svg_stroke,
        );
    }

    #[test]
    fn css_properties_values_api_no_initial_value() {
        // CSS Properties and Values L1 — no initial-value means property stays empty
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --no-initial { syntax: \"<custom-ident>\"; inherits: true; }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(!style.custom_props.contains_key("--no-initial"));
    }

    #[test]
    fn css_properties_values_api_declared_overrides_initial() {
        // CSS Properties and Values L1 — declared value overrides initial-value
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --color-prop { syntax: \"<color>\"; inherits: false; initial-value: red; } div { --color-prop: green; }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.custom_props.get("--color-prop").map(String::as_str), Some("green"));
    }

    #[test]
    fn css_properties_values_api_inherits_true() {
        // CSS Properties and Values L1 — inherits: true property inherits from parent
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --inherit-prop { syntax: \"<custom-ident>\"; inherits: true; initial-value: initial-val; } body { --inherit-prop: parent-val; }"
        );
        let root = ComputedStyle::root();
        let body = doc.body().unwrap();

        let body_style = compute_style(&doc, body, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(body_style.custom_props.get("--inherit-prop").map(String::as_str), Some("parent-val"));

        let div = doc.get(body).children[0];
        let div_style = compute_style(&doc, div, &sheet, &body_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.custom_props.get("--inherit-prop").map(String::as_str), Some("parent-val"));
    }

    #[test]
    fn css_properties_values_api_multiple_properties() {
        // CSS Properties and Values L1 — multiple @property rules
        let sheet = lumen_css_parser::parse(
            "@property --col1 { syntax: \"<color>\"; inherits: true; initial-value: red; } @property --col2 { syntax: \"<color>\"; inherits: false; initial-value: blue; }"
        );
        assert_eq!(sheet.properties.len(), 2);
    }

    #[test]
    fn css_properties_values_api_universal_syntax() {
        // CSS Properties and Values L1 — universal syntax "*" accepts any value
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --any-value { syntax: \"*\"; inherits: true; initial-value: calc(100% - 10px); }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        // Universal syntax should accept any value including calc()
        assert_eq!(style.custom_props.get("--any-value").map(String::as_str), Some("calc(100% - 10px)"));
    }

    #[test]
    fn css_properties_values_api_var_substitution_with_fallback() {
        // CSS Properties and Values L1 — var() with fallback when initial-value not used
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@property --size { syntax: \"<length>\"; inherits: false; initial-value: 5px; } div { width: var(--size, 10px); }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        // Compute style for div — should use initial-value for --size
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        // Custom property should be set via initial-value
        assert_eq!(style.custom_props.get("--size").map(String::as_str), Some("5px"));
    }

    // CSS Grid auto-fill/auto-fit/fit-content parsing tests (B-3)
    #[test]
    fn grid_track_size_parse_fit_content() {
        let parsed = GridTrackSize::parse_track_list("fit-content(200px)", false);
        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0], GridTrackSize::FitContent(_)));
    }

    #[test]
    fn grid_track_size_parse_fit_content_percentage() {
        let parsed = GridTrackSize::parse_track_list("fit-content(50%)", false);
        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0], GridTrackSize::FitContent(_)));
    }

    #[test]
    fn grid_template_columns_auto_fill_parse() {
        // `repeat(auto-fill, minmax(100px, 1fr))` should parse without errors
        let parsed = GridTrackSize::parse_track_list("repeat(auto-fill, minmax(100px, 1fr))", false);
        // Phase 1: auto-fill expands as single repeat unit (not yet full resolution)
        assert!(!parsed.is_empty(), "parse_track_list should not return empty for auto-fill repeat");
    }

    #[test]
    fn grid_template_columns_auto_fit_parse() {
        // `repeat(auto-fit, minmax(80px, 1fr))` should parse without errors
        let parsed = GridTrackSize::parse_track_list("repeat(auto-fit, minmax(80px, 1fr))", false);
        assert!(!parsed.is_empty(), "parse_track_list should not return empty for auto-fit repeat");
    }

    #[test]
    fn grid_template_columns_fixed_repeat_parse() {
        // `repeat(3, 100px)` should parse to 3 copies of 100px
        let parsed = GridTrackSize::parse_track_list("repeat(3, 100px)", false);
        assert_eq!(parsed.len(), 3, "repeat(3, ...) should expand to 3 tracks");
        for track in parsed {
            assert!(matches!(track, GridTrackSize::Length(_)), "each track should be Length");
        }
    }

    #[test]
    fn grid_template_columns_mixed_repeat() {
        // `100px repeat(2, 200px) 300px` should parse to [100px, 200px, 200px, 300px]
        let parsed = GridTrackSize::parse_track_list("100px repeat(2, 200px) 300px", false);
        assert_eq!(parsed.len(), 4, "mixed repeat should expand correctly");
    }

    #[test]
    fn grid_template_columns_auto_fill_fit_content() {
        // `repeat(auto-fill, fit-content(200px))` should parse
        let parsed = GridTrackSize::parse_track_list("repeat(auto-fill, fit-content(200px))", false);
        assert!(!parsed.is_empty(), "auto-fill with fit-content should parse");
    }

    #[test]
    fn color_mix_srgb_equal_weights() {
        // color-mix(in srgb, red, blue) → 50% blend → rgb(128, 0, 128)
        let c = parse_color("color-mix(in srgb, red, blue)").expect("should parse");
        assert!(c.r >= 127 && c.r <= 128, "r={}", c.r);
        assert_eq!(c.g, 0, "g");
        assert!(c.b >= 127 && c.b <= 128, "b={}", c.b);
    }

    #[test]
    fn color_mix_with_percentages() {
        // color-mix(in srgb, red 100%, blue 0%) → pure red
        let c = parse_color("color-mix(in srgb, red 100%, blue 0%)").expect("should parse");
        assert_eq!(c.r, 255);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn color_mix_invalid_returns_none() {
        // Missing "in" keyword → None
        assert!(parse_color("color-mix(srgb, red, blue)").is_none());
        // Only 2 comma-separated parts → None
        assert!(parse_color("color-mix(in srgb, red)").is_none());
    }

    // ── ::selection pseudo-element ─────────────────────────────────────────────

    fn make_selection_doc() -> (lumen_dom::Document, lumen_dom::NodeId) {
        let mut doc = lumen_dom::Document::new();
        let root = doc.root();
        let div = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(root, div);
        (doc, div)
    }

    #[test]
    fn selection_style_returns_some_when_rules_match() {
        // ::selection rule with matching div selector
        let css = "div::selection { background-color: #0078D4; color: white; }";
        let sheet = lumen_css_parser::parse(css);
        let (doc, node) = make_selection_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_selection_style(&doc, node, &sheet, &parent, vp, false);
        assert!(result.is_some(), "::selection rules should produce Some(style)");
        let s = result.unwrap();
        // background-color #0078D4 = rgb(0, 120, 212)
        if let Some(CssColor::Rgba(bg)) = s.background_color {
            assert_eq!(bg.r, 0,   "r should be 0");
            assert_eq!(bg.g, 120, "g should be 120");
            assert_eq!(bg.b, 212, "b should be 212");
        } else {
            panic!("background_color should be CssColor::Rgba, got {:?}", s.background_color);
        }
    }

    #[test]
    fn selection_style_returns_none_when_no_rules() {
        // No ::selection rules at all → None
        let sheet = lumen_css_parser::parse("div { color: red; }");
        let (doc, node) = make_selection_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_selection_style(&doc, node, &sheet, &parent, vp, false);
        assert!(result.is_none(), "no ::selection rules → None");
    }

    #[test]
    fn selection_style_no_content_required() {
        // ::selection without 'content' property should still return Some
        let sheet = lumen_css_parser::parse("div::selection { color: green; }");
        let (doc, node) = make_selection_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_selection_style(&doc, node, &sheet, &parent, vp, false);
        assert!(result.is_some(), "::selection without content should still return Some");
        let s = result.unwrap();
        // color: green = rgb(0, 128, 0)
        assert_eq!(s.color.r, 0,   "color red should be 0");
        assert_eq!(s.color.g, 128, "color green should be 128");
    }

    #[test]
    fn selection_style_inherits_font_from_parent() {
        // ::selection should inherit font-size from originating element
        let sheet = lumen_css_parser::parse("div::selection { background-color: yellow; }");
        let (doc, node) = make_selection_doc();
        let mut parent = ComputedStyle::root();
        parent.font_size = 24.0;
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_selection_style(&doc, node, &sheet, &parent, vp, false);
        assert!(result.is_some());
        let s = result.unwrap();
        assert!((s.font_size - 24.0).abs() < 0.01, "font-size should inherit: got {}", s.font_size);
    }

    // ── ::placeholder pseudo-element (CSS Pseudo-Elements L4 §4.10) ────────────

    fn make_placeholder_doc() -> (lumen_dom::Document, lumen_dom::NodeId) {
        let mut doc = lumen_dom::Document::new();
        let root = doc.root();
        let input = doc.create_element(lumen_dom::QualName::html("input"));
        doc.append_child(root, input);
        (doc, input)
    }

    #[test]
    fn placeholder_style_returns_some_when_rules_match() {
        let css = "input::placeholder { color: #808080; opacity: 0.5; }";
        let sheet = lumen_css_parser::parse(css);
        let (doc, node) = make_placeholder_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_pseudo_element_style(&doc, node, "placeholder", &sheet, &parent, vp, false);
        assert!(result.is_some(), "input::placeholder rules should produce Some(style)");
        let s = result.unwrap();
        assert_eq!(s.color.r, 0x80, "color r should be 0x80");
        assert_eq!(s.color.g, 0x80, "color g should be 0x80");
        assert!((s.opacity - 0.5).abs() < 1e-4, "opacity should be 0.5, got {}", s.opacity);
    }

    #[test]
    fn placeholder_style_returns_none_when_no_rules() {
        let sheet = lumen_css_parser::parse("input { color: red; }");
        let (doc, node) = make_placeholder_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_pseudo_element_style(&doc, node, "placeholder", &sheet, &parent, vp, false);
        assert!(result.is_none(), "no input::placeholder rules → None");
    }

    #[test]
    fn placeholder_style_no_content_required() {
        let sheet = lumen_css_parser::parse("input::placeholder { font-style: italic; }");
        let (doc, node) = make_placeholder_doc();
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_pseudo_element_style(&doc, node, "placeholder", &sheet, &parent, vp, false);
        assert!(result.is_some(), "::placeholder without content should still return Some");
        assert_eq!(result.unwrap().font_style, FontStyle::Italic);
    }

    #[test]
    fn placeholder_style_inherits_font_from_parent() {
        let sheet = lumen_css_parser::parse("input::placeholder { color: grey; }");
        let (doc, node) = make_placeholder_doc();
        let mut parent = ComputedStyle::root();
        parent.font_size = 20.0;
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let result = compute_pseudo_element_style(&doc, node, "placeholder", &sheet, &parent, vp, false);
        assert!(result.is_some());
        assert!((result.unwrap().font_size - 20.0).abs() < 0.01);
    }
