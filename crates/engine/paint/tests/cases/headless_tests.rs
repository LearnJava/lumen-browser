// Requires `backend-wgpu` feature (uses Renderer directly).
#![cfg(feature = "backend-wgpu")]
/// GPU headless render tests — require a real GPU adapter.
///
/// Marked `#[ignore]` by default so they don't run in CPU-only CI.
/// Run explicitly with:
///   cargo test -p lumen-paint --test headless_tests -- --include-ignored
use lumen_core::geom::Rect;
use lumen_core::ColorSpace;
use lumen_layout::Color;
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
    let rect = Rect { x: 4.0, y: 4.0, width: w - 8.0, height: h - 8.0 };
    vec![
        DisplayCommand::PushClipRoundedRect { rect: outer, radii: [8.0, 8.0, 8.0, 8.0] },
        DisplayCommand::PushClipRoundedRect { rect, radii: [8.0, 8.0, 8.0, 8.0] },
        DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
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
        12,
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
    // `mask-layer` — одна ячейка на два пайплайна (luminance/alpha).
    assert_eq!(r.pipelines_compiled(), 13, "прогрев собрал не тот набор пайплайнов");
    assert_eq!(r.warmed_pipeline_count(), 12);
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

    // Целевой случай: длинный план — больше одной подачи.
    let before = r.submissions();
    r.render_to_image(&many_clips_dl(64.0, 128.0, 12), 0.0, 0.0).expect("длинный кадр");
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
/// Вторая половина — контроль на ложно-зелёный: вложенный клип (в шейдере
/// контур один) обязан уровень открыть, иначе тест проходил бы и на рендере,
/// который скруглённые клипы вообще не обрабатывает.
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

    let before = r.rrect_clip_levels();
    r.render_to_image(&many_clips_dl(64.0, 128.0, 12), 0.0, 0.0).expect("кадр с вложенными");
    assert_eq!(
        r.rrect_clip_levels() - before,
        12,
        "вложенный клип обязан остаться на пути уровня (контроль негоден)",
    );
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
