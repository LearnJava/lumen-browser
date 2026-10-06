# BUG-1306 — JS-геометрия (`offsetWidth`, `getBoundingClientRect`) читается из устаревшего снимка: после загрузки `@font-face` и после мутации DOM до следующего кадра отдаёт оценку шрифта

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** shell/js (`crates/shell/src/page_pipeline.rs::collect_js_layout_snapshot` — снимок геометрии для `_lumen_get_bounding_rect`; связано с [BUG-1273](BUG-1273-OPEN.md) — тот же дефект для `--screenshot`; архитектурный корень — путь relayout/rAF, ADR-016, BUG-935/BUG-286 в `BUGS.md`)

## Симптом

testharness-страница, `font: 25px/1 Ahem` (`/fonts/ahem.css`), `<span>XX</span>`, ширина должна быть 50:

| момент чтения | `offsetWidth` |
|---|---|
| сразу после разбора (`load`, `document.fonts.ready` уже разрешён, `fonts.status = loaded`, через 39 мс) | 34,1064453125 (= 2 · 17,0532 = 2 · 0,682 em: оценка «средний глиф», не Ahem) |
| первое чтение через 2000 мс простоя (до этого страница ничего не читала) | 34,1064453125 — снимок не обновился после загрузки шрифта |
| то же, но страница прочла значение сразу после разбора, а повторное чтение — через 2000 мс | 50 |
| опрос каждые 20 мс с `t = 0` | значение меняется на 50 на ≈ 61-м мс (две пробы: 61 и 62 мс) |
| сразу после `appendChild` (синхронно, дважды подряд) | 34,1064453125 |
| после следующего `requestAnimationFrame` | 50 (верно) |

Итого: чтение отдаёт то, что лежит в снимке, и само по себе пересчёта не вызывает синхронно; верное значение появляется на следующем кадре (≈ 60 мс). Первое чтение после загрузки шрифта без предшествующего чтения остаётся оценочным даже через 2 с. `checkLayout(".grid")` в `document.fonts.ready.then(...)` — как раз одно синхронное чтение.

Те же числа для `display:inline-block`, `float`, `position:absolute`, `inline-grid`. После мутации сразу видны семейства `Ahem`, `Courier New`, `monospace`, `Arial`, `serif` с одной шириной 68,21 (4 · 17,05), через кадр — 100 / 60,0 / 54,98 / 66,7 / 72,2. Значение, которое страница видит синхронно, — не раскладка, а оценщик; верное появляется только после асинхронного пересчёта.

Спецификация (CSSOM View §«layout box properties»): запрос геометрии — принудительная синхронная раскладка (`document.body.offsetTop` после мутации обязан видеть мутацию).

## Как найдено

WPT-RUN-14 срез 7: 219 id / 4 007 упавших сабтестов `css/css-grid` — `check-layout-th.js` тесты, `font: 25px/1 Ahem`, `document.fonts.ready.then(() => checkLayout(".grid"))` (`abspos/positioned-grid-descendants-*`, `orthogonal-positioned-grid-descendants-*`, `alignment/grid-alignment-implies-size-change-*`, `grid-self-alignment-*`…). Ожидаемое `width 50`, получено `34.1064453125`. Тот же приём (`fonts.ready` → измерение) — 545 файлов `css/` (css-text 114, css-fonts 36, css-values 16, css-sizing 11…). Сами раскладки в этих тестах могут быть верны — проверить нельзя, пока измерение читает оценщик.

## Что делать

(1) Запрос геометрии (`offsetWidth`/`offsetHeight`/`getBoundingClientRect`/`getClientRects`/`scroll*`/`client*`) после мутации DOM/стиля или завершения загрузки шрифта перекладывает документ синхронно (или помечает снимок грязным и пересчитывает на чтении). (2) Завершение загрузки `@font-face` инвалидирует снимок без ожидания мутации (первое чтение через 2 с простоя всё ещё оценочное). Пересечение с BUG-935/286 (relayout/rAF) — решение за P1 при выборе реализации.

## Как проверить

`css/css-grid/abspos/positioned-grid-descendants-001.html`, `alignment/grid-alignment-implies-size-change-001.html`; проба выше (`run_report.py --all --root css/css-grid/abspos --limit 12`, временный `promise_test` с `setTimeout`/`rAF` и чтением `offsetWidth`). После правки число упавших сабтестов в `css/css-grid` падает на величину этого кластера (4 007), остаток — GRID-ABSPOS/GRID-RTL/GRID-VWM-2.

## Дополнение: WPT-RUN-14 срез 8 (2026-10-06, `css/css-grid`, часть 2)

В `grid-model`, `layout-algorithm`, `placement`, `subgrid` приём `document.fonts.ready` → измерение даёт ещё 23 id / 414 сабтестов (`grid-model/grid-gutters-and-flex-content-001.html`, `grid-find-fr-size-gutters-001/002.html`, `layout-algorithm/grid-intrinsic-track-sizes-001.html` и др.), в `grid-lanes` — ещё 4 id / 210 сабтестов. Итого в `css-grid` целиком: 242 + 4 id и около 4 600 сабтестов, не говорящих ни о grid, ни об Ahem-раскладке.
