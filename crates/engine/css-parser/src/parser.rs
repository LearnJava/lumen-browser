//! CSS-парсер (Phase 0+).
//!
//! Поддерживается:
//!   - правила `selector_list { decl_list }`;
//!   - simple selectors: type / class / id / universal / attribute / pseudo-class;
//!   - compound selectors (`p.foo#bar:first-child`);
//!   - complex selectors с combinator-ами: descendant ` `, child `>`,
//!     next-sibling `+`, later-sibling `~`;
//!   - attribute selectors `[name]`, `[name=val]`, `[name~=val]`, `[name|=val]`,
//!     `[name^=val]`, `[name$=val]`, `[name*=val]`;
//!   - structural pseudo-classes:
//!       - `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`;
//!       - `:first-of-type`, `:last-of-type`, `:only-of-type`;
//!       - `:nth-child(an+b)`, `:nth-last-child(an+b)`,
//!         `:nth-of-type(an+b)`, `:nth-last-of-type(an+b)` — формулы
//!         `an+b`, целые числа, ключевые слова `odd` / `even`;
//!       - `:not(selector-list)` — CSS Selectors L4 §5.4: отрицание
//!         selector-list-а. Внутри разрешены complex-селекторы и nested
//!         `:not`. Матчит элемент, если ни один из селекторов списка ему
//!         не подходит. Specificity = максимум по списку (как у `:is`);
//!       - `:is(selector-list)` / `:where(selector-list)` — CSS4; матчит,
//!         если матчит любой из селекторов списка. Внутри разрешены любые
//!         complex-селекторы. Specificity для `:is` = максимум по списку,
//!         для `:where` = 0.
//!   - interactive pseudo-classes (`:hover`, `:focus`, …) сохраняются как
//!     `PseudoClass::Unsupported(name)` и при матчинге всегда возвращают `false`;
//!   - pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат
//!     (т.к. в DOM им ничего не соответствует);
//!   - комментарии `/* */`, перечисление селекторов через `,`, опциональный
//!     trailing `;`. At-rules (`@media`, `@import`) пропускаются.
//!
//! Не поддерживается (отложено): namespace prefix в селекторах,
//! типизированные значения деклараций (length / color / calc).

use std::cmp::Ordering;

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
    /// отложен до bidi-движка). Невалидные аргументы → `Unsupported(name)`.
    Dir(DirArg),
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
    /// `::highlight(name)` (CSS Highlight API L1 §3) — custom text highlight.
    /// Аргумент `name` — ключ в `CSS.highlights` реестре. Phase 0: парсирует имя,
    /// Phase 1: вызывает `emit_text_with_highlights()` для рендеринга.
    Highlight(String),
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

fn compound_to_css_str(c: &CompoundSelector) -> String {
    c.parts.iter().map(simple_to_css_str).collect()
}

fn simple_to_css_str(s: &SimpleSelector) -> String {
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

fn attr_to_css_str(attr: &AttrSelector) -> String {
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
                format!("[{}{}\"{}\"", attr.name, op_str, v)
            }
        }
    }
}

fn nth_to_css_str(spec: &NthSpec) -> String {
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

fn sels_to_css_str(sels: &[ComplexSelector]) -> String {
    sels.iter().map(ComplexSelector::to_css_str).collect::<Vec<_>>().join(", ")
}

fn pc_to_css_str(pc: &PseudoClass) -> String {
    match pc {
        PseudoClass::FirstChild => ":first-child".into(),
        PseudoClass::LastChild => ":last-child".into(),
        PseudoClass::OnlyChild => ":only-child".into(),
        PseudoClass::Empty => ":empty".into(),
        PseudoClass::Root => ":root".into(),
        PseudoClass::FirstOfType => ":first-of-type".into(),
        PseudoClass::LastOfType => ":last-of-type".into(),
        PseudoClass::OnlyOfType => ":only-of-type".into(),
        PseudoClass::NthChild(spec, _) => format!(":nth-child({})", nth_to_css_str(spec)),
        PseudoClass::NthLastChild(spec, _) => format!(":nth-last-child({})", nth_to_css_str(spec)),
        PseudoClass::NthOfType(spec) => format!(":nth-of-type({})", nth_to_css_str(spec)),
        PseudoClass::NthLastOfType(spec) => format!(":nth-last-of-type({})", nth_to_css_str(spec)),
        PseudoClass::Not(sels) => format!(":not({})", sels_to_css_str(sels)),
        PseudoClass::Is(sels) => format!(":is({})", sels_to_css_str(sels)),
        PseudoClass::Where(sels) => format!(":where({})", sels_to_css_str(sels)),
        PseudoClass::Has(_) => ":has(…)".into(),
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
        PseudoClass::Lang(tags) => format!(":lang({})", tags.join(", ")),
        PseudoClass::Link => ":link".into(),
        PseudoClass::Visited => ":visited".into(),
        PseudoClass::AnyLink => ":any-link".into(),
        PseudoClass::InRange => ":in-range".into(),
        PseudoClass::OutOfRange => ":out-of-range".into(),
        PseudoClass::Dir(DirArg::Ltr) => ":dir(ltr)".into(),
        PseudoClass::Dir(DirArg::Rtl) => ":dir(rtl)".into(),
        PseudoClass::Scope => ":scope".into(),
        PseudoClass::Target => ":target".into(),
        PseudoClass::TargetWithin => ":target-within".into(),
        PseudoClass::Defined => ":defined".into(),
        PseudoClass::Fullscreen => ":fullscreen".into(),
        PseudoClass::Modal => ":modal".into(),
        PseudoClass::PopoverOpen => ":popover-open".into(),
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
        PseudoClass::Unsupported(name) => format!(":{name}"),
    }
}

fn pe_to_css_str(pe: &PseudoElementKind) -> String {
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
        PseudoElementKind::Highlight(name) => format!("::highlight({name})"),
        PseudoElementKind::Unknown(name) => format!("::{name}"),
    }
}

/// Максимум specificity среди списка ComplexSelector-ов. Используется для
/// `:is(...)` (CSS4 §17): pseudo-class contributes specificity of the most
/// specific item in its argument list.
fn max_list_specificity(list: &[ComplexSelector]) -> Option<Specificity> {
    list.iter().map(ComplexSelector::specificity).max()
}

fn accumulate_specificity(comp: &CompoundSelector, spec: &mut Specificity) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    /// `!important` флаг (CSS Cascade L4 §8.1). При равной specificity
    /// `important = true` побеждает `important = false`.
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
}

/// CSS Properties and Values L1 §1.1 — регистрация custom property через
/// `@property --name { syntax: ...; inherits: ...; initial-value: ...; }`.
/// Обязательные descriptors: `syntax`, `inherits`. `initial-value`
/// обязателен, если syntax не universal (`*`). Имя хранится с ведущими
/// `--` для прямого сравнения с `custom_props` в layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyRule {
    pub name: String,
    pub syntax: String,
    pub inherits: bool,
    pub initial_value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
    /// Зарегистрированные `@property`-правила. Порядок соответствует
    /// исходному CSS; повтор имени — последнее объявление побеждает (по
    /// CSS Properties and Values L1 §1.1).
    pub properties: Vec<PropertyRule>,
    /// `@media`-правила. Каждое содержит query и список вложенных rules.
    /// Применяются в каскаде только если `query.matches(ctx)` — см.
    /// `MediaQuery::matches`. Порядок source-position для tie-breaking
    /// в каскаде сохраняется через position в `Vec` (но фактическая
    /// специфика media rules в Phase 0 layout-у мерджится «как обычные»).
    pub media_rules: Vec<MediaRule>,
    /// `@import url("...");` декларации. Парсер собирает URL и опц.
    /// media-query (`@import url("a") screen and (min-width: 600px);`).
    /// Сам fetch и инкорпорация в каскад — задача потребителя (shell),
    /// потому что это требует сетевой/файловой загрузки. Phase 0:
    /// парсер только извлекает список, fetch отложен.
    pub imports: Vec<ImportRule>,
    /// `@font-face` правила. CSS Fonts L4 §4. Parser извлекает family,
    /// src, weight, style, display, unicode-range; реальная загрузка
    /// и регистрация в font-matcher — задача shell.
    pub font_faces: Vec<FontFaceRule>,
    /// CSS Cascade L5 §6.4 — порядок объявления layer-имён через
    /// statement-form `@layer base, components, utilities;`. В этом
    /// списке имена в **обратном** cascade-приоритете: первый имя имеет
    /// наименьший приоритет; unlayered rules выигрывают у всех layered.
    /// Анонимные layer-блоки (без имени) попадают сюда же с
    /// generated-именем `__anon_<n>__`.
    pub layer_order: Vec<String>,
    /// CSS Cascade L5 — block-form `@layer name { rules }`. Каждая
    /// запись — отдельный блок (повторное упоминание одного имени —
    /// отдельные записи; cascade-приоритет внутри layer-а — source-order).
    /// Phase 0 интеграция в каскад отложена — текущий compute_style
    /// итерирует только `rules`/`media_rules`. Здесь только parse+store.
    pub layers: Vec<LayerRule>,
    /// CSS Conditional Rules L3 §2 — `@supports (cond) { rules }`. Условие
    /// типизировано как [`SupportsCondition`]; вложенные rules применяются
    /// если `condition.evaluate(...)` истинно. Phase 0: parse+store +
    /// evaluator на основе списка известных property-имён; реальная
    /// интеграция в каскад — следующая задача (см. media_rules).
    pub supports_rules: Vec<SupportsRule>,
    /// CSS Animations L1 §3 — `@keyframes name { 0% {...} 50% {...} ... }`.
    /// Frames хранятся как `(offset_percent, declarations)`. Phase 0:
    /// parse+store; реальный animation runtime (interpolation, timing
    /// functions, animation-name связывание) отложен.
    pub keyframes: Vec<KeyframesRule>,
    /// CSS Counter Styles L3 §2 — `@counter-style name { ... }`. Phase 0:
    /// parse+store как `Vec<(name, declarations)>`. Реальное применение
    /// (список как кастомные markers через list-style-type) отложено.
    pub counter_styles: Vec<CounterStyleRule>,
    /// CSS Paged Media L3 §3 — `@page <selector>? { ... }`. Phase 0:
    /// parse+store. Реальная pagination — отдельная задача (Phase 2+).
    pub page_rules: Vec<PageRule>,
    /// CSS Cascade L6 — `@scope (<root>) [to (<limit>)] { rules }`. Phase 0:
    /// parse+store; реальная scope-фильтрация в каскаде отложена.
    pub scope_rules: Vec<ScopeRule>,
    /// CSS Transitions L2 §3.4 — `@starting-style { rules }`. Phase 0:
    /// parse+store. Применение при первом match (transition-from-display)
    /// отложено вместе с реальным transition runtime.
    pub starting_style_rules: Vec<StartingStyleRule>,
    /// CSS Containment L3 §3 — `@container <name>? (cond) { rules }`.
    /// Условие хранится как сырая строка (типизация query — отложена,
    /// нужна полная media-query-like grammar для container features).
    pub container_rules: Vec<ContainerRule>,
    /// CSS Fonts L4 §13 — `@font-palette-values --name { ... }`. Phase 0:
    /// parse+store. Matching against `font-palette` property and CPAL index
    /// resolution happen in layout (`resolve_font_palette_for_family`).
    pub font_palette_values: Vec<FontPaletteValuesRule>,
}

/// `@font-palette-values --name { font-family: ...; base-palette: N; override-colors: ... }`
/// CSS Fonts L4 §13. Defines a named custom color palette for a COLR color font.
/// Matched against an element's `font-palette` property value to resolve which
/// palette overrides apply at render time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontPaletteValuesRule {
    /// Dashed-ident name, e.g. `--my-palette`. Used to match `font-palette` property values.
    pub name: String,
    /// `font-family` descriptor — the font family this palette applies to (without quotes).
    pub font_family: Option<String>,
    /// `base-palette` descriptor — 0-based index of the built-in CPAL palette to start from.
    /// None means start from palette index 0 (the default palette).
    pub base_palette: Option<u16>,
    /// `override-colors` descriptor — raw `"<index> <color>"` pairs as strings.
    /// Stored raw for layout-side parsing via `parse_color`. Each entry is `(index, color_str)`.
    pub override_colors: Vec<(u16, String)>,
}

/// `@container <name>? <condition> { rules }` — CSS Containment L3 §3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRule {
    /// Имя container query (по умолчанию — None, match всех ancestor-ов
    /// с container-name / container-type).
    pub name: Option<String>,
    /// Сырая condition-строка типа `(min-width: 200px)` или `style(...)`.
    pub condition: String,
    pub rules: Vec<Rule>,
}

/// `@counter-style <name> { ... }` — CSS Counter Styles L3 §2.
/// Phase 0: parse+store. Descriptors (`system`, `symbols`, `suffix`,
/// `range`, `prefix`, `pad`, `negative`, ...) хранятся как declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterStyleRule {
    pub name: String,
    pub declarations: Vec<Declaration>,
}

/// `@page <selector>? { decls }` — CSS Paged Media L3 §3.
/// Selector — пустой (любая страница), `:first`, `:left`, `:right`,
/// `:blank`, named `page-name`. Phase 0: хранится сырая строка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRule {
    /// Pseudo-classes и/или page-name. Пустая строка = любой page.
    pub selector: String,
    pub declarations: Vec<Declaration>,
}

/// `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6.
/// `root` — селектор корня scope, `limit` — селектор upper boundary
/// (рекурсивный обход вниз останавливается на нём). Phase 0: оба
/// хранятся сырыми строками; реальный scope-matcher отложен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRule {
    /// Селектор корня scope. Может быть пустым (`@scope { ... }`
    /// без явного root — implicit `:scope` = stylesheet root).
    pub root: String,
    /// Опциональный limit (`to (<selector>)`). None — без верхней границы.
    pub limit: Option<String>,
    pub rules: Vec<Rule>,
}

/// `@starting-style { rules }` — CSS Transitions L2 §3.4. Контейнер
/// rules, применяющихся как initial state при first match (для
/// transition-on-display-changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartingStyleRule {
    pub rules: Vec<Rule>,
}

/// `@keyframes name { offset { decls } ... }` — CSS Animations L1 §3.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframesRule {
    pub name: String,
    /// Список frames в порядке появления в source. Один frame может
    /// иметь несколько offset-ов (selector-list типа `0%, 50%`) —
    /// разворачивается в отдельные записи.
    pub frames: Vec<Keyframe>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// Offset в долях `[0, 1]`. `from` → 0.0, `to` → 1.0. Невалидные
    /// (NaN или вне [0,1]) → пропускаются на этапе парсинга.
    pub offset: f32,
    pub declarations: Vec<Declaration>,
}

/// `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportsRule {
    pub condition: SupportsCondition,
    pub rules: Vec<Rule>,
}

/// Условие в `@supports (...)`. Грамматика:
/// `<condition> = <negation> | <conjunction> | <disjunction> | <test>`
/// `<negation>  = "not" <inside-parens>`
/// `<conjunction> = <test> ("and" <test>)+`
/// `<disjunction> = <test> ("or" <test>)+`
/// `<test>       = "(" <property>: <value> ")" | "(" <condition> ")"`.
///
/// Phase 0: парсер также распознаёт `selector(<simple>)` (CSS Conditional
/// L4) и сохраняет селектор как сырую строку — реальный матчинг отложен.
/// Неизвестные функциональные тесты (`font-tech(...)`, `font-format(...)`)
/// → `Unknown`, evaluator возвращает false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportsCondition {
    /// `(prop: value)` — declaration test. Текущий supports-evaluator
    /// проверяет, что `property` есть в списке known-property-имён,
    /// не валидируя value (для Phase 0 этого достаточно — мы поддерживаем
    /// конкретный набор properties, и tests типа `(display: grid)`
    /// возвращают true, потому что мы парсим `display`, даже если
    /// реального grid layout-а нет).
    Decl { property: String, value: String },
    Not(Box<SupportsCondition>),
    And(Vec<SupportsCondition>),
    Or(Vec<SupportsCondition>),
    /// `selector(<sel>)` — CSS Conditional L4. Phase 0 не оценивает.
    Selector(String),
    /// Невалидный или нераспознанный тест — evaluator возвращает false.
    Unknown,
}

impl SupportsCondition {
    /// Вычислить условие: вернуть `true`, если потребитель поддерживает
    /// все объявления в условии. `known_properties` — список property-
    /// имён, которые css-parser/layout распознают (например, `display`,
    /// `color`, `grid-template-columns`). `Selector(...)` и `Unknown`
    /// в Phase 0 возвращают false.
    pub fn evaluate(&self, known_properties: &[&str]) -> bool {
        match self {
            Self::Decl { property, .. } => known_properties
                .iter()
                .any(|p| p.eq_ignore_ascii_case(property)),
            Self::Not(c) => !c.evaluate(known_properties),
            Self::And(cs) => cs.iter().all(|c| c.evaluate(known_properties)),
            Self::Or(cs) => cs.iter().any(|c| c.evaluate(known_properties)),
            Self::Selector(_) | Self::Unknown => false,
        }
    }
}

/// `@layer name { rules }` блок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRule {
    /// Имя layer-а. Анонимный блок (`@layer { ... }`) получает имя
    /// `__anon_<n>__` где `n` — порядковый номер.
    pub name: String,
    pub rules: Vec<Rule>,
}

/// `@import` декларация. Per CSS Cascade L4 §6.5 + Media Queries L4:
/// `@import url("path");` или `@import url("path") <media-query>;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRule {
    /// URL для загрузки. Хранится как есть (без resolve относительно base).
    pub url: String,
    /// Опциональный media query — стиль применим только если query
    /// matches. Пустой Vec в `clauses` (=default) трактуется как
    /// «всегда применять» (= `@import url("...")` без media-фильтра).
    pub media: MediaQuery,
}

/// `@font-face { font-family: ...; src: url(...) format(...); ... }`
/// — CSS Fonts L4 §4. Регистрация webfont-ресурса для font-matcher-а.
/// Phase 0: парсер собирает основные descriptors; реальный fetch и
/// font-loading — задача font-matcher / shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFaceRule {
    /// `font-family: "Roboto"` — имя без кавычек.
    pub family: String,
    /// `src: url("..."), url("..."), local("...")` — список источников.
    pub sources: Vec<FontFaceSource>,
    /// `font-weight: 400 | bold | 100 200 ...` — хранится сырой строкой
    /// (font-matcher парсит keyword/число/диапазон по контексту). `None` = default (400).
    pub weight: Option<String>,
    /// `font-style: normal | italic | oblique`. `None` = default.
    pub style: Option<String>,
    /// `font-stretch: condensed | expanded | 75% 125% ...` — сырая строка. `None` = default (normal).
    pub stretch: Option<String>,
    /// `font-display: auto | block | swap | fallback | optional`. `None` = default (auto).
    pub display: Option<String>,
    /// `unicode-range: U+0000-FFFF, U+10000-1FFFF` — сырая строка.
    pub unicode_range: Option<String>,
    /// `font-variant: small-caps | ...` — CSS Fonts L3/L4 §7. Сырая строка.
    pub variant: Option<String>,
    /// `font-feature-settings: "liga" 1, "kern" 0` — CSS Fonts L3 §6. Сырая строка.
    pub feature_settings: Option<String>,
    /// `font-variation-settings: "wght" 400, "ital" 1` — CSS Fonts L4 §6 (variable fonts). Сырая строка.
    pub variation_settings: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFaceSource {
    pub kind: FontFaceSourceKind,
    /// Значение url или local — без кавычек.
    pub value: String,
    /// `format("woff2")` — hint о формате. None если не указан.
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFaceSourceKind {
    /// `src: url("...")` — внешний font-файл.
    Url,
    /// `src: local("...")` — системный шрифт по имени.
    Local,
}

/// Группа CSS-правил, вложенных в `@media`-блок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRule {
    pub query: MediaQuery,
    pub rules: Vec<Rule>,
}

/// Media query — OR-список AND-clauses (Media Queries L4 §3). Пустой
/// `clauses` (нет условий) трактуется как «всегда true» (= `@media all`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaQuery {
    /// Comma-separated OR-список. При пустом `clauses` query всегда
    /// матчит (`@media all`).
    pub clauses: Vec<MediaQueryClause>,
}

/// Одна clause в media query — AND-список feature/media-type условий
/// с опциональным `not`-модификатором.
///
/// Media Queries L4 §3.2: `not <media-query>` инвертирует результат
/// _всей_ clause. `only <media-type>` — L3-совместимый no-op-модификатор
/// (использовался для скрытия media-query от старых браузеров, для
/// современных парсеров значимого эффекта не несёт).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaQueryClause {
    /// Истина для `not screen and (min-width: 600px)` — инвертирует
    /// итоговый результат clause целиком. Per §3.2 unknown-условия
    /// внутри negated clause не дают `true`: clause с любым
    /// `Unsupported` оценивается как unknown и не матчит.
    pub negated: bool,
    /// AND-list. Пустой — clause-error (например, `not` без feature),
    /// `matches()` отдаст `false`.
    pub conditions: Vec<MediaCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCondition {
    /// `screen`, `print`, `all`, `handheld`, etc. — media type.
    /// Хранится lower-case. `all` всегда match. Прочие имена match
    /// если совпадают с `MediaContext::media_type` (lower-case).
    MediaType(String),
    /// `(min-width: 600px)` и подобные. Phase 0 поддерживает:
    /// min/max-width, min/max-height, orientation, prefers-color-scheme.
    Feature(MediaFeature),
    /// Любая `(unknown-feature: value)` — никогда не матчит (forward-compat).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    // Viewport dimensions — exact and range
    Width(f32),
    MinWidth(f32),
    MaxWidth(f32),
    Height(f32),
    MinHeight(f32),
    MaxHeight(f32),
    // Aspect ratio: numerator/denominator stored as f32 ratio
    AspectRatio(f32),
    MinAspectRatio(f32),
    MaxAspectRatio(f32),
    // Display
    Orientation(MediaOrientation),
    // User preferences (MQ L5, commonly used)
    PrefersColorScheme(ColorScheme),
    PrefersReducedMotion(bool),
    // CSS Forced Colors Mode (Forced Colors L1) — опубликована (active/none)
    ForcedColors(bool),
    // Interaction media features (Media Queries L4 §5.3-5.6)
    /// `(hover: none | hover)` — hover-способность основного указателя.
    Hover(MediaHover),
    /// `(any-hover: none | hover)` — hover-способность любого указателя.
    AnyHover(MediaHover),
    /// `(pointer: none | coarse | fine)` — точность основного указателя.
    Pointer(MediaPointer),
    /// `(any-pointer: none | coarse | fine)` — точность любого указателя.
    AnyPointer(MediaPointer),
    // User-preference media features (Media Queries L5 §5.5/§5.6)
    /// `(prefers-contrast: no-preference | more | less | custom)` —
    /// предпочтение пользователя по контрастности интерфейса.
    PrefersContrast(MediaContrast),
    /// `(prefers-reduced-data: no-preference | reduce)` —
    /// предпочтение пользователя по экономии сетевого трафика.
    PrefersReducedData(MediaReducedData),
}

impl Eq for MediaFeature {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOrientation {
    Portrait,
    Landscape,
}

/// Media Queries L4 §5.3/§5.5 — hover-способность указателя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaHover {
    /// Указатель не может наводиться без активации (тач-экран).
    None,
    /// Указатель может удобно наводиться (мышь).
    Hover,
}

/// Media Queries L4 §5.4/§5.6 — точность указателя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPointer {
    /// Указывающего устройства нет.
    None,
    /// Грубый указатель (палец на тач-экране).
    Coarse,
    /// Точный указатель (мышь, стилус).
    Fine,
}

/// Media Queries L5 §5.5 — `prefers-contrast`: запрошенный пользователем
/// уровень контрастности интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaContrast {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил больший контраст.
    More,
    /// Пользователь запросил меньший контраст.
    Less,
    /// Активирована пользовательская цветовая схема (forced colors и т.п.).
    Custom,
}

/// Media Queries L5 §5.6 — `prefers-reduced-data`: запрос на экономию
/// сетевого трафика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaReducedData {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил режим экономии трафика.
    Reduce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

/// Контекст, против которого матчатся media queries. Заполняется
/// shell-ом / layout-ом из текущего viewport-а и пользовательских
/// настроек.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaContext {
    /// «screen» / «print» / «all» / прочее.
    pub media_type: String,
    pub width: f32,
    pub height: f32,
    pub prefers_dark: bool,
    /// Соответствует `prefers-reduced-motion: reduce`.
    pub prefers_reduced_motion: bool,
    /// CSS Forced Colors: соответствует `(forced-colors: active)` media feature.
    pub forced_colors: bool,
    /// hover-способность основного указателя (`hover` media feature).
    pub hover: MediaHover,
    /// hover-способность любого указателя (`any-hover` media feature).
    pub any_hover: MediaHover,
    /// Точность основного указателя (`pointer` media feature).
    pub pointer: MediaPointer,
    /// Точность любого указателя (`any-pointer` media feature).
    pub any_pointer: MediaPointer,
    /// Предпочтение контрастности (`prefers-contrast` media feature).
    pub prefers_contrast: MediaContrast,
    /// Предпочтение экономии трафика (`prefers-reduced-data` media feature).
    pub prefers_reduced_data: MediaReducedData,
}

impl Default for MediaContext {
    fn default() -> Self {
        // Desktop-дефолты: есть мышь → hover-способность и точный указатель.
        Self {
            media_type: "screen".into(),
            width: 0.0,
            height: 0.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            hover: MediaHover::Hover,
            any_hover: MediaHover::Hover,
            pointer: MediaPointer::Fine,
            any_pointer: MediaPointer::Fine,
            // Desktop-дефолты: пользователь не запрашивал особый контраст
            // или экономию трафика.
            prefers_contrast: MediaContrast::NoPreference,
            prefers_reduced_data: MediaReducedData::NoPreference,
        }
    }
}

impl MediaQuery {
    /// Пустой query (= `@media all`) — true. Иначе хотя бы одна
    /// OR-clause должна быть истиной; внутри clause — все AND-условия.
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        self.clauses.iter().any(|clause| clause.matches(ctx))
    }
}

impl MediaQueryClause {
    /// Per Media Queries L4 §3.2: пустая `conditions` — clause invalid
    /// (например, `@media not` без media-type / feature) → false.
    /// `Unsupported` в любом условии делает clause «unknown» → false
    /// даже под `not` (spec: «If the result is unknown, then the
    /// negation also evaluates to unknown»). При known-результате
    /// `negated` инвертирует исход AND-conjunction.
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.conditions.is_empty() {
            return false;
        }
        if self
            .conditions
            .iter()
            .any(|c| matches!(c, MediaCondition::Unsupported))
        {
            return false;
        }
        let all_match = self.conditions.iter().all(|c| c.matches(ctx));
        if self.negated { !all_match } else { all_match }
    }
}

impl MediaCondition {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        match self {
            Self::MediaType(t) => t == "all" || t == &ctx.media_type,
            Self::Feature(f) => f.matches(ctx),
            Self::Unsupported => false,
        }
    }
}

impl MediaFeature {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        match self {
            Self::Width(px) => (ctx.width - px).abs() < 0.5,
            Self::MinWidth(px) => ctx.width >= *px,
            Self::MaxWidth(px) => ctx.width <= *px,
            Self::Height(px) => (ctx.height - px).abs() < 0.5,
            Self::MinHeight(px) => ctx.height >= *px,
            Self::MaxHeight(px) => ctx.height <= *px,
            Self::AspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                (actual - ratio).abs() < 0.01
            }
            Self::MinAspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                actual >= *ratio
            }
            Self::MaxAspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { 0.0 };
                actual <= *ratio
            }
            Self::Orientation(o) => {
                let actual = if ctx.width >= ctx.height {
                    MediaOrientation::Landscape
                } else {
                    MediaOrientation::Portrait
                };
                actual == *o
            }
            Self::PrefersColorScheme(scheme) => match scheme {
                ColorScheme::Dark => ctx.prefers_dark,
                ColorScheme::Light => !ctx.prefers_dark,
            },
            Self::PrefersReducedMotion(reduce) => ctx.prefers_reduced_motion == *reduce,
            Self::ForcedColors(active) => ctx.forced_colors == *active,
            Self::Hover(h) => ctx.hover == *h,
            Self::AnyHover(h) => ctx.any_hover == *h,
            Self::Pointer(p) => ctx.pointer == *p,
            Self::AnyPointer(p) => ctx.any_pointer == *p,
            Self::PrefersContrast(c) => ctx.prefers_contrast == *c,
            Self::PrefersReducedData(d) => ctx.prefers_reduced_data == *d,
        }
    }
}

pub fn parse(input: &str) -> Stylesheet {
    Parser::new(input).parse_stylesheet()
}

/// Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
/// окружающих фигурных скобок (CSS Style Attributes §2).
/// Используется для подключения inline-стилей к каскаду в `lumen-layout`
/// со specificity (1,0,0,0) согласно CSS Cascade L4 §6.4.3.
pub fn parse_inline_style(input: &str) -> Vec<Declaration> {
    Parser::new(input).parse_declaration_block()
}

/// Парсит строку CSS selector list (через запятую) и возвращает разобранные
/// `ComplexSelector`-ы. Невалидные или неизвестные части молча пропускаются.
/// Используется lumen-layout для selector-based lookup (find_box_by_selector).
pub fn parse_selector_list(input: &str) -> Vec<ComplexSelector> {
    Parser::new(input).parse_selector_list()
}

enum AtRuleOutcome {
    Property(PropertyRule),
    Media(MediaRule),
    Import(ImportRule),
    FontFace(FontFaceRule),
    FontPaletteValues(FontPaletteValuesRule),
    LayerNames(Vec<String>),
    LayerBlock {
        name: Option<String>,
        rules: Vec<Rule>,
    },
    Supports(SupportsRule),
    Keyframes(KeyframesRule),
    CounterStyle(CounterStyleRule),
    Page(PageRule),
    Scope(ScopeRule),
    StartingStyle(StartingStyleRule),
    Container(ContainerRule),
    None,
}

/// Парсит keyframe-селектор: `from` / `to` / `<percentage>` / списки
/// через запятую (`0%, 50%`). Возвращает offset-ы в [0, 1]; невалидные
/// токены пропускаются.
fn parse_keyframe_selectors(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    for tok in s.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        if t.eq_ignore_ascii_case("from") {
            out.push(0.0);
            continue;
        }
        if t.eq_ignore_ascii_case("to") {
            out.push(1.0);
            continue;
        }
        if let Some(num_str) = t.strip_suffix('%')
            && let Ok(n) = num_str.trim().parse::<f32>()
            && n.is_finite()
            && (0.0..=100.0).contains(&n)
        {
            out.push(n / 100.0);
        }
    }
    out
}

/// Layer-имя — CSS-ident, опционально с точками (sub-layers через
/// `base.text`, CSS Cascade L5 §6.4.1). Phase 0 поддерживает простые
/// имена (без точек) и dotted-имена как одну строку, не разбивая иерархию.
fn is_layer_name(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    s.split('.').all(|part| {
        let mut chars = part.chars();
        let Some(first) = chars.next() else { return false };
        if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
            return false;
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Парсит значение `src:` из `@font-face`: comma-separated список
/// `url("path") format("fmt")` или `local("name")`. Игнорирует
/// невалидные элементы (best-effort).
fn parse_font_face_src(src: &str) -> Vec<FontFaceSource> {
    let mut out = Vec::new();
    for item in split_top_level_commas(src) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        // Найти `url(` или `local(`.
        let (kind, after) = if let Some(rest) = item.strip_prefix("url(") {
            (FontFaceSourceKind::Url, rest)
        } else if let Some(rest) = item.strip_prefix("local(") {
            (FontFaceSourceKind::Local, rest)
        } else {
            continue;
        };
        let Some(close) = after.find(')') else {
            continue;
        };
        let inner = after[..close].trim().trim_matches(['"', '\''].as_ref());
        let tail = after[close + 1..].trim();
        // Опциональный `format("...")`.
        let format = if let Some(fmt_rest) = tail.strip_prefix("format(") {
            fmt_rest
                .find(')')
                .map(|end| fmt_rest[..end].trim().trim_matches(['"', '\''].as_ref()).to_string())
        } else {
            None
        };
        out.push(FontFaceSource {
            kind,
            value: inner.to_string(),
            format,
        });
    }
    out
}

/// Делит строку по top-level запятым (игнорирует запятые внутри `(...)`
/// и строковых литералов). Используется для `src:` value
/// (`url(a), url(b) format(c)`) и подобных list-значений.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string: Option<u8> = None;
    let mut start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = in_string {
            if b == q {
                in_string = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => in_string = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        out.push(&s[start..]);
    }
    out
}

/// Парсит `@supports`-условие из строки между `@supports` и `{`.
///
/// Грамматика (упрощённая): `<expr> = <term> (("and"|"or") <term>)*`,
/// `<term> = "not"? <atom>`, `<atom> = "(" <inner> ")" | "selector(" sel ")"`,
/// `<inner> = <expr> | <prop ":" value>`.
///
/// Phase 0 ограничения:
/// - Mixing `and` и `or` на одном уровне не разрешено (per spec), но
///   парсер lenient — берёт первый встретившийся combinator и применяет
///   его ко всем term-ам этого уровня. Реалистичные tests этого не
///   нарушают (`(a) and (b) and (c)` или `(a) or (b)`); смешанные — UB.
/// - Нерекурсивный `selector(...)` хранит сырой селектор; реальный
///   match — отложенная задача.
pub fn parse_supports_condition(s: &str) -> SupportsCondition {
    let s = s.trim();
    if s.is_empty() {
        return SupportsCondition::Unknown;
    }
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    let result = parse_supports_expr(bytes, &mut pos);
    skip_ws(bytes, &mut pos);
    if pos < bytes.len() {
        // Если что-то осталось — это синтаксическая ошибка; возвращаем
        // частично разобранное (lenient).
    }
    result
}

/// Парсит значение `override-colors` из `@font-palette-values`.
/// Формат: comma-separated `<u16-index> <color-string>` пары.
/// CSS Fonts L4 §13.3. Хранит color как raw string — resolve через
/// `parse_color` выполняется в layout при использовании palette.
fn parse_override_colors(s: &str) -> Vec<(u16, String)> {
    let mut result = Vec::new();
    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, char::is_whitespace);
        if let (Some(idx_str), Some(color_str)) = (parts.next(), parts.next())
            && let Ok(idx) = idx_str.trim().parse::<u16>()
        {
            let color = color_str.trim().to_string();
            if !color.is_empty() {
                result.push((idx, color));
            }
        }
    }
    result
}

fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && b[*p].is_ascii_whitespace() {
        *p += 1;
    }
}

fn match_keyword_ci(b: &[u8], p: &mut usize, kw: &[u8]) -> bool {
    skip_ws(b, p);
    if *p + kw.len() > b.len() {
        return false;
    }
    if !b[*p..*p + kw.len()].eq_ignore_ascii_case(kw) {
        return false;
    }
    // Граница: следующий символ — не ident-char.
    let after = *p + kw.len();
    if after < b.len() {
        let c = b[after];
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
            return false;
        }
    }
    *p = after;
    true
}

fn parse_supports_expr(b: &[u8], p: &mut usize) -> SupportsCondition {
    let first = parse_supports_term(b, p);
    skip_ws(b, p);
    // Определяем combinator (если есть).
    let saved = *p;
    if match_keyword_ci(b, p, b"and") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"and") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::And(terms);
    }
    *p = saved;
    if match_keyword_ci(b, p, b"or") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"or") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::Or(terms);
    }
    first
}

fn parse_supports_term(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    if match_keyword_ci(b, p, b"not") {
        let inner = parse_supports_atom(b, p);
        return SupportsCondition::Not(Box::new(inner));
    }
    parse_supports_atom(b, p)
}

fn parse_supports_atom(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    // `selector( ... )`
    let saved = *p;
    if *p + 9 <= b.len() && b[*p..*p + 9].eq_ignore_ascii_case(b"selector(") {
        *p += 9;
        let start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let sel_str = std::str::from_utf8(&b[start..*p]).unwrap_or("").trim().to_string();
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        return SupportsCondition::Selector(sel_str);
    }
    *p = saved;
    if *p < b.len() && b[*p] == b'(' {
        *p += 1;
        // Содержимое: может быть `<expr>` (nested condition) или
        // `<prop>: <value>`. Различаем по наличию `:` на верхнем уровне.
        let inner_start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let inner = std::str::from_utf8(&b[inner_start..*p]).unwrap_or("");
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        // Determine: declaration or nested condition. Top-level `:`?
        let inner_t = inner.trim();
        let mut colon_pos: Option<usize> = None;
        let inner_bytes = inner_t.as_bytes();
        let mut d: i32 = 0;
        for (i, &c) in inner_bytes.iter().enumerate() {
            match c {
                b'(' => d += 1,
                b')' => d -= 1,
                b':' if d == 0 => {
                    colon_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(idx) = colon_pos {
            let property = inner_t[..idx].trim().to_string();
            let value = inner_t[idx + 1..].trim().to_string();
            if property.is_empty() {
                return SupportsCondition::Unknown;
            }
            return SupportsCondition::Decl { property, value };
        }
        return parse_supports_condition(inner_t);
    }
    SupportsCondition::Unknown
}

/// Распарсить media query из строки между `@media` и `{`. Принимает
/// строку без обрамляющих whitespace. Грамматика (упрощённая, Media
/// Queries L4 §3):
/// ```text
/// query-list    = query [ "," query ]*
/// query         = [ "not" | "only" ]? primary [ "and" primary ]*
/// primary       = ident | "(" feature ")"
/// ```
///
/// Возвращает `MediaQuery` с `clauses.len() == 0` если строка пустая
/// (= `@media all`). Неизвестные feature-имена дают `Unsupported` (не
/// матчат) — это lenient parser для forward-compat.
pub fn parse_media_query(s: &str) -> MediaQuery {
    let s = s.trim();
    if s.is_empty() {
        return MediaQuery::default();
    }
    let clauses = s.split(',').map(parse_media_clause).collect();
    MediaQuery { clauses }
}

fn parse_media_clause(s: &str) -> MediaQueryClause {
    let mut input = s.trim();

    // Per L4 §3.2 ведущие `not`/`only` — модификаторы query. `only`
    // используется для скрытия от L3-without-media-queries браузеров —
    // для нас семантически no-op. `not` инвертирует clause.
    let mut negated = false;
    if let Some(rest) = strip_leading_keyword(input, "not") {
        negated = true;
        input = rest;
    } else if let Some(rest) = strip_leading_keyword(input, "only") {
        input = rest;
    }

    let mut conditions = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.starts_with('(') {
            // Найти match `)`.
            if let Some(end) = input.find(')') {
                let inner = &input[1..end];
                conditions.push(parse_media_feature(inner.trim()));
                input = &input[end + 1..];
            } else {
                return MediaQueryClause {
                    negated,
                    conditions: vec![MediaCondition::Unsupported],
                };
            }
        } else {
            let end = input
                .find(|c: char| c.is_whitespace() || c == '(' || c == ',')
                .unwrap_or(input.len());
            let word = &input[..end];
            input = &input[end..];
            if word.eq_ignore_ascii_case("and") {
                continue;
            }
            // Дополнительные `not`/`only` внутри clause — синтаксически
            // невалидны (L4 разрешает их только в позиции query-prefix
            // или внутри `(not (...))`-conditions, которые мы пока не
            // парсим). Считаем clause unknown, чтобы не сматчить случайно.
            if word.eq_ignore_ascii_case("not") || word.eq_ignore_ascii_case("only") {
                return MediaQueryClause {
                    negated,
                    conditions: vec![MediaCondition::Unsupported],
                };
            }
            conditions.push(MediaCondition::MediaType(word.to_ascii_lowercase()));
        }
    }

    if conditions.is_empty() {
        // `@media not` без feature / media-type — invalid query
        // (Media Queries L4 §3.2 «not <media-query>» требует body).
        conditions.push(MediaCondition::Unsupported);
    }

    MediaQueryClause { negated, conditions }
}

/// Если строка начинается с `keyword` (ASCII case-insensitive) и за ним
/// следует whitespace или `(` — отрезает префикс и возвращает остаток.
/// Иначе возвращает `None`. Нужно, чтобы `notebook` / `only-child` не
/// принимались за keyword.
fn strip_leading_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = input.trim_start();
    let lower = trimmed.as_bytes();
    let kw = keyword.as_bytes();
    if lower.len() < kw.len() + 1 {
        return None;
    }
    if !trimmed.is_char_boundary(kw.len()) {
        return None;
    }
    if !trimmed[..kw.len()].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let next = trimmed.as_bytes()[kw.len()];
    if !(next == b' ' || next == b'\t' || next == b'\n' || next == b'\r' || next == b'(') {
        return None;
    }
    Some(&trimmed[kw.len()..])
}

/// Парсит значение длины в px: `Npx`, `Nem` (1em=16px), `Nrem` (1rem=16px).
/// Используется только для media features, где viewport context недоступен.
fn parse_media_length_px(val: &str) -> Option<f32> {
    const ROOT_EM: f32 = 16.0;
    if let Some(n) = val.strip_suffix("px") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = val.strip_suffix("rem") {
        n.trim().parse::<f32>().ok().map(|v| v * ROOT_EM)
    } else if let Some(n) = val.strip_suffix("em") {
        n.trim().parse::<f32>().ok().map(|v| v * ROOT_EM)
    } else {
        None
    }
}

/// Парсит значение aspect-ratio: `N/M` или просто `N`.
fn parse_aspect_ratio(val: &str) -> Option<f32> {
    if let Some((n, d)) = val.split_once('/') {
        let n: f32 = n.trim().parse().ok()?;
        let d: f32 = d.trim().parse().ok()?;
        if d == 0.0 { return None; }
        Some(n / d)
    } else {
        val.trim().parse::<f32>().ok()
    }
}

fn parse_media_feature(s: &str) -> MediaCondition {
    // `feature: value` или просто `feature` (boolean feature, не поддерживаем).
    let Some((key, val)) = s.split_once(':') else {
        return MediaCondition::Unsupported;
    };
    let key = key.trim().to_ascii_lowercase();
    let val = val.trim();
    match key.as_str() {
        "width" | "min-width" | "max-width" | "height" | "min-height" | "max-height" => {
            let Some(px) = parse_media_length_px(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "width" => MediaFeature::Width(px),
                "min-width" => MediaFeature::MinWidth(px),
                "max-width" => MediaFeature::MaxWidth(px),
                "height" => MediaFeature::Height(px),
                "min-height" => MediaFeature::MinHeight(px),
                "max-height" => MediaFeature::MaxHeight(px),
                _ => unreachable!(),
            };
            MediaCondition::Feature(feature)
        }
        "aspect-ratio" | "min-aspect-ratio" | "max-aspect-ratio" => {
            let Some(ratio) = parse_aspect_ratio(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "aspect-ratio" => MediaFeature::AspectRatio(ratio),
                "min-aspect-ratio" => MediaFeature::MinAspectRatio(ratio),
                "max-aspect-ratio" => MediaFeature::MaxAspectRatio(ratio),
                _ => unreachable!(),
            };
            MediaCondition::Feature(feature)
        }
        "orientation" => match val.to_ascii_lowercase().as_str() {
            "portrait" => MediaCondition::Feature(MediaFeature::Orientation(MediaOrientation::Portrait)),
            "landscape" => MediaCondition::Feature(MediaFeature::Orientation(MediaOrientation::Landscape)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-color-scheme" => match val.to_ascii_lowercase().as_str() {
            "light" => MediaCondition::Feature(MediaFeature::PrefersColorScheme(ColorScheme::Light)),
            "dark" => MediaCondition::Feature(MediaFeature::PrefersColorScheme(ColorScheme::Dark)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-reduced-motion" => match val.to_ascii_lowercase().as_str() {
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedMotion(true)),
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedMotion(false)),
            _ => MediaCondition::Unsupported,
        },
        "forced-colors" => match val.to_ascii_lowercase().as_str() {
            "active" => MediaCondition::Feature(MediaFeature::ForcedColors(true)),
            "none" => MediaCondition::Feature(MediaFeature::ForcedColors(false)),
            _ => MediaCondition::Unsupported,
        },
        "hover" | "any-hover" => {
            let h = match val.to_ascii_lowercase().as_str() {
                "none" => MediaHover::None,
                "hover" => MediaHover::Hover,
                _ => return MediaCondition::Unsupported,
            };
            MediaCondition::Feature(if key == "hover" {
                MediaFeature::Hover(h)
            } else {
                MediaFeature::AnyHover(h)
            })
        }
        "pointer" | "any-pointer" => {
            let p = match val.to_ascii_lowercase().as_str() {
                "none" => MediaPointer::None,
                "coarse" => MediaPointer::Coarse,
                "fine" => MediaPointer::Fine,
                _ => return MediaCondition::Unsupported,
            };
            MediaCondition::Feature(if key == "pointer" {
                MediaFeature::Pointer(p)
            } else {
                MediaFeature::AnyPointer(p)
            })
        }
        "prefers-contrast" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::NoPreference)),
            "more" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::More)),
            "less" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::Less)),
            "custom" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::Custom)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-reduced-data" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedData(MediaReducedData::NoPreference)),
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedData(MediaReducedData::Reduce)),
            _ => MediaCondition::Unsupported,
        },
        _ => MediaCondition::Unsupported,
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn consume(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.consume();
                } else {
                    break;
                }
            }
            if self.rest().starts_with("/*") {
                self.pos += 2;
                while !self.rest().starts_with("*/") && self.pos < self.input.len() {
                    self.consume();
                }
                if self.rest().starts_with("*/") {
                    self.pos += 2;
                }
            } else {
                break;
            }
        }
    }

    /// Возвращает true, если был whitespace или comment, и продвигает позицию.
    fn skip_ws_and_comments_track(&mut self) -> bool {
        let start = self.pos;
        self.skip_ws_and_comments();
        self.pos != start
    }

    fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        let mut properties = Vec::new();
        let mut media_rules = Vec::new();
        let mut imports = Vec::new();
        let mut font_faces = Vec::new();
        let mut font_palette_values: Vec<FontPaletteValuesRule> = Vec::new();
        let mut layer_order: Vec<String> = Vec::new();
        let mut layers: Vec<LayerRule> = Vec::new();
        let mut supports_rules: Vec<SupportsRule> = Vec::new();
        let mut keyframes: Vec<KeyframesRule> = Vec::new();
        let mut counter_styles: Vec<CounterStyleRule> = Vec::new();
        let mut page_rules: Vec<PageRule> = Vec::new();
        let mut scope_rules: Vec<ScopeRule> = Vec::new();
        let mut starting_style_rules: Vec<StartingStyleRule> = Vec::new();
        let mut container_rules: Vec<ContainerRule> = Vec::new();
        let mut anon_counter: usize = 0;
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('@') => match self.parse_at_rule() {
                    AtRuleOutcome::Property(p) => properties.push(p),
                    AtRuleOutcome::Media(m) => media_rules.push(m),
                    AtRuleOutcome::Import(i) => imports.push(i),
                    AtRuleOutcome::FontFace(f) => font_faces.push(f),
                    AtRuleOutcome::FontPaletteValues(fp) => font_palette_values.push(fp),
                    AtRuleOutcome::LayerNames(names) => {
                        for n in names {
                            if !layer_order.iter().any(|e| e == &n) {
                                layer_order.push(n);
                            }
                        }
                    }
                    AtRuleOutcome::LayerBlock { name, rules: lr } => {
                        let resolved_name = name.unwrap_or_else(|| {
                            anon_counter += 1;
                            format!("__anon_{anon_counter}__")
                        });
                        if !layer_order.iter().any(|e| e == &resolved_name) {
                            layer_order.push(resolved_name.clone());
                        }
                        layers.push(LayerRule {
                            name: resolved_name,
                            rules: lr,
                        });
                    }
                    AtRuleOutcome::Supports(s) => supports_rules.push(s),
                    AtRuleOutcome::Keyframes(k) => keyframes.push(k),
                    AtRuleOutcome::CounterStyle(c) => counter_styles.push(c),
                    AtRuleOutcome::Page(p) => page_rules.push(p),
                    AtRuleOutcome::Scope(s) => scope_rules.push(s),
                    AtRuleOutcome::StartingStyle(s) => starting_style_rules.push(s),
                    AtRuleOutcome::Container(c) => container_rules.push(c),
                    AtRuleOutcome::None => {}
                },
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested); // CSS Nesting L1: flat-expanded nested rules
                        // CSS Nesting L1 §5: nested at-rules bubble up into the stylesheet.
                        for at in nested_at {
                            match at {
                                AtRuleOutcome::Media(m) => media_rules.push(m),
                                AtRuleOutcome::Supports(s) => supports_rules.push(s),
                                AtRuleOutcome::LayerNames(names) => {
                                    for n in names {
                                        if !layer_order.iter().any(|e| e == &n) {
                                            layer_order.push(n);
                                        }
                                    }
                                }
                                AtRuleOutcome::LayerBlock { name, rules: lr } => {
                                    let resolved = name.unwrap_or_else(|| {
                                        anon_counter += 1;
                                        format!("__anon_{anon_counter}__")
                                    });
                                    if !layer_order.iter().any(|e| e == &resolved) {
                                        layer_order.push(resolved.clone());
                                    }
                                    layers.push(LayerRule { name: resolved, rules: lr });
                                }
                                AtRuleOutcome::Container(c) => container_rules.push(c),
                                _ => {}
                            }
                        }
                    } else if self.pos == before {
                        // Защита от бесконечного цикла: parse_rule не сдвинул
                        // позицию — принудительно проглатываем один символ.
                        self.consume();
                    }
                }
            }
        }
        Stylesheet {
            rules,
            properties,
            media_rules,
            imports,
            font_faces,
            font_palette_values,
            layer_order,
            layers,
            supports_rules,
            keyframes,
            counter_styles,
            page_rules,
            scope_rules,
            starting_style_rules,
            container_rules,
        }
    }

    /// Распознаёт `@property --name { ... }` (CSS Properties and Values L1
    /// §1.1) и `@media <query> { <rules> }` (Media Queries L4).
    /// Все прочие @-правила синтаксически пропускает. Сама съедает
    /// либо `;`, либо полный `{ ... }`-блок.
    fn parse_at_rule(&mut self) -> AtRuleOutcome {
        let start = self.pos;
        self.consume(); // '@'
        let name = self.parse_ident().unwrap_or_default();
        if name.eq_ignore_ascii_case("property") {
            return self.parse_property_body().map_or(AtRuleOutcome::None, AtRuleOutcome::Property);
        }
        if name.eq_ignore_ascii_case("media") {
            return self.parse_media_rule().map_or(AtRuleOutcome::None, AtRuleOutcome::Media);
        }
        if name.eq_ignore_ascii_case("import") {
            return self.parse_import_body().map_or(AtRuleOutcome::None, AtRuleOutcome::Import);
        }
        if name.eq_ignore_ascii_case("font-face") {
            return self
                .parse_font_face_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::FontFace);
        }
        if name.eq_ignore_ascii_case("font-palette-values") {
            return self
                .parse_font_palette_values_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::FontPaletteValues);
        }
        if name.eq_ignore_ascii_case("layer") {
            return self.parse_layer_at_rule();
        }
        if name.eq_ignore_ascii_case("supports") {
            return self
                .parse_supports_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Supports);
        }
        if name.eq_ignore_ascii_case("keyframes")
            || name.eq_ignore_ascii_case("-webkit-keyframes")
        {
            return self
                .parse_keyframes_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Keyframes);
        }
        if name.eq_ignore_ascii_case("counter-style") {
            return self
                .parse_counter_style_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::CounterStyle);
        }
        if name.eq_ignore_ascii_case("page") {
            return self
                .parse_page_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Page);
        }
        if name.eq_ignore_ascii_case("scope") {
            return self
                .parse_scope_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Scope);
        }
        if name.eq_ignore_ascii_case("starting-style") {
            return self
                .parse_starting_style_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::StartingStyle);
        }
        if name.eq_ignore_ascii_case("container") {
            return self
                .parse_container_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Container);
        }
        // Прочее @-правило: откатимся к '@' и пропустим как раньше.
        self.pos = start;
        self.skip_at_rule();
        AtRuleOutcome::None
    }

    /// Парсит `@layer` — две формы:
    /// - **Statement-form**: `@layer base, components;` — список имён,
    ///   закрывается `;`. Регистрирует layer-имена без rules.
    /// - **Block-form**: `@layer name { rules }` или `@layer { rules }`
    ///   (анонимный). Содержит обычные rules внутри. Имя опционально.
    ///
    /// Различие — что встречается раньше: `;` (statement) или `{` (block).
    fn parse_layer_at_rule(&mut self) -> AtRuleOutcome {
        self.skip_ws_and_comments();
        // Собираем токены имени до `;` или `{`.
        let names_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ';' || c == '{' || c == '}' {
                break;
            }
            self.consume();
        }
        let prelude = self.input[names_start..self.pos].trim();
        match self.peek() {
            Some(';') => {
                self.consume();
                // Statement-form: список имён через запятую.
                let names: Vec<String> = prelude
                    .split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty() && is_layer_name(n))
                    .collect();
                AtRuleOutcome::LayerNames(names)
            }
            Some('{') => {
                self.consume();
                // Block-form: name опционально (может быть пустым для anon),
                // парсим rules до `}`.
                let name = if prelude.is_empty() {
                    None
                } else if is_layer_name(prelude) {
                    Some(prelude.to_string())
                } else {
                    // Невалидное имя (например, со скобками или невалидными
                    // символами) — пропустим как анонимный.
                    None
                };
                let mut rules = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    match self.peek() {
                        None => break,
                        Some('}') => {
                            self.consume();
                            break;
                        }
                        Some('@') => {
                            // Nested @-правила внутри layer пока не
                            // поддерживаем — skip.
                            self.skip_at_rule();
                        }
                        Some(_) => {
                            let before = self.pos;
                            if let Some((rule, nested, _)) = self.parse_rule() {
                                rules.push(rule);
                                rules.extend(nested);
                            } else if self.pos == before {
                                self.consume();
                            }
                        }
                    }
                }
                AtRuleOutcome::LayerBlock { name, rules }
            }
            _ => AtRuleOutcome::None,
        }
    }

    /// Парсит тело `@font-face { ... }` — обычный block declarations,
    /// но с font-face-specific descriptors (font-family / src / weight /
    /// style / stretch / display / unicode-range / variant /
    /// feature-settings / variation-settings). Прочие имена игнорируются.
    fn parse_font_face_body(&mut self) -> Option<FontFaceRule> {
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();

        let mut family: String = String::new();
        let mut src_str: Option<String> = None;
        let mut weight: Option<String> = None;
        let mut style: Option<String> = None;
        let mut stretch: Option<String> = None;
        let mut display: Option<String> = None;
        let mut unicode_range: Option<String> = None;
        let mut variant: Option<String> = None;
        let mut feature_settings: Option<String> = None;
        let mut variation_settings: Option<String> = None;

        for d in &declarations {
            let prop = d.property.to_ascii_lowercase();
            match prop.as_str() {
                "font-family" => {
                    let v = d.value.trim();
                    family = strip_css_string(v).map_or_else(|| v.to_string(), str::to_string);
                }
                "src" => src_str = Some(d.value.clone()),
                "font-weight" => weight = Some(d.value.trim().to_string()),
                "font-style" => style = Some(d.value.trim().to_string()),
                "font-stretch" => stretch = Some(d.value.trim().to_string()),
                "font-display" => display = Some(d.value.trim().to_string()),
                "unicode-range" => unicode_range = Some(d.value.trim().to_string()),
                "font-variant" => variant = Some(d.value.trim().to_string()),
                "font-feature-settings" => feature_settings = Some(d.value.trim().to_string()),
                "font-variation-settings" => variation_settings = Some(d.value.trim().to_string()),
                _ => {}
            }
        }
        if family.is_empty() {
            return None;
        }
        let sources = src_str.as_deref().map(parse_font_face_src).unwrap_or_default();
        Some(FontFaceRule {
            family,
            sources,
            weight,
            style,
            stretch,
            display,
            unicode_range,
            variant,
            feature_settings,
            variation_settings,
        })
    }

    /// Парсит `@font-palette-values --name { font-family: …; base-palette: N; override-colors: … }`.
    /// CSS Fonts L4 §13. Prelude — dashed-ident (e.g. `--cool`). Block contains
    /// descriptors: `font-family`, `base-palette` (u16 index), `override-colors`
    /// (comma-separated `<index> <color>` pairs). Returns `None` if the
    /// name is missing or no `{` follows.
    fn parse_font_palette_values_body(&mut self) -> Option<FontPaletteValuesRule> {
        self.skip_ws_and_comments();
        // Prelude: dashed-ident starting with '--'
        let name = self.parse_ident()?;
        if !name.starts_with("--") {
            self.skip_until_block_end();
            return None;
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();

        let mut font_family: Option<String> = None;
        let mut base_palette: Option<u16> = None;
        let mut override_colors: Vec<(u16, String)> = Vec::new();

        for d in &declarations {
            match d.property.to_ascii_lowercase().as_str() {
                "font-family" => {
                    let v = d.value.trim();
                    font_family =
                        Some(strip_css_string(v).map_or_else(|| v.to_string(), str::to_string));
                }
                "base-palette" => {
                    base_palette = d.value.trim().parse::<u16>().ok();
                }
                "override-colors" => {
                    override_colors = parse_override_colors(d.value.trim());
                }
                _ => {}
            }
        }
        Some(FontPaletteValuesRule {
            name,
            font_family,
            base_palette,
            override_colors,
        })
    }

    /// Парсит тело `@import url("...") [<media-query>];` или
    /// `@import "..." [<media-query>];`. Заканчивается на `;` (имеет
    /// statement-form, не блочную). Возвращает None если синтаксис
    /// нарушен; в любом случае съедает до `;` (или EOF).
    fn parse_import_body(&mut self) -> Option<ImportRule> {
        self.skip_ws_and_comments();
        // URL: либо `url("...")` / `url('...')` / `url(...)`, либо просто `"..."` / `'...'`.
        let url = self.parse_import_url()?;
        self.skip_ws_and_comments();
        // Опциональный media-query до `;`.
        let media_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ';' || c == '}' || c == '{' {
                break;
            }
            self.consume();
        }
        let media_str = self.input[media_start..self.pos].trim();
        let media = parse_media_query(media_str);
        // Сжираем `;` если есть.
        if self.peek() == Some(';') {
            self.consume();
        }
        Some(ImportRule { url, media })
    }

    /// Парсит URL для `@import` — `url("...")`, `url(...)`, или `"..."`/`'...'`.
    /// Позиция после успешного парсинга стоит ПОСЛЕ закрывающей кавычки/скобки.
    fn parse_import_url(&mut self) -> Option<String> {
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix("url(") {
            // Внутри parentheses: опц. quoted-string или unquoted-URL.
            let close_idx = after.find(')')?;
            let inner = &after[..close_idx];
            let url = inner.trim().trim_matches(['"', '\''].as_ref()).to_string();
            self.pos += 4 + close_idx + 1;
            return Some(url);
        }
        // Plain string без url().
        match self.peek()? {
            '"' | '\'' => {
                let quote = self.consume()?;
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == quote {
                        break;
                    }
                    self.consume();
                }
                if self.peek() != Some(quote) {
                    return None;
                }
                let url = self.input[start..self.pos].to_string();
                self.consume();
                Some(url)
            }
            _ => None,
        }
    }

    /// Парсит тело `@media <query> { <rules> }`. Грамматика query
    /// упрощённая: type-or-feature [and type-or-feature]* [, ...].
    /// Type-or-feature — ident (`screen`/`print`/...) или
    /// `(feature: value)`. Возвращает None если синтаксис не позволяет
    /// дойти до `{`; в этом случае откатывает позицию до конца блока
    /// чтобы стабильно продолжить парсинг stylesheet.
    fn parse_media_rule(&mut self) -> Option<MediaRule> {
        self.skip_ws_and_comments();
        // Собираем query-string до `{`.
        let query_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '{' {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let query_str = self.input[query_start..self.pos].trim();
        let query = parse_media_query(query_str);
        // Тело: рекурсивно парсим как обычные rules.
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила в media пока не поддерживаем — skip.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(MediaRule { query, rules })
    }

    /// Парсит тело `@supports <condition> { rules }` — CSS Conditional Rules L3 §2.
    /// Берёт сырую condition-строку до `{` (с балансировкой `(`/`)`),
    /// затем парсит её через [`parse_supports_condition`]. Тело — обычные
    /// rules до `}`. Возвращает `None` если структура нарушена.
    fn parse_supports_rule(&mut self) -> Option<SupportsRule> {
        self.skip_ws_and_comments();
        let cond_start = self.pos;
        let mut depth: i32 = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if c == '{' && depth == 0 {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let cond_str = self.input[cond_start..self.pos].trim();
        let condition = parse_supports_condition(cond_str);
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила внутри @supports пока skip.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(SupportsRule { condition, rules })
    }

    /// Парсит тело `@keyframes <name> { <frame>* }` — CSS Animations L1 §3.
    /// Frame-selector: `from` / `to` / `<percentage>`. Поддерживается
    /// `0%, 50% { ... }` (одна frame с несколькими offset-ами,
    /// разворачивается в две записи). `name` — CSS-ident.
    fn parse_keyframes_rule(&mut self) -> Option<KeyframesRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let mut frames: Vec<Keyframe> = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила внутри @keyframes по spec не разрешены.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    let frame_selector_start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == '{' || c == '}' {
                            break;
                        }
                        self.consume();
                    }
                    if self.peek() != Some('{') {
                        if self.pos == before {
                            self.consume();
                        }
                        continue;
                    }
                    let selector_str = self.input[frame_selector_start..self.pos].trim();
                    self.consume(); // '{'
                    let declarations = self.parse_declaration_block();
                    let offsets = parse_keyframe_selectors(selector_str);
                    for offset in offsets {
                        frames.push(Keyframe {
                            offset,
                            declarations: declarations.clone(),
                        });
                    }
                }
            }
        }
        Some(KeyframesRule { name, frames })
    }

    /// Парсит `@counter-style <name> { <descriptors> }` — CSS Counter Styles L3 §2.
    /// Descriptors хранятся как обычные declarations.
    fn parse_counter_style_rule(&mut self) -> Option<CounterStyleRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();
        Some(CounterStyleRule { name, declarations })
    }

    /// Парсит `@page <selector>? { <decls> }` — CSS Paged Media L3 §3.
    /// Selector сохраняется как сырая строка (`:first`, `:left`, имя
    /// страницы, и т.д.). Пустой selector — любая страница.
    fn parse_page_rule(&mut self) -> Option<PageRule> {
        self.skip_ws_and_comments();
        let sel_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '{' || c == ';' {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            // `@page <prelude>;` без блока — не валидно для CSS Paged Media.
            if self.peek() == Some(';') {
                self.consume();
            }
            return None;
        }
        let selector = self.input[sel_start..self.pos].trim().to_string();
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(PageRule {
            selector,
            declarations,
        })
    }

    /// Парсит `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6.
    /// Root и limit — сырые строки селекторов (без обрамляющих `(`/`)`).
    /// Без `(<root>)` — implicit scope (root = пустая строка).
    fn parse_scope_rule(&mut self) -> Option<ScopeRule> {
        self.skip_ws_and_comments();
        let mut root = String::new();
        let mut limit: Option<String> = None;
        // Опциональный `(<root>)`.
        if self.peek() == Some('(') {
            self.consume();
            let start = self.pos;
            let mut depth: i32 = 1;
            while let Some(c) = self.peek() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                self.consume();
            }
            root = self.input[start..self.pos].trim().to_string();
            if self.peek() == Some(')') {
                self.consume();
            }
        }
        self.skip_ws_and_comments();
        // Опциональный `to (<limit>)`.
        if self.rest().to_ascii_lowercase().starts_with("to") {
            // Граница: следующий после `to` — не ident-char.
            let after = self.pos + 2;
            let ok = self.input.as_bytes().get(after).is_none_or(|&c| {
                !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            });
            if ok {
                self.pos = after;
                self.skip_ws_and_comments();
                if self.peek() == Some('(') {
                    self.consume();
                    let start = self.pos;
                    let mut depth: i32 = 1;
                    while let Some(c) = self.peek() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.consume();
                    }
                    limit = Some(self.input[start..self.pos].trim().to_string());
                    if self.peek() == Some(')') {
                        self.consume();
                    }
                }
            }
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            return None;
        }
        self.consume();
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(ScopeRule {
            root,
            limit,
            rules,
        })
    }

    /// Парсит `@container <name>? <condition> { rules }` — CSS Containment L3 §3.
    /// Name — опциональный CSS-ident перед условием. Condition — балансированная
    /// строка до `{` (хранится сырой). Rules — обычные правила внутри.
    fn parse_container_rule(&mut self) -> Option<ContainerRule> {
        self.skip_ws_and_comments();
        // Опциональное имя: CSS-ident **только если** дальше не `(` —
        // если сразу `(`, это начало condition без имени.
        let name = if self.peek() != Some('(') && !self.starts_with_keyword("style") {
            self.parse_ident()
        } else {
            None
        };
        self.skip_ws_and_comments();
        // Condition: всё до `{` с учётом баланса `()`.
        let cond_start = self.pos;
        let mut depth: i32 = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if c == '{' && depth == 0 {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let condition = self.input[cond_start..self.pos].trim().to_string();
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(ContainerRule {
            name,
            condition,
            rules,
        })
    }

    /// Проверяет, начинается ли остаток с ключевого слова (case-insensitive)
    /// + не-ident разделитель. Используется для container `style(...)`.
    fn starts_with_keyword(&self, kw: &str) -> bool {
        let rest = self.rest();
        if !rest.to_ascii_lowercase().starts_with(kw) {
            return false;
        }
        rest.as_bytes()
            .get(kw.len())
            .is_none_or(|&c| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    }

    /// Парсит `@starting-style { rules }` — CSS Transitions L2 §3.4.
    fn parse_starting_style_rule(&mut self) -> Option<StartingStyleRule> {
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(StartingStyleRule { rules })
    }

    /// Парсит тело `@property`: имя `--name`, блок `{ ... }`, обязательные
    /// дескрипторы. Возвращает None если синтаксис нарушен или нет
    /// обязательных полей. В любом исходе позиция остаётся после `}`
    /// (или после `;` если блока не было, или EOF).
    fn parse_property_body(&mut self) -> Option<PropertyRule> {
        self.skip_ws_and_comments();
        // Имя должно начинаться с `--`.
        if !self.rest().starts_with("--") {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        self.consume();
        let tail = self.parse_ident().unwrap_or_default();
        if tail.is_empty() {
            self.skip_until_block_end();
            return None;
        }
        let name = format!("--{tail}");
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();

        // Извлекаем три обязательных дескриптора. Любые другие имена в теле
        // @property спецификацией не определены; их игнорируем (forward-compat).
        let mut syntax: Option<String> = None;
        let mut inherits: Option<bool> = None;
        let mut initial_value: Option<String> = None;
        for d in &declarations {
            let prop = d.property.to_ascii_lowercase();
            match prop.as_str() {
                "syntax" => {
                    // value — CSS-string в одиночных или двойных кавычках.
                    if let Some(stripped) = strip_css_string(d.value.trim()) {
                        syntax = Some(stripped.to_string());
                    }
                }
                "inherits" => {
                    let v = d.value.trim().to_ascii_lowercase();
                    if v == "true" {
                        inherits = Some(true);
                    } else if v == "false" {
                        inherits = Some(false);
                    }
                }
                "initial-value" => {
                    initial_value = Some(d.value.trim().to_string());
                }
                _ => {}
            }
        }

        let syntax = syntax?;
        let inherits = inherits?;
        // CSS Properties and Values L1 §1.1: если syntax не universal,
        // initial-value обязателен. В Phase 0 поддерживаем только syntax="*",
        // но валидируем по спеке — чужой syntax без initial-value invalid.
        if syntax != "*" && initial_value.is_none() {
            return None;
        }
        Some(PropertyRule {
            name,
            syntax,
            inherits,
            initial_value,
        })
    }

    /// Пропускает до конца `@-rule`-тела: либо `;`, либо `{ ... }` целиком.
    /// Используется при синтаксической ошибке внутри @property — потребитель
    /// не должен ловить declarations этого правила.
    fn skip_until_block_end(&mut self) {
        while let Some(c) = self.peek() {
            if c == '{' {
                self.consume();
                self.skip_block();
                return;
            }
            if c == ';' {
                self.consume();
                return;
            }
            self.consume();
        }
    }

    fn skip_at_rule(&mut self) {
        self.consume(); // '@'
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn skip_block(&mut self) {
        let mut depth = 1;
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    depth += 1;
                }
                '}' => {
                    self.consume();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_rule(&mut self) -> Option<(Rule, Vec<Rule>, Vec<AtRuleOutcome>)> {
        let start = self.pos;
        let selectors = self.parse_selector_list();
        self.skip_ws_and_comments();
        if selectors.is_empty() || self.peek() != Some('{') {
            if self.pos == start {
                self.consume();
            }
            self.recover_to_block_end();
            return None;
        }
        self.consume(); // '{'
        let (declarations, nested, at_rules) =
            self.parse_declaration_block_with_nesting(&selectors);
        Some((Rule { selectors, declarations }, nested, at_rules))
    }

    /// CSS Nesting L1 §3–§5 — parse declaration block that may contain nested rules and at-rules.
    /// Returns (declarations, flattened nested rules, nested at-rules).
    ///
    /// Handles:
    /// - `& selector { }` — explicit nesting with `&`
    /// - `.child { }`, `#id { }`, `[attr] { }`, `:hover { }`, `* { }` — implicit descendant nesting
    /// - `> .child { }`, `+ .sib { }`, `~ .sib { }` — implicit relative-combinator nesting
    /// - `@media / @supports / @layer / @container { }` — nested at-rules
    fn parse_declaration_block_with_nesting(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Declaration>, Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut decls = Vec::new();
        let mut nested: Vec<Rule> = Vec::new();
        let mut at_rules: Vec<AtRuleOutcome> = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                    continue;
                }
                Some('&') => {
                    // Explicit nesting with `&`.
                    let (r, a) = self.parse_nested_rule_amp(parent_sels);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit descendant — `.foo {}`, `#id {}`, `[attr] {}`,
                // `:pseudo {}`, `* {}` cannot start a property name, so treat as nested rule.
                Some('.') | Some('#') | Some('[') | Some(':') | Some('*') => {
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, None);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit relative — `> .foo {}`, `+ .sib {}`, `~ .sib {}`.
                Some('>') | Some('+') | Some('~') => {
                    // SAFETY: we just peeked this char, consume() cannot return None here.
                    let c = self.consume().unwrap_or('>');
                    let comb = match c {
                        '+' => Combinator::NextSibling,
                        '~' => Combinator::LaterSibling,
                        _ => Combinator::Child, // '>'
                    };
                    self.skip_ws_and_comments();
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, Some(comb));
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §5: nested at-rule.
                Some('@') => {
                    let ats = self.parse_nested_at_rule(parent_sels);
                    at_rules.extend(ats);
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        (decls, nested, at_rules)
    }

    /// Parse `& [combinator] selector-list { declarations }` and expand into flat rules.
    /// The `&` has already been peeked but not consumed.
    fn parse_nested_rule_amp(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        self.consume(); // consume '&'
        let had_ws = self.skip_ws_and_comments_track();
        // Determine if there's an explicit combinator after &.
        let combinator: Option<Combinator> = match self.peek() {
            Some('>') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::Child) }
            Some('+') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::NextSibling) }
            Some('~') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::LaterSibling) }
            Some('{') => None, // bare `& { }` — same element as parent
            _ if had_ws => Some(Combinator::Descendant),
            _ => None, // `&.class` / `&[attr]` / `&#id` — compound join
        };
        // Parse the selector list that follows (may be empty for bare `& { }`).
        let nested_sels: Vec<ComplexSelector> = if self.peek() == Some('{') {
            vec![] // bare `& { }` — same element
        } else {
            let s = self.parse_selector_list();
            if s.is_empty() {
                self.recover_to_block_end();
                return (vec![], vec![]);
            }
            s
        };
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Expand: combine each parent selector with each nested selector.
        let expanded_sels = if nested_sels.is_empty() {
            parent_sels.to_vec() // bare `& { }` = same as parent
        } else {
            expand_nesting(parent_sels, combinator, &nested_sels)
        };
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut result = vec![Rule { selectors: expanded_sels, declarations }];
        result.extend(sub_nested);
        (result, sub_at)
    }

    /// CSS Nesting L1 §4: implicit nesting — `.child { }` inside a rule block
    /// is treated as `& .child { }` (descendant). Called when we see a selector-
    /// start token (`.`, `#`, `[`, `:`, `*`) without an explicit `&`.
    /// `combinator` — pre-parsed explicit combinator (`>`, `+`, `~`), or `None`
    /// for implicit descendant.
    fn parse_implicit_nested_rule(
        &mut self,
        parent_sels: &[ComplexSelector],
        combinator: Option<Combinator>,
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        let nested_sels = self.parse_selector_list();
        if nested_sels.is_empty() {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Implicit nesting without explicit combinator → descendant.
        let comb = combinator.unwrap_or(Combinator::Descendant);
        let expanded_sels = expand_nesting(parent_sels, Some(comb), &nested_sels);
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut rules = vec![Rule { selectors: expanded_sels, declarations }];
        rules.extend(sub_nested);
        (rules, sub_at)
    }

    /// CSS Nesting L1 §5: nested at-rule inside a qualified rule.
    /// Example: `.parent { @media (min-width: 800px) { color: red; } }`
    /// expands to: `@media (min-width: 800px) { .parent { color: red; } }`.
    /// Supports `@media`, `@supports`, `@layer`, `@container`.
    fn parse_nested_at_rule(&mut self, parent_sels: &[ComplexSelector]) -> Vec<AtRuleOutcome> {
        let start = self.pos;
        self.consume(); // '@'
        let name = self.parse_ident().unwrap_or_default();
        self.skip_ws_and_comments();

        if name.eq_ignore_ascii_case("media") {
            let query_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' {
                    break;
                }
                self.consume();
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let query_str = self.input[query_start..self.pos].trim();
            let query = parse_media_query(query_str);
            self.consume(); // '{'
            let (decls, inner_rules, inner_at) =
                self.parse_declaration_block_with_nesting(parent_sels);
            let mut rules = Vec::new();
            if !decls.is_empty() {
                rules.push(Rule { selectors: parent_sels.to_vec(), declarations: decls });
            }
            rules.extend(inner_rules);
            let mut outcomes = vec![AtRuleOutcome::Media(MediaRule { query, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("supports") {
            let cond_start = self.pos;
            let mut depth: i32 = 0;
            while let Some(c) = self.peek() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                } else if c == '{' && depth == 0 {
                    break;
                }
                self.consume();
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let cond_str = self.input[cond_start..self.pos].trim();
            let condition = parse_supports_condition(cond_str);
            self.consume(); // '{'
            let (decls, inner_rules, inner_at) =
                self.parse_declaration_block_with_nesting(parent_sels);
            let mut rules = Vec::new();
            if !decls.is_empty() {
                rules.push(Rule { selectors: parent_sels.to_vec(), declarations: decls });
            }
            rules.extend(inner_rules);
            let mut outcomes =
                vec![AtRuleOutcome::Supports(SupportsRule { condition, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("layer") {
            let names_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' || c == ';' {
                    break;
                }
                self.consume();
            }
            let prelude = self.input[names_start..self.pos].trim();
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let layer_name = if prelude.is_empty() {
                None
            } else {
                Some(prelude.to_string())
            };
            self.consume(); // '{'
            let (decls, inner_rules, inner_at) =
                self.parse_declaration_block_with_nesting(parent_sels);
            let mut rules = Vec::new();
            if !decls.is_empty() {
                rules.push(Rule { selectors: parent_sels.to_vec(), declarations: decls });
            }
            rules.extend(inner_rules);
            let mut outcomes =
                vec![AtRuleOutcome::LayerBlock { name: layer_name, rules }];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("container") {
            let cond_start = self.pos;
            let mut depth: i32 = 0;
            while let Some(c) = self.peek() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                } else if c == '{' && depth == 0 {
                    break;
                }
                self.consume();
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let condition = self.input[cond_start..self.pos].trim().to_string();
            self.consume(); // '{'
            let (decls, inner_rules, inner_at) =
                self.parse_declaration_block_with_nesting(parent_sels);
            let mut rules = Vec::new();
            if !decls.is_empty() {
                rules.push(Rule { selectors: parent_sels.to_vec(), declarations: decls });
            }
            rules.extend(inner_rules);
            let mut outcomes = vec![AtRuleOutcome::Container(ContainerRule {
                name: None,
                condition,
                rules,
            })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        // Unknown nested at-rule — skip the block.
        self.pos = start;
        self.skip_at_rule();
        vec![]
    }

    fn recover_to_block_end(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                ';' => {
                    self.consume();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_selector_list(&mut self) -> Vec<ComplexSelector> {
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

    fn parse_complex_selector(&mut self) -> Option<ComplexSelector> {
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

    fn parse_compound_selector(&mut self) -> Option<CompoundSelector> {
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

    fn parse_simple_selector(&mut self) -> Option<SimpleSelector> {
        match self.peek()? {
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
            '[' => self.parse_attr_selector(),
            ':' => self.parse_pseudo(),
            c if is_ident_start(c) => Some(SimpleSelector::Type(self.parse_ident()?)),
            _ => None,
        }
    }

    fn parse_attr_selector(&mut self) -> Option<SimpleSelector> {
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

    fn parse_attr_value(&mut self) -> Option<String> {
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

    fn recover_to_attr_end(&mut self) {
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

    fn parse_pseudo(&mut self) -> Option<SimpleSelector> {
        self.consume(); // ':'
        let is_element = if self.peek() == Some(':') {
            self.consume();
            true
        } else {
            false
        };
        let name = self.parse_ident()?;
        let lower = name.to_ascii_lowercase();
        if self.peek() == Some('(') {
            self.consume();
            if is_element {
                // Функциональный pseudo-element (например ::slotted(...))
                let pe = self.parse_functional_pseudo_element(&lower);
                self.skip_to_paren_close();
                return Some(SimpleSelector::PseudoElement(pe.unwrap_or(
                    PseudoElementKind::Unknown(name)
                )));
            } else {
                // Функциональный pseudo-class (например :is(...))
                let pc = self.parse_functional_pseudo_body(&lower);
                self.skip_to_paren_close();
                return Some(SimpleSelector::PseudoClass(pc.unwrap_or_else(|| {
                    PseudoClass::Unsupported(name.clone())
                })));
            }
        }
        if is_element {
            let pe = match lower.as_str() {
                "before" => PseudoElementKind::Before,
                "after" => PseudoElementKind::After,
                "first-line" => PseudoElementKind::FirstLine,
                "first-letter" => PseudoElementKind::FirstLetter,
                "marker" => PseudoElementKind::Marker,
                "selection" => PseudoElementKind::Selection,
                _ => PseudoElementKind::Unknown(name),
            };
            return Some(SimpleSelector::PseudoElement(pe));
        }
        let pc = match lower.as_str() {
            "first-child" => PseudoClass::FirstChild,
            "last-child" => PseudoClass::LastChild,
            "only-child" => PseudoClass::OnlyChild,
            "empty" => PseudoClass::Empty,
            "root" => PseudoClass::Root,
            "first-of-type" => PseudoClass::FirstOfType,
            "last-of-type" => PseudoClass::LastOfType,
            "only-of-type" => PseudoClass::OnlyOfType,
            "placeholder-shown" => PseudoClass::PlaceholderShown,
            "required" => PseudoClass::Required,
            "optional" => PseudoClass::Optional,
            "read-only" => PseudoClass::ReadOnly,
            "read-write" => PseudoClass::ReadWrite,
            "disabled" => PseudoClass::Disabled,
            "enabled" => PseudoClass::Enabled,
            "checked" => PseudoClass::Checked,
            "indeterminate" => PseudoClass::Indeterminate,
            "default" => PseudoClass::Default,
            "hover" => PseudoClass::Hover,
            "focus" => PseudoClass::Focus,
            "active" => PseudoClass::Active,
            "focus-within" => PseudoClass::FocusWithin,
            "focus-visible" => PseudoClass::FocusVisible,
            "link" => PseudoClass::Link,
            "visited" => PseudoClass::Visited,
            "any-link" => PseudoClass::AnyLink,
            "valid" => PseudoClass::Valid,
            "invalid" => PseudoClass::Invalid,
            "user-valid" => PseudoClass::UserValid,
            "user-invalid" => PseudoClass::UserInvalid,
            "in-range" => PseudoClass::InRange,
            "out-of-range" => PseudoClass::OutOfRange,
            "scope" => PseudoClass::Scope,
            "target" => PseudoClass::Target,
            "target-within" => PseudoClass::TargetWithin,
            "defined" => PseudoClass::Defined,
            "fullscreen" => PseudoClass::Fullscreen,
            "modal" => PseudoClass::Modal,
            "popover-open" => PseudoClass::PopoverOpen,
            "current" => PseudoClass::Current,
            "past" => PseudoClass::Past,
            "future" => PseudoClass::Future,
            "host" => PseudoClass::Host(None),
            _ => PseudoClass::Unsupported(name),
        };
        Some(SimpleSelector::PseudoClass(pc))
    }

    /// Парсит тело `:foo(...)` для известных функциональных pseudo. Возвращает
    /// `None` для неизвестных или невалидных тел — caller обернёт в Unsupported
    /// и проглотит остаток до `)`.
    fn parse_functional_pseudo_body(&mut self, name_lower: &str) -> Option<PseudoClass> {
        match name_lower {
            "nth-child" => {
                let (spec, of) = self.parse_nth_spec_with_of()?;
                Some(PseudoClass::NthChild(spec, of))
            }
            "nth-last-child" => {
                let (spec, of) = self.parse_nth_spec_with_of()?;
                Some(PseudoClass::NthLastChild(spec, of))
            }
            "nth-of-type" => Some(PseudoClass::NthOfType(self.parse_nth_spec()?)),
            "nth-last-of-type" => Some(PseudoClass::NthLastOfType(self.parse_nth_spec()?)),
            "not" => {
                // CSS Selectors L4 §5.4: внутри `:not(...)` допустим полный
                // selector-list (complex-селекторы с combinator-ами), nested
                // `:not(:not(...))` тоже разрешён.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Not(list))
            }
            "is" => {
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                // Должны быть на `)`; иначе argument невалиден.
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Is(list))
            }
            "where" => {
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Where(list))
            }
            "has" => {
                // CSS Selectors L4 §17.2: relative-selector-list. Каждый
                // элемент — combinator + selector, или просто selector
                // (implicit descendant).
                let list = self.parse_relative_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Has(list))
            }
            "host" => {
                // CSS Scoping L1 §6.1: `:host` и `:host(selector-list)`.
                // При парсинге `:host(...)` парсим selector-list внутри.
                // Если список пустой — невалидно.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Host(Some(list)))
            }
            "dir" => {
                // CSS Selectors L4 §13.2: single keyword argument `ltr` или
                // `rtl`, ASCII case-insensitive. Остальные значения, включая
                // `auto`, — невалидны (фоллбэк на Unsupported у caller-а).
                self.skip_ws_and_comments();
                let mut kw = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphabetic() {
                        kw.push(c.to_ascii_lowercase());
                        self.consume();
                    } else {
                        break;
                    }
                }
                self.skip_ws_and_comments();
                if self.peek() != Some(')') {
                    return None;
                }
                match kw.as_str() {
                    "ltr" => Some(PseudoClass::Dir(DirArg::Ltr)),
                    "rtl" => Some(PseudoClass::Dir(DirArg::Rtl)),
                    _ => None,
                }
            }
            "lang" => {
                // CSS Selectors L4 §11: comma-list BCP 47 language tags.
                // Tag = ASCII alpha, после которого допустимы alpha/digit/`-`
                // (RFC 5646). Нормализуем к lowercase для case-insensitive
                // matching. Whitespace внутри и вокруг запятой допускается;
                // строковые литералы и quoted-tags по строгой спеке тоже
                // допускаются, но в Phase 0 поддерживаем ident-форму — этого
                // достаточно для подавляющего большинства author CSS.
                let mut tags: Vec<String> = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    if matches!(self.peek(), None | Some(')')) {
                        break;
                    }
                    let mut buf = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_alphanumeric() || c == '-' {
                            buf.push(c.to_ascii_lowercase());
                            self.consume();
                        } else {
                            break;
                        }
                    }
                    if buf.is_empty() {
                        return None;
                    }
                    tags.push(buf);
                    self.skip_ws_and_comments();
                    if self.peek() == Some(',') {
                        self.consume();
                    } else {
                        break;
                    }
                }
                if tags.is_empty() {
                    return None;
                }
                Some(PseudoClass::Lang(tags))
            }
            _ => None,
        }
    }

    /// Парсит relative-selector-list для `:has()`. Каждый элемент — опциональный
    /// ведущий combinator (`>`, `+`, `~`) + сам complex selector.
    fn parse_relative_selector_list(&mut self) -> Vec<RelativeSelector> {
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None | Some(')') => break,
                _ => {}
            }
            let combinator = match self.peek() {
                Some('>') => { self.consume(); Some(Combinator::Child) }
                Some('+') => { self.consume(); Some(Combinator::NextSibling) }
                Some('~') => { self.consume(); Some(Combinator::LaterSibling) }
                _ => None,
            };
            self.skip_ws_and_comments();
            let Some(selector) = self.parse_complex_selector() else {
                // Невалидный selector — пропускаем до запятой/конца.
                while let Some(c) = self.peek() {
                    if c == ',' || c == ')' { break; }
                    self.consume();
                }
                if self.peek() == Some(',') { self.consume(); }
                continue;
            };
            out.push(RelativeSelector { combinator, selector });
            self.skip_ws_and_comments();
            if self.peek() == Some(',') {
                self.consume();
            } else {
                break;
            }
        }
        out
    }

    /// Парсит тело функционального pseudo-element (например `::slotted(...)` или `::highlight(...)`).
    /// Возвращает `None` для неизвестных или невалидных тел — caller обернёт
    /// в `Unknown(name)` и проглотит остаток до `)`.
    fn parse_functional_pseudo_element(&mut self, name_lower: &str) -> Option<PseudoElementKind> {
        match name_lower {
            "slotted" => {
                // CSS Scoping L1 §6.2: `::slotted(selector-list)` матчит element,
                // который слотирован через этот `<slot>` и матчит хотя бы один
                // из селекторов списка.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Slotted(Some(list)))
            }
            "highlight" => {
                // CSS Highlight API L1 §3: `::highlight(name)` матчит элемент,
                // который стилизуется через highlight с заданным именем.
                self.skip_ws_and_comments();
                let name = self.parse_ident().unwrap_or_default();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || name.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Highlight(name))
            }
            _ => None,
        }
    }

    /// Парсит `an+b`, число или ключевые слова `odd`/`even`. Останавливается на
    /// `)` или конце ввода — caller съест `)` через `skip_to_paren_close`.
    /// **Не** парсит `of <selector-list>` — для этого `parse_nth_spec_with_of`;
    /// этот метод оставлен для `:nth-of-type` / `:nth-last-of-type` (per spec
    /// они не поддерживают `of` clause).
    fn parse_nth_spec(&mut self) -> Option<NthSpec> {
        self.skip_ws_and_comments();
        // Соберём «токен» формулы — всё до `)` или конца.
        let mut raw = String::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            raw.push(c);
            self.consume();
        }
        parse_nth_spec_str(raw.trim())
    }

    /// Парсит `an+b [of <selector-list>]` для `:nth-child` / `:nth-last-child`
    /// (CSS Selectors L4 §6.6.5.1). Возвращает `(NthSpec, Option<list>)`:
    /// `None` для list означает отсутствие `of`-clause; `Some(non-empty list)`
    /// — фильтр siblings. Пустой `of` clause (`of` без следующего selector-а)
    /// → возврат `None` из всего метода — caller fallback-ит на `Unsupported`.
    ///
    /// Алгоритм: собираем raw-tokens до встречи `of` (ASCII case-insensitive,
    /// окружённого whitespace или скобками — чтобы `2nof.x` не схлопывалось)
    /// либо `)`. Затем nth-spec парсится из собранного prefix; если за ним
    /// есть `of` — парсим selector-list до `)`.
    fn parse_nth_spec_with_of(&mut self) -> Option<(NthSpec, Option<Vec<ComplexSelector>>)> {
        self.skip_ws_and_comments();
        let mut raw = String::new();
        // Собираем nth-spec токены до встречи `of`-keyword (отделённого
        // whitespace по обе стороны: spec требует whitespace вокруг `of`,
        // чтобы `2nof.x` не схлопнулось как nth-spec `2nof` + `.x`).
        // Без `of` — собираем всё до `)`, как старый `parse_nth_spec`.
        loop {
            let saved = self.pos;
            self.skip_ws_and_comments();
            let after_ws = self.pos;
            let Some(c) = self.peek() else { break };
            if c == ')' {
                self.pos = saved;
                break;
            }
            if after_ws > saved && self.peek_ident_matches_of() {
                // Откатываемся к началу whitespace, чтобы of-clause увидел
                // boundary сам.
                self.pos = saved;
                break;
            }
            if after_ws > saved {
                raw.push(' ');
            }
            raw.push(c);
            self.consume();
        }
        let spec = parse_nth_spec_str(raw.trim())?;
        self.skip_ws_and_comments();
        if !self.peek_ident_matches_of() {
            return Some((spec, None));
        }
        self.consume(); // 'o'
        self.consume(); // 'f'
        self.skip_ws_and_comments();
        let list = self.parse_selector_list();
        self.skip_ws_and_comments();
        if list.is_empty() || self.peek() != Some(')') {
            return None;
        }
        Some((spec, Some(list)))
    }

    /// Возвращает true, если следующие 2 байта — `of` (ASCII case-insensitive)
    /// И за ними следует НЕ-ident-continuation байт (whitespace, `)`, EOF,
    /// и т.д.). Без consume.
    fn peek_ident_matches_of(&self) -> bool {
        let b = self.input.as_bytes();
        let p = self.pos;
        if p + 1 >= b.len() {
            return false;
        }
        if !b[p].eq_ignore_ascii_case(&b'o') || !b[p + 1].eq_ignore_ascii_case(&b'f') {
            return false;
        }
        match b.get(p + 2) {
            None => true,
            Some(&c) => !c.is_ascii_alphanumeric() && c != b'-' && c != b'_',
        }
    }

    fn skip_to_paren_close(&mut self) {
        let mut depth = 1;
        while let Some(c) = self.peek() {
            self.consume();
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_ident(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !is_ident_start(first) {
            return None;
        }
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.consume();
                s.push(c);
            } else {
                break;
            }
        }
        Some(s)
    }

    fn parse_declaration_block(&mut self) -> Vec<Declaration> {
        let mut decls = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                    continue;
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        decls
    }

    fn recover_to_decl_boundary(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '}' => return,
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_declaration(&mut self) -> Option<Declaration> {
        self.skip_ws_and_comments();
        let property = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some(':') {
            return None;
        }
        self.consume();
        let value = self.parse_value_until_terminator();
        let (value, important) = extract_important(value.trim());
        Some(Declaration {
            property,
            value,
            important,
        })
    }

    fn parse_value_until_terminator(&mut self) -> String {
        let mut s = String::new();
        let mut in_string: Option<char> = None;
        while let Some(c) = self.peek() {
            match (in_string, c) {
                (None, ';') | (None, '}') => break,
                (Some(q), c) if c == q => {
                    self.consume();
                    s.push(c);
                    in_string = None;
                }
                (None, '"') | (None, '\'') => {
                    self.consume();
                    s.push(c);
                    in_string = Some(c);
                }
                _ => {
                    self.consume();
                    s.push(c);
                }
            }
        }
        s
    }
}

/// CSS Cascade L4 §8.1: если значение оканчивается на `!important` (с
/// опциональным whitespace между `!` и словом, ASCII case-insensitive),
/// отделяет его и возвращает `(clean_value, true)`. Иначе — `(value, false)`.
///
/// Безопасно для строковых литералов: `content: "!important"` даёт
/// (value=`"!important"`, false), потому что после строки идёт `"`, а не
/// `important`. Не пытается обрабатывать комментарии внутри `!important`
/// (`!/* x */important`) и multiple `!important` — оба слишком экзотичны.
fn extract_important(value: &str) -> (String, bool) {
    let v = value.trim_end();
    let imp = b"important";
    if v.len() < imp.len() {
        return (value.to_string(), false);
    }
    if !v.as_bytes()[v.len() - imp.len()..].eq_ignore_ascii_case(imp) {
        return (value.to_string(), false);
    }
    let before_imp = v[..v.len() - imp.len()].trim_end();
    let Some(before_bang) = before_imp.strip_suffix('!') else {
        return (value.to_string(), false);
    };
    (before_bang.trim_end().to_string(), true)
}

/// Снимает с CSS-string значения (`"..."` или `'...'`) обрамляющие кавычки.
/// Возвращает None если значение не строковый литерал. Используется для
/// дескриптора `syntax` в `@property` (он обязан быть строкой по spec L1 §1.1).
/// Внутренние escape-последовательности (`\xNN`, `\<newline>`) не
/// поддерживаются — в Phase 0 syntax всегда `"*"`, и более сложные формы
/// (`"<length>"`, `"<color>"`) будут идти через тот же путь без escape-ов.
fn strip_css_string(v: &str) -> Option<&str> {
    let bytes = v.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0];
    if (q == b'"' || q == b'\'') && bytes[bytes.len() - 1] == q {
        Some(&v[1..v.len() - 1])
    } else {
        None
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-' || c >= '\u{00A0}'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Парсит формулу `an+b` из строки. Поддерживает `odd`, `even`, целые числа,
/// и любые комбинации `<int>?n<sign><int>?`. Пробелы внутри допустимы и
/// игнорируются (CSS spec).
fn parse_nth_spec_str(s: &str) -> Option<NthSpec> {
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

/// CSS Nesting L1 §3 — expand `& (combinator) nested` into concrete selectors.
///
/// `combinator = None`  → compound join (e.g. `&.foo` → `parent.foo`)
/// `combinator = Some(c)` → `parent c nested` (e.g. `& span` → `parent descendant span`)
fn expand_nesting(
    parents: &[ComplexSelector],
    combinator: Option<Combinator>,
    nested: &[ComplexSelector],
) -> Vec<ComplexSelector> {
    let mut result = Vec::new();
    for parent in parents {
        for n in nested {
            let expanded = match combinator {
                None => {
                    // `&.foo` → merge parent head with nested head, keep tails.
                    let mut head = parent.head.clone();
                    head.parts.extend_from_slice(&n.head.parts);
                    let mut tail = parent.tail.clone();
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head, tail }
                }
                Some(comb) => {
                    // `& span` → parent + (comb, nested_head) + nested_tail
                    let mut tail = parent.tail.clone();
                    tail.push((comb, n.head.clone()));
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head: parent.head.clone(), tail }
                }
            };
            result.push(expanded);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_css_str tests ──────────────────────────────────────────────────────

    #[test]
    fn to_css_str_type_selector() {
        let sel = parse_selector_list("div");
        assert_eq!(sel[0].to_css_str(), "div");
    }

    #[test]
    fn to_css_str_class_and_id() {
        let sel = parse_selector_list(".foo#bar");
        assert_eq!(sel[0].to_css_str(), ".foo#bar");
    }

    #[test]
    fn to_css_str_descendant_combinator() {
        let sel = parse_selector_list("div p");
        assert_eq!(sel[0].to_css_str(), "div p");
    }

    #[test]
    fn to_css_str_child_combinator() {
        let sel = parse_selector_list("ul > li");
        assert_eq!(sel[0].to_css_str(), "ul > li");
    }

    #[test]
    fn to_css_str_pseudo_class() {
        let sel = parse_selector_list("a:hover");
        assert_eq!(sel[0].to_css_str(), "a:hover");
    }

    #[test]
    fn to_css_str_first_child() {
        let sel = parse_selector_list("p:first-child");
        assert_eq!(sel[0].to_css_str(), "p:first-child");
    }

    #[test]
    fn to_css_str_nth_child() {
        let sel = parse_selector_list("li:nth-child(2n+1)");
        let s = sel[0].to_css_str();
        assert!(s.contains(":nth-child"), "got: {s}");
    }

    #[test]
    fn to_css_str_attribute() {
        let sel = parse_selector_list("[type=\"text\"]");
        let s = sel[0].to_css_str();
        assert!(s.contains("[type") && s.contains("text"), "got: {s}");
    }

    // ── existing test helpers ──────────────────────────────────────────────────

    /// Удобный конструктор для тестов: ComplexSelector из одной compound с
    /// единственным simple-селектором.
    fn one(part: SimpleSelector) -> ComplexSelector {
        ComplexSelector {
            head: CompoundSelector { parts: vec![part] },
            tail: Vec::new(),
        }
    }

    #[test]
    fn empty_input() {
        assert_eq!(parse(""), Stylesheet::default());
    }

    #[test]
    fn whitespace_and_comment_only() {
        assert_eq!(parse("  /* hi */  "), Stylesheet::default());
    }

    #[test]
    fn single_rule() {
        let s = parse("p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Type("p".into()))]);
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn class_selector() {
        let s = parse(".foo { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("foo".into()))]);
    }

    #[test]
    fn id_selector() {
        let s = parse("#bar { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Id("bar".into()))]);
    }

    #[test]
    fn universal_selector() {
        let s = parse("* { box-sizing: border-box; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Universal)]);
    }

    #[test]
    fn multiple_selectors() {
        let s = parse("p, h1, h2 { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![
                one(SimpleSelector::Type("p".into())),
                one(SimpleSelector::Type("h1".into())),
                one(SimpleSelector::Type("h2".into())),
            ]
        );
    }

    #[test]
    fn multiple_declarations() {
        let s = parse("p { color: red; font-size: 14px; margin: 0; }");
        assert_eq!(s.rules[0].declarations.len(), 3);
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
        assert_eq!(s.rules[0].declarations[1].value, "14px");
    }

    // ──────────────── !important (CSS Cascade L4 §8.1) ────────────────

    #[test]
    fn declaration_default_not_important() {
        let s = parse("p { color: red; }");
        assert!(!s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_basic() {
        let s = parse("p { color: red !important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_no_space_before_bang() {
        let s = parse("p { color: red!important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_case_insensitive() {
        let s = parse("p { color: red !IMPORTANT; }");
        assert!(s.rules[0].declarations[0].important);
    }

    #[test]
    fn declaration_important_with_whitespace_between_bang_and_word() {
        // CSS Syntax §5.5.4 разрешает whitespace внутри `!important`.
        let s = parse("p { color: red !  important; }");
        assert!(s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_inside_quotes_not_stripped() {
        // `content: "!important"` — литерал, не модификатор.
        let s = parse(r#"p { content: "!important"; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, r#""!important""#);
    }

    #[test]
    fn declaration_important_after_quoted_value() {
        // `font-family: "Arial" !important;` — флаг есть, value сохраняется.
        let s = parse(r#"p { font-family: "Arial" !important; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, r#""Arial""#);
    }

    #[test]
    fn declaration_important_works_for_multiple() {
        let s = parse("p { color: red !important; font-size: 14px; }");
        assert!(s.rules[0].declarations[0].important);
        assert!(!s.rules[0].declarations[1].important);
    }

    #[test]
    fn declaration_value_ending_with_important_word_alone_not_flag() {
        // `value: important;` — без `!`, не флаг.
        let s = parse("p { font-weight: important; }");
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, "important");
    }

    #[test]
    fn trailing_semicolon_optional() {
        let with = parse("p { color: red; }");
        let without = parse("p { color: red }");
        assert_eq!(with, without);
    }

    #[test]
    fn empty_rule() {
        let s = parse("p {}");
        assert_eq!(s.rules.len(), 1);
        assert!(s.rules[0].declarations.is_empty());
    }

    #[test]
    fn multiple_rules() {
        let s = parse("p { color: red; } h1 { font-size: 24px; }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].property, "font-size");
    }

    #[test]
    fn comments_between_and_within() {
        let s = parse("/* one */ p /* hmm */ { /* x */ color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn at_import_skipped() {
        let s = parse("@import \"foo.css\"; p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors[0], one(SimpleSelector::Type("p".into())));
    }

    #[test]
    fn at_media_block_skipped() {
        let s = parse("@media print { p { color: black; } } p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn cyrillic_class_selector() {
        let s = parse(".привет { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![one(SimpleSelector::Class("привет".into()))]
        );
    }

    #[test]
    fn cyrillic_value_with_quotes() {
        let s = parse(r#"p { font-family: "Иваново", sans-serif; }"#);
        assert_eq!(
            s.rules[0].declarations[0].value,
            r#""Иваново", sans-serif"#
        );
    }

    #[test]
    fn malformed_declaration_skipped() {
        let s = parse("p { color: red; broken; font-size: 14px; }");
        assert_eq!(s.rules[0].declarations.len(), 2);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
    }

    #[test]
    fn negative_and_complex_values() {
        let s = parse("p { margin: -10px; background: url(\"a.png\"); }");
        assert_eq!(s.rules[0].declarations[0].value, "-10px");
        assert_eq!(s.rules[0].declarations[1].value, "url(\"a.png\")");
    }

    #[test]
    fn vendor_prefix_property() {
        let s = parse("p { -webkit-user-select: none; }");
        assert_eq!(s.rules[0].declarations[0].property, "-webkit-user-select");
    }

    // ──────────────── compound selectors ────────────────

    #[test]
    fn compound_type_and_class() {
        let s = parse("p.foo { color: red; }");
        assert_eq!(s.rules[0].selectors.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
            ]
        );
    }

    #[test]
    fn compound_type_class_id() {
        let s = parse("p.foo#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
                SimpleSelector::Id("bar".into()),
            ]
        );
    }

    #[test]
    fn compound_two_classes() {
        let s = parse(".a.b { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Class("a".into()),
                SimpleSelector::Class("b".into()),
            ]
        );
    }

    // ──────────────── combinators ────────────────

    #[test]
    fn descendant_combinator() {
        let s = parse("div p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("p".into())]);
    }

    #[test]
    fn child_combinator() {
        let s = parse("ul > li { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::Child);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("li".into())]);
    }

    #[test]
    fn next_sibling_combinator() {
        let s = parse("h1 + p { margin-top: 0; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::NextSibling);
    }

    #[test]
    fn later_sibling_combinator() {
        let s = parse("h1 ~ p { color: gray; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::LaterSibling);
    }

    #[test]
    fn chained_combinators() {
        let s = parse("body main > article p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("body".into())]);
        assert_eq!(sel.tail.len(), 3);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[1].0, Combinator::Child);
        assert_eq!(sel.tail[2].0, Combinator::Descendant);
    }

    #[test]
    fn combinator_around_compound() {
        let s = parse("nav.main > a.link { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].1.parts.len(), 2);
    }

    // ──────────────── attribute selectors ────────────────

    #[test]
    fn attribute_presence() {
        let s = parse("[disabled] { opacity: 0.5; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "disabled");
                assert_eq!(a.op, None);
                assert_eq!(a.value, None);
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_unquoted() {
        let s = parse("[type=submit] { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "type");
                assert_eq!(a.op, Some(AttrOp::Equals));
                assert_eq!(a.value.as_deref(), Some("submit"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_quoted() {
        let s = parse(r#"[lang="ru-RU"] { font-family: serif; }"#);
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.value.as_deref(), Some("ru-RU"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_all_operators() {
        let ops = [
            ("[a~=v]", AttrOp::Includes),
            ("[a|=v]", AttrOp::DashMatch),
            ("[a^=v]", AttrOp::Prefix),
            ("[a$=v]", AttrOp::Suffix),
            ("[a*=v]", AttrOp::Substring),
        ];
        for (src, expected) in ops {
            let s = parse(&format!("{src} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::Attribute(a) => assert_eq!(a.op, Some(expected), "src={src}"),
                _ => panic!("expected attribute selector for {src}"),
            }
        }
    }

    #[test]
    fn attribute_combined_with_type() {
        let s = parse("a[href] { color: blue; }");
        let head = &s.rules[0].selectors[0].head;
        assert_eq!(head.parts.len(), 2);
        assert!(matches!(head.parts[0], SimpleSelector::Type(ref t) if t == "a"));
        assert!(matches!(&head.parts[1], SimpleSelector::Attribute(a) if a.name == "href"));
    }

    // ──────────────── case-insensitive attribute (CSS L4 §6.3.6) ────────────

    fn attr_at(s: &Stylesheet, rule: usize) -> &AttrSelector {
        match &s.rules[rule].selectors[0].head.parts[0] {
            SimpleSelector::Attribute(a) => a,
            other => panic!("expected attribute selector, got {other:?}"),
        }
    }

    #[test]
    fn attribute_case_insensitive_flag_lowercase() {
        let s = parse("[type=submit i] { color: red; }");
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("submit"));
    }

    #[test]
    fn attribute_case_insensitive_flag_uppercase() {
        // `I` тоже должен работать (флаги ASCII case-insensitive).
        let s = parse("[type=submit I] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_sensitive_explicit() {
        // `s` явно ставит case-sensitive (default).
        let s = parse("[type=submit s] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_quoted_value() {
        let s = parse(r#"[lang="EN-us" i] { color: red; }"#);
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("EN-us"));
    }

    #[test]
    fn attribute_case_insensitive_works_for_all_ops() {
        // Флаг `i` совместим со всеми операторами.
        for src in [
            "[a~=v i]",
            "[a|=v i]",
            "[a^=v i]",
            "[a$=v i]",
            "[a*=v i]",
        ] {
            let s = parse(&format!("{src} {{}}"));
            assert!(attr_at(&s, 0).case_insensitive, "ci flag lost in {src}");
        }
    }

    #[test]
    fn attribute_no_flag_default_case_sensitive() {
        let s = parse("[type=submit] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_extra_whitespace() {
        // Между value и `i` — любое количество пробелов.
        let s = parse("[type=submit   i ] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
    }

    // ──────────────── pseudo-classes / pseudo-elements ────────────────

    #[test]
    fn pseudo_first_child() {
        let s = parse("p:first-child { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(
            &head.parts[1],
            SimpleSelector::PseudoClass(PseudoClass::FirstChild)
        ));
    }

    #[test]
    fn pseudo_known_names() {
        let cases = [
            ("first-child", PseudoClass::FirstChild),
            ("last-child", PseudoClass::LastChild),
            ("only-child", PseudoClass::OnlyChild),
            ("empty", PseudoClass::Empty),
            ("root", PseudoClass::Root),
            ("first-of-type", PseudoClass::FirstOfType),
            ("last-of-type", PseudoClass::LastOfType),
            ("only-of-type", PseudoClass::OnlyOfType),
            ("placeholder-shown", PseudoClass::PlaceholderShown),
            ("required", PseudoClass::Required),
            ("optional", PseudoClass::Optional),
            ("read-only", PseudoClass::ReadOnly),
            ("read-write", PseudoClass::ReadWrite),
            ("disabled", PseudoClass::Disabled),
            ("enabled", PseudoClass::Enabled),
            ("checked", PseudoClass::Checked),
            ("indeterminate", PseudoClass::Indeterminate),
            ("default", PseudoClass::Default),
            ("link", PseudoClass::Link),
            ("visited", PseudoClass::Visited),
            ("any-link", PseudoClass::AnyLink),
            ("in-range", PseudoClass::InRange),
            ("out-of-range", PseudoClass::OutOfRange),
            ("scope", PseudoClass::Scope),
            ("target", PseudoClass::Target),
            ("target-within", PseudoClass::TargetWithin),
            ("defined", PseudoClass::Defined),
            ("fullscreen", PseudoClass::Fullscreen),
            ("modal", PseudoClass::Modal),
            ("popover-open", PseudoClass::PopoverOpen),
            ("current", PseudoClass::Current),
            ("past", PseudoClass::Past),
            ("future", PseudoClass::Future),
        ];
        for (name, expected) in cases {
            let s = parse(&format!(":{name} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::PseudoClass(pc) => assert_eq!(pc, &expected, "name={name}"),
                _ => panic!("expected pseudo-class for {name}"),
            }
        }
    }

    #[test]
    fn pseudo_unsupported_kept_as_name() {
        let s = parse(":hover { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Hover) => {},
            _ => panic!("expected Hover pseudo-class"),
        }
    }

    #[test]
    fn pseudo_nth_child_parsed() {
        let s = parse(":nth-child(2n+1) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(spec, of)) => {
                assert_eq!(*spec, NthSpec { a: 2, b: 1 });
                assert!(of.is_none(), "no of-clause expected");
            }
            _ => panic!("expected NthChild(2n+1), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_with_of_clause() {
        // CSS Selectors L4 §6.6.5.1.
        let s = parse(":nth-child(odd of .visible) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(spec, of)) => {
                assert_eq!(*spec, NthSpec::ODD);
                let list = of.as_ref().expect("of-clause expected");
                assert_eq!(list.len(), 1);
            }
            _ => panic!("expected NthChild with of-clause, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_last_child_with_of_clause() {
        let s = parse(":nth-last-child(1 of li.active) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthLastChild(spec, of)) => {
                assert_eq!(*spec, NthSpec { a: 0, b: 1 });
                assert!(of.is_some(), "of-clause expected");
            }
            _ => panic!("expected NthLastChild with of-clause, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_of_selector_list() {
        // `of` принимает selector-list через запятую.
        let s = parse(":nth-child(2n of .x, .y) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(_, Some(list))) => {
                assert_eq!(list.len(), 2);
            }
            _ => panic!("expected NthChild with selector-list of, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_empty_of_clause_invalid() {
        // `:nth-child(odd of)` без selector-а → invalid, fallback на Unsupported.
        let s = parse(":nth-child(odd of) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "nth-child"
        ));
    }

    #[test]
    fn pseudo_nth_of_type_does_not_accept_of_clause() {
        // CSS Selectors L4 §6.6.5.1: `of` clause НЕ применяется к
        // `:nth-of-type` (type filter — implicit). Если у пользователя там
        // случайно `of` — спека требует invalid; наш парсер собирает всё
        // в spec-string, который parse_nth_spec_str отвергает.
        let s = parse(":nth-of-type(odd of .x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "nth-of-type"
        ));
    }

    #[test]
    fn pseudo_lang_single_tag() {
        let s = parse(":lang(en) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_with_region() {
        let s = parse(":lang(en-US) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en-us".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_comma_list() {
        let s = parse(":lang(en, fr, ru) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string(), "fr".to_string(), "ru".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_case_normalized_to_lower() {
        let s = parse(":lang(EN, FR-CA) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string(), "fr-ca".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_empty_falls_back_to_unsupported() {
        // `:lang()` без аргументов — невалидно по spec, парсер откатывает
        // в Unsupported.
        let s = parse(":lang() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "lang"
        ));
    }

    #[test]
    fn pseudo_dir_ltr() {
        let s = parse(":dir(ltr) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Ltr))
        ));
    }

    #[test]
    fn pseudo_dir_rtl() {
        let s = parse(":dir(rtl) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Rtl))
        ));
    }

    #[test]
    fn pseudo_dir_case_insensitive_keyword() {
        let s = parse(":dir(LTR) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Ltr))
        ));
    }

    #[test]
    fn pseudo_dir_unknown_keyword_falls_back() {
        // `auto` — невалидный аргумент для :dir в spec (только ltr/rtl).
        // Парсер откатывает в Unsupported.
        let s = parse(":dir(auto) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "dir"
        ));
    }

    #[test]
    fn pseudo_dir_empty_falls_back() {
        let s = parse(":dir() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "dir"
        ));
    }

    #[test]
    fn pseudo_target_case_insensitive_name() {
        // pseudo-class names ASCII case-insensitive (CSS Syntax §4.4) —
        // `:TARGET` распознаётся как `:target`.
        for src in [":target { }", ":TARGET { }", ":Target { }"] {
            let s = parse(src);
            let p = &s.rules[0].selectors[0].head.parts[0];
            assert!(
                matches!(p, SimpleSelector::PseudoClass(PseudoClass::Target)),
                "name={src}"
            );
        }
    }

    #[test]
    fn pseudo_target_does_not_accept_arguments() {
        // `:target` — не functional pseudo. `:target(x)` — невалидное use,
        // fallback на Unsupported.
        let s = parse(":target(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "target"
        ));
    }

    #[test]
    fn pseudo_target_specificity_is_pseudo_class_level() {
        // CSS Selectors L4 §16: pseudo-class contributes (0,1,0) — class-уровень.
        let s = parse(":target { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 1, c: 0 });
    }

    #[test]
    fn pseudo_target_within_recognized() {
        // Подтверждение, что `:target-within` парсится как отдельный variant,
        // а не как `target`-ident с suffix-ом или Unsupported.
        let s = parse(":target-within { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::TargetWithin)
        ));
    }

    #[test]
    fn pseudo_target_within_does_not_accept_arguments() {
        // Не functional pseudo — `:target-within(x)` → Unsupported.
        let s = parse(":target-within(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "target-within"
        ));
    }

    #[test]
    fn pseudo_defined_case_insensitive_name() {
        // CSS Syntax §4.4: pseudo-class names ASCII case-insensitive.
        for src in [":defined { }", ":DEFINED { }", ":Defined { }"] {
            let s = parse(src);
            let p = &s.rules[0].selectors[0].head.parts[0];
            assert!(
                matches!(p, SimpleSelector::PseudoClass(PseudoClass::Defined)),
                "src={src}"
            );
        }
    }

    #[test]
    fn pseudo_defined_does_not_accept_arguments() {
        // `:defined` — не functional. `:defined(x)` → Unsupported.
        let s = parse(":defined(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "defined"
        ));
    }

    #[test]
    fn pseudo_defined_specificity_is_pseudo_class_level() {
        let s = parse(":defined { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 1, c: 0 });
    }

    #[test]
    fn pseudo_element_double_colon() {
        let s = parse("p::before { content: \"\"; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(&head.parts[1], SimpleSelector::PseudoElement(PseudoElementKind::Before)));
    }

    // ──────────────── specificity ────────────────

    #[test]
    fn specificity_universal_is_zero() {
        let s = parse("* { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 0, c: 0 });
    }

    #[test]
    fn specificity_type_is_001() {
        let s = parse("p { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn specificity_class_is_010() {
        let s = parse(".foo { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_id_is_100() {
        let s = parse("#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_complex() {
        // a#b.c[d] p:hover — id=1, class+attr+pseudo=3, type=2 → (1,3,2)
        let s = parse("a#b.c[d] p:hover { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 3, c: 2 }
        );
    }

    #[test]
    fn specificity_ordering() {
        let high = Specificity { a: 0, b: 1, c: 0 }; // .foo
        let low = Specificity { a: 0, b: 0, c: 5 }; // div div div div div
        assert!(high > low);
    }

    // ──────────────── edge cases для recovery ────────────────

    #[test]
    fn unknown_combinator_breaks_rule() {
        // `% p` — `%` не start_ident и не combinator, должен быть recovery.
        // Дальше нормальное правило парсится.
        let s = parse("% p { color: red; } a { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("a".into())]
        );
    }

    #[test]
    fn malformed_attribute_recovers() {
        let s = parse("[a$$=foo] { color: red; } p { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("p".into())]
        );
    }

    // ──────────────── functional pseudo: :nth-* ────────────────

    #[test]
    fn nth_spec_str_keywords() {
        assert_eq!(parse_nth_spec_str("odd"), Some(NthSpec { a: 2, b: 1 }));
        assert_eq!(parse_nth_spec_str("even"), Some(NthSpec { a: 2, b: 0 }));
        assert_eq!(parse_nth_spec_str("ODD"), Some(NthSpec { a: 2, b: 1 }));
    }

    #[test]
    fn nth_spec_str_formulas() {
        let cases = [
            ("n", (1, 0)),
            ("+n", (1, 0)),
            ("-n", (-1, 0)),
            ("2n", (2, 0)),
            ("2n+1", (2, 1)),
            ("2n-1", (2, -1)),
            ("-2n+3", (-2, 3)),
            ("3n+0", (3, 0)),
            ("5", (0, 5)),
            ("-5", (0, -5)),
            ("2n + 1", (2, 1)), // пробелы допустимы
            ("  2n  ", (2, 0)),
        ];
        for (s, (a, b)) in cases {
            assert_eq!(
                parse_nth_spec_str(s),
                Some(NthSpec { a, b }),
                "input={s}"
            );
        }
    }

    #[test]
    fn nth_spec_str_invalid() {
        assert_eq!(parse_nth_spec_str(""), None);
        assert_eq!(parse_nth_spec_str("abc"), None);
        assert_eq!(parse_nth_spec_str("2x+1"), None);
        assert_eq!(parse_nth_spec_str("n+"), None); // нет числа после знака
    }

    #[test]
    fn nth_spec_matches_arithmetic() {
        let odd = NthSpec::ODD; // 2n+1: 1, 3, 5, ...
        for i in [1, 3, 5, 7, 999] {
            assert!(odd.matches(i), "i={i}");
        }
        for i in [0, 2, 4, -1] {
            assert!(!odd.matches(i), "i={i}");
        }
    }

    #[test]
    fn nth_spec_matches_first_three() {
        // -n+3 → элементы 1, 2, 3 (n=2, 1, 0). Индексы в CSS — 1-based,
        // нулевой случай в реальном matching-е не возникает.
        let spec = NthSpec { a: -1, b: 3 };
        assert!(spec.matches(1));
        assert!(spec.matches(2));
        assert!(spec.matches(3));
        assert!(!spec.matches(4));
        assert!(!spec.matches(5));
    }

    #[test]
    fn nth_spec_matches_constant() {
        // 5 → ровно пятый.
        let spec = NthSpec { a: 0, b: 5 };
        assert!(spec.matches(5));
        assert!(!spec.matches(4));
        assert!(!spec.matches(10));
    }

    #[test]
    fn pseudo_nth_variants_parsed() {
        let cases = [
            ("nth-child", "(2n+1)"),
            ("nth-last-child", "(odd)"),
            ("nth-of-type", "(3)"),
            ("nth-last-of-type", "(-n+2)"),
        ];
        for (name, arg) in cases {
            let s = parse(&format!(":{name}{arg} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            let pc = match p {
                SimpleSelector::PseudoClass(pc) => pc,
                _ => panic!("expected pseudo-class for :{name}{arg}"),
            };
            let is_nth = matches!(
                pc,
                PseudoClass::NthChild(_, _)
                    | PseudoClass::NthLastChild(_, _)
                    | PseudoClass::NthOfType(_)
                    | PseudoClass::NthLastOfType(_)
            );
            assert!(is_nth, "name={name} got {pc:?}");
        }
    }

    #[test]
    fn pseudo_nth_invalid_arg_falls_back_to_unsupported() {
        let s = parse(":nth-child(abc) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) => {
                assert_eq!(n, "nth-child");
            }
            _ => panic!("expected Unsupported(nth-child), got {p:?}"),
        }
        // Парсер должен дойти до конца правила и не оставить мусора.
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    // ──────────────── functional pseudo: :not ────────────────

    #[test]
    fn pseudo_not_simple() {
        let s = parse(":not(.foo) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert!(list[0].tail.is_empty());
            }
            _ => panic!("expected :not(.foo), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_compound() {
        let s = parse(":not(p.hl) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts.len(), 2);
                assert!(matches!(&list[0].head.parts[0], SimpleSelector::Type(t) if t == "p"));
                assert!(matches!(&list[0].head.parts[1], SimpleSelector::Class(c) if c == "hl"));
            }
            _ => panic!("expected :not(p.hl)"),
        }
    }

    #[test]
    fn pseudo_not_with_combinator_l4() {
        // CSS Selectors L4 §5.4: combinator-ы внутри `:not` разрешены.
        let s = parse(":not(a > b) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].tail.len(), 1);
                assert_eq!(list[0].tail[0].0, Combinator::Child);
            }
            _ => panic!("expected :not(a > b), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_nested_l4() {
        // CSS Selectors L4 §5.4: nested `:not(:not(.x))` разрешён.
        let s = parse(":not(:not(.x)) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(outer)) => {
                assert_eq!(outer.len(), 1);
                let inner_part = &outer[0].head.parts[0];
                assert!(matches!(
                    inner_part,
                    SimpleSelector::PseudoClass(PseudoClass::Not(inner)) if inner.len() == 1
                ));
            }
            _ => panic!("expected :not(:not(.x)), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_selector_list() {
        // CSS Selectors L4 §5.4: список селекторов.
        let s = parse(":not(.foo, #bar) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert_eq!(list[1].head.parts, vec![SimpleSelector::Id("bar".into())]);
            }
            _ => panic!("expected :not(.foo, #bar), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_empty_falls_back() {
        // `:not()` без аргументов — невалидно, должен дать Unsupported.
        let s = parse(":not() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(
            matches!(p, SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "not"),
            "got {p:?}"
        );
    }

    #[test]
    fn specificity_not_uses_inner() {
        // :not(.foo) → max-of-list = (.foo) даёт b=1; сам :not — ноль.
        let s = parse(":not(.foo) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_not_with_id() {
        // :not(#x) → a=1, b=0, c=0.
        let s = parse(":not(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_not_list_takes_max() {
        // CSS Selectors L4 §16: `:not(.foo, #bar)` contributes max-specificity
        // по списку = (#bar) = (1,0,0).
        let s = parse(":not(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_not_complex_with_combinator() {
        // `:not(a > b)` → max specificity selector-а внутри = (0, 0, 2) (a + b
        // как type selectors).
        let s = parse(":not(a > b) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 2 }
        );
    }

    // ──────────────── functional pseudo: :is, :where ────────────────

    fn pseudo_at(s: &Stylesheet, rule: usize, sel: usize, part: usize) -> &PseudoClass {
        match &s.rules[rule].selectors[sel].head.parts[part] {
            SimpleSelector::PseudoClass(pc) => pc,
            other => panic!("expected pseudo-class, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_is_class_list() {
        let s = parse(":is(.foo, .bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert_eq!(list[1].head.parts, vec![SimpleSelector::Class("bar".into())]);
            }
            _ => panic!("expected :is(...), got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_where_class_list() {
        let s = parse(":where(.foo, #bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Where(list) if list.len() == 2), "got {pc:?}");
    }

    #[test]
    fn pseudo_is_with_combinator_inside() {
        // CSS4 разрешает combinator-ы внутри :is — в отличие от :not.
        let s = parse(":is(a > b, c d) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                // a > b: head 'a', tail [(Child, 'b')]
                assert_eq!(list[0].tail.len(), 1);
                assert_eq!(list[0].tail[0].0, Combinator::Child);
                // c d: head 'c', tail [(Descendant, 'd')]
                assert_eq!(list[1].tail.len(), 1);
                assert_eq!(list[1].tail[0].0, Combinator::Descendant);
            }
            _ => panic!("expected :is, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_is_with_type_selector() {
        let s = parse("article :is(h1, h2) { color: red; }");
        let sel = &s.rules[0].selectors[0];
        // head = 'article', tail = [(Descendant, compound{:is(h1, h2)})]
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("article".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert!(matches!(
            &sel.tail[0].1.parts[0],
            SimpleSelector::PseudoClass(PseudoClass::Is(list)) if list.len() == 2
        ));
    }

    #[test]
    fn pseudo_is_empty_falls_back() {
        // `:is()` без аргументов — невалидно, должен дать Unsupported.
        let s = parse(":is() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "is"), "got {pc:?}");
    }

    #[test]
    fn pseudo_where_empty_falls_back() {
        let s = parse(":where() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "where"), "got {pc:?}");
    }

    #[test]
    fn specificity_is_takes_max_of_list() {
        // :is(.foo, #bar) → max = (#bar) = (1,0,0).
        let s = parse(":is(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_is_only_classes() {
        // :is(.foo, .bar) → max = (0,1,0).
        let s = parse(":is(.foo, .bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_where_always_zero() {
        // :where(#x) → 0,0,0 даже при id внутри.
        let s = parse(":where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_where_combined_with_outer() {
        // `p:where(#x)` → p (c=1), :where contributes 0 → (0,0,1).
        let s = parse("p:where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn pseudo_is_with_whitespace_around_list() {
        // Внутри `:is( .foo , .bar )` бывают пробелы — парсер не должен терять
        // последний селектор из-за trailing whitespace перед `)`.
        let s = parse(":is( .foo , .bar ) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Is(list) if list.len() == 2), "got {pc:?}");
    }

    // ──────────────── :has() (CSS Selectors L4 §17.2) ────────────────

    #[test]
    fn pseudo_has_descendant_implicit() {
        // `article:has(img)` — implicit descendant.
        let s = parse("article:has(img) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        assert_eq!(head.parts.len(), 2);
        assert!(matches!(&head.parts[0], SimpleSelector::Type(t) if t == "article"));
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list.len(), 1);
                assert!(list[0].combinator.is_none());
                assert_eq!(list[0].selector.head.parts, vec![SimpleSelector::Type("img".into())]);
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_child_combinator() {
        // `:has(> .featured)` — прямой child.
        let s = parse(":has(> .featured) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Has(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].combinator, Some(Combinator::Child));
                assert_eq!(list[0].selector.head.parts, vec![SimpleSelector::Class("featured".into())]);
            }
            _ => panic!("expected :has, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_next_sibling() {
        // `h1:has(+ p)` — h1 followed by p.
        let s = parse("h1:has(+ p) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list[0].combinator, Some(Combinator::NextSibling));
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_later_sibling() {
        let s = parse("h1:has(~ p) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list[0].combinator, Some(Combinator::LaterSibling));
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_multiple_relative_selectors() {
        // Список через запятую.
        let s = parse(":has(.a, > .b, + p) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Has(list) => {
                assert_eq!(list.len(), 3);
                assert!(list[0].combinator.is_none());
                assert_eq!(list[1].combinator, Some(Combinator::Child));
                assert_eq!(list[2].combinator, Some(Combinator::NextSibling));
            }
            _ => panic!("expected :has, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_has_empty_falls_back() {
        let s = parse(":has() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "has"), "got {pc:?}");
    }

    #[test]
    fn specificity_has_takes_max_of_inner() {
        // :has(.foo, #bar) → max = (1,0,0) от #bar.
        let s = parse(":has(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_has_combinator_does_not_count() {
        // `:has(> .x)` — combinator не contributes specificity, только .x = (0,1,0).
        let s = parse(":has(> .x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    // ──────────────── CSS Variables L1 ────────────────

    #[test]
    fn custom_property_declaration_parsed() {
        // `--name: value` — обычная декларация, имя начинается с `--`.
        let s = parse(":root { --main-color: red; }");
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "--main-color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn var_in_value_preserved_verbatim() {
        // Substitution делает layout, парсер должен сохранить var() в value
        // как есть (вместе с whitespace внутри скобок и fallback после `,`).
        let s = parse("p { color: var(--c, blue); }");
        assert_eq!(s.rules[0].declarations[0].value, "var(--c, blue)");
    }

    #[test]
    fn custom_property_with_complex_value() {
        // Custom property value может содержать что угодно (включая запятые
        // и скобки) — парсер читает до `;` или `}` с уважением к строкам.
        let s = parse(":root { --shadow: 0 2px 4px rgba(0, 0, 0, 0.5); }");
        assert_eq!(
            s.rules[0].declarations[0].value,
            "0 2px 4px rgba(0, 0, 0, 0.5)"
        );
    }

    #[test]
    fn custom_property_important_flag() {
        // `!important` работает и для custom properties.
        let s = parse(":root { --c: red !important; }");
        assert_eq!(s.rules[0].declarations[0].property, "--c");
        assert_eq!(s.rules[0].declarations[0].value, "red");
        assert!(s.rules[0].declarations[0].important);
    }

    // CSS Properties and Values L1 §1.1 — @property

    #[test]
    fn at_property_basic() {
        let s = parse(
            "@property --main-color { syntax: \"*\"; inherits: false; initial-value: red; }",
        );
        assert_eq!(s.properties.len(), 1);
        let p = &s.properties[0];
        assert_eq!(p.name, "--main-color");
        assert_eq!(p.syntax, "*");
        assert!(!p.inherits);
        assert_eq!(p.initial_value.as_deref(), Some("red"));
        assert!(s.rules.is_empty());
    }

    #[test]
    fn at_property_universal_no_initial_value_ok() {
        // syntax="*" разрешает отсутствие initial-value.
        let s = parse("@property --x { syntax: \"*\"; inherits: true; }");
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.properties[0].name, "--x");
        assert!(s.properties[0].inherits);
        assert!(s.properties[0].initial_value.is_none());
    }

    #[test]
    fn at_property_non_universal_without_initial_invalid() {
        // syntax="<length>" без initial-value → @property невалидно.
        let s = parse("@property --w { syntax: \"<length>\"; inherits: false; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_missing_inherits_invalid() {
        let s = parse("@property --x { syntax: \"*\"; initial-value: 0; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_missing_syntax_invalid() {
        let s = parse("@property --x { inherits: true; initial-value: 0; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_name_without_dash_invalid() {
        // Имя без ведущих `--` — невалидно. Парсер съест блок и не зарегистрирует.
        let s = parse("@property foo { syntax: \"*\"; inherits: false; }");
        assert!(s.properties.is_empty());
    }

    #[test]
    fn at_property_inherits_case_insensitive() {
        // CSS Values L4 §2.4: keyword-ы ASCII case-insensitive.
        let s = parse("@property --x { SYNTAX: \"*\"; Inherits: TRUE; Initial-Value: 5px; }");
        assert_eq!(s.properties.len(), 1);
        assert!(s.properties[0].inherits);
        assert_eq!(s.properties[0].initial_value.as_deref(), Some("5px"));
    }

    #[test]
    fn at_property_then_normal_rule() {
        // После @property парсер продолжает разбирать обычные правила.
        let s = parse(
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; }\
             p { color: blue; }",
        );
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn at_property_duplicate_keeps_order() {
        // Две регистрации одного имени — сохраняем обе, последняя побеждает
        // на потребительской стороне (по spec — last wins, реализуем в layout).
        let s = parse(
            "@property --x { syntax: \"*\"; inherits: false; initial-value: 1; }\
             @property --x { syntax: \"*\"; inherits: true; initial-value: 2; }",
        );
        assert_eq!(s.properties.len(), 2);
        assert_eq!(s.properties[0].initial_value.as_deref(), Some("1"));
        assert_eq!(s.properties[1].initial_value.as_deref(), Some("2"));
        assert!(s.properties[1].inherits);
    }

    #[test]
    fn other_at_rule_still_skipped() {
        // Прочие @-правила (media/import/...) синтаксически пропускаются.
        let s = parse("@media (min-width: 100px) { p { color: red; } } p { color: blue; }");
        assert!(s.properties.is_empty());
        // @media тело пропущено целиком — остаётся только последнее `p`-правило.
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn at_property_syntax_single_quotes() {
        let s = parse("@property --c { syntax: '*'; inherits: false; initial-value: red; }");
        assert_eq!(s.properties.len(), 1);
        assert_eq!(s.properties[0].syntax, "*");
    }

    // ── @import ──

    #[test]
    fn at_import_url_double_quoted() {
        let s = parse("@import url(\"theme.css\");");
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "theme.css");
        assert!(s.imports[0].media.clauses.is_empty());
    }

    #[test]
    fn at_import_url_single_quoted() {
        let s = parse("@import url('theme.css');");
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_url_unquoted() {
        let s = parse("@import url(theme.css);");
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_bare_string() {
        let s = parse(r#"@import "theme.css";"#);
        assert_eq!(s.imports[0].url, "theme.css");
    }

    #[test]
    fn at_import_with_media_query() {
        let s = parse(r#"@import url("print.css") print;"#);
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "print.css");
        assert_eq!(s.imports[0].media.clauses.len(), 1);
        assert_eq!(s.imports[0].media.clauses[0].conditions.len(), 1);
        if let MediaCondition::MediaType(t) = &s.imports[0].media.clauses[0].conditions[0] {
            assert_eq!(t, "print");
        } else {
            panic!("expected MediaType");
        }
    }

    #[test]
    fn at_import_with_complex_media() {
        let s = parse(r#"@import url("mobile.css") screen and (max-width: 600px);"#);
        assert_eq!(s.imports[0].url, "mobile.css");
        assert_eq!(s.imports[0].media.clauses.len(), 1);
        // Должны быть MediaType("screen") и Feature(MaxWidth(600)).
        let clause = &s.imports[0].media.clauses[0];
        assert!(!clause.negated);
        assert_eq!(clause.conditions.len(), 2);
    }

    #[test]
    fn at_import_multiple_in_stylesheet() {
        let s = parse(r#"
            @import url("a.css");
            @import "b.css";
            @import url("c.css") screen;
            p { color: red; }
        "#);
        assert_eq!(s.imports.len(), 3);
        assert_eq!(s.imports[0].url, "a.css");
        assert_eq!(s.imports[1].url, "b.css");
        assert_eq!(s.imports[2].url, "c.css");
        // Обычное правило тоже должно распарситься.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_import_invalid_syntax_skipped() {
        // Без URL — должна пропуститься, не сломать остаток.
        let s = parse("@import garbage; p { color: red; }");
        assert!(s.imports.is_empty());
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_import_cyrillic_url() {
        let s = parse(r#"@import url("стили.css");"#);
        assert_eq!(s.imports[0].url, "стили.css");
    }

    // ── @font-face ──

    #[test]
    fn at_font_face_basic() {
        let s = parse(r#"
            @font-face {
                font-family: "Roboto";
                src: url("roboto.woff2") format("woff2");
            }
        "#);
        assert_eq!(s.font_faces.len(), 1);
        assert_eq!(s.font_faces[0].family, "Roboto");
        assert_eq!(s.font_faces[0].sources.len(), 1);
        assert_eq!(s.font_faces[0].sources[0].kind, FontFaceSourceKind::Url);
        assert_eq!(s.font_faces[0].sources[0].value, "roboto.woff2");
        assert_eq!(s.font_faces[0].sources[0].format, Some("woff2".to_string()));
    }

    #[test]
    fn at_font_face_multiple_sources() {
        let s = parse(r#"
            @font-face {
                font-family: "Body";
                src: local("Helvetica"), url("body.woff2") format("woff2"), url("body.ttf") format("truetype");
            }
        "#);
        let srcs = &s.font_faces[0].sources;
        assert_eq!(srcs.len(), 3);
        assert_eq!(srcs[0].kind, FontFaceSourceKind::Local);
        assert_eq!(srcs[0].value, "Helvetica");
        assert_eq!(srcs[0].format, None);
        assert_eq!(srcs[1].kind, FontFaceSourceKind::Url);
        assert_eq!(srcs[1].format, Some("woff2".to_string()));
        assert_eq!(srcs[2].format, Some("truetype".to_string()));
    }

    #[test]
    fn at_font_face_all_descriptors() {
        let s = parse(r#"
            @font-face {
                font-family: "Var";
                src: url("var.woff2");
                font-weight: 100 900;
                font-style: italic;
                font-display: swap;
                unicode-range: U+0000-007F, U+0400-04FF;
            }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.weight, Some("100 900".to_string()));
        assert_eq!(f.style, Some("italic".to_string()));
        assert_eq!(f.display, Some("swap".to_string()));
        assert_eq!(f.unicode_range, Some("U+0000-007F, U+0400-04FF".to_string()));
    }

    #[test]
    fn at_font_face_no_family_skipped() {
        // Без font-family декларации правило невалидно.
        let s = parse(r#"
            @font-face { src: url("x.woff2"); }
            p { color: red; }
        "#);
        assert!(s.font_faces.is_empty());
        // Обычное правило за ним парсится.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_font_face_unquoted_family() {
        // Допустимо: font-family без кавычек.
        let s = parse("@font-face { font-family: Roboto; src: url(r.ttf); }");
        assert_eq!(s.font_faces[0].family, "Roboto");
        assert_eq!(s.font_faces[0].sources[0].value, "r.ttf");
    }

    #[test]
    fn at_font_face_cyrillic_family() {
        let s = parse(r#"
            @font-face { font-family: "Гранит"; src: url("granit.woff2"); }
        "#);
        assert_eq!(s.font_faces[0].family, "Гранит");
    }

    #[test]
    fn at_font_face_stretch_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Condensed";
                src: url("cond.woff2");
                font-stretch: condensed;
            }
        "#);
        assert_eq!(s.font_faces[0].stretch, Some("condensed".to_string()));
    }

    #[test]
    fn at_font_face_stretch_range() {
        // CSS Fonts L4: font-stretch принимает два значения (диапазон).
        let s = parse(r#"
            @font-face {
                font-family: "VarFont";
                src: url("var.woff2");
                font-stretch: 75% 125%;
            }
        "#);
        assert_eq!(s.font_faces[0].stretch, Some("75% 125%".to_string()));
    }

    #[test]
    fn at_font_face_variant_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "SmallCaps";
                src: url("sc.woff2");
                font-variant: small-caps;
            }
        "#);
        assert_eq!(s.font_faces[0].variant, Some("small-caps".to_string()));
    }

    #[test]
    fn at_font_face_feature_settings_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Ligatured";
                src: url("lig.woff2");
                font-feature-settings: "liga" 1, "kern" 0;
            }
        "#);
        assert_eq!(
            s.font_faces[0].feature_settings,
            Some(r#""liga" 1, "kern" 0"#.to_string())
        );
    }

    #[test]
    fn at_font_face_variation_settings_descriptor() {
        let s = parse(r#"
            @font-face {
                font-family: "Variable";
                src: url("variable.woff2");
                font-variation-settings: "wght" 400, "ital" 1;
            }
        "#);
        assert_eq!(
            s.font_faces[0].variation_settings,
            Some(r#""wght" 400, "ital" 1"#.to_string())
        );
    }

    #[test]
    fn at_font_face_all_l4_descriptors() {
        // Полный набор CSS Fonts L4 дескрипторов в одном правиле.
        let s = parse(r#"
            @font-face {
                font-family: "FullSpec";
                src: url("full.woff2") format("woff2");
                font-weight: 100 900;
                font-style: oblique 20deg 50deg;
                font-stretch: 75% 125%;
                font-display: swap;
                unicode-range: U+0000-007F;
                font-variant: small-caps;
                font-feature-settings: "liga" 1;
                font-variation-settings: "wght" 700;
            }
        "#);
        let f = &s.font_faces[0];
        assert_eq!(f.family, "FullSpec");
        assert_eq!(f.weight, Some("100 900".to_string()));
        assert_eq!(f.style, Some("oblique 20deg 50deg".to_string()));
        assert_eq!(f.stretch, Some("75% 125%".to_string()));
        assert_eq!(f.display, Some("swap".to_string()));
        assert_eq!(f.unicode_range, Some("U+0000-007F".to_string()));
        assert_eq!(f.variant, Some("small-caps".to_string()));
        assert_eq!(f.feature_settings, Some("\"liga\" 1".to_string()));
        assert_eq!(f.variation_settings, Some("\"wght\" 700".to_string()));
    }

    #[test]
    fn split_top_level_commas_respects_parens_and_strings() {
        // Запятые внутри (...) и "..." не должны разделять.
        assert_eq!(
            split_top_level_commas("a, b(c, d), e \"f, g\", h"),
            vec!["a", " b(c, d)", " e \"f, g\"", " h"]
        );
    }

    #[test]
    fn parse_font_face_src_local_only() {
        let srcs = parse_font_face_src("local(\"Times New Roman\")");
        assert_eq!(srcs.len(), 1);
        assert_eq!(srcs[0].kind, FontFaceSourceKind::Local);
        assert_eq!(srcs[0].value, "Times New Roman");
        assert_eq!(srcs[0].format, None);
    }

    // ── @layer (CSS Cascade L5 §6.4) ──

    #[test]
    fn at_layer_statement_form_single_name() {
        let s = parse("@layer base;");
        assert_eq!(s.layer_order, vec!["base".to_string()]);
        assert!(s.layers.is_empty());
    }

    #[test]
    fn at_layer_statement_form_multiple_names() {
        let s = parse("@layer base, components, utilities;");
        assert_eq!(
            s.layer_order,
            vec!["base".to_string(), "components".to_string(), "utilities".to_string()]
        );
    }

    #[test]
    fn at_layer_block_form_with_name() {
        let s = parse(r#"
            @layer base {
                p { color: red; }
            }
        "#);
        assert_eq!(s.layer_order, vec!["base".to_string()]);
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "base");
        assert_eq!(s.layers[0].rules.len(), 1);
    }

    #[test]
    fn at_layer_block_form_anonymous() {
        let s = parse(r#"
            @layer {
                p { color: red; }
            }
        "#);
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "__anon_1__");
        assert_eq!(s.layer_order, vec!["__anon_1__".to_string()]);
    }

    #[test]
    fn at_layer_block_does_not_duplicate_in_order() {
        // Если статикой объявили `@layer base;`, а потом блок `@layer base { ... }`,
        // имя в layer_order должно быть один раз (idempotent insert).
        let s = parse(r#"
            @layer base;
            @layer base { p { color: red; } }
        "#);
        assert_eq!(s.layer_order, vec!["base".to_string()]);
    }

    #[test]
    fn at_layer_multiple_anon_blocks_get_unique_names() {
        let s = parse(r#"
            @layer { p { color: red; } }
            @layer { p { color: blue; } }
        "#);
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].name, "__anon_1__");
        assert_eq!(s.layers[1].name, "__anon_2__");
    }

    #[test]
    fn at_layer_mixed_form_order_preserved() {
        let s = parse(r#"
            @layer reset, base;
            @layer components { p { color: blue; } }
            @layer base { p { color: red; } }
        "#);
        // layer_order сохраняет порядок _первого_ упоминания.
        assert_eq!(
            s.layer_order,
            vec![
                "reset".to_string(),
                "base".to_string(),
                "components".to_string(),
            ]
        );
        // А layers содержит block-form правил (2 шт).
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].name, "components");
        assert_eq!(s.layers[1].name, "base");
    }

    #[test]
    fn at_layer_dotted_subname_ok() {
        // sub-layer-имя `base.text` — валидно.
        let s = parse("@layer base.text;");
        assert_eq!(s.layer_order, vec!["base.text".to_string()]);
    }

    #[test]
    fn at_layer_unlayered_rules_kept_separately() {
        let s = parse(r#"
            @layer base { p { color: red; } }
            div { color: blue; }
        "#);
        // Layered: p in base.
        assert_eq!(s.layers.len(), 1);
        // Unlayered: top-level div.
        assert_eq!(s.rules.len(), 1);
    }

    #[test]
    fn at_layer_invalid_name_skipped() {
        // `1invalid` начинается с цифры → не CSS-ident → пропускается.
        let s = parse("@layer 1invalid, valid;");
        assert_eq!(s.layer_order, vec!["valid".to_string()]);
    }

    #[test]
    fn is_layer_name_basic() {
        assert!(is_layer_name("base"));
        assert!(is_layer_name("base.text"));
        assert!(is_layer_name("_priv"));
        assert!(!is_layer_name("1invalid"));
        assert!(!is_layer_name(""));
        assert!(!is_layer_name("with space"));
    }

    // ── CSS Conditional Rules L3 §2 — @supports ──

    #[test]
    fn at_supports_simple_decl() {
        let s = parse("@supports (display: grid) { p { color: red; } }");
        assert_eq!(s.supports_rules.len(), 1);
        let r = &s.supports_rules[0];
        match &r.condition {
            SupportsCondition::Decl { property, value } => {
                assert_eq!(property, "display");
                assert_eq!(value, "grid");
            }
            other => panic!("expected Decl, got {other:?}"),
        }
        assert_eq!(r.rules.len(), 1);
    }

    #[test]
    fn at_supports_and_combinator() {
        let s = parse("@supports (display: grid) and (color: red) { p { color: red; } }");
        let r = &s.supports_rules[0];
        match &r.condition {
            SupportsCondition::And(terms) => assert_eq!(terms.len(), 2),
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_or_combinator() {
        let s = parse("@supports (display: flex) or (display: -webkit-flex) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Or(terms) => assert_eq!(terms.len(), 2),
            other => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_negation() {
        let s = parse("@supports not (display: pancake) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Not(inner) => match inner.as_ref() {
                SupportsCondition::Decl { property, .. } => assert_eq!(property, "display"),
                other => panic!("expected Decl inside Not, got {other:?}"),
            },
            other => panic!("expected Not, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_selector_test() {
        let s = parse("@supports selector(:has(a)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Selector(sel) => assert!(sel.contains(":has(a)")),
            other => panic!("expected Selector, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_evaluate_known_property() {
        let cond = parse_supports_condition("(display: grid)");
        assert!(cond.evaluate(&["display", "color"]));
        assert!(!cond.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_evaluate_and() {
        let cond = parse_supports_condition("(display: grid) and (color: red)");
        assert!(cond.evaluate(&["display", "color"]));
        assert!(!cond.evaluate(&["display"]));
    }

    #[test]
    fn at_supports_evaluate_or() {
        let cond = parse_supports_condition("(unknown: x) or (color: red)");
        assert!(cond.evaluate(&["color"]));
        assert!(!cond.evaluate(&["other"]));
    }

    #[test]
    fn at_supports_evaluate_not() {
        let cond = parse_supports_condition("not (unknown: x)");
        assert!(cond.evaluate(&["color"]));
        let cond2 = parse_supports_condition("not (color: red)");
        assert!(!cond2.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_nested_grouping() {
        // `((display: grid))` — внутренние скобки = nested condition.
        let s = parse("@supports ((display: grid)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Decl { property, .. } => assert_eq!(property, "display"),
            other => panic!("expected Decl after unwrapping, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_value_with_parens_balanced() {
        let s = parse("@supports (color: rgba(0, 0, 0, 0.5)) { p { color: red; } }");
        match &s.supports_rules[0].condition {
            SupportsCondition::Decl { property, value } => {
                assert_eq!(property, "color");
                assert!(value.contains("rgba"));
            }
            other => panic!("expected Decl, got {other:?}"),
        }
    }

    #[test]
    fn at_supports_evaluator_selector_returns_false() {
        let cond = parse_supports_condition("selector(:has(a))");
        // Phase 0 не оценивает selector() — всегда false.
        assert!(!cond.evaluate(&["color"]));
    }

    #[test]
    fn at_supports_empty_returns_unknown() {
        let cond = parse_supports_condition("");
        assert!(matches!(cond, SupportsCondition::Unknown));
        assert!(!cond.evaluate(&["color"]));
    }

    // ── CSS Animations L1 §3 — @keyframes ──

    #[test]
    fn at_keyframes_from_to() {
        let s = parse("@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(s.keyframes.len(), 1);
        let kf = &s.keyframes[0];
        assert_eq!(kf.name, "fade");
        assert_eq!(kf.frames.len(), 2);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 1.0).abs() < 1e-6);
        assert_eq!(kf.frames[0].declarations[0].property, "opacity");
    }

    #[test]
    fn at_keyframes_percentages() {
        let s = parse("@keyframes pulse { 0% { color: red; } 50% { color: blue; } 100% { color: red; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 3);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 0.5).abs() < 1e-6);
        assert!((kf.frames[2].offset - 1.0).abs() < 1e-6);
    }

    #[test]
    fn at_keyframes_multiple_offsets_per_frame() {
        // `0%, 50%` — один блок с двумя offset-ами, разворачивается.
        let s = parse("@keyframes z { 0%, 50% { color: red; } 100% { color: blue; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 3);
        assert!((kf.frames[0].offset - 0.0).abs() < 1e-6);
        assert!((kf.frames[1].offset - 0.5).abs() < 1e-6);
        // Декларации одинаковые между развёрнутыми frame-ами.
        assert_eq!(kf.frames[0].declarations[0].value, "red");
        assert_eq!(kf.frames[1].declarations[0].value, "red");
    }

    #[test]
    fn at_keyframes_webkit_prefix() {
        let s = parse("@-webkit-keyframes fade { from { x: 0; } }");
        assert_eq!(s.keyframes.len(), 1);
        assert_eq!(s.keyframes[0].name, "fade");
    }

    #[test]
    fn at_keyframes_invalid_offset_skipped() {
        // 150% > 100% → пропускается.
        let s = parse("@keyframes z { 0% { x: 1; } 150% { x: 2; } 100% { x: 3; } }");
        let kf = &s.keyframes[0];
        assert_eq!(kf.frames.len(), 2);
    }

    #[test]
    fn at_keyframes_empty_block() {
        let s = parse("@keyframes z { }");
        assert_eq!(s.keyframes.len(), 1);
        assert_eq!(s.keyframes[0].frames.len(), 0);
    }

    #[test]
    fn parse_keyframe_selectors_handles_keywords_and_percents() {
        assert_eq!(parse_keyframe_selectors("from"), vec![0.0]);
        assert_eq!(parse_keyframe_selectors("to"), vec![1.0]);
        assert_eq!(parse_keyframe_selectors("From"), vec![0.0]); // case-insensitive
        assert_eq!(parse_keyframe_selectors("0%, 50%, 100%"), vec![0.0, 0.5, 1.0]);
        assert_eq!(parse_keyframe_selectors("bogus"), Vec::<f32>::new());
        assert_eq!(parse_keyframe_selectors("-10%"), Vec::<f32>::new());
        assert_eq!(parse_keyframe_selectors("150%"), Vec::<f32>::new());
    }

    // ── CSS Counter Styles L3 §2 — @counter-style ──

    #[test]
    fn at_counter_style_basic() {
        let s = parse(
            "@counter-style thumbs { system: cyclic; symbols: \"\\1F44D\"; suffix: \" \"; }",
        );
        assert_eq!(s.counter_styles.len(), 1);
        let cs = &s.counter_styles[0];
        assert_eq!(cs.name, "thumbs");
        assert_eq!(cs.declarations.len(), 3);
        assert_eq!(cs.declarations[0].property, "system");
    }

    #[test]
    fn at_counter_style_empty_block() {
        let s = parse("@counter-style empty { }");
        assert_eq!(s.counter_styles.len(), 1);
        assert!(s.counter_styles[0].declarations.is_empty());
    }

    // ── CSS Paged Media L3 §3 — @page ──

    #[test]
    fn at_page_no_selector() {
        let s = parse("@page { margin: 2cm; }");
        assert_eq!(s.page_rules.len(), 1);
        let p = &s.page_rules[0];
        assert!(p.selector.is_empty());
        assert_eq!(p.declarations[0].property, "margin");
    }

    #[test]
    fn at_page_pseudo_selector() {
        let s = parse("@page :first { margin-top: 4cm; }");
        let p = &s.page_rules[0];
        assert_eq!(p.selector, ":first");
        assert_eq!(p.declarations.len(), 1);
    }

    #[test]
    fn at_page_named_selector() {
        let s = parse("@page cover :left { margin: 0; }");
        assert_eq!(s.page_rules[0].selector, "cover :left");
    }

    // ── CSS Cascade L6 — @scope ──

    #[test]
    fn at_scope_root_only() {
        let s = parse("@scope (.card) { h1 { color: red; } }");
        assert_eq!(s.scope_rules.len(), 1);
        let sc = &s.scope_rules[0];
        assert_eq!(sc.root, ".card");
        assert_eq!(sc.limit, None);
        assert_eq!(sc.rules.len(), 1);
    }

    #[test]
    fn at_scope_root_and_limit() {
        let s = parse("@scope (.card) to (.footer) { p { color: blue; } }");
        let sc = &s.scope_rules[0];
        assert_eq!(sc.root, ".card");
        assert_eq!(sc.limit.as_deref(), Some(".footer"));
    }

    #[test]
    fn at_scope_implicit() {
        let s = parse("@scope { h1 { color: red; } }");
        let sc = &s.scope_rules[0];
        assert!(sc.root.is_empty());
        assert_eq!(sc.limit, None);
        assert_eq!(sc.rules.len(), 1);
    }

    // ── CSS Transitions L2 §3.4 — @starting-style ──

    #[test]
    fn at_starting_style_basic() {
        let s = parse("@starting-style { dialog { opacity: 0; } }");
        assert_eq!(s.starting_style_rules.len(), 1);
        assert_eq!(s.starting_style_rules[0].rules.len(), 1);
    }

    #[test]
    fn at_starting_style_empty() {
        let s = parse("@starting-style { }");
        assert_eq!(s.starting_style_rules.len(), 1);
        assert!(s.starting_style_rules[0].rules.is_empty());
    }

    // ── CSS Containment L3 §3 — @container ──

    #[test]
    fn at_container_anonymous() {
        let s = parse("@container (min-width: 300px) { p { color: red; } }");
        assert_eq!(s.container_rules.len(), 1);
        let c = &s.container_rules[0];
        assert_eq!(c.name, None);
        assert!(c.condition.contains("min-width"));
        assert_eq!(c.rules.len(), 1);
    }

    #[test]
    fn at_container_named() {
        let s = parse("@container sidebar (min-width: 200px) { h1 { color: blue; } }");
        let c = &s.container_rules[0];
        assert_eq!(c.name.as_deref(), Some("sidebar"));
    }

    #[test]
    fn at_container_complex_condition() {
        let s = parse("@container (min-width: 200px) and (max-width: 600px) { p { } }");
        let c = &s.container_rules[0];
        assert!(c.condition.contains("and"));
    }

    // ── Media Queries L4 §3.2: not / only / prefers-color-scheme ──

    fn screen_ctx(width: f32) -> MediaContext {
        MediaContext {
            media_type: "screen".into(),
            width,
            height: 600.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            ..Default::default()
        }
    }

    #[test]
    fn media_query_only_parses_as_no_op() {
        let q = parse_media_query("only screen and (min-width: 300px)");
        assert_eq!(q.clauses.len(), 1);
        assert!(!q.clauses[0].negated);
        // `only screen` + `and (min-width: 300px)` → 2 условия.
        assert_eq!(q.clauses[0].conditions.len(), 2);
        assert!(q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_only_keyword_does_not_eat_media_type() {
        // Forward-compat: `only` без следующего media-type / feature
        // оставляет clause пустым → Unsupported.
        let q = parse_media_query("only");
        assert_eq!(q.clauses.len(), 1);
        assert_eq!(q.clauses[0].conditions, vec![MediaCondition::Unsupported]);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_inverts_match() {
        let q = parse_media_query("not screen");
        assert_eq!(q.clauses.len(), 1);
        assert!(q.clauses[0].negated);
        // screen-context — не матчит `not screen`.
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_matches_when_inner_false() {
        // not (min-width: 1000px) → инвертит «не достаточно широкий».
        let q = parse_media_query("not all and (min-width: 1000px)");
        assert!(q.clauses[0].negated);
        assert!(q.matches(&screen_ctx(500.0)));
        assert!(!q.matches(&screen_ctx(1200.0)));
    }

    #[test]
    fn media_query_not_with_unsupported_stays_unknown() {
        // Per §3.2: `not (unknown-feature: x)` → unknown, не true.
        let q = parse_media_query("not all and (gibberish: zzz)");
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_only_first_keyword_consumed() {
        // `not not` — второй not трактуется как невалидный токен → clause unknown.
        let q = parse_media_query("not not screen");
        assert!(q.clauses[0].negated);
        assert_eq!(q.clauses[0].conditions, vec![MediaCondition::Unsupported]);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_or_with_not_clause() {
        // `not screen, print` — на screen НЕ должно матчить (not screen → false на screen);
        // на print должно матчить (print clause = MediaType(print)).
        let q = parse_media_query("not screen, print");
        assert_eq!(q.clauses.len(), 2);
        assert!(q.clauses[0].negated);
        assert!(!q.clauses[1].negated);
        assert!(!q.matches(&screen_ctx(500.0)));
        let mut print_ctx = screen_ctx(500.0);
        print_ctx.media_type = "print".into();
        assert!(q.matches(&print_ctx));
    }

    #[test]
    fn media_query_not_keyword_must_be_separated() {
        // `notepad` (или другой ident, начинающийся с `not`) — НЕ keyword.
        let q = parse_media_query("notepad");
        // Trim+lower → media-type "notepad". Не матчит на screen.
        assert!(!q.clauses[0].negated);
        assert_eq!(q.clauses[0].conditions.len(), 1);
    }

    #[test]
    fn media_query_prefers_color_scheme_light_default() {
        let q = parse_media_query("(prefers-color-scheme: light)");
        assert!(q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_prefers_color_scheme_dark_matches_when_dark() {
        let q = parse_media_query("(prefers-color-scheme: dark)");
        let mut ctx = screen_ctx(500.0);
        ctx.prefers_dark = true;
        assert!(q.matches(&ctx));
        ctx.prefers_dark = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_not_prefers_dark() {
        // На светлой теме `not (prefers-color-scheme: dark)` должно матчить.
        let q = parse_media_query("not all and (prefers-color-scheme: dark)");
        assert!(q.clauses[0].negated);
        assert!(q.matches(&screen_ctx(500.0)));
        let mut dark = screen_ctx(500.0);
        dark.prefers_dark = true;
        assert!(!q.matches(&dark));
    }

    // ── MQ L3 §4: exact width/height, em/rem units ──

    #[test]
    fn media_query_width_exact_px() {
        let q = parse_media_query("(width: 1024px)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(q.matches(&ctx));
        ctx.width = 800.0;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_height_exact_px() {
        let q = parse_media_query("(height: 720px)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(q.matches(&ctx));
        ctx.height = 600.0;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_min_width_em() {
        // 48em = 48 * 16 = 768px
        let q = parse_media_query("(min-width: 48em)");
        assert!(q.matches(&screen_ctx(1024.0)));
        assert!(!q.matches(&screen_ctx(600.0)));
    }

    #[test]
    fn media_query_max_width_rem() {
        // 50rem = 50 * 16 = 800px
        let q = parse_media_query("(max-width: 50rem)");
        assert!(q.matches(&screen_ctx(600.0)));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_min_height_em() {
        // 30em = 30 * 16 = 480px
        let q = parse_media_query("(min-height: 30em)");
        let mut ctx = screen_ctx(800.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.height = 400.0;
        assert!(!q.matches(&ctx));
    }

    // ── MQ L3 §4.3: aspect-ratio ──

    #[test]
    fn media_query_min_aspect_ratio() {
        // min-aspect-ratio: 16/9 ≈ 1.777; 1024/720 ≈ 1.422 → не матчит
        let q = parse_media_query("(min-aspect-ratio: 16/9)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(!q.matches(&ctx)); // 1.422 < 1.777
        ctx.width = 1920.0;
        ctx.height = 720.0;
        assert!(q.matches(&ctx)); // 2.666 >= 1.777
    }

    #[test]
    fn media_query_max_aspect_ratio() {
        // max-aspect-ratio: 4/3 ≈ 1.333; 800/600 ≈ 1.333 → матчит
        let q = parse_media_query("(max-aspect-ratio: 4/3)");
        let mut ctx = screen_ctx(800.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.width = 1920.0;
        assert!(!q.matches(&ctx)); // 3.2 > 1.333
    }

    #[test]
    fn media_query_aspect_ratio_exact() {
        // aspect-ratio: 1/1 → квадрат
        let q = parse_media_query("(aspect-ratio: 1/1)");
        let mut ctx = screen_ctx(600.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.width = 800.0;
        assert!(!q.matches(&ctx));
    }

    // ── MQ L5 §6.4: prefers-reduced-motion ──

    #[test]
    fn media_query_prefers_reduced_motion_reduce() {
        let q = parse_media_query("(prefers-reduced-motion: reduce)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_motion = true;
        assert!(q.matches(&ctx));
        ctx.prefers_reduced_motion = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_reduced_motion_no_preference() {
        let q = parse_media_query("(prefers-reduced-motion: no-preference)");
        let ctx = screen_ctx(1024.0); // prefers_reduced_motion = false по умолчанию
        assert!(q.matches(&ctx));
    }

    // ── MQ: forced-colors (CSS Forced Colors Mode L1) ──

    #[test]
    fn media_query_forced_colors_active() {
        let q = parse_media_query("(forced-colors: active)");
        let mut ctx = screen_ctx(1024.0);
        ctx.forced_colors = true;
        assert!(q.matches(&ctx));
        ctx.forced_colors = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_forced_colors_none() {
        let q = parse_media_query("(forced-colors: none)");
        let ctx = screen_ctx(1024.0); // forced_colors = false по умолчанию
        assert!(q.matches(&ctx));
        let mut active = screen_ctx(1024.0);
        active.forced_colors = true;
        assert!(!q.matches(&active));
    }

    #[test]
    fn media_query_not_forced_colors_active() {
        let q = parse_media_query("not all and (forced-colors: active)");
        assert!(q.clauses[0].negated);
        let ctx = screen_ctx(1024.0); // forced_colors = false
        assert!(q.matches(&ctx));
        let mut active = screen_ctx(1024.0);
        active.forced_colors = true;
        assert!(!q.matches(&active));
    }

    #[test]
    fn media_query_forced_colors_case_insensitive() {
        let q = parse_media_query("(forced-colors: ACTIVE)");
        let mut ctx = screen_ctx(1024.0);
        ctx.forced_colors = true;
        assert!(q.matches(&ctx));
    }

    // ── MQ: hover / any-hover / pointer / any-pointer (Media Queries L4 §5.3-5.6) ──

    #[test]
    fn media_query_hover_hover_matches_desktop() {
        // screen_ctx наследует desktop-дефолты (hover: Hover).
        let q = parse_media_query("(hover: hover)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.hover = MediaHover::None;
        assert!(!q.matches(&touch));
    }

    #[test]
    fn media_query_hover_none() {
        let q = parse_media_query("(hover: none)");
        assert!(!q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.hover = MediaHover::None;
        assert!(q.matches(&touch));
    }

    #[test]
    fn media_query_any_hover() {
        let q = parse_media_query("(any-hover: hover)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.any_hover = MediaHover::None;
        assert!(!q.matches(&touch));
    }

    #[test]
    fn media_query_pointer_fine_matches_desktop() {
        let q = parse_media_query("(pointer: fine)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut coarse = screen_ctx(1024.0);
        coarse.pointer = MediaPointer::Coarse;
        assert!(!q.matches(&coarse));
    }

    #[test]
    fn media_query_pointer_coarse_and_none() {
        let coarse_q = parse_media_query("(pointer: coarse)");
        let none_q = parse_media_query("(pointer: none)");
        let mut ctx = screen_ctx(1024.0);
        ctx.pointer = MediaPointer::Coarse;
        assert!(coarse_q.matches(&ctx));
        assert!(!none_q.matches(&ctx));
        ctx.pointer = MediaPointer::None;
        assert!(none_q.matches(&ctx));
    }

    #[test]
    fn media_query_any_pointer() {
        let q = parse_media_query("(any-pointer: fine)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut coarse = screen_ctx(1024.0);
        coarse.any_pointer = MediaPointer::Coarse;
        assert!(!q.matches(&coarse));
    }

    #[test]
    fn media_query_hover_pointer_case_insensitive() {
        let q = parse_media_query("(POINTER: FINE)");
        assert!(q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_pointer_invalid_value_unsupported() {
        // Невалидное значение → Unsupported → clause никогда не матчит.
        let q = parse_media_query("(pointer: medium)");
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    // ── MQ L5 §5.5/§5.6: prefers-contrast / prefers-reduced-data ──

    #[test]
    fn media_query_prefers_contrast_no_preference_default() {
        // screen_ctx наследует desktop-дефолт (no-preference).
        let q = parse_media_query("(prefers-contrast: no-preference)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut more = screen_ctx(1024.0);
        more.prefers_contrast = MediaContrast::More;
        assert!(!q.matches(&more));
    }

    #[test]
    fn media_query_prefers_contrast_more_and_less() {
        let more_q = parse_media_query("(prefers-contrast: more)");
        let less_q = parse_media_query("(prefers-contrast: less)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::More;
        assert!(more_q.matches(&ctx));
        assert!(!less_q.matches(&ctx));
        ctx.prefers_contrast = MediaContrast::Less;
        assert!(less_q.matches(&ctx));
        assert!(!more_q.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_contrast_custom() {
        let q = parse_media_query("(prefers-contrast: custom)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::Custom;
        assert!(q.matches(&ctx));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_prefers_contrast_case_insensitive_and_invalid() {
        let q = parse_media_query("(PREFERS-CONTRAST: MORE)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::More;
        assert!(q.matches(&ctx));
        // Невалидное значение → Unsupported → никогда не матчит.
        let bad = parse_media_query("(prefers-contrast: high)");
        assert!(!bad.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_reduced_data_reduce() {
        let q = parse_media_query("(prefers-reduced-data: reduce)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_data = MediaReducedData::Reduce;
        assert!(q.matches(&ctx));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_prefers_reduced_data_no_preference_default() {
        let q = parse_media_query("(prefers-reduced-data: no-preference)");
        // Desktop-дефолт — no-preference.
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut reduce = screen_ctx(1024.0);
        reduce.prefers_reduced_data = MediaReducedData::Reduce;
        assert!(!q.matches(&reduce));
    }

    // ── Стиль: @media с новыми фичами применяется в каскаде ──

    #[test]
    fn media_rule_with_em_width_applies_in_layout() {
        // Парсинг: @media (min-width: 48em) - должен создать MediaRule с query.
        let s = parse("@media (min-width: 48em) { p { color: red; } }");
        assert_eq!(s.media_rules.len(), 1);
        let ctx = MediaContext {
            media_type: "screen".into(),
            width: 1024.0, // > 768px (48em)
            height: 720.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            ..Default::default()
        };
        assert!(s.media_rules[0].query.matches(&ctx));
        let ctx_narrow = MediaContext { width: 600.0, ..ctx.clone() };
        assert!(!s.media_rules[0].query.matches(&ctx_narrow));
    }

    #[test]
    fn inline_style_single_declaration() {
        let decls = parse_inline_style("color: red");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "red");
        assert!(!decls[0].important);
    }

    #[test]
    fn inline_style_multiple_declarations_with_trailing_semicolon() {
        let decls = parse_inline_style("color: red; background: #fff; padding: 5px 10px;");
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[1].property, "background");
        assert_eq!(decls[1].value, "#fff");
        assert_eq!(decls[2].property, "padding");
        assert_eq!(decls[2].value, "5px 10px");
    }

    #[test]
    fn inline_style_no_trailing_semicolon() {
        let decls = parse_inline_style("width: 100px; height: 50px");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[1].property, "height");
        assert_eq!(decls[1].value, "50px");
    }

    #[test]
    fn inline_style_important_flag() {
        let decls = parse_inline_style("color: red !important");
        assert_eq!(decls.len(), 1);
        assert!(decls[0].important);
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn inline_style_empty_input() {
        assert!(parse_inline_style("").is_empty());
        assert!(parse_inline_style("   ").is_empty());
        assert!(parse_inline_style(";;;").is_empty());
    }

    #[test]
    fn inline_style_recovers_from_invalid_declaration() {
        let decls = parse_inline_style("color: red; garbage no colon here; background: blue");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[1].property, "background");
    }

    #[test]
    fn inline_style_with_url_and_quotes() {
        let decls = parse_inline_style(
            r#"background-image: url("a;b.png"); content: 'hi; there'"#,
        );
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].value, r#"url("a;b.png")"#);
        assert_eq!(decls[1].value, "'hi; there'");
    }

    // --- CSS Nesting L1 ---

    fn two(a: SimpleSelector, comb: Combinator, b: SimpleSelector) -> ComplexSelector {
        ComplexSelector {
            head: CompoundSelector { parts: vec![a] },
            tail: vec![(comb, CompoundSelector { parts: vec![b] })],
        }
    }

    #[test]
    fn nesting_descendant_simple() {
        // `div { color: red; & span { color: blue; } }` →
        // 2 rules: div { color: red } and div span { color: blue }
        let s = parse("div { color: red; & span { color: blue; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
        assert_eq!(
            s.rules[1].selectors,
            vec![two(SimpleSelector::Type("div".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))]
        );
        assert_eq!(s.rules[1].declarations[0].property, "color");
        assert_eq!(s.rules[1].declarations[0].value, "blue");
    }

    #[test]
    fn nesting_child_combinator() {
        // `ul { & > li { list-style: none; } }`
        let s = parse("ul { & > li { list-style: none; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(
            s.rules[1].selectors,
            vec![two(SimpleSelector::Type("ul".into()), Combinator::Child, SimpleSelector::Type("li".into()))]
        );
        assert_eq!(s.rules[1].declarations[0].property, "list-style");
    }

    #[test]
    fn nesting_compound_join() {
        // `div { &.active { color: red; } }` → `div.active { color: red; }`
        let s = parse("div { &.active { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let sel = &s.rules[1].selectors[0];
        assert_eq!(sel.head.parts, vec![
            SimpleSelector::Type("div".into()),
            SimpleSelector::Class("active".into()),
        ]);
        assert!(sel.tail.is_empty());
    }

    #[test]
    fn nesting_bare_amp() {
        // `div { & { color: red; } }` → `div { color: red; }` (same element)
        let s = parse("div { color: blue; & { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(s.rules[1].declarations[0].value, "red");
    }

    #[test]
    fn nesting_multiple_parent_selectors() {
        // `h1, h2 { & span { color: red; } }` → `h1 span` and `h2 span`
        let s = parse("h1, h2 { & span { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested_sels = &s.rules[1].selectors;
        assert_eq!(nested_sels.len(), 2);
        assert_eq!(
            nested_sels[0],
            two(SimpleSelector::Type("h1".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))
        );
        assert_eq!(
            nested_sels[1],
            two(SimpleSelector::Type("h2".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))
        );
    }

    #[test]
    fn nesting_deep_two_levels() {
        // `div { & p { & em { color: red; } } }` → 3 rules: div, div p, div p em
        let s = parse("div { & p { & em { color: red; } } }");
        assert_eq!(s.rules.len(), 3);
        // div p em
        let sel = &s.rules[2].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(sel.tail.len(), 2);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("p".into())]);
        assert_eq!(sel.tail[1].0, Combinator::Descendant);
        assert_eq!(sel.tail[1].1.parts, vec![SimpleSelector::Type("em".into())]);
    }

    #[test]
    fn nesting_declarations_not_mixed_with_nested() {
        // Declarations and nested rules don't interfere
        let s = parse("p { margin: 0; & b { font-weight: bold; } padding: 5px; }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].declarations.len(), 2); // margin + padding
        assert_eq!(s.rules[0].declarations[0].property, "margin");
        assert_eq!(s.rules[0].declarations[1].property, "padding");
        assert_eq!(s.rules[1].declarations[0].property, "font-weight");
    }

    // ── CSS Nesting L1 §4: implicit nesting (without `&`) ───────────────────

    #[test]
    fn implicit_nesting_class_descendant() {
        // `.parent { .child { color: blue; } }` → `.parent .child { color: blue; }`
        let s = parse(".parent { .child { color: blue; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("parent".into()))]);
        assert!(s.rules[0].declarations.is_empty());
        // Nested rule: `.parent .child`
        let nested = &s.rules[1];
        assert_eq!(nested.selectors.len(), 1);
        assert_eq!(nested.selectors[0].head.parts, vec![SimpleSelector::Class("parent".into())]);
        assert_eq!(nested.selectors[0].tail.len(), 1);
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Class("child".into())]
        );
        assert_eq!(nested.declarations[0].property, "color");
        assert_eq!(nested.declarations[0].value, "blue");
    }

    #[test]
    fn implicit_nesting_id_descendant() {
        // `div { #hero { color: red; } }` → `div #hero { color: red; }`
        let s = parse("div { #hero { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Id("hero".into())]
        );
    }

    #[test]
    fn implicit_nesting_pseudo_class() {
        // `.btn { :hover { opacity: 0.8; } }` → `.btn :hover { opacity: 0.8; }`
        let s = parse(".btn { :hover { opacity: 0.8; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        // :hover is PseudoClass::Unsupported("hover") since not stateful-matched
        assert_eq!(nested.declarations[0].property, "opacity");
        assert_eq!(nested.declarations[0].value, "0.8");
    }

    #[test]
    fn implicit_nesting_universal() {
        // `div { * { box-sizing: border-box; } }` → `div * { box-sizing: border-box; }`
        let s = parse("div { * { box-sizing: border-box; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Universal]
        );
    }

    #[test]
    fn implicit_nesting_relative_child() {
        // `ul { > li { list-style: none; } }` → `ul > li { list-style: none; }`
        let s = parse("ul { > li { list-style: none; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Child);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Type("li".into())]
        );
        assert_eq!(nested.declarations[0].property, "list-style");
    }

    #[test]
    fn implicit_nesting_relative_next_sibling() {
        // `.a { + .b { color: red; } }` → `.a + .b { color: red; }`
        let s = parse(".a { + .b { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::NextSibling);
    }

    #[test]
    fn implicit_nesting_relative_later_sibling() {
        // `.a { ~ .b { color: red; } }` → `.a ~ .b { color: red; }`
        let s = parse(".a { ~ .b { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::LaterSibling);
    }

    #[test]
    fn implicit_nesting_with_declarations_mixed() {
        // `.card { color: red; .title { font-weight: bold; } padding: 8px; }`
        let s = parse(".card { color: red; .title { font-weight: bold; } padding: 8px; }");
        assert_eq!(s.rules.len(), 2);
        // Parent keeps both declarations
        assert_eq!(s.rules[0].declarations.len(), 2);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[1].property, "padding");
        // Nested rule gets its declaration
        assert_eq!(s.rules[1].declarations[0].property, "font-weight");
    }

    #[test]
    fn implicit_nesting_deep_two_levels() {
        // `div { .a { .b { color: red; } } }` → 3 rules: div, div .a, div .a .b
        let s = parse("div { .a { .b { color: red; } } }");
        assert_eq!(s.rules.len(), 3);
        let deepest = &s.rules[2];
        assert_eq!(deepest.selectors[0].head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(deepest.selectors[0].tail.len(), 2);
        assert_eq!(deepest.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(deepest.selectors[0].tail[1].0, Combinator::Descendant);
    }

    #[test]
    fn implicit_nesting_attribute_selector() {
        // `form { [required] { border-color: red; } }` → `form [required] { border-color: red; }`
        let s = parse("form { [required] { border-color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(nested.declarations[0].property, "border-color");
    }

    // ── CSS Nesting L1 §5: nested at-rules ─────────────────────────────────

    #[test]
    fn nested_at_media_basic() {
        // `.card { @media (min-width: 800px) { color: blue; } }`
        // → `@media (min-width: 800px) { .card { color: blue; } }` in media_rules
        let s = parse(".card { @media (min-width: 800px) { color: blue; } }");
        assert_eq!(s.rules.len(), 1); // parent rule (no decls)
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        assert_eq!(mr.rules.len(), 1);
        assert_eq!(mr.rules[0].selectors, vec![one(SimpleSelector::Class("card".into()))]);
        assert_eq!(mr.rules[0].declarations[0].property, "color");
        assert_eq!(mr.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn nested_at_media_with_nested_rule() {
        // `.parent { @media (max-width: 600px) { .child { color: red; } } }`
        // → `@media ... { .parent .child { color: red; } }`
        let s = parse(".parent { @media (max-width: 600px) { .child { color: red; } } }");
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        // .parent (empty decls rule from parent) is absent since decls empty; only the nested rule
        assert!(mr.rules.iter().any(|r| {
            r.selectors.iter().any(|sel| {
                sel.head.parts == vec![SimpleSelector::Class("parent".into())]
                    && sel.tail.len() == 1
                    && sel.tail[0].1.parts == vec![SimpleSelector::Class("child".into())]
            })
        }));
        let nested = mr.rules.iter().find(|r| r.selectors[0].tail.len() == 1).unwrap();
        assert_eq!(nested.declarations[0].property, "color");
    }

    #[test]
    fn nested_at_media_mixed_decls_and_rules() {
        // `.el { @media screen { color: red; .inner { opacity: 0.5; } } }`
        // → media_rules has: [.el { color: red }, .el .inner { opacity: 0.5 }]
        let s =
            parse(".el { @media screen { color: red; .inner { opacity: 0.5; } } }");
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        assert_eq!(mr.rules.len(), 2);
        // First rule: .el { color: red }
        assert_eq!(mr.rules[0].selectors, vec![one(SimpleSelector::Class("el".into()))]);
        assert_eq!(mr.rules[0].declarations[0].property, "color");
        // Second rule: .el .inner { opacity: 0.5 }
        assert_eq!(mr.rules[1].declarations[0].property, "opacity");
    }

    #[test]
    fn nested_at_supports() {
        // `div { @supports (display: grid) { display: grid; } }`
        // → `@supports ... { div { display: grid; } }` in supports_rules
        let s = parse("div { @supports (display: grid) { display: grid; } }");
        assert_eq!(s.supports_rules.len(), 1);
        let sr = &s.supports_rules[0];
        assert_eq!(sr.rules.len(), 1);
        assert_eq!(sr.rules[0].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(sr.rules[0].declarations[0].property, "display");
    }

    #[test]
    fn nested_at_layer() {
        // `.btn { @layer base { color: red; } }`
        // → `@layer base { .btn { color: red; } }` in layers
        let s = parse(".btn { @layer base { color: red; } }");
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "base");
        assert_eq!(s.layers[0].rules.len(), 1);
        assert_eq!(s.layers[0].rules[0].declarations[0].property, "color");
    }

    #[test]
    fn nested_at_container() {
        // `.grid { @container sidebar (min-width: 300px) { gap: 1rem; } }`
        let s = parse(".grid { @container sidebar (min-width: 300px) { gap: 1rem; } }");
        assert_eq!(s.container_rules.len(), 1);
        let cr = &s.container_rules[0];
        assert_eq!(cr.rules.len(), 1);
        assert_eq!(cr.rules[0].declarations[0].property, "gap");
    }

    // ──────────────── :host pseudo-class (CSS Scoping L1 §6.1) ────────────

    #[test]
    fn host_pseudo_class_simple() {
        // `:host { color: red; }` — простой :host без аргументов
        let s = parse(":host { color: red; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(None)) => {}
            _ => panic!("Expected Host(None), got {:?}", sel.head.parts[0]),
        }
        assert_eq!(s.rules[0].declarations[0].property, "color");
    }

    #[test]
    fn host_pseudo_class_with_selector_list() {
        // `:host(.foo) { display: block; }` — :host(selector-list)
        let s = parse(":host(.foo) { display: block; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("foo".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn host_pseudo_class_multiple_selectors_in_list() {
        // `:host(.primary, .secondary) { border: 1px solid; }` — multiple selectors
        let s = parse(":host(.primary, .secondary) { border: 1px solid; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("primary".into()));
                assert_eq!(list[1].head.parts[0], SimpleSelector::Class("secondary".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn host_pseudo_class_with_complex_selector() {
        // `:host(div.wrapper) { padding: 10px; }` — complex selector inside
        let s = parse(":host(div.wrapper) { padding: 10px; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Type("div".into()));
                assert_eq!(list[0].head.parts[1], SimpleSelector::Class("wrapper".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    // ────────────────── ::slotted pseudo-element (CSS Scoping L1 §6.2) ──

    #[test]
    fn slotted_pseudo_element_simple() {
        // `::slotted(.slot-content) { color: blue; }` — :slotted with selector
        let s = parse("::slotted(.slot-content) { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("slot-content".into()));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn slotted_pseudo_element_multiple_selectors() {
        // `::slotted(.primary, .secondary) { margin: 5px; }` — multiple selectors
        let s = parse("::slotted(.primary, .secondary) { margin: 5px; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("primary".into()));
                assert_eq!(list[1].head.parts[0], SimpleSelector::Class("secondary".into()));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn slotted_pseudo_element_with_type_selector() {
        // `::slotted(input[type="text"]) { border-color: green; }` — type selector with attribute
        let s = parse("::slotted(input[type=\"text\"]) { border-color: green; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Type("input".into()));
                assert!(list[0].head.parts.iter().any(|p| matches!(p, SimpleSelector::Attribute(_))));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    // ────────────────── ::highlight pseudo-element (CSS Highlight API L1 §3) ──

    #[test]
    fn highlight_pseudo_element_simple() {
        // `::highlight(search) { color: red; background: yellow; }` — simple name
        let s = parse("::highlight(search) { color: red; background: yellow; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "search");
            }
            _ => panic!("Expected Highlight(\"search\"), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn highlight_pseudo_element_custom_name() {
        // `::highlight(custom-highlight-name) { ... }` — hyphenated name
        let s = parse("::highlight(custom-highlight-name) { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "custom-highlight-name");
            }
            _ => panic!("Expected Highlight with name, got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn highlight_pseudo_element_with_combinator() {
        // `span::highlight(spelling) { color: red; }` — type selector + highlight
        let s = parse("span::highlight(spelling) { color: red; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.head.parts[0], SimpleSelector::Type("span".into()));
        match &sel.head.parts[1] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "spelling");
            }
            _ => panic!("Expected Highlight pseudo-element, got {:?}", sel.head.parts[1]),
        }
    }

    // ── @font-palette-values tests ──────────────────────────────────────────

    #[test]
    fn font_palette_values_basic() {
        let s = parse(r#"@font-palette-values --warm { font-family: "Bungee Spore"; base-palette: 0; }"#);
        assert_eq!(s.font_palette_values.len(), 1);
        let fp = &s.font_palette_values[0];
        assert_eq!(fp.name, "--warm");
        assert_eq!(fp.font_family.as_deref(), Some("Bungee Spore"));
        assert_eq!(fp.base_palette, Some(0));
        assert!(fp.override_colors.is_empty());
    }

    #[test]
    fn font_palette_values_override_colors() {
        let s = parse("@font-palette-values --cool { override-colors: 0 #ff0000, 1 #00ff00; }");
        let fp = &s.font_palette_values[0];
        assert_eq!(fp.override_colors.len(), 2);
        assert_eq!(fp.override_colors[0], (0, "#ff0000".to_string()));
        assert_eq!(fp.override_colors[1], (1, "#00ff00".to_string()));
    }

    #[test]
    fn font_palette_values_multiple_rules() {
        let s = parse(
            "@font-palette-values --a { base-palette: 1; } @font-palette-values --b { base-palette: 2; }",
        );
        assert_eq!(s.font_palette_values.len(), 2);
        assert_eq!(s.font_palette_values[0].name, "--a");
        assert_eq!(s.font_palette_values[1].name, "--b");
    }

    #[test]
    fn font_palette_values_no_double_dash_ignored() {
        // Prelude without '--' is invalid per CSS Fonts L4 §13 — treated as unknown.
        let s = parse("@font-palette-values myname { base-palette: 0; }");
        assert!(s.font_palette_values.is_empty());
    }

    #[test]
    fn font_palette_values_base_palette_none_when_absent() {
        let s = parse("@font-palette-values --x { font-family: F; }");
        assert_eq!(s.font_palette_values[0].base_palette, None);
    }

    #[test]
    fn font_palette_values_coexists_with_other_rules() {
        let s = parse(
            r#"div { color: red; } @font-palette-values --p { base-palette: 3; } p { margin: 0; }"#,
        );
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.font_palette_values.len(), 1);
        assert_eq!(s.font_palette_values[0].base_palette, Some(3));
    }
}
