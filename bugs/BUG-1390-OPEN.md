# BUG-1390 — `overflow-clip-margin` не расширяет клип в `--dump-display-list`, а `CSS.supports('overflow-clip-margin', '20px')` ложно

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** paint (`crates/engine/paint/src/display_list/walk.rs:1242` читает `overflow_clip_margin`, `box_layer.rs:128-175` — нет) + js (`CSS.supports`)

## Симптом

`--dump-display-list`, `overflow:clip; width:100px; height:50px`:

| стиль | `PushClipRect` | ожидается |
|---|---|---|
| `overflow-clip-margin:20px` | `(30,30,100,50)` | `(10,10,140,90)` |
| `overflow-clip-margin:padding-box 20px` | `(30,110,100,50)` | то же |

Единственное место, читающее `style.overflow_clip_margin`, — `walk.rs:1242`; в упорядоченном пути (`box_layer.rs`, там же строится клип для stacking context) его нет. Юнит-тест `overflow_clip_margin_expands_clip_region` (`display_list/tests/svg_table_and_hash.rs:1632`) зелёный; какой из двух путей берёт `--dump-display-list` в этих пробах, не выяснялось — результат: расширения нет (проба `--screenshot`: красный блок обрезан по `x 30…129`, т. е. без полей). Бокс-ключевое слово (`content-box`/`border-box`) на положение базового ребра не влияет (`walk.rs:1232-1241`).

`CSS.supports('overflow-clip-margin','20px')`, `'padding-box 20px'`, `'content-box'`, `'0px'` → `false`, хотя `element.style` значение принимает, а `getComputedStyle` возвращает `20px`.

## Как найдено

WPT-RUN-14 срез 16: `css-overflow/overflow-clip-margin-*` (18 корневых + 2 `-border-radius`, 25 thick), `parsing/overflow-clip-margin-computed.html` (20 из 20 сабтестов), `-hit-testing`, `-intersection-observer` — 31 id.

## Что делать

Вынести расширение клипа в общую функцию и вызывать из обоих путей; учесть `<visual-box>` и `border-radius` (`overflow-clip-margin-border-radius-*`); `CSS.supports` — из того же разбора, что и `element.style`.

## Как проверить

`css/css-overflow/overflow-clip-margin-007.html`, `parsing/overflow-clip-margin-computed.html`.
