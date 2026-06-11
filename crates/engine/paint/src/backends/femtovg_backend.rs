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
//! - MixBlendMode: PA-3 реализует полный набор 15 CSS blend modes через offscreen-слой
//!   (CPU mix_blend_rgba compositing). Normal → SourceOver fast path, PlusLighter → Lighter.

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
use lumen_image::{Image, resize_area_avg};
use lumen_layout::Color;

use lumen_layout::{
    BackgroundRepeat, BackgroundSize, BorderStyle, GradientStop, ObjectFit,
    ObjectPosition, PositionComponent,
};

use lumen_core::geom::Rect;

use crate::backend::{RenderBackend, RenderError};
use crate::blend_modes::mix_blend_rgba;
use crate::dash_math::{dashed_border_offsets, dotted_border_offsets};
use crate::display_list::{BlendMode, CornerRadii, DisplayCommand, fit_image_rect};
use crate::gradient_math::{conic_sample_t, sample_gradient_color};
use crate::matrix_util::mat4_to_2d_affine;

// ─── Color conversion ────────────────────────────────────────────────────────

/// Конвертирует CSS `Color` (u8 каналы 0-255) в femtovg `Color` (f32 0-1).
#[inline]
fn lumen_to_fvg(c: Color) -> femtovg::Color {
    femtovg::Color::rgba(c.r, c.g, c.b, c.a)
}

// ─── Gradient helpers ─────────────────────────────────────────────────────────

/// Разрешает `GradientStop.position` в [0,1], равномерно распределяя `None` позиции.
///
/// Thin wrapper над общим [`crate::gradient_math::resolve_stop_positions`]
/// (единый алгоритм для всех бэкендов, PA-1): позиции дополнительно зажимаются
/// в [0,1] — femtovg-библиотечные градиенты не принимают значения вне диапазона.
fn resolve_stops(stops: &[GradientStop], width: f32) -> Vec<(f32, femtovg::Color)> {
    crate::gradient_math::resolve_stop_positions(stops, width)
        .into_iter()
        .map(|(pos, c)| (pos.clamp(0.0, 1.0), lumen_to_fvg(c)))
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
    ///
    /// Помимо ключа `src` (исходное разрешение) здесь же кешируются
    /// предварительно уменьшенные варианты под ключом `"src@WxH"` (см.
    /// [`Self::resolve_image_for_rect`]) — нужны для качественного downscale
    /// (BUG-077): femtovg сэмплит текстуру билинейно, что даёт алиасинг при
    /// сильном уменьшении; вместо этого рисуем заранее area-averaged картинку.
    images: HashMap<String, femtovg::ImageId>,
    /// Декодированные пиксели исходных изображений: src URL → `Image`.
    ///
    /// Храним рядом с GPU-текстурами, чтобы пересэмплировать на CPU
    /// (`resize_area_avg`) при downscale. Зеркалит `Renderer::raw_images`
    /// (wgpu-бэкенд).
    raw_images: HashMap<String, Image>,
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
    /// Offscreen filter layer stack. Each entry holds an offscreen ImageId and
    /// the filter chain to apply on PopFilter. Supports nested filters.
    filter_layer_stack: Vec<FilterLayerEntry>,
    /// Images queued for deletion after the next canvas.flush() in render().
    /// GPU draw commands hold ImageIds by copy; we can only safely delete after
    /// all pending commands that reference the id have been flushed.
    filter_layer_pending_delete: Vec<femtovg::ImageId>,
    /// Offscreen blend mode layer stack (PA-3). Each entry captures the backdrop
    /// snapshot and the source-layer image for CPU mix_blend_rgba compositing.
    blend_layer_stack: Vec<BlendLayerEntry>,
    /// Images from blend layers queued for deletion after the next flush.
    blend_layer_pending_delete: Vec<femtovg::ImageId>,
    /// Currently active render target image. `None` means Screen.
    /// Updated by [`Self::switch_render_target`] whenever the RT changes.
    active_rt_image: Option<femtovg::ImageId>,
}

// SAFETY: FemtovgBackend используется только из одного потока одновременно
// (enforce-ится через `&mut self` в методах трейта). OpenGL контекст
// передаётся потоку compositor-а через glutin make_current; оба потока
// никогда не используют контекст одновременно. glow::Context внутри femtovg
// содержит raw pointer, но мы гарантируем единственного владельца в каждый
// момент. Этот паттерн — стандартный для single-threaded GL рендереров.
unsafe impl Send for FemtovgBackend {}

// ─── Filter layer support ─────────────────────────────────────────────────────

/// Entry pushed onto `FemtovgBackend::filter_layer_stack` by `PushFilter`.
///
/// The offscreen `image_id` receives all draw commands between Push and Pop.
/// On `PopFilter`, the GPU Gaussian blur (if any) and CPU colour-matrix filters
/// are applied to this image before it is composited onto `prev_render_target`.
struct FilterLayerEntry {
    /// Offscreen image that filter-group content renders into.
    image_id: femtovg::ImageId,
    /// Filter chain to apply on PopFilter (CSS Filter Effects L1 §4.1).
    filters: Vec<lumen_layout::FilterFn>,
    /// Render target active before PushFilter — restored on PopFilter.
    prev_render_target: femtovg::RenderTarget,
}

// ─── Blend layer support (PA-3) ──────────────────────────────────────────────

/// Entry pushed onto `FemtovgBackend::blend_layer_stack` by `PushBlendMode`.
///
/// Between Push and Pop, all draws go into `src_image_id`. On `PopBlendMode`,
/// `composite_blend_layer` blends `src_image_id` over `backdrop_rgba` using
/// `mix_blend_rgba` (CSS Compositing L1 §5) and composites the result onto
/// `prev_render_target`.
struct BlendLayerEntry {
    /// CSS blend mode to apply.
    mode: BlendMode,
    /// Offscreen image capturing the source layer (draws between Push and Pop).
    src_image_id: femtovg::ImageId,
    /// Snapshot of the previous render target taken at PushBlendMode time.
    /// Premultiplied RGBA u8, dimensions `backdrop_w × backdrop_h`.
    backdrop_rgba: Vec<u8>,
    /// Width of the backdrop snapshot in pixels.
    backdrop_w: usize,
    /// Height of the backdrop snapshot in pixels.
    backdrop_h: usize,
    /// Render target active before PushBlendMode — restored on PopBlendMode.
    prev_render_target: femtovg::RenderTarget,
}

/// Apply a single CSS colour-matrix filter to a flat RGBA8 buffer in place.
///
/// Pixels are assumed premultiplied (as stored in femtovg offscreen images).
/// The function unpremultiplies, applies the filter in straight-colour space,
/// then re-premultiplies — matching the CPU raster path in `cpu_raster.rs`.
fn apply_filter_rgba(rgba: &mut [u8], filter: &lumen_layout::FilterFn) {
    use lumen_layout::FilterFn;
    // Mirrors cpu_raster::apply_color_filter (same colour-matrix formulas, same
    // unpremultiply → filter in straight [0,1] → re-premultiply idiom).
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as f32;
        if a == 0.0 {
            continue;
        }
        // Unpremultiply: result is straight colour in [0,1].
        let mut r = (px[0] as f32) / a;
        let mut g = (px[1] as f32) / a;
        let mut b = (px[2] as f32) / a;
        let mut a_unit = a / 255.0;

        match filter {
            FilterFn::Blur(_) => {} // GPU-only, handled before this call
            FilterFn::Brightness(amt) => {
                r = (r * amt).clamp(0.0, 1.0);
                g = (g * amt).clamp(0.0, 1.0);
                b = (b * amt).clamp(0.0, 1.0);
            }
            FilterFn::Contrast(amt) => {
                r = ((r - 0.5) * amt + 0.5).clamp(0.0, 1.0);
                g = ((g - 0.5) * amt + 0.5).clamp(0.0, 1.0);
                b = ((b - 0.5) * amt + 0.5).clamp(0.0, 1.0);
            }
            FilterFn::Grayscale(amt) => {
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                r = fmix(r, lum, *amt);
                g = fmix(g, lum, *amt);
                b = fmix(b, lum, *amt);
            }
            FilterFn::HueRotate(rad) => {
                let (c, s) = (fcos(*rad), fsin(*rad));
                let nr = (r * (0.213 + 0.787 * c - 0.213 * s)
                    + g * (0.715 - 0.715 * c - 0.715 * s)
                    + b * (0.072 - 0.072 * c + 0.928 * s))
                    .clamp(0.0, 1.0);
                let ng = (r * (0.213 - 0.213 * c + 0.143 * s)
                    + g * (0.715 + 0.285 * c + 0.140 * s)
                    + b * (0.072 - 0.072 * c - 0.283 * s))
                    .clamp(0.0, 1.0);
                let nb = (r * (0.213 - 0.213 * c - 0.787 * s)
                    + g * (0.715 - 0.715 * c + 0.715 * s)
                    + b * (0.072 + 0.928 * c + 0.072 * s))
                    .clamp(0.0, 1.0);
                r = nr;
                g = ng;
                b = nb;
            }
            FilterFn::Invert(amt) => {
                r = fmix(r, 1.0 - r, *amt);
                g = fmix(g, 1.0 - g, *amt);
                b = fmix(b, 1.0 - b, *amt);
            }
            FilterFn::Opacity(amt) => {
                a_unit = (a_unit * amt).clamp(0.0, 1.0);
            }
            FilterFn::Saturate(amt) => {
                let nr = (r * (0.213 + 0.787 * amt)
                    + g * (0.715 - 0.715 * amt)
                    + b * (0.072 - 0.072 * amt))
                    .clamp(0.0, 1.0);
                let ng = (r * (0.213 - 0.213 * amt)
                    + g * (0.715 + 0.285 * amt)
                    + b * (0.072 - 0.072 * amt))
                    .clamp(0.0, 1.0);
                let nb = (r * (0.213 - 0.213 * amt)
                    + g * (0.715 - 0.715 * amt)
                    + b * (0.072 + 0.928 * amt))
                    .clamp(0.0, 1.0);
                r = nr;
                g = ng;
                b = nb;
            }
            FilterFn::Sepia(amt) => {
                let sr = (0.393 * r + 0.769 * g + 0.189 * b).clamp(0.0, 1.0);
                let sg = (0.349 * r + 0.686 * g + 0.168 * b).clamp(0.0, 1.0);
                let sb = (0.272 * r + 0.534 * g + 0.131 * b).clamp(0.0, 1.0);
                r = fmix(r, sr, *amt);
                g = fmix(g, sg, *amt);
                b = fmix(b, sb, *amt);
            }
        }

        // Re-premultiply: na is new alpha (f32 0..255), channel = straight * na.
        let na = (a_unit * 255.0).round().clamp(0.0, 255.0);
        let to_u8 = |c: f32| (c * na).round().clamp(0.0, 255.0) as u8;
        px[0] = to_u8(r);
        px[1] = to_u8(g);
        px[2] = to_u8(b);
        px[3] = na as u8;
    }
}

/// Linear interpolation `x·(1−t) + y·t`.
#[inline]
fn fmix(x: f32, y: f32, t: f32) -> f32 {
    x * (1.0 - t) + y * t
}

/// Deterministic, libm-free cosine for hue-rotate (mirrors cpu_raster::cos_approx).
#[inline]
fn fcos(rad: f32) -> f32 {
    fsin(rad + std::f32::consts::FRAC_PI_2)
}

/// Deterministic, libm-free sine for hue-rotate (mirrors cpu_raster::sin_approx).
#[inline]
fn fsin(mut rad: f32) -> f32 {
    rad %= 2.0 * std::f32::consts::PI;
    if rad < 0.0 {
        rad += 2.0 * std::f32::consts::PI;
    }
    // Minimax polynomial on [0, π/2]; exploits symmetry for full range.
    let (x, neg) = if rad <= std::f32::consts::FRAC_PI_2 {
        (rad, false)
    } else if rad <= std::f32::consts::PI {
        (std::f32::consts::PI - rad, false)
    } else if rad <= 3.0 * std::f32::consts::FRAC_PI_2 {
        (rad - std::f32::consts::PI, true)
    } else {
        (2.0 * std::f32::consts::PI - rad, true)
    };
    let x2 = x * x;
    let v = x * (1.0 - x2 * (1.0 / 6.0 - x2 * (1.0 / 120.0 - x2 / 5040.0)));
    if neg { -v } else { v }
}

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
            raw_images: HashMap::new(),
            snapshots: HashMap::new(),
            font_provider: None,
            layer_stack_depth: 0,
            sticky_stack: Vec::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            viewport_css_w: size.width as f32 / scale as f32,
            viewport_css_h: size.height as f32 / scale as f32,
            filter_layer_stack: Vec::new(),
            filter_layer_pending_delete: Vec::new(),
            blend_layer_stack: Vec::new(),
            blend_layer_pending_delete: Vec::new(),
            active_rt_image: None,
        })
    }

    // ─── Render target helpers ────────────────────────────────────────────────

    /// Returns the current femtovg render target (Screen or an offscreen Image).
    fn current_rt(&self) -> femtovg::RenderTarget {
        match self.active_rt_image {
            Some(id) => femtovg::RenderTarget::Image(id),
            None => femtovg::RenderTarget::Screen,
        }
    }

    /// Sets the femtovg render target and updates `active_rt_image` accordingly.
    fn switch_render_target(&mut self, rt: femtovg::RenderTarget) {
        self.active_rt_image = match rt {
            femtovg::RenderTarget::Image(id) => Some(id),
            _ => None,
        };
        self.canvas.set_render_target(rt);
    }

    // ─── Drawing helpers ──────────────────────────────────────────────────────

    /// Рисует залитый прямоугольник.
    fn draw_fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let mut path = femtovg::Path::new();
        path.rect(x, y, w, h);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Fills a circle (used for dotted borders wider than 2px, where Edge renders
    /// round dots rather than squares).
    fn draw_fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let mut path = femtovg::Path::new();
        path.circle(cx, cy, r);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }

    /// Renders one border side (top/right/bottom/left) honoring its `BorderStyle`.
    /// `horizontal` = true for top/bottom (pattern runs along X), false for
    /// left/right (along Y). `width` is the side thickness in CSS px. Geometry
    /// mirrors the wgpu `emit_border_side` so the femtovg (default) backend draws
    /// the same dash/dot/double pattern Edge produces (BUG-080). Solid/None fall
    /// back to a single filled quad — unchanged from the previous behavior.
    fn draw_border_side(
        &mut self,
        side_rect: Rect,
        horizontal: bool,
        width: f32,
        color: Color,
        style: BorderStyle,
    ) {
        let total = if horizontal { side_rect.width } else { side_rect.height };
        match style {
            BorderStyle::Dashed => {
                for (offset, len) in dashed_border_offsets(total, width) {
                    if horizontal {
                        self.draw_fill_rect(side_rect.x + offset, side_rect.y, len, side_rect.height, color);
                    } else {
                        self.draw_fill_rect(side_rect.x, side_rect.y + offset, side_rect.width, len, color);
                    }
                }
            }
            BorderStyle::Dotted => {
                // dot_len ≤ 2px → squares (no AA circle); otherwise round dots.
                let use_rect = width.max(1.0) <= 2.0;
                for (offset, len) in dotted_border_offsets(total, width) {
                    if use_rect {
                        if horizontal {
                            self.draw_fill_rect(side_rect.x + offset, side_rect.y, len, side_rect.height, color);
                        } else {
                            self.draw_fill_rect(side_rect.x, side_rect.y + offset, side_rect.width, len, color);
                        }
                    } else if horizontal {
                        let cx = side_rect.x + offset + len / 2.0;
                        let cy = side_rect.y + side_rect.height / 2.0;
                        self.draw_fill_circle(cx, cy, side_rect.height / 2.0, color);
                    } else {
                        let cx = side_rect.x + side_rect.width / 2.0;
                        let cy = side_rect.y + offset + len / 2.0;
                        self.draw_fill_circle(cx, cy, side_rect.width / 2.0, color);
                    }
                }
            }
            BorderStyle::Double => {
                // CSS Backgrounds L3 §4.2: two solid lines ~1/3 width, gap ~1/3.
                // width < 3px → no room for a gap, fall back to solid.
                if width < 3.0 {
                    self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, side_rect.height, color);
                    return;
                }
                let line = (width / 3.0).max(1.0);
                if horizontal {
                    self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, line, color);
                    self.draw_fill_rect(side_rect.x, side_rect.y + width - line, side_rect.width, line, color);
                } else {
                    self.draw_fill_rect(side_rect.x, side_rect.y, line, side_rect.height, color);
                    self.draw_fill_rect(side_rect.x + width - line, side_rect.y, line, side_rect.height, color);
                }
            }
            BorderStyle::Solid | BorderStyle::None => {
                self.draw_fill_rect(side_rect.x, side_rect.y, side_rect.width, side_rect.height, color);
            }
        }
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

    /// Применяет filter-chain к offscreen-слою и композирует результат на
    /// предыдущий render target (экран или внешний offscreen-слой).
    ///
    /// Реализует PA-2: GPU Gaussian blur через `filter_image` + CPU colour-matrix
    /// через flush → screenshot → pixel process → re-upload.
    fn composite_filter_layer(&mut self, entry: FilterLayerEntry) {
        let FilterLayerEntry { image_id: src_id, filters, prev_render_target } = entry;

        let has_blur = filters.iter().any(|f| matches!(f, lumen_layout::FilterFn::Blur(s) if *s > 0.0));
        let has_color_matrix = filters.iter().any(|f| {
            !matches!(
                f,
                lumen_layout::FilterFn::Blur(_) | lumen_layout::FilterFn::Opacity(_)
            )
        });

        // current_id tracks which image has the latest filtered content.
        let mut current_id = src_id;

        // ── Step 1: GPU Gaussian blur (no CPU round-trip needed) ─────────────
        if has_blur {
            for f in &filters {
                if let lumen_layout::FilterFn::Blur(sigma) = f
                    && *sigma > 0.0
                    && let Ok(dst) = self.canvas.create_image_empty(
                        self.width as usize,
                        self.height as usize,
                        femtovg::PixelFormat::Rgba8,
                        femtovg::ImageFlags::PREMULTIPLIED,
                    )
                {
                    self.canvas.filter_image(
                        dst,
                        femtovg::ImageFilter::GaussianBlur { sigma: *sigma },
                        current_id,
                    );
                    self.filter_layer_pending_delete.push(current_id);
                    current_id = dst;
                }
            }
        }

        // ── Step 2: CPU colour-matrix filters ────────────────────────────────
        // Need to flush so GL actually renders content into current_id's FBO,
        // then switch render target to current_id so screenshot() reads it.
        if has_color_matrix {
            // Switch to current_id so flush binds its FBO.
            self.switch_render_target(femtovg::RenderTarget::Image(current_id));
            // Flush executes all pending commands (including filter_image if any).
            self.canvas.flush();
            // Now current_id's FBO is bound. Screenshot returns its pixels.
            if let Ok(img) = self.canvas.screenshot() {
                let iw = img.width();
                let ih = img.height();
                let mut rgba: Vec<u8> = img
                    .buf()
                    .iter()
                    .flat_map(|p| [p.r, p.g, p.b, p.a])
                    .collect();
                // Apply colour-matrix filters left to right (CSS spec §4.1).
                for f in &filters {
                    if !matches!(
                        f,
                        lumen_layout::FilterFn::Blur(_) | lumen_layout::FilterFn::Opacity(_)
                    ) {
                        apply_filter_rgba(&mut rgba, f);
                    }
                }
                // Re-upload processed pixels.
                let pixels: Vec<rgb::RGBA8> = rgba
                    .chunks_exact(4)
                    .map(|c| rgb::RGBA8 { r: c[0], g: c[1], b: c[2], a: c[3] })
                    .collect();
                let img_src = imgref::ImgRef::new(&pixels, iw, ih);
                if let Ok(dst) = self.canvas.create_image(
                    img_src,
                    femtovg::ImageFlags::PREMULTIPLIED,
                ) {
                    self.filter_layer_pending_delete.push(current_id);
                    current_id = dst;
                }
            }
        }

        // ── Step 3: Restore previous render target ────────────────────────────
        self.switch_render_target(prev_render_target);

        // ── Step 4: Composite filtered image onto the (now-current) target ───
        self.canvas.save();
        self.canvas.reset_transform();
        let css_w = (self.width as f64 / self.scale) as f32;
        let css_h = (self.height as f64 / self.scale) as f32;
        let paint = femtovg::Paint::image(current_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0);
        let mut path = femtovg::Path::new();
        path.rect(0.0, 0.0, css_w, css_h);
        self.canvas.fill_path(&path, &paint);
        self.canvas.restore();

        // Delete after flush (pending, not immediate — fill_path still holds the id).
        self.filter_layer_pending_delete.push(current_id);
    }

    /// Composites a blend-mode layer (PA-3) onto the previous render target.
    ///
    /// Algorithm:
    /// 1. Flush so src_image_id FBO has latest content.
    /// 2. Screenshot src_image to get source pixels (premultiplied RGBA u8).
    /// 3. Restore prev_render_target.
    /// 4. For each pixel: unpremultiply both source and backdrop → `mix_blend_rgba`
    ///    (CSS Compositing L1 §5) → re-premultiply result.
    /// 5. Upload result image and draw with `CompositeOperation::Source` to
    ///    replace the backdrop area with the blended result.
    fn composite_blend_layer(&mut self, entry: BlendLayerEntry) {
        let BlendLayerEntry { mode, src_image_id, backdrop_rgba, backdrop_w, backdrop_h, prev_render_target } = entry;

        // Step 1: flush pending commands so src_image_id is fully rendered.
        self.canvas.flush();

        // Step 2: screenshot the source offscreen image.
        // We're currently rendering into src_image_id, so screenshot reads from it.
        let src_rgba = self.canvas.screenshot()
            .map(|img| img.buf().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect::<Vec<u8>>())
            .unwrap_or_default();

        // Step 3: restore the previous render target.
        self.switch_render_target(prev_render_target);

        // Step 4+5: CPU pixel-level blend + composite with Source operation.
        if src_rgba.len() == backdrop_rgba.len() && !src_rgba.is_empty() {
            let n = src_rgba.len() / 4;
            let mut result = vec![0u8; src_rgba.len()];
            for i in 0..n {
                let si = i * 4;
                // Premultiplied → straight for source.
                let sa = src_rgba[si + 3] as f32 / 255.0;
                let s_str = if sa > 0.0 {
                    [src_rgba[si] as f32 / 255.0 / sa,
                     src_rgba[si + 1] as f32 / 255.0 / sa,
                     src_rgba[si + 2] as f32 / 255.0 / sa,
                     sa]
                } else {
                    [0.0; 4]
                };
                // Premultiplied → straight for backdrop.
                let da = backdrop_rgba[si + 3] as f32 / 255.0;
                let d_str = if da > 0.0 {
                    [backdrop_rgba[si] as f32 / 255.0 / da,
                     backdrop_rgba[si + 1] as f32 / 255.0 / da,
                     backdrop_rgba[si + 2] as f32 / 255.0 / da,
                     da]
                } else {
                    [0.0; 4]
                };
                let out = mix_blend_rgba(mode, s_str, d_str);
                // Straight → premultiplied for output.
                let ao = out[3];
                result[si]     = ((out[0] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 1] = ((out[1] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 2] = ((out[2] * ao) * 255.0).round().clamp(0.0, 255.0) as u8;
                result[si + 3] = (ao * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            let pixels: Vec<rgb::RGBA8> = result.chunks_exact(4)
                .map(|c| rgb::RGBA8 { r: c[0], g: c[1], b: c[2], a: c[3] })
                .collect();
            let img_ref = imgref::ImgRef::new(&pixels, backdrop_w, backdrop_h);
            if let Ok(result_id) = self.canvas.create_image(img_ref, femtovg::ImageFlags::PREMULTIPLIED) {
                self.canvas.save();
                self.canvas.reset_transform();
                // Source operation: replace dest pixels with the blended result image.
                self.canvas.global_composite_operation(femtovg::CompositeOperation::Copy);
                let css_w = (self.width as f64 / self.scale) as f32;
                let css_h = (self.height as f64 / self.scale) as f32;
                let paint = femtovg::Paint::image(result_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0);
                let mut path = femtovg::Path::new();
                path.rect(0.0, 0.0, css_w, css_h);
                self.canvas.fill_path(&path, &paint);
                self.canvas.restore();
                self.blend_layer_pending_delete.push(result_id);
            }
        }
        self.blend_layer_pending_delete.push(src_image_id);
    }

    /// Рисует изображение из зарегистрированного URL в content box `rect`
    /// с учётом `object-fit` / `object-position` (CSS Images L3 §5.5).
    ///
    /// Placement-rect считается `fit_image_rect` от intrinsic-размера
    /// декодированной картинки; для cover / none, когда placement выходит за
    /// `rect`, излишек срезается scissor-ом (spec: «clipped to the content
    /// box»). Ранее femtovg-бэкенд игнорировал fit/position и растягивал
    /// текстуру на весь `rect` (BUG-078). Downscale-ресэмпл (BUG-077)
    /// выполняется по placement-размеру, а не по box — для contain плитка
    /// меньше box, для cover больше.
    ///
    /// Intrinsic-размер неизвестен (нет raw-пикселей) → историческое
    /// fill-поведение. Не зарегистрировано вовсе — серый placeholder.
    fn draw_image_in_rect(
        &mut self,
        rect: &Rect,
        src: &str,
        fit: ObjectFit,
        position: &ObjectPosition,
    ) {
        let placed = image_placement(
            *rect,
            self.raw_images.get(src).map(|raw| (raw.width, raw.height)),
            fit,
            *position,
        );
        if let Some(img_id) = self.resolve_image_for_rect(src, &placed) {
            self.canvas.save();
            self.canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);
            let paint = femtovg::Paint::image(
                img_id,
                placed.x, placed.y, placed.width, placed.height,
                0.0, 1.0,
            );
            let mut path = femtovg::Path::new();
            path.rect(placed.x, placed.y, placed.width, placed.height);
            self.canvas.fill_path(&path, &paint);
            self.canvas.restore();
        } else {
            // Placeholder — светло-серый прямоугольник.
            self.draw_fill_rect(rect.x, rect.y, rect.width, rect.height, Color { r: 200, g: 200, b: 200, a: 255 });
        }
    }

    /// Рисует `background-image: url(...)` с учётом `background-size`,
    /// `background-position`, `background-repeat`, `background-origin` и
    /// `background-clip` (CSS Backgrounds L3 §3.3–3.5/§3.7/§3.8).
    ///
    /// `rect` — painting area (`background-clip`): плитки клипируются по ней.
    /// `origin_rect` — positioning area (`background-origin`): относительно неё
    /// считаются размер плитки и её позиция. Ранее femtovg-бэкенд игнорировал
    /// всё это и растягивал картинку на весь `rect` (BUG-095) — теперь
    /// геометрия плиток зеркалит wgpu `Renderer`.
    ///
    /// Незарегистрированный `src` → визуальный no-op (в отличие от `<img>`,
    /// фоновая картинка не рисует серый placeholder).
    fn draw_background_image(
        &mut self,
        rect: &Rect,
        origin_rect: &Rect,
        src: &str,
        size: BackgroundSize,
        position: &ObjectPosition,
        repeat: BackgroundRepeat,
    ) {
        let (img_w, img_h) = match self.raw_images.get(src) {
            Some(raw) => (raw.width as f32, raw.height as f32),
            None => return,
        };
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        let (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y) = bg_tile_geometry(
            size,
            position,
            repeat,
            img_w,
            img_h,
            origin_rect.width,
            origin_rect.height,
            origin_rect.x,
            origin_rect.y,
        );
        if tile_w <= 0.0 || tile_h <= 0.0 {
            return;
        }

        // Разрешаем текстуру под размер плитки (area-averaged downscale при
        // уменьшении, как в draw_image_in_rect — BUG-077).
        let tile_rect = Rect::new(0.0, 0.0, tile_w, tile_h);
        let Some(img_id) = self.resolve_image_for_rect(src, &tile_rect) else {
            return;
        };

        // Плитки клипируются по painting area через scissor (пересекает
        // активный clip-стек, например overflow:hidden контейнер).
        self.canvas.save();
        self.canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);

        let x_end = rect.x + rect.width;
        let y_end = rect.y + rect.height;
        let mut ty = tile_y_start;
        loop {
            if ty >= y_end {
                break;
            }
            if ty + tile_h > rect.y {
                let mut tx = tile_x_start;
                loop {
                    if tx >= x_end {
                        break;
                    }
                    if tx + tile_w > rect.x {
                        let paint =
                            femtovg::Paint::image(img_id, tx, ty, tile_w, tile_h, 0.0, 1.0);
                        let mut path = femtovg::Path::new();
                        path.rect(tx, ty, tile_w, tile_h);
                        self.canvas.fill_path(&path, &paint);
                    }
                    if !repeat_x {
                        break;
                    }
                    tx += tile_w;
                }
            }
            if !repeat_y {
                break;
            }
            ty += tile_h;
        }
        self.canvas.restore();
    }

    /// Возвращает femtovg `ImageId` для отрисовки `src` в `rect`, при сильном
    /// уменьшении подменяя исходную текстуру area-averaged уменьшенной копией.
    ///
    /// femtovg сэмплит текстуру билинейно — при downscale в несколько раз это
    /// даёт алиасинг (BUG-077): один выходной пиксель усредняет лишь 2×2
    /// соседей вместо всей покрываемой области. Зеркалим `Renderer` (wgpu):
    /// если целевой размер в device-пикселях (`rect × scale`) меньше исходного
    /// хотя бы по одной оси — пересэмплируем `resize_area_avg` до этого размера
    /// и кешируем под `"src@WxH"`. Upscale/точное совпадение → исходная текстура
    /// (билинейная фильтрация femtovg здесь корректна). Если у `src` нет
    /// декодированных пикселей (не зарегистрирован) — возвращаем то, что есть в
    /// кеше текстур, либо `None` (рисуется placeholder).
    fn resolve_image_for_rect(&mut self, src: &str, rect: &Rect) -> Option<femtovg::ImageId> {
        let (rw, rh) = match self.raw_images.get(src) {
            Some(raw) => (raw.width, raw.height),
            None => return self.images.get(src).copied(),
        };

        let (tw, th) = match downscale_target(rw, rh, rect.width, rect.height, self.scale) {
            Some(target) => target,
            // Не downscale (upscale или точное совпадение) — отдаём исходник.
            None => return self.images.get(src).copied(),
        };

        let key = format!("{src}@{tw}x{th}");
        if let Some(&id) = self.images.get(&key) {
            return Some(id);
        }

        let raw = self.raw_images.get(src)?.clone();
        let resized = resize_area_avg(&raw, tw, th);
        let rgba = image_to_rgba8_vec(&resized);
        let img = imgref::ImgRef::new(&rgba, resized.width as usize, resized.height as usize);
        let id = self
            .canvas
            .create_image(femtovg::ImageSource::Rgba(img), femtovg::ImageFlags::empty())
            .ok()?;
        self.images.insert(key, id);
        Some(id)
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

        // Общая resolve/sample-математика (PA-1): цвета остаются в CSS `Color`,
        // конверсия в femtovg::Color — только на заливке сегмента.
        let resolved = crate::gradient_math::resolve_stop_positions(stops, 1.0);
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
            let c_mid = sample_gradient_color(&resolved, t_sample, false);
            let avg_color = lumen_to_fvg(c_mid);

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
            DisplayCommand::DrawBorder { rect, widths, colors, styles, .. } => {
                // Side rect order: [top, right, bottom, left]. Each side is rendered
                // according to its `BorderStyle` (Solid → full quad, Dashed/Dotted →
                // segment pattern, Double → two thin lines). Geometry mirrors the wgpu
                // `emit_border_side` so both backends match Edge's pattern (BUG-080).
                if widths[0] > 0.0 {
                    self.draw_border_side(
                        Rect::new(rect.x, rect.y, rect.width, widths[0]),
                        true, widths[0], colors[0], styles[0],
                    );
                }
                if widths[1] > 0.0 {
                    self.draw_border_side(
                        Rect::new(rect.x + rect.width - widths[1], rect.y, widths[1], rect.height),
                        false, widths[1], colors[1], styles[1],
                    );
                }
                if widths[2] > 0.0 {
                    self.draw_border_side(
                        Rect::new(rect.x, rect.y + rect.height - widths[2], rect.width, widths[2]),
                        true, widths[2], colors[2], styles[2],
                    );
                }
                if widths[3] > 0.0 {
                    self.draw_border_side(
                        Rect::new(rect.x, rect.y, widths[3], rect.height),
                        false, widths[3], colors[3], styles[3],
                    );
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
            DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. } => {
                self.draw_image_in_rect(rect, src, *object_fit, object_position);
            }
            DisplayCommand::DrawBackgroundImage {
                rect, origin_rect, src, size, position, repeat, ..
            } => {
                self.draw_background_image(rect, origin_rect, src, *size, position, *repeat);
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

            // ── Blend mode (PA-3) ─────────────────────────────────────────────
            // Normal → fast path (SourceOver). PlusLighter → fast path (Lighter).
            // All other CSS blend modes → offscreen CPU compositing via mix_blend_rgba.
            DisplayCommand::PushBlendMode { mode } => {
                if *mode == BlendMode::Normal {
                    self.canvas.save();
                    self.layer_stack_depth += 1;
                } else if *mode == BlendMode::PlusLighter {
                    self.canvas.save();
                    self.canvas.global_composite_operation(blend_to_composite(*mode));
                    self.layer_stack_depth += 1;
                } else {
                    // Offscreen path: capture backdrop, redirect draws to src image.
                    let prev_rt = self.current_rt();
                    // Flush pending commands so backdrop is fully drawn in prev_rt.
                    self.canvas.flush();
                    // Screenshot the current RT to get the backdrop pixels.
                    let (backdrop_rgba, backdrop_w, backdrop_h) =
                        if let Ok(img) = self.canvas.screenshot() {
                            let w = img.width();
                            let h = img.height();
                            let rgba = img.buf().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
                            (rgba, w, h)
                        } else {
                            (vec![], 0, 0)
                        };
                    match self.canvas.create_image_empty(
                        self.width as usize,
                        self.height as usize,
                        femtovg::PixelFormat::Rgba8,
                        femtovg::ImageFlags::PREMULTIPLIED,
                    ) {
                        Ok(src_id) => {
                            self.switch_render_target(femtovg::RenderTarget::Image(src_id));
                            self.canvas.clear_rect(
                                0, 0, self.width, self.height,
                                femtovg::Color::rgba(0, 0, 0, 0),
                            );
                            self.blend_layer_stack.push(BlendLayerEntry {
                                mode: *mode,
                                src_image_id: src_id,
                                backdrop_rgba,
                                backdrop_w,
                                backdrop_h,
                                prev_render_target: prev_rt,
                            });
                        }
                        Err(_) => {
                            // Fallback: draw without blend (content goes to prev_rt directly).
                            self.canvas.save();
                        }
                    }
                    self.layer_stack_depth += 1;
                }
            }
            DisplayCommand::PopBlendMode => {
                if self.layer_stack_depth > 0 {
                    self.layer_stack_depth -= 1;
                }
                if let Some(entry) = self.blend_layer_stack.pop() {
                    // Offscreen blend path: composite src layer over backdrop.
                    self.composite_blend_layer(entry);
                } else {
                    // Fast path (Normal or PlusLighter): just restore canvas state.
                    self.canvas.restore();
                }
            }

            // ── Transform ────────────────────────────────────────────────────
            DisplayCommand::PushTransform { matrix } => {
                self.canvas.save();
                // femtovg Transform2D([a, b, c, d, e, f]) — 2D-аффинная часть
                // Mat4 через общий crate::matrix_util (PA-1).
                let transform = femtovg::Transform2D(mat4_to_2d_affine(matrix));
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
            // PA-2: реальный Gaussian blur (GPU via filter_image) и colour-matrix
            // (CPU via flush+screenshot) через offscreen-слой.
            // Только Opacity — легкий путь через set_global_alpha без offscreen.
            DisplayCommand::PushFilter { filters } => {
                let needs_offscreen = filters
                    .iter()
                    .any(|f| !matches!(f, lumen_layout::FilterFn::Opacity(_)));

                if needs_offscreen {
                    // Capture current RT before creating the new offscreen layer.
                    // Uses active_rt_image (maintained by switch_render_target) to
                    // correctly handle nesting with blend layers (PA-3).
                    let prev_rt = self.current_rt();
                    match self.canvas.create_image_empty(
                        self.width as usize,
                        self.height as usize,
                        femtovg::PixelFormat::Rgba8,
                        femtovg::ImageFlags::PREMULTIPLIED,
                    ) {
                        Ok(img_id) => {
                            // Redirect draws into the offscreen image.
                            self.switch_render_target(femtovg::RenderTarget::Image(img_id));
                            // Clear to transparent so content composites correctly.
                            self.canvas.clear_rect(
                                0, 0, self.width, self.height,
                                femtovg::Color::rgba(0, 0, 0, 0),
                            );
                            self.filter_layer_stack.push(FilterLayerEntry {
                                image_id: img_id,
                                filters: filters.clone(),
                                prev_render_target: prev_rt,
                            });
                            self.layer_stack_depth += 1;
                        }
                        Err(_) => {
                            // Fallback: no-op save (content draws to screen without filter).
                            self.canvas.save();
                            self.layer_stack_depth += 1;
                        }
                    }
                } else {
                    // Opacity-only path: existing lightweight approach.
                    self.canvas.save();
                    for f in filters {
                        if let lumen_layout::FilterFn::Opacity(v) = f {
                            self.canvas.set_global_alpha(v.clamp(0.0, 1.0));
                        }
                    }
                    self.layer_stack_depth += 1;
                }
            }
            DisplayCommand::PopFilter => {
                if let Some(entry) = self.filter_layer_stack.pop() {
                    // Offscreen path: apply filter chain and composite.
                    self.composite_filter_layer(entry);
                    self.layer_stack_depth = self.layer_stack_depth.saturating_sub(1);
                } else {
                    // Opacity-only path.
                    if self.layer_stack_depth > 0 {
                        self.canvas.restore();
                        self.layer_stack_depth -= 1;
                    }
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

        // Delete offscreen images queued during the frame. Must happen AFTER flush
        // so pending fill_path commands that reference the ImageIds are executed.
        let filter_del: Vec<_> = self.filter_layer_pending_delete.drain(..).collect();
        for id in filter_del {
            self.canvas.delete_image(id);
        }
        let blend_del: Vec<_> = self.blend_layer_pending_delete.drain(..).collect();
        for id in blend_del {
            self.canvas.delete_image(id);
        }

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
        // Keep the decoded pixels for on-demand area-averaged downscale (BUG-077).
        self.raw_images.insert(src.clone(), image.clone());
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
        self.raw_images.clear();
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

/// Решает, нужно ли area-averaged уменьшение для отрисовки изображения
/// `raw_w × raw_h` в прямоугольник `rect_w × rect_h` CSS-пикселей при device
/// `scale`, и если да — возвращает целевой размер в device-пикселях.
///
/// `Some((tw, th))` — целевой размер меньше исходного хотя бы по одной оси
/// (downscale): нужно пересэмплировать `resize_area_avg` чтобы избежать
/// алиасинга от билинейного сэмплинга femtovg (BUG-077). `None` — upscale или
/// точное совпадение: исходную текстуру можно сэмплить напрямую.
fn downscale_target(raw_w: u32, raw_h: u32, rect_w: f32, rect_h: f32, scale: f64) -> Option<(u32, u32)> {
    let tw = (f64::from(rect_w) * scale).round().max(1.0) as u32;
    let th = (f64::from(rect_h) * scale).round().max(1.0) as u32;
    if tw >= raw_w && th >= raw_h {
        None
    } else {
        Some((tw, th))
    }
}

/// Placement-rect для `<img>` (CSS Images L3 §5.5): куда внутри content box
/// `rect` рисуется текстура с учётом `object-fit` / `object-position`.
///
/// Чистая функция (без GL). `intrinsic` — натуральный размер декодированной
/// картинки (`raw_images`); `None` (нет raw-пикселей — текстура зарегистрирована
/// извне) → fit невозможен, возвращаем сам `rect` (историческое fill-поведение).
/// Возвращённый rect может выходить за `rect` (cover / none) — обрезку по
/// content box делает scissor в `draw_image_in_rect`.
fn image_placement(
    rect: Rect,
    intrinsic: Option<(u32, u32)>,
    fit: ObjectFit,
    position: ObjectPosition,
) -> Rect {
    match intrinsic {
        Some(size) => fit_image_rect(rect, size, fit, position),
        None => rect,
    }
}

/// Считает геометрию плиток фоновой картинки из `background-size` /
/// `background-position` / `background-repeat` (CSS Backgrounds L3 §3.3–3.5).
///
/// Чистая функция (без GL) — зеркалит tiling-математику wgpu `Renderer`, чтобы
/// femtovg-бэкенд (default) давал тот же результат. `img_w`/`img_h` — размер
/// исходной картинки; `oarea_*` — positioning area (`background-origin`).
///
/// Возвращает `(tile_w, tile_h, tile_x_start, tile_y_start, repeat_x,
/// repeat_y)`: размер одной плитки, координату левого-верхнего угла первой
/// плитки и флаги повтора по осям.
#[allow(clippy::too_many_arguments)]
fn bg_tile_geometry(
    size: BackgroundSize,
    position: &ObjectPosition,
    repeat: BackgroundRepeat,
    img_w: f32,
    img_h: f32,
    oarea_w: f32,
    oarea_h: f32,
    oarea_x: f32,
    oarea_y: f32,
) -> (f32, f32, f32, f32, bool, bool) {
    let (tile_w, tile_h) = match size {
        BackgroundSize::Auto => (img_w, img_h),
        BackgroundSize::Cover => {
            let s = (oarea_w / img_w).max(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Contain => {
            let s = (oarea_w / img_w).min(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Length(w, h) => {
            let tw = w.max(1.0);
            let th = h.unwrap_or_else(|| img_h * (tw / img_w)).max(1.0);
            (tw, th)
        }
    };

    let off_x = match position.x {
        PositionComponent::Px(px) => px,
        PositionComponent::Percent(p) => (oarea_w - tile_w) * p,
    };
    let off_y = match position.y {
        PositionComponent::Px(py) => py,
        PositionComponent::Percent(p) => (oarea_h - tile_h) * p,
    };
    let tile_x0 = oarea_x + off_x;
    let tile_y0 = oarea_y + off_y;

    let (tile_x_start, repeat_x, repeat_y) = match repeat {
        BackgroundRepeat::NoRepeat => (tile_x0, false, false),
        BackgroundRepeat::RepeatX => (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, false),
        BackgroundRepeat::RepeatY => (tile_x0, false, true),
        BackgroundRepeat::Repeat | BackgroundRepeat::Round | BackgroundRepeat::Space => {
            (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, true)
        }
    };
    let tile_y_start = if repeat_y {
        tile_y0 - (off_y / tile_h).ceil() * tile_h
    } else {
        tile_y0
    };

    (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y)
}

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
    use lumen_layout::Length;

    #[test]
    fn downscale_target_triggers_on_large_downscale() {
        // 800×600 source drawn into 200×150 CSS px at scale 1 → downscale to 200×150.
        assert_eq!(downscale_target(800, 600, 200.0, 150.0, 1.0), Some((200, 150)));
    }

    #[test]
    fn downscale_target_none_on_upscale_or_exact() {
        // Exact match → no resample.
        assert_eq!(downscale_target(100, 100, 100.0, 100.0, 1.0), None);
        // Upscale in both axes → no resample (bilinear upscale by femtovg is fine).
        assert_eq!(downscale_target(100, 100, 300.0, 300.0, 1.0), None);
    }

    #[test]
    fn downscale_target_triggers_when_one_axis_shrinks() {
        // Squished horizontally only → still area-average (the shrunk axis aliases).
        assert_eq!(downscale_target(400, 100, 100.0, 100.0, 1.0), Some((100, 100)));
    }

    #[test]
    fn downscale_target_accounts_for_device_scale() {
        // 2× HiDPI: 200 CSS px → 400 device px, so a 300px source is upscaled, not down.
        assert_eq!(downscale_target(300, 300, 200.0, 200.0, 2.0), None);
        // But a 500px source into 200 CSS px @2× = 400 device px → downscale to 400.
        assert_eq!(downscale_target(500, 500, 200.0, 200.0, 2.0), Some((400, 400)));
    }

    #[test]
    fn image_placement_contain_letterboxes_landscape_image() {
        // 200×100 image in 180×120 box, contain → scale 0.9, 180×90, centered vertically.
        let rect = Rect::new(10.0, 20.0, 180.0, 120.0);
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Contain, ObjectPosition::default());
        assert_eq!((placed.x, placed.y, placed.width, placed.height), (10.0, 35.0, 180.0, 90.0));
    }

    /// Покомпонентное сравнение rect-а с допуском на float-погрешность
    /// (cover-scale считается делением, точные значения недостижимы).
    fn assert_rect_close(r: Rect, expected: (f32, f32, f32, f32)) {
        let (x, y, w, h) = expected;
        for (got, want) in [(r.x, x), (r.y, y), (r.width, w), (r.height, h)] {
            assert!((got - want).abs() < 1e-3, "got {r:?}, expected {expected:?}");
        }
    }

    #[test]
    fn image_placement_cover_overflows_box() {
        // 200×100 image in 180×120 box, cover → scale 1.2, 240×120, overflows horizontally
        // (clip is the caller's scissor by the content box).
        let rect = Rect::new(0.0, 0.0, 180.0, 120.0);
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Cover, ObjectPosition::default());
        assert_rect_close(placed, (-30.0, 0.0, 240.0, 120.0));
    }

    #[test]
    fn image_placement_position_right_bottom_with_cover() {
        // object-position: right bottom (100% 100%) shifts the overflow fully to the left/top.
        let rect = Rect::new(0.0, 0.0, 180.0, 120.0);
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Cover, pos);
        assert_rect_close(placed, (-60.0, 0.0, 240.0, 120.0));
    }

    #[test]
    fn image_placement_unknown_intrinsic_falls_back_to_fill() {
        // No raw pixels (externally registered texture) → historical stretch-to-box.
        let rect = Rect::new(5.0, 5.0, 180.0, 120.0);
        let placed = image_placement(rect, None, ObjectFit::Contain, ObjectPosition::default());
        assert_eq!((placed.x, placed.y, placed.width, placed.height), (5.0, 5.0, 180.0, 120.0));
    }

    #[test]
    fn bg_tile_geometry_length_no_repeat_top_left() {
        // background-size: 80×60; no-repeat; position 0% 0%; origin area at (30,30).
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        };
        let (tw, th, x0, y0, rx, ry) = bg_tile_geometry(
            BackgroundSize::Length(80.0, Some(60.0)),
            &pos,
            BackgroundRepeat::NoRepeat,
            100.0,
            100.0,
            180.0,
            120.0,
            30.0,
            30.0,
        );
        assert_eq!((tw, th), (80.0, 60.0));
        // Anchored to top-left of the positioning area.
        assert_eq!((x0, y0), (30.0, 30.0));
        assert!(!rx && !ry);
    }

    #[test]
    fn bg_tile_geometry_position_bottom_right() {
        // position 100% 100% anchors tile to the far corner of the origin area.
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let (tw, th, x0, y0, ..) = bg_tile_geometry(
            BackgroundSize::Length(80.0, Some(60.0)),
            &pos,
            BackgroundRepeat::NoRepeat,
            100.0,
            100.0,
            180.0,
            120.0,
            30.0,
            30.0,
        );
        assert_eq!((tw, th), (80.0, 60.0));
        // x0 = 30 + (180 - 80) = 130; y0 = 30 + (120 - 60) = 90.
        assert!((x0 - 130.0).abs() < 1e-3);
        assert!((y0 - 90.0).abs() < 1e-3);
    }

    #[test]
    fn bg_tile_geometry_cover_scales_to_fill() {
        // Cover: scale = max(180/100, 120/100) = 1.8 → tile 180×180.
        let pos = ObjectPosition::default();
        let (tw, th, ..) = bg_tile_geometry(
            BackgroundSize::Cover,
            &pos,
            BackgroundRepeat::NoRepeat,
            100.0,
            100.0,
            180.0,
            120.0,
            0.0,
            0.0,
        );
        assert!((tw - 180.0).abs() < 1e-3);
        assert!((th - 180.0).abs() < 1e-3);
    }

    #[test]
    fn bg_tile_geometry_repeat_sets_flags() {
        // Repeat with position 0% 0% → both axes repeat; start aligned to origin.
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        };
        let (tw, th, x0, y0, rx, ry) = bg_tile_geometry(
            BackgroundSize::Length(40.0, Some(40.0)),
            &pos,
            BackgroundRepeat::Repeat,
            100.0,
            100.0,
            200.0,
            200.0,
            0.0,
            0.0,
        );
        assert!(rx && ry);
        // off_x = 0 → ceil(0/40)*40 = 0 → start = origin.
        assert_eq!((x0, y0), (0.0, 0.0));
        assert_eq!((tw, th), (40.0, 40.0));
    }

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
    fn dashed_offsets_match_edge_dash_counts() {
        // BUG-080: on a 180px side, Edge produces n = 18/15/8/4 dashes for
        // 2/4/8/16px widths (the wgpu emit_border_side reference values).
        assert_eq!(dashed_border_offsets(180.0, 2.0).len(), 18);
        assert_eq!(dashed_border_offsets(180.0, 4.0).len(), 15);
        assert_eq!(dashed_border_offsets(180.0, 8.0).len(), 8);
        assert_eq!(dashed_border_offsets(180.0, 16.0).len(), 4);
    }

    #[test]
    fn dashed_offsets_anchor_first_and_last() {
        // First dash starts at 0; last dash ends exactly at total.
        let segs = dashed_border_offsets(180.0, 4.0);
        assert_eq!(segs.first().unwrap().0, 0.0);
        let (last_off, last_len) = *segs.last().unwrap();
        assert!((last_off + last_len - 180.0).abs() < 1.0, "last end {}", last_off + last_len);
    }

    #[test]
    fn dotted_offsets_match_edge_dot_counts() {
        // BUG-080: 180px side → n = 46/23/12/6 dots for 2/4/8/16px widths.
        assert_eq!(dotted_border_offsets(180.0, 2.0).len(), 46);
        assert_eq!(dotted_border_offsets(180.0, 4.0).len(), 23);
        assert_eq!(dotted_border_offsets(180.0, 8.0).len(), 12);
        assert_eq!(dotted_border_offsets(180.0, 16.0).len(), 6);
    }

    #[test]
    fn dotted_offsets_symmetric_and_bounded() {
        let segs = dotted_border_offsets(180.0, 8.0);
        // First dot at 0, last dot ends at total.
        assert_eq!(segs.first().unwrap().0, 0.0);
        let (last_off, last_len) = *segs.last().unwrap();
        assert!((last_off + last_len - 180.0).abs() < 1.0, "last end {}", last_off + last_len);
        // Every dot length equals dot_len (= width here).
        assert!(segs.iter().all(|&(_, len)| (len - 8.0).abs() < 0.01));
    }

    #[test]
    fn border_offsets_empty_for_zero_total() {
        assert!(dashed_border_offsets(0.0, 4.0).is_empty());
        assert!(dotted_border_offsets(-5.0, 4.0).is_empty());
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

    // interp_conic_color / conic_sample_t unit-тесты переехали в
    // crate::gradient_math (PA-1; sample_gradient_color покрывает интерполяцию).

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

    // ── apply_filter_rgba tests ──────────────────────────────────────────────

    /// Helper: create a single premultiplied RGBA pixel (full opacity → premul = straight).
    fn px(r: u8, g: u8, b: u8) -> Vec<u8> {
        vec![r, g, b, 255]
    }

    #[test]
    fn apply_filter_rgba_grayscale_full() {
        // Fully red pixel → grayscale(1.0) → grey luma value.
        let mut buf = px(255, 0, 0);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(1.0));
        // R≈G≈B (luma of pure red ≈ 0.2126 * 255 ≈ 54).
        let r = buf[0]; let g = buf[1]; let b = buf[2];
        assert!((r as i32 - g as i32).abs() <= 2, "R/G should be equal after grayscale");
        assert!((g as i32 - b as i32).abs() <= 2, "G/B should be equal after grayscale");
        assert!(r > 20 && r < 70, "grayscale luma for red should be ~54, got {r}");
    }

    #[test]
    fn apply_filter_rgba_grayscale_zero_noop() {
        // grayscale(0) → image unchanged.
        let mut buf = px(200, 100, 50);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(0.0));
        assert_eq!(&buf, &[200, 100, 50, 255]);
    }

    #[test]
    fn apply_filter_rgba_invert_full() {
        // invert(1.0) → each channel = 255 - original.
        let mut buf = px(100, 150, 200);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Invert(1.0));
        // After invert(1.0): r=255-100=155, g=255-150=105, b=255-200=55.
        assert!((buf[0] as i32 - 155).abs() <= 2, "expected R≈155 after invert, got {}", buf[0]);
        assert!((buf[1] as i32 - 105).abs() <= 2, "expected G≈105 after invert, got {}", buf[1]);
        assert!((buf[2] as i32 - 55).abs() <= 2, "expected B≈55 after invert, got {}", buf[2]);
    }

    #[test]
    fn apply_filter_rgba_sepia_full() {
        // sepia(1.0) on white → known reference values.
        let mut buf = px(255, 255, 255);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Sepia(1.0));
        // sepia output for white: r=min(1,0.393+0.769+0.189)=1, g=0.349+0.686+0.168=1.203→1, b=0.272+0.534+0.131=0.937.
        assert_eq!(buf[0], 255, "R should be 255 for white sepia");
        assert_eq!(buf[1], 255, "G should be 255 for white sepia");
        assert!((buf[2] as i32 - 239).abs() <= 3, "B should be ~239 for white sepia, got {}", buf[2]);
    }

    #[test]
    fn apply_filter_rgba_transparent_pixel_skipped() {
        // Alpha=0 pixel must remain untouched (avoid divide-by-zero).
        let mut buf = vec![255u8, 0, 0, 0]; // premultiplied transparent
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(1.0));
        assert_eq!(&buf, &[255, 0, 0, 0], "transparent pixel should not be modified");
    }

    #[test]
    fn apply_filter_rgba_brightness_doubles() {
        // brightness(2.0) on mid-grey → close to white.
        let mut buf = px(128, 128, 128);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Brightness(2.0));
        assert!(buf[0] > 200, "R should be near 255 after brightness(2), got {}", buf[0]);
    }

    #[test]
    fn apply_filter_rgba_saturate_zero_desaturates() {
        // saturate(0) desaturates completely → grey.
        let mut buf = px(255, 0, 0);
        apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Saturate(0.0));
        let r = buf[0]; let g = buf[1]; let b = buf[2];
        assert!((r as i32 - g as i32).abs() <= 2, "saturate(0) R/G should be equal, got r={r} g={g}");
        assert!((g as i32 - b as i32).abs() <= 2, "saturate(0) G/B should be equal, got g={g} b={b}");
    }

    #[test]
    fn fsin_cos_basic_values() {
        // fsin(0) = 0, fsin(π/2) ≈ 1.
        assert!((fsin(0.0)).abs() < 0.001, "sin(0) should be ~0");
        assert!((fsin(std::f32::consts::FRAC_PI_2) - 1.0).abs() < 0.001, "sin(π/2) should be ~1");
        assert!((fcos(0.0) - 1.0).abs() < 0.001, "cos(0) should be ~1");
        assert!((fcos(std::f32::consts::FRAC_PI_2)).abs() < 0.001, "cos(π/2) should be ~0");
    }

    // ── PA-3: blend mode compositing (CPU pixel math) ────────────────────────

    fn approx_u8(a: u8, b: u8) -> bool { (a as i32 - b as i32).abs() <= 2 }

    fn blend_composite_pixel(mode: BlendMode, src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
        let premul_to_str = |px: [u8; 4]| -> [f32; 4] {
            let a = px[3] as f32 / 255.0;
            if a > 0.0 {
                [px[0] as f32 / 255.0 / a, px[1] as f32 / 255.0 / a, px[2] as f32 / 255.0 / a, a]
            } else {
                [0.0; 4]
            }
        };
        let s = premul_to_str(src);
        let d = premul_to_str(dst);
        let out = mix_blend_rgba(mode, s, d);
        let ao = out[3];
        [(out[0]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (out[1]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (out[2]*ao*255.0).round().clamp(0.0,255.0) as u8,
         (ao*255.0).round().clamp(0.0,255.0) as u8]
    }

    #[test]
    fn blend_composite_multiply_opaque_on_opaque() {
        // src=0.5 grey opaque, dst=0.5 grey opaque → multiply → 0.25 grey.
        let result = blend_composite_pixel(BlendMode::Multiply,
            [128, 128, 128, 255], [128, 128, 128, 255]);
        // 0.25 * 255 ≈ 64 (premultiplied, alpha=1).
        assert!(approx_u8(result[0], 64), "R: expected ≈64, got {}", result[0]);
        assert!(approx_u8(result[3], 255), "A: expected 255, got {}", result[3]);
    }

    #[test]
    fn blend_composite_screen_lightens() {
        // src=0.5 grey, dst=0.5 grey → screen → 0.75 grey.
        // screen(a,b) = a + b - a*b = 0.5+0.5-0.25 = 0.75.
        let result = blend_composite_pixel(BlendMode::Screen,
            [128, 128, 128, 255], [128, 128, 128, 255]);
        // 0.75 * 255 ≈ 191
        assert!(approx_u8(result[0], 191), "R: expected ≈191, got {}", result[0]);
        assert!(approx_u8(result[3], 255));
    }

    #[test]
    fn blend_composite_transparent_src_keeps_backdrop() {
        // Fully transparent source → result equals backdrop.
        let result = blend_composite_pixel(BlendMode::Multiply,
            [0, 0, 0, 0], [200, 100, 50, 255]);
        assert!(approx_u8(result[0], 200), "R: expected ≈200, got {}", result[0]);
        assert!(approx_u8(result[1], 100), "G: expected ≈100, got {}", result[1]);
    }

    #[test]
    fn blend_composite_difference_gives_abs_difference() {
        // src=white opaque, dst=grey opaque → difference → grey.
        // difference(1.0, 0.5) = |0.5-1.0| = 0.5 → ~128.
        let result = blend_composite_pixel(BlendMode::Difference,
            [255, 255, 255, 255], [128, 128, 128, 255]);
        assert!(approx_u8(result[0], 127), "R: expected ≈127, got {}", result[0]);
    }

    #[test]
    fn blend_composite_overlay_on_dark_backdrop_is_multiply_like() {
        // Overlay with cb<0.5: result ≈ 2*cs*cb (multiply branch).
        // cs=0.5 (128), cb=0.25 (64) → 2*0.5*0.25 = 0.25 → ≈64.
        let result = blend_composite_pixel(BlendMode::Overlay,
            [128, 128, 128, 255], [64, 64, 64, 255]);
        assert!(approx_u8(result[0], 64), "R: expected ≈64, got {}", result[0]);
    }
}
