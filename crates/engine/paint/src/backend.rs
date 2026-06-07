//! `RenderBackend` trait — стабильный контракт между движком и GPU-бэкендами.
//!
//! Все рендер-бэкенды реализуют этот трейт; выше него (layout, shell, driver)
//! нет прямой зависимости от wgpu / femtovg / vello.
//!
//! # Бэкенды (определены в ADR-010)
//!
//! | Feature flag | Тип | Статус |
//! |---|---|---|
//! | `backend-wgpu` | `WgpuBackend` | Phase 1 (текущий) |
//! | `backend-femtovg` | `FemtovgBackend` | Phase 2 default |
//! | `backend-vello` | `VelloBackend` | Phase 3 default |
//! | `backend-cpu` | `CpuBackend` | CI / no-GPU |
//! | `compare` | `CompareBackend` | только тесты |
//!
//! # Использование
//!
//! Shell держит `Box<dyn RenderBackend>` и вызывает [`RenderBackend::render`]
//! на каждом кадре. Бэкенды не знают друг о друге и не импортируют типы
//! соседних бэкендов.

use std::fmt;
use std::sync::Arc;

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;

use crate::DisplayCommand;

// ─── Error ───────────────────────────────────────────────────────────────────

/// Ошибка рендера — возвращается из [`RenderBackend::render`].
///
/// Каждый вариант покрывает конкретный класс сбоев, чтобы shell мог
/// принять осмысленное решение о фallback или логировании.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    /// Surface потеряна (resize race, window minimize и т.д.).
    /// Shell должен вызвать [`RenderBackend::resize`] и повторить кадр.
    SurfaceLost,

    /// Драйвер вернул ошибку, после которой бэкенд не может продолжать работу.
    /// Shell должен попробовать fallback-бэкенд.
    DeviceLost(String),

    /// Ошибка компиляции шейдера (фатально — неисправимо).
    ShaderError(String),

    /// Любая другая ошибка с текстовым описанием.
    Other(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SurfaceLost => f.write_str("render surface lost"),
            Self::DeviceLost(msg) => write!(f, "device lost: {msg}"),
            Self::ShaderError(msg) => write!(f, "shader error: {msg}"),
            Self::Other(msg) => write!(f, "render error: {msg}"),
        }
    }
}

impl std::error::Error for RenderError {}

// ─── Trait ───────────────────────────────────────────────────────────────────

/// Стабильный интерфейс GPU-рендера для Lumen.
///
/// Принимает уже построенный [`DisplayCommand`]-список и превращает его
/// в пиксели. Не знает о layout, CSS или DOM — только о командах и изображениях.
///
/// `Send` обязателен: shell конструирует бэкенд в главном потоке, но может
/// передавать его в compositor-поток через [`ThreadedCompositor`].
///
/// [`ThreadedCompositor`]: crate::compositor::ThreadedCompositor
pub trait RenderBackend: Send {
    /// Рисует один кадр.
    ///
    /// - `content` — команды страницы (прокрученные на `scroll_x`/`scroll_y`).
    /// - `overlay` — команды поверх страницы (tab bar, панели, pop-up'ы).
    /// - `scroll_y` / `scroll_x` — текущий скролл страницы в CSS px.
    ///
    /// Возвращает [`RenderError::SurfaceLost`] при потере поверхности;
    /// shell должен вызвать [`resize`][RenderBackend::resize] и повторить.
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError>;

    /// Обновляет размер поверхности рендеринга (физические пиксели).
    ///
    /// Вызывается при изменении размера окна или изменении DPI.
    fn resize(&mut self, width: u32, height: u32);

    /// Устанавливает масштабный коэффициент (HiDPI).
    ///
    /// Используется для корректного отображения на экранах с высоким DPI.
    fn set_scale_factor(&mut self, scale: f64);

    /// Регистрирует изображение под ключом `src`.
    ///
    /// После вызова `DrawBackgroundImage` и `DrawImage` с этим `src`
    /// будут использовать переданное изображение.
    ///
    /// Возвращает `Err(msg)` если изображение не удалось загрузить в GPU.
    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String>;

    /// Сбрасывает все зарегистрированные изображения.
    ///
    /// Вызывается при навигации на новую страницу или при явной очистке кэша.
    fn clear_images(&mut self);

    /// Устанавливает провайдер шрифтов для растеризации глифов.
    ///
    /// `None` означает возврат к bundled Inter fallback.
    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>);

    /// Возвращает текущий размер viewport в **logical** (CSS) пикселях.
    ///
    /// Дефолт — 1024×720: hardcoded fallback до создания реального окна.
    /// Переопределяется в windowed-бэкендах; headless-бэкенды могут вернуть
    /// точный размер рендер-поверхности.
    fn viewport_size(&self) -> Size {
        Size { width: 1024.0, height: 720.0 }
    }

    /// Возвращает текущий device-pixel-ratio (HiDPI scale factor).
    ///
    /// Дефолт — 1.0. Windowed-бэкенды переопределяют через [`set_scale_factor`].
    ///
    /// [`set_scale_factor`]: RenderBackend::set_scale_factor
    fn scale_factor(&self) -> f64 {
        1.0
    }

    /// Предзагружает системные шрифты-fallback для Unicode-покрытия.
    ///
    /// Вызывается shell-ом один раз после создания бэкенда. Дефолт — no-op;
    /// wgpu-бэкенд переопределяет через [`Renderer::preload_curated_fallbacks`].
    ///
    /// [`Renderer::preload_curated_fallbacks`]: crate::renderer::Renderer::preload_curated_fallbacks
    fn preload_curated_fallbacks(&mut self) {}

    /// Реагирует на события memory-pressure — вытесняет layer-cache.
    ///
    /// Вызывается из poll-loop shell-а. Дефолт — no-op.
    fn on_layer_memory_pressure(&mut self, _level: MemoryPressureLevel) {}

    /// Promote a node to its own GPU layer for `will-change: transform/opacity/filter`.
    ///
    /// Default: no-op (backends that don't support GPU layers ignore this call).
    /// // CSS: will-change — P4 wires ComputedStyle.will_change to call this after relayout.
    fn promote_layer(&mut self, _node_id: u32, _width: u32, _height: u32) {}

    /// Returns `true` if the given node has a promoted GPU layer.
    ///
    /// Default: `false` (backends without GPU layer support always return false).
    fn is_layer_promoted(&self, _node_id: u32) -> bool {
        false
    }

    /// Remove the promoted GPU layer for a node.
    ///
    /// Default: no-op.
    fn demote_layer(&mut self, _node_id: u32) {}

    /// Возвращает сырые RGBA-пиксели после последнего [`render`][RenderBackend::render].
    ///
    /// Только headless-бэкенды реализуют это; windowed-бэкенды возвращают `None`.
    /// Используется в `lumen-driver` для snapshot-тестов.
    fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_error_display_surface_lost() {
        assert_eq!(RenderError::SurfaceLost.to_string(), "render surface lost");
    }

    #[test]
    fn render_error_display_device_lost() {
        let e = RenderError::DeviceLost("GPU removed".into());
        assert_eq!(e.to_string(), "device lost: GPU removed");
    }

    #[test]
    fn render_error_display_shader_error() {
        let e = RenderError::ShaderError("invalid WGSL".into());
        assert_eq!(e.to_string(), "shader error: invalid WGSL");
    }

    #[test]
    fn render_error_display_other() {
        let e = RenderError::Other("unknown".into());
        assert_eq!(e.to_string(), "render error: unknown");
    }

    #[test]
    fn render_error_clone_and_partial_eq() {
        let e1 = RenderError::SurfaceLost;
        let e2 = e1.clone();
        assert_eq!(e1, e2);
    }

    #[test]
    fn render_error_is_std_error() {
        // Проверяем что RenderError реализует std::error::Error
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&RenderError::SurfaceLost);
    }

    /// Нулевой бэкенд: реализует трейт с no-op методами для проверки
    /// что трейт объект-безопасен и может быть создан как Box<dyn RenderBackend>.
    struct NullBackend;

    impl RenderBackend for NullBackend {
        fn render(
            &mut self,
            _content: &[DisplayCommand],
            _overlay: &[DisplayCommand],
            _scroll_y: f32,
            _scroll_x: f32,
        ) -> Result<(), RenderError> {
            Ok(())
        }

        fn resize(&mut self, _width: u32, _height: u32) {}
        fn set_scale_factor(&mut self, _scale: f64) {}
        fn register_image(&mut self, _src: String, _image: &Image) -> Result<(), String> {
            Ok(())
        }
        fn clear_images(&mut self) {}
        fn set_font_provider(&mut self, _provider: Option<Arc<dyn FontProvider>>) {}
    }

    #[test]
    fn null_backend_is_object_safe() {
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        let result = b.render(&[], &[], 0.0, 0.0);
        assert!(result.is_ok());
    }

    #[test]
    fn null_backend_screenshot_rgba_returns_none() {
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        assert!(b.screenshot_rgba().is_none());
    }

    #[test]
    fn null_backend_register_image_ok() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgba8, data: vec![255; 4], icc_profile: None };
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        assert!(b.register_image("test".into(), &img).is_ok());
    }

    #[test]
    fn null_backend_viewport_size_default() {
        let b: Box<dyn RenderBackend> = Box::new(NullBackend);
        let sz = b.viewport_size();
        assert_eq!(sz.width, 1024.0, "default viewport width должен быть 1024");
        assert_eq!(sz.height, 720.0, "default viewport height должен быть 720");
    }

    #[test]
    fn null_backend_scale_factor_default() {
        let b: Box<dyn RenderBackend> = Box::new(NullBackend);
        assert_eq!(b.scale_factor(), 1.0, "default scale factor должен быть 1.0");
    }

    #[test]
    fn null_backend_preload_curated_fallbacks_noop() {
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        // Должен завершиться без паники
        b.preload_curated_fallbacks();
    }

    #[test]
    fn null_backend_on_layer_memory_pressure_noop() {
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        // Должен завершиться без паники для всех уровней
        b.on_layer_memory_pressure(MemoryPressureLevel::Low);
        b.on_layer_memory_pressure(MemoryPressureLevel::Medium);
        b.on_layer_memory_pressure(MemoryPressureLevel::High);
    }
}
