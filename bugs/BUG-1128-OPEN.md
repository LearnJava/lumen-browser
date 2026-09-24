# BUG-1128 — Событие `load` динамического `<script>` приходит только после исполнения всех скриптов очереди

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:11895-11910` `_lumen_script_load_external`: `setTimeout` → `fetch().then(execute).then(fire load)`, таймеры одного тика исполняются подряд)

## Симптом

SystemJS после `load` своего `<script>` забирает «последнюю регистрацию» (`System.register`).
В Lumen `load` первого скрипта видит регистрацию последнего, у остальных — `null`:
aliexpress (гео ru) — `[unhandled-rejection] Error: https://st.aestatic.net/…/react.17.0.1.js?cache=0
(SystemJS …errors.md#2)` для react, react-dom, login-utils и др. при ответах 200. Недорисованные
виджеты, 1218 узлов против 2805. На гео de SystemJS-ошибок нет — механизм подтверждён репро.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/scriptload.html`:

```html
<!doctype html><html><head><meta charset="utf-8"></head><body>
<script>
window.RES=[];window.__exec=[];
['m1','m2','m3','m4'].forEach(function(n){
  var s=document.createElement('script');s.src=n+'.js';s.async=true;
  s.addEventListener('load',function(){RES.push(n+'<-'+window.__last);window.__last=null;});
  s.addEventListener('error',function(){RES.push(n+':error');});
  document.head.appendChild(s);
});
</script></body></html>
```

`g5/m1.js`:

```js
window.__last='m1';window.__exec=(window.__exec||[]);window.__exec.push('m1');
```

**Результат:** Lumen: `['m1<-m4','m2<-null','m3<-null','m4<-null']`. Chrome: `['m1<-m1','m2<-m2','m3<-m3','m4<-m4']`.

## Что сделать

HTML LS §4.12.1.1 «execute the script element»: для внешнего скрипта шаг «fire an event
named load» идёт сразу после исполнения, в той же задаче. Слить исполнение и `load` в одну задачу
на скрипт. Критерий: репро даёт порядок Chrome.
