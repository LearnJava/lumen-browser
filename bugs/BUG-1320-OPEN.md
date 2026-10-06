# BUG-1320 — `repeat(1000, …)` в 100 000 экземплярах: 100 млн дорожек, 12 ГБ памяти и 20 секунд

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** css-parser/layout — `crates/engine/layout/src/style/values/flexgrid.rs` (разбор `grid-template-columns`/`rows`) и `box_tree/grid.rs` (раскладка): число дорожек, получающихся из `repeat(<integer>, …)`, ничем не ограничено.

## Симптом

Страница `css/css-grid/parsing/grid-template-columns-crash.html`:

```js
let value = '';
for (let i = 0; i < 100000; ++i) value += ` repeat(1000, ${i}px)`;
document.body.style.gridTemplateColumns = value;
document.body.textContent = 'PASS';
```

`lumen --dump-layout` на варианте, где `<body style="display:grid">` и число итераций — параметр:

| итераций (дорожек) | время | пик памяти |
|---|---|---|
| 5 (5 000) | 0,34 с | — |
| 20 (20 000) | 0,28 с | — |
| 100 000 (100 000 000) | 20,5 с | 12,44 ГБ (`PeakWorkingSet64`) |

В корпусном прогоне `run_corpus.py --max-browser-gb 4.0` убивает процесс на 4,11 ГБ: тест получает `ERROR` (`browsingContext.navigate … ` без ответа). Тест — crashtest-образец: достаточно не упасть и не зависнуть.

## Как найдено

WPT-RUN-14 срез 8: единственный `ERROR` в `css/css-grid` (1222 id); `.tmp/wpt-run14/grid-2/rss-cap-kills.jsonl`.

## Что делать

Ограничить суммарное число дорожек (UA вправе ограничить число дорожек; другие движки держат порядка 10 000): при разборе `repeat()` с произведением, превышающим предел, обрезать (или считать значение невалидным — крэш-тест от этого не зависит), не материализуя вектор. Проверка — тот же скрипт: время < 1 с, пик < 500 МБ.

## Как проверить

`css/css-grid/parsing/grid-template-columns-crash.html` (ожидается `PASS`, без срабатывания RSS-капа).
