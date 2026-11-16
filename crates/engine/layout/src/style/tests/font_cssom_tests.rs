//! Тесты `canonical_specified_font` — ключевые свойства CSS Fonts в inline-`style` CSSOM
//! (BUG-1408): одиночные ключевые слова, `font-palette` с `<dashed-ident>`, `font-synthesis`
//! и списки `font-variant-*` в порядке спецификации.

    use crate::style::canonical_specified_font;

    fn canon(prop: &str, v: &str) -> Option<String> {
        canonical_specified_font(prop, v)
    }

    #[test]
    fn single_keyword_properties() {
        assert_eq!(canon("font-kerning", "AUTO").as_deref(), Some("auto"));
        assert_eq!(canon("font-kerning", "none").as_deref(), Some("none"));
        assert_eq!(canon("font-kerning", "normal auto"), None);
        assert_eq!(canon("font-optical-sizing", "none").as_deref(), Some("none"));
        assert_eq!(canon("font-optical-sizing", "normal"), None);
        assert_eq!(canon("font-variant-caps", "All-Small-Caps").as_deref(), Some("all-small-caps"));
        assert_eq!(canon("font-variant-caps", "normal unicase"), None);
        assert_eq!(canon("font-variant-position", "super").as_deref(), Some("super"));
        assert_eq!(canon("font-variant-position", "super sub"), None);
        assert_eq!(canon("font-variant-emoji", "text").as_deref(), Some("text"));
        assert_eq!(canon("font-variant-emoji", "normal, unicode"), None);
    }

    #[test]
    fn font_palette_dashed_ident() {
        assert_eq!(canon("font-palette", "normal").as_deref(), Some("normal"));
        assert_eq!(canon("font-palette", "--Pitchfork").as_deref(), Some("--Pitchfork"));
        assert_eq!(canon("font-palette", "--").as_deref(), Some("--"));
        assert_eq!(canon("font-palette", "none"), None);
        assert_eq!(canon("font-palette", "A"), None);
    }

    #[test]
    fn variant_lists_use_spec_order() {
        assert_eq!(canon("font-variant-ligatures", "normal").as_deref(), Some("normal"));
        assert_eq!(canon("font-variant-ligatures", "none").as_deref(), Some("none"));
        assert_eq!(
            canon("font-variant-ligatures", "no-contextual common-ligatures").as_deref(),
            Some("common-ligatures no-contextual")
        );
        assert_eq!(canon("font-variant-ligatures", "common-ligatures no-common-ligatures"), None);
        assert_eq!(canon("font-variant-ligatures", "none normal"), None);

        assert_eq!(
            canon("font-variant-numeric", "oldstyle-nums tabular-nums diagonal-fractions").as_deref(),
            Some("oldstyle-nums tabular-nums diagonal-fractions")
        );
        assert_eq!(
            canon("font-variant-numeric", "slashed-zero ordinal stacked-fractions proportional-nums lining-nums")
                .as_deref(),
            Some("lining-nums proportional-nums stacked-fractions ordinal slashed-zero")
        );
        assert_eq!(canon("font-variant-numeric", "lining-nums oldstyle-nums"), None);
        assert_eq!(canon("font-variant-numeric", "normal lining-nums"), None);

        assert_eq!(
            canon("font-variant-east-asian", "ruby full-width simplified").as_deref(),
            Some("simplified full-width ruby")
        );
        assert_eq!(canon("font-variant-east-asian", "jis78 jis83"), None);
        assert_eq!(canon("font-variant-east-asian", "normal ruby"), None);
    }

    #[test]
    fn font_synthesis_fixed_order() {
        assert_eq!(canon("font-synthesis", "none").as_deref(), Some("none"));
        assert_eq!(canon("font-synthesis", "style weight").as_deref(), Some("weight style"));
        assert_eq!(
            canon("font-synthesis", "small-caps oblique-only weight ").as_deref(),
            Some("weight oblique-only small-caps")
        );
        assert_eq!(
            canon("font-synthesis", "position weight small-caps style").as_deref(),
            Some("weight style small-caps position")
        );
        assert_eq!(canon("font-synthesis", "none weight"), None);
        assert_eq!(canon("font-synthesis", "style oblique-only"), None);
        assert_eq!(canon("font-synthesis-weight", "auto").as_deref(), Some("auto"));
        assert_eq!(canon("font-synthesis-weight", "normal"), None);
        assert_eq!(canon("font-synthesis-style", "oblique-only").as_deref(), Some("oblique-only"));
        assert_eq!(canon("font-synthesis-style", "none oblique-only"), None);
        assert_eq!(canon("font-synthesis-position", "auto none"), None);
    }

    #[test]
    fn font_size_adjust_metric_and_number() {
        assert_eq!(canon("font-size-adjust", "none").as_deref(), Some("none"));
        assert_eq!(canon("font-size-adjust", "0.5").as_deref(), Some("0.5"));
        assert_eq!(canon("font-size-adjust", "ex-height 0.5").as_deref(), Some("0.5"));
        assert_eq!(canon("font-size-adjust", "cap-height 0.8").as_deref(), Some("cap-height 0.8"));
        assert_eq!(canon("font-size-adjust", "ic-height from-font").as_deref(), Some("ic-height from-font"));
        assert_eq!(canon("font-size-adjust", "calculate").as_deref(), None);
        assert_eq!(canon("font-size-adjust", "from-font from-font"), None);
        assert_eq!(canon("font-size-adjust", "cap-height calc(0.5 + 1)").as_deref(), Some("cap-height calc(1.5)"));
        assert_eq!(canon("font-size-adjust", "-10"), None);
        assert_eq!(canon("font-size-adjust", "none 0.5"), None);
        assert_eq!(canon("font-size-adjust", "ch 0.5"), None);
    }

    #[test]
    fn font_language_override_string() {
        assert_eq!(canon("font-language-override", "normal").as_deref(), Some("normal"));
        assert_eq!(canon("font-language-override", "\"KSW\"").as_deref(), Some("\"KSW\""));
        assert_eq!(canon("font-language-override", "\"ENG \"").as_deref(), Some("\"ENG\""));
        assert_eq!(canon("font-language-override", "' en '").as_deref(), Some("\" en\""));
        assert_eq!(canon("font-language-override", "\"1 %\"").as_deref(), Some("\"1 %\""));
        assert_eq!(canon("font-language-override", "auto"), None);
        assert_eq!(canon("font-language-override", "\"turkish\""), None);
        assert_eq!(canon("font-language-override", "\"xøx\""), None);
        assert_eq!(canon("font-language-override", "\"\""), None);
        assert_eq!(canon("font-language-override", "\"ENG  \""), None);
    }

    #[test]
    fn feature_settings_tags_and_flags() {
        let p = "font-feature-settings";
        assert_eq!(canon(p, "normal").as_deref(), Some("normal"));
        assert_eq!(canon(p, "\"dlig\" 1").as_deref(), Some("\"dlig\""));
        assert_eq!(canon(p, "\"smcp\" on").as_deref(), Some("\"smcp\""));
        assert_eq!(canon(p, "'c2sc'").as_deref(), Some("\"c2sc\""));
        assert_eq!(canon(p, "\"liga\" off").as_deref(), Some("\"liga\" 0"));
        assert_eq!(canon(p, "\"tnum\", 'hist'").as_deref(), Some("\"tnum\", \"hist\""));
        assert_eq!(canon(p, "\"PKRN\"").as_deref(), Some("\"PKRN\""));
        assert_eq!(
            canon(p, "\"\\22\\7d\\2f\\2a\"").as_deref(),
            Some("\"\\\"}/*\"")
        );
        assert_eq!(
            canon(p, "\"\\22\\22\\22\\22\"").as_deref(),
            Some("\"\\\"\\\"\\\"\\\"\"")
        );
        // Сериализованная запись обязана разбираться обратно (round-trip).
        assert_eq!(canon(p, "\"\\\"}/*\"").as_deref(), Some("\"\\\"}/*\""));
        assert_eq!(
            canon(p, "\"dlig\" 1, \"smcp\" on, \"dlig\" 0").as_deref(),
            Some("\"dlig\", \"smcp\", \"dlig\" 0")
        );
        assert_eq!(canon(p, "normal \"dlig\""), None);
        assert_eq!(canon(p, "\"c2sc\", normal"), None);
        assert_eq!(canon(p, "\"tnum\" \"hist\""), None);
        assert_eq!(canon(p, "\"silly\" off"), None);
        assert_eq!(canon(p, "dlig"), None);
    }

    #[test]
    fn variation_settings_tags_and_numbers() {
        let p = "font-variation-settings";
        assert_eq!(canon(p, "normal").as_deref(), Some("normal"));
        assert_eq!(canon(p, "\"wght\" 700").as_deref(), Some("\"wght\" 700"));
        assert_eq!(canon(p, "'wght' 700").as_deref(), Some("\"wght\" 700"));
        assert_eq!(
            canon(p, "\"wght\" 700, \"XHGT\" 0.7").as_deref(),
            Some("\"wght\" 700, \"XHGT\" 0.7")
        );
        assert_eq!(canon(p, "\"a cd\" 0.5").as_deref(), Some("\"a cd\" 0.5"));
        assert_eq!(canon(p, "\"ab@d\" 0.5").as_deref(), Some("\"ab@d\" 0.5"));
        assert_eq!(
            canon(p, "'wght' 1e3, 'slnt' -450.0e-1").as_deref(),
            Some("\"wght\" 1000, \"slnt\" -45")
        );
        assert_eq!(canon(p, "700"), None);
        assert_eq!(canon(p, "\"XHGT\""), None);
        assert_eq!(canon(p, "wght 700"), None);
        assert_eq!(canon(p, "normal, \"wght\" 700"), None);
        assert_eq!(canon(p, "\"wgt\" 700"), None);
        assert_eq!(canon(p, "\"XHGTX\" 0.7"), None);
        assert_eq!(canon(p, "\"abc\\1F\" 0.5"), None);
        assert_eq!(canon(p, "\"abc\\7F\" 0.5"), None);
        assert_eq!(canon(p, "\"abc\\A9\" 0.5"), None);
        assert_eq!(canon(p, "'wght' 200 'abcd' 400"), None);
        assert_eq!(canon(p, "'a' 1234"), None);
        assert_eq!(canon(p, "'abcde' 1234"), None);
        assert_eq!(canon(p, "'wght' 200, "), None);
        assert_eq!(canon(p, "'abcd\" 123"), None);
        assert_eq!(canon(p, "'wght' 100px"), None);
        assert_eq!(canon(p, "'wght' calc(100px + 200px)"), None);
        assert_eq!(canon(p, "'wght' 42%"), None);
        assert_eq!(canon(p, "'wght' calc(100%)"), None);
    }

    #[test]
    fn variant_alternates_functions() {
        let p = "font-variant-alternates";
        assert_eq!(canon(p, "normal").as_deref(), Some("normal"));
        assert_eq!(
            canon(p, "styleset(more, than, one)").as_deref(),
            Some("styleset(more, than, one)")
        );
        assert_eq!(
            canon(p, "character-variant(more, than, one)").as_deref(),
            Some("character-variant(more, than, one)")
        );
        assert_eq!(
            canon(p, "character-variant(more,than,one) styleset(x)").as_deref(),
            Some("styleset(x) character-variant(more, than, one)")
        );
        assert_eq!(
            canon(p, "swash(Swash) Historical-Forms").as_deref(),
            Some("historical-forms swash(Swash)")
        );
        assert_eq!(canon(p, "swash(several,different,aliases)"), None);
        assert_eq!(canon(p, "stylistic(more,than,one)"), None);
        assert_eq!(canon(p, "annotation()"), None);
        assert_eq!(canon(p, "historical-forms(argument)"), None);
        assert_eq!(canon(p, "annotation"), None);
        assert_eq!(canon(p, "ornaments stylistic"), None);
        assert_eq!(canon(p, "swash(one) swash(two)"), None);
        assert_eq!(canon(p, "unkown(one) myfunction(two)"), None);
        assert_eq!(canon(p, "styleset(thisone) styleset(something,else)"), None);
        assert_eq!(canon(p, "ornaments(one historical-forms"), None);
    }

    use crate::style::{expand_font, font_shorthand_value};

    fn pairs(v: &str) -> Option<Vec<(String, String)>> {
        expand_font(v).map(|l| l.into_iter().map(|(n, v)| (n.to_string(), v)).collect())
    }

    fn longhand(v: &str, name: &str) -> Option<String> {
        pairs(v)?.into_iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn compose(v: &str) -> Option<String> {
        let l = pairs(v)?;
        font_shorthand_value(&|n| l.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()))
    }

    #[test]
    fn font_shorthand_expands_leading_section_in_any_order() {
        let v = "italic small-caps bold condensed 10px / 1.2 serif";
        assert_eq!(longhand(v, "font-style").as_deref(), Some("italic"));
        assert_eq!(longhand(v, "font-variant-caps").as_deref(), Some("small-caps"));
        assert_eq!(longhand(v, "font-variant-ligatures").as_deref(), Some("normal"));
        assert_eq!(longhand(v, "font-weight").as_deref(), Some("bold"));
        assert_eq!(longhand(v, "font-stretch").as_deref(), Some("condensed"));
        assert_eq!(longhand(v, "font-size").as_deref(), Some("10px"));
        assert_eq!(longhand(v, "line-height").as_deref(), Some("1.2"));
        assert_eq!(longhand(v, "font-family").as_deref(), Some("serif"));

        // `normal` занимает любую свободную позицию, умолчания дописываются.
        let v = "normal normal normal italic 10px/normal Menu";
        assert_eq!(longhand(v, "font-style").as_deref(), Some("italic"));
        assert_eq!(longhand(v, "font-weight").as_deref(), Some("normal"));
        assert_eq!(longhand(v, "line-height").as_deref(), Some("normal"));
        assert_eq!(longhand(v, "font-family").as_deref(), Some("Menu"));
    }

    #[test]
    fn font_shorthand_rejects_off_grammar_values() {
        for v in [
            // Пять `normal` не помещаются в четыре позиции.
            "normal normal normal normal normal 20%/1.2 \"FB Armada\"",
            // Четыре позиции заняты, лишнему компоненту места нет.
            "italic small-caps lighter condensed normal smaller monospace",
            "normal 100 semi-condensed oblique small-caps 10px/normal Menu",
            // Одна позиция дважды.
            "italic oblique xx-small serif",
            "small-caps small-caps medium/normal sans-serif",
            "bold bolder xx-large/1.2 cursive",
            "ultra-condensed extra-condensed larger/calc(120% + 1.2em) fantasy",
            // Вне грамматики.
            "menu icon",
            "oldstyle-nums medium serif",
            "normal full-width medium serif",
            "italic titling-caps medium serif",
            // Нет `<font-family>` / `<font-size>`.
            "italic small-caps lighter condensed smaller",
            "100% larger / 2 fantasy",
            "",
        ] {
            assert_eq!(pairs(v), None, "font = {v:?} должен быть невалиден");
        }
    }

    #[test]
    fn font_shorthand_composes_back_from_longhands() {
        assert_eq!(compose("medium serif").as_deref(), Some("medium serif"));
        assert_eq!(
            compose("normal normal normal italic 10px/normal Menu").as_deref(),
            Some("italic 10px Menu")
        );
        assert_eq!(
            compose("small-caps 20%/1.2 FB Armada").as_deref(),
            Some("small-caps 20% / 1.2 FB Armada")
        );
        assert_eq!(
            compose("bold calc(30% - 40px)/calc(120% + 1.2em) monospace").as_deref(),
            Some("bold calc(30% - 40px) / calc(120% + 1.2em) monospace")
        );
        // `font` выражает только CSS2 `small-caps`; прочие оси `font-variant` не сериализуются.
        assert_eq!(compose("titling-caps medium serif"), None);
        assert_eq!(compose("normal full-width medium serif"), None);
        // Общий CSS-wide keyword сериализуется сам.
        let wide: Vec<(String, String)> = pairs("medium serif")
            .unwrap()
            .into_iter()
            .map(|(n, _)| (n, "inherit".to_string()))
            .collect();
        assert_eq!(
            font_shorthand_value(&|n| wide.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())).as_deref(),
            Some("inherit")
        );
    }
