//! `FemtovgBackend` — femtovg/OpenGL рендер-бэкенд, реализующий [`RenderBackend`].
//!
//! Phase 2 бэкенд: заменяет hand-written WGSL шейдеры на 2D GPU API femtovg
//! (OpenGL ES 2.0). Все [`DisplayCommand`] транслируются в вызовы `Canvas`.
//!
//! RB-5: скелет + базовые команды (FillRect/FillRoundedRect/DrawText/DrawBorder/PushClipRect).
//! RB-6: добавит все остальные ~30 вариантов DisplayCommand.
//!
//! # Архитектура
//!
//! ```text
//! FemtovgBackend
//!   ├── femtovg::Canvas<OpenGl>  ← основной 2D API
//!   ├── glutin PossiblyCurrentContext  ← OpenGL контекст
//!   └── glutin Surface<WindowSurface>  ← поверхность отрисовки
//! ```
//!
//! OpenGL контекст создаётся при вызове `new(window, font_bytes)`. Он привязан
//! к потоку (thread-affine), но может быть передан другому потоку через
//! `make_current` / `make_not_current`. `Send` реализован вручную (см. ниже).
//!
//! # Ограничения (Phase 2 / RB-5)
//!
//! - Изображения (`DrawImage`, `DrawBackgroundImage`) — no-op, задача RB-6.
//! - Градиенты — no-op, задача RB-6.
//! - Трансформы, фильтры, маски, прозрачность — no-op, задача RB-6.
//! - Скроллбары, scroll-layer — no-op, задача RB-6.

use std::collections::HashMap;
use std::sync::Arc;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::prelude::*;
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::GlWindow;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use lumen_core::ext::FontProvider;
use lumen_core::geom::Size;
use lumen_image::Image;
use lumen_layout::Color;

use crate::backend::{RenderBackend, RenderError};
use crate::display_list::{CornerRadii, DisplayCommand};

// ─── Color conversion ────────────────────────────────────────────────────────

/// Конвертирует CSS `Color` (u8 каналы 0-255) в femtovg `Color` (f32 0-1).
#[inline]
fn lumen_to_fvg(c: Color) -> femtovg::Color {
    femtovg::Color::rgba(c.r, c.g, c.b, c.a)
}

// ─── FemtovgBackend ──────────────────────────────────────────────────────────

/// femtovg/OpenGL рендер-бэкенд (Phase 2, ADR-010).
///
/// Реализует [`RenderBackend`] через femtovg 2D Canvas API поверх OpenGL.
/// Создаётся из winit-окна через [`FemtovgBackend::new`].
pub struct FemtovgBackend {
    /// femtovg Canvas — основной 2D-API.
    canvas: femtovg::Canvas<femtovg::renderer::OpenGl>,
    /// Текущий OpenGL контекст. Должен быть current перед любым вызовом canvas.
    gl_context: PossiblyCurrentContext,
    /// Surface для swap buffers.
    gl_surface: Surface<WindowSurface>,
    /// Ширина viewport в физических пикселях.
    width: u32,
    /// Высота viewport в физических пикселях.
    height: u32,
    /// Device pixel ratio (HiDPI).
    scale: f64,
    /// ID bundled-шрифта (Inter Regular) в femtovg atlas.
    font_id: Option<femtovg::FontId>,
    /// Зарегистрированные изображения: src URL → femtovg ImageId.
    images: HashMap<String, femtovg::ImageId>,
    /// Провайдер шрифтов для multi-family рендера (опциональный).
    font_provider: Option<Arc<dyn FontProvider>>,
    /// Глубина стека PushClipRect/PushOpacity/PushTransform.
    /// Каждый Push вызывает canvas.save(); каждый Pop — canvas.restore().
    layer_stack_depth: usize,
}

// SAFETY: FemtovgBackend используется только из одного потока одновременно
// (enforce-ится через `&mut self` в методах трейта). OpenGL контекст
// передаётся потоку compositor-а через glutin make_current; оба потока
// никогда не используют контекст одновременно. glow::Context внутри femtovg
// содержит raw pointer, но мы гарантируем единственного владельца в каждый
// момент. Этот паттерн — стандартный для single-threaded GL рендереров.
unsafe impl Send for FemtovgBackend {}

impl FemtovgBackend {
    /// Создаёт оконный femtovg-бэкенд из winit-окна.
    ///
    /// Инициализирует OpenGL контекст через glutin, создаёт femtovg Canvas
    /// и загружает bundled-шрифт `font_bytes`.
    ///
    /// # Errors
    /// Возвращает `Err` если:
    /// - GPU/драйвер не поддерживает OpenGL
    /// - glutin не может создать контекст или surface
    /// - femtovg не может инициализировать рендерер
    pub fn new(
        window: Arc<Window>,
        font_bytes: Vec<u8>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let display_handle = window.display_handle()?.as_raw();
        let window_handle = window.window_handle()?.as_raw();

        // Создаём GL Display — платформо-специфичный API (WGL/EGL/CGL).
        let gl_display = unsafe {
            // SAFETY: display_handle и window_handle живы всё время вызова.
            #[cfg(target_os = "windows")]
            let pref = DisplayApiPreference::EglThenWgl(Some(window_handle));
            #[cfg(target_os = "macos")]
            let pref = DisplayApiPreference::Cgl;
            #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
            let pref = DisplayApiPreference::Egl;
            Display::new(display_handle, pref)?
        };

        // Ищем подходящую GL-конфигурацию (RGBA 8-bit, без multisampling для скорости).
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(false)
            .build();

        let gl_config = unsafe {
            // SAFETY: шаблон совместим с gl_display.
            gl_display
                .find_configs(template)?
                .reduce(|a, b| if a.num_samples() > b.num_samples() { a } else { b })
                .ok_or("femtovg: no compatible OpenGL config")?
        };

        // Создаём OpenGL контекст (пока не-current).
        let ctx_attrs = ContextAttributesBuilder::new().build(Some(window_handle));
        let not_current = unsafe {
            // SAFETY: ctx_attrs и window_handle корректны.
            gl_display.create_context(&gl_config, &ctx_attrs)?
        };

        // Создаём surface из winit-окна.
        let surface_attrs = window
            .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
            .map_err(|e| format!("femtovg surface attrs: {e:?}"))?;
        let gl_surface = unsafe {
            // SAFETY: surface_attrs совместим с gl_config.
            gl_display.create_window_surface(&gl_config, &surface_attrs)?
        };

        // Делаем контекст current на текущем потоке.
        let gl_context = not_current.make_current(&gl_surface)?;

        // Создаём femtovg OpenGL рендерер через function-pointer loader.
        let renderer = unsafe {
            // SAFETY: gl_context current, gl_display живёт в рамках этой функции.
            femtovg::renderer::OpenGl::new_from_function_cstr(|s| {
                gl_display.get_proc_address(s)
            })
            .map_err(|e| format!("femtovg OpenGl renderer: {e:?}"))?
        };

        let mut canvas =
            femtovg::Canvas::new(renderer).map_err(|e| format!("femtovg canvas: {e:?}"))?;

        // Загружаем bundled Inter как fallback-шрифт.
        let font_id = canvas.add_font_mem(&font_bytes).ok();

        let size = window.inner_size();
        let scale = window.scale_factor();

        Ok(Self {
            canvas,
            gl_context,
            gl_surface,
            width: size.width,
            height: size.height,
            scale,
            font_id,
            images: HashMap::new(),
            font_provider: None,
            layer_stack_depth: 0,
        })
    }

    // ─── Drawing helpers ──────────────────────────────────────────────────────

    /// Рисует залитый прямоугольник.
    fn draw_fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let mut path = femtovg::Path::new();
        path.rect(x, y, w, h);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Рисует залитый прямоугольник с разными радиусами углов.
    ///
    /// femtovg поддерживает per-corner radius, но только круглые (не эллиптические).
    /// Для скелета берём `max(rx, ry)` каждого угла — достаточно для большинства CSS.
    fn draw_fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadii,
        color: Color,
    ) {
        let tl = radii.tl.max(radii.tl_y);
        let tr = radii.tr.max(radii.tr_y);
        let br = radii.br.max(radii.br_y);
        let bl = radii.bl.max(radii.bl_y);

        let mut path = femtovg::Path::new();
        // Порядок: TL, TR, BR, BL — совпадает с CSS border-radius.
        path.rounded_rect_varying(x, y, w, h, tl, tr, br, bl);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Рисует текст.
    ///
    /// Baseline ≈ 80% от font_size (аппроксимация для скелета;
    /// точные метрики — из font metrics в RB-6).
    fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color) {
        let mut paint = femtovg::Paint::color(lumen_to_fvg(color));
        if let Some(id) = self.font_id {
            paint.set_font(&[id]);
        }
        paint.set_font_size(font_size);
        let _ = self.canvas.fill_text(x, y + font_size * 0.8, text, &paint);
    }

    // ─── Command dispatch ─────────────────────────────────────────────────────

    /// Обрабатывает одну команду display list.
    fn render_command(&mut self, cmd: &DisplayCommand) {
        match cmd {
            // ── Базовые команды (RB-5) ──────────────────────────────────────
            DisplayCommand::FillRect { rect, color } => {
                self.draw_fill_rect(rect.x, rect.y, rect.width, rect.height, *color);
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                self.draw_fill_rounded_rect(
                    rect.x, rect.y, rect.width, rect.height, *radii, *color,
                );
            }
            DisplayCommand::DrawBorder { rect, widths, colors, .. } => {
                // 4 стороны: top, right, bottom, left — каждая как залитый прямоугольник.
                // top
                if widths[0] > 0.0 {
                    self.draw_fill_rect(rect.x, rect.y, rect.width, widths[0], colors[0]);
                }
                // right
                if widths[1] > 0.0 {
                    self.draw_fill_rect(
                        rect.x + rect.width - widths[1], rect.y,
                        widths[1], rect.height, colors[1],
                    );
                }
                // bottom
                if widths[2] > 0.0 {
                    self.draw_fill_rect(
                        rect.x, rect.y + rect.height - widths[2],
                        rect.width, widths[2], colors[2],
                    );
                }
                // left
                if widths[3] > 0.0 {
                    self.draw_fill_rect(rect.x, rect.y, widths[3], rect.height, colors[3]);
                }
            }
            DisplayCommand::DrawText { rect, text, font_size, color, .. } => {
                self.draw_text(rect.x, rect.y, text, *font_size, *color);
            }
            DisplayCommand::PushClipRect { rect } => {
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopClip => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Scroll layer — clip + translate ────────────────────────────
            DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } => {
                self.canvas.save();
                self.canvas.scissor(
                    clip_rect.x, clip_rect.y,
                    clip_rect.width, clip_rect.height,
                );
                self.canvas.translate(-scroll_x, -scroll_y);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopScrollLayer => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Стек-операции — сохраняем/восстанавливаем состояние canvas
            // TODO RB-6: реализовать реальную прозрачность, blend-mode, трансформы.
            DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PushMaskImage { .. }
            | DisplayCommand::PushMaskLinearGradient { .. }
            | DisplayCommand::PushMaskRadialGradient { .. }
            | DisplayCommand::PushMaskConicGradient { .. }
            | DisplayCommand::PushMaskLayer { .. } => {
                self.canvas.save();
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopOpacity
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PopTransform
            | DisplayCommand::PopFilter
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── TODO RB-6: остальные команды ────────────────────────────────
            // Следующие варианты будут реализованы в RB-6 (полный FemtovgBackend).
            DisplayCommand::DrawImage { .. }
            | DisplayCommand::DrawBackgroundImage { .. }
            | DisplayCommand::DrawLinearGradient { .. }
            | DisplayCommand::DrawRadialGradient { .. }
            | DisplayCommand::DrawConicGradient { .. }
            | DisplayCommand::DrawOutline { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawCrossFade { .. }
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::BoxModelOverlay { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer => {}
        }
    }
}

// ─── RenderBackend impl ───────────────────────────────────────────────────────

impl RenderBackend for FemtovgBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        self.canvas.set_size(self.width, self.height, self.scale as f32);
        self.canvas.clear_rect(
            0, 0, self.width, self.height,
            femtovg::Color::rgb(255, 255, 255),
        );

        // Контент — с учётом scroll.
        self.canvas.save();
        self.canvas.translate(-scroll_x, -scroll_y);
        for cmd in content {
            self.render_command(cmd);
        }
        self.canvas.restore();

        // Overlay — без scroll (tab bar, панели).
        for cmd in overlay {
            self.render_command(cmd);
        }

        self.canvas.flush();

        self.gl_surface
            .swap_buffers(&self.gl_context)
            .map_err(|e| RenderError::Other(e.to_string()))
    }

    fn resize(&mut self, width: u32, height: u32) {
        use std::num::NonZeroU32;
        self.width = width;
        self.height = height;
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            self.gl_surface.resize(&self.gl_context, w, h);
        }
    }

    fn set_scale_factor(&mut self, scale: f64) {
        self.scale = scale;
    }

    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String> {
        use femtovg::{ImageFlags, ImageSource};
        use imgref::ImgRef;
        use rgb::RGBA8;

        // Конвертируем в RGBA8 для femtovg ImageSource::Rgba.
        let rgba: Vec<RGBA8> = image_to_rgba8_vec(image);
        let img = ImgRef::new(&rgba, image.width as usize, image.height as usize);
        let id = self
            .canvas
            .create_image(ImageSource::Rgba(img), ImageFlags::empty())
            .map_err(|e| format!("femtovg register_image: {e:?}"))?;
        self.images.insert(src, id);
        Ok(())
    }

    fn clear_images(&mut self) {
        for (_, id) in self.images.drain() {
            self.canvas.delete_image(id);
        }
    }

    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.font_provider = provider;
    }

    fn viewport_size(&self) -> Size {
        Size {
            width: (self.width as f64 / self.scale) as f32,
            height: (self.height as f64 / self.scale) as f32,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.scale
    }
}

// ─── Image conversion helper ─────────────────────────────────────────────────

/// Конвертирует `lumen_image::Image` в вектор `RGBA8` пикселей для femtovg.
fn image_to_rgba8_vec(img: &Image) -> Vec<rgb::RGBA8> {
    use lumen_image::PixelFormat;
    use rgb::RGBA;
    match img.format {
        PixelFormat::Rgba8 => img
            .data
            .chunks_exact(4)
            .map(|px| RGBA { r: px[0], g: px[1], b: px[2], a: px[3] })
            .collect(),
        PixelFormat::Rgb8 => img
            .data
            .chunks_exact(3)
            .map(|px| RGBA { r: px[0], g: px[1], b: px[2], a: 255 })
            .collect(),
        PixelFormat::Gray8 => img
            .data
            .iter()
            .map(|&v| RGBA { r: v, g: v, b: v, a: 255 })
            .collect(),
        PixelFormat::GrayAlpha8 => img
            .data
            .chunks_exact(2)
            .map(|px| RGBA { r: px[0], g: px[0], b: px[0], a: px[1] })
            .collect(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Проверяем конвертацию CSS Color → femtovg Color.
    #[test]
    fn lumen_to_fvg_converts_rgba() {
        let c = Color { r: 10, g: 20, b: 30, a: 200 };
        let fvg = lumen_to_fvg(c);
        // femtovg хранит каналы как f32 [0,1]
        assert!((fvg.r - 10.0 / 255.0).abs() < 1e-3);
        assert!((fvg.g - 20.0 / 255.0).abs() < 1e-3);
        assert!((fvg.b - 30.0 / 255.0).abs() < 1e-3);
        assert!((fvg.a - 200.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn lumen_to_fvg_white() {
        let c = Color::WHITE;
        let fvg = lumen_to_fvg(c);
        assert!((fvg.r - 1.0).abs() < 1e-5);
        assert!((fvg.g - 1.0).abs() < 1e-5);
        assert!((fvg.b - 1.0).abs() < 1e-5);
        assert!((fvg.a - 1.0).abs() < 1e-5);
    }

    #[test]
    fn lumen_to_fvg_transparent() {
        let c = Color::TRANSPARENT;
        let fvg = lumen_to_fvg(c);
        assert_eq!(fvg.a, 0.0);
    }

    #[test]
    fn image_to_rgba8_vec_passthrough_rgba() {
        use lumen_image::{Image, PixelFormat};
        let img = Image {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![10, 20, 30, 200],
            icc_profile: None,
        };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 10);
        assert_eq!(out[0].g, 20);
        assert_eq!(out[0].b, 30);
        assert_eq!(out[0].a, 200);
    }

    #[test]
    fn image_to_rgba8_vec_expands_rgb() {
        use lumen_image::{Image, PixelFormat};
        let img = Image {
            width: 1,
            height: 1,
            format: PixelFormat::Rgb8,
            data: vec![10, 20, 30],
            icc_profile: None,
        };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 10);
        assert_eq!(out[0].g, 20);
        assert_eq!(out[0].b, 30);
        assert_eq!(out[0].a, 255);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray8() {
        use lumen_image::{Image, PixelFormat};
        let img = Image {
            width: 1,
            height: 1,
            format: PixelFormat::Gray8,
            data: vec![128],
            icc_profile: None,
        };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 128);
        assert_eq!(out[0].g, 128);
        assert_eq!(out[0].b, 128);
        assert_eq!(out[0].a, 255);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray_alpha() {
        use lumen_image::{Image, PixelFormat};
        let img = Image {
            width: 1,
            height: 1,
            format: PixelFormat::GrayAlpha8,
            data: vec![100, 200],
            icc_profile: None,
        };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 100);
        assert_eq!(out[0].a, 200);
    }

    /// FemtovgBackend должен быть Send (требование RenderBackend трейта).
    #[test]
    fn femtovg_backend_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FemtovgBackend>();
    }
}
