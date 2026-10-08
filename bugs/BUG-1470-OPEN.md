# BUG-1470 — `position:absolute`/`fixed` внутри inline-контекста ломает строку (следующий inline уходит на новую строку) либо занимает место в строке, а статическая позиция неверна

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — `position:absolute`/`fixed` потомок внутри строки: разрыв строки и статическая позиция)

## Симптом

Абсолютно позиционированный бокс вынут из потока (CSS 2.1 §9.6): он не должен ни разрывать строку, ни занимать в ней место, а его статическая позиция (без `left`/`top`) — место, где он стоял бы в потоке. У нас в `<div style="width:400px">[inline-block 50×10]<span id=a abs>…</span><span id=r>[inline-block 50×10]</span></div>`: `r` уходит на вторую строку (`(0,10)`, ожидается `(50,0)`) — даже когда у `a` заданы `left`/`top` и его статическая позиция не используется; `a` без инсетов встаёт в `(0,10)` вместо `(50,0)`. Если тот же `a` лежит в обёртке `<span>`, строка не ломается, но результат опять неверный: у `a` с текстом `a=(50,0)` верно, а `r` сдвинут на ширину `a` (`(82,0)`, ожидается `(50,0)`) — абсолютный бокс занимает место в строке; у `a` с заданными шириной и высотой `getBoundingClientRect` — `(0,0,0,0)` вместо `(50,0,30,10)`. Для `<div>` вместо `<span>` — то же, что для `<span>`. 63 id среза: `css-position/static-position/` (30), `position-absolute-in-inline-*`/`position-absolute-dynamic-static-position-*` (17), `css-position/multicol/` (16): 60 `thick`, 2 `thin-only`, 1 testharness (`position-absolute-in-inline-001`). Для `multicol` и вертикальных режимов записи проба не проводилась — отнесены по имени каталога; причина в них может быть другой.

## Проба

Проба (`--mcp`, `div` шириной 400 px, `B` = `display:inline-block;width:50px;height:10px;vertical-align:top`, `A` = `position:absolute;width:30px;height:10px`):

| разметка | `a` у нас | `r` у нас | ожидается |
|---|---|---|---|
| `B`, `r`(B) — без абсолютного | — | `(50,0)` | — |
| `B`, `a`(A), `r`(B) | `(0,10,30,10)` | `(0,10)` | `a` `(50,0)`, `r` `(50,0)` |
| то же, у `a` `left:5px;top:5px` | `(5,5,30,10)` | `(0,10)` | `a` `(5,5)`, `r` `(50,0)` |
| то же, `a` — `<div>` | `(0,10,30,10)` | `(0,10)` | как выше |
| `B`, `<span><span a(A)></span></span>`, `r`(B) | `(0,0,0,0)` | `(50,0)` | `a` `(50,0,30,10)`, `r` `(50,0)` |
| `B`, `<span><span a>CDE</span></span>`, `r`(B) | `(50,0,32.9,19.4)` | `(82,0)` | `r` `(50,0)` |

## Как найдено

WPT-RUN-14 срез 21: `css-position/static-position/htb-ltr-ltr.html`, `position-absolute-in-inline-002.html`, `css-position/multicol/static-position/vlr-ltr-ltr-in-multicol.tentative.html`.

## Что делать

Не создавать из абсолютного inline-потомка блок, разрывающий строку, и не включать его в ширину и перенос; запоминать позицию вставки (после предыдущего inline-контента) как статическую. Отдельно: нулевой `getBoundingClientRect` у абсолютного бокса с размерами внутри обёртки-`span`.

## Как проверить

Таблица выше; `css/css-position/static-position/htb-ltr-ltr.html`.
