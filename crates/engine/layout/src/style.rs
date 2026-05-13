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
    AttrOp, AttrSelector, Combinator, ComplexSelector, CompoundSelector, Declaration, PropertyRule,
    PseudoClass, SimpleSelector, Specificity, Stylesheet,
};
use lumen_dom::{Attribute, Document, NodeData, NodeId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
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
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None)
    }
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
    /// CSS UI L4 §3: outline. В отличие от border не сдвигает соседей и
    /// не учитывается в width/height (рисуется поверх / снаружи коробки).
    /// Color = None → currentColor. Не наследуется.
    pub outline_width: f32,
    pub outline_style: BorderStyle,
    pub outline_color: Option<Color>,
    /// CSS UI L4 §3.4 — outline-offset (resolved px). Положительное —
    /// outline отрисовывается дальше от боксa, отрицательное — внутрь.
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
}

impl ComputedStyle {
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
            outline_width: 0.0,
            outline_style: BorderStyle::None,
            outline_color: None,
            outline_offset: 0.0,
            accent_color: None,
            custom_props: HashMap::new(),
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
        outline_width: 0.0,
        outline_style: BorderStyle::None,
        outline_color: None,
        outline_offset: 0.0,
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
            !registry.get(key.as_str()).is_some_and(|p| !p.inherits)
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

    // Собираем все matched declarations с их sort key:
    // (important, specificity, rule_order, decl_index). `important` идёт
    // первым: после ascending sort `true > false`, поэтому !important идёт в
    // конец и побеждает normal даже при меньшей specificity (CSS Cascade L4
    // §8.1). Внутри одного origin `important = false` сначала разрешается
    // обычный каскад, потом тот же каскад применяется поверх с !important.
    let mut matched: Vec<(bool, Specificity, usize, usize, &Declaration)> = Vec::new();
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
                matched.push((decl.important, spec, rule_idx, decl_idx, decl));
            }
        }
    }
    matched.sort_by_key(|&(imp, spec, rule_idx, decl_idx, _)| (imp, spec, rule_idx, decl_idx));

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    let parent_fs = inherited.font_size;
    for (_, _, _, _, decl) in &matched {
        apply_font_size(&mut style, decl, parent_fs, viewport);
    }

    // Custom-properties pass: все `--name: value` декларации применяются
    // отдельно и ДО main-pass, чтобы любая обычная декларация в main-pass
    // могла видеть финальное значение custom property независимо от порядка
    // объявления в source. Каскад уже соблюдён через sort `matched`:
    // последующая запись с тем же ключом перебивает раннюю.
    for (_, _, _, _, decl) in &matched {
        if let Some(name) = decl.property.strip_prefix("--") {
            let key = format!("--{name}");
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
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`.
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;
    for (_, _, _, _, decl) in &matched {
        apply_declaration(&mut style, decl, em_basis, viewport, parent_weight);
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
            custom_props.insert((*name).to_string(), iv.clone());
        }
    }
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
        PseudoClass::NthChild(spec) => match element_index(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastChild(spec) => match element_index(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthOfType(spec) => match element_index_of_type(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastOfType(spec) => match element_index_of_type(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::Not(inner) => !matches_compound(inner, doc, node),
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
        PseudoClass::Unsupported(_) => false,
    }
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
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript" => {
            Display::None
        }
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
fn parse_box_shadow_one(s: &str, em_basis: f32, viewport: Size) -> Option<BoxShadow> {
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
        } else if let Some(c) = parse_color(&tok) {
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
fn parse_text_shadow_one(s: &str, em_basis: f32, viewport: Size) -> Option<TextShadow> {
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
        if let Some(c) = parse_color(&tok) {
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
pub fn parse_length(s: &str) -> Option<Length> {
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
    s.parse::<f32>().ok().map(Length::Px)
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
    let val: &str = if decl.value.contains("var(") {
        match expand_vars(&decl.value, &style.custom_props, 0) {
            Some(v) => {
                expanded = v;
                expanded.as_str()
            }
            None => return,
        }
    } else {
        decl.value.as_str()
    };
    match prop {
        "display" => {
            style.display = match val {
                "block" => Display::Block,
                "inline" => Display::Inline,
                "none" => Display::None,
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
            if let Some(c) = parse_color(val) {
                style.color = c;
            }
        }
        "background-color" | "background" => {
            if let Some(c) = parse_color(val) {
                style.background_color = Some(c);
            }
        }
        "accent-color" => {
            // CSS UI L4 §6.1: <color> | auto.
            // 'auto' = None — UA сама подберёт цвет (обычно системный акцент).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.accent_color = None;
            } else if let Some(c) = parse_color(trimmed) {
                style.accent_color = Some(c);
            }
        }
        "width" if val != "auto" => {
            style.width = parse_length(val).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        "height" if val != "auto" => {
            style.height = parse_length(val).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        // CSS 2.1 §10.4: min-/max- ширина и высота. Отрицательные значения
        // запрещены спецификацией — отбрасываем. `none` для max-* = снять
        // ограничение (None). `auto` для min-* (CSS3 Sizing default для
        // flex/grid) трактуем как None — Phase 0 без flex/grid, это
        // эквивалентно нулевому минимуму.
        "min-width" if val != "auto" => {
            style.min_width = parse_length(val)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "max-width" if val != "none" => {
            style.max_width = parse_length(val)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "min-height" if val != "auto" => {
            style.min_height = parse_length(val)
                .and_then(|l| l.resolve(em_basis, None, viewport))
                .filter(|v| *v >= 0.0);
        }
        "max-height" if val != "none" => {
            style.max_height = parse_length(val)
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
            if let Some(len) = parse_length(val)
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
            } else if let Some(len) = parse_length(val)
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
            } else if let Some(len) = parse_length(val)
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
                    if let Some(s) = parse_box_shadow_one(piece.trim(), em_basis, viewport) {
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
                    if let Some(s) = parse_text_shadow_one(piece.trim(), em_basis, viewport) {
                        shadows.push(s);
                    }
                }
                if !shadows.is_empty() {
                    style.text_shadow = shadows;
                }
            }
        }
        "outline" => {
            // outline shorthand — аналог border-shorthand, но применяется к
            // одному «слою» поверх коробки. Сбрасывает все три свойства.
            style.outline_width = 0.0;
            style.outline_style = BorderStyle::None;
            style.outline_color = None;
            for tok in val.split_whitespace() {
                if let Some(v) = resolve_box_length(tok, em_basis, viewport) {
                    style.outline_width = v;
                } else if is_border_style_kw(tok) {
                    style.outline_style = parse_border_style_kw(tok);
                } else if let Some(c) = parse_color(tok) {
                    style.outline_color = Some(c);
                }
            }
        }
        "outline-width" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.outline_width = v;
            }
        }
        "outline-style" => {
            style.outline_style = parse_border_style_kw(val);
        }
        "outline-color" => {
            if let Some(c) = parse_color(val) {
                style.outline_color = Some(c);
            }
        }
        "outline-offset" => {
            // <length>; отрицательные значения валидны (CSS UI L4 §3.4).
            if let Some(len) = parse_length(val)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.outline_offset = px;
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
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.margin_top = v;
                style.margin_right = v;
                style.margin_bottom = v;
                style.margin_left = v;
            }
        }
        "margin-top" => set_box_length(&mut style.margin_top, val, em_basis, viewport),
        "margin-right" => set_box_length(&mut style.margin_right, val, em_basis, viewport),
        "margin-bottom" => set_box_length(&mut style.margin_bottom, val, em_basis, viewport),
        "margin-left" => set_box_length(&mut style.margin_left, val, em_basis, viewport),
        "padding" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.padding_top = v;
                style.padding_right = v;
                style.padding_bottom = v;
                style.padding_left = v;
            }
        }
        "padding-top" => set_box_length(&mut style.padding_top, val, em_basis, viewport),
        "padding-right" => set_box_length(&mut style.padding_right, val, em_basis, viewport),
        "padding-bottom" => set_box_length(&mut style.padding_bottom, val, em_basis, viewport),
        "padding-left" => set_box_length(&mut style.padding_left, val, em_basis, viewport),
        "text-decoration" => {
            // Shorthand: `<line> <style> <color>` в любом порядке (CSS Text
            // Decoration L3 §2.1). Парсер собирает линии-keyword-ы и пытается
            // отдельно интерпретировать остатки как цвет (rgb/hsl/oklch/hex
            // /name). style (solid/wavy/…) и `blink` пока тихо игнорируем.
            let (line, color) = parse_text_decoration_shorthand(val);
            if let Some(d) = line {
                style.text_decoration_line = d;
            }
            if let Some(c) = color {
                style.text_decoration_color = Some(c);
            }
        }
        "text-decoration-line" => {
            let (line, _color) = parse_text_decoration_shorthand(val);
            if let Some(d) = line {
                style.text_decoration_line = d;
            }
        }
        "text-decoration-color" => {
            // `currentcolor` сбрасывает в None — даёт fallback на style.color
            // при рендеринге. CSS3 не описывает явное «возврат к default»,
            // но `currentColor` имеет ту же семантику.
            if val.eq_ignore_ascii_case("currentcolor") {
                style.text_decoration_color = None;
            } else if let Some(c) = parse_color(val) {
                style.text_decoration_color = Some(c);
            }
        }
        // ── Borders ───────────────────────────────────────────────────────────
        "border" => apply_border_shorthand(style, val, em_basis, viewport),
        "border-top" => apply_border_side_shorthand(
            &mut style.border_top_width, &mut style.border_top_style,
            &mut style.border_top_color, val, em_basis, viewport),
        "border-right" => apply_border_side_shorthand(
            &mut style.border_right_width, &mut style.border_right_style,
            &mut style.border_right_color, val, em_basis, viewport),
        "border-bottom" => apply_border_side_shorthand(
            &mut style.border_bottom_width, &mut style.border_bottom_style,
            &mut style.border_bottom_color, val, em_basis, viewport),
        "border-left" => apply_border_side_shorthand(
            &mut style.border_left_width, &mut style.border_left_style,
            &mut style.border_left_color, val, em_basis, viewport),
        "border-width" => {
            let sides = expand_border_4(val);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport) { style.border_top_width = v; }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport) { style.border_right_width = v; }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport) { style.border_bottom_width = v; }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport) { style.border_left_width = v; }
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
            if let Some(c) = parse_color(sides[0]) { style.border_top_color = Some(c); }
            if let Some(c) = parse_color(sides[1]) { style.border_right_color = Some(c); }
            if let Some(c) = parse_color(sides[2]) { style.border_bottom_color = Some(c); }
            if let Some(c) = parse_color(sides[3]) { style.border_left_color = Some(c); }
        }
        "border-radius" => {
            // CSS Backgrounds L3 §5.5 shorthand. Поддерживаем только
            // horizontal-radius (без `/`-formed elliptical часть). 1-4 токена
            // по правилу expand_border_4 (TL TR BR BL).
            // Формы вроде `5px / 10px` (elliptical) Phase 0 не поддерживает —
            // берём первую часть до `/`.
            let h_part = val.split('/').next().unwrap_or(val);
            let sides = expand_border_4(h_part);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport) {
                style.border_top_left_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport) {
                style.border_top_right_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport) {
                style.border_bottom_right_radius = v.max(0.0);
            }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport) {
                style.border_bottom_left_radius = v.max(0.0);
            }
        }
        "border-top-left-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.border_top_left_radius = v.max(0.0);
            }
        }
        "border-top-right-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.border_top_right_radius = v.max(0.0);
            }
        }
        "border-bottom-right-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.border_bottom_right_radius = v.max(0.0);
            }
        }
        "border-bottom-left-radius" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.border_bottom_left_radius = v.max(0.0);
            }
        }
        "border-top-width" => set_box_length(&mut style.border_top_width, val, em_basis, viewport),
        "border-right-width" => set_box_length(&mut style.border_right_width, val, em_basis, viewport),
        "border-bottom-width" => set_box_length(&mut style.border_bottom_width, val, em_basis, viewport),
        "border-left-width" => set_box_length(&mut style.border_left_width, val, em_basis, viewport),
        "border-top-style" => style.border_top_style = parse_border_style_kw(val),
        "border-right-style" => style.border_right_style = parse_border_style_kw(val),
        "border-bottom-style" => style.border_bottom_style = parse_border_style_kw(val),
        "border-left-style" => style.border_left_style = parse_border_style_kw(val),
        "border-top-color" => { if let Some(c) = parse_color(val) { style.border_top_color = Some(c); } }
        "border-right-color" => { if let Some(c) = parse_color(val) { style.border_right_color = Some(c); } }
        "border-bottom-color" => { if let Some(c) = parse_color(val) { style.border_bottom_color = Some(c); } }
        "border-left-color" => { if let Some(c) = parse_color(val) { style.border_left_color = Some(c); } }
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

/// Разбирает `text-decoration` shorthand или `text-decoration-line`.
///
/// Возвращает `(line, color)`. `color` извлекается только если в строке
/// есть остаточный токен после keyword-ов линий и стилей — и он успешно
/// парсится `parse_color`-ом.
///
/// Phase 0 keyword-ы линий: `underline`, `overline`, `line-through`, `none`.
/// `none` сбрасывает все линии (CSS3 «none — initial value», интуитивно
/// побеждает явный сброс). Стиль (`solid`/`wavy`/`dashed`/`dotted`/`double`)
/// и `blink` (CSS2 deprecated) пока тихо игнорируем — нет реализации в
/// paint, но токены распознаём, чтобы их остаток не попадал в color-парсер.
///
/// `currentcolor` keyword в shorthand сбрасывает text-decoration-color в
/// None (= fallback на currentColor при рендеринге).
fn parse_text_decoration_shorthand(val: &str) -> (Option<TextDecorationLine>, Option<Color>) {
    let mut out = TextDecorationLine::default();
    let mut any_line = false;
    let mut none_seen = false;
    let mut color: Option<Color> = None;
    let mut color_currentcolor = false;
    // Цвет может быть многословным: `rgb(0, 0, 0)`, `hsl(0 0% 0% / 1)`, …
    // Соберём «не-линия / не-стиль» токены и попытаемся склеить.
    let mut residue: Vec<&str> = Vec::new();
    for token in val.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "none" => {
                none_seen = true;
                any_line = true;
            }
            "underline" => {
                out.underline = true;
                any_line = true;
            }
            "overline" => {
                out.overline = true;
                any_line = true;
            }
            "line-through" => {
                out.line_through = true;
                any_line = true;
            }
            "solid" | "wavy" | "dashed" | "dotted" | "double" | "blink" => {
                // Стиль — пока не реализован, токен поглощается, чтобы не
                // попасть в color-парсер.
            }
            "currentcolor" => {
                color_currentcolor = true;
            }
            _ => residue.push(token),
        }
    }
    if !residue.is_empty() {
        // Попробуем сначала весь residue (на случай color-функции с
        // пробелами: `rgb(0 0 0)` → токены `rgb(0`, `0`, `0)`).
        let joined = residue.join(" ");
        if let Some(c) = parse_color(joined.trim()) {
            color = Some(c);
        } else {
            // Иначе пробуем токен за токеном — для named-color / hex без
            // пробелов внутри.
            for tok in &residue {
                if let Some(c) = parse_color(tok) {
                    color = Some(c);
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
        if none_seen { Some(TextDecorationLine::default()) } else { Some(out) }
    } else {
        None
    };
    (line, color)
}

/// Применяет `font-size`-декларацию, если она задана. Размер `em` берётся
/// относительно `parent_fs` (родительский font-size), `rem` — относительно
/// ROOT_FONT_SIZE, `%` — относительно `parent_fs`.
fn apply_font_size(
    style: &mut ComputedStyle,
    decl: &Declaration,
    parent_fs: f32,
    viewport: Size,
) {
    if decl.property != "font-size" {
        return;
    }
    let val = decl.value.as_str();
    let Some(len) = parse_length(val) else {
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
fn resolve_box_length(val: &str, em_basis: f32, viewport: Size) -> Option<f32> {
    let len = parse_length(val)?;
    match len {
        Length::Percent(_) => None,
        other => other.resolve(em_basis, None, viewport),
    }
}

fn set_box_length(target: &mut f32, val: &str, em_basis: f32, viewport: Size) {
    if let Some(v) = resolve_box_length(val, em_basis, viewport) {
        *target = v;
    }
}

fn is_border_style_kw(s: &str) -> bool {
    matches!(s.trim(), "none" | "solid" | "dashed" | "dotted")
}

fn parse_border_style_kw(s: &str) -> BorderStyle {
    match s.trim() {
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        _ => BorderStyle::None,
    }
}

/// Разбирает `border: <width> <style> <color>` (порядок произвольный, каждая
/// часть опциональна). Применяет найденные значения ко всем четырём сторонам.
fn apply_border_shorthand(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size) {
    let tokens: Vec<&str> = val.split_whitespace().collect();
    for tok in &tokens {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport) {
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
        } else if let Some(c) = parse_color(tok) {
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
) {
    for tok in val.split_whitespace() {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport) {
            *width = v;
        } else if is_border_style_kw(tok) {
            *bstyle = parse_border_style_kw(tok);
        } else if let Some(c) = parse_color(tok) {
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
    }
}

enum ColorFn {
    Rgb,
    Hsl,
    Oklch,
    // Прочие CSS4 расширения (lab / lch / oklab / color()) — позже.
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
        let (line, color) = parse_text_decoration_shorthand("underline");
        let d = line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(color.is_none());
    }

    #[test]
    fn text_decoration_none_returns_empty() {
        let (line, _) = parse_text_decoration_shorthand("none");
        assert!(line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_multiple_keywords_combine() {
        let (line, _) = parse_text_decoration_shorthand("overline underline");
        let d = line.unwrap();
        assert!(d.underline);
        assert!(d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_line_through_with_hyphen() {
        let (line, _) = parse_text_decoration_shorthand("line-through");
        assert!(line.unwrap().line_through);
    }

    #[test]
    fn text_decoration_none_with_other_clears_all() {
        // `none` всегда побеждает: интуитивный сброс.
        let (line, _) = parse_text_decoration_shorthand("underline none");
        assert!(line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_blink_and_style_tokens_ignored_for_line() {
        // `blink` и `solid` — игнорируем для line; теперь `red` — color.
        let (line, color) = parse_text_decoration_shorthand("underline blink solid");
        let d = line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(color.is_none(), "no color token → None");
    }

    #[test]
    fn text_decoration_unrecognized_only_returns_none_line() {
        let (line, _) = parse_text_decoration_shorthand("blink");
        assert!(line.is_none());
        let (line, _) = parse_text_decoration_shorthand("");
        assert!(line.is_none());
    }

    #[test]
    fn text_decoration_is_case_insensitive() {
        let (line, _) = parse_text_decoration_shorthand("UNDERLINE Line-Through");
        let d = line.unwrap();
        assert!(d.underline);
        assert!(d.line_through);
    }

    // ── text-decoration-color ───────────────────────────────────────────────

    #[test]
    fn text_decoration_color_named_in_shorthand() {
        // `text-decoration: underline red` — линия + цвет.
        let (line, color) = parse_text_decoration_shorthand("underline red");
        assert!(line.unwrap().underline);
        assert_eq!(color, Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_color_hex_in_shorthand() {
        let (line, color) = parse_text_decoration_shorthand("overline #00ff00");
        assert!(line.unwrap().overline);
        assert_eq!(color, Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_color_rgb_function_in_shorthand() {
        // Color-функция с пробелами (modern CSS syntax) — токены должны
        // склеиваться обратно.
        let (line, color) = parse_text_decoration_shorthand("line-through rgb(0 0 255)");
        assert!(line.unwrap().line_through);
        assert_eq!(color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
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
}
