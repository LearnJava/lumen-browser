# BUG-1142 — `navigator.javaEnabled` отсутствует

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — объект `navigator`, члена нет; `grep javaEnabled crates/js/src` — 0)

## Симптом

apple: `Uncaught TypeError: navigator.javaEnabled is not a function at t.Wb … at t.t.t.track …
at t.exports.submit` (Adobe Analytics). Падает только отправка трекинга; узлов в Lumen даже больше,
чем в Chrome (2242 против 1944).

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/api.html`:

```html
<!doctype html>
<html><head><meta charset="utf-8"><title>g5 api repro</title></head>
<body>
<div id="d" class="a b c">x</div>
<img id="svgc" src="comment.svg" width="18" height="18">
<img id="svgp" src="plain.svg" width="18" height="18">
<script>
function t(f){try{return f();}catch(e){return 'THROW '+(e&&e.name)+': '+(e&&e.message);}}
var d=document.getElementById('d');
window.R={
 atob_unpadded2: t(function(){return atob('YQ');}),
 atob_unpadded3: t(function(){return atob('YWI');}),
 atob_padded: t(function(){return atob('YQ==');}),
 atob_len1mod4: t(function(){return atob('YWJjZ');}),
 atob_eq_middle: t(function(){return atob('YQ==YQ==');}),
 cl_values: t(function(){return typeof d.classList.values;}),
 cl_entries: t(function(){return typeof d.classList.entries;}),
 cl_keys: t(function(){return typeof d.classList.keys;}),
 cl_iter: t(function(){return typeof d.classList[Symbol.iterator];}),
 cl_spread: t(function(){return [].concat(Array.from(d.classList)).join(',');}),
 cl_values_call: t(function(){return Array.from(d.classList.values()).join(',');}),
 javaEnabled: t(function(){return typeof navigator.javaEnabled;}),
 javaEnabled_call: t(function(){return navigator.javaEnabled();}),
 getBattery: t(function(){return typeof navigator.getBattery;}),
};
</script>
</body></html>
```

**Результат:** Lumen: `javaEnabled = undefined / THROW`. Chrome: `function / false`.

## Что сделать

HTML LS §8.9.1.6 `NavigatorPlugins`: `javaEnabled()` всегда возвращает `false`. Добавить на
`navigator` (при появлении `Navigator.prototype` из BUG-624 — туда). Критерий: репро даёт `false`.
