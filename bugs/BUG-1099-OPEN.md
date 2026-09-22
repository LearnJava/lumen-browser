# BUG-1099 — `new iframe.contentWindow.<Ctor>()` для конструктора вне фиксированного набора фасада строит пустышку без реального конструктора

**Статус:** OPEN
**Тип:** дефект реализованного кода — `wrapWinFacadeGlobals`'s fallback-обёртка
(`crates/js/src/frame_bridge.rs::wrapWinFacadeGlobals`, ветка `r.kind ===
'function'`) вызывает удалённый конструктор обычным вызовом, а не `new`, и
Rust-мост под ней (`crates/js/src/v8_runtime/eval.rs::peer_global_call`)
делает то же самое (`v8::Function::call`, не `new_instance`)
**Область:** JS shim (`crates/js/src/frame_bridge.rs`, функция
`wrapWinFacadeGlobals`, ~строка 2093-2110) и native-мост
(`crates/js/src/v8_runtime/eval.rs:613-654::peer_global_call`,
`crates/js/src/frame_bridge_globals.rs:73-92`)
**Владелец:** P1/P3 (`lumen-js` — cross-realm bridge)
**Заведён:** 2026-09-23 (WPT-RUN-7 срез 58, категория `connection-allowlist`)

## Симптом

`connection-allowlist/tentative/service-worker-dedicated-worker.https.sub.window.js`
и `service-worker-shared-worker.https.sub.window.js` создают воркер не через
собственный `window.Worker`/`window.SharedWorker`, а через
`iframe.contentWindow.Worker(...)`/`iframe.contentWindow.SharedWorker(...)`
(BUG-979 wrapWinFacadeGlobals fallback — `Worker`/`SharedWorker` не входят в
фиксированный IDL-набор `winFacade`, поэтому чтение идёт через Proxy-фолбэк
на реальный `globalThis` пира). Оба теста падают в `cleanup`:

```
TypeError: worker.terminate is not a function
TypeError: Cannot read properties of undefined (reading 'close')   // SharedWorker: worker.port
```

т.е. `new iframe.contentWindow.Worker(url)` возвращает объект без `.terminate`
и без `._id`, `new iframe.contentWindow.SharedWorker(url)` — без `.port`.

## Прямое измерение

WPT-RUN-7 срез 58, `connection-allowlist` (`--check --all --root
connection-allowlist --recursive --processes 7 --binary
target/dev-release/lumen.exe --log-raw`), 4 независимых прогона подряд —
оба файла стабильно `ERROR` в `cleanup`, один и тот же стек каждый раз:

```
TypeError: worker.terminate is not a function
    at <anonymous>:19:12
    at Test.cleanup (<anonymous>:3249:9)
```

Прочитан источник теста
(`tests/wpt/connection-allowlist/tentative/service-worker-dedicated-worker.https.sub.window.js:19`):
`const worker = new iframe.contentWindow.Worker(dw_script);` — конструктор
берётся с чужого `contentWindow`, не с собственного `window`.

**Корень подтверждён по коду, не предположение.** `wrapWinFacadeGlobals`
(`frame_bridge.rs`) — Proxy вокруг `winFacade`; для свойства, отсутствующего
на самом фасаде (`Worker`/`SharedWorker` не входят в список BUG-979
docstring'а), `get`-ловушка зовёт `_lumen_f_global_get(bid, prop)`; когда
удалённое значение — функция (`r.kind === 'function'`), возвращается
generic-обёртка:

```js
return function() {
  var args = Array.prototype.slice.call(arguments);
  var cr = _lumen_f_global_call(bid, String(prop), args);
  if (cr.kind === 'error') { throw new Error(cr.message); }
  return cr.value;
};
```

Эта обёртка не участвует в протоколе `new`: у неё нет `.prototype`,
совпадающего с реальным `Worker.prototype`, и её тело безусловно делает
`return cr.value`. `_lumen_f_global_call` на Rust-стороне
(`v8_runtime/eval.rs::peer_global_call`, строка 641) зовёт удалённый
`Worker` через `func.call(tc, recv=v8::undefined, &v8_args)` — обычный вызов
функции, не `Function::new_instance`. Настоящий `Worker` в `worker.rs`
(строка ~1408) — обычная функция вида `function Worker(url, opts) { ...
this._id = ...; }`, ничего явно не `return`ит, так что `peer_global_call`
получает `undefined`, и generic-обёртка тоже `return`ит `undefined`.

По правилам ES `new WrapperFn(args)`, когда тело `WrapperFn` явно
возвращает **не-объект** (здесь — `undefined`), возвращаемое значение `new`
игнорируется, и используется исходно созданный `this` — пустой объект с
прототипом `WrapperFn.prototype` (обычный `Function.prototype`-объект, а не
реальный `Worker.prototype`). Отсюда и `worker.terminate is not a
function` (метода нет ни на прототипе, ни на инстансе — конструктор
`Worker` реального пира вообще не выполнился с `this`, указывающим на этот
объект: `this._id = ...` внутри настоящего `Worker` осело на `this` того
вызова, каким его видел Rust-мост — `recv = undefined`, т.е. эффект
потерян независимо от того, что произошло внутри).

## Масштаб

Не специфично для `Worker`/`SharedWorker` — падает **любой** конструктор
целевого фрейма, до которого добираются через этот фолбэк (любой глобальный
класс вне фиксированного IDL-набора `winFacade`, включая пользовательские
классы, объявленные скриптом заэмбеженной страницы). В этом срезе поймано
на двух файлах (`service-worker-dedicated-worker`,
`service-worker-shared-worker`), но класс шире WPT-категории
`connection-allowlist` — воздействует на любой тест, кросс-фреймово
создающий объект через `contentWindow.<Ctor>`.

## Направление починки (не предписание)

`peer_global_call` не умеет `new` в принципе — нужен отдельный
`peer_global_construct`, вызывающий `v8::Function::new_instance` в
контексте пира, и генерик-обёртка в `wrapWinFacadeGlobals` должна отличать
`new`-вызов от обычного (`new.target` внутри обёртки) и уходить в этот
новый мост, а не в `_lumen_f_global_call`. Осторожно: `new_instance`
работает в контексте пира — результат (реальный `Worker`-инстанс пира)
нужно сериализовать через тот же `envelope_value`/`from_v8`, что и сейчас
для значений, а не пытаться протащить живой V8-объект между изолятами.

## Как проверить фикс

`tests/wpt/.venv/Scripts/python.exe tests/wpt/run_smoke.py --binary
target/dev-release/lumen.exe
/connection-allowlist/tentative/service-worker-dedicated-worker.https.sub.window.html
/connection-allowlist/tentative/service-worker-shared-worker.https.sub.window.html`
— оба должны закрыть `cleanup` без исключений (`worker.terminate`/
`worker.port.close` должны существовать и отработать).
