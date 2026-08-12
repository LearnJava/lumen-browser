// Requires `backend-wgpu` feature (uses Renderer directly).
#![cfg(feature = "backend-wgpu")]
/// GPU headless render tests — require a real GPU adapter.
///
/// Marked `#[ignore]` by default so they don't run in CPU-only CI.
/// Run explicitly with:
///   cargo test -p lumen-paint --test headless_tests -- --include-ignored
use lumen_core::geom::Rect;
use lumen_core::ColorSpace;
use lumen_layout::{Color, FilterFn};
use lumen_paint::{DisplayCommand, Renderer};

const INTER: &[u8] = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");

fn red_rect_dl(w: f32, h: f32) -> Vec<DisplayCommand> {
    vec![DisplayCommand::FillRect {
        rect: Rect { x: 0.0, y: 0.0, width: w, height: h },
        color: Color { r: 255, g: 0, b: 0, a: 255 },
    }]
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_render_dimensions() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    let img = r.render_to_image(&red_rect_dl(64.0, 48.0), 0.0, 0.0)
        .expect("render_to_image");
    assert_eq!(img.width, 64);
    assert_eq!(img.height, 48);
    assert_eq!(img.data.len(), 64 * 48 * 4);
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_render_red_rect() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    let img = r.render_to_image(&red_rect_dl(64.0, 64.0), 0.0, 0.0)
        .expect("render_to_image");

    // Centre pixel should be red (R=255 G=0 B=0 A=255).
    let cx = 32usize;
    let cy = 32usize;
    let offset = (cy * 64 + cx) * 4;
    let pix = &img.data[offset..offset + 4];
    assert_eq!(pix[0], 255, "R должен быть 255");
    assert_eq!(pix[1], 0,   "G должен быть 0");
    assert_eq!(pix[2], 0,   "B должен быть 0");
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_resize_updates_dimensions() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 32, 32, ColorSpace::Srgb)
        .expect("headless renderer");
    r.resize(128, 96);
    let img = r.render_to_image(&red_rect_dl(128.0, 96.0), 0.0, 0.0)
        .expect("render_to_image after resize");
    assert_eq!(img.width, 128);
    assert_eq!(img.height, 96);
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 — прогрев ленивых пайплайнов
// ─────────────────────────────────────────────────────────────────────────────

/// Display list со скруглённым клипом, которому нужен offscreen-уровень:
/// заставляет кадр взять ленивые `rrect-clip`/`composite`-пайплайны (BUG-406
/// вынес их из старта окна).
///
/// Клип **вложенный** намеренно: с BUG-405 срез 4 одиночный скруглённый клип
/// обслуживается контуром в шейдере и уровня не открывает, а внешний контур в
/// шейдере ровно один — поэтому уровень берёт внутренний клип.
fn rounded_clip_dl(w: f32, h: f32) -> Vec<DisplayCommand> {
    let outer = Rect { x: 2.0, y: 2.0, width: w - 4.0, height: h - 4.0 };
    let mid = Rect { x: 4.0, y: 4.0, width: w - 8.0, height: h - 8.0 };
    let rect = Rect { x: 6.0, y: 6.0, width: w - 12.0, height: h - 12.0 };
    // Вложенность ТРИ, а не два: два контура шейдер держит сам (BUG-405
    // срез 8), и от двойного клипа ленивый `rrect_clip`-пайплайн больше не
    // нужен — контроль «кадру нужен ленивый пайплайн» стал бы негодным.
    vec![
        DisplayCommand::PushClipRoundedRect { rect: outer, radii: [8.0, 8.0, 8.0, 8.0] },
        DisplayCommand::PushClipRoundedRect { rect: mid, radii: [8.0, 8.0, 8.0, 8.0] },
        DisplayCommand::PushClipRoundedRect { rect, radii: [8.0, 8.0, 8.0, 8.0] },
        DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
    ]
}

/// BUG-406: ленивые пайплайны не компилируются при создании рендера.
///
/// Проверяется счётчиком компиляций, а не временем старта: время зависит от
/// драйвера и загрузки машины, а «ни одна ячейка не заполнена» — нет.
#[test]
#[ignore = "requires GPU adapter"]
fn lazy_pipelines_are_absent_right_after_construction() {
    let r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    assert_eq!(
        r.warmed_pipeline_count(),
        0,
        "ленивые пайплайны не должны компилироваться в конструкторе (BUG-406)",
    );
}

/// BUG-405: после прогрева кадр, которому нужны ленивые пайплайны, **ничего**
/// не компилирует.
///
/// Это и есть гейт правки: «компиляция ушла с кадра» = «за время кадра
/// `pipelines_compiled()` не вырос». Первая половина теста — контроль на
/// ложно-зелёный: без прогрева тот же самый кадр счётчик поднимает, иначе
/// проверка проходила бы и на рендере, который эти пайплайны вообще не трогает.
///
/// Счётчик инстансный, а не процессный: тесты одного бинарника идут
/// параллельно, и на общем счётчике эти два рендера считали бы компиляции
/// друг друга.
#[test]
#[ignore = "requires GPU adapter"]
fn warmup_takes_pipeline_compilation_off_the_frame() {
    // Контроль: холодный рендер тот же кадр компилирует.
    let mut cold = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    cold.set_font_provider(None);
    cold.render_to_image(&rounded_clip_dl(64.0, 64.0), 0.0, 0.0)
        .expect("render_to_image (cold)");
    assert!(
        cold.pipelines_compiled() > 0,
        "контроль негоден: кадр не потребовал ни одного ленивого пайплайна",
    );

    // Прогретый рендер: тот же кадр — ноль компиляций.
    let mut warm = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    warm.set_font_provider(None);
    warm.warm_lazy_pipelines_blocking();
    assert_eq!(
        warm.warmed_pipeline_count(),
        14,
        "прогрев обязан заполнить все ленивые ячейки",
    );
    let before_warm = warm.pipelines_compiled();
    warm.render_to_image(&rounded_clip_dl(64.0, 64.0), 0.0, 0.0)
        .expect("render_to_image (warm)");
    assert_eq!(
        warm.pipelines_compiled(),
        before_warm,
        "после прогрева кадр не должен компилировать пайплайны (BUG-405)",
    );
}

/// BUG-405: прогрев компилирует каждый ленивый пайплайн ровно один раз.
///
/// Сторожит от «прогрели и тут же перекомпилировали»: если бы результат не
/// попадал в `OnceCell` (например, `set` молча проглотили), счётчик ячеек
/// остался бы нулём при выросшем счётчике компиляций.
#[test]
#[ignore = "requires GPU adapter"]
fn warmup_compiles_each_lazy_pipeline_exactly_once() {
    let r = Renderer::new_headless(INTER.to_vec(), 32, 32, ColorSpace::Srgb)
        .expect("headless renderer");
    assert_eq!(
        r.pipelines_compiled(),
        0,
        "до прогрева ленивые пайплайны не компилируются (BUG-406)",
    );
    r.warm_lazy_pipelines_blocking();
    // `mask-layer` — одна ячейка на два пайплайна (luminance/alpha), поэтому
    // компиляций на одну больше, чем ячеек.
    assert_eq!(r.pipelines_compiled(), 15, "прогрев собрал не тот набор пайплайнов");
    assert_eq!(r.warmed_pipeline_count(), 14);
}

// ─────────────────────────────────────────────────────────────────────────────
// CPU render tests (tiny-skia, no GPU required, feature="cpu-render")
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "cpu-render")]
#[test]
fn cpu_render_dimensions() {
    let img = Renderer::render_to_image_cpu(64, 48, &red_rect_dl(64.0, 48.0), &[], 0.0, 0.0)
        .expect("render_to_image_cpu");
    assert_eq!(img.width, 64);
    assert_eq!(img.height, 48);
    assert_eq!(img.data.len(), 64 * 48 * 4);
}

#[cfg(feature = "cpu-render")]
#[test]
fn cpu_render_red_rect() {
    let img = Renderer::render_to_image_cpu(64, 64, &red_rect_dl(64.0, 64.0), &[], 0.0, 0.0)
        .expect("render_to_image_cpu");

    // Центральный пиксель должен быть красным (R=255 G=0 B=0 A=255).
    let cx = 32usize;
    let cy = 32usize;
    let offset = (cy * 64 + cx) * 4;
    let pix = &img.data[offset..offset + 4];
    assert_eq!(pix[0], 255, "R должен быть 255");
    assert_eq!(pix[1], 0,   "G должен быть 0");
    assert_eq!(pix[2], 0,   "B должен быть 0");
    assert_eq!(pix[3], 255, "A должен быть 255");
}

#[cfg(feature = "cpu-render")]
#[test]
fn cpu_render_red_rect_partial() {
    // Рендерим красный квадрат 32x32 в левый верхний угол 64x64 canvas.
    let dl = vec![DisplayCommand::FillRect {
        rect: Rect { x: 0.0, y: 0.0, width: 32.0, height: 32.0 },
        color: Color { r: 255, g: 0, b: 0, a: 255 },
    }];
    let img = Renderer::render_to_image_cpu(64, 64, &dl, &[], 0.0, 0.0)
        .expect("render_to_image_cpu");

    // Пиксель в левом верхнем углу квадрата (10, 10) должен быть красным.
    let offset = (10 * 64 + 10) * 4;
    let pix = &img.data[offset..offset + 4];
    assert_eq!(pix[0], 255, "R в (10,10) должен быть 255");
    assert_eq!(pix[1], 0,   "G в (10,10) должен быть 0");
    assert_eq!(pix[2], 0,   "B в (10,10) должен быть 0");

    // Пиксель вне квадрата (50, 50) должен быть белым (фон).
    let offset = (50 * 64 + 50) * 4;
    let pix = &img.data[offset..offset + 4];
    assert_eq!(pix[0], 255, "R в (50,50) должен быть 255 (белый фон)");
    assert_eq!(pix[1], 255, "G в (50,50) должен быть 255 (белый фон)");
    assert_eq!(pix[2], 255, "B в (50,50) должен быть 255 (белый фон)");
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 срез 2 — подача кадра порциями
// ─────────────────────────────────────────────────────────────────────────────

/// Display list из `n` скруглённых клипов подряд: каждый добавляет в план
/// кадра свой draw + композит клипа, то есть план заведомо длиннее одной
/// порции подачи.
///
/// Все `n` завёрнуты в общий скруглённый клип: с BUG-405 срез 4 контур в
/// шейдере занимает внешний клип, и внутренние — те, чьи пассы считает этот
/// тест, — идут прежним путём уровня.
fn many_clips_dl(w: f32, h: f32, n: usize) -> Vec<DisplayCommand> {
    let mut dl = Vec::with_capacity(n * 3 + 2);
    dl.push(DisplayCommand::PushClipRoundedRect {
        rect: Rect { x: 2.0, y: 2.0, width: w - 4.0, height: h - 4.0 },
        radii: [2.0; 4],
    });
    for i in 0..n {
        let y = 4.0 + i as f32 * 6.0;
        let rect = Rect { x: 4.0, y, width: w - 8.0, height: 4.0 };
        dl.push(DisplayCommand::PushClipRoundedRect { rect, radii: [2.0; 4] });
        dl.push(DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } });
        dl.push(DisplayCommand::PopClip);
    }
    dl.push(DisplayCommand::PopClip);
    dl
}

/// `n` заливок, каждая под цепочкой из `depth` вложенных скруглённых клипов.
///
/// Шейдер держит [`SHADER_CLIP_MAX_CONTOURS`] = 2 контура (BUG-405 срез 8),
/// поэтому `depth` = 2 — целевой случай среза (уровней нет вовсе), а `depth`
/// = 3 — тот, что на уровне остаётся: он и служит контролем на ложно-зелёный.
fn nested_clips_dl(w: f32, h: f32, n: usize, depth: usize) -> Vec<DisplayCommand> {
    let mut dl = Vec::with_capacity(n * (2 * depth + 1));
    for i in 0..n {
        let y = 4.0 + i as f32 * 6.0;
        for d in 0..depth {
            let inset = d as f32 * 0.5;
            dl.push(DisplayCommand::PushClipRoundedRect {
                rect: Rect {
                    x: 4.0 + inset,
                    y: y + inset,
                    width: w - 8.0 - 2.0 * inset,
                    height: 4.0,
                },
                radii: [2.0; 4],
            });
        }
        dl.push(DisplayCommand::FillRect {
            rect: Rect { x: 4.0, y, width: w - 8.0, height: 4.0 },
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        });
        for _ in 0..depth {
            dl.push(DisplayCommand::PopClip);
        }
    }
    let _ = h;
    dl
}

/// BUG-405 срез 2: кадр из многих пассов подаётся несколькими командными
/// списками, а короткий кадр — по-прежнему одним.
///
/// Гейт стоит на счётчике подач, а не на времени кадра: «список не копится»
/// — утверждение о механизме, оно не зависит ни от железа, ни от загрузки
/// машины (время кадра зависит от обоих).
#[test]
#[ignore = "requires GPU adapter"]
fn long_frame_is_submitted_in_chunks() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    // Контроль: короткий кадр (один прямоугольник) — одна подача.
    let before = r.submissions();
    r.render_to_image(&red_rect_dl(64.0, 128.0), 0.0, 0.0).expect("короткий кадр");
    assert_eq!(
        r.submissions() - before,
        1,
        "короткий кадр не должен резаться на подачи",
    );

    // Целевой случай: длинный план — больше одной подачи. Список берётся с
    // вложенностью 3: два контура шейдер держит сам (срез 8), уровень —
    // и, значит, пассы — открывает только третий.
    let before = r.submissions();
    r.render_to_image(&nested_clips_dl(64.0, 128.0, 12, 3), 0.0, 0.0).expect("длинный кадр");
    assert!(
        r.submissions() - before > 1,
        "кадр из многих пассов подан одним списком: подач {}",
        r.submissions() - before,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 срез 4 — скруглённый клип контуром в шейдере, без offscreen-уровня
// ─────────────────────────────────────────────────────────────────────────────

/// `n` скруглённых клипов подряд на верхнем уровне (без объемлющего клипа) —
/// ровно тот случай, который срез 4 уводит с уровней в шейдер.
fn flat_clips_dl(w: f32, h: f32, n: usize) -> Vec<DisplayCommand> {
    let mut dl = Vec::with_capacity(n * 3);
    for i in 0..n {
        let y = 4.0 + i as f32 * 6.0;
        let rect = Rect { x: 4.0, y, width: w - 8.0, height: 4.0 };
        dl.push(DisplayCommand::PushClipRoundedRect { rect, radii: [2.0; 4] });
        dl.push(DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } });
        dl.push(DisplayCommand::PopClip);
    }
    let _ = h;
    dl
}

/// BUG-405 срез 4: скруглённый клип не открывает offscreen-уровень.
///
/// Гейт стоит на счётчике уровней, а не на времени кадра: «клип перестал
/// стоить своих трёх пассов» — утверждение о механизме, оно не зависит ни от
/// железа, ни от загрузки машины.
///
/// Вторая половина — контроль на ложно-зелёный: клип глубже двух контуров
/// обязан уровень открыть, иначе тест проходил бы и на рендере, который
/// скруглённые клипы вообще не обрабатывает.
#[test]
#[ignore = "requires GPU adapter"]
fn rounded_clip_costs_no_offscreen_level() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let before = r.rrect_clip_levels();
    r.render_to_image(&flat_clips_dl(64.0, 128.0, 12), 0.0, 0.0).expect("кадр с клипами");
    assert_eq!(
        r.rrect_clip_levels() - before,
        0,
        "скруглённый клип верхнего уровня всё ещё открывает offscreen-уровень",
    );

    // BUG-405 срез 8: вложенный клип обслуживает ВТОРОЙ контур того же слота.
    let (before, nested_before) = (r.rrect_clip_levels(), r.nested_shader_clips());
    r.render_to_image(&nested_clips_dl(64.0, 128.0, 12, 2), 0.0, 0.0)
        .expect("кадр с вложенными");
    assert_eq!(
        r.rrect_clip_levels() - before,
        0,
        "вложенный скруглённый клип всё ещё открывает offscreen-уровень",
    );
    assert_eq!(
        r.nested_shader_clips() - nested_before,
        12,
        "вложенные клипы не доехали до второго контура",
    );

    // Контроль на ложно-зелёный №1: третий контур в слот не помещается,
    // и такой клип обязан остаться на пути уровня.
    let (before, nested_before) = (r.rrect_clip_levels(), r.nested_shader_clips());
    r.render_to_image(&nested_clips_dl(64.0, 128.0, 12, 3), 0.0, 0.0)
        .expect("кадр с тройной вложенностью");
    assert_eq!(
        r.rrect_clip_levels() - before,
        12,
        "клип глубже двух контуров обязан открыть уровень (контроль негоден)",
    );
    assert_eq!(
        r.nested_shader_clips() - nested_before,
        12,
        "второй контур обязан достаться среднему клипу тройной цепочки",
    );

    // Контроль на ложно-зелёный №2: плечо A/B обязано вернуть прежнее
    // поведение — иначе счётчик уровней не различает два пути и гейт пуст.
    r.set_nested_shader_clip_enabled(false);
    let (before, nested_before) = (r.rrect_clip_levels(), r.nested_shader_clips());
    r.render_to_image(&nested_clips_dl(64.0, 128.0, 12, 2), 0.0, 0.0)
        .expect("кадр с вложенными без второго контура");
    assert_eq!(
        r.rrect_clip_levels() - before,
        12,
        "плечо без второго контура обязано открывать уровень (контроль негоден)",
    );
    assert_eq!(
        r.nested_shader_clips() - nested_before,
        0,
        "плечо без второго контура всё ещё считает вложенные клипы шейдерными",
    );
    r.set_nested_shader_clip_enabled(true);
}

/// BUG-405 срез 8: вложенный клип двумя контурами даёт ту же картинку, что и
/// прежний offscreen-уровень.
///
/// Оба плеча снимаются в одном процессе (`set_nested_shader_clip_enabled`) на
/// одном списке. Побайтового равенства тут быть не может: прежний путь
/// умножал покрытие ВНЕШНЕГО контура в восьмибитную текстуру уровня, а
/// композит домножал её на покрытие ВНУТРЕННЕГО — то есть округлял дважды;
/// новый берёт то же произведение во фрагменте и округляет один раз. Порог
/// назван измерением (см. bugs/BUG-405-OPEN.md, срез 8).
///
/// Проверки «центр залит» и «угол вырезан» обязательны у ОБОИХ плеч: два
/// пустых кадра (или два неклипнутых) сравнивались бы зелено.
#[test]
#[ignore = "requires GPU adapter"]
fn nested_shader_clip_matches_level() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 128, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let outer = Rect { x: 8.0, y: 8.0, width: 96.0, height: 48.0 };
    let inner = Rect { x: 16.0, y: 16.0, width: 80.0, height: 32.0 };
    let mut dl = vec![
        // Пара «внешний со скруглением + внутренний со скруглением»: углы
        // режут оба контура, и в углах живёт вся разница путей.
        DisplayCommand::PushClipRoundedRect { rect: outer, radii: [20.0; 4] },
        DisplayCommand::PushClipRoundedRect { rect: inner, radii: [12.0; 4] },
        DisplayCommand::FillRect { rect: outer, color: red },
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
    ];
    // Внутренний клип, ВЫЕЗЖАЮЩИЙ за внешний, — то, что происходит на
    // прокрутке: пересечение контуров обязано резать по обоим.
    dl.extend([
        DisplayCommand::PushClipRoundedRect {
            rect: Rect { x: 8.0, y: 64.0, width: 60.0, height: 28.0 },
            radii: [10.0; 4],
        },
        DisplayCommand::PushClipRoundedRect {
            rect: Rect { x: 40.0, y: 70.0, width: 60.0, height: 28.0 },
            radii: [8.0, 0.0, 8.0, 0.0],
        },
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 60.0, width: 128.0, height: 40.0 },
            color: Color { r: 0, g: 160, b: 0, a: 255 },
        },
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
    ]);
    // Под переносом и внутри видимого уровня — обе формы, в которых
    // вложенный клип доезжает до кадра на реальной странице.
    dl.push(DisplayCommand::PushTransform {
        matrix: lumen_layout::Mat4::translation_2d(0.0, 40.0),
    });
    dl.push(DisplayCommand::PushOpacity { alpha: 0.6, bounds: None });
    dl.extend([
        DisplayCommand::PushClipRoundedRect {
            rect: Rect { x: 12.0, y: 64.0, width: 100.0, height: 20.0 },
            radii: [9.0; 4],
        },
        DisplayCommand::PushClipRoundedRect {
            rect: Rect { x: 20.0, y: 66.0, width: 60.0, height: 16.0 },
            radii: [7.0; 4],
        },
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 60.0, width: 128.0, height: 30.0 },
            color: Color { r: 0, g: 0, b: 255, a: 255 },
        },
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
    ]);
    dl.push(DisplayCommand::PopOpacity);
    dl.push(DisplayCommand::PopTransform);

    r.set_nested_shader_clip_enabled(false);
    let before = r.rrect_clip_levels();
    let level = r.render_to_image(&dl, 0.0, 0.0).expect("плечо уровня");
    let levels_off = r.rrect_clip_levels() - before;

    r.set_nested_shader_clip_enabled(true);
    let (before, nested_before) = (r.rrect_clip_levels(), r.nested_shader_clips());
    let shader = r.render_to_image(&dl, 0.0, 0.0).expect("плечо второго контура");
    let (levels_on, nested) = (r.rrect_clip_levels() - before, r.nested_shader_clips() - nested_before);

    assert_eq!(levels_on, 0, "вложенный клип всё ещё открывает уровень");
    assert_eq!(nested, 3, "не все вложенные клипы списка ушли на второй контур");
    assert!(levels_off > 0, "плечо уровня не открывает уровней (контроль негоден)");

    let max_diff = level
        .data
        .iter()
        .zip(shader.data.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max_diff <= 1,
        "второй контур разошёлся с уровнем: {max_diff}/255 \
         (уровней {levels_off} -> {levels_on})",
    );

    // Клип обязан быть виден у ОБОИХ плеч: и центр залит, и угол вырезан.
    for (name, img) in [("уровень", &level), ("контур", &shader)] {
        let px = |x: usize, y: usize| {
            let o = (y * 128 + x) * 4;
            [img.data[o], img.data[o + 1], img.data[o + 2]]
        };
        assert_eq!(px(56, 32), [255, 0, 0], "центр вложенного клипа плеча «{name}» не залит");
        // (17,17) лежит вне скругления радиуса 12 у угла (16,16) внутреннего
        // контура: до центра скругления (28,28) — 15.6 px.
        assert_ne!(px(17, 17), [255, 0, 0], "угол вложенного клипа плеча «{name}» не вырезан");
    }
}

/// BUG-405 срез 4: углы шейдерного клипа действительно вырезаны.
///
/// Счётчик уровней говорит только «пассов нет»; без этой проверки правка,
/// которая просто перестала клипать, тоже была бы зелёной.
#[test]
#[ignore = "requires GPU adapter"]
fn shader_rounded_clip_carves_corners() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let rect = Rect { x: 4.0, y: 4.0, width: 56.0, height: 56.0 };
    let dl = vec![
        DisplayCommand::PushClipRoundedRect { rect, radii: [16.0; 4] },
        DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
        DisplayCommand::PopClip,
    ];
    let img = r.render_to_image(&dl, 0.0, 0.0).expect("render_to_image");
    let px = |x: usize, y: usize| {
        let o = (y * 64 + x) * 4;
        [img.data[o], img.data[o + 1], img.data[o + 2]]
    };

    // Центр — внутри контура: заливка видна.
    assert_eq!(px(32, 32), [255, 0, 0], "центр клипа обязан быть залит");
    // (5,5) лежит вне скругления радиуса 16 у угла (4,4): расстояние до центра
    // скругления (20,20) — 21.2 px.
    assert_ne!(px(5, 5), [255, 0, 0], "угол клипа не вырезан");
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 срез 5 — невидимый offscreen-уровень не разрезает пасс родителя
// ─────────────────────────────────────────────────────────────────────────────

/// Красная полоса, `n` opacity-групп с заданным содержимым между полосами.
///
/// `content` строит содержимое группы по её номеру: пустой вектор — уровень с
/// пустым bbox (то, что даёт прокрутка реального сайта: элемент с `opacity`,
/// поддерево которого в этом кадре ничего не рисует), непустой — обычный
/// видимый уровень (контроль).
fn opacity_groups_dl(
    n: usize,
    content: impl Fn(usize) -> Vec<DisplayCommand>,
) -> Vec<DisplayCommand> {
    let mut dl = Vec::new();
    for i in 0..n {
        let y = 2.0 + i as f32 * 6.0;
        dl.push(DisplayCommand::FillRect {
            rect: Rect { x: 2.0, y, width: 60.0, height: 4.0 },
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        });
        dl.push(DisplayCommand::PushOpacity { alpha: 0.5, bounds: None });
        dl.extend(content(i));
        dl.push(DisplayCommand::PopOpacity);
    }
    dl
}

/// BUG-405 срез 5: уровень, выброшенный viewport-cull-ом, не оставляет за собой
/// лишнего пасса родителя.
///
/// Гейт стоит на счётчиках механизма, а не на времени кадра: «разрез отменён»
/// (`cull_merges`) и «пассов у кадра столько, сколько целей» (`plan_passes`).
///
/// Вторая половина — контроль на ложно-зелёный: те же группы с непустым
/// содержимым обязаны остаться тремя пассами каждая (батч родителя, контент
/// уровня, композит). Без контроля тест прошёл бы и на правке, которая склеила
/// бы вообще всё, потеряв offscreen-уровни как таковые.
#[test]
#[ignore = "requires GPU adapter"]
fn culled_level_costs_no_pass_split() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    // 8 пустых opacity-групп вперемешку с полосами: цель кадра одна, значит и
    // пасс обязан быть один.
    let (p0, m0) = (r.plan_passes(), r.cull_merges());
    r.render_to_image(&opacity_groups_dl(8, |_| Vec::new()), 0.0, 0.0)
        .expect("кадр с выброшенными уровнями");
    assert_eq!(
        r.cull_merges() - m0,
        8,
        "разрезы пасса родителя вокруг выброшенных уровней не склеены",
    );
    assert_eq!(
        r.plan_passes() - p0,
        1,
        "кадр с одной целью обязан кодироваться одним пассом",
    );

    // Контроль: те же группы, но каждая рисует — уровни настоящие, пассы нужны.
    let (p0, m0) = (r.plan_passes(), r.cull_merges());
    r.render_to_image(
        &opacity_groups_dl(8, |i| {
            let y = 4.0 + i as f32 * 6.0;
            vec![DisplayCommand::FillRect {
                rect: Rect { x: 20.0, y, width: 20.0, height: 2.0 },
                color: Color { r: 0, g: 0, b: 255, a: 255 },
            }]
        }),
        0.0,
        0.0,
    )
    .expect("кадр с видимыми уровнями");
    assert_eq!(r.cull_merges() - m0, 0, "видимый уровень склеивать нельзя");
    assert_eq!(
        r.plan_passes() - p0,
        8 * 3,
        "видимый opacity-уровень обязан остаться тремя пассами (контроль негоден)",
    );
}

/// BUG-405 срез 5: склейка не теряет содержимое родителя.
///
/// Счётчик пассов сам по себе зелен и у правки, которая склеила батчи, потеряв
/// часть операций или `Clear` цели: проверяем пиксели по обе стороны от
/// выброшенного уровня и фон между ними.
#[test]
#[ignore = "requires GPU adapter"]
fn cull_merge_keeps_parent_content() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let dl = vec![
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 64.0, height: 8.0 },
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        },
        // Выброшенный уровень: содержимое целиком за поверхностью — его
        // операции обязаны уйти вместе с ним, а не попасть в пасс родителя.
        DisplayCommand::PushOpacity { alpha: 1.0, bounds: None },
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 400.0, width: 64.0, height: 8.0 },
            color: Color { r: 0, g: 255, b: 0, a: 255 },
        },
        DisplayCommand::PopOpacity,
        // Пустой выброшенный уровень между двумя операциями родителя.
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
        DisplayCommand::PopOpacity,
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 56.0, width: 64.0, height: 8.0 },
            color: Color { r: 0, g: 0, b: 255, a: 255 },
        },
    ];
    let m0 = r.cull_merges();
    let img = r.render_to_image(&dl, 0.0, 0.0).expect("render_to_image");
    assert_eq!(r.cull_merges() - m0, 2, "оба выброшенных уровня обязаны склеиться");

    let px = |x: usize, y: usize| {
        let o = (y * 64 + x) * 4;
        [img.data[o], img.data[o + 1], img.data[o + 2]]
    };
    assert_eq!(px(32, 4), [255, 0, 0], "полоса ДО выброшенного уровня потеряна");
    assert_eq!(px(32, 60), [0, 0, 255], "полоса ПОСЛЕ выброшенного уровня потеряна");
    // Между полосами — фон кадра (Clear цели выполнен ровно один раз).
    assert_eq!(px(32, 32), [255, 255, 255], "фон кадра не очищен");
}

/// Список с разнообразными выброшенными уровнями: пустые, с содержимым за
/// поверхностью, вложенные в видимый уровень, со скруглённым клипом и
/// blend-режимом внутри — плюс обычное содержимое родителя между ними.
///
/// Единицы — CSS px поверхности 64×64; всё, что с `y >= 300`, за поверхностью,
/// то есть его уровень будет выброшен viewport-cull-ом.
fn mixed_culled_levels_dl() -> Vec<DisplayCommand> {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let green = Color { r: 0, g: 160, b: 0, a: 255 };
    let off = Rect { x: 0.0, y: 400.0, width: 64.0, height: 20.0 };
    let mut dl = vec![
        DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 64.0, height: 6.0 },
            color: red,
        },
        // Пустой уровень.
        DisplayCommand::PushOpacity { alpha: 0.4, bounds: None },
        DisplayCommand::PopOpacity,
        // Уровень с содержимым за поверхностью — его операции обязаны уйти
        // вместе с ним, а не попасть в пасс родителя.
        DisplayCommand::PushOpacity { alpha: 0.9, bounds: None },
        DisplayCommand::FillRect { rect: off, color: green },
        DisplayCommand::PopOpacity,
        // Скруглённый клип (шейдерный слот) внутри выброшенного уровня:
        // его `SetClip` тоже обязан уйти.
        DisplayCommand::PushOpacity { alpha: 0.7, bounds: None },
        DisplayCommand::PushClipRoundedRect { rect: off, radii: [4.0; 4] },
        DisplayCommand::FillRect { rect: off, color: green },
        DisplayCommand::PopClip,
        DisplayCommand::PopOpacity,
        DisplayCommand::DrawText {
            rect: Rect { x: 2.0, y: 8.0, width: 60.0, height: 12.0 },
            text: "Ag".to_string(),
            font_size: 11.0,
            color: blue,
            font_family: Vec::new(),
            font_weight: Default::default(),
            font_style: Default::default(),
            font_stretch: Default::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 8.0,
            highlight_name: None,
            text_orientation: None,
        },
        // Видимый уровень с выброшенным уровнем ВНУТРИ.
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
        DisplayCommand::FillRect {
            rect: Rect { x: 4.0, y: 24.0, width: 40.0, height: 8.0 },
            color: green,
        },
        DisplayCommand::PushBlendMode {
            mode: lumen_paint::BlendMode::Multiply,
            bounds: off,
        },
        DisplayCommand::FillRect { rect: off, color: red },
        DisplayCommand::PopBlendMode,
        DisplayCommand::FillRect {
            rect: Rect { x: 4.0, y: 34.0, width: 40.0, height: 8.0 },
            color: blue,
        },
        DisplayCommand::PopOpacity,
        // Видимый скруглённый клип после всех склеек — контур не должен
        // «поехать» из-за того, что батч родителя стал длиннее.
        DisplayCommand::PushClipRoundedRect {
            rect: Rect { x: 4.0, y: 44.0, width: 56.0, height: 16.0 },
            radii: [6.0; 4],
        },
        DisplayCommand::FillRect {
            rect: Rect { x: 4.0, y: 44.0, width: 56.0, height: 16.0 },
            color: red,
        },
        DisplayCommand::PopClip,
    ];
    // Хвост из пустых уровней вперемешку с полосами — основная масса на
    // прокрутке реального сайта.
    for i in 0..6 {
        dl.push(DisplayCommand::PushOpacity { alpha: 0.6, bounds: None });
        dl.push(DisplayCommand::FillRect { rect: off, color: green });
        dl.push(DisplayCommand::PopOpacity);
        dl.push(DisplayCommand::FillRect {
            rect: Rect { x: 2.0 + i as f32 * 10.0, y: 62.0, width: 8.0, height: 2.0 },
            color: blue,
        });
    }
    dl
}

/// BUG-405 срез 5: склейка не меняет ни одного пикселя.
///
/// Оба плеча снимаются в одном процессе (`set_cull_merge_enabled`) на одном и
/// том же списке, и картинки сверяются побайтово. Гейт «headless-скриншот» для
/// wgpu-правки был бы ложно-зелёным (`lumen --screenshot` ведёт в
/// tiny-skia-растеризатор, срез 4), а `render_to_image` — тот самый wgpu-путь.
///
/// Счётчики в тесте обязательны: без них тест зелен и на списке, в котором
/// склейке нечего склеивать.
#[test]
#[ignore = "requires GPU adapter"]
fn cull_merge_is_pixel_identical() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    let dl = mixed_culled_levels_dl();

    r.set_cull_merge_enabled(false);
    let (p0, m0) = (r.plan_passes(), r.cull_merges());
    let before = r.render_to_image(&dl, 0.0, 0.0).expect("плечо без склейки");
    let (passes_off, merges_off) = (r.plan_passes() - p0, r.cull_merges() - m0);

    r.set_cull_merge_enabled(true);
    let (p0, m0) = (r.plan_passes(), r.cull_merges());
    let after = r.render_to_image(&dl, 0.0, 0.0).expect("плечо со склейкой");
    let (passes_on, merges_on) = (r.plan_passes() - p0, r.cull_merges() - m0);

    assert_eq!(merges_off, 0, "плечо A/B без склейки склеивает");
    assert!(merges_on >= 8, "список не задействовал склейку: {merges_on} склеек");
    assert!(
        passes_on < passes_off,
        "пассов не стало меньше: {passes_off} → {passes_on}",
    );
    assert_eq!(
        before.data, after.data,
        "склейка изменила пиксели: пассов {passes_off} → {passes_on}",
    );
}

/// Список из `n` filter-групп: у каждой один залитый прямоугольник, фильтры —
/// `filters`. Единицы — CSS px поверхности 64×64.
fn filter_groups_dl(n: usize, filters: Vec<FilterFn>) -> Vec<DisplayCommand> {
    let mut dl = Vec::new();
    for i in 0..n {
        let y = 2.0 + i as f32 * 8.0;
        let rect = Rect { x: 8.0, y, width: 40.0, height: 5.0 };
        dl.push(DisplayCommand::PushFilter { filters: filters.clone(), bounds: Some(rect) });
        dl.push(DisplayCommand::FillRect { rect, color: Color { r: 0, g: 0, b: 255, a: 255 } });
        dl.push(DisplayCommand::PopFilter);
    }
    dl
}

/// BUG-405 срез 6: filter-группа с блюром стоит двух пассов, а не трёх.
///
/// Гейт стоит на счётчике механизма, а не на времени кадра: вертикальный
/// проход блюра идёт сразу в родителя вместе с цветовыми фильтрами.
///
/// Три контроля на ложно-зелёный:
/// * плечо `set_blur_merge_enabled(false)` обязано дать прежние три пасса —
///   иначе тест зелен и на рендере, который блюр вовсе не считает;
/// * фильтр БЕЗ блюра как стоил одного пасса, так и стоит;
/// * блюр вместе с цветовым фильтром — те же два пасса (склейка не должна
///   отключаться, когда в списке есть и то и другое).
#[test]
#[ignore = "requires GPU adapter"]
fn blur_filter_costs_two_passes() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    // `filter_groups_dl` кладёт РОВНО одну заливку внутрь блюра — то есть
    // сигнатуру внешней тени, которую срез 7 уводит с уровня в батч родителя.
    // Здесь предмет проверки — сам filter-путь, поэтому аналитика выключена.
    r.set_shadow_analytic_enabled(false);

    let blur = filter_groups_dl(4, vec![FilterFn::Blur(3.0)]);

    let p0 = r.filter_passes();
    r.render_to_image(&blur, 0.0, 0.0).expect("кадр с блюром");
    assert_eq!(r.filter_passes() - p0, 4 * 2, "блюр обязан стоить двух пассов");

    r.set_blur_merge_enabled(false);
    let p0 = r.filter_passes();
    r.render_to_image(&blur, 0.0, 0.0).expect("кадр с блюром без склейки");
    assert_eq!(
        r.filter_passes() - p0,
        4 * 3,
        "плечо без склейки обязано остаться трёхпассовым (контроль негоден)",
    );
    r.set_blur_merge_enabled(true);

    let p0 = r.filter_passes();
    r.render_to_image(&filter_groups_dl(4, vec![FilterFn::Grayscale(1.0)]), 0.0, 0.0)
        .expect("кадр с цветовым фильтром");
    assert_eq!(r.filter_passes() - p0, 4, "фильтр без блюра стоит одного пасса");

    let p0 = r.filter_passes();
    r.render_to_image(
        &filter_groups_dl(4, vec![FilterFn::Blur(3.0), FilterFn::Grayscale(1.0)]),
        0.0,
        0.0,
    )
    .expect("кадр с блюром и цветовым фильтром");
    assert_eq!(
        r.filter_passes() - p0,
        4 * 2,
        "блюр вместе с цветовым фильтром обязан остаться двухпассовым",
    );
}

/// BUG-405 срез 6: склейка даёт ту же картинку, что и трёхпассовый путь.
///
/// Оба плеча снимаются в одном процессе (`set_blur_merge_enabled`) на одном
/// списке. Побайтового равенства тут быть не может и не должно: прежний путь
/// клал результат вертикального прохода в 8-битный слой и только потом
/// композитил, слитый — блендит его в родителя без этой промежуточной
/// квантовки. Поэтому гейт — расхождение не больше одного младшего бита.
///
/// Проверка «картинка вообще не пустая» обязательна: сравнение двух
/// одинаково пустых кадров зелено и у правки, которая потеряла блюр целиком.
#[test]
#[ignore = "requires GPU adapter"]
fn blur_merge_matches_two_pass() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 64, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    // См. `blur_filter_costs_two_passes`: списки из одной заливки под блюром —
    // сигнатура тени, а предмет этого гейта — склейка среза 6.
    r.set_shadow_analytic_enabled(false);
    // Блюр сам по себе, блюр с цветовыми фильтрами и вложенный в opacity —
    // все три формы, в которых filter-группа доезжает до кадра.
    let mut dl = filter_groups_dl(3, vec![FilterFn::Blur(3.0)]);
    dl.extend(filter_groups_dl(2, vec![FilterFn::Blur(2.0), FilterFn::Sepia(1.0)]));
    dl.push(DisplayCommand::PushOpacity { alpha: 0.5, bounds: None });
    dl.extend(filter_groups_dl(2, vec![FilterFn::Blur(4.0)]));
    dl.push(DisplayCommand::PopOpacity);

    r.set_blur_merge_enabled(false);
    let p0 = r.filter_passes();
    let before = r.render_to_image(&dl, 0.0, 0.0).expect("плечо трёх пассов");
    let passes_off = r.filter_passes() - p0;

    r.set_blur_merge_enabled(true);
    let p0 = r.filter_passes();
    let after = r.render_to_image(&dl, 0.0, 0.0).expect("плечо склейки");
    let passes_on = r.filter_passes() - p0;

    assert!(passes_on < passes_off, "пассов не стало меньше: {passes_off} -> {passes_on}");

    let max_diff = before
        .data
        .iter()
        .zip(after.data.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max_diff <= 1,
        "склейка изменила картинку: расхождение {max_diff}/255 (пассов {passes_off} -> {passes_on})",
    );

    // Полутон блюра обязан быть в кадре: без него сравнивались бы два пустых
    // кадра. Ищем пиксель, который не белый фон и не чистая заливка.
    let blurred = after
        .data
        .chunks_exact(4)
        .any(|px| px[2] > 32 && px[2] < 223 && px[0] < 223);
    assert!(blurred, "в кадре нет размытых пикселей — гейт пикселей негоден");
}

/// Внешняя тень так, как её кладёт `emit_box_shadows`: `PushFilter [Blur(σ)]`,
/// одна заливка, `PopFilter`. `radii = None` — прямые углы (`FillRect`).
fn box_shadow_dl(rect: Rect, sigma: f32, radii: Option<[f32; 4]>) -> Vec<DisplayCommand> {
    let color = Color { r: 0, g: 0, b: 0, a: 80 };
    let fill = match radii {
        None => DisplayCommand::FillRect { rect, color },
        Some(r) => DisplayCommand::FillRoundedRect {
            rect,
            color,
            radii: lumen_paint::CornerRadii {
                tl: r[0], tr: r[1], br: r[2], bl: r[3],
                tl_y: r[0], tr_y: r[1], br_y: r[2], bl_y: r[3],
            },
        },
    };
    vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(sigma)], bounds: Some(rect) },
        fill,
        DisplayCommand::PopFilter,
    ]
}

/// BUG-405 срез 7: внешняя тень не стоит ни одного пасса.
///
/// Гейт стоит на механизме (`shadow_draws` + `filter_passes`), а не на времени
/// кадра: тень рисуется операцией внутри уже открытого батча родителя, поэтому
/// её прежние три пасса (контент уровня, блюр H, слитый блюр V + композит)
/// исчезают целиком.
///
/// Три контроля на ложно-зелёный:
/// * плечо `set_shadow_analytic_enabled(false)` обязано дать прежние два пасса
///   на тень (срез 6) — иначе тест зелен и на рендере, который тени не рисует;
/// * блюр над ДВУМЯ заливками — не тень: уровень обязан остаться, иначе
///   сигнатура проглатывает произвольные filter-группы и меняет им картинку;
/// * блюр рядом с цветовым фильтром — тоже не тень (по той же причине).
#[test]
#[ignore = "requires GPU adapter"]
fn box_shadow_costs_no_pass() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 128, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let mut shadows = Vec::new();
    for i in 0..4 {
        let rect = Rect { x: 20.0, y: 16.0 + i as f32 * 24.0, width: 60.0, height: 12.0 };
        shadows.extend(box_shadow_dl(rect, 3.0, Some([4.0; 4])));
    }

    let (p0, s0) = (r.filter_passes(), r.shadow_draws());
    r.render_to_image(&shadows, 0.0, 0.0).expect("кадр с тенями");
    assert_eq!(r.shadow_draws() - s0, 4, "тени не ушли на аналитический путь");
    assert_eq!(r.filter_passes() - p0, 0, "тень всё ещё открывает уровень");

    r.set_shadow_analytic_enabled(false);
    let (p0, s0) = (r.filter_passes(), r.shadow_draws());
    r.render_to_image(&shadows, 0.0, 0.0).expect("кадр с тенями без аналитики");
    assert_eq!(r.shadow_draws() - s0, 0, "плечо A/B рисует тени аналитически");
    assert_eq!(
        r.filter_passes() - p0,
        4 * 2,
        "плечо без аналитики обязано остаться двухпассовым (контроль негоден)",
    );
    r.set_shadow_analytic_enabled(true);

    // Блюр над двумя заливками — не тень.
    let rect = Rect { x: 20.0, y: 20.0, width: 60.0, height: 12.0 };
    let color = Color { r: 0, g: 0, b: 0, a: 80 };
    let two_fills = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(3.0)], bounds: Some(rect) },
        DisplayCommand::FillRect { rect, color },
        DisplayCommand::FillRect {
            rect: Rect { x: 20.0, y: 40.0, width: 60.0, height: 12.0 },
            color,
        },
        DisplayCommand::PopFilter,
    ];
    let (p0, s0) = (r.filter_passes(), r.shadow_draws());
    r.render_to_image(&two_fills, 0.0, 0.0).expect("кадр с блюром над двумя заливками");
    assert_eq!(r.shadow_draws() - s0, 0, "две заливки под блюром приняты за тень");
    assert_eq!(r.filter_passes() - p0, 2, "filter-группа обязана остаться на уровне");

    // Блюр вместе с цветовым фильтром — не тень.
    let blur_and_color = vec![
        DisplayCommand::PushFilter {
            filters: vec![FilterFn::Blur(3.0), FilterFn::Sepia(1.0)],
            bounds: Some(rect),
        },
        DisplayCommand::FillRect { rect, color },
        DisplayCommand::PopFilter,
    ];
    let (p0, s0) = (r.filter_passes(), r.shadow_draws());
    r.render_to_image(&blur_and_color, 0.0, 0.0).expect("кадр с блюром и цветовым фильтром");
    assert_eq!(r.shadow_draws() - s0, 0, "блюр с цветовым фильтром принят за тень");
    assert_eq!(r.filter_passes() - p0, 2, "filter-группа обязана остаться на уровне");
}

/// BUG-405 срез 7: аналитическая тень даёт ту же картинку, что и размытый
/// offscreen-уровень.
///
/// Оба плеча снимаются в одном процессе (`set_shadow_analytic_enabled`) на
/// одном списке: прямоугольная тень, скруглённая, с разными σ, под переносом и
/// внутри opacity-уровня. Побайтового равенства тут быть не может — прежний
/// путь берёт свёртку восьмибитного растра фигуры, новый считает тот же
/// интеграл аналитически, — поэтому гейт числовой, а порог назван измерением
/// (см. bugs/BUG-405-OPEN.md, срез 7).
///
/// Проверка «в кадре есть полутон тени» обязательна: сравнение двух пустых
/// кадров зелено и у правки, потерявшей тень целиком.
#[test]
#[ignore = "requires GPU adapter"]
fn analytic_shadow_matches_blurred_level() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 128, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let mut dl = box_shadow_dl(Rect { x: 20.0, y: 14.0, width: 50.0, height: 10.0 }, 3.0, None);
    dl.extend(box_shadow_dl(
        Rect { x: 20.0, y: 40.0, width: 50.0, height: 10.0 },
        3.0,
        Some([5.0; 4]),
    ));
    dl.extend(box_shadow_dl(
        Rect { x: 20.0, y: 66.0, width: 50.0, height: 12.0 },
        1.5,
        Some([6.0, 0.0, 6.0, 0.0]),
    ));
    // Под переносом и внутри видимого уровня — обе формы, в которых тень
    // доезжает до кадра на реальной странице.
    dl.push(DisplayCommand::PushTransform {
        matrix: lumen_layout::Mat4::translation_2d(4.0, 92.0),
    });
    dl.extend(box_shadow_dl(Rect { x: 16.0, y: 0.0, width: 50.0, height: 10.0 }, 4.0, Some([3.0; 4])));
    dl.push(DisplayCommand::PopTransform);
    dl.push(DisplayCommand::PushOpacity { alpha: 0.6, bounds: None });
    dl.extend(box_shadow_dl(
        Rect { x: 20.0, y: 110.0, width: 50.0, height: 8.0 },
        2.0,
        Some([4.0; 4]),
    ));
    dl.push(DisplayCommand::PopOpacity);
    // Тень, выезжающая за край поверхности, — то, что происходит на прокрутке.
    // Прежний путь размывает содержимое текстуры уровня и на краю повторяет
    // краевой тексель (clamp), аналитический считает фигуру целиком: случай
    // обязан быть в гейте, а не остаться необмеренным.
    dl.extend(box_shadow_dl(
        Rect { x: -20.0, y: 88.0, width: 50.0, height: 10.0 },
        4.0,
        Some([4.0; 4]),
    ));

    r.set_shadow_analytic_enabled(false);
    let p0 = r.filter_passes();
    let level = r.render_to_image(&dl, 0.0, 0.0).expect("плечо уровня");
    let passes_level = r.filter_passes() - p0;

    r.set_shadow_analytic_enabled(true);
    let (p0, s0) = (r.filter_passes(), r.shadow_draws());
    let analytic = r.render_to_image(&dl, 0.0, 0.0).expect("плечо аналитики");
    let (passes_analytic, draws) = (r.filter_passes() - p0, r.shadow_draws() - s0);

    assert_eq!(draws, 6, "не все тени списка ушли на аналитический путь");
    assert_eq!(passes_analytic, 0, "аналитическая тень всё ещё стоит пассов");
    assert!(passes_level > 0, "плечо уровня не рисует тень пассами (контроль негоден)");

    let max_diff = level
        .data
        .iter()
        .zip(analytic.data.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    // Порог назван измерением: на этом списке расхождение ровно 1/255 —
    // младший бит там, где прежний путь квантует размытый слой в восемь бит,
    // а аналитика блендит результат в родителя без промежуточного слоя.
    assert!(
        max_diff <= 1,
        "аналитическая тень разошлась с размытым уровнем: {max_diff}/255 \
         (пассов {passes_level} -> {passes_analytic})",
    );

    // Полутон тени обязан быть в кадре у ОБОИХ плеч.
    for (name, img) in [("уровень", &level), ("аналитика", &analytic)] {
        let has_penumbra = img
            .data
            .chunks_exact(4)
            .any(|px| px[0] > 32 && px[0] < 223);
        assert!(has_penumbra, "в кадре плеча «{name}» нет полутона тени — гейт негоден");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 срез 9 — покрытие SVG-супа считается один раз на форму
// ─────────────────────────────────────────────────────────────────────────────

/// Список из `n` обводок-«галок» со сдвигом `shift` по обеим осям.
///
/// Обводка (`DrawSvgStroke`) — команда, у которой покрытие кромки считает
/// CPU-растеризатор `coverage_quads`; `shift` нужен контролю на промах:
/// сдвинутая фигура — ДРУГОЙ суп, и кэш обязан её пересчитать.
fn svg_strokes_dl(n: usize, shift: f32) -> Vec<DisplayCommand> {
    (0..n)
        .map(|i| {
            let y = 6.0 + i as f32 * 9.0 + shift;
            DisplayCommand::DrawSvgStroke {
                contours: vec![vec![
                    [6.0 + shift, y],
                    [14.0 + shift, y + 6.0],
                    [30.0 + shift, y - 4.0],
                ]],
                color: Color { r: 0, g: 0, b: 0, a: 255 },
                params: lumen_paint::svg_path::StrokeParams {
                    half_width: 1.5,
                    ..Default::default()
                },
            }
        })
        .collect()
}

/// BUG-405 срез 9: повторно встреченный суп не растеризуется второй раз.
///
/// Гейт стоит на счётчике попаданий, а не на времени фазы `collect`:
/// «одна и та же форма считается один раз» — утверждение о механизме, оно не
/// зависит ни от железа, ни от содержимого страницы.
///
/// Два контроля на ложно-зелёный. Первый: СДВИНУТАЯ фигура обязана дать
/// промахи — кэш, отвечающий готовым на любой запрос, рисовал бы чужие
/// пиксели и был бы тут зелен. Второй: плечо A/B обязано перестать попадать —
/// иначе счётчик не различает два пути и гейт пуст.
#[test]
#[ignore = "requires GPU adapter"]
fn coverage_cache_serves_repeated_soups() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    // Срез 12 поставил перед кэшем покрытия кэш целых фигур, который на
    // повторном кадре до растеризации не доходит вовсе. Гейт среза 9 обязан
    // проверять СВОЙ путь, поэтому верхний кэш здесь выключен.
    r.set_svg_shape_cache_enabled(false);

    let dl = svg_strokes_dl(12, 0.0);
    let (h0, m0) = r.coverage_cache_stats();
    r.render_to_image(&dl, 0.0, 0.0).expect("первый кадр");
    let (h1, m1) = r.coverage_cache_stats();
    assert_eq!(h1 - h0, 0, "первый кадр не мог попасть в пустой кэш");
    assert_eq!(m1 - m0, 12, "не все обводки дошли до растеризации покрытия");

    r.render_to_image(&dl, 0.0, 0.0).expect("второй кадр");
    let (h2, m2) = r.coverage_cache_stats();
    assert_eq!(h2 - h1, 12, "повторный кадр всё ещё пересчитывает покрытие");
    assert_eq!(m2 - m1, 0, "повторный кадр растеризовал покрытие заново");

    // Контроль на ложно-зелёный №1: другая геометрия — другой суп.
    r.render_to_image(&svg_strokes_dl(12, 3.5), 0.0, 0.0).expect("кадр со сдвигом");
    let (h3, m3) = r.coverage_cache_stats();
    assert_eq!(m3 - m2, 12, "сдвинутая фигура взята из кэша — ключ не различает формы");
    assert_eq!(h3 - h2, 0, "сдвинутая фигура засчитана попаданием");

    // Контроль на ложно-зелёный №2: плечо A/B обязано вернуть прежний путь.
    r.set_coverage_cache_enabled(false);
    r.render_to_image(&dl, 0.0, 0.0).expect("кадр без кэша");
    let (h4, m4) = r.coverage_cache_stats();
    assert_eq!(h4 - h3, 0, "плечо без кэша всё ещё попадает (контроль негоден)");
    assert_eq!(m4 - m3, 0, "плечо без кэша обязано обходить кэш целиком");
    r.set_coverage_cache_enabled(true);
}

/// BUG-405 срез 9: кадр с попаданиями в кэш побитово равен кадру без кэша.
///
/// Мемоизация чистой функции обязана давать РОВНО те же вершины, что и
/// пересчёт, — здесь, в отличие от срезов 7 и 8, гейт не численный, а
/// побайтовый: ключ сравнивается побитово и хранится в абсолютных координатах,
/// поэтому округление кромки не может разойтись.
///
/// Сравниваются три кадра: плечо без кэша, кадр промахов и кадр попаданий.
/// Последний и есть предмет проверки — первые два вместе доказывают, что
/// расхождение искали бы там, где оно возможно.
///
/// Проверка «обводка вообще нарисована, и у неё есть полутон» обязательна:
/// три пустых кадра сравнивались бы зелено, а кадр с бинарной кромкой
/// означал бы, что покрытие не считалось вовсе.
#[test]
#[ignore = "requires GPU adapter"]
fn coverage_cache_matches_recompute() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    // Как и в соседнем тесте: кэш фигур среза 12 не дал бы кадру попаданий
    // дойти до кэша покрытия, и гейт среза 9 остался бы без предмета.
    r.set_svg_shape_cache_enabled(false);
    let dl = svg_strokes_dl(12, 0.0);

    r.set_coverage_cache_enabled(false);
    let plain = r.render_to_image(&dl, 0.0, 0.0).expect("плечо без кэша");

    r.set_coverage_cache_enabled(true);
    let (h0, m0) = r.coverage_cache_stats();
    let miss = r.render_to_image(&dl, 0.0, 0.0).expect("кадр промахов");
    let (h1, m1) = r.coverage_cache_stats();
    let hit = r.render_to_image(&dl, 0.0, 0.0).expect("кадр попаданий");
    let (h2, _) = r.coverage_cache_stats();

    assert_eq!((h1 - h0, m1 - m0), (0, 12), "кадр промахов оказался не тем, чем назван");
    assert_eq!(h2 - h1, 12, "кадр попаданий не попал в кэш — сравнивать нечего");

    assert_eq!(plain.data, miss.data, "кадр промахов разошёлся с плечом без кэша");
    assert_eq!(plain.data, hit.data, "кадр попаданий разошёлся с плечом без кэша");

    // Обводка нарисована, и её кромка сглажена — иначе сравнение пусто.
    for (name, img) in [("без кэша", &plain), ("промахи", &miss), ("попадания", &hit)] {
        let opaque = img.data.chunks_exact(4).filter(|px| px[3] == 255 && px[0] < 32).count();
        assert!(opaque > 0, "в кадре плеча «{name}» нет обводки — гейт негоден");
        let has_edge = img
            .data
            .chunks_exact(4)
            .any(|px| px[0] > 32 && px[0] < 223);
        assert!(has_edge, "в кадре плеча «{name}» нет полутона кромки — покрытие не считалось");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-405 срез 12 — вся фигура SVG считается один раз
// ─────────────────────────────────────────────────────────────────────────────

/// BUG-405 срез 12: повторно встреченная фигура не тесселируется второй раз.
///
/// Гейт стоит на счётчике попаданий, а не на времени фазы `collect`: «одна и
/// та же фигура считается один раз» — утверждение о механизме.
///
/// Три контроля на ложно-зелёный. Первый: СДВИНУТАЯ фигура обязана дать
/// промахи — иначе кэш отвечал бы готовым на любой запрос. Второй: плечо
/// отката обязано перестать попадать, иначе счётчик не различает два пути.
/// Третий: тот же список, но заливками, обязан дать свои промахи — ключ,
/// не различающий вид команды, вернул бы обводку вместо заливки.
#[test]
#[ignore = "requires GPU adapter"]
fn svg_shape_cache_serves_repeated_shapes() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);

    let dl = svg_strokes_dl(12, 0.0);
    let (h0, m0) = r.svg_shape_cache_stats();
    r.render_to_image(&dl, 0.0, 0.0).expect("первый кадр");
    let (h1, m1) = r.svg_shape_cache_stats();
    assert_eq!(h1 - h0, 0, "первый кадр не мог попасть в пустой кэш");
    assert_eq!(m1 - m0, 12, "не все обводки дошли до тесселяции");

    r.render_to_image(&dl, 0.0, 0.0).expect("второй кадр");
    let (h2, m2) = r.svg_shape_cache_stats();
    assert_eq!(h2 - h1, 12, "повторный кадр всё ещё считает фигуры заново");
    assert_eq!(m2 - m1, 0, "повторный кадр тесселировал фигуры заново");

    // Контроль №1: другая геометрия — другая фигура.
    r.render_to_image(&svg_strokes_dl(12, 3.5), 0.0, 0.0).expect("кадр со сдвигом");
    let (h3, m3) = r.svg_shape_cache_stats();
    assert_eq!(m3 - m2, 12, "сдвинутая фигура взята из кэша — ключ не различает формы");
    assert_eq!(h3 - h2, 0, "сдвинутая фигура засчитана попаданием");

    // Контроль №2: те же контуры под заливкой — другой вид команды.
    let fills: Vec<DisplayCommand> = svg_strokes_dl(12, 0.0)
        .into_iter()
        .map(|cmd| match cmd {
            DisplayCommand::DrawSvgStroke { contours, color, .. } => {
                DisplayCommand::DrawSvgFill { contours, color }
            }
            other => other,
        })
        .collect();
    r.render_to_image(&fills, 0.0, 0.0).expect("кадр заливок");
    let (h4, m4) = r.svg_shape_cache_stats();
    assert_eq!(m4 - m3, 12, "заливка взята из кэша обводки — ключ не различает вид команды");
    assert_eq!(h4 - h3, 0, "заливка засчитана попаданием");

    // Контроль №3: плечо отката обязано обходить кэш целиком.
    r.set_svg_shape_cache_enabled(false);
    r.render_to_image(&dl, 0.0, 0.0).expect("кадр без кэша");
    let (h5, m5) = r.svg_shape_cache_stats();
    assert_eq!(h5 - h4, 0, "плечо без кэша всё ещё попадает (контроль негоден)");
    assert_eq!(m5 - m4, 0, "плечо без кэша обязано обходить кэш целиком");
    r.set_svg_shape_cache_enabled(true);
}

/// BUG-405 срез 12: кадр с попаданиями побитово равен кадру без кэша.
///
/// Мемоизация чистой цепочки обязана давать РОВНО те же вершины, что пересчёт:
/// ключ хранится в абсолютных координатах и сравнивается побитово, поэтому
/// гейт побайтовый, а не численный.
///
/// Сравниваются три кадра: плечо отката, кадр промахов и кадр попаданий.
/// Последний и есть предмет проверки — первые два вместе показывают, что
/// расхождение искали там, где оно возможно. Проверка «обводка нарисована и
/// её кромка сглажена» обязательна: три пустых кадра сравнивались бы зелено.
#[test]
#[ignore = "requires GPU adapter"]
fn svg_shape_cache_matches_recompute() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    let dl = svg_strokes_dl(12, 0.0);

    r.set_svg_shape_cache_enabled(false);
    let plain = r.render_to_image(&dl, 0.0, 0.0).expect("плечо без кэша");

    r.set_svg_shape_cache_enabled(true);
    let (h0, m0) = r.svg_shape_cache_stats();
    let miss = r.render_to_image(&dl, 0.0, 0.0).expect("кадр промахов");
    let (h1, m1) = r.svg_shape_cache_stats();
    let hit = r.render_to_image(&dl, 0.0, 0.0).expect("кадр попаданий");
    let (h2, _) = r.svg_shape_cache_stats();

    assert_eq!((h1 - h0, m1 - m0), (0, 12), "кадр промахов оказался не тем, чем назван");
    assert_eq!(h2 - h1, 12, "кадр попаданий не попал в кэш — сравнивать нечего");

    assert_eq!(plain.data, miss.data, "кадр промахов разошёлся с плечом без кэша");
    assert_eq!(plain.data, hit.data, "кадр попаданий разошёлся с плечом без кэша");

    for (name, img) in [("без кэша", &plain), ("промахи", &miss), ("попадания", &hit)] {
        let opaque = img.data.chunks_exact(4).filter(|px| px[3] == 255 && px[0] < 32).count();
        assert!(opaque > 0, "в кадре плеча «{name}» нет обводки — гейт негоден");
        let has_edge = img.data.chunks_exact(4).any(|px| px[0] > 32 && px[0] < 223);
        assert!(has_edge, "в кадре плеча «{name}» нет полутона кромки — покрытие не считалось");
    }
}

/// Список из `n` непересекающихся заливок одного цвета — то есть `n` подряд
/// идущих операций рисования с ОДНИМ состоянием пасса. Единицы — CSS px
/// поверхности 64×128.
fn same_state_fills_dl(n: usize) -> Vec<DisplayCommand> {
    (0..n)
        .map(|i| DisplayCommand::FillRect {
            rect: Rect { x: 4.0, y: 2.0 + i as f32 * 9.0, width: 40.0, height: 5.0 },
            color: Color { r: 0, g: 0, b: 255, a: 255 },
        })
        .collect()
}

/// Тот же список, но каждая вторая заливка — скруглённая: пайплайн меняется
/// на каждой операции, склеивать нечего.
fn alternating_kind_fills_dl(n: usize) -> Vec<DisplayCommand> {
    (0..n)
        .map(|i| {
            let rect = Rect { x: 4.0, y: 2.0 + i as f32 * 9.0, width: 40.0, height: 5.0 };
            let color = Color { r: 0, g: 0, b: 255, a: 255 };
            if i % 2 == 0 {
                DisplayCommand::FillRect { rect, color }
            } else {
                DisplayCommand::FillRoundedRect {
                    rect,
                    color,
                    radii: lumen_paint::CornerRadii {
                        tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0,
                        tl_y: 2.0, tr_y: 2.0, br_y: 2.0, bl_y: 2.0,
                    },
                }
            }
        })
        .collect()
}

/// BUG-405 срез 10: повторная команда состояния не отправляется, а соседние
/// диапазоны одного состояния сливаются в один `draw`.
///
/// Гейт стоит на счётчиках, а не на времени кадра: цена пасса платится в
/// `drop(pass)`, где `wgpu-core` проигрывает его команды в командный список,
/// поэтому «команд меньше» — утверждение о механизме, не зависящее от железа.
///
/// Два контроля на ложно-зелёный. Первый: плечо `set_state_elision_enabled`
/// обязано не отсеять и не склеить НИЧЕГО — иначе счётчики не различают два
/// пути и гейт пуст. Второй: на списке, где вид заливки чередуется, склеек
/// обязано быть ноль — счётчик, растущий всегда, был бы тут зелен.
#[test]
#[ignore = "requires GPU adapter"]
fn state_elision_skips_repeated_commands() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    r.set_font_provider(None);
    let dl = same_state_fills_dl(12);

    r.set_state_elision_enabled(false);
    let (e0, m0) = (r.state_elisions(), r.draw_merges());
    r.render_to_image(&dl, 0.0, 0.0).expect("плечо без отсева");
    let (e_off, m_off) = (r.state_elisions() - e0, r.draw_merges() - m0);
    assert_eq!(
        (e_off, m_off),
        (0, 0),
        "плечо A/B без отсева всё равно отсеивает (контроль негоден)",
    );

    r.set_state_elision_enabled(true);
    let (e0, m0) = (r.state_elisions(), r.draw_merges());
    r.render_to_image(&dl, 0.0, 0.0).expect("плечо с отсевом");
    let (e_on, m_on) = (r.state_elisions() - e0, r.draw_merges() - m0);
    assert_eq!(m_on, 11, "12 заливок одного состояния обязаны стать одним draw");
    assert!(e_on >= 33, "команды состояния не отсеяны: {e_on}");

    // Контроль на ложно-зелёный №2: вид заливки чередуется — склеивать нечего.
    let (e0, m0) = (r.state_elisions(), r.draw_merges());
    r.render_to_image(&alternating_kind_fills_dl(12), 0.0, 0.0)
        .expect("кадр с чередованием");
    let (e_alt, m_alt) = (r.state_elisions() - e0, r.draw_merges() - m0);
    assert_eq!(m_alt, 0, "склеены операции с разным пайплайном");
    assert!(e_alt > 0, "у чередования не отсеялась ни одна bind-группа: {e_alt}");
}

/// BUG-405 срез 10: отсев и склейка не меняют ни одного пикселя.
///
/// Оба плеча снимаются в одном процессе (`set_state_elision_enabled`) на одном
/// и том же списке и сверяются ПОБАЙТОВО: `draw(a..c)` подаёт те же примитивы
/// в том же порядке, что `draw(a..b)` + `draw(b..c)`, — численный порог здесь
/// скрывал бы дефект, а не допускал округление.
///
/// Список намеренно смешанный: заливки (склеиваются), скруглённые заливки и
/// текст (меняют пайплайн и bind-группу 1 — то место, где отсев может
/// оставить в пассе чужую группу).
#[test]
#[ignore = "requires GPU adapter"]
fn state_elision_is_pixel_identical() {
    let mut r = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer");
    let mut dl = same_state_fills_dl(6);
    dl.extend(alternating_kind_fills_dl(6));
    dl.push(DisplayCommand::DrawText {
        rect: Rect { x: 4.0, y: 112.0, width: 56.0, height: 14.0 },
        text: "Lumen".to_string(),
        font_size: 12.0,
        color: Color { r: 0, g: 0, b: 0, a: 255 },
        font_family: Vec::new(),
        font_weight: Default::default(),
        font_style: Default::default(),
        font_stretch: Default::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 8.0,
        highlight_name: None,
        text_orientation: None,
    });

    r.set_state_elision_enabled(false);
    let (e0, m0) = (r.state_elisions(), r.draw_merges());
    let plain = r.render_to_image(&dl, 0.0, 0.0).expect("плечо без отсева");
    assert_eq!(
        (r.state_elisions() - e0, r.draw_merges() - m0),
        (0, 0),
        "плечо A/B без отсева всё равно отсеивает (контроль негоден)",
    );

    r.set_state_elision_enabled(true);
    let (e0, m0) = (r.state_elisions(), r.draw_merges());
    let elided = r.render_to_image(&dl, 0.0, 0.0).expect("плечо с отсевом");
    let (e_on, m_on) = (r.state_elisions() - e0, r.draw_merges() - m0);
    assert!(m_on > 0, "список не задействовал склейку — сравнивать нечего");
    assert!(e_on > 0, "список не задействовал отсев — сравнивать нечего");

    assert_eq!(
        plain.data, elided.data,
        "отсев изменил пиксели: отсеяно {e_on} команд, склеено {m_on} draw",
    );

    // В кадре есть и заливка, и текст — иначе побайтовое равенство пусто.
    let blue = plain.data.chunks_exact(4).filter(|px| px[2] > 200 && px[0] < 64).count();
    assert!(blue > 0, "в кадре нет заливок — гейт негоден");
    let dark = plain.data.chunks_exact(4).filter(|px| px[0] < 64 && px[2] < 64).count();
    assert!(dark > 0, "в кадре нет текста — гейт негоден");
}

/// Список из одной строки текста — глифы попадают в атлас, поэтому первый
/// кадр с новой строкой делает заливку атласа, а повторный — нет.
fn text_dl(text: &str, y: f32) -> Vec<DisplayCommand> {
    vec![DisplayCommand::DrawText {
        rect: Rect { x: 2.0, y, width: 60.0, height: 14.0 },
        text: text.to_string(),
        font_size: 12.0,
        color: Color { r: 0, g: 0, b: 0, a: 255 },
        font_family: Vec::new(),
        font_weight: Default::default(),
        font_style: Default::default(),
        font_stretch: Default::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 8.0,
        highlight_name: None,
        text_orientation: None,
    }]
}

/// BUG-405 срез 11: в GPU уходят только изменившиеся строки атласа.
///
/// Плечи — два рендера в одном процессе: у каждого свой атлас, и оба видят
/// одну и ту же последовательность кадров, поэтому их атласы наполняются
/// одинаково. Одним рендером этот путь не снять: после первого кадра атлас
/// чист, и второе плечо не заливало бы ничего — гейт был бы пуст.
///
/// Проверяются и байты (предмет правки), и пиксели ВТОРОГО кадра: первый у
/// обоих плеч заливает свежий атлас целиком.
#[test]
#[ignore = "requires GPU adapter"]
fn atlas_partial_upload_sends_only_changed_rows() {
    const FULL: u64 = 1024 * 1024; // ATLAS_DIM × ATLAS_DIM, R8
    let mut whole = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer (плечо целой текстуры)");
    let mut rows = Renderer::new_headless(INTER.to_vec(), 64, 128, ColorSpace::Srgb)
        .expect("headless renderer (плечо строк)");
    whole.set_atlas_partial_upload_enabled(false);
    rows.set_atlas_partial_upload_enabled(true);

    let first = text_dl("Lumen", 20.0);
    let second = text_dl("Lumen ABCDEFGH", 20.0); // новые глифы → новые строки

    let mut img = [None, None];
    let mut bytes = [0u64; 2];
    let mut uploads = [0u64; 2];
    for (i, r) in [&mut whole, &mut rows].into_iter().enumerate() {
        r.render_to_image(&first, 0.0, 0.0).expect("первый кадр");
        let b0 = r.atlas_bytes_uploaded();
        assert_eq!(b0, FULL, "первый кадр обязан залить свежий атлас целиком: {b0}");
        let u0 = r.atlas_uploads();
        img[i] = Some(r.render_to_image(&second, 0.0, 0.0).expect("второй кадр"));
        bytes[i] = r.atlas_bytes_uploaded() - b0;
        uploads[i] = r.atlas_uploads() - u0;
    }

    assert_eq!(uploads, [1, 1], "плечи сделали разное число заливок: {uploads:?}");
    assert_eq!(bytes[0], FULL, "плечо целой текстуры залило не всю текстуру: {}", bytes[0]);
    assert!(
        bytes[1] > 0 && bytes[1] < FULL / 8,
        "построчная заливка не сузилась: {} байт из {FULL}",
        bytes[1],
    );

    let (a, b) = (img[0].take().expect("кадр"), img[1].take().expect("кадр"));
    assert_eq!(a.data, b.data, "построчная заливка изменила пиксели");
    let dark = a.data.chunks_exact(4).filter(|px| px[0] < 64 && px[2] < 64).count();
    assert!(dark > 0, "во втором кадре нет текста — гейт негоден");
}
