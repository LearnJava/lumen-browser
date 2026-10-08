# BUG-1468 — `position: sticky`: в корневом документе и внутри `overflow` контейнера липкий бокс не следует за прокруткой (`getBoundingClientRect` и снимок)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout/paint/shell (`crates/engine/paint/src/display_list/walk.rs:1056` — `BeginStickyLayer`; прокрутка вложенного контейнера в `crates/shell`)

## Симптом

Липкий бокс остаётся в потоке: после `scrollTop = 250` у контейнера с `overflow:auto` `getBoundingClientRect().top` липкого элемента не меняется (200 до и после при `top:100px`, ожидается 100), а на снимке `--screenshot` липкий зелёный квадрат в прокрученном контейнере стоит на позиции потока. Разные поверхности ведут себя по-разному: `--dump-display-list` на странице с `position:sticky` не выдаёт `BeginStickyLayer` вовсе (пишет обычный `FillRect`; тест `sticky_top_emits_begin_end_layer` строит список иначе, через `build_display_list`), `getBoundingClientRect` не знает о прокрутке контейнера ([BUG-1166](BUG-1166-OPEN.md)). Для `sticky` пикселей нет проверки ни в одном графическом тесте (`CC-CSS-3`: «нечем детерминированно заскриншотить прокрученное состояние»). 87 id среза: `css-position/sticky/` и `position-sticky-dynamic-ancestor-001` — 57 reftest `FAIL` (52 `thick`, 4 `thin-only`, 1 `identical`: снимки теста и эталона совпали, а в `wptrunner` он красный) и 30 testharness (63 из 76 упавших сабтестов; `position-sticky-top.html` ожидается 250, получено 200), плюс 5 `anchor-scroll-to-sticky-*` в кластере `anchor-scroll`.

## Проба

Проба (`--mcp`, контейнер 250×150 `overflow:auto`, липкий `top:100px`, `scrollTop=250`):

| проверка | у нас | ожидается |
|---|---|---|
| `st.getBoundingClientRect().top` до | 200 | 200 |
| `sc.scrollTop = 250; sc.scrollTop` | 250 | 250 |
| `st.getBoundingClientRect().top` после | **200** | 100 |
| `st.offsetTop` | 200 | 200 |
| `--dump-display-list` для `position:sticky;top:10px` в корне | нет `BeginStickyLayer` | `BeginStickyLayer … top=10` |
| `--screenshot`: контейнер 100 px, липкий `top:0`, высота 40, `scrollTop=80` | `60…99` — вне слоя прокрутки: не сдвинут и не прилип (`--dump-display-list`: `FillRect` стоит после `PopScrollLayer`) | `0…39`: прилип к верху контейнера |

Снимок `position-sticky-top-003.html` (`--viewport 800x600 --ahem`): три красных квадрата у цели, у эталона — три зелёных на их месте.

## Как найдено

WPT-RUN-14 срез 21: `css-position/sticky/position-sticky-top-003.html`, `position-sticky-top.html`, `position-sticky-left.html`.

## Что делать

Развести три поверхности: (1) почему `--dump-display-list` не содержит `BeginStickyLayer` при том, что `walk.rs` его пишет (путь `paint_ordered` против `build_display_list`; сверить со стеком `box_layer.rs:678`); (2) сдвиг `getBoundingClientRect`/`offsetTop` липкого элемента от `scrollTop` предка — общий с BUG-1166; (3) отрисовка `BeginStickyLayer` в CPU-растре `cpu_raster.rs` (`grep Sticky` даёт ноль — команда там не реализована, wgpu-путь `renderer.rs:3421` реализован), без чего `--screenshot` и `LumenRefTestExecutor` не видят прилипания. Правило `crates/engine/paint/CLAUDE.md`: новая команда реализуется в обоих растрах.

## Как проверить

Таблица выше; `css/css-position/sticky/position-sticky-top-003.html`, `position-sticky-top.html`.
