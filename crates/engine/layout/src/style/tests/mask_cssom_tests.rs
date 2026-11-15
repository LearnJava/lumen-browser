//! Тесты `canonical_specified_mask` — грамматика CSS Masking в inline-`style` CSSOM
//! (BUG-1408): слои `mask-*` по грамматике `background-*`, `mask-{origin,clip}` —
//! `<coord-box> | no-clip`, `mask-composite`/`mask-mode` — списки ключевых слов.

    use crate::style::canonical_specified_mask;

    fn canon(prop: &str, v: &str) -> Option<String> {
        canonical_specified_mask(prop, v)
    }

    #[test]
    fn layer_longhands_follow_the_background_grammar() {
        assert_eq!(canon("mask-size", "1px").as_deref(), Some("1px auto"));
        assert_eq!(canon("mask-size", "auto auto").as_deref(), Some("auto"));
        assert_eq!(canon("mask-size", "auto 1px, 2% 3%, contain").as_deref(), Some("auto 1px, 2% 3%, contain"));
        assert_eq!(canon("mask-size", "-1px"), None);
        assert_eq!(canon("mask-size", "1px 2px 3px"), None);

        assert_eq!(canon("mask-repeat", "repeat repeat").as_deref(), Some("repeat"));
        assert_eq!(canon("mask-repeat", "repeat no-repeat").as_deref(), Some("repeat-x"));
        assert_eq!(canon("mask-repeat", "repeat undefined"), None);

        assert_eq!(canon("mask-position", "top").as_deref(), Some("center top"));
        assert_eq!(canon("mask-position", "bottom 10% right 20%").as_deref(), Some("right 20% bottom 10%"));
        assert_eq!(canon("mask-position", "left right"), None);
        // Three-token form is invalid for `mask-position` (unlike `background-position`).
        assert_eq!(canon("mask-position", "right top 5px"), None);
        assert_eq!(canon("mask-position", "center left 1px"), None);
        assert_eq!(canon("mask-position", "left 4px top"), None);
        assert_eq!(canon("mask-position", "right 30% top 60px").as_deref(), Some("right 30% top 60px"));

        assert_eq!(canon("mask-image", "none").as_deref(), Some("none"));
        assert_eq!(canon("mask-image", "url(a.png), none").as_deref(), Some("url(\"a.png\"), none"));

        assert_eq!(canon("mask-origin", "BORDER-BOX").as_deref(), Some("border-box"));
        assert_eq!(canon("mask-origin", "margin-box"), None);
        assert_eq!(canon("mask-clip", "no-clip").as_deref(), Some("no-clip"));
        assert_eq!(canon("mask-clip", "fill-box, no-clip").as_deref(), Some("fill-box, no-clip"));
        assert_eq!(canon("mask-clip", "view-box").as_deref(), Some("view-box"));
        assert_eq!(canon("mask-composite", "add, subtract").as_deref(), Some("add, subtract"));
        assert_eq!(canon("mask-composite", "source-over"), None);
        assert_eq!(canon("mask-mode", "luminance").as_deref(), Some("luminance"));
        assert_eq!(canon("mask-mode", "auto"), None);
    }
