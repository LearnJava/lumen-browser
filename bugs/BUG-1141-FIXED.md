# BUG-1141 — XHR: последнее `progress` приходит после `readystatechange(4)`

**Статус:** FIXED 2026-09-26 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/xhr.rs:395-398` `commitResponse`: `_setReadyState(4)` раньше `_fireProgress('progress')`)

## Симптом

airbnb: 1–4× `Uncaught TypeError: Cannot read properties of null (reading
'getAllResponseHeaders') at XMLHttpRequest._fireProgress at commitResponse` — обработчик
`readystatechange` на `readyState === 4` обнуляет ссылку на XHR, а следующее за ним в Lumen
`progress` её читает.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/xhr.html`:

```html
<!doctype html><html><body><script>
window.EV=[];
var x=new XMLHttpRequest();var ref=x;
['progress','load','loadend','loadstart'].forEach(function(n){x.addEventListener(n,function(){EV.push(n+'@rs'+x.readyState+(ref?'':'(ref=null)'));if(n==='progress'){try{ref.getAllResponseHeaders();}catch(e){EV.push('ERR '+e.message)}}});});
x.onreadystatechange=function(){EV.push('rsc'+x.readyState);if(x.readyState===4)ref=null;};
x.open('GET','data.json');x.send();
</script></body></html>
```

**Результат:** Lumen: `[rsc1, loadstart, rsc2, rsc3, rsc4, progress@rs4, …]`. Chrome: `[rsc1, loadstart, rsc2, rsc3, progress@rs3, rsc4, load@rs4, loadend@rs4]`.

## Что сделать

XHR §3.6.5 «handle response end»: сначала «process response body» (последнее `progress`
при `readyState` 3), затем `readyState` = DONE, `readystatechange`, `load`, `loadend`. Поменять
порядок в `commitResponse`. Критерий: репро даёт порядок Chrome.

## Исправление

`crates/js/src/xhr.rs` `deliver()`: последнее `progress` (только в асинхронном режиме — XHR
«handle response end-of-body» шаг 7 для синхронного его не шлёт) теперь уходит при
`readyState` 3, до `_setReadyState(4)`; дальше `readystatechange(4)` → `load` → `loadend`.
Путь `blob:` идёт через тот же `deliver()`. Воркерный XHR (`worker_net_shim.js`) порядок уже
соблюдал. Тест `xhr_final_progress_precedes_done_readystatechange` сверяет полную
последовательность с Chrome: `rsc1, loadstart@1, rsc2, rsc3, progress@3, rsc4, load@4, loadend@4`.
