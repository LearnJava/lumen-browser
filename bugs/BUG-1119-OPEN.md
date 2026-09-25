# BUG-1119 — `document.cookie` молча не сохраняет запись: в V8-рантайм передаётся `cookie_jar = None`

**Статус:** OPEN
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
