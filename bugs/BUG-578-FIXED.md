# BUG-578: `ToggleEvent` interface missing — popover/`<details>` toggle fires a plain `Event` with hand-bolted `oldState`/`newState`

**Статус:** FIXED 2026-08-25 (P1)
**Компонент:** js (`crates/js/src/dom.rs:14888-14937` popover/`<details>`
toggle dispatch, `dom.rs:15013-15042` popover show/hide toggle dispatch — no
`ToggleEvent` constructor is defined anywhere in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL the event is an instance of ToggleEvent - ToggleEvent is not defined
FAIL ToggleEvent constructed with no arguments throws - ToggleEvent is not defined
...
```

154 occurrences (`popovers/toggleevent-interface.html` and every
`interactive-elements/the-details-element/*toggle*` test).

## Причина

Both dispatch sites that fire a `toggle`/`beforetoggle` event
(`<details>` at `dom.rs:14903/14924`, popover show/hide at
`dom.rs:15013/15042`) construct a plain `Event` and then manually assign
`oldState`/`newState` as ordinary own properties:

```js
var toggleEvt = new Event('toggle', { bubbles: false, cancelable: false });
toggleEvt.oldState = oldState;
toggleEvt.newState = newState;
_lumen_dispatch(pid, toggleEvt);
```

Per HTML LS, both events must be real `ToggleEvent` instances
(`ToggleEvent extends Event`, with `oldState`/`newState` as constructor-init
read-only accessors, `cancelable` defaulting `true` for `beforetoggle`). No
`ToggleEvent` constructor/prototype exists anywhere in `dom.rs` — grep for
`ToggleEvent` returns zero hits. Consequences beyond the interface-identity
check: `new ToggleEvent(...)` throws `ReferenceError` (breaks any test or
page script that constructs one directly, e.g. to dispatch a synthetic
toggle), `event instanceof ToggleEvent` always throws before it can even be
false, and `oldState`/`newState` are plain writable own properties instead
of the spec's read-only accessors.

## Масштаб

Medium, self-contained: every listed subtest lives in either
`popovers/toggleevent-interface.html` or
`interactive-elements/the-details-element/*toggle*`, i.e. the two dispatch
sites named above are the only two that need to switch constructors. The
functional show/hide/open/close behavior itself works today (confirmed:
`popover_toggle_event_fired`/`popover_beforetoggle_event_fired` unit tests
pass at `dom.rs:31137-31148`) — this is purely an event-*type* gap, not a
missing feature.

---

## Починено (P1, 2026-08-25)

Закрыт попутно с [BUG-851](BUG-851-FIXED.md): без `ToggleEvent` тот фикс не
измерим — `toggleEvent.html` в собственном `testEvent()` проверяет
`Object.getPrototypeOf(evt) === ToggleEvent.prototype` у **каждого** полученного
события, поэтому сколь угодно правильные `oldState`/`newState` всё равно давали
FAIL.

Класс добавлен рядом с `HashChangeEvent` (`crates/js/src/dom.rs`) и разведён по
всем пяти точкам диспатча: `<details>` (одна — новая, после BUG-851) и popover
(`beforetoggle`/`toggle` на показ и на скрытие). Три вещи сверх «завести
конструктор», каждую из которых потребовал прогон:

- **`oldState`/`newState` — readonly-аксессоры**, а не собственные записываемые
  свойства: `assert_readonly` идёт по цепочке прототипов и принимает либо
  дескриптор данных с `[[Writable]] === false`, либо аксессор без `[[Set]]`.
- **Конверсия WebIDL `DOMString` у членов словаря**: член, явно выставленный в
  `undefined`, считается отсутствующим (значение по умолчанию `''`), а `null`
  становится строкой `'null'`, не `''`.
- **`source`** (Popover API L2, `Element?`) — есть; `relatedTarget` намеренно
  отсутствует, это отдельный подтест.

Заодно исправлено то, что заявка отметила в скобках: `beforetoggle` popover'а
диспатчился с `cancelable: false`, так что `preventDefault()` в обработчике не
делал ничего. Теперь событие отменяемое, и отмена прерывает показ/скрытие
(Popover API §3.5).

### Замер

A/B по `html/semantics/popovers` (dev-release, Windows): подтесты
999/3886 → **1036/3886**, harness OK 80/103 → **81/103**.
`toggleevent-interface.html` 0/39 → **36/39**, `popover-events.html`
ERROR → **OK**, `popover-toggle-source.html` 0/7 → 1/7. Регрессий нет.
Цифры по `<details>` — в [BUG-851](BUG-851-FIXED.md).

**Остаток (не про `ToggleEvent`):** три подтеста
`toggleevent-interface.html` проверяют базовый конструктор `Event` —
`new Event()` без аргумента не бросает `TypeError`, а `null`/`undefined` в
качестве типа дают `''` вместо `'null'`/`'undefined'`; это общее для всех 26
классов событий шима. `<details>` своего `beforetoggle` по-прежнему не
диспатчит вовсе.
