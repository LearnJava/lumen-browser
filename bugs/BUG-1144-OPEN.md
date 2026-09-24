# BUG-1144 — `innerText` у элемента, который не рендерится (`display:none`, `<script>`), возвращает `''` вместо `textContent`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:~6665` геттер `innerText` / `_lumen_rendered_text` (BUG-413 срез 2) — нет шага «not being rendered → textContent»)

## Симптом

Найдено при разборе группы 2 (tiktok…quora); конкретный сайт, который ломается именно на этом,
не установлен. Геттер трактует отсутствие записи layout у элемента как прозрачную обёртку и
спускается в текстовые узлы, у которых записи тоже нет, — получается пусто.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g2/innertext.html`:

```html
<!doctype html><html><head><title>innerText hidden</title>
<script type="application/ld+json" id="ld">{"a":1}</script>
</head><body>
<div id="dn" style="display:none">hidden <b>text</b></div>
<script id="js">var x = 1;</script>
<template id="tpl"><p>t</p></template>
<script>
var r = {}; window.__r = r;
function it(id){ var e = document.getElementById(id); return JSON.stringify(e.innerText); }
r.ld = it('ld');
r.dn = it('dn');
r.js = it('js');
try { r.parsed = JSON.parse(document.getElementById('ld').innerText); } catch (e) { r.parseErr = String(e); }
var det = document.createElement('div'); det.textContent = 'detached'; r.detached = JSON.stringify(det.innerText);
setTimeout(function(){ r.ldLater = it('ld'); r.dnLater = it('dn'); }, 300);
</script></body></html>
```

**Результат:** Lumen: `dn=''`, `js=''` (и через 300 мс так же). Chrome: `dn='hidden text'`, `js='var x = 1;'`.

## Что сделать

HTML LS §3.2.7 «The innerText and outerText properties», шаг 1: если элемент не being
rendered или user agent не поддерживает рендеринг — вернуть descendant text content (`textContent`).
Критерий: репро даёт результат Chrome.
