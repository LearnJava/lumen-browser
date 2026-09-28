# BUG-1123 — `EventTarget` не в цепочке прототипов узлов, `window` и XHR: `EventTarget.prototype.addEventListener.call(node)` падает

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/event_target_shim.js:21-25` — метод работает только с `this._listeners`; `web_api_shim_mid.js:3215` `function Node` — `Node.prototype` не наследует `EventTarget.prototype`; `crates/js/src/xhr.rs:59` `_XhrEventTarget` — своя база)

## Симптом

- **youtube**: ShadyDOM сохраняет `EventTarget.prototype.addEventListener` как
  `window.__shady_native_addEventListener` и вызывает его с `this = window` → `at
  EventTarget.addEventListener (<anonymous>:254:31) at Fb` → `reading 'focus'`. Метод на прототипе
  пишет в `this._listeners`, которого у `window`, `document` и DOM-обёрток нет; у них свой
  `addEventListener` (`window.addEventListener === EventTarget.prototype.addEventListener` — `false`).
- **whatsapp**: Meta hyperion `ShadowPrototype(Node, EventTarget-shadow)` идёт `getPrototypeOf`
  от `Node.prototype` до `EventTarget.prototype` и не доходит → `[JS error] Invalid prototype
  chain`; модуль телеметрии отключается, контент не страдает.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g1/et_call.html`:

```html
<!doctype html><html><body><div id=d></div><script>
// ShadyDOM (webcomponents-sd.js) сохраняет EventTarget.prototype.addEventListener как «нативный» и вызывает его с this = window/document/элемент
var r={}, f=function(){ r.fired=(r.fired||0)+1; };
[['window',window],['document',document],['element',document.getElementById('d')]].forEach(function(p){
  try{ EventTarget.prototype.addEventListener.call(p[1],'focus',f,true); r['add_'+p[0]]='ok'; }catch(e){ r['add_'+p[0]]=String(e); }
});
r.same_on_window = window.addEventListener===EventTarget.prototype.addEventListener;
r.same_on_document = document.addEventListener===EventTarget.prototype.addEventListener;
r.same_on_element = document.getElementById('d').addEventListener===EventTarget.prototype.addEventListener;
try{ EventTarget.prototype.addEventListener.call(document.getElementById('d'),'click',f); document.getElementById('d').click(); r.element_click_fired=r.fired||0; }catch(e){ r.element_click=String(e); }
console.log("RESULT "+JSON.stringify(r)); window.__r=r;
</script></body></html>
```

**Результат:** Lumen: `add_window/add_document/add_element = "TypeError: Cannot read properties of undefined (reading 'focus')"`, `same_on_*=false`; цепочки из `.tmp/compat/g4/repro-doc.html`: `nodeChain [Node,Object]`, `divIsEventTarget=false`, `xhrIsEventTarget=false`. Chrome: `add_*=ok`, `same_on_*=true`, клик доставлен; `[Node,EventTarget,Object]`, `true`, `true`, `XMLHttpRequestEventTarget` есть.

## Что сделать

DOM §2.7: `Node : EventTarget`, `Window : EventTarget`, XHR §3 `XMLHttpRequestEventTarget :
EventTarget`. Связать `Node.prototype`, прототип `window` и `XMLHttpRequest.prototype` с
`EventTarget.prototype`, а `EventTarget.prototype.addEventListener`/`removeEventListener`/
`dispatchEvent` сделать общими: для узла и `window` делегировать в `_lumen_add_listener`, для
прочих — в `_listeners`. Критерий: репро даёт результат Chrome, `window.addEventListener ===
EventTarget.prototype.addEventListener`.

## Прогресс

**2026-09-28, P6, в рамках [BUG-1167](BUG-1167-FIXED.md):** сделана первая половина —
`EventTarget.prototype.addEventListener`/`removeEventListener`/`dispatchEvent`, вызванные на
`window`, `document` или узле, делегируют в собственную реализацию цели (хук
`_lumen_et_platform_impl`: объявлен в `event_target_shim.js`, заполняется в конце
`web_api_shim_tail_b.js`). ShadyDOM youtube теперь инициализируется до конца
(`window.ShadowRoot` — его класс). Первые три строки репро дают `ok`.

**Осталось:** `Node.prototype` → `EventTarget.prototype` в цепочке (`div instanceof EventTarget`),
`XMLHttpRequestEventTarget`, идентичность `window.addEventListener ===
EventTarget.prototype.addEventListener`. На youtube это следующий блокер:
`Uncaught TypeError: a.__shady_native_dispatchEvent is not a function` — ShadyDOM копирует
дескрипторы `EventTarget.prototype` в `__shady_native_*` на нём же, а узлы до него не доходят.

Указатель этого бага потерялся из очередей после передачи P6 (`13dabf206`) при каком-то ремапе;
возвращён в начало `STATUS-P6.md` — он из той же партии, что и уже закрытые BUG-1119..1146, и
сейчас следующий блокер youtube.

## Исправление (2026-09-28, P6)

Вторая половина сделана, критерий выполнен.

- `Node.prototype` → `EventTarget.prototype`, интерфейсный объект `Node` → `EventTarget`
  (`crates/js/src/shim/web_api_shim_mid.js`, блок BUG-1123 после установки членов узла). Три метода
  `EventTarget` больше не ставятся ни на `Node.prototype`, ни на `ShadowRoot.prototype`: реализации
  узла и теневого корня лежат в таблицах `_lumen_node_et`/`_lumen_shadow_et`, хук
  `_lumen_et_platform_impl` выбирает таблицу по цели.
- `window` и `document` отдают собственные методы в `_lumen_window_et`/`_lumen_document_et` в самом конце
  `web_api_shim_tail_b.js` и удаляют их у себя: до этого момента шим сам зовёт их, пока цепочек ещё нет.
  Убрана перепривязка `var addEventListener = window.addEventListener.bind(window)` — объявление `var`
  делало свойство глобала неконфигурируемым. Голый вызов `addEventListener(…)` работает через цепочку
  глобального объекта.
- `device_sensors.rs` оборачивал `window.addEventListener` собственным свойством (идентичность ломалась
  снова) — теперь оборачивает `_lumen_window_et.addEventListener`.
- XHR: `XMLHttpRequestEventTarget : EventTarget` (`crates/js/src/xhr.rs`), свои методы слушателей удалены —
  `once`, объекты `handleEvent` и `on<type>` идут из `EventTarget.prototype`. Порядок: слушатели, затем
  `on<type>` (было наоборот).
- Запасной путь `EventTarget.prototype` создаёт собственное хранилище `_listeners` при первом обращении:
  JS-only узел (`new Text()`) и подкласс без `EventTarget.call(this)` больше не бросают.

Тесты: `crates/js/src/dom/tests/v8_bug1123_event_target_chain.rs` (7), утверждение про
`Node.prototype.addEventListener` в `v8_bug1122_iface_protos.rs` перенесено на `EventTarget.prototype`.

**youtube** (видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release):
`a.__shady_native_dispatchEvent is not a function` ушла; `document.createElement('div') instanceof
EventTarget`, `window.addEventListener === EventTarget.prototype.addEventListener` — `true`,
`ShadyDOM.inUse`. Контент по-прежнему пуст (`ytd-app` без `shadowRoot`). Оставшиеся ошибки заведены
отдельно: ``CE connectedCallback (upgrade): Error: md`InjectionToken(PAGE_TOKEN)`` —
[BUG-1207](BUG-1207-OPEN.md) (была и до исправления, так что не следствие BUG-1123);
`EvalError: Code generation from strings disallowed` во фрейме accounts.google.com —
[BUG-1206](BUG-1206-OPEN.md).
