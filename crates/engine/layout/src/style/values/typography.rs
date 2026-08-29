//! Типы значений CSS для типографики и текста: `display`, выравнивание текста,
//! направление письма, курсор, видимость, пробелы/перенос, шрифтовые enum'ы
//! (`font-style`/`font-variant-*`/`font-stretch`/`font-weight`), декорации и
//! эмфазис текста, `forced-color-adjust`/`color-scheme`.
//!
//! Перенесено батчем SPLIT-ST16 из `crates/engine/layout/src/style.rs`
//! (анкер `enum Display` до конца `impl ColorScheme`) без правок тел.

use crate::style::computed::ComputedStyle;
use crate::style::values::color::Color;
use crate::style::TextWrapMode;

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
    /// CSS Display L3 — `display: flow-root`. Creates a BFC; treated as Block in layout.
    FlowRoot,
    /// CSS Display L3 — `display: contents`. Box itself generates no box;
    /// children participate in parent formatting context. Treated as Block (deferred).
    Contents,
    /// CSS 2.1 table display types — parsed/stored; table layout deferred.
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableColumnGroup,
    TableColumn,
    TableCell,
    TableCaption,
    /// CSS 2.1 — `display: list-item`. Generates principal block + marker box.
    ListItem,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// CSS `start`: left in LTR context, right in RTL context.
    /// This is the CSS-spec initial value (resolves at layout time via `direction`).
    #[default]
    Start,
    /// CSS `end`: right in LTR context, left in RTL context.
    End,
    Left,
    Center,
    Right,
}

/// CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`.
/// Выравнивание последней (или единственной) строки блока.
/// Phase 0: parse + store; применение при line layout — деferred.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

/// CSS Writing Modes L3 §2.1 — `direction: ltr | rtl`. Inherited.
///
/// Базовое направление потока inline-контента: задаёт paragraph embedding
/// level для Unicode Bidirectional Algorithm (`ltr` → 0, `rtl` → 1) и
/// разрешает логические значения `text-align: start|end` в физические
/// left/right. Реальный bidi-порядок фрагментов считает [`crate::bidi`],
/// применяет `box_tree::wrap_inline_run` → `align_lines`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// CSS Writing Modes L4 §2.2 — `unicode-bidi`. НЕ наследуется.
///
/// Управляет тем, как содержимое inline-бокса участвует в Unicode
/// Bidirectional Algorithm (UAX #9). Каждое значение эквивалентно обёртке
/// текста бокса в явные bidi-control-символы, которые [`crate::bidi`]
/// вставляет в текст параграфа перед прогоном UBA:
///
/// | Значение           | Обёртка (для `direction: ltr` / `rtl`)     |
/// |--------------------|--------------------------------------------|
/// | `normal`           | нет — содержимое сливается с окружением     |
/// | `embed`            | `LRE`/`RLE` … `PDF`                        |
/// | `isolate`          | `LRI`/`RLI` … `PDI`                        |
/// | `bidi-override`    | `LRO`/`RLO` … `PDF`                        |
/// | `isolate-override` | `FSI` `LRO`/`RLO` … `PDF` `PDI`            |
/// | `plaintext`        | `FSI` … `PDI` (направление — first-strong)  |
///
/// `plaintext` игнорирует `direction` бокса: базовое направление берётся
/// правилом P2/P3 из самого содержимого, что и делает `FSI`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnicodeBidi {
    /// Содержимое участвует в UBA наравне с соседями — bidi-control не вставляется.
    #[default]
    Normal,
    /// Дополнительный уровень вложенности (`LRE`/`RLE` … `PDF`).
    Embed,
    /// Изолированная последовательность (`LRI`/`RLI` … `PDI`).
    Isolate,
    /// Принудительное направление всех символов (`LRO`/`RLO` … `PDF`).
    BidiOverride,
    /// Изоляция + принудительное направление (`FSI` `LRO`/`RLO` … `PDF` `PDI`).
    IsolateOverride,
    /// Изоляция с first-strong базовым направлением (`FSI` … `PDI`).
    Plaintext,
}

/// Разбирает значение `unicode-bidi` (CSS Writing Modes L4 §2.2).
///
/// Ключевые слова CSS ASCII-case-insensitive (CSS Values L4 §2.4).
/// Legacy-префиксы `-webkit-`/`-moz-` у трёх изолирующих значений принимаются
/// как алиасы — так их до сих пор пишут в CSS локализованных страниц.
/// `None` — значение не распознано, объявление игнорируется.
pub(in crate::style) fn match_unicode_bidi(val: &str) -> Option<UnicodeBidi> {
    let v = val.trim().to_ascii_lowercase();
    let v = v.strip_prefix("-webkit-").or_else(|| v.strip_prefix("-moz-")).unwrap_or(&v);
    match v {
        "normal" => Some(UnicodeBidi::Normal),
        "embed" => Some(UnicodeBidi::Embed),
        "isolate" => Some(UnicodeBidi::Isolate),
        "bidi-override" => Some(UnicodeBidi::BidiOverride),
        "isolate-override" => Some(UnicodeBidi::IsolateOverride),
        "plaintext" => Some(UnicodeBidi::Plaintext),
        _ => None,
    }
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

/// CSS Text Module L3 §3.1 / L4 §2.1 — `white-space`. Inherited.
///
/// Управляет collapse-ом whitespace и переносами строк. В CSS Text L4 это
/// shorthand над `white-space-collapse` + `text-wrap-mode`; здесь хранится
/// «эффективное» комбинированное значение, которым пользуется layout, а
/// longhand-компоненты лежат в [`ComputedStyle::white_space_collapse`] и
/// `text_wrap_mode` и пересчитывают это поле через
/// [`WhiteSpace::combine`] при каждом применении.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    /// Preserves all whitespace including tabs and newlines; no line wrapping.
    Pre,
    /// Preserves whitespace; wraps at available width.
    PreWrap,
    /// Collapses spaces but preserves newlines; wraps at available width.
    PreLine,
    /// CSS Text L3 §3.1 `break-spaces` — like `pre-wrap`, but any sequence of
    /// preserved spaces takes up space and provides wrap opportunities.
    /// Phase 0: layout behaves as `pre-wrap` (trailing-space hang nuance
    /// deferred until the line-breaker distinguishes hanging spaces).
    BreakSpaces,
}

impl WhiteSpace {
    /// True when whitespace (tabs, newlines) is preserved rather than collapsed.
    pub fn preserves_whitespace(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces)
    }

    /// True when line wrapping is disabled (lines only break at forced breaks).
    pub fn is_nowrap(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::Nowrap)
    }

    /// True when segment breaks (`\n`) in the source are preserved as forced
    /// line breaks (CSS Text L4 §3.1: `preserve` / `preserve-breaks` /
    /// `break-spaces` collapse modes).
    pub fn preserves_newlines(self) -> bool {
        self.preserves_whitespace() || self == WhiteSpace::PreLine
    }

    /// CSS Text L4 §2.1 — recombine the two longhand components into the
    /// effective legacy value used by layout.
    ///
    /// `preserve-breaks + nowrap` and the `preserve-spaces` mode have no
    /// legacy equivalent; they map to the closest legacy value (`pre-line`
    /// and `pre-wrap`/`pre` respectively) — documented approximation.
    pub fn combine(collapse: WhiteSpaceCollapse, wrap: TextWrapMode) -> Self {
        let wraps = wrap == TextWrapMode::Wrap;
        match collapse {
            WhiteSpaceCollapse::Collapse => {
                if wraps { WhiteSpace::Normal } else { WhiteSpace::Nowrap }
            }
            WhiteSpaceCollapse::Preserve => {
                if wraps { WhiteSpace::PreWrap } else { WhiteSpace::Pre }
            }
            WhiteSpaceCollapse::PreserveBreaks => WhiteSpace::PreLine,
            WhiteSpaceCollapse::PreserveSpaces => {
                if wraps { WhiteSpace::PreWrap } else { WhiteSpace::Pre }
            }
            WhiteSpaceCollapse::BreakSpaces => {
                if wraps { WhiteSpace::BreakSpaces } else { WhiteSpace::Pre }
            }
        }
    }

    /// Decompose the legacy `white-space` value into its L4 collapse component
    /// (CSS Text L4 §2.1 shorthand expansion).
    pub fn collapse_component(self) -> WhiteSpaceCollapse {
        match self {
            WhiteSpace::Normal | WhiteSpace::Nowrap => WhiteSpaceCollapse::Collapse,
            WhiteSpace::Pre | WhiteSpace::PreWrap => WhiteSpaceCollapse::Preserve,
            WhiteSpace::PreLine => WhiteSpaceCollapse::PreserveBreaks,
            WhiteSpace::BreakSpaces => WhiteSpaceCollapse::BreakSpaces,
        }
    }

    /// Decompose the legacy `white-space` value into its L4 wrap component
    /// (CSS Text L4 §2.1 shorthand expansion).
    pub fn wrap_component(self) -> TextWrapMode {
        if self.is_nowrap() { TextWrapMode::Nowrap } else { TextWrapMode::Wrap }
    }
}

/// CSS Text Module L4 §3.1 — `white-space-collapse`. Inherited.
///
/// Longhand-компонента shorthand-а `white-space`, управляющая collapse-ом
/// пробелов и segment break-ов. Применение пересчитывает эффективное
/// [`ComputedStyle::white_space`] через [`WhiteSpace::combine`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    /// `collapse` (initial) — последовательности whitespace схлопываются.
    #[default]
    Collapse,
    /// `preserve` — пробелы и segment break-и сохраняются.
    Preserve,
    /// `preserve-breaks` — segment break-и сохраняются, пробелы схлопываются.
    PreserveBreaks,
    /// `preserve-spaces` — пробелы сохраняются, segment break-и и табы
    /// превращаются в пробелы. Phase 0: аппроксимируется как `preserve`.
    PreserveSpaces,
    /// `break-spaces` — как `preserve`, но preserved-пробелы занимают место
    /// и дают wrap opportunities.
    BreakSpaces,
}

impl WhiteSpaceCollapse {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "collapse" => Some(Self::Collapse),
            "preserve" => Some(Self::Preserve),
            "preserve-breaks" => Some(Self::PreserveBreaks),
            "preserve-spaces" => Some(Self::PreserveSpaces),
            "break-spaces" => Some(Self::BreakSpaces),
            _ => None,
        }
    }
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

/// CSS Fonts L4 §6.2 — `font-variant-caps`. Inherited.
///
/// Полный набор значений спецификации. `font-variant` — shorthand, из
/// которого сюда попадает только caps-компонента (остальные longhand-ы
/// — `-ligatures`, `-numeric`, `-east-asian`, `-position`, `-alternates`
/// — ещё не реализованы).
///
/// Рендеринг: пять значений синтезируются в layout-е (`caps_synthesis` в
/// `box_tree.rs` — заглавные буквы, уменьшенные до `SMALL_CAPS_SCALE`),
/// потому что bundled-шрифт (Inter) не содержит ни `smcp`, ни `c2sc`, ни
/// `pcap`. `TitlingCaps` синтезировать нечем — оно уходит в шейпер
/// OpenType-фичей `titl` (см. [`text_font_features`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontVariantCaps {
    /// `normal` (initial) — обычные глифы, никаких caps-подстановок.
    #[default]
    Normal,
    /// `small-caps` — строчные буквы показываются капителью (OpenType `smcp`).
    SmallCaps,
    /// `all-small-caps` — капителью показываются И строчные, И заглавные
    /// (OpenType `c2sc` + `smcp`).
    AllSmallCaps,
    /// `petite-caps` — как `small-caps`, но капитель ниже (OpenType `pcap`).
    /// Синтезируется идентично `small-caps` (Phase 0, как в Gecko).
    PetiteCaps,
    /// `all-petite-caps` — `c2pc` + `pcap`; синтезируется как `all-small-caps`.
    AllPetiteCaps,
    /// `unicase` — заглавные показываются капителью, строчные остаются
    /// строчными (OpenType `unic`).
    Unicase,
    /// `titling-caps` — заглавные заменяются на титульные формы (OpenType
    /// `titl`). Синтезу не поддаётся: без глифов шрифта это no-op.
    TitlingCaps,
}

impl FontVariantCaps {
    /// Разбирает keyword `font-variant-caps` (CSS Fonts L4 §6.2).
    /// `None` — токен не относится к caps-компоненте.
    pub fn from_keyword(kw: &str) -> Option<Self> {
        match kw {
            "normal" => Some(Self::Normal),
            "small-caps" => Some(Self::SmallCaps),
            "all-small-caps" => Some(Self::AllSmallCaps),
            "petite-caps" => Some(Self::PetiteCaps),
            "all-petite-caps" => Some(Self::AllPetiteCaps),
            "unicase" => Some(Self::Unicase),
            "titling-caps" => Some(Self::TitlingCaps),
            _ => None,
        }
    }

    /// OpenType-фичи, которые это значение включает в шейпере.
    ///
    /// Пусто для всех значений, кроме `titling-caps`: остальные
    /// синтезируются в layout-е (`caps_synthesis`), и включать вдобавок
    /// `smcp`/`c2sc` нельзя — по уже поднятому в верхний регистр тексту
    /// `c2sc` отработал бы второй раз и капитель уменьшилась бы дважды.
    pub fn feature_tags(self) -> &'static [[u8; 4]] {
        const TITL: [[u8; 4]; 1] = [*b"titl"];
        match self {
            Self::TitlingCaps => &TITL,
            _ => &[],
        }
    }

    /// CSS-сериализация значения (для `getComputedStyle` и layout-дампов).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::SmallCaps => "small-caps",
            Self::AllSmallCaps => "all-small-caps",
            Self::PetiteCaps => "petite-caps",
            Self::AllPetiteCaps => "all-petite-caps",
            Self::Unicase => "unicase",
            Self::TitlingCaps => "titling-caps",
        }
    }
}

/// CSS Fonts L4 §6.6 — `font-variant-emoji`.
///
/// Задаёт, какой вариант презентации выбирается для символа с эмодзи-формой:
/// текстовый (монохромный) или эмодзи (цветной), — не трогая сам символ.
/// Наследуется.
///
/// **Ограничение Lumen:** значение парсится, наследуется и публикуется в
/// `getComputedStyle`, но на выбор глифа пока не влияет — presentation
/// selection (variation selectors VS15/VS16, curated emoji-fallback в
/// `femtovg_backend`) свойство не читает. Реализовано ради
/// [CSS Color Adjust L1 §3.1](https://drafts.csswg.org/css-color-adjust-1/),
/// который требует форсировать вычисленное значение в forced-colors mode
/// (BUG-388).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontVariantEmoji {
    /// `normal` (initial) — презентацию выбирает UA по своим правилам.
    #[default]
    Normal,
    /// `text` — текстовая (монохромная) презентация.
    Text,
    /// `emoji` — эмодзи-презентация (цветная).
    Emoji,
    /// `unicode` — презентация строго по правилам Unicode (только явные
    /// variation selectors в тексте).
    Unicode,
}

impl FontVariantEmoji {
    /// Разбирает keyword `font-variant-emoji`. `None` — не наш токен.
    pub fn from_keyword(kw: &str) -> Option<Self> {
        match kw {
            "normal" => Some(Self::Normal),
            "text" => Some(Self::Text),
            "emoji" => Some(Self::Emoji),
            "unicode" => Some(Self::Unicode),
            _ => None,
        }
    }

    /// CSS-сериализация значения (для `getComputedStyle` и layout-дампов).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Text => "text",
            Self::Emoji => "emoji",
            Self::Unicode => "unicode",
        }
    }
}

/// Собирает набор OpenType-фич для `DrawText.font_features`.
///
/// CSS Fonts L4 §6.4 (Font Feature Resolution) задаёт порядок: сперва фичи
/// от `font-variant-*`, последними — `font-feature-settings`. Шейпер
/// (`otlayout::apply_feature_overrides`) применяет пары слева направо, так
/// что более поздняя запись перекрывает раннюю — то есть автор может
/// выключить фичу капители через `font-feature-settings`.
pub fn text_font_features(style: &ComputedStyle) -> Vec<([u8; 4], u32)> {
    let caps = style.font_variant_caps.feature_tags();
    let mut out = Vec::with_capacity(caps.len() + style.font_feature_settings.len());
    out.extend(caps.iter().map(|tag| (*tag, 1)));
    out.extend(style.font_feature_settings.iter().map(|f| (f.tag, f.value)));
    out
}

/// CSS Fonts L4 §7.12 — `font-optical-sizing`. Inherited.
///
/// `auto` (initial): UA automatically sets the `opsz` variation axis equal to
/// the computed `font-size` in px. `none`: opsz axis is not touched by the UA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontOpticalSizing {
    /// UA injects `opsz = font_size` into variation axes automatically.
    #[default]
    Auto,
    /// No automatic optical sizing; `font-variation-settings` controls opsz directly.
    None,
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
/// Значение доезжает до рендера двумя независимыми путями, которые
/// складываются: variable-шрифты получают ось `wdth`
/// (`DrawText::font_variation_axes`), а статические семейства с отдельными
/// condensed/expanded-файлами подбираются matcher-ом по `usWidthClass` из
/// OS/2 (`DrawText::font_stretch` → `FontProvider::pick_face`, CSS Fonts L4
/// §5.2). `text_rendering_eq` учитывает stretch, чтобы фрагменты с разным
/// stretch не сливались.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontStretch(pub u16);

impl FontStretch {
    /// 100% — нормальная ширина.
    pub const NORMAL: Self = Self(1000);

    /// Значение в CSS-процентах, округлённое до целого (50..200) — единицы
    /// [`lumen_core::FaceRecord::stretch`] и `usWidthClass`. Дробные
    /// keyword-ы (`semi-expanded` = 112.5%) округляются: шкала
    /// `usWidthClass` целочисленная и дробных ступеней не имеет.
    pub fn as_percent(self) -> u16 {
        (self.0 + 5) / 10
    }

    /// `<font-stretch-css3>`: keyword или `<percentage>` (CSS Fonts L4 §2.5).
    /// Берёт первый токен — L4 допускает диапазон из двух значений (это
    /// синтаксис дескриптора `@font-face`, для свойства второе значение
    /// игнорируется). `None` — значение не распознано.
    pub fn parse(val: &str) -> Option<Self> {
        let token = val.split_whitespace().next()?;
        if let Some(fs) = Self::from_keyword(token) {
            return Some(fs);
        }
        let pct = token.strip_suffix('%')?;
        let n = pct.trim().parse::<f32>().ok()?;
        // CSS Fonts L4 §2.5: percentage >= 0%. Out-of-range значения
        // формально валидны, но бесполезны для рендеринга и могут
        // переполнить u16 (max ≈ 6553%). Клампим в привычные [50%, 200%].
        let clamped = n.clamp(50.0, 200.0);
        Some(Self((clamped * 10.0).round() as u16))
    }

    pub(in crate::style) fn from_keyword(kw: &str) -> Option<Self> {
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

/// CSS Fonts L4 §7 — одна запись `font-variation-settings`.
///
/// `tag` — четырёхбайтный OpenType axis tag (например `b"wght"`, `b"wdth"`).
/// `value` — user-space значение из CSS (до нормализации fvar/avar).
/// Нормализация выполняется в renderer-е, который имеет доступ к таблицам
/// шрифта. `normal` → пустой Vec; renderer применяет default-instance.
#[derive(Debug, Clone, PartialEq)]
pub struct FontVariationSetting {
    pub tag: [u8; 4],
    pub value: f32,
}

/// CSS Fonts L3 §6 — одна запись `font-feature-settings`.
///
/// `tag` — четырёхбайтный OpenType feature tag (например `b"liga"`,
/// `b"smcp"`). `value` — целое значение фичи: `0` = выключена, `1`
/// (или `on`, или опущено) = включена, >1 = выбор альтернативы
/// (например `"salt" 2`). `normal` → пустой Vec; шейпер применяет свой
/// default-набор фич (`liga`/`clig`/`calt`/`rlig`/`ccmp` + `kern`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFeatureSetting {
    /// Четырёхбайтный OpenType feature tag (ASCII U+20–U+7E).
    pub tag: [u8; 4],
    /// Значение фичи: 0 = off, 1 = on, >1 = номер альтернативы.
    pub value: u32,
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

/// CSS Text Decoration L4 §3.5 — `text-decoration-skip-ink`. Controls whether
/// underlines and overlines skip over glyph ink (descenders).
///
/// Spec inherited: yes. Initial: `Auto`.
///
/// - `Auto` — UA may skip underlines/overlines where they cross glyph ink.
///   Only characters with known ink below baseline (g, j, p, q, y, Q, J)
///   receive gaps. Applies to underlines; overlines are unaffected (they sit
///   above the cap height in normal text).
/// - `All` — UA must skip over all glyphs, including those wholly above/below
///   the decoration line (more aggressive than Auto).
/// - `None` — Never skip; decoration is always a continuous line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecorationSkipInk {
    /// Skip where decoration crosses glyph descenders (default).
    #[default]
    Auto,
    /// Skip over all glyphs, even those above/below the line.
    All,
    /// Never skip; draw a continuous line.
    None,
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

/// CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`.
/// Управляет вертикальным положением underline относительно baseline.
/// Inherited. Initial: `Auto`.
/// Phase 0: parse + store; real offset calculation при underline paint — P2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextUnderlinePosition {
    /// UA выбирает оптимальное положение (обычно под baseline).
    #[default]
    Auto,
    /// Underline выровнен по шрифтовым метрикам (underline-position из OS/2).
    FromFont,
    /// Underline рисуется строго под текстом (под всеми нижними выносными
    /// символов, alphabetic baseline).
    Under,
    /// Для vertical writing-mode: underline рисуется с левой стороны.
    Left,
    /// Для vertical writing-mode: underline рисуется с правой стороны.
    Right,
}

/// CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`.
/// Позволяет автору отказаться от принудительной цветовой настройки UA (Forced Colors Mode).
/// Phase 0: parse + store; применение при принудительных цветах — P2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForcedColorAdjust {
    /// `auto` — UA может применять принудительные цвета.
    #[default]
    Auto,
    /// `none` — элемент сохраняет авторские цвета.
    None,
    /// `preserve-parent-color` — унаследовать у родителя.
    PreserveParentColor,
}

/// CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`.
/// Подсказывает UA, какую цветовую тему поддерживает элемент.
/// Используется через [`ColorScheme::used_dark`] для определения «used
/// color scheme» (§2.3) и через [`system_color`] для резолва системных
/// цветовых ключевых слов (`Canvas`, `ButtonFace` и т.д.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// `normal` — элемент не заявляет предпочтений; UA выбирает самостоятельно.
    #[default]
    Normal,
    /// `light` — элемент поддерживает светлую тему.
    Light,
    /// `dark` — элемент поддерживает тёмную тему.
    Dark,
    /// `light dark` — оба; предпочтение light.
    LightDark,
    /// `dark light` — оба; предпочтение dark.
    DarkLight,
    /// `only light` — только светлая тема, без авто-инверсии UA.
    OnlyLight,
    /// `only dark` — только тёмная тема.
    OnlyDark,
}

impl ColorScheme {
    /// CSS Color Adjustment L1 §2.3 — резолвит «used color scheme» элемента
    /// в булев флаг «тёмная тема».
    ///
    /// `prefer_dark` — предпочтение пользователя / ОС (`@media
    /// (prefers-color-scheme: dark)`, в shell — `Lumen.dark_mode`).
    ///
    /// Алгоритм:
    /// - `light` / `only light` → всегда светлая (форсирует тему, игнорируя ОС);
    /// - `dark` / `only dark` → всегда тёмная;
    /// - `normal` / `light dark` / `dark light` → следуют предпочтению ОС.
    ///   `normal` рендерится в дефолтной теме UA, которая у Lumen совпадает
    ///   с предпочтением ОС (страница без `color-scheme` темнеет в dark-mode).
    ///
    /// Возвращает `true`, если элемент должен рендериться в тёмной теме.
    #[must_use]
    pub fn used_dark(self, prefer_dark: bool) -> bool {
        match self {
            ColorScheme::Light | ColorScheme::OnlyLight => false,
            ColorScheme::Dark | ColorScheme::OnlyDark => true,
            ColorScheme::Normal | ColorScheme::LightDark | ColorScheme::DarkLight => prefer_dark,
        }
    }
}

