# BUG-1473 — `display: contents` перед flex-элементами и `display:inline` флекс-элемент с блоком внутри раскладываются иначе, чем эталон

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex_trampoline.rs`, `layout_dispatch.rs` — `display: contents` в flex, grid и table; `display:inline` элемент с блоком внутри flex-контейнера)

## Симптом

Для `display:contents` дети должны стать flex-элементами самого контейнера, а у flex-элемента с `display:inline` блокификация (§4 flex) делает его `block`, и вложенный блок встаёт под текстом. Проба на `<div class=flex>P<div class=contents><div>A</div></div><div class=contents><div class=inline>S<div>0</div></div></div>S</div>`: у нас `0` стоит в строке с `ASOS` (`z` в `(31.5,0)`), а у эталона `0` под первой `S`. Без `contents` флекс-элемент `inline` с блоком внутри выглядит верно (`b` 10,3×32, `z` `(0,16)`), то есть дефект — в комбинации «`display:contents` → дети в flex-раскладке». Простые случаи `contents` внутри `grid` (`a`/`b` в `(0,0)`/`(100,0)`) и `flex` (`A`, `B` подряд) верны. 53 id `css-display/display-contents-*` (49 `thick`, 1 `no-match-ref`, 3 testharness): `-dynamic-flex-*` (6), `-dynamic-table-*` (4), `-flex-*`, `-table-*`, `-fieldset`, `-first-line`, `-oof`, `-before-after`, `-dynamic-list`. Часть id (`table`, `fieldset`, `oof`, `first-line`) проба не проверяла — отнесены по имени файла.

## Проба

Проба (`--mcp`, шрифт Ahem 16 px): `<div style="display:flex">P<div style="display:contents"><div id=a>A</div></div><div style="display:contents"><div id=b style="display:inline">S<div id=z>0</div></div></div></div>`:

| элемент | у нас | ожидается |
|---|---|---|
| `a` | `(10.2,0)` | как у ссылки: `A` справа от `P` |
| `b` | `(21.3,0)`, ширина 10,3, высота 16 | блокифицирован: `S` над `0` |
| `z` | `(31.5,0)` — в той же строке | под `S`: `(21.3,16)` |

Снимок `display-contents-flex-001.html` — у теста `ASOS`, у эталона `ASS` и `0` под первой `S`.

## Как найдено

WPT-RUN-14 срез 21: `css-display/display-contents-flex-001.html`, `display-contents-table-001.html`, `display-contents-dynamic-flex-001.html`.

## Что делать

Разобрать, почему `display:inline` ребёнок `contents` внутри flex не блокифицируется (эталон — его же без `contents`, который верен), затем проверить `table`/`fieldset`/`::before`-варианты.

## Как проверить

Таблица выше; `css/css-display/display-contents-flex-001.html`.
