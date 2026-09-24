# BUG-1114 — навигация с не-2xx ответом выбрасывает тело: страницы-челленджи и страницы ошибок сайта не рендерятся

**Статус:** FIXED 2026-09-24 (P3)
**Заведён:** 2026-09-23 (P2, прогон top100-foreign против видимого Chrome 153,
[журнал](../docs/perf/journal.md) §2026-09-23 top100 split).
**Область:** network (`crates/network/src/lib.rs::fetch_with_redirect` — ветка
`status => return Err(Error::Network(format!("HTTP {status}")))`, ~строка 2964;
комментарий у `200..=299` прямо говорит «для 4xx/5xx — caller получает Err по статусу,
тело туда не доходит») + shell (`app/user_event.rs:386` — `LoadEvent::LoadError`).

## Симптом

Любой финальный статус вне 2xx/3xx/304 превращается в сетевую ошибку: вместо страницы
сервера Lumen показывает «Ошибка загрузки … network error: HTTP NNN». В прогоне
100 сайтов так потеряны страницы, которые Chrome рендерит:

| Сайт | Ответ сервера | Что в теле | Chrome 153 |
|---|---|---|---|
| ebay.com | 403, `server: AkamaiGHost`, `Set-Cookie: bm_s=…` | страница Akamai Bot Manager со скриптом-челленджем | прошёл челлендж: 2445 узлов, 58 ресурсов |
| stackoverflow.com | 302 → `/questions` 403, `cf-mitigated: challenge` | «Just a moment…» Cloudflare | прошёл: 3270 узлов, 40 ресурсов |
| costco.com | 404 | страница сайта | 40 узлов (свою страницу показал) |
| target.com | 429 | страница сайта | 19 узлов |

Тела ответов сняты `curl_cffi` с Chrome-отпечатком по тому же маршруту
(`.tmp/tlslab/` в worktree аудита). Для ebay/stackoverflow 403 отдаётся даже идеальной
имитации Chrome — это не отпечаток: челлендж ждёт, что браузер **выполнит скрипт из
тела 403** и перезапросит страницу с полученной cookie. Lumen тело не видит вовсе,
поэтому пройти челлендж не может в принципе.

## Почему это дефект, а не пробел

По HTML LS (navigate → «process a navigate fetch») ответ навигации с любым HTTP-статусом
— это документ; статус влияет только на `Response.status`, а не на то, рендерится ли тело.
Пустое тело / сетевая ошибка — единственный случай страницы ошибки браузера. Правка
локальная: вернуть навигации `Ok(Response)` для 4xx/5xx (с декодированием
Content-Encoding, как у 2xx) вместо `Err`, а решение «показать страницу ошибки»
оставить shell-у для пустого тела. Подресурсы (`<img>`, `fetch()`) не задевать —
у них своя семантика статуса (fetch() уже должен видеть 404 как `ok=false`, а не
reject; проверить заодно).

## Как проверить

- Локально: эхо-сервер, отдающий 403 с HTML-телом → Lumen должен показать тело.
- Живьём: stackoverflow.com, ebay.com, costco.com (4xx-страница сайта вместо
  «Ошибка загрузки»); прохождение челленджей — отдельный вопрос JS-совместимости,
  но без этого фикса он даже не начинается.


## `fetch()` — тот же корень (2026-09-24)

`fetch()` с ответом 4xx/5xx реджектится (`TypeError: fetch: network error`), а должен
резолвиться `Response{ok:false}` (Fetch §4.1: HTTP-ошибка — не network error). fandom:
`fetch('https://services.fandom.com/whoami/')` → 401 → `fetch error: network error: HTTP 401`.
Репро `.tmp/compat/g4/repro-fetch.html`: Lumen `fetch404 REJECT`, Chrome `resolved status=404
ok=false`, тело 335 байт. Тот же `lib.rs:2964` (`status => Err(HTTP {status})`), путь
`fetch_request_impl` → `fetch_with_redirect` (`lib.rs:5352`). Там же duolingo: переход на
`/errors/not-supported.html` (из-за отсутствующего `IntersectionObserverEntry`) даёт 404 и страницу
ошибки Lumen вместо тела.

## Исправлено P3 2026-09-24

Один общий корень у навигации и `fetch()` — оба заходили в `fetch_with_redirect` и
падали на одном и том же catch-all (`status => return Err(...)`, включая 401 без
auth-retry). Единая правка на уровне этой функции: новый параметр
`http_error_is_response: bool`, протянутый через все hop-ы редиректа рядом с
`is_top_level` (та же схема пробрасывания). Когда он `true`, финальный
не-2xx/3xx/304 статус декодирует `Content-Encoding` и возвращает `Ok((Response, Url))`
— тем же путём, что и 2xx-ветка — вместо `Err`; новый хелпер
`resolve_http_error_status` делит эту логику между 401-веткой (все четыре точки
отказа: нет `WWW-Authenticate`, нет подходящей challenge-схемы, провайдер не нашёл
creds, не собрался `Authorization`-заголовок) и генеральным catch-all.

Флаг **не** стал глобальным поведением — у части caller-ов есть документированный
контракт «4xx/5xx ⇒ `Err`», который ломать нельзя: `fetch_range`/`fetch_multi_range`
(416 и любой другой не-2xx — ошибка диапазона, не тело), `fetch_conditional`
(ad-block листы — 4xx означает «не удалось обновить», а не «вот новый список»),
движковые subresource-загрузки (`fetch_subresource_inner`/`fetch_cors`) и
`NetworkTransport::fetch` (обновление приложения, загрузки — 404 должен остаться
ошибкой, а не сохранённым на диск телом страницы-ошибки). Флаг включён точечно
только там, где HTML LS/Fetch реально требуют видеть тело: `fetch_page` и
`fetch_page_streaming` (навигация, оба call-сайта — кэш-revalidate и обычный) и
`fetch_request_impl` (`fetch()`).

Shell-у ничего чинить не потребовалось: `LoadEvent::LoadError` (`page_load.rs`) и
так срабатывает только на `Err` из `load_bytes`/`load_bytes_streaming`, а не на
код статуса — с телом вместо `Err` навигация просто идёт по обычному пути
`HtmlChunk`/`LoadDone`, и «показать страницу ошибки» остаётся решением сервера
(через тело) там, где оно есть, либо пустым документом там, где сервер вернул
`Content-Length: 0`.

Тесты (`crates/network/src/lib.rs`): `fetch_page_returns_body_for_403_instead_of_err`
(эхо-сервер 403 + HTML-тело → `fetch_page` возвращает `Ok` с этим телом, не `Err`),
`js_fetch_sync_resolves_ok_false_for_404_instead_of_rejecting` (то же для
`fetch_sync`). Полный `cargo test -p lumen-network --lib` — 2434/2434,
`cargo clippy -p lumen-network --all-targets -- -D warnings` чист.
