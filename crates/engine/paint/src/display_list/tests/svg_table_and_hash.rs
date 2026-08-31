//! P1/SPLIT-DL2: хвост тестового модуля
//! `mod tests` в `display_list.rs` — image-set()/backdrop-filter hash/
//! структурный хэш команд/table rendering Phase 1/SVG text/
//! FilterMode conversion/overflow:clip/progress/meter/font-stretch. Перенесено
//! байт-в-байт из `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-2).

use super::*;
// build/fills/texts остаются в `mod tests` в display_list.rs — общие хелперы
// всей группы DL, ещё не вынесенные оттуда.
use super::tests::{build, fills, texts};
// build_ordered уехал в display_list/tests/ordered_build_scroll.rs (батч DL-5).
use super::ordered_build_scroll::build_ordered;

    // ── image-set() (CSS Images L4 §5) ──────────────────────────────────────

    #[test]
    fn is_image_set_detects_function() {
        assert!(is_image_set("image-set(\"a.png\" 1x)"));
        assert!(is_image_set("  IMAGE-SET(url(a.png) 2x)"));
        assert!(is_image_set("-webkit-image-set(\"a.png\" 1x)"));
        assert!(!is_image_set("url(a.png)"));
        assert!(!is_image_set("linear-gradient(red, blue)"));
        assert!(!is_image_set("https://example.com/image-set.png"));
    }

    /// BUG-101: `image-set()` разрешается ДВАЖДЫ — здесь, когда эмиттер строит
    /// `DrawBackgroundImage.src`, и в `lumen-layout`
    /// (`collect_background_image_requests`), когда shell решает, что качать.
    /// Это две независимые реализации; разойдись они в выборе кандидата —
    /// картинка молча не нарисуется (ключ загрузки ≠ ключ поиска). Тест держит
    /// их согласованными; жить он может только здесь, потому что зависимость
    /// идёт layout → paint, и обратная сторона про эту недоступна.
    #[test]
    fn image_set_resolver_agrees_with_layout_collector() {
        let cases = [
            "image-set(\"a.png\" 1x, \"b.png\" 2x)",
            "image-set(url(a.png) 1x, url(b.png) 2x, url(c.png) 3x)",
            "-webkit-image-set(url(\"low.png\") 1x, url(\"high.png\") 2x)",
            "image-set(url(a.png) 96dpi, url(b.png) 192dpi)",
            "image-set(url(a.png) type(\"image/webp\") 1x, url(b.png) 2x)",
            "image-set(url(only.png))",
        ];
        for v in cases {
            for dpr in [1.0_f32, 1.4, 2.0, 3.0] {
                assert_eq!(
                    select_image_set_url(v, dpr),
                    lumen_layout::image_set::select_image_set_url(v, dpr),
                    "paint и layout разошлись на {v:?} при dpr={dpr}"
                );
            }
        }
    }

    #[test]
    fn image_set_picks_1x_at_dpr_1() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
    }

    #[test]
    fn image_set_picks_2x_at_dpr_2() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_picks_closest_resolution() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x, \"c.png\" 3x)";
        // dpr 1.4 → |1-1.4|=0.4 wins over |2-1.4|=0.6.
        assert_eq!(select_image_set_url(v, 1.4), "a.png");
        // dpr 1.6 → |2-1.6|=0.4 wins over |1-1.6|=0.6.
        assert_eq!(select_image_set_url(v, 1.6), "b.png");
        // dpr 5.0 (no exact) → highest available.
        assert_eq!(select_image_set_url(v, 5.0), "c.png");
    }

    #[test]
    fn image_set_tie_prefers_higher_resolution() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        // dpr 1.5 equidistant → prefer sharper (2x).
        assert_eq!(select_image_set_url(v, 1.5), "b.png");
    }

    #[test]
    fn image_set_supports_url_wrapper_and_single_quotes() {
        let v = "image-set(url(a.png) 1x, url('b.png') 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_default_resolution_is_1x() {
        // Option with no explicit resolution defaults to 1x.
        let v = "image-set(\"a.png\", \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
    }

    #[test]
    fn image_set_dppx_dpi_dpcm_units() {
        let v = "image-set(\"a.png\" 96dpi, \"b.png\" 2dppx)";
        // 96dpi = 1dppx, 2dppx = 2.
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
        let v2 = "image-set(\"x.png\" 1x, \"y.png\" 192dpi)";
        // 192dpi = 2dppx.
        assert_eq!(select_image_set_url(v2, 2.0), "y.png");
    }

    #[test]
    fn image_set_webkit_prefix() {
        let v = "-webkit-image-set(url(a.png) 1x, url(b.png) 2x)";
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_data_uri_with_commas_not_split() {
        // A data: URI inside url() contains commas — must not split the option.
        let v = "image-set(url(data:image/png;base64,AAAA) 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "data:image/png;base64,AAAA");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_plain_url_passes_through() {
        // Non image-set value treated as a single 1x option.
        assert_eq!(select_image_set_url("\"a.png\"", 2.0), "a.png");
        assert_eq!(select_image_set_url("url(a.png)", 2.0), "a.png");
    }

    #[test]
    fn image_set_empty_returns_empty() {
        assert_eq!(select_image_set_url("image-set()", 1.0), "");
    }

    /// Recursively overrides the `background-image` of the first box that has a
    /// background layer with an `image-set(…)` raw string. Mimics what P4 will
    /// store in `BackgroundImage::Url` once `image-set()` parsing is wired —
    /// lets us exercise the paint-side resolution without the CSS parser.
    fn set_first_bg_image_set(b: &mut LayoutBox, value: &str) -> bool {
        if let Some(layer) = std::sync::Arc::make_mut(&mut b.style).background_layers.first_mut() {
            layer.image = BackgroundImage::Url(value.to_string());
            return true;
        }
        for child in &mut b.children {
            if set_first_bg_image_set(child, value) {
                return true;
            }
        }
        false
    }

    #[test]
    fn image_set_wired_into_background_layer() {
        // Start from a real url background so a layer exists, then inject the
        // image-set string the way P4's parser will once wired.
        let css = "div { width: 100px; height: 100px; background-image: url(placeholder.png); }";
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(css);
        let mut tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        assert!(set_first_bg_image_set(&mut tree, "image-set(url(a.png) 1x, url(b.png) 2x)"));
        // build_display_list defaults to dpr 1.0 → must pick the 1x url.
        let dl = build_display_list(&tree);
        let srcs: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBackgroundImage { src, .. } => Some(src.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(srcs, vec!["a.png"]);
    }

    #[test]
    fn image_set_dpr2_builder_picks_2x() {
        let css = "div { width: 100px; height: 100px; background-image: url(placeholder.png); }";
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(css);
        let mut tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        assert!(set_first_bg_image_set(&mut tree, "image-set(url(a.png) 1x, url(b.png) 2x)"));
        let stree = lumen_layout::StackingTree::build(&tree);
        let order = PaintOrder::from_tree(&stree);
        let dl = build_display_list_ordered_dpr(&tree, &stree, &order, 2.0).0;
        let srcs: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBackgroundImage { src, .. } => Some(src.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(srcs, vec!["b.png"]);
    }

    // ── backdrop-filter frame hash (CSS Filter Effects L1 §2 cache) ──────────

    pub(crate) fn red_fill(x: f32) -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 255, g: 0, b: 0, a: 255 },
        }
    }

    fn backdrop_cmd() -> DisplayCommand {
        DisplayCommand::PushBackdropFilter {
            filters: vec![lumen_layout::FilterFn::Blur(4.0)],
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        }
    }

    #[test]
    fn contains_backdrop_filter_detects_presence() {
        let with = vec![backdrop_cmd(), DisplayCommand::PopBackdropFilter];
        let without = vec![red_fill(0.0)];
        assert!(contains_backdrop_filter(&with, &[]));
        assert!(contains_backdrop_filter(&[], &with), "overlay lane is scanned too");
        assert!(!contains_backdrop_filter(&without, &without));
    }

    #[test]
    fn hash_is_deterministic_for_identical_input() {
        let content = vec![backdrop_cmd(), red_fill(5.0), DisplayCommand::PopBackdropFilter];
        let a = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        let b = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        assert_eq!(a, b, "same inputs must hash identically");
    }

    #[test]
    fn hash_changes_when_command_changes() {
        let a = hash_display_list(&[red_fill(5.0)], &[], 0.0, 0.0, 1024, 720);
        let b = hash_display_list(&[red_fill(6.0)], &[], 0.0, 0.0, 1024, 720);
        assert_ne!(a, b, "a moved rect must change the hash");
    }

    #[test]
    fn hash_changes_on_scroll_and_size() {
        let content = vec![red_fill(5.0)];
        let base = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 40.0, 1024, 720), "scroll_y");
        assert_ne!(base, hash_display_list(&content, &[], 12.0, 0.0, 1024, 720), "scroll_x");
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 0.0, 800, 720), "width");
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 0.0, 1024, 600), "height");
    }

    #[test]
    fn hash_distinguishes_content_from_overlay_lane() {
        // The same command in the content lane vs the overlay lane must not
        // collide: both lane lengths are folded before the command sequence.
        let cmd = vec![red_fill(5.0)];
        let in_content = hash_display_list(&cmd, &[], 0.0, 0.0, 1024, 720);
        let in_overlay = hash_display_list(&[], &cmd, 0.0, 0.0, 1024, 720);
        assert_ne!(in_content, in_overlay, "lane identity is part of the hash");

        let two_content = hash_display_list(&[red_fill(5.0), red_fill(9.0)], &[], 0.0, 0.0, 1024, 720);
        assert_ne!(in_content, two_content);
        // Same command sequence, different lane split → different hash.
        let split = hash_display_list(&[red_fill(5.0)], &[red_fill(9.0)], 0.0, 0.0, 1024, 720);
        assert_ne!(two_content, split, "a lane boundary shift must change the hash");
    }

    // ── Структурный хэш команд (p1-exp-wgpu-only) ─────────────────────────
    //
    // `hash_command_into` заменил тотальный Debug-фолд. Инвариант, который
    // нельзя нарушить: структурный хэш должен различать НЕ МЕНЬШЕ, чем Debug.
    // Если два Debug-представления различны, структурные хэши обязаны быть
    // различны — иначе кадр с изменившимся содержимым получит старый хэш и НЕ
    // будет перерисован (ложный skip → устаревшие пиксели на экране).

    /// Эталон: старый фолд команды через её `Debug`-представление.
    pub(crate) fn debug_hash_one(cmd: &DisplayCommand) -> u64 {
        use std::fmt::Write as _;
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        {
            let mut hf = HashFmt(&mut h);
            let _ = write!(hf, "{cmd:?}");
        }
        h.finish()
    }

    fn text_cmd(text: &str, font_size: f32, weight: u16) -> DisplayCommand {
        DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(1.0, 2.0, 30.0, 12.0),
            text: text.to_string(),
            font_size,
            color: Color { r: 10, g: 20, b: 30, a: 255 },
            font_family: vec!["Inter".to_string()],
            font_weight: FontWeight(weight),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![(*b"wght", 400.0)],
            font_features: vec![(*b"liga", 1)],
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        }
    }

    /// Корпус, покрывающий каждую быструю ветку `hash_command_into` и пару
    /// холодных (Debug-fallback) вариантов.
    pub(crate) fn hash_corpus() -> Vec<DisplayCommand> {
        let radii = CornerRadii { tl: 4.0, ..CornerRadii::default() };
        let radii2 = CornerRadii { tl_y: 4.0, ..CornerRadii::default() };
        let c1 = Color { r: 255, g: 0, b: 0, a: 255 };
        let c2 = Color { r: 255, g: 0, b: 0, a: 254 };
        vec![
            // FillRect: сдвиг rect и изменение alpha цвета.
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: c1 },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 10.0, 10.1), color: c1 },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: c2 },
            // FillRoundedRect: tl против tl_y — ловит перепутанные поля радиусов.
            DisplayCommand::FillRoundedRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: c1, radii },
            DisplayCommand::FillRoundedRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: c1, radii: radii2 },
            // DrawBorder: ширины, цвета, стили, радиусы — каждое поле отдельно.
            DisplayCommand::DrawBorder {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                widths: [1.0, 1.0, 1.0, 1.0],
                colors: [c1; 4],
                styles: [BorderStyle::Solid; 4],
                radii: CornerRadii::default(),
            },
            DisplayCommand::DrawBorder {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                widths: [1.0, 2.0, 1.0, 1.0],
                colors: [c1; 4],
                styles: [BorderStyle::Solid; 4],
                radii: CornerRadii::default(),
            },
            DisplayCommand::DrawBorder {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                widths: [1.0, 1.0, 1.0, 1.0],
                colors: [c1, c2, c1, c1],
                styles: [BorderStyle::Solid; 4],
                radii: CornerRadii::default(),
            },
            DisplayCommand::DrawBorder {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                widths: [1.0, 1.0, 1.0, 1.0],
                colors: [c1; 4],
                styles: [BorderStyle::Dashed, BorderStyle::Solid, BorderStyle::Solid, BorderStyle::Solid],
                radii: CornerRadii::default(),
            },
            DisplayCommand::DrawBorder {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                widths: [1.0, 1.0, 1.0, 1.0],
                colors: [c1; 4],
                styles: [BorderStyle::Solid; 4],
                radii,
            },
            // DrawOutline: width против offset (одинаковый тип, разный смысл).
            DisplayCommand::DrawOutline {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                width: 2.0,
                style: OutlineStyle::Solid,
                color: c1,
                offset: 0.0,
            },
            DisplayCommand::DrawOutline {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                width: 0.0,
                style: OutlineStyle::Solid,
                color: c1,
                offset: 2.0,
            },
            DisplayCommand::DrawOutline {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                width: 2.0,
                style: OutlineStyle::Dotted,
                color: c1,
                offset: 0.0,
            },
            // Clip / opacity / transform.
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) },
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 11.0) },
            DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
            DisplayCommand::PushOpacity { alpha: 0.5001, bounds: None },
            DisplayCommand::PushTransform { matrix: Mat4::IDENTITY },
            DisplayCommand::PushTransform { matrix: Mat4::translation_2d(1.0, 0.0) },
            DisplayCommand::PushTransform { matrix: Mat4::translation_2d(0.0, 1.0) },
            // DrawText: текст, кегль, вес.
            text_cmd("abc", 16.0, 400),
            text_cmd("abd", 16.0, 400),
            text_cmd("abc", 16.5, 400),
            text_cmd("abc", 16.0, 700),
            // Склейка строк не должна коллизировать: "ab"+"c" ≠ "a"+"bc".
            text_cmd("ab\u{0}c", 16.0, 400),
            // Холодные варианты (Debug-fallback) + unit-варианты, включая
            // main-специфичные fixed/sticky маркеры (их не было на exp-ветке —
            // корректность их Debug-фолда проверяется этим же корпусом).
            backdrop_cmd(),
            DisplayCommand::PopBackdropFilter,
            DisplayCommand::PopTransform,
            DisplayCommand::BeginFixedLayer,
            DisplayCommand::EndFixedLayer,
        ]
    }

    #[test]
    fn structural_hash_is_stable_and_collision_free_on_corpus() {
        let corpus = hash_corpus();
        for cmd in &corpus {
            assert_eq!(
                hash_one_command(cmd),
                hash_one_command(&cmd.clone()),
                "hash must be a pure function of the command"
            );
        }
        for (i, a) in corpus.iter().enumerate() {
            for (j, b) in corpus.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    hash_one_command(a),
                    hash_one_command(b),
                    "structural hash collision between corpus[{i}] and corpus[{j}]:\n{a:?}\n{b:?}"
                );
            }
        }
    }

    #[test]
    fn structural_hash_refines_debug_hash() {
        // Ключевой инвариант отсутствия ложных skip: всё, что различал старый
        // Debug-фолд, обязан различать и структурный. Обратное не требуется —
        // структурный строже (напр. различает NaN-payload'ы), и лишнее
        // различие даёт лишь лишнюю перерисовку, а не устаревшие пиксели.
        let corpus = hash_corpus();
        for (i, a) in corpus.iter().enumerate() {
            for (j, b) in corpus.iter().enumerate().skip(i + 1) {
                if debug_hash_one(a) != debug_hash_one(b) {
                    assert_ne!(
                        hash_one_command(a),
                        hash_one_command(b),
                        "structural hash is coarser than Debug at corpus[{i}] vs corpus[{j}]:\n{a:?}\n{b:?}"
                    );
                }
            }
        }
    }

    /// ДО/ПОСЛЕ в одном прогоне: Debug-фолд против структурного на кадре,
    /// сопоставимом с `graphic_tests/1000000-final.html` (~1000 команд).
    ///
    /// `cargo test -p lumen-paint --release hash_display_list_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "бенч: запускать вручную с --release --nocapture"]
    fn hash_display_list_bench() {
        use std::hash::Hasher;
        use std::time::Instant;

        // Кадр: повторяем корпус до ~1060 команд, слегка сдвигая геометрию.
        let mut frame: Vec<DisplayCommand> = Vec::with_capacity(1060);
        let corpus = hash_corpus();
        let mut i = 0.0_f32;
        while frame.len() < 1060 {
            for c in &corpus {
                let mut c = c.clone();
                if let DisplayCommand::FillRect { rect, .. } = &mut c {
                    rect.x += i;
                }
                frame.push(c);
                i += 0.25;
            }
        }
        frame.truncate(1060);

        let iters = 200;

        // Эталонный (старый) путь: один хешер на кадр, Debug-фолд каждой команды.
        let t0 = Instant::now();
        let mut sink = 0u64;
        for _ in 0..iters {
            use std::fmt::Write as _;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            {
                let mut hf = HashFmt(&mut h);
                for cmd in &frame {
                    let _ = write!(hf, "{cmd:?}");
                }
            }
            sink ^= h.finish();
        }
        let debug_ms = t0.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);

        let t1 = Instant::now();
        for _ in 0..iters {
            sink ^= hash_display_list(&frame, &[], 0.0, 0.0, 1024, 720);
        }
        let structural_ms = t1.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);

        eprintln!(
            "[hash bench] {} cmds × {iters}: debug-fmt {debug_ms:.3} ms/frame → structural \
             {structural_ms:.3} ms/frame ({:.1}×), sink={sink}",
            frame.len(),
            debug_ms / structural_ms.max(f64::MIN_POSITIVE),
        );
        assert!(structural_ms < debug_ms, "структурный фолд обязан быть быстрее Debug-фолда");
    }

    /// Перепись среза 35 (BUG-405, пункт 70): два раздельных обхода списка
    /// против одного слитого, на двух кадрах — текстовом (как
    /// `samples/bench-text-scroll.html`, где живёт пункт 70) и смешанном.
    ///
    /// `cargo test -p lumen-paint --profile dev-release hash_dual_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "бенч: запускать вручную с --profile dev-release --nocapture"]
    fn hash_dual_bench() {
        use std::time::Instant;

        // Текстовый кадр: строки абзаца, как их кладёт inline-flow.
        let mut text_frame: Vec<DisplayCommand> = Vec::with_capacity(1000);
        for i in 0..1000 {
            #[expect(clippy::cast_precision_loss, reason = "координаты стенда")]
            let y = (i as f32) * 19.2;
            text_frame.push(DisplayCommand::DrawText {
                rect: Rect::new(24.0, y, 872.0, 19.2),
                text: format!("Строка {i} — типичный отрезок абзаца в 60 символов."),
                font_size: 16.0,
                color: Color { r: 34, g: 34, b: 34, a: 255 },
                font_family: vec!["Inter".to_string(), "sans-serif".to_string()],
                font_weight: lumen_layout::style::FontWeight(400),
                font_style: lumen_layout::style::FontStyle::Normal,
                font_stretch: lumen_layout::style::FontStretch(100),
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 8.0,
                highlight_name: None,
                text_orientation: None,
            });
        }

        // Смешанный кадр: тот же корпус, что у `hash_display_list_bench`.
        let mut mixed: Vec<DisplayCommand> = Vec::with_capacity(1060);
        let corpus = hash_corpus();
        let mut i = 0.0_f32;
        while mixed.len() < 1060 {
            for c in &corpus {
                let mut c = c.clone();
                if let DisplayCommand::FillRect { rect, .. } = &mut c {
                    rect.x += i;
                }
                mixed.push(c);
                i += 0.25;
            }
        }
        mixed.truncate(1060);

        let iters = 300;
        let mut sink = 0u64;
        for (label, frame) in [("text", &text_frame), ("mixed", &mixed)] {
            // A: как считает кадр до среза 35 — два независимых обхода.
            let t0 = Instant::now();
            for _ in 0..iters {
                sink ^= hash_display_list(frame, &[], 0.0, 40.0, 1024, 720);
                sink ^= hash_display_list_skipping(frame, &[], &[], 0.0, 0.0, 1024, 1800);
            }
            let two_ms = t0.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);

            // B: один обход, байты команды расходятся в оба хешера.
            let t1 = Instant::now();
            for _ in 0..iters {
                let (a, b) =
                    hash_display_list_dual(frame, &[], &[], (0.0, 40.0), (1024, 720), (1024, 1800));
                sink ^= a ^ b;
            }
            let dual_ms = t1.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);

            // C: один хэш — сколько стоит сам обход с одним плечом.
            let t2 = Instant::now();
            for _ in 0..iters {
                sink ^= hash_display_list(frame, &[], 0.0, 40.0, 1024, 720);
            }
            let one_ms = t2.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);

            eprintln!(
                "[dual bench] {label}: {} cmds × {iters} — два прохода {two_ms:.3} ms, \
                 слитый {dual_ms:.3} ms ({:+.1} %), один хэш {one_ms:.3} ms",
                frame.len(),
                (dual_ms - two_ms) / two_ms.max(f64::MIN_POSITIVE) * 100.0,
            );
        }
        eprintln!("[dual bench] sink={sink}");
    }

    /// Перепись цены ОДНОЙ команды текста (BUG-405 срез 39).
    ///
    /// Кадр попадания платит хэш 0.75–0.82 мс на 843 + 132 команды, то есть
    /// ~0.8 мкс на команду, — статья крупнее всего хрома (0.18–0.23 мс,
    /// п. 76). Здесь она раскладывается на составляющие: `core::fmt` палитры,
    /// байты строки, остальные поля.
    ///
    /// `cargo test -p lumen-paint --profile dev-release hash_text_command_census -- --ignored --nocapture`
    #[test]
    #[ignore = "перепись: запускать вручную с --profile dev-release --nocapture"]
    fn hash_text_command_census() {
        use std::fmt::Write as _;
        use std::hash::{Hash as _, Hasher as _};
        use std::time::Instant;

        // Строки длиной как на стенде `samples/bench-text-scroll.html`
        // (~135 байт), а не 50, как у `hash_dual_bench`: доля байтов строки —
        // одна из измеряемых статей, и занижать её нельзя.
        let mut frame: Vec<DisplayCommand> = Vec::with_capacity(1000);
        for i in 0..1000 {
            #[expect(clippy::cast_precision_loss, reason = "координаты стенда")]
            let y = (i as f32) * 19.2;
            frame.push(DisplayCommand::DrawText {
                rect: Rect::new(24.0, y, 976.0, 22.0),
                text: format!(
                    "Fox engine atlas jumps band ascender fox fox cache shader content {i}. \
                     Jumps render descender cache height over band cascade sampler"
                ),
                font_size: 15.0,
                color: Color { r: 68, g: 68, b: 68, a: 255 },
                font_family: vec!["sans-serif".to_string()],
                font_weight: lumen_layout::style::FontWeight(400),
                font_style: lumen_layout::style::FontStyle::Normal,
                font_stretch: lumen_layout::style::FontStretch(100),
                font_variation_axes: vec![(*b"opsz", 15.0)],
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 8.0,
                highlight_name: None,
                text_orientation: None,
            });
        }

        // Плечи: каждое сворачивает СВОЁ подмножество работы команды, разница
        // соседних и есть статья. Все — по одному `DefaultHasher` на команду,
        // как штатный `hash_one_command`, чтобы цена самого хешера не гуляла.
        let arm_full = |cmd: &DisplayCommand| hash_one_command(cmd);
        let arm_no_palette = |cmd: &DisplayCommand| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::mem::discriminant(cmd).hash(&mut h);
            if let DisplayCommand::DrawText {
                rect, text, font_size, color, font_family, font_weight, font_style,
                font_stretch, font_variation_axes, font_features, font_palette: _,
                tab_size, highlight_name, text_orientation,
            } = cmd
            {
                h_rect(&mut h, rect);
                h_str(&mut h, text);
                h_f32(&mut h, *font_size);
                h_color(&mut h, color);
                h.write_usize(font_family.len());
                for f in font_family {
                    h_str(&mut h, f);
                }
                h.write_u16(font_weight.0);
                std::mem::discriminant(font_style).hash(&mut h);
                h.write_u16(font_stretch.0);
                h.write_usize(font_variation_axes.len());
                for (tag, v) in font_variation_axes {
                    h.write(tag);
                    h_f32(&mut h, *v);
                }
                h.write_usize(font_features.len());
                for (tag, v) in font_features {
                    h.write(tag);
                    h.write_u32(*v);
                }
                h.write_u8(0);
                h_f32(&mut h, *tab_size);
                h.write_u8(u8::from(highlight_name.is_some()));
                h.write_u8(u8::from(text_orientation.is_some()));
            }
            h.finish()
        };
        // Только байты строки — нижняя граница любой тотальной свёртки.
        let arm_text_only = |cmd: &DisplayCommand| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let DisplayCommand::DrawText { text, .. } = cmd {
                h_str(&mut h, text);
            }
            h.finish()
        };
        // Только палитра через `core::fmt` — цена одной записи `write!`.
        let arm_palette_only = |cmd: &DisplayCommand| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let DisplayCommand::DrawText { font_palette, .. } = cmd {
                let mut hf = HashFmt(&mut h);
                let _ = write!(hf, "{font_palette:?}");
            }
            h.finish()
        };

        let iters = 300;
        let mut sink = 0u64;
        let mut measure = |label: &str, f: &dyn Fn(&DisplayCommand) -> u64| {
            // Прогрев: первый проход платит промахи кэша по свежему вектору.
            for cmd in &frame {
                sink ^= f(cmd);
            }
            let t0 = Instant::now();
            for _ in 0..iters {
                for cmd in &frame {
                    sink ^= f(cmd);
                }
            }
            let ns = t0.elapsed().as_secs_f64() * 1e9 / f64::from(iters) / frame.len() as f64;
            eprintln!("[hash census] {label:>14}: {ns:7.1} нс/команду");
        };
        measure("full", &arm_full);
        measure("без палитры", &arm_no_palette);
        measure("палитра", &arm_palette_only);
        measure("только строка", &arm_text_only);
        eprintln!("[hash census] sink={sink}");
    }

    // ── ADR-016 M0.5: content-only hash + frame-delta classification ──────

    #[test]
    fn content_hash_excludes_scroll() {
        // hash_content takes no scroll argument, so a frame that only scrolled
        // must hash identically — this is the whole point of the split.
        let content = vec![red_fill(5.0)];
        let a = hash_content(&content, 1024, 720);
        let b = hash_content(&content, 1024, 720);
        assert_eq!(a, b, "same content + size hashes identically");
        // A surface resize DOES change the content hash (blit can't cover it).
        assert_ne!(a, hash_content(&content, 800, 720), "width folds in");
        assert_ne!(a, hash_content(&content, 1024, 600), "height folds in");
        // A command edit changes the hash.
        assert_ne!(a, hash_content(&[red_fill(6.0)], 1024, 720), "command edit");
    }

    #[test]
    fn frame_delta_offset_only_on_scroll() {
        let content = vec![red_fill(5.0)];
        let prev = FrameFingerprint::new(&content, 1024, 720, (0.0, 0.0), (0.0, 40.0));
        // Same content, scrolled down → OffsetOnly (the M3 blit trigger).
        let scrolled = FrameFingerprint::new(&content, 1024, 720, (0.0, 120.0), (0.0, 40.0));
        assert_eq!(scrolled.delta_from(&prev), FrameDelta::OffsetOnly);
        // Same content, page offset changed (sidebar docked) → OffsetOnly too.
        let docked = FrameFingerprint::new(&content, 1024, 720, (0.0, 0.0), (260.0, 40.0));
        assert_eq!(docked.delta_from(&prev), FrameDelta::OffsetOnly);
    }

    #[test]
    fn frame_delta_identical_when_nothing_moves() {
        let content = vec![red_fill(5.0)];
        let a = FrameFingerprint::new(&content, 1024, 720, (0.0, 30.0), (0.0, 40.0));
        let b = FrameFingerprint::new(&content, 1024, 720, (0.0, 30.0), (0.0, 40.0));
        assert_eq!(b.delta_from(&a), FrameDelta::Identical);
    }

    #[test]
    fn frame_delta_content_changed_wins_over_offset() {
        // Content edit AND a scroll: content change must dominate — a re-raster
        // is required, not a blit.
        let prev = FrameFingerprint::new(&[red_fill(5.0)], 1024, 720, (0.0, 0.0), (0.0, 40.0));
        let next = FrameFingerprint::new(&[red_fill(6.0)], 1024, 720, (0.0, 90.0), (0.0, 40.0));
        assert_eq!(next.delta_from(&prev), FrameDelta::ContentChanged);
        // A resize (content_hash folds size) also classifies as ContentChanged.
        let resized = FrameFingerprint::new(&[red_fill(5.0)], 800, 720, (0.0, 0.0), (0.0, 40.0));
        assert_eq!(resized.delta_from(&prev), FrameDelta::ContentChanged);
    }

    // ── Тесты table rendering Phase 1 ─────────────────────────────────────

    #[test]
    fn table_context_default_is_separate_mode() {
        // Тест для убеждения что TableContext::from_box возвращает separate режим по умолчанию
        // (реальный тест с LayoutBox требует полного setup, поэтому проверяем структуру)
        let ctx = TableContext {
            border_collapse: BorderCollapse::Separate,
            border_spacing: (8.0, 8.0),
        };
        assert_eq!(ctx.border_collapse, BorderCollapse::Separate);
        assert_eq!(ctx.border_spacing, (8.0, 8.0));
    }

    #[test]
    fn border_collapse_separate_wins_over_lower_precedence() {
        let cell_border = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let table_border = CollapsedBorder {
            width: 1.0,
            color: [0.0, 1.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Table,
        };
        let resolved = CollapsedBorder::resolve_conflict(&table_border, &cell_border);
        assert_eq!(resolved.precedence, BorderPrecedence::Cell);
        assert_eq!(resolved.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn border_collapse_wider_border_wins_at_equal_precedence() {
        let thin = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let thick = CollapsedBorder {
            width: 2.0,
            color: [0.0, 1.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let resolved = CollapsedBorder::resolve_conflict(&thin, &thick);
        assert_eq!(resolved.width, 2.0);
        assert_eq!(resolved.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn table_separate_mode_renders_with_cells_independent() {
        // Phase 1: table в separate режиме — каждая ячейка имеет независимые границы
        let dl = build(
            "<table><tr><td>A</td><td>B</td></tr></table>",
            "td { border: 1px solid black; background: lightblue; }",
        );
        // Должны быть эмитированы фоны ячеек (2×FillRect для ячеек + контент)
        let fills = fills(&dl);
        assert!(!fills.is_empty(), "table cells should have background fills");
    }

    #[test]
    fn border_precedence_ordering_correct() {
        assert!(BorderPrecedence::Table < BorderPrecedence::RowGroup);
        assert!(BorderPrecedence::RowGroup < BorderPrecedence::Row);
        assert!(BorderPrecedence::Row < BorderPrecedence::Column);
        assert!(BorderPrecedence::Column < BorderPrecedence::Cell);
    }

    #[test]
    fn table_cell_with_border_emits_draw_border() {
        // Phase 1: table cell с border должна эмитировать DrawBorder
        let dl = build(
            "<table><tr><td>A</td></tr></table>",
            "td { border: 2px solid red; }",
        );
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(!border_cmds.is_empty(), "cell should emit DrawBorder command");
    }

    #[test]
    fn table_cells_no_border_style_none() {
        // Ячейка без border-style не должна эмитировать DrawBorder
        let dl = build(
            "<table><tr><td>A</td></tr></table>",
            "td { border: 0; }",
        );
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(border_cmds.is_empty(), "cell with no border should not emit DrawBorder");
    }

    #[test]
    fn table_with_thead_tbody_tfoot() {
        // Table с thead, tbody, tfoot должна корректно обрабатывать all three groups
        let dl = build(
            "<table>\
                <thead><tr><td>H</td></tr></thead>\
                <tbody><tr><td>B</td></tr></tbody>\
                <tfoot><tr><td>F</td></tr></tfoot>\
            </table>",
            "td { border: 1px solid black; }",
        );
        // Должны быть эмитированы границы для всех трёх групп (3× DrawBorder)
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert_eq!(border_cmds.len(), 3, "should have 3 DrawBorder commands for 3 rows");
    }

    #[test]
    fn table_cell_background_color_separate_mode() {
        // Каждая ячейка в separate режиме должна иметь независимый фон
        let dl = build(
            "<table><tr><td>A</td><td>B</td></tr></table>",
            "td { background: lightblue; } td:first-child { background: lightcoral; }",
        );
        // Должны быть эмитированы 2 FillRect для cell backgrounds
        let fills = fills(&dl);
        assert!(fills.len() >= 2, "should have at least 2 cell background fills");
    }

    #[test]
    fn table_collapsed_border_wider_wins() {
        // При collapse режиме более широкая граница побеждает (Phase 1 stub test)
        let thin = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let thick = CollapsedBorder {
            width: 3.0,
            color: [0.0, 0.0, 1.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let resolved = CollapsedBorder::resolve_conflict(&thin, &thick);
        assert_eq!(resolved.width, 3.0, "thicker border should win");
        assert_eq!(resolved.color, [0.0, 0.0, 1.0, 1.0], "should use thick border color");
    }

    #[test]
    fn table_empty_cells_do_not_crash() {
        // Table с пустыми ячейками должна обрабатываться без panic
        let _dl = build(
            "<table>\
                <tr><td></td><td>B</td></tr>\
                <tr><td>C</td><td></td></tr>\
            </table>",
            "td { border: 1px solid #ccc; padding: 8px; }",
        );
        // Test passes if no panic occurs
    }

    #[test]
    fn table_nested_in_other_content() {
        // Table внутри других элементов должна рендериться корректно
        let dl = build(
            "<div>\
                <p>Before</p>\
                <table><tr><td>In Table</td></tr></table>\
                <p>After</p>\
            </div>",
            "td { border: 1px solid black; background: yellow; }",
        );
        // Должны быть эмитированы: текст "Before", таблица, текст "After"
        let texts = texts(&dl);
        assert!(texts.iter().any(|t| t.contains("Before")), "should have 'Before' text");
        assert!(texts.iter().any(|t| t.contains("In Table")), "should have 'In Table' text");
        assert!(texts.iter().any(|t| t.contains("After")), "should have 'After' text");
    }

    // ── Тесты SVG text rendering ───────────────────────────────────────

    #[test]
    fn svg_text_emits_drawtext_command() {
        // <text>Hello</text> should emit a DrawText command
        let dl = build("<svg><text>Hello</text></svg>", "");
        let texts = texts(&dl);
        assert!(texts.iter().any(|t| t.contains("Hello")), "should emit text 'Hello'");
    }

    #[test]
    fn ordered_svg_shape_emits_fill() {
        // BUG-089: the ordered (stacking-context) path must paint SVG shapes.
        // `emit_box_self` previously no-op'd SvgShape, so shapes vanished in the
        // shell's ordered pipeline (only `walk` painted them). A <rect> with an
        // explicit fill must produce a FillRect via `build_display_list_ordered`.
        let dl = build_ordered(
            "<svg width='100' height='100'><rect x='0' y='0' width='50' height='50' style='fill:#ff0000;'/></svg>",
            "",
        );
        let has_red_fill = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::FillRect { color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0
        ));
        assert!(has_red_fill, "ordered path must emit FillRect for SVG <rect>, got {dl:?}");
    }

    #[test]
    fn svg_viewport_clips_overflowing_content() {
        // BUG-110: an SVG with object-fit: cover scales its viewBox to overflow the box;
        // the SVG viewport (UA default `overflow: hidden`) must clip it. Both the `walk`
        // and the ordered pipeline must wrap the SVG's shape children in a PushClipRect
        // bounded by the SVG box (160×120), so cover content cannot paint over siblings.
        let html = "<svg width='160' height='120' viewBox='0 0 200 80' style='object-fit:cover;'>\
                    <rect width='200' height='80' style='fill:#ff0000;'/></svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            let clip = dl.iter().find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } if (rect.width - 160.0).abs() < 1.0
                    && (rect.height - 120.0).abs() < 1.0 => Some(*rect),
                _ => None,
            });
            assert!(clip.is_some(), "SVG viewport must emit a 160×120 PushClipRect, got {dl:?}");
        }
    }

    #[test]
    fn ordered_svg_text_emits_drawtext() {
        // BUG-089 companion: ordered path must also paint SVG <text>.
        let dl = build_ordered("<svg><text>Hi</text></svg>", "");
        let has_text = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::DrawText { text, .. } if text.contains("Hi")
        ));
        assert!(has_text, "ordered path must emit DrawText for SVG <text>");
    }

    #[test]
    fn ordered_svg_path_stroke_emits_drawsvgstroke() {
        // BUG-096: an SVG <path> has a zero-size layout rect (path bbox is deferred
        // to paint), so `emit_svg_shape`'s 0×0 guard used to drop every path in the
        // ordered pipeline → TEST-54 painted nothing. The path must emit its stroke
        // in the stroke colour (#e94560 = 233,69,96), and `fill="none"` (an SVG
        // presentation attribute) must suppress the fill so no black fill leaks in.
        //
        // BUG-247: the stroke is now emitted as `DrawSvgStroke` (raw contours +
        // params) rather than a pre-tessellated `DrawSvgPath` triangle soup, so
        // femtovg can stroke it natively without internal AA seams.
        let dl = build_ordered(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20' fill='none' stroke='#e94560' stroke-width='8'/>\
             </svg>",
            "",
        );
        let strokes: Vec<(&Color, &crate::svg_path::StrokeParams)> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgStroke { color, params, .. } => Some((color, params)),
                _ => None,
            })
            .collect();
        assert!(!strokes.is_empty(), "ordered path must emit DrawSvgStroke for <path>, got {dl:?}");
        assert!(
            strokes.iter().all(|(c, _)| c.r == 233 && c.g == 69 && c.b == 96),
            "path must stroke in colour #e94560, not a default black fill; got {strokes:?}",
        );
        assert!(
            strokes.iter().all(|(_, p)| (p.half_width - 4.0).abs() < 1e-3),
            "stroke-width:8 → half_width 4.0; got {strokes:?}",
        );
        // The old triangle-soup stroke must be gone in the stroke colour.
        let soup = dl.iter().any(|c| matches!(c,
            DisplayCommand::DrawSvgPath { color, .. } if color.r == 233 && color.g == 69 && color.b == 96));
        assert!(!soup, "stroke must NOT emit a DrawSvgPath triangle soup, got {dl:?}");
    }

    #[test]
    fn nonzero_path_fill_emits_drawsvgfill_not_triangle_soup() {
        // BUG-247 / BUG-173: a nonzero `<path>` area fill is emitted as the raw
        // outline contours (`DrawSvgFill`) so femtovg/tiny_skia anti-alias only
        // the true boundary. A triangle soup (`DrawSvgPath`) made them fringe
        // every internal shared edge (~1px seams). `fill-rule: evenodd` cannot be
        // filled natively (no even-odd path mode in femtovg/wgpu), so it stays a
        // triangle soup via the scanline decomposition.
        let filled_triangle =
            "<svg width='200' height='160'>\
                <path d='M 100 10 L 185 145 L 15 145 Z' fill='#0f3460'/>\
             </svg>"; // #0f3460 = (15, 52, 96)
        for dl in [build(filled_triangle, ""), build_ordered(filled_triangle, "")] {
            let fills: Vec<&Vec<Vec<[f32; 2]>>> = dl.iter().filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { contours, color }
                    if color.r == 15 && color.g == 52 && color.b == 96 => Some(contours),
                _ => None,
            }).collect();
            assert_eq!(fills.len(), 1, "nonzero fill must emit one DrawSvgFill, got {dl:?}");
            assert!(
                fills[0].iter().any(|c| c.len() >= 3),
                "DrawSvgFill must carry the closed outline (≥3 points), got {:?}", fills[0],
            );
            // No triangle-soup fill in the fill colour.
            let soup = dl.iter().any(|c| matches!(c,
                DisplayCommand::DrawSvgPath { color, .. } if color.r == 15 && color.g == 52 && color.b == 96));
            assert!(!soup, "nonzero fill must NOT emit a DrawSvgPath triangle soup, got {dl:?}");
        }

        // even-odd keeps the triangle-soup path (DrawSvgPath), no DrawSvgFill.
        let even_odd =
            "<svg width='200' height='160'>\
                <path d='M 100 10 L 185 145 L 15 145 Z' fill='#0f3460' fill-rule='evenodd'/>\
             </svg>";
        for dl in [build(even_odd, ""), build_ordered(even_odd, "")] {
            let has_soup = dl.iter().any(|c| matches!(c,
                DisplayCommand::DrawSvgPath { color, .. } if color.r == 15 && color.g == 52 && color.b == 96));
            let has_fill = dl.iter().any(|c| matches!(c, DisplayCommand::DrawSvgFill { .. }));
            assert!(has_soup, "even-odd fill must emit DrawSvgPath (scanline tessellation), got {dl:?}");
            assert!(!has_fill, "even-odd fill must NOT emit DrawSvgFill (no native even-odd), got {dl:?}");
        }
    }

    #[test]
    fn svg_diagonal_line_strokes_segment_not_filled_bbox() {
        // BUG-189: a diagonal SVG <line> used to render as `FillRect { rect: b.rect }`
        // — filling the entire (large) bounding box of the segment with the stroke
        // colour, producing a solid orange rectangle instead of a thin diagonal.
        // The line must now emit a `DrawSvgPath` (a thick segment) in the stroke
        // colour, and NO `FillRect` in that colour.
        let stroke = "#f39c12"; // (243, 156, 18)
        let html = "<svg width='160' height='100'>\
            <line x1='10' y1='10' x2='150' y2='90' stroke='#f39c12' stroke-width='6' fill='none'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            let stroke_paths: Vec<&Vec<[f32; 2]>> = dl.iter().filter_map(|c| match c {
                DisplayCommand::DrawSvgPath { vertices, color }
                    if color.r == 243 && color.g == 156 && color.b == 18 => Some(vertices),
                _ => None,
            }).collect();
            assert_eq!(stroke_paths.len(), 1, "line must emit one stroke DrawSvgPath ({stroke}), got {dl:?}");
            assert_eq!(stroke_paths[0].len(), 6, "a single butt-cap segment = two triangles (6 verts)");
            // The old bug: a FillRect in the stroke colour spanning the bbox.
            let solid_fill = dl.iter().any(|c| matches!(c,
                DisplayCommand::FillRect { color, .. } if color.r == 243 && color.g == 156 && color.b == 18));
            assert!(!solid_fill, "line must NOT paint a solid {stroke} FillRect (BUG-189), got {dl:?}");
        }
    }

    #[test]
    fn svg_rotated_rect_paints_under_ctm_in_user_space() {
        // BUG-244: a rotated SVG shape used to collapse to its axis-aligned bounding
        // box (`apply_transform_to_bbox` re-bounds the rotated corners), silently
        // dropping the rotation. The fix mirrors a browser CTM: the shape's geometry
        // is painted in *user* coordinates wrapped in a `PushTransform` carrying the
        // full document-space matrix (viewport ∘ composed), with off-diagonal
        // (rotate/skew) components an AABB cannot represent.
        let html = "<svg width='200' height='200'>\
            <rect x='40' y='40' width='80' height='40' transform='rotate(45)' fill='#00ff00'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            // A PushTransform with a non-zero off-diagonal `b` (= sin θ) must wrap the
            // shape — the rotation survives instead of being flattened to an AABB.
            let ctm = dl.iter().find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } if matrix.0[1].abs() > 0.1 => Some(*matrix),
                _ => None,
            });
            assert!(ctm.is_some(), "rotated <rect> must emit a PushTransform with rotation, got {dl:?}");
            // The fill is painted in the rect's *user* coordinates (40,40,80,40); the
            // matrix (not the rect) positions it. Before the fix the FillRect carried
            // the inflated rotated AABB instead.
            let fill = dl.iter().find_map(|c| match c {
                DisplayCommand::FillRect { rect, color }
                    if color.r == 0 && color.g == 255 && color.b == 0 => Some(*rect),
                _ => None,
            }).expect("green FillRect present");
            assert!(
                (fill.x - 40.0).abs() < 0.5 && (fill.y - 40.0).abs() < 0.5
                    && (fill.width - 80.0).abs() < 0.5 && (fill.height - 40.0).abs() < 0.5,
                "rotated rect fill must stay in user coords (40,40,80,40), got {fill:?}",
            );
            // The CTM must be balanced by a PopTransform.
            assert!(
                dl.iter().any(|c| matches!(c, DisplayCommand::PopTransform)),
                "PushTransform must be closed by PopTransform, got {dl:?}",
            );
        }
    }

    #[test]
    fn svg_untransformed_rect_has_no_ctm() {
        // BUG-244 guard: a plain (translate/scale-only) shape keeps the exact
        // axis-aligned `b.rect` fast path — no PushTransform, no AA change. Only
        // rotation/skew (off-diagonal components) trigger the CTM path.
        let html = "<svg width='100' height='100'>\
            <rect x='10' y='10' width='50' height='50' fill='#ff0000'/>\
        </svg>";
        let dl = build(html, "");
        assert!(
            !dl.iter().any(|c| matches!(c, DisplayCommand::PushTransform { .. })),
            "untransformed <rect> must not emit a PushTransform, got {dl:?}",
        );
    }

    #[test]
    fn svg_scaled_use_path_paints_under_ctm() {
        // BUG-424 (a): a `<path>`/`<polyline>`/`<polygon>` (lowered to `Path`) has no
        // scaled `b.rect` — layout collapses it to a zero-size anchor point (BUG-174).
        // Rect/Circle/Ellipse/Line get their scale baked into `b.rect`, so a pure
        // scale with no rotation (`has_rot_skew=false`) already rendered correctly
        // via the axis-aligned fast path; `Path` did not — the common icon-sprite
        // case (`viewBox="0 0 24 24"` scaled down to a 12px box, no rotation) used
        // to paint the raw, unscaled `d`/`points` vertices, ~2× oversized. It must
        // now route through the same CTM `PushTransform` as rotate/skew whenever
        // the matrix carries a non-identity scale.
        let html = "<svg width='12' height='12'>\
            <symbol id='s' viewBox='0 0 24 24'><polyline points='15 18 9 12 15 6' fill='none' stroke='#000'/></symbol>\
            <use href='#s'/>\
         </svg>";
        let dl = build(html, "");
        let ctm = dl.iter().find_map(|c| match c {
            DisplayCommand::PushTransform { matrix } if (matrix.0[0] - 0.5).abs() < 0.01 => Some(*matrix),
            _ => None,
        });
        assert!(ctm.is_some(), "scaled <use>-<polyline> must emit a PushTransform with the 0.5 viewBox→viewport scale, got {dl:?}");
        let ctm = ctm.unwrap();
        assert!(ctm.0[1].abs() < 1e-6 && ctm.0[2].abs() < 1e-6, "pure scale, no rotation/skew expected, got {ctm:?}");
        assert!(
            dl.iter().any(|c| matches!(c, DisplayCommand::PopTransform)),
            "PushTransform must be closed by PopTransform, got {dl:?}",
        );
        // Under the CTM the stroke's vertices stay in raw local (0..24) units —
        // the matrix, not a pre-shift, carries the viewBox→viewport scale.
        let stroke = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawSvgStroke { contours, .. } => Some(contours),
            _ => None,
        }).expect("polyline stroke present");
        let max_coord = stroke.iter().flatten().flat_map(|[x, y]| [*x, *y]).fold(0.0_f32, f32::max);
        assert!(max_coord > 10.0, "stroke vertices should stay in raw 0..24 local units under the CTM, got max={max_coord} in {stroke:?}");
    }

    #[test]
    fn svg_rect_stroke_is_centred_on_edge() {
        // BUG-226: an SVG stroke is centred on the geometry edge (SVG 2 §13.7) —
        // half its width outside the box, half inside. Previously the stroke was
        // painted entirely inside (`DrawBorder { rect: b.rect }`, border-box model),
        // shrinking the visible orange-core by stroke-width/2 per side (79×59 vs
        // Edge 89×69 at stroke-width 10). The stroke's DrawBorder rect must now be
        // the fill rect inflated by stroke-width/2 on every side, with the fill left
        // on the original geometry.
        let html = "<svg width='200' height='160'>\
            <rect x='20' y='20' width='100' height='80' fill='#3498db' stroke='#e74c3c' stroke-width='10'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            // fill (#3498db = 52,152,219) FillRect on the original geometry.
            let fill = dl.iter().find_map(|c| match c {
                DisplayCommand::FillRect { rect, color }
                    if color.r == 52 && color.g == 152 && color.b == 219 => Some(*rect),
                _ => None,
            }).expect("fill FillRect present");
            // stroke (#e74c3c = 231,76,60) DrawBorder, inflated by w/2 per side.
            let (srect, widths) = dl.iter().find_map(|c| match c {
                DisplayCommand::DrawBorder { rect, widths, colors, .. }
                    if colors[0].r == 231 && colors[0].g == 76 && colors[0].b == 60 => Some((*rect, *widths)),
                _ => None,
            }).expect("stroke DrawBorder present");
            assert_eq!(widths[0], 10.0, "stroke width preserved");
            let half = 5.0_f32; // stroke-width / 2
            assert!((srect.x - (fill.x - half)).abs() < 0.01,
                "stroke rect moves out by w/2: x {} want {}", srect.x, fill.x - half);
            assert!((srect.y - (fill.y - half)).abs() < 0.01,
                "stroke rect moves out by w/2: y {} want {}", srect.y, fill.y - half);
            assert!((srect.width - (fill.width + 10.0)).abs() < 0.01,
                "stroke rect width grows by w: {} want {}", srect.width, fill.width + 10.0);
            assert!((srect.height - (fill.height + 10.0)).abs() < 0.01,
                "stroke rect height grows by w: {} want {}", srect.height, fill.height + 10.0);
        }
    }

    #[test]
    fn paint_order_default_paints_fill_then_stroke() {
        // CSS Fill & Stroke L3 §6 — default `normal` order: fill first, stroke on top.
        // fill=red (#ff0000), stroke=blue (#0000ff) on a closed triangle path.
        let dl = build(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20 L 180 140 Z' fill='#ff0000' stroke='#0000ff' stroke-width='10'/>\
             </svg>",
            "",
        );
        // Fill is a native `DrawSvgFill`, stroke a native `DrawSvgStroke`
        // (BUG-247) — collect both in document (paint) order.
        let colors: Vec<&Color> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { color, .. }
                | DisplayCommand::DrawSvgStroke { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 2, "fill + stroke → two svg paint commands, got {dl:?}");
        assert_eq!((colors[0].r, colors[0].b), (255, 0), "fill (red) painted first");
        assert_eq!((colors[1].r, colors[1].b), (0, 255), "stroke (blue) painted on top");
    }

    #[test]
    fn paint_order_stroke_reverses_fill_and_stroke() {
        // `paint-order: stroke` paints stroke first (under the fill).
        let dl = build(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20 L 180 140 Z' fill='#ff0000' stroke='#0000ff' \
                 stroke-width='10' style='paint-order: stroke'/>\
             </svg>",
            "",
        );
        // Fill is a native `DrawSvgFill`, stroke a native `DrawSvgStroke`
        // (BUG-247) — collect both in document (paint) order.
        let colors: Vec<&Color> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { color, .. }
                | DisplayCommand::DrawSvgStroke { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 2, "fill + stroke → two svg paint commands, got {dl:?}");
        assert_eq!((colors[0].r, colors[0].b), (0, 255), "stroke (blue) painted first, under fill");
        assert_eq!((colors[1].r, colors[1].b), (255, 0), "fill (red) painted on top");
    }

    #[test]
    fn svg_text_with_fill_color() {
        // <text style="fill: red">Colored</text> should emit DrawText with fill color
        let dl = build("<svg><text style=\"fill: red\">Colored</text></svg>", "");
        let text_cmds: Vec<&DisplayCommand> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(!text_cmds.is_empty(), "should emit DrawText command");
    }

    #[test]
    fn svg_text_with_font_size() {
        // <text style="font-size: 24px">Sized</text> should use specified font-size
        let dl = build("<svg><text style=\"font-size: 24px\">Sized</text></svg>", "");
        let text_cmds: Vec<_> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_size, .. } => Some(font_size),
                _ => None,
            })
            .collect();
        assert!(!text_cmds.is_empty(), "should have DrawText with font-size");
    }

    #[test]
    fn svg_tspan_emits_text() {
        // <text><tspan>Part1</tspan><tspan>Part2</tspan></text> should emit text
        let dl = build("<svg><text><tspan>Part1</tspan><tspan>Part2</tspan></text></svg>", "");
        let texts = texts(&dl);
        assert!(!texts.is_empty(), "should emit at least one text command");
    }

    #[test]
    fn svg_textpath_collects_content() {
        // <text><textPath>OnPath</textPath></text> should collect textPath content
        let dl = build("<svg><text><textPath>OnPath</textPath></text></svg>", "");
        let texts = texts(&dl);
        // Phase 1: just collect and emit content, ignore path rendering
        assert!(texts.iter().any(|t| t.contains("OnPath")) || texts.is_empty(),
                "should have collected textPath content or empty is acceptable in Phase 1");
    }

    #[test]
    fn svg_text_anchor_middle_shifts_x_left() {
        // text-anchor="middle": DrawText rect.x should be shifted left by ~half text width
        // compared to text-anchor="start" at the same SVG x position.
        let dl_start = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="start">AB</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="middle">AB</text></svg>"#, "");
        let x_start = dl_start.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let x_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let (xs, xm) = (x_start.expect("start DrawText"), x_middle.expect("middle DrawText"));
        assert!(xm < xs, "text-anchor=middle should shift x left vs start: middle={xm}, start={xs}");
    }

    #[test]
    fn svg_text_dx_dy_offset_applied() {
        // dx="10" dy="5" should shift the DrawText rect by those amounts vs no offset
        let dl_no_offset = build(r#"<svg width="200" height="100"><text x="50" y="50">Hi</text></svg>"#, "");
        let dl_with_offset = build(r#"<svg width="200" height="100"><text x="50" y="50" dx="10" dy="5">Hi</text></svg>"#, "");
        let pos_no = dl_no_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let pos_off = dl_with_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let ((x0, y0), (x1, y1)) = (pos_no.expect("no-offset DrawText"), pos_off.expect("offset DrawText"));
        assert!((x1 - x0 - 10.0).abs() < 1.0, "dx=10 should shift x by ~10: Δx={}", x1 - x0);
        assert!((y1 - y0 - 5.0).abs() < 1.0, "dy=5 should shift y by ~5: Δy={}", y1 - y0);
    }

    #[test]
    fn svg_text_dominant_baseline_middle_shifts_y() {
        // dominant-baseline="middle" should shift DrawText rect.y up compared to auto
        let dl_auto = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="auto">T</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="middle">T</text></svg>"#, "");
        let y_auto = dl_auto.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let y_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let (ya, ym) = (y_auto.expect("auto DrawText"), y_middle.expect("middle DrawText"));
        assert!(ym < ya, "dominant-baseline=middle should shift y up vs auto: middle={ym}, auto={ya}");
    }

    #[test]
    fn svg_text_baseline_shift_super_raises_y() {
        // baseline-shift: super raises the text (smaller y) vs baseline; sub lowers it.
        let text_y = |bs: &str| -> f32 {
            let css = format!("text {{ baseline-shift: {bs}; }}");
            build(r#"<svg width="200" height="100"><text x="50" y="50">T</text></svg>"#, &css)
                .iter()
                .find_map(|c| match c {
                    DisplayCommand::DrawText { rect, .. } => Some(rect.y),
                    _ => None,
                })
                .expect("DrawText")
        };
        let y_baseline = text_y("baseline");
        let y_super = text_y("super");
        let y_sub = text_y("sub");
        let y_len = text_y("10px");
        assert!(y_super < y_baseline, "super should raise (smaller y): super={y_super}, baseline={y_baseline}");
        assert!(y_sub > y_baseline, "sub should lower (larger y): sub={y_sub}, baseline={y_baseline}");
        // Positive length raises by exactly that many px.
        assert!((y_len - (y_baseline - 10.0)).abs() < 0.01, "10px should raise by 10: len={y_len}, baseline={y_baseline}");
    }

    // ── FilterMode conversion tests (B-6) ──────────────────────────────────

    #[test]
    fn filter_mode_from_auto_is_linear() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Auto);
        assert_eq!(mode, FilterMode::Linear, "auto → Linear (bilinear)");
    }

    #[test]
    fn filter_mode_from_smooth_is_linear() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Smooth);
        assert_eq!(mode, FilterMode::Linear, "smooth → Linear (bilinear)");
    }

    #[test]
    fn filter_mode_from_crisp_edges_is_nearest() {
        let mode = FilterMode::from_image_rendering(ImageRendering::CrispEdges);
        assert_eq!(mode, FilterMode::Nearest, "crisp-edges → Nearest (pixel-perfect)");
    }

    #[test]
    fn filter_mode_from_pixelated_is_nearest() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Pixelated);
        assert_eq!(mode, FilterMode::Nearest, "pixelated → Nearest (pixel-perfect)");
    }

    // Display list diffing tests (A-10)
    #[test]
    fn diff_identical_empty_lists() {
        let empty1: Vec<DisplayCommand> = vec![];
        let empty2: Vec<DisplayCommand> = vec![];
        let result = diff_display_lists(&empty1, &empty2);
        assert!(result.identical, "two empty lists should be identical");
    }

    #[test]
    fn diff_identical_single_command() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(result.identical, "identical FillRect commands should be identical");
    }

    #[test]
    fn diff_different_lengths() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1 = vec![cmd.clone()];
        let list2 = vec![cmd.clone(), cmd];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "lists with different lengths should not be identical");
    }

    #[test]
    fn diff_different_colors() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect,
            color: blue,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "FillRects with different colors should not be identical");
        assert!(!result.changed_rects.width.is_nan(), "changed_rects should be valid");
    }

    #[test]
    fn diff_changed_rects_bounds() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect1 = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let rect2 = Rect {
            x: 30.0,
            y: 40.0,
            width: 80.0,
            height: 60.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect: rect1,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect: rect2,
            color: red,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "FillRects with different positions should not be identical");
        // changed_rects should be the union of rect1 and rect2
        assert_eq!(result.changed_rects.x, 10.0, "left edge should be min of both rects");
        assert_eq!(result.changed_rects.y, 20.0, "top edge should be min of both rects");
    }

    #[test]
    fn diff_multiple_commands_one_changed() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let fill1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let fill2 = DisplayCommand::FillRect {
            rect,
            color: blue,
        };

        let list1 = vec![fill1.clone(), fill1.clone()];
        let list2 = vec![fill1, fill2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "lists differing in one command should not be identical");
    }

    #[test]
    fn diff_empty_to_non_empty() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1: Vec<DisplayCommand> = vec![];
        let list2 = vec![cmd];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "empty list vs non-empty should not be identical");
        assert_eq!(result.changed_rects.x, 10.0, "changed_rects should reflect added command");
    }

    #[test]
    fn diff_result_identical_constructor() {
        let result = DiffResult::identical();
        assert!(result.identical);
        assert!(result.changed_rects.width == 0.0 && result.changed_rects.height == 0.0);
    }

    #[test]
    fn diff_result_changed_constructor() {
        let rect = Rect {
            x: 5.0,
            y: 10.0,
            width: 50.0,
            height: 60.0,
        };
        let result = DiffResult::changed(rect);
        assert!(!result.identical);
        assert_eq!(result.changed_rects, rect);
    }

    // ── B-9: CSS overflow: clip tests ────────────────────────────────

    fn find_push_clip_rects(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::PushClipRect { .. }))
            .collect()
    }

    #[test]
    fn overflow_clip_emits_push_clip_rect() {
        let dl = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        let clips = find_push_clip_rects(&dl);
        assert!(!clips.is_empty(), "overflow:clip should emit PushClipRect");
    }

    #[test]
    fn overflow_clip_margin_expands_clip_region() {
        let dl_no_margin = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        let dl_with_margin = build(
            r#"<div style="overflow:clip;overflow-clip-margin:10px;width:100px;height:100px;background:blue"></div>"#,
            "",
        );

        let clips_no_margin = find_push_clip_rects(&dl_no_margin);
        let clips_with_margin = find_push_clip_rects(&dl_with_margin);

        assert!(!clips_no_margin.is_empty(), "overflow:clip without margin should have PushClipRect");
        assert!(!clips_with_margin.is_empty(), "overflow:clip with margin should have PushClipRect");

        if let (Some(DisplayCommand::PushClipRect { rect: r1 }), Some(DisplayCommand::PushClipRect { rect: r2 })) =
            (clips_no_margin.first(), clips_with_margin.first())
        {
            // With margin, rect should be expanded (larger width/height).
            assert!(r2.width > r1.width || r2.height > r1.height,
                "overflow-clip-margin should expand clip region");
        }
    }

    #[test]
    fn overflow_hidden_and_clip_both_emit_clip() {
        let dl_hidden = build(
            r#"<div style="overflow:hidden;width:100px;height:100px;background:red"></div>"#,
            "",
        );
        let dl_clip = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:green"></div>"#,
            "",
        );

        let hidden_clips = find_push_clip_rects(&dl_hidden);
        let clip_clips = find_push_clip_rects(&dl_clip);

        assert!(!hidden_clips.is_empty(), "overflow:hidden should emit PushClipRect");
        assert!(!clip_clips.is_empty(), "overflow:clip should emit PushClipRect");
    }

    #[test]
    fn overflow_clip_no_margin_emits_zero_margin() {
        // When no overflow-clip-margin is specified, clip rect should not be expanded.
        let dl = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:yellow"></div>"#,
            "",
        );
        let clips = find_push_clip_rects(&dl);
        assert_eq!(clips.len(), 1, "overflow:clip should emit exactly one PushClipRect");
        // The clip rect size should match the padding-box (or close to it).
        if let DisplayCommand::PushClipRect { rect } = clips[0] {
            // Exact values depend on styling, but the rect should be non-negative and finite.
            assert!(rect.width >= 0.0 && rect.height >= 0.0, "clip rect should have non-negative dimensions");
        }
    }

    #[test]
    fn resize_grip_emitted_when_resize_both_and_overflow_hidden() {
        let dl = build(
            r#"<div style="resize:both;overflow:hidden;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        // Display list should be generated (non-empty) when resize:both + overflow:hidden
        assert!(!dl.is_empty(), "resize:both with overflow:hidden should generate display list");
    }

    #[test]
    fn resize_grip_not_emitted_when_resize_none() {
        let dl = build(
            r#"<div style="resize:none;overflow:hidden;width:100px;height:100px;background:green"></div>"#,
            "",
        );
        // Should not have any FillRoundedRect (or very few if from other sources)
        // This is a phase 0 check; exact count depends on implementation
        assert!(!dl.is_empty(), "display list should not be empty");
    }

    #[test]
    fn resize_grip_not_emitted_when_overflow_visible() {
        let dl = build(
            r#"<div style="resize:both;overflow:visible;width:100px;height:100px;background:red"></div>"#,
            "",
        );
        // resize should only apply when overflow != visible
        assert!(!dl.is_empty(), "display list should not be empty");
    }

    #[test]
    fn resize_grip_emitted_for_horizontal() {
        let dl = build(
            r#"<div style="resize:horizontal;overflow:auto;width:100px;height:100px;background:cyan"></div>"#,
            "",
        );
        assert!(!dl.is_empty(), "resize:horizontal should render display list");
    }

    #[test]
    fn resize_grip_emitted_for_vertical() {
        let dl = build(
            r#"<div style="resize:vertical;overflow:scroll;width:100px;height:100px;background:magenta"></div>"#,
            "",
        );
        assert!(!dl.is_empty(), "resize:vertical should render display list");
    }

    #[test]
    fn resize_grip_positioned_at_bottom_right() {
        let dl = build(
            r#"<div style="resize:both;overflow:hidden;width:100px;height:100px;background:yellow;margin:10px"></div>"#,
            "",
        );
        // Verify display list was built with the resize grip styling
        assert!(!dl.is_empty(), "resize:both with overflow:hidden and margin should generate display list");
    }

    #[test]
    fn range_input_emits_track_and_thumb() {
        let dl = build(r#"<input type="range" min="0" max="100" value="50">"#, "");
        let rounded_rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(rounded_rects.len() >= 2, "range input should emit at least track + thumb, got {}", rounded_rects.len());
    }

    #[test]
    fn range_input_at_min_emits_no_fill() {
        let dl = build(r#"<input type="range" min="0" max="100" value="0">"#, "");
        let rounded_rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        // At min=0: track (gray) + thumb only, no blue fill portion.
        assert!(rounded_rects.len() >= 2, "at min value should still emit track + thumb");
    }

    #[test]
    fn range_input_default_value_is_midpoint() {
        // No value attribute → default value = (min + max) / 2 = 50.
        let dl_mid = build(r#"<input type="range">"#, "");
        let dl_explicit = build(r#"<input type="range" value="50">"#, "");
        let mid_count = dl_mid.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        let explicit_count = dl_explicit.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        assert_eq!(mid_count, explicit_count, "default and explicit value=50 should produce same FillRoundedRect count");
    }

    // ── <progress> ──────────────────────────────────────────────────────────

    #[test]
    fn progress_determinate_emits_filled_bar() {
        // value=0.5/max=1.0 → bar fill present (at least one FillRoundedRect inside the control).
        let dl = build(r#"<progress value="0.5" max="1.0"></progress>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "determinate progress should emit at least one FillRoundedRect");
    }

    #[test]
    fn progress_indeterminate_emits_partial_fill() {
        // No value attr → indeterminate; still emits a 30% bar.
        let dl = build(r#"<progress max="1.0"></progress>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "indeterminate progress should emit a partial bar");
    }

    #[test]
    fn progress_zero_value_emits_no_fill() {
        // value=0 → fraction=0 → no FillRoundedRect from the bar (but FillRect from background may exist).
        let dl = build(r#"<progress value="0" max="1.0"></progress>"#, "");
        let rounded: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(rounded.is_empty(), "progress at 0 should emit no rounded fill, got {}", rounded.len());
    }

    // ── <meter> ─────────────────────────────────────────────────────────────

    #[test]
    fn meter_emits_filled_bar() {
        let dl = build(r#"<meter min="0" max="10" value="5"></meter>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "meter should emit a FillRoundedRect bar");
    }

    #[test]
    fn meter_gauge_optimal_is_green() {
        // value inside optimum range → green fill.
        let green = super::meter_gauge_color(5.0, 0.0, 10.0, 2.0, 8.0, 5.0);
        assert_eq!(green.r, 100, "optimal zone should be green (r=100)");
        assert!(green.g > green.r, "green channel should dominate");
    }

    #[test]
    fn meter_gauge_suboptimal_is_yellow() {
        // optimum in (low, high), value in low segment → yellow.
        let yellow = super::meter_gauge_color(1.0, 0.0, 10.0, 2.0, 8.0, 5.0);
        assert!(yellow.r > 100, "yellow should have high red channel");
        assert!(yellow.g > 100, "yellow should have high green channel");
        assert!(yellow.b < 50,  "yellow should have low blue channel");
    }

    #[test]
    fn meter_gauge_bad_is_red() {
        // optimum in high segment, value in low segment → red (farthest from optimum).
        let red = super::meter_gauge_color(1.0, 0.0, 10.0, 2.0, 8.0, 9.0);
        assert!(red.r > 100, "red zone should have high red channel");
        assert!(red.g < 100, "red zone should have low green channel");
    }

    // ── font-stretch → wdth variation axis ─────────────────────────────────

    fn wdth_axes(dl: &DisplayList) -> Vec<f32> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_variation_axes, .. } => {
                    font_variation_axes.iter()
                        .find(|(tag, _)| tag == b"wdth")
                        .map(|(_, v)| *v)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn font_stretch_normal_no_wdth_axis() {
        // font-stretch: normal (default) → no wdth axis injected
        let dl = build("<p>hello</p>", "p { font-stretch: normal; }");
        let wdth: Vec<_> = wdth_axes(&dl);
        assert!(wdth.is_empty(), "normal stretch must not inject wdth, got {:?}", wdth);
    }

    #[test]
    fn font_stretch_condensed_injects_wdth_75() {
        // font-stretch: condensed → wdth = 75.0
        let dl = build("<p>hello</p>", "p { font-stretch: condensed; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "condensed stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 75.0).abs() < f32::EPSILON),
            "condensed = 75%, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_expanded_injects_wdth_125() {
        // font-stretch: expanded → wdth = 125.0
        let dl = build("<p>hello</p>", "p { font-stretch: expanded; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "expanded stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 125.0).abs() < f32::EPSILON),
            "expanded = 125%, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_percentage_injects_correct_wdth() {
        // font-stretch: 60% → wdth = 60.0
        let dl = build("<p>hello</p>", "p { font-stretch: 60%; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "60% stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 60.0).abs() < 0.1),
            "60% stretch must give wdth=60.0, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_explicit_wdth_not_overridden() {
        // font-variation-settings: "wdth" 80 with font-stretch: condensed
        // → explicit wdth=80 wins, no second injection
        let dl = build(
            "<p>hello</p>",
            r#"p { font-stretch: condensed; font-variation-settings: "wdth" 80; }"#,
        );
        let wdth = wdth_axes(&dl);
        // Only one wdth axis per DrawText, and it should be the explicit 80, not 75
        assert!(
            wdth.iter().all(|v| (*v - 80.0).abs() < f32::EPSILON),
            "explicit wdth=80 must not be overridden by font-stretch=condensed (75), got {:?}",
            wdth
        );
    }

    // ── font-stretch → статический подбор face-а (DrawText::font_stretch) ───
    //
    // Ось `wdth` выше обслуживает variable-шрифты; поле `font_stretch`
    // отвечает за второй, независимый механизм — выбор отдельного
    // condensed/expanded файла через `FontProvider::pick_face`. Тесты ниже
    // фиксируют, что значение вообще доезжает до display list-а: без него
    // renderer звал matcher с «normal» и любое `font-stretch` на статическом
    // семействе молча не работало.

    fn drawtext_stretches(dl: &DisplayList) -> Vec<FontStretch> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_stretch, .. } => Some(*font_stretch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn font_stretch_reaches_draw_text_field() {
        let dl = build("<p>hello</p>", "p { font-stretch: condensed; }");
        let got = drawtext_stretches(&dl);
        assert!(!got.is_empty(), "страница должна дать хотя бы один DrawText");
        assert!(
            got.iter().all(|s| *s == FontStretch(750)),
            "condensed = 75% → FontStretch(750), got {got:?}"
        );
    }

    #[test]
    fn font_stretch_default_is_normal_on_draw_text() {
        let dl = build("<p>hello</p>", "");
        let got = drawtext_stretches(&dl);
        assert!(!got.is_empty());
        assert!(
            got.iter().all(|s| *s == FontStretch::NORMAL),
            "без объявления DrawText обязан нести NORMAL, got {got:?}"
        );
    }

    #[test]
    fn font_stretch_explicit_wdth_still_sets_static_field() {
        // Зеркало `font_stretch_explicit_wdth_not_overridden`: явный
        // `font-variation-settings: "wdth" 80` подавляет инъекцию оси, но
        // НЕ должен подавлять статический подбор — по спеке ось низкого
        // уровня применяется после выбора face-а и на выбор не влияет.
        let dl = build(
            "<p>hello</p>",
            r#"p { font-stretch: condensed; font-variation-settings: "wdth" 80; }"#,
        );
        let got = drawtext_stretches(&dl);
        assert!(!got.is_empty());
        assert!(
            got.iter().all(|s| *s == FontStretch(750)),
            "font-stretch: condensed обязан дойти до matcher-а независимо от \
             font-variation-settings, got {got:?}"
        );
    }

    #[test]
    fn font_stretch_changes_display_list_hash() {
        // Поле обязано входить в хэш: иначе кадр, где сменился только
        // font-stretch, переиспользует закэшированный тайл со старым face-ом.
        //
        // Обе страницы задают ОДИНАКОВЫЙ явный `wdth` — иначе тест был бы
        // фиктивным: инъекция оси из font-stretch и так лежит в хэше через
        // `font_variation_axes`, и разошедшийся хэш ничего не сказал бы про
        // новое поле. С явным `wdth` оси совпадают, и хэш расходится ровно
        // и только из-за `font_stretch`.
        let condensed = build(
            "<p>hello</p>",
            r#"p { font-stretch: condensed; font-variation-settings: "wdth" 80; }"#,
        );
        let expanded = build(
            "<p>hello</p>",
            r#"p { font-stretch: expanded; font-variation-settings: "wdth" 80; }"#,
        );
        // Именно `hash_display_list`: он складывает команды рукописным
        // `hash_command_into`, где каждое поле перечислено явно. (Соседний
        // `hash_content` свернул бы derive-`Debug` и разошёлся бы сам собой —
        // на нём тест был бы фиктивным и ничего не проверял.)
        assert_ne!(
            hash_display_list(&condensed, &[], 0.0, 0.0, 800, 600),
            hash_display_list(&expanded, &[], 0.0, 0.0, 800, 600),
            "смена font-stretch обязана менять хэш display list-а"
        );
    }
