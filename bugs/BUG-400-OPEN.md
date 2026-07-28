# BUG-400 — `performance` не реализует интерфейс `Performance` целиком: не наследует `EventTarget`, нет `toJSON()`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10987-11048` — `var performance = {...}`, объектный литерал)
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

## Причина

`performance` (`dom.rs:10987`) — обычный объектный литерал `var performance
= { timeOrigin, now, mark, measure, getEntriesByName, getEntriesByType,
getEntries, clearMarks, clearMeasures, clearResourceTimings,
setResourceTimingBufferSize }`. Спека (W3C HR Time L3 §4,
https://w3c.github.io/hr-time/#the-performance-interface) требует:

```webidl
[Exposed=(Window,Worker)]
interface Performance : EventTarget {
  ...
  [Default] object toJSON();
};
```

— `Performance` обязан наследовать `EventTarget` (реально используется
контентом: `performance.addEventListener('resourcetimingbufferfull', …)`
из Resource Timing L2 §4.4) и иметь метод `toJSON()`, сериализующий
собственные перечислимые свойства (обычно используется `JSON.stringify`
на телеметрии перфоманса).

Оба отсутствуют, потому что `performance` никогда не был построен через
`EventTarget`-конструктор/прототип — в отличие от `PerformanceObserver`
(`dom.rs:11061`, тоже не EventTarget, но спекой и не требуется) объект
`performance` — синглтон, а не класс, что и упростили до плоского
литерала.

## Что нужно сделать

1. Сделать `performance` экземпляром (или прототипно связанным с)
   `EventTarget` — переиспользовать существующий глобальный
   `EventTarget`/`Event` из `WEB_API_SHIM` (см. `dom.rs:27527`,
   комментарий про `BUG-067/070`), чтобы `addEventListener`/
   `removeEventListener`/`dispatchEvent` появились честно, не
   заглушками.
2. Добавить `performance.toJSON()` — типовая реализация: копия
   перечислимых собственных свойств (`timeOrigin`) в новый объект
   (методы toJSON не включает).
3. `resourcetimingbufferfull`-событие (Resource Timing L2 §4.4) можно
   не реализовывать в этом фиксе — сам факт наличия
   `addEventListener`/`dispatchEvent` уже закрывает подтест; диспатч
   реального события — отдельный объём работы, если понадобится.

## Связанные

* `PerformanceObserver` (`dom.rs:11061`) — соседний класс той же секции,
  не требует EventTarget по спеке, не путать.
* Найдено вместе с [BUG-401](bugs/BUG-401-OPEN.md) (`performance`
  отсутствует в Worker global scope целиком) — тот же API, разные
  root cause и файлы (`dom.rs` vs `worker.rs`), поэтому заведены
  отдельно.
