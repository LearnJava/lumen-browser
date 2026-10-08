# BUG-1418 — `currentcolor` не рисуется как `background-color` и как цвет остановки градиента; `getComputedStyle().backgroundColor` возвращает слово `currentcolor`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/paint (`style/apply/paint.rs`, `crates/engine/paint/src/display_list/walk.rs:49/87/138`, `style/values/color.rs::to_color_opt`)

## Симптом

`<body style="margin:0;color:green">`, строки по 10 px, пиксель в x=5:

| `style` | пиксель | `--dump-display-list` |
|---|---|---|
| `background-color:currentcolor` | `(255,255,255)` | пусто |
| `background:currentcolor` | `(255,255,255)` | пусто |
| `background-color:CurrentColor` | `(255,255,255)` | пусто |
| `background-image:linear-gradient(currentcolor,currentcolor)` | `(255,255,255)` | `DrawLinearGradient … stops=1` |
| `background-image:linear-gradient(to right,currentcolor,blue)` | `(255,255,255)` | то же |
| `border:5px solid currentcolor` (контроль) | зелёный | `DrawBorder … c=[#008000ff,…]` |
| `background-color:inherit` от родителя с `currentcolor` | не красит | |

`getComputedStyle(el).backgroundColor` при `color:green;background-color:currentcolor` — `currentcolor` (спека: used/computed
`currentcolor` резолвится в `rgb(0, 128, 0)` для `background-color` — CSS Color 4 §4.4; `color`-наследование остаётся,
`currentcolor` в `color` — `inherit`).

## Как найдено

WPT-RUN-14 срез 19: `css-color/currentcolor-001…004`, `t44-currentcolor-background-b.xht`, `color-mix-currentcolor-001…003`
(в них `currentcolor` ещё и внутри `color-mix`), `light-dark-currentcolor` (`light-dark(currentColor, currentColor)`),
`css-images/color-stop-currentcolor.html` и ещё 5 `color-stop-*-currentcolor-invalidation.html`. Пробой подтверждено только
для `background-color`/`background`/градиентных остановок; остальные — по тексту теста.

## Что делать

В `walk.rs` (и в `background_mask.rs:593`, `inline_frag.rs:63`) резолвить `CssColor::CurrentColor` через
`resolve(style.color)`; остановки градиента держать как `CssColor` до слоя paint и резолвить там. Computed-значение
`background-color` в `selector_query.rs:560` отдавать резолвленным. Правка двигает пиксели — полный
`graphic_tests/run.py --continue-on-fail` и новые эталоны в том же коммите.

## Как проверить

Страница из таблицы; `css/css-color/currentcolor-001.html`…`004`, `css-images/color-stop-currentcolor.html`.
