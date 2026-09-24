# BUG-1121 — `document.referrer` отсутствует (`undefined` вместо строки)

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:~10842` — литерал `var document = {…}`: рядом `cookie`/`title`/`domain`, геттера `referrer` нет)

## Симптом

Четыре сайта падают на одной строке — разбор `document.referrer`:

| Сайт | Место | Ошибка |
|---|---|---|
| imgur | mixpanel `searchEngine` → `_set_default_superprops` → `init` | `Cannot read properties of undefined (reading 'search')`; страница пустая, 103 узла против 1171 |
| fandom | `tracking-4RLpXSVX.js`: `const ie=new dt(document.referrer)` на верхнем уровне модуля | `reading 'search'`; модуль и зависящий от него граф не исполняются |
| yahoo | `analytics-3.84.0.js` `getReferrer()` → `clref(document.referrer).indexOf` | `Rapid initialization error … reading 'indexOf'` |
| yahoo.co.jp | `ds-custom-logger-1.1.0` `cleanReferrer(document.referrer)` | `reading 'indexOf'` |

GAP-REFERRER (done) и BUG-859 касаются заголовка `Referer`, не `document.referrer`; BUG-811
упоминает пробел мимоходом.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/referrer.html`:

```html
<!doctype html><html><body><script>
// imgur: mixpanel's searchEngine() does `document.referrer.search(...)`; the cookie
// and referrer members of document.
window.R = {};
R.typeofReferrer = typeof document.referrer;
try { R.searchCall = document.referrer.search(/google/); } catch (e) { R.searchErr = String(e); }
</script></body></html>
```

**Результат:** Lumen: `typeofReferrer='undefined'`, `searchErr="TypeError: Cannot read properties of undefined (reading 'search')"`. Chrome: `'string'`, `searchCall=-1`.

## Что сделать

HTML LS §3.1.2 «The document's referrer»: геттер возвращает URL реферера документа
(сериализованный, после Referrer Policy) или `''`. Шелл уже считает реферер для заголовка
`Referer` навигации (GAP-REFERRER) — отдать то же значение в шим. Минимум — `''`, если реферера нет.
Критерий: репро даёт `'string'`; imgur, fandom, yahoo без этих ошибок.
