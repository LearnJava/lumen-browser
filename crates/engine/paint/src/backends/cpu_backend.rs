//! `CpuBackend` — headless CPU растеризатор (tiny-skia) реализующий [`RenderBackend`].
//!
//! Предназначен исключительно для тестирования и CI: рендерит без GPU,
//! детерминированно и одинаково на Windows/macOS/Linux.
//!
//! Доступен только с feature `backend-cpu`. Используется в [`CompareBackend`]
//! как «эталонный» headless-бэкенд для pixel-diff сравнения бэкендов.
//!
//! [`CompareBackend`]: super::compare_backend::CompareBackend

use std::sync::Arc;

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;

use crate::backend::{RenderBackend, RenderError};
use crate::DisplayCommand;

// ─── CpuBackend ──────────────────────────────────────────────────────────────

/// Headless CPU-бэкенд на tiny-skia: детерминированный рендер без GPU.
///
/// Реализует [`RenderBackend`] через [`crate::cpu_raster::rasterize_cpu`].
/// После каждого [`render`][RenderBackend::render] пиксели доступны через
/// [`screenshot_rgba`][RenderBackend::screenshot_rgba].
///
/// Используется [`CompareBackend`] и snapshot-тестами в `lumen-driver`.
///
/// [`CompareBackend`]: super::compare_backend::CompareBackend
pub struct CpuBackend {
    /// Ширина рендер-поверхности в физических пикселях.
    width: u32,
    /// Высота рендер-поверхности в физических пикселях.
    height: u32,
    /// RGBA8-пиксели последнего вызова [`render`][RenderBackend::render].
    last_pixels: Option<Vec<u8>>,
}

impl CpuBackend {
    /// Создаёт headless CPU-бэкенд с заданным размером поверхности.
    ///
    /// `width` и `height` — физические пиксели (без HiDPI scale).
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, last_pixels: None }
    }

    /// Возвращает Image из последнего рендера, если он был выполнен.
    pub fn last_image(&self) -> Option<Image> {
        self.last_pixels.as_ref().map(|pixels| Image {
            width: self.width,
            height: self.height,
            format: lumen_image::PixelFormat::Rgba8,
            data: pixels.clone(),
            icc_profile: None,
        })
    }
}

impl RenderBackend for CpuBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        // Объединяем content и overlay для CPU-растеризации.
        // overlay рисуется поверх content — это воспроизводит логику GPU-бэкендов.
        let mut commands = content.to_vec();
        commands.extend_from_slice(overlay);

        let image =
            crate::cpu_raster::rasterize_cpu(self.width, self.height, &commands, scroll_x, scroll_y)
                .map_err(|e| RenderError::Other(e.to_string()))?;

        self.last_pixels = Some(image.to_rgba8());
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        // Пиксели после resize инвалидны — очищаем.
        self.last_pixels = None;
    }

    fn set_scale_factor(&mut self, _scale: f64) {
        // CPU-бэкенд не использует HiDPI scale — рендерит в заданных физических пикселях.
    }

    fn register_image(&mut self, _src: String, _image: &Image) -> Result<(), String> {
        // CPU-бэкенд не кэширует изображения: DrawImage всегда рисует grey placeholder.
        // Это соответствует поведению InProcessSession::screenshot_cpu_rgba.
        Ok(())
    }

    fn clear_images(&mut self) {}

    fn set_font_provider(&mut self, _provider: Option<Arc<dyn FontProvider>>) {
        // CPU-бэкенд использует только bundled Inter — внешний провайдер игнорируется.
    }

    fn viewport_size(&self) -> Size {
        Size { width: self.width as f32, height: self.height as f32 }
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn on_layer_memory_pressure(&mut self, _level: MemoryPressureLevel) {
        // Нет GPU-кэшей — pressure-eviction не требуется.
    }

    fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
        self.last_pixels.clone()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;
    use lumen_layout::Color;

    fn red() -> Color {
        Color { r: 255, g: 0, b: 0, a: 255 }
    }

    #[test]
    fn cpu_backend_new_no_screenshot() {
        let mut b = CpuBackend::new(64, 64);
        assert!(b.screenshot_rgba().is_none(), "до render() пикселей нет");
    }

    #[test]
    fn cpu_backend_render_produces_pixels() {
        let mut b = CpuBackend::new(64, 64);
        let cmds = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 64.0, height: 64.0 },
            color: red(),
        }];
        b.render(&cmds, &[], 0.0, 0.0).expect("render OK");
        let px = b.screenshot_rgba().expect("пиксели есть после render");
        assert_eq!(px.len(), 64 * 64 * 4, "64×64 RGBA8");
    }

    #[test]
    fn cpu_backend_render_red_fill() {
        let mut b = CpuBackend::new(8, 8);
        let cmds = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
            color: red(),
        }];
        b.render(&cmds, &[], 0.0, 0.0).expect("render OK");
        let px = b.screenshot_rgba().unwrap();
        // Первый пиксель должен быть красным (R≥200, G<50, B<50, A=255)
        assert!(px[0] >= 200, "R канал должен быть высоким: {}", px[0]);
        assert!(px[1] < 50, "G канал должен быть низким: {}", px[1]);
        assert!(px[2] < 50, "B канал должен быть низким: {}", px[2]);
        assert_eq!(px[3], 255, "A должен быть 255");
    }

    #[test]
    fn cpu_backend_resize_clears_pixels() {
        let mut b = CpuBackend::new(32, 32);
        let cmds = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 32.0, height: 32.0 },
            color: red(),
        }];
        b.render(&cmds, &[], 0.0, 0.0).expect("render OK");
        assert!(b.screenshot_rgba().is_some());
        b.resize(64, 64);
        assert!(b.screenshot_rgba().is_none(), "после resize пиксели инвалидны");
    }

    #[test]
    fn cpu_backend_viewport_size() {
        let b = CpuBackend::new(1024, 720);
        let sz = b.viewport_size();
        assert_eq!(sz.width, 1024.0);
        assert_eq!(sz.height, 720.0);
    }

    #[test]
    fn cpu_backend_scale_factor_default() {
        let b = CpuBackend::new(64, 64);
        assert_eq!(b.scale_factor(), 1.0);
    }

    #[test]
    fn cpu_backend_overlay_composited() {
        // Overlay (белый прямоугольник поверх красного) должен перекрыть красный.
        let mut b = CpuBackend::new(8, 8);
        let content = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
            color: red(),
        }];
        let overlay = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
            color: Color::WHITE,
        }];
        b.render(&content, &overlay, 0.0, 0.0).expect("render OK");
        let px = b.screenshot_rgba().unwrap();
        // Первый пиксель должен быть белым (все каналы ≥200).
        assert!(px[0] >= 200, "R должен быть высоким (белый): {}", px[0]);
        assert!(px[1] >= 200, "G должен быть высоким (белый): {}", px[1]);
        assert!(px[2] >= 200, "B должен быть высоким (белый): {}", px[2]);
    }

    #[test]
    fn cpu_backend_last_image_dimensions() {
        let mut b = CpuBackend::new(16, 32);
        assert!(b.last_image().is_none());
        b.render(&[], &[], 0.0, 0.0).expect("render OK");
        let img = b.last_image().expect("image после render");
        assert_eq!(img.width, 16);
        assert_eq!(img.height, 32);
    }

    #[test]
    fn cpu_backend_register_image_ok() {
        let mut b = CpuBackend::new(8, 8);
        let img = Image {
            width: 1,
            height: 1,
            format: lumen_image::PixelFormat::Rgba8,
            data: vec![255, 0, 0, 255],
            icc_profile: None,
        };
        assert!(b.register_image("test.png".into(), &img).is_ok());
    }

    #[test]
    fn cpu_backend_memory_pressure_noop() {
        let mut b = CpuBackend::new(8, 8);
        // Не должен паниковать для всех уровней.
        b.on_layer_memory_pressure(MemoryPressureLevel::Low);
        b.on_layer_memory_pressure(MemoryPressureLevel::Medium);
        b.on_layer_memory_pressure(MemoryPressureLevel::High);
    }
}
