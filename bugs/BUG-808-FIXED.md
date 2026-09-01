# BUG-808 — WAAPI `Animation` не EventTarget: `animation.addEventListener('finish', …)` не срабатывает никогда, работает только свойство `onfinish`

**Статус:** FIXED 2026-08-24 (P1)
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 15 — побочная находка пробы `verify_event_delivery_gaps.py`)
**Область:** `crates/js/src/dom.rs:15333` (`function Animation(...)` — прототип не наследует `EventTarget` и не заводит собственный реестр слушателей), `dom.rs:15507` (`Animation.prototype._onFinish` — зовёт только `this.onfinish`), тот же паттерн в `Animation.prototype.cancel` (`this.oncancel`)
**Владелец:** P1 (движок). Заведён P2 в ходе WPT-задачи, починен P1 2026-08-24.

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
переприменяют стиль) и не [BUG-463](BUG-463-FIXED.md) (там `animate` висит на
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

## Починено (P1, 2026-08-24)

`Animation` стал настоящим `EventTarget`, все три типа события (`finish`,
`cancel`, `remove`) идут через один путь. `crates/js/src/dom.rs`, блок
`WEB_API_SHIM_TAIL_B`:

* конструктор зовёт `EventTarget.call(this)` — реестр слушателей на инстансе;
* `Animation.prototype = Object.create(EventTarget.prototype)` стоит **выше**
  всех `Object.defineProperty(Animation.prototype, …)`. Порядок здесь не
  косметика: замена объекта прототипа ниже аксессоров молча снесла бы
  `currentTime`/`startTime`/`playState`/`pending` и каждый метод;
* `_onFinish` и `cancel()` больше не зовут `this.onfinish`/`this.oncancel`
  напрямую, а вызывают общий `_fire(type)`; `dispatchEvent` базового класса сам
  проходит слушателей, а потом свойство `on<type>`;
* тип события — `AnimationPlaybackEvent` (§4.4.3) вместо голого `Event`, с
  `currentTime`/`timelineTime`; у `cancel` `currentTime === null` (§4.4.1).

### Два условия, которых в заявке не было, и оба решают исход

**1. Событие обязано ставиться задачей, а не диспатчиться синхронно.**
Спека говорит «queue a task to fire an animation playback event», и WPT
опирается на это буквально: `EventWatcher` вооружается **после** вызова,

```js
const w = new EventWatcher(t, animation, 'finish');
animation.finish();
await w.wait_for('finish');
```

— при синхронной доставке событие приходит, когда ожидания ещё нет, и
`EventWatcher` валит сабтест как «Not expecting event, but got finish event».
Замерено обеими версиями: синхронный `dispatchEvent` дал в `scroll-animations`
**409 → 407** сабтестов (то есть починка «наполовину» была бы регрессией),
задача — **409 → 410**. Задача пишется прямо в `_lumen_timers` с `nesting: 0`
(паттерн `_ro_schedule_initial`, BUG-661/BUG-807), чтобы клампа §8.6 в 4 мс не
было; объект события строится **в момент постановки**, поэтому времена в нём —
времена постановки, как того и требует спека.

**2. Новые члены прототипа обязаны быть неперечислимыми.**
`web-animations/interfaces/Animation/style-change-events.html` строит по
сабтесту на каждый ключ `Object.keys(Animation.prototype)`. Обычные
присваивания `constructor`/`_fire`/`_onRemove` изобрели три падающих сабтеста с
именами вида «Animation._fire produces expected style change events» — то есть
починка сама увеличила бы знаменатель тестами, названными внутренностями
движка. Все три заведены через `Object.defineProperty` без `enumerable`.

Соседняя находка, **не** тронутая здесь: тем же способом в этот тест уже
попадают давние внутренности `_scheduleRaf`/`_cancelRaf`/`_tick`/`_applyAtP`/
`_clearStyles`/`_onFinish` — в настоящем браузере `Object.keys` на этом
прототипе пуст. Это отдельный дефект формы объекта, а не событий, и трогать его
здесь значило бы сдвинуть знаменатель замера.

### Замер (A/B, одна машина, два бинарника dev-release)

| Категория | До | После |
|---|---|---|
| `web-animations` (217 файлов) | 971/4189 сабтестов, harness 135/157 | **974/4189**, harness 135/157 |
| `scroll-animations` (234) | 409/1956, harness 185/234 | **410/1956**, harness 185/234 |
| `css/css-animations` (155) | 408/1334, harness 118/155 | 408/1334, harness 118/155 |

`css/css-animations` нейтральна. Разовый `ERROR` на
`display-none-dont-cancel.tentative.html` в первом прогоне (harness 117 вместо
118) на повторе не воспроизвёлся ни на одном из двух бинарников — флак упавшей
функции очистки, не регрессия; состав сабтестов в обоих прогонах совпадал
побайтно.

Что ушло из отчётов (текст ассертов, `web-animations` + `scroll-animations`):

* `promise_test: Unhandled rejection with value: object "TypeError: animation.addEventListener is not a function"`;
* то же для `watchedNode.addEventListener` (это `EventWatcher` изнутри
  `testharness.js`);
* `event.currentTime should be null expected (object) null but got (undefined) undefined`;
* `event.currentTime should be the effect end expected a number but got a "undefined"` —
  превратилось в расхождение по точности (`ожидалось 100000, получено 100040`),
  то есть барьер сменился с «свойства нет» на «время считается неточно».

Проба из раздела «Как проверить фикс» — до: `waapi-finish`; после:
`waapi-finish-listener, waapi-finish`.

### Что осталось за границей

* `remove` (§4.4.2) диспатчить по-прежнему некому: автоматической замены
  анимаций в движке нет ([BUG-704](BUG-704-OPEN.md)). `_fire` заводит для него
  единственную точку вызова (`_onRemove`), чтобы, когда замена появится,
  событие не пришлось изобретать второй раз, и чтобы
  `addEventListener('remove', …)` уже был подключён.
* `event.timelineTime` приходит `null`, пока не прошёл первый тик rAF:
  `DocumentTimeline.currentTime` считается от `_wa_current_time`, а тот
  двигается только в тике. Из-за этого два сабтеста
  `web-animations/interfaces/Animation/finish.html` теперь падают на
  `expected a number but got a "object"` — раньше те же сабтесты падали на
  `TypeError` от `addEventListener`, то есть файл стал доходить дальше. Это
  дефект модели времени, а не событий.
* Порядок «слушатели, потом `on<type>`» взят у `EventTarget.prototype.dispatchEvent`
  — общая для всего шима аппроксимация (спека считает `on<type>` обычным
  слушателем, зарегистрированным в момент присваивания). Менять её точечно для
  одного `Animation` значило бы развести поведение внутри одного шима.

### Регрессионные тесты

`crates/js/src/dom.rs`, `mod tests`: `wa_animation_is_event_target`,
`wa_animation_finish_reaches_listener_and_property`,
`wa_animation_cancel_delivers_playback_event`,
`wa_animation_remove_event_listener_detaches`,
`wa_animation_new_prototype_members_are_not_enumerable`,
`wa_animation_accessors_survive_event_target_prototype`. Все они зовут
`_lumen_tick_timers()` после `finish()`/`cancel()` — по условию 1 событие
приходит задачей; существующий `animation_finish_fires_onfinish` обновлён тем
же образом.
