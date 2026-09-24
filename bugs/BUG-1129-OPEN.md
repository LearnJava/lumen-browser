# BUG-1129 — `load` окна не ждёт динамически вставленный `<script src>`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:11478` `_lumen_apply_ready_state('complete')` не учитывает ожидающие `_lumen_script_load_external`)

## Симптом

wordpress вызывает `wpcom.add_scripts(['https://accounts.google.com/gsi/client'])`, затем
`addEventListener('load', …google.accounts…)`. В Lumen `load` окна срабатывает раньше, чем
вставленный скрипт вообще запрошен — в логе `GET https://accounts.google.com/gsi/client` идёт после
`Uncaught TypeError: Cannot read properties of undefined (reading 'accounts')`.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/loaddelay.html`:

```html
<!doctype html><html><head><meta charset="utf-8"></head><body>
<script>
window.ORDER=[];
var s=document.createElement('script');s.src='m1.js';
s.onload=function(){ORDER.push('script-load:'+(window.__last||'?'));};
document.head.appendChild(s);
window.addEventListener('load',function(){ORDER.push('window-load exec='+JSON.stringify(window.__exec||[]));});
</script></body></html>
```

**Результат:** Lumen: `['window-load exec=[]','script-load:m1']`. Chrome: `['script-load:m1','window-load exec=["m1"]']`.

## Что сделать

HTML LS §4.12.1.1 «prepare the script element»: вставленный до окончания загрузки внешний
скрипт задерживает load event документа (delay the load event) до своего `load`/`error`.
`complete`/`load` окна ждать, пока не опустеет очередь таких скриптов. Критерий: репро даёт
порядок Chrome.
