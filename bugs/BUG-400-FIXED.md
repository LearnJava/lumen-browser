# BUG-400 — `performance` не реализует интерфейс `Performance` целиком: не наследует `EventTarget`, нет `toJSON()`

**Статус:** FIXED 2026-08-11
**Компонент:** js (`crates/js/src/dom.rs` — блок `// ── performance (HR Timer …`, бывший `var performance = {…}`, объектный литерал)
**Найден:** P2, WPT-VENDOR-hr-time (2026-07-28), прогон `run_report.py --all --root hr-time --recursive`

## Симптом

`tests/wpt/hr-time/basic.any.js`, подтест «Performance interface extends
EventTarget»:

```
FAIL Performance interface extends EventTarget. - self.performance.addEventListener is not a function
```

`tests/wpt/hr-time/performance-tojson.html`:

```
FAIL Test performance.toJSON() - assert_equals: expected "function" but got "undefined"
```

Тот же `TypeError: performance.addEventListener is not a function` —
14 хитов в `buffer-full-*.html` категории `resource-timing`
(WPT-VENDOR-resource-timing, 2026-08-05).

## Причина

`performance` — обычный объектный литерал `var performance = { timeOrigin,
now, mark, measure, getEntriesByName, getEntriesByType, getEntries,
clearMarks, clearMeasures, clearResourceTimings,
setResourceTimingBufferSize }`. Спека (W3C HR Time L3 §4,
https://w3c.github.io/hr-time/#the-performance-interface) требует:

```webidl
[Exposed=(Window,Worker)]
interface Performance : EventTarget {
  readonly attribute DOMHighResTimeStamp timeOrigin;
  [Default] object toJSON();
};
```

Синглтон — это всё ещё *экземпляр* интерфейса, а собранный присваиваниями
литерал прототипной цепочки не имеет вовсе: ни `addEventListener`
(реально используется контентом —
`performance.addEventListener('resourcetimingbufferfull', …)`, Resource
Timing L2 §4.4), ни `instanceof Performance`, ни самого интерфейсного
объекта `window.Performance`. Тот же класс дефекта, что
[BUG-367](BUG-367-FIXED.md) / [BUG-386](BUG-386-FIXED.md) /
[BUG-394](BUG-394-FIXED.md) / [BUG-664](BUG-664-OPEN.md) /
[BUG-668](BUG-668-OPEN.md) — «WebIDL-форма объекта собрана присваиваниями
вместо интерфейса».

## Исправление (P3, 2026-08-11)

`Performance` заведён как настоящий интерфейс: конструктор, бросающий
`TypeError('Illegal constructor')` (в IDL конструктора нет — тот же приём,
что у `Node`/`Element`/`Attr` в этом же шиме),
`Performance.prototype = Object.create(EventTarget.prototype)`, все
операции перенесены с литерала на прототип, экземпляр создаётся
`Object.create(Performance.prototype)` + `EventTarget.call(performance)`.
Интерфейсный объект выставлен как `window.Performance` рядом с
`window.PerformanceObserver`.

`addEventListener`/`removeEventListener`/`dispatchEvent` пришли честно —
это `EventTarget.prototype`-методы существующего шимового `EventTarget`
(`dom.rs:413`), а не заглушки; `_listeners` заводит
`EventTarget.call(…)` через `defineProperty` (неперечислимое поле), так
что состояние слушателей в перечисление экземпляра не попадает.

`timeOrigin` стал getter-only аксессором на прототипе — readonly-атрибут
WebIDL, класс [BUG-366](BUG-366-FIXED.md): страница не должна отвечать за
движок простым присваиванием. Раньше это было записываемое собственное
поле.

**Перенос операций на прототип — не косметика, а то, что делает
`toJSON()` осмысленным.** Дефолтная WebIDL-операция `toJSON` сериализует
*атрибуты*, а не операции; пока `now`/`mark`/… были собственными
перечислимыми свойствами литерала, «копия собственных перечислимых
свойств» как реализация `toJSON` тащила бы их внутрь результата. После
переноса у экземпляра собственных перечислимых свойств нет вовсе, и
`toJSON()` возвращает ровно `{ timeOrigin }` — единственный атрибут,
который Lumen реализует.

### Чего фикс намеренно не делает

* **`performance.timing` / `performance.navigation`** (легаси-partial
  Navigation Timing L2) не добавлены — заведён
  [BUG-767](BUG-767-OPEN.md). Названный заявкой
  `performance-tojson.html` проверяет не только `performance.toJSON`, но
  и `json.timing`/`json.navigation` с 21 миллисекундной вехой
  (`navigationStart`…`loadEventEnd`) и `type`/`redirectCount`, поэтому
  **тест целиком после этого фикса ещё не зелёный** — зеленеет ровно
  первое утверждение из симптома (`typeof performance.toJSON`), дальше
  падает на `typeof(timing.toJSON)`. Данных под эти вехи в движке нет ни
  одной: shell отдаёт навигационную запись как
  `_lumen_deliver_perf_entry('navigation', url, 0.0, duration_ms, null)`
  (`crates/shell/src/main.rs::deliver_nav_timing`) — только URL и общая
  длительность; на том же безданном источнике уже висит
  [BUG-640](BUG-640-OPEN.md) (современная L2-запись
  `PerformanceNavigationTiming` — такой же голый стаб), поэтому BUG-767
  им и заблокирован. Подставить 21 ноль означало бы позеленить тест,
  оставив фичу сломанной, поэтому интерфейсы отсутствуют, а не
  подделаны.
* **Диспатч `resourcetimingbufferfull`** (Resource Timing L2 §4.4) —
  как и предусматривал п. 3 заявки. Подписка теперь возможна, событие
  по-прежнему никто не шлёт; `buffer-full-*.html` (`resource-timing`)
  перестают падать на `TypeError`, но зелёными не станут.

## Проверка

6 новых юнит-тестов в `v8_perf_observers` (`cargo test -p lumen-js
--features v8-backend`, 2851 + 70 зелёных):

* `performance_extends_event_target` — дословный перенос подтеста из
  `basic.any.js` (`addEventListener` с `{once: true}` + `dispatchEvent`);
* `performance_prototype_chain_reaches_event_target` — цепочка, а не
  наличие трёх имён: литерал с *копиями* методов прошёл бы предыдущий
  тест и провалил бы любой `instanceof`;
* `performance_constructor_is_illegal` — `new Performance()` бросает
  `TypeError`;
* `performance_to_json_reports_time_origin` — `toJSON` есть, возвращает
  объект, `json.timeOrigin === performance.timeOrigin`, и
  `JSON.stringify(performance)` идёт через него;
* `performance_to_json_carries_attributes_only` — ключи результата ровно
  `timeOrigin` (операции не протекают);
* `performance_time_origin_is_readonly`.

Тесты бьют по реальному `install_dom` (фикстура `v8_runtime_with_dom`),
то есть по тому же пути шима и той же завершающей пломбировке
(`seal_internal_globals_v8`), что и живой браузер; `performance` ничем не
заглушается. Отдельная живая проба не потребовалась — в отличие от
[BUG-395](BUG-395-FIXED.md) / [BUG-397](BUG-397-FIXED.md) /
[BUG-399](BUG-399-FIXED.md), где собственные тесты модуля подменяли
`navigator`/HTTP-профиль/URL страницы. Пломбировка `Performance`/
`performance` не касается: её регексп (`internal_globals.rs`) ловит
только имена вида `__*` / `_*lumen*`.

## Связанные

* [BUG-767](BUG-767-OPEN.md) — `performance.timing`/`performance.navigation`
  (Navigation Timing L1/L2 legacy) отсутствуют; заведён этим фиксом.
* [BUG-401](BUG-401-FIXED.md) — `performance` отсутствует в Worker global
  scope целиком: тот же API, другой файл (`worker.rs`). Теперь у него
  появился готовый образец — прототип `Performance` можно поднять в
  воркер-глобал целиком, а не переписывать литерал второй раз.
* [BUG-696](BUG-696-OPEN.md) — `mark()`/`measure()` не валидируют
  аргументы (User Timing L3 §3.1/§3.3). Соседний дефект тех же методов,
  которых этот фикс коснулся только переносом на прототип; поведение
  сохранено дословно.
* `PerformanceObserver` (соседний класс той же секции) — по спеке
  `EventTarget` не требует, не путать.
* Тот же класс «не-`EventTarget`»: [BUG-664](BUG-664-OPEN.md)
  (`navigator.connection`), [BUG-668](BUG-668-OPEN.md)
  (`screen.orientation`).
