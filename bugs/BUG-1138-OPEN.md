# BUG-1138 — `Object.prototype.toString.call(document)` даёт `'[object Object]'`: нет `HTMLDocument` и `Symbol.toStringTag`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:11694` `Object.setPrototypeOf(document, Document.prototype)` — нет `HTMLDocument.prototype` и тега; `:3553` `Document.prototype`)

## Симптом

yahoo.co.jp `ual-2.10.2`: `w = /^(HTML)?Document$/.test(Object.prototype.toString.call(document).slice(8,-1))`
→ в Lumen `'Object'` → 8× `[util/offset.getOffset] windowまたはdocumentオブジェクトが存在しません`.
Сегодня сайт отвечает 403 (антибот, curl получает то же), ошибки сняты в прогоне 2026-09-23.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g4/repro-doc.html`:

```html
<!doctype html><html><head><meta charset="utf-8"><title>repro-doc-tags</title></head>
<body style="background:#eef"><h1>toStringTag / document.referrer / prototype chains</h1><pre id="o"></pre>
<script>
// yahoo.co.jp ual-2.10.2: w = /^(HTML)?Document$/.test(getType(document)); ds-custom-logger: document.referrer.indexOf(...)
// whatsapp hyperionDOM: new ShadowPrototype(Node, EventTarget-shadow) asserts Node.prototype chain reaches EventTarget.prototype
function tag(v) { try { return Object.prototype.toString.call(v); } catch (e) { return 'THROW ' + e; } }
function chain(p) { var a = []; while (p && a.length < 8) { a.push(p.constructor && p.constructor.name); p = Object.getPrototypeOf(p); } return a; }
var r = {
  tagWindow: tag(window), tagDocument: tag(document), tagHistory: tag(history), tagLocation: tag(location), tagNavigator: tag(navigator),
  referrerType: typeof document.referrer, referrerInDoc: 'referrer' in document,
  nodeChain: chain(Node.prototype), xhrChain: chain(XMLHttpRequest.prototype),
  divIsEventTarget: document.createElement('div') instanceof EventTarget,
  xhrIsEventTarget: new XMLHttpRequest() instanceof EventTarget,
  typeofHistory: typeof History
};
window.__r = r;
document.getElementById('o').textContent = JSON.stringify(r, null, 1);
</script></body></html>
```

**Результат:** Lumen: `tagDocument='[object Object]'`, `typeof HTMLDocument='undefined'`. Chrome: `'[object HTMLDocument]'`, `function`.

## Что сделать

HTML LS §3.1.1: `document` HTML-страницы — экземпляр `HTMLDocument : Document`
(`HTMLDocument.prototype.__proto__ === Document.prototype`); `Symbol.toStringTag` по WebIDL §3.7.6.
Критерий: репро даёт `'[object HTMLDocument]'`.
