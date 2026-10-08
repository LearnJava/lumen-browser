# BUG-1129 — `load` окна не ждёт динамически вставленный `<script src>`

**Статус:** FIXED 2026-09-26 (P6)
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

## Исправление (2026-09-26, P6)

Корень подтверждён: шелл зовёт `_lumen_apply_ready_state('complete')` (`notify_window_loaded`)
безусловно, а очередь вставленных скриптов (`_lumen_script_exec_queue`) никак не учитывалась.

Теперь `_lumen_script_load_external` (`crates/js/src/shim/web_api_shim_mid.js`) помечает скрипт,
вставленный до `complete`, как задерживающий load (`_lumen_load_delay_count`), и снимает пометку
после его `load`/`error` (для модульного — после промиса `import()`). `_lumen_apply_ready_state('complete')`
(`web_api_shim_tail_b.js`) при ненулевом счётчике только ставит `_lumen_load_deferred`;
последний завершившийся скрипт отдельной задачей (`setTimeout 0`, чтобы прошли микрозадачи его
обработчика `load`) переводит `readyState` в `complete` и шлёт `load` окна. Скрипт, вставленный
обработчиком `load` другого скрипта до этого момента, тоже задерживает load — как в Chrome.
`pageshow`, который шелл шлёт сразу за `load`, откладывается вместе с ним
(`_lumen_fire_page_lifecycle`, `web_api_shim_mid_b.js`), порядок `load → pageshow` сохраняется.

Бесконечно висящий запрос вставленного скрипта держит `load` бесконечно — как в Chrome.
Headless-режимы `complete` не шлют вовсе, их правка не касается.

Тест: `dom::tests::v8_whatwg_streams::inserted_script_delays_window_load` — репро на `blob:`-скрипте
плюс вызовы шелла `interactive → complete → pageshow`: ожидается
`["shell-done interactive","script-load","window-load complete exec=[\"m1\"]","pageshow"]`.

Живая проверка (окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, `m1.js` отдаётся с задержкой 800 мс,
репро плюс слушатель `pageshow`): `["script-load:m1","window-load exec=[\"m1\"]","pageshow"]` —
порядок Chrome. Живой wordpress не перепроверялся.
