# BUG-1135 — `import.meta.resolve()` склеивает строки вместо разрешения URL

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/import_meta.rs:55-62` `build_preamble`: `resolve = b.slice(0, lastIndexOf('/')+1) + s`; тест `crates/js/src/v8_esm.rs:1388` `v8_import_meta_resolve_relative` закрепляет неверный результат)

## Симптом

huggingface: предзагрузчик модулей Vite вызывает `import.meta.resolve('/front/build/kube-9c7437b/X.js')`
и получает `/front/build/kube-9c7437b//front/build/kube-9c7437b/X.js` → все `modulepreload`-запросы
падают (`link hint fetch failed … network error`). Модули всё равно грузятся через `import`, так что
деградация небольшая (1017 узлов против 1018), но пропадают все подсказки предзагрузки.
`new URL()` в Lumen разрешает правильно.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g6/site/importmeta.html`:

```html
<!doctype html><html><head><meta charset=utf-8></head><body><p>import.meta.resolve</p>
<script type=module src="/sub/dir/m.js"></script>
<script>window.__r=()=>window.__res||'module not run';</script></body></html>
```

`g6/site/sub/dir/m.js`:

```js
const r = {};
r.meta_url = import.meta.url;
r.has_resolve = typeof import.meta.resolve;
for (const s of ['/abs/x.js', './rel.js', '../up.js', 'https://example.com/y.js']) {
  try { r['resolve ' + s] = import.meta.resolve(s); } catch (e) { r['resolve ' + s] = 'THROW ' + e; }
  try { r['URL ' + s] = new URL(s, import.meta.url).href; } catch (e) { r['URL ' + s] = 'THROW ' + e; }
}
window.__res = r;
```

**Результат:** Lumen: `resolve('/abs/x.js')` = `http://127.0.0.1:8766/sub/dir//abs/x.js`, `'./rel.js'` → `.../sub/dir/./rel.js`, `'../up.js'` → `.../sub/dir/../up.js`. Chrome: `/abs/x.js`, `/sub/dir/rel.js`, `/sub/up.js`.

## Что сделать

HTML LS §8.1.5.5 «resolve a module specifier»: import map, затем «resolve a URL-like
module specifier» (URL-парсер относительно URL модуля); голый спецификатор без import map —
`TypeError`. Использовать тот же резолвер, что `import()` (`v8_esm.rs::resolve`), а тест
`v8_import_meta_resolve_relative` исправить. Критерий: репро даёт результат Chrome.
