# BUG-1165 — `MutationObserver` не видит изменений в документе из `DOMParser`

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/dom_parser.rs`: `DOMParser.parseFromString` строит отдельный
JS-DOM (`VDocument`/`VElement`/`VText`), у узлов нет `__nid__`; очередь `MutationObserver`
(`_mo_notify`, `crates/js/src/shim/web_api_shim_mid.js`) работает по номерам узлов арены

## Симптом

`dom/nodes/MutationObserver-textContent.html` (`run_smoke.py`, 2026-09-25, после BUG-863) —
TIMEOUT 3/4. Четвёртый сабтест:

```js
let xml = new DOMParser().parseFromString("<root></root>", "text/xml");
el = xml.createElement("somelement");
el.appendChild(xml.createCDATASection("foo"));
m = new MutationObserver(records => …);
m.observe(el, { childList: true });
el.textContent = "foo";           // колбэк не вызывается никогда
```

До BUG-863 сабтест падал раньше, на `createCDATASection`. Теперь фабрика есть, а сеттер
`VElement.textContent` меняет `childNodes` JS-массива и никому не сообщает.

## Что чинить

Либо уведомления из мутаций `VNode` в ту же очередь `MutationObserver`, либо (правильнее) строить
документ `DOMParser` в арене, как `createDocument`.
