# BUG-1121 — `document.referrer` отсутствует (`undefined` вместо строки)

**Статус:** FIXED 2026-09-25 (P6)
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

## Исправление (2026-09-25, P6)

- Живой `document` получил геттер `referrer` (`crates/js/src/shim/web_api_shim_mid.js`, рядом с
  `domain`), значение — `_lumen_document_referrer` (`web_api_shim_mid_b.js`, рядом с
  `_lumen_document_domain`). Только чтение: присваивание в нестрогом режиме молча игнорируется.
- Документ из `createHTMLDocument`/`createDocument` (`_lumen_build_detached_document`) и фасад
  документа `<iframe>` (`crates/js/src/frame_bridge.rs`) — тоже `''`, а не `undefined`.
- Значение сейчас всегда `''`: навигация верхнего уровня `Referer` не шлёт — это не пробел шима,
  а отдельный дефект сети/шелла, [BUG-1156](BUG-1156-OPEN.md). Переменная засевается в одном
  месте, когда он будет исправлен.

Тесты: `crates/js/src/dom/tests/v8_bug1121_document_referrer.rs` (присутствие, тип, `.search`/
`.indexOf`, только чтение, созданный документ).

## Проверка

Сборка `dev-release`, видимое окно, `--maximized`, без блокировщика (`LUMEN_NO_ADBLOCK=1`):

- Репро `referrer.html`: `typeofReferrer='string'`, `searchCall=-1` — как в Chrome.
- imgur, fandom, yahoo, yahoo.co.jp: ни одной ошибки `reading 'search'`/`reading 'indexOf'`,
  `typeof document.referrer === 'string'`.

Сайты при этом до Chrome не доходят — после этой ошибки видны следующие, у каждой своя заявка:

| Сайт | Следующая ошибка | Заявка |
|---|---|---|
| imgur (106 узлов, Chrome 1231) | `transformPose`: `getComputedStyle().transform` не `matrix(…)` → `null[1]` | [BUG-1157](BUG-1157-OPEN.md) |
| fandom (3473 / 3448) | `JSON.parse(cookie Geo)` → `"undefined" is not valid JSON` | [BUG-1119](BUG-1119-OPEN.md) |
| yahoo | `SyntaxError: Unexpected token ':'` в скрипте, вставленном `appendChild` | [BUG-1158](BUG-1158-OPEN.md) |
| yahoo.co.jp | `eval` пробы через 8 с: `JS context not available` | [BUG-1145](BUG-1145-OPEN.md) |
