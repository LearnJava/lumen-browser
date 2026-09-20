//! BUG-936 — прямое сравнение wgpu-headless и CPU-растеризатора на РЕАЛЬНОМ
//! `graphic_tests/156-svg-mask.html`, без Edge и без живого окна.
//!
//! `cpu_raster.rs`'s `LayerComposite::MaskLayer` уже подтверждён корректным
//! (побайтовое совпадение с Edge-эталоном — см. BUG-936). Если wgpu-путь
//! (`MaskLayerComposite`, `renderer.rs`) действительно ломает маскирование на
//! РЕАЛЬНОЙ странице, а не только в синтетических пробах `headless_tests.rs`
//! (все прошли), diff между двумя рендерами того же самого display list-а
//! это покажет — независимо от возможности снять скриншот живого окна,
//! которая в этой среде не работает (gdigrab не находит калибровочный маркер).
//!
//! Требует GPU-адаптер — `#[ignore]` по умолчанию.
//! Запуск: `cargo test -p lumen-driver --test all -- --include-ignored bug936 --nocapture`

use std::path::{Path, PathBuf};

use lumen_driver::{BrowserSession, InProcessSession};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
#[ignore = "requires GPU adapter"]
fn wgpu_vs_cpu_svg_mask_page() {
    let html = workspace_root().join("graphic_tests/156-svg-mask.html");
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", html.display()))
        .unwrap_or_else(|e| panic!("navigate 156-svg-mask: {e}"));

    let display_list = session.display_list_for_compare().expect("display_list_for_compare");
    let cpu_img = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");

    let width = cpu_img.width;
    let height = cpu_img.height;

    let mut renderer = lumen_paint::Renderer::new_headless(
        include_bytes!("../../../../assets/fonts/Inter-Regular.ttf").to_vec(),
        width,
        height,
        lumen_core::ColorSpace::Srgb,
    )
    .expect("headless renderer");
    renderer.set_font_provider(None);
    let wgpu_img = renderer
        .render_to_image(&display_list, 0.0, 0.0)
        .expect("render_to_image (wgpu headless)");

    assert_eq!((wgpu_img.width, wgpu_img.height), (width, height), "размеры холстов должны совпасть");

    // Диф по пикселям + по строкам, чтобы локализовать расхождение (6 панелей
    // выстроены по вертикали/горизонтали в 156-svg-mask.html).
    let mut diff_pixels = 0usize;
    let mut first_row_diff: Option<usize> = None;
    let mut last_row_diff: Option<usize> = None;
    let mut min_x_diff = width as usize;
    let mut max_x_diff = 0usize;
    let mut samples: Vec<(usize, usize, [u8; 4], [u8; 4])> = Vec::new();
    for y in 0..height as usize {
        let mut row_diff = false;
        for x in 0..width as usize {
            let o = (y * width as usize + x) * 4;
            let a = &cpu_img.data[o..o + 4];
            let b = &wgpu_img.data[o..o + 4];
            // Небольшой допуск на AA/цветовое округление — интересует
            // структурное расхождение (маска не применена), не суб-пиксель.
            let d = a.iter().zip(b.iter()).map(|(x, y)| (*x as i32 - *y as i32).abs()).sum::<i32>();
            if d > 32 {
                diff_pixels += 1;
                row_diff = true;
                min_x_diff = min_x_diff.min(x);
                max_x_diff = max_x_diff.max(x);
                if samples.len() < 20 && diff_pixels.is_multiple_of(137) {
                    samples.push((
                        x, y,
                        [a[0], a[1], a[2], a[3]],
                        [b[0], b[1], b[2], b[3]],
                    ));
                }
            }
        }
        if row_diff {
            first_row_diff.get_or_insert(y);
            last_row_diff = Some(y);
        }
    }
    // Контроль на суб-пиксельный сдвиг: если расхождение — это UV/округление
    // квада композита на ±1px, а не отсутствие маскирования, то смещение
    // wgpu-кадра на верный сдвиг должно резко уронить diff%.
    for (ox, oy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
        let mut d = 0usize;
        let mut n = 0usize;
        for y in 0..height as usize {
            let sy = y as i32 + oy;
            if sy < 0 || sy >= height as i32 { continue; }
            for x in 0..width as usize {
                let sx = x as i32 + ox;
                if sx < 0 || sx >= width as i32 { continue; }
                let oa = (y * width as usize + x) * 4;
                let ob = (sy as usize * width as usize + sx as usize) * 4;
                let a = &cpu_img.data[oa..oa + 4];
                let b = &wgpu_img.data[ob..ob + 4];
                let diff = a.iter().zip(b.iter()).map(|(p, q)| (*p as i32 - *q as i32).abs()).sum::<i32>();
                if diff > 32 { d += 1; }
                n += 1;
            }
        }
        eprintln!("[bug936] сдвиг wgpu на ({ox},{oy}): {d}/{n} px ({:.2}%)", d as f64 / n as f64 * 100.0);
    }

    eprintln!("[bug936] диапазон столбцов расхождения: {min_x_diff}..{max_x_diff}");
    for (x, y, cpu_px, wgpu_px) in &samples {
        eprintln!("[bug936] ({x},{y}): cpu={cpu_px:?} wgpu={wgpu_px:?}");
    }
    let total = (width * height) as usize;
    let pct = diff_pixels as f64 / total as f64 * 100.0;
    eprintln!(
        "[bug936] wgpu vs cpu на 156-svg-mask.html: {diff_pixels}/{total} px ({pct:.2}%), \
         строки расхождения: {first_row_diff:?}..{last_row_diff:?}"
    );

    // Не гейт (порог гибкий) — цель пробы: НАЙТИ и залоггировать локализацию,
    // а не провалить сборку. Печатаем вывод независимо от результата.
    if pct > 0.5 {
        eprintln!(
            "[bug936] РАСХОЖДЕНИЕ ПОДТВЕРЖДЕНО между wgpu-headless и CPU-путём на реальной странице"
        );
    } else {
        eprintln!(
            "[bug936] wgpu-headless и CPU совпадают в пределах допуска — путь MaskLayerComposite \
             не воспроизводит исходный дефект на текущем коде"
        );
    }
}
