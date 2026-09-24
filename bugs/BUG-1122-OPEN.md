# BUG-1122 — Члены `Node`/`Element` живут на скрытом прототипе обёртки, а не на `Element.prototype`/`Node.prototype`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:8725` `_lumen_wrapper_proto_for` — `_LUMEN_WRAPPER_MEMBERS`/`_LUMEN_WRAPPER_DESCRIPTORS` ставятся на прототип между экземпляром и `HTMLxxxElement.prototype`)

## Симптом

Библиотеки берут «нативные» методы с прототипов интерфейсов, а не с экземпляров:

- **youtube** (`webcomponents-sd.js`, ShadyDOM): признак `!(Element.prototype.attachShadow &&
  Node.prototype.getRootNode)` → в Lumen истина → включается полный полифилл. Его
  `E(Element.prototype,[…])` сохраняет методы через `getOwnPropertyDescriptor` и не находит их;
  дальше падения на CDATASection (BUG-863) и `EventTarget.prototype.addEventListener.call` (отдельный
  баг), DOM пропатчен наполовину, у custom elements нет `hasAttribute`/`querySelector`
  (`CE upgrade constructor: Constructing iron-iconset-svg: TypeError: this.hasAttribute is not a
  function`). 428 узлов против 1868.
- **DOMPurify 3.x** `lookupGetter` обходит цепочку от `Element.prototype` и для
  `cloneNode`/`remove`/`childNodes`/`parentNode`/`insertBefore`/`firstElementChild`/`textContent`/
  `nodeType`/`nodeName` получает fallback `() => null` → `Cannot set properties of null (setting
  '__removalCount')`. Видели на udemy в прогоне с блокировщиком; без него на сайте не воспроизвелось,
  репро стабильно.

После BUG-849 общие члены живут на прототипе `_lumen_wrapper_proto_for(iface)`, который стоит
**ниже** интерфейсного. BUG-1101 (FIXED) перенёс на `Node.prototype` только
`firstChild`/`nextSibling` — этот баг обобщает тот же дефект на остальные члены.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g1/iface_proto.html`:

```html
<!doctype html><html><body><div id=d></div><script>
// webcomponents-sd.js (YouTube): w.Sb = !(!Element.prototype.attachShadow || !Node.prototype.getRootNode); inUse = !w.Sb
// → если нативного Shadow DOM «нет» на прототипах интерфейсов, ShadyDOM включается и патчит DOM.
var el=document.getElementById('d'), r={};
r.EP_attachShadow=typeof Element.prototype.attachShadow;
r.NP_getRootNode=typeof Node.prototype.getRootNode;
r.el_attachShadow=typeof el.attachShadow;
r.el_getRootNode=typeof el.getRootNode;
r.shadydom_would_activate=!(Element.prototype.attachShadow&&Node.prototype.getRootNode);
var names=["hasAttribute","getAttribute","setAttribute","querySelector","querySelectorAll","appendChild","insertBefore","removeChild","attachShadow","getRootNode","closest","matches","append","remove","children","parentNode","textContent","innerHTML","addEventListener"];
r.missing_on_interface_protos=names.filter(function(k){return !(k in Element.prototype)&&!(k in Node.prototype)&&!(k in EventTarget.prototype);});
r.on_instance_chain=names.filter(function(k){return k in el;}).length+'/'+names.length;
console.log("RESULT "+JSON.stringify(r)); window.__r=r;
</script></body></html>
```

**Результат:** Lumen: `EP_attachShadow=undefined`, `NP_getRootNode=undefined`, `shadydom_would_activate=true`, `missing_on_interface_protos=[hasAttribute,getAttribute,setAttribute,querySelector,querySelectorAll,appendChild,insertBefore,removeChild,attachShadow,getRootNode,closest,matches,append,remove,children,parentNode,textContent,innerHTML]`. Chrome: всё `function`, `shadydom_would_activate=false`, `missing=[]`. Вариант с DOMPurify — `.tmp/compat/g3/dompurify_lookup.html`: Lumen 9 из 10 членов → `FALLBACK_NULL`, Chrome находит все.

## Что сделать

WebIDL §3.7: операции и атрибуты интерфейса — собственные свойства его interface prototype
object (`Node.prototype`, `Element.prototype`, `ParentNode`-миксин — на `Element.prototype`/
`Document.prototype`/`DocumentFragment.prototype`). Перенести члены из скрытого прототипа на
прототипы интерфейсов (атрибуты — accessor-дескрипторы с `get`/`set`), так чтобы
`Object.getOwnPropertyDescriptor(Element.prototype,'hasAttribute').value.call(el,'x')` работал.
Критерий: оба репро дают результат Chrome; youtube перемерить (зависит ещё от BUG-863 и бага
про `EventTarget`).
