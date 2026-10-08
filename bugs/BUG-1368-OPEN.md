# BUG-1368 — `counter-reset` / `counter-increment` на самом `::before` / `::after` не применяются

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 14, `css/CSS2` (text, linebox, fonts, generated-content, lists, bidi-text))
**Область:** layout (`crates/engine/layout/src/counters.rs::precompute_counters` ~1562 — `apply_reset`/`apply_increment` берут стиль элемента, стиль его `::before`/`::after` не читается)

## Симптом

`--dump-display-list`, свежие имена счётчиков, `div`:

| правило | нарисовано | ожидается |
|---|---|---|
| `#b::before{counter-increment:zb 3; content:"P:" counter(zb)}` | `P:0` | `P:3` |
| `#c::before{counter-reset:zc 7; content:"R:" counter(zc)}` | `R:0` | `R:7` |
| `#d::before{counter-reset:zd 7; counter-increment:zd; content:"RI:" counter(zd)}` | `RI:0` | `RI:8` |
| `#g::after{counter-increment:zg 5; content:" after:" counter(zg)}` у `#g{counter-increment:zg}` | `after:1` | `after:6` |
| `#f{counter-increment:zf} #f::before{content:"F:" counter(zf)}` (счётчик на элементе) | `F:1` | `F:1` — верно |

Счётчики на самом элементе работают, на его псевдоэлементе — нет: модуль обходит DOM и применяет `counter-*` только стилю узла. CSS Lists L3 §4.5: у `::before`/`::after` свои `counter-reset`/`counter-increment`/`counter-set`, действующие в позиции псевдоэлемента (после своего элемента для `::before`, после детей для `::after`).

## Как найдено

WPT-RUN-14 срез 14: 35 id после правки однодвоеточных селекторов остаются `thick`; `counter-increment` стоит в том же правиле, что и `content`, — `generated-content/content-011…035`, `content-counter-*`, `counters-order-000`, `before-after-*-001`, `lists/counter-reset-*`.

## Что делать

В `precompute_counters` на входе в элемент (после своих reset/increment) применить `counter-*` стиля `::before`, а перед выходом (после детей) — стиля `::after`; снимок для `::after` брать после детей (сейчас используется снимок «до детей», комментарий в шапке `counters.rs`).

## Как проверить

`css/CSS2/generated-content/content-011.xht`, `content-013.xht`, `counters-order-000.xht`, `lists/counter-reset-increment-002.xht`.

## Срез 25 (2026-10-08, P2, WPT-RUN-14 `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)

`css/css-lists`: 19 reftest `thick` (`counter-001`, `counter-004`, `counters-001/004/006`, `counters-scope-001…004`, `counter-set-001`, `marker-counter`, `*-display-contents`, `*-display-none`, `counter-reset-increment-overflow-underflow`, `pseudo-element-remove-update`, `deep-pseudo-element-remove-update`). Проба: `.b::before{counter-increment:x;content:counter(x)}` при `counter-reset:x 5` — `5`, `5` (ожидается `6`, `7`); `.b::before{counter-reset:y 3;content:counter(y)}` — `0`. См. также BUG-1568 (неявный `list-item`, `reversed()`, `display:none`).
