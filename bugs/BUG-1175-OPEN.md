# BUG-1175 — CSP: `script-src` не проверяется для вставленного скриптом `<script src>`, его судит `connect-src`

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, по ходу [BUG-1124](BUG-1124-FIXED.md); видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, сравнение с Chrome 153 тем же способом).
**Область:** js/network (`crates/js/src/shim/web_api_shim_mid.js:12334`
`_lumen_script_load_external` → JS-`fetch(url, {_lumenInitiatorType: 'script'})`, `:12352`;
`crates/network/src/lib.rs:5512` — `fetch_request_impl` применяет к нему `connect_src_gate`).

## Симптом

Вставленный скриптом `<script src>` (`document.createElement('script')` + `appendChild`)
грузится через общий JS-`fetch()`. Этот путь проверяет только `connect-src`. Поэтому:

- `script-src` для него не действует. Страница с `script-src 'nonce-abc'` (без
  `'strict-dynamic'`) исполняет вставленный скрипт без nonce, Chrome его блокирует;
- `connect-src` его ошибочно режет: при `connect-src 'none'` и отсутствии `script-src` Lumen
  скрипт не грузит (`script load failed … fetch: network error`), Chrome грузит и исполняет.

`securitypolicyviolation` в первом случае не приходит вовсе, во втором приходит с неверной
директивой.

## Репро

Сервер — `g3/server.py` из [BUG-1124](BUG-1124-FIXED.md) §Репро, в `PAGES` добавлены две страницы:

```python
'/dyn_connect_none.html': ("connect-src 'none'",
    b"""<!doctype html><html><body><div id=out>not run</div>
<script>var s = document.createElement('script'); s.src = '/dyn.js'; document.body.appendChild(s);</script>
</body></html>"""),
'/dyn_script_none.html': ("script-src 'nonce-abc'",
    b"""<!doctype html><html><body><div id=out>not run</div>
<script nonce="abc">var s = document.createElement('script'); s.src = '/dyn.js'; document.body.appendChild(s);</script>
</body></html>"""),
```

`/dyn.js` ставит `window.DYN_RAN = true`. Проба через 3 с после `document_ready`: `!!window.DYN_RAN`.

| Страница | Lumen | Chrome 153 |
|---|---|---|
| `/dyn_connect_none.html` | `false` (запрос заблокирован `connect-src`) | `true` |
| `/dyn_script_none.html` | `true` (`→ GET /dyn.js`, `← 200`) | `false` |

## Что сделать

Запрос скрипта — destination `script`: его судит `script-src` → `default-src`, а не
`connect-src` (CSP3 §6.1.1.1, таблица директив по destination). Для вставленного скриптом
`<script src>`:

1. не применять `connect_src_gate` к `fetch` с `_lumenInitiatorType: 'script'`;
2. применять `CspPolicy::script_element_fetch_allows` (`crates/network/src/csp.rs:242`) с
   `ScriptRequestMetadata { nonce, integrity, parser_inserted: false }`. При `'strict-dynamic'`
   такой скрипт пропускается, это шаг 1.3;
3. при блоке — `securitypolicyviolation` с `effectiveDirective: 'script-src-elem'` и `error` на
   элементе.

Тот же вопрос стоит для `<link rel=stylesheet>` (`:12580`, `_lumenInitiatorType: 'link'`) и
`@import` (`:13025`, `'css'`): их директива — `style-src`. Проверить при правке.

Критерий: обе страницы репро совпадают с Chrome; `/csp_nonce_ext.html` из BUG-1124 даёт `dyn:true`.
