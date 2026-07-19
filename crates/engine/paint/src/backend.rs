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
use lumen_layout::Color;

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
    /// TEMP BUG-272 diagnostics: human-readable summary of backend-owned
    /// memory (image caches, atlases). Default: empty string.
    fn debug_mem_report(&self) -> String {
        String::new()
    }

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

    /// Устанавливает превью-масштаб зума (ADR-016 M0.3).
    ///
    /// Позволяет мгновенно масштабировать уже отрисованный display-list
    /// вокруг верхнего-левого угла вьюпорта без полного relayout. Значение
    /// `scale` — это отношение нового `zoom_factor` к тому, при котором был
    /// свёрстан текущий display-list (`new_zoom / laid_out_zoom`). `1.0`
    /// означает «превью выключено», рендер идёт как обычно. Shell выставляет
    /// его на Ctrl+/-/0 и сбрасывает в `1.0` после дебаунс-relayout.
    ///
    /// Дефолт — no-op (бэкенды без поддержки превью игнорируют вызов и
    /// продолжают рисовать в масштабе 1:1).
    fn set_preview_scale(&mut self, _scale: f32) {}

    /// Устанавливает фиксированное смещение страницы в CSS px (ADR-016 M0.4).
    ///
    /// Это неизменный сдвиг контента, накладываемый **поверх** прокрутки: он
    /// опускает страницу ниже tab bar-а (`TAB_BAR_HEIGHT`) и сдвигает её вправо
    /// от левой docked-панели. Раньше shell оборачивал весь display-list в
    /// `PushTransform(translate(offset))` каждый кадр, копируя список целиком.
    /// Теперь смещение применяется рендер-стороной как дополнительная
    /// трансляция после scroll-трансляции — display-list рисуется по ссылке без
    /// per-frame клона (главный источник работы на горячем пути скролла).
    ///
    /// Порядок трансформаций сохраняется прежним: `scale(preview) ·
    /// translate(-scroll) · translate(offset)`, поэтому sticky-вычисления
    /// (использующие истинный `scroll`) и zoom-превью не меняются.
    ///
    /// Дефолт — no-op; бэкенды без поддержки продолжают ожидать смещение внутри
    /// самого display-list (см. [`supports_page_offset`]).
    ///
    /// [`supports_page_offset`]: RenderBackend::supports_page_offset
    fn set_page_offset(&mut self, _x: f32, _y: f32) {}

    /// Сообщает, применяет ли бэкенд смещение из [`set_page_offset`] сам.
    ///
    /// `true` — shell может рисовать display-list по ссылке (быстрый путь без
    /// per-frame клона). `false` (дефолт) — shell обязан по-прежнему оборачивать
    /// контент в `PushTransform(translate(offset))`, иначе страница нарисуется
    /// поверх tab bar-а. Femtovg (Phase 2 default) возвращает `true`.
    ///
    /// [`set_page_offset`]: RenderBackend::set_page_offset
    fn supports_page_offset(&self) -> bool {
        false
    }

    /// Как [`render`](Self::render), но с диапазонами анимируемых сегментов
    /// `content` (static/animated split скролл-композитора, EXPERIMENT.md §2:
    /// полоса кэшируется по статике, сегменты рисуются поверх каждым кадром).
    ///
    /// Дефолт игнорирует диапазоны — бэкенды без скролл-композитора рисуют
    /// монолитом, поведение не меняется.
    fn render_with_anim(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
        _anim_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), RenderError> {
        self.render(content, overlay, scroll_y, scroll_x)
    }

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
    /// Принимает `Arc<Image>` (не `&Image`): backend'ы, которым нужна CPU-копия
    /// декодированных пикселей после загрузки в GPU (femtovg's `raw_images` для
    /// area-averaged downscale, BUG-077), клонируют указатель, а не буфер, и
    /// разделяют аллокацию с вызывающей стороной (`IMAGE_CACHE` / CPU
    /// image-cache) вместо второго экземпляра каждой картинки (BUG-272 срез 17,
    /// тот же приём, что срез 6 применил к шрифтовым байтам).
    ///
    /// Возвращает `Err(msg)` если изображение не удалось загрузить в GPU.
    fn register_image(&mut self, src: String, image: Arc<Image>) -> Result<(), String>;

    /// Сбрасывает все зарегистрированные изображения.
    ///
    /// Вызывается при навигации на новую страницу или при явной очистке кэша.
    fn clear_images(&mut self);

    /// Регистрирует offscreen RGBA-снимок слоя под числовым `id`.
    ///
    /// После вызова [`DisplayCommand::DrawLayerSnapshot`] с тем же `id` рисует
    /// `image` в `rect` команды с её `alpha`. Используется движком View
    /// Transitions (CSS View Transitions L1 §4): shell захватывает старое и новое
    /// поддерево каждого `view-transition-name`-элемента в изображение и морфит
    /// его на протяжении перехода.
    ///
    /// `image` интерпретируется как [`PixelFormat::Rgba8`][lumen_image::PixelFormat]
    /// (иные форматы конвертируются). Возвращает `Err(msg)` при ошибке загрузки в
    /// GPU. Дефолт — no-op, возвращающий `Ok(())` (бэкенды без поддержки снимков
    /// игнорируют вызов).
    fn register_snapshot(&mut self, _id: u64, _image: &Image) -> Result<(), String> {
        Ok(())
    }

    /// Сбрасывает все зарегистрированные layer-снимки (см. [`register_snapshot`]).
    ///
    /// Вызывается shell-ом при завершении или отмене view-перехода, чтобы
    /// освободить per-element текстуры снимков. Дефолт — no-op.
    ///
    /// [`register_snapshot`]: RenderBackend::register_snapshot
    fn clear_snapshots(&mut self) {}

    /// Устанавливает провайдер шрифтов для растеризации глифов.
    ///
    /// `None` означает возврат к bundled Inter fallback.
    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>);

    /// Устанавливает фон канвы (CSS Backgrounds §3.11.1) — цвет, которым весь
    /// кадр заливается перед отрисовкой display-list.
    ///
    /// `Some(color)` — фон корневого элемента (распространённый с `<body>`/`<html>`),
    /// заливающий **всю** поверхность, чтобы фон страницы покрывал вьюпорт целиком,
    /// даже когда бокс корня меньше окна (фикс. 1024×720 страница в развёрнутом окне).
    /// `None` — UA-дефолт (белый). Дефолтная реализация — no-op (бэкенд продолжает
    /// чистить в белый). Shell вызывает перед каждым [`render`][RenderBackend::render]
    /// с результатом `lumen_layout::canvas_background_color`.
    fn set_canvas_background(&mut self, _color: Option<Color>) {}

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

    /// Реагирует на события memory-pressure — вытесняет glyph atlas.
    ///
    /// Medium: эвиктирует ~50% LRU глифов.  High: полная очистка.
    /// Вызывается из poll-loop shell-а вместе с `on_layer_memory_pressure`.
    /// Дефолт — no-op (бэкенды без GlyphAtlas игнорируют).
    fn on_atlas_memory_pressure(&mut self, _level: MemoryPressureLevel) {}

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

    /// Передаёт владение momentum-скроллом рендер-потоку (ADR-016 M1.3).
    ///
    /// UI-поток вызывает это при `TouchPhase::Ended` с ненулевой скоростью
    /// (`vel_y`/`vel_x` — CSS px/ms, `max_scroll_y`/`max_scroll_x` — экстенты
    /// для клампа). Пока UI-поток жив и шлёт кадры, они (latest-wins) ведут
    /// презентацию; но если UI-поток застопорился (долгий JS-тик, relayout),
    /// рендер-поток **сам** продолжает momentum на vsync из последнего
    /// закоммиченного кадра — презентация не замерзает.
    ///
    /// Дефолт — no-op: однопоточные бэкенды momentum-ом не владеют (его тикает
    /// сам shell в `RedrawRequested`), поэтому при выключенном рендер-потоке
    /// поведение не меняется.
    fn start_render_momentum(
        &mut self,
        _vel_y: f32,
        _vel_x: f32,
        _max_scroll_y: f32,
        _max_scroll_x: f32,
    ) {
    }

    /// Отменяет momentum-скролл, которым владеет рендер-поток (ADR-016 M1.3).
    ///
    /// Вызывается shell-ом при новом жесте, навигации или иной причине сбросить
    /// инерцию немедленно. Дефолт — no-op (см. [`start_render_momentum`]).
    ///
    /// [`start_render_momentum`]: RenderBackend::start_render_momentum
    fn stop_render_momentum(&mut self) {}

    /// Аннотирует следующий кадр в `LUMEN_FRAME_LOG` (ADR-016 M1).
    ///
    /// Рендер-поток вызывает это перед каждым [`render`](RenderBackend::render):
    /// `commit_id` — монотонный идентификатор коммита UI-потока, `self_tick` —
    /// `true`, когда кадр перерисован самим рендер-потоком при застопорившемся
    /// UI-потоке (momentum self-tick, M1.3). Аннотация попадает в строку
    /// `[frame] paint …` вместе с тегом потока, чтобы покадровый лог разных
    /// потоков не сливался в кашу (см. риск «Frame logs across threads» в плане)
    /// и было видно, что презентация продолжалась *во время* стойла.
    ///
    /// Дефолт — no-op: однопоточный путь (`LUMEN_RENDER_THREAD` выкл) рисует на
    /// UI-потоке, все кадры оттуда, аннотация не нужна.
    fn set_frame_commit_id(&mut self, _commit_id: u64, _self_tick: bool) {}
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
        fn register_image(&mut self, _src: String, _image: Arc<Image>) -> Result<(), String> {
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
        assert!(b.register_image("test".into(), Arc::new(img)).is_ok());
    }

    #[test]
    fn null_backend_register_snapshot_default_ok() {
        use lumen_image::{Image, PixelFormat};
        // Default trait impl accepts any snapshot and reports success; backends
        // without snapshot support silently ignore it (View Transitions L1).
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgba8, data: vec![255; 4], icc_profile: None };
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        assert!(b.register_snapshot(7, &img).is_ok());
        b.clear_snapshots(); // no-op, must not panic
    }

    #[test]
    fn null_backend_page_offset_default_unsupported() {
        // ADR-016 M0.4: бэкенд без поддержки page-offset сообщает об этом
        // (`false`), а `set_page_offset` — безопасный no-op. Shell по `false`
        // остаётся на пути с `PushTransform`-обёрткой, так что страница не
        // нарисуется поверх tab bar-а.
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        assert!(!b.supports_page_offset(), "дефолт — page-offset не поддерживается");
        b.set_page_offset(16.0, 36.0); // no-op, must not panic
    }

    #[test]
    fn null_backend_set_frame_commit_id_default_noop() {
        // ADR-016 M1.4: аннотация frame-log — no-op по умолчанию (однопоточный
        // путь), вызов с любым `self_tick` не должен паниковать.
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        b.set_frame_commit_id(42, false);
        b.set_frame_commit_id(42, true);
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

    #[test]
    fn null_backend_on_atlas_memory_pressure_noop() {
        let mut b: Box<dyn RenderBackend> = Box::new(NullBackend);
        // Default impl is a no-op — must not panic for any pressure level.
        b.on_atlas_memory_pressure(MemoryPressureLevel::Low);
        b.on_atlas_memory_pressure(MemoryPressureLevel::Medium);
        b.on_atlas_memory_pressure(MemoryPressureLevel::High);
    }
}
