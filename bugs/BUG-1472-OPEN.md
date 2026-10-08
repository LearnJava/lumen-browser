# BUG-1472 — `position: relative` на `<span>`, внутри которого блок, не сдвигает этот блок (блок-в-инлайне)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs::relative_offset` — `position:relative` на inline-боксе, содержащем блок)

## Симптом

Когда inline-бокс с `position:relative` разорван блоком-потомком, смещение inline-бокса применяется и к анонимным блокам, в которые разорван его поток, и к самому блоку (CSS 2.1 §9.2.1.1, §9.4.3). У нас смещение пропадает: `<span style="position:relative;top:50px;left:50px"><div id=g style="width:20px;height:20px"></div></span>` — `g` в `(0,0)`, ожидается `(50,50)`. Если перед блоком есть текст, он тоже не сдвигается как строка, а блок остаётся в потоке. `position-relative-001.html` строится на этом: `span{position:relative;top:100%;left:100%}` с внутренним `div{position:relative;top:-100px;left:-100px}` даёт `(-100,-100)` вместо `(0,0)`. 3 id среза: `position-relative-001.html`, `-002.html`, `-004.html` (проба — на структуре `-001`; `-002` и `-004` — по разметке: тот же `span{position:relative}` с блоком внутри). Остальные 12 id `position-relative-*` — другие дефекты, уже заведённые: процент от ширины вместо высоты ([BUG-1422](BUG-1422-OPEN.md); проба: `top:50%` у блока в контейнере высотой 200 px даёт `y=512` при ширине окна 1024, а на странице 300×200 — `y=150` вместо `100`: `015`, `016`, `aspect-ratio-001/002`) и `position:relative` на `tr`/`td` ([BUG-1471](BUG-1471-OPEN.md): `008…013`; `006`, `007` — процент от неопределённой высоты, по разметке).

## Проба

Проба (`--mcp`):

| разметка | `g` у нас | ожидается |
|---|---|---|
| `span{position:relative;top:50px;left:50px}` > `div#g` 20×20 | `(0,0)` | `(50,50)` |
| то же, перед `div` текст `t` | `(0,19.4)` | `(50,69.4)` |
| `div{height:100px}` > `span{relative;top:100%;left:100%}` > `div{relative;top:-100px;left:-100px}` | `(-100,-100)` | `(0,0)` |

## Как найдено

WPT-RUN-14 срез 21: `css-position/position-relative-001.html`, `position-relative-002.html`, `position-relative-004.html`.

## Что делать

Применять `relative_offset` inline-бокса к блокам, на которые он разорван, при построении block-in-inline (анонимные блоки получают смещение родителя-inline).

## Как проверить

Таблица выше; `css/css-position/position-relative-001.html`.
