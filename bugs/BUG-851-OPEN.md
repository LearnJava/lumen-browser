# BUG-851 — `<details>`: скриптовое изменение `open` не порождает `toggle` вовсе, а клик по `<summary>` переключает атрибут дважды — состояние не меняется, но событие о смене приходит

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 24 — живой замер, маркер `details-toggle-not-fired`)
**Область:** `crates/js/src/dom.rs:15522` (единственная точка диспатча `toggle` — слушатель `click` на `document`, ветка `tag === 'summary'`), `crates/js/src/dom.rs:15377` (`_lumen_run_activation_behavior`, ветка `SUMMARY` — второе переключение того же атрибута), отсутствие «attribute change steps» для `open`
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Два независимых дефекта одного элемента.

**1. Скриптовое изменение `open` молчит.** HTML LS §4.11.1 требует, чтобы
смена содержимого атрибута `open` (свойством, `setAttribute`, `removeAttribute`
или парсером) ставила задачу «details toggle event task» и та диспатчила
`toggle`:

```js
d.open = true;                 // ничего
d.setAttribute('open', '');    // ничего
d.removeAttribute('open');     // ничего
```

**2. Клик по `<summary>` переключает `open` дважды.** Событие приходит, но
состояние возвращается на место:

```js
d.addEventListener('toggle', () => console.log(d.open, d.hasAttribute('open')));
s.click();                     // внутри обработчика: true true
console.log(d.open);           // сразу после click(): false
```

То есть `<details>` не открывается ни одним доступным скрипту способом, а
единственное отправленное событие сообщает о переходе, которого не произошло.

## Прямое измерение

`tests/wpt/verify_frame_load_media_gaps.py --variant details-toggle
--variant details-summary-click` (2026-08-22, dev-release, Linux, коммит
`c583a90b4`, `--seconds 5`, страница жива — 9 тиков):

| действие | ожидалось | получено |
|---|---|---|
| `d.open = true` | `toggle` (oldState closed → open) | тишина |
| `d.removeAttribute('open')` | `toggle` | тишина |
| `d.setAttribute('open', '')` | `toggle` | тишина |
| `summary.click()` (закрытый) | `toggle` + `open === true` после клика | `toggle` (closed→open), **`open === false`, `hasAttribute('open') === false`, в `outerHTML` атрибута нет** |
| второй `summary.click()` | `toggle` (open→closed) | снова «closed→open», состояние снова не изменилось |

Замер `--variant click-attr-write` показывает, что дело не в откате записей
вообще: `setAttribute`/`className`, сделанные из обычного `click`-слушателя,
переживают диспатч (`caw-after`/`caw-later` видят их).

## Причина (локализована чтением кода)

Атрибут переключают **две** независимые точки, и обе срабатывают на один
скриптовый клик:

```js
// dom.rs:15522 — слушатель click на document, он же диспатчит toggle
if (wasOpen) { _lumen_remove_attr(pid, 'open'); }
else         { _lumen_set_attr(pid, 'open', ''); }
...
_lumen_dispatch(pid, toggleEvt);

// dom.rs:15377 — activation behaviour, вызывается ПОСЛЕ диспатча из
// HTMLElement.prototype.click() (dom.rs:15417) и dispatchEvent (dom.rs:4701)
if (_lumen_has_attr(parent, 'open')) _lumen_remove_attr(parent, 'open');
else _lumen_set_attr(parent, 'open', '');
```

Первая точка меняет атрибут во время диспатча (поэтому обработчик `toggle`
видит новое состояние), вторая возвращает его обратно после. Ни одна из них не
привязана к изменению атрибута как такового, поэтому путь «скрипт меняет
`open`» не диспатчит ничего.

Побочно: событие отправляется синхронно, тогда как спека ставит задачу, —
`toggleEvent.html` проверяет и это. Интерфейс события (`ToggleEvent` вместо
`Event`) — отдельный баг [BUG-578](BUG-578-OPEN.md).

## Масштаб

Маркер `details-toggle-not-fired` в `tests/wpt/timeout_audit.py` — **1 id**
остатка снимка WPT-RUN-5
(`html/semantics/interactive-elements/the-details-element/toggleEvent.html`),
но внутри него 9 из 11 подтестов в состоянии NOTRUN/TIMEOUT, и разделение
ровно по механизму: все «… should fire a toggle event» висят, а
«Setting open=false on a closed 'details' element should **not** fire a toggle
event» проходит. Родственный id
`the-summary-element/anchor-with-inline-element.html` висит по другой причине
([BUG-837](BUG-837-OPEN.md)).

## Направление починки (не предписание)

Перенести переключение `open` в activation behaviour (одну точку), а диспатч
`toggle` — в шаги изменения атрибута `open`, поставленные в очередь задачей.
Тогда оба пути — клик и скрипт — обслуживаются одним механизмом, а двойное
переключение исчезает само.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
   --variant details-toggle --variant details-summary-click` — ожидаются
   `details-toggle` на каждое изменение `open` и совпадение состояния до/после
   `click()`.
2. WPT: `run_report.py --all --root html/semantics/interactive-elements` —
   `toggleEvent.html` должен перестать быть TIMEOUT.
