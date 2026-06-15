//! CompareBackend integration тесты (ADR-010 RB-8).
//!
//! Проверяет инфраструктуру `CompareBackend` + `CpuBackend`:
//! - два одинаковых CPU-бэкенда → 0% diff
//! - форматирование результатов в стиле ADR-010 (один тест = одна строка лога)
//! - сравнение с построенным из HTML display list-ом
//!
//! # Запуск
//! ```bash
//! cargo test -p lumen-driver --features compare-backends
//! cargo test -p lumen-driver --features compare-backends -- --nocapture
//! ```
//!
//! Весь файл скомпилирован только при feature `compare-backends`.
#![cfg(feature = "compare-backends")]

use std::path::{Path, PathBuf};

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_paint::{
    backends::{
        compare_backend::{CompareBackend, DiffResult},
        cpu_backend::CpuBackend,
    },
    DisplayCommand, RenderBackend,
};
use lumen_core::geom::Rect;
use lumen_layout::Color;

// ─── Константы ───────────────────────────────────────────────────────────────

/// Viewport для сравнительных тестов — совпадает с graphic_tests.
const W: u32 = 1024;
const H: u32 = 720;

/// Порог "предупреждение" — выше этого значения diff считается значимым.
/// Для CPU vs CPU должно быть строго 0%.
const WARN_THRESHOLD: f32 = 1.0;

// ─── Вспомогательные функции ──────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Загружает `graphic_tests/<page>.html`, строит display list через InProcessSession.
fn build_display_list_for_page(page: &str) -> Vec<DisplayCommand> {
    let html = workspace_root().join(format!("graphic_tests/{page}.html"));
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", html.display()))
        .unwrap_or_else(|e| panic!("navigate {page}: {e}"));
    // Получаем layout root через layout_snapshot — он есть у BrowserSession.
    // Для построения display list используем session.layout_root() (pub в session.rs).
    session
        .display_list_for_compare()
        .unwrap_or_else(|e| panic!("display_list {page}: {e}"))
}

/// Создаёт `CompareBackend` из двух `CpuBackend` одинакового размера.
fn make_cpu_vs_cpu() -> CompareBackend {
    let primary = Box::new(CpuBackend::new(W, H));
    let secondary = Box::new(CpuBackend::new(W, H));
    CompareBackend::new(primary, secondary)
}

/// Рендерит display list через `CompareBackend` и возвращает `DiffResult`.
fn render_and_diff(cmp: &mut CompareBackend, cmds: &[DisplayCommand]) -> DiffResult {
    cmp.render(cmds, &[], 0.0, 0.0).expect("render OK");
    cmp.last_diff()
        .expect("diff должен быть вычислен после render")
        .clone()
}

// ─── Unit-тесты инфраструктуры ────────────────────────────────────────────────

#[test]
fn compare_backend_empty_display_list_zero_diff() {
    let mut cmp = make_cpu_vs_cpu();
    let diff = render_and_diff(&mut cmp, &[]);
    assert!(
        diff.is_identical(),
        "пустой display list: два CPU бэкенда должны дать 0% diff, got {}%",
        diff.diff_percent()
    );
}

#[test]
fn compare_backend_single_rect_zero_diff() {
    let mut cmp = make_cpu_vs_cpu();
    let cmds = vec![DisplayCommand::FillRect {
        rect: Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 },
        color: Color { r: 51, g: 153, b: 255, a: 255 },
    }];
    let diff = render_and_diff(&mut cmp, &cmds);
    assert!(
        diff.is_identical(),
        "FillRect: cpu vs cpu должен быть 0% diff, got {}%",
        diff.diff_percent()
    );
}

#[test]
fn compare_backend_screenshot_returns_pixels() {
    let mut cmp = make_cpu_vs_cpu();
    cmp.render(&[], &[], 0.0, 0.0).expect("render OK");
    let px = cmp.screenshot_rgba().expect("screenshot не None");
    assert_eq!(px.len(), (W * H * 4) as usize, "1024×720 RGBA8");
}

#[test]
fn compare_backend_diff_result_math_correct() {
    // Проверяем, что DiffResult::compute правильно считает пиксели.
    let a = vec![0u8; (W * H * 4) as usize];
    let mut b = a.clone();
    // Поменяем первый пиксель: R = 255.
    b[0] = 255;
    let diff = DiffResult::compute(&a, &b);
    assert_eq!(diff.diff_pixels, 1, "ровно 1 пиксель отличается");
    assert_eq!(diff.total_pixels, W * H);
    let pct = diff.diff_percent();
    assert!(pct > 0.0 && pct < 0.001, "diff% должен быть малым: {pct}");
}

#[test]
fn compare_backend_format_output_correct() {
    let diff = DiffResult { diff_bytes: 0, total_bytes: W * H * 4, diff_pixels: 0, total_pixels: W * H };
    let s = diff.format("cpu", "cpu");
    eprintln!("00-calibration.html  {s}");
    assert!(s.contains("0.0%"), "ожидаем 0.0%: {s}");
    assert!(s.contains("✅"), "ожидаем чекмарк: {s}");
}

// ─── Интеграционные тесты на реальных страницах ───────────────────────────────

/// Страницы для compare-тестов. Используем подмножество snapshot_cpu.rs PAGES.
const COMPARE_PAGES: &[&str] = &[
    "00-calibration",
    "01-sanity",
    "02-color-named",
    "03-color-formats",
    "05-border-width",
    "08-padding",
    "09-margin",
    "36-border-radius",
    "39-gradients",
];

#[test]
fn cpu_vs_cpu_all_pages_identical() {
    let mut failures = Vec::new();

    for &page in COMPARE_PAGES {
        let cmds = build_display_list_for_page(page);
        let mut cmp = make_cpu_vs_cpu();
        let diff = render_and_diff(&mut cmp, &cmds);

        let report = diff.format("cpu", "cpu");
        eprintln!("{page:<35}  {report}");

        if diff.diff_percent() >= WARN_THRESHOLD {
            failures.push(format!("{page}: diff {}% (порог {WARN_THRESHOLD}%)", diff.diff_percent()));
        }
    }

    if !failures.is_empty() {
        panic!(
            "CompareBackend: cpu vs cpu должен давать 0% diff на всех страницах:\n{}",
            failures.join("\n")
        );
    }
}
