# BUG-1172 — `window` как EventTarget: нет `handleEvent`, `once`, `signal`, дедупликации и `event.target`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid_b4.js` — объект-литерал `window`:
`addEventListener` `:1887`, `removeEventListener` `:1934`, `dispatchEvent` `:1950`)
**Найден:** 2026-09-25, P3, при починке [BUG-643](BUG-643-FIXED.md)

## Симптом

Проба в `V8JsRuntime` после `install_dom` (тест-модуль `device_sensors.rs`):

```js
var o2 = 0, t2 = null;
window.addEventListener('zz', { handleEvent() { o2++; } });
window.addEventListener('zz', function (e) { t2 = e.target; });
window.dispatchEvent(new Event('zz'));
// o2 === 0, t2 === null            (Chrome: 1, window)

var n1 = 0, n2 = 0;
function f1() { n1++; }
window.addEventListener('yy', f1);
window.addEventListener('yy', f1);
window.addEventListener('yy', function () { n2++; }, { once: true });
window.dispatchEvent(new Event('yy'));
window.dispatchEvent(new Event('yy'));
// n1 === 4, n2 === 2               (Chrome: 2, 1)
```

## Причина

`window` — не `EventTarget`, а объект-литерал со своими массивами слушателей по
типам (`_other_win_listeners[type]`, `_load_listeners`, …):

* `addEventListener` выходит сразу при `typeof fn !== 'function'` — слушатель-объект
  с `handleEvent` теряется молча;
* повторная регистрация того же `(type, fn, capture)` не отсекается;
* из опций читается только `capture` (`_lumen_capture_flag`), `once`/`passive`/`signal`
  игнорируются;
* `dispatchEvent` вызывает `fn.call(window, evt)`, но не ставит ни `event.target`,
  ни `event.currentTarget` (только путь всплытия от узла в `_lumen_invoke_at_window`,
  `web_api_shim_mid.js:929`, ставит `currentTarget`).

## Масштаб

Любая страница, подписывающаяся на `window` объектом (`handleEvent` — частый приём
в библиотеках), через `{ once: true }` или `AbortSignal`, либо читающая
`event.target` у событий, отправленных `window.dispatchEvent` (сюда же попадают
синтетические события шимов: `deviceorientation`, `gamepadconnected`, `unload` …).
Родственный, но другой дефект — [BUG-1123](BUG-1123-OPEN.md) (`window` не наследует
`EventTarget.prototype`); общий фикс — перевести `window` на общую реализацию
`EventTarget` — закрыл бы оба.
