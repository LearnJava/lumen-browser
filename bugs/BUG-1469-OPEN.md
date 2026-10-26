# BUG-1469 — Привязка к прокрутке для CSS Anchor Positioning не реализована: якорёный бокс не следует за прокруткой контейнера якоря

**Статус:** OPEN (ДОРАБОТКА → ANCHOR-SCROLL-2)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout/paint/js (`crates/engine/layout/src/anchor.rs` — `apply_anchor_positions` работает от геометрии без учёта прокрутки; скролл-контейнеры якорей)

## Симптом

Бокс с `position-anchor` + `anchor()` остаётся там, где его поставил layout без прокрутки: якорь внутри `overflow:auto` контейнера с `scrollTop=250` — `getBoundingClientRect` якоря и цели остаются `(50,300)` и `(50,330)` (CSS Anchor Positioning 1 §4.3: цель должна ехать с якорем; WPT `anchor-scroll-002.html`: ожидается совпадение `top` цели и якоря и 40/155/270 после прокрутки). Нет и обратного: `position-area` не учитывает прокрутку (`position-area-scrolling-*`), якорь вне видимой части не скрывает цель (`position-visibility`, см. CSS-SPECS), цепочки и `anchor-scope`. Не помогает и `getBoundingClientRect` контейнера ([BUG-1166](BUG-1166-OPEN.md)): даже без него `--screenshot` рисует цель на месте непрокрученного якоря (цель `position:fixed` с `left:anchor(left)` при `scrollTop=250`: синий якорь на y=50…79, зелёная цель на y=300…329 вместо y=50). 86 id среза: 52 reftest `thick`, 32 testharness (`OK`), 1 `ERROR`, 1 `TIMEOUT` — `anchor-scroll-*` (28), `position-area-scrolling-*`, `anchor-scroll-position-try-*` (14, ожидаемые числа 258/724/229 при полученных 358/924/429: ровно сдвиг на прокрутку), `anchor-scroll-update-*`, `anchor-scroll-chained-*`, `anchor-scroll-to-sticky-*`, `anchor-center-scroll*`, `scrollable-containing-block-*`.

## Проба

Проба (`--mcp`, контейнер `position:relative;height:100px;width:300px;overflow:auto`, внутри блок 600 px с якорем на `top:300px`, цель вне контейнера `left:anchor(left);top:anchor(bottom)`):

| проверка | у нас | ожидается |
|---|---|---|
| якорь до прокрутки | `(50,300)` | `(50,300)` |
| цель до прокрутки | `(50,330)` | `(50,330)` |
| якорь после `scrollTop=250` | `(50,300)` (это BUG-1166, не якорная позиция) | `(50,50)` |
| цель после `scrollTop=250` | `(50,330)` | `(50,80)` |
| `--screenshot`: цель `position:fixed; left:anchor(left); top:anchor(top)`, якорь в контейнере с `scrollTop=250` (`--viewport 800x600`) | якорь `y=50…79`, цель `y=300…329` | цель на `y=50` (поверх якоря) |

Решающая строка — снимок: цель берёт координаты якоря без прокрутки.

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/anchor-scroll-002.html`, `anchor-scroll-position-try-001.html`, `position-area-scrolling-001.tentative.html`.

## Что делать

Задача ANCHOR-SCROLL: учёт смещения прокрутки контейнеров якоря в `apply_anchor_positions` (компенсация в `BeginScrollLayer`-координатах или перерасчёт при прокрутке), в обоих растрах, и пересчёт при `scroll`-событии. Зависит от BUG-1166 для тестов на `getBoundingClientRect`.

## Как проверить

Таблица выше; `css/css-anchor-position/anchor-scroll-002.html`, `anchor-scroll-position-try-001.html`.

## Срез 1 (ANCHOR-SCROLL, 2026-10-21)

Влит показ: `LayoutBox::anchor_scroll` (контейнеры прокрутки вокруг якоря, которых нет среди предков цели; цепочки якорь-на-якоре), `refresh_anchor_scroll` (смещения на каждой прокрутке), paint оборачивает цель в `PushScrollLayer` с `id` контейнера (`patch_scroll_layer`/`apply_scroll_overrides` двигают её без пересборки). Проба из таблицы выше: цель `y=80` при якоре `y=50…79` (`scrollTop=250`, `--viewport 800x600`), цель `position:fixed` — на `y=20` при якоре на `y=20`. A/B по 210 reftest `css/css-anchor-position`: identical 40→50, без регрессий.

**Осталось (ANCHOR-SCROLL-2):** прямоугольники для скриптов (`getBoundingClientRect`/`offsetTop`) — прокрутку предков-скроллеров не вычитают (BUG-1166), а у цели нет сдвига `anchor_scroll`; `position-area` в прокручиваемом содержащем блоке; `position-try-fallbacks` при прокрутке (BUG-562); `position: fixed` + прокрутка страницы; якорь-`inline` (BUG-1477); hit-test цели по непрокрученному прямоугольнику. Из трёх критериев задачи (`anchor-scroll-002.html`, `-003.html`, `position-area-scrolling-005.html`) ни один не зелёный — все три читают геометрию через `getBoundingClientRect`/`offsetLeft`.

## Срез 2 (ANCHOR-SCROLL-2, 2026-10-26)

Геометрия для скриптов: `getBoundingClientRect`/`getClientRects`/`getBoxQuads` сдвигаются при чтении на прокрутку контейнеров-предков и контейнеров якоря (`_lumen_get_scroll_shift`), `offsetLeft`/`offsetTop` сохраняют только сдвиг по якорю. Оси раздельно: ось следует прокрутке якоря своих `anchor()`-вставок (`anchor-scroll-006`). `anchor-scroll-002`, `-006`, `-007` зелёные. **Осталось (ANCHOR-SCROLL-3):** `position-area` в прокручиваемом блоке, `position-try`, `fixed` при прокрутке страницы, inline-якорь без бокса (BUG-1477), hit-test цели.
