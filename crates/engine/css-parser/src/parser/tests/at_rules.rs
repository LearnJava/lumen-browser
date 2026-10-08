use super::*;

    // ──────────────── CSS Variables L1 ────────────────

    #[test]
    fn custom_property_declaration_parsed() {
        // `--name: value` — обычная декларация, имя начинается с `--`.
        let s = parse(":root { --main-color: red; }");
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "--main-color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn var_in_value_preserved_verbatim() {
        // Substitution делает layout, парсер должен сохранить var() в value
        // как есть (вместе с whitespace внутри скобок и fallback после `,`).
        let s = parse("p { color: var(--c, blue); }");
        assert_eq!(s.rules[0].declarations[0].value, "var(--c, blue)");
    }

    #[test]
    fn custom_property_with_complex_value() {
        // Custom property value может содержать что угодно (включая запятые
        // и скобки) — парсер читает до `;` или `}` с уважением к строкам.
        let s = parse(":root { --shadow: 0 2px 4px rgba(0, 0, 0, 0.5); }");
        assert_eq!(
            s.rules[0].declarations[0].value,
            "0 2px 4px rgba(0, 0, 0, 0.5)"
        );
    }

    #[test]
    fn custom_property_important_flag() {
        // `!important` работает и для custom properties.
        let s = parse(":root { --c: red !important; }");
        assert_eq!(s.rules[0].declarations[0].property, "--c");
        assert_eq!(s.rules[0].declarations[0].value, "red");
        assert!(s.rules[0].declarations[0].important);
    }

    // CSS Properties and Values L1 §1.1 — @property

    #[test]
    fn at_property_basic() {
        let s = parse(
            "@property --main-color { syntax: \"*\"; inherits: false; initial-value: red; }",
        );
        assert_eq!(s.properties.len(), 1);
        let p = &s.properties[0];
        assert_eq!(p.name, "--main-color");
        assert_eq!(p.syntax, "*");
        assert!(!p.inherits);
        assert_eq!(p.initial_value.as_deref(), Some("red"));
        assert!(s.rules.is_empty());
    }

    #[test]
    fn at_property_universal_no_initial_value_ok() {
        // syntax="*" разрешает отсутствие initial-value.
        let s = parse("@property --x { syntax: \"*\"; inherits: true; }");
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.properties[0].name, "--x");
        assert!(s.properties[0].inherits);
        assert!(s.properties[0].initial_value.is_none());
    }

    #[test]
    fn at_property_non_universal_without_initial_invalid() {
        // syntax="<length>" без initial-value → @property невалидно.
        let s = parse("@property --w { syntax: \"<length>\"; inherits: false; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_missing_inherits_invalid() {
        let s = parse("@property --x { syntax: \"*\"; initial-value: 0; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_missing_syntax_invalid() {
        let s = parse("@property --x { inherits: true; initial-value: 0; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_name_without_dash_invalid() {
        // Имя без ведущих `--` — невалидно. Парсер съест блок и не зарегистрирует.
        let s = parse("@property foo { syntax: \"*\"; inherits: false; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_inherits_case_insensitive() {
        // CSS Values L4 §2.4: keyword-ы ASCII case-insensitive.
        let s = parse("@property --x { SYNTAX: \"*\"; Inherits: TRUE; Initial-Value: 5px; }");
        assert_eq!(s.properties.len(), 1);
        assert!(s.properties[0].inherits);
        assert_eq!(s.properties[0].initial_value.as_deref(), Some("5px"));
    }

    #[test]
    fn at_property_then_normal_rule() {
        // После @property парсер продолжает разбирать обычные правила.
        let s = parse(
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; }\
             p { color: blue; }",
        );
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn at_property_duplicate_keeps_order() {
        // Две регистрации одного имени — сохраняем обе, последняя побеждает
        // на потребительской стороне (по spec — last wins, реализуем в layout).
        let s = parse(
            "@property --x { syntax: \"*\"; inherits: false; initial-value: 1; }\
             @property --x { syntax: \"*\"; inherits: true; initial-value: 2; }",
        );
        assert_eq!(s.properties.len(), 2);
        assert_eq!(s.properties[0].initial_value.as_deref(), Some("1"));
        assert_eq!(s.properties[1].initial_value.as_deref(), Some("2"));
        assert!(s.properties[1].inherits);
    }

    #[test]
    fn other_at_rule_still_skipped() {
        // Прочие @-правила (media/import/...) синтаксически пропускаются.
        let s = parse("@media (min-width: 100px) { p { color: red; } } p { color: blue; }");
        assert!(s.properties.is_empty());
        // @media тело пропущено целиком — остаётся только последнее `p`-правило.
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn at_property_syntax_single_quotes() {
        let s = parse("@property --c { syntax: '*'; inherits: false; initial-value: red; }");
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.properties[0].syntax, "*");
    }

    // ── @import ──

    #[test]
    fn at_import_url_double_quoted() {
        let s = parse("@import url(\"theme.css\");");
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "theme.css");
        assert!(s.imports[0].media.clauses.is_empty());
    }

    #[test]
    fn at_import_url_single_quoted() {
        let s = parse("@import url('theme.css');");
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_url_unquoted() {
        let s = parse("@import url(theme.css);");
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_bare_string() {
        let s = parse(r#"@import "theme.css";"#);
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_with_media_query() {
        let s = parse(r#"@import url("print.css") print;"#);
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "print.css");
        assert_eq!(s.imports[0].media.clauses.len(), 1);
        assert_eq!(s.imports[0].media.clauses[0].conditions.len(), 1);
        if let MediaCondition::MediaType(t) = &s.imports[0].media.clauses[0].conditions[0] {
            assert_eq!(t, "print");
        } else {
            panic!("expected MediaType");
        }
    }

    #[test]
    fn at_import_with_complex_media() {
        let s = parse(r#"@import url("mobile.css") screen and (max-width: 600px);"#);
        assert_eq!(s.imports[0].url, "mobile.css");
        assert_eq!(s.imports[0].media.clauses.len(), 1);
        // Должны быть MediaType("screen") и Feature(MaxWidth(600)).
        let clause = &s.imports[0].media.clauses[0];
        assert!(!clause.negated);
        assert_eq!(clause.conditions.len(), 2);
    }

    #[test]
    fn at_import_multiple_in_stylesheet() {
        let s = parse(r#"
            @import url("a.css");
            @import "b.css";
            @import url("c.css") screen;
            p { color: red; }
        "#);
        assert_eq!(s.imports.len(), 3);
        assert_eq!(s.imports[0].url, "a.css");
        assert_eq!(s.imports[1].url, "b.css");
        assert_eq!(s.imports[2].url, "c.css");
        // Обычное правило тоже должно распарситься.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_import_invalid_syntax_skipped() {
        // Без URL — должна пропуститься, не сломать остаток.
        let s = parse("@import garbage; p { color: red; }");
        assert!(s.imports.is_empty());
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_import_cyrillic_url() {
        let s = parse(r#"@import url("стили.css");"#);
        assert_eq!(s.imports[0].url, "стили.css");
    }

    // ── @font-face ──

    #[test]
    fn at_font_face_basic() {
        let s = parse(r#"
            @font-face {
                font-family: "Roboto";
                src: url("roboto.woff2") format("woff2");
            }
        "#);
        assert_eq!(s.font_faces.len(), 1);
        assert_eq!(s.font_faces[0].family, "Roboto");
        assert_eq!(s.font_faces[0].sources.len(), 1);
        assert_eq!(s.font_faces[0].sources[0].kind, FontFaceSourceKind::Url);
        assert_eq!(s.font_faces[0].sources[0].value, "roboto.woff2");
        assert_eq!(s.font_faces[0].sources[0].format, Some("woff2".to_string()));
    }

    #[test]
    fn at_font_face_multiple_sources() {
        let s = parse(r#"
            @font-face {
                font-family: "Body";
                src: local("Helvetica"), url("body.woff2") format("woff2"), url("body.ttf") format("truetype");
            }
        "#);
        let srcs = &s.font_faces[0].sources;
        assert_eq!(srcs.len(), 3);
        assert_eq!(srcs[0].kind, FontFaceSourceKind::Local);
        assert_eq!(srcs[0].value, "Helvetica");
        assert_eq!(srcs[0].format, None);
        assert_eq!(srcs[1].kind, FontFaceSourceKind::Url);
        assert_eq!(srcs[1].format, Some("woff2".to_string()));
        assert_eq!(srcs[2].format, Some("truetype".to_string()));
    }

    #[test]
    fn at_font_face_all_descriptors() {
        let s = parse(r#"
            @font-face {
                font-family: "Var";
                src: url("var.woff2");
                font-weight: 100 900;
                font-style: italic;
                font-display: swap;
                unicode-range: U+0000-007F, U+0400-04FF;
            }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.weight, Some("100 900".to_string()));
        assert_eq!(f.style, Some("italic".to_string()));
        assert_eq!(f.display, Some("swap".to_string()));
        assert_eq!(f.unicode_range, Some("U+0000-007F, U+0400-04FF".to_string()));
    }

    #[test]
    fn at_font_face_no_family_skipped() {
        // Без font-family декларации правило невалидно.
        let s = parse(r#"
            @font-face { src: url("x.woff2"); }
            p { color: red; }
        "#);
        assert!(s.font_faces.is_empty());
        // Обычное правило за ним парсится.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_font_face_unquoted_family() {
        // Допустимо: font-family без кавычек.
        let s = parse("@font-face { font-family: Roboto; src: url(r.ttf); }");
        assert_eq!(s.font_faces[0].family, "Roboto");
        assert_eq!(s.font_faces[0].sources[0].value, "r.ttf");
    }

    #[test]
    fn at_font_face_cyrillic_family() {
        let s = parse(r#"
            @font-face { font-family: "Гранит"; src: url("granit.woff2"); }
        "#);
        assert_eq!(s.font_faces[0].family, "Гранит");
    }

    #[test]
    fn at_font_face_stretch_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Condensed";
                src: url("cond.woff2");
                font-stretch: condensed;
            }
        "#);
        assert_eq!(s.font_faces[0].stretch, Some("condensed".to_string()));
    }

    #[test]
    fn at_font_face_stretch_range() {
        // CSS Fonts L4: font-stretch принимает два значения (диапазон).
        let s = parse(r#"
            @font-face {
                font-family: "VarFont";
                src: url("var.woff2");
                font-stretch: 75% 125%;
            }
        "#);
        assert_eq!(s.font_faces[0].stretch, Some("75% 125%".to_string()));
    }

    #[test]
    fn at_font_face_variant_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "SmallCaps";
                src: url("sc.woff2");
                font-variant: small-caps;
            }
        "#);
        assert_eq!(s.font_faces[0].variant, Some("small-caps".to_string()));
    }

    #[test]
    fn at_font_face_feature_settings_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Ligatured";
                src: url("lig.woff2");
                font-feature-settings: "liga" 1, "kern" 0;
            }
        "#);
        assert_eq!(
            s.font_faces[0].feature_settings,
            Some(r#""liga" 1, "kern" 0"#.to_string())
        );
    }

    #[test]
    fn at_font_face_variation_settings_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Variable";
                src: url("variable.woff2");
                font-variation-settings: "wght" 400, "ital" 1;
            }
        "#);
        assert_eq!(
            s.font_faces[0].variation_settings,
            Some(r#""wght" 400, "ital" 1"#.to_string())
        );
    }

    #[test]
    fn at_font_face_all_l4_descriptors() {
        // Полный набор CSS Fonts L4 дескрипторов в одном правиле.
        let s = parse(r#"
            @font-face {
                font-family: "FullSpec";
                src: url("full.woff2") format("woff2");
                font-weight: 100 900;
                font-style: oblique 20deg 50deg;
                font-stretch: 75% 125%;
                font-display: swap;
                unicode-range: U+0000-007F;
                font-variant: small-caps;
                font-feature-settings: "liga" 1;
                font-variation-settings: "wght" 700;
                ascent-override: 90%;
                descent-override: 10%;
                line-gap-override: normal;
                size-adjust: 105%;
            }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.family, "FullSpec");
        assert_eq!(f.weight, Some("100 900".to_string()));
        assert_eq!(f.style, Some("oblique 20deg 50deg".to_string()));
        assert_eq!(f.stretch, Some("75% 125%".to_string()));
        assert_eq!(f.display, Some("swap".to_string()));
        assert_eq!(f.unicode_range, Some("U+0000-007F".to_string()));
        assert_eq!(f.variant, Some("small-caps".to_string()));
        assert_eq!(f.feature_settings, Some("\"liga\" 1".to_string()));
        assert_eq!(f.variation_settings, Some("\"wght\" 700".to_string()));
        assert_eq!(f.ascent_override, Some("90%".to_string()));
        assert_eq!(f.descent_override, Some("10%".to_string()));
        assert_eq!(f.line_gap_override, Some("normal".to_string()));
        assert_eq!(f.size_adjust, Some("105%".to_string()));
    }

    #[test]
    fn at_font_face_override_descriptors() {
        let s = parse(r#"
            @font-face {
                font-family: "Overridden";
                src: url("o.woff2");
                ascent-override: 80%;
                descent-override: 20%;
                line-gap-override: 0%;
                size-adjust: 90%;
            }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.ascent_override, Some("80%".to_string()));
        assert_eq!(f.descent_override, Some("20%".to_string()));
        assert_eq!(f.line_gap_override, Some("0%".to_string()));
        assert_eq!(f.size_adjust, Some("90%".to_string()));
    }

    #[test]
    fn at_font_face_override_descriptors_absent_by_default() {
        let s = parse(r#"
            @font-face { font-family: "Plain"; src: url("p.woff2"); }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.ascent_override, None);
        assert_eq!(f.descent_override, None);
        assert_eq!(f.line_gap_override, None);
        assert_eq!(f.size_adjust, None);
    }

    #[test]
    fn split_top_level_commas_respects_parens_and_strings() {
        // Запятые внутри (...) и "..." не должны разделять.
        assert_eq!(
            split_top_level_commas("a, b(c, d), e \"f, g\", h"),
            vec!["a", " b(c, d)", " e \"f, g\"", " h"]
        );
    }

    #[test]
    fn parse_font_face_src_local_only() {
        let srcs = parse_font_face_src("local(\"Times New Roman\")");
        assert_eq!(srcs.len(), 1);
        assert_eq!(srcs[0].kind, FontFaceSourceKind::Local);
        assert_eq!(srcs[0].value, "Times New Roman");
        assert_eq!(srcs[0].format, None);
    }

    // ── @layer (CSS Cascade L5 §6.4) ──

    #[test]
    fn at_layer_statement_form_single_name() {
        let s = parse("@layer base;");
        assert_eq!(s.layer_order, vec!["base".to_string()]);
        assert!(s.layers.is_empty());
    }

    #[test]
    fn at_layer_statement_form_multiple_names() {
        let s = parse("@layer base, components, utilities;");
        assert_eq!(
            s.layer_order,
            vec!["base".to_string(), "components".to_string(), "utilities".to_string()]
        );
    }

    #[test]
    fn at_layer_block_form_with_name() {
        let s = parse(r#"
            @layer base {
                p { color: red; }
            }
        "#);
        assert_eq!(s.layer_order, vec!["base".to_string()]);
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "base");
        assert_eq!(s.layers[0].rules.len(), 1);
    }

    #[test]
    fn at_layer_block_form_anonymous() {
        let s = parse(r#"
            @layer {
                p { color: red; }
            }
        "#);
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "__anon_1__");
        assert_eq!(s.layer_order, vec!["__anon_1__".to_string()]);
    }

    #[test]
    fn at_layer_block_does_not_duplicate_in_order() {
        // Если статикой объявили `@layer base;`, а потом блок `@layer base { ... }`,
        // имя в layer_order должно быть один раз (idempotent insert).
        let s = parse(r#"
            @layer base;
            @layer base { p { color: red; } }
        "#);
        assert_eq!(s.layer_order, vec!["base".to_string()]);
    }

    #[test]
    fn at_layer_multiple_anon_blocks_get_unique_names() {
        let s = parse(r#"
            @layer { p { color: red; } }
            @layer { p { color: blue; } }
        "#);
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].name, "__anon_1__");
        assert_eq!(s.layers[1].name, "__anon_2__");
    }

    #[test]
    fn at_layer_mixed_form_order_preserved() {
        let s = parse(r#"
            @layer reset, base;
            @layer components { p { color: blue; } }
            @layer base { p { color: red; } }
        "#);
        // layer_order сохраняет порядок _первого_ упоминания.
        assert_eq!(
            s.layer_order,
            vec![
                "reset".to_string(),
                "base".to_string(),
                "components".to_string(),
            ]
        );
        // А layers содержит block-form правил (2 шт).
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].name, "components");
        assert_eq!(s.layers[1].name, "base");
    }

    #[test]
    fn at_layer_dotted_subname_ok() {
        // sub-layer-имя `base.text` — валидно.
        let s = parse("@layer base.text;");
        assert_eq!(s.layer_order, vec!["base.text".to_string()]);
    }

    #[test]
    fn at_layer_unlayered_rules_kept_separately() {
        let s = parse(r#"
            @layer base { p { color: red; } }
            div { color: blue; }
        "#);
        // Layered: p in base.
        assert_eq!(s.layers.len(), 1);
        // Unlayered: top-level div.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_layer_invalid_name_skipped() {
        // `1invalid` начинается с цифры → не CSS-ident → пропускается.
        let s = parse("@layer 1invalid, valid;");
        assert_eq!(s.layer_order, vec!["valid".to_string()]);
    }

    // ── @import layer() / supports() — CSS Cascade L5 §6.5 ──

    #[test]
    fn at_import_without_modifiers_has_no_layer_or_supports() {
        let s = parse(r#"@import "a.css";"#);
        assert_eq!(s.imports[0].layer, None);
        assert_eq!(s.imports[0].supports, None);
    }

    #[test]
    fn at_import_named_layer() {
        let s = parse(r#"@import url("a.css") layer(base);"#);
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "a.css");
        assert_eq!(s.imports[0].layer, Some(ImportLayer::Named("base".to_string())));
        assert!(s.imports[0].media.clauses.is_empty());
    }

    #[test]
    fn at_import_dotted_layer_name() {
        let s = parse(r#"@import "a.css" layer( framework.base );"#);
        assert_eq!(s.imports[0].layer, Some(ImportLayer::Named("framework.base".to_string())));
    }

    #[test]
    fn at_import_anonymous_layer() {
        let s = parse(r#"@import "a.css" layer;"#);
        assert_eq!(s.imports[0].layer, Some(ImportLayer::Anonymous));
        let s = parse(r#"@import "a.css" LAYER print;"#);
        assert_eq!(s.imports[0].layer, Some(ImportLayer::Anonymous));
        assert_eq!(s.imports[0].media.clauses.len(), 1);
    }

    #[test]
    fn at_import_layer_with_supports_and_media() {
        let s = parse(r#"@import url(a.css) layer(x) supports(display: grid) screen and (min-width: 600px);"#);
        let imp = &s.imports[0];
        assert_eq!(imp.layer, Some(ImportLayer::Named("x".to_string())));
        assert_eq!(
            imp.supports,
            Some(SupportsCondition::Decl { property: "display".into(), value: "grid".into() })
        );
        assert_eq!(imp.media.clauses.len(), 1);
    }

    #[test]
    fn at_import_supports_condition_form() {
        let s = parse(r#"@import "a.css" supports(not (display: grid));"#);
        assert!(matches!(s.imports[0].supports, Some(SupportsCondition::Not(_))));
        assert_eq!(s.imports[0].layer, None);
    }

    #[test]
    fn at_import_invalid_layer_name_drops_import() {
        let s = parse(r#"@import "a.css" layer(1bad); p { color: red; }"#);
        assert!(s.imports.is_empty());
        assert_eq!(s.rules.len(), 1, "following rule must survive");
        let s = parse(r#"@import "a.css" layer(); p { color: red; }"#);
        assert!(s.imports.is_empty());
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_import_layer_prefix_is_not_a_media_type() {
        // `layered` — не модификатор `layer`, а обычный media-токен.
        let s = parse(r#"@import "a.css" layered;"#);
        assert_eq!(s.imports[0].layer, None);
        assert_eq!(s.imports[0].media.clauses.len(), 1);
    }

    // ── @layer: вложенное содержимое ──

    #[test]
    fn nested_layer_gets_dotted_name() {
        let s = parse("@layer outer { @layer inner { p { color: red; } } a { color: blue; } }");
        assert_eq!(s.layer_order, vec!["outer".to_string(), "outer.inner".to_string()]);
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].name, "outer");
        assert_eq!(s.layers[0].rules.len(), 1);
        assert_eq!(s.layers[1].name, "outer.inner");
        assert_eq!(s.layers[1].rules.len(), 1);
    }

    #[test]
    fn nested_layer_statement_form_declares_dotted_names() {
        let s = parse("@layer outer { @layer a, b; }");
        assert_eq!(
            s.layer_order,
            vec!["outer".to_string(), "outer.a".to_string(), "outer.b".to_string()]
        );
    }

    #[test]
    fn media_inside_layer_stays_in_layer() {
        let s = parse("@layer base { p { color: red; } @media (min-width: 600px) { p { color: blue; } } }");
        assert!(s.media_rules.is_empty());
        assert_eq!(s.layers.len(), 2);
        assert!(s.layers[0].condition.is_none());
        assert_eq!(s.layers[1].name, "base");
        assert!(matches!(s.layers[1].condition, Some(LayerCondition::Media(_))));
        assert_eq!(s.layers[1].rules.len(), 1);
    }

    #[test]
    fn supports_inside_layer_stays_in_layer() {
        let s = parse("@layer base { @supports (display: grid) { p { color: blue; } } }");
        assert!(s.supports_rules.is_empty());
        assert!(matches!(s.layers[1].condition, Some(LayerCondition::Supports(_))));
    }

    #[test]
    fn layer_independent_at_rules_inside_layer_are_hoisted() {
        let s = parse(
            "@layer base { @font-face { font-family: X; src: url(x.woff2); } \
             @keyframes k { from { opacity: 0 } to { opacity: 1 } } p { color: red; } }",
        );
        assert_eq!(s.font_faces.len(), 1);
        assert_eq!(s.keyframes.len(), 1);
        assert_eq!(s.layers[0].rules.len(), 1);
    }

    #[test]
    fn is_layer_name_basic() {
        assert!(is_layer_name("base"));
        assert!(is_layer_name("base.text"));
        assert!(is_layer_name("_priv"));
        assert!(!is_layer_name("1invalid"));
        assert!(!is_layer_name(""));
        assert!(!is_layer_name("with space"));
    }

    // ── CSS Conditional Rules L3 §2 — @supports ──

    #[test]
    fn at_supports_simple_decl() {
        let s = parse("@supports (display: grid) { p { color: red; } }");
        assert_eq!(s.supports_rules.len(), 1);
        let r = &s.supports_rules[0];
        match &r.condition {
            SupportsCondition::Decl { property, value } => {
                assert_eq!(property, "display");
                assert_eq!(value, "grid");
            }
            other => panic!("expected Decl, got {other:?}"),
        }
        assert_eq!(r.rules.len(), 1);
    }

    #[test]
    fn at_supports_and_combinator() {
        let s = parse("@supports (display: grid) and (color: red) { p { color: red; } }");
        let r = &s.supports_rules[0];
        match &r.condition {
            SupportsCondition::And(terms) => assert_eq!(terms.len(), 2),
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_or_combinator() {
        let s = parse("@supports (display: flex) or (display: -webkit-flex) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Or(terms) => assert_eq!(terms.len(), 2),
            other => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_negation() {
        let s = parse("@supports not (display: pancake) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Not(inner) => match inner.as_ref() {
                SupportsCondition::Decl { property, .. } => assert_eq!(property, "display"),
                other => panic!("expected Decl inside Not, got {other:?}"),
            },
            other => panic!("expected Not, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_selector_test() {
        let s = parse("@supports selector(:has(a)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Selector(sel) => assert!(sel.contains(":has(a)")),
            other => panic!("expected Selector, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_evaluate_known_property() {
        let cond = parse_supports_condition("(display: grid)");
        assert!(cond.evaluate(&["display", "color"]));
        assert!(!cond.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_evaluate_custom_property_always_supported() {
        // BUG-501 gap 2: a `--`-prefixed custom property must always
        // evaluate as supported, regardless of `known_properties`.
        let cond = parse_supports_condition("(--my-var: revert-rule)");
        assert!(cond.evaluate(&[]));
        assert!(cond.evaluate(&["display", "color"]));
    }

    #[test]
    fn at_supports_evaluate_custom_property_in_combinators() {
        let and = parse_supports_condition("(--x: 1) and (display: grid)");
        assert!(and.evaluate(&["display"]));
        assert!(!and.evaluate(&[])); // display still unsupported here

        let or = parse_supports_condition("(unknown: x) or (--y: 1)");
        assert!(or.evaluate(&[]));
    }

    #[test]
    fn at_supports_evaluate_and() {
        let cond = parse_supports_condition("(display: grid) and (color: red)");
        assert!(cond.evaluate(&["display", "color"]));
        assert!(!cond.evaluate(&["display"]));
    }

    #[test]
    fn at_supports_evaluate_or() {
        let cond = parse_supports_condition("(unknown: x) or (color: red)");
        assert!(cond.evaluate(&["color"]));
        assert!(!cond.evaluate(&["other"]));
    }

    #[test]
    fn at_supports_evaluate_not() {
        let cond = parse_supports_condition("not (unknown: x)");
        assert!(cond.evaluate(&["color"]));
        let cond2 = parse_supports_condition("not (color: red)");
        assert!(!cond2.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_evaluate_selector_supported() {
        // Распознаваемые селекторы → true (known_properties не используется).
        for sel in [
            "selector(:has(a))",
            "selector(:is(.a, .b))",
            "selector(:where(div > p))",
            "selector(:not(.x))",
            "selector(a:hover)",
            "selector(:nth-child(2n+1 of .item))",
            "selector(::before)",
            "selector(::slotted(span))",
            "selector(div.cls#id[attr^=\"v\"] > p + q ~ r)",
        ] {
            let cond = parse_supports_condition(sel);
            assert!(cond.evaluate(&[]), "{sel} should be supported");
        }
    }

    #[test]
    fn at_supports_evaluate_selector_unsupported() {
        // Нераспознанные псевдо → false, даже если вложены в :is()/:has().
        for sel in [
            "selector(:totally-fake)",
            "selector(::made-up)",
            "selector(:is(.ok, :totally-fake))",
            "selector(:has(:totally-fake))",
            "selector(:not(::made-up))",
        ] {
            let cond = parse_supports_condition(sel);
            assert!(!cond.evaluate(&[]), "{sel} should be unsupported");
        }
    }

    #[test]
    fn complex_selector_is_supported_recurses() {
        // Прямая проверка ComplexSelector::is_supported на вложенности.
        let ok = parse_selector_list(":is(.a, :where(.b:has(> .c)))");
        assert!(ok.iter().all(ComplexSelector::is_supported));
        let bad = parse_selector_list(":is(.a, :where(:totally-fake))");
        assert!(!bad.iter().all(ComplexSelector::is_supported));
    }

    #[test]
    fn at_supports_font_tech_parse() {
        // `font-tech(<font-tech>)` типизируется в FontTech, аргумент lowercase.
        let s = parse("@supports font-tech(VARIATIONS) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::FontTech(t) => assert_eq!(t, "variations"),
            other => panic!("expected FontTech, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_font_format_parse_keyword_and_string() {
        // Keyword-форма.
        match &parse("@supports font-format(woff2) { p { x: y; } }").supports_rules[0].condition {
            SupportsCondition::FontFormat(f) => assert_eq!(f, "woff2"),
            other => panic!("expected FontFormat, got {other:?}"),
        }
        // Legacy-строковая форма — кавычки снимаются.
        match &parse("@supports font-format(\"opentype\") { p { x: y; } }").supports_rules[0].condition {
            SupportsCondition::FontFormat(f) => assert_eq!(f, "opentype"),
            other => panic!("expected FontFormat, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_evaluate_font_tech() {
        // Реализованные технологии → true.
        assert!(parse_supports_condition("font-tech(variations)").evaluate(&[]));
        assert!(parse_supports_condition("font-tech(features-opentype)").evaluate(&[]));
        // Нереализованные (цветные глифы, палитры, AAT, incremental) → false.
        for t in [
            "font-tech(color-colrv1)",
            "font-tech(color-svg)",
            "font-tech(palettes)",
            "font-tech(features-aat)",
            "font-tech(incremental)",
            "font-tech(totally-fake)",
        ] {
            assert!(!parse_supports_condition(t).evaluate(&[]), "{t} must be unsupported");
        }
    }

    #[test]
    fn at_supports_evaluate_font_format() {
        // Декодируемые форматы → true.
        for f in [
            "font-format(truetype)",
            "font-format(opentype)",
            "font-format(woff)",
            "font-format(woff2)",
            "font-format(\"woff2\")",
        ] {
            assert!(parse_supports_condition(f).evaluate(&[]), "{f} must be supported");
        }
        // Неподдержанные контейнеры/форматы → false.
        for f in [
            "font-format(collection)",
            "font-format(embedded-opentype)",
            "font-format(svg)",
            "font-format(totally-fake)",
        ] {
            assert!(!parse_supports_condition(f).evaluate(&[]), "{f} must be unsupported");
        }
    }

    #[test]
    fn at_supports_font_tech_format_in_combinators() {
        // Комбинируются с and/or/not как обычные условия.
        assert!(parse_supports_condition("font-format(woff2) and font-tech(variations)").evaluate(&[]));
        assert!(!parse_supports_condition("font-format(woff2) and font-tech(palettes)").evaluate(&[]));
        assert!(parse_supports_condition("font-format(svg) or font-format(woff2)").evaluate(&[]));
        assert!(parse_supports_condition("not font-tech(color-colrv1)").evaluate(&[]));
    }

    #[test]
    fn at_supports_nested_grouping() {
        // `((display: grid))` — внутренние скобки = nested condition.
        let s = parse("@supports ((display: grid)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Decl { property, .. } => assert_eq!(property, "display"),
            other => panic!("expected Decl after unwrapping, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_value_with_parens_balanced() {
        let s = parse("@supports (color: rgba(0, 0, 0, 0.5)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Decl { property, value } => {
                assert_eq!(property, "color");
                assert!(value.contains("rgba"));
            }
            other => panic!("expected Decl, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_evaluator_selector_recognized() {
        // selector() оценивается по распознаваемости селектора (CSS Conditional
        // L4 §4.2); known_properties здесь не участвует. `:has(a)` поддержан.
        let cond = parse_supports_condition("selector(:has(a))");
        assert!(cond.evaluate(&["color"]));
        // Нераспознанный псевдо → false.
        let bad = parse_supports_condition("selector(:totally-fake)");
        assert!(!bad.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_empty_returns_unknown() {
        let cond = parse_supports_condition("");
        assert!(matches!(cond, SupportsCondition::Unknown));
        assert!(!cond.evaluate(&["color"]));
    }

    // ── CSS Animations L1 §3 — @keyframes ──

    #[test]
    fn at_keyframes_from_to() {
        let s = parse("@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(s.keyframes.len(), 1);
        let kf = &s.keyframes[0];
        assert_eq!(kf.name, "fade");
        assert_eq!(kf.frames.len(), 2);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 1.0).abs() < 1e-6);
        assert_eq!(kf.frames[0].declarations[0].property, "opacity");
    }

    #[test]
    fn at_keyframes_percentages() {
        let s = parse("@keyframes pulse { 0% { color: red; } 50% { color: blue; } 100% { color: red; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 3);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 0.5).abs() < 1e-6);
        assert!((kf.frames[2].offset - 1.0).abs() < 1e-6);
    }

    #[test]
    fn at_keyframes_multiple_offsets_per_frame() {
        // `0%, 50%` — один блок с двумя offset-ами, разворачивается.
        let s = parse("@keyframes z { 0%, 50% { color: red; } 100% { color: blue; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 3);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 0.5).abs() < 1e-6);
        // Декларации одинаковые между развёрнутыми frame-ами.
        assert_eq!(kf.frames[0].declarations[0].value, "red");
        assert_eq!(kf.frames[1].declarations[0].value, "red");
    }

    #[test]
    fn at_keyframes_webkit_prefix() {
        let s = parse("@-webkit-keyframes fade { from { x: 0; } }");
        assert_eq!(s.keyframes.len(), 1);
        assert_eq!(s.keyframes[0].name, "fade");
    }

    #[test]
    fn at_keyframes_invalid_offset_skipped() {
        // 150% > 100% → пропускается.
        let s = parse("@keyframes z { 0% { x: 1; } 150% { x: 2; } 100% { x: 3; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 2);
    }

    #[test]
    fn at_keyframes_empty_block() {
        let s = parse("@keyframes z { }");
        assert_eq!(s.keyframes.len(), 1);
        assert_eq!(s.keyframes[0].frames.len(), 0);
    }

    #[test]
    fn parse_keyframe_selectors_handles_keywords_and_percents() {
        assert_eq!(parse_keyframe_selectors("from"), vec![0.0]);
        assert_eq!(parse_keyframe_selectors("to"), vec![1.0]);
        assert_eq!(parse_keyframe_selectors("From"), vec![0.0]); // case-insensitive
        assert_eq!(parse_keyframe_selectors("0%, 50%, 100%"), vec![0.0, 0.5, 1.0]);
        assert_eq!(parse_keyframe_selectors("bogus"), Vec::<f32>::new());
        assert_eq!(parse_keyframe_selectors("-10%"), Vec::<f32>::new());
        assert_eq!(parse_keyframe_selectors("150%"), Vec::<f32>::new());
    }

    // ── CSS Counter Styles L3 §2 — @counter-style ──

    #[test]
    fn at_counter_style_basic() {
        let s = parse(
            "@counter-style thumbs { system: cyclic; symbols: \"\\1F44D\"; suffix: \" \"; }",
        );
        assert_eq!(s.counter_styles.len(), 1);
        let cs = &s.counter_styles[0];
        assert_eq!(cs.name, "thumbs");
        assert_eq!(cs.declarations.len(), 3);
        assert_eq!(cs.declarations[0].property, "system");
    }

    #[test]
    fn at_counter_style_empty_block() {
        let s = parse("@counter-style empty { }");
        assert_eq!(s.counter_styles.len(), 1);
        assert!(s.counter_styles[0].declarations.is_empty());
    }

    // ── CSS Paged Media L3 §3 — @page ──

    #[test]
    fn at_page_no_selector() {
        let s = parse("@page { margin: 2cm; }");
        assert_eq!(s.page_rules.len(), 1);
        let p = &s.page_rules[0];
        assert!(p.selector.is_empty());
        assert_eq!(p.declarations[0].property, "margin");
    }

    #[test]
    fn at_page_pseudo_selector() {
        let s = parse("@page :first { margin-top: 4cm; }");
        let p = &s.page_rules[0];
        assert_eq!(p.selector, ":first");
        assert_eq!(p.declarations.len(), 1);
    }

    #[test]
    fn at_page_named_selector() {
        let s = parse("@page cover :left { margin: 0; }");
        assert_eq!(s.page_rules[0].selector, "cover :left");
    }

    // ── CSS Cascade L6 — @scope ──

    #[test]
    fn at_scope_root_only() {
        let s = parse("@scope (.card) { h1 { color: red; } }");
        assert_eq!(s.scope_rules.len(), 1);
        let sc = &s.scope_rules[0];
        assert_eq!(sc.root, ".card");
        assert_eq!(sc.limit, None);
        assert_eq!(sc.rules.len(), 1);
    }

    #[test]
    fn at_scope_root_and_limit() {
        let s = parse("@scope (.card) to (.footer) { p { color: blue; } }");
        let sc = &s.scope_rules[0];
        assert_eq!(sc.root, ".card");
        assert_eq!(sc.limit.as_deref(), Some(".footer"));
    }

    #[test]
    fn at_scope_implicit() {
        let s = parse("@scope { h1 { color: red; } }");
        let sc = &s.scope_rules[0];
        assert!(sc.root.is_empty());
        assert_eq!(sc.limit, None);
        assert_eq!(sc.rules.len(), 1);
    }

    // ── CSS Transitions L2 §3.4 — @starting-style ──

    #[test]
    fn at_starting_style_basic() {
        let s = parse("@starting-style { dialog { opacity: 0; } }");
        assert_eq!(s.starting_style_rules.len(), 1);
        assert_eq!(s.starting_style_rules[0].rules.len(), 1);
    }

    #[test]
    fn at_starting_style_empty() {
        let s = parse("@starting-style { }");
        assert_eq!(s.starting_style_rules.len(), 1);
        assert!(s.starting_style_rules[0].rules.is_empty());
    }

    // ── CSS Containment L3 §3 — @container ──

    #[test]
    fn at_container_anonymous() {
        let s = parse("@container (min-width: 300px) { p { color: red; } }");
        assert_eq!(s.container_rules.len(), 1);
        let c = &s.container_rules[0];
        assert_eq!(c.name, None);
        assert!(c.condition.contains("min-width"));
        assert_eq!(c.rules.len(), 1);
    }

    #[test]
    fn at_container_named() {
        let s = parse("@container sidebar (min-width: 200px) { h1 { color: blue; } }");
        let c = &s.container_rules[0];
        assert_eq!(c.name.as_deref(), Some("sidebar"));
    }

    #[test]
    fn at_container_complex_condition() {
        let s = parse("@container (min-width: 200px) and (max-width: 600px) { p { } }");
        let c = &s.container_rules[0];
        assert!(c.condition.contains("and"));
    }

    // ── Nested at-rules (2nd pass): @scope / @container внутри тела ──

    #[test]
    fn nested_scope_inside_rule() {
        // CSS Cascade L6 §3 + Nesting L1 §5: `@scope` внутри qualified-правила
        // всплывает на stylesheet-уровень как ScopeRule; декларации сворачиваются
        // в правило с родительским селектором.
        let s = parse(".card { color: black; @scope (.x) to (.y) { color: red; } }");
        assert_eq!(s.scope_rules.len(), 1);
        let sc = &s.scope_rules[0];
        assert_eq!(sc.root, ".x");
        assert_eq!(sc.limit.as_deref(), Some(".y"));
        assert_eq!(sc.rules.len(), 1);
        // Bare-декларация `color: red` привязана к родителю `.card`.
        assert_eq!(sc.rules[0].declarations[0].property, "color");
    }

    #[test]
    fn nested_scope_implicit_root() {
        // `@scope { ... }` без `(<root>)` — implicit `:scope`, root пустой.
        let s = parse(".box { @scope { .inner { color: blue; } } }");
        assert_eq!(s.scope_rules.len(), 1);
        let sc = &s.scope_rules[0];
        assert!(sc.root.is_empty());
        assert_eq!(sc.limit, None);
        assert_eq!(sc.rules.len(), 1);
    }

    #[test]
    fn nested_container_parses_name() {
        // Вложенный `@container <name> (...)` должен разбирать имя, а не хардкодить None.
        let s = parse(".card { @container sidebar (min-width: 200px) { color: green; } }");
        assert_eq!(s.container_rules.len(), 1);
        let c = &s.container_rules[0];
        assert_eq!(c.name.as_deref(), Some("sidebar"));
        assert!(c.condition.contains("min-width"));
        assert_eq!(c.rules.len(), 1);
    }

    #[test]
    fn nested_container_anonymous() {
        // Вложенный `@container (...)` без имени — name == None.
        let s = parse(".card { @container (min-width: 300px) { color: red; } }");
        let c = &s.container_rules[0];
        assert_eq!(c.name, None);
        assert_eq!(c.rules.len(), 1);
    }

    #[test]
    fn top_level_container_with_nested_media() {
        // CSS Containment L3 §3: top-level `@container` рекурсирует во вложенный
        // `@media` — тот всплывает на stylesheet-уровень (плоская модель).
        let s = parse("@container (min-width: 300px) { @media (min-width: 500px) { p { color: red; } } }");
        assert_eq!(s.container_rules.len(), 1);
        // Вложенный @media более не выбрасывается.
        assert_eq!(s.media_rules.len(), 1);
        assert_eq!(s.media_rules[0].rules.len(), 1);
    }

    #[test]
    fn top_level_container_with_nested_scope() {
        // Top-level `@container` c вложенным `@scope` — scope всплывает.
        let s = parse("@container sidebar (min-width: 200px) { @scope (.x) { h1 { color: red; } } }");
        assert_eq!(s.container_rules[0].name.as_deref(), Some("sidebar"));
        assert_eq!(s.scope_rules.len(), 1);
        assert_eq!(s.scope_rules[0].root, ".x");
    }

    #[test]
    fn top_level_container_named_direct_rules_still_work() {
        // Регрессия: тело top-level @container с type-селекторами не должно
        // ломаться от новой обработки nested at-rules.
        let s = parse("@container sidebar (min-width: 200px) { h1 { color: blue; } p { color: green; } }");
        let c = &s.container_rules[0];
        assert_eq!(c.name.as_deref(), Some("sidebar"));
        assert_eq!(c.rules.len(), 2);
    }

    #[test]
    fn top_level_container_with_nested_supports_and_layer() {
        // Регрессия: голый type-селектор (`p`) внутри `@supports`/`@layer`,
        // вложенных в `@container`, не должен теряться — та же bare-group-body
        // грамматика, что и для `@media` (см. `top_level_container_with_nested_media`).
        let s = parse(
            "@container (min-width: 300px) { \
                @supports (display: grid) { p { color: red; } } \
                @layer base { h1 { color: blue; } } \
            }",
        );
        assert_eq!(s.supports_rules.len(), 1);
        assert_eq!(s.supports_rules[0].rules.len(), 1);
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].rules.len(), 1);
    }

    #[test]
    fn inline_style_single_declaration() {
        let decls = parse_inline_style("color: red");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "red");
        assert!(!decls[0].important);
    }

    #[test]
    fn inline_style_multiple_declarations_with_trailing_semicolon() {
        let decls = parse_inline_style("color: red; background: #fff; padding: 5px 10px;");
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[1].property, "background");
        assert_eq!(decls[1].value, "#fff");
        assert_eq!(decls[2].property, "padding");
        assert_eq!(decls[2].value, "5px 10px");
    }

    #[test]
    fn inline_style_no_trailing_semicolon() {
        let decls = parse_inline_style("width: 100px; height: 50px");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[1].property, "height");
        assert_eq!(decls[1].value, "50px");
    }

    #[test]
    fn inline_style_important_flag() {
        let decls = parse_inline_style("color: red !important");
        assert_eq!(decls.len(), 1);
        assert!(decls[0].important);
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn inline_style_empty_input() {
        assert!(parse_inline_style("").is_empty());
        assert!(parse_inline_style("   ").is_empty());
        assert!(parse_inline_style(";;;").is_empty());
    }

    #[test]
    fn inline_style_recovers_from_invalid_declaration() {
        let decls = parse_inline_style("color: red; garbage no colon here; background: blue");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[1].property, "background");
    }

    #[test]
    fn inline_style_with_url_and_quotes() {
        let decls = parse_inline_style(
            r#"background-image: url("a;b.png"); content: 'hi; there'"#,
        );
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].value, r#"url("a;b.png")"#);
        assert_eq!(decls[1].value, "'hi; there'");
    }

    // ──────────────── BUG-793: at-rule без блока, закрытое `;` ────────────────
    // CSS Syntax L3 §5.4.2: `<semicolon-token>` завершает at-правило без
    // блока и разбор продолжается со следующего правила — `@media;` и
    // однотипные не должны съедать хвост таблицы стилей.

    #[test]
    fn media_without_block_does_not_eat_tail() {
        let s = parse("#a{width:100px} @media; #b{width:200px} #c{width:300px}");
        assert!(s.media_rules.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[0].declarations[0].value, "100px");
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn supports_without_block_does_not_eat_tail() {
        let s = parse("#a{width:100px} @supports; #b{width:200px} #c{width:300px}");
        assert!(s.supports_rules.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn keyframes_without_block_does_not_eat_tail() {
        let s = parse("#a{width:100px} @keyframes; #b{width:200px} #c{width:300px}");
        assert!(s.keyframes.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn keyframes_named_without_block_does_not_eat_tail() {
        // `@keyframes foo;` — name present, still no block.
        let s = parse("#a{width:100px} @keyframes foo; #b{width:200px} #c{width:300px}");
        assert!(s.keyframes.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn container_without_block_does_not_eat_tail() {
        let s = parse("#a{width:100px} @container; #b{width:200px} #c{width:300px}");
        assert!(s.container_rules.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn scope_without_block_does_not_eat_tail() {
        let s = parse("#a{width:100px} @scope; #b{width:200px} #c{width:300px}");
        assert!(s.scope_rules.is_empty());
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
        assert_eq!(s.rules[2].declarations[0].value, "300px");
    }

    #[test]
    fn supports_with_semicolon_inside_parens_not_treated_as_terminator() {
        // `;` внутри скобок не образует точку с запятой верхнего уровня —
        // оговорка к фиксу BUG-793.
        let s = parse("@supports (margin: 0; padding: 0) { #b{width:200px} }");
        assert_eq!(s.supports_rules.len(), 1);
        assert_eq!(s.supports_rules[0].rules.len(), 1);
    }

    #[test]
    fn nested_media_without_block_does_not_eat_tail() {
        // CSS Nesting L1 §5: same terminator rule applies to a nested at-rule
        // inside a qualified rule's own body.
        let s = parse(".p{ @media; } #b{width:200px}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].value, "200px");
    }

