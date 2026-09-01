//! Тесты `style.rs`: разбор цвета и цветовая коррекция: цветовые пространства, quirks,
//! named colors, `color-scheme`, системные и forced colors.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;
// Батч SPLIT-ST3 увёз таблицу named colors в `style/values/named_colors.rs`;
// это имя нужно только тестам, поэтому в `style.rs` оно не импортируется.
use crate::style::values::named_colors::NAMED_COLORS;

    // ── oklch() (CSS Color L4 §10.3) ───────────────────────────────────────

    /// Помощник: проверка близости каналов с допуском (округление 8-bit
    /// + конверсии в float дают ~±2).
    fn near(a: u8, b: u8, tol: i32) -> bool {
        (a as i32 - b as i32).abs() <= tol
    }

    #[test]
    fn oklch_white() {
        // L=1, C=0 — белый. Округление через linear→gamma.
        let c = parse_color("oklch(1 0 0)").unwrap();
        assert!(near(c.r, 255, 2), "r = {}", c.r);
        assert!(near(c.g, 255, 2));
        assert!(near(c.b, 255, 2));
        assert_eq!(c.a, 255);
    }

    #[test]
    fn oklch_black() {
        let c = parse_color("oklch(0 0 0)").unwrap();
        assert!(near(c.r, 0, 2));
        assert!(near(c.g, 0, 2));
        assert!(near(c.b, 0, 2));
    }

    #[test]
    fn oklch_red_approx() {
        // sRGB красный в oklch ≈ oklch(0.628 0.258 29.23deg). Округление f32
        // конверсий — даём допуск ±5.
        let c = parse_color("oklch(0.628 0.258 29.23)").unwrap();
        assert!(near(c.r, 255, 5), "r = {}", c.r);
        assert!(near(c.g, 0, 10), "g = {}", c.g);
        assert!(near(c.b, 0, 10), "b = {}", c.b);
    }

    #[test]
    fn oklch_lightness_as_percent() {
        // 100% = L=1 → белый.
        let pct = parse_color("oklch(100% 0 0)").unwrap();
        let num = parse_color("oklch(1 0 0)").unwrap();
        assert_eq!(pct, num);
    }

    #[test]
    fn oklch_with_alpha_slash() {
        let c = parse_color("oklch(0.5 0 0 / 0.5)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1, "a = {}", c.a);
    }

    #[test]
    fn oklch_with_hue_in_turn() {
        // Hue в turn — должен работать как у hsl().
        // 0.5turn = 180deg.
        let by_turn = parse_color("oklch(0.6 0.15 0.5turn)").unwrap();
        let by_deg = parse_color("oklch(0.6 0.15 180)").unwrap();
        assert_eq!(by_turn, by_deg);
    }

    #[test]
    fn oklch_chroma_clamp_negative_to_zero() {
        // Отрицательная chroma не имеет смысла — clamp на 0.
        let c = parse_color("oklch(0.5 -0.1 0)").unwrap();
        // Должен быть серый (chroma=0).
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn oklch_invalid_returns_none() {
        assert_eq!(parse_color("oklch(0.5)"), None);
        assert_eq!(parse_color("oklch(abc def ghi)"), None);
    }

    // ── CSS Color L5 §4 — relative color syntax ──

    #[test]
    fn relative_rgb_identity() {
        // `rgb(from red r g b)` reproduces the origin exactly.
        assert_eq!(parse_color("rgb(from red r g b)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("rgb(from #336699 r g b)"), Some(rgba(0x33, 0x66, 0x99, 255)));
    }

    #[test]
    fn relative_rgb_channel_reorder() {
        // Swapping keywords swaps channels: red → blue.
        assert_eq!(parse_color("rgb(from red g b r)"), Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn relative_rgb_calc_on_channel() {
        // calc() with a channel keyword.
        assert_eq!(parse_color("rgb(from red calc(r - 50) g b)"), Some(rgba(205, 0, 0, 255)));
    }

    #[test]
    fn relative_rgb_alpha_slash_and_keyword() {
        // Explicit alpha via percentage on the origin's channels.
        let c = parse_color("rgb(from #336699 r g b / 50%)").unwrap();
        assert_eq!((c.r, c.g, c.b), (0x33, 0x66, 0x99));
        assert!(near(c.a, 128, 1), "alpha = {}", c.a);
        // The `alpha` keyword resolves to the origin's alpha (here opaque).
        assert_eq!(parse_color("rgb(from red r g b / alpha)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn relative_hsl_identity_and_calc() {
        // hsl(from red h s l) round-trips back to red.
        assert_eq!(parse_color("hsl(from red h s l)"), Some(rgba(255, 0, 0, 255)));
        // Halving the lightness channel darkens red to ~maroon.
        let c = parse_color("hsl(from red h s calc(l * 0.5))").unwrap();
        assert!(near(c.r, 128, 3), "r = {}", c.r);
        assert_eq!((c.g, c.b), (0, 0));
    }

    #[test]
    fn relative_oklch_identity_with_alpha() {
        // White origin round-trips to white.
        let w = parse_color("oklch(from white l c h)").unwrap();
        assert!(near(w.r, 255, 4) && near(w.g, 255, 4) && near(w.b, 255, 4));
        // Red origin with explicit alpha; round-trip stays near red.
        let r = parse_color("oklch(from red l c h / 0.5)").unwrap();
        assert!(r.r > 240, "r = {}", r.r);
        assert!(near(r.a, 128, 2), "alpha = {}", r.a);
    }

    #[test]
    fn relative_color_invalid_returns_none() {
        // Bad origin color.
        assert_eq!(parse_color("rgb(from notacolor r g b)"), None);
        // Wrong component count.
        assert_eq!(parse_color("rgb(from red r g)"), None);
    }

    // ── CSS Color L4 §10.4 — oklab() ──

    #[test]
    fn oklab_white() {
        // oklab(1 0 0) → белый (a=0, b=0, L=1).
        let c = parse_color("oklab(1 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn oklab_black() {
        let c = parse_color("oklab(0 0 0)").unwrap();
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn oklab_neutral_gray() {
        // a=b=0 → серый.
        let c = parse_color("oklab(0.5 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn oklab_ab_percent() {
        // 100% = 0.4.
        let by_pct = parse_color("oklab(0.5 100% 0)").unwrap();
        let by_num = parse_color("oklab(0.5 0.4 0)").unwrap();
        assert_eq!(by_pct, by_num);
    }

    // ── CSS Color L4 §10.5 — lab() и lch() ──

    #[test]
    fn lab_white() {
        // lab(100 0 0) → белый.
        let c = parse_color("lab(100 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn lab_black() {
        let c = parse_color("lab(0 0 0)").unwrap();
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn lab_neutral_gray() {
        let c = parse_color("lab(50 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn lab_lightness_percent() {
        let by_pct = parse_color("lab(100% 0 0)").unwrap();
        let by_num = parse_color("lab(100 0 0)").unwrap();
        assert_eq!(by_pct, by_num);
    }

    #[test]
    fn lch_white() {
        let c = parse_color("lch(100 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn lch_neutral_when_chroma_zero() {
        let c = parse_color("lch(50 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn lch_with_alpha() {
        let c = parse_color("lch(50 0 0 / 0.5)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn lab_invalid_returns_none() {
        assert_eq!(parse_color("lab(50)"), None);
        assert_eq!(parse_color("lab(abc def ghi)"), None);
    }

    #[test]
    fn hsl_hue_wraps() {
        // 360° = 0°, должен дать тот же красный.
        assert_eq!(
            parse_color("hsl(360, 100%, 50%)"),
            parse_color("hsl(0, 100%, 50%)")
        );
    }

    #[test]
    fn hex_with_alpha_8_digits() {
        // #ff000080 → red, alpha 128.
        let c = parse_color("#ff000080").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn hex_short_with_alpha() {
        // #f008 → ff 00 00 88.
        let c = parse_color("#f008").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 0x88);
    }

    #[test]
    fn named_and_hex_still_work() {
        assert_eq!(parse_color("red"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#f00"), Some(rgba(255, 0, 0, 255)));
    }

    // ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

    #[test]
    fn quirks_hashless_hex_6_digit() {
        // В quirks-mode bare 6-hex парсится как color.
        assert_eq!(parse_color_legacy("ff0000", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("00ff00", true), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_color_legacy("0000ff", true), Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn quirks_hashless_hex_3_digit() {
        // `f00` → `#f00` → red.
        assert_eq!(parse_color_legacy("f00", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("0f0", true), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_color_legacy("00f", true), Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn quirks_hashless_hex_8_digit_with_alpha() {
        // `ff000080` → `#ff000080` → red, alpha 128.
        let c = parse_color_legacy("ff000080", true).unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn quirks_hashless_hex_case_insensitive() {
        // Hex digits ASCII case-insensitive.
        assert_eq!(parse_color_legacy("FF0000", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("Ff00aA", true), Some(rgba(255, 0, 170, 255)));
    }

    #[test]
    fn standards_hashless_hex_rejected() {
        // В Standards-mode bare hex без `#` — не color.
        assert_eq!(parse_color_legacy("ff0000", false), None);
        assert_eq!(parse_color_legacy("f00", false), None);
        assert_eq!(parse_color_legacy("ff000080", false), None);
    }

    #[test]
    fn quirks_hashless_hex_invalid_length() {
        // Длины не 3/6/8 — игнорируются даже в quirks.
        assert_eq!(parse_color_legacy("f", true), None);
        assert_eq!(parse_color_legacy("ff", true), None);
        assert_eq!(parse_color_legacy("ffff", true), None);
        assert_eq!(parse_color_legacy("fffff", true), None);
        assert_eq!(parse_color_legacy("fffffff", true), None);
        assert_eq!(parse_color_legacy("fffffffff", true), None);
    }

    #[test]
    fn quirks_hashless_hex_rejects_non_hex_chars() {
        // `xyz`, `g`, `0xff` — не hex.
        assert_eq!(parse_color_legacy("xyz", true), None);
        assert_eq!(parse_color_legacy("ggg", true), None);
        assert_eq!(parse_color_legacy("ff_000", true), None);
    }

    #[test]
    fn quirks_hashless_does_not_override_standard() {
        // Имя color побеждает hashless-quirk: `red` — это named color, не hex.
        assert_eq!(parse_color_legacy("red", true), Some(rgba(255, 0, 0, 255)));
        // `#ff0000` — обычный hex, парсится без quirk.
        assert_eq!(parse_color_legacy("#ff0000", true), Some(rgba(255, 0, 0, 255)));
        // `rgb(...)` — функциональный, тоже без quirk.
        assert_eq!(parse_color_legacy("rgb(255, 0, 0)", true), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn quirks_named_collision_three_letter_hex() {
        // CSS Color L3: `fff` в quirks парсится как `#fff` (white), хотя
        // `fff` не named color. `dad` — тоже не named, парсится как hex.
        assert_eq!(parse_color_legacy("fff", true), Some(rgba(255, 255, 255, 255)));
        assert_eq!(parse_color_legacy("dad", true), Some(rgba(0xdd, 0xaa, 0xdd, 255)));
    }

    #[test]
    fn quirks_already_hash_prefixed_not_double_processed() {
        // Уже с `#` — обычная ветка, quirks не вмешивается.
        assert_eq!(parse_color_legacy("#ff0000", true), Some(rgba(255, 0, 0, 255)));
        // Невалидный `#` + 4 hex digit-ов в L3 — `parse_hex_color` отдаёт #RGBA.
        // Quirks НЕ должна попытаться повторно добавить `#`.
        // (4-digit с `#` валиден; без `#` — длина 4 не в списке 3/6/8 → None.)
        assert_eq!(parse_color_legacy("ffff", true), None);
    }

    #[test]
    fn quirks_hashless_hex_not_applied_in_background_shorthand() {
        // BUG-079: hashless-hex quirk применяется только к лонгхендам
        // (`background-color` / `color` / `border-*-color`), но НЕ к шортхенду
        // `background`. Edge в quirks-mode сбрасывает `background: ff4444` как
        // невалидный → фон отсутствует.
        let vp = Size { width: 1024.0, height: 768.0 };
        let (_, color) = parse_single_bg_layer("ff4444", 16.0, vp, true);
        assert_eq!(color, None, "hashless hex недопустим в шортхенде background");
        // Контроль: валидные формы цвета по-прежнему работают в шортхенде.
        let (_, named) = parse_single_bg_layer("red", 16.0, vp, true);
        assert_eq!(named, Some(CssColor::Rgba(rgba(255, 0, 0, 255))));
        let (_, hexed) = parse_single_bg_layer("#3366cc", 16.0, vp, true);
        assert_eq!(hexed, Some(CssColor::Rgba(rgba(0x33, 0x66, 0xcc, 255))));
    }

    #[test]
    fn quirks_hashless_hex_still_applied_in_background_color_longhand() {
        // Контроль обратной стороны BUG-079: лонгхенд `background-color`
        // ДОЛЖЕН принимать hashless hex в quirks-mode.
        assert_eq!(
            parse_css_color_legacy("ff4444", true),
            Some(CssColor::Rgba(rgba(0xff, 0x44, 0x44, 255)))
        );
    }

    #[test]
    fn case_insensitive_function_names() {
        assert_eq!(parse_color("RGB(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("Rgba(0, 0, 0, 1)"), Some(rgba(0, 0, 0, 255)));
    }

    // ── Полный набор CSS3 named colors ────────────────────────────────────

    #[test]
    fn named_colors_table_is_sorted() {
        // Бинарный поиск требует сортировки. Защита от опечатки при добавлении
        // нового цвета не на своё место.
        for w in NAMED_COLORS.windows(2) {
            assert!(w[0].0 < w[1].0, "table not sorted at {} >= {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn named_color_count() {
        // Sanity-check: CSS3 = 147 named colors + `rebeccapurple` (CSS4 §6.1)
        // = 148. `transparent` обрабатывается отдельно, в таблице его нет.
        // Если число изменилось — обновить и тест, и CLAUDE.md.
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn named_color_typical_websafe() {
        assert_eq!(parse_color("cornflowerblue"), Some(rgba(100, 149, 237, 255)));
        assert_eq!(parse_color("dodgerblue"), Some(rgba(30, 144, 255, 255)));
        assert_eq!(parse_color("hotpink"), Some(rgba(255, 105, 180, 255)));
        assert_eq!(parse_color("indigo"), Some(rgba(75, 0, 130, 255)));
        assert_eq!(parse_color("teal"), Some(rgba(0, 128, 128, 255)));
    }

    #[test]
    fn named_color_grey_variants_match_gray() {
        // CSS принимает оба написания; цвета должны совпадать.
        assert_eq!(parse_color("gray"), parse_color("grey"));
        assert_eq!(parse_color("darkgray"), parse_color("darkgrey"));
        assert_eq!(parse_color("lightgray"), parse_color("lightgrey"));
        assert_eq!(parse_color("slategray"), parse_color("slategrey"));
        assert_eq!(parse_color("dimgray"), parse_color("dimgrey"));
    }

    #[test]
    fn named_color_rebeccapurple_css4() {
        // Добавлен в CSS Color L4 §6.1 в честь Ребекки Майер.
        assert_eq!(parse_color("rebeccapurple"), Some(rgba(102, 51, 153, 255)));
    }

    #[test]
    fn named_color_case_insensitive() {
        assert_eq!(parse_color("CornflowerBlue"), parse_color("cornflowerblue"));
        assert_eq!(parse_color("RED"), parse_color("red"));
    }

    #[test]
    fn named_color_transparent() {
        // Особый случай — alpha = 0.
        let c = parse_color("transparent").unwrap();
        assert_eq!(c, Color::TRANSPARENT);
        assert_eq!(c.a, 0);
    }

    #[test]
    fn named_color_unknown_returns_none() {
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("currentcolor"), None); // не реализовано как named
    }

    #[test]
    fn named_color_aqua_and_cyan_same() {
        // CSS3: оба имени дают (0, 255, 255).
        assert_eq!(parse_color("aqua"), parse_color("cyan"));
    }

    #[test]
    fn named_color_fuchsia_and_magenta_same() {
        // CSS3: оба имени дают (255, 0, 255).
        assert_eq!(parse_color("fuchsia"), parse_color("magenta"));
    }

    // ── color-scheme ──────────────────────────────────────────────────────────

    #[test]
    fn color_scheme_initial_normal() {
        let style = ComputedStyle::root();
        assert_eq!(style.color_scheme, ColorScheme::Normal);
    }

    #[test]
    fn color_scheme_light() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color-scheme: light; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color_scheme, ColorScheme::Light);
    }

    #[test]
    fn color_scheme_dark() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color-scheme: dark; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color_scheme, ColorScheme::Dark);
    }

    #[test]
    fn color_scheme_light_dark_order() {
        let doc = lumen_html_parser::parse("<span></span><em></em>");
        let ld_sheet = lumen_css_parser::parse("span { color-scheme: light dark; }");
        let dl_sheet = lumen_css_parser::parse("em { color-scheme: dark light; }");
        let root = ComputedStyle::root();
        let span = doc.get(doc.body().unwrap()).children[0];
        let em = doc.get(doc.body().unwrap()).children[1];
        let ld = compute_style(&doc, span, &ld_sheet, &root, Size::new(800.0, 600.0), false);
        let dl = compute_style(&doc, em, &dl_sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(ld.color_scheme, ColorScheme::LightDark);
        assert_eq!(dl.color_scheme, ColorScheme::DarkLight);
    }

    #[test]
    fn color_scheme_only_variants() {
        let doc = lumen_html_parser::parse("<span></span><em></em>");
        let ol_sheet = lumen_css_parser::parse("span { color-scheme: only light; }");
        let od_sheet = lumen_css_parser::parse("em { color-scheme: only dark; }");
        let root = ComputedStyle::root();
        let span = doc.get(doc.body().unwrap()).children[0];
        let em = doc.get(doc.body().unwrap()).children[1];
        let ol = compute_style(&doc, span, &ol_sheet, &root, Size::new(800.0, 600.0), false);
        let od = compute_style(&doc, em, &od_sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(ol.color_scheme, ColorScheme::OnlyLight);
        assert_eq!(od.color_scheme, ColorScheme::OnlyDark);
    }

    #[test]
    fn color_scheme_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { color-scheme: dark; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.color_scheme, ColorScheme::Dark);
        assert_eq!(span_style.color_scheme, ColorScheme::Dark);
    }

    #[test]
    fn color_scheme_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color-scheme: rainbow; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color_scheme, ColorScheme::Normal);
    }

    // ── color-scheme used-scheme switching (CSS Color Adjustment L1 §2.3, Y-4) ──

    #[test]
    fn used_dark_forces_light_regardless_of_os() {
        // `light` / `only light` force the light theme even in OS dark mode.
        assert!(!ColorScheme::Light.used_dark(true));
        assert!(!ColorScheme::OnlyLight.used_dark(true));
        assert!(!ColorScheme::Light.used_dark(false));
    }

    #[test]
    fn used_dark_forces_dark_regardless_of_os() {
        // `dark` / `only dark` force the dark theme even in OS light mode.
        assert!(ColorScheme::Dark.used_dark(false));
        assert!(ColorScheme::OnlyDark.used_dark(false));
        assert!(ColorScheme::Dark.used_dark(true));
    }

    #[test]
    fn used_dark_follows_os_for_normal_and_dual() {
        // `normal` / `light dark` / `dark light` defer to the OS preference.
        for cs in [ColorScheme::Normal, ColorScheme::LightDark, ColorScheme::DarkLight] {
            assert!(cs.used_dark(true), "{cs:?} should be dark under OS dark");
            assert!(!cs.used_dark(false), "{cs:?} should be light under OS light");
        }
    }

    /// Computes the `<input>` style through the full html → body → input
    /// inheritance chain so that a `:root { color-scheme }` rule propagates to
    /// the form control. `os_dark` is the OS `prefers-color-scheme: dark` value.
    fn input_style_with_scheme(css: &str, os_dark: bool) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<input type=\"text\">");
        let sheet = lumen_css_parser::parse(css);
        let vp = Size::new(800.0, 600.0);
        let html = doc.get(doc.root()).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "html")
        }).unwrap();
        let html_style = compute_style(&doc, html, &sheet, &ComputedStyle::root(), vp, os_dark);
        let body = doc.body().unwrap();
        let body_style = compute_style(&doc, body, &sheet, &html_style, vp, os_dark);
        let input = doc.get(body).children[0];
        compute_style(&doc, input, &sheet, &body_style, vp, os_dark)
    }

    #[test]
    fn color_scheme_light_forces_light_form_control_in_os_dark() {
        // OS dark mode, but `:root { color-scheme: light }` inherits to the
        // input → it must render with the LIGHT UA palette (white background).
        let style = input_style_with_scheme(":root { color-scheme: light; }", true);
        assert_eq!(
            style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 })),
            "light color-scheme must force light input bg in OS dark mode"
        );
        assert_eq!(style.color, Color { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn color_scheme_dark_forces_dark_form_control_in_os_light() {
        // OS light mode, but `:root { color-scheme: dark }` inherits to the
        // input → it must render with the DARK UA palette.
        let style = input_style_with_scheme(":root { color-scheme: dark; }", false);
        assert_eq!(
            style.background_color,
            Some(CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 })),
            "dark color-scheme must force dark input bg in OS light mode"
        );
        assert_eq!(style.color, Color { r: 255, g: 255, b: 255, a: 255 });
    }

    // ── system-color resolution (CSS Color 4 §6.2, Y-4) ──

    #[test]
    fn system_color_canvas_switches_by_scheme() {
        assert_eq!(system_color("canvas", false), Some(Color { r: 255, g: 255, b: 255, a: 255 }));
        assert_eq!(system_color("canvas", true), Some(Color { r: 30, g: 30, b: 30, a: 255 }));
    }

    #[test]
    fn system_color_canvastext_switches_by_scheme() {
        assert_eq!(system_color("canvastext", false), Some(Color { r: 0, g: 0, b: 0, a: 255 }));
        assert_eq!(system_color("canvastext", true), Some(Color { r: 255, g: 255, b: 255, a: 255 }));
    }

    #[test]
    fn system_color_buttonface_distinct_per_scheme() {
        let light = system_color("buttonface", false).unwrap();
        let dark = system_color("buttonface", true).unwrap();
        assert_ne!(light, dark, "ButtonFace must differ between light and dark");
    }

    #[test]
    fn system_color_unknown_returns_none() {
        assert_eq!(system_color("notasystemcolor", false), None);
        assert_eq!(system_color("rebeccapurple", true), None);
    }

    #[test]
    fn system_color_threedhighlight_differs_per_scheme() {
        let light = system_color("threedhighlight", false).unwrap();
        let dark = system_color("threedhighlight", true).unwrap();
        assert_ne!(light, dark, "ThreeDHighlight must differ between light and dark");
    }

    #[test]
    fn system_color_threedshadow_differs_per_scheme() {
        let light = system_color("threedshadow", false).unwrap();
        let dark = system_color("threedshadow", true).unwrap();
        assert_ne!(light, dark, "ThreeDShadow must differ between light and dark");
    }

    #[test]
    fn system_color_threedlightshadow_differs_per_scheme() {
        let light = system_color("threedlightshadow", false).unwrap();
        let dark = system_color("threedlightshadow", true).unwrap();
        assert_ne!(light, dark, "ThreeDLightShadow must differ between light and dark");
    }

    #[test]
    fn system_color_threeddarkshadow_differs_per_scheme() {
        let light = system_color("threeddarkshadow", false).unwrap();
        let dark = system_color("threeddarkshadow", true).unwrap();
        assert_ne!(light, dark, "ThreeDDarkShadow must differ between light and dark");
    }

    #[test]
    fn system_color_scrollbar_differs_per_scheme() {
        let light = system_color("scrollbar", false).unwrap();
        let dark = system_color("scrollbar", true).unwrap();
        assert_ne!(light, dark, "Scrollbar must differ between light and dark");
    }

    #[test]
    fn system_color_scrollbarthumb_differs_per_scheme() {
        let light = system_color("scrollbarthumb", false).unwrap();
        let dark = system_color("scrollbarthumb", true).unwrap();
        assert_ne!(light, dark, "ScrollbarThumb must differ between light and dark");
    }

    #[test]
    fn system_color_scrollbartrack_differs_per_scheme() {
        let light = system_color("scrollbartrack", false).unwrap();
        let dark = system_color("scrollbartrack", true).unwrap();
        assert_ne!(light, dark, "ScrollbarTrack must differ between light and dark");
    }

    #[test]
    fn system_color_light_values_match_edge() {
        // BUG-210: TEST-92 light-theme system colors must match Edge's
        // non-forced-colors capture (sampled from the reference screenshot).
        let c = |r, g, b| Color { r, g, b, a: 255 };
        assert_eq!(system_color("buttonface", false), Some(c(240, 240, 240)));
        assert_eq!(system_color("buttonborder", false), Some(c(0, 0, 0)));
        assert_eq!(system_color("highlight", false), Some(c(0, 120, 215)));
        assert_eq!(system_color("highlighttext", false), Some(c(255, 255, 255)));
        assert_eq!(system_color("linktext", false), Some(c(0, 102, 204)));
        assert_eq!(system_color("visitedtext", false), Some(c(0, 102, 204)));
        assert_eq!(system_color("activetext", false), Some(c(0, 102, 204)));
        assert_eq!(system_color("graytext", false), Some(c(109, 109, 109)));
        assert_eq!(system_color("accentcolor", false), Some(c(0, 117, 255)));
        // Deprecated keywords (CSS Color 4 §6.3): ThreeD* → ButtonBorder, Scrollbar → Canvas.
        assert_eq!(system_color("threedhighlight", false), Some(c(0, 0, 0)));
        assert_eq!(system_color("threedshadow", false), Some(c(0, 0, 0)));
        assert_eq!(system_color("scrollbar", false), Some(c(255, 255, 255)));
    }

    // ── CSS Color 4 §6.2 — cascade integration tests ──

    #[test]
    fn system_color_parse_keyword_canvas() {
        assert!(SystemColor::parse("Canvas").is_some());
        assert!(SystemColor::parse("canvas").is_some());
        assert!(SystemColor::parse("CANVAS").is_some());
        // canonical aliases
        assert_eq!(SystemColor::parse("window"), SystemColor::parse("canvas"));
    }

    #[test]
    fn system_color_parse_returns_none_for_named_color() {
        // "blue" is a named color, NOT a system color
        assert!(SystemColor::parse("blue").is_none());
        assert!(SystemColor::parse("red").is_none());
        assert!(SystemColor::parse("rebeccapurple").is_none());
    }

    #[test]
    fn system_color_in_css_cascade_color_property_light() {
        // color: Canvas on a light-scheme element → white (Canvas light)
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color: Canvas; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, false);
        assert_eq!(style.color, Color { r: 255, g: 255, b: 255, a: 255 }, "Canvas light = white");
    }

    #[test]
    fn system_color_in_css_cascade_color_property_dark() {
        // color: CanvasText in dark mode → white text
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color: CanvasText; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, true);
        assert_eq!(style.color, Color { r: 255, g: 255, b: 255, a: 255 }, "CanvasText dark = white");
    }

    #[test]
    fn system_color_in_background_color_light() {
        // background-color: Canvas on a light element → white
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { background-color: Canvas; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, false);
        let bg = style.background_color.expect("background-color should be set");
        assert_eq!(bg, CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 }), "Canvas light bg = white");
    }

    #[test]
    fn system_color_in_background_color_dark() {
        // background-color: Canvas in dark mode → #1e1e1e
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { background-color: Canvas; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, true);
        let bg = style.background_color.expect("background-color should be set");
        assert_eq!(bg, CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 }), "Canvas dark bg = #1e1e1e");
    }

    #[test]
    fn system_color_color_scheme_overrides_dark_mode() {
        // element with `color-scheme: light` uses light system colors even when dark_mode=true
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color-scheme: light; background-color: Canvas; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, true);
        let bg = style.background_color.expect("background-color should be set");
        // color-scheme: light forces light Canvas regardless of OS dark_mode
        assert_eq!(bg, CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 }), "Canvas light-forced = white");
    }

    #[test]
    fn system_color_border_color_resolved() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-top-color: ButtonFace; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style_light = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, false);
        let style_dark = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, true);
        // After post-pass, no System variants should remain
        assert!(!matches!(style_light.border_top_color, CssColor::System(_)), "System should be resolved in light");
        assert!(!matches!(style_dark.border_top_color, CssColor::System(_)), "System should be resolved in dark");
        // Light and dark ButtonFace should differ
        assert_ne!(style_light.border_top_color, style_dark.border_top_color, "ButtonFace differs by scheme");
    }

    // ── forced-color-adjust ───────────────────────────────────────────────────

    #[test]
    fn forced_color_adjust_initial_auto() {
        let style = ComputedStyle::root();
        assert_eq!(style.forced_color_adjust, ForcedColorAdjust::Auto);
    }

    #[test]
    fn forced_color_adjust_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { forced-color-adjust: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.forced_color_adjust, ForcedColorAdjust::None);
    }

    #[test]
    fn forced_color_adjust_preserve_parent_color() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { forced-color-adjust: preserve-parent-color; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.forced_color_adjust, ForcedColorAdjust::PreserveParentColor);
    }

    #[test]
    fn forced_color_adjust_inherited() {
        // CSS Color Adjustment L1 §4: forced-color-adjust is an inherited property.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { forced-color-adjust: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.forced_color_adjust, ForcedColorAdjust::None);
        assert_eq!(span_style.forced_color_adjust, ForcedColorAdjust::None);
    }

    #[test]
    fn forced_color_adjust_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { forced-color-adjust: always; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.forced_color_adjust, ForcedColorAdjust::Auto);
    }

    // ── Forced Colors Mode (CSS Color Adjustment L1 §3) ──────────────────────

    /// Runs `f` with Forced Colors Mode enabled on this thread, then disables it.
    fn with_forced_colors<F: FnOnce()>(f: F) {
        set_forced_colors(true);
        f();
        set_forced_colors(false);
    }

    #[test]
    fn forced_colors_color_forced_to_canvastext() {
        with_forced_colors(|| {
            let s = cascade_at("<div>", "div { color: red; }", &[0]);
            assert_eq!(s.color, SystemColor::CanvasText.resolve_color(false));
        });
    }

    #[test]
    fn forced_colors_adjust_none_keeps_author_colors() {
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { color: red; background-color: blue; forced-color-adjust: none; }",
                &[0],
            );
            assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
            assert_eq!(
                s.background_color,
                Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }))
            );
        });
    }

    #[test]
    fn forced_colors_link_gets_linktext() {
        with_forced_colors(|| {
            let s = cascade_at("<a href='#'>x</a>", "a { color: red; }", &[0]);
            assert_eq!(s.color, SystemColor::LinkText.resolve_color(false));
        });
    }

    #[test]
    fn forced_colors_disabled_control_gets_graytext() {
        with_forced_colors(|| {
            let s = cascade_at("<button disabled>x</button>", "button { color: red; }", &[0]);
            assert_eq!(s.color, SystemColor::GrayText.resolve_color(false));
        });
    }

    #[test]
    fn forced_colors_shadows_forced_to_none() {
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { box-shadow: 2px 2px 4px red; text-shadow: 1px 1px 2px blue; }",
                &[0],
            );
            assert!(s.box_shadow.is_empty());
            assert!(s.text_shadow.is_empty());
        });
    }

    #[test]
    fn forced_colors_background_forced_but_transparency_preserved() {
        with_forced_colors(|| {
            let opaque = cascade_at("<div>", "div { background-color: red; }", &[0]);
            assert_eq!(
                opaque.background_color,
                Some(CssColor::Rgba(SystemColor::Canvas.resolve_color(false)))
            );
            let unset = cascade_at("<div>", "div { color: red; }", &[0]);
            assert_eq!(unset.background_color, None);
            let transparent = cascade_at("<div>", "div { background-color: transparent; }", &[0]);
            assert_ne!(
                transparent.background_color,
                Some(CssColor::Rgba(SystemColor::Canvas.resolve_color(false)))
            );
        });
    }

    #[test]
    fn forced_colors_gradient_background_dropped_url_kept() {
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { background-image: linear-gradient(red, blue); }",
                &[0],
            );
            assert!(
                s.background_layers.iter().all(|l| matches!(l.image, BackgroundImage::None)),
                "gradient background must be forced to none"
            );
            let s = cascade_at("<div>", "div { background-image: url('a.png'); }", &[0]);
            assert!(
                s.background_layers.iter().any(|l| matches!(l.image, BackgroundImage::Url(_))),
                "url() background must be kept"
            );
        });
    }

    #[test]
    fn forced_colors_preserve_parent_color_keeps_color_forces_rest() {
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { color: red; border: 1px solid blue; \
                       forced-color-adjust: preserve-parent-color; }",
                &[0],
            );
            assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
            assert_eq!(
                s.border_top_color,
                CssColor::Rgba(SystemColor::CanvasText.resolve_color(false))
            );
        });
    }

    #[test]
    fn forced_colors_media_query_matches_when_active() {
        // `(forced-colors: active)` media feature is driven by the same flag.
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "@media (forced-colors: active) { div { display: none; } }",
                &[0],
            );
            assert_eq!(s.display, Display::None);
        });
        let s = cascade_at(
            "<div>",
            "@media (forced-colors: active) { div { display: none; } }",
            &[0],
        );
        assert_ne!(s.display, Display::None);
    }

    #[test]
    fn forced_colors_off_keeps_author_colors() {
        let s = cascade_at("<div>", "div { color: red; }", &[0]);
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ── BUG-388: scrollbar-color / font-variant-emoji under forced colors ─────

    #[test]
    fn forced_colors_scrollbar_color_forced_to_auto() {
        with_forced_colors(|| {
            // WPT forced-colors-mode-54: author pair must not survive.
            let s = cascade_at("<div>", "div { scrollbar-color: green red; }", &[0]);
            assert_eq!(s.scrollbar_color, None, "scrollbar-color must compute to auto");
        });
    }

    #[test]
    fn forced_colors_off_keeps_author_scrollbar_color() {
        let s = cascade_at("<div>", "div { scrollbar-color: green red; }", &[0]);
        assert!(s.scrollbar_color.is_some(), "without forced colors the pair stays");
    }

    #[test]
    fn forced_colors_scrollbar_color_forced_even_with_preserve_parent_color() {
        // §3.2: `preserve-parent-color` exempts only `color`.
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { scrollbar-color: green red; forced-color-adjust: preserve-parent-color; }",
                &[0],
            );
            assert_eq!(s.scrollbar_color, None);
        });
    }

    #[test]
    fn forced_colors_scrollbar_color_kept_with_adjust_none() {
        with_forced_colors(|| {
            let s = cascade_at(
                "<div>",
                "div { scrollbar-color: green red; forced-color-adjust: none; }",
                &[0],
            );
            assert!(s.scrollbar_color.is_some());
        });
    }

    #[test]
    fn forced_colors_font_variant_emoji_normal_and_unicode_become_text() {
        // WPT forced-colors-mode-60: `normal`/`unicode` → `text`; `text`/`emoji`
        // are left alone (§3.1 forces only the two neutral values).
        with_forced_colors(|| {
            for (author, expected) in [
                ("normal", FontVariantEmoji::Text),
                ("unicode", FontVariantEmoji::Text),
                ("text", FontVariantEmoji::Text),
                ("emoji", FontVariantEmoji::Emoji),
            ] {
                let css = format!("div {{ font-variant-emoji: {author}; }}");
                let s = cascade_at("<div>", &css, &[0]);
                assert_eq!(s.font_variant_emoji, expected, "author value `{author}`");
            }
        });
    }

    #[test]
    fn forced_colors_font_variant_emoji_forced_without_author_declaration() {
        // The initial value is `normal`, so an untouched element is forced too.
        with_forced_colors(|| {
            let s = cascade_at("<div>", "div { color: red; }", &[0]);
            assert_eq!(s.font_variant_emoji, FontVariantEmoji::Text);
        });
    }

    // --- print-color-adjust ---

    #[test]
    fn print_color_adjust_exact() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { print-color-adjust: exact; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.print_color_adjust, PrintColorAdjust::Exact);
    }

    #[test]
    fn print_color_adjust_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.print_color_adjust, PrintColorAdjust::Economy);
    }

    #[test]
    fn print_color_adjust_legacy_alias() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { color-adjust: exact; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.print_color_adjust, PrintColorAdjust::Exact);
    }

    #[test]
    fn print_color_adjust_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { print-color-adjust: exact; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.print_color_adjust, PrintColorAdjust::Exact);
        assert_eq!(span_style.print_color_adjust, PrintColorAdjust::Economy);
    }

    // ── color-contrast() (CSS Color L5 §11) ───────────────────────────────────

    #[test]
    fn color_contrast_picks_highest_without_target() {
        // Against white, black contrasts far more than dim grey → black wins.
        let c = parse_color("color-contrast(white vs #888, black)").expect("parse");
        assert_eq!(c, Color { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn color_contrast_first_meeting_target_keyword() {
        // Against white: #777 ratio ≈ 4.48 (< AA 4.5), #555 ≈ 7.0 (≥ AA).
        // First candidate meeting AA in list order is #555.
        let c = parse_color("color-contrast(white vs #777, #555, black to AA)").expect("parse");
        assert_eq!(c, Color { r: 0x55, g: 0x55, b: 0x55, a: 255 });
    }

    #[test]
    fn color_contrast_falls_back_to_best_when_target_unmet() {
        // Neither light grey meets AAA (7.0) against white → highest-contrast wins.
        let c = parse_color("color-contrast(white vs #bbb, #999 to AAA)").expect("parse");
        assert_eq!(c, Color { r: 0x99, g: 0x99, b: 0x99, a: 255 });
    }

    #[test]
    fn color_contrast_numeric_target() {
        // Bare numeric ratio target: 3.0. Against white, #777 (≈4.48) meets it.
        let c = parse_color("color-contrast(white vs #aaa, #777 to 3)").expect("parse");
        assert_eq!(c, Color { r: 0x77, g: 0x77, b: 0x77, a: 255 });
    }

    #[test]
    fn color_contrast_accepts_color_functions() {
        // Base and candidates may themselves be color functions (top-level `vs`
        // must be found past the parens/commas of rgb()).
        let c = parse_color("color-contrast(rgb(255, 255, 255) vs rgb(200, 200, 200), black)")
            .expect("parse");
        assert_eq!(c, Color { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn color_contrast_invalid_returns_none() {
        // Missing "vs" keyword → None.
        assert!(parse_color("color-contrast(white, black)").is_none());
        // Fewer than two candidates → None.
        assert!(parse_color("color-contrast(white vs black)").is_none());
        // Unparsable candidate color → None.
        assert!(parse_color("color-contrast(white vs black, notacolor)").is_none());
    }

    // ── color() predefined color spaces (CSS Color L4 §10) ─────────────────────

    /// Parse a `color()` string through the cascade colour path and resolve to
    /// a displayable sRGB `Color`.
    fn color_fn_srgb(s: &str) -> Color {
        match parse_css_color_legacy(s, false).expect("color() should parse") {
            CssColor::Wide(f) => f.to_srgb_color(),
            CssColor::Rgba(c) => c,
            other => panic!("unexpected CssColor variant: {other:?}"),
        }
    }

    #[test]
    fn color_fn_srgb_linear_extremes() {
        let black = color_fn_srgb("color(srgb-linear 0 0 0)");
        assert_eq!((black.r, black.g, black.b), (0, 0, 0));
        let white = color_fn_srgb("color(srgb-linear 1 1 1)");
        assert_eq!((white.r, white.g, white.b), (255, 255, 255));
        // Linear 0.5 → gamma-encoded sRGB ≈ 0.735 → ~188.
        let mid = color_fn_srgb("color(srgb-linear 0.5 0.5 0.5)");
        assert!(mid.r >= 186 && mid.r <= 190, "mid grey r={}", mid.r);
    }

    #[test]
    fn color_fn_xyz_d65_white_and_black() {
        // D65 reference white in XYZ → sRGB white.
        let white = color_fn_srgb("color(xyz-d65 0.9505 1.0 1.089)");
        assert!(white.r >= 253 && white.g >= 253 && white.b >= 253, "white={white:?}");
        // `xyz` is an alias for `xyz-d65`.
        let black = color_fn_srgb("color(xyz 0 0 0)");
        assert_eq!((black.r, black.g, black.b), (0, 0, 0));
    }

    #[test]
    fn color_fn_xyz_d50_black() {
        let black = color_fn_srgb("color(xyz-d50 0 0 0)");
        assert_eq!((black.r, black.g, black.b), (0, 0, 0));
    }

    #[test]
    fn color_fn_a98_and_prophoto_white() {
        // Each space's (1,1,1) is its own reference white → maps to sRGB white.
        let a98 = color_fn_srgb("color(a98-rgb 1 1 1)");
        assert!(a98.r >= 252 && a98.g >= 252 && a98.b >= 252, "a98 white={a98:?}");
        let pp = color_fn_srgb("color(prophoto-rgb 1 1 1)");
        assert!(pp.r >= 250 && pp.g >= 250 && pp.b >= 250, "prophoto white={pp:?}");
        let pp_black = color_fn_srgb("color(prophoto-rgb 0 0 0)");
        assert_eq!((pp_black.r, pp_black.g, pp_black.b), (0, 0, 0));
    }

    #[test]
    fn color_fn_predefined_alpha() {
        let c = color_fn_srgb("color(srgb-linear 0 0 0 / 0.5)");
        assert!(c.a >= 127 && c.a <= 128, "alpha={}", c.a);
    }

    #[test]
    fn color_fn_unknown_space_is_none() {
        // Unknown predefined space → whole color() is invalid.
        assert!(parse_css_color_legacy("color(foobar 1 2 3)", false).is_none());
    }

    // ── color() custom `@color-profile` reference (CSS Color L5 §4) ────────────

    #[test]
    fn color_fn_custom_profile_channels_pass_through_as_srgb() {
        // Real ICC transform is deferred — channels are treated as sRGB directly.
        let c = color_fn_srgb("color(--swop5c 1 0.5 0)");
        assert_eq!(c.r, 255);
        assert!(c.g >= 127 && c.g <= 128, "g={}", c.g);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn color_fn_custom_profile_alpha() {
        let c = color_fn_srgb("color(--swop5c 0 0 0 / 0.5)");
        assert!(c.a >= 127 && c.a <= 128, "alpha={}", c.a);
    }

    // ── ColorFloat.to_display (ph3-color-management Step 2) ────────────────────

    #[test]
    fn color_float_to_display_srgb_matches_linear_srgb() {
        let cf = ColorFloat {
            r: 0.8,
            g: 0.4,
            b: 0.2,
            a: 0.5,
            space: crate::ColorSpace::Srgb,
        };
        let display = cf.to_display(crate::ColorSpace::Srgb);
        let legacy = cf.to_linear_srgb();
        assert!(
            (display[0] - legacy[0]).abs() < 1e-4,
            "r mismatch: {} vs {}",
            display[0],
            legacy[0]
        );
        assert!(
            (display[1] - legacy[1]).abs() < 1e-4,
            "g mismatch: {} vs {}",
            display[1],
            legacy[1]
        );
        assert!(
            (display[2] - legacy[2]).abs() < 1e-4,
            "b mismatch: {} vs {}",
            display[2],
            legacy[2]
        );
        assert!(
            (display[3] - legacy[3]).abs() < 1e-4,
            "a mismatch: {} vs {}",
            display[3],
            legacy[3]
        );
    }

    #[test]
    fn color_float_to_display_same_space_is_identity_decoded() {
        // P3 source → P3 target: only gamma is decoded, channels preserved.
        let cf = ColorFloat {
            r: 0.5,
            g: 0.25,
            b: 0.75,
            a: 1.0,
            space: crate::ColorSpace::DisplayP3,
        };
        let out = cf.to_display(crate::ColorSpace::DisplayP3);
        assert!(
            (out[0] - srgb_gamma_decode(0.5)).abs() < 1e-4,
            "P3 r should decode in-place"
        );
        assert!(
            (out[1] - srgb_gamma_decode(0.25)).abs() < 1e-4,
            "P3 g should decode in-place"
        );
        assert!(
            (out[2] - srgb_gamma_decode(0.75)).abs() < 1e-4,
            "P3 b should decode in-place"
        );
        assert!((out[3] - 1.0).abs() < 1e-4, "alpha preserved");
    }

// ── hwb() и color() в parse_color (BUG-451) ────────────────────────────────

/// CSS Color L4 §7 — `hwb()` не разбиралась НИГДЕ в движке: ни каскадом, ни
/// Canvas 2D. Обратные конверсии в `color_mix.rs` были, но приватные и только
/// для смешивания.
#[test]
fn hwb_pure_hue_and_gray() {
    // Чистый тон: белизна и чернота нулевые.
    assert_eq!(parse_color("hwb(120 0% 0%)"), Some(rgba(0, 255, 0, 255)));
    assert_eq!(parse_color("hwb(0 0% 0%)"), Some(rgba(255, 0, 0, 255)));
    // w + b >= 1 — серый w / (w + b), тон не важен.
    assert_eq!(parse_color("hwb(120 50% 50%)"), Some(rgba(128, 128, 128, 255)));
    assert_eq!(parse_color("hwb(200 100% 0%)"), Some(rgba(255, 255, 255, 255)));
    assert_eq!(parse_color("hwb(200 0% 100%)"), Some(rgba(0, 0, 0, 255)));
    // Тон сжимается в [w, 1 - b].
    let c = parse_color("hwb(120 25% 25%)").unwrap();
    assert_eq!((c.r, c.g, c.b), (64, 191, 64));
    // Альфа через слэш.
    assert_eq!(parse_color("hwb(120 0% 0% / 0.5)").unwrap().a, 128);
    // Относительная форма пока не поддержана — и отвергается явно, а не
    // считается неверно.
    assert_eq!(parse_color("hwb(from red h w b)"), None);
    // Компоненты w/b — только проценты.
    assert_eq!(parse_color("hwb(120 0 0)"), None);
}

/// `color(<space> …)` раньше разбирался только через `CssColor::Wide`, то есть
/// был недоступен потребителям без каскада (Canvas 2D). `parse_color` теперь
/// принимает его последней веткой, сразу гамут-маппя в sRGB.
#[test]
fn parse_color_accepts_color_function() {
    assert_eq!(parse_color("color(srgb 0 1 0)"), Some(rgba(0, 255, 0, 255)));
    assert_eq!(parse_color("color(srgb 1 1 1)"), Some(rgba(255, 255, 255, 255)));
    assert_eq!(parse_color("color(srgb 0 0 0 / 0.5)").unwrap().a, 128);
    // display-p3 зелёный вне гамута sRGB — клипуется, но остаётся зелёным.
    let c = parse_color("color(display-p3 0 1 0)").unwrap();
    assert_eq!((c.r, c.g, c.b), (0, 255, 0));
    assert_eq!(parse_color("color(no-such-space 0 1 0)"), None);
}

/// BUG-451: длина hex-хвоста считалась в БАЙТАХ, а срезы шли байтовыми
/// индексами, поэтому один не-ASCII символ проходил проверку длины и рвал
/// границу UTF-8. Достижимо из обычного стиля страницы, не только из canvas.
#[test]
fn hex_color_with_non_ascii_does_not_panic() {
    assert_eq!(parse_color("#±a"), None);
    assert_eq!(parse_color("#±ab"), None);
    assert_eq!(parse_color("#ЖЖЖ"), None);
    assert_eq!(parse_color("#gg0"), None);
    // Валидный hex по-прежнему разбирается.
    assert_eq!(parse_color("#0f0"), Some(rgba(0, 255, 0, 255)));
    assert_eq!(parse_color("#0f08").unwrap().a, 136);
}

/// BUG-465: `CSSStyleDeclaration` `<color>` specified-value serialization —
/// hex/legacy-functional syntax canonicalizes to `rgb()`/`rgba()`, keyword
/// syntax (named/system/`currentcolor`/`transparent`/CSS-wide keywords) stays
/// a keyword, and a syntactically invalid `<color>` is rejected outright.
#[test]
fn canonical_specified_color_hex_and_functional_forms_become_rgb() {
    assert_eq!(canonical_specified_color("#f00"), Some("rgb(255, 0, 0)".to_string()));
    assert_eq!(canonical_specified_color("#ffffff"), Some("rgb(255, 255, 255)".to_string()));
    assert_eq!(canonical_specified_color("#1000"), Some("rgba(17, 0, 0, 0)".to_string()));
    assert_eq!(canonical_specified_color("rgb(0%, 0%, 1%)"), Some("rgb(0, 0, 3)".to_string()));
    assert_eq!(canonical_specified_color("rgb(0, 0, 256)"), Some("rgb(0, 0, 255)".to_string()));
    assert_eq!(canonical_specified_color("hsl(0, 100%, 50%)"), Some("rgb(255, 0, 0)".to_string()));
}

#[test]
fn canonical_specified_color_keeps_keyword_syntax_as_keyword() {
    assert_eq!(canonical_specified_color("red"), Some("red".to_string()));
    assert_eq!(canonical_specified_color("AQUA"), Some("aqua".to_string()));
    assert_eq!(canonical_specified_color("currentColor"), Some("currentcolor".to_string()));
    assert_eq!(canonical_specified_color("transparent"), Some("transparent".to_string()));
    assert_eq!(canonical_specified_color("Canvas"), Some("canvas".to_string()));
    // CSS-wide keywords apply to every property, not just `<color>`-typed
    // ones — kept verbatim (lowercased) rather than rejected.
    assert_eq!(canonical_specified_color("inherit"), Some("inherit".to_string()));
    assert_eq!(canonical_specified_color("initial"), Some("initial".to_string()));
    assert_eq!(canonical_specified_color("unset"), Some("unset".to_string()));
    assert_eq!(canonical_specified_color("revert"), Some("revert".to_string()));
}

#[test]
fn canonical_specified_color_rejects_invalid_syntax() {
    assert_eq!(canonical_specified_color(""), None);
    assert_eq!(canonical_specified_color("#00000"), None);
    assert_eq!(canonical_specified_color("#0000fg"), None);
    assert_eq!(canonical_specified_color("invalidValue"), None);
}
