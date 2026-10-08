# BUG-1566 — `@counter-style` с `system: cyclic`: отрицательное значение выводится со знаком (`-Y`), у эталона знака нет

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/counters.rs` — `format_counter_with_registry`, знак `negative`)

## Симптом

CSS Counter Styles 3: дескриптор `negative` применяется только в системах `symbolic`, `alphabetic`, `numeric`, `additive`; у `cyclic` значение зациклено по символам, знак не пишется. `@counter-style f{system:cyclic;symbols:"X" "Y"}` при `counter-reset:a -2` даёт `-Y`, при `-1` — `-X`. Эталон `counter-style-at-rule/system-cyclic-ref.html` для `start=-2` — `‣ ‣ ‣ ‣ ‣` / `‡ † ‡ † ‡` без знака.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `system:cyclic;symbols:"X" "Y";suffix:""`, `counter(a,f)` при `-2`, `-1`, `0`, `1` | `-Y`, `-X`, `Y`, `X` | `Y`, `X`, `Y`, `X` |
| `system:symbolic` при `-2` | `-2` (fallback decimal) | `-2` (fallback decimal) |

## Как найдено

WPT-RUN-14 срез 25: `css/css-counter-styles/counter-style-at-rule/system-cyclic.html`; кластер `cs-atrule-other` (`system-fixed.html` не проверялся).

## Что делать

Не писать `negative` для `cyclic` (и других систем с бесконечной областью без знака; `fixed` не проверялось) — CSS Counter Styles 3, дескриптор `negative` и описание систем.

## Как проверить

`css/css-counter-styles/counter-style-at-rule/system-cyclic.html`.
