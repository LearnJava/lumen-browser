//! Style cascade с поддержкой compound и complex selectors, attribute и
//! pseudo-class matching, specificity по CSS Selectors Level 3.
//!
//! Алгоритм каскада: для каждого правила в stylesheet проверяем, матчит ли оно
//! целевой элемент. Если матчит — для каждой декларации записываем «применять с
//! приоритетом (specificity, source_order)». В конце сортируем все
//! применимые декларации по этому ключу (по возрастанию) и применяем — так
//! правило с большей specificity перекрывает меньшую, а при равенстве выигрывает
//! более позднее.
//!
//! Matching complex selector-а — справа налево, жадно: для каждого combinator-а
//! берём первого подходящего предка/sibling-а без back-tracking. Для большинства
//! реальных страниц этого достаточно; патологические случаи `a b c` с
//! вложенными `a`-предками могут промахнуться — это известное упрощение, до
//! фазы со «честным» Selectors-движком.

use std::collections::HashMap;

use lumen_core::geom::Size;
use lumen_css_parser::{
    parse_inline_style, AttrOp, AttrSelector, Combinator, ComplexSelector, CompoundSelector,
    Declaration, DirArg, MediaContext, PropertyRule, PseudoClass, SimpleSelector, Specificity,
    Stylesheet,
};
use lumen_dom::{Attribute, Document, DocumentMode, NodeData, NodeId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    /// CSS Flexbox L1 §3 — `display: flex`. Phase 0: парсится и хранится,
    /// но в layout трактуется как `Block` (нет flex-алгоритма). Реальный
    /// flex-pass — отдельная задача.
    Flex,
    /// `display: inline-flex` — аналогично, парсится но трактуется как Inline.
    InlineFlex,
    /// CSS Grid L1 — `display: grid`. Парсится, трактуется как Block.
    Grid,
    /// `display: inline-grid`.
    InlineGrid,
    /// CSS 2.1 §9.2.4 — `display: inline-block`. Внешне ведёт себя как
    /// inline (участвует в inline-потоке родителя), внутри — block
    /// formatting context (имеет собственные width/height/padding/border).
    /// В layout собирается в `BoxKind::InlineBlockRow`.
    InlineBlock,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// CSS Writing Modes L3 §2.1 — `direction: ltr | rtl`. Inherited.
///
/// Базовое направление потока inline-контента. В Phase 0 layout только
/// хранит значение и распространяет через каскад — реальное применение
/// (RTL line-flow, перенос pivot point, bidi reordering через Unicode
/// Bidi Algorithm) требует Bidi-движка и переписанного wrap_inline_run.
/// Однако зафиксировать direction в `ComputedStyle` сейчас полезно для
/// двух будущих задач: (1) когда появится `dir="rtl"` HTML-атрибут или
/// `<bdo>` — у нас уже есть точка хранения; (2) когда возьмёмся за bidi —
/// каскад уже даёт нам базовое направление, не нужно его ретрофитить.
///
/// `rtl` пока не меняет рендеринг — это явный «отложено», документированный
/// в roadmap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// CSS Backgrounds L3 §4.6 — спецификация одной тени бокса.
///
/// `inset` тени рисуются внутри коробки (имитация vignetting), не-inset —
/// снаружи (drop-shadow). Color None = currentColor по spec. Blur и spread
/// — длины в пикселях; spread увеличивает / уменьшает форму перед blur-ом.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Option<Color>,
    pub inset: bool,
}

/// CSS Text Decoration L3 §4 — спецификация одной тени текста.
///
/// Отличается от BoxShadow: нет `inset`, нет `spread`. Color None =
/// currentColor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub color: Option<Color>,
}

/// CSS UI L4 §8.1 — `cursor`. Inherited.
///
/// Хранится как enum 17 стандартных keyword-ов. URL-fallback (`cursor:
/// url(custom.png), pointer`) отложен. `Auto` — пусть UA решает (для
/// большинства это `Default`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Auto,
    Default,
    None,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    AllScroll,
    ColResize,
    RowResize,
    NResize,
    EResize,
    SResize,
    WResize,
    NeResize,
    NwResize,
    SeResize,
    SwResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ZoomIn,
    ZoomOut,
}

/// CSS UI L4 §10.1 — `text-overflow`. Не наследуется.
///
/// Применяется к содержимому, которое не помещается в коробку — то есть
/// требует overflow != Visible (обычно `hidden`/`clip`) И отсутствие
/// переноса (white-space: nowrap или overflow на oneline). Без этих
/// условий не имеет эффекта.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// CSS Overflow L3 — `overflow`. Не наследуется.
///
/// `Visible` — содержимое выходит за пределы коробки и видно. `Hidden` —
/// клипуется (без скроллбара). `Clip` — то же, но без формирования
/// scroll container и без поддержки `overflow-anchor`. `Scroll` — всегда
/// показать scrollbar, `Auto` — показать только если контент не влезает.
/// Phase 0 layout только хранит — реальный clipping / scroll в paint
/// pipeline ещё нет.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

/// CSS Display L3 §4 — `visibility`. Inherited.
///
/// В отличие от `display: none`, элемент с `visibility: hidden` участвует
/// в layout (занимает место), но не рисуется. `Collapse` для table-row
/// эквивалентен `display: none` (CSS spec); вне таблиц ведёт себя как
/// `Hidden`. Inheritance — ключевое отличие от display, поэтому дочерний
/// элемент может явно вернуть себя через `visibility: visible`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

/// CSS Text Module L3 §3.1 — `white-space`. Inherited.
///
/// Управляет collapse-ом whitespace и переносами строк. Phase 0:
/// реализованы только `Normal` (default — collapse + wrap) и `Nowrap`
/// (collapse, без переноса). `Pre`/`PreWrap`/`PreLine` требуют сохранения
/// whitespace в input (сейчас split_whitespace его теряет) — отложены.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
}

/// CSS Text Module L3 §3.4 — `text-transform`. Inherited.
///
/// Применяется к текстовому содержимому при сборке inline-сегментов, до
/// word-wrapping и measurer-а. Cyrillic case-folding делается через
/// `char::to_uppercase` / `to_lowercase` стандартной библиотеки, что даёт
/// правильную обработку русских букв (А↔а, Я↔я и т.д.) без сюрпризов
/// типа турецкого `i`/`I`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    /// `capitalize`: первая буква каждого «слова» (по spec — character с
    /// Unicode property Letter) в верхний регистр. Phase 0: упрощённо —
    /// первая буква каждого whitespace-разделённого токена.
    Capitalize,
}

impl TextTransform {
    /// Применяет преобразование к строке. Не аллоцирует, если transform = None.
    pub fn apply(self, s: &str) -> String {
        match self {
            TextTransform::None => s.to_string(),
            TextTransform::Uppercase => s.to_uppercase(),
            TextTransform::Lowercase => s.to_lowercase(),
            TextTransform::Capitalize => {
                let mut out = String::with_capacity(s.len());
                let mut at_word_start = true;
                for ch in s.chars() {
                    if ch.is_whitespace() {
                        out.push(ch);
                        at_word_start = true;
                    } else if at_word_start {
                        out.extend(ch.to_uppercase());
                        at_word_start = false;
                    } else {
                        out.push(ch);
                    }
                }
                out
            }
        }
    }
}

/// CSS Fonts Module L4: `font-style: normal | italic | oblique`. Inherited.
///
/// Phase 0: layout различает свойство, рендерер пока использует один
/// шрифтовой файл (Inter Regular) и не отрисовывает italic-вариант. Поле
/// нужно, чтобы `text_rendering_eq` правильно разделял inline-фрагменты
/// — это корректно подготавливает структуру под подключение Italic-fontfile
/// или affine-skew transform позже.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// CSS Fonts L4 §6 — `font-variant` (упрощённый Phase 0). Inherited.
///
/// Полный `font-variant` — это shorthand над font-variant-caps,
/// -ligatures, -numeric и т.д. (CSS Fonts L4). Phase 0 поддерживаем
/// только два самых частых значения: `normal` и `small-caps`. Real
/// small-caps rendering требует OpenType feature `smcp` или fallback
/// на uppercase + меньший font-size — отложено.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontVariant {
    #[default]
    Normal,
    SmallCaps,
}

/// CSS Fonts Module L4 §2.5 — `font-stretch`. Inherited.
///
/// Хранится в десятых долях процента (u16): `normal` = 1000 (100%),
/// `condensed` = 750 (75%), `expanded` = 1250 (125%). Десятые нужны
/// из-за дробных keyword-ов: `semi-condensed` = 87.5% → 875,
/// `semi-expanded` = 112.5% → 1125. Численные проценты парсятся в
/// том же масштабе и клампятся в [50%, 200%] — Phase 0 не нужны
/// экстремальные значения, и это удерживает значение в u16 без
/// переполнения.
///
/// Phase 0: layout различает свойство, рендерер всегда Inter Regular
/// (real stretch-варианты требуют variable-font wdth-axis или отдельные
/// fontfiles). `text_rendering_eq` учитывает stretch, чтобы фрагменты
/// с разным stretch не сливались.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontStretch(pub u16);

impl FontStretch {
    /// 100% — нормальная ширина.
    pub const NORMAL: Self = Self(1000);

    fn from_keyword(kw: &str) -> Option<Self> {
        Some(match kw {
            "ultra-condensed" => Self(500),
            "extra-condensed" => Self(625),
            "condensed" => Self(750),
            "semi-condensed" => Self(875),
            "normal" => Self(1000),
            "semi-expanded" => Self(1125),
            "expanded" => Self(1250),
            "extra-expanded" => Self(1500),
            "ultra-expanded" => Self(2000),
            _ => return None,
        })
    }
}

impl Default for FontStretch {
    fn default() -> Self { Self::NORMAL }
}

/// CSS Fonts Module L4 §2.4 — `font-weight`. Inherited.
///
/// Хранится численно (1..1000), как в spec: `normal` = 400, `bold` = 700.
/// Ключевые слова `lighter` / `bolder` относительные — их разрешение
/// (по правилам §2.4.3) делается при парсинге: смотрим на родительский weight
/// и сдвигаем по таблице. `lighter` от 400 = 100; `bolder` от 400 = 700.
///
/// Phase 0: layout различает свойство, рендерер пока всегда Inter Regular —
/// real bold-варианта файлов нет. text_rendering_eq учитывает weight, чтобы
/// bold-фрагменты не сливались с обычными.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: Self = Self(400);
    pub const BOLD: Self = Self(700);

    pub fn is_bold(self) -> bool {
        self.0 >= 600
    }
}

impl Default for FontWeight {
    fn default() -> Self { Self::NORMAL }
}

/// Набор активных линий `text-decoration` для элемента.
///
/// CSS3 разделяет shorthand `text-decoration` на `-line`, `-style`, `-color`;
/// Phase 0 умеет только line (без двойных линий и кастомных цветов). Спецификация
/// CSS3 не наследует text-decoration-line, но визуально декорация всё равно
/// распространяется на потомков. Мы делаем явное наследование — это эквивалентно
/// поведению, ожидаемому от `a { text-decoration: underline }`, и при этом
/// позволяет дочернему элементу явно сбросить декорацию через
/// `text-decoration: none` (CSS3 для этого требует пересоздать stacking context,
/// но в нашей упрощённой модели достаточно перезаписать поле).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextDecorationLine {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

impl TextDecorationLine {
    pub const fn is_empty(self) -> bool {
        !self.underline && !self.overline && !self.line_through
    }
}

/// CSS Text Decoration L3 §2.2 — `text-decoration-style`. Стиль штриха
/// для всех активных линий (`underline` / `overline` / `line-through`).
///
/// Spec inherited: no — но в Phase 0 наследуем визуально, по той же причине
/// что [`TextDecorationLine`] (см. doc-комментарий выше).
///
/// Initial: `Solid`. Phase 0 рендерер рисует все стили как Solid одиночной
/// линией; реальное визуальное отличие (`Double` — две параллельные,
/// `Dotted` / `Dashed` — pattern, `Wavy` — синусоида) — задача P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

impl TextDecorationStyle {
    /// Парсит одиночный keyword. Возвращает `None` для невалидных и для
    /// keyword-ов, имеющих другой смысл в context-е shorthand (например,
    /// `none` — это `<line>`, не `<style>`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "solid" => Some(Self::Solid),
            "double" => Some(Self::Double),
            "dotted" => Some(Self::Dotted),
            "dashed" => Some(Self::Dashed),
            "wavy" => Some(Self::Wavy),
            _ => None,
        }
    }
}

/// CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Толщина
/// штриха для линий декорации.
///
/// - `Auto` — UA выбирает (наш default; в Phase 0 рендерер использует 1px).
/// - `FromFont` — берётся из шрифтового `underlinePosition` / `underlineThickness`
///   (post-таблица), если шрифт их экспортирует; иначе как `Auto`.
/// - `Length(px)` — явная resolved-px толщина (после `<length>` resolution).
/// - `Percentage(frac)` — доля от **1em parent font-size** (spec явно
///   ссылается на parent, не на свой font-size). Храним как fraction
///   `0.05` для `5%`; resolved-px вычисляется в renderer-е, где известен
///   parent.font_size.
///
/// Spec inherited: no — но в Phase 0 наследуем визуально, по той же причине
/// что [`TextDecorationLine`].
///
/// Phase 0 рендерер игнорирует это значение (всегда 1px); реальное
/// использование — задача P2.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TextDecorationThickness {
    #[default]
    Auto,
    FromFont,
    Length(f32),
    Percentage(f32),
}

/// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Форма emphasis-marks
/// (точечный набор над/под глифами).
///
/// Spec inherited: yes.
///
/// Grammar: `none | [ [ filled | open ] || [ dot | circle | double-circle |
/// triangle | sesame ] ] | <string>`. Если задан только fill keyword без
/// shape — UA fallback shape = `circle` для horizontal writing mode
/// (Phase 0 единственный supported); для vertical было бы `sesame`.
/// Если задан только shape без fill — fallback fill = `filled`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TextEmphasisStyle {
    #[default]
    None,
    /// Один из 5 предустановленных shape-ов, заполненный или контурный.
    Symbol {
        filled: bool,
        shape: TextEmphasisShape,
    },
    /// Произвольная строка-mark (по spec — первый grapheme cluster; в
    /// Phase 0 храним всю строку как есть, рендерер сам возьмёт первый
    /// graphem).
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEmphasisShape {
    Dot,
    #[default]
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
}

/// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Сторона
/// относительно текстовой строки, на которой рисуются marks.
///
/// Grammar: `[ over | under ] && [ right | left ]?`. Initial `over right`
/// для horizontal writing mode (наш default; для vertical было бы `over
/// right` тоже, но right имеет другой геометрический смысл — Phase 0 без
/// writing-mode не различает).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEmphasisPosition {
    #[default]
    OverRight,
    OverLeft,
    UnderRight,
    UnderLeft,
}

impl TextEmphasisPosition {
    pub fn is_over(self) -> bool {
        matches!(self, Self::OverRight | Self::OverLeft)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

/// Стиль линии CSS border. None = рамка не отображается (как `display: none`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None)
    }
}

/// CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
/// keyword-ы плюс `auto` (UA-defined focus indicator).
///
/// Phase 0: `Auto` рендерится как Solid с currentColor; отдельный variant
/// сохраняется, чтобы позже отличить «явный solid от автора» от «default
/// UA focus ring» — нужно для accessibility (нельзя глушить focus ring
/// через `outline-style: none` при `:focus-visible` в стиле UA).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineStyle {
    #[default]
    None,
    Auto,
    Solid,
    Dashed,
    Dotted,
}

impl OutlineStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, OutlineStyle::None)
    }
}

/// CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
/// `auto` (UA-defined контрастный цвет) и `currentColor` (вычисленный `color`
/// элемента).
///
/// Phase 0: `Auto` и `CurrentColor` оба резолвятся в `style.color` при
/// рендеринге — настоящий UA contrast требует знания фона за outline и
/// откладывается.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineColor {
    #[default]
    Auto,
    CurrentColor,
    Color(Color),
}

/// CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside.
/// Phase 0: parse+store; реальный break enforcement требует pagination /
/// multi-column layout pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BreakValue {
    #[default]
    Auto,
    /// `avoid` / `avoid-page` / `avoid-column` / `avoid-region` — все
    /// нормализуются в `Avoid`. Phase 0 не различает page vs column vs region.
    Avoid,
    /// `always` / `page` (для break-before/after).
    Always,
    /// `column` — принудительный column break.
    Column,
    /// `page` — принудительный page break.
    Page,
    /// `region` — принудительный region break.
    Region,
}

/// CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
///   - `ContentBox` (CSS default): размер контента; padding и border прибавляются сверху.
///   - `BorderBox`: размер вместе с padding и border; контент сжимается, чтобы влезть.
///
/// Свойство НЕ наследуется (CSS Basic UI 3 §4.1) — сбрасывается на default в каждом
/// `compute_style`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

/// CSS Positioned Layout L3 §3 — `position`. Не наследуется.
/// `Static` — нормальный поток (default). Остальные создают
/// containing-block-альтернативу и (для `Fixed` / `Sticky`, а также
/// `Relative` / `Absolute` с явным `z-index`) могут создавать
/// stacking context (§9.10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "relative" => Some(Self::Relative),
            "absolute" => Some(Self::Absolute),
            "fixed" => Some(Self::Fixed),
            "sticky" => Some(Self::Sticky),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
/// `Isolate` принудительно создаёт stacking context, обеспечивая
/// изоляцию blend / backdrop-filter эффектов потомков от внешних
/// слоёв.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}

impl Isolation {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "isolate" => Some(Self::Isolate),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
/// Любое значение, отличное от `Normal`, создаёт stacking context
/// (§9.10). Phase 0 layout только хранит — реальный compositor pipeline
/// для blend-effects появится у P2 (§16 трек, п.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MixBlendMode {
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

impl MixBlendMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "multiply" => Some(Self::Multiply),
            "screen" => Some(Self::Screen),
            "overlay" => Some(Self::Overlay),
            "darken" => Some(Self::Darken),
            "lighten" => Some(Self::Lighten),
            "color-dodge" => Some(Self::ColorDodge),
            "color-burn" => Some(Self::ColorBurn),
            "hard-light" => Some(Self::HardLight),
            "soft-light" => Some(Self::SoftLight),
            "difference" => Some(Self::Difference),
            "exclusion" => Some(Self::Exclusion),
            "hue" => Some(Self::Hue),
            "saturation" => Some(Self::Saturation),
            "color" => Some(Self::Color),
            "luminosity" => Some(Self::Luminosity),
            "plus-lighter" => Some(Self::PlusLighter),
            _ => None,
        }
    }
}

/// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
/// Default `Baseline`.
///
/// Keyword-варианты (`Baseline`, `Sub`, `Super`, `Top`, `TextTop`, `Middle`,
/// `Bottom`, `TextBottom`) — fixed enum values. `Length(px)` — resolved
/// сдвиг по вертикали от baseline (positive = up по CSS, как у всех
/// vertical-shift свойств). `Percent(p)` — процент от `line-height` текущего
/// элемента; разрешается во время layout-а, поскольку требует line-box
/// геометрии.
///
/// Phase 0: parsing + storage. Реальное применение к inline-flow требует
/// поля `y_offset` в `InlineFrag` и совместной правки `lumen-paint`
/// (DrawText.y-offset) — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    Bottom,
    TextBottom,
    /// Resolved px. Положительное — выше baseline, отрицательное — ниже
    /// (как `<length>` в CSS 2.1 §10.8.1).
    Length(f32),
    /// Процент от `line-height` элемента (CSS 2.1 §10.8.1). Резолвится
    /// в layout-pass — здесь хранится как есть.
    Percent(f32),
}

impl VerticalAlign {
    /// Парсит keyword-формы vertical-align. Не покрывает `<length>` /
    /// `<percentage>` — те идут через [`parse_length`] (см. apply_declaration).
    pub fn parse_keyword(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline" => Some(Self::Baseline),
            "sub" => Some(Self::Sub),
            "super" => Some(Self::Super),
            "top" => Some(Self::Top),
            "text-top" => Some(Self::TextTop),
            "middle" => Some(Self::Middle),
            "bottom" => Some(Self::Bottom),
            "text-bottom" => Some(Self::TextBottom),
            _ => None,
        }
    }
}


/// CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations.
/// Не наследуется (используется как per-list-entry значение в
/// transition/animation longhand-ах). Default по spec — `ease`, что
/// эквивалентно `cubic-bezier(0.25, 0.1, 0.25, 1.0)`.
///
/// P2 п.3B compositor offload и P1 п.3A Web Animations interpolation —
/// потребители этого AST: оба применяют функцию `progress(t) → [0, 1]`
/// к линейному времени `t ∈ [0, 1]` для получения eased progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimingFunction {
    /// `linear` ≡ `cubic-bezier(0, 0, 1, 1)`. progress(t) = t.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`. Также покрывает keyword-shortcuts:
    /// `ease` ≡ (0.25, 0.1, 0.25, 1.0);
    /// `ease-in` ≡ (0.42, 0, 1, 1);
    /// `ease-out` ≡ (0, 0, 0.58, 1);
    /// `ease-in-out` ≡ (0.42, 0, 0.58, 1).
    /// x1, x2 ∈ [0, 1] (spec); y1, y2 — unbounded.
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, <step-position>)`. `step-start` ≡ `steps(1, jump-start)`,
    /// `step-end` ≡ `steps(1, jump-end)`. `n` — положительное целое;
    /// для `jump-none` ещё и ≥ 2.
    Steps(u32, StepPosition),
}

impl Default for TimingFunction {
    fn default() -> Self {
        // CSS Transitions/Animations L1 — initial value = `ease`.
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    }
}

impl TimingFunction {
    /// Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
    /// `ease-in-out` / `step-start` / `step-end`) или функцию
    /// (`cubic-bezier(...)` / `steps(...)`). Возвращает `None` для
    /// невалидного значения (out-of-range x, n=0, неизвестный keyword).
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "linear" => return Some(Self::Linear),
            "ease" => return Some(Self::CubicBezier(0.25, 0.1, 0.25, 1.0)),
            "ease-in" => return Some(Self::CubicBezier(0.42, 0.0, 1.0, 1.0)),
            "ease-out" => return Some(Self::CubicBezier(0.0, 0.0, 0.58, 1.0)),
            "ease-in-out" => return Some(Self::CubicBezier(0.42, 0.0, 0.58, 1.0)),
            "step-start" => return Some(Self::Steps(1, StepPosition::JumpStart)),
            "step-end" => return Some(Self::Steps(1, StepPosition::JumpEnd)),
            _ => {}
        }
        if let Some(args) = t
            .strip_prefix("cubic-bezier(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.len() != 4 {
                return None;
            }
            let x1 = parts[0].parse::<f32>().ok()?;
            let y1 = parts[1].parse::<f32>().ok()?;
            let x2 = parts[2].parse::<f32>().ok()?;
            let y2 = parts[3].parse::<f32>().ok()?;
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return None;
            }
            return Some(Self::CubicBezier(x1, y1, x2, y2));
        }
        if let Some(args) = t
            .strip_prefix("steps(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.is_empty() || parts.len() > 2 {
                return None;
            }
            let n = parts[0].parse::<u32>().ok()?;
            if n == 0 {
                return None;
            }
            let pos = match parts.get(1).copied() {
                None => StepPosition::JumpEnd,
                Some("start") | Some("jump-start") => StepPosition::JumpStart,
                Some("end") | Some("jump-end") => StepPosition::JumpEnd,
                Some("jump-none") => {
                    if n < 2 {
                        return None;
                    }
                    StepPosition::JumpNone
                }
                Some("jump-both") => StepPosition::JumpBoth,
                _ => return None,
            };
            return Some(Self::Steps(n, pos));
        }
        None
    }

    /// CSS Transitions/Animations L1 — comma-list of timing functions.
    /// Пустые / невалидные entry — пропускаются (best-effort lenient).
    pub fn parse_list(s: &str) -> Vec<TimingFunction> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(TimingFunction::parse)
            .collect()
    }

    /// CSS Easing L1 §2 — компьютация eased progress.
    ///
    /// Принимает линейный input ratio `t ∈ [0, 1]` (input progress по spec)
    /// и возвращает output progress в [0, 1] для `Linear` и `Steps`. Для
    /// `CubicBezier` выход может выходить за `[0, 1]` (overshoot — клиент
    /// либо clamp-ает при применении к Length/Color, либо использует напрямую
    /// — например для `transform`).
    ///
    /// Вне `[0, 1]` входное `t` clamp-ается, как требует §2: «If input
    /// progress is less than 0, return 0. If input progress is greater
    /// than 1, return 1.» (реальные `fill-mode` / `direction` обрабатываются
    /// в animation engine ДО вызова progress().)
    pub fn progress(&self, t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        match *self {
            TimingFunction::Linear => x,
            TimingFunction::CubicBezier(x1, y1, x2, y2) => cubic_bezier_progress(x1, y1, x2, y2, x),
            TimingFunction::Steps(n, position) => steps_progress(n, position, x),
        }
    }
}

/// CSS Easing L1 §2.3 — cubic bezier easing. Кривая определена двумя
/// контрольными точками `(x1, y1)`, `(x2, y2)` с эндпоинтами `(0, 0)`,
/// `(1, 1)`. По заданному `x` (== input progress) находим параметр `u`,
/// такой что `bezier_axis(u, x1, x2) = x`, и возвращаем
/// `bezier_axis(u, y1, y2)` — eased output.
///
/// Алгоритм: Newton-Raphson (быстрая сходимость в большинстве кейсов) с
/// bisection fallback на случай, когда производная около нуля или Newton
/// расходится. Стандартный подход в Blink/WebKit/Gecko.
fn cubic_bezier_progress(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let u = solve_bezier_x(x1, x2, x);
    bezier_axis(u, y1, y2)
}

/// `B(u) = 3(1-u)²·u·c1 + 3(1-u)·u²·c2 + u³` для P0=(0,0), P3=(1,1).
fn bezier_axis(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * u * c1 + 3.0 * omu * u * u * c2 + u * u * u
}

/// `B'(u) = 3(1-u)²·c1 + 6(1-u)·u·(c2-c1) + 3u²·(1-c2)`.
fn bezier_axis_derivative(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * c1 + 6.0 * omu * u * (c2 - c1) + 3.0 * u * u * (1.0 - c2)
}

/// Solve `bezier_axis(u, x1, x2) = x` for `u ∈ [0, 1]`.
fn solve_bezier_x(x1: f32, x2: f32, x: f32) -> f32 {
    const EPS: f32 = 1e-6;
    let mut u = x;
    for _ in 0..8 {
        let xu = bezier_axis(u, x1, x2);
        let err = xu - x;
        if err.abs() < EPS {
            return u.clamp(0.0, 1.0);
        }
        let d = bezier_axis_derivative(u, x1, x2);
        if d.abs() < EPS {
            break;
        }
        u -= err / d;
        if !u.is_finite() {
            break;
        }
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..64 {
        let mid = (lo + hi) * 0.5;
        let xu = bezier_axis(mid, x1, x2);
        if (xu - x).abs() < EPS || (hi - lo) < EPS {
            return mid;
        }
        if xu < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// CSS Easing L1 §3.2 — `steps(n, <step-position>)` easing.
///
/// step-position определяет, сколько output-уровней и где «прыжки»:
/// - `jump-start` / `start`: n уровней `1/n, 2/n, ..., n/n`. Прыжок при t=0.
/// - `jump-end` / `end` (default): n+1 уровень `0/n, 1/n, ..., n/n`. Прыжок при t=1.
/// - `jump-none`: n уровней `0/(n-1), ..., (n-1)/(n-1) = 1`. Прыжков на границах нет.
/// - `jump-both`: n+2 уровня `1/(n+1), 2/(n+1), ..., (n+1)/(n+1) = 1`. Прыжки на обеих границах.
///
/// Для `t = 0` и `t = 1` корректно clamp-ается до границы output-диапазона.
fn steps_progress(n: u32, position: StepPosition, t: f32) -> f32 {
    let n_f = n as f32;
    let (raw_index, divisor, max_step) = match position {
        StepPosition::JumpStart => ((t * n_f).floor() + 1.0, n_f, n_f),
        StepPosition::JumpEnd => ((t * n_f).floor(), n_f, n_f),
        StepPosition::JumpNone => ((t * n_f).floor(), n_f - 1.0, n_f - 1.0),
        StepPosition::JumpBoth => ((t * n_f).floor() + 1.0, n_f + 1.0, n_f + 1.0),
    };
    let step = raw_index.max(0.0).min(max_step);
    (step / divisor).clamp(0.0, 1.0)
}

/// CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start` (alias `start`) — первый прыжок на t=0,
    /// последний шаг достигает 1 - 1/n.
    JumpStart,
    /// `jump-end` (alias `end`) — первый шаг на t > 0, последний прыжок
    /// на t=1. Default.
    #[default]
    JumpEnd,
    /// `jump-none` — `n` шагов, ни один на границе. Требует n ≥ 2.
    JumpNone,
    /// `jump-both` — n+1 шагов, оба на границах t=0 и t=1.
    JumpBoth,
}

/// CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
/// (может быть дробным; отрицательные значения трактуются как невалидные),
/// либо ключевое слово `infinite`. Default = `Finite(1.0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IterationCount {
    Finite(f32),
    Infinite,
}

impl Default for IterationCount {
    fn default() -> Self {
        IterationCount::Finite(1.0)
    }
}

impl IterationCount {
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("infinite") {
            return Some(Self::Infinite);
        }
        let n = t.parse::<f32>().ok()?;
        if n.is_finite() && n >= 0.0 {
            Some(Self::Finite(n))
        } else {
            None
        }
    }

    pub fn parse_list(s: &str) -> Vec<IterationCount> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(IterationCount::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationDirection {
    /// Прямое воспроизведение каждой итерации (0 → 100%).
    #[default]
    Normal,
    /// Обратное воспроизведение (100% → 0).
    Reverse,
    /// Чётные итерации normal, нечётные reverse.
    Alternate,
    /// Чётные reverse, нечётные normal.
    AlternateReverse,
}

impl AnimationDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "reverse" => Some(Self::Reverse),
            "alternate" => Some(Self::Alternate),
            "alternate-reverse" => Some(Self::AlternateReverse),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationDirection> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationDirection::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`.
/// Определяет, применяются ли значения keyframes до начала и/или после
/// окончания анимации.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationFillMode {
    /// До начала и после конца — используется computed-style без keyframes.
    #[default]
    None,
    /// После окончания — последняя keyframe сохраняется.
    Forwards,
    /// До начала — первая keyframe применяется.
    Backwards,
    /// Both `forwards` и `backwards` одновременно.
    Both,
}

impl AnimationFillMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "forwards" => Some(Self::Forwards),
            "backwards" => Some(Self::Backwards),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationFillMode> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationFillMode::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationPlayState {
    /// Анимация идёт. Default.
    #[default]
    Running,
    /// Пауза — текущее значение фиксируется.
    Paused,
}

impl AnimationPlayState {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationPlayState> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationPlayState::parse)
            .collect()
    }
}

/// CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству.
/// - `Inherit` — взять computed value родителя.
/// - `Initial` — взять initial value свойства из спецификации.
/// - `Unset` — для inherited-свойств = `Inherit`, для non-inherited = `Initial`.
/// - `Revert` — откатиться к значению предыдущего origin (UA → User → Author).
///   В Phase 0 UA / User origin отделены не полностью (только UA-hints для
///   italic/bold семантических тегов), поэтому `Revert` трактуется как
///   `Unset`. Это упрощение и редкие edge case-ы оно «не покажет правильно»,
///   но для типичного CSS-кода работает идентично.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssWideKeyword {
    Inherit,
    Initial,
    Unset,
    Revert,
}

/// ASCII case-insensitive проверка значения декларации на CSS-wide keyword.
/// Любое из четырёх ключевых слов в любом регистре, с trim-ом whitespace,
/// возвращает соответствующий `Some(...)`. Иначе — `None`.
pub fn parse_css_wide_keyword(value: &str) -> Option<CssWideKeyword> {
    let t = value.trim();
    if t.eq_ignore_ascii_case("inherit") {
        Some(CssWideKeyword::Inherit)
    } else if t.eq_ignore_ascii_case("initial") {
        Some(CssWideKeyword::Initial)
    } else if t.eq_ignore_ascii_case("unset") {
        Some(CssWideKeyword::Unset)
    } else if t.eq_ignore_ascii_case("revert") {
        Some(CssWideKeyword::Revert)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub text_align: TextAlign,
    /// CSS Writing Modes L3 §2.1 — направление inline-потока. Inherited.
    /// В Phase 0 layout/paint его пока не применяют — задел под bidi и
    /// HTML `dir`-атрибут. См. `Direction` для подробностей.
    pub direction: Direction,
    pub color: Color,
    pub background_color: Option<Color>,
    pub font_size: f32,
    pub line_height: f32,
    pub font_style: FontStyle,
    pub font_weight: FontWeight,
    /// CSS Fonts L4 §6 — font-variant (Phase 0: normal | small-caps). Inherited.
    pub font_variant: FontVariant,
    /// CSS Fonts L4 §2.5 — font-stretch (десятые доли процента; normal = 1000).
    /// Inherited.
    pub font_stretch: FontStretch,
    /// CSS Fonts L4 §3.1 — font-family как приоритизированный список имён.
    /// Inherited. Phase 0: рендерер пока всегда использует Inter, но layout
    /// уже хранит и распространяет список — задел под будущий font matcher.
    /// Generic-family имена (`serif`, `sans-serif`, `monospace`, `cursive`,
    /// `fantasy`, `system-ui`) сохраняются в этом же списке как обычные строки.
    /// Пустой Vec = inherited / default.
    pub font_family: Vec<String>,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    /// CSS Text L3 §7.1: отступ перед первой строкой inline-content
    /// текущего блока (resolved px). Inherited; применяется к каждому
    /// потомку, который порождает первую строку.
    pub text_indent: f32,
    /// CSS Text L3 §11.2: дополнительное расстояние между каждой парой
    /// символов и между словами (resolved px). Inherited. Может быть
    /// отрицательным (сжимает текст). Применяется в wrap_inline_run при
    /// расчёте ширин.
    pub letter_spacing: f32,
    /// CSS Text L3 §11.3: дополнительное расстояние **между словами**
    /// (resolved px). Inherited. В отличие от `letter-spacing`, добавляется
    /// только на word-boundary, не между всеми символами. Может быть
    /// отрицательным.
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
    /// CSS Text Decoration L3 §3 — `text-decoration-color`. None означает
    /// «использовать currentColor» (то есть `style.color` при рендеринге).
    /// Inherited через каскад (как и `text-decoration-line` в Phase 0 — см.
    /// decisions log).
    pub text_decoration_color: Option<Color>,
    /// CSS Text Decoration L3 §2.2 — `text-decoration-style`. Initial: Solid.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationStyle`]).
    pub text_decoration_style: TextDecorationStyle,
    /// CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Initial: Auto.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationThickness`]).
    pub text_decoration_thickness: TextDecorationThickness,
    /// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Inherited.
    /// Initial: `None` (нет emphasis marks). Phase 0 layout: parse+store;
    /// real rendering поверх каждого глифа — задача P2.
    pub text_emphasis_style: TextEmphasisStyle,
    /// CSS Text Decoration L4 §5.4 — `text-emphasis-color`. Inherited.
    /// Initial: `None` = currentColor (тот же паттерн, что
    /// `text_decoration_color`). При рендере резолвится в `style.color`.
    pub text_emphasis_color: Option<Color>,
    /// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Inherited.
    /// Initial: `OverRight` (horizontal writing-mode).
    pub text_emphasis_position: TextEmphasisPosition,
    /// Явная ширина (CSS `width: Npx`). None = auto (растягивается на контейнер).
    pub width: Option<f32>,
    /// Явная высота (CSS `height: Npx`). None = auto (по содержимому).
    pub height: Option<f32>,
    /// CSS 2.1 §10.4: нижняя граница ширины коробки. None = 0 (default).
    /// Применяется после `width`. Если min > max — побеждает min.
    pub min_width: Option<f32>,
    /// CSS 2.1 §10.4: верхняя граница ширины коробки. None = `none` (без
    /// ограничения).
    pub max_width: Option<f32>,
    /// CSS 2.1 §10.4: нижняя граница высоты коробки. None = 0 (default).
    pub min_height: Option<f32>,
    /// CSS 2.1 §10.4: верхняя граница высоты коробки. None = `none`.
    pub max_height: Option<f32>,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
    /// None = currentColor (используется style.color при рендеринге).
    pub border_top_color: Option<Color>,
    pub border_right_color: Option<Color>,
    pub border_bottom_color: Option<Color>,
    pub border_left_color: Option<Color>,
    pub box_sizing: BoxSizing,
    /// CSS Positioned Layout L3 §3 — `position`. Не наследуется. Default
    /// `Static`. Phase 0 layout не делает позиционирование (offsets top/right/
    /// bottom/left парсятся, но не применяются) — поле используется для
    /// определения stacking context (§9.10) и для будущего позиционирования.
    pub position: Position,
    /// CSS Positioned Layout L3 §9.3 — `z-index: auto | <integer>`. Не
    /// наследуется. `None` = `auto` (stacking context создаётся только если
    /// другие триггеры в §9.10 совпали). `Some(n)` = явный integer; для
    /// positioned- и flex/grid-item элементов это запускает создание
    /// stacking context.
    pub z_index: Option<i32>,
    /// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
    /// `Isolate` создаёт stacking context.
    pub isolation: Isolation,
    /// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
    /// Любое значение, отличное от `Normal`, создаёт stacking context.
    pub mix_blend_mode: MixBlendMode,
    /// CSS Backgrounds L3 §5: радиус скругления углов (resolved px).
    /// Один аспект (без elliptical x/y) — Phase 0 упрощение. Не наследуется.
    pub border_top_left_radius: f32,
    pub border_top_right_radius: f32,
    pub border_bottom_right_radius: f32,
    pub border_bottom_left_radius: f32,
    /// CSS Display L3 §4 — visibility. Inherited.
    pub visibility: Visibility,
    /// CSS UI L4 §8.1 — cursor. Inherited.
    pub cursor: Cursor,
    /// CSS Backgrounds L3 §4.6 — список теней. Не наследуется. Пустой Vec
    /// = `none`.
    pub box_shadow: Vec<BoxShadow>,
    /// CSS Text Decoration L3 §4 — список теней текста. Inherited
    /// (отличается от box-shadow!). Пустой Vec = `none`.
    pub text_shadow: Vec<TextShadow>,
    /// CSS Overflow L3 — отдельные поля для X и Y. Не наследуются.
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// CSS UI L4 §10.1 — text-overflow. Не наследуется.
    pub text_overflow: TextOverflow,
    /// CSS Color L3 §3.2 — opacity (0.0..=1.0). Не наследуется. Работает
    /// как alpha всего слоя (включая фон, бордер, текст и потомков). В
    /// Phase 0 layout только хранит — paint пока не применяет alpha
    /// blending этого уровня; индивидуальные альфы в `color`/`background`
    /// продолжают работать.
    pub opacity: f32,
    /// CSS Basic UI L4 §5: outline. В отличие от border не сдвигает соседей
    /// и не учитывается в width/height (рисуется поверх / снаружи коробки).
    /// Не наследуется.
    ///
    /// Initial computed `outline-width` = `medium` (3 px по UA convention);
    /// **used** value становится 0 при `outline-style: none` (CSS 2.1
    /// §17.6.1 / Basic UI L4 §5.2) — см. `outline_used_width()`. Поэтому
    /// «outline по умолчанию невидим» обеспечивается style=None, а не
    /// width=0.
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    pub outline_color: OutlineColor,
    /// CSS Basic UI L4 §5.5 — outline-offset (resolved px). Положительное —
    /// outline отрисовывается дальше от бокса, отрицательное — внутрь.
    pub outline_offset: f32,
    /// CSS UI L4 §6.1 — accent-color. Цвет встроенных form widgets
    /// (checkbox, radio, range, progress). `None` = `auto` (UA default).
    /// Inherited. В Phase 0 layout только хранит — real применение появится
    /// вместе с form-widget рендерингом.
    pub accent_color: Option<Color>,
    /// CSS Variables L1 — custom properties (`--name`). Все custom properties
    /// inherited (спека: `all custom properties are inherited by default`).
    /// Ключ — полное имя с ведущими `--`, значение — сырой текст из source.
    /// Substitution `var(--name [, fallback])` делается lazy при применении
    /// обычных деклараций (см. `apply_declaration`).
    pub custom_props: HashMap<String, String>,
    /// CSS Lists L3 §3 — `counter-reset: name [N]?`. Каждый element задаёт
    /// (имя-счётчика, начальное-значение). Не наследуется. Пустой `Vec`
    /// при отсутствии декларации или `counter-reset: none`. Реальное
    /// разрешение counter() в `content` pseudo-elements — отдельная задача
    /// (требует layout-time counter scoping walker).
    pub counter_reset: Vec<(String, i32)>,
    /// CSS Lists L3 §3 — `counter-increment: name [N]?`. Каждый element
    /// инкрементирует названный counter на N (default +1). Не наследуется.
    pub counter_increment: Vec<(String, i32)>,
    /// CSS Masking L1 §3 — `clip-path: <basic-shape> | none`. Не
    /// наследуется. Phase 0: parsing only — real geometric clipping
    /// в paint pipeline отложен.
    pub clip_path: Option<ClipPath>,
    /// CSS Transforms L1 §2 — `transform: <transform-list> | none`.
    /// Список функций — каждая `TransformFn` хранит параметры. Не
    /// наследуется. Phase 0: parsing only — apply matrix к paint —
    /// отложено.
    pub transform: Vec<TransformFn>,
    /// CSS Filter Effects L1 §3 — `filter: <filter-function-list> | none`.
    /// Список функций — blur/brightness/contrast/grayscale/etc. Не
    /// наследуется. Phase 0: parsing only.
    pub filter: Vec<FilterFn>,
    /// CSS Box Alignment L3 §8 — `row-gap` / `column-gap` для
    /// flex/grid container-ов. В пикселях (resolved). Default 0.
    /// Не наследуется. Phase 0: parsing only — real flex/grid algorithm
    /// не реализован, гарантированно gap не применяется.
    pub row_gap: f32,
    pub column_gap: f32,
    /// CSS Multi-column L1 §3.2 — `column-count: <integer> | auto`. `None`
    /// = `auto` (UA выбирает на основе column-width). Не наследуется.
    /// Phase 0: parsing only — реальный column layout pipeline отложен.
    pub column_count: Option<u32>,
    /// CSS Multi-column L1 §3.3 — `column-width: <length> | auto`. В px
    /// (resolved). `None` = `auto`. Не наследуется.
    pub column_width: Option<f32>,
    /// CSS Multi-column L1 §4.1 — `column-rule-width` (px). Default 0.
    pub column_rule_width: f32,
    /// CSS Multi-column L1 §4.2 — `column-rule-style`. Default `None`
    /// (без линии — линия рисуется только если style != None и width > 0).
    pub column_rule_style: BorderStyle,
    /// CSS Multi-column L1 §4.3 — `column-rule-color`. `None` = currentColor.
    pub column_rule_color: Option<Color>,
    /// CSS Multi-column L1 §6.1 — `column-span: none | all`. По умолчанию
    /// `None` (False), `Some(true)` = `all` (элемент растягивается через
    /// все колонки). Не наследуется. Phase 0: parse+store.
    pub column_span_all: bool,
    /// CSS Multi-column L1 §6.2 — `column-fill: auto | balance`. `false`
    /// = auto (default — заполняет последовательно), `true` = balance.
    /// Не наследуется.
    pub column_fill_balance: bool,
    /// CSS Fragmentation L3 §3.1 — `break-before`. Phase 0 — enum со
    /// значениями auto/avoid/always/page/column/region. Не наследуется.
    pub break_before: BreakValue,
    pub break_after: BreakValue,
    pub break_inside: BreakValue,
    /// CSS Sizing L4 §6.1 — `aspect-ratio: auto | <ratio> | auto <ratio>`.
    /// `None` = `auto` (UA выбирает). `Some((w, h))` = явное отношение
    /// W:H (например, 16:9 → (16.0, 9.0)). Не наследуется.
    /// Phase 0: parsing — real intrinsic-aspect-ratio enforcement
    /// требует layout-time pass.
    pub aspect_ratio: Option<(f32, f32)>,
    /// CSS Box Alignment L3 — alignment свойства для flex/grid items.
    /// Все не наследуются. Phase 0: parsing only.
    pub align_items: AlignValue,
    pub align_self: AlignValue,
    pub align_content: AlignValue,
    pub justify_items: AlignValue,
    pub justify_self: AlignValue,
    pub justify_content: AlignValue,
    /// CSS Backgrounds L3 — `background-image`.
    pub background_image: BackgroundImage,
    pub background_repeat: BackgroundRepeat,
    pub background_size: BackgroundSize,
    pub background_attachment: BackgroundAttachment,
    /// CSS Backgrounds L3 §3.7 — `background-origin`. Не наследуется. Default
    /// `PaddingBox`. Phase 0: parsing + storage; реальный выбор box-edge для
    /// позиционной системы background-image — отдельная задача с согласованием
    /// P2 (crate-ownership matrix).
    pub background_origin: BackgroundOrigin,
    /// CSS Backgrounds L3 §3.8 — `background-clip`. Не наследуется. Default
    /// `BorderBox`. Variant `Text` (CSS Backgrounds L4) хранится как atom;
    /// реальная отсечка по форме глифов требует mask-pipeline в `lumen-paint`.
    pub background_clip: BackgroundClip,
    /// CSS Backgrounds L3 §3.5 — `background-position`. Не наследуется.
    /// Default `0% 0%` (top-left). Phase 0: parsing + storage; реальное
    /// применение в paint pipeline (смещение background-image в pattern fill)
    /// — отдельная задача с согласованием P2 (см. crate-ownership matrix).
    /// Тип переиспользуется с `object-position`: те же 1-2-value формы,
    /// keyword / length / percentage. Multi-background-position (список
    /// через запятую) — отдельная задача после multi-background-image.
    pub background_position: ObjectPosition,
    /// CSS Will Change L1. Список имён свойств для optimization hint.
    /// Пустой Vec = `auto` (default). Не наследуется.
    pub will_change: Vec<String>,
    /// CSS Pointer Events L1. Default `auto`. Не наследуется.
    pub pointer_events: PointerEvents,
    /// CSS UI L4 §6.2 — `user-select`. Inherited (по спеке).
    pub user_select: UserSelect,
    /// CSS Overflow L3 — `scroll-behavior`. Inherited.
    pub scroll_behavior: ScrollBehavior,
    /// CSS Scroll Snap L1 §3.1 — `scroll-snap-type`. Не наследуется.
    pub scroll_snap_type: ScrollSnapType,
    /// CSS Scroll Snap L1 §6.1 — `scroll-snap-align`. Не наследуется.
    pub scroll_snap_align: ScrollSnapAlign,
    /// CSS Scroll Snap L1 §6.2 — `scroll-snap-stop`. Не наследуется.
    pub scroll_snap_stop: ScrollSnapStop,
    /// CSS Scroll Snap L1 §4 — `scroll-margin-*` (resolved px).
    pub scroll_margin_top: f32,
    pub scroll_margin_right: f32,
    pub scroll_margin_bottom: f32,
    pub scroll_margin_left: f32,
    /// CSS Scroll Snap L1 §4 — `scroll-padding-*` (resolved px).
    pub scroll_padding_top: f32,
    pub scroll_padding_right: f32,
    pub scroll_padding_bottom: f32,
    pub scroll_padding_left: f32,
    /// CSS Overscroll Behavior L1 §2 — `overscroll-behavior-x`. Не наследуется.
    pub overscroll_behavior_x: OverscrollBehavior,
    pub overscroll_behavior_y: OverscrollBehavior,
    /// CSS Text L3 §10.1 — `tab-size: <integer> | <length>`. Inherited.
    /// В пикселях если length; для integer хранится как число × 8 (default
    /// 8 spaces — стандартный default). Default 8 spaces = 64px при 8px-space.
    pub tab_size: f32,
    /// CSS UI L4 §6.3 — `caret-color: auto | <color>`. Inherited.
    /// `None` = auto (UA выбирает). `Some(color)` — явный цвет.
    pub caret_color: Option<Color>,
    /// CSS Text L3 §5.2 — `overflow-wrap: normal | break-word | anywhere`.
    /// Inherited. Default `Normal`.
    pub overflow_wrap: OverflowWrap,
    /// CSS Text L3 §5.1 — `word-break: normal | keep-all | break-all |
    /// break-word`. Inherited. Default `Normal`.
    pub word_break: WordBreak,
    /// CSS Text L3 §6 — `hyphens: none | manual | auto`. Inherited.
    /// Default `Manual`.
    pub hyphens: Hyphens,
    /// CSS Transforms L1 §6 — `transform-origin: <x> <y> <z>?` в px.
    /// Default `(50%, 50%, 0)` — центр коробки. Phase 0 хранит как
    /// пиксельные координаты после resolve (или None для процентных —
    /// нужен размер box-а; пока разрешаем только px/em/rem).
    pub transform_origin: (f32, f32, f32),
    /// CSS Transforms L2 §4 — `perspective: <length> | none`.
    /// `None` = no perspective; `Some(px)` = distance to camera.
    pub perspective: Option<f32>,
    /// CSS Lists L3 §2.1 — `list-style-type`.
    pub list_style_type: ListStyleType,
    /// CSS Lists L3 §2.3 — `list-style-position`.
    pub list_style_position: ListStylePosition,
    /// CSS Lists L3 §2.2 — `list-style-image: url(...) | none`.
    pub list_style_image: Option<String>,
    /// CSS Transitions L1 §3 — `transition-property: none | all | <ident>+`.
    pub transition_properties: Vec<String>,
    /// CSS Transitions L1 §3 — `transition-duration: <time>+` в секундах.
    pub transition_durations: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-delay: <time>+` в секундах.
    pub transition_delays: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-timing-function: <easing-function>+`.
    /// Per-property list; если длина короче `transition_properties`, при
    /// resolve-time spec велит cyclically reuse последний элемент.
    pub transition_timing_functions: Vec<TimingFunction>,
    /// CSS Animations L1 §3.1 — `animation-name: none | <keyframes-name>#`.
    /// `none` хранится как пустой `Vec` (нет анимаций); иначе список имён.
    /// Имя соответствует `@keyframes name { ... }` в [`Stylesheet`].
    pub animation_names: Vec<String>,
    /// CSS Animations L1 §3.2 — `animation-duration: <time>#`. Секунды.
    /// Параллельный список к `animation_names`; cyclically reuse при
    /// несовпадении длины (resolve в P1 п.3A scheduler).
    pub animation_durations: Vec<f32>,
    /// CSS Animations L1 §3.3 — `animation-timing-function: <easing-function>#`.
    pub animation_timing_functions: Vec<TimingFunction>,
    /// CSS Animations L1 §3.4 — `animation-delay: <time>#`. Секунды.
    /// Отрицательные значения допустимы и означают «анимация началась
    /// в прошлом» (используется для phase-offset нескольких анимаций).
    pub animation_delays: Vec<f32>,
    /// CSS Animations L1 §3.5 — `animation-iteration-count: <single-iteration-count>#`.
    pub animation_iteration_counts: Vec<IterationCount>,
    /// CSS Animations L1 §3.6 — `animation-direction: <single-animation-direction>#`.
    pub animation_directions: Vec<AnimationDirection>,
    /// CSS Animations L1 §3.7 — `animation-fill-mode: <single-animation-fill-mode>#`.
    pub animation_fill_modes: Vec<AnimationFillMode>,
    /// CSS Animations L1 §3.8 — `animation-play-state: <single-animation-play-state>#`.
    pub animation_play_states: Vec<AnimationPlayState>,
    /// CSS Masking L1 §4 — `mask-image: url(...) | linear-gradient(...) | none`.
    /// `BackgroundImage` переиспользуется как тип (same structure: None/Url/Gradient).
    pub mask_image: BackgroundImage,
    /// CSS Masking L1 §4 — `mask-repeat`. Те же значения, что у background-repeat.
    pub mask_repeat: BackgroundRepeat,
    /// CSS Masking L1 §4 — `mask-size`.
    pub mask_size: BackgroundSize,
    /// CSS Scrollbars 1 — `scrollbar-width: auto | thin | none`.
    pub scrollbar_width: ScrollbarWidth,
    /// CSS Scrollbars 1 — `scrollbar-color: auto | <color> <color>`
    /// (thumb-color + track-color).
    pub scrollbar_color: Option<(Color, Color)>,
    /// CSS Overflow L3 — `scrollbar-gutter: auto | stable | stable both-edges`.
    pub scrollbar_gutter: ScrollbarGutter,
    /// CSS Content L3 §2.1 — `content`. Используется в pseudo-elements
    /// (`::before` / `::after`) и для counter()-разрешения. Phase 0:
    /// parsing + storage; реальные pseudo-elements в layout — отдельная
    /// большая задача. Default `Normal` (use element's box-tree).
    pub content: Content,
    /// CSS Images L3 §5.5 — `object-fit`. Применяется только к replaced
    /// elements (`<img>` и пр.). Не наследуется. Default `Fill`.
    pub object_fit: ObjectFit,
    /// CSS Images L3 §5.5 — `object-position`. Не наследуется. Default
    /// `50% 50%` (центр коробки).
    pub object_position: ObjectPosition,
    /// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
    /// Default `Baseline`. Phase 0: parsing + storage; реальное применение
    /// (y_offset фрагмента в inline-flow и DrawText в paint) — отдельная
    /// задача с согласованием P2 (см. doc-comment на [`VerticalAlign`]).
    pub vertical_align: VerticalAlign,
    /// CSS Images L3 §6.1 — `image-rendering`. Inherited. Default `Auto`.
    /// Phase 0: parsing + storage; реальное переключение GPU sampler filter
    /// в `lumen-paint` (linear vs nearest-neighbour для `<img>` и background)
    /// — отдельная задача с согласованием P2.
    pub image_rendering: ImageRendering,
    /// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
    /// Default `Wrap`. Phase 0: parsing + storage; реальная связка с
    /// inline-flow line-breaker-ом (когда `Nowrap` подавляет soft wraps
    /// и эмитит overflowing line, эквивалентно legacy `white-space: nowrap`)
    /// — отдельная задача рядом с типизацией white-space (P1 1B).
    pub text_wrap_mode: TextWrapMode,
    /// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
    /// Default `Auto`. Phase 0: parsing + storage; реальная интерпретация
    /// `balance` / `pretty` / `stable` требует Knuth–Plass-style breaker-а
    /// и Unicode line-break tables — отложено до интеграции `UnicodeProvider`
    /// (provisional `icu4x`, P1 п.5).
    pub text_wrap_style: TextWrapStyle,
}

/// CSS Content L3 — value свойства `content`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Content {
    /// `normal` (default) — поведение по умолчанию для каждого element.
    #[default]
    Normal,
    /// `none` — pseudo-element не генерируется.
    None,
    /// Список фрагментов: строки, counter()/counters(), attr(), url().
    /// Phase 0 хранит список typed-фрагментов; конкатенация для render —
    /// задача paint pipeline.
    Items(Vec<ContentItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentItem {
    /// Литеральная строка из CSS-string-literal (без кавычек).
    String(String),
    /// `attr(name)` — значение HTML-атрибута текущего element.
    Attr(String),
    /// `url("path")` — изображение / external resource.
    Url(String),
    /// `counter(name [, style])` — значение counter-а. `style` — пока
    /// сырая строка (Phase 0 разрешит только `decimal` etc.).
    Counter {
        name: String,
        style: Option<String>,
    },
    /// `counters(name, separator [, style])` — вложенные counters
    /// (`1.2.3` через `.`).
    Counters {
        name: String,
        separator: String,
        style: Option<String>,
    },
    /// `open-quote` / `close-quote` — quotation marks per `quotes` property.
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
}

/// CSS Scrollbars 1 — `scrollbar-width`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    /// `thin` — тонкий scrollbar.
    Thin,
    /// `none` — без visible scrollbar (контент всё ещё скроллится через
    /// keyboard / touch / programmatic).
    None,
}

impl ScrollbarWidth {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "thin" => Some(Self::Thin),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scrollbar-gutter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarGutter {
    /// `auto` (default) — gutter появляется когда overflow:scroll.
    #[default]
    Auto,
    /// `stable` — gutter всегда зарезервирован (не двигает контент при scroll).
    Stable,
    /// `stable both-edges` — gutter на обоих краях для симметрии.
    StableBothEdges,
}

impl ScrollbarGutter {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "auto" {
            return Some(Self::Auto);
        }
        if lc == "stable" {
            return Some(Self::Stable);
        }
        // `stable both-edges` — двухтокеновая форма.
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        if tokens == ["stable", "both-edges"] {
            return Some(Self::StableBothEdges);
        }
        None
    }
}

/// CSS Lists L3 §2.1 — markers для list items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListStyleType {
    /// `none` — без marker.
    None,
    /// `disc` — закрашенный кружок (default для ul).
    #[default]
    Disc,
    /// `circle` — пустой кружок.
    Circle,
    /// `square` — квадратик.
    Square,
    /// `decimal` — 1, 2, 3, ... (default для ol).
    Decimal,
    /// `decimal-leading-zero` — 01, 02, ..., 09, 10, ...
    DecimalLeadingZero,
    /// `lower-roman` — i, ii, iii, ...
    LowerRoman,
    /// `upper-roman` — I, II, III, ...
    UpperRoman,
    /// `lower-alpha` / `lower-latin` — a, b, c, ...
    LowerAlpha,
    /// `upper-alpha` / `upper-latin` — A, B, C, ...
    UpperAlpha,
    /// `lower-greek` — α, β, γ, ...
    LowerGreek,
}

impl ListStyleType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "disc" => Some(Self::Disc),
            "circle" => Some(Self::Circle),
            "square" => Some(Self::Square),
            "decimal" => Some(Self::Decimal),
            "decimal-leading-zero" => Some(Self::DecimalLeadingZero),
            "lower-roman" => Some(Self::LowerRoman),
            "upper-roman" => Some(Self::UpperRoman),
            "lower-alpha" | "lower-latin" => Some(Self::LowerAlpha),
            "upper-alpha" | "upper-latin" => Some(Self::UpperAlpha),
            "lower-greek" => Some(Self::LowerGreek),
            _ => None,
        }
    }
}

/// CSS Lists L3 §2.3 — `list-style-position`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListStylePosition {
    /// `outside` (default) — marker вне content-area.
    #[default]
    Outside,
    /// `inside` — marker внутри content-area.
    Inside,
}

impl ListStylePosition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "outside" => Some(Self::Outside),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `overflow-wrap`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    /// `break-word` — разрешает перенос любого слова, чтобы не было overflow.
    BreakWord,
    /// `anywhere` — как `break-word`, но также влияет на intrinsic-width
    /// computation (CSS Text L3).
    Anywhere,
}

impl OverflowWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "break-word" => Some(Self::BreakWord),
            "anywhere" => Some(Self::Anywhere),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.1 — `word-break`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    /// `keep-all` — CJK не разбивается.
    KeepAll,
    /// `break-all` — разрыв в любом месте, кроме whitespace.
    BreakAll,
    /// `break-word` — legacy для `overflow-wrap: break-word`.
    BreakWord,
}

impl WordBreak {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "keep-all" => Some(Self::KeepAll),
            "break-all" => Some(Self::BreakAll),
            "break-word" => Some(Self::BreakWord),
            _ => None,
        }
    }
}

/// CSS Text L3 §6 — `hyphens`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hyphens {
    /// `none` — переносы запрещены.
    None,
    /// `manual` (default) — переносы только при явных hyphenation-точках
    /// (`&shy;` / U+00AD).
    #[default]
    Manual,
    /// `auto` — UA расставляет переносы по алгоритму (требует hyphenation
    /// dictionary).
    Auto,
}

impl Hyphens {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// CSS Pointer Events L1. Default `auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
    Visible,
    /// `painted` / `fill` / `stroke` / `all` — для SVG. В non-SVG
    /// контексте трактуются как `auto`.
    Painted,
    Fill,
    Stroke,
    All,
}

impl PointerEvents {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "visible" | "visiblepainted" | "visiblefill" | "visiblestroke" => {
                Some(Self::Visible)
            }
            "painted" => Some(Self::Painted),
            "fill" => Some(Self::Fill),
            "stroke" => Some(Self::Stroke),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS UI L4 §6.2 — `user-select`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserSelect {
    #[default]
    Auto,
    Text,
    None,
    Contain,
    All,
}

impl UserSelect {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "text" => Some(Self::Text),
            "none" => Some(Self::None),
            "contain" => Some(Self::Contain),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scroll-behavior`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Smooth,
}

/// CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapType {
    pub axis: ScrollSnapAxis,
    pub strictness: ScrollSnapStrictness,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAxis {
    #[default]
    None,
    X,
    Y,
    Block,
    Inline,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStrictness {
    #[default]
    Proximity,
    Mandatory,
}

/// CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapAlign {
    pub block: ScrollSnapAlignKeyword,
    pub inline: ScrollSnapAlignKeyword,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAlignKeyword {
    #[default]
    None,
    Start,
    End,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverscrollBehavior {
    #[default]
    Auto,
    Contain,
    None,
}

impl ScrollBehavior {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.1 — `background-image` value.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BackgroundImage {
    #[default]
    None,
    /// `url("path")` — внешний image-ресурс. Хранится без resolve
    /// относительно base-href.
    Url(String),
    /// `linear-gradient(...)`, `radial-gradient(...)`, `conic-gradient(...)`
    /// и их `repeating-` варианты. Phase 0 хранится сырая строка.
    Gradient(String),
}

/// CSS Backgrounds L3 §3.4 — `background-repeat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundRepeat {
    #[default]
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Round,
    Space,
}

impl BackgroundRepeat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "repeat" => Some(Self::Repeat),
            "no-repeat" => Some(Self::NoRepeat),
            "repeat-x" => Some(Self::RepeatX),
            "repeat-y" => Some(Self::RepeatY),
            "round" => Some(Self::Round),
            "space" => Some(Self::Space),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.5 — `background-size`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BackgroundSize {
    #[default]
    Auto,
    Cover,
    Contain,
    /// Width / Height в px. None для height = auto.
    Length(f32, Option<f32>),
}

/// CSS Backgrounds L3 §3.6 — `background-attachment`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundAttachment {
    #[default]
    Scroll,
    Fixed,
    Local,
}

impl BackgroundAttachment {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "scroll" => Some(Self::Scroll),
            "fixed" => Some(Self::Fixed),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited.
///
/// Определяет, к какому **краю box-а** привязана позиционная система
/// для `background-image` (initial = padding edge). На `background-color`
/// не влияет (тот всегда заливает border-edge независимо от origin).
///
/// **Phase 0 ограничение:** parsing + storage only. Реальное смещение
/// origin-у в paint pipeline (выбор `border_box` / `padding_box` /
/// `content_box` rect при расчёте начала tile-тиления) — отдельная
/// задача с согласованием P2 (crate-ownership matrix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundOrigin {
    /// `border-box` — позиционная система начинается с border-edge.
    BorderBox,
    /// `padding-box` (initial) — с padding-edge (= внутренний край border-а).
    #[default]
    PaddingBox,
    /// `content-box` — с content-edge (= внутренний край padding-а).
    ContentBox,
}

impl BackgroundOrigin {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited.
///
/// Определяет, к какому **краю box-а** обрезается `background-color`
/// и `background-image` (initial = border edge, т.е. фон видно даже
/// сквозь полупрозрачную рамку).
///
/// Variant `Text` (CSS Backgrounds L4) клипает фон по форме глифов —
/// классический паттерн «gradient text» через `background-clip: text`
/// и `color: transparent`. Реализация в paint требует подмаски через
/// glyph-cache mask-image — отдельная задача с согласованием P2.
///
/// **Phase 0 ограничение:** parsing + storage only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundClip {
    /// `border-box` (initial) — фон под border-ом виден.
    #[default]
    BorderBox,
    /// `padding-box` — фон обрезается до внутреннего края border-а.
    PaddingBox,
    /// `content-box` — фон только в content-area.
    ContentBox,
    /// `text` (CSS Backgrounds L4) — фон клипается по форме текста
    /// внутри box-а. Phase 0 хранит как atom, реальный clip — P2.
    Text,
}

impl BackgroundClip {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
/// (`<img>`, `<video>`, `<canvas>` и т.д.) и определяет, как «коробка»
/// заливается содержимым с учётом intrinsic-размеров. Не наследуется.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectFit {
    /// `fill` (default) — растянуть на размер коробки без сохранения
    /// aspect ratio. Картинка может быть искажена.
    #[default]
    Fill,
    /// `contain` — максимально большой размер с сохранением aspect ratio,
    /// при котором изображение **умещается** целиком (letterbox / pillarbox).
    Contain,
    /// `cover` — минимально большой размер с сохранением aspect ratio,
    /// при котором изображение **покрывает** коробку. Излишки клипятся
    /// по `object-position`.
    Cover,
    /// `none` — без масштабирования (intrinsic-размер 1:1). Излишки
    /// клипятся; недостаток заполняется по `object-position`.
    None,
    /// `scale-down` — `min(none, contain)`: если intrinsic-размер меньше
    /// коробки, ведёт себя как `none`; иначе как `contain`.
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "contain" => Some(Self::Contain),
            "cover" => Some(Self::Cover),
            "none" => Some(Self::None),
            "scale-down" => Some(Self::ScaleDown),
            _ => None,
        }
    }
}

/// CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
/// масштабировать растровое изображение (применимо к `<img>`, background-image,
/// canvas, и т.д.). Inherited.
///
/// Phase 0: parsing + storage. Реальное переключение GPU sampler filter
/// (`Linear` для `auto`/`smooth`/`high-quality`, `Nearest` для `pixelated`/
/// `crisp-edges`) в `lumen-paint` — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageRendering {
    /// `auto` (default) — UA выбирает алгоритм. Обычно — bilinear.
    #[default]
    Auto,
    /// `smooth` — high-quality scaling, оптимизирован для smooth gradient.
    /// На практике в современных движках = `auto`.
    Smooth,
    /// `high-quality` — высочайшее качество масштабирования (тяжелее `smooth`).
    /// Спецификация добавлена в CSS Images L4; считается переименованием
    /// `optimizeQuality` из L3 (которое теперь deprecated).
    HighQuality,
    /// `crisp-edges` — сохраняет контраст и резкость границ (pixel art /
    /// vector graphics). UA может использовать nearest-neighbour или
    /// edge-preserving алгоритм.
    CrispEdges,
    /// `pixelated` — nearest-neighbour. Полезно для масштабирования pixel art.
    Pixelated,
}

impl ImageRendering {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            "high-quality" => Some(Self::HighQuality),
            "crisp-edges" => Some(Self::CrispEdges),
            "pixelated" => Some(Self::Pixelated),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
///
/// Управляет тем, переносятся ли строки внутри блока. `wrap` — нормальный
/// перенос по soft wrap opportunities (initial). `nowrap` — текст растягивается
/// в одну линию, до явного break-control (`<br>`, preserved newline).
///
/// Является non-shorthand-частью `text-wrap` (§6.4.3) и одновременно
/// частью legacy `white-space` shorthand (§2.1 — `white-space-collapse` ||
/// `text-wrap-mode` || `white-space-trim`). В этой кодовой базе `white-space`
/// исторически хранится отдельным [`WhiteSpace`] enum-ом — связка двух полей
/// уйдёт в типизацию декрараций (P1 1B).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    /// `wrap` (initial) — обычный перенос строк.
    #[default]
    Wrap,
    /// `nowrap` — без переноса, текст в одну линию.
    Nowrap,
}

impl TextWrapMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrap" => Some(Self::Wrap),
            "nowrap" => Some(Self::Nowrap),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
///
/// Расширенные стратегии перевода строк. `auto` — UA выбирает по умолчанию
/// (обычно greedy first-fit). Остальные значения — типографические
/// улучшения, требующие реального line-breaker-а (Knuth–Plass / Latin
/// last-line orphan-prevention) — Phase 0 хранит как atom, применение
/// откладывается до интеграции с `UnicodeProvider` (provisional `icu4x`,
/// P1 п.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    /// `auto` (initial) — UA-default стратегия (обычно greedy).
    #[default]
    Auto,
    /// `balance` — балансировать длины строк короткого блока (≤ ~10 строк).
    Balance,
    /// `stable` — стабильные break-points при редактировании (для contenteditable).
    Stable,
    /// `pretty` — улучшенный last-line (без orphan / висячих слов).
    Pretty,
}

impl TextWrapStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "balance" => Some(Self::Balance),
            "stable" => Some(Self::Stable),
            "pretty" => Some(Self::Pretty),
            _ => None,
        }
    }
}

/// Одна компонента `object-position`. Length-варианты резолвятся в px
/// относительно края коробки (positive = от left/top); percentage —
/// относительно **свободного места** `box_size - content_size` (может быть
/// отрицательным, тогда излишек уходит за противоположный край). См.
/// CSS Images L3 §5.5 «object-position».
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionComponent {
    /// Length в px (после resolve em/rem/vw/...).
    Px(f32),
    /// Percentage в долях 1.0 (`50%` → 0.5). Резолвится на paint-стадии
    /// против свободного места: `offset = free_space * percent`.
    Percent(f32),
}

impl PositionComponent {
    /// Резолв в финальный px-offset относительно левого/верхнего края
    /// коробки. `free_space = box_size - content_size`; может быть
    /// отрицательным (content > box) — тогда offset тоже отрицательный,
    /// и излишек уезжает за противоположный край.
    pub fn resolve(self, free_space: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Percent(p) => free_space * p,
        }
    }
}

/// CSS Images L3 §5.5 — `object-position` (две компоненты, x + y).
/// Default — `50% 50%` (центр).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPosition {
    pub x: PositionComponent,
    pub y: PositionComponent,
}

impl Default for ObjectPosition {
    fn default() -> Self {
        Self {
            x: PositionComponent::Percent(0.5),
            y: PositionComponent::Percent(0.5),
        }
    }
}

impl ObjectPosition {
    /// CSS Backgrounds L3 §3.5 — initial value `background-position: 0% 0%`
    /// (top-left). Отличается от Object Position default (`50% 50%`, центр)
    /// специально потому, что `background-image` обычно anchored к top-left
    /// при первой укладке (см. CSS 2.1 §14.2.1).
    pub const fn background_initial() -> Self {
        Self {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        }
    }
}

impl ObjectPosition {
    /// CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
    /// поддерживает:
    ///   - keyword `center` (= 50%),
    ///   - axis-keywords `left|right|top|bottom`,
    ///   - один token (`50%`, `10px`, keyword) — второй = `center`,
    ///   - два token-а — первый x, второй y.
    ///
    /// Tri- и quad-форма (`<keyword> <length> <keyword> <length>` для
    /// сторон-якорей) — отложены: на современных страницах редкость.
    pub fn parse(s: &str, em_basis: f32, viewport: Size) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 2 {
            return None;
        }
        // Single-token: применяется к horizontal оси; вертикальная = center.
        // Если token — vertical keyword (`top`/`bottom`), то horizontal = center.
        if tokens.len() == 1 {
            let t = tokens[0];
            if t.eq_ignore_ascii_case("top") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(0.0),
                });
            }
            if t.eq_ignore_ascii_case("bottom") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(1.0),
                });
            }
            let x = parse_position_component(t, em_basis, viewport, /*vertical*/ false)?;
            return Some(Self {
                x,
                y: PositionComponent::Percent(0.5),
            });
        }
        // Two-token form: <x> <y>. Swap, если порядок инвертирован
        // (`top left` ≡ `left top`).
        let (t0, t1) = (tokens[0], tokens[1]);
        let (xtok, ytok) = if is_vertical_keyword(t0) || is_horizontal_keyword(t1) {
            (t1, t0)
        } else {
            (t0, t1)
        };
        let x = parse_position_component(xtok, em_basis, viewport, false)?;
        let y = parse_position_component(ytok, em_basis, viewport, true)?;
        Some(Self { x, y })
    }
}

fn is_vertical_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("top") || t.eq_ignore_ascii_case("bottom")
}

fn is_horizontal_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("left") || t.eq_ignore_ascii_case("right")
}

fn parse_position_component(
    t: &str,
    em_basis: f32,
    viewport: Size,
    vertical: bool,
) -> Option<PositionComponent> {
    // Keyword-формы.
    if t.eq_ignore_ascii_case("center") {
        return Some(PositionComponent::Percent(0.5));
    }
    if !vertical {
        if t.eq_ignore_ascii_case("left") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("right") {
            return Some(PositionComponent::Percent(1.0));
        }
        // top/bottom в horizontal-позиции — недопустимо.
        if is_vertical_keyword(t) {
            return None;
        }
    } else {
        if t.eq_ignore_ascii_case("top") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("bottom") {
            return Some(PositionComponent::Percent(1.0));
        }
        if is_horizontal_keyword(t) {
            return None;
        }
    }
    // Length / percentage. Percent-форма `50%` сохраняется как доля 0..=1
    // (без clamp — отрицательные и >100% валидны по спеке и используются
    // художниками для художественных смещений).
    if let Some(pct) = t.strip_suffix('%')
        && let Ok(n) = pct.trim().parse::<f32>()
    {
        return Some(PositionComponent::Percent(n / 100.0));
    }
    let len = parse_length(t)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(PositionComponent::Px(px))
}

/// CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств.
/// Phase 0: основной набор keyword-ов. `Auto` — default (resolve в
/// `Normal` или specific behavior контекстом).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlignValue {
    /// CSS keyword `auto` — default. Behavior зависит от контекста
    /// (parent layout type). Для absolute-positioned — `normal`.
    #[default]
    Auto,
    /// `normal` — default-behavior для conteneur'а (stretch for grid,
    /// start for flex).
    Normal,
    /// `stretch` — растянуть на доступное место (default для grid).
    Stretch,
    /// `start` / `flex-start` — выровнять к началу cross/main axis.
    Start,
    /// `end` / `flex-end` — выровнять к концу.
    End,
    /// `center` — выровнять по центру.
    Center,
    /// `baseline` — выровнять text-baseline (для align-items).
    Baseline,
    /// `space-between` — равные промежутки между items, по краям нет.
    SpaceBetween,
    /// `space-around` — промежутки между + половинные по краям.
    SpaceAround,
    /// `space-evenly` — все промежутки одинаковые, включая края.
    SpaceEvenly,
}

impl AlignValue {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => Some(Self::Auto),
            "normal" => Some(Self::Normal),
            "stretch" => Some(Self::Stretch),
            "start" | "flex-start" | "self-start" => Some(Self::Start),
            "end" | "flex-end" | "self-end" => Some(Self::End),
            "center" => Some(Self::Center),
            "baseline" | "first baseline" | "last baseline" => Some(Self::Baseline),
            "space-between" => Some(Self::SpaceBetween),
            "space-around" => Some(Self::SpaceAround),
            "space-evenly" => Some(Self::SpaceEvenly),
            _ => None,
        }
    }
}

/// CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
/// поддерживает: `inset(...)`, `circle(...)`, `ellipse(...)`,
/// `polygon(...)`. URL / `path()` / `none` отложены.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    /// `inset(top right bottom left)` — 1..=4 length-значения.
    Inset(Vec<f32>),
    /// `circle(radius at cx cy)` — radius и center (опц.).
    Circle {
        radius: f32,
        center: Option<(f32, f32)>,
    },
    /// `ellipse(rx ry at cx cy)`.
    Ellipse {
        rx: f32,
        ry: f32,
        center: Option<(f32, f32)>,
    },
    /// `polygon(x1 y1, x2 y2, ...)` — список вершин.
    Polygon(Vec<(f32, f32)>),
}

/// CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
/// translate/translateX/translateY, rotate, scale/scaleX/scaleY,
/// skew/skewX/skewY, matrix. 3D-варианты (translate3d, rotate3d,
/// и т.д.) отложены.
#[derive(Debug, Clone, PartialEq)]
pub enum TransformFn {
    Translate(f32, f32),
    TranslateX(f32),
    TranslateY(f32),
    /// Угол в радианах (нормализован парсером из deg/rad/turn/grad).
    Rotate(f32),
    Scale(f32, f32),
    ScaleX(f32),
    ScaleY(f32),
    SkewX(f32),
    SkewY(f32),
    Matrix([f32; 6]),
}

/// CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
/// все 9 стандартных функций кроме `drop-shadow` (требует rendering
/// pass — отложено).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterFn {
    /// `blur(<length>)` — радиус gaussian blur.
    Blur(f32),
    /// `brightness(<number-percentage>)`. 1.0 = unchanged.
    Brightness(f32),
    /// `contrast(<number-percentage>)`. 1.0 = unchanged.
    Contrast(f32),
    /// `grayscale(<number-percentage>)`. 0.0 = unchanged, 1.0 = full grayscale.
    Grayscale(f32),
    /// `hue-rotate(<angle>)` — угол в радианах.
    HueRotate(f32),
    /// `invert(<number-percentage>)`. 0.0 = unchanged, 1.0 = inverted.
    Invert(f32),
    /// `opacity(<number-percentage>)`. 1.0 = unchanged.
    Opacity(f32),
    /// `saturate(<number-percentage>)`. 1.0 = unchanged.
    Saturate(f32),
    /// `sepia(<number-percentage>)`. 0.0 = unchanged, 1.0 = full sepia.
    Sepia(f32),
}

/// CSS Images L3 §3.4 — единичный `<color-stop>` градиента.
///
/// `position == None` означает auto-распределение: при resolve до used-value
/// auto-stops равномерно разносятся между фиксированными соседями (spec §3.4.3
/// "Color stop processing"). Здесь типизация специфицированного значения —
/// auto хранится как `None`, без раскрытия.
///
/// Только цвет и позиция (length / percentage). Hint-stops (`<color-stop>,
/// <length-percentage>, <color-stop>`) — без позиции цвета, чисто
/// midpoint-маркер — пока не моделируем: они отрабатывают на интерполяции
/// между соседями и не имеют animation-смысла на уровне per-stop pair.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    pub position: Option<Length>,
}

impl ComputedStyle {
    /// CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
    /// равно 0, если `outline-style` равен `none` (это spec, не аппроксимация).
    /// Computed `outline_width` хранится как есть (medium = 3 по UA convention),
    /// чтобы `outline-style: solid` без явного width давал видимый outline.
    pub fn outline_used_width(&self) -> f32 {
        if matches!(self.outline_style, OutlineStyle::None) {
            0.0
        } else {
            self.outline_width
        }
    }

    /// Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
    /// насыщенность, letter/word-spacing, декорация). Используется для слияния
    /// inline-фрагментов в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
            && self.font_style == other.font_style
            && self.font_weight == other.font_weight
            && self.font_variant == other.font_variant
            && self.font_stretch == other.font_stretch
            && (self.letter_spacing - other.letter_spacing).abs() < f32::EPSILON
            && (self.word_spacing - other.word_spacing).abs() < f32::EPSILON
            && self.text_decoration_line == other.text_decoration_line
            && self.text_decoration_color == other.text_decoration_color
            && self.text_decoration_style == other.text_decoration_style
            && self.text_decoration_thickness == other.text_decoration_thickness
    }

    /// Стартовые значения для корня документа.
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            text_align: TextAlign::Left,
            direction: Direction::Ltr,
            color: Color::BLACK,
            background_color: None,
            font_size: 16.0,
            line_height: 1.2,
            font_style: FontStyle::Normal,
            font_weight: FontWeight::NORMAL,
            font_variant: FontVariant::Normal,
            font_stretch: FontStretch::NORMAL,
            font_family: Vec::new(),
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            text_indent: 0.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_decoration_line: TextDecorationLine::default(),
            text_decoration_color: None,
            text_decoration_style: TextDecorationStyle::Solid,
            text_decoration_thickness: TextDecorationThickness::Auto,
            text_emphasis_style: TextEmphasisStyle::None,
            text_emphasis_color: None,
            text_emphasis_position: TextEmphasisPosition::OverRight,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            border_top_color: None,
            border_right_color: None,
            border_bottom_color: None,
            border_left_color: None,
            box_sizing: BoxSizing::ContentBox,
            position: Position::Static,
            z_index: None,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            border_top_left_radius: 0.0,
            border_top_right_radius: 0.0,
            border_bottom_right_radius: 0.0,
            border_bottom_left_radius: 0.0,
            visibility: Visibility::Visible,
            cursor: Cursor::Auto,
            box_shadow: Vec::new(),
            text_shadow: Vec::new(),
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            text_overflow: TextOverflow::Clip,
            opacity: 1.0,
            outline_width: 3.0,
            outline_style: OutlineStyle::None,
            outline_color: OutlineColor::Auto,
            outline_offset: 0.0,
            accent_color: None,
            custom_props: HashMap::new(),
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            clip_path: None,
            transform: Vec::new(),
            filter: Vec::new(),
            row_gap: 0.0,
            column_gap: 0.0,
            column_count: None,
            column_width: None,
            column_rule_width: 0.0,
            column_rule_style: BorderStyle::None,
            column_rule_color: None,
            column_span_all: false,
            column_fill_balance: false,
            break_before: BreakValue::Auto,
            break_after: BreakValue::Auto,
            break_inside: BreakValue::Auto,
            aspect_ratio: None,
            align_items: AlignValue::Auto,
            align_self: AlignValue::Auto,
            align_content: AlignValue::Auto,
            justify_items: AlignValue::Auto,
            justify_self: AlignValue::Auto,
            justify_content: AlignValue::Auto,
            background_image: BackgroundImage::None,
            background_repeat: BackgroundRepeat::Repeat,
            background_size: BackgroundSize::Auto,
            background_attachment: BackgroundAttachment::Scroll,
            background_origin: BackgroundOrigin::PaddingBox,
            background_clip: BackgroundClip::BorderBox,
            background_position: ObjectPosition::background_initial(),
            will_change: Vec::new(),
            pointer_events: PointerEvents::Auto,
            user_select: UserSelect::Auto,
            scroll_behavior: ScrollBehavior::Auto,
            // CSS Scroll Snap / Overscroll defaults.
            scroll_snap_type: ScrollSnapType::default(),
            scroll_snap_align: ScrollSnapAlign::default(),
            scroll_snap_stop: ScrollSnapStop::default(),
            scroll_margin_top: 0.0,
            scroll_margin_right: 0.0,
            scroll_margin_bottom: 0.0,
            scroll_margin_left: 0.0,
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            overscroll_behavior_x: OverscrollBehavior::Auto,
            overscroll_behavior_y: OverscrollBehavior::Auto,
            // CSS Text typography defaults.
            tab_size: 64.0,  // 8 spaces × 8px-space-width default.
            caret_color: None,  // `auto`.
            overflow_wrap: OverflowWrap::Normal,
            word_break: WordBreak::Normal,
            hyphens: Hyphens::Manual,
            transform_origin: (0.0, 0.0, 0.0),
            perspective: None,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
            transition_properties: Vec::new(),
            transition_durations: Vec::new(),
            transition_delays: Vec::new(),
            transition_timing_functions: Vec::new(),
            animation_names: Vec::new(),
            animation_durations: Vec::new(),
            animation_timing_functions: Vec::new(),
            animation_delays: Vec::new(),
            animation_iteration_counts: Vec::new(),
            animation_directions: Vec::new(),
            animation_fill_modes: Vec::new(),
            animation_play_states: Vec::new(),
            mask_image: BackgroundImage::None,
            mask_repeat: BackgroundRepeat::Repeat,
            mask_size: BackgroundSize::Auto,
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_color: None,
            scrollbar_gutter: ScrollbarGutter::Auto,
            content: Content::Normal,
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            vertical_align: VerticalAlign::Baseline,
            image_rendering: ImageRendering::Auto,
            text_wrap_mode: TextWrapMode::Wrap,
            text_wrap_style: TextWrapStyle::Auto,
        }
    }
}

pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
) -> ComputedStyle {
    let mut style = ComputedStyle {
        display: default_display(doc, node),
        // Наследуемые свойства (CSS inherited properties).
        color: inherited.color,
        text_align: inherited.text_align,
        direction: inherited.direction,
        font_size: inherited.font_size,
        line_height: inherited.line_height,
        font_style: inherited.font_style,
        font_weight: inherited.font_weight,
        font_variant: inherited.font_variant,
        font_stretch: inherited.font_stretch,
        font_family: inherited.font_family.clone(),
        text_transform: inherited.text_transform,
        white_space: inherited.white_space,
        text_indent: inherited.text_indent,
        letter_spacing: inherited.letter_spacing,
        word_spacing: inherited.word_spacing,
        text_decoration_line: inherited.text_decoration_line,
        text_decoration_color: inherited.text_decoration_color,
        text_decoration_style: inherited.text_decoration_style,
        text_decoration_thickness: inherited.text_decoration_thickness,
        text_emphasis_style: inherited.text_emphasis_style.clone(),
        text_emphasis_color: inherited.text_emphasis_color,
        text_emphasis_position: inherited.text_emphasis_position,
        accent_color: inherited.accent_color,
        // CSS Variables L1: все custom properties inherited.
        custom_props: inherited.custom_props.clone(),
        // Ненаследуемые — сброс.
        background_color: None,
        width: None,
        height: None,
        min_width: None,
        max_width: None,
        min_height: None,
        max_height: None,
        margin_top: 0.0,
        margin_right: 0.0,
        margin_bottom: 0.0,
        margin_left: 0.0,
        padding_top: 0.0,
        padding_right: 0.0,
        padding_bottom: 0.0,
        padding_left: 0.0,
        border_top_width: 0.0,
        border_right_width: 0.0,
        border_bottom_width: 0.0,
        border_left_width: 0.0,
        border_top_style: BorderStyle::None,
        border_right_style: BorderStyle::None,
        border_bottom_style: BorderStyle::None,
        border_left_style: BorderStyle::None,
        border_top_color: None,
        border_right_color: None,
        border_bottom_color: None,
        border_left_color: None,
        box_sizing: BoxSizing::ContentBox,
        // CSS Positioned Layout L3 §3 / Compositing L1 — не наследуются.
        position: Position::Static,
        z_index: None,
        isolation: Isolation::Auto,
        mix_blend_mode: MixBlendMode::Normal,
        // border-radius не наследуется.
        border_top_left_radius: 0.0,
        border_top_right_radius: 0.0,
        border_bottom_right_radius: 0.0,
        border_bottom_left_radius: 0.0,
        // Inherited (CSS Display L3 §4).
        visibility: inherited.visibility,
        // Inherited (CSS UI L4 §8.1).
        cursor: inherited.cursor,
        // text-shadow inherited (CSS Text Decoration L3 §4).
        text_shadow: inherited.text_shadow.clone(),
        // Не наследуется.
        box_shadow: Vec::new(),
        overflow_x: Overflow::Visible,
        overflow_y: Overflow::Visible,
        text_overflow: TextOverflow::Clip,
        opacity: 1.0,
        outline_width: 3.0,
        outline_style: OutlineStyle::None,
        outline_color: OutlineColor::Auto,
        outline_offset: 0.0,
        // CSS Lists L3 §3 — не наследуются.
        counter_reset: Vec::new(),
        counter_increment: Vec::new(),
        // CSS Masking / Transforms / Filter — не наследуются.
        clip_path: None,
        transform: Vec::new(),
        filter: Vec::new(),
        // Box Alignment gap / Sizing aspect-ratio — не наследуются.
        row_gap: 0.0,
        column_gap: 0.0,
        // CSS Multi-column — не наследуются.
        column_count: None,
        column_width: None,
        column_rule_width: 0.0,
        column_rule_style: BorderStyle::None,
        column_rule_color: None,
        column_span_all: false,
        column_fill_balance: false,
        break_before: BreakValue::Auto,
        break_after: BreakValue::Auto,
        break_inside: BreakValue::Auto,
        aspect_ratio: None,
        // Box Alignment — все не наследуются, default = Auto.
        align_items: AlignValue::Auto,
        align_self: AlignValue::Auto,
        align_content: AlignValue::Auto,
        justify_items: AlignValue::Auto,
        justify_self: AlignValue::Auto,
        justify_content: AlignValue::Auto,
        // Backgrounds — не наследуются, defaults.
        background_image: BackgroundImage::None,
        background_repeat: BackgroundRepeat::Repeat,
        background_size: BackgroundSize::Auto,
        background_attachment: BackgroundAttachment::Scroll,
        background_origin: BackgroundOrigin::PaddingBox,
        background_clip: BackgroundClip::BorderBox,
        background_position: ObjectPosition::background_initial(),
        // Will Change / Pointer Events — не наследуются.
        will_change: Vec::new(),
        pointer_events: PointerEvents::Auto,
        // User Select / Scroll Behavior — наследуются.
        user_select: inherited.user_select,
        scroll_behavior: inherited.scroll_behavior,
        // Scroll Snap / Overscroll — не наследуются, defaults.
        scroll_snap_type: ScrollSnapType::default(),
        scroll_snap_align: ScrollSnapAlign::default(),
        scroll_snap_stop: ScrollSnapStop::default(),
        scroll_margin_top: 0.0,
        scroll_margin_right: 0.0,
        scroll_margin_bottom: 0.0,
        scroll_margin_left: 0.0,
        scroll_padding_top: 0.0,
        scroll_padding_right: 0.0,
        scroll_padding_bottom: 0.0,
        scroll_padding_left: 0.0,
        overscroll_behavior_x: OverscrollBehavior::Auto,
        overscroll_behavior_y: OverscrollBehavior::Auto,
        // CSS Text typography — все inherited.
        tab_size: inherited.tab_size,
        caret_color: inherited.caret_color,
        overflow_wrap: inherited.overflow_wrap,
        word_break: inherited.word_break,
        hyphens: inherited.hyphens,
        // CSS Transforms transform-origin + perspective — не наследуются.
        transform_origin: (0.0, 0.0, 0.0),
        perspective: None,
        // CSS Lists — list-style-* наследуются.
        list_style_type: inherited.list_style_type,
        list_style_position: inherited.list_style_position,
        list_style_image: inherited.list_style_image.clone(),
        // CSS Transitions / Animations — не наследуются. Initial = empty list.
        transition_properties: Vec::new(),
        transition_durations: Vec::new(),
        transition_delays: Vec::new(),
        transition_timing_functions: Vec::new(),
        animation_names: Vec::new(),
        animation_durations: Vec::new(),
        animation_timing_functions: Vec::new(),
        animation_delays: Vec::new(),
        animation_iteration_counts: Vec::new(),
        animation_directions: Vec::new(),
        animation_fill_modes: Vec::new(),
        animation_play_states: Vec::new(),
        // CSS Masking — не наследуется.
        mask_image: BackgroundImage::None,
        mask_repeat: BackgroundRepeat::Repeat,
        mask_size: BackgroundSize::Auto,
        // CSS Scrollbars — scrollbar-width/-color inherited;
        // scrollbar-gutter не наследуется.
        scrollbar_width: inherited.scrollbar_width,
        scrollbar_color: inherited.scrollbar_color,
        scrollbar_gutter: ScrollbarGutter::Auto,
        content: Content::Normal,
        // CSS Images L3 §5.5 — object-fit / object-position не наследуются.
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        // CSS 2.1 §10.8.1 — vertical-align не наследуется. Initial = baseline.
        vertical_align: VerticalAlign::Baseline,
        // CSS Images L3 §6.1 — image-rendering inherited.
        image_rendering: inherited.image_rendering,
        // CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style inherited.
        text_wrap_mode: inherited.text_wrap_mode,
        text_wrap_style: inherited.text_wrap_style,
    };

    // CSS Properties and Values L1 §1.1 — registry зарегистрированных
    // custom-properties. Карта строится локально для каждого узла:
    // на типичной странице 0..5 @property-правил, накладные расходы мизерны
    // в сравнении со стоимостью каскада. При повторе имени (см. spec —
    // last wins) `insert` корректно сохраняет последнее объявление.
    let registry: HashMap<&str, &PropertyRule> = sheet
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Откатываем у себя унаследованные значения тех зарегистрированных
    // custom-properties, у которых `inherits: false` — для них потомок
    // должен видеть либо локальную декларацию, либо initial-value, а не
    // родительское значение.
    if !registry.is_empty() {
        style.custom_props.retain(|key, _| {
            registry.get(key.as_str()).is_none_or(|p| p.inherits)
        });
    }

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        // Для не-элементов (Document, Text внутри anonymous-wrapping) тоже
        // применяем initial-value: var(--registered) в наследуемом стиле
        // должен резолвиться через initial-value, если декларации нет.
        apply_property_initial_values(&mut style.custom_props, &registry);
        return style;
    }

    // UA stylesheet: семантические элементы получают italic / bold по
    // умолчанию, CSS-декларации ниже могут это переопределить.
    if let Some(fs) = ua_font_style(doc, node) {
        style.font_style = fs;
    }
    if let Some(fw) = ua_font_weight(doc, node) {
        style.font_weight = fw;
    }

    // CSS Quirks Mode — Quirks-only UA-rule для `<table>`: сбрасывает
    // font / color / text-align / white-space к initial-values, чтобы
    // legacy table-layout страницы (где CSS на `<body>` задавал шрифт /
    // цвет) рендерились с дефолтным шрифтом таблицы, как в IE/Netscape.
    // В Standards / LimitedQuirks не применяется.
    apply_quirks_table_reset(doc, node, &mut style);
    // CSS Quirks Mode §3.2: replaced-элементы получают line-height: 1 как UA-правило.
    apply_quirks_line_height(doc, node, &mut style);

    // HTML presentational hints (HTML5 §10): для `<img>` атрибуты
    // `width`/`height` задают начальные значения соответствующих CSS-свойств.
    // Применяются ДО CSS-каскада, поэтому любое author-CSS правило
    // перекроет атрибут даже с specificity (0,0,1). Парсятся как unitless
    // целые пиксели — это HTML5 правило для `<img>`, единицы и проценты
    // в этих атрибутах игнорируются.
    apply_image_presentational_hints(doc, node, &mut style);

    // HTML5 §15 «Rendering»: `bgcolor` на `<body>` / `<table>` / `<thead>` /
    // `<tbody>` / `<tfoot>` / `<tr>` / `<td>` / `<th>` мапается на
    // `background-color` (presentational hint). Парсится по HTML5 §2.4.6
    // «rules for parsing a legacy color value» — более лояльный алгоритм,
    // чем CSS quirks hashless hex: принимает named colors, `#rgb` / `#rrggbb`,
    // hashless hex произвольной длины и любую строку, в которой можно
    // найти хотя бы какие-то hex-digits после padding-procedure.
    apply_bgcolor_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.6 «The page»: `text` атрибут на `<body>` и `<font color>`
    // на любом элементе мапаются на CSS `color` (presentational hint).
    // Парсятся тем же legacy-парсером, что и `bgcolor`. Author CSS поверх —
    // выигрывает. `<body link/vlink/alink>` отложены: `:link` единственный
    // матчится в Phase 0, `:visited`/`:active` без runtime — no-op.
    apply_text_color_presentational_hint(doc, node, &mut style);

    // CSS Cascade L4 §6.4.3 — inline style: парсим HTML-атрибут `style=""`
    // и кладём его декларации в отдельный буфер. Они подключаются к каскаду
    // через дополнительный sort-bit `is_inline` (ниже): внутри одного origin
    // (нормального или !important) inline всегда побеждает любой селектор —
    // это «Element-Attached Styles» тир в Cascade L4 §8.1, идущий после
    // Layer/Specificity/Order, но до Importance-инверсии.
    let inline_decls: Vec<Declaration> = doc
        .get(node)
        .get_attr("style")
        .filter(|s| !s.is_empty())
        .map(parse_inline_style)
        .unwrap_or_default();

    // Собираем все matched declarations с их sort key:
    // (important, is_inline, specificity, rule_order, decl_index). `important`
    // идёт первым: после ascending sort `true > false`, поэтому !important идёт
    // в конец и побеждает normal даже при меньшей specificity (CSS Cascade L4
    // §8.1). `is_inline` — вторым: в пределах одного `important` inline-style
    // атрибут побеждает стилевой лист (CSS Cascade L4 §6.4.3). Внутри одного
    // origin `important = false` сначала разрешается обычный каскад, потом тот
    // же каскад применяется поверх с !important.
    let mut matched: Vec<(bool, bool, Specificity, usize, usize, &Declaration)> = Vec::new();
    for (rule_idx, rule) in sheet.rules.iter().enumerate() {
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                matched.push((decl.important, false, spec, rule_idx, decl_idx, decl));
            }
        }
    }
    // CSS Media Queries L4: rules внутри `@media`-блока, чей query
    // совпадает с текущим MediaContext, добавляются в каскад. В Phase 0
    // упрощённый MediaContext: media_type="screen", width/height из
    // viewport, prefers_dark=false. Source-order между обычными и
    // @media-rules не сохраняется идеально (все @media идут после
    // обычных) — это известное ограничение.
    let media_ctx = media_context_from_viewport(viewport);
    let mut next_rule_idx = sheet.rules.len();
    for media in &sheet.media_rules {
        if !media.query.matches(&media_ctx) {
            next_rule_idx += media.rules.len();
            continue;
        }
        for rule in &media.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, false, spec, next_rule_idx, decl_idx, decl));
                }
            }
            next_rule_idx += 1;
        }
    }
    // Inline-style declarations подключаются с `is_inline = true` и
    // synthetic specificity = default (Cascade L4 §6.4.3 — реальная
    // specificity inline-стиля игнорируется в сортировке: за порядок
    // отвечает is_inline-бит, а внутри inline — источниковый порядок
    // декларации в атрибуте).
    for (decl_idx, decl) in inline_decls.iter().enumerate() {
        matched.push((
            decl.important,
            true,
            Specificity::default(),
            next_rule_idx,
            decl_idx,
            decl,
        ));
    }
    matched.sort_by_key(|&(imp, inline, spec, rule_idx, decl_idx, _)| {
        (imp, inline, spec, rule_idx, decl_idx)
    });

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    let parent_fs = inherited.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for (_, _, _, _, _, decl) in &matched {
        apply_font_size(&mut style, decl, parent_fs, viewport, is_quirks);
    }

    // Custom-properties pass: все `--name: value` декларации применяются
    // отдельно и ДО main-pass, чтобы любая обычная декларация в main-pass
    // могла видеть финальное значение custom property независимо от порядка
    // объявления в source. Каскад уже соблюдён через sort `matched`:
    // последующая запись с тем же ключом перебивает раннюю.
    //
    // CSS Properties and Values L1 §1.1 «invalid at computed value time»:
    // для зарегистрированных custom properties value валидируется против
    // `syntax`-дескриптора. Невалидное значение игнорируется — старое
    // значение (родительское inherited или initial-value) остаётся.
    // value, содержащее `var(`, пропускается без валидации — резолв
    // происходит позже, и итоговая строка может быть валидной.
    for (_, _, _, _, _, decl) in &matched {
        if let Some(name) = decl.property.strip_prefix("--") {
            let key = format!("--{name}");
            if let Some(prop_rule) = registry.get(key.as_str())
                && !decl.value.contains("var(")
                && !validate_against_syntax(&decl.value, &prop_rule.syntax)
            {
                // Invalid at computed value time — skip declaration.
                continue;
            }
            style.custom_props.insert(key, decl.value.clone());
        }
    }

    // CSS Properties and Values L1 §1.1: для каждого зарегистрированного
    // имени, у которого после custom-pass нет значения (ни унаследованного,
    // ни локально объявленного), подставить `initial-value`. Делается между
    // custom-pass и main-pass, чтобы `var(--registered)` в обычных
    // декларациях видел initial-value-fallback.
    apply_property_initial_values(&mut style.custom_props, &registry);

    // Main-pass: остальные декларации; em-basis теперь = current font_size.
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`;
    // `inherited` целиком — для CSS-wide keywords (CSS Cascade L4 §7).
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;
    for (_, _, _, _, _, decl) in &matched {
        apply_declaration(&mut style, decl, em_basis, viewport, parent_weight, inherited, is_quirks);
    }

    style
}

/// CSS Properties and Values L1 §1.1: для каждого зарегистрированного
/// custom property, у которого нет значения в `custom_props`, подставляет
/// `initial-value` (если он указан). Невызов для `inherits: true` имени
/// с унаследованным значением — потому что `contains_key` уже возвращает
/// true. Для `inherits: false` имени родительское значение было выпилено
/// в `compute_style` через `retain`.
fn apply_property_initial_values(
    custom_props: &mut HashMap<String, String>,
    registry: &HashMap<&str, &PropertyRule>,
) {
    for (name, p) in registry {
        if custom_props.contains_key(*name) {
            continue;
        }
        if let Some(iv) = &p.initial_value {
            // CSS Properties and Values L1 §1.1: initial-value валидируется
            // против syntax. Per spec — невалидный initial делает @property
            // невалидным целиком; Phase 0 более снисходителен и просто
            // не подставляет неподходящий initial (потомок без декларации
            // получит inherited или ничего).
            if validate_against_syntax(iv, &p.syntax) {
                custom_props.insert((*name).to_string(), iv.clone());
            }
        }
    }
}

/// CSS Properties and Values L1 §2 — упрощённая валидация значения
/// custom property против `syntax`-дескриптора.
///
/// Поддерживаются:
/// - `*` — универсал (любое значение проходит);
/// - `<length>` — px, em, rem, vh, vw, vmin, vmax (но не `%`);
/// - `<percentage>` — число с суффиксом `%`;
/// - `<length-percentage>` — union;
/// - `<color>` — любая форма, которую парсит `parse_color`;
/// - `<integer>` — целое со знаком;
/// - `<number>` — число с плавающей точкой;
/// - `<angle>` — `deg` / `rad` / `turn` / `grad`;
/// - `<time>` — `s` / `ms` (CSS Values L4 §8);
/// - `<resolution>` — `dpi` / `dpcm` / `dppx` / `x` (CSS Values L4 §9.1);
/// - `<custom-ident>` — идентификатор, не совпадающий с CSS-wide keyword.
///
/// Union через `|` — match если хоть одна альтернатива принимает. Прочие
/// типы (`<image>`, `<url>`, `<transform-function>`, и т.д.) и multipliers
/// (`+`, `#`) в Phase 0 трактуются как universal — возвращают `true`,
/// чтобы не отбраковывать корректные value у потребителей этих типов.
pub fn validate_against_syntax(value: &str, syntax: &str) -> bool {
    let syntax = syntax.trim();
    if syntax == "*" {
        return true;
    }
    let value = value.trim();
    // Union по `|`.
    for alt in syntax.split('|') {
        let alt = alt.trim();
        let matched = match alt {
            "<length>" => matches_syntax_length(value),
            "<percentage>" => matches_syntax_percentage(value),
            "<length-percentage>" => {
                matches_syntax_length(value) || matches_syntax_percentage(value)
            }
            "<color>" => parse_color(value).is_some(),
            "<integer>" => matches_syntax_integer(value),
            "<number>" => matches_syntax_number(value),
            "<angle>" => matches_syntax_angle(value),
            "<time>" => matches_syntax_time(value),
            "<resolution>" => matches_syntax_resolution(value),
            "<custom-ident>" => matches_syntax_custom_ident(value),
            // Неизвестный тип — permissive, чтобы не блокировать корректные
            // declarations с пока-неподдержанными syntax-формами.
            _ => true,
        };
        if matched {
            return true;
        }
    }
    false
}

fn matches_syntax_length(value: &str) -> bool {
    // <length> = px/em/rem/vh/vw/vmin/vmax/calc(...) — без `%`.
    match parse_length(value) {
        Some(Length::Percent(_)) => false,
        Some(_) => true,
        None => false,
    }
}

fn matches_syntax_percentage(value: &str) -> bool {
    matches!(parse_length(value), Some(Length::Percent(_)))
}

fn matches_syntax_integer(value: &str) -> bool {
    value.parse::<i64>().is_ok()
}

fn matches_syntax_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn matches_syntax_angle(value: &str) -> bool {
    // Number + один из суффиксов: deg, rad, turn, grad.
    for suffix in ["deg", "rad", "turn", "grad"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_time(value: &str) -> bool {
    // CSS Values L4 §8 — <time> с суффиксами `s` или `ms`.
    // Порядок важен: `ms` проверяем раньше `s`, иначе `200ms` распарсится
    // как 200m + остаток `s` (а `200m` не валидный number → false).
    for suffix in ["ms", "s"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_resolution(value: &str) -> bool {
    // CSS Values L4 §9.1 — <resolution> с суффиксами `dppx`/`dpcm`/`dpi`/`x`.
    // `dppx` проверяем раньше `dpi`/`dpcm` (длинный суффикс), `x` — последним
    // (резервный alias dppx; HTML5 media queries).
    for suffix in ["dppx", "dpcm", "dpi", "x"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_custom_ident(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // CSS-wide keywords нельзя использовать как custom-ident.
    if parse_css_wide_keyword(value).is_some() {
        return false;
    }
    // Также запрещены `default` (CSS spec) и `none` в большинстве контекстов.
    // Простая проверка: ident начинается с letter / `_` / `-`, дальше —
    // alphanumeric / `-` / `_`. ASCII-only для простоты.
    let mut chars = value.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ──────────────── selector matching ────────────────

fn matches_complex(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    // Справа налево с back-tracking. Алгоритм:
    //   1. Складываем (compounds, combinators) в массивы.
    //   2. Рекурсивно: матчим последний compound на текущем `node`; если ОК
    //      и осталось > 0 compound-ов левее, для combinator-а перед ним
    //      перебираем ВСЕ возможные кандидаты (предки для descendant /
    //      earlier-siblings для later-sibling) и рекурсивно матчим суффикс
    //      в каждом. child / next-sibling имеют ровно одного кандидата.
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    matches_chain(&compounds, &combinators, doc, node)
}

/// Рекурсивный matcher с back-tracking. `compounds[last]` матчится на `node`;
/// для левее идущих compound-ов перебираем кандидатов согласно combinator-у.
fn matches_chain(
    compounds: &[&CompoundSelector],
    combinators: &[Combinator],
    doc: &Document,
    node: NodeId,
) -> bool {
    let n = compounds.len();
    debug_assert_eq!(combinators.len(), n - 1);

    if !matches_compound(compounds[n - 1], doc, node) {
        return false;
    }
    if n == 1 {
        return true;
    }

    let comb = combinators[n - 2];
    let prev_compounds = &compounds[..n - 1];
    let prev_combinators = &combinators[..n - 2];

    match comb {
        Combinator::Descendant => {
            // Перебираем всех предков как кандидатов.
            let mut cur = doc.get(node).parent;
            while let Some(p) = cur {
                if is_element(doc, p)
                    && matches_chain(prev_compounds, prev_combinators, doc, p)
                {
                    return true;
                }
                cur = doc.get(p).parent;
            }
            false
        }
        Combinator::Child => {
            // Один кандидат: parent.
            let Some(parent) = doc.get(node).parent else { return false; };
            if !is_element(doc, parent) {
                return false;
            }
            matches_chain(prev_compounds, prev_combinators, doc, parent)
        }
        Combinator::NextSibling => {
            // Один кандидат: предыдущий element-sibling.
            let Some(prev) = previous_element_sibling(doc, node) else { return false; };
            matches_chain(prev_compounds, prev_combinators, doc, prev)
        }
        Combinator::LaterSibling => {
            // Перебираем все earlier-siblings как кандидатов.
            let mut sib = previous_element_sibling(doc, node);
            while let Some(s) = sib {
                if matches_chain(prev_compounds, prev_combinators, doc, s) {
                    return true;
                }
                sib = previous_element_sibling(doc, s);
            }
            false
        }
    }
}

fn matches_compound(compound: &CompoundSelector, doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return false;
    };
    for part in &compound.parts {
        if !matches_simple(part, doc, node, &name.local, attrs) {
            return false;
        }
    }
    true
}

fn matches_simple(
    sel: &SimpleSelector,
    doc: &Document,
    node: NodeId,
    tag: &str,
    attrs: &[Attribute],
) -> bool {
    match sel {
        SimpleSelector::Type(t) => t == tag,
        SimpleSelector::Class(c) => attrs
            .iter()
            .find(|a| a.name.local == "class")
            .map(|a| a.value.split_whitespace().any(|w| w == c))
            .unwrap_or(false),
        SimpleSelector::Id(i) => attrs
            .iter()
            .find(|a| a.name.local == "id")
            .map(|a| a.value == *i)
            .unwrap_or(false),
        SimpleSelector::Universal => true,
        SimpleSelector::Attribute(a) => matches_attribute(a, attrs),
        SimpleSelector::PseudoClass(p) => matches_pseudo_class(p, doc, node),
        SimpleSelector::PseudoElement(_) => false,
    }
}

fn matches_attribute(sel: &AttrSelector, attrs: &[Attribute]) -> bool {
    let Some(attr) = attrs.iter().find(|a| a.name.local == sel.name) else {
        return false;
    };
    let ci = sel.case_insensitive;
    match (sel.op, sel.value.as_deref()) {
        (None, _) => true,
        (Some(AttrOp::Equals), Some(v)) => str_eq(&attr.value, v, ci),
        (Some(AttrOp::Includes), Some(v)) => {
            !v.is_empty() && attr.value.split_whitespace().any(|w| str_eq(w, v, ci))
        }
        (Some(AttrOp::DashMatch), Some(v)) => {
            // Точное совпадение или префикс с разделителем `-`. `i` применяется
            // к обеим частям сравнения (CSS L4 §6.3.6).
            str_eq(&attr.value, v, ci) || str_starts_with(&attr.value, &format!("{v}-"), ci)
        }
        (Some(AttrOp::Prefix), Some(v)) => !v.is_empty() && str_starts_with(&attr.value, v, ci),
        (Some(AttrOp::Suffix), Some(v)) => !v.is_empty() && str_ends_with(&attr.value, v, ci),
        (Some(AttrOp::Substring), Some(v)) => !v.is_empty() && str_contains(&attr.value, v, ci),
        _ => false,
    }
}

/// ASCII case-insensitive (если `ci`) сравнение, иначе побайтовое. Cyrillic и
/// другой не-ASCII всегда сравнивается побайтово (`eq_ignore_ascii_case` не
/// трогает байты со старшим битом). Работа через `as_bytes()` нужна, чтобы
/// `starts_with`/`ends_with`/`contains` не упирались в char-boundary в
/// многобайтовых UTF-8 строках.
fn str_eq(a: &str, b: &str, ci: bool) -> bool {
    if ci { a.eq_ignore_ascii_case(b) } else { a == b }
}

fn str_starts_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.starts_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[..n.len()].eq_ignore_ascii_case(n)
}

fn str_ends_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.ends_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[h.len() - n.len()..].eq_ignore_ascii_case(n)
}

fn str_contains(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.contains(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    if h.len() < n.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn matches_pseudo_class(p: &PseudoClass, doc: &Document, node: NodeId) -> bool {
    match p {
        PseudoClass::FirstChild => is_first_element_child(doc, node),
        PseudoClass::LastChild => is_last_element_child(doc, node),
        PseudoClass::OnlyChild => {
            is_first_element_child(doc, node) && is_last_element_child(doc, node)
        }
        PseudoClass::Empty => is_empty_element(doc, node),
        PseudoClass::Root => is_root_element(doc, node),
        PseudoClass::FirstOfType => is_first_of_type(doc, node),
        PseudoClass::LastOfType => is_last_of_type(doc, node),
        PseudoClass::OnlyOfType => is_first_of_type(doc, node) && is_last_of_type(doc, node),
        PseudoClass::NthChild(spec, of) => {
            match element_index_filtered(doc, node, false, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthLastChild(spec, of) => {
            match element_index_filtered(doc, node, true, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthOfType(spec) => match element_index_of_type(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastOfType(spec) => match element_index_of_type(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::Not(list) => {
            // CSS Selectors L4 §5.4: матчит, если ни один селектор из списка
            // элементу не подходит. Внутри допустимы complex-селекторы и
            // nested `:not` — рекурсия идёт через `matches_complex`.
            !list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            // CSS4 §17: матчит, если матчит хоть один селектор из списка.
            // `:where(...)` отличается только тем, что contributes 0 specificity —
            // matching identical с `:is`.
            list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Has(list) => {
            // CSS Selectors L4 §17.2: матчит элемент E, если хоть один из
            // relative selectors удовлетворён каким-то элементом в его
            // поддереве (для combinator None или Child) или sibling-цепочке
            // (для NextSibling / LaterSibling). Внутри matches_complex —
            // тот же recursive matcher с back-tracking, относительно
            // кандидата (а не E); кандидаты ищутся согласно combinator-у.
            list.iter().any(|rs| matches_relative(rs, doc, node))
        }
        PseudoClass::PlaceholderShown => matches_placeholder_shown(doc, node),
        PseudoClass::Required => matches_required(doc, node, true),
        PseudoClass::Optional => matches_required(doc, node, false),
        PseudoClass::ReadOnly => matches_read_only(doc, node),
        PseudoClass::ReadWrite => matches_read_write(doc, node),
        PseudoClass::Disabled => matches_disabled(doc, node, true),
        PseudoClass::Enabled => matches_disabled(doc, node, false),
        PseudoClass::Checked => matches_checked(doc, node),
        PseudoClass::Indeterminate => matches_indeterminate(doc, node),
        PseudoClass::Default => matches_default(doc, node),
        PseudoClass::Lang(tags) => matches_lang(doc, node, tags),
        PseudoClass::Dir(arg) => matches_dir(doc, node, *arg),
        PseudoClass::Link => matches_any_link(doc, node),
        // CSS Selectors L4 §6.2.3: `:visited` требует history-runtime
        // (`lumen-storage::History` + safe-history-API с privacy-ограничениями).
        // Phase 0 без runtime — всегда false; никакая ссылка не считается
        // посещённой. Это безопасный default (соответствует privacy-by-default
        // принципу проекта №1: ничего не утекает через стилизацию).
        PseudoClass::Visited => false,
        PseudoClass::AnyLink => matches_any_link(doc, node),
        // CSS Selectors L4 §4.2: `:scope` matches the document's root element
        // в author-CSS context (без runtime querySelector). Эквивалент `:root`.
        // Реальная разница появится при integration с DOM querySelector API
        // (P3 + JS-runtime) — пока что в layout-cascade оба ведут себя
        // одинаково.
        PseudoClass::Scope => is_root_element(doc, node),
        // CSS Selectors L4 §9.6: `:target` matches element с id равным
        // URL fragment-у (case-sensitive — HTML LS §3.2.6 делает `id`
        // case-sensitive, поэтому matcher не lowercase'ит). Без fragment-а
        // (`Document::target() == None`) — никакой element не матчит.
        // Phase 0: значение target_id выставляет shell-интеграция (P3) при
        // навигации; до её появления matcher всегда возвращает false.
        PseudoClass::Target => matches_target(doc, node),
        // CSS Selectors L4 §9.7: `:target-within` — element сам :target или
        // у него в поддереве есть :target-element. Short-circuit при
        // `Document::target() == None` — на странице без fragment-а никто
        // не матчит, walk поддерева не нужен.
        PseudoClass::TargetWithin => matches_target_within(doc, node),
        // CSS Selectors L4 §6.4.1, HTML LS §4.13.5 — `:defined` матчит
        // built-in HTML/SVG/MathML элементы и зарегистрированные custom
        // elements. Custom-element-имена по HTML LS §4.13.2 обязаны иметь
        // ASCII `-`; без registry в Phase 0 matcher использует это правило
        // как аппроксимацию: имя без `-` → built-in (defined); имя с `-` →
        // un-registered custom element (undefined). Когда P3 поднимет
        // registry, проверка станет `built-in || registry.has(name)`.
        PseudoClass::Defined => matches_defined(doc, node),
        // Fullscreen API §4.2 `:fullscreen` — runtime-only: top-layer
        // элементов, поднятых через `Element.requestFullscreen()`. Phase 0
        // без Fullscreen API в shell — всегда `false` (privacy-/UX-safe
        // default: страница не может имитировать fullscreen-стили
        // вне реального fullscreen-режима).
        PseudoClass::Fullscreen => false,
        // CSS Selectors L4 §16.5.2 `:modal` — `<dialog>` после
        // `dialog.showModal()` (но не `dialog.show()` non-modal) или
        // элемент в fullscreen top-layer. Runtime-only: атрибут `open`
        // не разделяет modal vs non-modal dialog. Phase 0 без dialog
        // runtime — всегда `false`.
        PseudoClass::Modal => false,
        // HTML LS §6.12.2 `:popover-open` — popover в открытом состоянии
        // после `element.showPopover()` / клика по `popovertarget`.
        // Runtime-only: атрибут `popover` декларирует тип, но не открытое
        // состояние. Phase 0 без Popover API runtime — всегда `false`.
        PseudoClass::PopoverOpen => false,
        // CSS Selectors L4 §11.4 time-dimensional pseudo-classes —
        // `:current` / `:past` / `:future` matches на active / elapsed /
        // upcoming моменты в timed-text потоке (WebVTT cue rendering при
        // воспроизведении видео/аудио). Runtime-only: нужна синхронизация с
        // media timeline и cue lifecycle. Phase 0 без timed-text runtime
        // все три всегда `false`.
        PseudoClass::Current => false,
        PseudoClass::Past => false,
        PseudoClass::Future => false,
        PseudoClass::InRange => matches_in_range(doc, node) == Some(true),
        PseudoClass::OutOfRange => matches_in_range(doc, node) == Some(false),
        PseudoClass::Unsupported(_) => false,
    }
}

/// `:defined` matcher per CSS Selectors L4 §6.4.1 / HTML LS §4.13.5.
///
/// Текстовые / комментарные ноды псевдо-классам не подвергаются вообще
/// (Selector L4 §3.1 «selectors only apply to elements»), но selector
/// engine приходит сюда только для элементов — на всякий случай делаем
/// fast-fail на не-элемент.
fn matches_defined(doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return false;
    };
    // HTML LS §4.13.2 «Valid custom element name»: имя custom-element-а
    // обязано содержать дефис. Это единственная синтаксическая разница
    // между «built-in» и «custom». В Phase 0 без CustomElementRegistry
    // считаем все built-in defined, все custom-имена — undefined.
    !name.local.as_str().contains('-')
}

/// Default-значение `<input type>` — `text` (HTML5 §4.10.5.1.2). Возвращает
/// lower-case значение `type`-атрибута; пустая строка трактуется как `text`.
fn input_type_lower(doc: &Document, node: NodeId) -> String {
    let node_ref = doc.get(node);
    node_ref
        .get_attr("type")
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "text".to_string())
}

/// `<input>`-типы, к которым применимы `:read-only` / `:read-write` per HTML5
/// §4.16.4 «mutable input» — text-like (введение текста).
fn input_is_text_like(input_type: &str) -> bool {
    matches!(
        input_type,
        "text"
            | "search"
            | "url"
            | "tel"
            | "email"
            | "password"
            | "number"
            | "date"
            | "month"
            | "week"
            | "time"
            | "datetime-local"
    )
}

/// `<input>`-типы, к которым применим `required` per HTML5 §4.10.3 — text-like
/// + `checkbox` / `radio` / `file`.
fn input_supports_required(input_type: &str) -> bool {
    input_is_text_like(input_type)
        || matches!(input_type, "checkbox" | "radio" | "file")
}

/// CSS Selectors L4 §15.4 / HTML5 §4.10.3 `:required` / `:optional`.
/// `want_required = true` → `:required`, иначе `:optional`. Возвращает true
/// только для form control-ов, к которым применим атрибут `required`.
///
/// Применимо: `<select>`, `<textarea>`, и `<input>` text-like / checkbox /
/// radio / file. Прочие элементы (`<input type=hidden>`, `<button>`, `<div>`)
/// не матчатся ни одним из двух.
fn matches_required(doc: &Document, node: NodeId, want_required: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let applies = match tag {
        "select" | "textarea" => true,
        "input" => input_supports_required(&input_type_lower(doc, node)),
        _ => false,
    };
    if !applies {
        return false;
    }
    let has_required = node_ref.get_attr("required").is_some();
    has_required == want_required
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-write` — «mutable» form
/// control или `contenteditable`-элемент.
///
/// True для:
///   - `<input>` text-like type БЕЗ `readonly` и БЕЗ `disabled`;
///   - `<textarea>` БЕЗ `readonly` и БЕЗ `disabled`;
///   - любого элемента с эффективным `contenteditable="true"` (включая
///     наследование от ancestor — `contenteditable=""` тоже считается true).
///
/// Прочие элементы — false (и матчат `:read-only`).
fn matches_read_write(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let is_form_mutable = match tag {
        "input" => {
            input_is_text_like(&input_type_lower(doc, node))
                && node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        "textarea" => {
            node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        _ => false,
    };
    if is_form_mutable {
        return true;
    }
    is_effectively_contenteditable(doc, node)
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-only` — «not mutable».
///
/// Per spec: «matches all other HTML elements» — то есть все Element-ы, не
/// попадающие под `:read-write`. Не Element-ы (Text / Comment / Document) не
/// матчатся ничем.
fn matches_read_only(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    if !matches!(node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    !matches_read_write(doc, node)
}

/// Эффективное значение `contenteditable` с наследованием от ancestor-ов.
/// `contenteditable="true"` или `contenteditable=""` (пустая строка) → true;
/// `contenteditable="false"` → false (и обрывает наследование); отсутствие
/// атрибута на узле — смотрим выше.
fn is_effectively_contenteditable(doc: &Document, node: NodeId) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        let node_ref = doc.get(n);
        if let NodeData::Element { .. } = node_ref.data
            && let Some(v) = node_ref.get_attr("contenteditable")
        {
            let lower = v.trim().to_ascii_lowercase();
            if lower.is_empty() || lower == "true" {
                return true;
            }
            if lower == "false" {
                return false;
            }
        }
        cur = node_ref.parent;
    }
    false
}

/// HTML5 §4.10.19.2 «can be disabled»-элементы — `<button>`, `<input>`,
/// `<select>`, `<textarea>`, `<optgroup>`, `<option>`, `<fieldset>`.
fn is_disableable_form_control(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
    )
}

/// CSS Selectors L4 §14.2 / HTML5 §4.10.19.2 `:disabled` / `:enabled`.
/// `want_disabled = true` → `:disabled`, иначе `:enabled`.
///
/// Элемент считается disabled, если:
///   - применим к `:disabled` per `is_disableable_form_control` И;
///   - либо у него самого есть атрибут `disabled`;
///   - либо у `<option>` ancestor-`<optgroup>` имеет `disabled` (HTML5 §4.10.10);
///   - либо элемент находится внутри `<fieldset disabled>` И НЕ внутри
///     первого `<legend>`-ребёнка этого fieldset (HTML5 §4.10.16).
///     `<fieldset>` сам disabled только по собственному атрибуту, не от
///     ancestor-fieldset.
///
/// Прочие элементы (`<div>`, `<p>`, и т.д.) — не матчат ни `:disabled`, ни
/// `:enabled`.
fn matches_disabled(doc: &Document, node: NodeId, want_disabled: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !is_disableable_form_control(tag) {
        return false;
    }
    let actually_disabled = is_actually_disabled(doc, node, tag);
    actually_disabled == want_disabled
}

fn is_actually_disabled(doc: &Document, node: NodeId, tag: &str) -> bool {
    let node_ref = doc.get(node);
    if node_ref.get_attr("disabled").is_some() {
        return true;
    }
    // `<option>` наследует disabled от непосредственного `<optgroup>`-родителя
    // (HTML5 §4.10.10): «An option element is disabled if its disabled attribute
    // is set or if it is a child of an optgroup element whose disabled attribute
    // is set».
    if tag == "option"
        && let Some(p) = node_ref.parent
    {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "optgroup"
            && p_ref.get_attr("disabled").is_some()
        {
            return true;
        }
    }
    // `<fieldset>` сам disabled только по собственному атрибуту; ancestor-walk
    // для него не нужен.
    if tag == "fieldset" {
        return false;
    }
    // Form control внутри `<fieldset disabled>` — disabled, кроме случая, когда
    // он лежит в первом `<legend>`-ребёнке этого fieldset (HTML5 §4.10.16).
    let mut child = node;
    let mut cur = node_ref.parent;
    while let Some(p) = cur {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "fieldset"
            && p_ref.get_attr("disabled").is_some()
            && !is_descendant_of_first_legend_child(doc, p, child)
        {
            return true;
        }
        child = p;
        cur = p_ref.parent;
    }
    false
}

/// True, если `descendant_chain_start` — это сам first-`<legend>`-ребёнок
/// `fieldset` или лежит в его поддереве. Для проверки достаточно посмотреть на
/// `child` — тот узел, через которого мы дошли до fieldset; если он же —
/// первый element-child `<legend>`, то вся ветка живёт под legend.
fn is_descendant_of_first_legend_child(
    doc: &Document,
    fieldset: NodeId,
    child_on_path: NodeId,
) -> bool {
    let first_legend = doc
        .get(fieldset)
        .children
        .iter()
        .copied()
        .find(|&c| is_element(doc, c))
        .filter(|&c| {
            let c_ref = doc.get(c);
            matches!(&c_ref.data, NodeData::Element { name, .. } if name.local.as_str() == "legend")
        });
    matches!(first_legend, Some(l) if l == child_on_path)
}

/// CSS Selectors L4 §15.1 `:placeholder-shown` — true для form-control,
/// у которого есть непустой `placeholder`-атрибут И value-атрибут отсутствует
/// либо пустой.
///
/// В Phase 0 без runtime form-state значение никем не вводится — текущее
/// значение определяется только author-объявленным `value`-атрибутом. Этого
/// достаточно для условных стилей вроде `input:placeholder-shown { color:
/// gray }` на статически отрисованной форме.
fn matches_placeholder_shown(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if tag != "input" && tag != "textarea" {
        return false;
    }
    let Some(placeholder) = node_ref.get_attr("placeholder") else {
        return false;
    };
    if placeholder.trim().is_empty() {
        return false;
    }
    // `value` атрибут с непустым содержимым → пользователь (или author)
    // уже задал контент, placeholder скрыт. `<textarea>`-у HTML присваивает
    // значение через текстовых детей (а не через атрибут), но Phase 0
    // нашей кодовой базы DOM-mutations нет — текстовое содержимое <textarea>
    // в DOM тоже трактуем как «не пустое значение».
    if let Some(value) = node_ref.get_attr("value")
        && !value.is_empty()
    {
        return false;
    }
    if tag == "textarea" && has_non_whitespace_text(doc, node) {
        return false;
    }
    true
}

/// `:checked` (CSS Selectors L4 §10.1). Pure attribute-based matcher без
/// runtime form-state:
/// - `<input type=checkbox|radio>` с атрибутом `checked` (значение атрибута
///   не имеет значения — спецификация трактует наличие как true);
/// - `<option>` с атрибутом `selected`.
///
/// Динамически переключённый через клик/JS checkbox не отражается в
/// DOM-атрибутах и здесь не учитывается — Phase 0 без form-state runtime.
fn matches_checked(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t != "checkbox" && t != "radio" {
                return false;
            }
            node_ref.get_attr("checked").is_some()
        }
        "option" => node_ref.get_attr("selected").is_some(),
        _ => false,
    }
}

/// `:indeterminate` (CSS Selectors L4 §10.2, HTML5 §4.16.3 + §4.10.18.4).
/// Применяется к:
/// - `<input type=checkbox>` с DOM-флагом indeterminate (Phase 0: всегда
///   `false` — флаг существует только через JS `.indeterminate = true`,
///   которого пока нет);
/// - `<input type=radio>` в группе (одинаковый `name` внутри ближайшей
///   form-owner-области) без ни одного checked-радио. Если радио без `name`,
///   группа = только сам элемент — тогда indeterminate ≡ нет `checked`;
/// - `<progress>` без атрибута `value` (indeterminate progress per HTML5).
fn matches_indeterminate(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t == "radio" {
                // Найти ближайший <form>-предок; если нет — корень документа.
                let scope = nearest_form_or_root(doc, node);
                let radio_name = node_ref.get_attr("name").map(|s| s.to_string());
                !any_descendant(doc, scope, |n| {
                    if !is_element(doc, n) {
                        return false;
                    }
                    let other = doc.get(n);
                    let NodeData::Element { name: n2, .. } = &other.data else {
                        return false;
                    };
                    if n2.local.as_str() != "input" {
                        return false;
                    }
                    let t2 = input_type_lower(doc, n);
                    if t2 != "radio" {
                        return false;
                    }
                    // Радио считается членом той же группы если name совпадает
                    // (или оба отсутствуют — узкая группа из одного элемента).
                    let n2_name = other.get_attr("name").map(|s| s.to_string());
                    if n2_name != radio_name {
                        return false;
                    }
                    other.get_attr("checked").is_some()
                })
            } else {
                // Phase 0: checkbox indeterminate выставляется только через
                // JS — DOM не выражает этого. Всегда false.
                false
            }
        }
        "progress" => node_ref.get_attr("value").is_none(),
        _ => false,
    }
}

/// `:default` (CSS Selectors L4 §10.4, HTML5 §4.16.3) — «по-умолчанию
/// активный» form control:
/// - `<option>` с атрибутом `selected`;
/// - checkbox/radio с атрибутом `checked`;
/// - default submit-button формы — первая в DOM-порядке формы
///   `<button type=submit>` / `<input type=submit|image>`. `type=submit` —
///   default для `<button>` (HTML5 §4.10.8) и для `<input>` без `type` это
///   `text`, поэтому submit-button обязан иметь `type=submit`.
fn matches_default(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    match tag {
        "option" => node_ref.get_attr("selected").is_some(),
        "input" => {
            let t = input_type_lower(doc, node);
            if (t == "checkbox" || t == "radio") && node_ref.get_attr("checked").is_some() {
                return true;
            }
            if t == "submit" || t == "image" {
                return is_default_submit_button(doc, node);
            }
            false
        }
        "button" => {
            // default-type для <button> = submit (HTML5 §4.10.8).
            let t = node_ref
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t != "submit" {
                return false;
            }
            is_default_submit_button(doc, node)
        }
        _ => false,
    }
}

/// Default submit-button формы — первая submit-кнопка в DOM-порядке внутри
/// ближайшего `<form>`-предка (HTML5 §4.10.22.3 «implicit submission»).
/// Если предка `<form>` нет, кнопка не form-owner-связана и не считается
/// default.
fn is_default_submit_button(doc: &Document, node: NodeId) -> bool {
    let Some(form) = nearest_form(doc, node) else {
        return false;
    };
    let mut found: Option<NodeId> = None;
    walk_first_submit(doc, form, &mut found);
    found == Some(node)
}

/// Pre-order обход поддерева form в поиске первой submit-кнопки. Сохраняет
/// результат в `found` и останавливается раньше через короткое замыкание
/// `is_some()` на ранних уровнях.
fn walk_first_submit(doc: &Document, scope: NodeId, found: &mut Option<NodeId>) {
    if found.is_some() {
        return;
    }
    for &child in &doc.get(scope).children {
        if found.is_some() {
            return;
        }
        if !is_element(doc, child) {
            continue;
        }
        let NodeData::Element { name, .. } = &doc.get(child).data else {
            continue;
        };
        let tag = name.local.as_str();
        if tag == "input" {
            let t = input_type_lower(doc, child);
            if t == "submit" || t == "image" {
                *found = Some(child);
                return;
            }
        } else if tag == "button" {
            let t = doc
                .get(child)
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t == "submit" {
                *found = Some(child);
                return;
            }
        }
        walk_first_submit(doc, child, found);
    }
}

/// Ближайший `<form>`-предок (или сам node, если он `<form>`). None — нет.
fn nearest_form(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { name, .. } = &doc.get(n).data
            && name.local.as_str() == "form"
        {
            return Some(n);
        }
        cur = doc.get(n).parent;
    }
    None
}

/// Ближайший `<form>`-предок или корень документа — scope-для-обхода
/// radio-группы. Возвращает корень документа если предка `<form>` нет.
fn nearest_form_or_root(doc: &Document, node: NodeId) -> NodeId {
    nearest_form(doc, node).unwrap_or_else(|| doc.root())
}

/// `:lang(<tag>#)` (CSS Selectors L4 §11). Элемент матчит, если его
/// content-language matches хотя бы один из tag-ов в списке по RFC 4647
/// §3.3.1 «basic filtering»: range matches tag, если range — exact equal
/// или range — proper prefix tag с границей по `-`. То есть `:lang(en)`
/// matches `lang="en"`, `lang="en-US"`, `lang="en-Latn-GB"`, но не
/// `lang="english"` и не `lang="fr-en"` (последний — `fr` + `en` — `en`
/// здесь регион/вариант, не language).
///
/// Content-language определяется через ближайший `lang` или `xml:lang`
/// атрибут вверх по дереву (HTML5 §3.2.6 «inheritance»; xml:lang —
/// исторически из XHTML, до сих пор используется в реальных страницах).
/// Если ни один ancestor не имеет `lang`, элемент не имеет языка и не
/// матчит ни один tag — кроме пустого `*` (Selectors L4 расширение пока
/// не поддерживается).
fn matches_lang(doc: &Document, node: NodeId, tags: &[String]) -> bool {
    let Some(content_lang) = element_lang(doc, node) else {
        return false;
    };
    let content_lc = content_lang.to_ascii_lowercase();
    tags.iter().any(|range| lang_range_matches(range, &content_lc))
}

/// Определяет content-language элемента, walking up ancestors. Сначала
/// `lang`, потом `xml:lang` на том же узле; затем родитель, и так далее.
/// Возвращает None если ни у кого нет атрибута либо найденное значение —
/// пустая строка (HTML5: `lang=""` — «явно неизвестен», не наследует от
/// предков — Phase 0 трактует как «нет языка»).
fn element_lang(doc: &Document, node: NodeId) -> Option<String> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data {
            let nr = doc.get(n);
            if let Some(v) = nr.get_attr("lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
            if let Some(v) = nr.get_attr("xml:lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
        }
        cur = doc.get(n).parent;
    }
    None
}

/// RFC 4647 §3.3.1 «basic filtering»: language range matches language tag,
/// если range — case-insensitive prefix tag с границей по `-` или концом
/// строки. Обе стороны уже ожидаются в lowercase.
fn lang_range_matches(range_lc: &str, tag_lc: &str) -> bool {
    if range_lc == tag_lc {
        return true;
    }
    if let Some(rest) = tag_lc.strip_prefix(range_lc) {
        return rest.starts_with('-');
    }
    false
}

/// `:any-link` / `:link` (CSS Selectors L4 §6.2.1 / §6.2.2, HTML5 §4.6).
/// Hyperlinks в HTML: `<a>`, `<area>`, `<link>` элементы с **непустым**
/// `href`-атрибутом (HTML5 §4.6.1 — hyperlink требует non-empty href; пустой
/// href трактуется как ссылка на текущий документ и формально валиден, но
/// все mainstream браузеры считают такой элемент hyperlink-ом — мы тоже).
/// Spec различает hyperlink (`href` присутствует) от non-hyperlink (no href),
/// последний не матчит ни `:link`, ни `:visited`, ни `:any-link`.
fn matches_any_link(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !matches!(tag, "a" | "area" | "link") {
        return false;
    }
    node_ref.get_attr("href").is_some()
}

/// `:target` matcher (CSS Selectors L4 §9.6). Возвращает true, если у элемента
/// есть `id`-атрибут, равный текущему `Document::target()` (URL fragment без
/// `:in-range` / `:out-of-range` (CSS Selectors L4 §14.5, HTML5 §4.10.21.4).
///
/// Возвращает `Some(true)` если value в [min, max], `Some(false)` если вне,
/// `None` если у элемента нет range-limitations или нет displayed value.
/// Phase 0: поддерживаются только `type=number` и `type=range`.
fn matches_in_range(doc: &Document, node: NodeId) -> Option<bool> {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return None;
    };
    if name.local.as_str() != "input" {
        return None;
    }
    let t = input_type_lower(doc, node);
    let supports_numeric = matches!(t.as_str(), "number" | "range");
    if !supports_numeric {
        return None;
    }

    let min_attr = node_ref.get_attr("min").and_then(parse_html_number);
    let max_attr = node_ref.get_attr("max").and_then(parse_html_number);

    let (min, max) = match t.as_str() {
        "range" => (min_attr.unwrap_or(0.0), max_attr.unwrap_or(100.0)),
        _ => {
            if min_attr.is_none() && max_attr.is_none() {
                return None;
            }
            (min_attr.unwrap_or(f64::NEG_INFINITY), max_attr.unwrap_or(f64::INFINITY))
        }
    };

    let value = match node_ref.get_attr("value").and_then(parse_html_number) {
        Some(v) => v,
        None => {
            if t == "range" {
                // Spec §4.10.5.1.13: default value = min + (max-min)/2, clamped.
                let mid = min + (max - min) / 2.0;
                mid.clamp(min, max)
            } else {
                return None;
            }
        }
    };

    Some(value >= min && value <= max)
}

/// Парсит HTML5 «valid floating-point number» (§2.5.5).
/// Отбрасывает leading `+`, NaN и ±∞ (не допускаются spec-ом).
fn parse_html_number(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.starts_with('+') {
        return None;
    }
    let v: f64 = trimmed.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// `#`). Comparison case-sensitive — HTML id case-sensitive per HTML LS §3.2.6.
/// Текстовые узлы и не-element-узлы не матчат.
fn matches_target(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    let node_ref = doc.get(node);
    if !matches!(&node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    node_ref.get_attr("id") == Some(target)
}

/// `:target-within` matcher (CSS Selectors L4 §9.7). Element matches if it
/// itself is `:target`, OR has any descendant element matching `:target`.
/// Short-circuits на `Document::target() == None` (нет fragment-а — никто не
/// матчит, сэкономим обход поддерева).
fn matches_target_within(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    if !is_element(doc, node) {
        return false;
    }
    if doc.get(node).get_attr("id") == Some(target) {
        return true;
    }
    any_descendant(doc, node, |n| doc.get(n).get_attr("id") == Some(target))
}

/// `:dir(ltr|rtl)` (CSS Selectors L4 §13.2). Матчит элемент с
/// соответствующей directionality, определяемой через `dir`-атрибут
/// (с inherited fallback от ближайшего ancestor-а). При отсутствии
/// `dir` нигде в цепочке — default `ltr` (HTML5 §3.2.6.1).
fn matches_dir(doc: &Document, node: NodeId, want: DirArg) -> bool {
    element_directionality(doc, node) == want
}

/// Computes content-directionality элемента по HTML5 §3.2.6.1
/// «directionality»: значение `dir`-атрибута самого элемента, либо
/// унаследовано от ближайшего ancestor с `dir`-атрибутом. Default `ltr`.
///
/// Phase 0 не реализует real auto-direction (UAX #9 first-strong scan по
/// текстовому содержимому для `<bdi>` и `dir="auto"`) — оба трактуются
/// как `ltr`, что соответствует поведению типичных страниц на латинице.
/// Real bidi откладывается до layout-bidi движка (см. lumen-layout `Отложено`).
fn element_directionality(doc: &Document, node: NodeId) -> DirArg {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data
            && let Some(v) = doc.get(n).get_attr("dir")
        {
            return match v.trim().to_ascii_lowercase().as_str() {
                "ltr" => DirArg::Ltr,
                "rtl" => DirArg::Rtl,
                // `auto` и любое другое значение — Phase 0 fallback to ltr;
                // продолжаем walking up НЕ нужно: spec говорит, что
                // `dir` атрибут на самом элементе финализирует
                // directionality (`auto` тоже считается «явным»).
                _ => DirArg::Ltr,
            };
        }
        cur = doc.get(n).parent;
    }
    DirArg::Ltr
}

/// Проверка: у узла есть хоть один text-ребёнок с непустым содержимым
/// (после whitespace-trim). Нужно для `<textarea>` чьё «значение» — это
/// его текстовый контент в DOM (HTML5 §4.10.11), а не `value`-атрибут.
fn has_non_whitespace_text(doc: &Document, node: NodeId) -> bool {
    for &child in &doc.get(node).children {
        if let NodeData::Text(t) = &doc.get(child).data
            && !t.trim().is_empty()
        {
            return true;
        }
    }
    false
}

/// Проверяет, что хоть один кандидат относительно `scope` (в зависимости от
/// combinator-а) удовлетворяет внутреннему selector-у.
fn matches_relative(rs: &lumen_css_parser::RelativeSelector, doc: &Document, scope: NodeId) -> bool {
    match rs.combinator {
        // Implicit descendant — обходим всё поддерево scope.
        None => any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n)),
        Some(Combinator::Child) => {
            // Прямые element-children scope.
            doc.get(scope).children.iter().any(|&c| {
                is_element(doc, c) && matches_complex(&rs.selector, doc, c)
            })
        }
        Some(Combinator::NextSibling) => {
            // Прямой следующий element-sibling.
            next_element_sibling(doc, scope)
                .map(|n| matches_complex(&rs.selector, doc, n))
                .unwrap_or(false)
        }
        Some(Combinator::LaterSibling) => {
            // Любой последующий element-sibling.
            let mut cur = next_element_sibling(doc, scope);
            while let Some(n) = cur {
                if matches_complex(&rs.selector, doc, n) {
                    return true;
                }
                cur = next_element_sibling(doc, n);
            }
            false
        }
        // Descendant как explicit combinator — то же что None.
        Some(Combinator::Descendant) => {
            any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n))
        }
    }
}

/// True если хоть один element-descendant `root` удовлетворяет `pred`. Сам
/// `root` не проверяется — только потомки (по spec :has() ищет среди
/// descendants, не включая E).
fn any_descendant<F: Fn(NodeId) -> bool>(doc: &Document, root: NodeId, pred: F) -> bool {
    fn walk<F: Fn(NodeId) -> bool>(doc: &Document, n: NodeId, pred: &F) -> bool {
        for &c in &doc.get(n).children {
            if is_element(doc, c) && pred(c) {
                return true;
            }
            if walk(doc, c, pred) {
                return true;
            }
        }
        false
    }
    walk(doc, root, &pred)
}

fn next_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[idx + 1..].iter().copied().find(|&id| is_element(doc, id))
}

/// 1-based индекс элемента среди element-sibling-ов. Если `from_end` —
/// считаем с конца. None — если узел не элемент или нет родителя.
fn element_index(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    if !is_element(doc, node) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов, удовлетворяющих опциональному
/// `of <selector-list>` фильтру (CSS Selectors L4 §6.6.5.1). При `of=None`
/// эквивалент `element_index` (все element-sibling-ы). При `of=Some(list)`:
/// сначала проверяем, что сам узел матчит хотя бы один из селекторов
/// списка — иначе `:nth-child(... of S)` не применим, возвращаем None;
/// затем считаем index среди siblings, удовлетворяющих тому же list-у.
fn element_index_filtered(
    doc: &Document,
    node: NodeId,
    from_end: bool,
    of: Option<&[ComplexSelector]>,
) -> Option<i32> {
    let Some(list) = of else {
        return element_index(doc, node, from_end);
    };
    if !is_element(doc, node) {
        return None;
    }
    // Сам элемент должен матчить хотя бы один селектор list-а — иначе
    // `:nth-child(an+b of S)` к нему вообще не применяется.
    if !list.iter().any(|s| matches_complex(s, doc, node)) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        if !list.iter().any(|s| matches_complex(s, doc, id)) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов **того же тега**.
fn element_index_of_type(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    let self_name = match &doc.get(node).data {
        NodeData::Element { name, .. } => name,
        _ => return None,
    };
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        let same_type = matches!(
            &doc.get(id).data,
            NodeData::Element { name, .. } if name == self_name
        );
        if !same_type {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

fn is_first_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, false) == Some(1)
}

fn is_last_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, true) == Some(1)
}

// ──────────────── DOM-traversal хелперы ────────────────

fn is_element(doc: &Document, node: NodeId) -> bool {
    matches!(doc.get(node).data, NodeData::Element { .. })
}

fn previous_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[..idx]
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
}

fn is_first_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_last_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_empty_element(doc: &Document, node: NodeId) -> bool {
    // `:empty` — нет ни элементов-детей, ни текстовых узлов с непустым контентом.
    doc.get(node).children.iter().all(|&cid| {
        matches!(
            doc.get(cid).data,
            NodeData::Comment(_) | NodeData::Doctype { .. }
        ) || matches!(&doc.get(cid).data, NodeData::Text(t) if t.is_empty())
    })
}

fn is_root_element(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    matches!(doc.get(parent).data, NodeData::Document)
}

// ──────────────── default display / declarations ────────────────

fn default_display(doc: &Document, node: NodeId) -> Display {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return Display::Block;
    };
    match name.local.as_str() {
        // <head> и его метаданные никогда не рендерятся как видимый контент.
        // `<source>` и `<track>` — child-кандидаты `<picture>` / `<video>` /
        // `<audio>`; реальное визуальное представление даёт inner `<img>`
        // (резолвится `pick_picture_source`) или сам media-элемент. Сами
        // эти теги в DOM есть, но layout-бокса не порождают.
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript"
        | "source" | "track" => Display::None,
        // Inline-уровневые элементы. Phase 0: пока трактуем как block — текст
        // внутри `<a>`/`<span>` будет на своей строке. Это известное ограничение.
        "a" | "span" | "b" | "i" | "em" | "strong" | "code" | "small" | "sub" | "sup"
        | "label" | "abbr" | "cite" | "q" | "mark" | "u" => Display::Inline,
        _ => Display::Block,
    }
}

/// Разбивает строку на куски по запятым, не пересекая `(...)` (для
/// shadow-list, где цвет может быть `rgba(0, 0, 0, 0.5)` с запятыми).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Парсит одну box-shadow спецификацию. Формат:
/// `[inset]? <length>{2,4} <color>?` — токены произвольно перемешаны.
fn parse_box_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<BoxShadow> {
    // Сложность: цветовые функции (`rgba(...)`) содержат пробелы — наивный
    // split_whitespace их разорвёт. Восстанавливаем токены, балансируя `()`.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut inset = false;
    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    // Должно быть 2-4 длины (offset-x, offset-y, blur?, spread?).
    let (offset_x, offset_y, blur, spread) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0, 0.0),
        [x, y, b] => (*x, *y, *b, 0.0),
        [x, y, b, sp] => (*x, *y, *b, *sp),
        _ => return None,
    };

    Some(BoxShadow { offset_x, offset_y, blur, spread, color, inset })
}

/// Парсит одну text-shadow спецификацию. Формат:
/// `<length>{2,3} <color>?` (без inset, без spread).
fn parse_text_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<TextShadow> {
    // Тот же tokenization-трюк, что у box-shadow — балансируем `()`,
    // чтобы цветовые функции не разрывались.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    let (offset_x, offset_y, blur) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0),
        [x, y, b] => (*x, *y, *b),
        _ => return None,
    };

    Some(TextShadow { offset_x, offset_y, blur, color })
}

/// CSS UI L4 §8.1: парсит keyword в `Cursor`. None = неизвестное.
fn parse_cursor_kw(s: &str) -> Option<Cursor> {
    Some(match s {
        "auto" => Cursor::Auto,
        "default" => Cursor::Default,
        "none" => Cursor::None,
        "context-menu" => Cursor::ContextMenu,
        "help" => Cursor::Help,
        "pointer" => Cursor::Pointer,
        "progress" => Cursor::Progress,
        "wait" => Cursor::Wait,
        "cell" => Cursor::Cell,
        "crosshair" => Cursor::Crosshair,
        "text" => Cursor::Text,
        "vertical-text" => Cursor::VerticalText,
        "alias" => Cursor::Alias,
        "copy" => Cursor::Copy,
        "move" => Cursor::Move,
        "no-drop" => Cursor::NoDrop,
        "not-allowed" => Cursor::NotAllowed,
        "grab" => Cursor::Grab,
        "grabbing" => Cursor::Grabbing,
        "all-scroll" => Cursor::AllScroll,
        "col-resize" => Cursor::ColResize,
        "row-resize" => Cursor::RowResize,
        "n-resize" => Cursor::NResize,
        "e-resize" => Cursor::EResize,
        "s-resize" => Cursor::SResize,
        "w-resize" => Cursor::WResize,
        "ne-resize" => Cursor::NeResize,
        "nw-resize" => Cursor::NwResize,
        "se-resize" => Cursor::SeResize,
        "sw-resize" => Cursor::SwResize,
        "ew-resize" => Cursor::EwResize,
        "ns-resize" => Cursor::NsResize,
        "nesw-resize" => Cursor::NeswResize,
        "nwse-resize" => Cursor::NwseResize,
        "zoom-in" => Cursor::ZoomIn,
        "zoom-out" => Cursor::ZoomOut,
        _ => return None,
    })
}

/// CSS Overflow L3: парсит keyword в `Overflow`. None = неизвестное.
fn parse_overflow_kw(s: &str) -> Option<Overflow> {
    match s {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}

/// Эмулирует UA stylesheet для font-style: HTML §15.3.3 рекомендует italic
/// для `<em>` / `<i>` / `<cite>` / `<dfn>` / `<address>` / `<var>`. Возвращает
/// `Some(Italic)` для них, `None` для остальных (= наследовать как обычно).
fn ua_font_style(doc: &Document, node: NodeId) -> Option<FontStyle> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "em" | "i" | "cite" | "dfn" | "address" | "var" => Some(FontStyle::Italic),
        _ => None,
    }
}

/// Применяет HTML presentational hints для `<img>`: атрибуты `width` и
/// `height` парсятся как unitless целые пиксели и пишутся в `style.width` /
/// `style.height`. Любое author-CSS правило в каскаде ниже перекроет
/// атрибут — это и есть смысл «presentational hint»: атрибут эквивалентен
/// UA-стилю с specificity 0, который проигрывает любой author-декларации
/// (HTML5 §10 «Mapped attributes»). Невалидные значения (отрицательные,
/// нечисловые, с единицами) HTML5 spec предписывает игнорировать.
fn apply_image_presentational_hints(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local != "img" {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(w) = node_ref.get_attr("width").and_then(parse_html_dimension) {
        style.width = Some(w);
    }
    if let Some(h) = node_ref.get_attr("height").and_then(parse_html_dimension) {
        style.height = Some(h);
    }
}

/// HTML5 §15: `bgcolor` атрибут на `<body>` / table-related элементах
/// мапается на `background-color` (presentational hint). Парсится через
/// HTML5 §2.4.6 «rules for parsing a legacy color value». Любое author-CSS
/// правило в каскаде ниже перекроет hint — так и устроена presentational
/// hint конструкция.
///
/// Список тегов взят из HTML5 §15.3.6 (`<body>`) и §15.3.8 (table-tree).
/// Phase 0 ещё не делает табличный layout — но bgcolor попадает в
/// `style.background_color` всё равно, чтобы при появлении table-layout
/// рендеринг сразу работал.
fn apply_bgcolor_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "body" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("bgcolor")
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.background_color = Some(c);
    }
}

/// HTML5 §15.3.6 «The page» (для `<body text>`) + §15.3.2 «Phrasing
/// content» (для `<font color>`): мапает legacy-атрибуты на CSS `color`.
///
/// - `<body text="…">` → `body.color`. Через CSS-наследование цвет
///   распространяется на всех потомков, у которых нет явного `color`.
/// - `<font color="…">` → элементный `color`. Атрибут применим к любому
///   элементу с именем `font`, в т.ч. внутри других элементов.
///
/// `<body link/vlink/alink>` отложены: hyperlink coloring требует UA
/// stylesheet с descendant-селектором (`body :link { color: … }`), а в
/// Phase 0 без visited/active runtime два из трёх атрибутов всё равно
/// были бы no-op.
///
/// Парсинг — `parse_legacy_color_html_attr` (HTML5 §2.4.6). Hint
/// применяется ДО CSS-каскада, поэтому любое author-CSS правило
/// перекроет атрибут.
fn apply_text_color_presentational_hint(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    let node_ref = doc.get(node);
    let attr_name = match tag {
        "body" => "text",
        "font" => "color",
        _ => return,
    };
    if let Some(val) = node_ref.get_attr(attr_name)
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.color = c;
    }
}

/// HTML5 §2.4.6 «rules for parsing a legacy color value».
///
/// Используется для presentational hint-атрибутов вроде `<body bgcolor>`,
/// `<td bgcolor>`, `<body text>`, `<font color>`. Алгоритм значительно
/// лояльнее CSS-парсера: принимает named colors, `#rgb` / `#rrggbb`,
/// hashless hex произвольной длины, и через padding/truncate process
/// выдаёт цвет из почти любой непустой строки, отличной от
/// «transparent».
///
/// Отказы (Spec: «error»):
/// - пустая строка / только whitespace;
/// - ASCII case-insensitive match «transparent».
///
/// Все остальные строки возвращают непустой цвет — это нужно для
/// совместимости с legacy-разметкой, где атрибуты часто содержат мусор.
///
/// Реализация работает в `Vec<char>` (Unicode code points), как требует
/// spec — не в байтах. Не-BMP code-point (> U+FFFF) заменяется на две
/// ASCII-«0» (spec step 6).
fn parse_legacy_color_html_attr(input: &str) -> Option<Color> {
    // Step 1-2: empty → error.
    if input.is_empty() {
        return None;
    }
    // Step 3: strip leading/trailing ASCII whitespace.
    let trimmed = input.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' '));
    if trimmed.is_empty() {
        return None;
    }
    // Step 4: case-insensitive «transparent» → error.
    if trimmed.eq_ignore_ascii_case("transparent") {
        return None;
    }
    // Step 5: named X11 / CSS3 color.
    let lc = trimmed.to_ascii_lowercase();
    // `named_color` принимает уже-lc имя и для «transparent» вернул бы
    // TRANSPARENT-константу — но мы уже отказали выше, так что попадание
    // невозможно.
    if let Some(c) = named_color(&lc) {
        return Some(c);
    }
    // Step 6: special-case 4-char `#xyz` short hex.
    let bytes = trimmed.as_bytes();
    if trimmed.len() == 4
        && bytes[0] == b'#'
        && bytes[1].is_ascii_hexdigit()
        && bytes[2].is_ascii_hexdigit()
        && bytes[3].is_ascii_hexdigit()
    {
        let r = hex_digit_value(bytes[1]) * 17;
        let g = hex_digit_value(bytes[2]) * 17;
        let b = hex_digit_value(bytes[3]) * 17;
        return Some(Color { r, g, b, a: 255 });
    }
    // Step 7: replace non-BMP code-points с двумя «0»; затем truncate до 128.
    let mut chars: Vec<char> = Vec::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if (c as u32) > 0xFFFF {
            chars.push('0');
            chars.push('0');
        } else {
            chars.push(c);
        }
    }
    if chars.len() > 128 {
        chars.truncate(128);
    }
    // Step 8: leading `#` удаляется.
    if !chars.is_empty() && chars[0] == '#' {
        chars.remove(0);
    }
    // Step 9: не-hex-digits заменяются на «0».
    for c in &mut chars {
        if !c.is_ascii_hexdigit() {
            *c = '0';
        }
    }
    // Step 10: padding нулями до длины > 0 и multiple of 3.
    while chars.is_empty() || !chars.len().is_multiple_of(3) {
        chars.push('0');
    }
    // Step 11: split на три равных компонента.
    let mut length = chars.len() / 3;
    let mut red: Vec<char> = chars[0..length].to_vec();
    let mut green: Vec<char> = chars[length..length * 2].to_vec();
    let mut blue: Vec<char> = chars[length * 2..length * 3].to_vec();
    // Step 12: если length > 8, оставляем только последние 8 (срезаем leading).
    if length > 8 {
        let skip = length - 8;
        red.drain(0..skip);
        green.drain(0..skip);
        blue.drain(0..skip);
        length = 8;
    }
    // Step 13: пока length > 2 и у всех трёх компонентов лидирующий «0» —
    // удаляем по «0» из каждого. Это «strip common leading zeros».
    while length > 2 && red[0] == '0' && green[0] == '0' && blue[0] == '0' {
        red.remove(0);
        green.remove(0);
        blue.remove(0);
        length -= 1;
    }
    // Step 14: если length всё ещё > 2, оставляем только первые 2.
    if length > 2 {
        red.truncate(2);
        green.truncate(2);
        blue.truncate(2);
    }
    // Step 15-19: parse hex.
    let r = u8::from_str_radix(&red.iter().collect::<String>(), 16).ok()?;
    let g = u8::from_str_radix(&green.iter().collect::<String>(), 16).ok()?;
    let b = u8::from_str_radix(&blue.iter().collect::<String>(), 16).ok()?;
    Some(Color { r, g, b, a: 255 })
}

/// Значение ASCII hex-digit как 0..=15. Caller гарантирует
/// `is_ascii_hexdigit()` — иначе возвращает 0.
fn hex_digit_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// HTML5 «rules for parsing dimension values»: unitless целое число
/// пикселей, опциональный trailing `%` (Phase 0 пропускаем процентный
/// случай — нужен containing-block-width). Отрицательные значения
/// невалидны.
fn parse_html_dimension(s: &str) -> Option<f32> {
    let s = s.trim();
    // Процентные размеры пока не поддерживаем — требуют containing block.
    if s.ends_with('%') {
        return None;
    }
    // Берём префикс из цифр (HTML5 принимает мусор после), парсим как u32.
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok().map(|n| n as f32)
}

/// CSS Quirks Mode — UA-rule только для Quirks-mode: элемент `<table>`
/// сбрасывает font / color / text-align / white-space-related свойства
/// к initial-values, не наследует от родителя. Эквивалент UA-stylesheet
/// правила (как в Chromium / Firefox / WebKit):
///
/// ```css
/// table {
///     font-size: medium;
///     font-weight: normal;
///     font-style: normal;
///     font-variant: normal;
///     line-height: normal;
///     color: -webkit-text;
///     text-align: -webkit-auto;
///     white-space: normal;
///     font-family: -webkit-default;
/// }
/// ```
///
/// Эффект: classics 90-х/2000-х с `<body style="font: 20px serif; color:
/// blue">` + table-layout не «протекают» в таблицу — таблица отрисовывается
/// дефолтным шрифтом / цветом. В Standards / LimitedQuirks таблица
/// наследует обычно. Author CSS поверх Quirks-reset выигрывает: spec
/// §UA-stylesheet — это самый низкий cascade origin.
fn apply_quirks_table_reset(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "table" {
        return;
    }
    style.font_size = ROOT_FONT_SIZE;
    style.line_height = 1.2;
    style.font_family = Vec::new();
    style.font_style = FontStyle::Normal;
    style.font_variant = FontVariant::Normal;
    style.font_weight = FontWeight::NORMAL;
    style.font_stretch = FontStretch::NORMAL;
    style.color = Color::BLACK;
    style.text_align = TextAlign::Left;
    style.white_space = WhiteSpace::Normal;
}

/// CSS Quirks Mode §3.2: в quirks-mode replaced-элементы получают UA-правило
/// `line-height: 1`, которое блокирует наследование «normal» и убирает зазор
/// под `<img>` в inline-контексте (так делал IE7). Author CSS поверх — выигрывает.
fn apply_quirks_line_height(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if matches!(
        name.local.as_str(),
        "img" | "video" | "canvas" | "embed" | "object"
            | "iframe" | "input" | "textarea" | "select" | "audio"
    ) {
        style.line_height = 1.0;
    }
}

/// UA stylesheet для font-weight: `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>`
/// получают bold по умолчанию (HTML §15.3.3).
fn ua_font_weight(doc: &Document, node: NodeId) -> Option<FontWeight> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "b" | "strong" | "th" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            Some(FontWeight::BOLD)
        }
        _ => None,
    }
}

/// Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
/// семейства; кавычки (одинарные или двойные) обрамляют имя с пробелами.
/// Имена без кавычек: один или несколько whitespace-разделённых
/// идентификаторов сливаются в одну строку с одним пробелом
/// (`Times New Roman` → `"Times New Roman"`). Пустые имена пропускаются.
pub fn parse_font_family(val: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = val.chars().peekable();
    while chars.peek().is_some() {
        // Пропускаем ведущий whitespace и запятые.
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&first) = chars.peek() else { break };
        let name = if first == '"' || first == '\'' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == first { break; }
                s.push(c);
            }
            // Пропускаем до следующей запятой / EOF.
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
            }
            s
        } else {
            // Unquoted: собираем до запятой, схлопывая whitespace в один пробел.
            let mut s = String::new();
            let mut prev_space = false;
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
                if c.is_whitespace() {
                    if !s.is_empty() && !prev_space {
                        s.push(' ');
                        prev_space = true;
                    }
                } else {
                    s.push(c);
                    prev_space = false;
                }
            }
            // Trim trailing space.
            while s.ends_with(' ') {
                s.pop();
            }
            s
        };
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}

/// Парсит CSS `font-weight`. Поддерживает:
///   - `normal` → 400, `bold` → 700;
///   - численные `100`..`900` (или любое число 1..1000 — Variable Fonts);
///   - относительные `lighter` / `bolder` — резолвятся относительно `parent`
///     по таблице из CSS Fonts L4 §2.4.3.
fn parse_font_weight(val: &str, parent: FontWeight) -> Option<FontWeight> {
    match val.trim() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        "lighter" => Some(relative_lighter(parent)),
        "bolder" => Some(relative_bolder(parent)),
        s => s.parse::<u16>().ok().filter(|&n| (1..=1000).contains(&n)).map(FontWeight),
    }
}

/// CSS Fonts L4 §2.4.3 таблица для `lighter`. Сужаем weight в сторону normal.
fn relative_lighter(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        100..=349 => 100,
        350..=549 => 100,
        550..=749 => 400,
        _ => 700, // 750..=1000
    })
}

/// CSS Fonts L4 §2.4.3 таблица для `bolder`.
fn relative_bolder(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        0..=349 => 400,
        350..=549 => 700,
        550..=749 => 900,
        _ => 900,
    })
}

/// Корневой font-size в CSS — 16px на момент Phase 0 (без `<html>`-стилей и
/// настроек пользователя). Используется как базис для `rem`.
pub const ROOT_FONT_SIZE: f32 = 16.0;

/// Типизированная длина CSS до резолва в пиксели.
///
/// Не `Copy`, потому что вариант `Calc` хранит `Box<CalcNode>` с поддеревом
/// выражения. Использования полагались только на `Clone` / match-pattern-ы,
/// где `v` копируется как `f32`, а не `len` как `Length`.
#[derive(Debug, Clone, PartialEq)]
pub enum Length {
    Px(f32),
    /// `em` — относительно font-size текущего/родительского элемента
    /// (для свойства `font-size` — родительского, для остального — текущего).
    Em(f32),
    /// `rem` — относительно font-size корня документа (ROOT_FONT_SIZE).
    Rem(f32),
    /// `%` — процент. Базис зависит от свойства: для `font-size` это
    /// `em_basis`, для `line-height` — текущий font-size, для
    /// margin/padding/width — containing block width (Phase 0 пока не считает,
    /// нужны honest contain blocks; до тех пор `%` в margin/padding
    /// игнорируется).
    Percent(f32),
    /// `vh` — 1% от высоты viewport (CSS Values L3 §6.1.2).
    Vh(f32),
    /// `vw` — 1% от ширины viewport.
    Vw(f32),
    /// `vmin` — 1% от меньшей из двух сторон viewport.
    Vmin(f32),
    /// `vmax` — 1% от большей из двух сторон viewport.
    Vmax(f32),
    /// CSS Values L4 §10 — `calc()` выражение. Резолвится через
    /// `CalcNode::resolve`, который рекурсивно вычисляет поддерево
    /// в `f32`-пикселях, используя те же `em_basis` / `percent_basis` /
    /// `viewport`, что и обычный `Length`.
    Calc(Box<CalcNode>),
}

/// CSS Values L4 §10 — AST `calc()`-выражения. Хранится как двоичное дерево
/// (`Add`/`Sub`/`Mul`/`Div`) с листовыми `Length` и unitless `Number`.
/// `Number` нужен для умножения / деления, где спецификация требует, чтобы
/// один операнд был unitless. В Phase 0 мы не валидируем строго типы
/// операндов (`px * px` математически считается, но семантически бессмысленно
/// — реальный CSS такого не пишет, а наш resolve всё равно даёт `f32`).
#[derive(Debug, Clone, PartialEq)]
pub enum CalcNode {
    /// Листовое length-значение (`10px`, `2em`, `50%`, …).
    Length(Length),
    /// Unitless число (например `2` в `calc(2 * 10px)`). Для углов
    /// (`45deg`, `1turn`) лексер тоже даёт Number — конвертирует в радианы
    /// сразу при чтении.
    Number(f32),
    Add(Box<CalcNode>, Box<CalcNode>),
    Sub(Box<CalcNode>, Box<CalcNode>),
    Mul(Box<CalcNode>, Box<CalcNode>),
    Div(Box<CalcNode>, Box<CalcNode>),
    /// CSS Values L4 §10.6.1 — `min(a, b, ...)`. Минимум по списку.
    Min(Vec<CalcNode>),
    /// CSS Values L4 §10.6.2 — `max(a, b, ...)`. Максимум по списку.
    Max(Vec<CalcNode>),
    /// CSS Values L4 §10.6.3 — `clamp(min, val, max)`. Эквивалентно
    /// `max(min, min(val, max))`. Если `min > max` — побеждает `min`.
    Clamp(Box<CalcNode>, Box<CalcNode>, Box<CalcNode>),
    /// CSS Values L4 §10.7-10.9 — научные math-функции: тригонометрия
    /// (`sin/cos/tan/asin/acos/atan/atan2`), экспоненциальные
    /// (`pow/sqrt/exp/log/hypot`), signs/stepping (`abs/sign/mod/rem/round`).
    /// Все 15 функций унифицированы под `Func(MathFn, args)`: арность
    /// и формула — внутри `resolve` по match-у на MathFn.
    Func(MathFn, Vec<CalcNode>),
}

/// CSS Values L4 §10.7-10.9 — научные math-функции. Имена case-insensitive
/// (нормализованы в нижний регистр в лексере).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFn {
    // §10.7 trig
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    // §10.8 exponential
    Pow,
    Sqrt,
    Exp,
    Log,
    Hypot,
    // §10.9 sign/stepping
    Abs,
    Sign,
    Mod,
    Rem,
    /// CSS Values L4 §10.5.1 — `round( <rounding-strategy>?, A, B? )`.
    /// Strategy keyword вычисляется парсером и зашит в variant; отсутствие
    /// keyword-а ≡ `Nearest`.
    Round(RoundStrategy),
}

/// CSS Values L4 §10.5.1 — стратегия округления для `round()`.
/// Опускание keyword-а в `round(A[, B])` ≡ `Nearest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStrategy {
    /// Ближайшее кратное step; при равноудалённости — в сторону +∞
    /// (`f32::round` round-half-away-from-zero, но spec в §10.5.1 говорит
    /// «toward +∞»; различие незаметно для положительного step и нечастых
    /// граничных случаев).
    Nearest,
    /// Меньшее или равное кратное step, всегда в сторону +∞
    /// (`ceil(A/B) * B`).
    Up,
    /// Большее или равное кратное step, всегда в сторону −∞
    /// (`floor(A/B) * B`).
    Down,
    /// Округление к нулю (`trunc(A/B) * B`). Для положительных A совпадает
    /// с `Down`, для отрицательных — с `Up`.
    ToZero,
}

impl CalcNode {
    /// Резолвит выражение в `f32`-пиксели по тем же правилам, что
    /// `Length::resolve`. Возвращает `None` если:
    ///   - хотя бы один листовой `Length::Percent` не имеет `percent_basis`
    ///     (контекст не задан);
    ///   - деление на 0;
    ///   - пустой список аргументов в `min()` / `max()`.
    pub fn resolve(
        &self,
        em_basis: f32,
        percent_basis: Option<f32>,
        viewport: Size,
    ) -> Option<f32> {
        match self {
            CalcNode::Length(l) => l.resolve(em_basis, percent_basis, viewport),
            CalcNode::Number(n) => Some(*n),
            CalcNode::Add(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    + b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Sub(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    - b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Mul(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    * b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Div(a, b) => {
                let denom = b.resolve(em_basis, percent_basis, viewport)?;
                if denom == 0.0 {
                    return None;
                }
                Some(a.resolve(em_basis, percent_basis, viewport)? / denom)
            }
            CalcNode::Min(args) => {
                if args.is_empty() {
                    return None;
                }
                let mut acc = args[0].resolve(em_basis, percent_basis, viewport)?;
                for n in &args[1..] {
                    let v = n.resolve(em_basis, percent_basis, viewport)?;
                    if v < acc {
                        acc = v;
                    }
                }
                Some(acc)
            }
            CalcNode::Max(args) => {
                if args.is_empty() {
                    return None;
                }
                let mut acc = args[0].resolve(em_basis, percent_basis, viewport)?;
                for n in &args[1..] {
                    let v = n.resolve(em_basis, percent_basis, viewport)?;
                    if v > acc {
                        acc = v;
                    }
                }
                Some(acc)
            }
            CalcNode::Clamp(min, val, max) => {
                let mn = min.resolve(em_basis, percent_basis, viewport)?;
                let v = val.resolve(em_basis, percent_basis, viewport)?;
                let mx = max.resolve(em_basis, percent_basis, viewport)?;
                // CSS Values L4 §10.6.3: clamp(min, val, max) ≡
                // max(min, min(val, max)). При min > max побеждает min.
                let inner = if v < mx { v } else { mx };
                Some(if mn > inner { mn } else { inner })
            }
            CalcNode::Func(func, args) => {
                resolve_math_func(*func, args, em_basis, percent_basis, viewport)
            }
        }
    }
}

/// Резолвит научную math-функцию. Валидация арности уже сделана парсером —
/// здесь предполагаем правильное число аргументов. Все вычисления делаются
/// в `f64` для точности (особенно для trig / log), результат сужается до
/// `f32`. Возвращает None если резолв одного из аргументов даёт None
/// (например, `%` без containing block) или результат не конечный
/// (`sqrt(-1)`, `log(0)`, `1.0 / 0.0` и т.п.).
fn resolve_math_func(
    func: MathFn,
    args: &[CalcNode],
    em_basis: f32,
    percent_basis: Option<f32>,
    viewport: Size,
) -> Option<f32> {
    let resolve = |n: &CalcNode| -> Option<f64> {
        n.resolve(em_basis, percent_basis, viewport).map(f64::from)
    };
    let result: f64 = match func {
        MathFn::Sin => resolve(&args[0])?.sin(),
        MathFn::Cos => resolve(&args[0])?.cos(),
        MathFn::Tan => resolve(&args[0])?.tan(),
        MathFn::Asin => resolve(&args[0])?.asin(),
        MathFn::Acos => resolve(&args[0])?.acos(),
        MathFn::Atan => resolve(&args[0])?.atan(),
        MathFn::Atan2 => {
            let y = resolve(&args[0])?;
            let x = resolve(&args[1])?;
            y.atan2(x)
        }
        MathFn::Pow => {
            let base = resolve(&args[0])?;
            let exp = resolve(&args[1])?;
            base.powf(exp)
        }
        MathFn::Sqrt => resolve(&args[0])?.sqrt(),
        MathFn::Exp => resolve(&args[0])?.exp(),
        MathFn::Log => {
            let v = resolve(&args[0])?;
            if args.len() == 2 {
                // log(value, base) — логарифм по основанию.
                let base = resolve(&args[1])?;
                v.log(base)
            } else {
                // Единственный аргумент: натуральный логарифм (CSS §10.8.5).
                v.ln()
            }
        }
        MathFn::Hypot => {
            // hypot(a, b, ...) = sqrt(a² + b² + ...). spec.
            let mut sum_sq = 0.0_f64;
            for a in args {
                let v = resolve(a)?;
                sum_sq += v * v;
            }
            sum_sq.sqrt()
        }
        MathFn::Abs => resolve(&args[0])?.abs(),
        MathFn::Sign => {
            // CSS sign(0) = 0 (spec §10.9.2); std signum даёт +1 для 0.0
            // и -1 для -0.0. Обрабатываем явно.
            let v = resolve(&args[0])?;
            if v == 0.0 {
                0.0
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        MathFn::Mod => {
            // CSS mod (§10.9.3): результат имеет знак делителя.
            // `((a % b) + b) % b` — стандартная формула positive-mod.
            let a = resolve(&args[0])?;
            let b = resolve(&args[1])?;
            if b == 0.0 {
                return None;
            }
            ((a % b) + b) % b
        }
        MathFn::Rem => {
            // CSS rem (§10.9.4): truncated remainder, sign от делимого
            // (тот же `%` в Rust для f64).
            let a = resolve(&args[0])?;
            let b = resolve(&args[1])?;
            if b == 0.0 {
                return None;
            }
            a % b
        }
        MathFn::Round(strategy) => {
            // round([<strategy>,] val[, step]). Без step (нет 2-го arg) —
            // step = 1, как в spec §10.5.1. step ≠ 0 (иначе ÷ 0 → None).
            // Знак step сохраняется: spec не делает abs, и для nearest
            // результат симметричен, а для up/down/to-zero — нет (это та же
            // semantics, что у chrome/firefox). NaN ловится финальным
            // `is_finite()`-чеком.
            let val = resolve(&args[0])?;
            let step = if args.len() == 2 {
                let s = resolve(&args[1])?;
                if s == 0.0 {
                    return None;
                }
                s
            } else {
                1.0
            };
            let ratio = val / step;
            let rounded = match strategy {
                RoundStrategy::Nearest => ratio.round(),
                RoundStrategy::Up => ratio.ceil(),
                RoundStrategy::Down => ratio.floor(),
                RoundStrategy::ToZero => ratio.trunc(),
            };
            rounded * step
        }
    };
    if result.is_finite() {
        Some(result as f32)
    } else {
        None
    }
}

impl Length {
    /// Возвращает длину в пикселях. `em_basis` — fs, относительно которого
    /// считать `em` (родителя для font-size; текущего элемента для остального).
    /// `percent_basis` — длина, относительно которой считать `%` (None если
    /// контекст ещё не определён — тогда `%` даёт None).
    /// `viewport` — размер viewport-а для `vh`/`vw`/`vmin`/`vmax`.
    pub fn resolve(&self, em_basis: f32, percent_basis: Option<f32>, viewport: Size) -> Option<f32> {
        match self {
            Length::Px(v) => Some(*v),
            Length::Em(v) => Some(*v * em_basis),
            Length::Rem(v) => Some(*v * ROOT_FONT_SIZE),
            Length::Percent(v) => percent_basis.map(|b| *v / 100.0 * b),
            Length::Vh(v) => Some(*v / 100.0 * viewport.height),
            Length::Vw(v) => Some(*v / 100.0 * viewport.width),
            Length::Vmin(v) => Some(*v / 100.0 * viewport.width.min(viewport.height)),
            Length::Vmax(v) => Some(*v / 100.0 * viewport.width.max(viewport.height)),
            Length::Calc(node) => node.resolve(em_basis, percent_basis, viewport),
        }
    }
}

/// Парсит CSS-длину: число + опциональная единица (`px`, `em`, `rem`, `%`,
/// `vh`/`vw`/`vmin`/`vmax`). Голое число (`0`) считаем `Px(0)` — CSS позволяет
/// опускать единицу только для нуля, но мы прощаем и для других чисел.
///
/// Порядок проверки суффиксов важен: более длинные сначала (`vmin`/`vmax`
/// перед `vw`/`vh`, `rem` перед `em`).
/// CSS Quirks Mode §3.3: в quirks-mode unitless non-zero число принимается
/// как px; в standards-mode — только `0` валиден без единицы (CSS Values §6).
fn parse_length_q(s: &str, is_quirks: bool) -> Option<Length> {
    let s = s.trim();
    // CSS Values L4: math-функции calc() / min() / max() / clamp().
    // Если значение начинается с буквы и содержит `(` — обрабатываем как
    // функциональный вызов через общий tokenize_calc + parse_calc_expr;
    // parse_calc_factor распознаёт ident+lparen как function call.
    if looks_like_function_call(s)
        && let Some(len) = parse_math_function_value(s) {
        return Some(len);
    }
    if let Some(num) = s.strip_suffix("px") {
        return num.trim().parse::<f32>().ok().map(Length::Px);
    }
    if let Some(num) = s.strip_suffix("rem") {
        return num.trim().parse::<f32>().ok().map(Length::Rem);
    }
    if let Some(num) = s.strip_suffix("em") {
        return num.trim().parse::<f32>().ok().map(Length::Em);
    }
    if let Some(num) = s.strip_suffix("vmin") {
        return num.trim().parse::<f32>().ok().map(Length::Vmin);
    }
    if let Some(num) = s.strip_suffix("vmax") {
        return num.trim().parse::<f32>().ok().map(Length::Vmax);
    }
    if let Some(num) = s.strip_suffix("vh") {
        return num.trim().parse::<f32>().ok().map(Length::Vh);
    }
    if let Some(num) = s.strip_suffix("vw") {
        return num.trim().parse::<f32>().ok().map(Length::Vw);
    }
    if let Some(num) = s.strip_suffix('%') {
        return num.trim().parse::<f32>().ok().map(Length::Percent);
    }
    let n = s.parse::<f32>().ok()?;
    if n == 0.0 || is_quirks { Some(Length::Px(n)) } else { None }
}

pub fn parse_length(s: &str) -> Option<Length> {
    parse_length_q(s, true)
}

/// Похоже ли значение на функциональный вызов CSS math-функции?
/// Минимальный критерий: начинается с ASCII-буквы и содержит `(`.
/// Точное соответствие именам функций (`calc`/`min`/`max`/`clamp`)
/// проверяется в parse_calc_factor.
fn looks_like_function_call(s: &str) -> bool {
    matches!(s.as_bytes().first(), Some(b) if b.is_ascii_alphabetic())
        && s.contains('(')
}

/// Парсит top-level math-функцию (`calc(...)` / `min(...)` / `max(...)` /
/// `clamp(...)`) как обычный length-литерал, оборачивая результат в
/// `Length::Calc`. Возвращает None, если разбор не удался — `parse_length`
/// тогда падает в обычную strip_suffix-ветку.
fn parse_math_function_value(s: &str) -> Option<Length> {
    let tokens = tokenize_calc(s)?;
    let mut pos = 0usize;
    let node = parse_calc_expr(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return None;
    }
    Some(Length::Calc(Box::new(node)))
}

// ──────────────── calc() лексер + парсер ────────────────

#[derive(Debug, Clone, PartialEq)]
enum CalcToken {
    /// Числовой токен с (опциональным) unit-суффиксом.
    Num(f32, String),
    /// Идентификатор функции (`calc`, `min`, `max`, `clamp`). Хранится в
    /// нижнем регистре — CSS function names ASCII case-insensitive.
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    /// Разделитель аргументов функции.
    Comma,
}

/// Лексер `calc()` тела. Возвращает None при синтаксической ошибке (например,
/// неизвестный символ или сломанное число).
///
/// `-` всегда токенизируется как `Minus` (не как часть числа). Унарный
/// минус (`calc(-10px + 5px)`) разрешается парсером через
/// `factor := ('-' | '+') factor | …`. Это даёт корректное поведение и для
/// `10px - 5px` (whitespace по спецификации), и для `10px-5px` (lenient).
fn tokenize_calc(s: &str) -> Option<Vec<CalcToken>> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let tok = match b {
            b'+' => CalcToken::Plus,
            b'-' => CalcToken::Minus,
            b'*' => CalcToken::Star,
            b'/' => CalcToken::Slash,
            b'(' => CalcToken::LParen,
            b')' => CalcToken::RParen,
            b',' => CalcToken::Comma,
            // Число без ведущего знака (знак — отдельный токен).
            b'0'..=b'9' | b'.' => {
                let (num, unit, end) = lex_number(bytes, i)?;
                tokens.push(CalcToken::Num(num, unit));
                i = end;
                continue;
            }
            // Идентификатор функции — буквенный старт + опц. цифры/дефис
            // (так в имени `atan2` лексер не споткнётся на `2`).
            c if c.is_ascii_alphabetic() => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-')
                {
                    i += 1;
                }
                let name = std::str::from_utf8(&bytes[start..i])
                    .ok()?
                    .to_ascii_lowercase();
                tokens.push(CalcToken::Ident(name));
                continue;
            }
            _ => return None,
        };
        tokens.push(tok);
        i += 1;
    }
    Some(tokens)
}

/// Парсит число (без знака) + опциональный unit-суффикс начиная с `bytes[start]`.
/// Возвращает (значение, unit, индекс после конца токена). Знак лежит
/// отдельным `Minus`/`Plus`-токеном.
fn lex_number(bytes: &[u8], start: usize) -> Option<(f32, String, usize)> {
    let mut i = start;
    let num_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let num_end = i;
    if num_end == num_start {
        return None;
    }
    let num_str = std::str::from_utf8(&bytes[num_start..num_end]).ok()?;
    let num = num_str.parse::<f32>().ok()?;
    // Unit-суффикс: буквы (для px/em/rem/vh/vw/vmin/vmax) или `%`.
    let unit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == unit_start && matches!(bytes.get(i), Some(b'%')) {
        i += 1;
    }
    let unit =
        std::str::from_utf8(&bytes[unit_start..i]).ok()?.to_ascii_lowercase();
    Some((num, unit, i))
}

/// `expr := term (('+' | '-') term)*`
fn parse_calc_expr(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    let mut left = parse_calc_term(tokens, pos)?;
    loop {
        match tokens.get(*pos) {
            Some(CalcToken::Plus) => {
                *pos += 1;
                let right = parse_calc_term(tokens, pos)?;
                left = CalcNode::Add(Box::new(left), Box::new(right));
            }
            Some(CalcToken::Minus) => {
                *pos += 1;
                let right = parse_calc_term(tokens, pos)?;
                left = CalcNode::Sub(Box::new(left), Box::new(right));
            }
            _ => return Some(left),
        }
    }
}

/// `term := factor (('*' | '/') factor)*`
fn parse_calc_term(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    let mut left = parse_calc_factor(tokens, pos)?;
    loop {
        match tokens.get(*pos) {
            Some(CalcToken::Star) => {
                *pos += 1;
                let right = parse_calc_factor(tokens, pos)?;
                left = CalcNode::Mul(Box::new(left), Box::new(right));
            }
            Some(CalcToken::Slash) => {
                *pos += 1;
                let right = parse_calc_factor(tokens, pos)?;
                left = CalcNode::Div(Box::new(left), Box::new(right));
            }
            _ => return Some(left),
        }
    }
}

/// `factor := ('-' | '+') factor | function | Num(value, unit) | '(' expr ')'`
///
/// `function := Ident '(' arg-list ')'` где `Ident` — одно из `calc` /
/// `min` / `max` / `clamp` (CSS Values L4 §10 и §10.6). Унарный `-`
/// реализуется как `0 - factor`. Унарный `+` — no-op.
fn parse_calc_factor(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    match tokens.get(*pos)? {
        CalcToken::Minus => {
            *pos += 1;
            let inner = parse_calc_factor(tokens, pos)?;
            Some(CalcNode::Sub(
                Box::new(CalcNode::Number(0.0)),
                Box::new(inner),
            ))
        }
        CalcToken::Plus => {
            *pos += 1;
            parse_calc_factor(tokens, pos)
        }
        CalcToken::LParen => {
            *pos += 1;
            let inner = parse_calc_expr(tokens, pos)?;
            if !matches!(tokens.get(*pos), Some(CalcToken::RParen)) {
                return None;
            }
            *pos += 1;
            Some(inner)
        }
        CalcToken::Ident(name) => {
            let name = name.clone();
            *pos += 1;
            if !matches!(tokens.get(*pos), Some(CalcToken::LParen)) {
                return None;
            }
            *pos += 1;
            parse_function_call(&name, tokens, pos)
        }
        CalcToken::Num(v, unit) => {
            let v = *v;
            let unit = unit.clone();
            *pos += 1;
            calc_num_to_node(v, &unit)
        }
        _ => None,
    }
}

/// Парсит тело math-функции после `<name>(` (открывающая скобка уже
/// съедена), ожидает `)` в конце. Поддерживает `calc` (один expr),
/// `min` / `max` (1+ expr через `,`), `clamp` (ровно 3 expr через `,`).
/// Неизвестное имя → None.
fn parse_function_call(
    name: &str,
    tokens: &[CalcToken],
    pos: &mut usize,
) -> Option<CalcNode> {
    // CSS Values L4 §10.5.1: `round( <rounding-strategy>?, A, B? )` —
    // первый аргумент-keyword. Распознаём ДО общего parse_arg_list, чтобы
    // ident-без-`(` не падал в `parse_calc_factor` как «функция без скобок».
    // После keyword обязательна `,` — strategy без последующего expr невалиден.
    let round_strategy = if name == "round" {
        if let Some(CalcToken::Ident(kw)) = tokens.get(*pos)
            && let Some(s) = parse_round_strategy(kw)
        {
            *pos += 1;
            if !matches!(tokens.get(*pos), Some(CalcToken::Comma)) {
                return None;
            }
            *pos += 1;
            Some(s)
        } else {
            None
        }
    } else {
        None
    };

    let args = parse_arg_list(tokens, pos)?;
    if !matches!(tokens.get(*pos), Some(CalcToken::RParen)) {
        return None;
    }
    *pos += 1;
    match name {
        "calc" => {
            if args.len() != 1 {
                return None;
            }
            Some(args.into_iter().next().unwrap())
        }
        "min" => {
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Min(args))
        }
        "max" => {
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Max(args))
        }
        "clamp" => {
            if args.len() != 3 {
                return None;
            }
            let mut it = args.into_iter();
            let a = it.next().unwrap();
            let b = it.next().unwrap();
            let c = it.next().unwrap();
            Some(CalcNode::Clamp(Box::new(a), Box::new(b), Box::new(c)))
        }
        // CSS Values L4 §10.7-10.9 — научные math-функции.
        // Имя → (MathFn, валидное число аргументов). Проверяем арность тут,
        // resolve_math_func предполагает корректность.
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "exp"
        | "abs" | "sign" => {
            if args.len() != 1 {
                return None;
            }
            let func = match name {
                "sin" => MathFn::Sin,
                "cos" => MathFn::Cos,
                "tan" => MathFn::Tan,
                "asin" => MathFn::Asin,
                "acos" => MathFn::Acos,
                "atan" => MathFn::Atan,
                "sqrt" => MathFn::Sqrt,
                "exp" => MathFn::Exp,
                "abs" => MathFn::Abs,
                "sign" => MathFn::Sign,
                _ => unreachable!(),
            };
            Some(CalcNode::Func(func, args))
        }
        "atan2" | "pow" | "mod" | "rem" => {
            if args.len() != 2 {
                return None;
            }
            let func = match name {
                "atan2" => MathFn::Atan2,
                "pow" => MathFn::Pow,
                "mod" => MathFn::Mod,
                "rem" => MathFn::Rem,
                _ => unreachable!(),
            };
            Some(CalcNode::Func(func, args))
        }
        "log" => {
            // 1 или 2 аргумента: log(x) = ln(x), log(x, base) = log_base(x).
            if args.is_empty() || args.len() > 2 {
                return None;
            }
            Some(CalcNode::Func(MathFn::Log, args))
        }
        "hypot" => {
            // 1+ аргумента.
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Func(MathFn::Hypot, args))
        }
        "round" => {
            // round([<strategy>,] val[, step]). Strategy keyword уже снят
            // вверху функции и зашит в `MathFn::Round(...)`; здесь остаётся
            // классический args-чек 1..=2.
            if args.is_empty() || args.len() > 2 {
                return None;
            }
            let s = round_strategy.unwrap_or(RoundStrategy::Nearest);
            Some(CalcNode::Func(MathFn::Round(s), args))
        }
        _ => None, // незнакомая math-функция
    }
}

/// CSS Values L4 §10.5.1: `<rounding-strategy>` = `nearest | up | down | to-zero`.
/// Имя приходит уже в нижнем регистре из лексера; неподходящий ident → None.
fn parse_round_strategy(name: &str) -> Option<RoundStrategy> {
    match name {
        "nearest" => Some(RoundStrategy::Nearest),
        "up" => Some(RoundStrategy::Up),
        "down" => Some(RoundStrategy::Down),
        "to-zero" => Some(RoundStrategy::ToZero),
        _ => None,
    }
}

/// Парсит список аргументов функции — один или больше expr-ов через
/// запятые. Останавливается перед `)`; не съедает его.
fn parse_arg_list(tokens: &[CalcToken], pos: &mut usize) -> Option<Vec<CalcNode>> {
    let mut args = Vec::new();
    args.push(parse_calc_expr(tokens, pos)?);
    while matches!(tokens.get(*pos), Some(CalcToken::Comma)) {
        *pos += 1;
        args.push(parse_calc_expr(tokens, pos)?);
    }
    Some(args)
}

/// Преобразует пару (число, unit) в `CalcNode`. Пустой unit → `Number`,
/// length-units → `Length::*`, angle-units (deg/rad/turn/grad) →
/// `Number(radians)` (по CSS Values L4 §10.7 — trig-функции принимают
/// число или angle; unitless считается уже в радианах). Неизвестный unit
/// (`pt`, `mm`, …) даёт None.
fn calc_num_to_node(value: f32, unit: &str) -> Option<CalcNode> {
    if unit.is_empty() {
        return Some(CalcNode::Number(value));
    }
    // Angle-units: конвертируем в радианы и храним как Number.
    // Это позволяет sin/cos/tan корректно работать с любой формой угла,
    // и сохраняет результат asin/acos/atan/atan2 как plain number
    // (по умолчанию интерпретируется как радианы при подаче обратно в trig).
    let pi = std::f32::consts::PI;
    match unit {
        "deg" => return Some(CalcNode::Number(value * pi / 180.0)),
        "rad" => return Some(CalcNode::Number(value)),
        "turn" => return Some(CalcNode::Number(value * 2.0 * pi)),
        "grad" => return Some(CalcNode::Number(value * pi / 200.0)),
        _ => {}
    }
    let length = match unit {
        "px" => Length::Px(value),
        "em" => Length::Em(value),
        "rem" => Length::Rem(value),
        "vh" => Length::Vh(value),
        "vw" => Length::Vw(value),
        "vmin" => Length::Vmin(value),
        "vmax" => Length::Vmax(value),
        "%" => Length::Percent(value),
        _ => return None,
    };
    Some(CalcNode::Length(length))
}

/// Глубина рекурсии при разворачивании `var()` — защита от циклов вида
/// `--a: var(--b); --b: var(--a)`. CSS spec не задаёт точного предела;
/// 32 уровня хватает для любого реалистичного nesting, а зацикленные
/// определения отсекутся быстро.
const VAR_EXPAND_MAX_DEPTH: u32 = 32;

/// CSS Variables L1 §3: рекурсивно разворачивает все `var(--name [, fallback])`
/// в `value`. Возвращает None, если:
///   - встретилась `var()` с именем, которого нет в `custom`, и нет fallback;
///   - превышена глубина рекурсии (cycle / слишком глубокий nest);
///   - синтаксис `var(...)` сломан (нет закрывающей скобки).
///
/// При успехе — возвращает строку с подставленными значениями. Все
/// substitution-ы делаются как plain string replacement; типы значений
/// проверит уже сам `apply_declaration` после expand.
fn expand_vars(value: &str, custom: &HashMap<String, String>, depth: u32) -> Option<String> {
    if depth > VAR_EXPAND_MAX_DEPTH {
        return None;
    }
    let Some(start) = find_var_open(value) else {
        return Some(value.to_string());
    };
    let prefix = &value[..start];
    let after_open = &value[start + 4..]; // skip "var("
    let (args, after_close) = parse_balanced_to_close(after_open)?;
    let (name, fallback) = split_var_args(args);
    if !name.starts_with("--") {
        return None;
    }
    let resolved = if let Some(v) = custom.get(name) {
        expand_vars(v.trim(), custom, depth + 1)?
    } else if let Some(fb) = fallback {
        expand_vars(fb.trim(), custom, depth + 1)?
    } else {
        return None;
    };
    let combined = format!("{prefix}{resolved}{after_close}");
    expand_vars(&combined, custom, depth + 1)
}

/// Раскрывает все `env(name [<index>...]?, fallback?)` в value.
/// CSS Environment Variables L1: `env()` — это var()-подобная подстановка
/// из UA-supplied registry. Имена не имеют `--` префикса (env-имена —
/// `safe-area-inset-top`, `viewport-segment-width` и т.д.).
///
/// Phase 0: registry — пустой `HashMap`, все env-вызовы попадают в
/// fallback. Это даёт корректное `padding: env(safe-area-inset-top, 0px)`
/// → `padding: 0px`. Indices (`env(name 0 1, fallback)`) парсятся, но
/// игнорируются (используется только name до пробела).
fn expand_env_vars(
    value: &str,
    env_registry: &HashMap<String, String>,
    depth: u32,
) -> Option<String> {
    if depth > VAR_EXPAND_MAX_DEPTH {
        return None;
    }
    let Some(start) = find_env_open(value) else {
        return Some(value.to_string());
    };
    let prefix = &value[..start];
    let after_open = &value[start + 4..]; // skip "env("
    let (args, after_close) = parse_balanced_to_close(after_open)?;
    let (name_part, fallback) = split_var_args(args);
    // Indices в name-part: `safe-area-inset-top` или `viewport-segment-width 0 0`.
    let env_name = name_part.split_whitespace().next().unwrap_or("");
    if env_name.is_empty() {
        return None;
    }
    let resolved = if let Some(v) = env_registry.get(env_name) {
        expand_env_vars(v.trim(), env_registry, depth + 1)?
    } else if let Some(fb) = fallback {
        expand_env_vars(fb.trim(), env_registry, depth + 1)?
    } else {
        return None;
    };
    let combined = format!("{prefix}{resolved}{after_close}");
    expand_env_vars(&combined, env_registry, depth + 1)
}

/// Аналог `find_var_open` для `env(`. Учитывает строковые литералы.
fn find_env_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i + 4 <= bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => {
                in_string = None;
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                in_string = Some(b);
                i += 1;
            }
            (None, b'e') if &bytes[i..i + 4] == b"env(" => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// UA env-registry. Phase 0: пустой; вызовы `env(name, fallback)`
/// возвращают fallback. В Phase 2+ значения будут заполняться shell-ом
/// из реального viewport state (safe-area, виртуальная клавиатура).
fn empty_env_registry() -> HashMap<String, String> {
    HashMap::new()
}

/// Находит позицию первого `var(` в `s` вне строковых литералов. Возвращает
/// индекс символа `v`. Учитывает одинарные и двойные кавычки, чтобы
/// `content: "var(x)"` не давал ложного матча.
fn find_var_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i + 4 <= bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => {
                in_string = None;
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                in_string = Some(b);
                i += 1;
            }
            (None, b'v') if &bytes[i..i + 4] == b"var(" => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Принимает строку, начинающуюся **сразу после** `var(`, и читает её до
/// парной закрывающей скобки с учётом вложенных `(...)` и строковых литералов.
/// Возвращает (содержимое внутри `var(...)`, остаток после `)`).
fn parse_balanced_to_close(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut depth = 1u32;
    let mut in_string: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => in_string = None,
            (None, b'"') | (None, b'\'') => in_string = Some(b),
            (None, b'(') => depth += 1,
            (None, b')') => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Разбивает аргументы `var(...)` на (имя, опциональный fallback) по первой
/// top-level запятой. Запятые внутри вложенных скобок или строк — не граница.
fn split_var_args(s: &str) -> (&str, Option<&str>) {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut in_string: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (in_string, b) {
            (Some(q), c) if c == q => in_string = None,
            (None, b'"') | (None, b'\'') => in_string = Some(b),
            (None, b'(') => depth += 1,
            (None, b')') => depth = depth.saturating_sub(1),
            (None, b',') if depth == 0 => {
                return (s[..i].trim(), Some(s[i + 1..].trim()));
            }
            _ => {}
        }
    }
    (s.trim(), None)
}

fn apply_declaration(
    style: &mut ComputedStyle,
    decl: &Declaration,
    em_basis: f32,
    viewport: Size,
    parent_font_weight: FontWeight,
    inherited: &ComputedStyle,
    is_quirks: bool,
) {
    let prop = decl.property.as_str();

    // Custom properties обрабатываются в отдельном pass до этого момента
    // (см. compute_style). Здесь — игнорируем.
    if prop.starts_with("--") {
        return;
    }

    // CSS Variables L1 §3: подстановка `var(--name [, fallback])` на этапе
    // применения. Если value содержит `var(` — пробуем expand с текущей
    // картой custom_props. При неудаче (имя не найдено и нет fallback,
    // глубина рекурсии превышена, синтаксическая ошибка) декларация
    // считается отсутствующей (CSS Variables L1 §3.3 «invalid at computed
    // value time»). `expanded` живёт до конца функции, чтобы `val` остался
    // валидным `&str`.
    let expanded;
    let val: &str = if decl.value.contains("var(") || decl.value.contains("env(") {
        let after_var = if decl.value.contains("var(") {
            match expand_vars(&decl.value, &style.custom_props, 0) {
                Some(v) => v,
                None => return,
            }
        } else {
            decl.value.clone()
        };
        // CSS Environment Variables L1: env() раскрывается ПОСЛЕ var(),
        // потому что custom property может содержать `env(...)`.
        if after_var.contains("env(") {
            match expand_env_vars(&after_var, &empty_env_registry(), 0) {
                Some(v) => {
                    expanded = v;
                    expanded.as_str()
                }
                None => return,
            }
        } else {
            expanded = after_var;
            expanded.as_str()
        }
    } else {
        decl.value.as_str()
    };

    // CSS Cascade L4 §7: CSS-wide keywords (inherit / initial / unset /
    // revert) применимы к любому свойству. Делается ДО property-specific
    // парсинга, чтобы не дублировать проверку в 30+ branch-ах. font-size
    // обрабатывается в `apply_font_size` (pre-pass) — здесь повторно для
    // случая, когда font-size попал в main-pass через невидимую генерик
    // декларацию (no-op, font-size уже выставлен).
    if let Some(kw) = parse_css_wide_keyword(val) {
        apply_css_wide_keyword(style, prop, kw, inherited);
        return;
    }
    match prop {
        "display" => {
            style.display = match val {
                "block" => Display::Block,
                "inline" => Display::Inline,
                "none" => Display::None,
                "flex" => Display::Flex,
                "inline-flex" => Display::InlineFlex,
                "grid" => Display::Grid,
                "inline-grid" => Display::InlineGrid,
                "inline-block" => Display::InlineBlock,
                _ => style.display,
            };
        }
        "text-align" => {
            style.text_align = match val {
                "left" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => style.text_align,
            };
        }
        "direction" => {
            // CSS Writing Modes L3 §2.1. Keyword-ы case-insensitive по
            // правилам CSS («property keyword values are ASCII case-
            // insensitive», CSS Values L4 §2.4). Невалидное значение
            // оставляет inherited (или предыдущее) направление.
            if val.eq_ignore_ascii_case("ltr") {
                style.direction = Direction::Ltr;
            } else if val.eq_ignore_ascii_case("rtl") {
                style.direction = Direction::Rtl;
            }
        }
        "color" => {
            if let Some(c) = parse_color_legacy(val, is_quirks) {
                style.color = c;
            }
        }
        "background-color" | "background" => {
            if let Some(c) = parse_color_legacy(val, is_quirks) {
                style.background_color = Some(c);
            }
        }
        "accent-color" => {
            // CSS UI L4 §6.1: <color> | auto.
            // 'auto' = None — UA сама подберёт цвет (обычно системный акцент).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.accent_color = None;
            } else if let Some(c) = parse_color_legacy(trimmed, is_quirks) {
                style.accent_color = Some(c);
            }
        }
        "object-fit" => {
            if let Some(of) = ObjectFit::parse(val) {
                style.object_fit = of;
            }
        }
        "object-position" => {
            if let Some(op) = ObjectPosition::parse(val, em_basis, viewport) {
                style.object_position = op;
            }
        }
        "vertical-align" => {
            // CSS 2.1 §10.8.1: keyword | <percentage> | <length>.
            // Keyword первым (text-top / text-bottom — двусоставные, не
            // конфликтуют с length-парсером); затем length: Percent сохраняем
            // как Percent (резолвится по line-height в layout-pass), остальные
            // резолвим к px относительно текущего font-size.
            if let Some(va) = VerticalAlign::parse_keyword(val) {
                style.vertical_align = va;
            } else if let Some(len) = parse_length_q(val, is_quirks) {
                match len {
                    Length::Percent(p) => style.vertical_align = VerticalAlign::Percent(p),
                    other => {
                        if let Some(px) = other.resolve(em_basis, None, viewport) {
                            style.vertical_align = VerticalAlign::Length(px);
                        }
                    }
                }
            }
        }
        "image-rendering" => {
            // CSS Images L3 §6.1: enum-keyword. Inherited.
            if let Some(v) = ImageRendering::parse(val) {
                style.image_rendering = v;
            }
        }
        "text-wrap-mode" => {
            // CSS Text Module Level 4 §6.4.1: wrap | nowrap. Inherited.
            if let Some(v) = TextWrapMode::parse(val) {
                style.text_wrap_mode = v;
            }
        }
        "text-wrap-style" => {
            // CSS Text Module Level 4 §6.4.2: auto | balance | stable | pretty. Inherited.
            if let Some(v) = TextWrapStyle::parse(val) {
                style.text_wrap_style = v;
            }
        }
        "text-wrap" => {
            // CSS Text Module Level 4 §6.4.3: shorthand для text-wrap-mode и
            // text-wrap-style; синтаксис `<'text-wrap-mode'> || <'text-wrap-style'>`.
            // 1 или 2 идентификатора, любой порядок. Каждый shorthand сбрасывает
            // обе longhand-компоненты к initial-value, после чего применяются
            // указанные. См. CSS Cascade L4 §3.1 для семантики shorthand reset.
            apply_text_wrap_shorthand(style, val);
        }
        "width" if val != "auto" => {
            style.width = parse_length_q(val, is_quirks).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        "height" if val != "auto" => {
            style.height = parse_length_q(val, is_quirks).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        // CSS 2.1 §10.4: min-/max- ширина и высота. Отрицательные значения
        // запрещены спецификацией — отбрасываем. `none` для max-* = снять
        // ограничение (None). `auto` для min-* (CSS3 Sizing default для
        // flex/grid) трактуем как None — Phase 0 без flex/grid, это
        // эквивалентно нулевому минимуму.
        "min-width" if val != "auto" => {
            style.min_width = parse_length_q(val, is_quirks)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "max-width" if val != "none" => {
            style.max_width = parse_length_q(val, is_quirks)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "min-height" if val != "auto" => {
            style.min_height = parse_length_q(val, is_quirks)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "max-height" if val != "none" => {
            style.max_height = parse_length_q(val, is_quirks)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "font-size" => {
            // Обрабатывается в pre-pass; в этой ветке пропускаем.
        }
        "font-style" => {
            // CSS Fonts L4 — normal | italic | oblique. Прочее (`oblique 10deg`,
            // `oblique -5deg`) пока не поддерживаем — берём как oblique.
            style.font_style = match val.split_whitespace().next() {
                Some("italic") => FontStyle::Italic,
                Some("oblique") => FontStyle::Oblique,
                Some("normal") => FontStyle::Normal,
                _ => style.font_style,
            };
        }
        "font-weight" => {
            if let Some(w) = parse_font_weight(val, parent_font_weight) {
                style.font_weight = w;
            }
        }
        "font-family" => {
            let list = parse_font_family(val);
            if !list.is_empty() {
                style.font_family = list;
            }
        }
        "font-variant" | "font-variant-caps" => {
            // Phase 0: только normal | small-caps. Прочие keyword-ы
            // (all-small-caps, petite-caps, …) и связанные субсвойства
            // (font-variant-ligatures, -numeric, и т.д.) — отложены.
            style.font_variant = match val.split_whitespace().next() {
                Some("small-caps") => FontVariant::SmallCaps,
                Some("normal") => FontVariant::Normal,
                _ => style.font_variant,
            };
        }
        "font-stretch" => {
            let token = val.split_whitespace().next().unwrap_or("");
            if let Some(fs) = FontStretch::from_keyword(token) {
                style.font_stretch = fs;
            } else if let Some(pct) = token.strip_suffix('%')
                && let Ok(n) = pct.trim().parse::<f32>()
            {
                // CSS Fonts L4 §2.5: percentage >= 0%. Out-of-range
                // значения формально валидны, но бесполезны для рендеринга
                // и могут переполнить u16 (max ≈ 6553%). Клампим в
                // привычные [50%, 200%].
                let clamped = n.clamp(50.0, 200.0);
                style.font_stretch = FontStretch((clamped * 10.0).round() as u16);
            }
        }
        "text-indent" => {
            // CSS Text L3 §7.1: <length> | <percentage>. % требует
            // containing-block-width — Phase 0 пока игнорирует, как и в
            // margin/padding. Поддерживаем px/em/rem/vh/vw.
            if let Some(len) = parse_length_q(val, is_quirks)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.text_indent = px;
            }
        }
        "letter-spacing" => {
            // CSS Text L3 §11.2: normal (= 0) | <length>. Может быть
            // отрицательным.
            if val.trim() == "normal" {
                style.letter_spacing = 0.0;
            } else if let Some(len) = parse_length_q(val, is_quirks)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.letter_spacing = px;
            }
        }
        "word-spacing" => {
            // CSS Text L3 §11.3: normal (= 0) | <length> | <percentage>.
            // % требует ширину space-glyph и Phase 0 не считаем.
            if val.trim() == "normal" {
                style.word_spacing = 0.0;
            } else if let Some(len) = parse_length_q(val, is_quirks)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.word_spacing = px;
            }
        }
        "text-transform" => {
            // CSS Text L3: none | uppercase | lowercase | capitalize.
            // `full-width` / `full-size-kana` отложены (CJK-специфика).
            style.text_transform = match val.split_whitespace().next() {
                Some("none") => TextTransform::None,
                Some("uppercase") => TextTransform::Uppercase,
                Some("lowercase") => TextTransform::Lowercase,
                Some("capitalize") => TextTransform::Capitalize,
                _ => style.text_transform,
            };
        }
        "white-space" => {
            // CSS Text L3 §3.1: phase 0 — normal | nowrap. Pre-варианты
            // требуют preserved whitespace в input и пока игнорируются
            // (молча сохраняют текущее значение).
            style.white_space = match val.trim() {
                "normal" => WhiteSpace::Normal,
                "nowrap" => WhiteSpace::Nowrap,
                _ => style.white_space,
            };
        }
        "visibility" => {
            style.visibility = match val.trim() {
                "visible" => Visibility::Visible,
                "hidden" => Visibility::Hidden,
                "collapse" => Visibility::Collapse,
                _ => style.visibility,
            };
        }
        "overflow" => {
            // CSS Overflow L3: shorthand. Один токен — оба axis; два — x y.
            let toks: Vec<&str> = val.split_whitespace().collect();
            match toks.as_slice() {
                [a] => {
                    if let Some(o) = parse_overflow_kw(a) {
                        style.overflow_x = o;
                        style.overflow_y = o;
                    }
                }
                [a, b] => {
                    if let Some(o) = parse_overflow_kw(a) { style.overflow_x = o; }
                    if let Some(o) = parse_overflow_kw(b) { style.overflow_y = o; }
                }
                _ => {}
            }
        }
        "overflow-x" => {
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_x = o;
            }
        }
        "overflow-y" => {
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_y = o;
            }
        }
        "text-overflow" => {
            // CSS UI L4: clip | ellipsis. <string> (custom marker) и
            // two-value формы не поддерживаем в Phase 0.
            style.text_overflow = match val.split_whitespace().next() {
                Some("clip") => TextOverflow::Clip,
                Some("ellipsis") => TextOverflow::Ellipsis,
                _ => style.text_overflow,
            };
        }
        "cursor" => {
            // CSS UI L4 §8.1: список url(), затем обязательный keyword.
            // url(...) пока не поддерживаем — берём ПОСЛЕДНИЙ
            // comma-separated токен (это и есть keyword fallback).
            let last = val.rsplit(',').next().unwrap_or("").trim();
            if let Some(c) = parse_cursor_kw(last) {
                style.cursor = c;
            }
        }
        "box-shadow" => {
            // CSS Backgrounds L3 §4.6: comma-separated. `none` сбрасывает.
            if val.trim() == "none" {
                style.box_shadow = Vec::new();
            } else {
                let mut shadows = Vec::new();
                for piece in split_top_level_commas(val) {
                    if let Some(s) = parse_box_shadow_one(piece.trim(), em_basis, viewport, is_quirks) {
                        shadows.push(s);
                    }
                }
                if !shadows.is_empty() {
                    style.box_shadow = shadows;
                }
            }
        }
        "text-shadow" => {
            // CSS Text Decoration L3 §4: то же что box-shadow, но без inset
            // и spread. `none` сбрасывает (важно: text-shadow inherited,
            // явное `none` нужно чтобы откатить родительское).
            if val.trim() == "none" {
                style.text_shadow = Vec::new();
            } else {
                let mut shadows = Vec::new();
                for piece in split_top_level_commas(val) {
                    if let Some(s) = parse_text_shadow_one(piece.trim(), em_basis, viewport, is_quirks) {
                        shadows.push(s);
                    }
                }
                if !shadows.is_empty() {
                    style.text_shadow = shadows;
                }
            }
        }
        "outline" => {
            // CSS Basic UI L4 §5.1 — `outline` shorthand сбрасывает все три
            // longhand-а в initial и парсит токены `[<'outline-color'> ||
            // <'outline-style'> || <'outline-width'>]` в любом порядке.
            // Каждый slot заполняется первым подходящим токеном.
            style.outline_width = 3.0; // medium
            style.outline_style = OutlineStyle::None;
            style.outline_color = OutlineColor::Auto;
            let mut width_set = false;
            let mut style_set = false;
            let mut color_set = false;
            for tok in val.split_whitespace() {
                if !style_set
                    && let Some(s) = parse_outline_style_opt(tok)
                {
                    style.outline_style = s;
                    style_set = true;
                } else if !width_set
                    && let Some(w) = parse_line_width(tok, em_basis, viewport, is_quirks)
                {
                    style.outline_width = w;
                    width_set = true;
                } else if !color_set
                    && let Some(c) = parse_outline_color_opt(tok, is_quirks)
                {
                    style.outline_color = c;
                    color_set = true;
                }
            }
        }
        "outline-width" => {
            if let Some(v) = parse_line_width(val, em_basis, viewport, is_quirks) {
                style.outline_width = v;
            }
        }
        "outline-style" => {
            if let Some(s) = parse_outline_style_opt(val) {
                style.outline_style = s;
            }
        }
        "outline-color" => {
            if let Some(c) = parse_outline_color_opt(val, is_quirks) {
                style.outline_color = c;
            }
        }
        "outline-offset" => {
            // <length>; отрицательные значения валидны (CSS UI L4 §3.4).
            if let Some(len) = parse_length_q(val, is_quirks)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.outline_offset = px;
            }
        }
        "counter-reset" => {
            // CSS Lists L3 §3 — `none | (<custom-ident> <integer>?)+`.
            // Default value на счётчик при отсутствии числа = 0 (по spec).
            // `none` сбрасывает всё.
            style.counter_reset = parse_counter_list(val, 0);
        }
        "counter-increment" => {
            // CSS Lists L3 §3 — `none | (<custom-ident> <integer>?)+`.
            // Default value = 1 (по spec).
            style.counter_increment = parse_counter_list(val, 1);
        }
        "clip-path" => {
            // CSS Masking L1 §3 — basic-shape | none. `none` чистит.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.clip_path = None;
            } else if let Some(cp) = parse_clip_path(trimmed) {
                style.clip_path = Some(cp);
            }
        }
        "transform" => {
            // CSS Transforms L1 §2 — `none | <transform-list>`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.transform = Vec::new();
            } else {
                style.transform = parse_transform_list(trimmed);
            }
        }
        "filter" => {
            // CSS Filter Effects L1 §3 — `none | <filter-function-list>`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.filter = Vec::new();
            } else {
                style.filter = parse_filter_list(trimmed);
            }
        }
        "row-gap" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.row_gap = px.max(0.0);
            }
        }
        "column-gap" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.column_gap = px.max(0.0);
            }
        }
        "gap" => {
            // Shorthand: `<row-gap> <column-gap>?` (если column отсутствует,
            // = row).
            let parts: Vec<&str> = val.split_whitespace().collect();
            if !parts.is_empty() {
                let row = resolve_box_length(parts[0], em_basis, viewport, is_quirks).map(|v| v.max(0.0));
                let col = if parts.len() >= 2 {
                    resolve_box_length(parts[1], em_basis, viewport, is_quirks).map(|v| v.max(0.0))
                } else {
                    row
                };
                if let (Some(r), Some(c)) = (row, col) {
                    style.row_gap = r;
                    style.column_gap = c;
                }
            }
        }
        "column-count" => {
            // CSS Multi-column L1 §3.2: <integer> | auto.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.column_count = None;
            } else if let Ok(n) = trimmed.parse::<u32>()
                && n > 0
            {
                style.column_count = Some(n);
            }
        }
        "column-width" => {
            // CSS Multi-column L1 §3.3: <length> | auto.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.column_width = None;
            } else if let Some(px) = resolve_box_length(trimmed, em_basis, viewport, is_quirks)
                && px >= 0.0
            {
                style.column_width = Some(px);
            }
        }
        "columns" => {
            // CSS Multi-column L1 §3.4 shorthand: <column-width> || <column-count>.
            // Любой токен может быть `auto`. Length → width, integer → count.
            let parts: Vec<&str> = val.split_whitespace().collect();
            let mut count: Option<u32> = None;
            let mut width: Option<f32> = None;
            let mut had_width = false;
            let mut had_count = false;
            for p in &parts {
                if p.eq_ignore_ascii_case("auto") {
                    // Один auto — не назначаем, оставляем None для обоих.
                    continue;
                }
                if let Ok(n) = p.parse::<u32>()
                    && n > 0
                    && !had_count
                {
                    count = Some(n);
                    had_count = true;
                    continue;
                }
                if let Some(px) = resolve_box_length(p, em_basis, viewport, is_quirks)
                    && px >= 0.0
                    && !had_width
                {
                    width = Some(px);
                    had_width = true;
                }
            }
            // Если хотя бы один токен распознали — применяем.
            if had_width || had_count {
                style.column_width = width;
                style.column_count = count;
            }
        }
        "column-rule-width" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.column_rule_width = px.max(0.0);
            }
        }
        "column-rule-style" => {
            style.column_rule_style = parse_border_style_opt(val.trim()).unwrap_or(BorderStyle::None);
        }
        "column-rule-color" => {
            style.column_rule_color = parse_color_legacy(val.trim(), is_quirks);
        }
        "column-rule" => {
            // Shorthand: <width> || <style> || <color>. Любой порядок.
            let mut rest = val.trim().to_string();
            // Color может содержать пробелы (rgba(...)), но в Phase 0 — простой
            // word-by-word проход.
            for tok in val.split_whitespace() {
                if let Some(s) = parse_border_style_opt(tok) {
                    style.column_rule_style = s;
                    rest = rest.replacen(tok, "", 1);
                    continue;
                }
                if let Some(px) = resolve_box_length(tok, em_basis, viewport, is_quirks)
                    && px >= 0.0
                {
                    style.column_rule_width = px;
                    rest = rest.replacen(tok, "", 1);
                    continue;
                }
                if let Some(c) = parse_color_legacy(tok, is_quirks) {
                    style.column_rule_color = Some(c);
                    rest = rest.replacen(tok, "", 1);
                }
            }
            // Если в rest осталось что-то с скобками (`rgba(...)`) — пытаемся
            // парсить как цвет.
            let rest = rest.trim();
            if !rest.is_empty()
                && style.column_rule_color.is_none()
                && let Some(c) = parse_color_legacy(rest, is_quirks)
            {
                style.column_rule_color = Some(c);
            }
        }
        "column-span" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "all" => style.column_span_all = true,
                "none" => style.column_span_all = false,
                _ => {}
            }
        }
        "column-fill" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "balance" => style.column_fill_balance = true,
                "auto" => style.column_fill_balance = false,
                _ => {}
            }
        }
        "break-before" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_before = v;
            }
        }
        "break-after" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_after = v;
            }
        }
        "break-inside" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_inside = v;
            }
        }
        "aspect-ratio" => {
            // CSS Sizing L4 §6.1: `auto | <ratio>`. <ratio> = number или
            // `W / H`. Phase 0 игнорирует `auto <ratio>` форму
            // (intrinsic + override).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.aspect_ratio = None;
            } else if let Some(r) = parse_aspect_ratio_value(trimmed) {
                style.aspect_ratio = Some(r);
            }
        }
        // CSS Box Alignment L3 — alignment свойства. Парсятся как одно
        // значение (полная грамматика с baseline-fallback и safe/unsafe —
        // отложена).
        "align-items" => {
            if let Some(v) = AlignValue::parse(val) {
                style.align_items = v;
            }
        }
        "align-self" => {
            if let Some(v) = AlignValue::parse(val) {
                style.align_self = v;
            }
        }
        "align-content" => {
            if let Some(v) = AlignValue::parse(val) {
                style.align_content = v;
            }
        }
        "justify-items" => {
            if let Some(v) = AlignValue::parse(val) {
                style.justify_items = v;
            }
        }
        "justify-self" => {
            if let Some(v) = AlignValue::parse(val) {
                style.justify_self = v;
            }
        }
        "justify-content" => {
            if let Some(v) = AlignValue::parse(val) {
                style.justify_content = v;
            }
        }
        // Shorthand: `place-items: <align-items> [<justify-items>]?`
        "place-items" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_items = a;
                style.justify_items = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "place-self" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_self = a;
                style.justify_self = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "background-image" => {
            // CSS Backgrounds L3 §3.1. `none` сбрасывает.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.background_image = BackgroundImage::None;
            } else if let Some(url) = parse_url_value(trimmed) {
                style.background_image = BackgroundImage::Url(url);
            } else if is_gradient_function(trimmed) {
                // Gradients хранятся сырой строкой — типизация отложена.
                style.background_image = BackgroundImage::Gradient(trimmed.to_string());
            }
        }
        "background-repeat" => {
            if let Some(v) = BackgroundRepeat::parse(val) {
                style.background_repeat = v;
            }
        }
        "background-size" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.background_size = BackgroundSize::Auto;
            } else if trimmed.eq_ignore_ascii_case("cover") {
                style.background_size = BackgroundSize::Cover;
            } else if trimmed.eq_ignore_ascii_case("contain") {
                style.background_size = BackgroundSize::Contain;
            } else {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                let w = parts.first().and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks));
                let h = parts.get(1).and_then(|s| {
                    if s.eq_ignore_ascii_case("auto") {
                        None
                    } else {
                        resolve_box_length(s, em_basis, viewport, is_quirks)
                    }
                });
                if let Some(w) = w {
                    style.background_size = BackgroundSize::Length(w, h);
                }
            }
        }
        "background-attachment" => {
            if let Some(v) = BackgroundAttachment::parse(val) {
                style.background_attachment = v;
            }
        }
        "background-origin" => {
            // CSS Backgrounds L3 §3.7: border-box | padding-box | content-box.
            // Non-inherited; initial padding-box. Multi-value (для multi-background)
            // — отложено до multi-background-image (хранение Vec).
            if let Some(v) = BackgroundOrigin::parse(val) {
                style.background_origin = v;
            }
        }
        "background-clip" => {
            // CSS Backgrounds L3 §3.8 + L4 (`text`):
            // border-box | padding-box | content-box | text. Non-inherited;
            // initial border-box. Multi-value (для multi-background) —
            // отложено до multi-background-image.
            if let Some(v) = BackgroundClip::parse(val) {
                style.background_clip = v;
            }
        }
        "background-position" => {
            // CSS Backgrounds L3 §3.5. Парсер `<position>` переиспользуется
            // с `object-position` (`ObjectPosition::parse`), но default
            // для background-position другой — `0% 0%`, не `50% 50%`,
            // поэтому используется отдельная константа `background_initial`.
            // Multi-value список через запятую (для multi-background-image)
            // — отдельная задача; здесь принимается один position.
            if let Some(p) = ObjectPosition::parse(val, em_basis, viewport) {
                style.background_position = p;
            }
        }
        "will-change" => {
            // CSS Will Change L1: `auto | <ident-list>`. Lenient parser —
            // comma-separated ident-имена.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.will_change = Vec::new();
            } else {
                style.will_change = trimmed
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && is_css_ident(s))
                    .collect();
            }
        }
        "position" => {
            if let Some(v) = Position::parse(val) {
                style.position = v;
            }
        }
        "z-index" => {
            // CSS Positioned Layout L3 §9.3 — `auto | <integer>`.
            // `auto` → None (stacking context зависит от других триггеров);
            // целое → Some(n).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.z_index = None;
            } else if let Ok(n) = trimmed.parse::<i32>() {
                style.z_index = Some(n);
            }
        }
        "isolation" => {
            if let Some(v) = Isolation::parse(val) {
                style.isolation = v;
            }
        }
        "mix-blend-mode" => {
            if let Some(v) = MixBlendMode::parse(val) {
                style.mix_blend_mode = v;
            }
        }
        "pointer-events" => {
            if let Some(v) = PointerEvents::parse(val) {
                style.pointer_events = v;
            }
        }
        "user-select" => {
            if let Some(v) = UserSelect::parse(val) {
                style.user_select = v;
            }
        }
        "scroll-behavior" => {
            if let Some(v) = ScrollBehavior::parse(val) {
                style.scroll_behavior = v;
            }
        }
        "scroll-snap-type" => {
            if let Some(v) = parse_scroll_snap_type(val) {
                style.scroll_snap_type = v;
            }
        }
        "scroll-snap-align" => {
            if let Some(v) = parse_scroll_snap_align(val) {
                style.scroll_snap_align = v;
            }
        }
        "scroll-snap-stop" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "normal" => style.scroll_snap_stop = ScrollSnapStop::Normal,
                "always" => style.scroll_snap_stop = ScrollSnapStop::Always,
                _ => {}
            }
        }
        "scroll-margin-top" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_top = px;
            }
        }
        "scroll-margin-right" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_right = px;
            }
        }
        "scroll-margin-bottom" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_bottom = px;
            }
        }
        "scroll-margin-left" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_left = px;
            }
        }
        "scroll-margin" => {
            let parts: Vec<f32> = val
                .split_whitespace()
                .filter_map(|p| resolve_box_length(p, em_basis, viewport, is_quirks))
                .collect();
            let (t, r, b, l) = expand_4_sides(&parts);
            style.scroll_margin_top = t;
            style.scroll_margin_right = r;
            style.scroll_margin_bottom = b;
            style.scroll_margin_left = l;
        }
        "scroll-padding-top" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_top = px;
            }
        }
        "scroll-padding-right" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_right = px;
            }
        }
        "scroll-padding-bottom" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_bottom = px;
            }
        }
        "scroll-padding-left" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_left = px;
            }
        }
        "scroll-padding" => {
            let parts: Vec<f32> = val
                .split_whitespace()
                .filter_map(|p| resolve_box_length(p, em_basis, viewport, is_quirks))
                .collect();
            let (t, r, b, l) = expand_4_sides(&parts);
            style.scroll_padding_top = t;
            style.scroll_padding_right = r;
            style.scroll_padding_bottom = b;
            style.scroll_padding_left = l;
        }
        "overscroll-behavior-x" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_x = v;
            }
        }
        "overscroll-behavior-y" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_y = v;
            }
        }
        "overscroll-behavior" => {
            // Shorthand: 1 значение — оба, 2 значения — x и y.
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(x) = parts.first().and_then(|p| parse_overscroll_behavior(p)) {
                style.overscroll_behavior_x = x;
                let y = parts.get(1).and_then(|p| parse_overscroll_behavior(p)).unwrap_or(x);
                style.overscroll_behavior_y = y;
            }
        }
        "tab-size" => {
            // CSS Text L3 §10.1: <integer> или <length>. Integer = ширина
            // в spaces; принимаем как 8px-per-space heuristic. Length —
            // resolved-px.
            let trimmed = val.trim();
            if let Ok(n) = trimmed.parse::<i32>() {
                style.tab_size = (n.max(0) as f32) * 8.0;
            } else if let Some(px) = resolve_box_length(trimmed, em_basis, viewport, is_quirks) {
                style.tab_size = px.max(0.0);
            }
        }
        "caret-color" => {
            // CSS UI L4 §6.3: auto | <color>.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.caret_color = None;
            } else if let Some(c) = parse_color_legacy(trimmed, is_quirks) {
                style.caret_color = Some(c);
            }
        }
        "overflow-wrap" | "word-wrap" => {
            // `word-wrap` — legacy alias для `overflow-wrap`.
            if let Some(v) = OverflowWrap::parse(val) {
                style.overflow_wrap = v;
            }
        }
        "word-break" => {
            if let Some(v) = WordBreak::parse(val) {
                style.word_break = v;
            }
        }
        "hyphens" => {
            if let Some(v) = Hyphens::parse(val) {
                style.hyphens = v;
            }
        }
        "transform-origin" => {
            // CSS Transforms L1 §6: <position> [<length>]?
            // Phase 0: парсим 1-3 значения как px. Keywords (center / top /
            // bottom / left / right) пока не поддерживаем.
            let parts: Vec<&str> = val.split_whitespace().collect();
            let x = parts.first().and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks)).unwrap_or(0.0);
            let y = parts.get(1).and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks)).unwrap_or(0.0);
            let z = parts.get(2).and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks)).unwrap_or(0.0);
            style.transform_origin = (x, y, z);
        }
        "perspective" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.perspective = None;
            } else if let Some(px) = resolve_box_length(trimmed, em_basis, viewport, is_quirks) {
                style.perspective = if px > 0.0 { Some(px) } else { None };
            }
        }
        "list-style-type" => {
            if let Some(v) = ListStyleType::parse(val) {
                style.list_style_type = v;
            }
        }
        "list-style-position" => {
            if let Some(v) = ListStylePosition::parse(val) {
                style.list_style_position = v;
            }
        }
        "list-style-image" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.list_style_image = None;
            } else if let Some(u) = parse_url_value(trimmed) {
                style.list_style_image = Some(u);
            }
        }
        "list-style" => {
            // Shorthand: type | position | image, в любом порядке.
            // Простой парсер: пытаемся каждое слово.
            for token in val.split_whitespace() {
                if let Some(t) = ListStyleType::parse(token) {
                    style.list_style_type = t;
                } else if let Some(p) = ListStylePosition::parse(token) {
                    style.list_style_position = p;
                } else if let Some(u) = parse_url_value(token) {
                    style.list_style_image = Some(u);
                } else if token.eq_ignore_ascii_case("none") {
                    // `none` неоднозначен: type=None И image=None. Per spec,
                    // `none` сначала применяется к type, потом к image (если
                    // повторяется). Простая трактовка: первый none → type=None,
                    // последующие → image=None.
                    if !matches!(style.list_style_type, ListStyleType::None) {
                        style.list_style_type = ListStyleType::None;
                    } else {
                        style.list_style_image = None;
                    }
                }
            }
        }
        "transition-property" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.transition_properties = Vec::new();
            } else if trimmed.eq_ignore_ascii_case("all") {
                style.transition_properties = vec!["all".to_string()];
            } else {
                style.transition_properties = trimmed
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        "transition-duration" => {
            style.transition_durations = parse_time_list(val);
        }
        "transition-delay" => {
            style.transition_delays = parse_time_list(val);
        }
        "transition-timing-function" => {
            style.transition_timing_functions = TimingFunction::parse_list(val);
        }
        "animation" => {
            apply_animation_shorthand(style, val);
        }
        "transition" => {
            apply_transition_shorthand(style, val);
        }
        "animation-name" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
                style.animation_names = Vec::new();
            } else {
                style.animation_names = split_top_level_commas(trimmed)
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
                    .collect();
            }
        }
        "animation-duration" => {
            style.animation_durations = parse_time_list(val);
        }
        "animation-delay" => {
            style.animation_delays = parse_time_list(val);
        }
        "animation-timing-function" => {
            style.animation_timing_functions = TimingFunction::parse_list(val);
        }
        "animation-iteration-count" => {
            style.animation_iteration_counts = IterationCount::parse_list(val);
        }
        "animation-direction" => {
            style.animation_directions = AnimationDirection::parse_list(val);
        }
        "animation-fill-mode" => {
            style.animation_fill_modes = AnimationFillMode::parse_list(val);
        }
        "animation-play-state" => {
            style.animation_play_states = AnimationPlayState::parse_list(val);
        }
        "mask-image" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.mask_image = BackgroundImage::None;
            } else if let Some(u) = parse_url_value(trimmed) {
                style.mask_image = BackgroundImage::Url(u);
            } else if is_gradient_function(trimmed) {
                style.mask_image = BackgroundImage::Gradient(trimmed.to_string());
            }
        }
        "mask-repeat" => {
            if let Some(v) = BackgroundRepeat::parse(val) {
                style.mask_repeat = v;
            }
        }
        "mask-size" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.mask_size = BackgroundSize::Auto;
            } else if trimmed.eq_ignore_ascii_case("cover") {
                style.mask_size = BackgroundSize::Cover;
            } else if trimmed.eq_ignore_ascii_case("contain") {
                style.mask_size = BackgroundSize::Contain;
            } else {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                let w = parts.first().and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks));
                let h = parts.get(1).and_then(|s| {
                    if s.eq_ignore_ascii_case("auto") {
                        None
                    } else {
                        resolve_box_length(s, em_basis, viewport, is_quirks)
                    }
                });
                if let Some(w) = w {
                    style.mask_size = BackgroundSize::Length(w, h);
                }
            }
        }
        "scrollbar-width" => {
            if let Some(v) = ScrollbarWidth::parse(val) {
                style.scrollbar_width = v;
            }
        }
        "scrollbar-color" => {
            // `auto` или два цвета (thumb + track).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.scrollbar_color = None;
            } else {
                // Парсим два color-значения, разделённые whitespace.
                // Простая реализация: split по `)` чтобы разделить
                // `rgb(...)` пары. Иначе — split_whitespace.
                let mut pieces: Vec<String> = Vec::new();
                let mut current = String::new();
                let mut depth = 0i32;
                for c in trimmed.chars() {
                    current.push(c);
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            pieces.push(current.trim().to_string());
                            current.clear();
                        }
                    } else if c.is_whitespace() && depth == 0 && !current.trim().is_empty() {
                        let trimmed_piece = current.trim().to_string();
                        if !trimmed_piece.is_empty() {
                            pieces.push(trimmed_piece);
                        }
                        current.clear();
                    }
                }
                if !current.trim().is_empty() {
                    pieces.push(current.trim().to_string());
                }
                pieces.retain(|p| !p.is_empty());
                if pieces.len() == 2
                    && let (Some(thumb), Some(track)) =
                        (parse_color_legacy(&pieces[0], is_quirks), parse_color_legacy(&pieces[1], is_quirks))
                {
                    style.scrollbar_color = Some((thumb, track));
                }
            }
        }
        "scrollbar-gutter" => {
            if let Some(v) = ScrollbarGutter::parse(val) {
                style.scrollbar_gutter = v;
            }
        }
        "content" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("normal") {
                style.content = Content::Normal;
            } else if trimmed.eq_ignore_ascii_case("none") {
                style.content = Content::None;
            } else {
                let items = parse_content_items(trimmed);
                if !items.is_empty() {
                    style.content = Content::Items(items);
                }
            }
        }
        "place-content" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_content = a;
                style.justify_content = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "opacity" => {
            // CSS Color L3 §3.2: <number 0..1> или <percentage>. Out-of-range
            // clamp-ается. Невалидные значения игнорируются.
            let v = val.trim();
            let parsed = if let Some(pct) = v.strip_suffix('%') {
                pct.trim().parse::<f32>().ok().map(|n| n / 100.0)
            } else {
                v.parse::<f32>().ok()
            };
            if let Some(o) = parsed {
                style.opacity = o.clamp(0.0, 1.0);
            }
        }
        "line-height" => {
            // `1.5` (unitless) — коэффициент. `1.5em` — то же самое.
            // `150%` — то же самое. `24px` / `5vh` — конкретная высота,
            // переводим в коэффициент / font_size.
            if let Ok(v) = val.parse::<f32>() {
                style.line_height = v;
            } else if let Some(len) = parse_length(val) {
                match &len {
                    Length::Px(v) => style.line_height = v / style.font_size,
                    Length::Em(v) => style.line_height = *v,
                    Length::Rem(v) => {
                        style.line_height = v * ROOT_FONT_SIZE / style.font_size;
                    }
                    Length::Percent(v) => style.line_height = v / 100.0,
                    Length::Vh(_)
                    | Length::Vw(_)
                    | Length::Vmin(_)
                    | Length::Vmax(_)
                    | Length::Calc(_) => {
                        // Резолвим в px и переводим в коэффициент.
                        // Для calc() — то же самое: если выражение содержит
                        // только unitless (`calc(1 + 0.5)`) → результат уже
                        // коэффициент, но мы не умеем сейчас отличить unitless
                        // от px; делим всегда на font_size — это даёт верный
                        // ответ для length-результатов и неверный для чистых
                        // чисел внутри calc. Phase 0 ограничение: для чистых
                        // чисел используйте bare-form `line-height: 1.5`.
                        if let Some(px) = len.resolve(em_basis, None, viewport) {
                            style.line_height = px / style.font_size;
                        }
                    }
                }
            }
        }
        "margin" => {
            if let Some((t, r, b, l)) = parse_box_shorthand(val, em_basis, viewport, is_quirks) {
                style.margin_top = t;
                style.margin_right = r;
                style.margin_bottom = b;
                style.margin_left = l;
            }
        }
        "margin-top" => set_box_length(&mut style.margin_top, val, em_basis, viewport, is_quirks),
        "margin-right" => set_box_length(&mut style.margin_right, val, em_basis, viewport, is_quirks),
        "margin-bottom" => set_box_length(&mut style.margin_bottom, val, em_basis, viewport, is_quirks),
        "margin-left" => set_box_length(&mut style.margin_left, val, em_basis, viewport, is_quirks),
        "padding" => {
            if let Some((t, r, b, l)) = parse_box_shorthand(val, em_basis, viewport, is_quirks) {
                style.padding_top = t;
                style.padding_right = r;
                style.padding_bottom = b;
                style.padding_left = l;
            }
        }
        "padding-top" => set_box_length(&mut style.padding_top, val, em_basis, viewport, is_quirks),
        "padding-right" => set_box_length(&mut style.padding_right, val, em_basis, viewport, is_quirks),
        "padding-bottom" => set_box_length(&mut style.padding_bottom, val, em_basis, viewport, is_quirks),
        "padding-left" => set_box_length(&mut style.padding_left, val, em_basis, viewport, is_quirks),
        "text-decoration" => {
            // Shorthand: `<line> || <style> || <color>` в любом порядке (CSS Text
            // Decoration L3 §2.1). Спецификация L3 не включает thickness в
            // shorthand — для неё отдельный longhand. Per spec shorthand сбрасывает
            // все 4 longhand-а к initial, затем применяет указанные значения.
            let parsed = parse_text_decoration_shorthand_q(val, is_quirks);
            // Если shorthand был полностью невалиден (ни одного распознанного
            // токена) — declaration ignored. Распознаём по тому, что хоть
            // что-то распарсилось.
            if parsed.any_recognized {
                style.text_decoration_line = parsed.line.unwrap_or_default();
                style.text_decoration_color = parsed.color;
                style.text_decoration_style = parsed.style.unwrap_or_default();
                // text-decoration-thickness shorthand-ом не сбрасывается
                // (исключена из L3 shorthand-а; см. §2.1).
            }
        }
        "text-decoration-line" => {
            let parsed = parse_text_decoration_shorthand_q(val, is_quirks);
            if let Some(d) = parsed.line {
                style.text_decoration_line = d;
            }
        }
        "text-decoration-color" => {
            // `currentcolor` сбрасывает в None — даёт fallback на style.color
            // при рендеринге. CSS3 не описывает явное «возврат к default»,
            // но `currentColor` имеет ту же семантику.
            if val.eq_ignore_ascii_case("currentcolor") {
                style.text_decoration_color = None;
            } else if let Some(c) = parse_color_legacy(val, is_quirks) {
                style.text_decoration_color = Some(c);
            }
        }
        "text-decoration-style" => {
            // CSS Text Decoration L3 §2.2 — единственный keyword из
            // `solid | double | dotted | dashed | wavy`. Невалидное — ignored.
            if let Some(s) = TextDecorationStyle::parse(val) {
                style.text_decoration_style = s;
            }
        }
        "text-decoration-thickness" => {
            // CSS Text Decoration L3 §2.3 — `auto | from-font | <length> |
            // <percentage>`. Невалидное — ignored.
            if let Some(t) = parse_text_decoration_thickness(val, em_basis, viewport) {
                style.text_decoration_thickness = t;
            }
        }
        "text-emphasis-style" => {
            // CSS Text Decoration L4 §5.3 — `none | [ filled | open ] ||
            // [ dot | circle | double-circle | triangle | sesame ] | <string>`.
            if let Some(s) = parse_text_emphasis_style(val) {
                style.text_emphasis_style = s;
            }
        }
        "text-emphasis-color" => {
            // CSS Text Decoration L4 §5.4. `currentcolor` → None (fallback на
            // style.color при рендеринге; тот же паттерн, что у
            // text-decoration-color).
            if val.eq_ignore_ascii_case("currentcolor") {
                style.text_emphasis_color = None;
            } else if let Some(c) = parse_color_legacy(val, is_quirks) {
                style.text_emphasis_color = Some(c);
            }
        }
        "text-emphasis-position" => {
            // CSS Text Decoration L4 §5.5 — `[over | under] && [right | left]?`.
            if let Some(p) = parse_text_emphasis_position(val) {
                style.text_emphasis_position = p;
            }
        }
        "text-emphasis" => {
            // CSS Text Decoration L4 §5.6 — shorthand для -style и -color
            // (НЕ включает -position по spec). Сбрасывает обе longhand-ы в
            // initial и потом извлекает style+color из value.
            apply_text_emphasis_shorthand(style, val, is_quirks);
        }
        // ── Borders ───────────────────────────────────────────────────────────
        "border" => apply_border_shorthand(style, val, em_basis, viewport, is_quirks),
        "border-top" => apply_border_side_shorthand(
            &mut style.border_top_width, &mut style.border_top_style,
            &mut style.border_top_color, val, em_basis, viewport, is_quirks),
        "border-right" => apply_border_side_shorthand(
            &mut style.border_right_width, &mut style.border_right_style,
            &mut style.border_right_color, val, em_basis, viewport, is_quirks),
        "border-bottom" => apply_border_side_shorthand(
            &mut style.border_bottom_width, &mut style.border_bottom_style,
            &mut style.border_bottom_color, val, em_basis, viewport, is_quirks),
        "border-left" => apply_border_side_shorthand(
            &mut style.border_left_width, &mut style.border_left_style,
            &mut style.border_left_color, val, em_basis, viewport, is_quirks),
        "border-width" => {
            let sides = expand_border_4(val);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport, is_quirks) { style.border_top_width = v; }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport, is_quirks) { style.border_right_width = v; }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport, is_quirks) { style.border_bottom_width = v; }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport, is_quirks) { style.border_left_width = v; }
        }
        "border-style" => {
            let sides = expand_border_4(val);
            style.border_top_style = parse_border_style_kw(sides[0]);
            style.border_right_style = parse_border_style_kw(sides[1]);
            style.border_bottom_style = parse_border_style_kw(sides[2]);
            style.border_left_style = parse_border_style_kw(sides[3]);
        }
        "border-color" => {
            let sides = expand_border_4(val);
            if let Some(c) = parse_color_legacy(sides[0], is_quirks) { style.border_top_color = Some(c); }
            if let Some(c) = parse_color_legacy(sides[1], is_quirks) { style.border_right_color = Some(c); }
            if let Some(c) = parse_color_legacy(sides[2], is_quirks) { style.border_bottom_color = Some(c); }
            if let Some(c) = parse_color_legacy(sides[3], is_quirks) { style.border_left_color = Some(c); }
        }
        "border-radius" => {
            // CSS Backgrounds L3 §5.5 shorthand. Поддерживаем только
            // horizontal-radius (без `/`-formed elliptical часть). 1-4 токена
            // по правилу expand_border_4 (TL TR BR BL).
            // Формы вроде `5px / 10px` (elliptical) Phase 0 не поддерживает —
            // берём первую часть до `/`.
            let h_part = val.split('/').next().unwrap_or(val);
            let sides = expand_border_4(h_part);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport, is_quirks) {
                style.border_top_left_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport, is_quirks) {
                style.border_top_right_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius = v.max(0.0);
            }
        }
        "border-top-left-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_top_left_radius = v.max(0.0);
            }
        }
        "border-top-right-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_top_right_radius = v.max(0.0);
            }
        }
        "border-bottom-right-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius = v.max(0.0);
            }
        }
        "border-bottom-left-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius = v.max(0.0);
            }
        }
        "border-top-width" => set_box_length(&mut style.border_top_width, val, em_basis, viewport, is_quirks),
        "border-right-width" => set_box_length(&mut style.border_right_width, val, em_basis, viewport, is_quirks),
        "border-bottom-width" => set_box_length(&mut style.border_bottom_width, val, em_basis, viewport, is_quirks),
        "border-left-width" => set_box_length(&mut style.border_left_width, val, em_basis, viewport, is_quirks),
        "border-top-style" => style.border_top_style = parse_border_style_kw(val),
        "border-right-style" => style.border_right_style = parse_border_style_kw(val),
        "border-bottom-style" => style.border_bottom_style = parse_border_style_kw(val),
        "border-left-style" => style.border_left_style = parse_border_style_kw(val),
        "border-top-color" => { if let Some(c) = parse_color_legacy(val, is_quirks) { style.border_top_color = Some(c); } }
        "border-right-color" => { if let Some(c) = parse_color_legacy(val, is_quirks) { style.border_right_color = Some(c); } }
        "border-bottom-color" => { if let Some(c) = parse_color_legacy(val, is_quirks) { style.border_bottom_color = Some(c); } }
        "border-left-color" => { if let Some(c) = parse_color_legacy(val, is_quirks) { style.border_left_color = Some(c); } }
        "box-sizing" => {
            style.box_sizing = match val.trim().to_ascii_lowercase().as_str() {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => style.box_sizing,
            };
        }
        _ => {}
    }
}

/// Результат разбора `text-decoration` shorthand-а.
///
/// `any_recognized` отличает «полностью невалидный shorthand → declaration
/// ignored» от «частично распознанный → применяется initial для непроставленных
/// сторон». Без него `text-decoration: foo` молча сбрасывал бы существующее
/// значение к initial.
pub(crate) struct ParsedTextDecorationShorthand {
    pub line: Option<TextDecorationLine>,
    pub color: Option<Color>,
    pub style: Option<TextDecorationStyle>,
    pub any_recognized: bool,
}

/// Разбирает `text-decoration` shorthand или `text-decoration-line`.
///
/// CSS Text Decoration L3 §2.1 shorthand: `<line> || <style> || <color>` в любом
/// порядке. `text-decoration-thickness` исключена из L3 shorthand-а (L4
/// собирается её туда вернуть; пока следуем L3).
///
/// Phase 0 keyword-ы линий: `underline`, `overline`, `line-through`, `none`.
/// `none` сбрасывает все линии (CSS3 «none — initial value», явный сброс
/// побеждает другие line-keyword-ы).
/// Стиль: `solid`/`wavy`/`dashed`/`dotted`/`double`. `blink` (CSS2 deprecated)
/// тихо поглощаем, чтобы не попадал в color-парсер.
///
/// `currentcolor` keyword сбрасывает color в None (= fallback на currentColor
/// при рендеринге).
/// Wrapper для тестов и потребителей вне quirks-aware каскада.
#[cfg(test)]
fn parse_text_decoration_shorthand(val: &str) -> ParsedTextDecorationShorthand {
    parse_text_decoration_shorthand_q(val, false)
}

fn parse_text_decoration_shorthand_q(val: &str, is_quirks: bool) -> ParsedTextDecorationShorthand {
    let mut out_line = TextDecorationLine::default();
    let mut any_line = false;
    let mut none_seen = false;
    let mut out_style: Option<TextDecorationStyle> = None;
    let mut color: Option<Color> = None;
    let mut color_currentcolor = false;
    let mut any_recognized = false;
    // Цвет может быть многословным: `rgb(0, 0, 0)`, `hsl(0 0% 0% / 1)`, …
    // Соберём «не-линия / не-стиль» токены и попытаемся склеить.
    let mut residue: Vec<&str> = Vec::new();
    for token in val.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "none" => {
                none_seen = true;
                any_line = true;
                any_recognized = true;
            }
            "underline" => {
                out_line.underline = true;
                any_line = true;
                any_recognized = true;
            }
            "overline" => {
                out_line.overline = true;
                any_line = true;
                any_recognized = true;
            }
            "line-through" => {
                out_line.line_through = true;
                any_line = true;
                any_recognized = true;
            }
            "solid" => {
                out_style = Some(TextDecorationStyle::Solid);
                any_recognized = true;
            }
            "double" => {
                out_style = Some(TextDecorationStyle::Double);
                any_recognized = true;
            }
            "dotted" => {
                out_style = Some(TextDecorationStyle::Dotted);
                any_recognized = true;
            }
            "dashed" => {
                out_style = Some(TextDecorationStyle::Dashed);
                any_recognized = true;
            }
            "wavy" => {
                out_style = Some(TextDecorationStyle::Wavy);
                any_recognized = true;
            }
            "blink" => {
                // CSS2 deprecated; токен поглощаем, чтобы он не попал в
                // color-парсер.
                any_recognized = true;
            }
            "currentcolor" => {
                color_currentcolor = true;
                any_recognized = true;
            }
            _ => residue.push(token),
        }
    }
    if !residue.is_empty() {
        // Попробуем сначала весь residue (на случай color-функции с
        // пробелами: `rgb(0 0 0)` → токены `rgb(0`, `0`, `0)`).
        let joined = residue.join(" ");
        if let Some(c) = parse_color_legacy(joined.trim(), is_quirks) {
            color = Some(c);
            any_recognized = true;
        } else {
            // Иначе пробуем токен за токеном — для named-color / hex без
            // пробелов внутри.
            for tok in &residue {
                if let Some(c) = parse_color_legacy(tok, is_quirks) {
                    color = Some(c);
                    any_recognized = true;
                    break;
                }
            }
        }
    }
    if color_currentcolor && color.is_none() {
        // `currentcolor` явно встретился — но это не value «нет цвета»;
        // у нас представление currentColor = None, поэтому не ставим color
        // — кто-то снаружи решит, что это сброс. В shorthand `text-decoration`
        // ничего не делаем (style.text_decoration_color остаётся как есть).
    }
    let line = if any_line {
        if none_seen { Some(TextDecorationLine::default()) } else { Some(out_line) }
    } else {
        None
    };
    ParsedTextDecorationShorthand {
        line,
        color,
        style: out_style,
        any_recognized,
    }
}

/// Парсит значение `text-decoration-thickness` (CSS Text Decoration L3 §2.3).
///
/// `auto | from-font | <length> | <percentage>`. Длина резолвится в
/// resolved-px через [`Length::resolve`] (поддерживает px/em/rem/vw/vh/calc).
/// Процент сохраняется как fraction (`5%` → 0.05) — финальное домножение на
/// parent.font_size происходит в renderer-е по spec.
fn parse_text_decoration_thickness(
    val: &str,
    em_basis: f32,
    viewport: Size,
) -> Option<TextDecorationThickness> {
    let trimmed = val.trim();
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "auto" => return Some(TextDecorationThickness::Auto),
        "from-font" => return Some(TextDecorationThickness::FromFont),
        _ => {}
    }
    if let Some(pct_str) = trimmed.strip_suffix('%')
        && let Ok(n) = pct_str.trim().parse::<f32>()
    {
        return Some(TextDecorationThickness::Percentage(n / 100.0));
    }
    let len = parse_length(trimmed)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(TextDecorationThickness::Length(px))
}

fn parse_text_emphasis_shape(s: &str) -> Option<TextEmphasisShape> {
    match s.to_ascii_lowercase().as_str() {
        "dot" => Some(TextEmphasisShape::Dot),
        "circle" => Some(TextEmphasisShape::Circle),
        "double-circle" => Some(TextEmphasisShape::DoubleCircle),
        "triangle" => Some(TextEmphasisShape::Triangle),
        "sesame" => Some(TextEmphasisShape::Sesame),
        _ => None,
    }
}

fn parse_text_emphasis_fill(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "filled" => Some(true),
        "open" => Some(false),
        _ => None,
    }
}

/// Извлекает первый строковый литерал в value: `"X"` или `'X'`. Возвращает
/// (content_without_quotes, rest_after_close). Невалидное / unterminated → None.
fn extract_first_string(val: &str) -> Option<(String, &str)> {
    let trimmed = val.trim_start();
    let mut chars = trimmed.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    for (i, ch) in chars {
        if ch == quote {
            let start_byte = trimmed.char_indices().next()?.0 + quote.len_utf8();
            let content = trimmed[start_byte..i].to_string();
            return Some((content, &trimmed[i + ch.len_utf8()..]));
        }
    }
    None
}

/// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Returns `None` если
/// value не парсится (invalid declaration ignored).
fn parse_text_emphasis_style(val: &str) -> Option<TextEmphasisStyle> {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Some(TextEmphasisStyle::None);
    }
    if let Some((s, rest)) = extract_first_string(trimmed) {
        if !rest.trim().is_empty() {
            return None;
        }
        return Some(TextEmphasisStyle::String(s));
    }
    let mut fill: Option<bool> = None;
    let mut shape: Option<TextEmphasisShape> = None;
    for tok in trimmed.split_whitespace() {
        if let Some(f) = parse_text_emphasis_fill(tok) {
            if fill.is_some() {
                return None;
            }
            fill = Some(f);
        } else if let Some(sh) = parse_text_emphasis_shape(tok) {
            if shape.is_some() {
                return None;
            }
            shape = Some(sh);
        } else {
            return None;
        }
    }
    if fill.is_none() && shape.is_none() {
        return None;
    }
    Some(TextEmphasisStyle::Symbol {
        filled: fill.unwrap_or(true),
        shape: shape.unwrap_or(TextEmphasisShape::Circle),
    })
}

/// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Grammar
/// `[ over | under ] && [ right | left ]?`. Spec: vertical axis (over/under)
/// обязателен, horizontal axis (right/left) опционален с default `right`.
fn parse_text_emphasis_position(val: &str) -> Option<TextEmphasisPosition> {
    let mut over: Option<bool> = None;
    let mut right: Option<bool> = None;
    for tok in val.split_whitespace() {
        match tok.to_ascii_lowercase().as_str() {
            "over" => {
                if over.is_some() {
                    return None;
                }
                over = Some(true);
            }
            "under" => {
                if over.is_some() {
                    return None;
                }
                over = Some(false);
            }
            "right" => {
                if right.is_some() {
                    return None;
                }
                right = Some(true);
            }
            "left" => {
                if right.is_some() {
                    return None;
                }
                right = Some(false);
            }
            _ => return None,
        }
    }
    let over = over?;
    let right = right.unwrap_or(true);
    Some(match (over, right) {
        (true, true) => TextEmphasisPosition::OverRight,
        (true, false) => TextEmphasisPosition::OverLeft,
        (false, true) => TextEmphasisPosition::UnderRight,
        (false, false) => TextEmphasisPosition::UnderLeft,
    })
}

/// CSS Text Decoration L4 §5.6 — `text-emphasis` shorthand для `-style` и
/// `-color`. По spec position НЕ часть shorthand-а.
///
/// Извлекает первый color-токен (consumes полностью) и оставшийся текст
/// парсит как text-emphasis-style. Невалидные cases — оба longhand-а
/// сбрасываются к initial.
fn apply_text_emphasis_shorthand(style: &mut ComputedStyle, val: &str, is_quirks: bool) {
    style.text_emphasis_style = TextEmphasisStyle::None;
    style.text_emphasis_color = None;
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return;
    }

    // string-форма `text-emphasis: "★"` — без других токенов.
    if let Some((s, rest)) = extract_first_string(trimmed)
        && rest.trim().is_empty()
    {
        style.text_emphasis_style = TextEmphasisStyle::String(s);
        return;
    }

    let mut color: Option<Color> = None;
    let mut saw_currentcolor = false;
    let mut style_tokens: Vec<&str> = Vec::new();
    for tok in trimmed.split_whitespace() {
        if tok.eq_ignore_ascii_case("currentcolor") {
            if color.is_some() || saw_currentcolor {
                return;
            }
            saw_currentcolor = true;
            continue;
        }
        if parse_text_emphasis_fill(tok).is_some() || parse_text_emphasis_shape(tok).is_some() {
            style_tokens.push(tok);
            continue;
        }
        if !saw_currentcolor && color.is_none()
            && let Some(c) = parse_color_legacy(tok, is_quirks)
        {
            color = Some(c);
            continue;
        }
        return;
    }

    style.text_emphasis_color = if saw_currentcolor { None } else { color };

    if style_tokens.is_empty() {
        return;
    }
    let joined = style_tokens.join(" ");
    if joined.eq_ignore_ascii_case("none") {
        style.text_emphasis_style = TextEmphasisStyle::None;
        return;
    }
    if let Some(s) = parse_text_emphasis_style(&joined) {
        style.text_emphasis_style = s;
    }
}

/// CSS Text Module Level 4 §6.4.3 — `text-wrap` shorthand.
///
/// Сбрасывает обе longhand-компоненты (`text-wrap-mode` / `text-wrap-style`)
/// к initial-value и применяет распознанные токены. Грамматика
/// `<'text-wrap-mode'> || <'text-wrap-style'>` — 1..=2 keyword-а, любой
/// порядок, без повторов внутри своего слота. Нераспознанный токен ⇒
/// весь shorthand невалиден (initial-значения сохраняются как «после reset»).
fn apply_text_wrap_shorthand(style: &mut ComputedStyle, val: &str) {
    style.text_wrap_mode = TextWrapMode::Wrap;
    style.text_wrap_style = TextWrapStyle::Auto;

    let mut mode: Option<TextWrapMode> = None;
    let mut wrap_style: Option<TextWrapStyle> = None;
    for tok in val.split_whitespace() {
        if let Some(m) = TextWrapMode::parse(tok) {
            if mode.is_some() {
                return;
            }
            mode = Some(m);
            continue;
        }
        if let Some(s) = TextWrapStyle::parse(tok) {
            if wrap_style.is_some() {
                return;
            }
            wrap_style = Some(s);
            continue;
        }
        return;
    }
    if let Some(m) = mode {
        style.text_wrap_mode = m;
    }
    if let Some(s) = wrap_style {
        style.text_wrap_style = s;
    }
}

/// CSS Cascade L4 §7 — применить CSS-wide keyword к одному свойству.
///
/// Источник значения:
/// - `Inherit` — всегда родительский computed value (для любого свойства).
/// - `Initial` — всегда initial value свойства из спецификации
///   (берётся из `ComputedStyle::root()`).
/// - `Unset` / `Revert` — для inherited-свойств работает как `Inherit`,
///   для non-inherited как `Initial`. `Revert` в Phase 0 ≡ `Unset`
///   (UA / User origin не отделены чётко, только UA-hints для italic/bold).
///
/// Per-property список синхронизирован с `apply_declaration` и `compute_style`-init —
/// неизвестные имена молча игнорируются.
fn apply_css_wide_keyword(
    style: &mut ComputedStyle,
    prop: &str,
    kw: CssWideKeyword,
    inherited: &ComputedStyle,
) {
    use CssWideKeyword::{Inherit, Revert, Unset};
    // Initial-значения как у root документа. ComputedStyle::root() выделяет
    // несколько Vec/HashMap, но эта функция вызывается только при обнаружении
    // CSS-wide-keyword в декларации — редкий путь, накладные расходы
    // незаметны на типичной странице.
    let init = ComputedStyle::root();

    // Helper «inherited property»: Inherit/Unset/Revert → inherited, Initial → init.
    let inh = matches!(kw, Inherit | Unset | Revert);
    // Helper «non-inherited property»: Inherit → inherited, Initial/Unset/Revert → init.
    let inh_only_inherit = matches!(kw, Inherit);

    match prop {
        // ──────── Inherited properties ────────
        "color" => style.color = if inh { inherited.color } else { init.color },
        "font-size" => {
            // Font-size уже обработан в apply_font_size (pre-pass). Здесь
            // повторно — если main-pass почему-то его коснётся, оставляем
            // корректный итог.
            style.font_size = if inh { inherited.font_size } else { init.font_size };
        }
        "line-height" => {
            style.line_height = if inh { inherited.line_height } else { init.line_height };
        }
        "font-style" => {
            style.font_style = if inh { inherited.font_style } else { init.font_style };
        }
        "font-weight" => {
            style.font_weight = if inh { inherited.font_weight } else { init.font_weight };
        }
        "font-variant" | "font-variant-caps" => {
            style.font_variant = if inh { inherited.font_variant } else { init.font_variant };
        }
        "font-stretch" => {
            style.font_stretch = if inh { inherited.font_stretch } else { init.font_stretch };
        }
        "font-family" => {
            style.font_family = if inh {
                inherited.font_family.clone()
            } else {
                init.font_family.clone()
            };
        }
        "text-align" => {
            style.text_align = if inh { inherited.text_align } else { init.text_align };
        }
        "direction" => {
            style.direction = if inh { inherited.direction } else { init.direction };
        }
        "text-transform" => {
            style.text_transform = if inh { inherited.text_transform } else { init.text_transform };
        }
        "white-space" => {
            style.white_space = if inh { inherited.white_space } else { init.white_space };
        }
        "text-indent" => {
            style.text_indent = if inh { inherited.text_indent } else { init.text_indent };
        }
        "letter-spacing" => {
            style.letter_spacing = if inh { inherited.letter_spacing } else { init.letter_spacing };
        }
        "word-spacing" => {
            style.word_spacing = if inh { inherited.word_spacing } else { init.word_spacing };
        }
        "text-decoration-line" | "text-decoration" => {
            style.text_decoration_line = if inh {
                inherited.text_decoration_line
            } else {
                init.text_decoration_line
            };
            style.text_decoration_color = if inh {
                inherited.text_decoration_color
            } else {
                init.text_decoration_color
            };
            // L3 shorthand сбрасывает также style (но не thickness — он
            // исключён из L3 shorthand-а; см. parse_text_decoration_shorthand_q).
            if prop == "text-decoration" {
                style.text_decoration_style = if inh {
                    inherited.text_decoration_style
                } else {
                    init.text_decoration_style
                };
            }
        }
        "text-decoration-color" => {
            style.text_decoration_color = if inh {
                inherited.text_decoration_color
            } else {
                init.text_decoration_color
            };
        }
        "text-decoration-style" => {
            style.text_decoration_style = if inh {
                inherited.text_decoration_style
            } else {
                init.text_decoration_style
            };
        }
        "text-decoration-thickness" => {
            style.text_decoration_thickness = if inh {
                inherited.text_decoration_thickness
            } else {
                init.text_decoration_thickness
            };
        }
        "text-emphasis-style" | "text-emphasis" => {
            style.text_emphasis_style = if inh {
                inherited.text_emphasis_style.clone()
            } else {
                init.text_emphasis_style.clone()
            };
            if prop == "text-emphasis" {
                style.text_emphasis_color = if inh {
                    inherited.text_emphasis_color
                } else {
                    init.text_emphasis_color
                };
            }
        }
        "text-emphasis-color" => {
            style.text_emphasis_color = if inh {
                inherited.text_emphasis_color
            } else {
                init.text_emphasis_color
            };
        }
        "text-emphasis-position" => {
            style.text_emphasis_position = if inh {
                inherited.text_emphasis_position
            } else {
                init.text_emphasis_position
            };
        }
        "text-shadow" => {
            style.text_shadow = if inh {
                inherited.text_shadow.clone()
            } else {
                init.text_shadow.clone()
            };
        }
        "visibility" => {
            style.visibility = if inh { inherited.visibility } else { init.visibility };
        }
        "cursor" => {
            style.cursor = if inh { inherited.cursor } else { init.cursor };
        }
        "accent-color" => {
            style.accent_color = if inh { inherited.accent_color } else { init.accent_color };
        }

        // ──────── Non-inherited properties ────────
        "display" => {
            style.display = if inh_only_inherit { inherited.display } else { init.display };
        }
        "background-color" | "background" => {
            style.background_color = if inh_only_inherit {
                inherited.background_color
            } else {
                init.background_color
            };
        }
        "width" => style.width = if inh_only_inherit { inherited.width } else { init.width },
        "height" => style.height = if inh_only_inherit { inherited.height } else { init.height },
        "min-width" => {
            style.min_width = if inh_only_inherit { inherited.min_width } else { init.min_width };
        }
        "max-width" => {
            style.max_width = if inh_only_inherit { inherited.max_width } else { init.max_width };
        }
        "min-height" => {
            style.min_height = if inh_only_inherit { inherited.min_height } else { init.min_height };
        }
        "max-height" => {
            style.max_height = if inh_only_inherit { inherited.max_height } else { init.max_height };
        }
        "margin-top" => {
            style.margin_top = if inh_only_inherit { inherited.margin_top } else { init.margin_top };
        }
        "margin-right" => {
            style.margin_right = if inh_only_inherit { inherited.margin_right } else { init.margin_right };
        }
        "margin-bottom" => {
            style.margin_bottom = if inh_only_inherit { inherited.margin_bottom } else { init.margin_bottom };
        }
        "margin-left" => {
            style.margin_left = if inh_only_inherit { inherited.margin_left } else { init.margin_left };
        }
        "margin" => {
            // shorthand → reset все 4 стороны
            let (t, r, b, l) = if inh_only_inherit {
                (inherited.margin_top, inherited.margin_right, inherited.margin_bottom, inherited.margin_left)
            } else {
                (init.margin_top, init.margin_right, init.margin_bottom, init.margin_left)
            };
            style.margin_top = t;
            style.margin_right = r;
            style.margin_bottom = b;
            style.margin_left = l;
        }
        "padding-top" => {
            style.padding_top = if inh_only_inherit { inherited.padding_top } else { init.padding_top };
        }
        "padding-right" => {
            style.padding_right = if inh_only_inherit { inherited.padding_right } else { init.padding_right };
        }
        "padding-bottom" => {
            style.padding_bottom = if inh_only_inherit { inherited.padding_bottom } else { init.padding_bottom };
        }
        "padding-left" => {
            style.padding_left = if inh_only_inherit { inherited.padding_left } else { init.padding_left };
        }
        "padding" => {
            let (t, r, b, l) = if inh_only_inherit {
                (inherited.padding_top, inherited.padding_right, inherited.padding_bottom, inherited.padding_left)
            } else {
                (init.padding_top, init.padding_right, init.padding_bottom, init.padding_left)
            };
            style.padding_top = t;
            style.padding_right = r;
            style.padding_bottom = b;
            style.padding_left = l;
        }
        "box-sizing" => {
            style.box_sizing = if inh_only_inherit { inherited.box_sizing } else { init.box_sizing };
        }
        "opacity" => {
            style.opacity = if inh_only_inherit { inherited.opacity } else { init.opacity };
        }
        "overflow" => {
            let (x, y) = if inh_only_inherit {
                (inherited.overflow_x, inherited.overflow_y)
            } else {
                (init.overflow_x, init.overflow_y)
            };
            style.overflow_x = x;
            style.overflow_y = y;
        }
        "overflow-x" => {
            style.overflow_x = if inh_only_inherit { inherited.overflow_x } else { init.overflow_x };
        }
        "overflow-y" => {
            style.overflow_y = if inh_only_inherit { inherited.overflow_y } else { init.overflow_y };
        }
        "text-overflow" => {
            style.text_overflow = if inh_only_inherit { inherited.text_overflow } else { init.text_overflow };
        }
        "box-shadow" => {
            style.box_shadow = if inh_only_inherit {
                inherited.box_shadow.clone()
            } else {
                init.box_shadow.clone()
            };
        }
        "outline-width" => {
            style.outline_width = if inh_only_inherit { inherited.outline_width } else { init.outline_width };
        }
        "outline-style" => {
            style.outline_style = if inh_only_inherit { inherited.outline_style } else { init.outline_style };
        }
        "outline-color" => {
            style.outline_color = if inh_only_inherit { inherited.outline_color } else { init.outline_color };
        }
        "outline-offset" => {
            style.outline_offset = if inh_only_inherit { inherited.outline_offset } else { init.outline_offset };
        }
        "outline" => {
            // shorthand: width + style + color (offset не входит per spec).
            if inh_only_inherit {
                style.outline_width = inherited.outline_width;
                style.outline_style = inherited.outline_style;
                style.outline_color = inherited.outline_color;
            } else {
                style.outline_width = init.outline_width;
                style.outline_style = init.outline_style;
                style.outline_color = init.outline_color;
            }
        }
        // border-* per-side individual + shorthands
        "border-top-width" => style.border_top_width = if inh_only_inherit { inherited.border_top_width } else { init.border_top_width },
        "border-right-width" => style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width },
        "border-bottom-width" => style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width },
        "border-left-width" => style.border_left_width = if inh_only_inherit { inherited.border_left_width } else { init.border_left_width },
        "border-top-style" => style.border_top_style = if inh_only_inherit { inherited.border_top_style } else { init.border_top_style },
        "border-right-style" => style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style },
        "border-bottom-style" => style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style },
        "border-left-style" => style.border_left_style = if inh_only_inherit { inherited.border_left_style } else { init.border_left_style },
        "border-top-color" => style.border_top_color = if inh_only_inherit { inherited.border_top_color } else { init.border_top_color },
        "border-right-color" => style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color },
        "border-bottom-color" => style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color },
        "border-left-color" => style.border_left_color = if inh_only_inherit { inherited.border_left_color } else { init.border_left_color },
        // border-width / -style / -color shorthand → 4 стороны
        "border-width" => {
            let v = if inh_only_inherit {
                (inherited.border_top_width, inherited.border_right_width, inherited.border_bottom_width, inherited.border_left_width)
            } else {
                (init.border_top_width, init.border_right_width, init.border_bottom_width, init.border_left_width)
            };
            style.border_top_width = v.0;
            style.border_right_width = v.1;
            style.border_bottom_width = v.2;
            style.border_left_width = v.3;
        }
        "border-style" => {
            let v = if inh_only_inherit {
                (inherited.border_top_style, inherited.border_right_style, inherited.border_bottom_style, inherited.border_left_style)
            } else {
                (init.border_top_style, init.border_right_style, init.border_bottom_style, init.border_left_style)
            };
            style.border_top_style = v.0;
            style.border_right_style = v.1;
            style.border_bottom_style = v.2;
            style.border_left_style = v.3;
        }
        "border-color" => {
            let v = if inh_only_inherit {
                (inherited.border_top_color, inherited.border_right_color, inherited.border_bottom_color, inherited.border_left_color)
            } else {
                (init.border_top_color, init.border_right_color, init.border_bottom_color, init.border_left_color)
            };
            style.border_top_color = v.0;
            style.border_right_color = v.1;
            style.border_bottom_color = v.2;
            style.border_left_color = v.3;
        }
        // border / border-top / -right / -bottom / -left shorthand: width + style + color на сторону.
        "border" => {
            let (w, s, c) = if inh_only_inherit {
                (inherited.border_top_width, inherited.border_top_style, inherited.border_top_color)
            } else {
                (init.border_top_width, init.border_top_style, init.border_top_color)
            };
            for (sw, ss, sc) in [
                (&mut style.border_top_width, &mut style.border_top_style, &mut style.border_top_color),
                (&mut style.border_right_width, &mut style.border_right_style, &mut style.border_right_color),
                (&mut style.border_bottom_width, &mut style.border_bottom_style, &mut style.border_bottom_color),
                (&mut style.border_left_width, &mut style.border_left_style, &mut style.border_left_color),
            ] {
                *sw = w;
                *ss = s;
                *sc = c;
            }
        }
        "border-top" => {
            style.border_top_width = if inh_only_inherit { inherited.border_top_width } else { init.border_top_width };
            style.border_top_style = if inh_only_inherit { inherited.border_top_style } else { init.border_top_style };
            style.border_top_color = if inh_only_inherit { inherited.border_top_color } else { init.border_top_color };
        }
        "border-right" => {
            style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width };
            style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style };
            style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color };
        }
        "border-bottom" => {
            style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width };
            style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style };
            style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color };
        }
        "border-left" => {
            style.border_left_width = if inh_only_inherit { inherited.border_left_width } else { init.border_left_width };
            style.border_left_style = if inh_only_inherit { inherited.border_left_style } else { init.border_left_style };
            style.border_left_color = if inh_only_inherit { inherited.border_left_color } else { init.border_left_color };
        }
        // border-radius (CSS Backgrounds L3 §5) — 4 угла.
        "border-top-left-radius" => {
            style.border_top_left_radius = if inh_only_inherit { inherited.border_top_left_radius } else { init.border_top_left_radius };
        }
        "border-top-right-radius" => {
            style.border_top_right_radius = if inh_only_inherit { inherited.border_top_right_radius } else { init.border_top_right_radius };
        }
        "border-bottom-right-radius" => {
            style.border_bottom_right_radius = if inh_only_inherit { inherited.border_bottom_right_radius } else { init.border_bottom_right_radius };
        }
        "border-bottom-left-radius" => {
            style.border_bottom_left_radius = if inh_only_inherit { inherited.border_bottom_left_radius } else { init.border_bottom_left_radius };
        }
        "border-radius" => {
            let v = if inh_only_inherit {
                (inherited.border_top_left_radius, inherited.border_top_right_radius, inherited.border_bottom_right_radius, inherited.border_bottom_left_radius)
            } else {
                (init.border_top_left_radius, init.border_top_right_radius, init.border_bottom_right_radius, init.border_bottom_left_radius)
            };
            style.border_top_left_radius = v.0;
            style.border_top_right_radius = v.1;
            style.border_bottom_right_radius = v.2;
            style.border_bottom_left_radius = v.3;
        }
        // CSS Lists L3 §3 — не наследуются; Inherit пуллит из inherited,
        // прочие — initial (пустой Vec).
        "counter-reset" => {
            style.counter_reset = if inh_only_inherit {
                inherited.counter_reset.clone()
            } else {
                init.counter_reset.clone()
            };
        }
        "counter-increment" => {
            style.counter_increment = if inh_only_inherit {
                inherited.counter_increment.clone()
            } else {
                init.counter_increment.clone()
            };
        }
        // Masking / Transforms / Filter — все non-inherited.
        "clip-path" => {
            style.clip_path = if inh_only_inherit { inherited.clip_path.clone() } else { init.clip_path.clone() };
        }
        "transform" => {
            style.transform = if inh_only_inherit { inherited.transform.clone() } else { init.transform.clone() };
        }
        "filter" => {
            style.filter = if inh_only_inherit { inherited.filter.clone() } else { init.filter.clone() };
        }
        // CSS Positioned Layout / Compositing — non-inherited.
        "position" => {
            style.position = if inh_only_inherit { inherited.position } else { init.position };
        }
        "z-index" => {
            style.z_index = if inh_only_inherit { inherited.z_index } else { init.z_index };
        }
        "isolation" => {
            style.isolation = if inh_only_inherit { inherited.isolation } else { init.isolation };
        }
        "mix-blend-mode" => {
            style.mix_blend_mode = if inh_only_inherit { inherited.mix_blend_mode } else { init.mix_blend_mode };
        }
        // CSS Images L3 §5.5 — object-fit / object-position non-inherited.
        "object-fit" => {
            style.object_fit = if inh_only_inherit { inherited.object_fit } else { init.object_fit };
        }
        "object-position" => {
            style.object_position = if inh_only_inherit {
                inherited.object_position
            } else {
                init.object_position
            };
        }
        // CSS 2.1 §10.8.1 — vertical-align non-inherited.
        "vertical-align" => {
            style.vertical_align = if inh_only_inherit {
                inherited.vertical_align
            } else {
                init.vertical_align
            };
        }
        // CSS Backgrounds L3 §3.5 — background-position non-inherited.
        "background-position" => {
            style.background_position = if inh_only_inherit {
                inherited.background_position
            } else {
                init.background_position
            };
        }
        // CSS Backgrounds L3 §3.7 / §3.8 — background-origin / background-clip
        // non-inherited.
        "background-origin" => {
            style.background_origin = if inh_only_inherit {
                inherited.background_origin
            } else {
                init.background_origin
            };
        }
        "background-clip" => {
            style.background_clip = if inh_only_inherit {
                inherited.background_clip
            } else {
                init.background_clip
            };
        }
        // CSS Images L3 §6.1 — image-rendering inherited. inh — общий
        // алиас «брать inherited.value» (для inherited работает и при
        // Inherit, и при Unset; см. вычисление inh выше).
        "image-rendering" => {
            style.image_rendering = if inh {
                inherited.image_rendering
            } else {
                init.image_rendering
            };
        }
        // CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style
        // оба inherited; shorthand text-wrap раскрывается на два longhand-а
        // и применяет CSS-wide ключевое слово к каждому.
        "text-wrap-mode" => {
            style.text_wrap_mode = if inh {
                inherited.text_wrap_mode
            } else {
                init.text_wrap_mode
            };
        }
        "text-wrap-style" => {
            style.text_wrap_style = if inh {
                inherited.text_wrap_style
            } else {
                init.text_wrap_style
            };
        }
        "text-wrap" => {
            style.text_wrap_mode = if inh {
                inherited.text_wrap_mode
            } else {
                init.text_wrap_mode
            };
            style.text_wrap_style = if inh {
                inherited.text_wrap_style
            } else {
                init.text_wrap_style
            };
        }
        // Прочие / неизвестные — silent no-op.
        _ => {}
    }
}

/// Парсер CSS Lists L3 §3 `counter-reset` / `counter-increment` value.
/// Формат: `none | (<custom-ident> <integer>?)+`. Возвращает `Vec` пар
/// (имя, число); `default` подставляется когда integer не указан.
///
/// `none` (case-insensitive) → пустой `Vec`. Невалидные ident-ы и числа
/// — пропускаем без ошибки, как best-effort lenient parser.
fn parse_counter_list(value: &str, default: i32) -> Vec<(String, i32)> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut tokens = v.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        // Имя счётчика — CSS ident: ASCII alphabetic / `_` / `-` начало,
        // дальше alphanumeric / `-` / `_`. Простой strict check; пропускаем
        // токены, не похожие на ident.
        if !is_css_ident(tok) {
            continue;
        }
        // Следующий токен — опц. integer.
        let n = if let Some(&peeked) = tokens.peek() {
            if let Ok(parsed) = peeked.parse::<i32>() {
                tokens.next();
                parsed
            } else {
                default
            }
        } else {
            default
        };
        out.push((tok.to_string(), n));
    }
    out
}

fn is_css_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Парсит угол в радианах из строки вида `45deg`, `1.5rad`, `0.25turn`,
/// `100grad`. Без единицы — number-as-radians (для совместимости).
fn parse_angle_to_radians(s: &str) -> Option<f32> {
    let s = s.trim();
    for (suffix, factor) in [
        ("deg", std::f32::consts::PI / 180.0),
        ("rad", 1.0),
        ("turn", std::f32::consts::TAU),
        ("grad", std::f32::consts::PI / 200.0),
    ] {
        if let Some(num) = s.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f32>()
        {
            return Some(v * factor);
        }
    }
    s.parse::<f32>().ok()
}

/// Парсит `<number>` или `<percentage>` для filter-функций.
/// Number 0..=1.0 (или %  0..=100%) — типичная семантика.
fn parse_number_or_percent(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        num.trim().parse::<f32>().ok().map(|v| v / 100.0)
    } else {
        s.parse::<f32>().ok()
    }
}

/// Распарсить `<length>` в px (без `%`). Поддерживает px/em/rem
/// упрощённо — em/rem трактуем как 16px-base; viewport-units игнорируем
/// (Phase 0 — clip-path/transform/filter не критичны к точному
/// разрешению относительных длин на этапе parsing).
fn parse_length_px(s: &str) -> Option<f32> {
    let s = s.trim();
    for (suffix, factor) in [("px", 1.0), ("em", 16.0), ("rem", 16.0)] {
        if let Some(num) = s.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f32>()
        {
            return Some(v * factor);
        }
    }
    // Без единицы — допустимо для 0.
    s.parse::<f32>().ok()
}

/// Парсит `<basic-shape>` для `clip-path` (CSS Masking L1 §3.5).
/// Поддерживает: `inset(t r b l)`, `circle(r at cx cy)`,
/// `ellipse(rx ry at cx cy)`, `polygon(x1 y1, x2 y2, ...)`.
fn parse_clip_path(s: &str) -> Option<ClipPath> {
    let s = s.trim();
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    let func = s[..open].trim().to_ascii_lowercase();
    let inner = s[open + 1..close].trim();
    match func.as_str() {
        "inset" => {
            let parts: Vec<f32> = inner
                .split_whitespace()
                .filter_map(parse_length_px)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(ClipPath::Inset(parts))
            }
        }
        "circle" => {
            // `radius` [`at cx cy`]
            let (radius_part, at_part) = if let Some(idx) = inner.find(" at ") {
                (&inner[..idx], Some(&inner[idx + 4..]))
            } else {
                (inner, None)
            };
            let radius = parse_length_px(radius_part.trim())?;
            let center = at_part.and_then(parse_at_pair);
            Some(ClipPath::Circle { radius, center })
        }
        "ellipse" => {
            let (radii_part, at_part) = if let Some(idx) = inner.find(" at ") {
                (&inner[..idx], Some(&inner[idx + 4..]))
            } else {
                (inner, None)
            };
            let radii: Vec<f32> = radii_part
                .split_whitespace()
                .filter_map(parse_length_px)
                .collect();
            if radii.len() < 2 {
                return None;
            }
            let center = at_part.and_then(parse_at_pair);
            Some(ClipPath::Ellipse {
                rx: radii[0],
                ry: radii[1],
                center,
            })
        }
        "polygon" => {
            let mut vertices = Vec::new();
            for pair in inner.split(',') {
                let coords: Vec<f32> = pair
                    .split_whitespace()
                    .filter_map(parse_length_px)
                    .collect();
                if coords.len() >= 2 {
                    vertices.push((coords[0], coords[1]));
                }
            }
            if vertices.is_empty() {
                None
            } else {
                Some(ClipPath::Polygon(vertices))
            }
        }
        _ => None,
    }
}

fn parse_at_pair(s: &str) -> Option<(f32, f32)> {
    let parts: Vec<f32> = s.split_whitespace().filter_map(parse_length_px).collect();
    if parts.len() >= 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

/// Парсит `<transform-list>` — последовательность `func(args)` через
/// whitespace (без запятых). Каждая `func` распознаётся отдельно.
fn parse_transform_list(s: &str) -> Vec<TransformFn> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Read ident до `(`.
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = s[name_start..i].trim().to_ascii_lowercase();
        if name.is_empty() {
            break;
        }
        // Expect `(`.
        if i >= bytes.len() || bytes[i] != b'(' {
            break;
        }
        i += 1;
        // Find matching `)`.
        let args_start = i;
        let mut depth = 1usize;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let args = &s[args_start..i.saturating_sub(1)];
        if let Some(tf) = parse_transform_fn(&name, args) {
            out.push(tf);
        }
    }
    out
}

fn parse_transform_fn(name: &str, args: &str) -> Option<TransformFn> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    match name {
        "translate" => {
            let x = parse_length_px(parts.first()?)?;
            let y = parts.get(1).and_then(|s| parse_length_px(s)).unwrap_or(0.0);
            Some(TransformFn::Translate(x, y))
        }
        "translatex" => parse_length_px(parts.first()?).map(TransformFn::TranslateX),
        "translatey" => parse_length_px(parts.first()?).map(TransformFn::TranslateY),
        "rotate" => parse_angle_to_radians(parts.first()?).map(TransformFn::Rotate),
        "scale" => {
            let x = parts.first()?.parse::<f32>().ok()?;
            let y = parts
                .get(1)
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(x);
            Some(TransformFn::Scale(x, y))
        }
        "scalex" => parts.first()?.parse::<f32>().ok().map(TransformFn::ScaleX),
        "scaley" => parts.first()?.parse::<f32>().ok().map(TransformFn::ScaleY),
        "skewx" => parse_angle_to_radians(parts.first()?).map(TransformFn::SkewX),
        "skewy" => parse_angle_to_radians(parts.first()?).map(TransformFn::SkewY),
        "skew" => {
            // `skew(x, y)` — для совместимости. Phase 0: храним как X-only.
            parse_angle_to_radians(parts.first()?).map(TransformFn::SkewX)
        }
        "matrix" => {
            if parts.len() != 6 {
                return None;
            }
            let mut m = [0.0f32; 6];
            for (i, p) in parts.iter().enumerate() {
                m[i] = p.parse::<f32>().ok()?;
            }
            Some(TransformFn::Matrix(m))
        }
        _ => None,
    }
}

/// Парсит `<filter-function-list>` — последовательность функций
/// через whitespace.
fn parse_filter_list(s: &str) -> Vec<FilterFn> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = s[name_start..i].trim().to_ascii_lowercase();
        if name.is_empty() {
            break;
        }
        if i >= bytes.len() || bytes[i] != b'(' {
            break;
        }
        i += 1;
        let args_start = i;
        let mut depth = 1usize;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let args = s[args_start..i.saturating_sub(1)].trim();
        if let Some(f) = parse_filter_fn(&name, args) {
            out.push(f);
        }
    }
    out
}

/// CSS Content L3 — парсер `content` value на список `ContentItem`.
/// Whitespace-separated; кавычки литералов и `()` функций соблюдаются.
fn parse_content_items(s: &str) -> Vec<ContentItem> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let quote = c;
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let literal = &s[start..i];
            out.push(ContentItem::String(literal.to_string()));
            if i < bytes.len() {
                i += 1; // closing quote
            }
        } else if c.is_ascii_alphabetic() || c == b'-' {
            // Ident, может быть keyword (open-quote/...) или function-call.
            let name_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-')
            {
                i += 1;
            }
            let name = s[name_start..i].to_ascii_lowercase();
            if i < bytes.len() && bytes[i] == b'(' {
                // Function call. Find matching `)`.
                i += 1;
                let args_start = i;
                let mut depth = 1usize;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                let args = &s[args_start..i.saturating_sub(1)];
                if let Some(item) = parse_content_fn(&name, args) {
                    out.push(item);
                }
            } else {
                // Keyword.
                match name.as_str() {
                    "open-quote" => out.push(ContentItem::OpenQuote),
                    "close-quote" => out.push(ContentItem::CloseQuote),
                    "no-open-quote" => out.push(ContentItem::NoOpenQuote),
                    "no-close-quote" => out.push(ContentItem::NoCloseQuote),
                    _ => {}  // unknown ident — skip
                }
            }
        } else {
            i += 1;  // skip unknown character
        }
    }
    out
}

fn parse_content_fn(name: &str, args: &str) -> Option<ContentItem> {
    match name {
        "url" => {
            let inner = args.trim().trim_matches(['"', '\''].as_ref());
            Some(ContentItem::Url(inner.to_string()))
        }
        "attr" => {
            let attr_name = args.trim().trim_matches(['"', '\''].as_ref());
            if attr_name.is_empty() {
                None
            } else {
                Some(ContentItem::Attr(attr_name.to_string()))
            }
        }
        "counter" => {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            let counter_name = parts.first()?.to_string();
            if counter_name.is_empty() {
                return None;
            }
            let style = parts.get(1).map(|s| s.to_string());
            Some(ContentItem::Counter {
                name: counter_name,
                style,
            })
        }
        "counters" => {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            let counter_name = parts.first()?.to_string();
            let separator_raw = parts.get(1)?;
            let separator = separator_raw.trim_matches(['"', '\''].as_ref()).to_string();
            let style = parts.get(2).map(|s| s.to_string());
            Some(ContentItem::Counters {
                name: counter_name,
                separator,
                style,
            })
        }
        _ => None,
    }
}

/// CSS Animations L1 §4 — `animation` shorthand.
///
/// Синтаксис: `animation = <single-animation>#`, где
///
/// ```text
/// <single-animation> = <time> || <easing-function> || <time>
///                   || <single-animation-iteration-count>
///                   || <single-animation-direction>
///                   || <single-animation-fill-mode>
///                   || <single-animation-play-state>
///                   || [ none | <keyframes-name> ]
/// ```
///
/// Оператор `||` (CSS Values §1.3.4) разрешает любому subset-у этих 8
/// «слотов» появляться в любом порядке. Первое подходящее `<time>` —
/// duration, второе — delay. Любой identifier-токен, не подходящий ни
/// под один keyword-slot, считается keyframes-name.
///
/// Поведение по spec semantics:
/// - Shorthand сбрасывает ВСЕ 8 longhand Vec-ов: каждый layer (= одна
///   позиция в comma-list) даёт строго одну запись в каждый из 8 Vec-ов;
///   un-set значения — initial-value (`""` для name, `0.0s` для time-ов,
///   `Default::default()` для остальных).
/// - Один токен в позиции, где slot уже занят, — fall-through к
///   следующему slot-у; если ни один не подошёл, токен трактуется как
///   keyframes-name.
/// - `none` без других именных кандидатов → `animation-fill-mode: none`
///   (он валиден без других конфликтов). Это компромисс per Blink/WebKit:
///   результат `animation: none` — пустое имя у этого layer-а →
///   эффективно отсутствие анимации.
fn apply_animation_shorthand(style: &mut ComputedStyle, val: &str) {
    let mut names: Vec<String> = Vec::new();
    let mut durations: Vec<f32> = Vec::new();
    let mut timings: Vec<TimingFunction> = Vec::new();
    let mut delays: Vec<f32> = Vec::new();
    let mut iters: Vec<IterationCount> = Vec::new();
    let mut dirs: Vec<AnimationDirection> = Vec::new();
    let mut fills: Vec<AnimationFillMode> = Vec::new();
    let mut plays: Vec<AnimationPlayState> = Vec::new();

    for layer in split_top_level_commas(val) {
        let layer = layer.trim();
        if layer.is_empty() {
            continue;
        }
        let parsed = parse_single_animation(layer);
        names.push(parsed.name);
        durations.push(parsed.duration);
        timings.push(parsed.timing);
        delays.push(parsed.delay);
        iters.push(parsed.iter);
        dirs.push(parsed.direction);
        fills.push(parsed.fill);
        plays.push(parsed.play_state);
    }

    style.animation_names = names;
    style.animation_durations = durations;
    style.animation_timing_functions = timings;
    style.animation_delays = delays;
    style.animation_iteration_counts = iters;
    style.animation_directions = dirs;
    style.animation_fill_modes = fills;
    style.animation_play_states = plays;
}

/// Результат парсинга одного `<single-animation>` для shorthand. Все
/// поля заполнены: либо явное значение из CSS, либо initial-value.
/// Это обеспечивает совпадение длин всех 8 longhand Vec-ов после
/// shorthand-разворота (см. [`apply_animation_shorthand`]).
struct SingleAnimation {
    name: String,
    duration: f32,
    timing: TimingFunction,
    delay: f32,
    iter: IterationCount,
    direction: AnimationDirection,
    fill: AnimationFillMode,
    play_state: AnimationPlayState,
}

impl Default for SingleAnimation {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.0,
            timing: TimingFunction::default(),
            delay: 0.0,
            iter: IterationCount::default(),
            direction: AnimationDirection::default(),
            fill: AnimationFillMode::default(),
            play_state: AnimationPlayState::default(),
        }
    }
}

/// Парсит одну `<single-animation>`-секцию: токенизация с учётом круглых
/// скобок (cubic-bezier / steps содержат запятые и пробелы), classify по
/// первому подходящему slot-у, fall-through к следующему при коллизии,
/// последний кандидат — keyframes-name.
fn parse_single_animation(s: &str) -> SingleAnimation {
    let mut out = SingleAnimation::default();
    let mut duration_set = false;
    let mut delay_set = false;
    let mut timing_set = false;
    let mut iter_set = false;
    let mut direction_set = false;
    let mut fill_set = false;
    let mut play_set = false;
    let mut name_set = false;

    for tok in tokenize_with_parens(s) {
        // 1) <time>: первое → duration, второе → delay. Per spec ordering.
        if let Some(t) = parse_time_seconds(&tok) {
            if !duration_set {
                out.duration = t;
                duration_set = true;
                continue;
            }
            if !delay_set {
                out.delay = t;
                delay_set = true;
                continue;
            }
            // Третье «<time>» некуда положить — игнорируем (spec: invalid).
            continue;
        }
        // 2) <easing-function>: keyword / cubic-bezier(...) / steps(...).
        if !timing_set
            && let Some(tf) = TimingFunction::parse(&tok)
        {
            out.timing = tf;
            timing_set = true;
            continue;
        }
        // 3) <iteration-count>: `infinite` или unitless f32 ≥ 0.
        if !iter_set
            && let Some(ic) = IterationCount::parse(&tok)
        {
            out.iter = ic;
            iter_set = true;
            continue;
        }
        // 4) <direction>.
        if !direction_set
            && let Some(d) = AnimationDirection::parse(&tok)
        {
            out.direction = d;
            direction_set = true;
            continue;
        }
        // 5) <fill-mode>. `none` совпадает здесь и используется ДО name —
        // совпадает с поведением Blink/WebKit/Gecko.
        if !fill_set
            && let Some(fm) = AnimationFillMode::parse(&tok)
        {
            out.fill = fm;
            fill_set = true;
            continue;
        }
        // 6) <play-state>.
        if !play_set
            && let Some(ps) = AnimationPlayState::parse(&tok)
        {
            out.play_state = ps;
            play_set = true;
            continue;
        }
        // 7) keyframes-name: любой токен, не подошедший выше. Только
        // первый кандидат остаётся; последующие игнорируются (spec:
        // дубликат недопустим, два keyframes-name делают объявление
        // invalid; lenient — пропускаем).
        if !name_set && !tok.is_empty() {
            out.name = tok;
            name_set = true;
        }
    }
    out
}

/// CSS Transitions L1 §3 — `transition` shorthand.
///
/// Синтаксис: `transition = <single-transition>#`, где
///
/// ```text
/// <single-transition> = [ none | <single-transition-property> ]
///                    || <time> || <easing-function> || <time>
/// ```
///
/// Слоты per layer (порядок в `||` произвольный):
/// - 2 × `<time>`: первый — duration, второй — delay.
/// - `<easing-function>`: timing function (linear / ease / cubic-bezier(…)
///   / steps(…) / step-start / step-end).
/// - property: `none` или CSS-ident (любое property name, плюс keyword
///   `all`). Default = `all`.
///
/// Shorthand сбрасывает все 4 longhand Vec-а; каждый layer (одна позиция
/// в comma-list) кладёт строго одну запись в каждый Vec. Un-set значения
/// → initial-value (duration/delay = 0s, timing = ease, property = "all").
///
/// `none` в позиции property сохраняется как литеральная строка `"none"`
/// — consumer (transition scheduler) skip-нет такие layers. Это отличается
/// от longhand-парсинга `transition-property: none` (там → пустой Vec),
/// что даёт parallel-length-инвариант после shorthand-развёртки.
fn apply_transition_shorthand(style: &mut ComputedStyle, val: &str) {
    let mut props: Vec<String> = Vec::new();
    let mut durations: Vec<f32> = Vec::new();
    let mut timings: Vec<TimingFunction> = Vec::new();
    let mut delays: Vec<f32> = Vec::new();

    for layer in split_top_level_commas(val) {
        let layer = layer.trim();
        if layer.is_empty() {
            continue;
        }
        let parsed = parse_single_transition(layer);
        props.push(parsed.property);
        durations.push(parsed.duration);
        timings.push(parsed.timing);
        delays.push(parsed.delay);
    }

    style.transition_properties = props;
    style.transition_durations = durations;
    style.transition_timing_functions = timings;
    style.transition_delays = delays;
}

/// Результат парсинга одного `<single-transition>` слоя. Все 4 поля
/// заполнены: либо явное значение из CSS, либо initial-value.
struct SingleTransition {
    property: String,
    duration: f32,
    timing: TimingFunction,
    delay: f32,
}

impl Default for SingleTransition {
    fn default() -> Self {
        Self {
            property: "all".to_string(),
            duration: 0.0,
            timing: TimingFunction::default(),
            delay: 0.0,
        }
    }
}

/// Парсит одну `<single-transition>`-секцию. Tokenize-with-parens →
/// classify по первому подходящему slot-у. Property — последний
/// fallback (любой ident, не подошедший под time / easing).
fn parse_single_transition(s: &str) -> SingleTransition {
    let mut out = SingleTransition::default();
    let mut duration_set = false;
    let mut delay_set = false;
    let mut timing_set = false;
    let mut property_set = false;

    for tok in tokenize_with_parens(s) {
        if let Some(t) = parse_time_seconds(&tok) {
            if !duration_set {
                out.duration = t;
                duration_set = true;
                continue;
            }
            if !delay_set {
                out.delay = t;
                delay_set = true;
                continue;
            }
            continue;
        }
        if !timing_set
            && let Some(tf) = TimingFunction::parse(&tok)
        {
            out.timing = tf;
            timing_set = true;
            continue;
        }
        if !property_set && !tok.is_empty() {
            out.property = tok;
            property_set = true;
        }
    }
    out
}

/// Whitespace-разделение `<single-animation>`-слоя с уважением к
/// круглым скобкам (`cubic-bezier(0.42, 0, 0.58, 1)` — один токен,
/// несмотря на запятые и пробелы внутри).
fn tokenize_with_parens(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }
    tokens
}

/// CSS Values L4 §8 — список `<time>` значений через запятую.
/// Возвращает Vec секунд (ms → /1000, s → as-is).
fn parse_time_list(s: &str) -> Vec<f32> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(parse_time_seconds)
        .collect()
}

fn parse_time_seconds(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("ms") {
        return num.trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    if let Some(num) = s.strip_suffix('s') {
        return num.trim().parse::<f32>().ok();
    }
    None
}

/// Извлечь URL из `url(...)`-функции. Поддерживает кавычки и без них.
/// Возвращает None если строка не выглядит как url().
fn parse_url_value(s: &str) -> Option<String> {
    let s = s.trim();
    let after = s.strip_prefix("url(")?;
    let close = after.rfind(')')?;
    let inner = after[..close].trim().trim_matches(['"', '\''].as_ref());
    Some(inner.to_string())
}

/// Проверка, является ли value одной из gradient-функций.
fn is_gradient_function(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.starts_with("linear-gradient(")
        || s.starts_with("radial-gradient(")
        || s.starts_with("conic-gradient(")
        || s.starts_with("repeating-linear-gradient(")
        || s.starts_with("repeating-radial-gradient(")
        || s.starts_with("repeating-conic-gradient(")
}

/// CSS Sizing L4 §6.1 — парсит `<ratio>`: либо одно положительное
/// число (трактуется как W:1), либо `W / H` пара. Phase 0 не
/// поддерживает `auto <ratio>` форму (она бы хранилась как fallback,
/// но требует расширения структуры).
fn parse_aspect_ratio_value(s: &str) -> Option<(f32, f32)> {
    let s = s.trim();
    if let Some((w_str, h_str)) = s.split_once('/') {
        let w = w_str.trim().parse::<f32>().ok()?;
        let h = h_str.trim().parse::<f32>().ok()?;
        if w > 0.0 && h > 0.0 {
            return Some((w, h));
        }
        return None;
    }
    // Single number — W:1.
    let v = s.parse::<f32>().ok()?;
    if v > 0.0 {
        Some((v, 1.0))
    } else {
        None
    }
}

fn parse_filter_fn(name: &str, args: &str) -> Option<FilterFn> {
    match name {
        "blur" => parse_length_px(args).map(FilterFn::Blur),
        "brightness" => parse_number_or_percent(args).map(FilterFn::Brightness),
        "contrast" => parse_number_or_percent(args).map(FilterFn::Contrast),
        "grayscale" => parse_number_or_percent(args).map(FilterFn::Grayscale),
        "hue-rotate" => parse_angle_to_radians(args).map(FilterFn::HueRotate),
        "invert" => parse_number_or_percent(args).map(FilterFn::Invert),
        "opacity" => parse_number_or_percent(args).map(FilterFn::Opacity),
        "saturate" => parse_number_or_percent(args).map(FilterFn::Saturate),
        "sepia" => parse_number_or_percent(args).map(FilterFn::Sepia),
        _ => None,
    }
}

/// Контекст для `@media`-запросов из viewport-а. Phase 0 упрощение:
/// media_type всегда "screen", prefers_dark = false. Shell может в
/// будущем переопределить через явный API.
fn media_context_from_viewport(viewport: Size) -> MediaContext {
    MediaContext {
        media_type: "screen".into(),
        width: viewport.width,
        height: viewport.height,
        prefers_dark: false,
    }
}

/// Применяет `font-size`-декларацию, если она задана. Размер `em` берётся
/// относительно `parent_fs` (родительский font-size), `rem` — относительно
/// ROOT_FONT_SIZE, `%` — относительно `parent_fs`.
fn apply_font_size(
    style: &mut ComputedStyle,
    decl: &Declaration,
    parent_fs: f32,
    viewport: Size,
    is_quirks: bool,
) {
    if decl.property != "font-size" {
        return;
    }
    let val = decl.value.as_str();
    // CSS Cascade L4 §7: CSS-wide keywords. font-size — inherited;
    // unset == inherit; revert == unset (Phase 0 без чёткой UA-origin границы).
    if let Some(kw) = parse_css_wide_keyword(val) {
        style.font_size = match kw {
            CssWideKeyword::Inherit
            | CssWideKeyword::Unset
            | CssWideKeyword::Revert => parent_fs,
            CssWideKeyword::Initial => ROOT_FONT_SIZE,
        };
        return;
    }
    let Some(len) = parse_length_q(val, is_quirks) else {
        return;
    };
    // Для font-size: em и % считаются от parent_fs; vh/vw/vmin/vmax — от viewport.
    style.font_size = match &len {
        Length::Px(v) => *v,
        Length::Em(v) => *v * parent_fs,
        Length::Rem(v) => *v * ROOT_FONT_SIZE,
        Length::Percent(v) => *v / 100.0 * parent_fs,
        Length::Vh(v) => *v / 100.0 * viewport.height,
        Length::Vw(v) => *v / 100.0 * viewport.width,
        Length::Vmin(v) => *v / 100.0 * viewport.width.min(viewport.height),
        Length::Vmax(v) => *v / 100.0 * viewport.width.max(viewport.height),
        // `calc()` для font-size: резолвим с em_basis = parent_fs и
        // percent_basis = parent_fs (для `%` внутри выражения). vh/vw
        // используют viewport, что уже делает CalcNode::resolve.
        Length::Calc(node) => match node.resolve(parent_fs, Some(parent_fs), viewport) {
            Some(v) => v,
            None => return,
        },
    };
}

/// Резолвит длину для margin / padding / border. `%` в Phase 0 не поддержан
/// (нужна containing-block-width), возвращает None.
fn resolve_box_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let len = parse_length_q(val, is_quirks)?;
    match len {
        Length::Percent(_) => None,
        other => other.resolve(em_basis, None, viewport),
    }
}

fn set_box_length(target: &mut f32, val: &str, em_basis: f32, viewport: Size, is_quirks: bool) {
    if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
        *target = v;
    }
}

/// Токенизирует CSS box shorthand значение по пробелам вне скобок.
/// Нужно для `calc(5px + 3px)` — пробелы внутри calc() не разделяют tokens.
fn split_box_tokens(val: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth: u32 = 0;
    let mut start: Option<usize> = None;
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ' ' | '\t' if depth == 0 => {
                if let Some(s) = start {
                    tokens.push(&val[s..i]);
                    start = None;
                }
                continue;
            }
            _ => {}
        }
        if start.is_none() && (ch != ' ' && ch != '\t') {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push(&val[s..]);
    }
    tokens
}

/// Парсит CSS box shorthand (margin / padding) с 1-4 пробельно-разделёнными
/// длинами. CSS 2.1 §8.3: 1 → все стороны; 2 → [верт] [гориз];
/// 3 → [top] [гориз] [bottom]; 4 → [top] [right] [bottom] [left].
/// `auto` трактуется как 0 (авто-margin centering — отдельная задача).
fn parse_box_shorthand(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<(f32, f32, f32, f32)> {
    let resolve = |s: &str| -> Option<f32> {
        if s == "auto" { return Some(0.0); }
        resolve_box_length(s, em_basis, viewport, is_quirks)
    };
    let parts = split_box_tokens(val);
    match parts.as_slice() {
        [a] => { let v = resolve(a)?; Some((v, v, v, v)) }
        [tb, lr] => {
            Some((resolve(tb)?, resolve(lr)?, resolve(tb)?, resolve(lr)?))
        }
        [t, lr, b] => {
            Some((resolve(t)?, resolve(lr)?, resolve(b)?, resolve(lr)?))
        }
        [t, r, b, l] => {
            Some((resolve(t)?, resolve(r)?, resolve(b)?, resolve(l)?))
        }
        _ => None,
    }
}

fn is_border_style_kw(s: &str) -> bool {
    matches!(s.trim(), "none" | "solid" | "dashed" | "dotted" | "double")
}

fn parse_border_style_kw(s: &str) -> BorderStyle {
    match s.trim() {
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        _ => BorderStyle::None,
    }
}

fn parse_border_style_opt(s: &str) -> Option<BorderStyle> {
    match s.trim() {
        "none" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        "double" => Some(BorderStyle::Double),
        _ => None,
    }
}

/// CSS Backgrounds L3 §4.2 / Basic UI L4 §5.2 — `<line-width>` =
/// `<length> | thin | medium | thick`. UA convention: thin=1, medium=3,
/// thick=5 (Chromium/Firefox/WebKit совпадают).
fn parse_line_width(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    match val.trim() {
        s if s.eq_ignore_ascii_case("thin") => Some(1.0),
        s if s.eq_ignore_ascii_case("medium") => Some(3.0),
        s if s.eq_ignore_ascii_case("thick") => Some(5.0),
        other => resolve_box_length(other, em_basis, viewport, is_quirks),
    }
}

/// CSS Basic UI L4 §5.3 — `outline-style: auto | <'border-style'>`. Возвращает
/// `None` для невалидного токена, чтобы caller мог попробовать его как
/// width/color в shorthand.
fn parse_outline_style_opt(s: &str) -> Option<OutlineStyle> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(OutlineStyle::Auto);
    }
    match parse_border_style_opt(s)? {
        BorderStyle::None => Some(OutlineStyle::None),
        BorderStyle::Solid | BorderStyle::Double => Some(OutlineStyle::Solid),
        BorderStyle::Dashed => Some(OutlineStyle::Dashed),
        BorderStyle::Dotted => Some(OutlineStyle::Dotted),
    }
}

/// CSS Basic UI L4 §5.4 — `outline-color: auto | <color>`. `currentcolor`
/// — это CSS Color L3 keyword, выделяется в отдельный variant, чтобы
/// renderer мог разрешить его в момент paint (а не подмёшивать
/// `style.color` на этапе cascade — последнее ломает наследование при
/// последующем изменении `color`).
fn parse_outline_color_opt(s: &str, is_quirks: bool) -> Option<OutlineColor> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(OutlineColor::Auto);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(OutlineColor::CurrentColor);
    }
    parse_color_legacy(s, is_quirks).map(OutlineColor::Color)
}

/// Расширяет 1-4 значения `Vec<f32>` в (top, right, bottom, left) по
/// стандартному CSS-правилу (1 значение → все четыре, 2 значения → v-h,
/// 3 значения → top-h-bottom, 4 значения — TRBL).
fn expand_4_sides(parts: &[f32]) -> (f32, f32, f32, f32) {
    match parts.len() {
        1 => (parts[0], parts[0], parts[0], parts[0]),
        2 => (parts[0], parts[1], parts[0], parts[1]),
        3 => (parts[0], parts[1], parts[2], parts[1]),
        _ if parts.len() >= 4 => (parts[0], parts[1], parts[2], parts[3]),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

fn parse_scroll_snap_type(s: &str) -> Option<ScrollSnapType> {
    let s = s.trim().to_ascii_lowercase();
    if s == "none" {
        return Some(ScrollSnapType::default());
    }
    let mut axis = ScrollSnapAxis::None;
    let mut strict = ScrollSnapStrictness::Proximity;
    for tok in s.split_whitespace() {
        match tok {
            "x" => axis = ScrollSnapAxis::X,
            "y" => axis = ScrollSnapAxis::Y,
            "block" => axis = ScrollSnapAxis::Block,
            "inline" => axis = ScrollSnapAxis::Inline,
            "both" => axis = ScrollSnapAxis::Both,
            "mandatory" => strict = ScrollSnapStrictness::Mandatory,
            "proximity" => strict = ScrollSnapStrictness::Proximity,
            _ => {}
        }
    }
    Some(ScrollSnapType {
        axis,
        strictness: strict,
    })
}

fn parse_scroll_snap_align(s: &str) -> Option<ScrollSnapAlign> {
    let parts: Vec<ScrollSnapAlignKeyword> = s
        .split_whitespace()
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "none" => ScrollSnapAlignKeyword::None,
            "start" => ScrollSnapAlignKeyword::Start,
            "end" => ScrollSnapAlignKeyword::End,
            "center" => ScrollSnapAlignKeyword::Center,
            _ => ScrollSnapAlignKeyword::None,
        })
        .collect();
    match parts.len() {
        1 => Some(ScrollSnapAlign {
            block: parts[0],
            inline: parts[0],
        }),
        2 => Some(ScrollSnapAlign {
            block: parts[0],
            inline: parts[1],
        }),
        _ => None,
    }
}

fn parse_overscroll_behavior(s: &str) -> Option<OverscrollBehavior> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(OverscrollBehavior::Auto),
        "contain" => Some(OverscrollBehavior::Contain),
        "none" => Some(OverscrollBehavior::None),
        _ => None,
    }
}

/// Парсит CSS Fragmentation L3 §3.1 `break-*` keyword.
fn parse_break_value(s: &str) -> Option<BreakValue> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(BreakValue::Auto),
        "avoid" | "avoid-page" | "avoid-column" | "avoid-region" => Some(BreakValue::Avoid),
        "always" => Some(BreakValue::Always),
        "page" => Some(BreakValue::Page),
        "column" => Some(BreakValue::Column),
        "region" => Some(BreakValue::Region),
        _ => None,
    }
}

/// Разбирает `border: <width> <style> <color>` (порядок произвольный, каждая
/// часть опциональна). Применяет найденные значения ко всем четырём сторонам.
fn apply_border_shorthand(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size, is_quirks: bool) {
    let tokens: Vec<&str> = val.split_whitespace().collect();
    for tok in &tokens {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport, is_quirks) {
            style.border_top_width = v;
            style.border_right_width = v;
            style.border_bottom_width = v;
            style.border_left_width = v;
        } else if is_border_style_kw(tok) {
            let bs = parse_border_style_kw(tok);
            style.border_top_style = bs;
            style.border_right_style = bs;
            style.border_bottom_style = bs;
            style.border_left_style = bs;
        } else if let Some(c) = parse_color_legacy(tok, is_quirks) {
            style.border_top_color = Some(c);
            style.border_right_color = Some(c);
            style.border_bottom_color = Some(c);
            style.border_left_color = Some(c);
        }
    }
}

/// Разбирает `border-{top,right,bottom,left}: <width> <style> <color>` в одну сторону.
fn apply_border_side_shorthand(
    width: &mut f32,
    bstyle: &mut BorderStyle,
    color: &mut Option<Color>,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) {
    for tok in val.split_whitespace() {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport, is_quirks) {
            *width = v;
        } else if is_border_style_kw(tok) {
            *bstyle = parse_border_style_kw(tok);
        } else if let Some(c) = parse_color_legacy(tok, is_quirks) {
            *color = Some(c);
        }
    }
}

/// Разворачивает 1–4 токена в 4-элементный массив по CSS-правилу:
/// 1 → (T, R, B, L) = all same
/// 2 → (T=B, R=L)
/// 3 → (T, R=L, B)
/// 4 → (T, R, B, L)
fn expand_border_4(val: &str) -> [&str; 4] {
    let parts: Vec<&str> = val.split_whitespace().collect();
    match parts.len() {
        1 => [parts[0], parts[0], parts[0], parts[0]],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        _ => {
            let t = parts[0];
            let r = parts.get(1).copied().unwrap_or(t);
            let b = parts.get(2).copied().unwrap_or(t);
            let l = parts.get(3).copied().unwrap_or(r);
            [t, r, b, l]
        }
    }
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(c) = named_color(&s.to_ascii_lowercase()) {
        return Some(c);
    }
    if let Some(c) = parse_hex_color(s) {
        return Some(c);
    }
    parse_function_color(s)
}

/// CSS Quirks Mode §3.4 «hashless hex color quirk».
///
/// В quirks-mode значение `<color>`, не парсящееся стандартным `parse_color`,
/// но состоящее ровно из 3, 6 или 8 ASCII hex-digits без ведущего `#`,
/// трактуется так, будто `#` присутствовал. То есть в `<body
/// style="color: ff0000">` цвет — красный, при условии что
/// `Document.mode() == Quirks`.
///
/// Длины 3/6/8 покрывают `#rgb` / `#rrggbb` / `#rrggbbaa`. Spec упоминает
/// также длины 7/9, но они появляются только из патологического
/// padding-парсинга «legacy color value» (HTML5 §2.4.6) и в реальных
/// браузерах не используются для CSS quirks.
///
/// В Standards / LimitedQuirks функция полностью эквивалентна `parse_color`.
fn parse_color_legacy(s: &str, is_quirks: bool) -> Option<Color> {
    if let Some(c) = parse_color(s) {
        return Some(c);
    }
    if !is_quirks {
        return None;
    }
    let trimmed = s.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let len = trimmed.len();
    if !matches!(len, 3 | 6 | 8) {
        return None;
    }
    if !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let with_hash = format!("#{trimmed}");
    parse_color(&with_hash)
}

/// CSS Color Module Level 3 §4.3 — X11 / SVG named colors. Принимает имя
/// уже в нижнем регистре. Возвращает None для неизвестного имени.
///
/// Реализовано бинарным поиском по сортированному списку: O(log n) на
/// lookup, no allocations, читается как табличный data-driven код.
/// `transparent` (CSS Color L3) — отдельная константа, потому что у него
/// alpha = 0. `currentcolor` не реализуется здесь — это keyword уровня
/// каскада, требующий доступа к computed `color`.
fn named_color(name_lc: &str) -> Option<Color> {
    if name_lc == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    NAMED_COLORS
        .binary_search_by_key(&name_lc, |&(n, _)| n)
        .ok()
        .map(|i| {
            let (_, (r, g, b)) = NAMED_COLORS[i];
            Color { r, g, b, a: 255 }
        })
}

/// Таблица CSS3 named colors (147 имён), отсортированная по имени для
/// бинарного поиска. `grey`-варианты и `gray`-варианты — оба перечислены.
/// Имена из CSS Color Module Level 4 §6.1: `rebeccapurple` тоже включён.
#[rustfmt::skip]
const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("aliceblue",            (240, 248, 255)),
    ("antiquewhite",         (250, 235, 215)),
    ("aqua",                 (  0, 255, 255)),
    ("aquamarine",           (127, 255, 212)),
    ("azure",                (240, 255, 255)),
    ("beige",                (245, 245, 220)),
    ("bisque",               (255, 228, 196)),
    ("black",                (  0,   0,   0)),
    ("blanchedalmond",       (255, 235, 205)),
    ("blue",                 (  0,   0, 255)),
    ("blueviolet",           (138,  43, 226)),
    ("brown",                (165,  42,  42)),
    ("burlywood",            (222, 184, 135)),
    ("cadetblue",            ( 95, 158, 160)),
    ("chartreuse",           (127, 255,   0)),
    ("chocolate",            (210, 105,  30)),
    ("coral",                (255, 127,  80)),
    ("cornflowerblue",       (100, 149, 237)),
    ("cornsilk",             (255, 248, 220)),
    ("crimson",              (220,  20,  60)),
    ("cyan",                 (  0, 255, 255)),
    ("darkblue",             (  0,   0, 139)),
    ("darkcyan",             (  0, 139, 139)),
    ("darkgoldenrod",        (184, 134,  11)),
    ("darkgray",             (169, 169, 169)),
    ("darkgreen",            (  0, 100,   0)),
    ("darkgrey",             (169, 169, 169)),
    ("darkkhaki",            (189, 183, 107)),
    ("darkmagenta",          (139,   0, 139)),
    ("darkolivegreen",       ( 85, 107,  47)),
    ("darkorange",           (255, 140,   0)),
    ("darkorchid",           (153,  50, 204)),
    ("darkred",              (139,   0,   0)),
    ("darksalmon",           (233, 150, 122)),
    ("darkseagreen",         (143, 188, 143)),
    ("darkslateblue",        ( 72,  61, 139)),
    ("darkslategray",        ( 47,  79,  79)),
    ("darkslategrey",        ( 47,  79,  79)),
    ("darkturquoise",        (  0, 206, 209)),
    ("darkviolet",           (148,   0, 211)),
    ("deeppink",             (255,  20, 147)),
    ("deepskyblue",          (  0, 191, 255)),
    ("dimgray",              (105, 105, 105)),
    ("dimgrey",              (105, 105, 105)),
    ("dodgerblue",           ( 30, 144, 255)),
    ("firebrick",            (178,  34,  34)),
    ("floralwhite",          (255, 250, 240)),
    ("forestgreen",          ( 34, 139,  34)),
    ("fuchsia",              (255,   0, 255)),
    ("gainsboro",            (220, 220, 220)),
    ("ghostwhite",           (248, 248, 255)),
    ("gold",                 (255, 215,   0)),
    ("goldenrod",            (218, 165,  32)),
    ("gray",                 (128, 128, 128)),
    ("green",                (  0, 128,   0)),
    ("greenyellow",          (173, 255,  47)),
    ("grey",                 (128, 128, 128)),
    ("honeydew",             (240, 255, 240)),
    ("hotpink",              (255, 105, 180)),
    ("indianred",            (205,  92,  92)),
    ("indigo",               ( 75,   0, 130)),
    ("ivory",                (255, 255, 240)),
    ("khaki",                (240, 230, 140)),
    ("lavender",             (230, 230, 250)),
    ("lavenderblush",        (255, 240, 245)),
    ("lawngreen",            (124, 252,   0)),
    ("lemonchiffon",         (255, 250, 205)),
    ("lightblue",            (173, 216, 230)),
    ("lightcoral",           (240, 128, 128)),
    ("lightcyan",            (224, 255, 255)),
    ("lightgoldenrodyellow", (250, 250, 210)),
    ("lightgray",            (211, 211, 211)),
    ("lightgreen",           (144, 238, 144)),
    ("lightgrey",            (211, 211, 211)),
    ("lightpink",            (255, 182, 193)),
    ("lightsalmon",          (255, 160, 122)),
    ("lightseagreen",        ( 32, 178, 170)),
    ("lightskyblue",         (135, 206, 250)),
    ("lightslategray",       (119, 136, 153)),
    ("lightslategrey",       (119, 136, 153)),
    ("lightsteelblue",       (176, 196, 222)),
    ("lightyellow",          (255, 255, 224)),
    ("lime",                 (  0, 255,   0)),
    ("limegreen",            ( 50, 205,  50)),
    ("linen",                (250, 240, 230)),
    ("magenta",              (255,   0, 255)),
    ("maroon",               (128,   0,   0)),
    ("mediumaquamarine",     (102, 205, 170)),
    ("mediumblue",           (  0,   0, 205)),
    ("mediumorchid",         (186,  85, 211)),
    ("mediumpurple",         (147, 112, 219)),
    ("mediumseagreen",       ( 60, 179, 113)),
    ("mediumslateblue",      (123, 104, 238)),
    ("mediumspringgreen",    (  0, 250, 154)),
    ("mediumturquoise",      ( 72, 209, 204)),
    ("mediumvioletred",      (199,  21, 133)),
    ("midnightblue",         ( 25,  25, 112)),
    ("mintcream",            (245, 255, 250)),
    ("mistyrose",            (255, 228, 225)),
    ("moccasin",             (255, 228, 181)),
    ("navajowhite",          (255, 222, 173)),
    ("navy",                 (  0,   0, 128)),
    ("oldlace",              (253, 245, 230)),
    ("olive",                (128, 128,   0)),
    ("olivedrab",            (107, 142,  35)),
    ("orange",               (255, 165,   0)),
    ("orangered",            (255,  69,   0)),
    ("orchid",               (218, 112, 214)),
    ("palegoldenrod",        (238, 232, 170)),
    ("palegreen",            (152, 251, 152)),
    ("paleturquoise",        (175, 238, 238)),
    ("palevioletred",        (219, 112, 147)),
    ("papayawhip",           (255, 239, 213)),
    ("peachpuff",            (255, 218, 185)),
    ("peru",                 (205, 133,  63)),
    ("pink",                 (255, 192, 203)),
    ("plum",                 (221, 160, 221)),
    ("powderblue",           (176, 224, 230)),
    ("purple",               (128,   0, 128)),
    ("rebeccapurple",        (102,  51, 153)),
    ("red",                  (255,   0,   0)),
    ("rosybrown",            (188, 143, 143)),
    ("royalblue",            ( 65, 105, 225)),
    ("saddlebrown",          (139,  69,  19)),
    ("salmon",               (250, 128, 114)),
    ("sandybrown",           (244, 164,  96)),
    ("seagreen",             ( 46, 139,  87)),
    ("seashell",             (255, 245, 238)),
    ("sienna",               (160,  82,  45)),
    ("silver",               (192, 192, 192)),
    ("skyblue",              (135, 206, 235)),
    ("slateblue",            (106,  90, 205)),
    ("slategray",            (112, 128, 144)),
    ("slategrey",            (112, 128, 144)),
    ("snow",                 (255, 250, 250)),
    ("springgreen",          (  0, 255, 127)),
    ("steelblue",            ( 70, 130, 180)),
    ("tan",                  (210, 180, 140)),
    ("teal",                 (  0, 128, 128)),
    ("thistle",              (216, 191, 216)),
    ("tomato",               (255,  99,  71)),
    ("turquoise",            ( 64, 224, 208)),
    ("violet",               (238, 130, 238)),
    ("wheat",                (245, 222, 179)),
    ("white",                (255, 255, 255)),
    ("whitesmoke",           (245, 245, 245)),
    ("yellow",               (255, 255,   0)),
    ("yellowgreen",          (154, 205,  50)),
];

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            // #RGB → #RRGGBB: каждый ниббл дублируется.
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: 255 })
        }
        4 => {
            // #RGBA — CSS4: каждый ниббл дублируется.
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()?;
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: a * 17 })
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color { r, g, b, a: 255 })
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color { r, g, b, a })
        }
        _ => None,
    }
}

/// Парсит `rgb(…)`, `rgba(…)`, `hsl(…)`, `hsla(…)`. Поддерживает запятые
/// и whitespace как разделители, как `rgb`/`rgba` синонимы, так и `hsl`/`hsla`.
/// Компоненты:
///   - rgb: целое 0–255 или процент 0–100% (для каждого канала);
///   - hsl: hue в градусах (число или `<n>deg`), saturation и lightness в %;
///   - alpha (4-й компонент): float 0..1 или процент 0–100%. По умолчанию 1.
fn parse_function_color(s: &str) -> Option<Color> {
    let lower = s.to_ascii_lowercase();
    let (kind, body) = if let Some(b) = lower.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("hsla(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else if let Some(b) = lower.strip_prefix("hsl(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else if let Some(b) = lower.strip_prefix("oklch(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Oklch, b)
    } else if let Some(b) = lower.strip_prefix("oklab(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Oklab, b)
    } else if let Some(b) = lower.strip_prefix("lab(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Lab, b)
    } else if let Some(b) = lower.strip_prefix("lch(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Lch, b)
    } else {
        return None;
    };
    let parts = split_color_args(body);
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let alpha = if parts.len() == 4 {
        parse_alpha_component(&parts[3])?
    } else {
        255
    };
    match kind {
        ColorFn::Rgb => {
            let r = parse_rgb_component(&parts[0])?;
            let g = parse_rgb_component(&parts[1])?;
            let b = parse_rgb_component(&parts[2])?;
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Hsl => {
            let h = parse_hue_component(&parts[0])?;
            let s = parse_percent_component(&parts[1])?;
            let l = parse_percent_component(&parts[2])?;
            let (r, g, b) = hsl_to_rgb(h, s, l);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Oklch => {
            // L: 0..1 как число или 0..100% (в spec L=0%..100% соответствует 0..1).
            let l = parse_oklch_lightness(&parts[0])?;
            // C: число или процент (100% = 0.4 по spec L4 §10.3 reference range).
            let c = parse_oklch_chroma(&parts[1])?;
            let h = parse_hue_component(&parts[2])?;
            let (r, g, b) = oklch_to_srgb(l, c, h);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Oklab => {
            // OKLab: L=0..1, a/b — unitless (~±0.4). 100% для a/b = ±0.4.
            let l = parse_oklch_lightness(&parts[0])?;
            let a = parse_oklab_ab(&parts[1])?;
            let b = parse_oklab_ab(&parts[2])?;
            let (r, g, b) = oklab_to_srgb(l, a, b);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Lab => {
            // CIE Lab (D50): L=0..100, a/b — unitless (~±125). 100% = ±125.
            let l = parse_lab_lightness(&parts[0])?;
            let a = parse_lab_ab(&parts[1])?;
            let b = parse_lab_ab(&parts[2])?;
            let (r, g, b) = lab_to_srgb(l, a, b);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Lch => {
            // LCH: L=0..100, C≥0 (100% = 150), H в градусах.
            let l = parse_lab_lightness(&parts[0])?;
            let c = parse_lch_chroma(&parts[1])?;
            let h = parse_hue_component(&parts[2])?;
            let h_rad = h.to_radians();
            let a = c * h_rad.cos();
            let b_v = c * h_rad.sin();
            let (r, g, b) = lab_to_srgb(l, a, b_v);
            Some(Color { r, g, b, a: alpha })
        }
    }
}

enum ColorFn {
    Rgb,
    Hsl,
    Oklch,
    Oklab,
    Lab,
    Lch,
    // Прочие CSS4 расширения (color()) — позже.
}

/// Парсит lightness для oklch: число 0..1 или процент 0..100% → 0..1.
fn parse_oklch_lightness(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

/// Парсит chroma для oklch: число (0..~0.4 типично) или процент (100% = 0.4).
fn parse_oklch_chroma(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.3: 100% = 0.4.
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0 * 0.4).max(0.0));
    }
    s.parse::<f32>().ok().map(|v| v.max(0.0))
}

/// Парсит a/b для oklab: число (~±0.4) или процент (100% = 0.4).
fn parse_oklab_ab(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.4: 100% = 0.4 для a/b.
        return pct.trim().parse::<f32>().ok().map(|p| p / 100.0 * 0.4);
    }
    s.parse::<f32>().ok()
}

/// Парсит lightness для CIE Lab/LCH: число 0..100 или процент 0..100%.
fn parse_lab_lightness(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|p| p.clamp(0.0, 100.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 100.0))
}

/// Парсит a/b для CIE Lab: число (~±125) или процент (100% = 125).
fn parse_lab_ab(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.5: 100% = 125.
        return pct.trim().parse::<f32>().ok().map(|p| p / 100.0 * 125.0);
    }
    s.parse::<f32>().ok()
}

/// Парсит chroma для LCH: число (≥0, ~0..230) или процент (100% = 150).
fn parse_lch_chroma(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.5: 100% = 150 для LCH.
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0 * 150.0).max(0.0));
    }
    s.parse::<f32>().ok().map(|v| v.max(0.0))
}

/// CSS Color L4 §10.4: OKLab напрямую → linear sRGB → gamma sRGB.
/// `l` ∈ [0,1], `a`/`b` — unitless. Алгоритм — second half of oklch_to_srgb.
fn oklab_to_srgb(l: f32, a: f32, b: f32) -> (u8, u8, u8) {
    let l_ = l + 0.396_337_77 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_35 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    let lr = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let lg = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let lb = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    (encode_srgb(lr), encode_srgb(lg), encode_srgb(lb))
}

/// CSS Color L4 §10.5: CIE Lab (D50) → XYZ → D65 (Bradford) → linear sRGB.
/// `l` ∈ [0,100], `a`/`b` — unitless (CIE units, не процентные).
fn lab_to_srgb(l: f32, a: f32, b: f32) -> (u8, u8, u8) {
    // Lab → XYZ (D50). Алгоритм CIE 15.3 §8.4.2.
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let epsilon = 216.0 / 24389.0; // ≈ 0.008856
    let kappa = 24389.0 / 27.0; // ≈ 903.3
    let cube_or_linear = |f: f32, scaled: f32| -> f32 {
        let cubed = f * f * f;
        if cubed > epsilon {
            cubed
        } else {
            scaled / kappa
        }
    };
    let yr = if l > kappa * epsilon {
        let v = (l + 16.0) / 116.0;
        v * v * v
    } else {
        l / kappa
    };
    let xr = cube_or_linear(fx, 116.0 * fx - 16.0);
    let zr = cube_or_linear(fz, 116.0 * fz - 16.0);
    // D50 reference white (CIE 15.3 illuminant D50).
    let xn = 0.964_22;
    let yn = 1.0;
    let zn = 0.825_21;
    let x_d50 = xr * xn;
    let y_d50 = yr * yn;
    let z_d50 = zr * zn;
    // Bradford D50→D65 adaptation (CSS Color L4 §11).
    let x_d65 = 0.955_576_6 * x_d50 - 0.023_039_3 * y_d50 + 0.063_163_6 * z_d50;
    let y_d65 = -0.028_289_5 * x_d50 + 1.009_941_6 * y_d50 + 0.021_007_7 * z_d50;
    let z_d65 = 0.012_298_2 * x_d50 - 0.020_483_0 * y_d50 + 1.329_909_8 * z_d50;
    // D65 XYZ → linear sRGB (sRGB primary matrix, CIE 1931).
    let lr = 3.240_625_5 * x_d65 - 1.537_208 * y_d65 - 0.498_628_6 * z_d65;
    let lg = -0.968_930_7 * x_d65 + 1.875_756_1 * y_d65 + 0.041_517_5 * z_d65;
    let lb = 0.055_710_1 * x_d65 - 0.204_021_1 * y_d65 + 1.056_995_9 * z_d65;
    (encode_srgb(lr), encode_srgb(lg), encode_srgb(lb))
}

/// Linear sRGB → gamma sRGB (IEC 61966-2-1).
fn encode_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// CSS Color L4 §10.3: OKLCH → OKLab → linear sRGB → sRGB (gamma-encoded).
/// `l` ∈ [0,1], `c` ≥ 0, `h_deg` в градусах.
fn oklch_to_srgb(l: f32, c: f32, h_deg: f32) -> (u8, u8, u8) {
    // OKLCH → OKLab.
    let h_rad = h_deg.to_radians();
    let a = c * h_rad.cos();
    let b = c * h_rad.sin();

    // OKLab → linear LMS → linear sRGB. Константы из CSS Color L4 §10.3,
    // округлены до f32-precision.
    let l_ = l + 0.396_337_77 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_35 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    let lr = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let lg = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let lb = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;

    // Linear sRGB → gamma sRGB (per IEC 61966-2-1).
    fn encode(c: f32) -> u8 {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        clamp_byte(v * 255.0)
    }
    (encode(lr), encode(lg), encode(lb))
}

/// Разбивает тело функции по запятой или whitespace (CSS4 разрешает оба),
/// плюс по `/` для отделения alpha в новом синтаксисе `rgb(255 0 0 / 0.5)`.
fn split_color_args(body: &str) -> Vec<String> {
    // Если есть запятые — режем по ним (legacy CSS3).
    if body.contains(',') {
        return body.split(',').map(|s| s.trim().to_string()).collect();
    }
    // Modern CSS4: `r g b` или `r g b / a`. Слэш отделяет alpha.
    let normalized = body.replace('/', " / ");
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    // Ищем `/` — разделитель alpha.
    if let Some(slash) = tokens.iter().position(|&t| t == "/") {
        let mut head: Vec<String> = tokens[..slash].iter().map(|t| t.to_string()).collect();
        if let Some(alpha) = tokens.get(slash + 1) {
            head.push((*alpha).to_string());
        }
        head
    } else {
        tokens.iter().map(|t| t.to_string()).collect()
    }
}

fn parse_rgb_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n))
}

fn parse_alpha_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n * 255.0))
}

/// Парсит hue в градусах. Поддерживает четыре единицы CSS Color L4 §9:
///   - `deg` или без единицы — градусы (default);
///   - `turn` — оборот (1turn = 360deg, как `<a href>` в Кубе Рубика);
///   - `rad` — радианы (1rad = 180/π deg ≈ 57.296deg);
///   - `grad` — гоны (1grad = 0.9deg, full turn = 400grad).
///
/// Порядок проверки суффиксов важен: более длинные сначала, иначе
/// `grad` будет ошибочно ловиться как `rad`.
fn parse_hue_component(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("turn") {
        return num.trim().parse::<f32>().ok().map(|v| v * 360.0);
    }
    if let Some(num) = s.strip_suffix("grad") {
        return num.trim().parse::<f32>().ok().map(|v| v * 0.9);
    }
    if let Some(num) = s.strip_suffix("rad") {
        return num.trim().parse::<f32>().ok().map(|v| v * (180.0 / std::f32::consts::PI));
    }
    let s = s.strip_suffix("deg").unwrap_or(s);
    s.trim().parse::<f32>().ok()
}

fn parse_percent_component(s: &str) -> Option<f32> {
    let s = s.trim();
    let pct = s.strip_suffix('%')?;
    let p = pct.trim().parse::<f32>().ok()?;
    Some((p / 100.0).clamp(0.0, 1.0))
}

fn clamp_byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// Преобразование HSL → RGB по CSS Color Module Level 3 (как у whatwg).
/// `h` — в градусах (любое значение, нормализуется по mod 360),
/// `s` и `l` — нормированные 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 360.0;
    if s == 0.0 {
        let v = clamp_byte(l * 255.0);
        return (v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    (clamp_byte(r * 255.0), clamp_byte(g * 255.0), clamp_byte(b * 255.0))
}

fn hue_to_rgb(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    #[test]
    fn rgb_legacy_commas() {
        assert_eq!(parse_color("rgb(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("rgb(0, 128, 0)"), Some(rgba(0, 128, 0, 255)));
    }

    #[test]
    fn rgb_modern_whitespace() {
        assert_eq!(parse_color("rgb(255 0 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_percent_components() {
        // 100% = 255, 50% = 128 (округление).
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some(rgba(255, 0, 0, 255)));
        let half = parse_color("rgb(50%, 50%, 50%)").unwrap();
        assert!((half.r as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgba_with_alpha_float() {
        // alpha 0.5 → 128 (округление 127.5).
        let c = parse_color("rgba(255, 0, 0, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1, "a={}", c.a);
    }

    #[test]
    fn rgba_with_alpha_percent() {
        let c = parse_color("rgba(255, 0, 0, 50%)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_modern_slash_alpha() {
        // Modern syntax: rgb(r g b / a) — без `rgba` префикса.
        let c = parse_color("rgb(255 0 0 / 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_out_of_range_clamps() {
        // 300 должно зажаться до 255, -10 до 0.
        assert_eq!(parse_color("rgb(300, -10, 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_invalid_components() {
        assert_eq!(parse_color("rgb(abc, def, ghi)"), None);
        assert_eq!(parse_color("rgb(255, 0)"), None);
        assert_eq!(parse_color("rgb()"), None);
    }

    #[test]
    fn hsl_primary_colors() {
        assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(
            parse_color("hsl(120, 100%, 50%)"),
            Some(rgba(0, 255, 0, 255))
        );
        assert_eq!(
            parse_color("hsl(240, 100%, 50%)"),
            Some(rgba(0, 0, 255, 255))
        );
    }

    #[test]
    fn hsl_with_deg_unit() {
        assert_eq!(
            parse_color("hsl(0deg, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_turn() {
        // 0.5turn = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(0.5turn, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 1turn = 360deg = 0deg → red.
        assert_eq!(
            parse_color("hsl(1turn, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_rad() {
        // π rad = 180deg → cyan. f32 округление допустимо.
        let c = parse_color("hsl(3.14159265rad, 100%, 50%)").unwrap();
        assert_eq!(c.r, 0);
        assert!(c.g >= 254);
        assert!(c.b >= 254);
    }

    #[test]
    fn hsl_hue_in_grad() {
        // 200grad = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(200grad, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 400grad = 360deg = 0 → red.
        assert_eq!(
            parse_color("hsl(400grad, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_units_dont_collide() {
        // `grad` не должен ловиться как `rad`. 100grad = 90deg → жёлто-зелёный.
        // А 100rad = 5729.58deg, mod 360 ≈ 329.58 — пурпурно-розовый. Цвета
        // должны отличаться, иначе суффикс ловится не тот.
        let g = parse_color("hsl(100grad, 100%, 50%)").unwrap();
        let r = parse_color("hsl(100rad, 100%, 50%)").unwrap();
        assert_ne!(g, r, "grad и rad дают разные цвета");
    }

    #[test]
    fn hsl_grayscale_when_saturation_zero() {
        // s=0 → lightness как оттенок серого.
        let c = parse_color("hsl(0, 0%, 50%)").unwrap();
        assert!((c.r as i32 - 128).abs() <= 1);
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn hsla_with_alpha() {
        let c = parse_color("hsla(0, 100%, 50%, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    // ── oklch() (CSS Color L4 §10.3) ───────────────────────────────────────

    /// Помощник: проверка близости каналов с допуском (округление 8-bit
    /// + конверсии в float дают ~±2).
    fn near(a: u8, b: u8, tol: i32) -> bool {
        (a as i32 - b as i32).abs() <= tol
    }

    #[test]
    fn oklch_white() {
        // L=1, C=0 — белый. Округление через linear→gamma.
        let c = parse_color("oklch(1 0 0)").unwrap();
        assert!(near(c.r, 255, 2), "r = {}", c.r);
        assert!(near(c.g, 255, 2));
        assert!(near(c.b, 255, 2));
        assert_eq!(c.a, 255);
    }

    #[test]
    fn oklch_black() {
        let c = parse_color("oklch(0 0 0)").unwrap();
        assert!(near(c.r, 0, 2));
        assert!(near(c.g, 0, 2));
        assert!(near(c.b, 0, 2));
    }

    #[test]
    fn oklch_red_approx() {
        // sRGB красный в oklch ≈ oklch(0.628 0.258 29.23deg). Округление f32
        // конверсий — даём допуск ±5.
        let c = parse_color("oklch(0.628 0.258 29.23)").unwrap();
        assert!(near(c.r, 255, 5), "r = {}", c.r);
        assert!(near(c.g, 0, 10), "g = {}", c.g);
        assert!(near(c.b, 0, 10), "b = {}", c.b);
    }

    #[test]
    fn oklch_lightness_as_percent() {
        // 100% = L=1 → белый.
        let pct = parse_color("oklch(100% 0 0)").unwrap();
        let num = parse_color("oklch(1 0 0)").unwrap();
        assert_eq!(pct, num);
    }

    #[test]
    fn oklch_with_alpha_slash() {
        let c = parse_color("oklch(0.5 0 0 / 0.5)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1, "a = {}", c.a);
    }

    #[test]
    fn oklch_with_hue_in_turn() {
        // Hue в turn — должен работать как у hsl().
        // 0.5turn = 180deg.
        let by_turn = parse_color("oklch(0.6 0.15 0.5turn)").unwrap();
        let by_deg = parse_color("oklch(0.6 0.15 180)").unwrap();
        assert_eq!(by_turn, by_deg);
    }

    #[test]
    fn oklch_chroma_clamp_negative_to_zero() {
        // Отрицательная chroma не имеет смысла — clamp на 0.
        let c = parse_color("oklch(0.5 -0.1 0)").unwrap();
        // Должен быть серый (chroma=0).
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn oklch_invalid_returns_none() {
        assert_eq!(parse_color("oklch(0.5)"), None);
        assert_eq!(parse_color("oklch(abc def ghi)"), None);
    }

    // ── CSS Color L4 §10.4 — oklab() ──

    #[test]
    fn oklab_white() {
        // oklab(1 0 0) → белый (a=0, b=0, L=1).
        let c = parse_color("oklab(1 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn oklab_black() {
        let c = parse_color("oklab(0 0 0)").unwrap();
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn oklab_neutral_gray() {
        // a=b=0 → серый.
        let c = parse_color("oklab(0.5 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn oklab_ab_percent() {
        // 100% = 0.4.
        let by_pct = parse_color("oklab(0.5 100% 0)").unwrap();
        let by_num = parse_color("oklab(0.5 0.4 0)").unwrap();
        assert_eq!(by_pct, by_num);
    }

    // ── CSS Color L4 §10.5 — lab() и lch() ──

    #[test]
    fn lab_white() {
        // lab(100 0 0) → белый.
        let c = parse_color("lab(100 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn lab_black() {
        let c = parse_color("lab(0 0 0)").unwrap();
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn lab_neutral_gray() {
        let c = parse_color("lab(50 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn lab_lightness_percent() {
        let by_pct = parse_color("lab(100% 0 0)").unwrap();
        let by_num = parse_color("lab(100 0 0)").unwrap();
        assert_eq!(by_pct, by_num);
    }

    #[test]
    fn lch_white() {
        let c = parse_color("lch(100 0 0)").unwrap();
        assert!(near(c.r, 255, 5));
        assert!(near(c.g, 255, 5));
        assert!(near(c.b, 255, 5));
    }

    #[test]
    fn lch_neutral_when_chroma_zero() {
        let c = parse_color("lch(50 0 0)").unwrap();
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn lch_with_alpha() {
        let c = parse_color("lch(50 0 0 / 0.5)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn lab_invalid_returns_none() {
        assert_eq!(parse_color("lab(50)"), None);
        assert_eq!(parse_color("lab(abc def ghi)"), None);
    }

    #[test]
    fn hsl_hue_wraps() {
        // 360° = 0°, должен дать тот же красный.
        assert_eq!(
            parse_color("hsl(360, 100%, 50%)"),
            parse_color("hsl(0, 100%, 50%)")
        );
    }

    #[test]
    fn hex_with_alpha_8_digits() {
        // #ff000080 → red, alpha 128.
        let c = parse_color("#ff000080").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn hex_short_with_alpha() {
        // #f008 → ff 00 00 88.
        let c = parse_color("#f008").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 0x88);
    }

    #[test]
    fn named_and_hex_still_work() {
        assert_eq!(parse_color("red"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#f00"), Some(rgba(255, 0, 0, 255)));
    }

    // ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

    #[test]
    fn quirks_hashless_hex_6_digit() {
        // В quirks-mode bare 6-hex парсится как color.
        assert_eq!(parse_color_legacy("ff0000", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("00ff00", true), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_color_legacy("0000ff", true), Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn quirks_hashless_hex_3_digit() {
        // `f00` → `#f00` → red.
        assert_eq!(parse_color_legacy("f00", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("0f0", true), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_color_legacy("00f", true), Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn quirks_hashless_hex_8_digit_with_alpha() {
        // `ff000080` → `#ff000080` → red, alpha 128.
        let c = parse_color_legacy("ff000080", true).unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn quirks_hashless_hex_case_insensitive() {
        // Hex digits ASCII case-insensitive.
        assert_eq!(parse_color_legacy("FF0000", true), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color_legacy("Ff00aA", true), Some(rgba(255, 0, 170, 255)));
    }

    #[test]
    fn standards_hashless_hex_rejected() {
        // В Standards-mode bare hex без `#` — не color.
        assert_eq!(parse_color_legacy("ff0000", false), None);
        assert_eq!(parse_color_legacy("f00", false), None);
        assert_eq!(parse_color_legacy("ff000080", false), None);
    }

    #[test]
    fn quirks_hashless_hex_invalid_length() {
        // Длины не 3/6/8 — игнорируются даже в quirks.
        assert_eq!(parse_color_legacy("f", true), None);
        assert_eq!(parse_color_legacy("ff", true), None);
        assert_eq!(parse_color_legacy("ffff", true), None);
        assert_eq!(parse_color_legacy("fffff", true), None);
        assert_eq!(parse_color_legacy("fffffff", true), None);
        assert_eq!(parse_color_legacy("fffffffff", true), None);
    }

    #[test]
    fn quirks_hashless_hex_rejects_non_hex_chars() {
        // `xyz`, `g`, `0xff` — не hex.
        assert_eq!(parse_color_legacy("xyz", true), None);
        assert_eq!(parse_color_legacy("ggg", true), None);
        assert_eq!(parse_color_legacy("ff_000", true), None);
    }

    #[test]
    fn quirks_hashless_does_not_override_standard() {
        // Имя color побеждает hashless-quirk: `red` — это named color, не hex.
        assert_eq!(parse_color_legacy("red", true), Some(rgba(255, 0, 0, 255)));
        // `#ff0000` — обычный hex, парсится без quirk.
        assert_eq!(parse_color_legacy("#ff0000", true), Some(rgba(255, 0, 0, 255)));
        // `rgb(...)` — функциональный, тоже без quirk.
        assert_eq!(parse_color_legacy("rgb(255, 0, 0)", true), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn quirks_named_collision_three_letter_hex() {
        // CSS Color L3: `fff` в quirks парсится как `#fff` (white), хотя
        // `fff` не named color. `dad` — тоже не named, парсится как hex.
        assert_eq!(parse_color_legacy("fff", true), Some(rgba(255, 255, 255, 255)));
        assert_eq!(parse_color_legacy("dad", true), Some(rgba(0xdd, 0xaa, 0xdd, 255)));
    }

    #[test]
    fn quirks_already_hash_prefixed_not_double_processed() {
        // Уже с `#` — обычная ветка, quirks не вмешивается.
        assert_eq!(parse_color_legacy("#ff0000", true), Some(rgba(255, 0, 0, 255)));
        // Невалидный `#` + 4 hex digit-ов в L3 — `parse_hex_color` отдаёт #RGBA.
        // Quirks НЕ должна попытаться повторно добавить `#`.
        // (4-digit с `#` валиден; без `#` — длина 4 не в списке 3/6/8 → None.)
        assert_eq!(parse_color_legacy("ffff", true), None);
    }

    #[test]
    fn case_insensitive_function_names() {
        assert_eq!(parse_color("RGB(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("Rgba(0, 0, 0, 1)"), Some(rgba(0, 0, 0, 255)));
    }

    // ── Полный набор CSS3 named colors ────────────────────────────────────

    #[test]
    fn named_colors_table_is_sorted() {
        // Бинарный поиск требует сортировки. Защита от опечатки при добавлении
        // нового цвета не на своё место.
        for w in NAMED_COLORS.windows(2) {
            assert!(w[0].0 < w[1].0, "table not sorted at {} >= {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn named_color_count() {
        // Sanity-check: CSS3 = 147 named colors + `rebeccapurple` (CSS4 §6.1)
        // = 148. `transparent` обрабатывается отдельно, в таблице его нет.
        // Если число изменилось — обновить и тест, и CLAUDE.md.
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn named_color_typical_websafe() {
        assert_eq!(parse_color("cornflowerblue"), Some(rgba(100, 149, 237, 255)));
        assert_eq!(parse_color("dodgerblue"), Some(rgba(30, 144, 255, 255)));
        assert_eq!(parse_color("hotpink"), Some(rgba(255, 105, 180, 255)));
        assert_eq!(parse_color("indigo"), Some(rgba(75, 0, 130, 255)));
        assert_eq!(parse_color("teal"), Some(rgba(0, 128, 128, 255)));
    }

    #[test]
    fn named_color_grey_variants_match_gray() {
        // CSS принимает оба написания; цвета должны совпадать.
        assert_eq!(parse_color("gray"), parse_color("grey"));
        assert_eq!(parse_color("darkgray"), parse_color("darkgrey"));
        assert_eq!(parse_color("lightgray"), parse_color("lightgrey"));
        assert_eq!(parse_color("slategray"), parse_color("slategrey"));
        assert_eq!(parse_color("dimgray"), parse_color("dimgrey"));
    }

    #[test]
    fn named_color_rebeccapurple_css4() {
        // Добавлен в CSS Color L4 §6.1 в честь Ребекки Майер.
        assert_eq!(parse_color("rebeccapurple"), Some(rgba(102, 51, 153, 255)));
    }

    #[test]
    fn named_color_case_insensitive() {
        assert_eq!(parse_color("CornflowerBlue"), parse_color("cornflowerblue"));
        assert_eq!(parse_color("RED"), parse_color("red"));
    }

    #[test]
    fn named_color_transparent() {
        // Особый случай — alpha = 0.
        let c = parse_color("transparent").unwrap();
        assert_eq!(c, Color::TRANSPARENT);
        assert_eq!(c.a, 0);
    }

    #[test]
    fn named_color_unknown_returns_none() {
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("currentcolor"), None); // не реализовано как named
    }

    #[test]
    fn named_color_aqua_and_cyan_same() {
        // CSS3: оба имени дают (0, 255, 255).
        assert_eq!(parse_color("aqua"), parse_color("cyan"));
    }

    #[test]
    fn named_color_fuchsia_and_magenta_same() {
        // CSS3: оба имени дают (255, 0, 255).
        assert_eq!(parse_color("fuchsia"), parse_color("magenta"));
    }

    // ── Relative units: parse_length + resolve ────────────────────────────

    #[test]
    fn parse_length_recognizes_units() {
        assert_eq!(parse_length("10px"), Some(Length::Px(10.0)));
        assert_eq!(parse_length("1.5em"), Some(Length::Em(1.5)));
        assert_eq!(parse_length("2rem"), Some(Length::Rem(2.0)));
        assert_eq!(parse_length("50%"), Some(Length::Percent(50.0)));
        assert_eq!(parse_length("0"), Some(Length::Px(0.0)));
        // Пробелы вокруг числа допустимы.
        assert_eq!(parse_length(" 10 px "), Some(Length::Px(10.0)));
        // Мусор → None.
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length("px"), None);
    }

    // ── CSS Quirks Mode §3.3: unitless length quirk ───────────────────────

    #[test]
    fn unitless_length_quirks_mode_accepts_as_px() {
        // quirks=true: unitless non-zero → px
        assert_eq!(parse_length_q("10", true), Some(Length::Px(10.0)));
        assert_eq!(parse_length_q("1.5", true), Some(Length::Px(1.5)));
        assert_eq!(parse_length_q("-5", true), Some(Length::Px(-5.0)));
    }

    #[test]
    fn unitless_length_standards_mode_rejects_nonzero() {
        // quirks=false: unitless non-zero → None (CSS Values §6)
        assert_eq!(parse_length_q("10", false), None);
        assert_eq!(parse_length_q("1.5", false), None);
        assert_eq!(parse_length_q("-5", false), None);
    }

    #[test]
    fn unitless_zero_always_valid() {
        // `0` валиден без единицы в обоих режимах (CSS Values §6)
        assert_eq!(parse_length_q("0", true), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0", false), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0.0", true), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0.0", false), Some(Length::Px(0.0)));
    }

    #[test]
    fn unitless_quirk_does_not_affect_dimensioned_values() {
        // Значения с единицами работают в обоих режимах
        assert_eq!(parse_length_q("10px", false), Some(Length::Px(10.0)));
        assert_eq!(parse_length_q("2em", false), Some(Length::Em(2.0)));
        assert_eq!(parse_length_q("50%", false), Some(Length::Percent(50.0)));
    }

    // ── IE7 line-height quirk (CSS Quirks Mode §3.2) ─────────────────────

    #[test]
    fn ie7_line_height_quirk_img_gets_1_in_quirks_mode() {
        // HTML без DOCTYPE → quirks mode; <img> должен получить line-height: 1.
        let s = cascade_at("<img>", "", &[0]);
        assert!(
            (s.line_height - 1.0).abs() < f32::EPSILON,
            "quirks <img> line_height={} (ожидалось 1.0)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_img_not_applied_in_standards_mode() {
        // С <!DOCTYPE html> → standards mode; line-height должен остаться normal (1.2).
        let s = cascade_at("<!DOCTYPE html><img>", "", &[0]);
        assert!(
            (s.line_height - 1.2).abs() < f32::EPSILON,
            "standards <img> line_height={} (ожидалось 1.2)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_author_css_overrides() {
        // Author CSS побеждает UA-правило quirk.
        let s = cascade_at("<img>", "img { line-height: 2; }", &[0]);
        assert!(
            (s.line_height - 2.0).abs() < f32::EPSILON,
            "quirks <img> с author CSS line_height={} (ожидалось 2.0)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_other_replaced_elements() {
        // Quirk применяется ко всем replaced-элементам.
        for tag in &["video", "canvas", "embed", "iframe", "input", "textarea", "select"] {
            let html = format!("<{tag}>");
            let s = cascade_at(&html, "", &[0]);
            assert!(
                (s.line_height - 1.0).abs() < f32::EPSILON,
                "quirks <{tag}> line_height={} (ожидалось 1.0)",
                s.line_height
            );
        }
    }

    #[test]
    fn ie7_line_height_quirk_not_applied_to_block_div() {
        // <div> — не replaced element; quirk не применяется.
        let s = cascade_at("<div></div>", "", &[0]);
        assert!(
            (s.line_height - 1.2).abs() < f32::EPSILON,
            "quirks <div> line_height={} (ожидалось 1.2)",
            s.line_height
        );
    }

    /// Тестовый viewport: квадратный, чтобы vh == vw, vmin == vmax.
    fn vp() -> Size { Size::new(1000.0, 1000.0) }

    #[test]
    fn length_resolve_px_is_identity() {
        assert_eq!(Length::Px(12.0).resolve(16.0, Some(100.0), vp()), Some(12.0));
    }

    #[test]
    fn length_resolve_em_uses_basis() {
        // 1.5em при basis 20 = 30.
        assert_eq!(Length::Em(1.5).resolve(20.0, None, vp()), Some(30.0));
    }

    #[test]
    fn length_resolve_rem_ignores_basis() {
        // rem всегда от ROOT_FONT_SIZE = 16.
        assert_eq!(Length::Rem(2.0).resolve(999.0, None, vp()), Some(32.0));
    }

    #[test]
    fn length_resolve_percent_needs_basis() {
        assert_eq!(Length::Percent(50.0).resolve(16.0, Some(200.0), vp()), Some(100.0));
        assert_eq!(Length::Percent(50.0).resolve(16.0, None, vp()), None);
    }

    // ── viewport units ────────────────────────────────────────────────────

    #[test]
    fn parse_length_recognizes_viewport_units() {
        assert_eq!(parse_length("50vh"), Some(Length::Vh(50.0)));
        assert_eq!(parse_length("50vw"), Some(Length::Vw(50.0)));
        assert_eq!(parse_length("10vmin"), Some(Length::Vmin(10.0)));
        assert_eq!(parse_length("10vmax"), Some(Length::Vmax(10.0)));
        // Дробные значения тоже.
        assert_eq!(parse_length("1.5vh"), Some(Length::Vh(1.5)));
    }

    #[test]
    fn length_resolve_vh_uses_viewport_height() {
        // 50vh от viewport (1024 x 768) = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vh(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vw_uses_viewport_width() {
        // 25vw от viewport (1024 x 768) = 256.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vw(25.0).resolve(16.0, None, v), Some(256.0));
    }

    #[test]
    fn length_resolve_vmin_uses_smaller_dimension() {
        // 50vmin от viewport (1024 x 768) — min = 768; 50% = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmin(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vmax_uses_larger_dimension() {
        // 50vmax от viewport (1024 x 768) — max = 1024; 50% = 512.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmax(50.0).resolve(16.0, None, v), Some(512.0));
    }

    // ── text-decoration parsing ────────────────────────────────────────────

    #[test]
    fn text_decoration_underline_sets_only_underline() {
        let p = parse_text_decoration_shorthand("underline");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(p.color.is_none());
    }

    #[test]
    fn text_decoration_none_returns_empty() {
        let p = parse_text_decoration_shorthand("none");
        assert!(p.line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_multiple_keywords_combine() {
        let p = parse_text_decoration_shorthand("overline underline");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_line_through_with_hyphen() {
        let p = parse_text_decoration_shorthand("line-through");
        assert!(p.line.unwrap().line_through);
    }

    #[test]
    fn text_decoration_none_with_other_clears_all() {
        // `none` всегда побеждает: интуитивный сброс.
        let p = parse_text_decoration_shorthand("underline none");
        assert!(p.line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_blink_and_style_tokens_ignored_for_line() {
        // `blink` — поглощаем (CSS2 deprecated); `solid` — это style, не line.
        let p = parse_text_decoration_shorthand("underline blink solid");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(p.color.is_none(), "no color token → None");
        assert_eq!(p.style, Some(TextDecorationStyle::Solid));
    }

    #[test]
    fn text_decoration_unrecognized_only_returns_none_line() {
        let p = parse_text_decoration_shorthand("blink");
        assert!(p.line.is_none());
        let p = parse_text_decoration_shorthand("");
        assert!(p.line.is_none());
    }

    #[test]
    fn text_decoration_is_case_insensitive() {
        let p = parse_text_decoration_shorthand("UNDERLINE Line-Through");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(d.line_through);
    }

    // ── text-decoration-color ───────────────────────────────────────────────

    #[test]
    fn text_decoration_color_named_in_shorthand() {
        // `text-decoration: underline red` — линия + цвет.
        let p = parse_text_decoration_shorthand("underline red");
        assert!(p.line.unwrap().underline);
        assert_eq!(p.color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_color_hex_in_shorthand() {
        let p = parse_text_decoration_shorthand("overline #00ff00");
        assert!(p.line.unwrap().overline);
        assert_eq!(p.color, Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_color_rgb_function_in_shorthand() {
        // Color-функция с пробелами (modern CSS syntax) — токены должны
        // склеиваться обратно.
        let p = parse_text_decoration_shorthand("line-through rgb(0 0 255)");
        assert!(p.line.unwrap().line_through);
        assert_eq!(p.color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn text_decoration_color_property_named() {
        // Отдельное свойство text-decoration-color.
        let s = style_for("text-decoration-color: blue");
        assert_eq!(s.text_decoration_color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn text_decoration_color_currentcolor_resets() {
        // `currentcolor` сбрасывает text-decoration-color в None.
        let s = style_for("text-decoration-color: red; text-decoration-color: currentcolor");
        assert_eq!(s.text_decoration_color, None);
    }

    #[test]
    fn text_decoration_color_not_inherited_to_separate_branch() {
        // Через каскад наследуется (как и text-decoration-line в Phase 0):
        // дочерний `<p>` получает родительский text-decoration-color.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-color: red; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(div_style.text_decoration_color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        assert_eq!(p_style.text_decoration_color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_shorthand_sets_color_via_apply() {
        // Полный путь через apply_declaration.
        let s = style_for("text-decoration: underline blue");
        assert!(s.text_decoration_line.underline);
        assert_eq!(s.text_decoration_color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn text_decoration_color_default_is_none() {
        // По умолчанию text-decoration-color = None → currentColor при
        // рендеринге.
        let s = ComputedStyle::root();
        assert!(s.text_decoration_color.is_none());
    }

    // ── text-decoration-style ──────────────────────────────────────────────

    #[test]
    fn text_decoration_style_default_is_solid() {
        let s = ComputedStyle::root();
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_longhand_keywords() {
        assert_eq!(style_for("text-decoration-style: double").text_decoration_style,
                   TextDecorationStyle::Double);
        assert_eq!(style_for("text-decoration-style: dotted").text_decoration_style,
                   TextDecorationStyle::Dotted);
        assert_eq!(style_for("text-decoration-style: dashed").text_decoration_style,
                   TextDecorationStyle::Dashed);
        assert_eq!(style_for("text-decoration-style: wavy").text_decoration_style,
                   TextDecorationStyle::Wavy);
        assert_eq!(style_for("text-decoration-style: solid").text_decoration_style,
                   TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_invalid_ignored() {
        // Невалидное значение — declaration ignored, initial остаётся.
        let s = style_for("text-decoration-style: invalid-value");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_case_insensitive() {
        assert_eq!(style_for("text-decoration-style: WAVY").text_decoration_style,
                   TextDecorationStyle::Wavy);
        assert_eq!(style_for("text-decoration-style: Dotted").text_decoration_style,
                   TextDecorationStyle::Dotted);
    }

    #[test]
    fn text_decoration_style_in_shorthand() {
        // `text-decoration: underline wavy red` — все три компонента.
        let s = style_for("text-decoration: underline wavy red");
        assert!(s.text_decoration_line.underline);
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Wavy);
        assert_eq!(s.text_decoration_color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_style_shorthand_resets_to_initial() {
        // CSS Text Decoration L3 §2.1: shorthand сбрасывает все longhand-ы
        // (кроме thickness — она исключена из L3 shorthand-а).
        let s = style_for("text-decoration-style: wavy; text-decoration: underline");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid,
                   "shorthand сбросил style к initial");
        assert!(s.text_decoration_line.underline);
    }

    #[test]
    fn text_decoration_style_inherited_via_cascade() {
        // Phase 0 каскадирует text-decoration-style через inherit (как и
        // line / color).
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-style: dotted; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(div_style.text_decoration_style, TextDecorationStyle::Dotted);
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        assert_eq!(p_style.text_decoration_style, TextDecorationStyle::Dotted);
    }

    // ── text-decoration-thickness ──────────────────────────────────────────

    #[test]
    fn text_decoration_thickness_default_is_auto() {
        let s = ComputedStyle::root();
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    #[test]
    fn text_decoration_thickness_keywords() {
        assert_eq!(style_for("text-decoration-thickness: auto").text_decoration_thickness,
                   TextDecorationThickness::Auto);
        assert_eq!(style_for("text-decoration-thickness: from-font").text_decoration_thickness,
                   TextDecorationThickness::FromFont);
    }

    #[test]
    fn text_decoration_thickness_length_px() {
        let s = style_for("text-decoration-thickness: 3px");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 3.0).abs() < 0.01),
            other => panic!("expected Length(3.0), got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_length_em_resolved() {
        // 0.5em при font-size 16 → 8px (resolve через em_basis).
        let s = style_for("text-decoration-thickness: 0.5em");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 8.0).abs() < 0.01,
                                                            "0.5em @ 16px = 8, got {px}"),
            other => panic!("expected Length, got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_percentage() {
        // 25% хранится как fraction 0.25.
        let s = style_for("text-decoration-thickness: 25%");
        match s.text_decoration_thickness {
            TextDecorationThickness::Percentage(f) => assert!((f - 0.25).abs() < 0.001),
            other => panic!("expected Percentage(0.25), got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_invalid_ignored() {
        let s = style_for("text-decoration-thickness: foobar");
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    #[test]
    fn text_decoration_thickness_case_insensitive() {
        assert_eq!(style_for("text-decoration-thickness: AUTO").text_decoration_thickness,
                   TextDecorationThickness::Auto);
        assert_eq!(style_for("text-decoration-thickness: From-Font").text_decoration_thickness,
                   TextDecorationThickness::FromFont);
    }

    #[test]
    fn text_decoration_thickness_not_in_l3_shorthand() {
        // CSS Text Decoration L3 §2.1 — thickness НЕ входит в shorthand.
        // Установка через longhand + shorthand не должна сбрасывать thickness.
        let s = style_for("text-decoration-thickness: 5px; text-decoration: underline");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 5.0).abs() < 0.01,
                                                            "shorthand НЕ должен сбрасывать thickness"),
            other => panic!("expected Length(5.0), got {other:?}"),
        }
        assert!(s.text_decoration_line.underline);
    }

    #[test]
    fn text_decoration_thickness_inherited_via_cascade() {
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-thickness: 4px; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        match p_style.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 4.0).abs() < 0.01),
            other => panic!("expected inherited Length(4.0), got {other:?}"),
        }
    }

    // ── CSS-wide keywords для text-decoration-style / -thickness ───────────

    #[test]
    fn text_decoration_style_initial_keyword_resets() {
        // `initial` сбрасывает к спецификационному initial (Solid).
        let s = style_for("text-decoration-style: wavy; text-decoration-style: initial");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_thickness_initial_keyword_resets() {
        let s = style_for("text-decoration-thickness: 5px; text-decoration-thickness: initial");
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    // ── Border parsing ────────────────────────────────────────────────────────

    fn style_for(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(&format!("p {{ {css} }}"));
        let root_style = ComputedStyle::root();
        let p = doc.get(doc.root()).children[0];
        compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0))
    }

    #[test]
    fn border_shorthand_sets_all_sides() {
        let s = style_for("border: 2px solid red");
        assert!((s.border_top_width - 2.0).abs() < 0.01);
        assert!((s.border_right_width - 2.0).abs() < 0.01);
        assert!((s.border_bottom_width - 2.0).abs() < 0.01);
        assert!((s.border_left_width - 2.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Solid);
        assert_eq!(s.border_right_style, BorderStyle::Solid);
        assert_eq!(s.border_bottom_style, BorderStyle::Solid);
        assert_eq!(s.border_left_style, BorderStyle::Solid);
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        assert_eq!(s.border_top_color, Some(red));
        assert_eq!(s.border_right_color, Some(red));
    }

    #[test]
    fn border_width_shorthand_1_value() {
        let s = style_for("border-width: 5px");
        assert!((s.border_top_width - 5.0).abs() < 0.01);
        assert!((s.border_right_width - 5.0).abs() < 0.01);
        assert!((s.border_bottom_width - 5.0).abs() < 0.01);
        assert!((s.border_left_width - 5.0).abs() < 0.01);
    }

    #[test]
    fn border_style_sets_all_sides() {
        let s = style_for("border-style: dashed");
        assert_eq!(s.border_top_style, BorderStyle::Dashed);
        assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
    }

    #[test]
    fn border_color_shorthand() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let s = style_for("border-color: blue");
        assert_eq!(s.border_top_color, Some(blue));
        assert_eq!(s.border_left_color, Some(blue));
    }

    #[test]
    fn border_top_side_shorthand() {
        let s = style_for("border-top: 3px dotted green");
        assert!((s.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Dotted);
        let green = Color { r: 0, g: 128, b: 0, a: 255 };
        assert_eq!(s.border_top_color, Some(green));
        // Остальные стороны — не изменены.
        assert!((s.border_right_width - 0.0).abs() < 0.01);
        assert_eq!(s.border_right_style, BorderStyle::None);
    }

    #[test]
    fn border_per_side_width_properties() {
        let s = style_for("border-left-width: 4px; border-right-width: 6px");
        assert!((s.border_left_width - 4.0).abs() < 0.01);
        assert!((s.border_right_width - 6.0).abs() < 0.01);
        assert!((s.border_top_width - 0.0).abs() < 0.01);
    }

    #[test]
    fn border_no_color_means_none() {
        let s = style_for("border: 2px solid");
        assert!(s.border_top_color.is_none());
    }

    #[test]
    fn border_style_kw_none_is_invisible() {
        assert!(!BorderStyle::None.is_visible());
        assert!(BorderStyle::Solid.is_visible());
        assert!(BorderStyle::Dashed.is_visible());
        assert!(BorderStyle::Dotted.is_visible());
        assert!(BorderStyle::Double.is_visible());
    }

    #[test]
    fn border_style_double_parses() {
        let s = style_for("border: 6px double red");
        assert_eq!(s.border_top_style, BorderStyle::Double);
        assert_eq!(s.border_right_style, BorderStyle::Double);
        assert_eq!(s.border_bottom_style, BorderStyle::Double);
        assert_eq!(s.border_left_style, BorderStyle::Double);
    }

    #[test]
    fn border_style_double_per_side() {
        let s = style_for("border-top-style: double; border-bottom-style: double");
        assert_eq!(s.border_top_style, BorderStyle::Double);
        assert_eq!(s.border_bottom_style, BorderStyle::Double);
        assert_eq!(s.border_left_style, BorderStyle::None);
    }

    // ── box-sizing parsing ─────────────────────────────────────────────────

    #[test]
    fn box_sizing_default_is_content_box() {
        let s = style_for("color: red");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_border_box_parses() {
        let s = style_for("box-sizing: border-box");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_content_box_parses_back_to_default() {
        // Явное content-box после border-box возвращает к default.
        let s = style_for("box-sizing: border-box; box-sizing: content-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_case_insensitive() {
        let s = style_for("box-sizing: BORDER-BOX");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_unknown_value_keeps_default() {
        // CSS-парсер не должен падать на мусоре — оставляет предыдущее значение.
        let s = style_for("box-sizing: padding-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_not_inherited() {
        // box-sizing — non-inherited (CSS Basic UI 3 §4.1).
        // Дочерний <p> не получает border-box от родительского <div>.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { box-sizing: border-box; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        assert_eq!(div_style.box_sizing, BoxSizing::BorderBox);
        assert_eq!(p_style.box_sizing, BoxSizing::ContentBox);
    }

    // ──────────────── CSS Variables L1: custom properties + var() ────────────────

    #[test]
    fn custom_prop_stored_in_computed_style() {
        let s = style_for("--main-color: red");
        assert_eq!(
            s.custom_props.get("--main-color").map(String::as_str),
            Some("red")
        );
    }

    #[test]
    fn custom_prop_does_not_match_known_property() {
        // `--display: block` НЕ должно повлиять на свойство display.
        // Должно только лечь в custom_props.
        let s = style_for("--display: block");
        assert_eq!(s.display, Display::Block); // default для <p>
        assert_eq!(s.custom_props.get("--display").map(String::as_str), Some("block"));
    }

    #[test]
    fn var_substitutes_simple_value() {
        let s = style_for("--c: red; color: var(--c)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_substitutes_length_value() {
        let s = style_for("--w: 50px; width: var(--w)");
        assert!((s.width.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn var_uses_fallback_when_name_unknown() {
        // --c не задан — берём fallback (blue).
        let s = style_for("color: var(--unknown, blue)");
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn var_without_fallback_and_unknown_is_dropped() {
        // var() не разрешается и нет fallback → декларация игнорится,
        // color остаётся inherited (root() = black).
        let s = style_for("color: var(--unknown)");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn var_resolved_value_overrides_default() {
        // --c определён, fallback есть, но не используется (имя найдено).
        let s = style_for("--c: red; color: var(--c, blue)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_cascade_later_wins() {
        // Последняя декларация --x с той же specificity побеждает.
        let s = style_for("--x: red; --x: blue; color: var(--x)");
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn var_resolved_after_main_pass_regardless_of_source_order() {
        // --c объявлен ПОСЛЕ color: var(--c) — всё равно подставляется,
        // потому что custom-pass идёт до main-pass.
        let s = style_for("color: var(--c); --c: red");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_nested_substitution() {
        // var() resolves to another var() — должен раскрываться рекурсивно.
        let s = style_for("--a: var(--b); --b: red; color: var(--a)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_cycle_dropped_safely() {
        // --a -> --b -> --a — рекурсия превышает лимит → declaration ignored
        // → color остаётся default (black).
        let s = style_for("--a: var(--b); --b: var(--a); color: var(--a)");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn var_inherits_from_parent() {
        // Custom properties inherit (CSS Variables L1 §2). Объявленное на
        // <div> --main должно быть видно у потомка <p>.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet =
            lumen_css_parser::parse("div { --main: green; } p { color: var(--main); }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        // Inherited custom prop виден у потомка.
        assert_eq!(p_style.custom_props.get("--main").map(String::as_str), Some("green"));
        assert_eq!(p_style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn var_fallback_with_inner_comma_and_parens() {
        // Fallback содержит rgba(...) с запятыми — не должен порваться по
        // первой `,`. Top-level запятая отделяет имя от fallback, остальные —
        // часть fallback.
        let s = style_for("color: var(--c, rgba(255, 0, 0, 0.5))");
        let c = s.color;
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn var_within_string_literal_not_expanded() {
        // `"var(--x)"` внутри строкового литерала — это литерал, не
        // substitution. Свойство `content` мы не applay-им в Phase 0, поэтому
        // проверка идёт от обратного: find_var_open видит `var(` ВНЕ строки.
        // Берём color: чтобы content-like ситуация не помешала, проверяем
        // напрямую expand_vars.
        let mut custom = HashMap::new();
        custom.insert("--x".to_string(), "red".to_string());
        // Только литерал — никакого реального var() — должен остаться как есть.
        assert_eq!(
            expand_vars("\"var(--x)\"", &custom, 0).as_deref(),
            Some("\"var(--x)\"")
        );
    }

    #[test]
    fn var_specificity_more_important() {
        // !important на --x перебивает обычный --x с большей specificity?
        // Нет — !important побеждает (CSS Cascade L4 §8.1).
        let doc = lumen_html_parser::parse("<p class=\"a\">x</p>");
        let sheet = lumen_css_parser::parse(
            "p { --c: red !important; } .a { --c: blue; } p { color: var(--c); }",
        );
        let root_style = ComputedStyle::root();
        let p = doc.get(doc.root()).children[0];
        let s = compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_multiple_in_one_value_via_border_shorthand() {
        // border shorthand принимает `<width> <style> <color>` — три токена.
        // Все три могут прийти из var(). Проверяем, что expand_vars
        // корректно разворачивает несколько var() в одной строке.
        let s = style_for("--w: 2px; --s: solid; --c: red; border: var(--w) var(--s) var(--c)");
        assert!((s.border_top_width - 2.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Solid);
        assert_eq!(s.border_top_color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn expand_vars_pure_passthrough() {
        // Нет var() — должен вернуть точно такую же строку.
        let custom = HashMap::new();
        assert_eq!(expand_vars("10px solid red", &custom, 0).as_deref(), Some("10px solid red"));
    }

    #[test]
    fn expand_vars_unclosed_paren_is_none() {
        // Сломанный синтаксис — declaration treated as invalid.
        let mut custom = HashMap::new();
        custom.insert("--x".to_string(), "red".to_string());
        assert_eq!(expand_vars("color: var(--x", &custom, 0), None);
    }

    // ──────────────── CSS Properties and Values L1 §1.1: @property ────────────────

    /// Прогоняет каскад вдоль `path` от root до целевого узла,
    /// возвращая ComputedStyle конкретного узла. Каждый шаг — реальный
    /// `compute_style` с inherited от предыдущего шага. Это позволяет
    /// проверить inherits-семантику @property на двухуровневом дереве.
    fn cascade_at(html: &str, css: &str, path: &[usize]) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let viewport = Size::new(800.0, 600.0);
        let mut id = doc.root();
        let mut style =
            compute_style(&doc, id, &sheet, &ComputedStyle::root(), viewport);
        for &idx in path {
            id = doc.get(id).children[idx];
            style = compute_style(&doc, id, &sheet, &style, viewport);
        }
        style
    }

    #[test]
    fn at_property_initial_value_used_when_no_declaration() {
        // var(--c) без декларации, но --c зарегистрирована с initial-value.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_inherits_false_blocks_inheritance() {
        // --c унаследовалось бы от :root, но `inherits: false` → потомок
        // его не видит и берёт initial-value (blue).
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: blue; } \
             div { --c: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_property_inherits_true_passes_to_child() {
        // С `inherits: true` — потомок видит родительское значение.
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: true; initial-value: blue; } \
             div { --c: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_local_declaration_overrides_initial() {
        // Локальная декларация --c=green побеждает initial-value=red.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             p { --c: green; color: var(--c); }",
            &[0],
        );
        // CSS3 green = rgb(0, 128, 0).
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn at_property_without_initial_value_no_fallback() {
        // syntax="*" без initial-value: имя зарегистрировано (inherits:false),
        // но var(--c) не найдёт значения → declaration invalid, color остаётся
        // inherited (root() = black).
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn at_property_initial_value_visible_to_child_inherits_true() {
        // На корне нет декларации --c. Регистрация дала ему initial-value=red
        // и inherits:true. Дочерний `p` должен унаследовать initial-value
        // через стандартный наследование-каскад.
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: true; initial-value: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_last_registration_wins() {
        // Две регистрации одного имени: последняя побеждает (HashMap insert
        // в `registry`-build перезапишет первую).
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             @property --c { syntax: \"*\"; inherits: false; initial-value: green; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn invalid_at_property_does_not_register() {
        // @property без `inherits` — невалидно: имя не регистрируется, var()
        // без значения → declaration invalid → color остаётся inherited.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; initial-value: red; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color::BLACK);
    }

    // ──────────────── CSS Values L4 §10 — calc() ────────────────

    fn resolved_calc(s: &str, em: f32, pb: Option<f32>, vp: Size) -> Option<f32> {
        let len = parse_length(s)?;
        len.resolve(em, pb, vp)
    }

    #[test]
    fn calc_simple_add_px() {
        let v = resolved_calc("calc(10px + 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_simple_sub_px() {
        let v = resolved_calc("calc(50px - 8px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(42.0));
    }

    #[test]
    fn calc_mul_unitless_left() {
        let v = resolved_calc("calc(2 * 10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_mul_unitless_right() {
        let v = resolved_calc("calc(10px * 3)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_div_by_unitless() {
        let v = resolved_calc("calc(20px / 4)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(5.0));
    }

    #[test]
    fn calc_div_by_zero_is_none() {
        let v = resolved_calc("calc(10px / 0)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, None);
    }

    #[test]
    fn calc_precedence_mul_before_add() {
        // 2 + 3 * 4 = 14 (не 20).
        let v = resolved_calc("calc(2px + 3 * 4px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(14.0));
    }

    #[test]
    fn calc_parens_override_precedence() {
        let v = resolved_calc("calc((2 + 3) * 4px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_em_uses_em_basis() {
        // 2em = 2 * 24 = 48 при em_basis=24.
        let v = resolved_calc("calc(2em + 10px)", 24.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(58.0));
    }

    #[test]
    fn calc_rem_uses_root_fs() {
        // 1rem = 16; 1rem + 4 = 20.
        let v = resolved_calc("calc(1rem + 4px)", 24.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_viewport_units() {
        // 100vw = 800, 50vh = 300 при viewport (800,600). 800 + 300 = 1100.
        let v = resolved_calc(
            "calc(100vw - 50vh)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(500.0)); // 800 - 300
    }

    #[test]
    fn calc_percent_uses_basis() {
        // 50% от 200 = 100; 100 - 10 = 90.
        let v = resolved_calc(
            "calc(50% - 10px)",
            16.0,
            Some(200.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(90.0));
    }

    #[test]
    fn calc_percent_without_basis_is_none() {
        // % без containing block — None (declaration ignored).
        let v = resolved_calc("calc(50% + 10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, None);
    }

    #[test]
    fn calc_unary_negative() {
        // -10px + 20px = 10.
        let v = resolved_calc("calc(-10px + 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_unary_negative_after_paren() {
        let v = resolved_calc("calc(20px + (-5px))", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(15.0));
    }

    #[test]
    fn calc_decimal_values() {
        let v = resolved_calc("calc(0.5 * 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_case_insensitive_prefix() {
        // CSS keyword `calc` ASCII case-insensitive.
        let v = resolved_calc("CALC(5px + 5px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_unknown_unit_invalid() {
        // pt не поддерживаем в Phase 0 → парсер вернёт None.
        assert!(parse_length("calc(10pt + 5px)").is_none());
    }

    #[test]
    fn calc_in_width_property_applies() {
        // Интеграция: width: calc(10px * 2 + 20px) = 40px.
        let s = style_for("width: calc(10px * 2 + 20px)");
        assert_eq!(s.width, Some(40.0));
    }

    #[test]
    fn calc_in_padding_property_applies() {
        // padding shorthand берёт одно length — calc() даёт 5+3=8px.
        let s = style_for("padding: calc(5px + 3px)");
        assert!((s.padding_top - 8.0).abs() < 0.01);
        assert!((s.padding_right - 8.0).abs() < 0.01);
    }

    #[test]
    fn padding_two_values_shorthand() {
        // padding: vertical horizontal → top=bottom=8, left=right=12.
        let s = style_for("padding: 8px 12px");
        assert!((s.padding_top - 8.0).abs() < 0.01, "top");
        assert!((s.padding_right - 12.0).abs() < 0.01, "right");
        assert!((s.padding_bottom - 8.0).abs() < 0.01, "bottom");
        assert!((s.padding_left - 12.0).abs() < 0.01, "left");
    }

    #[test]
    fn padding_four_values_shorthand() {
        // padding: top right bottom left.
        let s = style_for("padding: 4px 8px 12px 16px");
        assert!((s.padding_top - 4.0).abs() < 0.01, "top");
        assert!((s.padding_right - 8.0).abs() < 0.01, "right");
        assert!((s.padding_bottom - 12.0).abs() < 0.01, "bottom");
        assert!((s.padding_left - 16.0).abs() < 0.01, "left");
    }

    #[test]
    fn margin_four_values_shorthand() {
        // margin: 0 6px 6px 0 — реальный CSS из графических тестов.
        let s = style_for("margin: 0 6px 6px 0");
        assert!((s.margin_top - 0.0).abs() < 0.01, "top");
        assert!((s.margin_right - 6.0).abs() < 0.01, "right");
        assert!((s.margin_bottom - 6.0).abs() < 0.01, "bottom");
        assert!((s.margin_left - 0.0).abs() < 0.01, "left");
    }

    #[test]
    fn calc_with_var_inside() {
        // var() сначала разворачивается → строка `calc(10px + 5px)`,
        // потом парсится calc() → 15.
        let s = style_for("--gap: 10px; padding: calc(var(--gap) + 5px)");
        assert!((s.padding_top - 15.0).abs() < 0.01);
    }

    #[test]
    fn calc_unbalanced_paren_invalid() {
        assert!(parse_length("calc(10px + 5px").is_none());
        assert!(parse_length("calc((10px + 5px)").is_none());
    }

    #[test]
    fn calc_empty_invalid() {
        assert!(parse_length("calc()").is_none());
    }

    // ──────────────── CSS Values L4 §10.6: min() / max() / clamp() ────────────────

    #[test]
    fn min_two_lengths_picks_smaller() {
        let v = resolved_calc("min(50px, 100px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_many_lengths() {
        let v = resolved_calc("min(30px, 10px, 20px, 5px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(5.0));
    }

    #[test]
    fn min_mixed_units_resolves_to_px() {
        // 2em = 32, 50% от 100 = 50, 24px → min = 24px.
        let v = resolved_calc(
            "min(2em, 50%, 24px)",
            16.0,
            Some(100.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(24.0));
    }

    #[test]
    fn max_picks_larger() {
        let v = resolved_calc("max(50px, 100px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(100.0));
    }

    #[test]
    fn max_with_viewport_unit() {
        // 100vw = 800; max(800, 200, 1000px) = 1000.
        let v = resolved_calc(
            "max(100vw, 200px, 1000px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(1000.0));
    }

    #[test]
    fn clamp_value_inside_range() {
        // clamp(10, 50, 100) = 50.
        let v = resolved_calc(
            "clamp(10px, 50px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn clamp_value_below_min() {
        // clamp(20, 5, 100) = 20 (min wins).
        let v = resolved_calc(
            "clamp(20px, 5px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_value_above_max() {
        // clamp(10, 200, 100) = 100 (max wins).
        let v = resolved_calc(
            "clamp(10px, 200px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(100.0));
    }

    #[test]
    fn clamp_min_greater_than_max() {
        // CSS spec: clamp(min, val, max) ≡ max(min, min(val, max)).
        // При min=50, max=10: inner=min(val, 10), max(50, inner) = 50.
        let v = resolved_calc(
            "clamp(50px, 30px, 10px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_max_nested_inside_calc() {
        // calc(10px + min(20px, 30px)) = 10 + 20 = 30.
        let v = resolved_calc(
            "calc(10px + min(20px, 30px))",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_nested_inside_max() {
        // max(calc(10px * 2), 15px) = max(20, 15) = 20.
        let v = resolved_calc(
            "max(calc(10px * 2), 15px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_inside_min() {
        // min(clamp(10, 50, 100), 80) = min(50, 80) = 50.
        let v = resolved_calc(
            "min(clamp(10px, 50px, 100px), 80px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_with_calc_expression_inside() {
        // min(2 * 10px, 30px) = min(20, 30) = 20.
        // Здесь `2 * 10px` это обычное calc-expression внутри min,
        // не требует обёртки calc(...).
        let v = resolved_calc(
            "min(2 * 10px, 30px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_wrong_arg_count_invalid() {
        // clamp требует ровно 3 аргумента.
        assert!(parse_length("clamp(10px, 20px)").is_none());
        assert!(parse_length("clamp(10px, 20px, 30px, 40px)").is_none());
    }

    #[test]
    fn min_empty_invalid() {
        assert!(parse_length("min()").is_none());
    }

    #[test]
    fn max_empty_invalid() {
        assert!(parse_length("max()").is_none());
    }

    #[test]
    fn min_in_width_property_applies() {
        // width: min(50px, 200px) = 50px.
        let s = style_for("width: min(50px, 200px)");
        assert_eq!(s.width, Some(50.0));
    }

    #[test]
    fn clamp_in_width_property_applies() {
        // width: clamp(50px, 100px, 200px) = 100px.
        let s = style_for("width: clamp(50px, 100px, 200px)");
        assert_eq!(s.width, Some(100.0));
    }

    #[test]
    fn min_with_var_inside() {
        // var() → строка → min() работает.
        let s = style_for("--w: 80px; width: min(var(--w), 50px)");
        assert_eq!(s.width, Some(50.0));
    }

    #[test]
    fn min_case_insensitive() {
        // CSS function names ASCII case-insensitive.
        let v = resolved_calc("MIN(10px, 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn unknown_function_invalid() {
        // Реально несуществующие функции → declaration invalid.
        // (sin/cos/abs реализованы — см. секцию scientific math funcs ниже).
        assert!(parse_length("xyzzy(45deg)").is_none());
        assert!(parse_length("nonexistent(10px)").is_none());
    }

    #[test]
    fn nested_calc_inside_calc() {
        // calc(calc(10px + 5px) * 2) = 30. Раньше nested calc был
        // отложен — теперь работает через function-call в factor.
        let v = resolved_calc(
            "calc(calc(10px + 5px) * 2)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    // ──── CSS Values L4 §10.7-10.9: scientific math funcs ────

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn rc_unitless(s: &str) -> Option<f32> {
        resolved_calc(s, 16.0, None, Size::new(800.0, 600.0))
    }

    // §10.7 trigonometry

    #[test]
    fn sin_radians_zero() {
        assert!(approx(rc_unitless("sin(0)").unwrap(), 0.0));
    }

    #[test]
    fn sin_45_degrees() {
        // sin(45deg) = √2/2 ≈ 0.7071.
        let v = rc_unitless("sin(45deg)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_1_SQRT_2), "got {v}");
    }

    #[test]
    fn cos_180_degrees() {
        let v = rc_unitless("cos(180deg)").unwrap();
        assert!(approx(v, -1.0), "got {v}");
    }

    #[test]
    fn cos_half_turn() {
        // 0.5turn = 180deg → cos = -1.
        let v = rc_unitless("cos(0.5turn)").unwrap();
        assert!(approx(v, -1.0), "got {v}");
    }

    #[test]
    fn tan_45_degrees() {
        let v = rc_unitless("tan(45deg)").unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn asin_1_returns_radians() {
        // asin(1) = π/2 rad.
        let v = rc_unitless("asin(1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_2), "got {v}");
    }

    #[test]
    fn atan_one_returns_pi_quarter() {
        let v = rc_unitless("atan(1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_4), "got {v}");
    }

    #[test]
    fn atan2_y_x() {
        // atan2(1, 1) = π/4.
        let v = rc_unitless("atan2(1, 1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_4), "got {v}");
    }

    #[test]
    fn sin_unitless_is_radians() {
        // По CSS spec число без unit в sin — радианы.
        // sin(π/2) = 1.
        let v = rc_unitless("sin(1.5707963)").unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn grad_unit_converts_to_radians() {
        // 200grad = π (полукруг). sin(π) ≈ 0.
        let v = rc_unitless("sin(200grad)").unwrap();
        assert!(v.abs() < 1e-4, "got {v}");
    }

    // §10.8 exponential

    #[test]
    fn pow_2_10() {
        assert!(approx(rc_unitless("pow(2, 10)").unwrap(), 1024.0));
    }

    #[test]
    fn sqrt_16() {
        assert!(approx(rc_unitless("sqrt(16)").unwrap(), 4.0));
    }

    #[test]
    fn sqrt_negative_returns_none() {
        // sqrt(-1) = NaN → None.
        assert_eq!(rc_unitless("sqrt(-1)"), None);
    }

    #[test]
    fn exp_zero_is_one() {
        assert!(approx(rc_unitless("exp(0)").unwrap(), 1.0));
    }

    #[test]
    fn log_e_is_one() {
        // log(e) с одним аргументом = ln(e) = 1.
        let v = rc_unitless(&format!("log({})", std::f32::consts::E)).unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn log_base_2_of_8() {
        // log(8, 2) = 3.
        let v = rc_unitless("log(8, 2)").unwrap();
        assert!(approx(v, 3.0), "got {v}");
    }

    #[test]
    fn log_of_zero_returns_none() {
        // ln(0) = -∞ → not finite → None.
        assert_eq!(rc_unitless("log(0)"), None);
    }

    #[test]
    fn hypot_two_args_3_4() {
        // hypot(3, 4) = 5 (классический Pythagoras).
        assert!(approx(rc_unitless("hypot(3, 4)").unwrap(), 5.0));
    }

    #[test]
    fn hypot_variadic_three_args() {
        // hypot(2, 3, 6) = sqrt(4+9+36) = sqrt(49) = 7.
        assert!(approx(rc_unitless("hypot(2, 3, 6)").unwrap(), 7.0));
    }

    #[test]
    fn hypot_single_arg_is_abs() {
        // hypot(-5) = sqrt(25) = 5.
        assert!(approx(rc_unitless("hypot(-5)").unwrap(), 5.0));
    }

    // §10.9 sign / stepping

    #[test]
    fn abs_negative_to_positive() {
        let v = resolved_calc("abs(-10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn abs_in_calc() {
        // calc(100px - abs(-20px)) = 100 - 20 = 80.
        let v = resolved_calc(
            "calc(100px - abs(-20px))",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(80.0));
    }

    #[test]
    fn sign_positive_negative_zero() {
        assert_eq!(rc_unitless("sign(5)"), Some(1.0));
        assert_eq!(rc_unitless("sign(-3)"), Some(-1.0));
        assert_eq!(rc_unitless("sign(0)"), Some(0.0));
    }

    #[test]
    fn mod_basic() {
        // 10 mod 3 = 1 (result имеет знак делителя).
        assert!(approx(rc_unitless("mod(10, 3)").unwrap(), 1.0));
    }

    #[test]
    fn mod_negative_dividend() {
        // mod(-1, 3) = 2 (CSS mod: знак делителя; -1 % 3 = -1, +3 = 2, %3 = 2).
        assert!(approx(rc_unitless("mod(-1, 3)").unwrap(), 2.0));
    }

    #[test]
    fn rem_negative_dividend() {
        // rem(-1, 3) = -1 (truncated remainder: знак делимого).
        assert!(approx(rc_unitless("rem(-1, 3)").unwrap(), -1.0));
    }

    #[test]
    fn mod_by_zero_invalid() {
        assert_eq!(rc_unitless("mod(10, 0)"), None);
    }

    #[test]
    fn round_to_integer() {
        assert!(approx(rc_unitless("round(3.7)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(3.4)").unwrap(), 3.0));
    }

    #[test]
    fn round_to_step() {
        // round(13, 5) = 15 (ближайшее кратное 5).
        assert!(approx(rc_unitless("round(13, 5)").unwrap(), 15.0));
        // round(12, 5) = 10.
        assert!(approx(rc_unitless("round(12, 5)").unwrap(), 10.0));
    }

    #[test]
    fn round_to_step_in_width() {
        // width: round(13px, 5px) = 15px.
        let s = style_for("width: round(13px, 5px)");
        assert_eq!(s.width, Some(15.0));
    }

    #[test]
    fn round_with_zero_step_invalid() {
        assert_eq!(rc_unitless("round(13, 0)"), None);
    }

    // CSS Values L4 §10.5.1 — strategy keyword (nearest/up/down/to-zero).

    #[test]
    fn round_up_to_integer() {
        // round(up, 3.1) = 4 — ceil дробного.
        assert!(approx(rc_unitless("round(up, 3.1)").unwrap(), 4.0));
        // round(up, 3.0) = 3 — целое не двигается.
        assert!(approx(rc_unitless("round(up, 3)").unwrap(), 3.0));
    }

    #[test]
    fn round_down_to_integer() {
        // round(down, 3.9) = 3 — floor дробного.
        assert!(approx(rc_unitless("round(down, 3.9)").unwrap(), 3.0));
    }

    #[test]
    fn round_to_zero_basic() {
        // round(to-zero, 3.9) = 3 — trunc положительного.
        assert!(approx(rc_unitless("round(to-zero, 3.9)").unwrap(), 3.0));
        // round(to-zero, -3.9) = -3 — отличается от floor(-3.9) = -4.
        assert!(approx(rc_unitless("round(to-zero, -3.9)").unwrap(), -3.0));
    }

    #[test]
    fn round_up_negative() {
        // round(up, -3.1) = -3 — ceil к +∞.
        assert!(approx(rc_unitless("round(up, -3.1)").unwrap(), -3.0));
    }

    #[test]
    fn round_down_negative() {
        // round(down, -3.1) = -4 — floor к -∞.
        assert!(approx(rc_unitless("round(down, -3.1)").unwrap(), -4.0));
    }

    #[test]
    fn round_nearest_explicit() {
        // Явный nearest эквивалентен без-strategy форме.
        assert!(approx(rc_unitless("round(nearest, 3.7)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(nearest, 3.4)").unwrap(), 3.0));
    }

    #[test]
    fn round_strategy_with_step() {
        // round(up, 13, 5) = 15 — ceil(13/5)*5 = 3*5.
        assert!(approx(rc_unitless("round(up, 13, 5)").unwrap(), 15.0));
        // round(down, 13, 5) = 10.
        assert!(approx(rc_unitless("round(down, 13, 5)").unwrap(), 10.0));
        // round(up, 11, 5) = 15.
        assert!(approx(rc_unitless("round(up, 11, 5)").unwrap(), 15.0));
        // round(to-zero, -11, 5) = -10 (vs down = -15).
        assert!(approx(rc_unitless("round(to-zero, -11, 5)").unwrap(), -10.0));
    }

    #[test]
    fn round_strategy_case_insensitive() {
        // Keyword-ы CSS-стандарт case-insensitive (Values L4 §2.4).
        assert!(approx(rc_unitless("round(UP, 3.1)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(To-Zero, -3.9)").unwrap(), -3.0));
    }

    #[test]
    fn round_strategy_in_width() {
        // width: round(up, 13px, 5px) = 15px.
        let s = style_for("width: round(up, 13px, 5px)");
        assert_eq!(s.width, Some(15.0));
    }

    #[test]
    fn round_strategy_zero_step_invalid() {
        // step=0 → declaration invalid, как и для round без strategy.
        assert_eq!(rc_unitless("round(up, 13, 0)"), None);
    }

    #[test]
    fn round_unknown_strategy_invalid() {
        // `floor` не keyword в strategy — declaration invalid.
        // (lexer пропустит ident `floor`, но parse_function_call для round
        // ждёт после ident либо `,` со strategy, либо expr; одинокий ident-без-`(`
        // в parse_calc_factor возвращает None.)
        assert_eq!(rc_unitless("round(floor, 3.7)"), None);
    }

    #[test]
    fn round_strategy_without_value_invalid() {
        // strategy + `,` + пусто → parse_arg_list падает.
        assert_eq!(rc_unitless("round(up,)"), None);
        // strategy без запятой → ident-arg в parse_calc_factor возвращает None.
        assert_eq!(rc_unitless("round(up 3.1)"), None);
    }

    // Интеграция

    #[test]
    fn math_func_nested_in_calc_and_min() {
        // min(abs(-50px), sqrt(900) * 1px) = min(50, 30) = 30.
        let v = resolved_calc(
            "min(abs(-50px), sqrt(900) * 1px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn pow_in_width_property() {
        // width: pow(2, 5) * 1px = 32px.
        let s = style_for("width: calc(pow(2, 5) * 1px)");
        assert_eq!(s.width, Some(32.0));
    }

    #[test]
    fn sin_with_var_arg() {
        // var() разворачивается до парсинга calc — sin принимает результат.
        let s = style_for("--a: 90deg; width: calc(sin(var(--a)) * 100px)");
        // sin(π/2) = 1, поэтому width = 100.
        assert!((s.width.unwrap() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn wrong_arity_invalid() {
        // sin требует ровно 1 аргумент.
        assert!(parse_length("sin(1, 2)").is_none());
        // pow требует ровно 2.
        assert!(parse_length("pow(2)").is_none());
        assert!(parse_length("pow(2, 3, 4)").is_none());
        // hypot — 1+, поэтому 0 — invalid.
        assert!(parse_length("hypot()").is_none());
    }

    #[test]
    fn math_func_case_insensitive() {
        // CSS function names ASCII case-insensitive.
        assert_eq!(rc_unitless("ABS(-5)"), Some(5.0));
        assert_eq!(rc_unitless("Sqrt(9)"), Some(3.0));
    }

    // ──────────────── CSS Images L3 §5.5: object-fit / object-position ────────────────

    #[test]
    fn object_fit_default_is_fill() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_keywords_parse() {
        for (val, expected) in [
            ("fill", ObjectFit::Fill),
            ("contain", ObjectFit::Contain),
            ("cover", ObjectFit::Cover),
            ("none", ObjectFit::None),
            ("scale-down", ObjectFit::ScaleDown),
        ] {
            let s = cascade_at(
                "<img>",
                &format!("img {{ object-fit: {val}; }}"),
                &[0],
            );
            assert_eq!(s.object_fit, expected, "for value {val}");
        }
    }

    #[test]
    fn object_fit_invalid_value_ignored() {
        // CSS Cascade §8.1: невалидное значение → declaration invalid →
        // используется предыдущее (initial = Fill).
        let s = cascade_at("<img>", "img { object-fit: bogus; }", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_case_insensitive() {
        // CSS Values L4 §2.4 — keywords ASCII case-insensitive.
        let s = cascade_at("<img>", "img { object-fit: COVER; }", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Cover);
    }

    #[test]
    fn object_fit_not_inherited() {
        // object-fit non-inherited; <img> внутри <div> не подхватывает div { ... }
        // (хотя div и не replaced, но пример демонстрирует отсутствие inheritance
        // через initial-value у потомка-не-замены).
        let s = cascade_at(
            "<div><img></div>",
            "div { object-fit: cover; }",
            &[0, 0],
        );
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` всегда работает, даже для
        // non-inherited свойства.
        let s = cascade_at(
            "<div><img></div>",
            "div { object-fit: contain; } img { object-fit: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.object_fit, ObjectFit::Contain);
    }

    #[test]
    fn object_position_default_is_center() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(
            s.object_position,
            ObjectPosition {
                x: PositionComponent::Percent(0.5),
                y: PositionComponent::Percent(0.5),
            }
        );
    }

    #[test]
    fn object_position_two_percent_values() {
        let s = cascade_at(
            "<img>",
            "img { object-position: 25% 75%; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.25));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.75));
    }

    #[test]
    fn object_position_two_lengths() {
        let s = cascade_at(
            "<img>",
            "img { object-position: 10px 20px; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Px(10.0));
        assert_eq!(s.object_position.y, PositionComponent::Px(20.0));
    }

    #[test]
    fn object_position_single_value_centers_y() {
        // Один token → x = token, y = center (50%).
        let s = cascade_at(
            "<img>",
            "img { object-position: 10px; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Px(10.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn object_position_keyword_left_top() {
        let s = cascade_at(
            "<img>",
            "img { object-position: left top; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_keyword_right_bottom() {
        let s = cascade_at(
            "<img>",
            "img { object-position: right bottom; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(1.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(1.0));
    }

    #[test]
    fn object_position_keyword_swap_top_left_means_left_top() {
        // CSS Values L4 §9.4: `top left` ≡ `left top`.
        let s = cascade_at(
            "<img>",
            "img { object-position: top left; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_single_top_centers_x() {
        let s = cascade_at("<img>", "img { object-position: top; }", &[0]);
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_single_center_is_50_50() {
        let s = cascade_at("<img>", "img { object-position: center; }", &[0]);
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn object_position_invalid_value_keeps_default() {
        // 3 token-а — пока не поддерживаем; декларация ignored.
        let s = cascade_at(
            "<img>",
            "img { object-position: left 10px top; }",
            &[0],
        );
        // initial-value сохранён.
        assert_eq!(s.object_position, ObjectPosition::default());
    }

    #[test]
    fn object_position_negative_percent_allowed() {
        // Художественное смещение `-25% 110%` валидно.
        let s = cascade_at(
            "<img>",
            "img { object-position: -25% 110%; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(-0.25));
        assert_eq!(s.object_position.y, PositionComponent::Percent(1.1));
    }

    #[test]
    fn position_component_resolve_percent_against_free_space() {
        let pc = PositionComponent::Percent(0.5);
        assert!((pc.resolve(100.0) - 50.0).abs() < f32::EPSILON);
        // Отрицательное free_space (content больше box) — offset отрицательный.
        assert!((pc.resolve(-40.0) - (-20.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn position_component_resolve_px_ignores_free_space() {
        let pc = PositionComponent::Px(15.0);
        assert!((pc.resolve(0.0) - 15.0).abs() < f32::EPSILON);
        assert!((pc.resolve(1000.0) - 15.0).abs() < f32::EPSILON);
    }

    // -------- vertical-align (CSS 2.1 §10.8.1) --------

    #[test]
    fn vertical_align_default_is_baseline() {
        let s = cascade_at("<span></span>", "", &[0]);
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_all_keywords_parse() {
        for (val, expected) in [
            ("baseline", VerticalAlign::Baseline),
            ("sub", VerticalAlign::Sub),
            ("super", VerticalAlign::Super),
            ("top", VerticalAlign::Top),
            ("text-top", VerticalAlign::TextTop),
            ("middle", VerticalAlign::Middle),
            ("bottom", VerticalAlign::Bottom),
            ("text-bottom", VerticalAlign::TextBottom),
        ] {
            let s = cascade_at(
                "<span></span>",
                &format!("span {{ vertical-align: {val}; }}"),
                &[0],
            );
            assert_eq!(s.vertical_align, expected, "for value {val}");
        }
    }

    #[test]
    fn vertical_align_keywords_case_insensitive() {
        // CSS Values L4 §2.4 — keywords ASCII case-insensitive.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: TEXT-Top; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::TextTop);
    }

    #[test]
    fn vertical_align_length_px() {
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: 5px; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(5.0));
    }

    #[test]
    fn vertical_align_negative_length() {
        // Спецификация допускает отрицательные значения — сдвиг вниз
        // от baseline.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: -3px; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(-3.0));
    }

    #[test]
    fn vertical_align_em_resolved_against_element_font_size() {
        // em для vertical-align резолвится по текущему font-size (10pxx2=20).
        // Используем явный font-size 20 чтобы избежать зависимости от
        // initial 16px (UA stylesheet может его не выставлять).
        let s = cascade_at(
            "<span></span>",
            "span { font-size: 20px; vertical-align: 0.5em; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(10.0));
    }

    #[test]
    fn vertical_align_percent_kept_as_percent() {
        // % резолвится по line-height в layout-pass, не на этапе cascade —
        // поэтому здесь должен остаться как Percent(50.0).
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: 50%; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Percent(50.0));
    }

    #[test]
    fn vertical_align_invalid_value_ignored() {
        // Невалидное значение — declaration invalid; остаётся initial.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: bogus; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_not_inherited() {
        // CSS 2.1 §10.8.1 — non-inherited. Ребёнок без своей декларации
        // получает initial-value, а не значение родителя.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: super; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` принудительно тянет значение
        // родителя даже для non-inherited свойства.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: sub; } span { vertical-align: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Sub);
    }

    #[test]
    fn vertical_align_initial_keyword_resets() {
        // `initial` всегда даёт initial-value свойства (Baseline).
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: top; vertical-align: initial; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_unset_for_non_inherited_is_initial() {
        // CSS Cascade L4 §7: `unset` = `initial` для non-inherited.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: middle; } span { vertical-align: unset; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    // -------- background-position (CSS Backgrounds L3 §3.5) --------

    #[test]
    fn background_position_default_is_top_left() {
        // CSS Backgrounds L3 §3.5 — initial `0% 0%`, отличается от
        // object-position default (`50% 50%`).
        let s = cascade_at("<div></div>", "", &[0]);
        assert_eq!(
            s.background_position,
            ObjectPosition {
                x: PositionComponent::Percent(0.0),
                y: PositionComponent::Percent(0.0),
            }
        );
    }

    #[test]
    fn background_position_two_percent_values() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 25% 75%; }",
            &[0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Percent(0.25));
        assert_eq!(s.background_position.y, PositionComponent::Percent(0.75));
    }

    #[test]
    fn background_position_two_lengths() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 10px 20px; }",
            &[0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Px(10.0));
        assert_eq!(s.background_position.y, PositionComponent::Px(20.0));
    }

    #[test]
    fn background_position_single_value_centers_y() {
        // Один token — второй компонент defaults to `center` (50%).
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 30%; }",
            &[0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Percent(0.30));
        assert_eq!(s.background_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_keyword_right_bottom() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: right bottom; }",
            &[0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Percent(1.0));
        assert_eq!(s.background_position.y, PositionComponent::Percent(1.0));
    }

    #[test]
    fn background_position_keyword_center() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: center; }",
            &[0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.background_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_invalid_value_ignored() {
        // Невалидное value → declaration invalid → остаётся initial.
        let s = cascade_at(
            "<div></div>",
            "div { background-position: bogus; }",
            &[0],
        );
        assert_eq!(
            s.background_position,
            ObjectPosition::background_initial()
        );
    }

    #[test]
    fn background_position_not_inherited() {
        // CSS Backgrounds L3 — non-inherited; ребёнок без своей декларации
        // получает initial (`0% 0%`), а не родительское `right bottom`.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-position: right bottom; }",
            &[0, 0],
        );
        assert_eq!(
            s.background_position,
            ObjectPosition::background_initial()
        );
    }

    #[test]
    fn background_position_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` принудительно тянет parent value
        // даже для non-inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-position: center; } p { background-position: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.background_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_initial_resets_to_top_left() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 80% 90%; background-position: initial; }",
            &[0],
        );
        assert_eq!(
            s.background_position,
            ObjectPosition::background_initial()
        );
    }

    // -------- image-rendering (CSS Images L3 §6.1) --------

    #[test]
    fn image_rendering_default_is_auto() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_all_keywords_parse() {
        for (val, expected) in [
            ("auto", ImageRendering::Auto),
            ("smooth", ImageRendering::Smooth),
            ("high-quality", ImageRendering::HighQuality),
            ("crisp-edges", ImageRendering::CrispEdges),
            ("pixelated", ImageRendering::Pixelated),
        ] {
            let s = cascade_at(
                "<img>",
                &format!("img {{ image-rendering: {val}; }}"),
                &[0],
            );
            assert_eq!(s.image_rendering, expected, "for value {val}");
        }
    }

    #[test]
    fn image_rendering_case_insensitive() {
        let s = cascade_at(
            "<img>",
            "img { image-rendering: PIXELATED; }",
            &[0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Pixelated);
    }

    #[test]
    fn image_rendering_invalid_value_ignored() {
        let s = cascade_at(
            "<img>",
            "img { image-rendering: bogus; }",
            &[0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_inherited() {
        // CSS Images L3 §6.1 — inherited. Ребёнок без своей декларации
        // получает значение от родителя.
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Pixelated);
    }

    #[test]
    fn image_rendering_child_override_wins() {
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; } img { image-rendering: smooth; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Smooth);
    }

    #[test]
    fn image_rendering_initial_keyword_resets() {
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; } img { image-rendering: initial; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_unset_for_inherited_is_inherit() {
        // CSS Cascade L4 §7: `unset` для inherited-свойства == `inherit`.
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: crisp-edges; } img { image-rendering: unset; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::CrispEdges);
    }

    // ── CSS Backgrounds L3 §3.7 / §3.8 — background-origin / background-clip ──

    #[test]
    fn background_origin_default_is_padding_box() {
        let s = cascade_at("<div></div>", "", &[0]);
        assert_eq!(s.background_origin, BackgroundOrigin::PaddingBox);
    }

    #[test]
    fn background_origin_all_keywords_parse() {
        for (val, expected) in [
            ("border-box", BackgroundOrigin::BorderBox),
            ("padding-box", BackgroundOrigin::PaddingBox),
            ("content-box", BackgroundOrigin::ContentBox),
        ] {
            let s = cascade_at(
                "<div></div>",
                &format!("div {{ background-origin: {val}; }}"),
                &[0],
            );
            assert_eq!(s.background_origin, expected, "for value {val}");
        }
    }

    #[test]
    fn background_origin_case_insensitive() {
        let s = cascade_at("<div></div>", "div { background-origin: BORDER-BOX; }", &[0]);
        assert_eq!(s.background_origin, BackgroundOrigin::BorderBox);
    }

    #[test]
    fn background_origin_invalid_value_ignored() {
        let s = cascade_at("<div></div>", "div { background-origin: bogus; }", &[0]);
        assert_eq!(s.background_origin, BackgroundOrigin::PaddingBox);
    }

    #[test]
    fn background_origin_not_inherited() {
        // CSS Backgrounds L3 §3.7 — non-inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; }",
            &[0, 0],
        );
        assert_eq!(s.background_origin, BackgroundOrigin::PaddingBox);
    }

    #[test]
    fn background_origin_inherit_keyword_takes_parent() {
        // `inherit` явно тянет значение родителя даже для non-inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; } p { background-origin: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_origin, BackgroundOrigin::ContentBox);
    }

    #[test]
    fn background_origin_initial_keyword_resets() {
        let s = cascade_at(
            "<div></div>",
            "div { background-origin: content-box; } div { background-origin: initial; }",
            &[0],
        );
        assert_eq!(s.background_origin, BackgroundOrigin::PaddingBox);
    }

    #[test]
    fn background_origin_unset_for_non_inherited_is_initial() {
        // CSS Cascade L4 §7: `unset` для non-inherited == `initial`.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; } p { background-origin: unset; }",
            &[0, 0],
        );
        assert_eq!(s.background_origin, BackgroundOrigin::PaddingBox);
    }

    #[test]
    fn background_clip_default_is_border_box() {
        let s = cascade_at("<div></div>", "", &[0]);
        assert_eq!(s.background_clip, BackgroundClip::BorderBox);
    }

    #[test]
    fn background_clip_all_keywords_parse() {
        for (val, expected) in [
            ("border-box", BackgroundClip::BorderBox),
            ("padding-box", BackgroundClip::PaddingBox),
            ("content-box", BackgroundClip::ContentBox),
            ("text", BackgroundClip::Text),
        ] {
            let s = cascade_at(
                "<div></div>",
                &format!("div {{ background-clip: {val}; }}"),
                &[0],
            );
            assert_eq!(s.background_clip, expected, "for value {val}");
        }
    }

    #[test]
    fn background_clip_case_insensitive() {
        let s = cascade_at("<div></div>", "div { background-clip: TEXT; }", &[0]);
        assert_eq!(s.background_clip, BackgroundClip::Text);
    }

    #[test]
    fn background_clip_invalid_value_ignored() {
        let s = cascade_at("<div></div>", "div { background-clip: bogus; }", &[0]);
        assert_eq!(s.background_clip, BackgroundClip::BorderBox);
    }

    #[test]
    fn background_clip_not_inherited() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-clip: padding-box; }",
            &[0, 0],
        );
        assert_eq!(s.background_clip, BackgroundClip::BorderBox);
    }

    #[test]
    fn background_clip_inherit_keyword_takes_parent() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-clip: text; } p { background-clip: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_clip, BackgroundClip::Text);
    }

    #[test]
    fn background_clip_initial_keyword_resets() {
        let s = cascade_at(
            "<div></div>",
            "div { background-clip: text; } div { background-clip: initial; }",
            &[0],
        );
        assert_eq!(s.background_clip, BackgroundClip::BorderBox);
    }

    // ── CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style / text-wrap ──

    #[test]
    fn text_wrap_defaults_are_initial() {
        let s = cascade_at("<p></p>", "", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_mode_keywords_parse() {
        for (val, expected) in [
            ("wrap", TextWrapMode::Wrap),
            ("nowrap", TextWrapMode::Nowrap),
        ] {
            let s = cascade_at(
                "<p></p>",
                &format!("p {{ text-wrap-mode: {val}; }}"),
                &[0],
            );
            assert_eq!(s.text_wrap_mode, expected, "for value {val}");
        }
    }

    #[test]
    fn text_wrap_style_keywords_parse() {
        for (val, expected) in [
            ("auto", TextWrapStyle::Auto),
            ("balance", TextWrapStyle::Balance),
            ("stable", TextWrapStyle::Stable),
            ("pretty", TextWrapStyle::Pretty),
        ] {
            let s = cascade_at(
                "<p></p>",
                &format!("p {{ text-wrap-style: {val}; }}"),
                &[0],
            );
            assert_eq!(s.text_wrap_style, expected, "for value {val}");
        }
    }

    #[test]
    fn text_wrap_mode_case_insensitive() {
        let s = cascade_at("<p></p>", "p { text-wrap-mode: NOWRAP; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_invalid_longhand_ignored() {
        // Невалидное значение longhand → declaration invalid → initial.
        let s = cascade_at("<p></p>", "p { text-wrap-mode: bogus; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        let s = cascade_at("<p></p>", "p { text-wrap-style: bogus; }", &[0]);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_single_mode() {
        let s = cascade_at("<p></p>", "p { text-wrap: nowrap; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_single_style() {
        let s = cascade_at("<p></p>", "p { text-wrap: balance; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_mode_then_style() {
        let s = cascade_at("<p></p>", "p { text-wrap: nowrap pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Pretty);
    }

    #[test]
    fn text_wrap_shorthand_style_then_mode() {
        // `<'mode'> || <'style'>` — порядок свободный.
        let s = cascade_at("<p></p>", "p { text-wrap: pretty nowrap; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Pretty);
    }

    #[test]
    fn text_wrap_shorthand_resets_longhands() {
        // Shorthand сбрасывает обе компоненты к initial, даже если в правиле
        // только одна указана.
        let s = cascade_at(
            "<p></p>",
            "p { text-wrap-mode: nowrap; text-wrap-style: pretty; text-wrap: balance; }",
            &[0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_invalid_token_aborts() {
        // Нераспознанный токен ⇒ shorthand отбрасывается; обе longhand остаются
        // initial после reset (см. doc-comment на apply_text_wrap_shorthand).
        let s = cascade_at("<p></p>", "p { text-wrap: bogus pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_duplicate_slot_aborts() {
        // Два token-а из одного слота (две стилистические опции) ⇒ невалидно.
        let s = cascade_at("<p></p>", "p { text-wrap: balance pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_mode_inherited() {
        // CSS Text 4 §6.4.1 — text-wrap-mode inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_style_inherited() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-style: balance; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_child_override_wins() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; } p { text-wrap-mode: wrap; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
    }

    #[test]
    fn text_wrap_initial_keyword_resets() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-style: pretty; } p { text-wrap-style: initial; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_unset_for_inherited_is_inherit() {
        // CSS Cascade L4 §7: `unset` для inherited-свойства ≡ `inherit`.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; } p { text-wrap-mode: unset; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_shorthand_css_wide_keyword_inherit_both() {
        // CSS-wide-keyword на shorthand применяется к обоим longhand-ам.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; text-wrap-style: balance; } \
             p { text-wrap: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_css_wide_keyword_initial_both() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; text-wrap-style: balance; } \
             p { text-wrap: initial; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn linear_progress_is_identity() {
        let f = TimingFunction::Linear;
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.25), 0.25));
        assert!(approx(f.progress(0.5), 0.5));
        assert!(approx(f.progress(0.75), 0.75));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn progress_clamps_t_out_of_range() {
        let f = TimingFunction::Linear;
        assert!(approx(f.progress(-0.5), 0.0));
        assert!(approx(f.progress(2.0), 1.0));
    }

    #[test]
    fn ease_keyword_endpoints() {
        let f = TimingFunction::parse("ease").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        // Midpoint of ease (cubic-bezier(0.25, 0.1, 0.25, 1.0)) ≈ 0.802 per
        // spec curves — well above 0.5, как и должно быть для ease-out shape.
        let mid = f.progress(0.5);
        assert!(mid > 0.7 && mid < 0.85, "ease(0.5) was {mid}");
    }

    #[test]
    fn ease_in_starts_slow() {
        // cubic-bezier(0.42, 0.0, 1.0, 1.0) — output быстро растёт во второй
        // половине, медленно в первой. progress(0.25) должен быть < 0.25.
        let f = TimingFunction::parse("ease-in").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        assert!(f.progress(0.25) < 0.15);
    }

    #[test]
    fn ease_out_starts_fast() {
        // cubic-bezier(0.0, 0.0, 0.58, 1.0) — output быстро растёт в первой
        // половине. progress(0.25) должен быть > 0.25.
        let f = TimingFunction::parse("ease-out").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        assert!(f.progress(0.25) > 0.35);
    }

    #[test]
    fn ease_in_out_is_symmetric_around_half() {
        // cubic-bezier(0.42, 0.0, 0.58, 1.0) — симметрично:
        // f(0.5) ≈ 0.5; f(t) + f(1-t) ≈ 1.
        let f = TimingFunction::parse("ease-in-out").unwrap();
        assert!(approx(f.progress(0.5), 0.5));
        let a = f.progress(0.2);
        let b = f.progress(0.8);
        assert!(approx(a + b, 1.0), "ease-in-out asymmetric: {a} + {b}");
    }

    #[test]
    fn cubic_bezier_diagonal_equals_linear() {
        // cubic-bezier(0, 0, 1, 1) ≡ linear (control points collinear с (0,0)→(1,1)).
        let f = TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0);
        for &t in &[0.0_f32, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0] {
            assert!(
                (f.progress(t) - t).abs() < 1e-3,
                "diagonal bezier deviated at t={t}: {}",
                f.progress(t)
            );
        }
    }

    #[test]
    fn cubic_bezier_overshoot_allowed() {
        // Контрольные y вне [0,1] → output может выходить за [0,1] (анимации
        // "spring" / bounce). Спека не clamp-ает output.
        let f = TimingFunction::CubicBezier(0.5, 1.5, 0.5, -0.5);
        let mid = f.progress(0.5);
        // По симметрии в середине ≈ 0.5, но в первой четверти > 1 не успеет
        // — overshoot скорее в y2. Главное — обработка корректна.
        let y_at_quarter = f.progress(0.25);
        let y_at_three_quarters = f.progress(0.75);
        // Симметричная кривая: f(t) + f(1-t) ≈ 1.
        assert!(approx(y_at_quarter + y_at_three_quarters, 1.0));
        assert!(approx(mid, 0.5));
    }

    #[test]
    fn steps_jump_end_default() {
        // steps(4, jump-end): 4 шага 0, 1/4, 2/4, 3/4 на интервалах
        // [0, 1/4), [1/4, 2/4), [2/4, 3/4), [3/4, 1); t=1 → 1.
        let f = TimingFunction::Steps(4, StepPosition::JumpEnd);
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.1), 0.0));
        assert!(approx(f.progress(0.25), 0.25));
        assert!(approx(f.progress(0.49), 0.25));
        assert!(approx(f.progress(0.5), 0.5));
        assert!(approx(f.progress(0.75), 0.75));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_start() {
        // steps(4, jump-start): 4 шага 1/4, 2/4, 3/4, 1 (прыжок при t=0).
        let f = TimingFunction::Steps(4, StepPosition::JumpStart);
        assert!(approx(f.progress(0.0), 0.25));
        assert!(approx(f.progress(0.1), 0.25));
        assert!(approx(f.progress(0.25), 0.5));
        assert!(approx(f.progress(0.5), 0.75));
        assert!(approx(f.progress(0.75), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_none() {
        // steps(4, jump-none): 4 уровня 0, 1/3, 2/3, 1 (нет прыжков на границах).
        let f = TimingFunction::Steps(4, StepPosition::JumpNone);
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.24), 0.0));
        assert!(approx(f.progress(0.25), 1.0 / 3.0));
        assert!(approx(f.progress(0.5), 2.0 / 3.0));
        assert!(approx(f.progress(0.75), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_both() {
        // steps(4, jump-both): 5 шагов 1/5, 2/5, 3/5, 4/5, 1 (прыжки на обеих границах).
        let f = TimingFunction::Steps(4, StepPosition::JumpBoth);
        assert!(approx(f.progress(0.0), 0.2));
        assert!(approx(f.progress(0.1), 0.2));
        assert!(approx(f.progress(0.25), 0.4));
        assert!(approx(f.progress(0.5), 0.6));
        assert!(approx(f.progress(0.75), 0.8));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn step_start_keyword_jumps_immediately() {
        let f = TimingFunction::parse("step-start").unwrap();
        assert!(approx(f.progress(0.0), 1.0));
        assert!(approx(f.progress(0.5), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn step_end_keyword_jumps_at_end() {
        let f = TimingFunction::parse("step-end").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.5), 0.0));
        assert!(approx(f.progress(0.99), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    // ──────────────── CSS Cascade L4 §6.4.3: inline style attribute ────────────────

    #[test]
    fn inline_style_background_applies() {
        // Базовая проверка BUG-003 fix — inline `style="background: ..."`
        // должен подключаться к каскаду и давать цветной фон.
        let s = cascade_at(
            r#"<div style="background: red;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.background_color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn inline_style_overrides_class_rule() {
        // CSS Cascade L4 §6.4.3: inline побеждает любой селектор в author origin.
        let s = cascade_at(
            r#"<div class="k" style="color: blue;">x</div>"#,
            ".k { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn inline_style_overrides_id_rule() {
        // Inline побеждает даже ID-селектор, чья specificity (1,0,0) выше
        // class (0,1,0): inline-tier приоритетнее specificity.
        let s = cascade_at(
            r#"<div id="x" style="color: blue;">x</div>"#,
            "#x { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn inline_style_important_beats_class_important() {
        // Inline !important побеждает class !important (равная Importance,
        // разные тиры — Element-Attached Styles побеждает в author!important).
        let s = cascade_at(
            r#"<div class="k" style="color: blue !important;">x</div>"#,
            ".k { color: red !important; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn class_important_beats_inline_normal() {
        // Author !important побеждает author normal (включая inline normal),
        // потому что Importance — главный sort-критерий.
        let s = cascade_at(
            r#"<div class="k" style="color: blue;">x</div>"#,
            ".k { color: red !important; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn inline_style_multiple_properties() {
        let s = cascade_at(
            r#"<div style="background: green; color: yellow; padding: 5px;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.background_color, Some(Color { r: 0, g: 128, b: 0, a: 255 }));
        assert_eq!(s.color, Color { r: 255, g: 255, b: 0, a: 255 });
        assert_eq!(s.padding_top, 5.0);
        assert_eq!(s.padding_right, 5.0);
        assert_eq!(s.padding_bottom, 5.0);
        assert_eq!(s.padding_left, 5.0);
    }

    #[test]
    fn inline_style_display_none_hides_element() {
        // BUG-001 manifestation: `style="display:none"` через inline должен
        // ставить display = None.
        let s = cascade_at(
            r#"<div style="display: none;">hidden</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.display, Display::None);
    }

    #[test]
    fn inline_style_empty_attribute_is_noop() {
        // Пустой `style=""` не ломает каскад; class-rule остаётся в силе.
        let s = cascade_at(
            r#"<div class="k" style="">x</div>"#,
            ".k { color: red; }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn inline_style_invalid_declaration_skipped() {
        // Невалидное declaration пропускается (recovery в parse_inline_style),
        // валидные применяются.
        let s = cascade_at(
            r#"<div style="garbage no colon; color: blue;">x</div>"#,
            "",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    // === animation shorthand parsing (CSS Animations L1 §4) ===

    fn shorthand(val: &str) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        apply_animation_shorthand(&mut s, val);
        s
    }

    #[test]
    fn shorthand_single_name_only() {
        let s = shorthand("slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert_eq!(s.animation_durations, vec![0.0]);
        assert_eq!(s.animation_delays, vec![0.0]);
        assert_eq!(s.animation_timing_functions.len(), 1);
        assert_eq!(s.animation_iteration_counts, vec![IterationCount::Finite(1.0)]);
        assert_eq!(s.animation_directions, vec![AnimationDirection::Normal]);
        assert_eq!(s.animation_fill_modes, vec![AnimationFillMode::None]);
        assert_eq!(s.animation_play_states, vec![AnimationPlayState::Running]);
    }

    #[test]
    fn shorthand_duration_then_name() {
        // Самый частый кейс в реальном CSS.
        let s = shorthand("2s slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn shorthand_duration_easing_name() {
        let s = shorthand("2s linear slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[0], TimingFunction::Linear);
    }

    #[test]
    fn shorthand_two_times_duration_and_delay() {
        // Первое <time> = duration, второе = delay.
        let s = shorthand("2s 0.5s slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.5).abs() < 1e-4);
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_negative_delay_allowed() {
        // Spec: negative delay = «анимация началась в прошлом».
        let s = shorthand("2s -0.5s slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - -0.5).abs() < 1e-4);
    }

    #[test]
    fn shorthand_ms_units() {
        let s = shorthand("500ms 100ms slidein");
        assert!((s.animation_durations[0] - 0.5).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.1).abs() < 1e-4);
    }

    #[test]
    fn shorthand_full_form_in_canonical_order() {
        // duration, easing, delay, iter-count, direction, fill-mode, play-state, name.
        let s = shorthand("2s ease-in 1s 3 alternate forwards paused slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
        assert!((s.animation_delays[0] - 1.0).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(3.0));
        assert_eq!(s.animation_directions[0], AnimationDirection::Alternate);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_any_order() {
        // `||` operator — токены могут идти в любом порядке.
        let s = shorthand("slidein alternate-reverse 1.5s infinite ease-out");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert_eq!(
            s.animation_directions[0],
            AnimationDirection::AlternateReverse
        );
        assert!((s.animation_durations[0] - 1.5).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn shorthand_cubic_bezier_with_spaces_inside() {
        // Tokenizer должен трактовать `cubic-bezier(0.42, 0, 0.58, 1)` как
        // один токен, несмотря на запятые/пробелы внутри.
        let s = shorthand("2s cubic-bezier(0.42, 0, 0.58, 1) slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
        );
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_steps_with_args() {
        let s = shorthand("1s steps(4, end) slidein");
        assert!((s.animation_durations[0] - 1.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::Steps(4, StepPosition::JumpEnd)
        );
    }

    #[test]
    fn shorthand_multiple_layers() {
        // Comma-list: 2 layers, каждый со своим набором.
        let s = shorthand("2s slidein, 3s linear slideout");
        assert_eq!(s.animation_names, vec!["slidein".to_string(), "slideout".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_durations[1] - 3.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[1], TimingFunction::Linear);
        // Layer 1 timing — default (ease).
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
    }

    #[test]
    fn shorthand_three_layers_parallel_lengths() {
        // Все 8 Vec-ов должны иметь одинаковую длину = числу layer-ов.
        let s = shorthand("1s a, 2s b, 3s c");
        assert_eq!(s.animation_names.len(), 3);
        assert_eq!(s.animation_durations.len(), 3);
        assert_eq!(s.animation_timing_functions.len(), 3);
        assert_eq!(s.animation_delays.len(), 3);
        assert_eq!(s.animation_iteration_counts.len(), 3);
        assert_eq!(s.animation_directions.len(), 3);
        assert_eq!(s.animation_fill_modes.len(), 3);
        assert_eq!(s.animation_play_states.len(), 3);
    }

    #[test]
    fn shorthand_none_keyword() {
        // `animation: none` — single layer, `none` падает в fill-mode-slot.
        // Имя остаётся пустым → consumer (animation scheduler) skip-нет.
        let s = shorthand("none");
        assert_eq!(s.animation_names, vec![String::new()]);
        assert_eq!(s.animation_fill_modes, vec![AnimationFillMode::None]);
    }

    #[test]
    fn shorthand_iteration_count_number() {
        let s = shorthand("2s 5 slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(5.0));
    }

    #[test]
    fn shorthand_iteration_count_infinite() {
        let s = shorthand("2s infinite slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
    }

    #[test]
    fn shorthand_iteration_count_fractional() {
        let s = shorthand("2s 2.5 slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.5));
    }

    #[test]
    fn shorthand_resets_previously_set_longhands() {
        // CSS Cascade L4 §6.2: shorthand сбрасывает ВСЕ longhand-ы к их
        // initial-value, если они не упомянуты в shorthand-е.
        let mut s = ComputedStyle::root();
        s.animation_delays = vec![5.0, 10.0];
        s.animation_fill_modes = vec![AnimationFillMode::Forwards];
        s.animation_directions = vec![AnimationDirection::Reverse];
        apply_animation_shorthand(&mut s, "2s slidein");
        // duration упомянут → 2s. delay/fill/direction не упомянуты → initial.
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::None);
        assert_eq!(s.animation_directions[0], AnimationDirection::Normal);
    }

    #[test]
    fn shorthand_empty_value_clears_all() {
        // Пустое значение → нет layer-ов → все Vec-и пустые.
        let s = shorthand("");
        assert!(s.animation_names.is_empty());
        assert!(s.animation_durations.is_empty());
        assert!(s.animation_timing_functions.is_empty());
        assert!(s.animation_delays.is_empty());
        assert!(s.animation_iteration_counts.is_empty());
        assert!(s.animation_directions.is_empty());
        assert!(s.animation_fill_modes.is_empty());
        assert!(s.animation_play_states.is_empty());
    }

    #[test]
    fn shorthand_only_keywords_no_name() {
        // Если имя не указано, name остаётся пустым.
        let s = shorthand("2s linear forwards");
        assert_eq!(s.animation_names, vec![String::new()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[0], TimingFunction::Linear);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
    }

    #[test]
    fn shorthand_step_start_keyword() {
        let s = shorthand("0.5s step-start slidein");
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::Steps(1, StepPosition::JumpStart)
        );
    }

    #[test]
    fn shorthand_paused_play_state() {
        let s = shorthand("2s paused slidein");
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
    }

    #[test]
    fn shorthand_reverse_direction() {
        let s = shorthand("2s reverse slidein");
        assert_eq!(s.animation_directions[0], AnimationDirection::Reverse);
    }

    #[test]
    fn shorthand_both_fill_mode() {
        let s = shorthand("2s both slidein");
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Both);
    }

    #[test]
    fn shorthand_through_apply_declaration() {
        // Полная цепочка: Declaration → apply_declaration. Sanity-check
        // что branch в match подхватывает shorthand.
        let mut s = ComputedStyle::root();
        let viewport = Size {
            width: 1024.0,
            height: 768.0,
        };
        let inherited = ComputedStyle::root();
        let decl = Declaration {
            property: "animation".to_string(),
            value: "2s ease-in-out 0.5s 2 alternate forwards paused fade".to_string(),
            important: false,
        };
        apply_declaration(&mut s, &decl, 16.0, viewport, FontWeight::default(), &inherited, false);
        assert_eq!(s.animation_names, vec!["fade".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.5).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.0));
        assert_eq!(s.animation_directions[0], AnimationDirection::Alternate);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
    }

    #[test]
    fn shorthand_tokenize_with_parens_handles_nested() {
        // Sanity-check helper: вложенные скобки не разбиваются на пробелах.
        let tokens = tokenize_with_parens("a cubic-bezier(0.1, 0.2, 0.3, 0.4) b");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], "a");
        // Внутри скобок пробелы и запятые сохраняются — это один токен.
        assert_eq!(tokens[1], "cubic-bezier(0.1, 0.2, 0.3, 0.4)");
        assert_eq!(tokens[2], "b");
    }

    // === transition shorthand parsing (CSS Transitions L1 §3) ===

    fn ts(val: &str) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        apply_transition_shorthand(&mut s, val);
        s
    }

    #[test]
    fn transition_shorthand_duration_only() {
        // `transition: 1s` → property = initial "all".
        let s = ts("1s");
        assert_eq!(s.transition_properties, vec!["all".to_string()]);
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_property_and_duration() {
        let s = ts("opacity 0.3s");
        assert_eq!(s.transition_properties, vec!["opacity".to_string()]);
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_full_form() {
        let s = ts("opacity 0.3s ease-out 0.1s");
        assert_eq!(s.transition_properties, vec!["opacity".to_string()]);
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_any_order() {
        // Per spec — `||` оператор, любой порядок.
        let s = ts("ease-in 0.5s transform 0.2s");
        assert_eq!(s.transition_properties, vec!["transform".to_string()]);
        assert!((s.transition_durations[0] - 0.5).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.2).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_ms_units() {
        let s = ts("opacity 200ms 50ms");
        assert!((s.transition_durations[0] - 0.2).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.05).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_multiple_layers() {
        let s = ts("opacity 0.3s, transform 0.5s ease-in");
        assert_eq!(
            s.transition_properties,
            vec!["opacity".to_string(), "transform".to_string()]
        );
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_durations[1] - 0.5).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[1],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_three_layers_parallel_lengths() {
        // Все 4 Vec-а должны иметь длину = числу layers.
        let s = ts("opacity 1s, transform 2s linear, color 3s ease-in 0.5s");
        assert_eq!(s.transition_properties.len(), 3);
        assert_eq!(s.transition_durations.len(), 3);
        assert_eq!(s.transition_timing_functions.len(), 3);
        assert_eq!(s.transition_delays.len(), 3);
    }

    #[test]
    fn transition_shorthand_none_layer() {
        // `transition: none` — single layer, property=none, остальное — initial.
        let s = ts("none");
        assert_eq!(s.transition_properties, vec!["none".to_string()]);
        assert!((s.transition_durations[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_cubic_bezier_with_spaces_inside() {
        let s = ts("opacity 0.5s cubic-bezier(0.1, 0.2, 0.3, 0.4)");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.1, 0.2, 0.3, 0.4)
        );
    }

    #[test]
    fn transition_shorthand_steps_with_args() {
        let s = ts("opacity 1s steps(4, end)");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::Steps(4, StepPosition::JumpEnd)
        );
    }

    #[test]
    fn transition_shorthand_resets_previously_set_longhands() {
        // CSS Cascade L4 §6.2: shorthand сбрасывает longhand-ы к initial.
        let mut s = ComputedStyle::root();
        s.transition_durations = vec![5.0, 10.0];
        s.transition_delays = vec![1.0, 2.0];
        s.transition_timing_functions = vec![TimingFunction::Linear];
        apply_transition_shorthand(&mut s, "opacity 0.3s");
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
        assert_eq!(s.transition_durations.len(), 1);
        assert_eq!(s.transition_delays.len(), 1);
        assert_eq!(s.transition_timing_functions.len(), 1);
    }

    #[test]
    fn transition_shorthand_empty_value_clears_all() {
        let s = ts("");
        assert!(s.transition_properties.is_empty());
        assert!(s.transition_durations.is_empty());
        assert!(s.transition_timing_functions.is_empty());
        assert!(s.transition_delays.is_empty());
    }

    #[test]
    fn transition_shorthand_only_timing() {
        // `transition: linear` — property=all (initial), duration=0.
        let s = ts("linear");
        assert_eq!(s.transition_properties, vec!["all".to_string()]);
        assert_eq!(s.transition_timing_functions[0], TimingFunction::Linear);
        assert!((s.transition_durations[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_step_start_keyword() {
        let s = ts("opacity 0.5s step-start");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::Steps(1, StepPosition::JumpStart)
        );
    }

    #[test]
    fn transition_shorthand_through_apply_declaration() {
        // Полная цепочка: Declaration → apply_declaration.
        let mut s = ComputedStyle::root();
        let viewport = Size {
            width: 1024.0,
            height: 768.0,
        };
        let inherited = ComputedStyle::root();
        let decl = Declaration {
            property: "transition".to_string(),
            value: "transform 0.4s ease-in-out 0.1s".to_string(),
            important: false,
        };
        apply_declaration(&mut s, &decl, 16.0, viewport, FontWeight::default(), &inherited, false);
        assert_eq!(s.transition_properties, vec!["transform".to_string()]);
        assert!((s.transition_durations[0] - 0.4).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_negative_delay_allowed() {
        // CSS Transitions L1 §3: negative delay допустим — анимация
        // начинается с прогрессом, как будто уже игралась.
        let s = ts("opacity 1s -0.2s");
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - (-0.2)).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_two_times_duration_and_delay() {
        // 1s сначала = duration, 0.5s потом = delay.
        let s = ts("1s 0.5s");
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.5).abs() < 1e-4);
    }

    // ── HTML5 §2.4.6 «rules for parsing a legacy color value» ─────────────

    #[test]
    fn legacy_color_empty_is_error() {
        assert_eq!(parse_legacy_color_html_attr(""), None);
    }

    #[test]
    fn legacy_color_whitespace_only_is_error() {
        // Spec step 3 trim → empty → fail.
        assert_eq!(parse_legacy_color_html_attr("   "), None);
        assert_eq!(parse_legacy_color_html_attr("\t\n\r"), None);
    }

    #[test]
    fn legacy_color_transparent_keyword_is_error() {
        // Spec step 4: «transparent» — единственный keyword, дающий error.
        assert_eq!(parse_legacy_color_html_attr("transparent"), None);
        assert_eq!(parse_legacy_color_html_attr("TRANSPARENT"), None);
        assert_eq!(parse_legacy_color_html_attr("  Transparent  "), None);
    }

    #[test]
    fn legacy_color_named_lookup() {
        assert_eq!(parse_legacy_color_html_attr("red"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("RED"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("Blue"), Some(rgba(0, 0, 255, 255)));
        assert_eq!(parse_legacy_color_html_attr("rebeccapurple"), Some(rgba(102, 51, 153, 255)));
    }

    #[test]
    fn legacy_color_hash_short_hex() {
        // Spec step 6: 4-char #rgb с hex-digits expand до #rrggbb.
        assert_eq!(parse_legacy_color_html_attr("#f00"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#0f0"), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#abc"), Some(rgba(170, 187, 204, 255)));
    }

    #[test]
    fn legacy_color_hash_long_hex() {
        // Spec steps 8+: # удаляется, остальное идёт в общий padding-procedure.
        assert_eq!(parse_legacy_color_html_attr("#ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#FF0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#abcdef"), Some(rgba(0xab, 0xcd, 0xef, 255)));
    }

    #[test]
    fn legacy_color_hashless_hex_6_digits() {
        // HTML legacy (в отличие от CSS quirk!) принимает hashless hex.
        // 6 digits → split на 3 по 2 → strip leading zeros (none) → r,g,b.
        assert_eq!(parse_legacy_color_html_attr("ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("00ff00"), Some(rgba(0, 255, 0, 255)));
    }

    #[test]
    fn legacy_color_hashless_3_digits_no_expand() {
        // ВАЖНО: для hashless `f00` short-hex expand (step 6) НЕ работает —
        // он только для 4-char `#xyz`. «f00» проходит через общий path:
        // split на 3 по 1 → length=1, не пакуется до 2 → r=0xf, g=0, b=0.
        assert_eq!(parse_legacy_color_html_attr("f00"), Some(rgba(15, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_garbage_replaced_with_zeros() {
        // Spec step 9: не-hex chars заменяются на «0». «garbage» → «0a0ba0e».
        // Длина 7 не multiple of 3 → padding до 9 → «0a0ba0e00».
        // Split: «0a0», «ba0», «e00». length=3. Все ведущие? red[0]='0',
        // green[0]='b' — нет, не strip. Truncate до 2 каждый: «0a», «ba», «e0».
        assert_eq!(parse_legacy_color_html_attr("garbage"), Some(rgba(0x0a, 0xba, 0xe0, 255)));
    }

    #[test]
    fn legacy_color_pads_short_to_multiple_of_three() {
        // «1» → padding → «100» → split «1»,«0»,«0» → r=1, g=0, b=0.
        // length=1, не > 2, не truncate. Парсим: «1»→1, «0»→0, «0»→0.
        assert_eq!(parse_legacy_color_html_attr("1"), Some(rgba(1, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_strips_common_leading_zeros() {
        // «000a000b000c» → length=4, > 2. Все ведущие «0»? red[0]='0',
        // green[0]='0', blue[0]='0' → strip. length=3, всё ещё все ведущие
        // '0' → strip. length=2, останавливаемся. red=«0a», green=«0b», blue=«0c».
        assert_eq!(parse_legacy_color_html_attr("000a000b000c"), Some(rgba(0x0a, 0x0b, 0x0c, 255)));
    }

    #[test]
    fn legacy_color_truncates_after_strip() {
        // «aabbccdd0aabbccdd0aabbccdd0» — 27 chars, length=9. Step 12:
        // length > 8 → срезаем leading 1 (=length-8) из каждого, length=8.
        // Затем step 13 / 14. Проверяем что точно не паникует и валидный
        // цвет, без захода в детали значения.
        let result = parse_legacy_color_html_attr("aabbccdd0aabbccdd0aabbccdd0");
        assert!(result.is_some());
    }

    #[test]
    fn legacy_color_strips_hash_from_long_string() {
        // С `#` префиксом, но не вписывается в step 6 (длина ≠ 4): идёт через
        // step 8 (strip `#`) + общий процесс. «#xyz» с не-hex → `0`-replace.
        // Здесь «#ff» → strip `#` → «ff» → pad до 3 → «ff0» → split «f»,«f»,«0»
        // → length=1 → r=15, g=15, b=0.
        assert_eq!(parse_legacy_color_html_attr("#ff"), Some(rgba(15, 15, 0, 255)));
    }

    #[test]
    fn legacy_color_4char_hash_with_non_hex_takes_general_path() {
        // «#xyz» — длина 4, начинается с `#`, но `x` не hex → step 6 не
        // срабатывает. Идёт общий путь: strip `#` → «xyz» → replace non-hex
        // → «000» → split «0»,«0»,«0» → r=g=b=0.
        assert_eq!(parse_legacy_color_html_attr("#xyz"), Some(rgba(0, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_non_bmp_replaced_with_two_zeros() {
        // U+1F3A8 (🎨) > U+FFFF → заменяется на «00». «🎨» → «00» → pad до 3
        // → «000» → r=g=b=0.
        assert_eq!(parse_legacy_color_html_attr("🎨"), Some(rgba(0, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_trim_outer_whitespace() {
        // Spec step 3 strip leading/trailing whitespace — но не внутренний
        // (мусор внутри идёт через replace-non-hex).
        assert_eq!(parse_legacy_color_html_attr("  red  "), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("\t#ff0000\n"), Some(rgba(255, 0, 0, 255)));
    }

    // ── apply_bgcolor_presentational_hint integration ────────────────────

    fn doc_root_child_style(html: &str) -> ComputedStyle {
        // Берём первого ребёнка document root (`<body>` / `<table>` / ...),
        // считаем для него ComputedStyle с пустым CSS.
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let node = doc.get(doc.root()).children[0];
        compute_style(&doc, node, &sheet, &root_style, Size::new(800.0, 600.0))
    }

    #[test]
    fn bgcolor_hint_body_named() {
        let s = doc_root_child_style("<body bgcolor=\"red\"></body>");
        assert_eq!(s.background_color, Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn bgcolor_hint_body_hash() {
        let s = doc_root_child_style("<body bgcolor=\"#00ff00\"></body>");
        assert_eq!(s.background_color, Some(rgba(0, 255, 0, 255)));
    }

    #[test]
    fn bgcolor_hint_body_hashless_legacy() {
        // Главное отличие HTML legacy от CSS quirk: hashless hex принимается
        // без зависимости от document mode.
        let s = doc_root_child_style("<body bgcolor=\"0000ff\"></body>");
        assert_eq!(s.background_color, Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn bgcolor_hint_table_named() {
        let s = doc_root_child_style("<table bgcolor=\"yellow\"></table>");
        assert_eq!(s.background_color, Some(rgba(255, 255, 0, 255)));
    }

    #[test]
    fn bgcolor_hint_not_applied_to_div() {
        // <div bgcolor="red"> — bgcolor не присутствует в spec для div,
        // hint игнорируется.
        let s = doc_root_child_style("<div bgcolor=\"red\"></div>");
        assert_eq!(s.background_color, None);
    }

    #[test]
    fn bgcolor_hint_transparent_does_not_apply() {
        // «transparent» — error в legacy-парсере, hint не применяется.
        let s = doc_root_child_style("<body bgcolor=\"transparent\"></body>");
        assert_eq!(s.background_color, None);
    }

    #[test]
    fn bgcolor_hint_overridden_by_author_css() {
        // Presentational hint имеет lowest specificity — любой author CSS
        // перекрывает (HTML5 §10 «Mapped attributes»).
        let doc = lumen_html_parser::parse("<body bgcolor=\"red\"></body>");
        let sheet = lumen_css_parser::parse("body { background-color: blue; }");
        let root_style = ComputedStyle::root();
        let body = doc.get(doc.root()).children[0];
        let s = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.background_color, Some(rgba(0, 0, 255, 255)));
    }

    #[test]
    fn bgcolor_hint_td_inside_table() {
        // td тоже принимает bgcolor.
        let doc = lumen_html_parser::parse("<table><tr><td bgcolor=\"#abcdef\">x</td></tr></table>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        // Найдём td через обход.
        let table = doc.get(doc.root()).children[0];
        let tr = doc.get(table).children[0];
        let td = doc.get(tr).children[0];
        let s = compute_style(&doc, td, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.background_color, Some(rgba(0xab, 0xcd, 0xef, 255)));
    }

    // ── apply_text_color_presentational_hint integration ─────────────────

    #[test]
    fn text_hint_body_named() {
        let s = doc_root_child_style("<body text=\"red\"></body>");
        assert_eq!(s.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn text_hint_body_hash() {
        let s = doc_root_child_style("<body text=\"#00ff00\"></body>");
        assert_eq!(s.color, rgba(0, 255, 0, 255));
    }

    #[test]
    fn text_hint_body_hashless_legacy() {
        // Hashless hex принимается legacy-парсером без зависимости от
        // document mode — как и в bgcolor.
        let s = doc_root_child_style("<body text=\"0000ff\"></body>");
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn text_hint_transparent_does_not_apply() {
        // «transparent» — error в legacy-парсере, hint не применяется
        // → color остаётся default (BLACK через initial).
        let s = doc_root_child_style("<body text=\"transparent\"></body>");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn text_hint_not_applied_to_div() {
        // <div text="red"> — `text` атрибут не присутствует в spec для div,
        // hint игнорируется.
        let s = doc_root_child_style("<div text=\"red\"></div>");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn text_hint_overridden_by_author_css() {
        // Presentational hint имеет lowest specificity — author CSS перекрывает.
        let doc = lumen_html_parser::parse("<body text=\"red\"></body>");
        let sheet = lumen_css_parser::parse("body { color: blue; }");
        let root_style = ComputedStyle::root();
        let body = doc.get(doc.root()).children[0];
        let s = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn text_hint_body_inherits_to_children() {
        // CSS `color` — inherited; legacy `text` на `<body>` должно через
        // наследование красить потомков без явного color.
        let doc = lumen_html_parser::parse("<body text=\"red\"><div>x</div></body>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let body = doc.get(doc.root()).children[0];
        let div = doc.get(body).children[0];
        let body_style = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0));
        let div_style = compute_style(&doc, div, &sheet, &body_style, Size::new(800.0, 600.0));
        assert_eq!(div_style.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn font_color_hint_named() {
        // <font color="red"> сам по себе. doc_root_child_style вернёт стиль
        // <font>-элемента; tree builder может обернуть его в <body> —
        // используем явный обход.
        let doc = lumen_html_parser::parse("<font color=\"red\">x</font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn font_color_hint_hash() {
        let doc = lumen_html_parser::parse("<font color=\"#abcdef\">x</font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, rgba(0xab, 0xcd, 0xef, 255));
    }

    #[test]
    fn font_color_hint_overridden_by_author_css() {
        let doc = lumen_html_parser::parse("<font color=\"red\">x</font>");
        let sheet = lumen_css_parser::parse("font { color: blue; }");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn font_color_hint_inherits_to_children() {
        let doc =
            lumen_html_parser::parse("<font color=\"red\"><span>x</span></font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let span = find_first_element(&doc, font, "span").expect("span found");
        let font_style = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0));
        let span_style =
            compute_style(&doc, span, &sheet, &font_style, Size::new(800.0, 600.0));
        assert_eq!(span_style.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn color_attr_on_div_does_not_apply() {
        // `color` атрибут — presentational hint только для `<font>`. На
        // `<div color="red">` игнорируется.
        let doc = lumen_html_parser::parse("<div color=\"red\">x</div>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let s = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(s.color, Color::BLACK);
    }

    fn find_first_element(
        doc: &lumen_dom::Document,
        from: lumen_dom::NodeId,
        local: &str,
    ) -> Option<lumen_dom::NodeId> {
        let node = doc.get(from);
        if let lumen_dom::NodeData::Element { name, .. } = &node.data
            && name.local == local
        {
            return Some(from);
        }
        for child in &node.children {
            if let Some(found) = find_first_element(doc, *child, local) {
                return Some(found);
            }
        }
        None
    }

    // ── matches_defined (CSS Selectors L4 §6.4.1 / HTML LS §4.13.5) ──────

    fn first_child_of_root(doc: &lumen_dom::Document) -> lumen_dom::NodeId {
        doc.get(doc.root()).children[0]
    }

    #[test]
    fn defined_matches_builtin_html_element() {
        // `<div>` — built-in, defined.
        let doc = lumen_html_parser::parse("<div></div>");
        let node = first_child_of_root(&doc);
        assert!(matches_defined(&doc, node));
    }

    #[test]
    fn defined_matches_arbitrary_unknown_no_hyphen() {
        // `<foo>` без дефиса не может быть валидным custom-element-именем
        // (HTML LS §4.13.2 требует дефис), значит трактуется как built-in
        // unknown — defined.
        let doc = lumen_html_parser::parse("<foo></foo>");
        let node = first_child_of_root(&doc);
        assert!(matches_defined(&doc, node));
    }

    #[test]
    fn defined_does_not_match_custom_element_name() {
        // `<my-button>` — валидное custom-element-имя, в Phase 0 без
        // registry никогда не defined.
        let doc = lumen_html_parser::parse("<my-button></my-button>");
        let node = first_child_of_root(&doc);
        assert!(!matches_defined(&doc, node));
    }

    #[test]
    fn defined_does_not_match_deep_custom_element_name() {
        // Имя с несколькими дефисами — тоже custom (`<x-y-z>` валидно).
        let doc = lumen_html_parser::parse("<x-y-z></x-y-z>");
        let node = first_child_of_root(&doc);
        assert!(!matches_defined(&doc, node));
    }

    #[test]
    fn defined_selector_filters_custom_elements_in_cascade() {
        // E2E: `:not(:defined) { display: none }` скрывает custom-element
        // (FOUC-protection idiom). Built-in остаётся видимым.
        let doc =
            lumen_html_parser::parse("<my-card></my-card><div></div>");
        let sheet =
            lumen_css_parser::parse(":not(:defined) { display: none; }");
        let root_style = ComputedStyle::root();
        let root = doc.get(doc.root());
        let my_card = root.children[0];
        let div = root.children[1];
        let my_card_style =
            compute_style(&doc, my_card, &sheet, &root_style, Size::new(800.0, 600.0));
        let div_style =
            compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(my_card_style.display, Display::None);
        assert_ne!(div_style.display, Display::None);
    }
}
