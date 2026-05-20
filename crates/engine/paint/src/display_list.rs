//! Display list — линейный список графических команд, выработанных из
//! дерева layout. Растеризатору (renderer) уже не нужно понимать DOM/CSS:
//! он рендерит то, что ему говорят.
//!
//! Phase 0 — только `FillRect` и `DrawText`. Тени, скругления, градиенты,
//! border-радиусы — позже, по запросу. Координаты — экранные пиксели от
//! верхнего левого угла окна.

use lumen_core::geom::Rect;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, forward_box_transform,
    transform_fns_to_matrix, CompositorAnimFrame,
    BackgroundClip, BackgroundImage, BorderStyle, BoxKind, Color, CssColor, FontStyle, FontWeight,
    InlineFrag, LayoutBox, Mat4, MixBlendMode as LayoutBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, PaintOrder, PaintPhase, PositionComponent,
    StackingContextId, StackingTree, TextDecorationStyle, TextDecorationThickness, TextOverflow,
    Visibility,
};

/// CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
/// `Normal` (no-op); остальные 16 mode-ов парсятся в CSS-каскаде, но
/// реальный composite-pipeline для них — задача P2 п.4 (mix-blend-mode).
/// `PlusLighter` — из CSS Compositing & Blending L2 §6, реализуется
/// как additive compositing с pre-multiplied alpha.
/// Хранится в `DisplayCommand::PushBlendMode` как stub-значение, чтобы
/// расширить enum без правки потребителей.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PlusLighter,
}

impl BlendMode {
    /// Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
    /// Compositing & Blending L1 §5). Case-insensitive — `MULTIPLY` и
    /// `multiply` оба возвращают `Multiply`. Возвращает `None` на
    /// нераспознанной строке; caller (CSS-каскад) трактует это как
    /// invalid declaration и применяет initial value (`Normal`).
    #[must_use]
    pub fn from_keyword(s: &str) -> Option<Self> {
        // ASCII case fold — keyword-ы CSS все ASCII, дешёвый match
        // через to_ascii_lowercase в стек-буфер не нужен (хватает
        // `eq_ignore_ascii_case`).
        for (kw, mode) in [
            ("normal", Self::Normal),
            ("multiply", Self::Multiply),
            ("screen", Self::Screen),
            ("overlay", Self::Overlay),
            ("darken", Self::Darken),
            ("lighten", Self::Lighten),
            ("color-dodge", Self::ColorDodge),
            ("color-burn", Self::ColorBurn),
            ("hard-light", Self::HardLight),
            ("soft-light", Self::SoftLight),
            ("difference", Self::Difference),
            ("exclusion", Self::Exclusion),
            ("hue", Self::Hue),
            ("saturation", Self::Saturation),
            ("color", Self::Color),
            ("luminosity", Self::Luminosity),
            ("plus-lighter", Self::PlusLighter),
        ] {
            if s.eq_ignore_ascii_case(kw) {
                return Some(mode);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect {
        rect: Rect,
        color: Color,
    },
    DrawBorder {
        rect: Rect,
        /// Ширины сторон: [top, right, bottom, left].
        widths: [f32; 4],
        /// Цвета сторон: [top, right, bottom, left].
        colors: [Color; 4],
        /// Стили сторон: [top, right, bottom, left]. CSS Backgrounds L3 §6.
        /// `None` обычно фильтруется emit-side через `is_visible()`, в команду
        /// попадает Solid / Dashed / Dotted (по текущему `BorderStyle` enum).
        /// Renderer разворачивает Dashed/Dotted в pattern из штрихов / точек.
        styles: [BorderStyle; 4],
    },
    /// CSS Basic UI L4 §5 — `outline`. Рисуется СНАРУЖИ box-а (в отличие
    /// от border, который часть box-model), не занимает место в layout,
    /// может перекрывать соседей и не ловит pointer-события. `rect` —
    /// исходная коробка box-а (renderer сам расширит её на `offset` и
    /// `width`). `style` ≠ None / Hidden — иначе emit не происходит.
    /// `color` уже разрешён в конкретный `Color` на emission-стороне
    /// (Auto / CurrentColor резолвится в `style.color`).
    /// Phase 0: renderer рисует `Auto` как Solid (UA focus ring без хвоста).
    /// `Dashed`/`Dotted` реализованы через `emit_outline_side`. `Double`
    /// маппится на Solid в `parse_outline_style_opt` (нет отдельного variant-а).
    DrawOutline {
        rect: Rect,
        width: f32,
        style: OutlineStyle,
        color: Color,
        offset: f32,
    },
    DrawText {
        rect: Rect,
        text: String,
        font_size: f32,
        color: Color,
        /// CSS Fonts L4 §3.1 — приоритизированный список имён семейств.
        /// Пустой Vec означает «никакой явной family-инструкции» — renderer
        /// использует bundled-шрифт (Inter Regular). Renderer перебирает имена
        /// через `FontProvider::pick_face`; первый найденный face побеждает.
        font_family: Vec<String>,
        /// CSS-вес 1..1000. По умолчанию 400 (Regular). Передаётся в
        /// `FontProvider::pick_face`; алгоритм матчинга — CSS Fonts L4 §5.2.
        font_weight: FontWeight,
        /// `font-style`. По умолчанию Normal.
        font_style: FontStyle,
        /// CSS Fonts L4 §7 — user-space variation axes из `font-variation-settings`.
        /// Пары `(tag, value)` в user units — нормализация через fvar+avar
        /// выполняется в renderer-е, который имеет доступ к шрифтовым таблицам.
        /// Пустой Vec = `normal` (default-instance без variation deltas).
        font_variation_axes: Vec<([u8; 4], f32)>,
    },
    /// Растровое изображение из `<img>`. `rect` — итоговая коробка после
    /// расчёта по CSS (width/height + HTML presentational hints), `src` —
    /// строка ссылки на ресурс из исходного атрибута (декодирование и
    /// загрузка пикселей — отдельная задача, см. roadmap). `alt` — alternate
    /// text для случаев, когда renderer не может отобразить картинку.
    /// `object_fit` / `object_position` (CSS Images L3 §5.5) определяют,
    /// как intrinsic-размер изображения вписывается в `rect`; renderer
    /// читает их вместе с известным intrinsic-размером (доступен на
    /// GPU-cache стороне) для расчёта итогового quad.
    ///
    /// Renderer Phase 0 рисует placeholder rect (светло-серый прямоугольник),
    /// если картинка не зарегистрирована в GPU-cache.
    DrawImage {
        rect: Rect,
        src: String,
        alt: String,
        object_fit: ObjectFit,
        object_position: ObjectPosition,
    },
    /// CSS Backgrounds L3 §3.10 — `background-image: url(...)`. `rect` —
    /// background-painting area из [`background_clip_rect`] (учитывает
    /// `background-clip`: border-box / padding-box / content-box). `src` —
    /// URL картинки, тот же ключ, что shell кладёт в `Renderer::register_image`.
    ///
    /// Эмиттер выпускает ТОЛЬКО для `BackgroundImage::Url(_)` (gradient-ы
    /// парсятся, но Phase 0 не растрит — см. `style.background_image`).
    /// Порядок: после `FillRect` для background-color, до border (CSS
    /// Backgrounds L3 §3.10 — painting order: bg-color → bg-image → border).
    ///
    /// Phase 0 ограничения (renderer Stretches картинку на весь `rect`):
    /// * `background-size` игнорируется (де-факто `100% 100%`).
    /// * `background-position` / `background-origin` игнорируются (0,0).
    /// * `background-repeat` игнорируется (картинка не тайлится).
    /// * `background-attachment: fixed` не поддерживается (rect скроллится).
    ///
    /// Если картинка не зарегистрирована в GPU-cache — команда визуально
    /// no-op (background-color уже эмитнут отдельным FillRect).
    DrawBackgroundImage {
        rect: Rect,
        src: String,
    },
    /// Sprint 0 P2 stub. Открывает rect-клип: все последующие команды до
    /// парного `PopClip` рисуются только в пределах `rect`. Используется
    /// для `overflow: hidden`, `clip-path: inset(...)`. Phase 0: эмиттер
    /// в `build_display_list` не выпускает, renderer игнорирует. Когда
    /// P1 п.2A (stacking contexts impl) заполнит данные, эмиттер начнёт
    /// выпускать; до этого момента — interface-first stub.
    PushClipRect { rect: Rect },
    /// Закрывает rect-клип, открытый ближайшим `PushClipRect`. Парность
    /// гарантируется эмиттером.
    PopClip,
    /// Sprint 0 P2 stub. Открывает opacity-группу: все последующие
    /// команды до парного `PopOpacity` композитятся как off-screen-layer
    /// и накладываются с `alpha`. Используется для `opacity != 1`. Phase 0:
    /// эмиттер не выпускает (нужен compositor с layer-pipeline-ом —
    /// roadmap-задача), renderer игнорирует.
    PushOpacity { alpha: f32 },
    /// Закрывает opacity-группу.
    PopOpacity,
    /// Открывает blend-группу с указанным режимом смешения
    /// (CSS Compositing & Blending L1 §5). Все последующие команды до
    /// парного `PopBlendMode` применяются поверх родительского контекста
    /// через `mode`. `BlendMode::Normal` — стандартный alpha-over (no-op).
    /// Phase 0: renderer отслеживает стек через `current_blend_mode()`,
    /// но использует Normal pipeline для всех режимов; реальный pipeline
    /// switch — P2 1B.4.
    PushBlendMode { mode: BlendMode },
    /// Закрывает blend-группу.
    PopBlendMode,
    /// Рисует ранее загруженный GPU-снимок слоя (см. `Renderer::upload_layer_snapshot`)
    /// как текстурированный quad в `rect`. UV покрывает весь снимок ([0,0]→[1,1]).
    /// `alpha` — финальная прозрачность (0.0=прозрачный, 1.0=непрозрачный).
    /// Если снимок с `id` не зарегистрирован — команда молча игнорируется.
    /// Используется compositor-ом для повторного использования неизменных слоёв.
    DrawLayerSnapshot { id: u64, rect: Rect, alpha: f32 },
    /// CSS Transforms L1 §13 — открывает transform-группу. Все последующие
    /// команды до парного `PopTransform` рисуются с применением `matrix` к
    /// координатам вершин (forward-матрица в viewport-системе, уже включает
    /// `T(pivot)·M·T(-pivot)` по `transform-origin`). Phase 0 — 2D affine:
    /// translate / rotate / scale / skew / matrix2d. Z/W-колонки игнорируются.
    ///
    /// Стек transform-ов в renderer-е перемножается с предыдущим топом, что
    /// корректно отражает CSS-семантику вложенных трансформов (каждый transform
    /// создаёт SC и применяется к собственному поддереву + детям).
    ///
    /// Phase 0 ограничения:
    /// - `PushClipRect` под не-identity transform-ом использует axis-aligned
    ///   bounding box трансформированного rect-а как scissor — корректно
    ///   только для translate-чистых трансформов; rotate/scale могут потерять
    ///   точность по краям. Полноценный clip через clip-mask — P2 п.4+.
    /// - DrawBorder / DrawOutline эмитят 4 axis-aligned rect-а под стороны;
    ///   при rotate они трансформируются по-отдельности, что выглядит
    ///   корректно для translate/scale, но может рассинхронизировать стыки
    ///   углов при больших углах rotate. Mitre-углы — отдельная задача.
    PushTransform { matrix: Mat4 },
    /// Закрывает transform-группу.
    PopTransform,
}

pub type DisplayList = Vec<DisplayCommand>;

fn object_fit_name(f: ObjectFit) -> &'static str {
    match f {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::None => "none",
        ObjectFit::ScaleDown => "scale-down",
    }
}

fn position_component_name(p: PositionComponent) -> String {
    match p {
        PositionComponent::Px(px) => format!("{px:.2}px"),
        PositionComponent::Percent(pc) => format!("{:.2}%", pc * 100.0),
    }
}

/// CSS Images L3 §5.5 — `object-fit` placement: где располагается
/// «полное» изображение внутри коробки (intrinsic-картинка после scale,
/// без обрезки). Возвращённый прямоугольник может быть больше `box_rect`
/// (cover / none на крупной картинке) — обрезку по box делает
/// [`fit_image_quad`] на стадии вычисления GPU-quad-а.
///
/// `intrinsic_size = (w, h)` — натуральный пиксельный размер декодированного
/// изображения; нулевые / отрицательные стороны коробки → возврат самой
/// коробки без масштабирования (deg fallback, рисовать всё равно нечего).
#[must_use]
pub fn fit_image_rect(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Rect {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return box_rect;
    }
    let iw = iw as f32;
    let ih = ih as f32;
    let bw = box_rect.width;
    let bh = box_rect.height;

    let (cw, ch) = match fit {
        ObjectFit::Fill => (bw, bh),
        ObjectFit::None => (iw, ih),
        ObjectFit::Contain => fit_with_ratio(iw, ih, bw, bh, /*cover*/ false),
        ObjectFit::Cover => fit_with_ratio(iw, ih, bw, bh, /*cover*/ true),
        ObjectFit::ScaleDown => {
            // `min(none, contain)` — выбираем результат с меньшей площадью.
            let (nw, nh) = (iw, ih);
            let (kw, kh) = fit_with_ratio(iw, ih, bw, bh, false);
            if nw * nh <= kw * kh { (nw, nh) } else { (kw, kh) }
        }
    };

    let free_x = bw - cw;
    let free_y = bh - ch;
    let off_x = position.x.resolve(free_x);
    let off_y = position.y.resolve(free_y);
    Rect::new(box_rect.x + off_x, box_rect.y + off_y, cw, ch)
}

fn fit_with_ratio(iw: f32, ih: f32, bw: f32, bh: f32, cover: bool) -> (f32, f32) {
    // contain = min(scale_w, scale_h); cover = max(...).
    let sx = bw / iw;
    let sy = bh / ih;
    let s = if cover { sx.max(sy) } else { sx.min(sy) };
    (iw * s, ih * s)
}

/// Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
/// (см. [`fit_image_rect`]) с `box_rect` плюс соответствующие UV-bounds
/// исходной текстуры. Спецификация CSS Images L3 §5.5 требует «clipped to
/// the content box» — для cover / none, когда картинка выходит за коробку,
/// мы делаем clip через UV (рисуем меньший quad с поджатыми UV), без
/// scissor-state в GPU pipeline.
///
/// Возвращает `None`, если intrinsic-размер нулевой, коробка пуста или
/// пересечение placement и box пусто (placement полностью снаружи box —
/// в норме не случается, но возможны deg-edge с отрицательным
/// `object-position`).
#[must_use]
pub fn fit_image_quad(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Option<(Rect, [f32; 2], [f32; 2])> {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return None;
    }
    let placed = fit_image_rect(box_rect, intrinsic_size, fit, position);
    if placed.width <= 0.0 || placed.height <= 0.0 {
        return None;
    }
    let bx0 = box_rect.x;
    let by0 = box_rect.y;
    let bx1 = box_rect.x + box_rect.width;
    let by1 = box_rect.y + box_rect.height;
    let px0 = placed.x;
    let py0 = placed.y;
    let px1 = placed.x + placed.width;
    let py1 = placed.y + placed.height;
    let vx0 = px0.max(bx0);
    let vy0 = py0.max(by0);
    let vx1 = px1.min(bx1);
    let vy1 = py1.min(by1);
    if vx1 <= vx0 || vy1 <= vy0 {
        return None;
    }
    let visible = Rect::new(vx0, vy0, vx1 - vx0, vy1 - vy0);
    let u0 = (vx0 - px0) / placed.width;
    let v0 = (vy0 - py0) / placed.height;
    let u1 = (vx1 - px0) / placed.width;
    let v1 = (vy1 - py0) / placed.height;
    Some((visible, [u0, v0], [u1, v1]))
}

/// Сериализует display list в детерминированный текст для snapshot-тестов.
///
/// Формат (одна команда — одна строка):
/// - `FillRect (x.xx, y.xx, w.xx, h.xx) #rrggbbaa`
/// - `DrawBorder (x.xx, y.xx, w.xx, h.xx) w=[t,r,b,l] c=[#top,#right,#bottom,#left]`
///   плюс `s=[t,r,b,l]` если хоть один стиль ≠ Solid (bw-compat: чистый
///   Solid-border печатается как раньше, snapshot-ы не ломаются).
/// - `DrawText (x.xx, y.xx, w.xx, h.xx) "text" fs.xx #rrggbbaa`
///
/// Сокращённый префикс `BorderStyle` для snapshot-сериализатора.
/// None уже фильтруется emit-side, но обрабатываем для устойчивости.
fn border_style_short(s: BorderStyle) -> &'static str {
    match s {
        BorderStyle::None => "n",
        BorderStyle::Solid => "s",
        BorderStyle::Dashed => "da",
        BorderStyle::Dotted => "do",
        BorderStyle::Double => "db",
    }
}

pub fn serialize_display_list(dl: &[DisplayCommand]) -> String {
    let mut out = String::new();
    for cmd in dl {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                out.push_str(&format!(
                    "FillRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawBorder {
                rect,
                widths: [wt, wr, wb, wl],
                colors: [ct, cr, cb, cl],
                styles: [st, sr, sb, sl],
            } => {
                out.push_str(&format!(
                    "DrawBorder ({:.2}, {:.2}, {:.2}, {:.2}) \
                     w=[{:.2},{:.2},{:.2},{:.2}] \
                     c=[#{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x},\
                        #{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x}]",
                    rect.x, rect.y, rect.width, rect.height,
                    wt, wr, wb, wl,
                    ct.r, ct.g, ct.b, ct.a,
                    cr.r, cr.g, cr.b, cr.a,
                    cb.r, cb.g, cb.b, cb.a,
                    cl.r, cl.g, cl.b, cl.a,
                ));
                let any_non_solid = ![*st, *sr, *sb, *sl]
                    .iter()
                    .all(|s| matches!(s, BorderStyle::Solid | BorderStyle::None));
                if any_non_solid {
                    out.push_str(&format!(
                        " s=[{},{},{},{}]",
                        border_style_short(*st),
                        border_style_short(*sr),
                        border_style_short(*sb),
                        border_style_short(*sl),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::DrawText {
                rect, text, font_size, color, font_family, font_weight, font_style,
                font_variation_axes,
            } => {
                out.push_str(&format!(
                    "DrawText ({:.2}, {:.2}, {:.2}, {:.2}) {:?} {:.2} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    text,
                    font_size,
                    color.r, color.g, color.b, color.a,
                ));
                if !font_family.is_empty() {
                    out.push_str(" family=[");
                    for (i, name) in font_family.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&format!("{name:?}"));
                    }
                    out.push(']');
                }
                if *font_weight != FontWeight::NORMAL {
                    out.push_str(&format!(" w={}", font_weight.0));
                }
                if *font_style != FontStyle::Normal {
                    out.push_str(match font_style {
                        FontStyle::Italic => " style=italic",
                        FontStyle::Oblique => " style=oblique",
                        FontStyle::Normal => "",
                    });
                }
                if !font_variation_axes.is_empty() {
                    out.push_str(" var=[");
                    for (i, (tag, val)) in font_variation_axes.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let tag_str = std::str::from_utf8(tag).unwrap_or("????");
                        out.push_str(&format!("{tag_str:?}={val}"));
                    }
                    out.push(']');
                }
                out.push('\n');
            }
            DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
                out.push_str(&format!(
                    "DrawOutline ({:.2}, {:.2}, {:.2}, {:.2}) w={:.2} \
                     s={} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    width,
                    outline_style_name(*style),
                    color.r, color.g, color.b, color.a,
                ));
                if *offset != 0.0 {
                    out.push_str(&format!(" off={offset:.2}"));
                }
                out.push('\n');
            }
            DisplayCommand::DrawImage { rect, src, alt, object_fit, object_position } => {
                out.push_str(&format!(
                    "DrawImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} alt={alt:?}",
                    rect.x, rect.y, rect.width, rect.height,
                ));
                if *object_fit != ObjectFit::Fill {
                    out.push_str(&format!(" fit={}", object_fit_name(*object_fit)));
                }
                if *object_position != ObjectPosition::default() {
                    out.push_str(&format!(
                        " pos={} {}",
                        position_component_name(object_position.x),
                        position_component_name(object_position.y),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::DrawBackgroundImage { rect, src } => {
                out.push_str(&format!(
                    "DrawBackgroundImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushClipRect { rect } => {
                out.push_str(&format!(
                    "PushClipRect ({:.2}, {:.2}, {:.2}, {:.2})\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PopClip => {
                out.push_str("PopClip\n");
            }
            DisplayCommand::PushOpacity { alpha } => {
                out.push_str(&format!("PushOpacity {alpha:.3}\n"));
            }
            DisplayCommand::PopOpacity => {
                out.push_str("PopOpacity\n");
            }
            DisplayCommand::PushBlendMode { mode } => {
                out.push_str(&format!("PushBlendMode {}\n", blend_mode_name(*mode)));
            }
            DisplayCommand::PopBlendMode => {
                out.push_str("PopBlendMode\n");
            }
            DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                out.push_str(&format!(
                    "DrawLayerSnapshot id={id} ({:.2}, {:.2}, {:.2}, {:.2}) alpha={alpha:.3}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushTransform { matrix } => {
                // 2D affine: x'=a·x+c·y+e, y'=b·x+d·y+f. Печатаем 6 значимых
                // компонент в snapshot-friendly формате — детерминированный
                // обход, не зависящий от Z/W-колонок (Phase 0 — 2D).
                let m = &matrix.0;
                let a = m[0];
                let b = m[1];
                let c = m[4];
                let d = m[5];
                let e = m[12];
                let f = m[13];
                out.push_str(&format!(
                    "PushTransform [{a:.3} {b:.3} {c:.3} {d:.3} {e:.3} {f:.3}]\n"
                ));
            }
            DisplayCommand::PopTransform => {
                out.push_str("PopTransform\n");
            }
        }
    }
    out
}

fn outline_style_name(s: OutlineStyle) -> &'static str {
    match s {
        OutlineStyle::None => "none",
        OutlineStyle::Auto => "auto",
        OutlineStyle::Solid => "solid",
        OutlineStyle::Dashed => "dashed",
        OutlineStyle::Dotted => "dotted",
    }
}

fn blend_mode_name(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        BlendMode::PlusLighter => "plus-lighter",
    }
}

pub fn build_display_list(root: &LayoutBox) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list);
    list
}

/// Like `build_display_list` but applies compositor animation overrides per node.
///
/// For each node that has an entry in `anim`, opacity and/or transform values
/// from the override replace the style's values in the emitted PushOpacity /
/// PushTransform commands. Layout geometry (rect, padding, children) is unchanged —
/// this avoids a full relayout while still producing correct frames.
///
/// Pass `None` (or an empty frame) to fall back to the same output as
/// `build_display_list`.
pub fn build_display_list_with_anim(
    root: &LayoutBox,
    anim: Option<&CompositorAnimFrame>,
) -> DisplayList {
    let mut list = Vec::new();
    walk_with_anim(root, anim, &mut list);
    list
}

/// Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E).
///
/// Разница с [`build_display_list`]: для документа с несколькими
/// stacking-контекстами child-SC рисуются в правильных слотах parent SC
/// (negative-z до контента, auto/0 и positive-z после).
///
/// Phase 0 упрощение: фазы `BlockBackgrounds` / `Floats` / `InlineContent`
/// лумпятся в один «контент» bucket per SC, эмитимый при фазе
/// `InlineContent`. Точное разделение по фазам 3/4/5 (block vs float vs
/// inline-level descendant) — отдельная задача после flex / float layout.
///
/// Bucket-per-SC структура:
/// - `pre`: layer-ops, открываемые при входе в SC (PushOpacity / PushBlendMode
///   / PushClipRect) — собственный SC-owner с `opacity<1` / `mix-blend-mode`
///   ≠ normal / `overflow` ≠ visible.
/// - `root_bg`: bg/border SC-owner box-а (фаза 1 «RootBackground»).
/// - `contents`: всё остальное содержимое SC (descendants, исключая собственно
///   SC-creating потомков — те идут в свои buckets).
/// - `post`: парные Pop-команды, в обратном порядке к `pre`.
///
/// **Phase 0 ограничение для layer-ops:** `pre` / `post` SC-owner-а охватывают
/// только `root_bg + contents` собственного SC, **не** child-SC потомков (они
/// рисуются после `InlineContent` parent-SC в линейном порядке, а `post` уже
/// эмитится в той же `InlineContent`-фазе). Для строгой семантики
/// `opacity / blend-mode` родителя на child-SC потребуется либо stack-based
/// эмиссия с явным end-of-SC маркером в `PaintOrder`, либо группировка
/// child-SC внутри parent-bucket. Renderer сейчас всё равно игнорирует
/// Push/Pop (роадмап P2 п.1B шаг (c) — реальный layer-pipeline), так что
/// текущая эмиссия — interface-level: парность сохранена, потребители
/// (compositor) видят сами триггеры; уточнение охвата child-SC — отдельный
/// шаг при реальном compositor pipeline.
pub fn build_display_list_ordered(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
) -> DisplayList {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true);

    let mut out = Vec::new();
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                out.append(&mut bucket.pre);
                out.append(&mut bucket.root_bg);
            }
            PaintPhase::InlineContent => {
                out.append(&mut bucket.contents);
                out.append(&mut bucket.post);
            }
            // Phase 0: BlockBackgrounds / Floats merged into InlineContent;
            // marker-фазы (NegativeZ / PositionedAndZAuto / PositiveZ) в
            // выводе `PaintOrder::from_tree` не появляются — рекурсия
            // энкодирует их позицию через линейный порядок.
            _ => {}
        }
    }
    out
}

#[derive(Default, Clone)]
struct ScBucket {
    /// PushOpacity / PushBlendMode / PushClipRect — открывают layer-effects
    /// SC-owner-а перед собственным фоном.
    pre: Vec<DisplayCommand>,
    /// CSS 2.1 Appendix E phase 1 — bg/border SC-owner box-а.
    root_bg: Vec<DisplayCommand>,
    /// Фазы 3/4/5 — descendants SC-owner-а кроме child-SC-creating box-ов.
    contents: Vec<DisplayCommand>,
    /// Pop* в обратном порядке к `pre`. Эмитится после `contents` в фазе
    /// `InlineContent`. См. Phase 0 ограничение в docstring
    /// `build_display_list_ordered`.
    post: Vec<DisplayCommand>,
}

/// CSS Compositing & Blending L1 §5: маппинг style-уровневого `MixBlendMode`
/// (lumen-layout) в paint-уровневый `BlendMode` (lumen-paint). Enum-ы
/// разные, чтобы не тянуть зависимость paint → layout в обратную сторону;
/// варианты совпадают 1:1.
fn map_blend_mode(m: LayoutBlendMode) -> BlendMode {
    match m {
        LayoutBlendMode::Normal => BlendMode::Normal,
        LayoutBlendMode::Multiply => BlendMode::Multiply,
        LayoutBlendMode::Screen => BlendMode::Screen,
        LayoutBlendMode::Overlay => BlendMode::Overlay,
        LayoutBlendMode::Darken => BlendMode::Darken,
        LayoutBlendMode::Lighten => BlendMode::Lighten,
        LayoutBlendMode::ColorDodge => BlendMode::ColorDodge,
        LayoutBlendMode::ColorBurn => BlendMode::ColorBurn,
        LayoutBlendMode::HardLight => BlendMode::HardLight,
        LayoutBlendMode::SoftLight => BlendMode::SoftLight,
        LayoutBlendMode::Difference => BlendMode::Difference,
        LayoutBlendMode::Exclusion => BlendMode::Exclusion,
        LayoutBlendMode::Hue => BlendMode::Hue,
        LayoutBlendMode::Saturation => BlendMode::Saturation,
        LayoutBlendMode::Color => BlendMode::Color,
        LayoutBlendMode::Luminosity => BlendMode::Luminosity,
        LayoutBlendMode::PlusLighter => BlendMode::PlusLighter,
    }
}

/// CSS Overflow L3 §3.2: значения, при которых overflow создаёт clip-bound
/// для содержимого. `Visible` не клипает.
fn overflow_clips(o: Overflow) -> bool {
    matches!(
        o,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    )
}

/// Em-fraction for approximating U+2026 HORIZONTAL ELLIPSIS advance width.
/// Empirically derived from Inter Regular; the outer overflow:hidden clip
/// prevents pixel bleed if the renderer's actual advance differs slightly.
const ELLIPSIS_EM: f32 = 0.65;

/// Emits shadow + DrawText + decorations for every visible frag in `line`.
fn emit_text_frags(
    line: &[InlineFrag],
    container_x: f32,
    container_width: f32,
    line_y: f32,
    line_h: f32,
    out: &mut Vec<DisplayCommand>,
) {
    for frag in line {
        if !matches!(frag.style.visibility, Visibility::Visible) {
            continue;
        }
        let base_rect = Rect::new(container_x + frag.x, line_y, container_width, line_h);
        emit_text_shadows(out, base_rect, line_h, frag);
        out.push(DisplayCommand::DrawText {
            rect: base_rect,
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color: frag.style.color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: frag
                .style
                .font_variation_settings
                .iter()
                .map(|a| (a.tag, a.value))
                .collect(),
        });
        push_text_decoration(out, container_x, line_y, frag);
    }
}

/// Renders all lines of a [`BoxKind::InlineRun`].
///
/// When `text-overflow: ellipsis` (CSS UI L4 §3) is active on the box style
/// AND a line's text extends past `b.rect.width`, the line is rendered with:
/// 1. A [`DisplayCommand::PushClipRect`] narrowed by the ellipsis glyph width.
/// 2. Normal text emission inside the clip.
/// 3. [`DisplayCommand::PopClip`].
/// 4. A [`DisplayCommand::DrawText`] "…" at the clip boundary.
///
/// Requires `overflow_x != visible` on the box (CSS UI L4 §3 precondition).
/// The parent block's overflow:hidden clip ensures no pixel escapes the container.
fn emit_inline_run(b: &LayoutBox, lines: &[Vec<InlineFrag>], out: &mut Vec<DisplayCommand>) {
    let line_h = b.style.font_size * b.style.line_height;
    let wants_ellipsis = matches!(b.style.text_overflow, TextOverflow::Ellipsis)
        && overflow_clips(b.style.overflow_x);

    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = b.rect.y + line_idx as f32 * line_h;

        // Phase 1: inline frag backgrounds (under text).
        for frag in line.iter() {
            if !matches!(frag.style.visibility, Visibility::Visible) {
                continue;
            }
            emit_inline_frag_box(out, b.rect.x, line_y, line_h, frag);
        }

        // Detect text-overflow: find first visible frag that extends past container.
        let overflow_frag = if wants_ellipsis {
            line.iter().find(|f| {
                matches!(f.style.visibility, Visibility::Visible)
                    && f.x + f.width > b.rect.width
            })
        } else {
            None
        };

        // Phase 2: text — with or without ellipsis clip.
        if let Some(ef) = overflow_frag {
            let ew = ef.style.font_size * ELLIPSIS_EM;
            let clip_w = (b.rect.width - ew).max(0.0);
            out.push(DisplayCommand::PushClipRect {
                rect: Rect::new(b.rect.x, line_y, clip_w, line_h),
            });
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, out);
            out.push(DisplayCommand::PopClip);
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(b.rect.x + clip_w, line_y, ew, line_h),
                text: "\u{2026}".to_string(),
                font_size: ef.style.font_size,
                color: ef.style.color,
                font_family: ef.style.font_family.clone(),
                font_weight: ef.style.font_weight,
                font_style: ef.style.font_style,
                font_variation_axes: ef
                    .style
                    .font_variation_settings
                    .iter()
                    .map(|a| (a.tag, a.value))
                    .collect(),
            });
        } else {
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, out);
        }
    }
}

/// Собирает layer-effect триггеры одного box-а в pair (pre, post).
/// Push-команды складываются в `pre` в порядке, парные `Pop` в `post` —
/// в обратном порядке (LIFO). Возвращает пустые векторы для боксов без
/// триггеров **или для анонимных боксов** (InlineRun / Skip), у которых
/// нет своего DOM-элемента, к которому компилятор стиля привязал бы
/// triggering свойство.
///
/// Симметрия с `box_can_own_stacking_context` / `box_can_own_property_node`:
/// анонимные InlineRun-ы клонируют style родителя (включая opacity и
/// overflow), и эмиссия layer-ops для них дала бы фантомные парные
/// Push/Pop поверх настоящих от parent-Block-а. Та же защита здесь.
///
/// Триггеры:
/// - `opacity < 1.0` → `PushOpacity { alpha } / PopOpacity`.
/// - `mix-blend-mode != Normal` → `PushBlendMode { mode } / PopBlendMode`.
/// - `overflow-x / overflow-y` ∈ {hidden, clip, scroll, auto} →
///   `PushClipRect { rect: b.rect } / PopClip`.
/// - `transform != []` → `PushTransform { matrix } / PopTransform`.
///   Matrix считается через `forward_box_transform`: T(pivot)·M·T(-pivot)
///   в viewport-координатах, pivot = b.rect.origin + transform_origin.
///
/// Порядок Push-команд (для child compositor-а смысла не несёт, но
/// детерминирован для тестируемости): Clip → Blend → Opacity → Transform.
/// Pop — в обратном (Transform → Opacity → Blend → Clip). Transform пушится
/// последним, чтобы преобразовывать всё содержимое SC (включая собственные
/// background/border бокса, эмитимые в `root_bg`).
fn box_layer_ops(b: &LayoutBox) -> (Vec<DisplayCommand>, Vec<DisplayCommand>) {
    let mut pre = Vec::new();
    let mut post = Vec::new();
    if !box_can_own_stacking_context(b) {
        return (pre, post);
    }
    let s = &b.style;

    if overflow_clips(s.overflow_x) || overflow_clips(s.overflow_y) {
        pre.push(DisplayCommand::PushClipRect { rect: b.rect });
        post.push(DisplayCommand::PopClip);
    }
    if s.mix_blend_mode != LayoutBlendMode::Normal {
        pre.push(DisplayCommand::PushBlendMode {
            mode: map_blend_mode(s.mix_blend_mode),
        });
        post.push(DisplayCommand::PopBlendMode);
    }
    if s.opacity < 1.0 {
        pre.push(DisplayCommand::PushOpacity { alpha: s.opacity });
        post.push(DisplayCommand::PopOpacity);
    }
    if let Some(matrix) = forward_box_transform(b) {
        pre.push(DisplayCommand::PushTransform { matrix });
        post.push(DisplayCommand::PopTransform);
    }
    // post в LIFO порядке относительно pre.
    post.reverse();
    (pre, post)
}

/// Walk-функция, идентичная по триггерам `StackingTree::build`: pre-order,
/// SC-id присваивается монотонно при обнаружении SC-creating потомка.
/// Boxes без SC-trigger остаются в `current_sc`.
///
/// Layer-ops эмиссия:
/// - Для SC-owner (`is_sc_root == true`) Push идёт в `bucket.pre`, Pop в
///   `bucket.post`.
/// - Для non-SC box-а (typically `overflow: hidden` без других триггеров —
///   opacity/blend сами триггерят SC) Push/Pop эмитятся inline в
///   `bucket.contents` вокруг собственного contents-emit-а и потомков.
fn fill_buckets(
    b: &LayoutBox,
    current_sc: StackingContextId,
    next_sc_id: &mut u32,
    buckets: &mut [ScBucket],
    is_sc_root: bool,
) {
    let (pre_ops, post_ops) = box_layer_ops(b);

    if is_sc_root {
        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.pre.extend(pre_ops);
        emit_box_self(b, &mut bucket.root_bg);
        // `post` эмитится в фазе InlineContent после descendants — заполним
        // его сейчас, чтобы не повторно вычислять триггеры.
        bucket.post.extend(post_ops);

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false);
            }
        }
    } else {
        // Non-SC box: inline Push/Pop в contents текущего SC. Это нужно для
        // `overflow:hidden` на обычном in-flow box-е (opacity/blend
        // триггерят SC сами, до сюда не дойдут с не-пустым pre_ops).
        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.contents.extend(pre_ops);
        emit_box_self(b, &mut bucket.contents);

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false);
            }
        }

        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.contents.extend(post_ops);
    }
}

/// Если у box-а видимый `outline` — эмитит `DrawOutline`. Caller гарантирует
/// правильный порядок (outline рисуется ПОВЕРХ контента box-а и его детей,
/// но в **рамках своей stacking phase** — Phase 0 без точного разделения
/// фаз outline эмитится сразу после background/border bounding-box-а у
/// `emit_box_self` и после children в `walk`, чтобы потомки не закрывали
/// его пиксели в случае negative `outline-offset`).
///
/// Per CSS Basic UI L4 §5.4: `OutlineColor::Auto` / `CurrentColor`
/// резолвятся в `style.color` (Phase 0 без UA contrast-цвета).
/// Эмитит per-fragment text-shadow DrawText-команды ПЕРЕД основным
/// DrawText. Несколько теней в списке: spec CSS Text Decoration L3 §6
/// — «the first shadow is on top, subsequent shadows are layered
/// behind it», что в painter's order означает обратный обход
/// (последний рисуется первым, первый — последним за основным
/// текстом). Phase 0 — без `blur`: тень = тот же текст со смещением
/// Рисует фон и рамку inline-элемента для одного `InlineFrag`.
///
/// `container_x` — левый край InlineRun-бокса.
/// `frag.x` — смещение текста от container_x (уже учитывает padding_left + border_left).
/// Фон рисуется от border-box левого края до border-box правого края.
fn emit_inline_frag_box(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    line_y: f32,
    line_h: f32,
    frag: &InlineFrag,
) {
    if !frag.is_element_box {
        return;
    }
    let s = &frag.style;
    let bl = s.border_left_width;
    let br = s.border_right_width;
    let bt = s.border_top_width;
    let bb = s.border_bottom_width;

    // Border-box left edge = text_x - padding_left - border_left.
    let box_x = container_x + frag.x - frag.padding_left - bl;
    // Border-box width = border_left + padding_left + text + padding_right + border_right.
    let box_w = bl + frag.padding_left + frag.width + frag.padding_right + br;
    let box_h = line_h;
    let box_y = line_y;

    // Background (CSS Backgrounds L3: painted over padding+border area).
    if let Some(CssColor::Rgba(bg)) = s.background_color
        && bg.a > 0
        && box_w > 0.0
    {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(box_x, box_y, box_w, box_h),
            color: bg,
        });
    }

    // Border.
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if has_border && box_w > 0.0 {
        let cur = s.color;
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(box_x, box_y, box_w, box_h),
            widths: [bt, br, bb, bl],
            colors: [
                s.border_top_color.resolve(cur),
                s.border_right_color.resolve(cur),
                s.border_bottom_color.resolve(cur),
                s.border_left_color.resolve(cur),
            ],
            styles: [
                s.border_top_style,
                s.border_right_style,
                s.border_bottom_style,
                s.border_left_style,
            ],
        });
    }
}

/// (offset_x, offset_y) и shadow.color (None → currentColor =
/// frag.style.color).
fn emit_text_shadows(
    out: &mut Vec<DisplayCommand>,
    base_rect: Rect,
    line_h: f32,
    frag: &InlineFrag,
) {
    if frag.style.text_shadow.is_empty() {
        return;
    }
    for shadow in frag.style.text_shadow.iter().rev() {
        let color = shadow.color.unwrap_or(frag.style.color);
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                base_rect.x + shadow.offset_x,
                base_rect.y + shadow.offset_y,
                base_rect.width,
                line_h,
            ),
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: frag.style.font_variation_settings
                .iter().map(|s| (s.tag, s.value)).collect(),
        });
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip` clip rect для фона.
/// Phase 0 (без border-radius — углы прямоугольные):
/// * `BorderBox` (initial): `b.rect` без изменений.
/// * `PaddingBox`: shrink на border-widths по всем сторонам.
/// * `ContentBox`: shrink на border + padding.
/// * `Text` (L4): Phase 0 fallback на `BorderBox` (реальный glyph-mask
///   clip требует off-screen alpha-pass, P2 п.4+).
///
/// `max(0.0)` страхует от negative-w/h на очень узких box-ах.
fn background_clip_rect(b: &LayoutBox) -> Rect {
    let s = &b.style;
    match s.background_clip {
        BackgroundClip::BorderBox | BackgroundClip::Text => b.rect,
        BackgroundClip::PaddingBox => Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
        ),
        BackgroundClip::ContentBox => Rect::new(
            b.rect.x + s.border_left_width + s.padding_left.px(),
            b.rect.y + s.border_top_width + s.padding_top.px(),
            (b.rect.width
                - s.border_left_width
                - s.border_right_width
                - s.padding_left.px()
                - s.padding_right.px())
            .max(0.0),
            (b.rect.height
                - s.border_top_width
                - s.border_bottom_width
                - s.padding_top.px()
                - s.padding_bottom.px())
            .max(0.0),
        ),
    }
}

/// CSS Backgrounds L3 §3.10 — эмитит `background-image: url(...)` поверх
/// background-color и под border-ом (см. painter's order). Gradient-вариант
/// `BackgroundImage::Gradient` Phase 0 не растрит — парсер сохранил строку,
/// renderer её игнорирует до отдельной задачи.
///
/// Использует [`background_clip_rect`] для определения области рисования —
/// идентично тому, как тот же clip применяется к background-color FillRect-у.
/// Пустой rect (width/height ≤ 0) — no-op: GPU всё равно отбракует, а
/// сэкономим память display list-а.
fn emit_background_image(out: &mut Vec<DisplayCommand>, b: &LayoutBox) {
    if let BackgroundImage::Url(src) = &b.style.background_image
        && !src.is_empty()
    {
        let clip = background_clip_rect(b);
        if clip.width > 0.0 && clip.height > 0.0 {
            out.push(DisplayCommand::DrawBackgroundImage { rect: clip, src: src.clone() });
        }
    }
}

/// Эмитит outset box-shadow ПЕРЕД background (painter's order по CSS
/// Backgrounds L3 §4.6 — shadow «cast … behind the element», то есть
/// под background-color). Phase 0:
/// * `blur` игнорируется — требует off-screen Gaussian pass (P2 п.4+);
///   shadow = резкий FillRect со смещением и spread.
/// * `inset` тени рисуются отдельно — `emit_inset_box_shadows` после
///   background и до border, по спеке §3.5.1 «inset shadows are drawn
///   inside the box, above the background and below the border».
/// * Multiple shadows: per spec «the first shadow is on top» —
///   эмитим в reverse iter (последняя в CSS-списке рисуется первой /
///   ниже всех, первая — последней-перед-background).
/// * `spread`: расширяет / сжимает rect ± по всем сторонам перед
///   смещением. Полностью схлопывающийся rect (w/h ≤ 0) — skip.
/// * Полностью прозрачная shadow (color.a == 0) — skip.
fn emit_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    for shadow in s.box_shadow.iter().rev() {
        if shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        let x = b.rect.x + shadow.offset_x - shadow.spread;
        let y = b.rect.y + shadow.offset_y - shadow.spread;
        let w = b.rect.width + 2.0 * shadow.spread;
        let h = b.rect.height + 2.0 * shadow.spread;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y, w, h),
            color,
        });
    }
}

/// Эмитит inset box-shadow МЕЖДУ background и border (CSS Backgrounds
/// L3 §3.5.1: «inset shadows are drawn inside the padding edge of the
/// box, above the background but below the border and content»).
///
/// Геометрия per spec:
/// * **outer** = padding-box (border-rect минус border-widths) — это
///   область, в которой видна тень; тень клипается outer-ом.
/// * **inner** = `outer`, **смещённый** на `(offset_x, offset_y)` и
///   **сжатый** на `spread` (положительный spread → меньший inner →
///   шире кольцо тени; отрицательный spread → inner может выйти за
///   outer → тень коллапсирует к нулю).
///
/// Видимая тень = `outer \ (inner ∩ outer)` — кольцо/каёмка. Phase 0
/// без border-radius / blur разворачивается в 4 FillRect-а (top /
/// bottom / left / right), окаймляющие «дырку» внутри outer. Если
/// inner полностью НЕ пересекается с outer — заливаем весь outer
/// одним FillRect (тень закрывает всё). Если inner полностью покрывает
/// outer (отрицательный spread достаточной величины) — ничего не
/// эмитим.
///
/// Multiple inset shadows: тот же reverse-iter, что у outset — «first
/// shadow on top» (последняя в CSS-списке кладётся первой, первая —
/// последней; верхние перекрывают нижние). Несколько inset друг над
/// другом — нормальный паттерн под «двойную» обводку.
///
/// Phase 0 ограничения, совпадающие с outset:
/// * `blur` игнорируется (нужен Gaussian pass).
/// * Полностью прозрачная shadow (`color.a == 0`) — skip.
/// * `currentColor` для `color: None` берётся из `s.color`.
fn emit_inset_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    let outer_x = b.rect.x + s.border_left_width;
    let outer_y = b.rect.y + s.border_top_width;
    let outer_w = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let outer_h = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    if outer_w <= 0.0 || outer_h <= 0.0 {
        return;
    }
    let outer_right = outer_x + outer_w;
    let outer_bottom = outer_y + outer_h;
    for shadow in s.box_shadow.iter().rev() {
        if !shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        // inner = outer, translated by offset, then inset by spread.
        let inner_x = outer_x + shadow.offset_x + shadow.spread;
        let inner_y = outer_y + shadow.offset_y + shadow.spread;
        let inner_right = outer_right + shadow.offset_x - shadow.spread;
        let inner_bottom = outer_bottom + shadow.offset_y - shadow.spread;
        // Inner полностью покрывает outer — кольцо нулевое, тени не видно.
        if inner_x <= outer_x
            && inner_y <= outer_y
            && inner_right >= outer_right
            && inner_bottom >= outer_bottom
        {
            continue;
        }
        // Inner не пересекает outer — тень покрывает весь outer.
        let no_overlap = inner_x >= outer_right
            || inner_y >= outer_bottom
            || inner_right <= outer_x
            || inner_bottom <= outer_y;
        if no_overlap {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, outer_h),
                color,
            });
            continue;
        }
        // Hole = inner clamped to outer.
        let hole_left = inner_x.max(outer_x);
        let hole_top = inner_y.max(outer_y);
        let hole_right = inner_right.min(outer_right);
        let hole_bottom = inner_bottom.min(outer_bottom);
        // Top frame.
        if hole_top > outer_y {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, hole_top - outer_y),
                color,
            });
        }
        // Bottom frame.
        if hole_bottom < outer_bottom {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_bottom, outer_w, outer_bottom - hole_bottom),
                color,
            });
        }
        // Left frame.
        if hole_left > outer_x {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_top, hole_left - outer_x, hole_bottom - hole_top),
                color,
            });
        }
        // Right frame.
        if hole_right < outer_right {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(
                    hole_right,
                    hole_top,
                    outer_right - hole_right,
                    hole_bottom - hole_top,
                ),
                color,
            });
        }
    }
}

fn emit_outline(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if !s.outline_style.is_visible() || s.outline_width <= 0.0 {
        return;
    }
    let color = match s.outline_color {
        OutlineColor::Color(c) => c,
        OutlineColor::Auto | OutlineColor::CurrentColor => s.color,
    };
    out.push(DisplayCommand::DrawOutline {
        rect: b.rect,
        width: s.outline_width,
        style: s.outline_style,
        color,
        offset: s.outline_offset.px(),
    });
}

/// CSS Display L3 §4 — `visibility: hidden` (и `collapse` для не-table
/// per spec) делает box-self **не-рисуемым** (background, border,
/// outline, box-shadow, content), но layout остаётся (`Skip` иной
/// семантики). Children по-прежнему обходятся: visibility наследуется,
/// но child может явно вернуть себя через `visibility: visible`.
fn is_paint_visible(b: &LayoutBox) -> bool {
    matches!(b.style.visibility, Visibility::Visible)
}

/// CSS Color L3 §3.2 — `opacity: 0` создаёт stacking context, и после
/// off-screen compositor pass весь subtree даёт fully-transparent
/// результат. Phase 0 без compositor-pass-ов: pure-pixel skip всего
/// subtree (children тоже не рисуются — это отличие от visibility:
/// hidden, где children могут override через `:visible`). Сравнение
/// `<= 0.0` страхует от sub-normal значений, попавших в opacity
/// через клипанг — layout cascade clamp-ит в `[0.0, 1.0]`, но
/// defensive check дешёвый. opacity > 0 && < 1 Phase 0 не обрабатывается
/// (требует off-screen pass с per-pixel alpha multiply — P2 п.4+).
fn is_opacity_subtree_painted(b: &LayoutBox) -> bool {
    b.style.opacity > 0.0
}

/// Эмитит DisplayCommand-ы для одного box-а БЕЗ рекурсии в детей. Аналог
/// тела `walk` для одного box-а.
fn emit_box_self(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    // opacity:0 → whole-subtree invisible (см. is_opacity_subtree_painted).
    // emit_box_self не идёт в children, но self-content тоже skip-аем.
    if !is_opacity_subtree_painted(b) {
        return;
    }
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b);
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                });
            }
            emit_outline(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, out);
        }
        BoxKind::InlineBlockRow | BoxKind::InlineSpace => {}
        BoxKind::Image { src, alt } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b);
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                });
            }
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
                src: src.clone(),
                alt: alt.clone(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
            });
            emit_outline(b, out);
        }
    }
}

fn walk(b: &LayoutBox, out: &mut DisplayList) {
    // CSS Color L3 §3.2 — opacity:0 на box-е делает весь subtree после
    // composite полностью прозрачным. Phase 0 эмулирует это pure-pixel
    // skip-ом (отличие от visibility:hidden, где children могут
    // override через `:visible` — opacity-0 такого override не имеет).
    if !is_opacity_subtree_painted(b) {
        return;
    }
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            // CSS Color L3 §3: opacity < 1.0 creates compositing layer.
            let has_opacity = b.style.opacity < 1.0; // >0.0 already checked above
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: b.style.opacity });
            }
            // CSS Transforms L1 §13: forward-матрица применяется до родителя,
            // т.е. PushTransform — ВНУТРИ opacity-layer-а. Применяется ко
            // всему содержимому box-а (включая собственный background/border).
            let transform = forward_box_transform(b);
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }
            // CSS Display L3 §4 — `visibility: hidden`: self не рисуется
            // (фон/border/outline/shadow), но children обходятся (inherited
            // visibility, но child может вернуть себя через `:visible`).
            let self_visible = is_paint_visible(b);
            if self_visible {
                emit_box_shadows(b, out);
                if let Some(CssColor::Rgba(bg)) = b.style.background_color
                    && bg.a > 0
                {
                    let clip = background_clip_rect(b);
                    if clip.width > 0.0 && clip.height > 0.0 {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    }
                }
                emit_background_image(out, b);
                emit_inset_box_shadows(b, out);
                let s = &b.style;
                let has_border = s.border_top_style.is_visible()
                    || s.border_right_style.is_visible()
                    || s.border_bottom_style.is_visible()
                    || s.border_left_style.is_visible();
                if has_border {
                    let cur = s.color;
                    out.push(DisplayCommand::DrawBorder {
                        rect: b.rect,
                        widths: [
                            s.border_top_width, s.border_right_width,
                            s.border_bottom_width, s.border_left_width,
                        ],
                        colors: [
                            s.border_top_color.resolve(cur),
                            s.border_right_color.resolve(cur),
                            s.border_bottom_color.resolve(cur),
                            s.border_left_color.resolve(cur),
                        ],
                        styles: [
                            s.border_top_style, s.border_right_style,
                            s.border_bottom_style, s.border_left_style,
                        ],
                    });
                }
            }
            for child in &b.children {
                walk(child, out);
            }
            if self_visible {
                // CSS Basic UI L4 §5: outline рисуется поверх контента box-а
                // (включая children), снаружи bounding-box-а. Phase 0 без
                // деления paint phases для outline — эмитим в конце box-walk-а.
                emit_outline(b, out);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
        }
        BoxKind::InlineBlockRow => {
            // Анонимный контейнер: нет фона/бордера собственного.
            // Просто рекурсивно рисуем всех дочерних (BoxKind::Block).
            for child in &b.children {
                walk(child, out);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, out);
        }
        BoxKind::Image { src, alt } => {
            // visibility:hidden на `<img>` пропускает всё (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order для replaced element: фон → bg-image → border → <img>.
            // background/border у `<img>` валидны по CSS — например, для
            // подложки на время загрузки или рамки вокруг картинки.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b);
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                });
            }
            // Image content внутри padding/border-области; в Phase 0
            // padding/border ещё не сжимают content-area Image (только
            // расширяют коробку), `rect` — полная коробка вместе с border.
            // object-fit / object-position читаются на render-стадии вместе
            // с известным intrinsic-размером изображения.
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
                src: src.clone(),
                alt: alt.clone(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
            });
            emit_outline(b, out);
        }
    }
}

/// Эмитит FillRect-ы для активных линий text-decoration. Геометрия —
/// приблизительная: baseline ≈ line_y + font_size * 0.80 (соответствует
/// ascent ratio Inter, на котором рендерер позиционирует глифы). Толщина
/// резолвится через [`resolve_decoration_thickness`] из
/// `text-decoration-thickness` (L3 §2.3). Стиль (`Solid` / `Double` /
/// `Dotted` / `Dashed` / `Wavy`, L3 §2.2) разворачивается в один или
/// несколько FillRect-ов через [`emit_decoration_line`]. Цвет — из
/// `text-decoration-color` с fallback на currentColor (L3 §3).
fn push_text_decoration(out: &mut DisplayList, container_x: f32, line_y: f32, frag: &InlineFrag) {
    let decoration = frag.style.text_decoration_line;
    if decoration.is_empty() || frag.width <= 0.0 {
        return;
    }
    let fs = frag.style.font_size;
    let baseline_y = line_y + fs * 0.80;
    let thickness = resolve_decoration_thickness(frag.style.text_decoration_thickness, fs);
    let style = frag.style.text_decoration_style;
    let x = container_x + frag.x;
    let color = frag.style.text_decoration_color.resolve(frag.style.color);

    if decoration.underline {
        let y = baseline_y + fs * 0.10;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
    if decoration.line_through {
        let y = baseline_y - fs * 0.30;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
    if decoration.overline {
        let y = baseline_y - fs * 0.78;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
}

/// Резолвит [`TextDecorationThickness`] в device-px по CSS Text Decoration
/// L3 §2.3. `Auto` / `FromFont` — UA дефолт ≈ 7% от font-size (минимум
/// 1px); Phase 0 без font-access для `FromFont`, поэтому тот же default.
/// `Length` — уже resolved-px из cascade. `Percentage` хранится как
/// fraction; spec ссылается на 1em **parent** font-size, Phase 0
/// используем frag.font_size как приближение (документировано в
/// `style.rs`).
fn resolve_decoration_thickness(value: TextDecorationThickness, font_size: f32) -> f32 {
    match value {
        TextDecorationThickness::Auto | TextDecorationThickness::FromFont => {
            (font_size * 0.07).max(1.0)
        }
        TextDecorationThickness::Length(px) => px.max(0.0),
        TextDecorationThickness::Percentage(frac) => (frac * font_size).max(0.0),
    }
}

/// Эмитит FillRect-ы для одной decoration-линии в выбранном стиле
/// (CSS Text Decoration L3 §2.2). `(x, y)` — верхний левый угол.
///
/// - `Solid` — один rect (initial).
/// - `Double` — два параллельных rect-а с gap = thickness; итого
///   span ≈ 3 × thickness, верхний у `y`, нижний у `y + 2·t`.
/// - `Dotted` — серия квадратиков `thickness × thickness`, шаг
///   `2 × thickness` (gap = thickness). Геометрия UA-defined; выбран
///   простой 1:1 паттерн.
/// - `Dashed` — серия штрихов длиной `2 × thickness`, шаг `3 × thickness`
///   (gap = thickness). UA-defined.
/// - `Wavy` — синусоидальная волна аппроксимируется серией узких
///   axis-aligned столбцов (renderer pipeline без curves): сдвиг
///   центра толщины по `dy = sin(2π · rel_x / λ) · A`, где
///   `A = WAVY_AMPLITUDE_FACTOR · thickness`, `λ =
///   WAVY_WAVELENGTH_FACTOR · thickness`. Шаг между columns =
///   `max(1, thickness · 0.5)` — компромисс между визуальной
///   гладкостью и числом FillRect-ов (≈ 2 sample / thickness CSS px).
///   Толщина каждого column = thickness, ширина = step (или остаток
///   до `x + width`). Видимый ascent/descent от baseline = `A + t/2`.
fn emit_decoration_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
) {
    if width <= 0.0 || thickness <= 0.0 {
        return;
    }
    match style {
        TextDecorationStyle::Solid => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Wavy => {
            emit_wavy_line(out, x, y, width, thickness, color);
        }
        TextDecorationStyle::Double => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y + 2.0 * thickness, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Dotted => {
            let step = thickness * 2.0;
            let end = x + width;
            let mut cx = x;
            while cx + thickness <= end + f32::EPSILON {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, thickness, thickness),
                    color,
                });
                cx += step;
            }
        }
        TextDecorationStyle::Dashed => {
            let dash = thickness * 2.0;
            let step = thickness * 3.0;
            let end = x + width;
            let mut cx = x;
            while cx < end {
                let w = (end - cx).min(dash);
                if w <= 0.0 {
                    break;
                }
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, w, thickness),
                    color,
                });
                cx += step;
            }
        }
    }
}

/// Амплитуда волны в долях `thickness` — peak-deviation центра от
/// baseline в обе стороны. 1.5×thickness даёт ясно различимую волну
/// без излишнего вертикального expansion за пределы line-box-а.
const WAVY_AMPLITUDE_FACTOR: f32 = 1.5;

/// Длина волны в долях `thickness`. 4×thickness — UA-defined компромисс
/// (Chrome ≈ 3-4×, Firefox ≈ 6×; берём середину). При thickness=1px →
/// период 4px, ~3 цикла на каждые 12 CSS-px font-size.
const WAVY_WAVELENGTH_FACTOR: f32 = 4.0;

/// Аппроксимирует синусоидальную линию серией axis-aligned FillRect-ов:
/// для каждого sampled-X эмитим тонкий столбец `[x, x+step] × [cy+dy-t/2,
/// cy+dy+t/2]`, где `cy = y + t/2` — центр толщины, `dy = sin(2π·rel/λ)·A`.
/// Step выбран `max(1, t·0.5)`: ниже — растёт число FillRect (≈ 2·width/t),
/// выше — лестница становится грубее, что особенно заметно при крутых
/// склонах волны (там `|dy'| → t·A/λ·2π`).
fn emit_wavy_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let amplitude = thickness * WAVY_AMPLITUDE_FACTOR;
    let wavelength = thickness * WAVY_WAVELENGTH_FACTOR;
    let step = (thickness * 0.5).max(1.0);
    let cy = y + thickness * 0.5;
    let end = x + width;
    let mut cx = x;
    while cx < end {
        let w = step.min(end - cx);
        if w <= 0.0 {
            break;
        }
        // Используем центр столбца как sample-точку — это даёт
        // чуть более точную аппроксимацию, чем left-edge sampling.
        let sample_x = cx + w * 0.5;
        let phase = (sample_x - x) / wavelength * std::f32::consts::TAU;
        let dy = phase.sin() * amplitude;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(cx, cy + dy - thickness * 0.5, w, thickness),
            color,
        });
        cx += step;
    }
}

/// Like `walk` but applies `CompositorAnimFrame` overrides for opacity and transform.
///
/// When a node has an animated opacity or transform, the overridden values replace
/// the style values in the emitted Push* commands. All other paint (FillRect, DrawText,
/// borders, shadows) uses the base style unchanged.
fn walk_with_anim(b: &LayoutBox, anim: Option<&CompositorAnimFrame>, out: &mut DisplayList) {
    let ov = anim.and_then(|a| a.get(b.node));

    // Determine effective opacity: animated override wins over style.
    let effective_opacity = ov.and_then(|o| o.opacity).unwrap_or(b.style.opacity);

    // Skip completely invisible subtrees (same rule as walk, but uses effective opacity).
    if effective_opacity == 0.0 && b.style.opacity == 0.0 {
        // Both animated and static are zero — nothing to paint.
        if !is_opacity_subtree_painted(b) {
            return;
        }
    } else if effective_opacity == 0.0 {
        // Animated to zero — skip this subtree.
        return;
    } else if !is_opacity_subtree_painted(b) && ov.and_then(|o| o.opacity).is_none() {
        // Base style opacity is 0 and no anim override — skip.
        return;
    }

    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            let has_opacity = effective_opacity < 1.0;
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: effective_opacity });
            }

            // Determine effective transform: animated override wins over style.
            let transform = if let Some(fns) = ov.and_then(|o| o.transform.as_deref()) {
                let (ox, oy, _) = b.style.transform_origin;
                transform_fns_to_matrix(fns, b.rect.x + ox, b.rect.y + oy)
            } else {
                forward_box_transform(b)
            };
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }

            let self_visible = is_paint_visible(b);
            if self_visible {
                emit_box_shadows(b, out);
                if let Some(CssColor::Rgba(bg)) = b.style.background_color
                    && bg.a > 0
                {
                    let clip = background_clip_rect(b);
                    if clip.width > 0.0 && clip.height > 0.0 {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    }
                }
                emit_inset_box_shadows(b, out);
                let s = &b.style;
                let has_border = s.border_top_style.is_visible()
                    || s.border_right_style.is_visible()
                    || s.border_bottom_style.is_visible()
                    || s.border_left_style.is_visible();
                if has_border {
                    let cur = s.color;
                    out.push(DisplayCommand::DrawBorder {
                        rect: b.rect,
                        widths: [
                            s.border_top_width, s.border_right_width,
                            s.border_bottom_width, s.border_left_width,
                        ],
                        colors: [
                            s.border_top_color.resolve(cur),
                            s.border_right_color.resolve(cur),
                            s.border_bottom_color.resolve(cur),
                            s.border_left_color.resolve(cur),
                        ],
                        styles: [
                            s.border_top_style, s.border_right_style,
                            s.border_bottom_style, s.border_left_style,
                        ],
                    });
                }
            }
            for child in &b.children {
                walk_with_anim(child, anim, out);
            }
            if self_visible {
                emit_outline(b, out);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
        }
        BoxKind::InlineBlockRow => {
            for child in &b.children {
                walk_with_anim(child, anim, out);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, out);
        }
        // Image and other kinds: no compositor-offloadable properties, delegate to walk.
        _ => {
            walk(b, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    fn build(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        build_display_list(&tree)
    }

    struct Fixed8;
    impl lumen_layout::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn build_wrapped(html: &str, css: &str, width: f32) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8);
        build_display_list(&tree)
    }

    fn fills(dl: &DisplayList) -> Vec<&Color> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    fn texts(dl: &DisplayList) -> Vec<&str> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn empty_input_empty_list() {
        let dl = build("", "");
        assert!(dl.is_empty());
    }

    #[test]
    fn block_with_background_emits_fill() {
        let dl = build("<p>x</p>", "p { background: red; }");
        let f = fills(&dl);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].r, 255);
    }

    #[test]
    fn block_without_background_no_fill() {
        let dl = build("<p>x</p>", "");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn text_node_emits_draw_text() {
        let dl = build("<p>hello</p>", "");
        assert_eq!(texts(&dl), vec!["hello"]);
    }

    #[test]
    fn cyrillic_text_preserved() {
        let dl = build("<p>Привет, мир</p>", "");
        assert_eq!(texts(&dl), vec!["Привет, мир"]);
    }

    #[test]
    fn nested_backgrounds_in_parent_then_child_order() {
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } p { background: blue; }",
        );
        let f = fills(&dl);
        assert_eq!(f.len(), 2);
        // Сначала parent (под текст), потом child — естественный paint-порядок.
        assert_eq!(f[0].r, 255);
        assert_eq!(f[1].b, 255);
    }

    #[test]
    fn transparent_background_omitted() {
        let dl = build("<p>x</p>", "p { background-color: transparent; }");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn skipped_boxes_emit_nothing() {
        let dl = build("<p>x</p><!-- comment --><p>y</p>", "");
        // Только два текстовых узла; комментарий не даёт команды.
        assert_eq!(texts(&dl).len(), 2);
    }

    #[test]
    fn display_none_emits_nothing() {
        let dl = build(
            r#"<p class="x">hidden</p><p>visible</p>"#,
            ".x { display: none; }",
        );
        assert_eq!(texts(&dl), vec!["visible"]);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// При переносе текста на 2 строки должны быть эмитированы 2 DrawText.
    #[test]
    fn wrapped_text_emits_multiple_draw_text() {
        // "hello world" = 11×8 = 88px. Viewport 60px → перенос на 2 строки.
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        assert_eq!(texts(&dl), vec!["hello", "world"]);
    }

    /// Вторая строка у `DrawText` должна быть смещена по Y на line_height.
    #[test]
    fn wrapped_lines_have_correct_y_offset() {
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        let draw_texts: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(draw_texts.len(), 2);
        let line_h = 16.0_f32 * 1.2; // font_size=16, line_height=1.2
        assert!((draw_texts[0].y - 0.0).abs() < 0.01);
        assert!((draw_texts[1].y - line_h).abs() < 0.1, "y1={}", draw_texts[1].y);
    }

    /// Текст без переноса всё равно рисуется одной командой.
    #[test]
    fn no_wrap_single_draw_text() {
        let dl = build_wrapped("<p>hi</p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hi"]);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// Текст с <span> внутри — один DrawText (одинаковый стиль → фрагменты сливаются).
    #[test]
    fn inline_same_style_merges_into_one_draw_text() {
        let dl = build_wrapped("<p>hello <span>world</span></p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hello world"]);
    }

    /// <a> с цветом → два DrawText: "Hello" и "link" с разными цветами.
    #[test]
    fn inline_different_style_emits_separate_draw_texts() {
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let t = texts(&dl);
        assert_eq!(t, vec!["Hello", "link"]);
        // Второй DrawText должен быть синим.
        let blue_cmds: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } if text == "link" => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(blue_cmds.len(), 1);
        assert_eq!(blue_cmds[0].b, 255);
    }

    /// X-координата второго фрагмента должна быть правее первого.
    #[test]
    fn inline_fragments_have_increasing_x() {
        // "Hello" (5*8=40) + space(8) + "link" → link начинается в x=48.
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 0.0).abs() < 0.01, "Hello должно быть в x=0");
        assert!(
            rects[1].x > rects[0].x,
            "link должно быть правее: Hello.x={}, link.x={}",
            rects[0].x,
            rects[1].x
        );
    }

    // ── Тесты text-decoration ───────────────────────────────────────────────

    fn fill_rects(dl: &DisplayList) -> Vec<&Rect> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, .. } => Some(rect),
                _ => None,
            })
            .collect()
    }

    /// `<a>` с `text-decoration: underline` эмитирует и DrawText, и FillRect.
    #[test]
    fn underline_emits_draw_text_and_fill_rect() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        assert_eq!(texts(&dl), vec!["link"]);
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1, "expected one underline FillRect");
        // "link" = 4×8 = 32px.
        assert!((rects[0].width - 32.0).abs() < 0.01, "width={}", rects[0].width);
    }

    /// Underline должен идти ниже baseline (под глифами).
    #[test]
    fn underline_positioned_below_baseline() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // line_y = 0, baseline ≈ 0 + 16*0.80 = 12.8, underline y ≈ 12.8 + 16*0.10 = 14.4.
        assert!(
            (rects[0].y - 14.4).abs() < 0.5,
            "underline y should be near 14.4, got {}",
            rects[0].y
        );
    }

    /// line-through лежит выше baseline, не ниже.
    #[test]
    fn line_through_positioned_above_baseline() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 12.8, line-through y ≈ 12.8 - 16*0.30 = 8.0.
        assert!(
            (rects[0].y - 8.0).abs() < 0.5,
            "line-through y should be near 8.0, got {}",
            rects[0].y
        );
    }

    /// overline лежит над текстом.
    #[test]
    fn overline_positioned_above_text() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: overline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 12.8, overline y ≈ 12.8 - 16*0.78 ≈ 0.32.
        assert!(
            rects[0].y < 1.0,
            "overline y should be near top, got {}",
            rects[0].y
        );
    }

    /// `text-decoration: underline line-through` эмитирует две линии.
    #[test]
    fn multiple_decorations_emit_multiple_rects() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "expected underline + line-through rects");
    }

    /// Цвет линии совпадает с цветом текста (currentColor).
    #[test]
    fn decoration_uses_text_color() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { color: red; text-decoration: underline; }",
            800.0,
        );
        let colors: Vec<&Color> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].r, 255);
        assert_eq!(colors[0].g, 0);
    }

    /// Соседние фрагменты разной декорации не сливаются.
    #[test]
    fn fragments_with_different_decoration_dont_merge() {
        let dl = build_wrapped(
            "<p>plain <a>underlined</a> tail</p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let t = texts(&dl);
        // 3 фрагмента: "plain", "underlined", "tail".
        assert_eq!(t, vec!["plain", "underlined", "tail"]);
        // Underline только под средним.
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// Унаследованная декорация продолжает работать у потомков.
    #[test]
    fn decoration_inherits_into_descendants() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "p { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // Span наследует underline → FillRect эмитится.
        assert!(!rects.is_empty(), "underline should propagate to span");
    }

    /// `text-decoration: none` на потомке отменяет наследуемую декорацию.
    #[test]
    fn none_on_descendant_overrides_inherited_underline() {
        let dl = build_wrapped(
            "<p><a>off</a></p>",
            "p { text-decoration: underline; } a { text-decoration: none; }",
            800.0,
        );
        assert!(fill_rects(&dl).is_empty(), "a should override underline");
    }

    /// `text-decoration: underline solid` — sanity, что explicit Solid ведёт
    /// себя как default (один FillRect).
    #[test]
    fn style_solid_emits_one_rect() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline solid; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// `Double` — две параллельные линии той же ширины с gap = thickness;
    /// второй rect ниже первого на `2 × thickness`.
    #[test]
    fn style_double_emits_two_parallel_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline double; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "Double = two parallel lines");
        assert!((rects[0].width - rects[1].width).abs() < 0.01);
        let t = (16.0_f32 * 0.07).max(1.0);
        let dy = rects[1].y - rects[0].y;
        assert!(
            (dy - 2.0 * t).abs() < 0.05,
            "expected dy ≈ 2·t = {}, got {dy}",
            2.0 * t
        );
    }

    /// Двойной underline + line-through → 4 rect-а суммарно.
    #[test]
    fn double_with_multiple_lines_emits_four_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline line-through double; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 4);
    }

    /// `Dotted` — серия квадратиков `thickness × thickness`, count > 5
    /// для текста шириной 80px (10 символов × 8px char-width).
    #[test]
    fn style_dotted_emits_square_dots() {
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dotted; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 5, "got {} dots, expected many", rects.len());
        // Каждый dot — квадрат width = height = thickness.
        let t = (16.0_f32 * 0.07).max(1.0);
        for r in &rects {
            assert!(
                (r.width - r.height).abs() < 0.01,
                "dot not square: {}×{}",
                r.width,
                r.height
            );
            assert!(
                (r.width - t).abs() < 0.01,
                "dot width={}, expected t={t}",
                r.width
            );
        }
    }

    /// `Dashed` — серия штрихов длиной `2 × thickness`, count > 3.
    #[test]
    fn style_dashed_emits_dashes() {
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dashed; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 3, "got {} dashes", rects.len());
        let t = (16.0_f32 * 0.07).max(1.0);
        // Все dashes кроме, возможно, последнего — длиной 2·t.
        // Высота — thickness.
        for r in &rects[..rects.len() - 1] {
            assert!(
                (r.width - 2.0 * t).abs() < 0.05,
                "dash width={}, expected {}",
                r.width,
                2.0 * t
            );
            assert!((r.height - t).abs() < 0.01);
        }
    }

    /// `Wavy` эмитит серию тонких axis-aligned столбцов, аппроксимирующих
    /// синусоиду. Каждый столбец = `step × thickness`, sin-сдвиг центра.
    #[test]
    fn style_wavy_emits_sampled_columns() {
        // Один inline char ≈ 8px @ 16px font; thickness = 16·0.07 ≈ 1.12,
        // step = max(1, 1.12·0.5) = 1.0 → ~8 столбцов.
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(
            rects.len() >= 4,
            "wavy emits multiple columns, got {}",
            rects.len()
        );
        // Sum of widths ≈ underline-width (8px).
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (total_w - 8.0).abs() < 0.1,
            "columns cover full width: sum={}, expected ≈ 8",
            total_w
        );
        // Все столбцы — одной thickness (height).
        let h0 = rects[0].height;
        for r in &rects {
            assert!((r.height - h0).abs() < 0.01, "uniform thickness");
        }
        // Y-координаты не одинаковы — иначе это бы Solid line.
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            y_max - y_min > 0.5,
            "wavy must vertically displace columns: range={}",
            y_max - y_min
        );
    }

    /// Амплитуда sin-сдвига должна не превышать `1.5 × thickness`
    /// (peak deviation от центра в обе стороны). Sum-y-range ≤
    /// 2·A + thickness, и не сильно меньше — амплитуда должна
    /// достигаться хотя бы раз на достаточной ширине.
    #[test]
    fn style_wavy_amplitude_matches_factor() {
        // 40px ширина с большой толщиной → волна успевает достичь обоих peak-ов.
        let dl = build_wrapped(
            "<p><a>xxxxx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() >= 8);
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        // A = 4 * 1.5 = 6; peak-to-peak ≈ 12, отступы между centers
        // достигают этого диапазона.
        let y_range = y_max - y_min;
        assert!(
            y_range > 6.0,
            "amplitude expected ≥ 6, got range={}",
            y_range
        );
        assert!(
            y_range <= 13.0,
            "amplitude should not exceed 2·A=12 (+1 sampling tolerance), got {}",
            y_range
        );
    }

    /// Wavy uses the same color as Solid (text-decoration-color / fallback).
    #[test]
    fn style_wavy_preserves_color() {
        let dl = build_wrapped(
            "<p style=\"color: red\"><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let fills: Vec<_> = dl
            .iter()
            .filter_map(|cmd| match cmd {
                DisplayCommand::FillRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert!(!fills.is_empty());
        for c in &fills {
            assert_eq!([c.r, c.g, c.b, c.a], [255, 0, 0, 255]);
        }
    }

    /// Каждый wavy column не выпадает за горизонтальные границы линии:
    /// последний column обрезается до остатка, не overshoot-ит.
    #[test]
    fn style_wavy_columns_clip_to_width() {
        let dl = build_wrapped(
            "<p><a>xx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 3px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // x-min равен старту линии; x-max не превышает старт+width.
        let x_start = rects.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        let x_end = rects
            .iter()
            .map(|r| r.x + r.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (x_end - x_start - total_w).abs() < 0.01,
            "columns are non-overlapping and tile the line",
        );
    }

    /// `text-decoration-thickness: 4px` override-ит default 7%.
    #[test]
    fn thickness_length_overrides_default() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "thickness height={}, expected 4",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: 25%` → 25% от font-size (Phase 0 от
    /// frag.font_size, не parent — задокументировано в style.rs).
    #[test]
    fn thickness_percentage_resolves_against_font_size() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 25%; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "expected 0.25·16 = 4, got {}",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: from-font` в Phase 0 — без font-доступа,
    /// поэтому совпадает с `Auto` (≈ 7% от font-size).
    #[test]
    fn thickness_from_font_falls_back_to_auto() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: from-font; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        let default = (16.0_f32 * 0.07).max(1.0);
        assert!(
            (rects[0].height - default).abs() < 0.01,
            "height={}, expected ≈ {default}",
            rects[0].height
        );
    }

    /// Inline-ран переносится: второй DrawText смещён по Y.
    #[test]
    fn inline_run_wrap_y_offset() {
        // "aa" (16px) + " " (8) + "bb" (16) = 40px > 30px viewport → перенос.
        let dl = build_wrapped("<p>aa <span>bb</span></p>", "", 30.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].y - 0.0).abs() < 0.01);
        let line_h = 16.0_f32 * 1.2;
        assert!((rects[1].y - line_h).abs() < 0.1, "y1={}", rects[1].y);
    }

    // ── Тесты border рендеринга ─────────────────────────────────────────────

    fn borders(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect()
    }

    #[test]
    fn border_solid_emits_draw_border() {
        let dl = build("<p>x</p>", "p { border: 2px solid red; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1, "должна быть одна DrawBorder команда");
        if let DisplayCommand::DrawBorder { widths, colors, styles, .. } = b[0] {
            assert!((widths[0] - 2.0).abs() < 0.01, "top width");
            assert!((widths[1] - 2.0).abs() < 0.01, "right width");
            assert_eq!(colors[0].r, 255, "top color — red");
            assert_eq!(
                *styles,
                [
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                ],
            );
        }
    }

    #[test]
    fn border_dashed_styles_propagate_to_command() {
        let dl = build("<p>x</p>", "p { border: 3px dashed blue; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(
                *styles,
                [
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                ],
            );
        }
    }

    #[test]
    fn border_mixed_styles_per_side() {
        let dl = build(
            "<p>x</p>",
            "p { border-top: 2px solid black; \
                 border-right: 2px dashed black; \
                 border-bottom: 2px dotted black; \
                 border-left: 2px solid black; }",
        );
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(styles[0], BorderStyle::Solid);
            assert_eq!(styles[1], BorderStyle::Dashed);
            assert_eq!(styles[2], BorderStyle::Dotted);
            assert_eq!(styles[3], BorderStyle::Solid);
        }
    }

    #[test]
    fn serialize_drawborder_solid_omits_styles() {
        // bw-compat: чистый Solid не печатает `s=[...]` — snapshot-ы
        // прежней версии остаются валидными.
        let dl = build("<p>x</p>", "p { border: 2px solid black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(!s.contains(" s=["), "Solid не печатает s=[...]: {s}");
    }

    #[test]
    fn serialize_drawborder_dashed_emits_styles_field() {
        let dl = build("<p>x</p>", "p { border: 2px dashed black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(
            s.contains(" s=[da,da,da,da]"),
            "Dashed эмитит s=[...]: {s}"
        );
    }

    #[test]
    fn serialize_drawborder_dotted_short_marker() {
        let dl = build("<p>x</p>", "p { border: 2px dotted black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[do,do,do,do]"), "Dotted: {s}");
    }

    #[test]
    fn serialize_drawborder_mixed_marks_only_non_solid() {
        let dl = build(
            "<p>x</p>",
            "p { border: 2px solid black; \
                 border-right-style: dashed; }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[s,da,s,s]"), "Mixed: {s}");
    }

    #[test]
    fn border_none_style_no_draw_border() {
        // border-width без border-style (default None) → DrawBorder не эмитируется.
        let dl = build("<p>x</p>", "p { border-width: 2px; }");
        assert!(borders(&dl).is_empty());
    }

    #[test]
    fn border_increases_height() {
        // Без border: высота = font_size * line_height = 16 * 1.2 = 19.2
        let no_border = build("<p>x</p>", "");
        let with_border = build("<p>x</p>", "p { border: 5px solid black; }");

        let height_of = |dl: &DisplayList| -> f32 {
            dl.iter()
                .find_map(|c| match c {
                    DisplayCommand::DrawText { rect, .. } => Some(rect.y),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        // Текст должен быть смещён на 5px вниз из-за border-top.
        let y_no = height_of(&no_border);
        let y_with = height_of(&with_border);
        assert!(
            (y_with - y_no - 5.0).abs() < 0.1,
            "y_no={y_no}, y_with={y_with}"
        );
    }

    #[test]
    fn border_color_none_uses_current_color() {
        // border без color → currentColor (наследуется из color: blue).
        let dl = build("<p>x</p>", "p { color: blue; border: 2px solid; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { colors, .. } = b[0] {
            assert_eq!(colors[0].b, 255, "border color should be blue (currentColor)");
        }
    }

    #[test]
    fn border_shorthand_in_serialize() {
        // serialize_display_list корректно форматирует DrawBorder.
        let dl = build("<p>x</p>", "p { border: 3px solid red; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"), "должна быть строка DrawBorder");
        assert!(s.contains("3.00"), "ширина 3px");
    }

    // ── Тесты <img> / DrawImage ─────────────────────────────────────────────

    fn images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect()
    }

    #[test]
    fn img_emits_draw_image() {
        let dl = build(r#"<img src="logo.png" alt="Logo" width="100" height="50">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, src, alt, .. } = imgs[0] {
            assert_eq!(src, "logo.png");
            assert_eq!(alt, "Logo");
            assert!((rect.width - 100.0).abs() < 0.1);
            assert!((rect.height - 50.0).abs() < 0.1);
        }
    }

    #[test]
    fn img_with_background_and_border_paints_in_order() {
        // Painter's order для replaced element: FillRect (bg) → DrawBorder →
        // DrawImage. Image идёт последним, чтобы быть над фоном.
        let dl = build(
            r#"<img src="x" width="50" height="50">"#,
            "img { background: blue; border: 2px solid red; }",
        );
        // Должны присутствовать все три команды.
        let kinds: Vec<&str> = dl
            .iter()
            .map(|c| match c {
                DisplayCommand::FillRect { .. } => "FillRect",
                DisplayCommand::DrawBorder { .. } => "DrawBorder",
                DisplayCommand::DrawOutline { .. } => "DrawOutline",
                DisplayCommand::DrawImage { .. } => "DrawImage",
                DisplayCommand::DrawBackgroundImage { .. } => "DrawBackgroundImage",
                DisplayCommand::DrawText { .. } => "DrawText",
                DisplayCommand::PushClipRect { .. } => "PushClipRect",
                DisplayCommand::PopClip => "PopClip",
                DisplayCommand::PushOpacity { .. } => "PushOpacity",
                DisplayCommand::PopOpacity => "PopOpacity",
                DisplayCommand::PushBlendMode { .. } => "PushBlendMode",
                DisplayCommand::PopBlendMode => "PopBlendMode",
                DisplayCommand::DrawLayerSnapshot { .. } => "DrawLayerSnapshot",
                DisplayCommand::PushTransform { .. } => "PushTransform",
                DisplayCommand::PopTransform => "PopTransform",
            })
            .collect();
        assert_eq!(kinds, vec!["FillRect", "DrawBorder", "DrawImage"]);
    }

    #[test]
    fn img_serialize_includes_src_and_alt() {
        let dl = build(
            r#"<img src="photo.jpg" alt="A photo" width="80" height="40">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawImage"), "must contain DrawImage line");
        assert!(s.contains(r#"src="photo.jpg""#), "must contain src");
        assert!(s.contains(r#"alt="A photo""#), "must contain alt");
    }

    // ── Тесты background-image url() / DrawBackgroundImage ─────────────────

    fn bg_images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBackgroundImage { .. }))
            .collect()
    }

    #[test]
    fn block_background_image_url_emits_draw_background_image() {
        let dl = build(
            "<div>x</div>",
            "div { width: 80px; height: 40px; background-image: url(bg.png); }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1, "должна быть одна команда DrawBackgroundImage");
        if let DisplayCommand::DrawBackgroundImage { rect, src } = bgs[0] {
            assert_eq!(src, "bg.png");
            assert!((rect.width - 80.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((rect.height - 40.0).abs() < 0.1, "rect.height={}", rect.height);
        }
    }

    #[test]
    fn background_image_none_emits_nothing() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; background-image: none; }",
        );
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_default_emits_nothing() {
        // initial value `none` (CSS Backgrounds L3 §3.10): отсутствие свойства
        // не должно эмитить DrawBackgroundImage.
        let dl = build("<div>x</div>", "div { width: 50px; height: 20px; }");
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_gradient_not_painted() {
        // Phase 0: gradient парсится, но не растрит — DrawBackgroundImage
        // эмитится только для BackgroundImage::Url.
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: linear-gradient(red, blue); }",
        );
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_paints_after_color_before_border() {
        // CSS Backgrounds L3 §3.10 — painting order: bg-color → bg-image → border.
        let dl = build(
            "<div></div>",
            "div { width: 60px; height: 30px; \
             background-color: red; background-image: url(b.png); \
             border: 2px solid blue; }",
        );
        let kinds: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { .. } => Some("FillRect"),
                DisplayCommand::DrawBackgroundImage { .. } => Some("DrawBackgroundImage"),
                DisplayCommand::DrawBorder { .. } => Some("DrawBorder"),
                _ => None,
            })
            .collect();
        // Allow surrounding commands; check relative order of the three.
        let fill = kinds.iter().position(|k| *k == "FillRect").expect("FillRect emitted");
        let bg = kinds.iter().position(|k| *k == "DrawBackgroundImage").expect("bg-image emitted");
        let border = kinds.iter().position(|k| *k == "DrawBorder").expect("border emitted");
        assert!(fill < bg, "bg-color must precede bg-image (kinds={kinds:?})");
        assert!(bg < border, "bg-image must precede border (kinds={kinds:?})");
    }

    #[test]
    fn background_image_serialize_includes_src() {
        let dl = build(
            "<div>x</div>",
            "div { width: 40px; height: 10px; background-image: url(\"hero.jpg\"); }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBackgroundImage"), "should contain DrawBackgroundImage line");
        assert!(s.contains(r#"src="hero.jpg""#), "should contain quoted src");
    }

    #[test]
    fn background_image_respects_background_clip_padding_box() {
        // background-clip: padding-box ужимает rect под border на каждой стороне.
        // box-sizing по умолчанию content-box: width=100 — это контент,
        // полная коробка с border 5×2 = 110×70. PaddingBox shrink → 100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; background-clip: padding-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, .. } = bgs[0] {
            assert!((rect.width - 100.0).abs() < 0.1, "got {}", rect.width);
            assert!((rect.height - 60.0).abs() < 0.1, "got {}", rect.height);
        }
    }

    #[test]
    fn img_without_dimensions_emits_zero_rect() {
        // Без размеров — placeholder 0×0; команда всё равно эмитится,
        // потому что DOM-узел существует. Renderer просто не нарисует ничего.
        let dl = build(r#"<img src="x">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!(rect.width.abs() < 0.1);
            assert!(rect.height.abs() < 0.1);
        }
    }

    #[test]
    fn multiple_imgs_emit_multiple_draw_image() {
        let dl = build(
            r#"<img src="a.png" width="10" height="10"><img src="b.png" width="20" height="20">"#,
            "",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 2);
    }

    // ── Тесты fit_image_rect / fit_image_quad (CSS Images L3 §5.5) ──────────

    fn box100() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 100.0)
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn approx_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) -> bool {
        approx_eq(r.x, x) && approx_eq(r.y, y) && approx_eq(r.width, w) && approx_eq(r.height, h)
    }

    #[test]
    fn fit_fill_stretches_to_box() {
        let placed = fit_image_rect(box100(), (50, 200), ObjectFit::Fill, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_contain_letterboxes_wide_image() {
        // 200×100 в 100×100: scale=0.5, placed=100×50, центрируется по y.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn fit_contain_pillarboxes_tall_image() {
        // 100×200 в 100×100: scale=0.5, placed=50×100, центрируется по x.
        let placed = fit_image_rect(box100(), (100, 200), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 0.0, 50.0, 100.0));
    }

    #[test]
    fn fit_cover_overflows_wide_image() {
        // 200×100 в 100×100 при cover: scale=1.0, placed=200×100, центр →
        // x=-50, y=0.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, -50.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn fit_none_keeps_intrinsic_size() {
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, ObjectPosition::default());
        // 50×50 центрируется в 100×100.
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_none_when_smaller() {
        // 50×50 меньше 100×100 — none даёт меньшую площадь, чем contain.
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_contain_when_larger() {
        // 200×200 больше 100×100 — contain даёт меньшую площадь.
        let placed = fit_image_rect(box100(), (200, 200), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_position_top_left_aligns_to_origin() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 0.0, 0.0, 50.0, 50.0));
    }

    #[test]
    fn fit_position_bottom_right_aligns_to_corner() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 50.0, 50.0, 50.0, 50.0));
    }

    #[test]
    fn fit_zero_intrinsic_size_returns_box() {
        let placed = fit_image_rect(box100(), (0, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn quad_contain_returns_full_uvs() {
        // contain не выходит за box → uv = [0,0]..[1,1].
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Contain,
            ObjectPosition::default(),
        )
        .expect("contain visible");
        assert!(approx_rect(visible, 0.0, 25.0, 100.0, 50.0));
        assert!(approx_eq(uv0[0], 0.0) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 1.0) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_cover_crops_uvs_horizontally() {
        // 200×100 cover в 100×100: placement=200×100 at x=-50; visible=
        // box100; UV: u0=(0-(-50))/200=0.25, u1=(100-(-50))/200=0.75.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Cover,
            ObjectPosition::default(),
        )
        .expect("cover visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_none_with_oversized_image_crops_uvs() {
        // none при 200×200 в 100×100 — placement=200×200 at (-50,-50);
        // visible=box100; UV: 0.25..0.75 по обеим осям.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 200),
            ObjectFit::None,
            ObjectPosition::default(),
        )
        .expect("none-larger visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.25));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 0.75));
    }

    #[test]
    fn quad_zero_intrinsic_returns_none() {
        assert!(fit_image_quad(
            box100(),
            (0, 0),
            ObjectFit::Fill,
            ObjectPosition::default()
        )
        .is_none());
    }

    #[test]
    fn quad_serialize_includes_fit_and_position() {
        // Когда fit/position отличны от дефолтов — в snapshot-серилизатор
        // попадают «fit=» и «pos=» поля. Проверяем через ручной DisplayList,
        // чтобы не возиться с CSS-парсингом object-fit.
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition {
                x: PositionComponent::Px(10.0),
                y: PositionComponent::Percent(0.0),
            },
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("fit=cover"), "{s}");
        assert!(s.contains("pos=10.00px 0.00%"), "{s}");
    }

    #[test]
    fn quad_serialize_omits_defaults() {
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
        }];
        let s = serialize_display_list(&dl);
        assert!(!s.contains("fit="), "{s}");
        assert!(!s.contains("pos="), "{s}");
    }

    #[test]
    fn push_clip_rect_serializes() {
        let dl = vec![DisplayCommand::PushClipRect {
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
        }];
        let s = serialize_display_list(&dl);
        assert_eq!(s, "PushClipRect (10.00, 20.00, 100.00, 50.00)\n");
    }

    #[test]
    fn pop_clip_serializes() {
        let dl = vec![DisplayCommand::PopClip];
        assert_eq!(serialize_display_list(&dl), "PopClip\n");
    }

    #[test]
    fn push_opacity_serializes_with_alpha() {
        let dl = vec![DisplayCommand::PushOpacity { alpha: 0.5 }];
        assert_eq!(serialize_display_list(&dl), "PushOpacity 0.500\n");
    }

    #[test]
    fn pop_opacity_serializes() {
        let dl = vec![DisplayCommand::PopOpacity];
        assert_eq!(serialize_display_list(&dl), "PopOpacity\n");
    }

    #[test]
    fn push_blend_mode_serializes_with_name() {
        let dl = vec![DisplayCommand::PushBlendMode {
            mode: BlendMode::Multiply,
        }];
        assert_eq!(serialize_display_list(&dl), "PushBlendMode multiply\n");
    }

    #[test]
    fn pop_blend_mode_serializes() {
        let dl = vec![DisplayCommand::PopBlendMode];
        assert_eq!(serialize_display_list(&dl), "PopBlendMode\n");
    }

    #[test]
    fn blend_mode_from_keyword_all_16_modes() {
        let cases = [
            ("normal", BlendMode::Normal),
            ("multiply", BlendMode::Multiply),
            ("screen", BlendMode::Screen),
            ("overlay", BlendMode::Overlay),
            ("darken", BlendMode::Darken),
            ("lighten", BlendMode::Lighten),
            ("color-dodge", BlendMode::ColorDodge),
            ("color-burn", BlendMode::ColorBurn),
            ("hard-light", BlendMode::HardLight),
            ("soft-light", BlendMode::SoftLight),
            ("difference", BlendMode::Difference),
            ("exclusion", BlendMode::Exclusion),
            ("hue", BlendMode::Hue),
            ("saturation", BlendMode::Saturation),
            ("color", BlendMode::Color),
            ("luminosity", BlendMode::Luminosity),
        ];
        for (kw, expected) in cases {
            assert_eq!(
                BlendMode::from_keyword(kw),
                Some(expected),
                "keyword {kw:?} → {expected:?}"
            );
        }
    }

    #[test]
    fn blend_mode_from_keyword_case_insensitive() {
        assert_eq!(
            BlendMode::from_keyword("MULTIPLY"),
            Some(BlendMode::Multiply)
        );
        assert_eq!(
            BlendMode::from_keyword("Color-Dodge"),
            Some(BlendMode::ColorDodge)
        );
        assert_eq!(
            BlendMode::from_keyword("hArD-LiGhT"),
            Some(BlendMode::HardLight)
        );
    }

    #[test]
    fn blend_mode_from_keyword_unknown_returns_none() {
        assert_eq!(BlendMode::from_keyword(""), None);
        assert_eq!(BlendMode::from_keyword("bogus"), None);
        // CSS использует kebab-case с дефисом; underscore — не валидный
        assert_eq!(BlendMode::from_keyword("color_dodge"), None);
        // Без префикса/суффикса
        assert_eq!(BlendMode::from_keyword("dodge"), None);
        // С пробелами не парсим — должна быть отдельная команда trim caller-ом
        assert_eq!(BlendMode::from_keyword(" multiply "), None);
    }

    #[test]
    fn blend_mode_default_is_normal() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
    }

    #[test]
    fn nested_layer_ops_serialize_in_order() {
        let dl = vec![
            DisplayCommand::PushClipRect {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            DisplayCommand::PushOpacity { alpha: 0.7 },
            DisplayCommand::FillRect {
                rect: Rect::new(10.0, 10.0, 50.0, 50.0),
                color: Color::BLACK,
            },
            DisplayCommand::PopOpacity,
            DisplayCommand::PopClip,
        ];
        let s = serialize_display_list(&dl);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "PushClipRect (0.00, 0.00, 100.00, 100.00)");
        assert_eq!(lines[1], "PushOpacity 0.700");
        assert!(lines[2].starts_with("FillRect"));
        assert_eq!(lines[3], "PopOpacity");
        assert_eq!(lines[4], "PopClip");
    }

    // ── build_display_list_ordered ─────────────────────────────────────

    fn build_ordered(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        build_display_list_ordered(&tree, &stacking_tree, &order)
    }

    #[test]
    fn ordered_single_sc_matches_dom_order_output() {
        // На странице без stacking-triggers `build_display_list_ordered`
        // и `build_display_list` должны эмитить ровно одинаковые команды
        // (порядок DOM = paint order для одного SC).
        let html = "<div style='background:#f00;'>hello</div>";
        let css = "";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let dom = build_display_list(&tree);
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        let ordered = build_display_list_ordered(&tree, &stacking_tree, &order);
        assert_eq!(dom, ordered);
    }

    #[test]
    fn ordered_positive_z_child_painted_after_root_content() {
        // <div z=1 (opacity)>SC-creating</div> рядом с inline-текстом.
        // Ordered-вывод: root.bg → root.contents (включая текст) →
        // child-SC contents (заминусованный, чтобы создать SC).
        //
        // Используем opacity:0.5 как SC-trigger без z-index (auto = phase 6,
        // эмитится ПОСЛЕ root.InlineContent).
        let dl = build_ordered(
            "<p>hello</p><div>world</div>",
            "div { opacity: 0.5; }",
        );
        // Должны быть текстовые узлы из обеих секций. Главное —
        // div-content (world) появляется после p-content (hello).
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        let world_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "world")
        });
        assert!(
            hello_idx.is_some() && world_idx.is_some(),
            "обе строки должны рендериться"
        );
        assert!(
            hello_idx.unwrap() < world_idx.unwrap(),
            "child-SC (opacity div, phase 6) рисуется ПОСЛЕ root.contents (phase 5)"
        );
    }

    #[test]
    fn ordered_negative_z_child_painted_before_root_content() {
        // div с position:relative + z-index:-1 создаёт SC с negative-z.
        // Должен рисоваться до root.InlineContent (т.е. до текста "hello").
        let dl = build_ordered(
            "<div>neg</div><p>hello</p>",
            "div { position: relative; z-index: -1; background: #0f0; }",
        );
        // neg-content (DrawText "neg" внутри div) должен идти до root.contents
        // ("hello" внутри p).
        let neg_text = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "neg")
        });
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        assert!(neg_text.is_some(), "должен быть DrawText neg");
        assert!(hello_idx.is_some(), "должен быть DrawText hello");
        assert!(
            neg_text.unwrap() < hello_idx.unwrap(),
            "neg-z div (phase 2) рисуется ДО root.InlineContent (phase 5)"
        );
    }

    // ── layer-ops эмиссия в build_display_list_ordered ─────────────────

    /// Helper: количество вхождений варианта в DisplayList.
    fn count_variant(dl: &DisplayList, predicate: impl Fn(&DisplayCommand) -> bool) -> usize {
        dl.iter().filter(|c| predicate(c)).count()
    }

    #[test]
    fn ordered_opacity_lt_one_emits_push_pop_pair() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.5; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 1, "opacity<1 → один PushOpacity");
        assert_eq!(pops, 1, "и парный PopOpacity");

        // Push до контента, Pop после.
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        let text_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "x"));
        assert!(push_idx < pop_idx);
        if let Some(text_idx) = text_idx {
            assert!(push_idx < text_idx);
            assert!(text_idx < pop_idx);
        }
    }

    #[test]
    fn ordered_opacity_alpha_value_preserved() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.25; }");
        let push = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        if let DisplayCommand::PushOpacity { alpha } = push {
            assert!((alpha - 0.25).abs() < 1e-6);
        } else {
            panic!("expected PushOpacity");
        }
    }

    #[test]
    fn ordered_opacity_one_does_not_emit() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 1; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert_eq!(pushes, 0, "opacity:1 не триггерит Push");
    }

    #[test]
    fn ordered_mix_blend_mode_emits_push_pop() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: multiply; }",
        );
        let pushes: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushBlendMode { mode } => Some(*mode),
                _ => None,
            })
            .collect();
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopBlendMode));
        assert_eq!(pushes, vec![BlendMode::Multiply]);
        assert_eq!(pops, 1);
    }

    #[test]
    fn ordered_mix_blend_mode_normal_does_not_emit() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: normal; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushBlendMode { .. }));
        assert_eq!(pushes, 0);
    }

    #[test]
    fn ordered_overflow_hidden_on_sc_owner_emits_clip() {
        // div c opacity<1 (= SC-owner) + overflow:hidden → Push/PopClipRect
        // в SC-owner bucket. Opacity тоже эмитится; проверяем clip отдельно.
        let dl = build_ordered(
            "<div>x</div>",
            "div { opacity: 0.5; overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(pushes_clip.len(), 1, "overflow:hidden → один PushClipRect");
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pops_clip, 1);
    }

    #[test]
    fn ordered_overflow_hidden_on_non_sc_emits_clip_inline() {
        // div c overflow:hidden НЕ создаёт SC (overflow — не SC-trigger).
        // PushClipRect эмитится inline в bucket.contents текущего SC.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pushes_clip, 1);
        assert_eq!(pops_clip, 1);
        // SC не появился: PushOpacity/PushBlendMode не должны быть.
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. })),
            0
        );
    }

    #[test]
    fn ordered_overflow_visible_does_not_emit_clip() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: visible; opacity: 0.5; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert_eq!(pushes_clip, 0, "overflow:visible не клипает");
    }

    #[test]
    fn ordered_overflow_x_alone_triggers_clip() {
        // Любое из overflow-x / overflow-y ≠ visible — достаточно для clip.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert_eq!(pushes_clip, 1);
    }

    #[test]
    fn ordered_combined_opacity_blend_clip_emit_lifo() {
        // SC-owner со всеми тремя триггерами: проверяем парность и LIFO.
        let dl = build_ordered(
            "<div>x</div>",
            "div {
                opacity: 0.5;
                mix-blend-mode: multiply;
                overflow: hidden;
                width: 100px;
                height: 50px;
            }",
        );
        // Извлекаем последовательность layer-ops (без других команд).
        let ops: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DisplayCommand::PushClipRect { .. }
                        | DisplayCommand::PopClip
                        | DisplayCommand::PushBlendMode { .. }
                        | DisplayCommand::PopBlendMode
                        | DisplayCommand::PushOpacity { .. }
                        | DisplayCommand::PopOpacity
                )
            })
            .collect();
        // Ожидаемый порядок (см. box_layer_ops): Clip → Blend → Opacity (Push),
        // потом Opacity → Blend → Clip (Pop) для LIFO-парности.
        assert_eq!(ops.len(), 6, "три триггера = 6 layer-ops");
        assert!(matches!(ops[0], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(ops[1], DisplayCommand::PushBlendMode { .. }));
        assert!(matches!(ops[2], DisplayCommand::PushOpacity { .. }));
        assert!(matches!(ops[3], DisplayCommand::PopOpacity));
        assert!(matches!(ops[4], DisplayCommand::PopBlendMode));
        assert!(matches!(ops[5], DisplayCommand::PopClip));
    }

    #[test]
    fn ordered_nested_opacity_emits_two_pairs() {
        // Внешний div с opacity, внутренний div с opacity. Каждый создаёт
        // свой SC; должно быть 2 пары PushOpacity/PopOpacity.
        let dl = build_ordered(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { opacity: 0.5; } .inner { opacity: 0.25; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 2);
        assert_eq!(pops, 2);
    }

    #[test]
    fn ordered_no_triggers_emits_no_layer_ops() {
        // Простая страница без opacity/blend/overflow — ни одной layer-op.
        let dl = build_ordered("<p>hello</p>", "");
        let any_layer_op = dl.iter().any(|c| {
            matches!(
                c,
                DisplayCommand::PushClipRect { .. }
                    | DisplayCommand::PopClip
                    | DisplayCommand::PushBlendMode { .. }
                    | DisplayCommand::PopBlendMode
                    | DisplayCommand::PushOpacity { .. }
                    | DisplayCommand::PopOpacity
            )
        });
        assert!(!any_layer_op);
    }

    #[test]
    fn ordered_clip_rect_matches_box_rect() {
        // PushClipRect должен использовать b.rect (после layout-а).
        // Не привязываемся к точным значениям — проверяем, что rect не нулевой.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 200px; height: 100px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть PushClipRect");
        assert!(rect.width > 0.0 && rect.height > 0.0);
    }

    #[test]
    fn ordered_empty_tree_produces_empty_list() {
        // Деградированный случай: StackingTree без contexts, layout —
        // пустая страница (одинокий root Block без детей и без bg/border).
        let doc = lumen_html_parser::parse("");
        let sheet = lumen_css_parser::parse("");
        let tree =
            lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        let dl = build_display_list_ordered(
            &tree,
            &lumen_layout::StackingTree { contexts: vec![] },
            &lumen_layout::PaintOrder::default(),
        );
        assert!(dl.is_empty(), "пустой PaintOrder → пустой display list");
    }

    // ───────── outline rendering ─────────

    fn outlines(dl: &DisplayList) -> Vec<(&Color, f32, f32, OutlineStyle)> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawOutline { color, width, offset, style, .. } => {
                    Some((color, *width, *offset, *style))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn outline_solid_emits_draw_outline() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1, "ровно одна DrawOutline на div");
        let (color, width, offset, style) = o[0];
        assert_eq!(color.r, 255);
        assert!((width - 2.0).abs() < 0.01);
        assert!((offset - 0.0).abs() < 0.01);
        assert_eq!(style, OutlineStyle::Solid);
    }

    #[test]
    fn outline_none_emits_nothing() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px none red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline:none → no DrawOutline");
    }

    #[test]
    fn outline_zero_width_emits_nothing() {
        // outline-width: 0 → invisible (CSS Basic UI L4 §5.1).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 0 solid red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline-width:0 → no DrawOutline");
    }

    #[test]
    fn outline_offset_is_preserved() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; \
             outline: 2px solid red; outline-offset: 5px; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        assert!((o[0].2 - 5.0).abs() < 0.01, "offset=5px должен сохраниться");
    }

    #[test]
    fn outline_color_currentcolor_resolves_to_text_color() {
        // currentColor → CSS color (Phase 0 reduces Auto/CurrentColor to color).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: rgb(10, 20, 30); \
             outline: 2px solid currentColor; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        let (color, _, _, _) = o[0];
        assert_eq!((color.r, color.g, color.b), (10, 20, 30));
    }

    #[test]
    fn outline_after_children_in_walk() {
        // Outline parent-а должен идти ПОСЛЕ background ребёнка — иначе при
        // негативном outline-offset (Phase 2) outline парента закрывался бы
        // содержимым ребёнка. Phase 0 проверка ordering: DrawOutline
        // последняя из своего box-а.
        let dl = build(
            "<div><p></p></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; } \
             p { display: block; background: blue; width: 30px; height: 10px; }",
        );
        let outline_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawOutline { .. }))
            .expect("должна быть DrawOutline");
        // FillRect ребёнка (background: blue) должен идти раньше DrawOutline.
        let child_bg_idx = dl
            .iter()
            .enumerate()
            .find(|(_, c)| matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255))
            .map(|(i, _)| i)
            .expect("должен быть синий FillRect ребёнка");
        assert!(
            child_bg_idx < outline_idx,
            "outline (idx {outline_idx}) должен идти после child background (idx {child_bg_idx})"
        );
    }

    #[test]
    fn outline_serializes_with_short_offset_only_when_nonzero() {
        // DrawOutline с offset=0 не выводит `off=…` в сериализацию (как
        // DrawText опускает default-значения).
        let dl = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 0.0,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawOutline (0.00, 0.00, 100.00, 50.00) w=2.00 s=solid #ff0000ff"));
        assert!(!s.contains("off="));

        // Non-zero offset → должен присутствовать.
        let dl2 = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 5.0,
        }];
        let s2 = serialize_display_list(&dl2);
        assert!(s2.contains("off=5.00"));
    }

    // ───────── text-shadow rendering ─────────

    fn texts_with_colors(dl: &DisplayList) -> Vec<(String, [u8; 3])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } => {
                    Some((text.clone(), [color.r, color.g, color.b]))
                }
                _ => None,
            })
            .collect()
    }

    fn text_rects(dl: &DisplayList) -> Vec<(String, [f32; 2])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, rect, .. } => {
                    Some((text.clone(), [rect.x, rect.y]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn text_shadow_none_emits_only_main_text() {
        // Без text-shadow — ровно один DrawText на фрагмент (как раньше).
        let dl = build("<p>hello</p>", "p { color: black; }");
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].0, "hello");
    }

    #[test]
    fn text_shadow_one_emits_shadow_before_main() {
        // Один text-shadow → 2 DrawText: сначала shadow, потом main.
        // Spec painter's order: shadow рисуется ПОД основным текстом.
        let dl = build(
            "<p>hi</p>",
            "p { color: black; text-shadow: 2px 3px red; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2, "shadow + main = 2 DrawText");
        // Painter's order: shadow первый (под main), main второй (поверх).
        assert_eq!(texts[0].1, [255, 0, 0], "первый = красная тень");
        assert_eq!(texts[1].1, [0, 0, 0], "второй = чёрный основной");
        // Тень смещена на (2, 3) px относительно main.
        let rects = text_rects(&dl);
        let dx = rects[0].1[0] - rects[1].1[0];
        let dy = rects[0].1[1] - rects[1].1[1];
        assert!((dx - 2.0).abs() < 0.01, "shadow_x смещён на 2px, got {dx}");
        assert!((dy - 3.0).abs() < 0.01, "shadow_y смещён на 3px, got {dy}");
    }

    #[test]
    fn text_shadow_multiple_reverse_order() {
        // Spec L3 §6: «first shadow is on top, subsequent shadows are
        // layered behind it». Значит painter's order: последняя в списке
        // рисуется первой (под всеми), первая — последней (над всеми, но
        // под main). Список: red(1px), green(2px), blue(3px) — порядок
        // эмиссии: blue → green → red → main.
        let dl = build(
            "<p>z</p>",
            "p { color: black; \
             text-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 4, "3 shadows + main = 4 DrawText");
        assert_eq!(texts[0].1, [0, 0, 255], "blue painted first (deepest)");
        assert_eq!(texts[1].1, [0, 128, 0], "green painted second");
        assert_eq!(texts[2].1, [255, 0, 0], "red painted third");
        assert_eq!(texts[3].1, [0, 0, 0], "main painted last (top)");
    }

    #[test]
    fn text_shadow_color_omitted_uses_currentcolor() {
        // CSS Text Decoration L3 §6: «If <color> is not specified, the
        // value used for color (currentColor) is used.»
        let dl = build(
            "<p>x</p>",
            "p { color: rgb(10, 20, 30); text-shadow: 1px 1px; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2);
        // Shadow color = currentColor = (10, 20, 30).
        assert_eq!(texts[0].1, [10, 20, 30]);
        assert_eq!(texts[1].1, [10, 20, 30]);
    }

    // ───────── box-shadow rendering ─────────

    fn fills_with_color(dl: &DisplayList) -> Vec<(Rect, [u8; 4])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, color } => {
                    Some((*rect, [color.r, color.g, color.b, color.a]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn box_shadow_none_emits_no_extra_fill() {
        // Без box-shadow div с background даёт ровно одну FillRect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1, [255, 0, 0, 255]);
    }

    #[test]
    fn box_shadow_outset_emits_fill_before_background() {
        // Outset shadow → 2 FillRect: сначала shadow (под bg), потом bg.
        // shadow смещена на (3, 5) px.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Painter's order: shadow первый (под bg).
        assert_eq!(fills[0].1, [0, 0, 0, 255], "shadow первой");
        assert_eq!(fills[1].1, [255, 255, 255, 255], "background второй");
        // shadow смещена на (3, 5).
        let dx = fills[0].0.x - fills[1].0.x;
        let dy = fills[0].0.y - fills[1].0.y;
        assert!((dx - 3.0).abs() < 0.01);
        assert!((dy - 5.0).abs() < 0.01);
        // Размер shadow совпадает с box (spread=0).
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_offset_emits_frame() {
        // offset (3, 5) внутри 100×50 без border / spread:
        // outer = padding-box = (0..100, 0..50).
        // inner = (3..103, 5..55) — частично за outer.
        // hole = inner ∩ outer = (3..100, 5..50).
        // Тень = 4 кольцевых рамки; нулевая bottom (50..50) и right (100..100)
        // skip-ятся. Остаются top (0..5) + left (0..3 на полосе 5..50).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + top frame + left frame = 3.
        assert_eq!(fills.len(), 3);
        // Painter's order: bg первый, inset тени поверх.
        assert_eq!(fills[0].1, [255, 0, 0, 255], "bg = red");
        // Top frame: x=0, y=0, w=100, h=5.
        assert_eq!(fills[1].1[..3], [0, 0, 0], "frame = black");
        let top = fills[1].0;
        assert!((top.x - 0.0).abs() < 0.01);
        assert!((top.y - 0.0).abs() < 0.01);
        assert!((top.width - 100.0).abs() < 0.01);
        assert!((top.height - 5.0).abs() < 0.01);
        // Left frame: x=0, y=5, w=3, h=45.
        let left = fills[2].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.y - 5.0).abs() < 0.01);
        assert!((left.width - 3.0).abs() < 0.01);
        assert!((left.height - 45.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_spread_only_emits_four_frames() {
        // Только spread, без offset: inner симметрично сжат на 10px →
        // hole = (10..90, 10..40). Все 4 рамки видимы.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 4 frames.
        assert_eq!(fills.len(), 5);
        assert_eq!(fills[0].1, [255, 255, 255, 255], "bg = white");
        // Все 4 рамки = black.
        for fill in &fills[1..] {
            assert_eq!(fill.1[..3], [0, 0, 0]);
        }
        // Top (0, 0, 100, 10).
        let top = fills[1].0;
        assert!((top.height - 10.0).abs() < 0.01);
        // Bottom (0, 40, 100, 10).
        let bottom = fills[2].0;
        assert!((bottom.y - 40.0).abs() < 0.01);
        assert!((bottom.height - 10.0).abs() < 0.01);
        // Left (0, 10, 10, 30).
        let left = fills[3].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.width - 10.0).abs() < 0.01);
        assert!((left.height - 30.0).abs() < 0.01);
        // Right (90, 10, 10, 30).
        let right = fills[4].0;
        assert!((right.x - 90.0).abs() < 0.01);
        assert!((right.width - 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_large_offset_fills_whole_outer() {
        // offset_x=200 при width=100 → inner полностью справа от outer.
        // no_overlap → один FillRect, покрывающий весь padding-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 200px 0 black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2, "bg + single full-outer shadow");
        assert_eq!(fills[1].1[..3], [0, 0, 0]);
        let shadow = fills[1].0;
        assert!((shadow.width - 100.0).abs() < 0.01);
        assert!((shadow.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_negative_spread_covers_outer_skips() {
        // Отрицательный spread с большим модулем — inner полностью покрывает
        // outer (расширен наружу с каждой стороны). Тени не видно.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        // Только bg.
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1[..3], [255, 255, 255]);
    }

    #[test]
    fn box_shadow_inset_uses_padding_box_when_border_present() {
        // box-sizing: border-box + 100×50 + border:5px → padding-box =
        // (5, 5, 90, 40). offset 0,0 + spread 5 → inner = (10, 10, 80, 30)
        // внутри padding-box. Все 4 frames лежат строго в padding-box.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 100px; height: 50px; \
             background: white; border: 5px solid green; \
             box-shadow: inset 0 0 0 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames + bg + (possibly border fills через DrawBorder; они
        // не попадают в fills_with_color — DrawBorder отдельный command).
        let shadow_fills: Vec<_> = fills
            .iter()
            .filter(|(_, c)| c[..3] == [0, 0, 0])
            .collect();
        assert_eq!(shadow_fills.len(), 4, "border-aware padding-box → 4 frames");
        // Все рамки лежат внутри padding-box: x in [5..95], y in [5..45].
        for (rect, _) in &shadow_fills {
            assert!(rect.x >= 5.0 - 0.01, "left edge inside padding-box: {}", rect.x);
            assert!(
                rect.x + rect.width <= 95.0 + 0.01,
                "right edge inside padding-box: {}",
                rect.x + rect.width
            );
            assert!(rect.y >= 5.0 - 0.01, "top edge inside padding-box: {}", rect.y);
            assert!(
                rect.y + rect.height <= 45.0 + 0.01,
                "bottom edge inside padding-box: {}",
                rect.y + rect.height
            );
        }
    }

    #[test]
    fn box_shadow_inset_currentcolor_fallback() {
        // CSS Backgrounds L3 §4.6 — отсутствующий color = currentColor.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: blue; \
             box-shadow: inset 0 0 0 10px; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames (без bg).
        assert_eq!(fills.len(), 4);
        for fill in &fills {
            assert_eq!(fill.1[..3], [0, 0, 255], "frame = currentColor (blue)");
        }
    }

    #[test]
    fn box_shadow_inset_multiple_reverse_order() {
        // Spec: «first shadow is on top» — последний inset эмитим первым,
        // первый — последним (поверх всех).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 5px red, inset 0 0 0 10px green, inset 0 0 0 15px blue; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 3 inset × 4 frames = 1 + 12 = 13. Но frames с w=0 / h=0
        // skip-ятся; spread > 0 всегда даёт все 4 frames.
        assert_eq!(fills.len(), 13);
        assert_eq!(fills[0].1[..3], [255, 255, 255], "bg first");
        // Дальше — blue (последний CSS-shadow рисуется первым).
        for fill in &fills[1..5] {
            assert_eq!(fill.1[..3], [0, 0, 255]);
        }
        for fill in &fills[5..9] {
            assert_eq!(fill.1[..3], [0, 128, 0]);
        }
        // red — поверх всех (первый CSS-shadow рисуется последним).
        for fill in &fills[9..13] {
            assert_eq!(fill.1[..3], [255, 0, 0]);
        }
    }

    #[test]
    fn box_shadow_inset_and_outset_coexist() {
        // Одна inset и одна outset — outset перед bg, inset после bg.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px red, inset 0 0 0 5px blue; }",
        );
        let fills = fills_with_color(&dl);
        // outset (1) + bg (1) + inset (4 frames) = 6.
        assert_eq!(fills.len(), 6);
        assert_eq!(fills[0].1[..3], [255, 0, 0], "outset red first");
        assert_eq!(fills[1].1[..3], [255, 255, 255], "bg second");
        for fill in &fills[2..6] {
            assert_eq!(fill.1[..3], [0, 0, 255], "inset blue frames");
        }
    }

    #[test]
    fn box_shadow_inset_transparent_color_skipped() {
        // a=0 — shadow невидим, не эмитим.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 0 0 0 10px rgba(0,0,0,0); }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "transparent inset shadow skipped");
        assert_eq!(fills[0].1[..3], [255, 0, 0]);
    }

    #[test]
    fn box_shadow_spread_expands_rect() {
        // spread=10 → shadow rect расширен на 10px по всем сторонам.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        let shadow_rect = fills[0].0;
        let bg_rect = fills[1].0;
        // shadow расширен на 10 по всем сторонам.
        assert!((shadow_rect.width - bg_rect.width - 20.0).abs() < 0.01);
        assert!((shadow_rect.height - bg_rect.height - 20.0).abs() < 0.01);
        assert!((shadow_rect.x - bg_rect.x + 10.0).abs() < 0.01);
        assert!((shadow_rect.y - bg_rect.y + 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_reverse_order() {
        // Spec: «first shadow is on top». Painter's order: последняя
        // shadow рисуется первой (ниже всех), первая — последней-перед-bg.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 4, "3 shadows + bg = 4 FillRect");
        assert_eq!(fills[0].1[..3], [0, 0, 255]); // blue первой (ниже всех)
        assert_eq!(fills[1].1[..3], [0, 128, 0]); // green
        assert_eq!(fills[2].1[..3], [255, 0, 0]); // red (поверх теней)
        assert_eq!(fills[3].1[..3], [255, 255, 255]); // bg (поверх всего)
    }

    #[test]
    fn box_shadow_color_omitted_uses_currentcolor() {
        // CSS Backgrounds L3 §4.6 — «If no color is specified, the value
        // of the color property is used».
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             color: rgb(10, 20, 30); box-shadow: 2px 2px; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].1[..3], [10, 20, 30]);
    }

    #[test]
    fn box_shadow_negative_spread_collapses_to_skip() {
        // spread=-100 на box 50×50 → final w/h = -150, отрицательный
        // → пропускаем (не эмитим бессмысленный FillRect).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "collapsed shadow пропускается");
    }

    #[test]
    fn box_shadow_transparent_color_skipped() {
        // a == 0 → нечего рисовать.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 5px 5px transparent; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
    }

    #[test]
    fn box_shadow_blur_ignored_phase0() {
        // blur не влияет на rect в Phase 0 — эмитим резкую копию.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px 20px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Размер shadow == размер box (spread=0); blur игнорируется.
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
        assert!((fills[0].0.height - fills[1].0.height).abs() < 0.01);
    }

    // ───────── background-clip rendering ─────────

    fn first_bg_rect(dl: &DisplayList) -> Rect {
        dl.iter()
            .find_map(|c| match c {
                // bg = single non-shadow FillRect: ищем по цвету ≠ pre-shadow
                DisplayCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("должна быть хотя бы одна FillRect")
    }

    #[test]
    fn background_clip_border_box_default_uses_full_rect() {
        // BorderBox initial: bg рисуется на полный b.rect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; }",
        );
        let bg = first_bg_rect(&dl);
        // box-sizing: content-box default → внешний размер = 100 + 2*20 + 2*5 = 150.
        assert!((bg.width - 150.0).abs() < 0.01);
        assert!((bg.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_padding_box_shrinks_by_border() {
        // PaddingBox: bg ужимается на border (по 5px со всех сторон).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: padding-box; }",
        );
        let bg = first_bg_rect(&dl);
        // padding-box = border-box minus 2*5 border = 150 - 10 = 140.
        assert!((bg.width - 140.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 90.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x на левый border (+5).
        assert!((bg.x - 5.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_content_box_shrinks_by_border_plus_padding() {
        // ContentBox: bg ужимается на border + padding.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: content-box; }",
        );
        let bg = first_bg_rect(&dl);
        // content-box = border-box minus 2*(5+20) = 150 - 50 = 100.
        assert!((bg.width - 100.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 50.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x = border + padding = 5 + 20 = 25.
        assert!((bg.x - 25.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_text_falls_back_to_border_box_phase0() {
        // Phase 0 без glyph-mask: text-clip эмитим как border-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             background-clip: text; }",
        );
        let bg = first_bg_rect(&dl);
        assert!((bg.width - 100.0).abs() < 0.01);
        assert!((bg.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_collapsed_rect_skipped() {
        // Если border + padding больше box-а → clip rect collapses to 0 → skip.
        // box-sizing:border-box + width:50 + border:30 → content = 50 - 60 = -10,
        // max(0) → 0 → FillRect bg не эмитится.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 50px; height: 20px; \
             border: 30px solid black; \
             background: red; background-clip: content-box; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapsed bg должен быть пропущен");
    }

    // ───────── visibility: hidden ─────────

    fn cmd_count(dl: &DisplayList) -> usize {
        dl.iter()
            .filter(|c| !matches!(c, DisplayCommand::PushClipRect { .. }
                                  | DisplayCommand::PopClip
                                  | DisplayCommand::PushOpacity { .. }
                                  | DisplayCommand::PopOpacity
                                  | DisplayCommand::PushBlendMode { .. }
                                  | DisplayCommand::PopBlendMode))
            .count()
    }

    #[test]
    fn visibility_hidden_block_suppresses_self_paint() {
        let visible = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; }",
        );
        let hidden = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; \
             visibility: hidden; }",
        );
        // visible: FillRect (bg) + DrawBorder.
        assert!(cmd_count(&visible) >= 2);
        // hidden: ничего из self не эмитим (никаких children → пусто).
        assert_eq!(cmd_count(&hidden), 0);
    }

    #[test]
    fn visibility_hidden_block_still_walks_visible_children() {
        // Parent hidden, child явно visible (override через inherit).
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; visibility: hidden; } \
             p { display: block; background: blue; visibility: visible; \
                 width: 20px; height: 10px; }",
        );
        // Должна быть синяя FillRect от child, но не красная от parent.
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255)
        });
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        assert!(blues.count() >= 1, "child должен рисоваться");
        assert_eq!(reds.count(), 0, "parent bg не рисуется");
    }

    #[test]
    fn visibility_hidden_skips_text() {
        // text inherits visibility=hidden → DrawText не эмитим.
        let dl = build(
            "<p>hello</p>",
            "p { visibility: hidden; color: black; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "hidden parent → text не эмитим");
    }

    // Note: inline visibility override (parent hidden + child <span>
    // visibility:visible) зависит от того, что layout формирует отдельный
    // InlineFrag со style от span. Тест на это случае отложен — текущее
    // layout-поведение может склеивать text-nodes в один frag со
    // стилем родителя. Когда P1 разделит inline-fragments по style-runs,
    // добавим этот test обратно.

    #[test]
    fn visibility_collapse_treated_as_hidden_outside_table() {
        // CSS L3 §4: vne table-row `collapse` ведёт себя как `hidden`.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; \
             visibility: collapse; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapse вне table → hidden");
    }

    #[test]
    fn visibility_hidden_image_skipped() {
        // visibility:hidden на `<img>` — DrawImage не эмитим.
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { visibility: hidden; }",
        );
        let images: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect();
        assert!(images.is_empty());
    }

    // ───────── opacity:0 skip ─────────

    #[test]
    fn opacity_zero_skips_block_and_subtree() {
        // opacity:0 на parent → ни parent, ни children не рисуются.
        let dl = build(
            "<div><p>x</p></div>",
            "div { opacity: 0; background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let fills_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        assert_eq!(fills_count, 0, "opacity:0 → whole subtree skipped");
    }

    #[test]
    fn opacity_zero_skips_text() {
        let dl = build(
            "<p>hello</p>",
            "p { opacity: 0; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "opacity:0 → text skipped");
    }

    #[test]
    fn opacity_one_renders_normally() {
        // Sanity: opacity:1 default — всё рисуется.
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255 && color.r == 0)
        });
        assert!(reds.count() >= 1);
        assert!(blues.count() >= 1);
    }

    #[test]
    fn opacity_half_phase0_does_not_change_emission() {
        // Phase 0: opacity > 0 && < 1 не обрабатывается; FillRect эмитим
        // с original color без модификации (true compositing — P2 п.4+).
        let dl = build(
            "<div></div>",
            "div { background: red; opacity: 0.5; width: 50px; height: 30px; }",
        );
        let reds: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert_eq!(reds.len(), 1, "opacity:0.5 не skip-аем; alpha не множим в Phase 0");
    }

    #[test]
    fn opacity_zero_image_subtree_skipped() {
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { opacity: 0; }",
        );
        let any: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }
                                  | DisplayCommand::FillRect { .. }
                                  | DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(any.is_empty());
    }

    // ── transform pipeline (P2) ────────────────────────────────────────────

    #[test]
    fn transform_none_emits_no_push() {
        let dl = build("<div>x</div>", "div { background: #f00; }");
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. })),
            0,
        );
    }

    #[test]
    fn transform_translate_emits_push_pop_pair() {
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 20px);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn transform_translate_matrix_has_expected_offsets() {
        // translate(50px, 70px) с default transform-origin (Phase 0 — (0,0)):
        // matrix = T(0,0)·T(50,70)·T(-0,-0) = T(50,70).
        // 2D affine: x'=x+50, y'=y+70 → (a,b,c,d,e,f) = (1,0,0,1,50,70).
        let dl = build(
            r#"<div style="background: red; transform: translate(50px, 70px);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .expect("PushTransform missing");
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        let e = push.0[12];
        let f = push.0[13];
        assert!((a - 1.0).abs() < 1e-5);
        assert!(b.abs() < 1e-5);
        assert!(c.abs() < 1e-5);
        assert!((d - 1.0).abs() < 1e-5);
        assert!((e - 50.0).abs() < 1e-5);
        assert!((f - 70.0).abs() < 1e-5);
    }

    #[test]
    fn transform_push_wraps_box_content() {
        // PushTransform идёт до собственного FillRect фона, PopTransform — после.
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 0);">x</div>"#,
            "",
        );
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let fill_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .unwrap();
        assert!(push_idx < fill_idx, "Push должен идти до контента");
        assert!(fill_idx < pop_idx, "Pop должен идти после контента");
    }

    #[test]
    fn transform_after_opacity_in_walk_order() {
        // Phase 0 simple `walk`: PushOpacity → PushTransform → content →
        // PopTransform → PopOpacity. Transform применяется ВНУТРИ opacity-
        // layer-а (его эффект — на off-screen layer перед композицией).
        let dl = build(
            r#"<div style="background: red; opacity: 0.5; transform: scale(2);">x</div>"#,
            "",
        );
        let push_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let push_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let pop_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        assert!(push_op < push_tr);
        assert!(push_tr < pop_tr);
        assert!(pop_tr < pop_op);
    }

    #[test]
    fn transform_serialize_2d_affine_components() {
        let dl = vec![
            DisplayCommand::PushTransform {
                matrix: Mat4::from_2d_affine(2.0, 0.0, 0.0, 0.5, 10.0, -20.0),
            },
            DisplayCommand::PopTransform,
        ];
        let s = serialize_display_list(&dl);
        // a=2.000 b=0.000 c=0.000 d=0.500 e=10.000 f=-20.000.
        assert_eq!(
            s,
            "PushTransform [2.000 0.000 0.000 0.500 10.000 -20.000]\nPopTransform\n"
        );
    }

    #[test]
    fn transform_ordered_emits_via_box_layer_ops() {
        // build_display_list_ordered идёт через box_layer_ops; должен дать
        // Push/Pop пару наряду с simple walk-ом.
        let dl = build_ordered(
            r#"<div style="background: red; transform: rotate(45deg);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn transform_origin_affects_matrix() {
        // С transform-origin (10, 20) и translate(0, 0) матрица =
        // T(10+box_x, 20+box_y) · I · T(-(10+box_x), -(20+box_y)) = I.
        // Здесь box_x/box_y зависят от layout; берём rotate чтобы origin
        // действительно изменял результат. rotate(90deg) с origin (0,0) -
        // точка (1,0) → (0,1). С origin (10,0) — точка (1,0) → (10, -9).
        // Просто проверяем что матрица не identity при rotate.
        let dl = build(
            r#"<div style="background: red; transform: rotate(90deg);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .unwrap();
        assert!(!push.is_identity(), "rotate(90deg) ≠ identity");
        // sin/cos(90°): a=cos=0, b=sin=1, c=-sin=-1, d=cos=0.
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        assert!(a.abs() < 1e-5);
        assert!((b - 1.0).abs() < 1e-5);
        assert!((c + 1.0).abs() < 1e-5);
        assert!(d.abs() < 1e-5);
    }

    // ─── build_display_list_with_anim ────────────────────────────────────────

    use lumen_layout::{CompositorAnimFrame, CompositorOverride};
    use lumen_dom::NodeId;
    use std::collections::HashMap;

    fn build_anim(html: &str, css: &str, overrides: HashMap<NodeId, CompositorOverride>) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let frame = CompositorAnimFrame { overrides, has_active: true };
        build_display_list_with_anim(&tree, Some(&frame))
    }

    #[test]
    fn anim_no_overrides_same_as_base() {
        let html = r#"<div style="background:red;width:100px;height:50px"></div>"#;
        let base = build(html, "");
        let anim = build_anim(html, "", HashMap::new());
        assert_eq!(base.len(), anim.len(), "empty overrides: same DL length");
    }

    #[test]
    fn anim_none_frame_same_as_base() {
        let html = r#"<div style="background:blue;width:80px;height:40px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let base = build_display_list(&tree);
        let with_none = build_display_list_with_anim(&tree, None);
        assert_eq!(base.len(), with_none.len());
    }

    #[test]
    fn anim_opacity_override_emits_push_opacity() {
        // A div without opacity in style — no PushOpacity in base DL.
        let html = r#"<div style="background:green;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));

        let base = build_display_list(&tree);
        let has_push_base = base.iter().any(|c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert!(!has_push_base, "base DL should have no PushOpacity");

        // Override opacity=0.5 for the body node (root).
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.5), transform: None });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let anim_dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        assert_eq!(push_count, 1, "should emit one PushOpacity for the animated node");
        assert_eq!(pop_count, 1, "PushOpacity/PopOpacity must be balanced");

        if let Some(DisplayCommand::PushOpacity { alpha }) = anim_dl.iter().find(|c| matches!(c, DisplayCommand::PushOpacity { .. })) {
            assert!((*alpha - 0.5).abs() < 1e-5, "opacity should be 0.5, got {alpha}");
        }
    }

    #[test]
    fn anim_push_pop_balanced() {
        // Any DL produced by with_anim must have balanced Push/Pop pairs.
        let html = r#"<div style="background:red;width:200px;height:100px">
            <div style="background:blue;width:100px;height:50px"></div>
        </div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.7), transform: None });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        let push_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PushTransform { .. })).count();
        let pop_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PopTransform)).count();
        assert_eq!(push_op, pop_op, "PushOpacity/PopOpacity must balance");
        assert_eq!(push_tx, pop_tx, "PushTransform/PopTransform must balance");
    }
}
