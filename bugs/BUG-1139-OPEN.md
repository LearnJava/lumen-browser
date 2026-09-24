# BUG-1139 — `MessageEvent` от `window.postMessage`: `target`/`currentTarget` равны `null`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b3.js:2292-2302` `window.postMessage` — прямой вызов `_message_listeners[i](ev)` без dispatch; то же в `_lumen_deliver_frame_message`, `:2325-2334`)

## Симптом

webmd `app-core.8c32828e.js:42:10429`: `window.addEventListener("message", t => … t.target.location.origin …)`
→ `Uncaught TypeError: Cannot read properties of null (reading 'location')`. Событие не проходит через
`dispatchEvent`: `onmessage` и слушатели вызываются напрямую с `new MessageEvent(...)`, поэтому
`target`/`currentTarget`/`eventPhase` не выставлены.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g1/msg_target.html`:

```html
<!doctype html><html><body><script>
// WebMD app-core.js: window.addEventListener("message", t => { ... t.target.location.origin ... })
window.__r={};
window.addEventListener('message',function(t){
  window.__r={target_is_null:t.target===null, target_is_window:t.target===window, currentTarget_is_window:t.currentTarget===window,
              origin:t.origin, data:t.data, source_is_window:t.source===window,
              loc_origin:(function(){try{return t.target.location.origin}catch(e){return String(e)}})()};
});
window.postMessage({message:'x'},'*');
</script></body></html>
```

**Результат:** Lumen: `target_is_null=true`, `target_is_window=false`, `currentTarget_is_window=false`, `loc_origin="TypeError: Cannot read properties of null (reading 'location')"`. Chrome: `target_is_window=true`, `currentTarget_is_window=true`, `loc_origin=http://127.0.0.1:8761`.

## Что сделать

HTML LS §9.3.3 «window post message steps»: задача, которая **fire an event** `message` в
целевом `Window` — то есть через обычный dispatch (`target`, `currentTarget`, фазы, `stopPropagation`,
`once`). Доставлять через `dispatchEvent` на `window` в обоих местах. Критерий: репро даёт результат
Chrome.
