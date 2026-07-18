# BUG-302: Element.getElementsByClassName отсутствует в DOM-шиме

**Статус:** OPEN
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
