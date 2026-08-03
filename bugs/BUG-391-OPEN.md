# BUG-391 — `matches()`/`querySelector(All)`/`closest()` никогда не бросают `SyntaxError` на невалидный или неподдерживаемый селектор

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/selector_query.rs:335-422` —
`query_all`, `query_all_within`, `query_all_scoped`, `matches_selector`), js
(`crates/js/src/dom.rs:5924` — `matches`, `5934` — `closest`, аналогичные
обёртки `querySelector`/`querySelectorAll`)
**Найден:** P2, WPT-VENDOR-fullscreen (2026-07-28), тест
`rendering/fullscreen-pseudo-class-support.html`

## Симптом

```
:fullscreen pseudo-class support
- assert_throws_dom: function "() => document.body.matches(':halfscreen')" did not throw
```

Тест — стандартный WPT-паттерн feature-detection: перед проверкой
`:fullscreen` он утверждает, что заведомо несуществующий псевдокласс
`:halfscreen` бросает `SyntaxError` (precondition, чтобы отличить «браузер не
поддерживает `:fullscreen`» от «браузер вообще не валидирует селекторы»).
`document.body.matches(':halfscreen')` в Lumen тихо возвращает `false` вместо
исключения.

## Причина

Все четыре точки входа селекторного движка спроектированы так, чтобы никогда
не бросать: `query_all`/`query_all_within`/`query_all_scoped` возвращают
`Vec::new()`, `matches_selector` возвращает `false`, когда
`parse_selector_list(sel)` не смог распарсить ни один селектор из списка —
задокументировано явно в doc-комментариях каждой функции ("Returns an empty
Vec when sel is empty, all selectors are invalid..."). Не различаются два
разных случая: (1) синтаксически некорректный селектор (лишняя скобка,
пустая строка) и (2) синтаксически валидный, но неизвестный движку токен —
`:halfscreen` парсится как псевдокласс, которого нет в списке распознаваемых.
По DOM LS `#dom-element-matches`/`#dom-parentnode-queryselector` оба случая
обязаны бросать `SyntaxError` DOMException, а не молча трактоваться как «не
подошло».

Обёртки JS-шима (`dom.rs:5924` `matches`, `5934` `closest`, querySelector(All)
на `document`/`Element`/`DocumentFragment`/`ShadowRoot`) просто прокидывают
булев/Option/Vec результат нативов в JS — исключение неоткуда взять, раз его
нет уже в Rust-слое.

## Масштаб

Затрагивает весь набор: `Element.matches()`, `document.querySelector()`,
`querySelectorAll()`, `Element.closest()` — везде, где вызывается
`parse_selector_list`. Вне WPT — любой код, полагающийся на throw для
feature-detection нового CSS-синтаксиса (общий паттерн, не специфичный для
fullscreen), либо ожидающий исключение на опечатку в селекторе, получает
тихий "не найдено" вместо диагностируемой ошибки.

## Как чинить

`parse_selector_list` уже различает "пусто/невалидно" через возврат пустого
списка — нужно, чтобы вызывающий код на JS-границе (`v8_runtime.rs:1180+`,
регистрация `_lumen_node_matches_selector`/`_lumen_query_selector*`) отличал
"валидный список, ноль совпадений" от "список невалиден/пуст" и во втором
случае бросал `SyntaxError` DOMException в JS, а не возвращал
`false`/`None`/`[]`. Проще всего — сделать Rust-функции возвращающими
`Result<_, SelectorParseError>` (или отдельный `pub fn is_valid_selector`)
и матчить исход на границе `reg!`, конвертируя ошибку в JS-исключение через
существующий механизм throw (см. как это сделано для других `SyntaxError`
DOMException в кодовой базе, например URL-парсинг).

Регрессия без WPT: `document.querySelector(':bogus-pseudo')` и
`el.matches('(')` должны бросать `SyntaxError`; `document.querySelector('.no-match')`
(валидный селектор, ноль совпадений) должен по-прежнему возвращать `null`, не
бросать.

## Связанные

* Не является причиной провала самого `:fullscreen` (тест проверяет его
  отдельным assert'ом ниже throw-precondition) — `:fullscreen` как таковой в
  Lumen не проверялся этим прогоном (тест падает на precondition раньше).

## Срез 16 (`css/css-forms`, 2026-08-02) — same mechanism, a new facet: structural/argument validity of pseudo-elements is unenforced too

`css/support/parsing-testcommon.js::test_invalid_selector` asserts
`document.querySelector(selector)` throws `SyntaxError` for selectors that
are syntactically well-formed per grammar but semantically invalid — a
combinator or another pseudo-element following a pseudo-element
(`::checkmark *`, `::checkmark::checkmark`, `::before::checkmark`,
`::checkmark::before`, `::slotted(*)::checkmark::slotted(*)`), and an
out-of-range argument to a functional pseudo-element (`::picker(foo)`,
`::picker()`, bare `::picker`). None of these throw in Lumen — 31 subtests
across `parsing/checkmark-pseudo-element.html` (11),
`parsing/picker-icon-pseudo-element.html` (11),
`parsing/picker-select-pseudo-element.html` (9).

Confirmed this is not `::checkmark`/`::picker`-specific by a live
`--mcp-live-port` probe against `::before` (a pseudo-element that has been
supported for the whole life of the project):
`document.querySelector('::before *')` and
`document.querySelector('::before::before')` both return normally (no
throw) — same gap the fullscreen finding above already names architecturally
("(2) синтаксически валидный, но неизвестный движку токен" was the described
case; this adds a third: "(3) синтаксически валидный по грамматике каждого
отдельного псевдо-элемента, но недопустимая структура/аргумент по правилам
конкретного псевдо-элемента"). Source-confirmed for the argument-validity
half: `crates/engine/css-parser/src/parser.rs:4539-4549`'s `"picker"` arm
accepts *any* non-empty ident as the functional pseudo-element's argument
(`PseudoElementKind::Picker(arg.to_ascii_lowercase())`, no check against the
single spec-defined value `"select"`), so `::picker(foo)` parses
successfully instead of being rejected at parse time — there is no
combinator/pseudo-stacking check anywhere in the file either (`grep -n
"PseudoElement" parser.rs` for validation/combinator/reject logic returns
nothing).

Same fix location as the rest of this bug (the JS-boundary throw needs to
happen wherever `parse_selector_list`'s caller currently swallows a `None`/
empty result into `false`/`[]`/no-op) — this slice doesn't change the "как
чинить" plan, just widens what "invalid" needs to mean at that boundary:
not just "unknown token", but also "known pseudo-element used in a
structurally or argument-wise invalid way". `.ini` under
`tests/wpt/metadata/css/css-forms/` for the 3 files, `expected: FAIL` per
affected subtest.

## Срез 26 (`css/css-highlight-api`, 2026-08-03)

Same shape, a new pseudo-element: `highlight-pseudo-parsing.html`'s
"should be an invalid selector" subtests (`"::highlight"` with no argument,
`"::before::highlight(foo)"`, `"::highlight(foo).a"`,
`"::highlight(foo)::after"`, `"::highlight(foo):hover"`,
`":not(::highlight(foo)))"` — 6 subtests) all fail to throw
`SyntaxError`/`DOMException` from `document.querySelector(selector)`. The
file's "should be a valid selector" subtests fail for the unrelated reason
already covered by [BUG-485](BUG-485-OPEN.md) (shared
`test_valid_selector`/`test_invalid_selector` helper's
`document.head.append(style)`). `.ini` under
`tests/wpt/metadata/css/css-highlight-api/`.

## Срез 29 (`css/css-shadow`, 2026-08-03)

`host-context-parsing.html` — 48 subtests, all "should be an invalid
selector" cases for `:host-context` used malformed (bare, empty
parens, comma-separated, or with a compound-selector-list argument like
`.a + .b`): `document.querySelector(selector)` never throws
`SyntaxError`/`DOMException`. Same shape as every prior slice of this bug —
the parser accepts a structurally invalid pseudo-class argument rather than
rejecting it. `.ini` under `tests/wpt/metadata/css/css-shadow/`.

## Срез 30 (`css/css-pseudo` + `css/css-view-transitions` + `css/selectors`, 2026-08-03)

Largest extension yet — 14 files/695 subtests, dominated by
`css-view-transitions/parsing/pseudo-elements-invalid.html` alone (675 of
the 695): every malformed `::view-transition-group()`/`-image-pair()`/
`-old()`/`-new()` argument (missing/empty `*`-or-ident argument, invalid
nesting after `::before`/`::after`, disallowed combinators) is silently
accepted instead of throwing `SyntaxError` from
`document.querySelector`/`matches`. Same shape, smaller counts:
`css-pseudo/parsing/highlight-pseudos.html` (24, `::highlight()` argument
validity), `highlight-pseudos-search-text.tentative.html` (27,
`::search-text()`), `tree-abiding-pseudo-elements.html` (22, structural
validity of `::before`/`::after`/`::marker`/`::placeholder` combined with
other pseudo-classes), `css-view-transitions/parsing/pseudo-elements-
invalid-with-classes.html` (20, `::view-transition-group(*.class)` variant),
and 9 `css/selectors/parsing/parse-*.html` files (`:has()`, `:is()`/
`:where()`, `::part()`, `::slotted()`, `:state()`, `:not()`, heading
selectors — 66 subtests combined) covering invalid nesting/argument shapes
for each. `.ini` under `tests/wpt/metadata/css/css-pseudo/`,
`tests/wpt/metadata/css/css-view-transitions/`,
`tests/wpt/metadata/css/selectors/`.
