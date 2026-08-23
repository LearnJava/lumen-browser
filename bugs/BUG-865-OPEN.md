# BUG-865 — опция `passive` у `addEventListener` не разбирается вовсе: `preventDefault()` работает там, где спека требует его игнорировать

**Статус:** OPEN
**Заведён:** 2026-08-23 (P2, `WPT-VENDOR-dom-rest` — первый прогон довендоренной категории `dom`)
**Область:** `crates/js/src/dom.rs:443-460` — `EventTarget.prototype.addEventListener` читает из `options` только `capture` и `once`; слова `passive` нет во всём `crates/js/src/dom.rs` (единственное совпадение по крейту — `navigator_bindings.rs`, к событиям отношения не имеет)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
addEventListener('touchstart', e => e.preventDefault(), {passive: true});
// событие всё равно становится defaultPrevented — опция молча выброшена
```

DOM §2.7/§2.8 требует: (1) при `passive: true` вызов `preventDefault()` внутри
слушателя — no-op (`in passive listener` флаг), (2) для `touchstart`/`touchmove`/
`wheel`/`mousewheel`, повешенных на `Window`, `Document` или `document.body`,
значение `passive` по умолчанию — `true`, а не `false`.

Побочно ломается и стандартный приём определения поддержки опций-словаря:
`addEventListener('x', null, {get passive(){ supported = true; }})` — геттер
никогда не дёргается, потому что `options.passive` не читается.

## Прямое измерение

Прогон `run_report.py --all --root dom --recursive` (2026-08-23, Linux, dev-release):

| Тест | Результат |
|---|---|
| `/dom/events/AddEventListenerOptions-passive.any.{html,worker.html,…}` | все сабтесты FAIL, в т.ч. `assert_true: addEventListener doesn't support the passive option expected true got false` и `Incorrect defaultPrevented for options: {"passive":true} expected false but got true` |
| `/dom/events/passive-by-default.html` | 57 FAIL `assert_equals: defaultPrevented expected false but got true` — вся матрица «тип события × цель» |
| `/dom/events/non-cancelable-when-passive/*` | 6 файлов, `promise_test: Unhandled rejection` |

## Что чинить

1. Читать `options.passive` в `addEventListener` (и учитывать его в ключе
   дедупликации слушателей — по спеке ключ это `(type, callback, capture)`,
   `passive` в ключ не входит, но должен сохраняться в записи).
2. Реализовать дефолт `passive: true` для `touchstart`/`touchmove`/`wheel`/
   `mousewheel` на `Window`/`Document`/`body`.
3. В пути диспатча выставлять флаг «в пассивном слушателе» и делать
   `Event.prototype.preventDefault` (и присваивание `returnValue = false`)
   no-op на время вызова такого слушателя.

Отдельно от «не поддерживается» это ещё и **тихий** дефект: страница, которая
просит пассивный слушатель ради скролла, получает обычный, и её
`preventDefault()` действительно отменяет скролл — поведение противоположно
запрошенному, без единого предупреждения.
