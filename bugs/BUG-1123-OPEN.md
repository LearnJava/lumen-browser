# BUG-1123 — `EventTarget` не в цепочке прототипов узлов, `window` и XHR: `EventTarget.prototype.addEventListener.call(node)` падает

**Статус:** OPEN
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
