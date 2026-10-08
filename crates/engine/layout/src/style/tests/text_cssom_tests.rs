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

/// BUG-1329 — языковые правила регистра `text-transform` (CSS Text L3 §2.1).
mod case_lang {
    use crate::style::{transform_text_by_value as t, CaseLang, TextTransform};

    #[test]
    fn lang_tags_are_classified_by_primary_subtag() {
        assert_eq!(CaseLang::from_tag("tr"), CaseLang::Turkic);
        assert_eq!(CaseLang::from_tag("AZ-Latn"), CaseLang::Turkic);
        assert_eq!(CaseLang::from_tag("nl_BE"), CaseLang::Dutch);
        assert_eq!(CaseLang::from_tag("lt"), CaseLang::Lithuanian);
        assert_eq!(CaseLang::from_tag("ga-IE"), CaseLang::Irish);
        assert_eq!(CaseLang::from_tag("en"), CaseLang::Other);
        assert_eq!(CaseLang::from_tag(""), CaseLang::Other);
        assert_eq!(CaseLang::from_tag("trx"), CaseLang::Other);
    }

    #[test]
    fn turkic_upper_and_lower() {
        assert_eq!(t("uppercase", "tr", "i\u{131}I"), "\u{130}II");
        assert_eq!(t("lowercase", "az", "\u{130}I"), "i\u{131}");
        assert_eq!(t("lowercase", "tr", "I\u{307}"), "i");
        assert_eq!(t("capitalize", "tr", "istanbul"), "\u{130}stanbul");
        assert_eq!(t("uppercase", "en", "i\u{131}"), "II");
        assert_eq!(t("lowercase", "en", "\u{130}"), "i\u{307}");
    }

    #[test]
    fn lithuanian_dots() {
        assert_eq!(t("uppercase", "lt", "i\u{307}\u{300}"), "I\u{300}");
        assert_eq!(t("uppercase", "lt", "\u{12F}\u{307}"), "\u{12E}");
        assert_eq!(t("uppercase", "lt", "x\u{307}"), "X\u{307}");
        assert_eq!(t("lowercase", "lt", "\u{CC}"), "i\u{307}\u{300}");
        assert_eq!(t("lowercase", "lt", "J\u{301}"), "j\u{307}\u{301}");
        assert_eq!(t("lowercase", "lt", "I"), "i");
    }

    #[test]
    fn dutch_ij_and_irish_prefixes() {
        assert_eq!(t("capitalize", "nl", "ijsland ijs"), "IJsland IJs");
        assert_eq!(t("capitalize", "nl", "Ijsland"), "IJsland");
        assert_eq!(t("capitalize", "en", "ijsland"), "Ijsland");
        assert_eq!(t("uppercase", "ga", "tAthair nAthair na"), "tATHAIR nATHAIR NA");
        assert_eq!(t("capitalize", "ga", "tAthair tathair"), "tAthair Tathair");
    }

    #[test]
    fn capitalize_word_boundaries_follow_uax29() {
        assert_eq!(t("capitalize", "en", "john's apple foo_bar"), "John's Apple Foo_bar");
        assert_eq!(t("capitalize", "en", "foo-bar"), "Foo-Bar");
        assert_eq!(t("capitalize", "en", "(hello) \"world\""), "(Hello) \"World\"");
        assert_eq!(t("capitalize", "en", "3.14 a.b l\u{2019}eau"), "3.14 A.b L\u{2019}eau");
        assert_eq!(t("capitalize", "en", "1st ab12 x"), "1st Ab12 X");
        assert_eq!(t("capitalize", "en", ""), "");
    }

    #[test]
    fn combined_with_full_width_and_invalid_value() {
        assert_eq!(t("uppercase full-width", "en", "ab"), "\u{FF21}\u{FF22}");
        assert_eq!(t("bogus", "tr", "i"), "i");
        assert_eq!(TextTransform::Uppercase.apply("i"), "I");
    }
}
