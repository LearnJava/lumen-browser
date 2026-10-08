//! PERF-10: режим прогона без растеризации (`--no-paint`).
//!
//! [`NoPaintBackend`] — [`RenderBackend`], который ничего не рисует и не трогает
//! GPU: `render` — no-op, изображений/снимков он не хранит. Размер и DPR он
//! только запоминает, потому что движок берёт из бэкенда размер вьюпорта
//! (`viewport_size`/`scale_factor`) для layout — `getBoundingClientRect`,
//! media queries и `innerWidth` продолжают работать как в обычном окне.
//!
//! Окно остаётся видимым, но не активируется (`on_resumed`,
//! `with_active(false)`): скрытое окно не получает `RedrawRequested`, а на нём
//! живут `load` картинок и часть rAF-работы. Режим убирает из WPT-прогона
//! testharness-категорий инициализацию wgpu/DX12, растеризацию и увод фокуса.
//! Всё, что требует пикселей (`screenshot_rgba`, reftest через
//! `--ipc-server`), этим режимом не обслуживается — он отдаёт `None`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lumen_core::ext::FontProvider;
use lumen_core::geom::Size;
use lumen_image::Image;
use lumen_paint::{DisplayCommand, RenderBackend, RenderError};

/// Процесс-глобальный флаг режима: ставится один раз в `main` из `--no-paint`
/// / `LUMEN_NO_PAINT=1`, читается в `on_resumed`. Атомик, а не `set_var`:
/// запись в окружение процесса небезопасна при живых потоках.
static NO_PAINT: AtomicBool = AtomicBool::new(false);

/// Включить/выключить режим «без растеризации» (`--no-paint`).
pub(crate) fn set_no_paint(enabled: bool) {
    NO_PAINT.store(enabled, Ordering::Relaxed);
}

/// Включён ли режим `--no-paint` в этом процессе.
pub(crate) fn no_paint_enabled() -> bool {
    NO_PAINT.load(Ordering::Relaxed)
}

/// Бэкенд-пустышка: хранит размер поверхности и DPR, не рисует.
pub(crate) struct NoPaintBackend {
    /// Ширина «поверхности» в физических пикселях.
    width: u32,
    /// Высота «поверхности» в физических пикселях.
    height: u32,
    /// HiDPI-коэффициент.
    scale: f64,
}

impl NoPaintBackend {
    /// Создаёт бэкенд с начальным размером окна (физические пиксели) и DPR.
    pub(crate) fn new(width: u32, height: u32, scale: f64) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            scale: if scale > 0.0 { scale } else { 1.0 },
        }
    }
}

impl RenderBackend for NoPaintBackend {
    fn render(
        &mut self,
        _content: &[DisplayCommand],
        _overlay: &[DisplayCommand],
        _scroll_y: f32,
        _scroll_x: f32,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    fn set_scale_factor(&mut self, scale: f64) {
        if scale > 0.0 {
            self.scale = scale;
        }
    }

    fn register_image(&mut self, _src: String, _image: Arc<Image>) -> Result<(), String> {
        Ok(())
    }

    fn clear_images(&mut self) {}

    fn set_font_provider(&mut self, _provider: Option<Arc<dyn FontProvider>>) {}

    fn viewport_size(&self) -> Size {
        Size {
            width: (f64::from(self.width) / self.scale) as f32,
            height: (f64::from(self.height) / self.scale) as f32,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.scale
    }

    // Фаст-пас page-offset нужен только чтобы shell не клонировал display list
    // на каждом кадре; здесь кадра нет, но `true` оставляет путь без клона.
    fn supports_page_offset(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_physical_over_scale() {
        let mut b = NoPaintBackend::new(2048, 1440, 2.0);
        let s = b.viewport_size();
        assert_eq!((s.width, s.height), (1024.0, 720.0));
        b.resize(1000, 500);
        b.set_scale_factor(1.0);
        let s = b.viewport_size();
        assert_eq!((s.width, s.height), (1000.0, 500.0));
    }

    #[test]
    fn render_is_noop_and_screenshot_absent() {
        let mut b: Box<dyn RenderBackend> = Box::new(NoPaintBackend::new(800, 600, 1.0));
        assert!(b.render(&[], &[], 0.0, 0.0).is_ok());
        assert!(b.screenshot_rgba().is_none());
    }

    #[test]
    fn degenerate_size_and_scale_are_clamped() {
        let b = NoPaintBackend::new(0, 0, 0.0);
        let s = b.viewport_size();
        assert_eq!((s.width, s.height), (1.0, 1.0));
        assert_eq!(b.scale_factor(), 1.0);
    }

    #[test]
    fn flag_roundtrip() {
        set_no_paint(true);
        assert!(no_paint_enabled());
        set_no_paint(false);
        assert!(!no_paint_enabled());
    }
}
