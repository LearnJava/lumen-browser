# BUG-808 — WAAPI `Animation` не EventTarget: `animation.addEventListener('finish', …)` не срабатывает никогда, работает только свойство `onfinish`

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 15 — побочная находка пробы `verify_event_delivery_gaps.py`)
**Область:** `crates/js/src/dom.rs:15333` (`function Animation(...)` — прототип не наследует `EventTarget` и не заводит собственный реестр слушателей), `dom.rs:15507` (`Animation.prototype._onFinish` — зовёт только `this.onfinish`), тот же паттерн в `Animation.prototype.cancel` (`this.oncancel`)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

На одной и той же анимации срабатывает свойство, а слушатель бросает
`TypeError` (страница при этом продолжает жить — исключение никто не видит):

```js
var anim = el.animate([{opacity: 1}, {opacity: 0}], {duration: 100});
anim.onfinish = () => console.log("A");                      // печатается
anim.addEventListener("finish", () => console.log("B"));     // TypeError
```

Замер — `tests/wpt/verify_event_delivery_gaps.py --variant waapi-finish`
(живое окно, http, dev-release, Linux, 2026-08-21): маркер `waapi-finish`
(свойство) в логе есть, `waapi-finish-listener` (слушатель) — нет, дважды
подряд.

## Причина (локализована чтением кода)

`Animation` — обычный JS-класс шима, а не `EventTarget`:
`Animation.prototype._onFinish` (`dom.rs:15507`) состоит из вызова
`this.onfinish(new Event('finish'))` и резолва промиса `finished`; ничего,
что просматривало бы список слушателей, в прототипе нет, и цепочка
прототипов к `EventTarget` не подключена (`grep -n "Animation.prototype = "`
— нет строки, в отличие от `EventSource`/`Performance`, где
`Object.create(EventTarget.prototype)` стоит). Поэтому вызов не просто молчит,
а **бросает**: в логе пробы стоит
`script error: JS runtime error: anim.addEventListener is not a function` — и
дальше по [BUG-591](BUG-591-FIXED.md) исключение не доходит до `testharness.js`,
так что тест не падает, а виснет. То же самое у `cancel` (`this.oncancel`) и у
события `remove`, которого нет вовсе.

Не дубль [BUG-530](BUG-530-OPEN.md) (там `pause()`+`currentTime` не
переприменяют стиль) и не [BUG-463](BUG-463-OPEN.md) (там `animate` висит на
инстансе, а не на прототипе) — это третий, независимый пробел того же шима.

## Масштаб

В вендоренном корпусе 32 файла используют идиому
`addEventListener('finish'|'cancel'|'remove')` / `new EventWatcher(t,
animation, …)`; из них 4 — в `web-animations`. **В остатке `unclassified`
снимка WPT-RUN-5 таких id ноль** — эти файлы сегодня падают раньше и по
другим причинам, поэтому отдельного механизма в `timeout_audit.py` дефект не
получил. Цена по корпусу проявится после починки более ранних дефектов
(BUG-463/BUG-530), и тогда это станет следующим барьером в `web-animations`.

## Направление починки (не предписание)

Дать `Animation` полноценный `EventTarget`: собственный реестр слушателей в
конструкторе и общий `_dispatch(type, event)`, который зовёт и свойство
`on<type>`, и всех зарегистрированных слушателей — в порядке спеки (свойство
участвует как обычный слушатель, зарегистрированный в момент присваивания).
Покрыть три типа: `finish`, `cancel`, `remove`.

## Как проверить фикс

`verify_event_delivery_gaps.py --variant waapi-finish` печатает оба маркера —
`waapi-finish` и `waapi-finish-listener`.
