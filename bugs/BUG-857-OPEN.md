# BUG-857 — событие `selectionchange` не диспатчится нигде и никогда: в кодовой базе нет ни одного упоминания

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `selection-events`)
**Область:** `crates/js/src/dom.rs` — `grep -rn selectionchange crates/` даёт **ноль** совпадений; `onselectionchange` нет ни у `document`, ни у текстовых контролов. Соседний `select` (у `<input>`/`<textarea>`) диспатчится исправно
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
document.addEventListener('selectionchange', () => console.log('doc'));   // молчит
input.addEventListener('selectionchange', () => console.log('elem'));     // молчит
input.select();                       // 'select' приходит, 'selectionchange' — нет
input.setSelectionRange(1, 4);        // тишина
input.setRangeText('XY', 1, 3, 'select');
```

Спека (Selection API §3.5 + HTML LS §4.10.5.6) требует ставить задачу
`selection change` при каждом изменении выделения: у текстового контрола
событие всплывает от элемента к документу (`bubbles: true`, `composed: false`),
у обычного выделения приходит на `document`.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant selection-events`
(2026-08-23, dev-release, Linux, `main` = `530d0a444`, `--seconds 5`,
страница жива — 9 тиков):

| действие | ожидалось | получено |
|---|---|---|
| `input.select()` | `select` + `selectionchange` | только `select` |
| `input.setRangeText('XY',1,3,'select')` | `select` + `selectionchange` | только `select` (значение `aXYdef` — сам API исправен) |
| `textarea.setSelectionRange(1,4)` | `selectionchange` | тишина |
| `'onselectionchange' in input` | `true` | **false** |
| `document`-слушатель `selectionchange` | приходит на каждое из трёх | ни разу |

То есть механика выделения (`selectionStart/End`, `setRangeText`, `select`)
работает — отсутствует ровно уведомление.

## Масштаб

Механизм `selectionchange-never-fired` в `tests/wpt/timeout_audit.py` —
**4 id** остатка снимка WPT-RUN-5: `selection/textcontrols/selectionchange-bubble.html`
(зависшие подтесты `selectionchange bubbles from input`,
`… from input when focused`, `… from textarea`),
`html/semantics/forms/textfieldselection/textfieldselection-setRangeText.html`
(`text setRangeText fires a select event when fully selected` и соседние),
`selection/selection-nested-video.html`, `uievents/textInput/api.html`.

## Направление починки (не предписание)

Диспатчить `selectionchange` из тех же точек, откуда уже уходит `select`
(`setSelectionRange`, `select`, `setRangeText`, изменение позиции каретки в
редактируемом поле), плюс из обновления `window.getSelection()`. По спеке
событие ставится в очередь (не синхронно) и не более одного раза на задачу.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant selection-events` — ожидаются `se-elem-selectionchange` и
   `se-doc-selectionchange` на каждое из трёх изменений.
2. WPT: `run_report.py --all --root selection/textcontrols`.
