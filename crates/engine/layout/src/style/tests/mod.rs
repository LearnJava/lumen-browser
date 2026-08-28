//! Тесты `crates/engine/layout/src/style.rs`.
//!
//! Шапка модуля: общие хелперы, которые видят все потомки, и объявления
//! подмодулей по темам будущих производственных модулей (SPLIT-ST3…ST18).
//! Тела перенесены батчем SPLIT-ST1 без правок, вместе с отступом модуля.

    use super::*;
    mod box_model;
    mod cascade;
    mod color;
    mod fonts;
    mod images;
    mod layout_props;
    mod restyle;
    mod text;
    mod timeline;
    mod ua;
    mod values;

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// CSS Masking L1 §4.6 — `mask-clip` accepts the full `<coord-box> | no-clip`
    /// grammar (superset of `background-clip`), unlike `BackgroundClip::parse`
    /// which rejects the SVG boxes and `no-clip`.
    #[test]
    fn mask_clip_parses_full_coord_box_grammar() {
        assert_eq!(MaskClip::parse("border-box"), Some(MaskClip::BorderBox));
        assert_eq!(MaskClip::parse("padding-box"), Some(MaskClip::PaddingBox));
        assert_eq!(MaskClip::parse("content-box"), Some(MaskClip::ContentBox));
        assert_eq!(MaskClip::parse("fill-box"), Some(MaskClip::FillBox));
        assert_eq!(MaskClip::parse("stroke-box"), Some(MaskClip::StrokeBox));
        assert_eq!(MaskClip::parse("view-box"), Some(MaskClip::ViewBox));
        assert_eq!(MaskClip::parse("no-clip"), Some(MaskClip::NoClip));
        assert_eq!(MaskClip::parse("  VIEW-BOX  "), Some(MaskClip::ViewBox));
        // `text` is a background-clip keyword only; `mask-clip` rejects it.
        assert_eq!(MaskClip::parse("text"), None);
        assert_eq!(MaskClip::parse("bogus"), None);
        // Default is border-box (initial value).
        assert_eq!(MaskClip::default(), MaskClip::BorderBox);
    }

    #[test]
    fn bug270_print_media_flag_switches_cascade_media_type() {
        // BUG-270: media_context_from_viewport reflects the print flag so the
        // cascade filters `@media print`/`@media screen` correctly during PDF
        // rendering. The flag is a sticky thread-local — reset it around the test.
        let vp = Size::new(816.0, 1056.0);

        set_print_media(false);
        assert!(!print_media_active());
        assert_eq!(media_context_from_viewport(vp, false).media_type, "screen");

        set_print_media(true);
        assert!(print_media_active());
        assert_eq!(media_context_from_viewport(vp, false).media_type, "print");

        // Reset so later tests on this thread see the screen default.
        set_print_media(false);
        assert_eq!(media_context_from_viewport(vp, false).media_type, "screen");
    }

    #[test]
    fn initial_letter_parse() {
        // normal → no effect.
        assert_eq!(parse_initial_letter("normal"), Some((1.0, 0)));
        assert_eq!(parse_initial_letter("  Normal "), Some((1.0, 0)));
        // single value: sink = auto (0 → floor(size) resolved at layout).
        assert_eq!(parse_initial_letter("3"), Some((3.0, 0)));
        assert_eq!(parse_initial_letter("2.5"), Some((2.5, 0)));
        // two values: explicit sink.
        assert_eq!(parse_initial_letter("3 2"), Some((3.0, 2)));
        // size must be ≥ 1, sink ≥ 1.
        assert_eq!(parse_initial_letter("0.5"), None);
        assert_eq!(parse_initial_letter("3 0"), None);
        assert_eq!(parse_initial_letter("-1"), None);
        // malformed.
        assert_eq!(parse_initial_letter("foo"), None);
        assert_eq!(parse_initial_letter("3 2 1"), None);
        assert_eq!(parse_initial_letter(""), None);
    }

    #[test]
    fn initial_letter_apply_declaration() {
        // Property reaches ComputedStyle via the cascade.
        let sheet = lumen_css_parser::parse("p { initial-letter: 4 3; }");
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.initial_letter_size, 4.0);
        assert_eq!(st.initial_letter_sink, 3);
    }

    #[test]
    fn ruby_properties_apply_declaration() {
        // CSS Ruby L1: все три свойства доезжают до ComputedStyle через каскад.
        let sheet = lumen_css_parser::parse(
            "p { ruby-position: under; ruby-align: center; ruby-merge: merge; }",
        );
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.ruby_position, RubyPosition::Under);
        assert_eq!(st.ruby_align, RubyAlign::Center);
        assert_eq!(st.ruby_merge, RubyMerge::Merge);
    }

    #[test]
    fn ruby_properties_initial_values() {
        // Без объявлений — initial по спеке: over / space-around / separate.
        let sheet = lumen_css_parser::parse("p { color: red; }");
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.ruby_position, RubyPosition::Over);
        assert_eq!(st.ruby_align, RubyAlign::SpaceAround);
        assert_eq!(st.ruby_merge, RubyMerge::Separate);
    }

    #[test]
    fn ruby_properties_inherited_by_child() {
        // Все три свойства наследуются: <span> внутри <p> получает значения родителя.
        let sheet = lumen_css_parser::parse(
            "p { ruby-position: under; ruby-align: start; ruby-merge: auto; }",
        );
        let doc = lumen_html_parser::parse("<p><span>x</span></p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let parent = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        let sid = doc.get(pid).children[0];
        let child = compute_style(&doc, sid, &sheet, &parent, Size::new(800.0, 600.0), false);
        assert_eq!(child.ruby_position, RubyPosition::Under);
        assert_eq!(child.ruby_align, RubyAlign::Start);
        assert_eq!(child.ruby_merge, RubyMerge::Auto);
    }

    #[test]
    fn ruby_properties_invalid_values_ignored() {
        // Невалидные (и неподдерживаемый inter-character) значения игнорируются,
        // остаются initial; alternate парсится как over.
        let sheet = lumen_css_parser::parse(
            "p { ruby-position: inter-character; ruby-align: bogus; ruby-merge: 42; }",
        );
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.ruby_position, RubyPosition::Over);
        assert_eq!(st.ruby_align, RubyAlign::SpaceAround);
        assert_eq!(st.ruby_merge, RubyMerge::Separate);
    }

    #[test]
    fn webkit_scrollbar_width_maps_to_scrollbar_width_keyword() {
        // CC-CSS-1: `::-webkit-scrollbar{width}` has no standard keyword equivalent —
        // bucketed into thin (<=9px) vs auto (>9px). The chrome reference's own
        // 9px falls on the `thin` side of the bucket boundary.
        // BUG-341 S11: the translation only runs for elements that can show a
        // scrollbar, so the subject has to be a scroll container.
        let sheet = lumen_css_parser::parse(
            "div { overflow: auto; } div::-webkit-scrollbar { width: 9px; }",
        );
        let doc = lumen_html_parser::parse("<div>x</div>");
        let did = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(&doc, did, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        assert_eq!(st.scrollbar_width, ScrollbarWidth::Thin);

        let sheet = lumen_css_parser::parse(
            "div { overflow: auto; } div::-webkit-scrollbar { width: 16px; }",
        );
        let st = compute_style(&doc, did, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        assert_eq!(st.scrollbar_width, ScrollbarWidth::Auto);

        let sheet = lumen_css_parser::parse(
            "div { overflow: auto; } div::-webkit-scrollbar { width: 0; }",
        );
        let st = compute_style(&doc, did, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        assert_eq!(st.scrollbar_width, ScrollbarWidth::None);
    }

    #[test]
    fn webkit_scrollbar_thumb_track_map_to_scrollbar_color() {
        // CC-CSS-1: `-thumb`/`-track{background}` translate onto the standard
        // `scrollbar-color` (thumb, track) pair — chrome reference pattern.
        let sheet = lumen_css_parser::parse(
            "div { overflow: auto; } \
             div::-webkit-scrollbar-thumb { background: #112233; } \
             div::-webkit-scrollbar-track { background: #445566; }",
        );
        let doc = lumen_html_parser::parse("<div>x</div>");
        let did = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(&doc, did, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        let (thumb, track) = st.scrollbar_color.expect("scrollbar-color should be set");
        assert_eq!(thumb, Color { r: 0x11, g: 0x22, b: 0x33, a: 255 });
        assert_eq!(track, Color { r: 0x44, g: 0x55, b: 0x66, a: 255 });
    }

    #[test]
    fn webkit_scrollbar_thumb_without_track_leaves_scrollbar_color_unset() {
        // Only one side declared: no honest per-side default to graft onto the
        // missing side, so the standard `scrollbar-color` stays untouched (falls
        // back to UA defaults in paint) rather than fabricating a track color.
        let sheet = lumen_css_parser::parse(
            "div { overflow: auto; } div::-webkit-scrollbar-thumb { background: #112233; }",
        );
        let doc = lumen_html_parser::parse("<div>x</div>");
        let did = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(&doc, did, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        assert_eq!(st.scrollbar_color, None);
    }

    #[test]
    fn webkit_font_smoothing_is_parsed_and_ignored() {
        // -webkit-font-smoothing has no Lumen equivalent (rasterizer antialiasing
        // is always on) — the declaration must not error or drop the rest of the
        // rule; it just falls through apply_declaration's catch-all as a no-op.
        let sheet = lumen_css_parser::parse(
            "p { -webkit-font-smoothing: antialiased; color: rgb(1, 2, 3); }",
        );
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(&doc, pid, &sheet, &ComputedStyle::root(), Size::new(800.0, 600.0), false);
        assert_eq!(st.color, Color { r: 1, g: 2, b: 3, a: 255 });
    }

    #[test]
    fn math_properties_apply_declaration() {
        // MathML Core: оба свойства доезжают до ComputedStyle через каскад.
        let sheet = lumen_css_parser::parse("p { math-style: compact; math-depth: 2; }");
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.math_style, MathStyle::Compact);
        assert_eq!(st.math_depth, 2);
    }

    #[test]
    fn math_properties_initial_values() {
        // Без объявлений — initial по спеке: normal / 0.
        let sheet = lumen_css_parser::parse("p { color: red; }");
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.math_style, MathStyle::Normal);
        assert_eq!(st.math_depth, 0);
    }

    #[test]
    fn math_properties_inherited_by_child() {
        // Оба свойства наследуются: <span> внутри <p> получает значения родителя.
        let sheet = lumen_css_parser::parse("p { math-style: compact; math-depth: 3; }");
        let doc = lumen_html_parser::parse("<p><span>x</span></p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let parent = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        let sid = doc.get(pid).children[0];
        let child = compute_style(&doc, sid, &sheet, &parent, Size::new(800.0, 600.0), false);
        assert_eq!(child.math_style, MathStyle::Compact);
        assert_eq!(child.math_depth, 3);
    }

    #[test]
    fn math_depth_add_and_auto_add_resolve_against_inherited() {
        // add(n) = inherited + n; auto-add = inherited + 1 только при
        // унаследованном math-style: compact (MathML Core §2.1.2).
        let sheet = lumen_css_parser::parse(
            "p { math-style: compact; math-depth: 1; } \
             span { math-depth: add(2); } \
             b { math-depth: auto-add; }",
        );
        let doc = lumen_html_parser::parse("<p><span>x</span><b>y</b></p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let parent = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(parent.math_depth, 1);
        let sid = doc.get(pid).children[0];
        let span = compute_style(&doc, sid, &sheet, &parent, Size::new(800.0, 600.0), false);
        assert_eq!(span.math_depth, 3);
        let bid = doc.get(pid).children[1];
        let bold = compute_style(&doc, bid, &sheet, &parent, Size::new(800.0, 600.0), false);
        assert_eq!(bold.math_depth, 2);
    }

    #[test]
    fn math_depth_auto_add_noop_when_style_normal() {
        // При унаследованном math-style: normal auto-add не инкрементирует.
        let sheet = lumen_css_parser::parse("span { math-depth: auto-add; }");
        let doc = lumen_html_parser::parse("<p><span>x</span></p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let parent = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        let sid = doc.get(pid).children[0];
        let span = compute_style(&doc, sid, &sheet, &parent, Size::new(800.0, 600.0), false);
        assert_eq!(span.math_depth, 0);
    }

    #[test]
    fn math_properties_invalid_values_ignored() {
        // Невалидные значения игнорируются, остаются initial.
        let sheet = lumen_css_parser::parse("p { math-style: bogus; math-depth: foo; }");
        let doc = lumen_html_parser::parse("<p>Hi</p>");
        let pid = doc.get(doc.body().unwrap()).children[0];
        let st = compute_style(
            &doc,
            pid,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(st.math_style, MathStyle::Normal);
        assert_eq!(st.math_depth, 0);
    }

    #[test]
    fn rgb_legacy_commas() {
        assert_eq!(parse_color("rgb(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("rgb(0, 128, 0)"), Some(rgba(0, 128, 0, 255)));
    }

    #[test]
    fn rgb_modern_whitespace() {
        assert_eq!(parse_color("rgb(255 0 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_percent_components() {
        // 100% = 255, 50% = 128 (округление).
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some(rgba(255, 0, 0, 255)));
        let half = parse_color("rgb(50%, 50%, 50%)").unwrap();
        assert!((half.r as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgba_with_alpha_float() {
        // alpha 0.5 → 128 (округление 127.5).
        let c = parse_color("rgba(255, 0, 0, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1, "a={}", c.a);
    }

    #[test]
    fn rgba_with_alpha_percent() {
        let c = parse_color("rgba(255, 0, 0, 50%)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_modern_slash_alpha() {
        // Modern syntax: rgb(r g b / a) — без `rgba` префикса.
        let c = parse_color("rgb(255 0 0 / 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_out_of_range_clamps() {
        // 300 должно зажаться до 255, -10 до 0.
        assert_eq!(parse_color("rgb(300, -10, 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_invalid_components() {
        assert_eq!(parse_color("rgb(abc, def, ghi)"), None);
        assert_eq!(parse_color("rgb(255, 0)"), None);
        assert_eq!(parse_color("rgb()"), None);
    }

    #[test]
    fn hsl_primary_colors() {
        assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(
            parse_color("hsl(120, 100%, 50%)"),
            Some(rgba(0, 255, 0, 255))
        );
        assert_eq!(
            parse_color("hsl(240, 100%, 50%)"),
            Some(rgba(0, 0, 255, 255))
        );
    }

    #[test]
    fn hsl_with_deg_unit() {
        assert_eq!(
            parse_color("hsl(0deg, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_turn() {
        // 0.5turn = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(0.5turn, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 1turn = 360deg = 0deg → red.
        assert_eq!(
            parse_color("hsl(1turn, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_rad() {
        // π rad = 180deg → cyan. f32 округление допустимо.
        let c = parse_color("hsl(3.14159265rad, 100%, 50%)").unwrap();
        assert_eq!(c.r, 0);
        assert!(c.g >= 254);
        assert!(c.b >= 254);
    }

    #[test]
    fn hsl_hue_in_grad() {
        // 200grad = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(200grad, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 400grad = 360deg = 0 → red.
        assert_eq!(
            parse_color("hsl(400grad, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_units_dont_collide() {
        // `grad` не должен ловиться как `rad`. 100grad = 90deg → жёлто-зелёный.
        // А 100rad = 5729.58deg, mod 360 ≈ 329.58 — пурпурно-розовый. Цвета
        // должны отличаться, иначе суффикс ловится не тот.
        let g = parse_color("hsl(100grad, 100%, 50%)").unwrap();
        let r = parse_color("hsl(100rad, 100%, 50%)").unwrap();
        assert_ne!(g, r, "grad и rad дают разные цвета");
    }

    #[test]
    fn hsl_grayscale_when_saturation_zero() {
        // s=0 → lightness как оттенок серого.
        let c = parse_color("hsl(0, 0%, 50%)").unwrap();
        assert!((c.r as i32 - 128).abs() <= 1);
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn hsla_with_alpha() {
        let c = parse_color("hsla(0, 100%, 50%, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    /// Прогоняет каскад вдоль `path` от `<body>` до целевого узла,
    /// возвращая ComputedStyle конкретного узла. Каждый шаг — реальный
    /// `compute_style` с inherited от предыдущего шага. Это позволяет
    /// проверить inherits-семантику @property на двухуровневом дереве.
    /// Путь `&[0]` означает первый child `<body>`, `&[0, 1]` — второй child
    /// первого child, и т.д.
    fn cascade_at(html: &str, css: &str, path: &[usize]) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let viewport = Size::new(800.0, 600.0);
        // Start from <body> so that path[0]=0 refers to the first user element,
        // not to the implicit <html> wrapper injected by the HTML5 parser.
        let mut id = doc.body().unwrap_or_else(|| doc.root());
        let mut style =
            compute_style(&doc, id, &sheet, &ComputedStyle::root(), viewport, false);
        for &idx in path {
            id = doc.get(id).children[idx];
            style = compute_style(&doc, id, &sheet, &style, viewport, false);
        }
        style
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn style_for(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(&format!("p {{ {css} }}"));
        let root_style = ComputedStyle::root();
        let p = doc.get(doc.body().unwrap()).children[0];
        compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0), false)
    }

    /// Тестовый viewport: квадратный, чтобы vh == vw, vmin == vmax.
    fn vp() -> Size { Size::new(1000.0, 1000.0) }

    #[test]
    fn length_resolve_px_is_identity() {
        assert_eq!(Length::Px(12.0).resolve(16.0, Some(100.0), vp()), Some(12.0));
    }

    /// Helper: apply a single CSS property to a fresh ComputedStyle.
    fn ts_prop(prop: &str, val: &str) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        let decl = Declaration { property: prop.to_string(), value: val.to_string(), important: false };
        let vp = Size::new(800.0, 600.0);
        apply_declaration(&mut s, &decl, 16.0, vp, FontWeight::NORMAL, &ComputedStyle::root(), &ComputedStyle::root(), false, false);
        s
    }

    fn doc_root_child_style(html: &str) -> ComputedStyle {
        // With the HTML5 parser, elements are placed inside html→body.
        // Tests that pass "<body ...>" get the body element itself.
        // Tests that pass "<table ...>" or "<div ...>" get the first child of body.
        // We pick the outermost user element: if the html string starts with a
        // non-body/html tag, we take body.children[0]; otherwise we take body.
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let body_id = doc.body().unwrap_or_else(|| doc.root());
        // If the first child of body exists and is a non-body element, use it.
        // (handles "<table>...", "<div>...", "<p>..." etc. directly)
        let node = {
            let body_children = &doc.get(body_id).children;
            if !body_children.is_empty() {
                let first = body_children[0];
                if let lumen_dom::NodeData::Element { name, .. } = &doc.get(first).data {
                    if name.local != "body" && name.local != "html" {
                        first
                    } else {
                        body_id
                    }
                } else {
                    body_id
                }
            } else {
                body_id
            }
        };
        compute_style(&doc, node, &sheet, &root_style, Size::new(800.0, 600.0), false)
    }
