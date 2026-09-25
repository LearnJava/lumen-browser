# BUG-1119 — `document.cookie` молча не сохраняет запись: в V8-рантайм передаётся `cookie_jar = None`

**Статус:** FIXED 2026-09-25 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/v8_runtime.rs:156` — `let cookie_jar = None`, дальше `install_cookie` ставит заглушки `_lumen_cookie_get → ""`/`_lumen_cookie_set → {}`)

## Симптом

Проверка `Cookies.enabled` на login.microsoftonline.com пишет пробную cookie и читает её
обратно; в Lumen читается `''`, в аудит-логе `cookiesdisabled`, дальше страница показывает ветку
«cookies отключены» с пустым UI: 31 узел против 99 в Chrome.

`crates/js/src/v8_runtime.rs:156` создаёт `cookie_jar` как `None` («Cookie access is not part of the
S3 DOM-core signature»), и `install::install_cookie` (`v8_runtime/install/storage.rs:385-414`)
при `None` регистрирует заглушки: чтение всегда `""`, запись — no-op. Банка cookie в шелле есть
(`window_mode.rs:369`, отдаётся сетевому стеку через `with_cookie_jar`), в JS она не доходит.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/cookie.html`:

```html
<!doctype html><html><body><script>
window.R = {};
R.enabled = navigator.cookieEnabled;
R.docDomain = (function(){ try { return String(document.domain); } catch (e) { return '!' + e; } })();
function has(name) { return document.cookie.split(';').some(function (p) { return p.trim().split('=')[0] === name; }); }
function t(key, name, str) {
  try { document.cookie = str; R[key] = has(name); } catch (e) { R[key] = '!' + e; }
}
t('plain', 'c_plain', 'c_plain=1');
t('path', 'c_path', 'c_path=1; path=/');
t('noSpace', 'c_nosp', 'c_nosp=1;path=/');
t('samesiteNone', 'c_ssn', 'c_ssn=1; path=/; SameSite=None');
t('expires', 'c_exp', 'c_exp=1; expires=' + new Date(Date.now() + 86400e3).toUTCString());
t('maxage', 'c_ma', 'c_ma=1; max-age=3600');
// login.microsoftonline.com Cookies.enabled(): "CkTst=G<ts>;domain=<document.domain>;path=/"
t('msftExact', 'CkTst', 'CkTst=G' + Date.now() + ';domain=' + document.domain + ';path=/');
t('domainHost', 'c_dom', 'c_dom=1; domain=' + location.hostname);
R.all = document.cookie;
R.referrerType = typeof document.referrer;
R.referrerInDoc = 'referrer' in document;
</script></body></html>
```

**Результат:** Lumen: `all=''` во всех 10 вариантах. Chrome: `c_plain=1; c_path=1; c_nosp=1; c_exp=1; c_ma=1; CkTst=G…; c_dom=1` (не хранится только `SameSite=None` без `Secure`, это верно).

## Что сделать

Передать в `V8JsRuntime` ту же `CookieJar` вкладки, что получает сетевой стек (`CookieProvider`
в `lumen_core::ext`), и убрать `None` в `v8_runtime.rs:156`. RFC 6265 §5.3/§5.4 и HTML LS
§3.1.3 «document.cookie»: запись идёт через тот же алгоритм, что `Set-Cookie`, `HttpOnly` из скрипта
не читается и не пишется. Критерий: репро выше даёт в Lumen те же 7 cookie, что в Chrome, и
cookie, выставленная скриптом, уходит в заголовке `Cookie` следующего запроса.

## Реальные сайты: imdb, espn, amazon (2026-09-25, P6, после BUG-892)

После починки `document.scripts` ([BUG-892](BUG-892-FIXED.md)) челлендж AWS WAF на imdb
проходит `inputs` → `verify` (200), но токен `aws-waf-token` пишется через `document.cookie`
(`aws-waf-token=…;path=/;domain=.imdb.com;expires=…;secure;SameSite=Lax`) и теряется: на живой
странице imdb все шесть проб-записей, включая голое `'t1=a'`, читаются обратно как `''`. Сайт
отдаёт 202 с челленджем снова, после четырёх попыток — «Max challenge attempts exceeded»
(14 узлов). espn и amazon стоят на том же челлендже.

## Реальный сайт: fandom (2026-09-25, P6, после BUG-1121)

После починки `document.referrer` ([BUG-1121](BUG-1121-FIXED.md)) модуль `tracking-*.js` на
fandom больше не падает, и первой ошибкой верхнего уровня становится
`SyntaxError: "undefined" is not valid JSON` в `_common-DwRVE_Wm.js` (`O()` → `ge()`,
`initLogoTakeover`): `JSON.parse(D.get("Geo"))`, где `D` читает cookie `Geo`. В Chrome
`document.cookie` на fandom — 9 записей, `Geo={"region":"01","city":"tallinn",…,"country":"EE"}`;
в Lumen — одна пустая запись, `Geo` нет. Cookie ставит сервер или скрипт — в любом случае
`document.cookie` её не видит; это тот же дефект, отдельной заявки не заводится.

## Исправление (2026-09-25, P6)

**Причина.** `V8JsRuntime::install_dom` жёстко ставил `cookie_jar = None` («не часть сигнатуры
S3»), и никто из шелла банку в рантайм не передавал, хотя та же `CookieJar` вкладки уже уходила
в `HttpClient` страницы. Второй слой: даже с банкой `install_cookie` читал и писал по пути `/`
вместо пути документа и через сетевые методы — `HttpOnly` было бы видно скрипту.

**Что сделано.**

1. `lumen_core::ext::CookieProvider` получил пару «non-HTTP API» — `get_for_script` /
   `set_from_script` (по умолчанию пусто/игнор, чтобы реализация, не различающая `HttpOnly`,
   не отдала его скрипту). `CookieJarProvider` (`crates/storage/src/cookies.rs`) реализует их
   поверх общего storage model: чтение без `HttpOnly` (RFC 6265 §5.4 шаг 1), запись отбрасывает
   строку с `HttpOnly` и не перезаписывает существующую `HttpOnly`-cookie (§5.3 шаг 10,
   `CookieJar::is_http_only`). `Set-Cookie` и `document.cookie` идут через один `store`.
2. `V8JsRuntime::with_cookie_jar` — банка вкладки ставится до `install_dom`, как
   `with_session_storage` (BUG-836). `install_cookie` берёт хост через
   `host_ascii_normalized`, путь документа для path-match и default-path записи, а документ без
   сетевого хоста (`about:`, `data:`, `file:`) считает cookie-averse (HTML LS §3.1.3).
3. Шелл передаёт банку во все пути, где рождается рантайм документа: `run_scripts_with_dom`
   (новый параметр; `page_pipeline` — `cookie_jar` страницы, фрейм — `env.cookie_jar`, кроме
   opaque origin, гибернация — банка восстановления) и разморозка из bfcache
   (`active_cookie_jar()`). Headless `--dump`/PDF, как и раньше, без банки.

Разбиение (`top_level_site`) у скрипта `None` — ровно то же, что у `HttpClient` страницы
(`with_cookie_jar(…, None)`), поэтому скрипт и сеть видят одну и ту же партицию.

**Проверка.**

- `cargo test -p lumen-js --features v8-backend --lib -- v8_bug1119` — 6 тестов
  (`crates/js/src/dom/tests/v8_bug1119_document_cookie.rs`): семь форм репро хранятся, `SameSite=None`
  без `Secure` — нет; cookie из скрипта уходит в `Cookie` следующего запроса, `Set-Cookie` читается
  скриптом; `HttpOnly` не видна и не перезаписывается; путь документа ограничивает чтение и задаёт
  default-path; `Secure` с `http:` отбрасывается; cookie-averse документы и рантайм без банки — пусто.
- Живое окно `--maximized`, без блокировщика, `probe.py both`, Chrome 153 — одинаково:
  - `g3/cookie.html` (сервер `127.0.0.1` с `/echo`, отдающим заголовок `Cookie`): Lumen
    `all` = те же 7 cookie, что в Chrome; в `Cookie` синхронного XHR — `c_path`, `c_nosp`, `CkTst`
    (у остальных default-path `/g3`), как у Chrome;
  - страница с `Set-Cookie: srv=hdr` и `srv_ho=secret; HttpOnly`, скрипт пишет `srv_ho=overwrite`:
    оба браузера — `document.cookie = 'srv=hdr'`, в заголовке `srv=hdr; srv_ho=secret`.
- Реальные сайты (те же пробы):
  - login.microsoftonline.com — 138 узлов против 136 у Chrome (до правки 31 против 99), форма входа;
  - fandom — `Geo` есть, 9 cookie в обоих, 3456 / 3448 узлов, ошибок JS нет;
  - imdb — челлендж AWS WAF пройден: `202` → `inputs` → `verify` → повторный `GET /` = `200`.
    Страница дальше падает на styled-components #17 (`HTMLStyleElement.sheet === null` сразу после
    `appendChild`) — это остаток [BUG-493](BUG-493-FIXED.md), записан туда.
