# BUG-1283 — оконный shell зависает на документе высотой порядка 10¹⁰ px, хотя layout и display list строятся за 0,4 с

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 4, `css/css-break`)
**Область:** shell (оконный конвейер после layout — не локализовано; не растр и не layout, см. «Что исключено»)

## Симптом

Страница с одним боксом высотой 10¹⁰ px и фоном:

```html
<!DOCTYPE html>
<div style="height:10000000000px;background:green"></div>
```

Под wptrunner (окно, BiDi) браузер не отвечает на BiDi-вызов 20 с — `TIMEOUT … (LUMEN_WPT_HARD_CAP)`. Это видно
и на crashtest (окно с отрисовкой), и на testharness, который `run_corpus.py` запускает с `--no-paint`.

Те же страницы через CLI быстрые: `--dump-layout`, `--dump-display-list` и `--screenshot --viewport 800x600` —
по 0,35–0,6 с, все с кодом 0.

Порог и зависимость (пробы под `run_corpus.py`, по одному процессу):

| страница | итог |
|---|---|
| `height: 1e9px; background: green` | PASS, 0,3 с |
| `height: 2.1e9px` / `2.2e9px`, тот же фон | PASS, ≈10 с каждый |
| `height: 1e10px`, тот же фон | TIMEOUT (> 20 с) |
| `height: 2.2e9px` без фона | PASS, 0,3 с |
| `margin-top: 1e10px; height: 10px; background: green` | PASS, 0,3 с |
| `<div style="height:1e10px"></div><p>x</p>` (текст на y ≈ 1e10) | TIMEOUT |
| `padding-top: 1e10px`, пустой `<div>` | PASS |

Время растёт с высотой нарисованного (или достижимого через текст) содержимого, а не с высотой документа:
маленький бокс на `y = 1e10` проходит.

## Что исключено

- **Растр.** С `--no-paint` (`NoPaintBackend`, без wgpu) зависание то же.
- **Layout и построение display list.** `--dump-layout`/`--dump-display-list` того же файла — 0,4 с;
  `DrawBorder (8.00, 8.00, 1008.00, 10000000000.00)` и т.п. строятся сразу.

Остаётся то, что оконный shell делает с готовым деревом и display list сверх CLI-снимка: что-то, пропорциональное
высоте содержимого в пикселях (тайлы, сетка hit-test, scroll-слои, снимок `layout_rects`, a11y — не проверялось).

## Как найдено

WPT-RUN-14 срез 4: три crashtest `css/css-break` — `nested-float-in-multicol-crash.html` (`padding-top: 1e10px`
у блоков с текстом в multicol), `grid/grid-large-end-border-crash.html` (`border-bottom: 4294967295px`),
`flexbox/flexbox-fragmentation-layout-001-crash.html` (рамки и `max-block-size` порядка 10¹⁰ px) — все три
TIMEOUT в прогоне и при повторном отдельном прогоне. Каждый crashtest проверяет только «браузер не упал и не
завис», так что для WPT это прямой провал, а на живой странице — зависание вкладки от одного правила CSS.

## Попутно (отдельный механизм, не этот баг)

`--screenshot` бокса с `border-top: ridge` (или `groove`) шириной ≥ 1e10 px зависает и в CLI (> 40 с;
`solid` той же ширины — 0,4 с, `ridge 1e7px` — 1,1 с): `border_bevel::side_pieces` обходит рамку построчно
(`for r in 0..h`) и попиксельно на стыках (`crates/engine/paint/src/border_bevel.rs:162`, `:177`), то есть время
линейно по ширине рамки в пикселях. Тот же код зовут wgpu (`renderer.rs`) и femtovg. В WPT с этим сталкивается
`flexbox-fragmentation-layout-001-crash.html` (`border-top: ridge 3289332275mm`), но он висит и по основной причине.

## Как проверить

Три crashtest выше — PASS под
`run_corpus.py --prefixes css/css-break/nested-float-in-multicol-crash.html,css/css-break/grid/grid-large-end-border-crash.html,css/css-break/flexbox/flexbox-fragmentation-layout-001-crash.html`;
страница из «Симптома» отвечает на BiDi быстрее секунды.
