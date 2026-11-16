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

    /// Значение `mask` раскладывается по лонгхендам и собирается обратно канонически.
    fn round_trip(v: &str) -> Option<String> {
        let decls = crate::style::expand_mask(v)?;
        let map: std::collections::HashMap<&str, String> = decls.into_iter().collect();
        crate::style::mask_shorthand_value(&|n| map.get(n).cloned())
    }

    #[test]
    fn mask_shorthand_round_trips_canonically() {
        for (v, want) in [
            ("none", "none"),
            ("linear-gradient(to left bottom, red, blue)", "linear-gradient(to left bottom, red, blue)"),
            ("linear-gradient(to left bottom, red, blue) luminance", "linear-gradient(to left bottom, red, blue) luminance"),
            ("linear-gradient(to left bottom, red, blue) 1px 2px", "linear-gradient(to left bottom, red, blue) 1px 2px"),
            ("url(a.png) 1px 2px / contain", "url(\"a.png\") 1px 2px / contain"),
            ("repeat-y", "repeat-y"),
            ("border-box", "none"),
            ("content-box", "content-box"),
            ("linear-gradient(to left bottom, red, blue) padding-box", "linear-gradient(to left bottom, red, blue) padding-box"),
            ("url(a.png) fill-box", "url(\"a.png\") fill-box"),
            ("view-box", "view-box"),
            ("no-clip", "no-clip"),
            ("url(a.png) add", "url(\"a.png\")"),
            ("subtract", "subtract"),
            ("linear-gradient(to left bottom, red, blue) exclude", "linear-gradient(to left bottom, red, blue) exclude"),
            ("alpha", "alpha"),
            ("url(a.png) alpha", "url(\"a.png\") alpha"),
            ("border-box border-box", "none"),
            ("content-box content-box", "content-box"),
            ("border-box content-box", "border-box content-box"),
            ("border-box no-clip", "no-clip"),
            ("none alpha", "alpha"),
            (
                "intersect no-clip space round 1px 2px / contain stroke-box linear-gradient(to left bottom, red, blue) luminance",
                "linear-gradient(to left bottom, red, blue) 1px 2px / contain space round stroke-box no-clip intersect luminance",
            ),
            (
                "intersect no-clip space round 1px 2px / contain view-box, stroke-box linear-gradient(to left bottom, red, blue) luminance",
                "1px 2px / contain space round view-box no-clip intersect, linear-gradient(to left bottom, red, blue) stroke-box luminance",
            ),
            ("none, linear-gradient(to left bottom, red, blue) padding-box", "none, linear-gradient(to left bottom, red, blue) padding-box"),
            ("1px 2px/contain", "1px 2px / contain"),
        ] {
            assert_eq!(round_trip(v).as_deref(), Some(want), "mask: {v}");
        }
    }

    #[test]
    fn mask_shorthand_expands_to_layer_longhands() {
        let decls = crate::style::expand_mask("none").unwrap();
        let map: std::collections::HashMap<&str, String> = decls.into_iter().collect();
        assert_eq!(map["mask-image"], "none");
        assert_eq!(map["mask-position"], "0% 0%");
        assert_eq!(map["mask-size"], "auto");
        assert_eq!(map["mask-repeat"], "repeat");
        assert_eq!(map["mask-origin"], "border-box");
        assert_eq!(map["mask-clip"], "border-box");
        assert_eq!(map["mask-composite"], "add");
        assert_eq!(map["mask-mode"], "match-source");
        assert_eq!(map["mask-border-source"], "none");
        assert_eq!(map["mask-border-outset"], "0");
        assert_eq!(map["mask-border-repeat"], "stretch");
        assert_eq!(map["mask-border-slice"], "0");
        assert_eq!(map["mask-border-width"], "auto");
    }

    #[test]
    fn invalid_mask_shorthand_is_rejected() {
        for v in [
            "none linear-gradient(to 0% 0%, rgb(0, 0, 0), rgb(0, 0, 255))",
            "none alpha luminance",
            "linear-gradient(to 0% 0%, rgb(0, 0, 0), rgb(0, 0, 255)) url(\"https://example.com/\")",
            "1px 2px 3px",
            "1px 2px 3px 4px",
            "1px 2px / contain / cover",
            "repeat-y repeat-x",
            "stroke-box no-clip view-box",
            "border-box view-box padding-box",
            "no-clip border-box content-box",
            "content-box stroke-box no-clip",
            "margin-box",
            "add intersect",
        ] {
            assert!(crate::style::expand_mask(v).is_none(), "mask: {v}");
        }
    }
