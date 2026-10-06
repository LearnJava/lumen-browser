# BUG-1329 — `text-transform`: `innerText` при `capitalize` не совпадает с раскладкой; язык (`lang=tr|lt|…`) не учитывается

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** js/layout (`crates/js/src/shim/*` — `innerText`; `TextTransform::apply` в `crates/engine/layout/src/style/values/typography.rs` не получает язык)

## Симптом

1. Раскладка капитализирует верно (`--dump-layout`: `john's apple foo_bar` → `John's Apple Foo_bar`), а `element.innerText` — `John'S Apple Foo_Bar` (апостроф и подчёркивание сочтены границей слова). WPT `text-transform-capitalize-036.html`: 3 из 6 сабтестов.
2. `uppercase` не учитывает `lang`: `<div lang=tr>iı</div>` → `II` (ожидается `İI`); `lang=lt` — без поведения для `i̇`. `apply(&str)` языка не получает.
3. `text-transform-upperlower-107.html`: `Selection.toString()` для `ß` при `uppercase` даёт `ß`, ожидается `SS` (раскладка при этом рисует `SS`).

## Как найдено

WPT-RUN-14 срез 10: `text-transform-tailoring-*` (8 id, включая `-dynamic`), `text-transform-capitalize-*` (7 id; 6 reftest `thick` — причина не разобрана, пробой подтверждён только `innerText`), `text-transform-upperlower-*` (20 id; пробой подтверждён только `-107`, остальные не разобраны).

## Что делать

Передавать язык элемента в `TextTransform::apply` (tr/az `i`/`İ`, lt, ga); `innerText` и выделение приводить к той же функции, что раскладка.

## Как проверить

`css/css-text/text-transform/text-transform-tailoring-001.html`, `text-transform-capitalize-036.html`.
