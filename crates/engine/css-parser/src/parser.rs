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

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

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
fn compound_is_supported(c: &CompoundSelector) -> bool {
    c.parts.iter().all(simple_is_supported)
}

/// True если простой селектор распознаётся движком. Type/Class/Id/Universal/
/// Attribute поддержаны безусловно; псевдо делегируют в свои проверки.
fn simple_is_supported(s: &SimpleSelector) -> bool {
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
fn pseudo_class_is_supported(pc: &PseudoClass) -> bool {
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

/// True если псевдо-элемент распознан. `Unknown(_)` → `false`; `::slotted()`
/// рекурсивно проверяет свой аргумент-селектор.
fn pseudo_element_is_supported(pe: &PseudoElementKind) -> bool {
    match pe {
        PseudoElementKind::Unknown(_) => false,
        PseudoElementKind::Slotted(Some(list)) => list.iter().all(ComplexSelector::is_supported),
        _ => true,
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
        PseudoClass::State(name) => format!(":state({name})"),
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
        PseudoElementKind::Placeholder => "::placeholder".into(),
        PseudoElementKind::Highlight(name) => format!("::highlight({name})"),
        PseudoElementKind::Picker(name) => format!("::picker({name})"),
        PseudoElementKind::Checkmark => "::checkmark".into(),
        PseudoElementKind::PickerIcon => "::picker-icon".into(),
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

/// Process-unique identity of one `Stylesheet`'s **content**.
///
/// Exists so that a consumer caching something derived from a sheet (the
/// cascade's rule index, `lumen_layout::style`) can key that cache by
/// identity instead of by the sheet's address. An address is not an identity:
/// a freed sheet's address is handed straight back to the next allocation, so
/// an address-keyed cache has to be invalidated on every use to stay honest —
/// which is the same as having no cache across passes at all (BUG-341 S21).
///
/// A revision is minted fresh for every `Stylesheet` that comes into existence
/// (parse, `Default`, `Clone`) and is never reused, so two sheets can share one
/// only by one being a snapshot of the other before either was mutated. The
/// counter is `u64`: at one sheet per nanosecond it wraps in 584 years.
///
/// **The invariant a cache relies on**: while a sheet's revision is unchanged,
/// its rules are unchanged. Every in-place mutation must therefore go through
/// [`Stylesheet::merge_from`] or announce itself with
/// [`Stylesheet::mark_mutated`]. This is not left to review — the test
/// `every_stylesheet_mutation_in_the_workspace_announces_itself` scans the
/// workspace sources and fails the build on a direct `push`/`extend`/… into any
/// rule container outside this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StylesheetRevision(u64);

/// Source of [`StylesheetRevision`] values. Starts at 1 so that 0 is available
/// to consumers as a "no sheet seen yet" sentinel.
static NEXT_STYLESHEET_REVISION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl StylesheetRevision {
    /// Mints a revision no other `Stylesheet` has held or will hold.
    fn fresh() -> Self {
        Self(NEXT_STYLESHEET_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

#[derive(Debug)]
pub struct Stylesheet {
    /// Content identity — see [`StylesheetRevision`]. Private: it is minted,
    /// never chosen, and a hand-set value would silently license a stale cache.
    revision: StylesheetRevision,
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
    /// CSS Color L5 §4 — `@color-profile --name { src: ...; rendering-intent: ...; }`.
    /// Phase 0: parse+store. Matching against `color(--name ...)` and used-value
    /// resolution happen in layout (`resolve_color_profile`); real ICC transform
    /// is deferred — channels are treated as already-sRGB.
    pub color_profiles: Vec<ColorProfileRule>,
    /// CSS Functions and Mixins L1 — `@function --name(<params>) { decls }`.
    /// Author-defined custom function, invoked as `--name(<args>)` from any
    /// property value. Parsing covers positional parameters with optional
    /// defaults and a raw `returns` type; evaluation (positional argument
    /// binding, local `--x` declarations, `result` substitution) happens in
    /// layout (`expand_custom_functions`, style.rs). Conditional group rules
    /// inside the body (`@media`, `@container`) are not yet supported.
    pub function_rules: Vec<FunctionRule>,
}

impl Default for Stylesheet {
    /// An empty sheet — with its own revision, like any other sheet. Two
    /// `Stylesheet::default()` values are `==` but not the same sheet, and a
    /// cache keyed by revision must not confuse them: the first may be filled
    /// in afterwards through its public fields (which is what the workspace
    /// gate makes visible).
    fn default() -> Self {
        Self {
            revision: StylesheetRevision::fresh(),
            rules: Vec::new(),
            properties: Vec::new(),
            media_rules: Vec::new(),
            imports: Vec::new(),
            font_faces: Vec::new(),
            layer_order: Vec::new(),
            layers: Vec::new(),
            supports_rules: Vec::new(),
            keyframes: Vec::new(),
            counter_styles: Vec::new(),
            page_rules: Vec::new(),
            scope_rules: Vec::new(),
            starting_style_rules: Vec::new(),
            container_rules: Vec::new(),
            font_palette_values: Vec::new(),
            color_profiles: Vec::new(),
            function_rules: Vec::new(),
        }
    }
}

impl Clone for Stylesheet {
    /// Copies the content and mints a **new** revision.
    ///
    /// Hand-written rather than derived on purpose: the clone is a separate
    /// sheet that its owner may mutate independently, so letting it inherit the
    /// original's revision would let a mutation of one silently authorise a
    /// cached index for the other. Sharing a revision is only sound while both
    /// are frozen, and nothing here can promise that.
    fn clone(&self) -> Self {
        Self {
            revision: StylesheetRevision::fresh(),
            rules: self.rules.clone(),
            properties: self.properties.clone(),
            media_rules: self.media_rules.clone(),
            imports: self.imports.clone(),
            font_faces: self.font_faces.clone(),
            layer_order: self.layer_order.clone(),
            layers: self.layers.clone(),
            supports_rules: self.supports_rules.clone(),
            keyframes: self.keyframes.clone(),
            counter_styles: self.counter_styles.clone(),
            page_rules: self.page_rules.clone(),
            scope_rules: self.scope_rules.clone(),
            starting_style_rules: self.starting_style_rules.clone(),
            container_rules: self.container_rules.clone(),
            font_palette_values: self.font_palette_values.clone(),
            color_profiles: self.color_profiles.clone(),
            function_rules: self.function_rules.clone(),
        }
    }
}

impl PartialEq for Stylesheet {
    /// Content equality. The revision is identity, not content, and two sheets
    /// parsed from the same CSS must compare equal.
    fn eq(&self, other: &Self) -> bool {
        self.rules == other.rules
            && self.properties == other.properties
            && self.media_rules == other.media_rules
            && self.imports == other.imports
            && self.font_faces == other.font_faces
            && self.layer_order == other.layer_order
            && self.layers == other.layers
            && self.supports_rules == other.supports_rules
            && self.keyframes == other.keyframes
            && self.counter_styles == other.counter_styles
            && self.page_rules == other.page_rules
            && self.scope_rules == other.scope_rules
            && self.starting_style_rules == other.starting_style_rules
            && self.container_rules == other.container_rules
            && self.font_palette_values == other.font_palette_values
            && self.color_profiles == other.color_profiles
            && self.function_rules == other.function_rules
    }
}

impl Stylesheet {
    /// This sheet's content identity — see [`StylesheetRevision`].
    pub fn revision(&self) -> StylesheetRevision {
        self.revision
    }

    /// Declares that this sheet's rules were changed in place, invalidating
    /// every cache keyed by [`Stylesheet::revision`].
    ///
    /// Needed only when rules are reached through the public fields directly;
    /// [`Stylesheet::merge_from`] already does it.
    pub fn mark_mutated(&mut self) {
        self.revision = StylesheetRevision::fresh();
    }

    /// Appends every rule of `other` to this sheet and mints a new revision.
    ///
    /// This is how a sheet grows while it is being streamed in
    /// (`LoadEvent::CssLoaded`): each `<link>`/`<style>` that finishes loading
    /// is merged into the sheet the next paint uses. Written here, beside the
    /// field list, because the previous hand-rolled version at the call site
    /// listed the fields it knew about and had fallen two behind
    /// (`color_profiles`, `function_rules` — so a streamed `@color-profile` or
    /// `@function` was silently dropped).
    pub fn merge_from(&mut self, other: Stylesheet) {
        let Stylesheet {
            revision: _,
            rules,
            properties,
            media_rules,
            imports,
            font_faces,
            layer_order,
            layers,
            supports_rules,
            keyframes,
            counter_styles,
            page_rules,
            scope_rules,
            starting_style_rules,
            container_rules,
            font_palette_values,
            color_profiles,
            function_rules,
        } = other;
        self.rules.extend(rules);
        self.properties.extend(properties);
        self.media_rules.extend(media_rules);
        self.imports.extend(imports);
        self.font_faces.extend(font_faces);
        self.layer_order.extend(layer_order);
        self.layers.extend(layers);
        self.supports_rules.extend(supports_rules);
        self.keyframes.extend(keyframes);
        self.counter_styles.extend(counter_styles);
        self.page_rules.extend(page_rules);
        self.scope_rules.extend(scope_rules);
        self.starting_style_rules.extend(starting_style_rules);
        self.container_rules.extend(container_rules);
        self.font_palette_values.extend(font_palette_values);
        self.color_profiles.extend(color_profiles);
        self.function_rules.extend(function_rules);
        self.mark_mutated();
    }
}

/// `@function <name>(<params>) [returns <type>]? { declarations }` — CSS
/// Functions and Mixins L1. Declares an author-defined custom function
/// invoked from property values as `<name>(<args>)`. `<name>` is a
/// dashed-ident (function-token grammar: no whitespace before `(`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRule {
    /// Dashed-ident name, e.g. `--double`. Matched against `<name>(...)` calls.
    pub name: String,
    /// Positional parameters in declared order.
    pub parameters: Vec<FunctionParameter>,
    /// Raw `returns <type>` descriptor, if present. Stored but not type-checked
    /// (call-site substitution is untyped string substitution, same as `var()`).
    pub returns: Option<String>,
    /// Body declarations in source order: local `--x: ...;` custom properties
    /// used to build up a value, plus the `result: <value>;` descriptor that
    /// gives the function's return value.
    pub declarations: Vec<Declaration>,
}

/// One parameter of an `@function` rule: `--name` or `--name: <default>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParameter {
    /// Dashed-ident parameter name, e.g. `--x`. Referenced inside the body via `var(--x)`.
    pub name: String,
    /// Optional default value, substituted when the call site omits this argument.
    pub default: Option<String>,
}

/// `@color-profile --name { src: url(...); rendering-intent: ...; }` — CSS
/// Color L5 §4. Declares a named custom colour profile referenced from
/// `color(--name c1 c2 c3)`. Phase 0: descriptors are parsed and stored;
/// actual ICC-based colour transform is deferred (layout treats the profile's
/// channels as already-sRGB once a matching name is found).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorProfileRule {
    /// Dashed-ident name, e.g. `--swop5c`. Used to match `color(--name ...)` values.
    pub name: String,
    /// `src` descriptor — URL of the ICC profile resource (loading deferred).
    pub src: Option<String>,
    /// `rendering-intent` descriptor — one of `relative-colorimetric` (default),
    /// `absolute-colorimetric`, `perceptual`, `saturation`.
    pub rendering_intent: Option<String>,
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
/// L4) и сохраняет селектор как сырую строку.
/// Функциональные тесты `font-tech(<font-tech>)` и
/// `font-format(<font-format>)` (CSS Conditional L4 §4 / CSS Fonts L4 §4.3)
/// тоже типизированы — evaluator сверяет аргумент со списком технологий и
/// форматов шрифтов, поддержанных движком `lumen-font`.
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
    /// `font-tech(<font-tech>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово технологии шрифта (например,
    /// `variations`, `color-colrv1`, `features-opentype`). Evaluator
    /// возвращает `true`, если технология реализована в `lumen-font`.
    FontTech(String),
    /// `font-format(<font-format>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово формата шрифта (например, `woff2`,
    /// `opentype`, `truetype`). Кавычки legacy-строкового синтаксиса
    /// (`font-format("woff2")`) снимаются при разборе. Evaluator возвращает
    /// `true`, если формат декодируется движком `lumen-font`.
    FontFormat(String),
    /// Невалидный или нераспознанный тест — evaluator возвращает false.
    Unknown,
}

/// Технологии шрифтов (`<font-tech>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` реально реализует: OpenType-фичи (GSUB/GPOS) и вариативные
/// шрифты (fvar/gvar/avar/HVAR/MVAR). Цветные глифы (COLR/CPAL, sbix, CBDT,
/// SVG-in-OpenType), палитры, AAT/Graphite-фичи и инкрементальная загрузка
/// пока не поддержаны — см. `crates/engine/font/src/lib.rs` (заголовок).
const SUPPORTED_FONT_TECH: &[&str] = &["features-opentype", "variations"];

/// Форматы шрифтов (`<font-format>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` умеет декодировать: TrueType (glyf), OpenType (CFF/glyf +
/// OT layout), WOFF1 (`decode_woff1`) и WOFF2 (`decode_woff2`). Контейнеры
/// `collection` (.ttc), `embedded-opentype` (EOT) и `svg`-шрифты не
/// поддержаны — см. `crates/engine/font/src/woff2.rs` и `lib.rs`.
const SUPPORTED_FONT_FORMAT: &[&str] = &["opentype", "truetype", "woff", "woff2"];

impl SupportsCondition {
    /// Вычислить условие: вернуть `true`, если потребитель поддерживает
    /// все объявления в условии. `known_properties` — список property-
    /// имён, которые css-parser/layout распознают (например, `display`,
    /// `color`, `grid-template-columns`).
    ///
    /// `Selector(<sel>)` (CSS Conditional L4 §4.2 `selector()`) парсится и
    /// признаётся поддержанным, если каждая его часть распознаётся движком —
    /// см. [`ComplexSelector::is_supported`]. Пустой/невалидный селектор → `false`.
    /// `FontTech`/`FontFormat` сверяются со списками технологий и форматов,
    /// которые реально реализует `lumen-font` ([`SUPPORTED_FONT_TECH`] /
    /// [`SUPPORTED_FONT_FORMAT`]). `Unknown` → `false`.
    pub fn evaluate(&self, known_properties: &[&str]) -> bool {
        match self {
            Self::Decl { property, .. } => known_properties
                .iter()
                .any(|p| p.eq_ignore_ascii_case(property)),
            Self::Not(c) => !c.evaluate(known_properties),
            Self::And(cs) => cs.iter().all(|c| c.evaluate(known_properties)),
            Self::Or(cs) => cs.iter().any(|c| c.evaluate(known_properties)),
            Self::Selector(sel) => {
                let list = parse_selector_list(sel);
                !list.is_empty() && list.iter().all(ComplexSelector::is_supported)
            }
            Self::FontTech(tech) => SUPPORTED_FONT_TECH
                .iter()
                .any(|t| t.eq_ignore_ascii_case(tech)),
            Self::FontFormat(fmt) => SUPPORTED_FONT_FORMAT
                .iter()
                .any(|f| f.eq_ignore_ascii_case(fmt)),
            Self::Unknown => false,
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
    /// `(prefers-reduced-transparency: no-preference | reduce)` —
    /// предпочтение пользователя по уменьшению полупрозрачности UI
    /// (Media Queries L5 §5.7).
    PrefersReducedTransparency(MediaReducedTransparency),
    /// `(scripting: none | initial-only | enabled)` — доступность скриптов
    /// при рендеринге документа (Media Queries L5 §6.2).
    Scripting(MediaScripting),
    /// `(inverted-colors: none | inverted)` — инвертирует ли ОС/UA выводимые
    /// цвета (например, режим «инверсия цветов» доступности) (Media Queries
    /// L5 §5.8).
    InvertedColors(MediaInvertedColors),
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

/// Media Queries L5 §5.7 — `prefers-reduced-transparency`: запрос на
/// уменьшение полупрозрачных/blur-эффектов в интерфейсе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaReducedTransparency {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил уменьшение полупрозрачности.
    Reduce,
}

/// Media Queries L5 §6.2 — `scripting`: доступность JavaScript в текущем
/// окружении рендеринга.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaScripting {
    /// Скрипты полностью недоступны (например, отключены пользователем).
    None,
    /// Скрипты исполняются только при первичной загрузке, но не далее
    /// (например, статический снимок страницы для печати).
    InitialOnly,
    /// Скрипты доступны и исполняются на протяжении всей жизни документа.
    Enabled,
}

/// Media Queries L5 §5.8 — `inverted-colors`: инвертирует ли пользовательское
/// окружение (ОС/UA) выводимые цвета.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaInvertedColors {
    /// Цвета выводятся как есть (значение по умолчанию).
    None,
    /// Цвета инвертируются окружением.
    Inverted,
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
    /// Предпочтение уменьшения полупрозрачности
    /// (`prefers-reduced-transparency` media feature).
    pub prefers_reduced_transparency: MediaReducedTransparency,
    /// Доступность скриптов (`scripting` media feature). У Lumen есть
    /// встроенный JS-движок (QuickJS), поэтому desktop-дефолт — `Enabled`.
    pub scripting: MediaScripting,
    /// Инверсия цветов окружением (`inverted-colors` media feature).
    pub inverted_colors: MediaInvertedColors,
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
            prefers_reduced_transparency: MediaReducedTransparency::NoPreference,
            // Lumen исполняет JS (QuickJS) → скрипты включены, как в Edge.
            scripting: MediaScripting::Enabled,
            // Desktop-дефолт: ОС не инвертирует цвета.
            inverted_colors: MediaInvertedColors::None,
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
            Self::PrefersReducedTransparency(t) => ctx.prefers_reduced_transparency == *t,
            Self::Scripting(s) => ctx.scripting == *s,
            Self::InvertedColors(i) => ctx.inverted_colors == *i,
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
    let mut parser = Parser::new(input);
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
fn complex_selector_is_valid(sel: &ComplexSelector, allow_pseudo_element: bool) -> bool {
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
fn compound_selector_is_valid(compound: &CompoundSelector) -> bool {
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
                if let Some(prev) = seen_pe
                    && !(pseudo_element_allows_user_action(prev) && is_user_action_pseudo_class(pc))
                {
                    return false;
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
/// Спека допускает только маркер сгенерированного контента
/// (`::before::marker`, `::after::marker`, CSS Pseudo-Elements L4 §2.4).
fn pseudo_element_pair_allowed(first: &PseudoElementKind, second: &PseudoElementKind) -> bool {
    matches!(
        (first, second),
        (
            PseudoElementKind::Before | PseudoElementKind::After,
            PseudoElementKind::Marker
        )
    )
}

/// Может ли за pseudo-element-ом стоять user-action-псевдокласс.
/// Highlight-псевдоэлементы (`::selection`, `::highlight()`) не допускают
/// после себя ничего (CSS Highlight API L1 §2.2).
fn pseudo_element_allows_user_action(pe: &PseudoElementKind) -> bool {
    !matches!(
        pe,
        PseudoElementKind::Selection | PseudoElementKind::Highlight(_)
    )
}

/// User-action-псевдоклассы (CSS Selectors L4 §4) — единственное, что спека
/// разрешает после tree-abiding pseudo-element-а. `:is()`/`:where()`
/// прозрачны, если целиком состоят из таких же псевдоклассов.
fn is_user_action_pseudo_class(pc: &PseudoClass) -> bool {
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
fn pseudo_element_is_valid(pe: &PseudoElementKind) -> bool {
    match pe {
        PseudoElementKind::Unknown(_) => false,
        // `::picker()` определён спекой только для аргумента `select`.
        PseudoElementKind::Picker(arg) => arg == "select",
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
fn pseudo_class_is_valid(pc: &PseudoClass) -> bool {
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
    ColorProfile(ColorProfileRule),
    Function(FunctionRule),
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

/// Если ввод в позиции `*p` начинается с функции `name` (case-insensitive),
/// продвинуть `*p` за закрывающую `)` и вернуть содержимое скобок как строку.
/// Иначе оставить `*p` без изменений и вернуть `None`. Учитывает вложенные
/// скобки в аргументе (хотя для `font-tech`/`font-format` они не нужны).
fn match_func_arg(b: &[u8], p: &mut usize, name: &[u8]) -> Option<String> {
    let n = name.len();
    if *p + n > b.len() || !b[*p..*p + n].eq_ignore_ascii_case(name) {
        return None;
    }
    let start = *p + n;
    let mut q = start;
    let mut depth: i32 = 1;
    while q < b.len() && depth > 0 {
        match b[q] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            break;
        }
        q += 1;
    }
    let arg = std::str::from_utf8(&b[start..q]).unwrap_or("").to_string();
    if q < b.len() && b[q] == b')' {
        q += 1;
    }
    *p = q;
    Some(arg)
}

fn parse_supports_atom(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    // `font-tech( <font-tech> )` / `font-format( <font-format> )`
    // (CSS Conditional L4 §4 / CSS Fonts L4 §4.3). Один ident-аргумент;
    // у `font-format` допустим legacy-строковый синтаксис (кавычки снимаем).
    if let Some(arg) = match_func_arg(b, p, b"font-tech(") {
        return SupportsCondition::FontTech(arg.trim().to_ascii_lowercase());
    }
    if let Some(arg) = match_func_arg(b, p, b"font-format(") {
        let unquoted = arg.trim().trim_matches(['"', '\'']).trim();
        return SupportsCondition::FontFormat(unquoted.to_ascii_lowercase());
    }
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
        "prefers-reduced-transparency" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedTransparency(MediaReducedTransparency::NoPreference)),
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedTransparency(MediaReducedTransparency::Reduce)),
            _ => MediaCondition::Unsupported,
        },
        "scripting" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::None)),
            "initial-only" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::InitialOnly)),
            "enabled" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::Enabled)),
            _ => MediaCondition::Unsupported,
        },
        "inverted-colors" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::InvertedColors(MediaInvertedColors::None)),
            "inverted" => MediaCondition::Feature(MediaFeature::InvertedColors(MediaInvertedColors::Inverted)),
            _ => MediaCondition::Unsupported,
        },
        _ => MediaCondition::Unsupported,
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    /// At-rules, всплывающие из тела top-level conditional-group rule (сейчас
    /// только `@container`), которые должны попасть в stylesheet-уровневые
    /// коллекции, но не могут быть возвращены через одиночный `AtRuleOutcome`
    /// из [`Self::parse_at_rule`]. [`Self::parse_stylesheet`] опустошает буфер
    /// после каждого top-level `@`-правила.
    bubbled: Vec<AtRuleOutcome>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            bubbled: Vec::new(),
        }
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
        let mut color_profiles: Vec<ColorProfileRule> = Vec::new();
        let mut function_rules: Vec<FunctionRule> = Vec::new();
        let mut anon_counter: usize = 0;
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('@') => {
                    let primary = self.parse_at_rule();
                    // Primary-outcome + at-rules, всплывшие из тела top-level
                    // conditional-group rule (сейчас @container) через `bubbled`.
                    let mut outcomes = std::mem::take(&mut self.bubbled);
                    outcomes.insert(0, primary);
                    for outcome in outcomes {
                        match outcome {
                            AtRuleOutcome::Property(p) => properties.push(p),
                            AtRuleOutcome::Media(m) => media_rules.push(m),
                            AtRuleOutcome::Import(i) => imports.push(i),
                            AtRuleOutcome::FontFace(f) => font_faces.push(f),
                            AtRuleOutcome::FontPaletteValues(fp) => {
                                font_palette_values.push(fp)
                            }
                            AtRuleOutcome::ColorProfile(cp) => color_profiles.push(cp),
                            AtRuleOutcome::Function(f) => function_rules.push(f),
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
                            AtRuleOutcome::StartingStyle(s) => {
                                starting_style_rules.push(s)
                            }
                            AtRuleOutcome::Container(c) => container_rules.push(c),
                            AtRuleOutcome::None => {}
                        }
                    }
                }
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
                                AtRuleOutcome::Scope(s) => scope_rules.push(s),
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
            revision: StylesheetRevision::fresh(),
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
            color_profiles,
            function_rules,
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
        if name.eq_ignore_ascii_case("color-profile") {
            return self
                .parse_color_profile_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::ColorProfile);
        }
        if name.eq_ignore_ascii_case("function") {
            return self
                .parse_function_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Function);
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

    /// Парсит `@color-profile --name { src: url(...); rendering-intent: ...; }`.
    /// CSS Color L5 §4. Prelude — dashed-ident (e.g. `--swop5c`). Block contains
    /// descriptors: `src` (URL, via `parse_import_url`), `rendering-intent`
    /// (keyword, stored raw). Returns `None` if the name is missing or no `{`
    /// follows.
    fn parse_color_profile_body(&mut self) -> Option<ColorProfileRule> {
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

        let mut src: Option<String> = None;
        let mut rendering_intent: Option<String> = None;

        for d in &declarations {
            match d.property.to_ascii_lowercase().as_str() {
                "src" => {
                    src = Parser::new(d.value.trim()).parse_import_url();
                }
                "rendering-intent" => {
                    rendering_intent = Some(d.value.trim().to_ascii_lowercase());
                }
                _ => {}
            }
        }
        Some(ColorProfileRule {
            name,
            src,
            rendering_intent,
        })
    }

    /// Парсит `@function <name>(<params>) [returns <type>]? { decls }` — CSS
    /// Functions and Mixins L1. Prelude — dashed-ident сразу (без пробела,
    /// function-token grammar) за которым следует `(`. Параметры — список
    /// `--param [: <default>]` через запятую (`--foo()` — пустой список).
    /// Опциональный `returns <type>` перед `{` хранится сырой строкой, без
    /// типизации. Возвращает `None`, если prelude не dashed-ident-function-
    /// token или блок `{ ... }` отсутствует.
    fn parse_function_rule(&mut self) -> Option<FunctionRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        if !name.starts_with("--") || self.peek() != Some('(') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '('
        let params_str = self.read_balanced_parens()?;
        let parameters: Vec<FunctionParameter> = split_top_level_commas(&params_str)
            .into_iter()
            .filter_map(|raw| {
                let raw = raw.trim();
                if raw.is_empty() {
                    return None;
                }
                let param = match raw.split_once(':') {
                    Some((n, default)) => FunctionParameter {
                        name: n.trim().to_string(),
                        default: Some(default.trim().to_string()),
                    },
                    None => FunctionParameter { name: raw.to_string(), default: None },
                };
                param.name.starts_with("--").then_some(param)
            })
            .collect();

        self.skip_ws_and_comments();
        let mut returns = None;
        if self.skip_optional_returns_keyword() {
            self.skip_ws_and_comments();
            let type_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' {
                    break;
                }
                self.consume();
            }
            let raw_type = self.input[type_start..self.pos].trim();
            if !raw_type.is_empty() {
                returns = Some(raw_type.to_string());
            }
        }

        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(FunctionRule { name, parameters, returns, declarations })
    }

    /// Читает содержимое между уже открытой `(` (позиция парсера сразу
    /// после неё) и парной закрывающей скобкой, съедая закрывающую. Учитывает
    /// вложенные `(...)` и строковые литералы (`)`/`(` внутри строк не меняют
    /// depth). Возвращает `None`, если EOF наступил раньше закрывающей скобки.
    fn read_balanced_parens(&mut self) -> Option<String> {
        let mut depth = 1u32;
        let mut in_string: Option<char> = None;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            match (in_string, c) {
                (Some(q), ch) if ch == q => {
                    in_string = None;
                    out.push(ch);
                    self.consume();
                }
                (None, '"') | (None, '\'') => {
                    in_string = Some(c);
                    out.push(c);
                    self.consume();
                }
                (None, '(') => {
                    depth += 1;
                    out.push(c);
                    self.consume();
                }
                (None, ')') => {
                    depth -= 1;
                    self.consume();
                    if depth == 0 {
                        return Some(out);
                    }
                    out.push(')');
                }
                _ => {
                    out.push(c);
                    self.consume();
                }
            }
        }
    }

    /// Если позиция стоит на слове `returns` (case-insensitive), за которым
    /// НЕ следует ident-continuation байт, продвигает позицию за это слово
    /// и возвращает `true`. Иначе — не трогает позицию, возвращает `false`.
    fn skip_optional_returns_keyword(&mut self) -> bool {
        let bytes = self.input.as_bytes();
        let p = self.pos;
        if p + 7 > bytes.len() || !bytes[p..p + 7].eq_ignore_ascii_case(b"returns") {
            return false;
        }
        if let Some(&c) = bytes.get(p + 7)
            && (c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return false;
        }
        self.pos += 7;
        true
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
    /// Парсит прелюдию `@scope` — `(<root>)? [to (<limit>)]?` (CSS Cascade L6 §3).
    /// Возвращает сырой селектор корня (`String`; пустая строка = отсутствует
    /// `(<root>)`, implicit `:scope`) и опциональный сырой селектор limit из
    /// `to (<limit>)`. Курсор остаётся на первом токене после прелюдии (обычно
    /// `{`). Общий код для [`Self::parse_scope_rule`] (top-level) и ветки
    /// `@scope` в [`Self::parse_nested_at_rule`] (nested).
    fn parse_scope_prelude(&mut self) -> (String, Option<String>) {
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
        (root, limit)
    }

    fn parse_scope_rule(&mut self) -> Option<ScopeRule> {
        let (root, limit) = self.parse_scope_prelude();
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

    /// Парсит прелюдию `@container` — `<name>? <condition>` (CSS Containment L3
    /// §3). Имя — опциональный CSS-ident перед условием (только если дальше не
    /// `(` и не `style(`). Condition — сырая балансированная строка до `{`.
    /// Курсор остаётся на `{`. Возвращает `None`, если `{` не найден (структура
    /// нарушена). Общий код для [`Self::parse_container_rule`] (top-level) и
    /// ветки `@container` в [`Self::parse_nested_at_rule`] (nested).
    fn parse_container_prelude(&mut self) -> Option<(Option<String>, String)> {
        self.skip_ws_and_comments();
        // Опциональное имя: CSS-ident **только если** дальше не `(` —
        // если сразу `(`, это начало condition без имени. `style(...)` — тоже
        // condition, а не имя.
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
        Some((name, condition))
    }

    /// Парсит `@container <name>? <condition> { rules }` — CSS Containment L3 §3.
    /// Name — опциональный CSS-ident перед условием. Condition — балансированная
    /// строка до `{` (хранится сырой). Rules — обычные правила внутри. Вложенные
    /// at-rules в теле (`@media`, `@supports`, `@layer`, `@container`, `@scope`)
    /// парсятся рекурсивно и всплывают в stylesheet через [`Self::bubbled`]
    /// (плоская модель — container-condition к ним не привязывается, как и для
    /// at-rule-in-at-rule в [`Self::parse_declaration_block_with_nesting`]).
    fn parse_container_rule(&mut self) -> Option<ContainerRule> {
        let (name, condition) = self.parse_container_prelude()?;
        self.consume(); // '{'
        let (rules, bubbled) = self.parse_bare_group_body();
        self.bubbled.extend(bubbled);
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

    /// Парсит тело group at-rule (`@container`/`@media`/…), которое не вложено
    /// ни в какое qualified-правило (`parent_sels` пуст) — то есть ведёт себя
    /// как обычный rule-list stylesheet-уровня: bare-объявления здесь
    /// невалидны (нет селектора, к которому их привязать), поэтому любой
    /// не-`@`-токен — это обычное qualified-правило с произвольным селектором
    /// (включая голый type-селектор вроде `p`, который CSS Nesting L1 §4
    /// запрещает как неоднозначный только внутри уже открытого style-правила).
    /// Вложенные at-rules всплывают отдельным `Vec` (плоская модель). Курсор
    /// должен стоять сразу после `{`, потребляет закрывающую `}`.
    fn parse_bare_group_body(&mut self) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut rules = Vec::new();
        let mut at_rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    at_rules.extend(self.parse_nested_at_rule(&[]));
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                        at_rules.extend(nested_at);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        (rules, at_rules)
    }

    /// Парсит тело nested conditional-group at-rule (после уже consume-нутого
    /// `{`) с полной рекурсией CSS Nesting L1 §5: bare-декларации сворачиваются
    /// в синтетическое правило с селекторами `parent_sels`, вложенные правила
    /// добавляются следом, вложенные at-rules возвращаются отдельным `Vec`
    /// (всплывают на stylesheet-уровень). Общий код для веток `@media` /
    /// `@supports` / `@layer` / `@container` / `@scope` в
    /// [`Self::parse_nested_at_rule`]. Курсор должен стоять сразу после `{`.
    /// Пустой `parent_sels` означает, что мы на самом деле не вложены ни в
    /// какое qualified-правило (например, `@media` внутри `@container` на
    /// stylesheet-уровне) — тогда делегирует в [`Self::parse_bare_group_body`],
    /// у которой другая грамматика тела (rule-list, а не declarations+nesting).
    fn parse_nested_group_body(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        if parent_sels.is_empty() {
            return self.parse_bare_group_body();
        }
        let (decls, inner_rules, inner_at) =
            self.parse_declaration_block_with_nesting(parent_sels);
        let mut rules = Vec::new();
        if !decls.is_empty() {
            rules.push(Rule {
                selectors: parent_sels.to_vec(),
                declarations: decls,
            });
        }
        rules.extend(inner_rules);
        (rules, inner_at)
    }

    /// CSS Nesting L1 §5: nested at-rule inside a qualified rule.
    /// Example: `.parent { @media (min-width: 800px) { color: red; } }`
    /// expands to: `@media (min-width: 800px) { .parent { color: red; } }`.
    /// Supports `@media`, `@supports`, `@layer`, `@container`, `@scope`.
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
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
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
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
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
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::LayerBlock { name: layer_name, rules }];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("container") {
            // CSS Containment L3 §3: опциональное имя перед condition — тот же
            // разбор прелюдии, что и для top-level `@container`.
            let Some((cont_name, condition)) = self.parse_container_prelude() else {
                return vec![];
            };
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes = vec![AtRuleOutcome::Container(ContainerRule {
                name: cont_name,
                condition,
                rules,
            })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("scope") {
            // CSS Cascade L6 §3: `@scope (<root>)? [to (<limit>)]?` вложенный в
            // qualified-правило. Прелюдия — тот же разбор, что и для top-level
            // `@scope`; тело — рекурсивный declaration-block с `parent_sels`.
            let (root, limit) = self.parse_scope_prelude();
            self.skip_ws_and_comments();
            if self.peek() != Some('{') {
                return vec![];
            }
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::Scope(ScopeRule { root, limit, rules })];
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
    fn parse_selector_list_strict(&mut self) -> Option<Vec<ComplexSelector>> {
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
                "placeholder" => PseudoElementKind::Placeholder,
                "checkmark" => PseudoElementKind::Checkmark,
                "picker-icon" => PseudoElementKind::PickerIcon,
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
            "state" => {
                // CSS Selectors L4 §17.4: single custom-ident argument,
                // case-sensitive (custom-ident, not a BCP-47 tag — no
                // lowercasing, unlike `:lang()`).
                self.skip_ws_and_comments();
                let ident = self.parse_ident()?;
                self.skip_ws_and_comments();
                if self.peek() != Some(')') {
                    return None;
                }
                Some(PseudoClass::State(ident))
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
            "picker" => {
                // HTML/CSS «Customizable Select»: `::picker(select)` targets the
                // pop-up picker part of a `base-select` `<select>`. Only the
                // `select` ident is defined today; store it for forward-compat.
                self.skip_ws_and_comments();
                let arg = self.parse_ident().unwrap_or_default();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || arg.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Picker(arg.to_ascii_lowercase()))
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
#[path = "parser/tests/revision.rs"]
mod revision_tests;

#[cfg(test)]
#[path = "parser/tests/selectors.rs"]
mod selectors_tests;
#[cfg(test)]
pub(crate) use selectors_tests::one;

#[cfg(test)]
#[path = "parser/tests/at_rules.rs"]
mod at_rules_tests;

#[cfg(test)]
#[path = "parser/tests/nesting.rs"]
mod nesting_tests;
