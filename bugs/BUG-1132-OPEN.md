# BUG-1132 — Сериализация `innerHTML`/`outerHTML` экранирует текст внутри `<script>`/`<style>`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/v8_runtime/dom_helpers.rs::serialize_node` — `NodeData::Text` всегда через `escape_html_text`, без проверки родителя)

## Симптом

bing хранит тела инлайн-скриптов в `<script type=text/rms>` и переносит их в исполняемые через
`innerHTML`. Lumen отдаёт экранированный текст (скрипт #21: `//&lt;![CDATA[ … &amp;&amp; …`) и
исполняет его → `Uncaught SyntaxError: Unexpected token ;`, дальше страница пишет текст ошибки в
`<body>`. `ReferenceError: _w` на bing есть и в Chrome — это не причина.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g2/scriptinner.html`:

```html
<!doctype html><html><head><title>script innerHTML</title></head><body>
<div id="defer"><script type="text/rms">//<![CDATA[
var a = 1 && 2; if (a < 3) window.__ran = 'yes';
//]]></script></div>
<script>
var r = {}; window.__r = r;
var s = document.querySelector('#defer script');
r.innerHTML = s.innerHTML;
r.text = s.text;
r.textContent = s.textContent;
r.innerText = s.innerText;
r.divInner = document.getElementById('defer').innerHTML.slice(0, 120);
var st = document.createElement('style'); st.textContent = 'a>b{x:"&<"}'; r.styleInner = st.innerHTML;
// re-run the way deferred loaders do
var n = document.createElement('script'); n.text = s.innerHTML;
try { document.body.appendChild(n); } catch (e) { r.appendErr = String(e); }
r.ran = window.__ran || 'no';
</script></body></html>
```

**Результат:** Lumen: `script.innerHTML = '//&lt;![CDATA[ var a = 1 &amp;&amp; 2; if (a &lt; 3)...'`, пересозданный скрипт — `SyntaxError: Unexpected token &`, `ran='no'`. Chrome: сырой текст, `ran='yes'`.

## Что сделать

HTML LS §13.3 «Serializing HTML fragments»: текстовый узел, родитель которого `style`,
`script`, `xmp`, `iframe`, `noembed`, `noframes`, `plaintext` (и `noscript` при включённом
скриптинге), выводится буквально, без экранирования. Проверять родителя в `serialize_node`.
Критерий: репро даёт `ran='yes'`.
