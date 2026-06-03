//! `CompareBackend` — рендер двумя бэкендами параллельно + pixel-diff (ADR-010).
//!
//! Используется для валидации нового бэкенда (например, vello) против эталонного
//! (femtovg или cpu) перед повышением нового в default. Запускается через:
//!
//! ```bash
//! cargo test -p lumen-driver --features compare-backends
//! ```
//!
//! Для каждой страницы сравнение выводит:
//! ```text
//! 01-colors.html      cpu vs cpu  0.0%  ✅
//! 30-css-filter.html  cpu vs cpu  0.0%  ✅
//! ```
//!
//! Доступен только с feature `compare`.

use std::sync::Arc;

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;

use crate::backend::{RenderBackend, RenderError};
use crate::DisplayCommand;

// ─── DiffResult ──────────────────────────────────────────────────────────────

/// Результат pixel-diff сравнения двух бэкендов.
///
/// Считается после каждого [`CompareBackend::render`] вызова.
/// `diff_pixels` — число байт (не пикселей) которые различаются; для пиксельного
/// счёта поделите на 4 (RGBA8 = 4 байта на пиксель).
#[derive(Debug, Clone, PartialEq)]
pub struct DiffResult {
    /// Число различающихся **байт** (RGBA8: 4 байта на пиксель).
    pub diff_bytes: u32,

    /// Общее число байт в изображении (= width × height × 4).
    pub total_bytes: u32,

    /// Число различающихся **пикселей** (= diff_bytes / 4).
    pub diff_pixels: u32,

    /// Общее число пикселей (= total_bytes / 4).
    pub total_pixels: u32,
}

impl DiffResult {
    /// Доля отличающихся пикселей в процентах (0.0 – 100.0).
    ///
    /// Возвращает 0.0 если `total_pixels == 0`.
    pub fn diff_percent(&self) -> f32 {
        if self.total_pixels == 0 {
            return 0.0;
        }
        self.diff_pixels as f32 / self.total_pixels as f32 * 100.0
    }

    /// `true` если бэкенды дали побитово идентичные результаты.
    pub fn is_identical(&self) -> bool {
        self.diff_pixels == 0
    }

    /// Форматирует результат в строку для логов.
    ///
    /// Пример: `"cpu vs cpu  0.0%  ✅"` или `"cpu vs vello  3.2%  ⚠️"`.
    pub fn format(&self, primary_name: &str, secondary_name: &str) -> String {
        let icon = if self.diff_percent() < 1.0 { "✅" } else { "⚠️" };
        format!(
            "{primary_name} vs {secondary_name}  {:.1}%  {icon}",
            self.diff_percent()
        )
    }

    /// Вычисляет DiffResult из двух RGBA8-буферов одинакового размера.
    ///
    /// Паникует если буферы имеют разный размер — это программная ошибка
    /// (CompareBackend должен обеспечить одинаковый size у обоих бэкендов).
    pub fn compute(primary: &[u8], secondary: &[u8]) -> Self {
        assert_eq!(
            primary.len(),
            secondary.len(),
            "CompareBackend: primary и secondary должны иметь одинаковый размер буфера"
        );
        let total_bytes = primary.len() as u32;
        let total_pixels = total_bytes / 4;

        // Считаем пиксели (a != b для любого из 4 каналов = 1 отличающийся пиксель).
        let diff_pixels = primary
            .chunks(4)
            .zip(secondary.chunks(4))
            .filter(|(a, b)| a != b)
            .count() as u32;

        let diff_bytes = primary
            .iter()
            .zip(secondary.iter())
            .filter(|(a, b)| a != b)
            .count() as u32;

        Self { diff_bytes, total_bytes, diff_pixels, total_pixels }
    }
}

// ─── CompareBackend ───────────────────────────────────────────────────────────

/// Тестовый бэкенд: рендерит двумя бэкендами + вычисляет pixel-diff.
///
/// После каждого [`render`][RenderBackend::render] результат доступен
/// через [`CompareBackend::last_diff`].
///
/// Оба бэкенда должны реализовывать [`screenshot_rgba`][RenderBackend::screenshot_rgba];
/// если один из них вернул `None` — diff пропускается, `last_diff` не обновляется.
///
/// # Пример
/// ```no_run
/// use lumen_paint::backends::cpu_backend::CpuBackend;
/// use lumen_paint::backends::compare_backend::CompareBackend;
/// use lumen_paint::RenderBackend;
///
/// let primary = Box::new(CpuBackend::new(1024, 720));
/// let secondary = Box::new(CpuBackend::new(1024, 720));
/// let mut compare = CompareBackend::new(primary, secondary);
/// compare.render(&[], &[], 0.0, 0.0).unwrap();
/// let diff = compare.last_diff().unwrap();
/// assert!(diff.is_identical());
/// ```
pub struct CompareBackend {
    /// Первичный бэкенд — также является источником для [`screenshot_rgba`][RenderBackend::screenshot_rgba].
    primary: Box<dyn RenderBackend>,

    /// Вторичный бэкенд — сравнивается с primary.
    secondary: Box<dyn RenderBackend>,

    /// Результат последнего pixel-diff (None до первого render).
    last_diff: Option<DiffResult>,
}

impl CompareBackend {
    /// Создаёт CompareBackend из двух headless-бэкендов.
    ///
    /// Оба бэкенда должны реализовывать `screenshot_rgba()` — иначе diff
    /// не будет вычисляться и `last_diff()` всегда вернёт `None`.
    pub fn new(primary: Box<dyn RenderBackend>, secondary: Box<dyn RenderBackend>) -> Self {
        Self { primary, secondary, last_diff: None }
    }

    /// Возвращает результат pixel-diff последнего render-а.
    ///
    /// `None` если render ещё не вызывался, или если один из бэкендов
    /// не реализует `screenshot_rgba()`.
    pub fn last_diff(&self) -> Option<&DiffResult> {
        self.last_diff.as_ref()
    }

    /// Предоставляет read-only доступ к первичному бэкенду.
    pub fn primary(&self) -> &dyn RenderBackend {
        self.primary.as_ref()
    }

    /// Предоставляет read-only доступ к вторичному бэкенду.
    pub fn secondary(&self) -> &dyn RenderBackend {
        self.secondary.as_ref()
    }
}

// ─── RenderBackend impl ───────────────────────────────────────────────────────

impl RenderBackend for CompareBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        // Рендерим оба бэкенда; propagate первую ошибку.
        self.primary.render(content, overlay, scroll_y, scroll_x)?;
        self.secondary.render(content, overlay, scroll_y, scroll_x)?;

        // Вычисляем diff если оба бэкенда поддерживают screenshot.
        if let (Some(a), Some(b)) = (self.primary.screenshot_rgba(), self.secondary.screenshot_rgba())
            && a.len() == b.len()
            && !a.is_empty()
        {
            self.last_diff = Some(DiffResult::compute(&a, &b));
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.primary.resize(width, height);
        self.secondary.resize(width, height);
        self.last_diff = None;
    }

    fn set_scale_factor(&mut self, scale: f64) {
        self.primary.set_scale_factor(scale);
        self.secondary.set_scale_factor(scale);
    }

    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String> {
        // Регистрируем в обоих бэкендах; вторая ошибка побеждает.
        let r1 = self.primary.register_image(src.clone(), image);
        let r2 = self.secondary.register_image(src, image);
        r1.and(r2)
    }

    fn clear_images(&mut self) {
        self.primary.clear_images();
        self.secondary.clear_images();
    }

    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.primary.set_font_provider(provider.clone());
        self.secondary.set_font_provider(provider);
    }

    fn viewport_size(&self) -> Size {
        // Возвращаем размер primary.
        self.primary.viewport_size()
    }

    fn scale_factor(&self) -> f64 {
        self.primary.scale_factor()
    }

    fn on_layer_memory_pressure(&mut self, level: MemoryPressureLevel) {
        self.primary.on_layer_memory_pressure(level);
        self.secondary.on_layer_memory_pressure(level);
    }

    fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
        // Возвращаем пиксели primary — они и есть «результат» рендера.
        self.primary.screenshot_rgba()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DiffResult tests ──

    #[test]
    fn diff_result_identical_buffers() {
        let buf = vec![255u8; 16]; // 4 пикселя
        let diff = DiffResult::compute(&buf, &buf.clone());
        assert_eq!(diff.diff_pixels, 0);
        assert!(diff.is_identical());
        assert_eq!(diff.diff_percent(), 0.0);
    }

    #[test]
    fn diff_result_all_differ() {
        let a = vec![0u8; 16];
        let b = vec![255u8; 16];
        let diff = DiffResult::compute(&a, &b);
        assert_eq!(diff.diff_pixels, 4, "все 4 пикселя отличаются");
        assert_eq!(diff.total_pixels, 4);
        assert_eq!(diff.diff_percent(), 100.0);
    }

    #[test]
    fn diff_result_half_differ() {
        // 8 пикселей: первые 4 = 0, вторые 4 = 255.
        let a = vec![0u8; 32];
        let mut b = vec![0u8; 32];
        b[16..32].fill(255);
        let diff = DiffResult::compute(&a, &b);
        assert_eq!(diff.diff_pixels, 4, "4 из 8 пикселей отличаются");
        assert!((diff.diff_percent() - 50.0).abs() < 0.01);
    }

    #[test]
    fn diff_result_empty_buffers() {
        let diff = DiffResult::compute(&[], &[]);
        assert_eq!(diff.total_pixels, 0);
        assert_eq!(diff.diff_percent(), 0.0, "нет деления на ноль");
    }

    #[test]
    fn diff_result_format_identical() {
        let diff = DiffResult { diff_bytes: 0, total_bytes: 400, diff_pixels: 0, total_pixels: 100 };
        let s = diff.format("cpu", "cpu");
        assert!(s.contains("0.0%"), "ожидаем 0.0%: {s}");
        assert!(s.contains("✅"), "ожидаем зелёный чекмарк: {s}");
    }

    #[test]
    fn diff_result_format_diverging() {
        let diff = DiffResult { diff_bytes: 160, total_bytes: 400, diff_pixels: 40, total_pixels: 100 };
        let s = diff.format("cpu", "vello");
        assert!(s.contains("40.0%"), "ожидаем 40.0%: {s}");
        assert!(s.contains("⚠️"), "ожидаем предупреждение: {s}");
    }

    // ── CompareBackend tests ──

    /// Нулевой бэкенд: не рендерит ничего, screenshot возвращает прозрачный буфер.
    struct NullHeadlessBackend {
        width: u32,
        height: u32,
    }

    impl RenderBackend for NullHeadlessBackend {
        fn render(
            &mut self,
            _content: &[DisplayCommand],
            _overlay: &[DisplayCommand],
            _scroll_y: f32,
            _scroll_x: f32,
        ) -> Result<(), RenderError> {
            Ok(())
        }
        fn resize(&mut self, w: u32, h: u32) {
            self.width = w;
            self.height = h;
        }
        fn set_scale_factor(&mut self, _scale: f64) {}
        fn register_image(&mut self, _src: String, _image: &Image) -> Result<(), String> {
            Ok(())
        }
        fn clear_images(&mut self) {}
        fn set_font_provider(&mut self, _provider: Option<Arc<dyn FontProvider>>) {}
        fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
            Some(vec![0u8; (self.width * self.height * 4) as usize])
        }
    }

    #[test]
    fn compare_backend_identical_null_backends() {
        let a = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let b = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let mut cmp = CompareBackend::new(a, b);
        cmp.render(&[], &[], 0.0, 0.0).expect("render OK");
        let diff = cmp.last_diff().expect("diff должен быть вычислен");
        assert!(diff.is_identical(), "два одинаковых нулевых бэкенда — diff = 0");
    }

    #[test]
    fn compare_backend_no_diff_before_render() {
        let a = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let b = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let cmp = CompareBackend::new(a, b);
        assert!(cmp.last_diff().is_none(), "до render() diff не определён");
    }

    #[test]
    fn compare_backend_screenshot_rgba_from_primary() {
        let a = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let b = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let mut cmp = CompareBackend::new(a, b);
        cmp.render(&[], &[], 0.0, 0.0).expect("render OK");
        let px = cmp.screenshot_rgba().expect("screenshot от primary");
        assert_eq!(px.len(), 4 * 4 * 4, "4×4 RGBA8");
    }

    #[test]
    fn compare_backend_resize_clears_diff() {
        let a = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let b = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let mut cmp = CompareBackend::new(a, b);
        cmp.render(&[], &[], 0.0, 0.0).expect("render OK");
        assert!(cmp.last_diff().is_some());
        cmp.resize(16, 16);
        assert!(cmp.last_diff().is_none(), "после resize diff сброшен");
    }

    #[test]
    fn compare_backend_register_image_delegates() {
        let a = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let b = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let mut cmp = CompareBackend::new(a, b);
        let img = Image {
            width: 1,
            height: 1,
            format: lumen_image::PixelFormat::Rgba8,
            data: vec![255, 0, 0, 255],
            icc_profile: None,
        };
        assert!(cmp.register_image("test.png".into(), &img).is_ok());
    }

    #[test]
    fn compare_backend_viewport_size_from_primary() {
        let a = Box::new(NullHeadlessBackend { width: 1024, height: 720 });
        let b = Box::new(NullHeadlessBackend { width: 512, height: 400 });
        let cmp = CompareBackend::new(a, b);
        let sz = cmp.viewport_size();
        assert_eq!(sz.width, 1024.0, "берём размер primary");
        assert_eq!(sz.height, 720.0, "берём размер primary");
    }

    /// Бэкенд возвращающий отличный буфер чтобы проверить что diff != 0.
    struct WhiteBackend {
        width: u32,
        height: u32,
    }

    impl RenderBackend for WhiteBackend {
        fn render(
            &mut self,
            _c: &[DisplayCommand],
            _o: &[DisplayCommand],
            _sy: f32,
            _sx: f32,
        ) -> Result<(), RenderError> {
            Ok(())
        }
        fn resize(&mut self, w: u32, h: u32) {
            self.width = w;
            self.height = h;
        }
        fn set_scale_factor(&mut self, _: f64) {}
        fn register_image(&mut self, _: String, _: &Image) -> Result<(), String> {
            Ok(())
        }
        fn clear_images(&mut self) {}
        fn set_font_provider(&mut self, _: Option<Arc<dyn FontProvider>>) {}
        fn screenshot_rgba(&mut self) -> Option<Vec<u8>> {
            Some(vec![255u8; (self.width * self.height * 4) as usize])
        }
    }

    #[test]
    fn compare_backend_detects_difference() {
        // primary = нулевой (прозрачный), secondary = белый → должен быть diff.
        let a = Box::new(NullHeadlessBackend { width: 8, height: 8 });
        let b = Box::new(WhiteBackend { width: 8, height: 8 });
        let mut cmp = CompareBackend::new(a, b);
        cmp.render(&[], &[], 0.0, 0.0).expect("render OK");
        let diff = cmp.last_diff().expect("diff вычислен");
        assert!(!diff.is_identical(), "нулевой vs белый — должен быть diff");
        assert!(diff.diff_percent() > 0.0, "diff% > 0");
    }

    #[test]
    fn compare_backend_memory_pressure_delegates() {
        let a = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let b = Box::new(NullHeadlessBackend { width: 4, height: 4 });
        let mut cmp = CompareBackend::new(a, b);
        // Не должен паниковать.
        cmp.on_layer_memory_pressure(MemoryPressureLevel::High);
    }

    #[test]
    fn diff_result_single_pixel_differ() {
        // 1 пиксель = 4 байта. Первый байт отличается.
        let mut a = vec![0u8; 16]; // 4 пикселя
        let b = vec![0u8; 16];
        a[0] = 1; // первый пиксель — R-канал отличается
        let diff = DiffResult::compute(&a, &b);
        assert_eq!(diff.diff_pixels, 1, "ровно 1 пиксель отличается");
        assert_eq!(diff.diff_bytes, 1, "ровно 1 байт отличается");
    }
}
