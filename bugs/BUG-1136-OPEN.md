# BUG-1136 — `Element.prototype.getAttributeNames` отсутствует

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — у `Element.prototype` метода нет; есть только у `VElement` в `crates/js/src/dom_parser.rs:229`, а сам шим проверяет его наличие в `:10801`)

## Симптом

samsung.com: `typeof Element.prototype.getAttributeNames === 'undefined'`, вызов даёт
`TypeError: d.getAttributeNames is not a function`. Второй дефект той же страницы —
`iframe.contentWindow === null` (BUG-480) с каскадом `Unexpected end of JSON input` и `reading '$q'`;
`<body>` пустой (samsung/de: 67 узлов против 3865). Упоминание метода есть только в BUG-1107-FIXED.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g2/attrnames.html`:

```html
<!doctype html><html><head><title>getAttributeNames</title></head><body>
<div id="d" class="a" data-x="1" aria-label="z">x</div>
<pre id="out"></pre>
<script>
var d = document.getElementById('d'), r = {};
r.typeofEl = typeof d.getAttributeNames;
r.inProto = 'getAttributeNames' in Element.prototype;
try { r.names = d.getAttributeNames(); } catch (e) { r.namesErr = String(e); }
r.hasAttributes = d.hasAttributes();
r.attrsLen = d.attributes.length;
window.__r = r;
document.getElementById('out').textContent = JSON.stringify(r);
</script></body></html>
```

**Результат:** Lumen: `typeofEl='undefined'`, `inProto=false`, `namesErr='TypeError: d.getAttributeNames is not a function'`, `hasAttributes=true`, `attrsLen=4`. Chrome: `function`, `['id','class','data-x','aria-label']`.

## Что сделать

DOM §4.9: `getAttributeNames()` — квалифицированные имена атрибутов в порядке списка
атрибутов; на `Element.prototype`, по образцу `hasAttributes` (`_lumen_get_attr_names(nid)`).
Критерий: репро даёт результат Chrome.
