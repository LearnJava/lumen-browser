//! Тесты `canonical_specified_text` — грамматика свойств CSS Text в inline-`style` CSSOM (BUG-1325).

    use crate::style::canonical_specified_text;

    fn canon(prop: &str, v: &str) -> Option<String> {
        canonical_specified_text(prop, v)
    }

    #[test]
    fn tab_size_accepts_number_or_non_negative_length() {
        assert_eq!(canon("tab-size", "0").as_deref(), Some("0"));
        assert_eq!(canon("tab-size", "2.5").as_deref(), Some("2.5"));
        assert_eq!(canon("tab-size", "10px").as_deref(), Some("10px"));
        assert_eq!(canon("tab-size", "calc(2em + 3ex)").as_deref(), Some("calc(2em + 3ex)"));
        for bad in ["-10px", "-20", "30%", "calc(40% + 50px)", "auto", "4 4"] {
            assert!(canon("tab-size", bad).is_none(), "{bad:?} должно быть отклонено");
        }
    }

    #[test]
    fn spacing_is_normal_or_length_percentage() {
        for prop in ["letter-spacing", "word-spacing"] {
            assert_eq!(canon(prop, "normal").as_deref(), Some("normal"));
            assert_eq!(canon(prop, "0").as_deref(), Some("0px"));
            assert_eq!(canon(prop, "-10px").as_deref(), Some("-10px"));
            assert_eq!(canon(prop, "120%").as_deref(), Some("120%"));
            assert_eq!(canon(prop, "calc(2ch - 30%)").as_deref(), Some("calc(-30% + 2ch)"));
            for bad in ["auto", "20", "normal 10px", "10% 10px"] {
                assert!(canon(prop, bad).is_none(), "{prop}: {bad:?} должно быть отклонено");
            }
        }
    }

    #[test]
    fn text_indent_orders_modifiers_after_length() {
        assert_eq!(canon("text-indent", "10px").as_deref(), Some("10px"));
        assert_eq!(canon("text-indent", "hanging calc(50% + 60px)").as_deref(), Some("calc(50% + 60px) hanging"));
        assert_eq!(canon("text-indent", "each-line 10px").as_deref(), Some("10px each-line"));
        assert_eq!(canon("text-indent", "calc(50% + 60px) each-line hanging").as_deref(), Some("calc(50% + 60px) hanging each-line"));
        for bad in ["auto", "hanging", "each-line", "10", "10px hanging 20px", "hanging 20% hanging", "each-line each-line"] {
            assert!(canon("text-indent", bad).is_none(), "{bad:?} должно быть отклонено");
        }
    }

    #[test]
    fn text_transform_orders_components() {
        assert_eq!(canon("text-transform", "none").as_deref(), Some("none"));
        assert_eq!(canon("text-transform", "math-auto").as_deref(), Some("math-auto"));
        assert_eq!(canon("text-transform", "full-width lowercase").as_deref(), Some("lowercase full-width"));
        assert_eq!(
            canon("text-transform", "full-size-kana full-width capitalize").as_deref(),
            Some("capitalize full-width full-size-kana")
        );
        for bad in ["none uppercase", "uppercase lowercase", "full-width full-width", "math-auto full-width", "auto"] {
            assert!(canon("text-transform", bad).is_none(), "{bad:?} должно быть отклонено");
        }
    }
