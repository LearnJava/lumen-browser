# BUG-1175 — CSP: `script-src` не проверяется для вставленного скриптом `<script src>`, его судит `connect-src`

**Статус:** FIXED 2026-09-26 (P3)
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

## Исправление (2026-09-26, P3)

- `fetch_request_impl` (`crates/network/src/lib.rs`) больше не применяет `connect_src_gate` к
  запросу с destination `script`/`style`.
- Новый I/O-free pre-check `JsFetchProvider::check_element_src(destination, url, nonce,
  integrity)`. В `HttpClient` его обслуживают `element_src_policy`/`with_element_src_policy`/
  `element_src_gate`: для `script` вызывается `script_element_fetch_allows` с
  `parser_inserted: false`, для `style` новая `CspPolicy::style_element_fetch_allows`
  (сначала nonce, потом URL). Политику ставит `page_pipeline.rs` вместе с остальными гейтами.
- Нативная привязка `_lumen_check_element_src` (`crates/js/src/v8_runtime/install/net.rs`)
  возвращает `[effectiveDirective, blockedUri, originalPolicy]` или `[]`.
- Шим (`_lumen_element_src_blocked`, `web_api_shim_mid.js`) спрашивает её до `fetch()`:
  - `<script src>`: сразу `securitypolicyviolation` с `script-src-elem`, `error` элемента
    в свою очередь исполнения;
  - `<link rel=stylesheet>`/`@import`: только `error`. Событие нарушения для листов шлёт
    `style-src`-гейт shell-а (срез 7), второе было бы дублем.

Проба (видимое окно, `LUMEN_NO_ADBLOCK=1`, Chrome 153), `!!window.DYN_RAN`:

| Страница | Lumen | Chrome 153 |
|---|---|---|
| `/dyn_connect_none.html` | `true` | `true` |
| `/dyn_script_none.html` | `false`, `GET /dyn.js` нет | `false`, `GET /dyn.js` нет |
| `/dyn_nonce_ok.html` (вставленный скрипт с `nonce=abc`) | `true` | `true` |
| `/csp_nonce_ext.html` (BUG-1124) | `dyn:true, ext:true` | `dyn:true, ext:true` |

`<link rel=stylesheet>`, вставленный скриптом: при `connect-src 'none'` `load` в обоих браузерах.
При `style-src 'none'` `error` в обоих, но Lumen всё равно применяет лист. Это отдельный путь
shell-а, [BUG-1176](BUG-1176-OPEN.md).

Не сделано: `script-src-elem`/`style-src-elem` как самостоятельные директивы не разбираются,
`effective_sources` падает сразу на `default-src`. Парсерный `<script src>` (BUG-1124)
по-прежнему сообщает `violatedDirective: 'script-src'`.
