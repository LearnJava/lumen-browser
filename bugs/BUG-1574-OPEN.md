# BUG-1574 — Программная прокрутка (`scrollTo`/`scrollBy`/`scrollTop =`/`scrollIntoView`) не привязывается к точкам snap и не учитывает `scroll-padding`

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout/shell/js (`find_scroll_snap_y` по `CSS-SPECS.md` подключён к прокрутке из shell, а `scrollTo`/`scrollTop` его не вызывают — установлено пробой, не чтением кода; `scroll-padding` контейнера без snap не участвует в `scrollIntoView`)

## Симптом

По `CSS-SPECS.md` («Scroll Snap L1») привязка подключена к прокрутке из shell (`start_smooth_scroll`/`scroll_x_by`); колёсико и клавиши в этом срезе не проверены (тесты `input/*` — TIMEOUT, причина не установлена). JS-прокрутка не привязывается — проба ниже. Позиция после программной прокрутки произвольная; `scroll-padding` контейнера без snap в `scrollIntoView` не участвует (`scroll-margin` участвует).

## Проба

`run_smoke.py` + testharness, контейнер 200×200, дети по 150 px, `scroll-snap-align: start`:

| действие | у нас | ожидается |
|---|---|---|
| `scroll-snap-type: y mandatory`, `scrollTo(0,100)` | 100 | 150 |
| то же, `scrollTop = 40` | 40 | 0 |
| после `scrollTop = 40` — `scrollBy(0,170)` | 210 | 150 (привязка 0 → 170 → 150) |
| `scroll-padding-top: 20px` без snap, `children[2].scrollIntoView()` | 300 | 280 |
| `scroll-margin-top: 30px` у ребёнка, `scrollIntoView()` | 270 | 270 |
| `scroll-snap-type: y mandatory` + `scroll-padding-top: 20px`, `scrollTo(0,140)` | 140 | 130 |

## Как найдено

WPT-RUN-14 срез 26: `css/css-scroll-snap/scrollTo-scrollBy-snaps.html` (40 сабтестов), `snap-after-relayout/*`, `scroll-target-align-*`/`scroll-target-padding-*`/`scroll-target-margin-*` (25 reftest `thick`; `scrollIntoView()` вызывают 9 из них, причина остальных 16 не установлена).

## Что делать

Прогонять результат JS-прокрутки через ту же функцию выбора snap, что использует shell (с учётом `scroll-snap-stop`, направления и `proximity`), после смены `scroll-snap-type`/`scroll-snap-align`/размеров выполнять `re-snap`, а `scroll-padding` включать в вычисление `scrollIntoView` независимо от snap.

## Как проверить

`css/css-scroll-snap/scrollTo-scrollBy-snaps.html` (после BUG-1573), `scroll-target-align-002.html`.
