//! Display list — линейный список графических команд, выработанных из
//! дерева layout. Растеризатору (renderer) уже не нужно понимать DOM/CSS:
//! он рендерит то, что ему говорят.
//!
//! Phase 0 — только `FillRect` и `DrawText`. Тени, скругления, градиенты,
//! border-радиусы — позже, по запросу. Координаты — экранные пиксели от
//! верхнего левого угла окна.

use lumen_core::geom::Rect;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, BoxKind, Color, FontStyle, FontWeight,
    InlineFrag, LayoutBox, MixBlendMode as LayoutBlendMode, ObjectFit, ObjectPosition, Overflow,
    PaintOrder, PaintPhase, PositionComponent, StackingContextId, StackingTree,
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
        /// Variable Fonts L1 — per-axis **normalized** variation coordinates
        /// (`[-1.0, 1.0]` per axis, длина = `Font::fvar().axis_count`
        /// выбранного face-а). Пустой Vec = default-instance (как если бы
        /// `font-variation-settings: normal`); никаких deltas не применяется,
        /// растеризация эквивалентна pre-variable-fonts поведению.
        ///
        /// Layer responsibility: P1 cascade `font-variation-settings`
        /// формирует userspace `(tag, value)`-список, **затем нормализует**
        /// в этот вектор через `fvar.axes` clamping + `avar.normalize`
        /// перед эмиссией DrawText. Renderer (P2) использует его как
        /// (a) аргумент `Font::glyph_resolved_with_coords`, (b) часть atlas
        /// cache key через `AtlasKey::hash_coords` — без этого variant glyph
        /// перезаписывал бы default-instance в multi-size atlas.
        ///
        /// Phase 0: P1 cascade пока не реализован — все text-emission
        /// сайты эмитят empty Vec (default-instance), пути renderer-а
        /// short-circuit-ятся. Это interface-first hook для P1.
        font_variation_coords: Vec<f32>,
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
    /// Sprint 0 P2 stub. Открывает blend-группу с указанным режимом
    /// смешения (CSS Compositing & Blending L1 §5). Все последующие
    /// команды до парного `PopBlendMode` композитятся через `mode` поверх
    /// родительского контекста. `BlendMode::Normal` — no-op (стандарт).
    /// Phase 0: эмиттер не выпускает, renderer игнорирует — реальный
    /// blend-pipeline это P2 п.4.
    PushBlendMode { mode: BlendMode },
    /// Закрывает blend-группу.
    PopBlendMode,
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
/// - `DrawText (x.xx, y.xx, w.xx, h.xx) "text" fs.xx #rrggbbaa`
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
            DisplayCommand::DrawBorder { rect, widths: [wt, wr, wb, wl], colors: [ct, cr, cb, cl] } => {
                out.push_str(&format!(
                    "DrawBorder ({:.2}, {:.2}, {:.2}, {:.2}) \
                     w=[{:.2},{:.2},{:.2},{:.2}] \
                     c=[#{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x},\
                        #{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x}]\n",
                    rect.x, rect.y, rect.width, rect.height,
                    wt, wr, wb, wl,
                    ct.r, ct.g, ct.b, ct.a,
                    cr.r, cr.g, cr.b, cr.a,
                    cb.r, cb.g, cb.b, cb.a,
                    cl.r, cl.g, cl.b, cl.a,
                ));
            }
            DisplayCommand::DrawText {
                rect, text, font_size, color, font_family, font_weight, font_style,
                font_variation_coords,
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
                if !font_variation_coords.is_empty() {
                    out.push_str(" var=[");
                    for (i, &c) in font_variation_coords.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&format!("{c:.3}"));
                    }
                    out.push(']');
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
        }
    }
    out
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
///
/// Порядок Push-команд (для child compositor-а смысла не несёт, но
/// детерминирован для тестируемости): Clip → Blend → Opacity. Pop —
/// в обратном (Opacity → Blend → Clip). Так visual-результат не зависит:
/// все эффекты применяются на off-screen-layer-е одного box-а.
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

/// Эмитит DisplayCommand-ы для одного box-а БЕЗ рекурсии в детей. Аналог
/// тела `walk` для одного box-а.
fn emit_box_self(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            if let Some(bg) = b.style.background_color
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect {
                    rect: b.rect,
                    color: bg,
                });
            }
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
                        s.border_top_color.unwrap_or(cur),
                        s.border_right_color.unwrap_or(cur),
                        s.border_bottom_color.unwrap_or(cur),
                        s.border_left_color.unwrap_or(cur),
                    ],
                });
            }
        }
        BoxKind::InlineRun { lines, .. } => {
            let line_h = b.style.font_size * b.style.line_height;
            for (line_idx, line) in lines.iter().enumerate() {
                let line_y = b.rect.y + line_idx as f32 * line_h;
                for frag in line {
                    out.push(DisplayCommand::DrawText {
                        rect: Rect::new(b.rect.x + frag.x, line_y, b.rect.width, line_h),
                        text: frag.text.clone(),
                        font_size: frag.style.font_size,
                        color: frag.style.color,
                        font_family: frag.style.font_family.clone(),
                        font_weight: frag.style.font_weight,
                        font_style: frag.style.font_style,
                        // P1 cascade `font-variation-settings` пока не
                        // реализован — emit default-instance (empty Vec).
                        // Renderer short-circuit-ит на pre-VF путь.
                        font_variation_coords: Vec::new(),
                    });
                    push_text_decoration(out, b.rect.x, line_y, frag);
                }
            }
        }
        BoxKind::Image { src, alt } => {
            if let Some(bg) = b.style.background_color
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect {
                    rect: b.rect,
                    color: bg,
                });
            }
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
                        s.border_top_color.unwrap_or(cur),
                        s.border_right_color.unwrap_or(cur),
                        s.border_bottom_color.unwrap_or(cur),
                        s.border_left_color.unwrap_or(cur),
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
        }
    }
}

fn walk(b: &LayoutBox, out: &mut DisplayList) {
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            if let Some(bg) = b.style.background_color
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect {
                    rect: b.rect,
                    color: bg,
                });
            }
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
                        s.border_top_color.unwrap_or(cur),
                        s.border_right_color.unwrap_or(cur),
                        s.border_bottom_color.unwrap_or(cur),
                        s.border_left_color.unwrap_or(cur),
                    ],
                });
            }
            for child in &b.children {
                walk(child, out);
            }
        }
        BoxKind::InlineRun { lines, .. } => {
            let line_h = b.style.font_size * b.style.line_height;
            for (line_idx, line) in lines.iter().enumerate() {
                let line_y = b.rect.y + line_idx as f32 * line_h;
                for frag in line {
                    out.push(DisplayCommand::DrawText {
                        rect: Rect::new(b.rect.x + frag.x, line_y, b.rect.width, line_h),
                        text: frag.text.clone(),
                        font_size: frag.style.font_size,
                        color: frag.style.color,
                        font_family: frag.style.font_family.clone(),
                        font_weight: frag.style.font_weight,
                        font_style: frag.style.font_style,
                        // P1 cascade `font-variation-settings` пока не
                        // реализован — emit default-instance (empty Vec).
                        // Renderer short-circuit-ит на pre-VF путь.
                        font_variation_coords: Vec::new(),
                    });
                    push_text_decoration(out, b.rect.x, line_y, frag);
                }
            }
        }
        BoxKind::Image { src, alt } => {
            // Painter's order для replaced element: фон → border → image.
            // background/border у `<img>` валидны по CSS — например, для
            // подложки на время загрузки или рамки вокруг картинки.
            if let Some(bg) = b.style.background_color
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect {
                    rect: b.rect,
                    color: bg,
                });
            }
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
                        s.border_top_color.unwrap_or(cur),
                        s.border_right_color.unwrap_or(cur),
                        s.border_bottom_color.unwrap_or(cur),
                        s.border_left_color.unwrap_or(cur),
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
        }
    }
}

/// Эмитит FillRect-ы для активных линий text-decoration. Геометрия —
/// приблизительная: baseline ≈ line_y + font_size * 0.80 (соответствует
/// ascent ratio Inter, на котором рендерер позиционирует глифы). Толщина —
/// около 7% от font_size, минимум 1px. Цвет — цвет самого фрагмента
/// (упрощение Phase 0 — CSS3 говорит использовать text-decoration-color,
/// который у нас не реализован, поэтому falls back на currentColor).
fn push_text_decoration(out: &mut DisplayList, container_x: f32, line_y: f32, frag: &InlineFrag) {
    let decoration = frag.style.text_decoration_line;
    if decoration.is_empty() || frag.width <= 0.0 {
        return;
    }
    let fs = frag.style.font_size;
    let baseline_y = line_y + fs * 0.80;
    let thickness = (fs * 0.07).max(1.0);
    let x = container_x + frag.x;
    // CSS Text Decoration L3 §3: text-decoration-color, fallback на
    // currentColor (= frag.style.color).
    let color = frag.style.text_decoration_color.unwrap_or(frag.style.color);

    if decoration.underline {
        // Под baseline, ниже на ~10% от размера шрифта.
        let y = baseline_y + fs * 0.10;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y, frag.width, thickness),
            color,
        });
    }
    if decoration.line_through {
        // Примерно по середине строчных букв (mid x-height): ~30% выше baseline.
        let y = baseline_y - fs * 0.30;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y, frag.width, thickness),
            color,
        });
    }
    if decoration.overline {
        // Чуть выше верха capital-line (≈ font_size * 0.75 над baseline).
        let y = baseline_y - fs * 0.78;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y, frag.width, thickness),
            color,
        });
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
        if let DisplayCommand::DrawBorder { widths, colors, .. } = b[0] {
            assert!((widths[0] - 2.0).abs() < 0.01, "top width");
            assert!((widths[1] - 2.0).abs() < 0.01, "right width");
            assert_eq!(colors[0].r, 255, "top color — red");
        }
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
                DisplayCommand::DrawImage { .. } => "DrawImage",
                DisplayCommand::DrawText { .. } => "DrawText",
                DisplayCommand::PushClipRect { .. } => "PushClipRect",
                DisplayCommand::PopClip => "PopClip",
                DisplayCommand::PushOpacity { .. } => "PushOpacity",
                DisplayCommand::PopOpacity => "PopOpacity",
                DisplayCommand::PushBlendMode { .. } => "PushBlendMode",
                DisplayCommand::PopBlendMode => "PopBlendMode",
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
}
