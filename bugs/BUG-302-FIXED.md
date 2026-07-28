# BUG-302: Element.getElementsByClassName отсутствует в DOM-шиме

**Статус:** FIXED 2026-07-19
**Дата:** 2026-07-17
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** первым прогоном `scripts/perf_audit.py` (skill `/lumen-perf-audit`) на news.ycombinator.com

## Симптом

Скрипты HN падают при исполнении:

```
script error: JS runtime error: el.getElementsByClassName is not a function
```

`grep getElementsByClassName crates/js/src/*.rs` находит метод только в
`dom_parser.rs` (документы DOMParser) — в основном шиме `WEB_API_SHIM`
(`dom.rs`) его нет ни на `Element.prototype`, ни на `document`/`Document`.

## Воспроизведение

```bash
cargo run -p lumen-shell --profile dev-release -- --dump-layout https://news.ycombinator.com/
# stderr: script error: JS runtime error: el.getElementsByClassName is not a function
```

Либо `python scripts/perf_audit.py --only hn` — строка попадает в
`error_lines` записи results.json.

## Ожидание

`getElementsByClassName(names)` живёт на `Document` и на `Element` (DOM
Standard §4.5/§4.9, live HTMLCollection; в нашем шиме допустим статический
массив, как соседние `getElementsByTagName`). Реализовать в engine-agnostic
`WEB_API_SHIM` — та же семья пробелов, что [BUG-299](BUG-299-FIXED.md)
(`insertAdjacentText` отсутствовал целиком).

## Замечание

Ошибка обрывает site-скрипты HN целиком — потенциально маскирует дальнейшие
несовместимости; после фикса перемерить сайт аудитом.

## Решение (2026-07-19, P3)

Реализовано в engine-agnostic `WEB_API_SHIM` (`crates/js/src/dom.rs`):

- Новый helper `_lumen_class_selector(names)` разбивает аргумент по whitespace,
  отбрасывает пустые токены и строит compound-селектор класса (`.a.b`). Пустой
  список токенов даёт `null`, чтобы вызывающий короткозамкнул на пустой массив
  (селектор `''` иначе бросил бы в query-движке).
- `getElementsByClassName` добавлен на `document` (делегирует
  `_lumen_query_selector_all`) и на `Element` (scoped, `_lumen_query_selector_all_scoped`
  по descendant-поддереву). Возвращается статический массив, а не live
  `HTMLCollection` — та же упрощённая семантика, что у соседнего
  `getElementsByTagName` (см. комментарий там).

Ограничение (осознанное, как у `getElementsByTagName`): токены класса
подставляются в селектор без CSS-эскейпинга — экзотические имена классов со
спецсимволами не поддержаны; реальные сайты используют идентификаторные имена.

Покрыто юнит-тестами `get_elements_by_class_name_document` и
`get_elements_by_class_name_scoped_element` в `crates/js/src/dom.rs`.
