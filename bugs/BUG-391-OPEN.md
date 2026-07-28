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
