//! `WgpuBackend` — обёртка над [`Renderer`] реализующая [`RenderBackend`].
//!
//! Транслирует [`wgpu::SurfaceError`] и [`ImageRegisterError`] в [`RenderError`],
//! предоставляя единый контракт трейта независимо от wgpu-версии.
//!
//! Phase 1: текущий бэкенд по умолчанию.
//! Phase 2: будет вытеснен `FemtovgBackend` (ADR-010).

use std::sync::Arc;

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;
use winit::window::Window;

use crate::backend::{RenderBackend, RenderError};
use crate::renderer::Renderer;
use crate::DisplayCommand;

// ─── Конвертация ошибок ───────────────────────────────────────────────────────

/// Преобразует [`wgpu::SurfaceError`] в [`RenderError`].
///
/// `Lost`/`Outdated`/`Timeout` — оба означают, что поверхность нужно
/// пересоздать; shell вызовет [`RenderBackend::resize`] и повторит кадр.
/// `OutOfMemory` / `Other` — фатальные; shell переключится на fallback.
fn surface_error_to_render_error(e: wgpu::SurfaceError) -> RenderError {
    match e {
        wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Timeout => {
            RenderError::SurfaceLost
        }
        wgpu::SurfaceError::OutOfMemory => {
            RenderError::DeviceLost("wgpu: out of memory".into())
        }
        wgpu::SurfaceError::Other => {
            RenderError::Other("wgpu: unknown surface error".into())
        }
    }
}

// ─── WgpuBackend ─────────────────────────────────────────────────────────────

/// wgpu-бэкенд: тонкая обёртка над [`Renderer`], реализующая [`RenderBackend`].
///
/// Единственная задача — трансляция ошибок. Вся GPU-логика остаётся в
/// [`Renderer`]; `WgpuBackend` не добавляет состояния.
///
/// `screenshot_rgba()` возвращает `None` — windowed режим не поддерживает
/// readback. Для headless-рендера используйте [`Renderer::render_to_image`]
/// напрямую через [`WgpuBackend::renderer_mut`].
pub struct WgpuBackend {
    /// Внутренний wgpu-растеризатор.
    renderer: Renderer,
}

impl WgpuBackend {
    /// Создаёт оконный бэкенд из winit-окна.
    ///
    /// # Errors
    /// Возвращает `Err` если GPU-адаптер недоступен или инициализация шейдеров
    /// завершилась ошибкой.
    pub fn new(
        window: Arc<Window>,
        font_bytes: Vec<u8>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { renderer: Renderer::new(window, font_bytes)? })
    }

    /// Создаёт headless-бэкенд для тестов и `--print-to-pdf`.
    ///
    /// # Errors
    /// Возвращает `Err` если GPU-адаптер недоступен.
    pub fn new_headless(
        font_bytes: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { renderer: Renderer::new_headless(font_bytes, width, height)? })
    }

    /// Неизменяемый доступ к внутреннему [`Renderer`].
    ///
    /// Используется для операций, не охватываемых трейтом
    /// (например, `render_to_image`, `render_print_pages`).
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Изменяемый доступ к внутреннему [`Renderer`].
    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }
}

impl RenderBackend for WgpuBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        self.renderer
            .render(content, overlay, scroll_y, scroll_x)
            .map_err(surface_error_to_render_error)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    fn set_scale_factor(&mut self, scale: f64) {
        self.renderer.set_scale_factor(scale);
    }

    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String> {
        self.renderer
            .register_image(src, image)
            .map_err(|e| e.to_string())
    }

    fn clear_images(&mut self) {
        self.renderer.clear_images();
    }

    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.renderer.set_font_provider(provider);
    }

    fn viewport_size(&self) -> Size {
        let s = self.renderer.viewport_size();
        Size { width: s.width as f32, height: s.height as f32 }
    }

    fn scale_factor(&self) -> f64 {
        self.renderer.scale_factor()
    }

    fn preload_curated_fallbacks(&mut self) {
        self.renderer.preload_curated_fallbacks();
    }

    fn on_layer_memory_pressure(&mut self, level: MemoryPressureLevel) {
        self.renderer.layer_cache_mut().on_memory_pressure(level);
    }

    fn promote_layer(&mut self, node_id: u32, width: u32, height: u32) {
        self.renderer.promote_layer(node_id, width, height);
    }

    fn is_layer_promoted(&self, node_id: u32) -> bool {
        self.renderer.is_layer_promoted(node_id)
    }

    fn demote_layer(&mut self, node_id: u32) {
        self.renderer.demote_layer(node_id);
    }

    // screenshot_rgba() → None (windowed; default impl)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::RenderError;

    // Тесты без GPU: проверяем только конвертацию ошибок и Send-совместимость.

    #[test]
    fn surface_lost_maps_to_render_error_surface_lost() {
        assert_eq!(
            surface_error_to_render_error(wgpu::SurfaceError::Lost),
            RenderError::SurfaceLost,
        );
    }

    #[test]
    fn surface_outdated_maps_to_render_error_surface_lost() {
        assert_eq!(
            surface_error_to_render_error(wgpu::SurfaceError::Outdated),
            RenderError::SurfaceLost,
        );
    }

    #[test]
    fn surface_timeout_maps_to_render_error_surface_lost() {
        assert_eq!(
            surface_error_to_render_error(wgpu::SurfaceError::Timeout),
            RenderError::SurfaceLost,
        );
    }

    #[test]
    fn surface_oom_maps_to_device_lost() {
        match surface_error_to_render_error(wgpu::SurfaceError::OutOfMemory) {
            RenderError::DeviceLost(_) => {}
            other => panic!("ожидали DeviceLost, получили {other:?}"),
        }
    }

    #[test]
    fn surface_other_maps_to_render_error_other() {
        match surface_error_to_render_error(wgpu::SurfaceError::Other) {
            RenderError::Other(_) => {}
            other => panic!("ожидали Other, получили {other:?}"),
        }
    }

    /// `WgpuBackend: Send` — необходимо для передачи в `ThreadedCompositor`.
    #[test]
    fn wgpu_backend_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<WgpuBackend>();
    }
}
