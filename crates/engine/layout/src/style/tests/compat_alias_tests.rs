//! WHATWG Compat §3 `-webkit-*` simple aliases (COMPAT-CSS-ALIAS) в каскаде.

    use super::*;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn style_of(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(r#"<div></div>"#);
        let sheet = lumen_css_parser::parse(&format!("div {{ {css} }}"));
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children.first().copied().expect("div");
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    #[test]
    fn webkit_transform_and_unprefixed_agree() {
        let a = style_of("-webkit-transform: rotate(10deg)");
        let b = style_of("transform: rotate(10deg)");
        assert!(!a.transform.is_empty());
        assert_eq!(a.transform, b.transform);
    }

    #[test]
    fn source_order_decides_between_prefixed_and_unprefixed() {
        let s = style_of("transform: rotate(10deg); -webkit-transform: none");
        assert!(s.transform.is_empty());
        let s = style_of("-webkit-transform: none; transform: rotate(10deg)");
        assert!(!s.transform.is_empty());
    }

    #[test]
    fn webkit_background_clip_text_without_pair_applies() {
        let s = style_of("-webkit-background-clip: text");
        assert_eq!(s.background_layers.first().map(|l| l.clip), Some(BackgroundClip::Text));
    }

    #[test]
    fn webkit_animation_and_transition_apply() {
        let s = style_of("-webkit-animation-name: spin; -webkit-transition: -webkit-transform 1s");
        assert_eq!(s.animation_names, vec!["spin".to_string()]);
        assert_eq!(s.transition_properties, vec!["transform".to_string()]);
        let s = style_of("-webkit-transition-property: -webkit-transform, opacity");
        assert_eq!(s.transition_properties, vec!["transform".to_string(), "opacity".to_string()]);
    }

    #[test]
    fn webkit_mask_image_and_border_radius_apply() {
        let s = style_of("-webkit-mask-image: linear-gradient(red, blue); -webkit-border-radius: 7px");
        assert!(!s.mask_layers.is_empty());
        assert_ne!(s.border_top_left_radius, style_of("").border_top_left_radius);
    }

    #[test]
    fn every_alias_is_supported() {
        for &(alias, _) in lumen_css_parser::COMPAT_CSS_ALIASES {
            assert!(lumen_css_parser::SUPPORTED_PROPERTIES.contains(&alias), "{alias} missing");
        }
    }
