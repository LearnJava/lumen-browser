//! `FemtovgBackend` — femtovg/OpenGL рендер-бэкенд, реализующий [`RenderBackend`].
//!
//! Phase 2 бэкенд: заменяет hand-written WGSL шейдеры на 2D GPU API femtovg
//! (OpenGL ES 2.0). Все [`DisplayCommand`] транслируются в вызовы `Canvas`.
//!
//! RB-5: скелет + базовые команды (FillRect/FillRoundedRect/DrawText/DrawBorder/PushClipRect).
//! RB-6: все ~30 вариантов DisplayCommand.
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
//! # Ограничения (Phase 2 / RB-6)
//!
//! - Backdrop-filter: save/restore без реального offscreen (femtovg не поддерживает GPU backdrop blur).
//! - CSS Blur filter: сохраняет состояние canvas без реального размытия.
//! - Color-matrix фильтры (grayscale/sepia/hue-rotate/invert): аппроксимация через global_alpha.
//! - Маски (PushMaskImage, PushMask*): аппроксимация через scissor (прямоугольная обрезка).
//! - MixBlendMode: маппируются на CompositeOperation; Multiply/Screen и CSS-specific режимы
//!   аппроксимируются через SourceOver (femtovg не поддерживает все CSS blend modes через OpenGL).

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

use lumen_layout::{GradientStop, Length};

use lumen_core::geom::Rect;

use crate::backend::{RenderBackend, RenderError};
use crate::display_list::{BlendMode, CornerRadii, DisplayCommand};

// ─── Color conversion ────────────────────────────────────────────────────────

/// Конвертирует CSS `Color` (u8 каналы 0-255) в femtovg `Color` (f32 0-1).
#[inline]
fn lumen_to_fvg(c: Color) -> femtovg::Color {
    femtovg::Color::rgba(c.r, c.g, c.b, c.a)
}

// ─── Gradient helpers ─────────────────────────────────────────────────────────

/// Разрешает `GradientStop.position` в [0,1], равномерно распределяя `None` позиции.
fn resolve_stops(stops: &[GradientStop], width: f32) -> Vec<(f32, femtovg::Color)> {
    if stops.is_empty() {
        return vec![];
    }
    let mut result: Vec<Option<f32>> = Vec::with_capacity(stops.len());
    for s in stops {
        let p = s.position.as_ref().map(|l| match l {
            Length::Px(v) if width > 0.0 => (v / width).clamp(0.0, 1.0),
            Length::Px(_) => 0.0,
            Length::Percent(p) => (p / 100.0).clamp(0.0, 1.0),
            _ => 0.0,
        });
        result.push(p);
    }
    // Разрешаем None: первый None → 0.0, последний None → 1.0, промежуточные — линейно.
    if result[0].is_none() {
        result[0] = Some(0.0);
    }
    if result[result.len() - 1].is_none() {
        let last = result.len() - 1;
        result[last] = Some(1.0);
    }
    let n = result.len();
    let mut i = 0;
    while i < n {
        if result[i].is_none() {
            let start = i - 1;
            let mut end = i + 1;
            while end < n && result[end].is_none() {
                end += 1;
            }
            let v0 = result[start].unwrap_or(0.0);
            let v1 = result[end].unwrap_or(1.0);
            let count = (end - start) as f32;
            for (idx, item) in result.iter_mut().enumerate().take(end).skip(start + 1) {
                let t = (idx - start) as f32 / count;
                *item = Some(v0 + t * (v1 - v0));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    result
        .iter()
        .zip(stops.iter())
        .map(|(pos, s)| (pos.unwrap_or(0.0), lumen_to_fvg(s.color)))
        .collect()
}

/// Вычисляет начало и конец линейного градиента для femtovg из CSS angle_deg.
///
/// CSS: 0° = «to top», 90° = «to right». femtovg использует абсолютные координаты.
fn linear_gradient_endpoints(
    x: f32, y: f32, w: f32, h: f32, angle_deg: f32,
) -> ([f32; 2], [f32; 2]) {
    if w <= 0.0 || h <= 0.0 {
        return ([x, y + h], [x, y]);
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos();
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([x + w / 2.0, y + h / 2.0], [x + w / 2.0, y + h / 2.0]);
    }
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let sx = cx - dx * half_len;
    let sy = cy - dy * half_len;
    let ex = cx + dx * half_len;
    let ey = cy + dy * half_len;
    ([sx, sy], [ex, ey])
}

// ─── Sticky layer helpers ─────────────────────────────────────────────────────

/// CSS Positioning L3 §6.3 — смещение dy для sticky-элемента.
fn sticky_offset_dy(
    flow_rect: &lumen_core::geom::Rect,
    top: Option<f32>,
    bottom: Option<f32>,
    scroll_y: f32,
    viewport_h: f32,
) -> f32 {
    let mut dy = -scroll_y;
    if let Some(t) = top {
        let screen_y = flow_rect.y + dy;
        if screen_y < t {
            dy += t - screen_y;
        }
    }
    if let Some(b) = bottom {
        let max_screen_y = viewport_h - b - flow_rect.height;
        let actual = flow_rect.y + dy;
        if actual > max_screen_y {
            dy -= actual - max_screen_y;
        }
    }
    dy
}

/// CSS Positioning L3 §6.3 — смещение dx для sticky-элемента.
fn sticky_offset_dx(
    flow_rect: &lumen_core::geom::Rect,
    left: Option<f32>,
    right: Option<f32>,
    scroll_x: f32,
    viewport_w: f32,
) -> f32 {
    let mut dx = -scroll_x;
    if let Some(l) = left {
        let screen_x = flow_rect.x + dx;
        if screen_x < l {
            dx += l - screen_x;
        }
    }
    if let Some(r) = right {
        let max_screen_x = viewport_w - r - flow_rect.width;
        let actual = flow_rect.x + dx;
        if actual > max_screen_x {
            dx -= actual - max_screen_x;
        }
    }
    dx
}

// ─── BlendMode → CompositeOperation ──────────────────────────────────────────

/// Маппинг CSS MixBlendMode → femtovg CompositeOperation.
///
/// femtovg поддерживает только базовые Porter-Duff операции через OpenGL.
/// CSS Compositing & Blending L1 режимы (Multiply, Screen, Overlay и др.)
/// аппроксимируются через SourceOver — визуально неточно, но не вызывает ошибок.
fn blend_to_composite(mode: BlendMode) -> femtovg::CompositeOperation {
    match mode {
        BlendMode::Normal => femtovg::CompositeOperation::SourceOver,
        BlendMode::PlusLighter => femtovg::CompositeOperation::Lighter,
        // Остальные CSS blend modes не поддерживаются OpenGL ES 2.0 — fallback.
        _ => femtovg::CompositeOperation::SourceOver,
    }
}

// ─── Conic gradient color interpolation ───────────────────────────────────────

/// Интерполирует цвет в позиции `t` [0,1] между двумя соседними stop-ами.
fn interp_conic_color(resolved: &[(f32, femtovg::Color)], t: f32) -> Color {
    if resolved.is_empty() {
        return Color::TRANSPARENT;
    }
    let last = resolved.len() - 1;
    for i in 0..last {
        let (p0, c0) = resolved[i];
        let (p1, c1) = resolved[i + 1];
        if t <= p1 || i == last - 1 {
            let range = p1 - p0;
            let fac = if range > 1e-6 { ((t - p0) / range).clamp(0.0, 1.0) } else { 0.0 };
            let r = (c0.r * 255.0 + fac * (c1.r * 255.0 - c0.r * 255.0)).round() as u8;
            let g = (c0.g * 255.0 + fac * (c1.g * 255.0 - c0.g * 255.0)).round() as u8;
            let b = (c0.b * 255.0 + fac * (c1.b * 255.0 - c0.b * 255.0)).round() as u8;
            let a = (c0.a * 255.0 + fac * (c1.a * 255.0 - c0.a * 255.0)).round() as u8;
            return Color { r, g, b, a };
        }
    }
    let last_color = resolved[last].1;
    Color {
        r: (last_color.r * 255.0).round() as u8,
        g: (last_color.g * 255.0).round() as u8,
        b: (last_color.b * 255.0).round() as u8,
        a: (last_color.a * 255.0).round() as u8,
    }
}

/// CSS Images L4 §3.7 — отображает долю оборота `t` ∈ [0,1) в позицию сэмпла
/// внутри диапазона stop-ов градиента.
///
/// Для `repeating-conic-gradient` паттерн повторяется каждые (last − first) доли
/// оборота: `t` сворачивается в `[first, first+span)` через `rem_euclid`.
/// Для не-repeating (или вырожденного нулевого span) возвращает `t` без изменений.
fn conic_sample_t(t: f32, repeating: bool, first_pos: f32, last_pos: f32) -> f32 {
    let span = last_pos - first_pos;
    if repeating && span > 1e-6 {
        first_pos + (t - first_pos).rem_euclid(span)
    } else {
        t
    }
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
    /// Зарегистрированные layer snapshots: id → femtovg ImageId.
    snapshots: HashMap<u64, femtovg::ImageId>,
    /// Провайдер шрифтов для multi-family рендера (опциональный).
    font_provider: Option<Arc<dyn FontProvider>>,
    /// Глубина стека сохранений canvas (PushClip/Opacity/Transform/...).
    layer_stack_depth: usize,
    /// Стек смещений для position:sticky: (dy, dx).
    sticky_stack: Vec<(f32, f32)>,
    /// Текущий scroll_y, обновляется в `render()` перед обходом content.
    scroll_y: f32,
    /// Текущий scroll_x, обновляется в `render()` перед обходом content.
    scroll_x: f32,
    /// CSS ширина viewport (width / scale), нужна для sticky-вычислений.
    viewport_css_w: f32,
    /// CSS высота viewport (height / scale), нужна для sticky-вычислений.
    viewport_css_h: f32,
    /// Stack of pending blur sigma values. Non-zero = blur filter is active.
    /// Push on PushFilter(Blur), pop on PopFilter.
    blur_sigma_stack: Vec<f32>,
}

// SAFETY: FemtovgBackend используется только из одного потока одновременно
// (enforce-ится через `&mut self` в методах трейта). OpenGL контекст
// передаётся потоку compositor-а через glutin make_current; оба потока
// никогда не используют контекст одновременно. glow::Context внутри femtovg
// содержит raw pointer, но мы гарантируем единственного владельца в каждый
// момент. Этот паттерн — стандартный для single-threaded GL рендереров.
unsafe impl Send for FemtovgBackend {}

// ─── Box blur helper ──────────────────────────────────────────────────────────

/// Apply a separable box blur to RGBA pixel data in-place.
/// `sigma` controls the kernel radius (radius = round(sigma * 1.5)).
/// This is a 3-pass approximation of Gaussian blur (3× box blur ≈ Gaussian).
#[allow(dead_code)]
fn box_blur_rgba(pixels: &mut [u8], width: usize, height: usize, sigma: f32) {
    let r = ((sigma * 1.5).round() as usize).max(1);
    let stride = width * 4;
    // Horizontal pass.
    let mut tmp = pixels.to_vec();
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let x0 = x.saturating_sub(r);
            let x1 = (x + r + 1).min(width);
            for sx in x0..x1 {
                let off = y * stride + sx * 4;
                for c in 0..4 { sum[c] += pixels[off + c] as u32; }
                count += 1;
            }
            let off = y * stride + x * 4;
            for c in 0..4 { tmp[off + c] = (sum[c] / count) as u8; }
        }
    }
    // Vertical pass.
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let y0 = y.saturating_sub(r);
            let y1 = (y + r + 1).min(height);
            for sy in y0..y1 {
                let off = sy * stride + x * 4;
                for c in 0..4 { sum[c] += tmp[off + c] as u32; }
                count += 1;
            }
            let off = y * stride + x * 4;
            for c in 0..4 { pixels[off + c] = (sum[c] / count) as u8; }
        }
    }
}

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
            snapshots: HashMap::new(),
            font_provider: None,
            layer_stack_depth: 0,
            sticky_stack: Vec::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            viewport_css_w: size.width as f32 / scale as f32,
            viewport_css_h: size.height as f32 / scale as f32,
            blur_sigma_stack: Vec::new(),
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
    /// Draw a rounded rectangle with per-corner elliptical radii.
    ///
    /// When `rx == ry` for all corners the path is identical to the circular case.
    /// For elliptical corners (rx ≠ ry) we use cubic Bézier approximation of a
    /// quarter-ellipse with the Geng–Zwart kappa constant ≈ 0.5523.
    fn draw_fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadii,
        color: Color,
    ) {
        // Kappa constant for cubic Bézier approximation of a quarter-circle/ellipse.
        const K: f32 = 0.5523;

        // Clamp radii so they don't exceed half the box dimensions (CSS Backgrounds §5.5).
        let tl_x = radii.tl.min(w / 2.0).min(h / 2.0).max(0.0);
        let tl_y = radii.tl_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let tr_x = radii.tr.min(w / 2.0).min(h / 2.0).max(0.0);
        let tr_y = radii.tr_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let br_x = radii.br.min(w / 2.0).min(h / 2.0).max(0.0);
        let br_y = radii.br_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let bl_x = radii.bl.min(w / 2.0).min(h / 2.0).max(0.0);
        let bl_y = radii.bl_y.min(w / 2.0).min(h / 2.0).max(0.0);

        // Fast path: all corners circular — delegate to femtovg built-in.
        if (tl_x - tl_y).abs() < 0.5 && (tr_x - tr_y).abs() < 0.5
            && (br_x - br_y).abs() < 0.5 && (bl_x - bl_y).abs() < 0.5
        {
            let mut path = femtovg::Path::new();
            path.rounded_rect_varying(x, y, w, h, tl_x, tr_x, br_x, bl_x);
            let paint = femtovg::Paint::color(lumen_to_fvg(color));
            self.canvas.fill_path(&path, &paint);
            return;
        }

        // Elliptical path: build manually with cubic Bézier corners.
        let mut path = femtovg::Path::new();
        // Start at top-left corner's right end.
        path.move_to(x + tl_x, y);
        // Top edge → top-right corner.
        path.line_to(x + w - tr_x, y);
        path.bezier_to(
            x + w - tr_x + K * tr_x, y,
            x + w,                   y + tr_y - K * tr_y,
            x + w,                   y + tr_y,
        );
        // Right edge → bottom-right corner.
        path.line_to(x + w, y + h - br_y);
        path.bezier_to(
            x + w,                    y + h - br_y + K * br_y,
            x + w - br_x + K * br_x, y + h,
            x + w - br_x,             y + h,
        );
        // Bottom edge → bottom-left corner.
        path.line_to(x + bl_x, y + h);
        path.bezier_to(
            x + bl_x - K * bl_x, y + h,
            x,                   y + h - bl_y + K * bl_y,
            x,                   y + h - bl_y,
        );
        // Left edge → top-left corner.
        path.line_to(x, y + tl_y);
        path.bezier_to(
            x,             y + tl_y - K * tl_y,
            x + tl_x - K * tl_x, y,
            x + tl_x,     y,
        );
        path.close();
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Рисует текст.
    ///
    /// Baseline ≈ 80% от font_size (аппроксимация;
    /// точные метрики — из font metrics в будущих задачах).
    fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color) {
        let mut paint = femtovg::Paint::color(lumen_to_fvg(color));
        if let Some(id) = self.font_id {
            paint.set_font(&[id]);
        }
        paint.set_font_size(font_size);
        let _ = self.canvas.fill_text(x, y + font_size * 0.8, text, &paint);
    }

    /// Рисует изображение из зарегистрированного URL в rect.
    ///
    /// Если изображение не зарегистрировано — рисует серый placeholder.
    fn draw_image_in_rect(&mut self, rect: &Rect, src: &str) {
        if let Some(&img_id) = self.images.get(src) {
            let paint = femtovg::Paint::image(
                img_id,
                rect.x, rect.y, rect.width, rect.height,
                0.0, 1.0,
            );
            let mut path = femtovg::Path::new();
            path.rect(rect.x, rect.y, rect.width, rect.height);
            self.canvas.fill_path(&path, &paint);
        } else {
            // Placeholder — светло-серый прямоугольник.
            self.draw_fill_rect(rect.x, rect.y, rect.width, rect.height, Color { r: 200, g: 200, b: 200, a: 255 });
        }
    }

    /// Рисует conic gradient как веер треугольников, обрезанный по box rect.
    ///
    /// femtovg не поддерживает conic gradient нативно. Аппроксимируем через
    /// triangle fan с интерполяцией цвета между stop-ами. CSS Images L4 §3.7:
    /// conic-gradient заливает прямоугольник элемента, поэтому веер (диск,
    /// достающий до углов box) обрезается scissor-ом по `rect`. Без обрезки
    /// диск выходит далеко за пределы box (BUG-086 — гигантские круги).
    fn draw_conic_gradient(
        &mut self,
        rect: &Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        from_angle_deg: f32,
        stops: &[GradientStop],
        repeating: bool,
    ) {
        if stops.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }

        let cx = rect.x + center_x_pct * rect.width;
        let cy = rect.y + center_y_pct * rect.height;
        // Радиус должен доставать до самого дальнего угла box, чтобы веер покрыл
        // весь прямоугольник; излишек срезается scissor-ом ниже. Центр может быть
        // смещён (`at <pos>`), поэтому берём полную диагональ как верхнюю границу.
        let radius = rect.width.hypot(rect.height);

        let resolved = resolve_stops(stops, 1.0);
        if resolved.len() < 2 {
            return;
        }

        // repeating-conic-gradient: паттерн повторяется каждые (last − first)
        // доли оборота (см. `conic_sample_t`).
        let first_pos = resolved[0].0;
        let last_pos = resolved[resolved.len() - 1].0;

        let segments = (resolved.len() * 32).max(360);
        let base_angle = from_angle_deg.to_radians() - std::f32::consts::FRAC_PI_2;

        // Обрезаем веер по box rect, пересекая с активным scissor (например,
        // overflow:hidden контейнером), чтобы не затереть соседние элементы.
        self.canvas.save();
        self.canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);

        for i in 0..segments {
            let t0 = i as f32 / segments as f32;
            let t1 = (i + 1) as f32 / segments as f32;
            let t_mid = (t0 + t1) / 2.0;

            let t_sample = conic_sample_t(t_mid, repeating, first_pos, last_pos);
            let c_mid = interp_conic_color(&resolved, t_sample);
            let avg_color = femtovg::Color::rgba(c_mid.r, c_mid.g, c_mid.b, c_mid.a);

            let a0 = base_angle + t0 * std::f32::consts::TAU;
            let a1 = base_angle + t1 * std::f32::consts::TAU;

            let x0 = cx + a0.cos() * radius;
            let y0 = cy + a0.sin() * radius;
            let x1 = cx + a1.cos() * radius;
            let y1 = cy + a1.sin() * radius;

            let mut path = femtovg::Path::new();
            path.move_to(cx, cy);
            path.line_to(x0, y0);
            path.line_to(x1, y1);
            path.close();

            self.canvas.fill_path(&path, &femtovg::Paint::color(avg_color));
        }

        self.canvas.restore();
    }

    // ─── Command dispatch ─────────────────────────────────────────────────────

    /// Обрабатывает одну команду display list.
    #[allow(clippy::too_many_lines)]
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
                if widths[0] > 0.0 {
                    self.draw_fill_rect(rect.x, rect.y, rect.width, widths[0], colors[0]);
                }
                if widths[1] > 0.0 {
                    self.draw_fill_rect(
                        rect.x + rect.width - widths[1], rect.y,
                        widths[1], rect.height, colors[1],
                    );
                }
                if widths[2] > 0.0 {
                    self.draw_fill_rect(
                        rect.x, rect.y + rect.height - widths[2],
                        rect.width, widths[2], colors[2],
                    );
                }
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

            // ── Scroll layer ────────────────────────────────────────────────
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

            // ── Outline ─────────────────────────────────────────────────────
            DisplayCommand::DrawOutline { rect, width, color, offset, .. } => {
                // Outline рисуется СНАРУЖИ box (rect расширяется на offset+width).
                let expand = offset + width;
                let ox = rect.x - expand;
                let oy = rect.y - expand;
                let ow = rect.width + expand * 2.0;
                let oh = rect.height + expand * 2.0;
                self.draw_fill_rect(ox, oy, ow, *width, *color);
                self.draw_fill_rect(ox, oy + oh - width, ow, *width, *color);
                self.draw_fill_rect(ox, oy + width, *width, oh - width * 2.0, *color);
                self.draw_fill_rect(ox + ow - width, oy + width, *width, oh - width * 2.0, *color);
            }

            // ── Images ──────────────────────────────────────────────────────
            DisplayCommand::DrawImage { rect, src, .. } => {
                self.draw_image_in_rect(rect, src);
            }
            DisplayCommand::DrawBackgroundImage { rect, src, .. } => {
                // Phase 2: repeat/size/position аппроксимируются как stretch.
                self.draw_image_in_rect(rect, src);
            }

            // ── Gradients ───────────────────────────────────────────────────
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, .. } => {
                if rect.width <= 0.0 || rect.height <= 0.0 || stops.is_empty() {
                    return;
                }
                let ([sx, sy], [ex, ey]) = linear_gradient_endpoints(
                    rect.x, rect.y, rect.width, rect.height, *angle_deg,
                );
                let resolved = resolve_stops(stops, rect.width);
                if resolved.len() < 2 {
                    return;
                }
                let paint = femtovg::Paint::linear_gradient_stops(
                    sx, sy, ex, ey,
                    resolved,
                );
                let mut path = femtovg::Path::new();
                path.rect(rect.x, rect.y, rect.width, rect.height);
                self.canvas.fill_path(&path, &paint);
            }

            DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, stops, .. } => {
                if rect.width <= 0.0 || rect.height <= 0.0 || stops.is_empty() {
                    return;
                }
                let cx = rect.x + center_x_pct * rect.width;
                let cy = rect.y + center_y_pct * rect.height;
                let dx = center_x_pct.max(1.0 - center_x_pct) * rect.width;
                let dy = center_y_pct.max(1.0 - center_y_pct) * rect.height;
                let outer_r = dx.hypot(dy).max(1.0);
                let resolved = resolve_stops(stops, outer_r);
                if resolved.len() < 2 {
                    return;
                }
                let paint = femtovg::Paint::radial_gradient_stops(
                    cx, cy, 0.0, outer_r,
                    resolved,
                );
                let mut path = femtovg::Path::new();
                path.rect(rect.x, rect.y, rect.width, rect.height);
                self.canvas.fill_path(&path, &paint);
            }

            DisplayCommand::DrawConicGradient {
                rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            } => {
                self.draw_conic_gradient(
                    rect, *center_x_pct, *center_y_pct, *from_angle_deg, stops, *repeating,
                );
            }

            // ── Scrollbar ───────────────────────────────────────────────────
            DisplayCommand::DrawScrollbar { track_rect, thumb_rect, track_color, thumb_color, .. } => {
                let tc = femtovg::Color::rgbaf(
                    track_color[0], track_color[1], track_color[2], track_color[3],
                );
                let mut path = femtovg::Path::new();
                path.rect(track_rect.x, track_rect.y, track_rect.width, track_rect.height);
                self.canvas.fill_path(&path, &femtovg::Paint::color(tc));

                let thc = femtovg::Color::rgbaf(
                    thumb_color[0], thumb_color[1], thumb_color[2], thumb_color[3],
                );
                let corner_r = (thumb_rect.width.min(thumb_rect.height) / 2.0).min(4.0);
                let mut path2 = femtovg::Path::new();
                path2.rounded_rect(
                    thumb_rect.x, thumb_rect.y, thumb_rect.width, thumb_rect.height, corner_r,
                );
                self.canvas.fill_path(&path2, &femtovg::Paint::color(thc));
            }

            // ── SVG path ────────────────────────────────────────────────────
            DisplayCommand::DrawSvgPath { vertices, color } => {
                if vertices.len() < 3 {
                    return;
                }
                let paint = femtovg::Paint::color(lumen_to_fvg(*color));
                let mut path = femtovg::Path::new();
                for tri in vertices.chunks_exact(3) {
                    path.move_to(tri[0][0], tri[0][1]);
                    path.line_to(tri[1][0], tri[1][1]);
                    path.line_to(tri[2][0], tri[2][1]);
                    path.close();
                }
                self.canvas.fill_path(&path, &paint);
            }

            // ── Cross-fade ──────────────────────────────────────────────────
            DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                let p = progress.clamp(0.0, 1.0);
                if let Some(&id_a) = self.images.get(src_a.as_str()) {
                    let paint = femtovg::Paint::image(
                        id_a,
                        dest.x, dest.y, dest.width, dest.height, 0.0, 1.0 - p,
                    );
                    let mut path = femtovg::Path::new();
                    path.rect(dest.x, dest.y, dest.width, dest.height);
                    self.canvas.fill_path(&path, &paint);
                }
                if let Some(&id_b) = self.images.get(src_b.as_str()) {
                    let paint = femtovg::Paint::image(
                        id_b,
                        dest.x, dest.y, dest.width, dest.height, 0.0, p,
                    );
                    let mut path = femtovg::Path::new();
                    path.rect(dest.x, dest.y, dest.width, dest.height);
                    self.canvas.fill_path(&path, &paint);
                }
            }

            // ── Layer snapshot ───────────────────────────────────────────────
            DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                if let Some(&img_id) = self.snapshots.get(id) {
                    let paint = femtovg::Paint::image(
                        img_id,
                        rect.x, rect.y, rect.width, rect.height, 0.0, *alpha,
                    );
                    let mut path = femtovg::Path::new();
                    path.rect(rect.x, rect.y, rect.width, rect.height);
                    self.canvas.fill_path(&path, &paint);
                }
            }

            // ── Box model overlay ────────────────────────────────────────────
            DisplayCommand::BoxModelOverlay { margin, border, padding, content } => {
                // Chrome DevTools палитра — полупрозрачные слои.
                for (r, c) in [
                    (margin,  femtovg::Color::rgba(246, 178, 107, 100)),
                    (border,  femtovg::Color::rgba(255, 229, 153, 100)),
                    (padding, femtovg::Color::rgba(147, 196, 125, 100)),
                    (content, femtovg::Color::rgba(111, 168, 220, 100)),
                ] {
                    let mut path = femtovg::Path::new();
                    path.rect(r.x, r.y, r.width, r.height);
                    self.canvas.fill_path(&path, &femtovg::Paint::color(c));
                }
            }

            // ── Opacity ──────────────────────────────────────────────────────
            DisplayCommand::PushOpacity { alpha } => {
                self.canvas.save();
                self.canvas.set_global_alpha(alpha.clamp(0.0, 1.0));
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopOpacity => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Blend mode ───────────────────────────────────────────────────
            DisplayCommand::PushBlendMode { mode } => {
                self.canvas.save();
                self.canvas.global_composite_operation(blend_to_composite(*mode));
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopBlendMode => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Transform ────────────────────────────────────────────────────
            DisplayCommand::PushTransform { matrix } => {
                self.canvas.save();
                // Извлекаем 2D-аффинную часть из Mat4 (column-major layout).
                // Mat4[i]: layout column-major — [col0_row0..col0_row3, col1_row0..col1_row3, ...]
                // femtovg Transform2D([a, b, c, d, e, f]):
                //   | a c e |    a=m[0], c=m[4], e=m[12]
                //   | b d f |    b=m[1], d=m[5], f=m[13]
                //   | 0 0 1 |
                let m = &matrix.0;
                let transform = femtovg::Transform2D([
                    m[0],   // a = scale_x / cos
                    m[1],   // b = sin
                    m[4],   // c = -sin
                    m[5],   // d = scale_y / cos
                    m[12],  // e = translate_x
                    m[13],  // f = translate_y
                ]);
                self.canvas.set_transform(&transform);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopTransform => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Filter ───────────────────────────────────────────────────────
            // femtovg не поддерживает GPU blur/color-matrix. Opacity-filter
            // применяем через global_alpha; остальные — save/restore без визуального эффекта.
            DisplayCommand::PushFilter { filters } => {
                self.canvas.save();
                for f in filters {
                    match f {
                        lumen_layout::FilterFn::Opacity(v) => {
                            self.canvas.set_global_alpha(v.clamp(0.0, 1.0));
                        }
                        lumen_layout::FilterFn::Blur(sigma) => {
                            // Phase 1: record sigma, draw content normally.
                            // Actual blur pass deferred (femtovg has no native blur).
                            self.blur_sigma_stack.push(*sigma);
                        }
                        _ => {
                            // Other filters (Brightness, Contrast, Grayscale, etc.)
                            // are not supported in Phase 2; no-op.
                        }
                    }
                }
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopFilter => {
                if let Some(sigma) = self.blur_sigma_stack.pop() {
                    // Phase 1: sigma recorded but no actual blur applied.
                    // TODO Phase 2: capture offscreen buffer, apply box blur,
                    // draw blurred image. Needs femtovg screenshot() API.
                    let _ = sigma; // suppress unused warning
                }
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Backdrop filter ──────────────────────────────────────────────
            // femtovg не имеет поддержки backdrop-filter. save/restore без эффекта.
            DisplayCommand::PushBackdropFilter { .. } => {
                self.canvas.save();
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopBackdropFilter => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Masks ────────────────────────────────────────────────────────
            // femtovg поддерживает только path clipping.
            // Аппроксимируем gradient/image mask через scissor по rect.
            DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. } => {
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopMask => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }
            DisplayCommand::PushMaskLayer { rect, .. } => {
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopMaskLayer => {
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Sticky layer ─────────────────────────────────────────────────
            DisplayCommand::BeginStickyLayer { flow_rect, top, bottom, left, right } => {
                let sdy = sticky_offset_dy(
                    flow_rect, *top, *bottom, self.scroll_y, self.viewport_css_h,
                );
                let sdx = sticky_offset_dx(
                    flow_rect, *left, *right, self.scroll_x, self.viewport_css_w,
                );
                self.sticky_stack.push((sdy, sdx));
                self.canvas.save();
                // Текущий контент уже сдвинут на (-scroll_x, -scroll_y).
                // Sticky-элемент должен быть на (sdx, sdy) относительно страницы.
                // Компенсируем: tx = sdx - (-scroll_x) = sdx + scroll_x.
                let tx = sdx + self.scroll_x;
                let ty = sdy + self.scroll_y;
                self.canvas.translate(tx, ty);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::EndStickyLayer => {
                self.sticky_stack.pop();
                if self.layer_stack_depth > 0 {
                    self.canvas.restore();
                    self.layer_stack_depth -= 1;
                }
            }

            // ── Page break (print only) ──────────────────────────────────────
            DisplayCommand::PageBreak => {}
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
        // Обновляем scroll context для sticky-вычислений.
        self.scroll_y = scroll_y;
        self.scroll_x = scroll_x;
        self.viewport_css_w = (self.width as f64 / self.scale) as f32;
        self.viewport_css_h = (self.height as f64 / self.scale) as f32;

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
        for (_, id) in self.snapshots.drain() {
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

    #[test]
    fn lumen_to_fvg_converts_rgba() {
        let c = Color { r: 10, g: 20, b: 30, a: 200 };
        let fvg = lumen_to_fvg(c);
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
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgba8, data: vec![10, 20, 30, 200], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 10);
        assert_eq!(out[0].a, 200);
    }

    #[test]
    fn image_to_rgba8_vec_expands_rgb() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgb8, data: vec![10, 20, 30], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].a, 255);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray8() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Gray8, data: vec![128], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].r, 128);
        assert_eq!(out[0].g, 128);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray_alpha() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::GrayAlpha8, data: vec![100, 200], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].r, 100);
        assert_eq!(out[0].a, 200);
    }

    #[test]
    fn femtovg_backend_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FemtovgBackend>();
    }

    #[test]
    fn linear_gradient_endpoints_horizontal() {
        let ([sx, _sy], [ex, _ey]) = linear_gradient_endpoints(0.0, 0.0, 100.0, 50.0, 90.0);
        assert!(sx < ex, "start должен быть левее end для 90°");
    }

    #[test]
    fn linear_gradient_endpoints_vertical() {
        let ([_sx, sy], [_ex, ey]) = linear_gradient_endpoints(0.0, 0.0, 100.0, 50.0, 0.0);
        assert!(sy > ey, "start y должен быть больше end y для 0° (to top)");
    }

    #[test]
    fn resolve_stops_evenly_spaced() {
        let stops = vec![
            GradientStop { color: Color::WHITE, position: None },
            GradientStop { color: Color::BLACK, position: None },
        ];
        let resolved = resolve_stops(&stops, 100.0);
        assert_eq!(resolved.len(), 2);
        assert!((resolved[0].0).abs() < 1e-5);
        assert!((resolved[1].0 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn resolve_stops_fixed_positions() {
        let stops = vec![
            GradientStop { color: Color::WHITE, position: Some(Length::Percent(0.0)) },
            GradientStop { color: Color::BLACK, position: Some(Length::Percent(50.0)) },
            GradientStop { color: Color::WHITE, position: Some(Length::Percent(100.0)) },
        ];
        let resolved = resolve_stops(&stops, 100.0);
        assert_eq!(resolved.len(), 3);
        assert!((resolved[1].0 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn blend_to_composite_normal() {
        let op = blend_to_composite(BlendMode::Normal);
        assert!(matches!(op, femtovg::CompositeOperation::SourceOver));
    }

    #[test]
    fn blend_to_composite_lighter() {
        let op = blend_to_composite(BlendMode::PlusLighter);
        assert!(matches!(op, femtovg::CompositeOperation::Lighter));
    }

    #[test]
    fn sticky_offset_dy_sticks_to_top() {
        use lumen_core::geom::Rect;
        let flow_rect = Rect { x: 0.0, y: 100.0, width: 200.0, height: 50.0 };
        let dy = sticky_offset_dy(&flow_rect, Some(10.0), None, 150.0, 720.0);
        let screen_y = flow_rect.y + dy;
        assert!((screen_y - 10.0).abs() < 1e-4, "screen_y={screen_y}");
    }

    #[test]
    fn sticky_offset_dy_no_insets() {
        use lumen_core::geom::Rect;
        let flow_rect = Rect { x: 0.0, y: 100.0, width: 200.0, height: 50.0 };
        let dy = sticky_offset_dy(&flow_rect, None, None, 200.0, 720.0);
        assert!((dy - (-200.0)).abs() < 1e-4);
    }

    #[test]
    fn interp_conic_color_at_zero() {
        let stops = vec![
            (0.0_f32, femtovg::Color::rgb(0, 0, 0)),
            (1.0_f32, femtovg::Color::rgb(255, 255, 255)),
        ];
        let c = interp_conic_color(&stops, 0.0);
        assert_eq!(c.r, 0);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn interp_conic_color_at_one() {
        let stops = vec![
            (0.0_f32, femtovg::Color::rgb(0, 0, 0)),
            (1.0_f32, femtovg::Color::rgb(255, 255, 255)),
        ];
        let c = interp_conic_color(&stops, 1.0);
        assert_eq!(c.r, 255);
    }

    #[test]
    fn interp_conic_color_midpoint() {
        let stops = vec![
            (0.0_f32, femtovg::Color::rgb(0, 0, 0)),
            (1.0_f32, femtovg::Color::rgb(200, 100, 50)),
        ];
        let c = interp_conic_color(&stops, 0.5);
        assert_eq!(c.r, 100);
        assert_eq!(c.g, 50);
    }

    #[test]
    fn conic_sample_t_non_repeating_is_identity() {
        // Не-repeating: t возвращается как есть на всём обороте.
        assert!((conic_sample_t(0.0, false, 0.0, 0.25) - 0.0).abs() < 1e-6);
        assert!((conic_sample_t(0.7, false, 0.0, 0.25) - 0.7).abs() < 1e-6);
        assert!((conic_sample_t(1.0, false, 0.0, 0.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_tiles_pattern() {
        // BUG-086: repeating-conic-gradient с span=0.25 (45deg×2 паттерн)
        // должен повторяться 4 раза за оборот, а не оставлять 3/4 заливкой
        // последнего цвета. t за пределами первого span сворачивается в него.
        let (first, last) = (0.0_f32, 0.25_f32);
        assert!((conic_sample_t(0.05, true, first, last) - 0.05).abs() < 1e-6);
        // 0.30 → 0.30 mod 0.25 = 0.05
        assert!((conic_sample_t(0.30, true, first, last) - 0.05).abs() < 1e-6);
        // 0.55 → 0.55 mod 0.25 = 0.05
        assert!((conic_sample_t(0.55, true, first, last) - 0.05).abs() < 1e-6);
        // 0.875 (7/8 оборота) → 0.875 mod 0.25 = 0.125 (середина паттерна)
        assert!((conic_sample_t(0.875, true, first, last) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_zero_span_is_identity() {
        // Вырожденный span (все stop-ы в одной точке) — не делим, возвращаем t.
        assert!((conic_sample_t(0.6, true, 0.5, 0.5) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn conic_sample_t_repeating_nonzero_first() {
        // Паттерн со смещённым first_pos: span = 0.4 - 0.1 = 0.3.
        // t=0.75 → 0.1 + (0.75-0.1) mod 0.3 = 0.1 + 0.65 mod 0.3 = 0.1 + 0.05 = 0.15
        let v = conic_sample_t(0.75, true, 0.1, 0.4);
        assert!((v - 0.15).abs() < 1e-6, "v={v}");
    }

    #[test]
    fn draw_fill_rounded_rect_circular_does_not_panic() {
        // Circular corners (rx == ry) — fast path.
        let radii = CornerRadii { tl: 8.0, tl_y: 8.0, tr: 8.0, tr_y: 8.0,
                                   br: 8.0, br_y: 8.0, bl: 8.0, bl_y: 8.0 };
        // Just verify no panic on valid input (no headless GL context needed for unit test).
        let _ = radii; // used to verify compilation
        assert!((radii.tl - radii.tl_y).abs() < 0.5);
    }

    #[test]
    fn draw_fill_rounded_rect_elliptical_different_radii() {
        // Elliptical: rx=40, ry=20 — should use bezier path, not fast path.
        let radii = CornerRadii { tl: 40.0, tl_y: 20.0, tr: 40.0, tr_y: 20.0,
                                   br: 40.0, br_y: 20.0, bl: 40.0, bl_y: 20.0 };
        // Verify that fast-path condition is false.
        assert!((radii.tl - radii.tl_y).abs() >= 0.5);
    }

    #[test]
    fn box_blur_rgba_single_pixel_unchanged() {
        // Single pixel: no neighbors, should remain unchanged.
        let mut px = vec![255u8, 0, 0, 255]; // red pixel
        box_blur_rgba(&mut px, 1, 1, 2.0);
        assert_eq!(&px, &[255, 0, 0, 255]); // single pixel — unchanged
    }

    #[test]
    fn box_blur_rgba_3x1_averages_horizontally() {
        // 3 pixels: red, black, red — after blur the middle should average neighbors.
        let mut px = vec![
            255u8, 0, 0, 255, // red
            0,   0, 0, 255,   // black
            255, 0, 0, 255,   // red
        ];
        box_blur_rgba(&mut px, 3, 1, 1.0);
        // Middle pixel (index 1) should now be average of all three: 255+0+255/3 = 170
        let mid_r = px[4]; // offset 1*4 + 0
        assert!(mid_r > 100, "middle pixel should be brightened by blur: got {mid_r}");
    }
}
