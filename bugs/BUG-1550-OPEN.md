# BUG-1550 — `counter(name, <custom-style>)` добавляет `prefix` и `suffix` стиля (по умолчанию `". "`): `counter(a, f)` — `X.` вместо `X`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/counters.rs` — `format_counter_with_registry` и вызов из `counter()`/`counters()`)

## Симптом

CSS Counter Styles 3 различает «counter representation» и «marker representation»: `prefix`/`suffix` входят только в представление маркера (`::marker`, `list-style-type`), а `counter()`/`counters()` отдают представление без них. У пользовательского `@counter-style` (`system: cyclic; symbols: "X" "Y"`) `counter(a, f)` даёт `X.`, а с `prefix:"<"; suffix:"!"` — `<X!`. У встроенных (`upper-roman`) верно — `II`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `@counter-style f{system:cyclic;symbols:"X" "Y"}`, `counter-reset:a 3`, `::before{content:counter(a,f)}` — текст | `X.` | `X` |
| то же с `prefix:"<"; suffix:"!"` — текст `"[" counter(a,f) "]"` | `[<X!]` | `[X]` |
| `counters(a,"-",f)` с `f` по умолчанию | `[X.]` | `[X]` |
| `counter(a, upper-roman)` | `II` | `II` |

## Как найдено

WPT-RUN-14 срез 25: кластер `cs-atrule-other` (`css-counter-styles/counter-style-at-rule/*` — 10 id без пустых `li` и без escape); пересекается с BUG-1566.

## Что делать

В `counter()`/`counters()` не добавлять `prefix`/`suffix`; их применять только в `build_list_marker_text` для маркера.

## Как проверить

`<style>@counter-style f{system:cyclic;symbols:"X" "Y"} #n::before{content:counter(a,f)}</style>` при `counter-reset:a 3` даёт `X`.
