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
