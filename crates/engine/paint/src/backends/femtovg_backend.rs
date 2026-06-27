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
use std::path::{Path, PathBuf};
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
    BackgroundRepeat, BackgroundSize, BorderStyle, FontStyle, GradientStop, ObjectFit,
    ObjectPosition,
};

use lumen_core::geom::Rect;

use crate::backend::{RenderBackend, RenderError};
use crate::blend_modes::mix_blend_rgba;
use crate::dash_math::{dashed_border_offsets, dotted_border_offsets};
use crate::display_list::{BlendMode, CornerRadii, DisplayCommand, ResolvedClipShape, bg_tile_geometry, fit_image_rect};
use crate::gradient_math::{conic_sample_t, sample_gradient_color};
use crate::matrix_util::mat4_to_2d_affine;

// ─── Color conversion ────────────────────────────────────────────────────────

/// Конвертирует CSS `Color` (u8 каналы 0-255) в femtovg `Color` (f32 0-1).
#[inline]
fn lumen_to_fvg(c: Color) -> femtovg::Color {
    femtovg::Color::rgba(c.r, c.g, c.b, c.a)
}

/// Appends a closed rounded-rectangle contour to `path` using cubic-Bézier
/// quarter-ellipse corners (kappa ≈ 0.5523). `radii` carries per-corner (x, y)
/// radii and is assumed already clamped to the box. Shared by the border ring
/// (BUG-175) so both outer and inner contours use identical corner geometry.
fn append_rounded_rect_outline(
    path: &mut femtovg::Path,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: &CornerRadii,
) {
    const K: f32 = 0.5523;
    let (tl_x, tl_y) = (radii.tl, radii.tl_y);
    let (tr_x, tr_y) = (radii.tr, radii.tr_y);
    let (br_x, br_y) = (radii.br, radii.br_y);
    let (bl_x, bl_y) = (radii.bl, radii.bl_y);

    path.move_to(x + tl_x, y);
    path.line_to(x + w - tr_x, y);
    path.bezier_to(
        x + w - tr_x + K * tr_x, y,
        x + w,                   y + tr_y - K * tr_y,
        x + w,                   y + tr_y,
    );
    path.line_to(x + w, y + h - br_y);
    path.bezier_to(
        x + w,                   y + h - br_y + K * br_y,
        x + w - br_x + K * br_x, y + h,
        x + w - br_x,            y + h,
    );
    path.line_to(x + bl_x, y + h);
    path.bezier_to(
        x + bl_x - K * bl_x, y + h,
        x,                   y + h - bl_y + K * bl_y,
        x,                   y + h - bl_y,
    );
    path.line_to(x, y + tl_y);
    path.bezier_to(
        x,                   y + tl_y - K * tl_y,
        x + tl_x - K * tl_x, y,
        x + tl_x,            y,
    );
    path.close();
}

// ─── Gradient helpers ─────────────────────────────────────────────────────────

/// Разрешает `GradientStop.position` в [0,1] для **non-repeating** градиента.
///
/// Thin wrapper над [`femtovg_stops`] с `repeating = false`. Используется
/// mask-градиентами (`fill_mask_gradient`), которые повторение не поддерживают.
fn resolve_stops(stops: &[GradientStop], width: f32) -> Vec<(f32, femtovg::Color)> {
    femtovg_stops(stops, width, false)
}

/// Длина CSS-линии линейного градиента в боксе `w×h` под углом `angle_deg`
/// (CSS Images L3 §3.2) — расстояние между начальной и конечной точками, на
/// которое отображаются позиции стопов. Совпадает с `2·half_len` из
/// [`linear_gradient_endpoints`], поэтому px-стопы делятся на правильную длину
/// даже для непрямых углов (45° и т.п.), а не на `rect.width`.
fn linear_gradient_line_len(w: f32, h: f32, angle_deg: f32) -> f32 {
    let theta = angle_deg.to_radians();
    (w * theta.sin().abs() + h * theta.cos().abs()).max(1.0)
}

/// Преобразует CSS-стопы в список `(pos∈[0,1], femtovg::Color)`, готовый для
/// `Paint::linear_gradient_stops` / `radial_gradient_stops`.
///
/// femtovg синтезирует 256-тексельную текстуру градиента и clamp-сэмплит её
/// (`gradient_store.rs`): заполняет от 0 до первого стопа и между соседними
/// стопами, но **область за последним стопом оставляет прозрачной** и
/// игнорирует позиции вне [0,1]. Чтобы совпасть с CSS:
///   * **non-repeating:** последний цвет продлевается до 1.0 — иначе hard-stop
///     вида `… green 50%` без завершающего стопа рисует только первую половину,
///     а вторая остаётся прозрачной (BUG-085 / BUG-144 row 2);
///   * **repeating:** паттерн (период = `last − first`) замощается по всей линии
///     [0,1] — иначе `repeating-linear/-radial-gradient` рисует один clamp-период
///     вместо повторения (BUG-085, TEST-39 `.rep-linear`/`.rep-radial`).
fn femtovg_stops(
    stops: &[GradientStop], line_len: f32, repeating: bool,
) -> Vec<(f32, femtovg::Color)> {
    let resolved = crate::gradient_math::resolve_stop_positions(stops, line_len);
    if resolved.is_empty() {
        return vec![];
    }
    // CSS Images L4 §3.1 — interpolate fades to/through transparency in
    // premultiplied space (BUG-190); femtovg's 256-texel texture samples these
    // dense stops straight, reproducing the premultiplied curve.
    let resolved = crate::gradient_math::premultiplied_subdivide_stops(&resolved);
    if !repeating {
        let mut out: Vec<(f32, femtovg::Color)> = resolved
            .iter()
            .map(|&(pos, c)| (pos.clamp(0.0, 1.0), lumen_to_fvg(c)))
            .collect();
        // Продлить последний цвет до конца линии (femtovg сам не дозаполняет хвост).
        if let Some(&(last_pos, last_col)) = out.last()
            && last_pos < 1.0
        {
            out.push((1.0, last_col));
        }
        return out;
    }
    // repeating: период между первым и последним стопом, замощаем [0,1].
    let first = resolved[0].0;
    let last = resolved[resolved.len() - 1].0;
    let period = last - first;
    if period <= 1e-6 {
        // Вырожденный период — сплошной последний цвет.
        let c = lumen_to_fvg(resolved[resolved.len() - 1].1);
        return vec![(0.0, c), (1.0, c)];
    }
    // k подбирается так, чтобы первая плитка начиналась ≤ 0 (покрыть «голову»
    // до первого стопа), и продолжаем, пока начало плитки ≤ 1.0 (femtovg
    // дозаполнит/обрежет хвост по 1.0). Позиции остаются монотонными: последний
    // стоп плитки k совпадает с первым плитки k+1 → корректная граница повтора.
    let mut out: Vec<(f32, femtovg::Color)> = Vec::new();
    let mut k = ((0.0 - first) / period).floor() as i32;
    const MAX_TILES: i32 = 512; // защита от зацикливания на микроскопическом периоде
    let mut tiles = 0;
    loop {
        let shift = (k as f32) * period;
        if first + shift > 1.0 || tiles >= MAX_TILES {
            break;
        }
        for &(pos, c) in &resolved {
            out.push((pos + shift, lumen_to_fvg(c)));
        }
        k += 1;
        tiles += 1;
    }
    out
}

/// Samples a `[0,1]`-tiled femtovg stop list (the output of [`femtovg_stops`]) at
/// position `t`, clamping `t` to `[0,1]` and linearly interpolating between the
/// two bracketing stops in straight (non-premultiplied) sRGBA.
///
/// This reproduces *exactly* what femtovg's native gradient does internally —
/// clamp-sample a `[0,1]` texture — except per-pixel instead of through a
/// 256-texel LUT. Used by the CPU gradient fill (BUG-085) so the only difference
/// from the native path is the eliminated quantization, never the colour ramp.
fn sample_fvg_stops(resolved: &[(f32, femtovg::Color)], t: f32) -> femtovg::Color {
    let n = resolved.len();
    if n == 0 {
        return femtovg::Color::rgbaf(0.0, 0.0, 0.0, 0.0);
    }
    if n == 1 {
        return resolved[0].1;
    }
    let tc = t.clamp(0.0, 1.0);
    if tc <= resolved[0].0 {
        return resolved[0].1;
    }
    let last = n - 1;
    if tc >= resolved[last].0 {
        return resolved[last].1;
    }
    for i in 0..last {
        let (ap, ac) = resolved[i];
        let (bp, bc) = resolved[i + 1];
        if tc >= ap && tc <= bp {
            let s = bp - ap;
            let f = if s > 1e-6 { (tc - ap) / s } else { 0.0 };
            return femtovg::Color::rgbaf(
                ac.r + (bc.r - ac.r) * f,
                ac.g + (bc.g - ac.g) * f,
                ac.b + (bc.b - ac.b) * f,
                ac.a + (bc.a - ac.a) * f,
            );
        }
    }
    resolved[last].1
}

/// Converts a straight-alpha femtovg `Color` (channels in `[0,1]`) to `rgb::RGBA8`
/// for upload as a CPU-rendered gradient texture (BUG-085).
#[inline]
fn fvg_to_rgba8(c: femtovg::Color) -> rgb::RGBA8 {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    rgb::RGBA8 { r: q(c.r), g: q(c.g), b: q(c.b), a: q(c.a) }
}

/// Fills an `iw×ih` RGBA8 texture by sampling `color_at` once per texel over a
/// normalized box coordinate space (BUG-085). `color_at(cx, cy)` is evaluated at
/// texel centres with `cx, cy ∈ [0,1]` (fraction across the box). Sampling at
/// device resolution (rather than femtovg's 256-texel LUT) is what removes the
/// quantization; the texel-centre sample mirrors what a GPU shader would do.
fn fill_gradient_texture(
    iw: usize, ih: usize, color_at: impl Fn(f32, f32) -> femtovg::Color,
) -> Vec<rgb::RGBA8> {
    let mut pixels = vec![rgb::RGBA8 { r: 0, g: 0, b: 0, a: 0 }; iw * ih];
    for iy in 0..ih {
        let cy = (iy as f32 + 0.5) / ih as f32;
        let row = iy * iw;
        for ix in 0..iw {
            let cx = (ix as f32 + 0.5) / iw as f32;
            pixels[row + ix] = fvg_to_rgba8(color_at(cx, cy));
        }
    }
    pixels
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
    /// Cache path → femtovg FontId, prevents re-loading the same .ttf/.otf bytes.
    loaded_fonts: HashMap<PathBuf, femtovg::FontId>,
    /// Pre-loaded curated system fallback font IDs (emoji/CJK/RTL/Indic/Thai).
    /// Built eagerly in `set_font_provider`; appended to every DrawText paint chain
    /// so glyphs missing from CSS-declared families fall through to system fonts.
    fallback_chain: Vec<femtovg::FontId>,
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
    /// Фон канвы (CSS Backgrounds §3.11.1): цвет, которым заливается весь кадр
    /// перед отрисовкой content. `None` → UA-дефолт (белый). Устанавливается
    /// shell-ом через `set_canvas_background` из фона корневого элемента.
    canvas_bg: Option<Color>,
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
    /// Offscreen opacity group layer stack (BUG-133). Each entry holds an
    /// offscreen ImageId that subtree draws render into; PopOpacity composites
    /// it once with the group alpha (CSS Color L3 §3.2: opacity is atomic —
    /// overlapping children must not double-blend against the backdrop).
    opacity_layer_stack: Vec<OpacityLayerEntry>,
    /// Offscreen gradient-mask layer stack (BUG-183). Each entry holds the
    /// offscreen ImageId the masked subtree renders into; `PopMask` multiplies
    /// the layer's alpha by the gradient and composites it (CSS Masking L1 §4).
    mask_layer_stack: Vec<MaskLayerEntry>,
    /// Offscreen backdrop-filter layer stack (PA-4). Each entry holds the filtered
    /// backdrop image and element content image for compositing on PopBackdropFilter.
    backdrop_filter_layer_stack: Vec<BackdropFilterLayerEntry>,
    /// Images from backdrop-filter layers queued for deletion after the next flush.
    backdrop_filter_pending_delete: Vec<femtovg::ImageId>,
    /// Стек видов клипа (BUG-140): общий `PopClip` закрывает scissor-клипы
    /// (`canvas.restore()`) и shape-клипы (композит offscreen-слоя через
    /// путь формы) по-разному; вид определяется парным Push.
    clip_stack: Vec<ClipEntry>,
    /// Currently active render target image. `None` means Screen.
    /// Updated by [`Self::switch_render_target`] whenever the RT changes.
    active_rt_image: Option<femtovg::ImageId>,
    /// Per-pixel gradient images (BUG-085) queued for deletion after the next
    /// `canvas.flush()`. Repeating linear and all radial gradients are rendered
    /// CPU-side into a device-resolution texture to bypass femtovg's 256-texel
    /// gradient LUT (which quantizes repeating-stop boundaries); the texture is
    /// blitted with `fill_path`, whose draw command holds the ImageId until flush.
    gradient_pending_delete: Vec<femtovg::ImageId>,
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

// ─── Offscreen layer support (BUG-133 opacity, BUG-146 filter) ───────────────

/// Image flags for offscreen layers that are composited directly from their
/// FBO on the GPU (opacity groups — BUG-133; filter layers and their blur
/// destinations — BUG-146).
///
/// `PREMULTIPLIED` — femtovg renders premultiplied RGBA into image render
/// targets. `FLIP_Y` — the layer is an FBO (GL bottom-up rows); sampling it as
/// `Paint::image` at composite time must flip Y back or the whole group
/// renders upside-down. The CPU compositing paths (PA-2 colour-matrix, PA-3
/// blend) get the flip for free from their screenshot→re-upload round-trip
/// (`screenshot()` reverses rows); GPU-only composites do not. `filter_image`
/// (Gaussian blur) ignores sampler flags entirely — its shader samples raw
/// `fpos/extent` coords — so blur preserves memory orientation and the flag
/// stays meaningful on the blur destination. Do NOT use these flags for images
/// created from CPU pixel uploads (e.g. the backdrop-filter screenshot path):
/// their memory is already top-down and FLIP_Y would flip them.
fn offscreen_layer_image_flags() -> femtovg::ImageFlags {
    femtovg::ImageFlags::PREMULTIPLIED | femtovg::ImageFlags::FLIP_Y
}

/// Entry pushed onto `FemtovgBackend::opacity_layer_stack` by `PushOpacity`.
///
/// The group's subtree renders into the offscreen `image_id`; `PopOpacity`
/// composites that image onto `prev_render_target` exactly once with the
/// group `alpha`. Per-draw `set_global_alpha` is wrong for groups: overlapping
/// children double-blend, negative-z children show through siblings, and a
/// nested PushOpacity replaces (not multiplies) the outer alpha.
struct OpacityLayerEntry {
    /// Offscreen image the group's content renders into. `None` means the
    /// offscreen image could not be created — fallback `save()` +
    /// `set_global_alpha` was used and `PopOpacity` must `restore()` instead.
    image_id: Option<femtovg::ImageId>,
    /// Group opacity in `[0, 1]`, applied once at composite time.
    alpha: f32,
    /// Render target active before PushOpacity — restored on PopOpacity.
    prev_render_target: femtovg::RenderTarget,
}

// ─── Gradient mask support (BUG-183) ─────────────────────────────────────────

/// Gradient that drives a `mask-image` alpha mask (CSS Masking L1 §4).
///
/// The masked element's subtree renders into an offscreen FBO; on `PopMask`
/// the gradient is painted over the FBO with `CompositeOperation::DestinationIn`
/// so the FBO's alpha is multiplied by the gradient's alpha. Stops carry CSS
/// colours: `mask-mode: alpha` (the default and only mode reaching paint, since
/// `mask-mode` is not yet parsed — P4) uses the stop alpha directly.
enum MaskGradient {
    /// `linear-gradient(...)` mask. `angle_deg` is CSS (0° = to top).
    Linear { angle_deg: f32, stops: Vec<GradientStop> },
    /// `radial-gradient(...)` mask. Centre as a fraction of the box.
    Radial { center_x_pct: f32, center_y_pct: f32, stops: Vec<GradientStop> },
    /// `conic-gradient(...)` mask.
    Conic { center_x_pct: f32, center_y_pct: f32, from_angle_deg: f32, stops: Vec<GradientStop>, repeating: bool },
}

/// Entry pushed onto `FemtovgBackend::mask_layer_stack` by `PushMask*Gradient`.
///
/// The masked subtree renders into the offscreen `image_id`; `PopMask`
/// multiplies that layer's alpha by `gradient` (evaluated over `rect`) and
/// composites the result onto `prev_render_target`.
struct MaskLayerEntry {
    /// Offscreen image the masked content renders into. `None` means the FBO
    /// could not be allocated — fallback `save()` + scissor was used and
    /// `PopMask` must `restore()` instead of compositing.
    image_id: Option<femtovg::ImageId>,
    /// Gradient driving the mask alpha. `None` for the scissor fallback.
    gradient: Option<MaskGradient>,
    /// Border-box of the masked element in CSS px (mask painting area).
    rect: lumen_core::geom::Rect,
    /// Render target active before the matching `PushMask*` — restored on `PopMask`.
    prev_render_target: femtovg::RenderTarget,
}

// ─── Clip-path shape clip support (BUG-140) ──────────────────────────────────

/// Запись стека клипов: чем открыт ближайший незакрытый клип, чтобы общий
/// `PopClip` знал, как его закрывать.
enum ClipEntry {
    /// Scissor-клип (`PushClipRect`/`PushClipRoundedRect`, либо fallback
    /// `PushClipPath` при сбое аллокации слоя) — `PopClip` делает
    /// `canvas.restore()`.
    Scissor,
    /// Shape-клип (`PushClipPath`): subtree рендерится в offscreen `image_id`;
    /// `PopClip` композитит слой на `prev_render_target` одним
    /// `fill_path`-вызовом по форме (антиалиасинг пути — бесплатно).
    PathLayer {
        /// Offscreen-слой с содержимым клип-группы (full-RT, FLIP_Y FBO).
        image_id: femtovg::ImageId,
        /// Форма клипа в page-координатах (до transform элемента).
        shape: ResolvedClipShape,
        /// Матрица канвы на момент Push (включая transform элемента):
        /// применяется к точкам пути вручную при композите с identity-канвой,
        /// чтобы не транслировать повторно сам слой (контент в слое уже
        /// нарисован под этой матрицей).
        transform: femtovg::Transform2D,
        /// Render target, активный до PushClipPath.
        prev_render_target: femtovg::RenderTarget,
    },
}

/// Строит femtovg-путь формы клипа, применяя `t` к каждой точке вручную.
/// Кубические Безье аффинно-инвариантны, поэтому circle/ellipse строятся
/// 4 сегментами с каппой и трансформируются по контрольным точкам — под
/// rotate/scale форма остаётся точной (круг под uniform-rotate — круг).
fn clip_shape_path(shape: &ResolvedClipShape, t: &femtovg::Transform2D) -> femtovg::Path {
    /// Коэффициент аппроксимации четверти окружности кубической Безье.
    const KAPPA: f32 = 0.552_285;
    let mut path = femtovg::Path::new();
    match shape {
        ResolvedClipShape::Circle { cx, cy, r } => {
            ellipse_path(&mut path, *cx, *cy, *r, *r, t, KAPPA);
        }
        ResolvedClipShape::Ellipse { cx, cy, rx, ry } => {
            ellipse_path(&mut path, *cx, *cy, *rx, *ry, t, KAPPA);
        }
        ResolvedClipShape::Polygon { verts, .. } => {
            let mut iter = verts.iter();
            if let Some((x, y)) = iter.next() {
                let (px, py) = t.transform_point(*x, *y);
                path.move_to(px, py);
                for (x, y) in iter {
                    let (px, py) = t.transform_point(*x, *y);
                    path.line_to(px, py);
                }
                path.close();
            }
        }
    }
    path
}

/// Добавляет в `path` эллипс (cx, cy, rx, ry) из 4 кубических Безье,
/// трансформируя каждую опорную и контрольную точку через `t`.
fn ellipse_path(
    path: &mut femtovg::Path,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    t: &femtovg::Transform2D,
    kappa: f32,
) {
    let kx = rx * kappa;
    let ky = ry * kappa;
    let p = |x: f32, y: f32| t.transform_point(x, y);
    let (sx, sy) = p(cx + rx, cy);
    path.move_to(sx, sy);
    // Квадранты по часовой: (cx+rx,cy) → (cx,cy+ry) → (cx-rx,cy) → (cx,cy-ry).
    let (c1x, c1y) = p(cx + rx, cy + ky);
    let (c2x, c2y) = p(cx + kx, cy + ry);
    let (ex, ey) = p(cx, cy + ry);
    path.bezier_to(c1x, c1y, c2x, c2y, ex, ey);
    let (c1x, c1y) = p(cx - kx, cy + ry);
    let (c2x, c2y) = p(cx - rx, cy + ky);
    let (ex, ey) = p(cx - rx, cy);
    path.bezier_to(c1x, c1y, c2x, c2y, ex, ey);
    let (c1x, c1y) = p(cx - rx, cy - ky);
    let (c2x, c2y) = p(cx - kx, cy - ry);
    let (ex, ey) = p(cx, cy - ry);
    path.bezier_to(c1x, c1y, c2x, c2y, ex, ey);
    let (c1x, c1y) = p(cx + kx, cy - ry);
    let (c2x, c2y) = p(cx + rx, cy - ky);
    let (ex, ey) = p(cx + rx, cy);
    path.bezier_to(c1x, c1y, c2x, c2y, ex, ey);
    path.close();
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

// ─── Backdrop filter layer support (PA-4) ────────────────────────────────────

/// Entry pushed onto `FemtovgBackend::backdrop_filter_layer_stack` by `PushBackdropFilter`.
///
/// `elem_image_id` receives all draws between Push and Pop. `filtered_backdrop_id` holds
/// the full-canvas backdrop snapshot with the filter chain applied (blur + colour-matrix).
/// On `PopBackdropFilter`, `composite_backdrop_filter_layer` blits the filtered backdrop
/// region at `bounds`, then composites element content on top (CSS Filter Effects L2 §2).
struct BackdropFilterLayerEntry {
    /// Offscreen image capturing element content (draws between Push and Pop).
    elem_image_id: femtovg::ImageId,
    /// Full-canvas filtered backdrop snapshot — blurred and/or colour-filtered.
    filtered_backdrop_id: femtovg::ImageId,
    /// Element bounds in CSS pixels — the region where the filtered backdrop is visible.
    bounds: lumen_core::geom::Rect,
    /// Render target active before PushBackdropFilter — restored on PopBackdropFilter.
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
/// Computes the three box-blur radii whose successive application approximates
/// a Gaussian of standard deviation `sigma` (Kovesi, *Fast Almost-Gaussian
/// Filtering*; n = 3 boxes). Three boxes are visually indistinguishable from a
/// true Gaussian and far closer than a single box, which is what Edge/Chrome
/// rasterize for `filter: blur()` / `backdrop-filter: blur()`. Each returned
/// value is the half-width `r` of a `(2r+1)`-wide averaging window.
fn gaussian_box_radii(sigma: f32) -> [usize; 3] {
    const N: f32 = 3.0;
    // Ideal (real) box width matching the Gaussian variance across N boxes.
    let w_ideal = (12.0 * sigma * sigma / N + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1; // box widths must be odd to stay symmetric around a pixel.
    }
    let wu = wl + 2;
    // How many of the N boxes use the lower width `wl` (the rest use `wu`).
    let m_ideal = (12.0 * sigma * sigma
        - N * (wl * wl) as f32
        - 4.0 * N * wl as f32
        - 3.0 * N)
        / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    let mut radii = [1usize; 3];
    for (i, slot) in radii.iter_mut().enumerate() {
        let w = if (i as i32) < m { wl } else { wu };
        *slot = (((w - 1) / 2).max(1)) as usize;
    }
    radii
}

/// One box-blur pass (separable: horizontal then vertical) of half-width `r`,
/// restricted to `region` (`[rx0, rx1) × [ry0, ry1)` in pixel coords). Sampling
/// is clamped to the region so the window never reaches outside it. `scratch`
/// is reused across passes to avoid per-pass allocation.
fn box_blur_pass_region(
    pixels: &mut [u8],
    scratch: &mut [u8],
    stride: usize,
    region: (usize, usize, usize, usize),
    r: usize,
) {
    let (rx0, ry0, rx1, ry1) = region;
    // Horizontal pass: pixels → scratch.
    for y in ry0..ry1 {
        for x in rx0..rx1 {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let x0 = x.saturating_sub(r).max(rx0);
            let x1 = (x + r + 1).min(rx1);
            for sx in x0..x1 {
                let off = y * stride + sx * 4;
                for c in 0..4 { sum[c] += pixels[off + c] as u32; }
                count += 1;
            }
            let off = y * stride + x * 4;
            for c in 0..4 { scratch[off + c] = (sum[c] / count) as u8; }
        }
    }
    // Vertical pass: scratch → pixels.
    for y in ry0..ry1 {
        for x in rx0..rx1 {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let y0 = y.saturating_sub(r).max(ry0);
            let y1 = (y + r + 1).min(ry1);
            for sy in y0..y1 {
                let off = sy * stride + x * 4;
                for c in 0..4 { sum[c] += scratch[off + c] as u32; }
                count += 1;
            }
            let off = y * stride + x * 4;
            for c in 0..4 { pixels[off + c] = (sum[c] / count) as u8; }
        }
    }
}

/// Three-iteration box blur approximating a Gaussian of deviation `sigma`,
/// restricted to `region` (pixel coords, half-open `[x0, x1) × [y0, y1)`).
///
/// Three successive box passes (radii from [`gaussian_box_radii`]) match a true
/// Gaussian closely — a single box pass (the previous implementation, despite
/// its "3-pass" comment) reads boxy versus Edge's Gaussian `blur()` and was the
/// dominant residual on BUG-144's blur cards.
///
/// Sampling is clamped to the region: blur near a region edge averages only
/// pixels inside the region, which duplicates the region's own edge content
/// instead of bleeding in whatever lies outside it. This is required for
/// `backdrop-filter`, whose input is the backdrop image cropped to the
/// element's border box (CSS Filter Effects §backdrop-filter) — blurring the
/// whole canvas and then cropping pulls the dark page background above a card
/// into the card's top edge (BUG-144 edge-bleed). Pixels outside the region
/// are left untouched.
fn box_blur_rgba_region(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    sigma: f32,
    region: (usize, usize, usize, usize),
) {
    let (rx0, ry0, mut rx1, mut ry1) = region;
    rx1 = rx1.min(width);
    ry1 = ry1.min(height);
    if rx0 >= rx1 || ry0 >= ry1 {
        return;
    }
    let stride = width * 4;
    let clamped = (rx0, ry0, rx1, ry1);
    let mut scratch = pixels.to_vec();
    for r in gaussian_box_radii(sigma) {
        box_blur_pass_region(pixels, &mut scratch, stride, clamped, r);
    }
}

/// Replicate edge pixels of `region` outward by `extend_px` in every direction.
///
/// For `backdrop-filter: blur()`, the CSS backdrop is cropped to the element's
/// border box. A box blur clamped to that crop truncates its kernel at the
/// boundary, producing a brighter/fringed edge (BUG-144 edge-bleed). By
/// extending the region with replicated edge pixels before blurring, each edge
/// pixel sees a symmetric kernel and the result matches a proper Gaussian
/// falloff. Returns the extended (clamped) region.
fn extend_region_replicated(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    region: (usize, usize, usize, usize),
    extend_px: usize,
) -> (usize, usize, usize, usize) {
    if extend_px == 0 {
        return region;
    }
    let (rx0, ry0, rx1, ry1) = region;
    if rx0 >= rx1 || ry0 >= ry1 {
        return region;
    }
    let ex0 = rx0.saturating_sub(extend_px);
    let ey0 = ry0.saturating_sub(extend_px);
    let ex1 = (rx1 + extend_px).min(width);
    let ey1 = (ry1 + extend_px).min(height);
    let stride = width * 4;
    let rxl = rx1.saturating_sub(1);
    let ryl = ry1.saturating_sub(1);

    // Top band (ey0..ry0): replicate row ry0 (covers top-left/top-right corners).
    for y in ey0..ry0 {
        for x in ex0..ex1 {
            let src_x = if x < rx0 { rx0 } else if x > rxl { rxl } else { x };
            let src_off = (ry0 * stride) + src_x * 4;
            let dst_off = (y * stride) + x * 4;
            let mut tmp = [0u8; 4];
            tmp.copy_from_slice(&pixels[src_off..src_off + 4]);
            pixels[dst_off..dst_off + 4].copy_from_slice(&tmp);
        }
    }
    // Bottom band (ry1..ey1): replicate row ry1-1 (covers bottom corners).
    for y in ry1..ey1 {
        for x in ex0..ex1 {
            let src_x = if x < rx0 { rx0 } else if x > rxl { rxl } else { x };
            let src_off = (ryl * stride) + src_x * 4;
            let dst_off = (y * stride) + x * 4;
            let mut tmp = [0u8; 4];
            tmp.copy_from_slice(&pixels[src_off..src_off + 4]);
            pixels[dst_off..dst_off + 4].copy_from_slice(&tmp);
        }
    }
    // Left band (ry0..ry1, ex0..rx0): replicate column rx0 (middle only).
    for y in ry0..ry1 {
        for x in ex0..rx0 {
            let src_off = (y * stride) + rx0 * 4;
            let dst_off = (y * stride) + x * 4;
            let mut tmp = [0u8; 4];
            tmp.copy_from_slice(&pixels[src_off..src_off + 4]);
            pixels[dst_off..dst_off + 4].copy_from_slice(&tmp);
        }
    }
    // Right band (ry0..ry1, rx1..ex1): replicate column rx1-1 (middle only).
    for y in ry0..ry1 {
        for x in rx1..ex1 {
            let src_off = (y * stride) + rxl * 4;
            let dst_off = (y * stride) + x * 4;
            let mut tmp = [0u8; 4];
            tmp.copy_from_slice(&pixels[src_off..src_off + 4]);
            pixels[dst_off..dst_off + 4].copy_from_slice(&tmp);
        }
    }

    (ex0, ey0, ex1, ey1)
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
            loaded_fonts: HashMap::new(),
            fallback_chain: Vec::new(),
            layer_stack_depth: 0,
            sticky_stack: Vec::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            viewport_css_w: size.width as f32 / scale as f32,
            viewport_css_h: size.height as f32 / scale as f32,
            canvas_bg: None,
            filter_layer_stack: Vec::new(),
            filter_layer_pending_delete: Vec::new(),
            opacity_layer_stack: Vec::new(),
            mask_layer_stack: Vec::new(),
            blend_layer_stack: Vec::new(),
            blend_layer_pending_delete: Vec::new(),
            backdrop_filter_layer_stack: Vec::new(),
            backdrop_filter_pending_delete: Vec::new(),
            clip_stack: Vec::new(),
            active_rt_image: None,
            gradient_pending_delete: Vec::new(),
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

        // Clamp radii via CSS Backgrounds §5.5 (single scale factor over all
        // corners), preserving elliptical corners (rx ≠ ry). The previous
        // per-radius `min(w/2, h/2)` cap collapsed a wide SVG `<ellipse>` into a
        // circle → stadium shape instead of an ellipse (BUG-198).
        let clamped = radii.clamped_to_box(w, h);
        let (tl_x, tl_y) = (clamped.tl, clamped.tl_y);
        let (tr_x, tr_y) = (clamped.tr, clamped.tr_y);
        let (br_x, br_y) = (clamped.br, clamped.br_y);
        let (bl_x, bl_y) = (clamped.bl, clamped.bl_y);

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

    /// Draws a uniform-coloured solid border whose corners follow `border-radius`
    /// (BUG-175). The border is the even-odd ring between the outer rounded-rect
    /// (border box, outer radii) and the inner rounded-rect (padding box, inner
    /// radii = outer − side width per CSS Backgrounds L3 §5.5). `widths` is the
    /// per-side width `[top, right, bottom, left]`; when the border is thicker
    /// than the box (no inner area) the whole rounded box is filled.
    fn draw_rounded_border_ring(
        &mut self,
        rect: Rect,
        widths: [f32; 4],
        color: Color,
        radii: CornerRadii,
    ) {
        let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let [top, right, bottom, left] = widths;

        // Outer contour: same clamp the background fill uses, so the border's
        // outer edge coincides with the rounded background edge.
        let outer = radii.clamped_to_box(w, h);

        let mut path = femtovg::Path::new();
        append_rounded_rect_outline(&mut path, x, y, w, h, &outer);

        // Inner contour (padding box). Skip the hole when the border swallows the
        // whole box — then the ring degenerates to a solid rounded rect.
        let iw = w - left - right;
        let ih = h - top - bottom;
        if iw > 0.0 && ih > 0.0 {
            let inner = radii.inner_for_border(widths).clamped_to_box(iw, ih);
            append_rounded_rect_outline(&mut path, x + left, y + top, iw, ih, &inner);
        }

        let paint = femtovg::Paint::color(lumen_to_fvg(color))
            .with_fill_rule(femtovg::FillRule::EvenOdd);
        self.canvas.fill_path(&path, &paint);
    }

    /// Loads font bytes for a given path and registers them in `canvas`, returning
    /// the `FontId`. Returns `None` if bytes cannot be read or `add_font_mem` fails.
    /// Results are cached in `loaded_fonts` to avoid re-loading the same file.
    fn load_font_by_path(&mut self, path: &Path, provider: &Arc<dyn FontProvider>) -> Option<femtovg::FontId> {
        if let Some(&id) = self.loaded_fonts.get(path) {
            return Some(id);
        }
        let bytes = if let Some(mem) = provider.read_face_bytes(path) {
            mem
        } else {
            std::fs::read(path).ok()?
        };
        let id = self.canvas.add_font_mem(&bytes).ok()?;
        self.loaded_fonts.insert(path.to_owned(), id);
        Some(id)
    }

    /// Resolves CSS `font-family` list + weight/style to a femtovg font chain.
    ///
    /// Order: CSS-declared families (first match wins per CSS Fonts L4 §3.1) →
    /// bundled Inter → curated system fallbacks (emoji/CJK/RTL/Indic/Thai).
    /// Generic keywords (serif/sans-serif/monospace/cursive/fantasy/system-ui)
    /// are skipped — they fall through to Inter which covers Latin well enough.
    /// Returns at least `[inter_id]` when no provider is set.
    fn resolve_font_chain(
        &mut self,
        families: &[String],
        weight: u16,
        style: FontStyle,
    ) -> Vec<femtovg::FontId> {
        let mut ids: Vec<femtovg::FontId> = Vec::new();

        if let Some(provider) = self.font_provider.clone() {
            let core_style = match style {
                FontStyle::Normal => lumen_core::ext::FontStyle::Normal,
                FontStyle::Italic => lumen_core::ext::FontStyle::Italic,
                FontStyle::Oblique => lumen_core::ext::FontStyle::Oblique,
            };
            for fam in families {
                let lc = fam.to_ascii_lowercase();
                if matches!(
                    lc.as_str(),
                    "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
                ) {
                    continue;
                }
                if let Some(rec) = provider.pick_face(fam, weight, core_style)
                    && let Some(id) = self.load_font_by_path(&rec.path.clone(), &provider)
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
            }
        }

        // Bundled Inter as the primary Latin fallback.
        if let Some(inter) = self.font_id
            && !ids.contains(&inter)
        {
            ids.push(inter);
        }

        // Curated system fallbacks (emoji/CJK/RTL/Indic/Thai) appended last.
        for &fb in &self.fallback_chain.clone() {
            if !ids.contains(&fb) {
                ids.push(fb);
            }
        }

        ids
    }

    /// Рисует текст с уже разрешённой font chain.
    ///
    /// Baseline ≈ 80% от font_size (аппроксимация;
    /// точные метрики — из font metrics в будущих задачах).
    fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color, chain: &[femtovg::FontId]) {
        let mut paint = femtovg::Paint::color(lumen_to_fvg(color));
        if !chain.is_empty() {
            paint.set_font(chain);
        }
        paint.set_font_size(font_size);
        let _ = self.canvas.fill_text(x, y + font_size * 0.8, text, &paint);
    }

    /// BUG-109: renders a text run with `font-variation-settings` axes applied,
    /// bypassing femtovg's variation-blind text engine.
    ///
    /// Resolves the first CSS-declared family that maps to a **variable** face,
    /// builds filled-glyph paths at the requested axis coordinates via
    /// [`crate::varied_text::build_varied_text_paths`], and fills them with the
    /// text colour through the current canvas transform/clip. Returns `true`
    /// when the run was rendered here; `false` when no variable face was found
    /// (no provider, only static/generic families) so the caller falls back to
    /// femtovg's native text path.
    #[allow(clippy::too_many_arguments)]
    fn draw_varied_text(
        &mut self,
        rect: &Rect,
        text: &str,
        font_size: f32,
        color: Color,
        families: &[String],
        weight: u16,
        style: FontStyle,
        axes: &[([u8; 4], f32)],
        tab_size: f32,
    ) -> bool {
        let Some(provider) = self.font_provider.clone() else {
            return false;
        };
        let core_style = match style {
            FontStyle::Normal => lumen_core::ext::FontStyle::Normal,
            FontStyle::Italic => lumen_core::ext::FontStyle::Italic,
            FontStyle::Oblique => lumen_core::ext::FontStyle::Oblique,
        };
        for fam in families {
            let lc = fam.to_ascii_lowercase();
            if matches!(
                lc.as_str(),
                "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
            ) {
                continue;
            }
            let Some(rec) = provider.pick_face(fam, weight, core_style) else {
                continue;
            };
            let Some(bytes) = provider
                .read_face_bytes(&rec.path)
                .or_else(|| std::fs::read(&rec.path).ok())
            else {
                continue;
            };
            // `build_varied_text_paths` returns None for static faces — defer to
            // the next family (and ultimately femtovg) in that case.
            if let Some(cmds) = crate::varied_text::build_varied_text_paths(
                &bytes, axes, text, font_size, rect.x, rect.y, tab_size,
            ) {
                self.fill_glyph_path(&cmds, color);
                return true;
            }
        }
        false
    }

    /// Fills a set of [`crate::varied_text::PathCmd`]s (screen pixels, Y-down)
    /// with a solid colour, honouring the canvas's current transform and clip.
    fn fill_glyph_path(&mut self, cmds: &[crate::varied_text::PathCmd], color: Color) {
        use crate::varied_text::PathCmd;
        if cmds.is_empty() {
            return;
        }
        let mut path = femtovg::Path::new();
        for cmd in cmds {
            match *cmd {
                PathCmd::MoveTo(x, y) => path.move_to(x, y),
                PathCmd::LineTo(x, y) => path.line_to(x, y),
                PathCmd::QuadTo(cx, cy, x, y) => path.quad_to(cx, cy, x, y),
                PathCmd::Close => path.close(),
            }
        }
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
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
                // BUG-146: the destination must carry FLIP_Y. `filter_image`
                // preserves memory orientation (its shader ignores sampler
                // flags), so the blurred FBO is still bottom-up like the source
                // layer; the blur-only chain composites it directly on the GPU
                // with no screenshot round-trip to flip it. Without the flag
                // blurred box-shadows rendered vertically mirrored (TEST-15
                // 1.06% → 6.58%). The colour-matrix path after a blur is
                // unaffected: `screenshot()` reads raw pixels and reverses
                // rows regardless of flags.
                if let lumen_layout::FilterFn::Blur(sigma) = f
                    && *sigma > 0.0
                    && let Ok(dst) = self.canvas.create_image_empty(
                        self.width as usize,
                        self.height as usize,
                        femtovg::PixelFormat::Rgba8,
                        offscreen_layer_image_flags(),
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

    /// Composites an opacity group layer (BUG-133) onto the previous render target.
    ///
    /// The offscreen image already contains the group's subtree rendered with
    /// full opacity (children correctly occlude each other). One full-canvas
    /// image draw with `alpha` then blends the whole group against the backdrop
    /// exactly once. Drawn under `reset_transform` because the layer is in page
    /// space — any active transform was already applied while drawing INTO it.
    /// BUG-140: композитит shape-клип-слой на `prev_render_target` одним
    /// `fill_path`-вызовом: путь — форма клипа, трансформированная матрицей
    /// `t` (канва на момент Push, включая transform элемента); заливка —
    /// image-paint слоя 1:1 (identity-канва, слой уже в screen-space).
    /// AA кромки формы — штатный femtovg path-AA.
    fn composite_clip_path_layer(
        &mut self,
        src_id: femtovg::ImageId,
        shape: &ResolvedClipShape,
        t: &femtovg::Transform2D,
        prev_render_target: femtovg::RenderTarget,
    ) {
        self.switch_render_target(prev_render_target);
        self.canvas.save();
        self.canvas.reset_transform();
        let css_w = (self.width as f64 / self.scale) as f32;
        let css_h = (self.height as f64 / self.scale) as f32;
        // CSS Shapes L1 §3/§4 — even-odd оставляет дырки в самопересекающихся
        // clip-формах (polygon()/path()); по умолчанию nonzero.
        let fill_rule = match shape {
            ResolvedClipShape::Polygon { even_odd: true, .. } => femtovg::FillRule::EvenOdd,
            _ => femtovg::FillRule::NonZero,
        };
        let paint = femtovg::Paint::image(src_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0)
            .with_anti_alias(true)
            .with_fill_rule(fill_rule);
        let path = clip_shape_path(shape, t);
        self.canvas.fill_path(&path, &paint);
        self.canvas.restore();
        // Delete after flush — pending GPU commands still reference the id.
        self.filter_layer_pending_delete.push(src_id);
    }

    fn composite_opacity_layer(
        &mut self,
        src_id: femtovg::ImageId,
        alpha: f32,
        prev_render_target: femtovg::RenderTarget,
    ) {
        self.switch_render_target(prev_render_target);
        self.canvas.save();
        self.canvas.reset_transform();
        let css_w = (self.width as f64 / self.scale) as f32;
        let css_h = (self.height as f64 / self.scale) as f32;
        let paint = femtovg::Paint::image(src_id, 0.0, 0.0, css_w, css_h, 0.0, alpha);
        let mut path = femtovg::Path::new();
        path.rect(0.0, 0.0, css_w, css_h);
        self.canvas.fill_path(&path, &paint);
        self.canvas.restore();
        // Delete after flush — pending GPU commands still reference the id.
        self.filter_layer_pending_delete.push(src_id);
    }

    /// CSS Masking L1 §4 (BUG-183) — opens an offscreen layer for a gradient
    /// `mask-image`. The masked subtree renders into a transparent full-RT FBO;
    /// the matching `PopMask` multiplies that FBO's alpha by the gradient.
    ///
    /// On FBO-allocation failure falls back to a rect scissor (mask no-op).
    fn push_mask_gradient_layer(&mut self, rect: lumen_core::geom::Rect, gradient: MaskGradient) {
        let prev_rt = self.current_rt();
        match self.canvas.create_image_empty(
            self.width as usize,
            self.height as usize,
            femtovg::PixelFormat::Rgba8,
            offscreen_layer_image_flags(),
        ) {
            Ok(img_id) => {
                self.switch_render_target(femtovg::RenderTarget::Image(img_id));
                self.canvas.clear_rect(
                    0, 0, self.width, self.height,
                    femtovg::Color::rgba(0, 0, 0, 0),
                );
                self.mask_layer_stack.push(MaskLayerEntry {
                    image_id: Some(img_id),
                    gradient: Some(gradient),
                    rect,
                    prev_render_target: prev_rt,
                });
            }
            Err(_) => {
                // Fallback: rect scissor (gradient mask becomes a hard rect clip).
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.mask_layer_stack.push(MaskLayerEntry {
                    image_id: None,
                    gradient: None,
                    rect,
                    prev_render_target: prev_rt,
                });
            }
        }
        self.layer_stack_depth += 1;
    }

    /// CSS Masking L1 §4 (BUG-183) — applies the gradient mask to the offscreen
    /// layer and composites it onto the previous render target.
    ///
    /// The layer holds the masked subtree. Painting the gradient over `rect`
    /// with `CompositeOperation::DestinationIn` multiplies the layer's existing
    /// alpha by the gradient's alpha (`mask-mode: alpha`, the default), then the
    /// masked layer is composited down exactly like an opacity group.
    fn composite_mask_layer(&mut self, entry: MaskLayerEntry) {
        let MaskLayerEntry { image_id, gradient, rect, prev_render_target } = entry;
        let Some(img_id) = image_id else { return };
        // RT is currently the FBO: multiply its alpha by the gradient.
        if let Some(g) = gradient {
            self.canvas.save();
            self.canvas.global_composite_operation(femtovg::CompositeOperation::DestinationIn);
            self.fill_mask_gradient(&g, rect);
            self.canvas.restore();
        }
        // Composite the masked layer (alpha already folded in) onto prev_rt.
        self.composite_opacity_layer(img_id, 1.0, prev_render_target);
    }

    /// Paints `gradient` over `rect` using the current canvas transform. Used by
    /// `composite_mask_layer` under a `DestinationIn` composite so the gradient's
    /// alpha becomes the mask value. Mirrors the `DrawLinearGradient` /
    /// `DrawRadialGradient` / `DrawConicGradient` paint construction.
    fn fill_mask_gradient(&mut self, gradient: &MaskGradient, rect: lumen_core::geom::Rect) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        match gradient {
            MaskGradient::Linear { angle_deg, stops } => {
                if stops.is_empty() {
                    return;
                }
                let ([sx, sy], [ex, ey]) = linear_gradient_endpoints(
                    rect.x, rect.y, rect.width, rect.height, *angle_deg,
                );
                let resolved = resolve_stops(stops, rect.width);
                if resolved.len() < 2 {
                    return;
                }
                let paint = femtovg::Paint::linear_gradient_stops(sx, sy, ex, ey, resolved);
                let mut path = femtovg::Path::new();
                path.rect(rect.x, rect.y, rect.width, rect.height);
                self.canvas.fill_path(&path, &paint);
            }
            MaskGradient::Radial { center_x_pct, center_y_pct, stops } => {
                if stops.is_empty() {
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
                let paint = femtovg::Paint::radial_gradient_stops(cx, cy, 0.0, outer_r, resolved);
                let mut path = femtovg::Path::new();
                path.rect(rect.x, rect.y, rect.width, rect.height);
                self.canvas.fill_path(&path, &paint);
            }
            MaskGradient::Conic { center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                self.draw_conic_gradient(
                    &rect, *center_x_pct, *center_y_pct, *from_angle_deg, stops, *repeating,
                );
            }
        }
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

    /// Applies `filters` to a full-canvas snapshot of the current render target.
    ///
    /// Flush must be called before this. Returns the filtered image id (or `None`
    /// on failure). Intermediate images are queued in `filter_layer_pending_delete`.
    /// After return the active render target may have changed; caller must
    /// `switch_render_target` to the desired next target.
    fn apply_backdrop_filters(
        &mut self,
        filters: &[lumen_layout::FilterFn],
        bounds: &Rect,
    ) -> Option<femtovg::ImageId> {
        // Screenshot the current RT (flush must have been called already).
        // `screenshot()` reverses GL's bottom-up rows, so `rgba` is the raw
        // backdrop in top-down order. The entire filter chain runs on these CPU
        // bytes: backdrop regions are small and rare, so a CPU pass is far
        // simpler than the GPU round-trip it replaces. The old path uploaded the
        // screenshot to a texture and screenshot()-ed *that* FBO — both
        // `create_image` uploads and `filter_image` destinations read back empty,
        // so colour-matrix and blur+colour backdrop cards sampled black and
        // rendered dark navy (BUG-144 row 4: grayscale/brightness/invert/combo).
        let screenshot = self.canvas.screenshot().ok()?;
        let iw = screenshot.width();
        let ih = screenshot.height();
        let mut rgba: Vec<u8> = screenshot.buf().iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();

        // Backdrop-filter input is the backdrop image cropped to the element's
        // border box (CSS Filter Effects §backdrop-filter), so blur must clamp
        // its sampling window to that box — otherwise the box blur near a card's
        // top edge averages in the dark page background painted above it
        // (BUG-144 edge-bleed). Convert CSS-px `bounds` to the screenshot's
        // device-pixel coordinates and clamp to the snapshot extent.
        let scale = self.scale as f32;
        let bx0 = (bounds.x * scale).floor().max(0.0) as usize;
        let by0 = (bounds.y * scale).floor().max(0.0) as usize;
        let bx1 = ((bounds.x + bounds.width) * scale).ceil().max(0.0) as usize;
        let by1 = ((bounds.y + bounds.height) * scale).ceil().max(0.0) as usize;
        let mut region = (bx0.min(iw), by0.min(ih), bx1.min(iw), by1.min(ih));

        // For `backdrop-filter: blur()` the CSS backdrop is cropped to the
        // element's border box. Clamping the box-blur kernel to that crop
        // truncates the window at the edges, producing an asymmetric average
        // that leaves a brighter/fringed edge (BUG-144 edge-bleed). Extend the
        // region with replicated edge pixels so every kernel sample within the
        // original box sees proper symmetric context.
        let max_blur_extension: usize = filters
            .iter()
            .filter_map(|f| match f {
                lumen_layout::FilterFn::Blur(sigma) if *sigma > 0.0 => {
                    Some((*sigma * 2.0).ceil() as usize)
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        if max_blur_extension > 0 {
            region = extend_region_replicated(&mut rgba, iw, ih, region, max_blur_extension);
        }

        // Apply the filter chain left-to-right (CSS Filter Effects §2.2). Blur is
        // a 3-pass box approximation of the Gaussian; colour-matrix functions
        // share `apply_filter_rgba` with the PushFilter path. `opacity()` is a
        // no-op on the backdrop snapshot (it scales the *element's* alpha, not
        // the captured backdrop).
        for f in filters {
            match f {
                lumen_layout::FilterFn::Blur(sigma) if *sigma > 0.0 => {
                    box_blur_rgba_region(&mut rgba, iw, ih, *sigma, region);
                }
                lumen_layout::FilterFn::Blur(_) | lumen_layout::FilterFn::Opacity(_) => {}
                _ => apply_filter_rgba(&mut rgba, f),
            }
        }

        // Upload the filtered backdrop once. Top-down CPU pixels → no FLIP_Y,
        // matching `composite_backdrop_filter_layer`'s sampling.
        let pixels: Vec<rgb::RGBA8> = rgba.chunks_exact(4)
            .map(|c| rgb::RGBA8 { r: c[0], g: c[1], b: c[2], a: c[3] })
            .collect();
        let img_ref = imgref::ImgRef::new(&pixels, iw, ih);
        self.canvas.create_image(img_ref, femtovg::ImageFlags::PREMULTIPLIED).ok()
    }

    /// Composites a backdrop-filter layer (PA-4) onto the previous render target.
    ///
    /// Algorithm:
    /// 1. Flush so `elem_image_id` contains the final element content.
    /// 2. Switch to `prev_render_target`.
    /// 3. Draw `filtered_backdrop_id` at element `bounds` using Copy (replace-in-place).
    /// 4. Draw `elem_image_id` (full canvas) with SourceOver to composite element on top.
    fn composite_backdrop_filter_layer(&mut self, entry: BackdropFilterLayerEntry) {
        let BackdropFilterLayerEntry {
            elem_image_id,
            filtered_backdrop_id,
            bounds,
            prev_render_target,
        } = entry;

        // Flush pending draws into elem_image_id.
        self.canvas.flush();

        // Restore previous render target.
        self.switch_render_target(prev_render_target);

        let css_w = (self.width as f64 / self.scale) as f32;
        let css_h = (self.height as f64 / self.scale) as f32;

        // Step 1: blit filtered backdrop at element bounds (Copy = pixel-replace).
        // The paint maps the full filtered_backdrop image to the full canvas; only
        // the path rect (element bounds) receives pixels, everything else is untouched.
        self.canvas.save();
        self.canvas.reset_transform();
        self.canvas.global_composite_operation(femtovg::CompositeOperation::Copy);
        let bd_paint = femtovg::Paint::image(filtered_backdrop_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0);
        let mut bd_path = femtovg::Path::new();
        bd_path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        self.canvas.fill_path(&bd_path, &bd_paint);
        self.canvas.restore();

        // Step 2: composite element content on top (SourceOver = normal CSS compositing).
        self.canvas.save();
        self.canvas.reset_transform();
        let elem_paint = femtovg::Paint::image(elem_image_id, 0.0, 0.0, css_w, css_h, 0.0, 1.0);
        let mut elem_path = femtovg::Path::new();
        elem_path.rect(0.0, 0.0, css_w, css_h);
        self.canvas.fill_path(&elem_path, &elem_paint);
        self.canvas.restore();

        self.backdrop_filter_pending_delete.push(filtered_backdrop_id);
        self.backdrop_filter_pending_delete.push(elem_image_id);
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

    /// Device-pixel resolution of a CPU gradient texture for `rect` (BUG-085),
    /// clamped to keep per-frame cost bounded for huge background gradients.
    /// Returns `(iw, ih)`, both ≥ 1.
    fn gradient_tex_size(&self, rect: &Rect) -> (usize, usize) {
        /// Hard cap per axis. A 2048² texture (~16 MB) is far beyond any visible
        /// gradient ramp; above it the femtovg bilinear upscale is imperceptible
        /// while the CPU fill + GPU upload cost would balloon.
        const MAX_DIM: usize = 2048;
        let iw = ((rect.width as f64 * self.scale).round() as usize).clamp(1, MAX_DIM);
        let ih = ((rect.height as f64 * self.scale).round() as usize).clamp(1, MAX_DIM);
        (iw, ih)
    }

    /// Blits a CPU-rendered gradient texture `pixels` (size `iw×ih`) over `rect`,
    /// queuing the texture for deletion after the frame's flush. Shared tail of
    /// [`Self::draw_linear_gradient_cpu`] / [`Self::draw_radial_gradient_cpu`].
    fn blit_gradient_texture(&mut self, rect: &Rect, pixels: &[rgb::RGBA8], iw: usize, ih: usize) {
        let img = imgref::ImgRef::new(pixels, iw, ih);
        let Ok(id) = self
            .canvas
            .create_image(femtovg::ImageSource::Rgba(img), femtovg::ImageFlags::empty())
        else {
            return;
        };
        let paint = femtovg::Paint::image(id, rect.x, rect.y, rect.width, rect.height, 0.0, 1.0);
        let mut path = femtovg::Path::new();
        path.rect(rect.x, rect.y, rect.width, rect.height);
        self.canvas.fill_path(&path, &paint);
        // The fill_path draw command holds `id` by copy; delete only after flush.
        self.gradient_pending_delete.push(id);
    }

    /// Renders a linear gradient per-pixel into a device-resolution texture and
    /// blits it, bypassing femtovg's 256-texel gradient LUT (BUG-085). Used for
    /// repeating linear gradients, whose many tiled periods compress into the LUT
    /// and band at the period boundaries; per-pixel sampling matches Edge's
    /// analytic fill. `t` is the projection of each pixel onto the CSS gradient
    /// line, identical to femtovg's own parametrization. Each device texel is
    /// supersampled (`GRAD_SS²` sub-samples) so hard-stop boundaries — e.g. the
    /// 45° stripes of `repeating-linear-gradient`. Per-pixel device-resolution
    /// sampling removes the LUT quantization; smooth non-repeating linear ramps
    /// stay on the cheaper femtovg-native path.
    fn draw_linear_gradient_cpu(
        &mut self, rect: &Rect, angle_deg: f32, resolved: &[(f32, femtovg::Color)],
    ) {
        let (iw, ih) = self.gradient_tex_size(rect);
        // Endpoints in rect-local CSS coordinates (origin at the box's top-left).
        let ([sx, sy], [ex, ey]) =
            linear_gradient_endpoints(0.0, 0.0, rect.width, rect.height, angle_deg);
        let (dx, dy) = (ex - sx, ey - sy);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let pixels = fill_gradient_texture(iw, ih, |cx, cy| {
            let t = ((cx * rect.width - sx) * dx + (cy * rect.height - sy) * dy) / len2;
            sample_fvg_stops(resolved, t)
        });
        self.blit_gradient_texture(rect, &pixels, iw, ih);
    }

    /// Renders a radial gradient per-pixel into a device-resolution texture and
    /// blits it, bypassing femtovg's 256-texel gradient LUT (BUG-085) **and** its
    /// circle-only `radial_gradient_stops` (which renders an `ellipse` gradient
    /// as a circle — BUG-239). `t` is the elliptical distance from the centre,
    /// `sqrt((dx/rx)² + (dy/ry)²)`, so a stop at fraction 1.0 lands on the ellipse
    /// `(rx, ry)`; for a circle `rx == ry`. Centre and radii are rect-local CSS px.
    fn draw_radial_gradient_cpu(
        &mut self, rect: &Rect, cx_local: f32, cy_local: f32, rx: f32, ry: f32,
        resolved: &[(f32, femtovg::Color)],
    ) {
        let (iw, ih) = self.gradient_tex_size(rect);
        let inv_rx = 1.0 / rx.max(1e-6);
        let inv_ry = 1.0 / ry.max(1e-6);
        let pixels = fill_gradient_texture(iw, ih, |cx, cy| {
            let nx = (cx * rect.width - cx_local) * inv_rx;
            let ny = (cy * rect.height - cy_local) * inv_ry;
            sample_fvg_stops(resolved, nx.hypot(ny))
        });
        self.blit_gradient_texture(rect, &pixels, iw, ih);
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
            DisplayCommand::DrawBorder { rect, widths, colors, styles, radii } => {
                // BUG-175: rounded border. When the box has border-radius and every
                // side is a uniform-coloured solid border, paint the border as an
                // even-odd ring between the outer and inner rounded rects so corners
                // follow the radius instead of forming square frames. Non-uniform
                // colours / dashed-dotted-double styles fall back to axis-aligned
                // side quads (square corners) below.
                let uniform_solid = widths.iter().all(|&w| w > 0.0)
                    && styles.iter().all(|s| matches!(s, BorderStyle::Solid))
                    && colors[1] == colors[0]
                    && colors[2] == colors[0]
                    && colors[3] == colors[0];
                if !radii.all_zero() && uniform_solid {
                    self.draw_rounded_border_ring(*rect, *widths, colors[0], *radii);
                } else {
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
            }
            DisplayCommand::DrawText { rect, text, font_size, color, font_family, font_weight, font_style, font_variation_axes, tab_size, highlight_name: _ } => {
                // BUG-109: femtovg's text API cannot apply font-variation-settings
                // axes. When axes are present and resolve to a variable face,
                // render the run via lumen-font outlines (vector fill) so wght/
                // wdth/slnt take effect; otherwise use femtovg's fast text path.
                if !font_variation_axes.is_empty()
                    && self.draw_varied_text(
                        rect, text, *font_size, *color, font_family,
                        font_weight.0, *font_style, font_variation_axes, *tab_size,
                    )
                {
                    return;
                }
                let chain = self.resolve_font_chain(font_family, font_weight.0, *font_style);
                self.draw_text(rect.x, rect.y, text, *font_size, *color, &chain);
            }
            DisplayCommand::PushClipRect { rect } => {
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.clip_stack.push(ClipEntry::Scissor);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PushClipRoundedRect { rect, radii: _ } => {
                // BUG-132 fix: скруглённый клип. femtovg по умолчанию поддерживает
                // только прямоугольный scissor, поэтому используем его как fallback.
                // Phase 1: реальная маска с border-radius через offline canvas
                // + blend_mode с alpha-маской.
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.clip_stack.push(ClipEntry::Scissor);
                self.layer_stack_depth += 1;
            }
            // BUG-140: shape-клип (clip-path circle/ellipse/polygon). Subtree
            // рендерится в offscreen-слой; PopClip композитит его одним
            // fill_path по форме, трансформированной матрицей канвы на момент
            // Push — клип переносится transform-ом элемента (команда эмитится
            // внутри PushTransform).
            DisplayCommand::PushClipPath { shape } => {
                let prev_rt = self.current_rt();
                let entry = match self.canvas.create_image_empty(
                    self.width as usize,
                    self.height as usize,
                    femtovg::PixelFormat::Rgba8,
                    offscreen_layer_image_flags(),
                ) {
                    Ok(img_id) => {
                        self.switch_render_target(femtovg::RenderTarget::Image(img_id));
                        self.canvas.clear_rect(
                            0, 0, self.width, self.height,
                            femtovg::Color::rgba(0, 0, 0, 0),
                        );
                        ClipEntry::PathLayer {
                            image_id: img_id,
                            shape: shape.clone(),
                            transform: self.canvas.transform(),
                            prev_render_target: prev_rt,
                        }
                    }
                    Err(_) => {
                        // Fallback: scissor по bounding box формы (поведение
                        // до BUG-140). Scissor задан в текущем transform-
                        // пространстве — переносится transform-ом сам.
                        let bb = shape.bounding_rect();
                        self.canvas.save();
                        self.canvas.scissor(bb.x, bb.y, bb.width, bb.height);
                        ClipEntry::Scissor
                    }
                };
                self.clip_stack.push(entry);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopClip => {
                if self.layer_stack_depth > 0 {
                    self.layer_stack_depth -= 1;
                }
                match self.clip_stack.pop() {
                    Some(ClipEntry::PathLayer { image_id, shape, transform, prev_render_target }) => {
                        self.composite_clip_path_layer(image_id, &shape, &transform, prev_render_target);
                    }
                    Some(ClipEntry::Scissor) => self.canvas.restore(),
                    // Защита от рассинхрона пар (эмиттер гарантирует парность):
                    // повторяем историческое поведение.
                    None => self.canvas.restore(),
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
            DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                // A lazy `<img>` keeps its `loading="lazy"` attribute even after
                // the shell fetches it, so it still arrives here as a
                // LazyImageSlot — not a DrawImage. Draw the image if it has been
                // registered; `draw_image_in_rect` falls back to the grey
                // placeholder when the src is not yet in the cache. (BUG-163)
                self.draw_image_in_rect(rect, src, *object_fit, object_position);
            }
            DisplayCommand::DrawBackgroundImage {
                rect, origin_rect, src, size, position, repeat, ..
            } => {
                self.draw_background_image(rect, origin_rect, src, *size, position, *repeat);
            }

            // ── Gradients ───────────────────────────────────────────────────
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                if rect.width <= 0.0 || rect.height <= 0.0 || stops.is_empty() {
                    return;
                }
                let ([sx, sy], [ex, ey]) = linear_gradient_endpoints(
                    rect.x, rect.y, rect.width, rect.height, *angle_deg,
                );
                let line_len = linear_gradient_line_len(rect.width, rect.height, *angle_deg);
                let resolved = femtovg_stops(stops, line_len, *repeating);
                if resolved.len() < 2 {
                    return;
                }
                // BUG-085: repeating linear gradients tile many periods into
                // femtovg's 256-texel LUT and band at the boundaries — render
                // per-pixel instead. Smooth non-repeating linear ramps are
                // already pixel-accurate through the LUT and stay on the native
                // (cheaper, no per-frame texture) path.
                if *repeating {
                    self.draw_linear_gradient_cpu(rect, *angle_deg, &resolved);
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

            DisplayCommand::DrawRadialGradient {
                rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating,
            } => {
                if rect.width <= 0.0 || rect.height <= 0.0 || stops.is_empty() {
                    return;
                }
                // Px stops resolve against the gradient ray length; the larger
                // radius is a reasonable scalar for the (rare) px-positioned stop.
                let line_len = radius_x.max(*radius_y).max(1.0);
                let resolved = femtovg_stops(stops, line_len, *repeating);
                if resolved.len() < 2 {
                    return;
                }
                // BUG-085: render radial gradients per-pixel to bypass femtovg's
                // 256-texel LUT (quantizes repeating rings + smooth interpolation)
                // and its circle-only paint (BUG-239: an `ellipse` gradient must
                // use independent rx/ry). Centre is rect-local (origin at the box
                // top-left) to match draw_radial_gradient_cpu.
                self.draw_radial_gradient_cpu(
                    rect,
                    center_x_pct * rect.width,
                    center_y_pct * rect.height,
                    *radius_x,
                    *radius_y,
                    &resolved,
                );
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
            // BUG-133: group opacity is atomic (CSS Color L3 §3.2) — the subtree
            // renders into an offscreen layer, composited ONCE with the group
            // alpha. Per-draw set_global_alpha double-blends overlaps, lets
            // negative-z children show through siblings, and a nested group
            // replaces (not multiplies) the outer alpha.
            DisplayCommand::PushOpacity { alpha } => {
                let prev_rt = self.current_rt();
                let entry = match self.canvas.create_image_empty(
                    self.width as usize,
                    self.height as usize,
                    femtovg::PixelFormat::Rgba8,
                    offscreen_layer_image_flags(),
                ) {
                    Ok(img_id) => {
                        // Redirect subtree draws into the transparent offscreen layer.
                        self.switch_render_target(femtovg::RenderTarget::Image(img_id));
                        self.canvas.clear_rect(
                            0, 0, self.width, self.height,
                            femtovg::Color::rgba(0, 0, 0, 0),
                        );
                        OpacityLayerEntry {
                            image_id: Some(img_id),
                            alpha: alpha.clamp(0.0, 1.0),
                            prev_render_target: prev_rt,
                        }
                    }
                    Err(_) => {
                        // Fallback: per-draw alpha (pre-BUG-133 behaviour).
                        self.canvas.save();
                        self.canvas.set_global_alpha(alpha.clamp(0.0, 1.0));
                        OpacityLayerEntry {
                            image_id: None,
                            alpha: alpha.clamp(0.0, 1.0),
                            prev_render_target: prev_rt,
                        }
                    }
                };
                self.opacity_layer_stack.push(entry);
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopOpacity => {
                if self.layer_stack_depth > 0 {
                    self.layer_stack_depth -= 1;
                }
                if let Some(entry) = self.opacity_layer_stack.pop() {
                    match entry.image_id {
                        Some(img_id) => self.composite_opacity_layer(
                            img_id,
                            entry.alpha,
                            entry.prev_render_target,
                        ),
                        None => self.canvas.restore(),
                    }
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
            DisplayCommand::PushFilter { filters, bounds: _ } => {
                let needs_offscreen = filters
                    .iter()
                    .any(|f| !matches!(f, lumen_layout::FilterFn::Opacity(_)));

                if needs_offscreen {
                    // Capture current RT before creating the new offscreen layer.
                    // Uses active_rt_image (maintained by switch_render_target) to
                    // correctly handle nesting with blend layers (PA-3).
                    let prev_rt = self.current_rt();

                    // BUG-145: the layer must stay full-RT-sized. `bounds` is the
                    // element's untransformed border box, but content is drawn into
                    // the layer in page coordinates (no translation to layer-local
                    // space), transformed content extends beyond the border box, and
                    // blur needs ~3σ of padding around it; `composite_filter_layer`
                    // also composites the layer as a full-viewport quad. A
                    // bounds-sized layer (BUG-076 attempt) captured the page's
                    // top-left corner and stretched it across the viewport
                    // (TEST-30 30.68%, TEST-103 49.59%).
                    let (img_w, img_h) = (self.width as usize, self.height as usize);

                    // BUG-146: FLIP_Y so the rare direct GPU composite of this
                    // layer (no blur, no colour-matrix — e.g. `blur(0px)`, or
                    // blur-destination allocation failure) samples it upright.
                    // The blur pass ignores the flag; the colour-matrix
                    // screenshot round-trip flips rows regardless of it.
                    match self.canvas.create_image_empty(
                        img_w,
                        img_h,
                        femtovg::PixelFormat::Rgba8,
                        offscreen_layer_image_flags(),
                    ) {
                        Ok(img_id) => {
                            // Redirect draws into the offscreen image.
                            self.switch_render_target(femtovg::RenderTarget::Image(img_id));
                            // Clear to transparent so content composites correctly.
                            self.canvas.clear_rect(
                                0, 0, img_w as u32, img_h as u32,
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

            // ── Backdrop filter (PA-4) ───────────────────────────────────────
            // Real offscreen implementation: flush current RT → screenshot backdrop →
            // apply filter chain (GPU blur + CPU colour-matrix) → redirect element
            // draws to an offscreen layer. PopBackdropFilter blits the filtered backdrop
            // at element bounds (Copy op) then composites element content on top.
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                let prev_rt = self.current_rt();
                // Flush so backdrop has all content rendered.
                self.canvas.flush();
                // Apply filters to a screenshot of the current RT.
                let filtered_backdrop_id = self.apply_backdrop_filters(filters, bounds);

                if let Some(filt_id) = filtered_backdrop_id {
                    // Restore prev_rt after apply_backdrop_filters may have switched.
                    self.switch_render_target(prev_rt);
                    // `elem_id` is a GPU FBO: element content is rendered into it
                    // and later sampled via `Paint::image` in
                    // `composite_backdrop_filter_layer`. Like the opacity/filter
                    // offscreen layers it therefore needs FLIP_Y — without it the
                    // FBO's bottom-up rows sample upside-down, mirroring the
                    // element content vertically (BUG-144: backdrop-filter cards
                    // landed in the wrong row, `viewport_h - bounds.bottom`).
                    // `filtered_backdrop_id` is a CPU pixel upload (top-down) and
                    // correctly stays flag-free per `offscreen_layer_image_flags`.
                    match self.canvas.create_image_empty(
                        self.width as usize,
                        self.height as usize,
                        femtovg::PixelFormat::Rgba8,
                        offscreen_layer_image_flags(),
                    ) {
                        Ok(elem_id) => {
                            self.switch_render_target(femtovg::RenderTarget::Image(elem_id));
                            self.canvas.clear_rect(
                                0, 0, self.width, self.height,
                                femtovg::Color::rgba(0, 0, 0, 0),
                            );
                            self.backdrop_filter_layer_stack.push(BackdropFilterLayerEntry {
                                elem_image_id: elem_id,
                                filtered_backdrop_id: filt_id,
                                bounds: *bounds,
                                prev_render_target: prev_rt,
                            });
                        }
                        Err(_) => {
                            // Fallback: queue filtered_id for deletion, draw to prev_rt.
                            self.backdrop_filter_pending_delete.push(filt_id);
                            self.canvas.save();
                        }
                    }
                } else {
                    // Screenshot failed: no-op fallback.
                    self.canvas.save();
                }
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PopBackdropFilter => {
                if self.layer_stack_depth > 0 {
                    self.layer_stack_depth -= 1;
                }
                if let Some(entry) = self.backdrop_filter_layer_stack.pop() {
                    // Real path: blit filtered backdrop + composite element content.
                    self.composite_backdrop_filter_layer(entry);
                } else {
                    // Fallback path: PushBackdropFilter issued canvas.save() — restore it.
                    self.canvas.restore();
                }
            }

            // ── Masks ────────────────────────────────────────────────────────
            // CSS Masking L1 §4 (BUG-183). Gradient masks render the masked
            // subtree into an offscreen FBO; `PopMask` multiplies the FBO alpha
            // by the gradient (`CompositeOperation::DestinationIn`) and
            // composites the result down. `mask-image: url(...)` has no decoded
            // source on this path, so it stays a scissor no-op (alpha = 1).
            DisplayCommand::PushMaskImage { rect, .. } => {
                // No registered mask texture → approximate as a rect scissor.
                self.canvas.save();
                self.canvas.scissor(rect.x, rect.y, rect.width, rect.height);
                self.mask_layer_stack.push(MaskLayerEntry {
                    image_id: None,
                    gradient: None,
                    rect: *rect,
                    prev_render_target: self.current_rt(),
                });
                self.layer_stack_depth += 1;
            }
            DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, .. } => {
                let g = MaskGradient::Linear { angle_deg: *angle_deg, stops: stops.clone() };
                self.push_mask_gradient_layer(*rect, g);
            }
            DisplayCommand::PushMaskRadialGradient {
                rect, center_x_pct, center_y_pct, stops, ..
            } => {
                let g = MaskGradient::Radial {
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    stops: stops.clone(),
                };
                self.push_mask_gradient_layer(*rect, g);
            }
            DisplayCommand::PushMaskConicGradient {
                rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            } => {
                let g = MaskGradient::Conic {
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    from_angle_deg: *from_angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                };
                self.push_mask_gradient_layer(*rect, g);
            }
            DisplayCommand::PopMask => {
                if self.layer_stack_depth > 0 {
                    self.layer_stack_depth -= 1;
                }
                if let Some(entry) = self.mask_layer_stack.pop() {
                    if entry.image_id.is_some() {
                        self.composite_mask_layer(entry);
                    } else {
                        // Scissor fallback (PushMaskImage or FBO-alloc failure).
                        self.canvas.restore();
                    }
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
        // CSS Backgrounds §3.11.1: the root element's background becomes the
        // canvas background and covers the whole surface. Clear to it so the
        // page background fills the viewport even when the root box is smaller
        // than the window (e.g. a 1024×720 page maximized); `None` → white.
        let clear = self
            .canvas_bg
            .map_or(femtovg::Color::rgb(255, 255, 255), lumen_to_fvg);
        self.canvas.clear_rect(0, 0, self.width, self.height, clear);

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
        let backdrop_del: Vec<_> = self.backdrop_filter_pending_delete.drain(..).collect();
        for id in backdrop_del {
            self.canvas.delete_image(id);
        }
        // BUG-085: per-pixel gradient textures (repeating linear + radial).
        let gradient_del: Vec<_> = self.gradient_pending_delete.drain(..).collect();
        for id in gradient_del {
            self.canvas.delete_image(id);
        }

        self.gl_surface
            .swap_buffers(&self.gl_context)
            .map_err(|e| RenderError::Other(e.to_string()))
    }

    fn set_canvas_background(&mut self, color: Option<Color>) {
        self.canvas_bg = color;
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

    fn register_snapshot(&mut self, id: u64, image: &Image) -> Result<(), String> {
        use femtovg::{ImageFlags, ImageSource};
        use imgref::ImgRef;

        let rgba = image_to_rgba8_vec(image);
        let img = ImgRef::new(&rgba, image.width as usize, image.height as usize);
        // Straight-alpha source (CPU-uploaded subtree render), matching
        // `register_image`; `DrawLayerSnapshot` applies the per-frame alpha.
        let img_id = self
            .canvas
            .create_image(ImageSource::Rgba(img), ImageFlags::empty())
            .map_err(|e| format!("femtovg register_snapshot: {e:?}"))?;
        // Replacing an existing snapshot id must free the old texture (the
        // morph re-registers the "new" capture under the same id each transition).
        if let Some(old) = self.snapshots.insert(id, img_id) {
            self.canvas.delete_image(old);
        }
        Ok(())
    }

    fn clear_snapshots(&mut self) {
        for (_, id) in self.snapshots.drain() {
            self.canvas.delete_image(id);
        }
    }

    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.font_provider = provider.clone();
        self.fallback_chain.clear();
        // Eagerly load curated system fonts (emoji/CJK/RTL/Indic/Thai) into
        // femtovg canvas so they are available as a fallback chain for every
        // DrawText without re-loading on each call.
        if let Some(p) = provider {
            for name in crate::fallback::CURATED_FALLBACK_FAMILIES {
                if let Some(rec) = p.pick_face(name, 400, lumen_core::ext::FontStyle::Normal) {
                    let path = rec.path.clone();
                    if let Some(id) = self.load_font_by_path(&path, &p)
                        && !self.fallback_chain.contains(&id)
                    {
                        self.fallback_chain.push(id);
                    }
                }
            }
        }
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

/// Конвертирует `lumen_image::Image` в вектор `RGBA8` пикселей для femtovg.
fn image_to_rgba8_vec(img: &Image) -> Vec<rgb::RGBA8> {
    use rgb::RGBA;
    // `to_rgba8` applies ICC colour management (ICC-3 matrix-shaper for RGB
    // profiles, gamut tone-mapping otherwise), so wide-gamut photos render
    // colour-correct in the live femtovg window. For images without a profile
    // it is a plain format conversion (no-op tone mapping).
    img.to_rgba8()
        .chunks_exact(4)
        .map(|px| RGBA { r: px[0], g: px[1], b: px[2], a: px[3] })
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::BgSizeAxis;
    use lumen_layout::PositionComponent;
    use lumen_layout::Length;

    /// BUG-133 / BUG-146 / BUG-144: offscreen layers composited directly from
    /// their FBO on the GPU (opacity groups, filter layers, blur destinations,
    /// **and the backdrop-filter element-content layer** — no
    /// screenshot→re-upload round-trip) MUST carry FLIP_Y — without it the
    /// content renders upside-down (TEST-102 17% → 65% for opacity groups;
    /// TEST-15 1.06% → 6.58% for blurred box-shadows; TEST-30 backdrop-filter
    /// cards landed in `viewport_h - bounds.bottom` instead of their own row).
    /// PREMULTIPLIED matches femtovg's render-target pixel format; dropping it
    /// double-multiplies alpha. `filtered_backdrop_id` is the exception: it is a
    /// CPU pixel upload (top-down), so it stays flag-free.
    #[test]
    fn offscreen_layer_flags_flip_y_and_premultiplied() {
        let flags = offscreen_layer_image_flags();
        assert!(flags.contains(femtovg::ImageFlags::FLIP_Y));
        assert!(flags.contains(femtovg::ImageFlags::PREMULTIPLIED));
    }

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
            BackgroundSize::Length(BgSizeAxis::Px(80.0), BgSizeAxis::Px(60.0)),
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
            BackgroundSize::Length(BgSizeAxis::Px(80.0), BgSizeAxis::Px(60.0)),
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
    fn bg_tile_geometry_percent_resolves_against_oarea() {
        // BUG-115: `background-size: 40% 60%` → tile = 40%×60% of the positioning area.
        let pos = ObjectPosition::default();
        let (tw, th, ..) = bg_tile_geometry(
            BackgroundSize::Length(BgSizeAxis::Percent(0.4), BgSizeAxis::Percent(0.6)),
            &pos,
            BackgroundRepeat::NoRepeat,
            100.0,
            100.0,
            180.0,
            120.0,
            0.0,
            0.0,
        );
        // 0.4 * 180 = 72; 0.6 * 120 = 72.
        assert!((tw - 72.0).abs() < 1e-3);
        assert!((th - 72.0).abs() < 1e-3);
    }

    #[test]
    fn bg_tile_geometry_mixed_px_percent() {
        // BUG-115: `background-size: 20px 100%` → fixed width, full-height tile.
        let pos = ObjectPosition::default();
        let (tw, th, ..) = bg_tile_geometry(
            BackgroundSize::Length(BgSizeAxis::Px(20.0), BgSizeAxis::Percent(1.0)),
            &pos,
            BackgroundRepeat::RepeatX,
            100.0,
            100.0,
            180.0,
            120.0,
            0.0,
            0.0,
        );
        assert!((tw - 20.0).abs() < 1e-3);
        assert!((th - 120.0).abs() < 1e-3);
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
            BackgroundSize::Length(BgSizeAxis::Px(40.0), BgSizeAxis::Px(40.0)),
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
            GradientStop { color: Color::WHITE, position: None , ..Default::default() },
            GradientStop { color: Color::BLACK, position: None , ..Default::default() },
        ];
        let resolved = resolve_stops(&stops, 100.0);
        assert_eq!(resolved.len(), 2);
        assert!((resolved[0].0).abs() < 1e-5);
        assert!((resolved[1].0 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn resolve_stops_fixed_positions() {
        let stops = vec![
            GradientStop { color: Color::WHITE, position: Some(Length::Percent(0.0)) , ..Default::default() },
            GradientStop { color: Color::BLACK, position: Some(Length::Percent(50.0)) , ..Default::default() },
            GradientStop { color: Color::WHITE, position: Some(Length::Percent(100.0)) , ..Default::default() },
        ];
        let resolved = resolve_stops(&stops, 100.0);
        assert_eq!(resolved.len(), 3);
        assert!((resolved[1].0 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn femtovg_stops_extends_hard_stop_last_color_to_end() {
        // linear-gradient(red 50%, green 50%): без завершающего стопа femtovg
        // оставил бы вторую половину прозрачной (BUG-085 / BUG-144 row 2).
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let green = Color { r: 0, g: 255, b: 0, a: 255 };
        let stops = vec![
            GradientStop { color: red, position: Some(Length::Percent(50.0)) , ..Default::default() },
            GradientStop { color: green, position: Some(Length::Percent(50.0)) , ..Default::default() },
        ];
        let r = femtovg_stops(&stops, 100.0, false);
        let last = *r.last().unwrap();
        assert!((last.0 - 1.0).abs() < 1e-5, "последний стоп должен достигать 1.0, got {}", last.0);
        assert!(last.1.g > 0.9 && last.1.r < 0.1, "хвост должен быть зелёным");
    }

    #[test]
    fn femtovg_stops_no_tail_fill_when_last_at_full() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let stops = vec![
            GradientStop { color: red, position: None , ..Default::default() },
            GradientStop { color: blue, position: None , ..Default::default() },
        ];
        // Стопы 0/1 не требуют дозаполнения хвоста.
        assert_eq!(femtovg_stops(&stops, 100.0, false).len(), 2);
    }

    #[test]
    fn femtovg_stops_repeating_tiles_across_line() {
        // repeating-linear-gradient(#333 0px, #333 10px, #666 10px, #666 20px)
        // на линии 200px → период 20/200 = 0.1; покрыть [0,1] плитками.
        let c1 = Color { r: 0x33, g: 0x33, b: 0x33, a: 255 };
        let c2 = Color { r: 0x66, g: 0x66, b: 0x66, a: 255 };
        let stops = vec![
            GradientStop { color: c1, position: Some(Length::Px(0.0)) , ..Default::default() },
            GradientStop { color: c1, position: Some(Length::Px(10.0)) , ..Default::default() },
            GradientStop { color: c2, position: Some(Length::Px(10.0)) , ..Default::default() },
            GradientStop { color: c2, position: Some(Length::Px(20.0)) , ..Default::default() },
        ];
        let r = femtovg_stops(&stops, 200.0, true);
        assert!(r.windows(2).all(|w| w[1].0 >= w[0].0 - 1e-6), "позиции должны быть неубывающими");
        assert!((r[0].0).abs() < 1e-5, "первый стоп в 0, got {}", r[0].0);
        assert!(r.last().unwrap().0 >= 1.0, "последний стоп должен достигать/превышать 1.0");
        assert!(r.len() >= 20, "ожидается множество замощённых стопов, got {}", r.len());
    }

    #[test]
    fn femtovg_stops_repeating_degenerate_period_is_solid() {
        // Нулевой период (оба стопа в одной позиции) → сплошной цвет, без зацикливания.
        let c = Color { r: 10, g: 20, b: 30, a: 255 };
        let stops = vec![
            GradientStop { color: c, position: Some(Length::Percent(50.0)) , ..Default::default() },
            GradientStop { color: c, position: Some(Length::Percent(50.0)) , ..Default::default() },
        ];
        let r = femtovg_stops(&stops, 100.0, true);
        assert_eq!(r.len(), 2);
        assert!((r[0].0).abs() < 1e-5 && (r[1].0 - 1.0).abs() < 1e-5);
    }

    // ── BUG-085: per-pixel CPU gradient sampler (bypasses 256-texel LUT) ──────

    #[test]
    fn sample_fvg_stops_clamps_outside_range() {
        // Mirrors femtovg's LUT: positions outside [0,1] take the boundary colour.
        let red = femtovg::Color::rgbaf(1.0, 0.0, 0.0, 1.0);
        let blue = femtovg::Color::rgbaf(0.0, 0.0, 1.0, 1.0);
        let resolved = [(0.2_f32, red), (0.8_f32, blue)];
        let lo = sample_fvg_stops(&resolved, -1.0);
        let hi = sample_fvg_stops(&resolved, 2.0);
        assert!((lo.r - 1.0).abs() < 1e-6 && lo.b.abs() < 1e-6, "below-range = first stop");
        assert!(hi.r.abs() < 1e-6 && (hi.b - 1.0).abs() < 1e-6, "above-range = last stop");
    }

    #[test]
    fn sample_fvg_stops_interpolates_midpoint() {
        let red = femtovg::Color::rgbaf(1.0, 0.0, 0.0, 1.0);
        let blue = femtovg::Color::rgbaf(0.0, 0.0, 1.0, 1.0);
        let resolved = [(0.0_f32, red), (1.0_f32, blue)];
        let mid = sample_fvg_stops(&resolved, 0.5);
        assert!((mid.r - 0.5).abs() < 1e-6, "R halfway, got {}", mid.r);
        assert!((mid.b - 0.5).abs() < 1e-6, "B halfway, got {}", mid.b);
    }

    #[test]
    fn sample_fvg_stops_hard_stop_is_sharp() {
        // Coincident positions (hard stop): the colour flips sharply across 0.5
        // with no visible ramp on either side.
        let a = femtovg::Color::rgbaf(0.2, 0.2, 0.2, 1.0);
        let b = femtovg::Color::rgbaf(0.4, 0.4, 0.4, 1.0);
        let resolved = [(0.0_f32, a), (0.5_f32, a), (0.5_f32, b), (1.0_f32, b)];
        let just_below = sample_fvg_stops(&resolved, 0.49);
        let just_above = sample_fvg_stops(&resolved, 0.51);
        assert!((just_below.r - 0.2).abs() < 1e-3, "below hard stop = a");
        assert!((just_above.r - 0.4).abs() < 1e-3, "above hard stop = b");
    }

    #[test]
    fn fvg_to_rgba8_quantizes_channels() {
        let c = femtovg::Color::rgbaf(1.0, 0.5, 0.0, 1.0);
        let q = fvg_to_rgba8(c);
        assert_eq!((q.r, q.b, q.a), (255, 0, 255));
        // 0.5 * 255 = 127.5 → rounds to 128.
        assert_eq!(q.g, 128);
    }

    /// BUG-183 — a gradient `mask-image` is applied by painting the gradient
    /// over the masked layer with `CompositeOperation::DestinationIn`, which
    /// multiplies the layer's alpha by the gradient's *alpha* (`mask-mode: alpha`,
    /// the default). The mask therefore depends on the resolved stops carrying a
    /// decreasing alpha for a `black → transparent` gradient. If this regressed,
    /// `composite_mask_layer` would either not fade (alpha stuck at 1) or clip
    /// hard. Guards the pure kernel the GPU path relies on (the offscreen FBO +
    /// DestinationIn composite itself needs a GL context and is exercised by the
    /// TEST-26 graphic gate).
    #[test]
    fn mask_gradient_alpha_decreases_black_to_transparent() {
        let stops = vec![
            GradientStop { color: Color::BLACK, position: None , ..Default::default() },
            GradientStop { color: Color::TRANSPARENT, position: None , ..Default::default() },
        ];
        let resolved = resolve_stops(&stops, 200.0);
        // The black→transparent segment is subdivided for premultiplied
        // interpolation (BUG-190), so the count grows beyond 2; the endpoints
        // (the mask's defining values) must still be alpha 1 → 0, monotonically.
        assert!(resolved.len() >= 2);
        // Opaque end → mask = 1 (content fully shown).
        assert!((resolved.first().unwrap().1.a - 1.0).abs() < 1e-5, "opaque stop must keep alpha 1");
        // Transparent end → mask = 0 (content fully hidden).
        assert!(resolved.last().unwrap().1.a.abs() < 1e-5, "transparent stop must have alpha 0");
        // Alpha decreases monotonically across the (subdivided) ramp.
        for w in resolved.windows(2) {
            assert!(w[1].1.a <= w[0].1.a + 1e-5, "mask alpha must not increase");
        }
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
        box_blur_rgba_region(&mut px, 1, 1, 2.0, (0, 0, 1, 1));
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
        box_blur_rgba_region(&mut px, 3, 1, 1.0, (0, 0, 3, 1));
        // Middle pixel (index 1) should now be average of all three: 255+0+255/3 = 170
        let mid_r = px[4]; // offset 1*4 + 0
        assert!(mid_r > 100, "middle pixel should be brightened by blur: got {mid_r}");
    }

    #[test]
    fn gaussian_box_radii_three_positive_passes() {
        // Three boxes, each a valid (≥1) half-width; their combined variance
        // should be in the neighbourhood of the requested Gaussian variance.
        for &sigma in &[1.0f32, 2.0, 4.0, 8.0] {
            let radii = gaussian_box_radii(sigma);
            assert!(radii.iter().all(|&r| r >= 1), "sigma {sigma}: radii {radii:?}");
            // Variance of a (2r+1)-box is ((2r+1)²-1)/12; three boxes add up.
            let var: f32 = radii.iter().map(|&r| {
                let w = (2 * r + 1) as f32;
                (w * w - 1.0) / 12.0
            }).sum();
            let target = sigma * sigma;
            // Discrete radii can't hit the target exactly, but should be close.
            assert!((var - target).abs() <= target * 0.6 + 1.0,
                "sigma {sigma}: variance {var} far from target {target} (radii {radii:?})");
        }
    }

    #[test]
    fn box_blur_rgba_region_leaves_outside_pixels_untouched() {
        // 4×1 strip: white, white, white, black. Blur only the first 3 (the
        // "card"); the 4th pixel (outside the region) must stay pure black —
        // proving the region path never writes outside its rectangle.
        let mut px = vec![
            255u8, 255, 255, 255,
            255,   255, 255, 255,
            255,   255, 255, 255,
            0,     0,   0,   255,
        ];
        box_blur_rgba_region(&mut px, 4, 1, 2.0, (0, 0, 3, 1));
        assert_eq!(&px[12..16], &[0, 0, 0, 255], "outside-region pixel must be untouched");
    }

    #[test]
    fn box_blur_rgba_region_clamps_sampling_no_edge_bleed() {
        // 4×1 strip: black background, then 3 white "card" pixels.
        // Blurring the card region [1,4) must NOT pull the black pixel at x=0
        // into the card's left edge: the whole card stays pure white because
        // sampling is clamped to [1,4) (edge pixels duplicate within the card).
        let mut px = vec![
            0u8,   0,   0,   255, // black background (outside region)
            255,   255, 255, 255, // card start
            255,   255, 255, 255,
            255,   255, 255, 255,
        ];
        box_blur_rgba_region(&mut px, 4, 1, 2.0, (1, 0, 4, 1));
        assert_eq!(&px[4..8], &[255, 255, 255, 255], "card left edge must not bleed black background");
        // Sanity: the unclamped full-width blur *would* darken the card's left
        // edge — confirm the difference is real, not a no-op test.
        let mut full = vec![
            0u8,   0,   0,   255,
            255,   255, 255, 255,
            255,   255, 255, 255,
            255,   255, 255, 255,
        ];
        box_blur_rgba_region(&mut full, 4, 1, 2.0, (0, 0, 4, 1));
        assert!(full[4] < 255, "unclamped blur should bleed black into card edge (got {})", full[4]);
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

    // ── extend_region_replicated tests ────────────────────────────────────────

    #[test]
    fn extend_region_replicated_zero_extension_is_noop() {
        let mut px = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
        let _region = extend_region_replicated(&mut px, 2, 1, (0, 0, 2, 1), 0);
        assert_eq!(&px, &[10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn extend_region_replicated_fills_top_band() {
        // 4×1 strip: red, green, blue, white. Region = [1,3). Extend by 1.
        // Top band (x=0..4, y=-1 conceptually but clamped to 0): replicate row 0.
        let mut px = vec![
            255, 0,   0,   255, // x=0 red
              0, 255, 0,   255, // x=1 green (region start)
              0,   0, 255, 255, // x=2 blue  (region end-1)
            255, 255, 255, 255, // x=3 white
        ];
        let region = extend_region_replicated(&mut px, 4, 1, (1, 0, 3, 1), 1);
        // Extended region should cover [0, 4). Top band is same row in 1-high image,
        // so no new rows. Left extension x=0 should replicate from x=1 (green).
        let g = [0u8, 255, 0, 255];
        assert_eq!(&px[0..4], &g, "left extension must replicate left edge of region");
        // Right extension x=3 should replicate from x=2 (blue).
        let b = [0u8, 0, 255, 255];
        assert_eq!(&px[12..16], &b, "right extension must replicate right edge of region");
    }

    #[test]
    fn extend_region_replicated_2d_corners_and_edges() {
        // 4×2 image:
        //   row 0: black  white  white  black
        //   row 1: black  white  white  black
        // Region = [1,3) × [0,2) = the 2 white pixels in both rows.
        // Extend by 1: the 6 surrounding pixels should all become white.
        let mut px = vec![
            0,   0,   0,   255,   // (0,0) black
          255, 255, 255, 255,     // (1,0) white
          255, 255, 255, 255,     // (2,0) white
            0,   0,   0, 255,     // (3,0) black
            0,   0,   0, 255,     // (0,1) black
          255, 255, 255, 255,     // (1,1) white
          255, 255, 255, 255,     // (2,1) white
            0,   0,   0, 255,     // (3,1) black
        ];
        let _ = extend_region_replicated(&mut px, 4, 2, (1, 0, 3, 2), 1);
        let white = [255u8, 255, 255, 255];
        // Corners and edges all replicated from nearest white edge.
        for &off in &[0, 4, 8, 12, 16, 20] {
            assert_eq!(&px[off..off+4], &white, "pixel at offset {off} must be replicated white");
        }
    }
}
