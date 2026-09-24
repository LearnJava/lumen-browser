# BUG-1137 — Нет интерфейса `History`: `typeof History === 'undefined'`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js:1006` — `var history = {` — литерал без интерфейса)

## Симптом

whatsapp: Meta hyperion `new c(History,null,{sampleObject:window.history})` →
`ErrorUtils caught an error: History is not defined [Caught in: Module "hyperionDOM"]`. Отключается
модуль телеметрии, контент не страдает (1314 узлов против 1310 без блокировщика). History ни в одном
открытом баге не упомянут.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g4/repro-history.html`:

```html
<!doctype html><html><head><meta charset="utf-8"><title>repro-history-iface</title></head>
<body style="background:#eef"><h1>History interface</h1><pre id="o"></pre>
<script>
// Minimal repro: whatsapp/Meta "hyperion" does `new ShadowPrototype(History, null, {sampleObject: window.history})`
var r = {};
r.typeofHistory = typeof History;
try { r.instanceofHistory = history instanceof History; } catch (e) { r.instanceofHistory = 'THROW ' + e.name + ': ' + e.message; }
try { r.protoIsHistoryProto = Object.getPrototypeOf(history) === History.prototype; } catch (e) { r.protoIsHistoryProto = 'THROW ' + e.name + ': ' + e.message; }
r.toStringTag = Object.prototype.toString.call(history);
r.ctorName = history.constructor && history.constructor.name;
window.__r = r;
document.getElementById('o').textContent = JSON.stringify(r);
</script></body></html>
```

**Результат:** Lumen: `ReferenceError: History is not defined`, `toString.call(history)='[object Object]'`, `history.constructor.name='Object'`. Chrome: `function`, `instanceof true`, `'[object History]'`, `'History'`.

## Что сделать

HTML LS §7.4.2: интерфейс `History` (не конструируемый, `Illegal constructor`), `history` —
его экземпляр, члены — на `History.prototype`, `Symbol.toStringTag` = `'History'`. Сделать вместе с
BUG-624/BUG-637 по одному образцу. Критерий: репро даёт результат Chrome.
