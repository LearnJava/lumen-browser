# BUG-1599 — `mix-blend-mode`: элемент смешивается с фоном страницы, а не с группой родителя-стекинг-контекста

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** paint (`crates/engine/paint/src/display_list/` — группа `mix-blend-mode`; `cpu_raster.rs`)

## Симптом

`mix-blend-mode` по спецификации смешивает элемент с содержимым **своего стекинг-контекста** (Compositing 1 §3.2, isolated group), а за его пределами — не смешивает. Родитель с `position:absolute; z-index:1` создаёт контекст; потомок с `mix-blend-mode: difference`, вылезающий за границы родителя, вне родителя должен остаться как есть. У нас вылезшая часть смешивается с белым фоном страницы.

## Проба

`css/compositing/mix-blend-mode/mix-blend-mode-blended-element-with-transparent-pixels.html`, `--screenshot` 800×600, цвета пикселей рядом с границей родителя:

| точка | у нас | эталон |
|---|---|---|
| внутри родителя (30,100), (60,80) | (0,255,0) | (0,255,0) |
| вне родителя, в полосе вылезшего `#0FF` (125,60) | **(255,0,0)** — `difference(белый, #0FF)` | **(0,255,255)** |

То есть смешивание внутри группы работает, а изоляция группы — нет. Контрольная страница `.tmp/s28/mb.html` (синий родитель с `z-index:1` и `#0ff`-потомок `difference`) подтверждает: внутри (0,255,0), снаружи — красный.

## Охват

WPT-RUN-14 S28: 40 id — `compositing/mix-blend-mode/*` (29 reftest, 26 `thick`), `compositing/svg/*` (2), `compositing/root-element-*` (5, из них 4 TIMEOUT-`crashtest` на новом бинаре — см. BUG-1597), `compositing/background-blending/*`. Причина установлена пробой только для `…with-transparent-pixels`; в остальных группах (3D-transform, `canvas`, `iframe`, `mask`, `filter`, `overflow`+`border-radius`, SVG) — по именам, возможны отдельные дефекты.

## Как проверить

`css/compositing/mix-blend-mode/mix-blend-mode-blended-element-with-transparent-pixels.html`, `…-overflowing-child.html`, `…-parent-with-text.html`.
