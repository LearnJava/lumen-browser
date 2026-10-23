//! Селекторы CSS: типы, специфичность, валидация и разбор selector-list.
//!
//! Вырезано из `parser.rs` (SPLIT-CP1 срез 2/2) без изменения поведения.

// Долг по документации: код перенесён из `parser.rs` как есть; файл
// написан до включения `missing_docs`. Счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::cmp::Ordering;

mod pseudo;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Type(String),
    Class(String),
    Id(String),
    Universal,
    Attribute(AttrSelector),
    PseudoClass(PseudoClass),
    /// `::before`, `::after`, `::slotted()` и т.д.
    PseudoElement(PseudoElementKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrSelector {
    pub name: String,
    pub op: Option<AttrOp>,
    pub value: Option<String>,
    /// Модификатор `i` из CSS Selectors L4 §6.3.6 — ASCII case-insensitive
    /// сравнение значения. `s` явно ставит false (как default). Применим только
    /// при `op = Some(_)`; без оператора (`[attr]`) флаг игнорируется парсером.
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `=` — точное совпадение.
    Equals,
    /// `~=` — значение содержит whitespace-разделённое слово.
    Includes,
    /// `|=` — точное совпадение или префикс с `-` (для `lang="ru-RU"`).
    DashMatch,
    /// `^=` — префикс.
    Prefix,
    /// `$=` — суффикс.
    Suffix,
    /// `*=` — подстрока.
    Substring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoClass {
    FirstChild,
    LastChild,
    OnlyChild,
    Empty,
    Root,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    /// `:nth-child(an+b [of <selector-list>])` — индекс среди всех
    /// element-sibling-ов (1-based). Опциональный `of <selector-list>`
    /// clause (CSS Selectors L4 §6.6.5.1) фильтрует sibling-pool перед
    /// нумерацией: только элементы, матчащие хотя бы один из селекторов,
    /// участвуют в подсчёте.
    NthChild(NthSpec, Option<Vec<ComplexSelector>>),
    /// `:nth-last-child(an+b [of <selector-list>])` — то же с конца.
    NthLastChild(NthSpec, Option<Vec<ComplexSelector>>),
    /// `:nth-of-type(an+b)` — индекс среди sibling-ов того же тега.
    NthOfType(NthSpec),
    /// `:nth-last-of-type(an+b)` — индекс с конца среди sibling-ов того же тега.
    NthLastOfType(NthSpec),
    /// `:not(selector-list)` — CSS Selectors L4 §5.4: отрицание selector-
    /// list-а. Внутри допустимы complex-селекторы (с combinator-ами) и
    /// nested `:not`. Specificity = максимум по списку (как у `:is`).
    /// Матчит элемент, если ни один из селекторов списка ему не подходит.
    Not(Vec<ComplexSelector>),
    /// `:is(s1, s2, …)` — матчит, если матчит хоть один из селекторов.
    /// CSS4 Selectors §17. Specificity вычисляется как максимум по списку
    /// (наследуется в родителя), независимо от того, какой именно матчит.
    Is(Vec<ComplexSelector>),
    /// `:where(s1, s2, …)` — то же, что `:is`, но specificity = 0 (всегда).
    /// Полезно для default-стилей, которые легко перебить любым правилом.
    Where(Vec<ComplexSelector>),
    /// `:has(rs1, rs2, …)` — relational pseudo-class (CSS Selectors L4
    /// §17.2). Матчит элемент E, в поддереве/sibling-цепочке которого есть
    /// элемент, удовлетворяющий хоть одному из relative-селекторов. Каждый
    /// `RelativeSelector` опционально начинается с combinator-а; если
    /// combinator опущен — implicit descendant. Specificity contributes
    /// максимум по списку (как :is).
    Has(Vec<RelativeSelector>),
    /// `:placeholder-shown` (CSS Selectors L4 §15.1) — матчит form-control
    /// (`<input>` / `<textarea>`) с непустым `placeholder`-атрибутом, пока
    /// пользователь не ввёл значение. В Phase 0 без form-state runtime
    /// «не ввёл значение» сводится к «нет `value`-атрибута либо он пустой»
    /// — matcher делает соответствующую проверку на DOM.
    PlaceholderShown,
    /// `:required` (CSS Selectors L4 §15.4, HTML5 §4.10.3) — form control с
    /// атрибутом `required`. Применимо к `<input>`, `<textarea>`, `<select>`;
    /// для `<input>` исключаются типы, где required не имеет смысла (`hidden`,
    /// `range`, `color`, `submit`, `image`, `reset`, `button`).
    Required,
    /// `:optional` (CSS Selectors L4 §15.4, HTML5 §4.10.3) — form control,
    /// который может быть `required`, но без атрибута `required`. Дополняет
    /// `:required`, не пересекается с ним по множеству элементов.
    Optional,
    /// `:read-only` (CSS Selectors L4 §15.5, HTML5 §4.16.4) — элемент, чьё
    /// содержимое не редактируется пользователем. Применимо к `<input>` с
    /// атрибутом `readonly` или `disabled` (исключая non-editable input
    /// types), `<textarea>` с `readonly`/`disabled`, прочим элементам без
    /// `contenteditable`.
    ReadOnly,
    /// `:read-write` (CSS Selectors L4 §15.5, HTML5 §4.16.4) — элемент,
    /// редактируемый пользователем. Применимо к `<input>` / `<textarea>` без
    /// `readonly`/`disabled` (для input — текстовые types), и к элементам
    /// с `contenteditable="true"`.
    ReadWrite,
    /// `:disabled` (CSS Selectors L4 §14.2, HTML5 §4.10.19.2) — form control,
    /// у которого атрибут `disabled` либо находится внутри disabled-`<fieldset>`
    /// (вне `<legend>` первого ребёнка). Применимо к `<button>`, `<input>`,
    /// `<select>`, `<textarea>`, `<option>`, `<optgroup>`, `<fieldset>`.
    Disabled,
    /// `:enabled` (CSS Selectors L4 §14.2, HTML5 §4.10.19.2) — form control,
    /// который может быть disabled, но не disabled сейчас. Дополняет
    /// `:disabled`, не пересекается с ним.
    Enabled,
    /// `:checked` (CSS Selectors L4 §10.1, HTML5 §4.16.3) — checkbox/radio с
    /// атрибутом `checked` либо `<option>` с атрибутом `selected`. В Phase 0
    /// без runtime form-state — pure attribute-based matching: пользовательская
    /// «отметка» checkbox через клик не отражается в DOM-атрибутах и не
    /// учитывается. Этого достаточно для author CSS «default-checked» стилей.
    Checked,
    /// `:indeterminate` (CSS Selectors L4 §10.2, HTML5 §4.16.3) — checkbox
    /// в indeterminate-состоянии (выставляется только через JS `.indeterminate
    /// = true` — не выражено в DOM, в Phase 0 всегда `false` для checkbox);
    /// radio в группе с одинаковым `name` без single `checked`-радио; элемент
    /// `<progress>` без атрибута `value`. Для radio matcher обходит siblings
    /// по форме / документу, проверяя что нет checked-собрата.
    Indeterminate,
    /// `:default` (CSS Selectors L4 §10.4, HTML5 §4.16.3) — «по-умолчанию
    /// активный» form control: `<option selected>` внутри `<select>`,
    /// checkbox/radio с атрибутом `checked`, default-submit-button формы
    /// (первая `<button type=submit>` / `<input type=submit|image>` в DOM-
    /// порядке формы). В Phase 0 — pure attribute-based + simple form-default-
    /// button heuristic без runtime state.
    Default,
    /// `:lang(<language-tag>#)` (CSS Selectors L4 §11). Comma-list BCP 47
    /// language tags. Элемент матчит, если его content-language (через
    /// `lang`/`xml:lang` атрибут с наследованием от ancestor-ов) matches
    /// хотя бы один из tag-ов в списке по правилам RFC 4647 §3.3.1
    /// "basic filtering" — prefix-match с границей по `-` или концу строки.
    ///
    /// Tag-и нормализованы к ASCII lowercase при парсинге (BCP 47 спека
    /// делает language tags case-insensitive). Пустой список → парсер
    /// fallback-ит на `Unsupported(name)`.
    Lang(Vec<String>),
    /// `:link` (CSS Selectors L4 §6.2.2) — unvisited hyperlink. HTML
    /// hyperlinks: `<a>` / `<area>` / `<link>` элементы с `href`-атрибутом.
    /// В Phase 0 без history-runtime все ссылки трактуются как unvisited
    /// (нет visited-state). Эквивалентен `:any-link` для author-CSS.
    Link,
    /// `:visited` (CSS Selectors L4 §6.2.3) — посещённый hyperlink. В Phase 0
    /// без history-runtime всегда `false`. Реальная реализация требует
    /// safe-history-API с privacy-restrictions (CSS Privacy and Security §6)
    /// — отдельная задача с интеграцией к `lumen-storage::History`.
    Visited,
    /// `:any-link` (CSS Selectors L4 §6.2.1) — любая ссылка независимо от
    /// visited-state, эквивалент `:is(:link, :visited)`. Pure DOM-based:
    /// `<a>` / `<area>` / `<link>` с `href`-атрибутом.
    AnyLink,
    /// `:in-range` (CSS Selectors L4 §14.5, HTML5 §4.10.21.4) — `<input>` с
    /// range-валидацией (`type=number|range`), чьё текущее значение лежит в
    /// `[min, max]`. Phase 0: «текущее значение» = `value`-атрибут.
    InRange,
    /// `:out-of-range` (CSS Selectors L4 §14.5) — input с range-валидацией,
    /// чьё значение вне `[min, max]`. Дополняет `:in-range`. Элементы без
    /// range-limitations не матчат ни одну из двух pseudo.
    OutOfRange,
    /// `:dir(ltr|rtl)` (CSS Selectors L4 §13.2). Single keyword argument
    /// (`ltr` или `rtl`, ASCII case-insensitive). Матчит элемент с
    /// соответствующей directionality, определяемой через `dir`-атрибут
    /// самого элемента или ближайшего ancestor-а (HTML5 §3.2.6.1).
    /// При отсутствии `dir` — default `ltr`. `dir="auto"` в Phase 0
    /// трактуется как `ltr` (real auto-direction по UAX #9 first-strong
    /// отложен до bidi-движка).
    Dir(DirArg),
    /// `:dir(<ident>)` с идентификатором, отличным от `ltr`/`rtl`: по Selectors
    /// L4 §13.2 синтаксически валиден, но не матчит ни один элемент. Хранит
    /// идентификатор как написан — для сериализации.
    DirOther(String),
    /// `:state(<custom-ident>)` (CSS Selectors L4 §17.4 / WHATWG HTML §4.13.2
    /// `CustomStateSet`). Матчит custom-element, у которого
    /// `ElementInternals.states` содержит данный state-ident. Custom-ident —
    /// case-sensitive (в отличие от `:lang()`), single identifier without
    /// comma-list. Невалидный/пустой аргумент → `Unsupported(name)`.
    State(String),
    /// `:scope` (CSS Selectors L4 §4.2) — root of selector matching context.
    /// В author-CSS-stylesheet без runtime querySelector/matches API scope =
    /// document root element. Spec: «In all other contexts, :scope matches
    /// the document's root element, exactly like :root.» Реальная разница с
    /// `:root` появится при integration с DOM querySelector API (P3 +
    /// JS-runtime), где scope = the element on which the selector matching
    /// is rooted (e.g. el.querySelector(':scope > .x') ищет относительно el).
    Scope,
    /// `:target` (CSS Selectors L4 §9.6, HTML LS §7.10.6 «the indicated part
    /// of the document»). Матчит element, чей `id`-атрибут равен текущему
    /// URL fragment-у документа. Comparison case-sensitive (HTML id
    /// case-sensitive per HTML LS §3.2.6). Если в URL нет fragment-а —
    /// никакой element не матчит.
    ///
    /// Phase 0: matcher читает `Document::target()`. Shell-интеграция
    /// (выставление target_id из URL fragment при загрузке) — отдельная
    /// P3-задача; до её появления `:target` молча возвращает `false` для
    /// всех элементов (privacy-safe default — стилизация не утекает через
    /// URL).
    Target,
    /// `:target-within` (CSS Selectors L4 §9.7). Матчит element, который сам
    /// удовлетворяет `:target`, либо у которого в поддереве (любой descendant)
    /// есть element, удовлетворяющий `:target`. Используется чтобы стилизовать
    /// «контейнер с активным фрагментом», например подсвечивать `<section>`
    /// под текущим якорем.
    ///
    /// Эквивалентно `:has(:target), :target`. Отдельный matcher (а не
    /// expansion в `:has`-form) — для прямолинейности и чтобы не зависеть от
    /// relational pseudo при простом sub-tree обходе. Phase 0 ограничение —
    /// то же, что у `:target`: без shell-интеграции `Document::target()`
    /// возвращает `None`, и matcher молча даёт `false`.
    TargetWithin,
    /// `:defined` (CSS Selectors L4 §6.4.1, HTML LS §4.13.5) — матчит элементы,
    /// которые определены: все built-in HTML / SVG / MathML элементы, а также
    /// зарегистрированные custom elements. Не-`:defined` — custom-element-имя,
    /// которое ещё не передано в `CustomElementRegistry.define()`.
    ///
    /// По HTML LS §4.13.2 имя custom-element-а обязано содержать ASCII `-`
    /// (например, `<my-button>`) — это отличает их от built-in. В Phase 0 без
    /// custom-elements registry matcher использует это правило как
    /// аппроксимацию: local name без `-` → defined (built-in); local name с
    /// `-` → undefined (registry пуст). Когда P3 поднимет registry,
    /// проверка станет: `built-in || registry.has(name)`.
    Defined,
    /// `:fullscreen` (Fullscreen API spec §4.2 «:fullscreen pseudo-class») —
    /// матчит элемент, который в данный момент находится в fullscreen-режиме
    /// (был поднят через `Element.requestFullscreen()`), а также его
    /// ancestor-ы по top-layer-цепочке. Phase 0 без Fullscreen API runtime —
    /// всегда `false`. Реальная реализация требует top-layer state в shell-е
    /// и JS bindings (P3).
    Fullscreen,
    /// `:modal` (CSS Selectors L4 §16.5.2) — матчит элемент в modal state.
    /// В HTML LS это `<dialog>`, открытый через `dialog.showModal()` (но
    /// **не** `dialog.show()` — non-modal); также элемент в Fullscreen
    /// API top-layer. Phase 0 без dialog/fullscreen runtime — всегда
    /// `false` (атрибут `open` сам по себе не делает dialog modal, потому
    /// нельзя имитировать через pure DOM-check).
    Modal,
    /// `:popover-open` (HTML LS §6.12.2 «Popover API») — матчит элемент
    /// с `popover`-атрибутом в открытом состоянии (после
    /// `element.showPopover()` или клика по `popovertarget`-кнопке).
    /// Phase 0 без Popover API runtime — всегда `false`: атрибут `popover`
    /// определяет, что элемент **может быть** popover-ом, но открытое
    /// состояние — runtime-only.
    PopoverOpen,
    /// `:interest-source` (Interest Invokers, WHATWG HTML PR #11006) —
    /// `interestfor`-инвокер, у которого сейчас показан интерес. Runtime-only:
    /// JS-шим ставит `data-lumen-interest-source`.
    InterestSource,
    /// `:interest-target` — цель показанного интереса
    /// (`data-lumen-interest-target`).
    InterestTarget,
    /// `:current` (CSS Selectors L4 §11.4.1) — element, представляющий
    /// текущий «момент» в timed-text потоке (например, активный WebVTT cue
    /// при видео-воспроизведении). Phase 0 без timed-text runtime — всегда
    /// `false`. Реальная реализация требует синхронизации с media timeline
    /// и WebVTT cue lifecycle (P3, Phase 3+).
    Current,
    /// `:past` (CSS Selectors L4 §11.4.2) — element, представляющий уже
    /// прошедший момент в timed-text потоке (предшествует `:current`).
    /// Phase 0 без timed-text runtime — всегда `false`.
    Past,
    /// `:future` (CSS Selectors L4 §11.4.3) — element, представляющий
    /// ещё-не-наступивший момент в timed-text потоке (следует за `:current`).
    /// Phase 0 без timed-text runtime — всегда `false`.
    Future,
    /// `:valid` (CSS Selectors L4 §14.1, HTML5 §4.10.21.3) — form control,
    /// чьё текущее значение удовлетворяет всем ограничениям (constraint
    /// validation). Phase 0: pure DOM/attribute-based: `valueMissing` (required +
    /// пустое значение), `typeMismatch` (email/url формат), `rangeOverflow/
    /// Underflow` (min/max на number/range). Без runtime JS не учитывается
    /// `setCustomValidity()`.
    Valid,
    /// `:invalid` (CSS Selectors L4 §14.1) — форм-контрол, нарушающий хотя бы
    /// одно ограничение. Дополняет `:valid`, не пересекается. Элементы, не
    /// являющиеся кандидатами для constraint validation, не матчат ни `:valid`,
    /// ни `:invalid`.
    Invalid,
    /// `:user-valid` (CSS Selectors L4 §14.3) — как `:valid`, но только после
    /// того, как пользователь взаимодействовал с полем. Phase 0 без интерактивного
    /// состояния — всегда `false`.
    UserValid,
    /// `:user-invalid` (CSS Selectors L4 §14.3) — как `:invalid`, но только
    /// после взаимодействия пользователя. Phase 0 — всегда `false`.
    UserInvalid,
    /// `:host` и `:host(selector-list)` (CSS Scoping L1 §6.1) — для shadow DOM.
    /// `:host` матчит shadow host element внутри shadow tree.
    /// `:host(s1, s2, ...)` матчит host если он матчит хотя бы один из селекторов.
    /// Specificity вычисляется как для `:is` — максимум по списку.
    /// None = простой `:host`; Some(list) = `:host(selector-list)`.
    Host(Option<Vec<ComplexSelector>>),
    /// `:hover` (CSS Selectors L4 §4.3) — элемент под указателем или с потомком
    /// под указателем. Состояние хранится thread-locally в `lumen-layout`
    /// через `set_interactive_state`; matcher проверяет, является ли тестируемый
    /// элемент предком или самим hovered-узлом (CSS Selectors L4 §4.3 «or one
    /// of its descendants»).
    Hover,
    /// `:focus` (CSS Selectors L4 §4.4) — элемент, у которого есть keyboard
    /// focus. Хранится thread-locally; matcher — точное совпадение с focus-узлом
    /// (в отличие от `:hover`, фокус не «наследуется» предками — для этого есть
    /// `:focus-within`).
    Focus,
    /// `:active` (CSS Selectors L4 §4.5) — элемент, активированный пользователем
    /// (кнопка мыши нажата и не отпущена). По спеке матчит элемент И его предков.
    Active,
    /// `:focus-within` (CSS Selectors L4 §4.4.2) — элемент или его потомок имеет
    /// keyboard focus. Matcher проверяет, является ли тестируемый элемент
    /// предком-или-собой focus-узла.
    FocusWithin,
    /// `:focus-visible` (CSS Selectors L4 §4.4.3) — как `:focus`, но только
    /// если индикатор фокуса должен быть виден по эвристике UA (обычно
    /// при навигации клавиатурой, не мышью). В Phase 0 синоним `:focus`.
    FocusVisible,
    /// `:heading` / `:heading(n)` (WHATWG «heading level» draft,
    /// [whatwg/html#11086](https://github.com/whatwg/html/pull/11086)).
    /// `:heading` (arg `None`) матчит любой `<h1>`–`<h6>`. `:heading(n)`
    /// матчит только заголовок, чей *эффективный уровень* равен `n`:
    /// базовый уровень тега (h1=1 … h6=6) плюс сумма `headingoffset`
    /// content-атрибутов по цепочке предок-или-сам-элемент, обнуляемая на
    /// ближайшем `headingreset` предке-или-себе (после чего собственный
    /// `headingoffset` того же узла всё равно прибавляется — «resetting
    /// applies after headingOffset»), где каждое отдельное значение
    /// `headingoffset` сперва клампится в `>= 0` (отрицательный offset не
    /// вычитает, а игнорируется), а итоговый уровень клампится в `[1, 9]`.
    /// Невалидный/отсутствующий `headingoffset` на узле = 0. Матчер —
    /// `lumen-layout` (`style/matching/forms.rs`), не здесь.
    Heading(Option<i32>),
    /// Неизвестные или ещё-не-реализованные псевдо-классы. Всегда `false`.
    /// Хранится имя для отладки и корректного подсчёта specificity (0-1-0).
    Unsupported(String),
}

/// Pseudo-element селекторы (CSS Pseudo-Elements L4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoElementKind {
    /// `::before` (CSS Pseudo-Elements L4 §5.1) — generates a box перед content element.
    /// В Phase 0 никогда не матчит (генерируемых DOM-узлов нет).
    Before,
    /// `::after` (CSS Pseudo-Elements L4 §5.2) — generates a box после content element.
    /// В Phase 0 никогда не матчит.
    After,
    /// `::first-line` (CSS Pseudo-Elements L4 §5.3) — form первой строки блока.
    /// Заполняется P1 в layout через InlineRun.is_first_line флаг.
    FirstLine,
    /// `::first-letter` (CSS Pseudo-Elements L4 §5.4) — первая letter первого текстового node-а.
    /// Заполняется P1 в layout через PseudoKind marker в segmentах.
    FirstLetter,
    /// `::slotted(selector-list)` (CSS Scoping L1 §6.2) — для shadow DOM.
    /// Матчит элемент, который слотирован через `<slot>` и матчит хотя бы один
    /// из селекторов списка. None = нет селектора (невалидно для ::slotted, но parser может вернуть).
    Slotted(Option<Vec<ComplexSelector>>),
    /// `::marker` (CSS Pseudo-Elements L4 §5.5) — маркер (bullet/number) list item.
    /// В Phase 0 парсится как обычное имя; P4 вводит как enum для будущей специализации.
    Marker,
    /// `::selection` (CSS Pseudo-Elements L4 §5.6) — selected text.
    /// В Phase 0 парсится как имя; P3 интеграция с DOM selection для highlight.
    Selection,
    /// `::target-text` (CSS Pseudo-Elements L4 §5.7 / WICG Scroll To Text
    /// Fragment §4) — text matched by a `:~:text=` URL fragment directive.
    /// Same restricted property subset as `::selection` (STTF-1 остаток).
    TargetText,
    /// `::spelling-error` (CSS Pseudo-Elements L4 §3.2) — text the UA flags as
    /// misspelled. Same restricted property subset as `::selection`.
    SpellingError,
    /// `::grammar-error` (CSS Pseudo-Elements L4 §3.2) — text the UA flags as
    /// grammatically wrong. Same restricted property subset as `::selection`.
    GrammarError,
    /// `::placeholder` (CSS Pseudo-Elements L4 §4.10) — placeholder hint text
    /// of a text-like `<input>`/`<textarea>` (matched while the field's `value`
    /// is empty). P4 wires: color/opacity/font-* overrides applied when the
    /// UA paints the `placeholder` attribute hint.
    Placeholder,
    /// `::highlight(name)` (CSS Highlight API L1 §3) — custom text highlight.
    /// Аргумент `name` — ключ в `CSS.highlights` реестре. Phase 0: парсирует имя,
    /// Phase 1: вызывает `emit_text_with_highlights()` для рендеринга.
    Highlight(String),
    /// `::picker(select)` (HTML/CSS «Customizable Select») — the pop-up picker
    /// part of a `<select>` rendered with `appearance: base-select`. The
    /// argument is the picker ident (currently only `select`); stored so future
    /// picker kinds stay distinguishable.
    Picker(String),
    /// `::checkmark` (HTML «Customizable Select») — the tick shown next to the
    /// currently-selected `<option>` inside a `base-select` picker.
    Checkmark,
    /// `::picker-icon` (HTML «Customizable Select») — the disclosure/arrow icon
    /// on the `base-select` trigger button.
    PickerIcon,
    /// `::scroll-marker` (CSS Overflow L5 §scroll-marker) — the navigational
    /// marker box CSS can generate for a scroll-snap-target element (BUG-505
    /// срез 6: selector-grammar recognition only — no box materializes yet,
    /// same Phase-0 shape `::before`/`::after` had before layout wired them).
    ScrollMarker,
    /// `::scroll-marker-group` (CSS Overflow L5 §scroll-marker-group) — the
    /// container box for a scroll container's `::scroll-marker` children.
    /// Not to be confused with the `scroll-marker-group` *property* (own
    /// `ComputedStyle` field, `style/apply/layout.rs`) that places this
    /// pseudo-element `before`/`after` its originating element.
    ScrollMarkerGroup,
    /// `::column` (CSS Multi-column L2 §column-pseudo) — one fragmentation column of a multicol
    /// container, a target for `::scroll-marker`. Grammar only: no `::column` box is generated yet,
    /// and `background`/`border` have no effect on it, so the cascade finds nothing to apply.
    Column,
    /// `::scroll-button(<direction>)` (CSS Overflow L5 §scroll-buttons) — a
    /// scroll container's directional scroll-by-one-page button. Argument is
    /// one of `up`/`down`/`left`/`right`/`block-start`/`inline-start`/
    /// `inline-end`/`block-end`/`*` (lower-cased); validity of the argument
    /// itself is checked by `pseudo_element_is_valid`, not here.
    ScrollButton(String),
    /// `::view-transition` (CSS View Transitions L1 §6.1) — the root of the
    /// view-transition pseudo-tree, originating from the document element.
    ViewTransition,
    /// `::view-transition-group(<name>)` (CSS View Transitions L1 §6.2) — the
    /// morphing group of the captured element named `<name>`. The argument is
    /// the `view-transition-name` it targets or `*` (every name); stored as
    /// written (names are case-sensitive `<custom-ident>`s).
    ViewTransitionGroup(String),
    /// `::view-transition-image-pair(<name>)` (CSS View Transitions L1 §6.3) —
    /// the isolating container holding the old and new snapshots of a group.
    ViewTransitionImagePair(String),
    /// `::view-transition-old(<name>)` (CSS View Transitions L1 §6.4) — the
    /// snapshot of the element as it was before the DOM update.
    ViewTransitionOld(String),
    /// `::view-transition-new(<name>)` (CSS View Transitions L1 §6.5) — the
    /// live representation of the element after the DOM update.
    ViewTransitionNew(String),
    /// Неизвестный pseudo-element (например, `::custom-pseudo` или typo).
    /// Хранится имя для диагностики.
    Unknown(String),
}

/// Аргумент `:dir(...)` pseudo-class (CSS Selectors L4 §13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirArg {
    Ltr,
    Rtl,
}

/// Один элемент relative-selector-list-а из `:has()`. `combinator` — если
/// `Some(c)`, проверяемые элементы выбираются относительно scope (E) через
/// `c`: Child → прямые дети E; NextSibling → следующий sibling; LaterSibling
/// → последующие siblings. Если `None`, implicit Descendant — любой
/// элемент в поддереве E.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeSelector {
    pub combinator: Option<Combinator>,
    pub selector: ComplexSelector,
}

/// Формула `an+b` из CSS Selectors §6.6.5.1. Элемент с 1-based индексом `i`
/// матчит, если существует целое `n >= 0` такое, что `i = a*n + b`.
///
/// Преобразование ключевых слов:
///   - `odd` → `2n+1`;
///   - `even` → `2n+0`;
///   - просто число `5` → `0n+5` (точное совпадение).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NthSpec {
    pub a: i32,
    pub b: i32,
}

impl NthSpec {
    pub const ODD: Self = Self { a: 2, b: 1 };
    pub const EVEN: Self = Self { a: 2, b: 0 };

    /// Возвращает true, если элемент с 1-based индексом `index` матчит формулу.
    pub fn matches(&self, index: i32) -> bool {
        if self.a == 0 {
            return index == self.b;
        }
        // Нужно: index = a*n + b, n >= 0 (целое).
        // Значит (index - b) делится на a, и (index - b) / a >= 0.
        let diff = index - self.b;
        if diff == 0 {
            return true; // n = 0
        }
        if diff % self.a != 0 {
            return false;
        }
        let n = diff / self.a;
        n >= 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    pub parts: Vec<SimpleSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// Пробел между compound-ами: `a b` — `b` потомок `a`.
    Descendant,
    /// `>` — прямой ребёнок.
    Child,
    /// `+` — следующий sibling.
    NextSibling,
    /// `~` — любой последующий sibling.
    LaterSibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    /// Левый compound. Например, в `a b > c`: head = `a`,
    /// tail = `[(Descendant, b), (Child, c)]`.
    pub head: CompoundSelector,
    pub tail: Vec<(Combinator, CompoundSelector)>,
}

impl ComplexSelector {
    /// Specificity по CSS Selectors Level 3 §16:
    /// - `a` — число `#id`-частей;
    /// - `b` — число классов, attribute-селекторов и pseudo-classes;
    /// - `c` — число type-селекторов и pseudo-elements.
    ///
    /// Universal `*` и combinator-ы не считаются.
    pub fn specificity(&self) -> Specificity {
        let mut spec = Specificity::default();
        accumulate_specificity(&self.head, &mut spec);
        for (_, comp) in &self.tail {
            accumulate_specificity(comp, &mut spec);
        }
        spec
    }

    /// CSS Conditional L4 §4.2 — распознаёт ли движок этот селектор целиком?
    ///
    /// Используется для `@supports selector(<complex-selector>)`: возвращает
    /// `true`, если каждый compound и каждая простая часть (включая аргументы
    /// функциональных псевдо-классов `:is()`/`:not()`/`:where()`/`:has()`/
    /// `:nth-child(... of ...)`/`:host()` и `::slotted()`) распознаётся. Любая
    /// `PseudoClass::Unsupported` или `PseudoElementKind::Unknown` → `false`.
    ///
    /// «Поддержан» означает *распознаётся синтаксически*, а не «матчит хотя бы
    /// один элемент» — псевдо вроде `:visited`/`:fullscreen`, которые в Phase 0
    /// всегда дают `false` при матчинге, всё равно считаются поддержанными.
    pub fn is_supported(&self) -> bool {
        compound_is_supported(&self.head)
            && self.tail.iter().all(|(_, c)| compound_is_supported(c))
    }

    /// Serialise this selector back to a CSS selector string.
    ///
    /// Best-effort round-trip for DevTools display (§PH3-1 Styles panel).
    /// Structurally equivalent to the original; whitespace may differ slightly.
    pub fn to_css_str(&self) -> String {
        let mut s = compound_to_css_str(&self.head);
        for (combinator, compound) in &self.tail {
            match combinator {
                Combinator::Descendant => s.push(' '),
                Combinator::Child => s.push_str(" > "),
                Combinator::NextSibling => s.push_str(" + "),
                Combinator::LaterSibling => s.push_str(" ~ "),
            }
            s.push_str(&compound_to_css_str(compound));
        }
        s
    }
}

/// True если все простые селекторы compound-а распознаются (см.
/// [`ComplexSelector::is_supported`]).
pub(crate) fn compound_is_supported(c: &CompoundSelector) -> bool {
    c.parts.iter().all(simple_is_supported)
}

/// True если простой селектор распознаётся движком. Type/Class/Id/Universal/
/// Attribute поддержаны безусловно; псевдо делегируют в свои проверки.
pub(crate) fn simple_is_supported(s: &SimpleSelector) -> bool {
    match s {
        SimpleSelector::Type(_)
        | SimpleSelector::Class(_)
        | SimpleSelector::Id(_)
        | SimpleSelector::Universal
        | SimpleSelector::Attribute(_) => true,
        SimpleSelector::PseudoClass(pc) => pseudo_class_is_supported(pc),
        SimpleSelector::PseudoElement(pe) => pseudo_element_is_supported(pe),
    }
}

/// True если псевдо-класс распознан. `Unsupported(_)` → `false`; функциональные
/// псевдо рекурсивно проверяют свои аргументы-селекторы.
pub(crate) fn pseudo_class_is_supported(pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::Unsupported(_) => false,
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
            list.iter().all(ComplexSelector::is_supported)
        }
        PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)) => {
            list.iter().all(ComplexSelector::is_supported)
        }
        PseudoClass::Has(rels) => rels.iter().all(|r| r.selector.is_supported()),
        PseudoClass::Host(Some(list)) => list.iter().all(ComplexSelector::is_supported),
        _ => true,
    }
}

/// Псевдоэлементы, определённые спецификациями, но не смоделированные
/// движком (отдельного варианта [`PseudoElementKind`] нет, лежат в `Unknown`).
/// Синтаксически валидны (`querySelectorAll('::backdrop')` — пустой список, не
/// `SyntaxError`) и ничего не выбирают. `part(…)` приходит уже с аргументом.
fn is_known_unmodeled_pseudo_element(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "backdrop"
            | "file-selector-button"
            | "details-content"
            | "search-text"
            // Совместимость (Compat Standard §3): `-webkit-`-псевдоэлементы
            // элементов управления, которые поддерживает и Chromium.
            | "-webkit-slider-thumb"
            | "-webkit-slider-runnable-track"
            | "-webkit-scrollbar"
            | "-webkit-scrollbar-button"
            | "-webkit-scrollbar-thumb"
            | "-webkit-scrollbar-track"
            | "-webkit-scrollbar-track-piece"
            | "-webkit-scrollbar-corner"
            | "-webkit-resizer"
            | "-webkit-inner-spin-button"
            | "-webkit-outer-spin-button"
            | "-webkit-search-cancel-button"
            | "-webkit-search-decoration"
            | "-webkit-search-results-button"
            | "-webkit-search-results-decoration"
            | "-webkit-progress-bar"
            | "-webkit-progress-value"
            | "-webkit-progress-inner-element"
            | "-webkit-meter-bar"
            | "-webkit-meter-optimum-value"
            | "-webkit-meter-suboptimum-value"
            | "-webkit-meter-even-less-good-value"
            | "-webkit-file-upload-button"
            | "-webkit-details-marker"
            | "-webkit-calendar-picker-indicator"
            | "-webkit-datetime-edit"
            | "-webkit-color-swatch"
            | "-webkit-color-swatch-wrapper"
    ) || lower.starts_with("part(")
}

/// True если псевдо-элемент распознан. `Unknown(_)` → `false`; `::slotted()`
/// рекурсивно проверяет свой аргумент-селектор.
pub(crate) fn pseudo_element_is_supported(pe: &PseudoElementKind) -> bool {
    match pe {
        PseudoElementKind::Unknown(name) => is_known_unmodeled_pseudo_element(name),
        PseudoElementKind::Slotted(Some(list)) => list.iter().all(ComplexSelector::is_supported),
        _ => true,
    }
}

pub(crate) fn compound_to_css_str(c: &CompoundSelector) -> String {
    c.parts.iter().map(simple_to_css_str).collect()
}

pub(crate) fn simple_to_css_str(s: &SimpleSelector) -> String {
    match s {
        SimpleSelector::Type(name) => name.clone(),
        SimpleSelector::Class(name) => format!(".{name}"),
        SimpleSelector::Id(name) => format!("#{name}"),
        SimpleSelector::Universal => "*".into(),
        SimpleSelector::Attribute(attr) => attr_to_css_str(attr),
        SimpleSelector::PseudoClass(pc) => pc_to_css_str(pc),
        SimpleSelector::PseudoElement(pe) => pe_to_css_str(pe),
    }
}

pub(crate) fn attr_to_css_str(attr: &AttrSelector) -> String {
    match (&attr.op, &attr.value) {
        (None, _) => format!("[{}]", attr.name),
        (Some(op), val) => {
            let op_str = match op {
                AttrOp::Equals => "=",
                AttrOp::Includes => "~=",
                AttrOp::DashMatch => "|=",
                AttrOp::Prefix => "^=",
                AttrOp::Suffix => "$=",
                AttrOp::Substring => "*=",
            };
            let v = val.as_deref().unwrap_or("");
            if attr.case_insensitive {
                format!("[{}{}\"{}\" i]", attr.name, op_str, v)
            } else {
                format!("[{}{}\"{}\"]", attr.name, op_str, v)
            }
        }
    }
}

pub(crate) fn nth_to_css_str(spec: &NthSpec) -> String {
    if spec.a == 0 {
        return spec.b.to_string();
    }
    if spec.b == 0 {
        return format!("{}n", spec.a);
    }
    if spec.b < 0 {
        format!("{}n{}", spec.a, spec.b)
    } else {
        format!("{}n+{}", spec.a, spec.b)
    }
}

pub(crate) fn sels_to_css_str(sels: &[ComplexSelector]) -> String {
    sels.iter().map(ComplexSelector::to_css_str).collect::<Vec<_>>().join(", ")
}

/// Serialise `:has()`'s relative-selector-list back to CSS text. Each item's
/// leading combinator (`>`/`+`/`~`) is printed only when explicit — implicit
/// descendant (`None`, or the redundant `Some(Combinator::Descendant)`) has
/// no token of its own, matching how `:has(img)` (not `:has( img)`) is written.
pub(crate) fn relative_sels_to_css_str(rels: &[RelativeSelector]) -> String {
    rels.iter()
        .map(|rs| {
            let prefix = match rs.combinator {
                Some(Combinator::Child) => "> ",
                Some(Combinator::NextSibling) => "+ ",
                Some(Combinator::LaterSibling) => "~ ",
                Some(Combinator::Descendant) | None => "",
            };
            format!("{prefix}{}", rs.selector.to_css_str())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn pc_to_css_str(pc: &PseudoClass) -> String {
    match pc {
        PseudoClass::FirstChild => ":first-child".into(),
        PseudoClass::LastChild => ":last-child".into(),
        PseudoClass::OnlyChild => ":only-child".into(),
        PseudoClass::Empty => ":empty".into(),
        PseudoClass::Root => ":root".into(),
        PseudoClass::FirstOfType => ":first-of-type".into(),
        PseudoClass::LastOfType => ":last-of-type".into(),
        PseudoClass::OnlyOfType => ":only-of-type".into(),
        PseudoClass::NthChild(spec, of) => match of {
            Some(list) => format!(":nth-child({} of {})", nth_to_css_str(spec), sels_to_css_str(list)),
            None => format!(":nth-child({})", nth_to_css_str(spec)),
        },
        PseudoClass::NthLastChild(spec, of) => match of {
            Some(list) => format!(":nth-last-child({} of {})", nth_to_css_str(spec), sels_to_css_str(list)),
            None => format!(":nth-last-child({})", nth_to_css_str(spec)),
        },
        PseudoClass::NthOfType(spec) => format!(":nth-of-type({})", nth_to_css_str(spec)),
        PseudoClass::NthLastOfType(spec) => format!(":nth-last-of-type({})", nth_to_css_str(spec)),
        PseudoClass::Not(sels) => format!(":not({})", sels_to_css_str(sels)),
        PseudoClass::Is(sels) => format!(":is({})", sels_to_css_str(sels)),
        PseudoClass::Where(sels) => format!(":where({})", sels_to_css_str(sels)),
        PseudoClass::Has(rels) => format!(":has({})", relative_sels_to_css_str(rels)),
        PseudoClass::PlaceholderShown => ":placeholder-shown".into(),
        PseudoClass::Required => ":required".into(),
        PseudoClass::Optional => ":optional".into(),
        PseudoClass::ReadOnly => ":read-only".into(),
        PseudoClass::ReadWrite => ":read-write".into(),
        PseudoClass::Disabled => ":disabled".into(),
        PseudoClass::Enabled => ":enabled".into(),
        PseudoClass::Checked => ":checked".into(),
        PseudoClass::Indeterminate => ":indeterminate".into(),
        PseudoClass::Default => ":default".into(),
        PseudoClass::Lang(tags) => format!(
            ":lang({})",
            tags.iter().map(|t| serialize_lang_range(t)).collect::<Vec<_>>().join(", ")
        ),
        PseudoClass::DirOther(kw) => format!(":dir({kw})"),
        PseudoClass::Link => ":link".into(),
        PseudoClass::Visited => ":visited".into(),
        PseudoClass::AnyLink => ":any-link".into(),
        PseudoClass::InRange => ":in-range".into(),
        PseudoClass::OutOfRange => ":out-of-range".into(),
        PseudoClass::Dir(DirArg::Ltr) => ":dir(ltr)".into(),
        PseudoClass::Dir(DirArg::Rtl) => ":dir(rtl)".into(),
        PseudoClass::State(name) => format!(":state({name})"),
        PseudoClass::Scope => ":scope".into(),
        PseudoClass::Target => ":target".into(),
        PseudoClass::TargetWithin => ":target-within".into(),
        PseudoClass::Defined => ":defined".into(),
        PseudoClass::Fullscreen => ":fullscreen".into(),
        PseudoClass::Modal => ":modal".into(),
        PseudoClass::PopoverOpen => ":popover-open".into(),
        PseudoClass::InterestSource => ":interest-source".into(),
        PseudoClass::InterestTarget => ":interest-target".into(),
        PseudoClass::Current => ":current".into(),
        PseudoClass::Past => ":past".into(),
        PseudoClass::Future => ":future".into(),
        PseudoClass::Valid => ":valid".into(),
        PseudoClass::Invalid => ":invalid".into(),
        PseudoClass::UserValid => ":user-valid".into(),
        PseudoClass::UserInvalid => ":user-invalid".into(),
        PseudoClass::Host(None) => ":host".into(),
        PseudoClass::Host(Some(sels)) => format!(":host({})", sels_to_css_str(sels)),
        PseudoClass::Hover => ":hover".into(),
        PseudoClass::Focus => ":focus".into(),
        PseudoClass::Active => ":active".into(),
        PseudoClass::FocusWithin => ":focus-within".into(),
        PseudoClass::FocusVisible => ":focus-visible".into(),
        PseudoClass::Heading(None) => ":heading".into(),
        PseudoClass::Heading(Some(n)) => format!(":heading({n})"),
        PseudoClass::Unsupported(name) => format!(":{name}"),
    }
}

pub(crate) fn pe_to_css_str(pe: &PseudoElementKind) -> String {
    match pe {
        PseudoElementKind::Before => "::before".into(),
        PseudoElementKind::After => "::after".into(),
        PseudoElementKind::FirstLine => "::first-line".into(),
        PseudoElementKind::FirstLetter => "::first-letter".into(),
        PseudoElementKind::Slotted(None) => "::slotted()".into(),
        PseudoElementKind::Slotted(Some(sels)) => {
            format!("::slotted({})", sels_to_css_str(sels))
        }
        PseudoElementKind::Marker => "::marker".into(),
        PseudoElementKind::Selection => "::selection".into(),
        PseudoElementKind::TargetText => "::target-text".into(),
        PseudoElementKind::SpellingError => "::spelling-error".into(),
        PseudoElementKind::GrammarError => "::grammar-error".into(),
        PseudoElementKind::Placeholder => "::placeholder".into(),
        PseudoElementKind::Highlight(name) => format!("::highlight({name})"),
        PseudoElementKind::Picker(name) => format!("::picker({name})"),
        PseudoElementKind::Checkmark => "::checkmark".into(),
        PseudoElementKind::PickerIcon => "::picker-icon".into(),
        PseudoElementKind::ScrollMarker => "::scroll-marker".into(),
        PseudoElementKind::ScrollMarkerGroup => "::scroll-marker-group".into(),
        PseudoElementKind::Column => "::column".into(),
        PseudoElementKind::ScrollButton(dir) => format!("::scroll-button({dir})"),
        PseudoElementKind::ViewTransition => "::view-transition".into(),
        PseudoElementKind::ViewTransitionGroup(n) => format!("::view-transition-group({n})"),
        PseudoElementKind::ViewTransitionImagePair(n) => format!("::view-transition-image-pair({n})"),
        PseudoElementKind::ViewTransitionOld(n) => format!("::view-transition-old({n})"),
        PseudoElementKind::ViewTransitionNew(n) => format!("::view-transition-new({n})"),
        PseudoElementKind::Unknown(name) => format!("::{name}"),
    }
}

/// Максимум specificity среди списка ComplexSelector-ов. Используется для
/// `:is(...)` (CSS4 §17): pseudo-class contributes specificity of the most
/// specific item in its argument list.
pub(crate) fn max_list_specificity(list: &[ComplexSelector]) -> Option<Specificity> {
    list.iter().map(ComplexSelector::specificity).max()
}

pub(crate) fn accumulate_specificity(comp: &CompoundSelector, spec: &mut Specificity) {
    for part in &comp.parts {
        match part {
            SimpleSelector::Id(_) => spec.a = spec.a.saturating_add(1),
            SimpleSelector::Class(_) | SimpleSelector::Attribute(_) => {
                spec.b = spec.b.saturating_add(1);
            }
            SimpleSelector::PseudoClass(pc) => {
                // `:not(...)` / `:is(...)` сами не считаются, contributes max
                // specificity по списку (CSS Selectors L4 §16, §17). `:where(...)`
                // — всегда 0.
                match pc {
                    PseudoClass::Not(list) | PseudoClass::Is(list) => {
                        if let Some(max) = max_list_specificity(list) {
                            spec.a = spec.a.saturating_add(max.a);
                            spec.b = spec.b.saturating_add(max.b);
                            spec.c = spec.c.saturating_add(max.c);
                        }
                    }
                    PseudoClass::Where(_) => {} // contributes 0
                    PseudoClass::Has(list) => {
                        // CSS Selectors L4 §17.2: то же что :is — максимум
                        // по содержимому. Берём specificity внутреннего
                        // ComplexSelector каждого RelativeSelector (без учёта
                        // ведущего combinator-а — он не имеет specificity).
                        let max = list
                            .iter()
                            .map(|rs| rs.selector.specificity())
                            .max();
                        if let Some(max) = max {
                            spec.a = spec.a.saturating_add(max.a);
                            spec.b = spec.b.saturating_add(max.b);
                            spec.c = spec.c.saturating_add(max.c);
                        }
                    }
                    PseudoClass::NthChild(_, of) | PseudoClass::NthLastChild(_, of) => {
                        // CSS Selectors L4 §17 «:nth-child(<an+b> of S)»:
                        // specificity = 1 pseudo-class + max-specificity of S
                        // (если S задан). Без `of` clause — только 1.
                        spec.b = spec.b.saturating_add(1);
                        if let Some(list) = of
                            && let Some(max) = max_list_specificity(list)
                        {
                            spec.a = spec.a.saturating_add(max.a);
                            spec.b = spec.b.saturating_add(max.b);
                            spec.c = spec.c.saturating_add(max.c);
                        }
                    }
                    PseudoClass::Host(opt_list) => {
                        // CSS Scoping L1 §6.1: `:host` и `:host(selector-list)`.
                        // Specificity = 1 pseudo-class + max-specificity of list
                        // (если :host(...) задан). Аналогично :is.
                        spec.b = spec.b.saturating_add(1);
                        if let Some(list) = opt_list
                            && let Some(max) = max_list_specificity(list)
                        {
                            spec.a = spec.a.saturating_add(max.a);
                            spec.b = spec.b.saturating_add(max.b);
                            spec.c = spec.c.saturating_add(max.c);
                        }
                    }
                    _ => spec.b = spec.b.saturating_add(1),
                }
            }
            SimpleSelector::Type(_) | SimpleSelector::PseudoElement(_) => {
                spec.c = spec.c.saturating_add(1);
            }
            SimpleSelector::Universal => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Specificity {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl Ord for Specificity {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.a, self.b, self.c).cmp(&(other.a, other.b, other.c))
    }
}

impl PartialOrd for Specificity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Язык-диапазон `:lang()` в сериализации: ident, если это допустимый ident
/// (латиница, цифры не в начале, `-`, `*`), иначе строка в двойных кавычках.
fn serialize_lang_range(range: &str) -> String {
    let plain = !range.is_empty()
        && !range.starts_with(|c: char| c.is_ascii_digit())
        && range.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '*'));
    if plain {
        range.to_string()
    } else {
        format!("\"{}\"", range.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Парсит строку CSS selector list (через запятую) и возвращает разобранные
/// `ComplexSelector`-ы. Невалидные или неизвестные части молча пропускаются.
/// Используется lumen-layout для selector-based lookup (find_box_by_selector).
pub fn parse_selector_list(input: &str) -> Vec<ComplexSelector> {
    Parser::new(&close_unclosed_blocks(input)).parse_selector_list()
}

/// CSS Syntax §5.4.7: незакрытый блок/функция/строка закрывается в конце
/// ввода, поэтому `span:not([class]` — валидный селектор. Дописывает к
/// `input` недостающие закрывающие `)`/`]`/кавычку; без незакрытых блоков
/// возвращает вход как есть. Применяется на DOM-границе
/// (`querySelector`/`matches`/`closest`), а не к преамбулам правил таблицы
/// стилей, где `{` уже ограничивает прелюдию.
pub(crate) fn close_unclosed_blocks(input: &str) -> std::borrow::Cow<'_, str> {
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '/') if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            (None, '(') => stack.push(')'),
            (None, '[') => stack.push(']'),
            (None, ')' | ']') => {
                stack.pop();
            }
            _ => {}
        }
    }
    if stack.is_empty() && quote.is_none() {
        return std::borrow::Cow::Borrowed(input);
    }
    let mut out = input.to_string();
    out.extend(quote);
    out.extend(stack.iter().rev());
    std::borrow::Cow::Owned(out)
}

/// Проверяет, валиден ли `input` как selector-list по правилам DOM LS для
/// `querySelector`/`querySelectorAll`/`matches`/`closest` (BUG-391).
///
/// [`parse_selector_list`] намеренно прощает всё: неизвестный псевдокласс
/// становится [`PseudoClass::Unsupported`], мусор в хвосте просто
/// отбрасывается. Для каскада это правильно (невалидное правило молча
/// выпадает из stylesheet-а), но DOM-методы обязаны бросать `SyntaxError`
/// DOMException, а не возвращать «ничего не нашлось» — иначе стандартный
/// feature-detection (`assert_throws_dom('SyntaxError', () =>
/// el.matches(':unknown-pseudo'))`) не отличает «не поддерживается» от
/// «не совпало».
///
/// Селектор считается невалидным, если:
/// * он не разбирается целиком (пустая строка, мусор в хвосте, висящая
///   запятая, незакрытая скобка);
/// * где-либо встречается неизвестный псевдокласс или pseudo-element;
/// * pseudo-element стоит не в последнем compound-е (`::before *`);
/// * после pseudo-element-а идёт что-то, кроме разрешённых спекой
///   user-action-псевдоклассов (`::before.cls`, `::marker::marker`);
/// * аргумент функционального pseudo-element-а вне допустимого множества
///   (`::picker(foo)`).
///
/// Аргументы `:is()`/`:where()` — forgiving-selector-list по CSS Selectors
/// L4 §3.2, поэтому их содержимое не валидируется; `:not()`/`:has()`
/// non-forgiving и проверяются рекурсивно.
pub fn is_valid_selector_list(input: &str) -> bool {
    let input = close_unclosed_blocks(input);
    let mut parser = Parser::new(&input);
    let Some(list) = parser.parse_selector_list_strict() else {
        return false;
    };
    parser.skip_ws_and_comments();
    if parser.peek().is_some() {
        return false;
    }
    list.iter().all(|c| complex_selector_is_valid(c, true))
}

/// Проверяет один complex-селектор. `allow_pseudo_element` = false для
/// вложенных списков (`:not()`, `:has()`, `::slotted()`), где спека
/// pseudo-element-ы запрещает.
pub(crate) fn complex_selector_is_valid(sel: &ComplexSelector, allow_pseudo_element: bool) -> bool {
    let last_idx = sel.tail.len();
    for (idx, compound) in std::iter::once(&sel.head)
        .chain(sel.tail.iter().map(|(_, c)| c))
        .enumerate()
    {
        let has_pe = compound
            .parts
            .iter()
            .any(|p| matches!(p, SimpleSelector::PseudoElement(_)));
        if has_pe && (!allow_pseudo_element || idx != last_idx) {
            // Pseudo-element допустим только в самом правом compound-е:
            // комбинатор после него запрещён (CSS Pseudo-Elements L4 §2.1).
            return false;
        }
        if !compound_selector_is_valid(compound) {
            return false;
        }
    }
    true
}

/// Проверяет один compound: порядок частей относительно pseudo-element-а и
/// валидность каждой простой части.
pub(crate) fn compound_selector_is_valid(compound: &CompoundSelector) -> bool {
    let mut seen_pe: Option<&PseudoElementKind> = None;
    for part in &compound.parts {
        match part {
            SimpleSelector::PseudoElement(pe) => {
                if let Some(prev) = seen_pe
                    && !pseudo_element_pair_allowed(prev, pe)
                {
                    return false;
                }
                if !pseudo_element_is_valid(pe) {
                    return false;
                }
                seen_pe = Some(pe);
            }
            SimpleSelector::PseudoClass(pc) => {
                if let Some(prev) = seen_pe {
                    let allowed = (pseudo_element_allows_user_action(prev)
                        && is_user_action_pseudo_class(pc))
                        // `::scroll-button()` is a focusable, potentially
                        // disabled control (CSS Overflow L5 §scroll-buttons)
                        // — `:disabled`/`:enabled` are valid trailing
                        // pseudo-classes for it specifically, confirmed by
                        // `scroll-buttons-valid.html`'s own matrix.
                        || (matches!(prev, PseudoElementKind::ScrollButton(_))
                            && matches!(pc, PseudoClass::Disabled | PseudoClass::Enabled));
                    if !allowed {
                        return false;
                    }
                }
                if !pseudo_class_is_valid(pc) {
                    return false;
                }
            }
            // Тип/класс/id/атрибут/`*` после pseudo-element-а запрещены.
            _ => {
                if seen_pe.is_some() {
                    return false;
                }
            }
        }
    }
    true
}

/// Разрешена ли пара «pseudo-element сразу за pseudo-element-ом».
/// Спека допускает маркер сгенерированного контента (`::before::marker`,
/// `::after::marker`, CSS Pseudo-Elements L4 §2.4) и колонку multicol с её
/// маркером (`::before::column`, `::column::scroll-marker`, CSS Multi-column L2
/// §column-pseudo).
pub(crate) fn pseudo_element_pair_allowed(first: &PseudoElementKind, second: &PseudoElementKind) -> bool {
    // `::details-content` — tree-abiding псевдоэлемент (Pseudo-Elements L4
    // §3.2), у него есть собственные `::before`/`::after`.
    if let (PseudoElementKind::Unknown(name), PseudoElementKind::Before | PseudoElementKind::After) =
        (first, second)
        && name.eq_ignore_ascii_case("details-content")
    {
        return true;
    }
    matches!(
        (first, second),
        (
            PseudoElementKind::Before | PseudoElementKind::After,
            PseudoElementKind::Marker | PseudoElementKind::Column
        ) | (PseudoElementKind::Column, PseudoElementKind::ScrollMarker)
    )
}

/// Может ли за pseudo-element-ом стоять user-action-псевдокласс.
/// Highlight-псевдоэлементы (`::selection`, `::highlight()`) не допускают
/// после себя ничего (CSS Highlight API L1 §2.2).
pub(crate) fn pseudo_element_allows_user_action(pe: &PseudoElementKind) -> bool {
    !matches!(
        pe,
        PseudoElementKind::Selection
            | PseudoElementKind::TargetText
            | PseudoElementKind::SpellingError
            | PseudoElementKind::GrammarError
            | PseudoElementKind::Highlight(_)
            | PseudoElementKind::Column
    )
}

/// User-action-псевдоклассы (CSS Selectors L4 §4) — единственное, что спека
/// разрешает после tree-abiding pseudo-element-а. `:is()`/`:where()`
/// прозрачны, если целиком состоят из таких же псевдоклассов.
pub(crate) fn is_user_action_pseudo_class(pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::Hover
        | PseudoClass::Active
        | PseudoClass::Focus
        | PseudoClass::FocusWithin
        | PseudoClass::FocusVisible => true,
        PseudoClass::Is(list) | PseudoClass::Where(list) => list.iter().all(|c| {
            c.tail.is_empty()
                && c.head.parts.iter().all(|p| {
                    matches!(p, SimpleSelector::PseudoClass(inner) if is_user_action_pseudo_class(inner))
                })
        }),
        _ => false,
    }
}

/// Валиден ли pseudo-element: известен движку и с допустимым аргументом.
pub(crate) fn pseudo_element_is_valid(pe: &PseudoElementKind) -> bool {
    match pe {
        PseudoElementKind::Unknown(name) => is_known_unmodeled_pseudo_element(name),
        // `::picker()` определён спекой только для аргумента `select`.
        PseudoElementKind::Picker(arg) => arg == "select",
        // `::scroll-button()` (CSS Overflow L5 §scroll-buttons): the closed
        // set of physical/logical directions plus the `*` wildcard —
        // confirmed exhaustively by `scroll-buttons-{valid,invalid}.html`
        // (`north`/`5051`/empty/multi-value all rejected here).
        PseudoElementKind::ScrollButton(dir) => matches!(
            dir.as_str(),
            "up" | "down" | "left" | "right" | "block-start" | "inline-start"
                | "inline-end" | "block-end" | "*"
        ),
        // `::slotted` без аргумента — синтаксически невозможен по грамматике.
        PseudoElementKind::Slotted(None) => false,
        PseudoElementKind::Slotted(Some(list)) => {
            list.iter().all(|c| complex_selector_is_valid(c, false))
        }
        _ => true,
    }
}

/// Валиден ли псевдокласс: известен движку, а его аргумент-список (для
/// non-forgiving функциональных форм) сам валиден.
pub(crate) fn pseudo_class_is_valid(pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::Unsupported(_) => false,
        // `:is()`/`:where()` — forgiving-selector-list (CSS Selectors L4
        // §3.2): невалидные элементы списка отбрасываются, а не делают
        // невалидным весь селектор, поэтому содержимое не проверяем.
        PseudoClass::Is(_) | PseudoClass::Where(_) => true,
        PseudoClass::Not(list) | PseudoClass::Host(Some(list)) => {
            list.iter().all(|c| complex_selector_is_valid(c, false))
        }
        PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)) => {
            list.iter().all(|c| complex_selector_is_valid(c, false))
        }
        PseudoClass::Has(list) => list
            .iter()
            .all(|r| complex_selector_is_valid(&r.selector, false)),
        _ => true,
    }
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_selector_list(&mut self) -> Vec<ComplexSelector> {
        let mut sels = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.parse_complex_selector() {
                Some(s) => sels.push(s),
                None => break,
            }
            self.skip_ws_and_comments();
            if self.peek() == Some(',') {
                self.consume();
                continue;
            }
            break;
        }
        sels
    }

    /// Forgiving-selector-list (Selectors L4 §3.4) для аргументов `:is()` /
    /// `:where()`: элемент, который не разобрался целиком, отбрасывается
    /// (пропускается до ближайшей верхнеуровневой `,` или `)`), а не делает
    /// невалидным весь список. Останавливается на `)` либо конце ввода, не
    /// съедая их.
    fn parse_forgiving_selector_list(&mut self) -> Vec<ComplexSelector> {
        let mut sels = Vec::new();
        loop {
            self.skip_ws_and_comments();
            let start = self.pos;
            let parsed = self.parse_complex_selector();
            self.skip_ws_and_comments();
            match (parsed, self.peek()) {
                (Some(s), None | Some(',' | ')')) => sels.push(s),
                _ => {
                    self.pos = start;
                    self.skip_forgiving_item();
                }
            }
            if self.peek() == Some(',') {
                self.consume();
                continue;
            }
            break;
        }
        sels
    }

    /// Пропускает остаток элемента forgiving-списка до верхнеуровневой `,`
    /// или закрывающей `)` внешнего списка (не потребляя её), учитывая
    /// вложенные скобки, строки и экранирование.
    fn skip_forgiving_item(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            match c {
                ',' | ')' if depth == 0 => return,
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '\\' => {
                    self.consume();
                }
                '"' | '\'' => {
                    self.consume();
                    while let Some(q) = self.consume() {
                        if q == '\\' {
                            self.consume();
                        } else if q == c {
                            break;
                        }
                    }
                    continue;
                }
                _ => {}
            }
            self.consume();
        }
    }

    /// Строгий вариант [`Parser::parse_selector_list`] для DOM-границы
    /// (`querySelector`/`matches`, BUG-391): возвращает `None`, если хоть один
    /// элемент списка не разобрался целиком, вместо того чтобы молча вернуть
    /// уже накопленный префикс. Лояльный вариант на `"div,"` отдаёт `[div]`
    /// и оставляет позицию после запятой — по нему невозможно отличить
    /// «список кончился» от «следующий элемент невалиден».
    ///
    /// Хвост входа не проверяется — это делает вызывающий
    /// [`is_valid_selector_list`], потому что тот же метод используется для
    /// вложенных списков, где ожидается `)`.
    pub(crate) fn parse_selector_list_strict(&mut self) -> Option<Vec<ComplexSelector>> {
        let mut sels = Vec::new();
        loop {
            self.skip_ws_and_comments();
            sels.push(self.parse_complex_selector()?);
            self.skip_ws_and_comments();
            if self.peek() == Some(',') {
                self.consume();
                continue;
            }
            break;
        }
        Some(sels)
    }

    pub(crate) fn parse_complex_selector(&mut self) -> Option<ComplexSelector> {
        let head = self.parse_compound_selector()?;
        let mut tail = Vec::new();
        loop {
            // Между compound-ами может быть whitespace + явный combinator,
            // либо просто whitespace (descendant), либо ничего (значит конец).
            let had_ws = self.skip_ws_and_comments_track();
            match self.peek() {
                // `)` — конец списка внутри функционального pseudo (`:is(...)` /
                // `:where(...)`); вне его `)` не появляется в правильном CSS.
                None | Some(',') | Some('{') | Some('}') | Some(')') => break,
                Some('>') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::Child, comp));
                }
                Some('+') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::NextSibling, comp));
                }
                Some('~') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::LaterSibling, comp));
                }
                Some(_) if had_ws => {
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::Descendant, comp));
                }
                Some(_) => break,
            }
        }
        Some(ComplexSelector { head, tail })
    }

    pub(crate) fn parse_compound_selector(&mut self) -> Option<CompoundSelector> {
        let mut parts = Vec::new();
        while let Some(part) = self.parse_simple_selector() {
            parts.push(part);
        }
        if parts.is_empty() {
            None
        } else {
            Some(CompoundSelector { parts })
        }
    }

    pub(crate) fn parse_simple_selector(&mut self) -> Option<SimpleSelector> {
        match self.peek()? {
            // Selectors §6.3: `*|name` / `|name` — префикс пространства имён.
            // `@namespace` не поддержан, поэтому префикс отбрасывается:
            // wptrunner генерирует `:root > *|body:nth-child(2)` для клика.
            '*' if self.peek_at(1) == Some('|') && self.peek_at(2) != Some('=') => {
                self.consume();
                self.consume();
                self.parse_simple_selector()
            }
            '|' if matches!(self.peek_at(1), Some(c) if c == '*' || c == '\\' || is_ident_start(c)) => {
                // `|E` — элемент без пространства имён; у HTML-элементов его нет,
                // компаунд не должен совпасть ни с чем. `|` не может быть именем
                // тега, так что `Type("|")` — заведомо пустое множество.
                self.consume();
                self.parse_simple_selector()?;
                Some(SimpleSelector::Type("|".to_string()))
            }
            '*' => {
                self.consume();
                Some(SimpleSelector::Universal)
            }
            '.' => {
                self.consume();
                Some(SimpleSelector::Class(self.parse_ident()?))
            }
            '#' => {
                self.consume();
                Some(SimpleSelector::Id(self.parse_ident()?))
            }
            // Nesting L1 §3.2: `&` вне вложенного правила (верхний уровень,
            // `selector(&)`) — `:where(:scope)`. Внутри style-правила `&` подставляется
            // раньше, до разбора селектора.
            '&' => {
                self.consume();
                Some(SimpleSelector::PseudoClass(PseudoClass::Where(vec![ComplexSelector {
                    head: CompoundSelector {
                        parts: vec![SimpleSelector::PseudoClass(PseudoClass::Scope)],
                    },
                    tail: Vec::new(),
                }])))
            }
            '[' => self.parse_attr_selector(),
            ':' => self.parse_pseudo(),
            c if is_ident_start(c) || c == '\\' => {
                let name = self.parse_ident()?;
                // `ns|E`: `@namespace` не поддержан, а необъявленный префикс
                // по Namespaces §6.3 — невалидный селектор.
                if self.peek() == Some('|') && self.peek_at(1) != Some('=') {
                    return None;
                }
                Some(SimpleSelector::Type(name))
            }
            _ => None,
        }
    }

    pub(crate) fn parse_attr_selector(&mut self) -> Option<SimpleSelector> {
        self.consume(); // '['
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        let op = match self.peek()? {
            ']' => {
                self.consume();
                return Some(SimpleSelector::Attribute(AttrSelector {
                    name,
                    op: None,
                    value: None,
                    case_insensitive: false,
                }));
            }
            '=' => {
                self.consume();
                AttrOp::Equals
            }
            '~' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Includes
            }
            '|' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::DashMatch
            }
            '^' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Prefix
            }
            '$' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Suffix
            }
            '*' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Substring
            }
            _ => {
                self.recover_to_attr_end();
                return None;
            }
        };
        self.skip_ws_and_comments();
        let value = self.parse_attr_value()?;
        self.skip_ws_and_comments();
        // CSS Selectors L4 §6.3.6: `i` или `s` после value — модификатор
        // сравнения. `i` — ASCII case-insensitive, `s` — explicit case-sensitive
        // (default). Парсятся case-insensitively сами по себе (`I` / `S` тоже
        // валидны).
        let case_insensitive = match self.peek() {
            Some('i' | 'I') => {
                self.consume();
                self.skip_ws_and_comments();
                true
            }
            Some('s' | 'S') => {
                self.consume();
                self.skip_ws_and_comments();
                false
            }
            _ => false,
        };
        if self.peek() != Some(']') {
            self.recover_to_attr_end();
            return None;
        }
        self.consume(); // ']'
        Some(SimpleSelector::Attribute(AttrSelector {
            name,
            op: Some(op),
            value: Some(value),
            case_insensitive,
        }))
    }

    pub(crate) fn parse_attr_value(&mut self) -> Option<String> {
        match self.peek()? {
            q @ ('"' | '\'') => {
                self.consume();
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c == q {
                        self.consume();
                        return Some(s);
                    }
                    self.consume();
                    s.push(c);
                }
                None
            }
            _ => self.parse_ident(),
        }
    }

    pub(crate) fn recover_to_attr_end(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ']' => {
                    self.consume();
                    return;
                }
                '{' | '}' | ';' => return,
                _ => {
                    self.consume();
                }
            }
        }
    }

}

/// Парсит формулу `an+b` из строки. Поддерживает `odd`, `even`, целые числа,
/// и любые комбинации `<int>?n<sign><int>?`. Пробелы внутри допустимы и
/// игнорируются (CSS spec).
pub(crate) fn parse_nth_spec_str(s: &str) -> Option<NthSpec> {
    let s: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if s == "odd" {
        return Some(NthSpec::ODD);
    }
    if s == "even" {
        return Some(NthSpec::EVEN);
    }
    if let Some(n_pos) = s.find('n') {
        let a_part = &s[..n_pos];
        let b_part = &s[n_pos + 1..];
        let a: i32 = match a_part {
            "" | "+" => 1,
            "-" => -1,
            _ => a_part.parse().ok()?,
        };
        let b: i32 = if b_part.is_empty() {
            0
        } else {
            if !b_part.starts_with('+') && !b_part.starts_with('-') {
                return None;
            }
            b_part.parse().ok()?
        };
        Some(NthSpec { a, b })
    } else {
        Some(NthSpec { a: 0, b: s.parse().ok()? })
    }
}
