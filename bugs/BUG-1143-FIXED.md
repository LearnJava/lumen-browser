# BUG-1143 — Нет `window.toolbar`/`locationbar`/`menubar`/`personalbar`/`scrollbars`/`statusbar` и интерфейса `BarProp`

**Статус:** FIXED 2026-09-28 (P6, ветка `p6-bug-1143`)
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

## Исправление (2026-09-28, P6)

`crates/js/src/shim/web_api_shim_tail_mc.js`, сразу после `window.visualViewport`:

- `BarProp` — нелегальный конструктор (`TypeError`), на прототипе геттер `visible` без сеттера и
  `Symbol.toStringTag = 'BarProp'`.
- Шесть атрибутов — шесть **разных** объектов (`toolbar !== menubar`), каждый — собственный
  enumerable/configurable аксессор `window` с сеттером, который затеняет его data-свойством
  (`[Replaceable]`, WebIDL §3.7.6; форма, которую проверяет
  `html/browsers/the-window-object/window-properties.https.html`). На `globalThis` их переносит
  копирование дескрипторов в `web_api_shim_tail_b.js`, поэтому голые идентификаторы работают.
- `visible` всегда `true`: по спеке это отрицание «is popup» верхнего traversable, а Lumen
  открывает любой `window.open()` полноценной вкладкой с UI — всплывающих окон без UI нет.
  Если такие появятся, геттер должен читать флаг popup контекста.

Тесты: `crates/js/src/dom/tests/v8_bug1143_barprop.rs` (3 шт.).

**Живой прогон** (видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, Vulkan, GTX 1050):
репро `barprop.html` — `visible=true` ×6, `BarProp='function'`, ровно как Chrome. weibo:
цепочка `passport.weibo.com/visitor` → `login.sina.com.cn` → `weibo.com/newlogin` доходит до
конца за ~25 с (бандл `index-Xve1TSN5.js` — 6,8 МБ), страница строит 2370 узлов (до фикса 30,
Chrome 1684), ошибки `toolbar is not defined` в stderr нет.
