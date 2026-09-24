# BUG-1143 — Нет `window.toolbar`/`locationbar`/`menubar`/`personalbar`/`scrollbars`/`statusbar` и интерфейса `BarProp`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/*` — ни одного определения `toolbar`/`locationbar`/`BarProp`, `grep` — 0)

## Симптом

weibo: модуль `index-Xve1TSN5.js` содержит `components:{Toolbar:toolbar}` с обращением к голой
глобали `toolbar` → `toolbar is not defined`, 30 узлов против 1684. Замечено в прогоне, дошедшем до
`weibo.com/newlogin`; в повторе без блокировщика до `newlogin` не дошли из-за оборванного
TLS-рукопожатия с `login.sina.com.cn` (отдельная гипотеза в журнале).

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/barprop.html`:

```html
<!doctype html><html><body><script>
window.R = {};
['toolbar', 'locationbar', 'menubar', 'personalbar', 'scrollbars', 'statusbar', 'BarProp'].forEach(function (n) {
  try { var v = eval(n); R[n] = (v && typeof v === 'object') ? ('visible' in v ? 'visible=' + v.visible : 'object') : typeof v; }
  catch (e) { R[n] = '!' + e.name; }
});
R.referrer = typeof document.referrer;
</script></body></html>
```

**Результат:** Lumen: `toolbar..statusbar/BarProp = '!ReferenceError'`. Chrome: `'visible=true'` ×6, `BarProp='function'`.

## Что сделать

HTML LS §7.2.4: шесть атрибутов `Window`, каждый — `BarProp` с `visible` (в обычном окне
`true`, во всплывающем без UI — `false`). Критерий: репро даёт результат Chrome.
