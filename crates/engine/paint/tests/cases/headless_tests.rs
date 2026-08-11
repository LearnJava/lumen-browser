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
        13,
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
    assert_eq!(r.pipelines_compiled(), 14, "прогрев собрал не тот набор пайплайнов");
    assert_eq!(r.warmed_pipeline_count(), 13);
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
