//! `VelloBackend` — Vello рендер-бэкенд, реализующий [`RenderBackend`].
//!
//! Phase 3 бэкенд (ADR-010 §Migration path). Текущий статус — **заглушка**:
//! компилируется, логирует все вызовы, не рисует пиксели.
//!
//! Когда vello API стабилизируется (ожидается vello 1.0), здесь появится:
//! - `translate_to_scene(commands) -> vello::Scene` — трансляция DisplayCommand;
//! - `VelloBackend::render()` — сабмит Scene через wgpu.
//!
//! Согласно ADR-010: **все** импорты `vello::*` должны оставаться только в этом
//! файле. Слои выше (layout, shell, driver) не знают о vello.
//!
//! # Изоляция
//!
//! ```text
//! VelloBackend
//!   ├── (будущий) vello::Scene      ← только здесь
//!   ├── (будущий) wgpu Device/Queue ← surface layer для vello
//!   └── translate_to_scene()        ← только здесь
//! ```
//!
//! # RB-7 (текущее состояние)
//!
//! Заглушка без вызовов vello. Все методы логируют факт вызова и возвращают Ok.
//! `screenshot_rgba()` возвращает `Some(vec![])` — headless-совместимость для
//! будущих CompareBackend тестов (RB-8).

use std::sync::Arc;

use lumen_core::ext::FontProvider;
use lumen_core::geom::Size;
use lumen_image::Image;

use crate::backend::{RenderBackend, RenderError};
use crate::display_list::DisplayCommand;

// ─── VelloBackend ─────────────────────────────────────────────────────────────

/// Phase 3 рендер-бэкенд на базе Vello (ADR-010, RB-7 заглушка).
///
/// В Phase 3 использует `vello::Scene` + wgpu surface layer для рендеринга.
/// Текущая реализация — no-op: логирует вызовы, пиксели не рисует.
pub struct VelloBackend {
    /// Физическая ширина рендер-поверхности (px).
    width: u32,
    /// Физическая высота рендер-поверхности (px).
    height: u32,
    /// Device-pixel-ratio (HiDPI scale factor).
    scale: f64,
}

impl VelloBackend {
    /// Создаёт заглушку `VelloBackend` с начальным размером поверхности.
    ///
    /// В Phase 3 здесь будет инициализация wgpu device/queue/surface
    /// и `vello::Renderer` с конфигурацией AaConfig.
    pub fn new(width: u32, height: u32) -> Self {
        eprintln!("VelloBackend::new({width}×{height}) — stub, vello не подключён (RB-10)");
        Self { width, height, scale: 1.0 }
    }
}

impl RenderBackend for VelloBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        eprintln!(
            "VelloBackend::render(content={}, overlay={}, scroll=({scroll_x},{scroll_y})) — stub no-op",
            content.len(),
            overlay.len()
        );
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        eprintln!("VelloBackend::resize({width}×{height})");
        self.width = width;
        self.height = height;
    }

    fn set_scale_factor(&mut self, scale: f64) {
        eprintln!("VelloBackend::set_scale_factor({scale})");
        self.scale = scale;
    }

    fn register_image(&mut self, src: String, _image: &Image) -> Result<(), String> {
        eprintln!("VelloBackend::register_image({src:?}) — no-op");
        Ok(())
    }

    fn clear_images(&mut self) {
        eprintln!("VelloBackend::clear_images() — no-op");
    }

    fn set_font_provider(&mut self, _provider: Option<Arc<dyn FontProvider>>) {
        eprintln!("VelloBackend::set_font_provider() — no-op");
    }

    fn viewport_size(&self) -> Size {
        Size {
            width: self.width as f32 / self.scale as f32,
            height: self.height as f32 / self.scale as f32,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.scale
    }

    /// Заглушка возвращает пустой буфер, а не `None` — чтобы `CompareBackend`
    /// (RB-8) мог запрашивать скриншот и получать «чистое» изображение.
    fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
        let len = (self.width * self.height * 4) as usize;
        Some(vec![0u8; len])
    }
}

// VelloBackend будет thread-affine только при наличии реального wgpu Device.
// Текущая заглушка не содержит ни одного !Send поля, поэтому Send автоматичен.

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_backend() -> VelloBackend {
        VelloBackend::new(1024, 720)
    }

    #[test]
    fn new_sets_dimensions() {
        let b = make_backend();
        assert_eq!(b.width, 1024);
        assert_eq!(b.height, 720);
    }

    #[test]
    fn viewport_size_default_scale() {
        let b = make_backend();
        let sz = b.viewport_size();
        assert!((sz.width - 1024.0).abs() < 0.01);
        assert!((sz.height - 720.0).abs() < 0.01);
    }

    #[test]
    fn viewport_size_with_scale() {
        let mut b = make_backend();
        b.set_scale_factor(2.0);
        let sz = b.viewport_size();
        assert!((sz.width - 512.0).abs() < 0.01);
        assert!((sz.height - 360.0).abs() < 0.01);
    }

    #[test]
    fn scale_factor_default_is_one() {
        let b = make_backend();
        assert_eq!(b.scale_factor(), 1.0);
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut b = make_backend();
        b.resize(800, 600);
        assert_eq!(b.width, 800);
        assert_eq!(b.height, 600);
    }

    #[test]
    fn render_returns_ok() {
        let mut b = make_backend();
        let result = b.render(&[], &[], 0.0, 0.0);
        assert!(result.is_ok());
    }

    #[test]
    fn render_with_commands_returns_ok() {
        use lumen_layout::Color;
        use crate::display_list::DisplayCommand;
        use lumen_core::geom::Rect;

        let mut b = make_backend();
        let content = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        }];
        let overlay = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 },
            color: Color { r: 0, g: 255, b: 0, a: 255 },
        }];
        let result = b.render(&content, &overlay, 5.0, 3.0);
        assert!(result.is_ok());
    }

    #[test]
    fn register_image_ok() {
        use lumen_image::{Image, PixelFormat};

        let mut b = make_backend();
        let img = Image {
            width: 2,
            height: 2,
            format: PixelFormat::Rgba8,
            data: vec![0u8; 16],
            icc_profile: None,
        };
        assert!(b.register_image("test.png".into(), &img).is_ok());
    }

    #[test]
    fn screenshot_rgba_returns_correct_size() {
        let mut b = VelloBackend::new(4, 4);
        let pixels = b.screenshot_rgba().expect("должен возвращать Some");
        assert_eq!(pixels.len(), 4 * 4 * 4, "4×4 RGBA = 64 байта");
    }

    #[test]
    fn screenshot_rgba_after_resize() {
        let mut b = make_backend();
        b.resize(8, 8);
        let pixels = b.screenshot_rgba().expect("должен возвращать Some");
        assert_eq!(pixels.len(), 8 * 8 * 4, "8×8 RGBA = 256 байт");
    }

    #[test]
    fn screenshot_rgba_all_transparent() {
        let mut b = VelloBackend::new(2, 2);
        let pixels = b.screenshot_rgba().unwrap();
        assert!(pixels.iter().all(|&p| p == 0), "заглушка должна возвращать прозрачные пиксели");
    }

    #[test]
    fn is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<VelloBackend>();
    }

    #[test]
    fn as_render_backend_trait_object() {
        let mut b: Box<dyn RenderBackend> = Box::new(make_backend());
        assert!(b.render(&[], &[], 0.0, 0.0).is_ok());
        assert!(b.screenshot_rgba().is_some());
    }
}
