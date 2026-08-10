# BUG-756 — cookie default-path берётся как полный путь запроса вместо RFC 6265 §5.1.4: кука, поставленная без `Path`, не доезжает до соседнего пути

**Статус:** FIXED 2026-08-10
**Компонент:** network (`crates/network/src/lib.rs:2566-2567` — вычисление
`default_path` перед `jar.process_set_cookie`); потребитель —
`crates/storage/src/cookies.rs::parse_set_cookie_with_psl` (строка
`let mut path = default_path.to_string();`) и `path_matches` (там же)
**Найден:** 2026-08-10, диагностика живого логина `tbank.ru`

## Симптом

Логин на `https://www.tbank.ru/login/?redirectTo=/invest/portfolio/` не доходит
до формы входа. Цепочка (в Chrome/Edge и в curl проходит целиком):

1. `www.tbank.ru/login/?redirectTo=…` → **301** → `www.tbank.ru/auth/login/?redirectTo=…`
2. SPA (`sso-newauth`, tramvai) навигирует на
   `www.tbank.ru/api/common/v1/session/authorize/?…&post_complete_redirect_uri=…/invest/portfolio/`
3. → **303** → `id.tbank.ru/auth/authorize?state=<JWT>&client_id=portal-api&code_challenge=…`
   (OAuth PKCE). Этот ответ ставит `SSO_CONVERSATION_CSRF_<ID>=…; Max-Age=1800;
   Secure; HttpOnly; SameSite=None` — **без атрибута `Path`**
4. → **303** → `id.tbank.ru/auth/step?cid=<ID>` → 200, форма входа

Lumen проходит шаги 1–3 и на шаге 4 получает не форму, а
`…/auth/error?cid=…&error=invalid_request` — SSO-форму ошибки
(`window.formData.form = "error"`, `error: "invalid_request"`).

## Причина

`crates/network/src/lib.rs:2566`:

```rust
let req_path = url.path_and_query();
let default_path = req_path.split('?').next().unwrap_or("/");
```

Это **полный путь запроса**. RFC 6265 §5.1.4 (default-path) требует другого:

> …output the characters of the uri-path from the first character up to, but not
> including, the **right-most** `%x2F ("/")`. Если после этого строка пуста или
> `/` в пути ровно один — default-path = `/`.

То есть для запроса `/auth/authorize` default-path обязан быть `/auth`, а Lumen
кладёт `/auth/authorize`. `parse_set_cookie_with_psl` берёт переданное значение
дословно (`let mut path = default_path.to_string();`), и кука сохраняется с
`Path=/auth/authorize`. На следующем hop-е `path_matches("/auth/step",
"/auth/authorize")` даёт `false` → conversation-CSRF-кука не отправляется →
сервер не находит conversation → `invalid_request`.

Точка вычисления в кодовой базе **одна** (grep `default_path` по `crates/`
даёт только `lib.rs:2567`, объявление трейта в `core/src/ext.rs:374` и
несвязанный тест в `websocket/upgrade.rs`), так что дефект общий для всех
запросов, а не только для редирект-цепочек — просто на редирект-цепочке он
проявляется чаще всего.

## Репро (минимальное, без tbank)

Локальный HTTP-сервер: `/auth/authorize` отвечает
`303` + `Set-Cookie: SSO_CSRF=abc123; Max-Age=1800; HttpOnly` (без `Path`) и
`Location: step?cid=42`; `/auth/step` печатает пришедший `Cookie`.

| цель редиректа | curl (эталон) | Lumen |
|---|---|---|
| `/auth/step?cid=42` | `Cookie: SSO_CSRF=abc123` | `Cookie: ` (пусто) |
| `/auth/authorize/sub?cid=42` | `Cookie: SSO_CSRF=abc123` | `Cookie: SSO_CSRF=abc123` |

Вторая строка важна: она доказывает, что кука **сохранена** — просто с путём
`/auth/authorize` вместо `/auth`. Это не «кука отброшена», а «кука с неверным
Path».

## Как чинить

Заменить вычисление на алгоритм RFC 6265 §5.1.4 — по-хорошему отдельной
функцией в `lumen-storage` (рядом с `path_matches`, чтобы правило хранения и
правило сопоставления лежали вместе), а `network` вызывал бы её:

```rust
/// RFC 6265 §5.1.4 default-path.
fn default_path(uri_path: &str) -> &str {
    if !uri_path.starts_with('/') { return "/"; }
    match uri_path.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &uri_path[..i],
    }
}
```

Тесты: `/auth/authorize` → `/auth`; `/auth/` → `/auth`; `/index.html` → `/`;
`/` → `/`; `` → `/`; `foo` (без ведущего `/`) → `/`. Плюс интеграционный на
редирект-цепочке в `crates/network/src/lib.rs` (там уже есть mock-серверные
тесты hop-ов, см. `redirect`-тесты около строк 5381/5706) — кука, поставленная
на hop 1 без `Path`, обязана уйти на hop 2 с соседним путём.

Внимание при правке: `parse_set_cookie*` — публичный API, `default_path`
приходит снаружи; менять надо **источник**, иначе поведение разъедется между
network и другими вызывающими.

## Исправление (2026-08-10)

Правило §5.1.4 переехало в `lumen-storage` и стало публичным —
`crates/storage/src/cookies.rs::default_path`, соседняя функция к
`path_matches`, как и предполагалось в «Как чинить». Применяется оно **не в
`network`**, а на границе трейта: `CookieJarProvider::process_set_cookie`
(`cookies.rs`) выводит default-path из полученного request-path. Причина —
`lumen-network` не зависит от `lumen-storage` (deps: `lumen-core`, `lumen-ipc`),
так что «network вызывает функцию из storage» стоило бы новой зависимости
поперёк слоёв; граница трейта `CookieProvider` даёт то же «одно место», зато
общее для всех caller-ов — сетевого hop-а и `document.cookie`
(`v8_runtime.rs`), — и не трогает публичный `parse_set_cookie*`, который
по-прежнему берёт default-path дословно (его doc-комментарий теперь явно
отсылает к `default_path`).

`network` перестал вычислять путь: отдаёт request-target hop-а как есть.
Контракт трейта переформулирован в `crates/core/src/ext.rs` — параметр
переименован `default_path` → `request_path` с явным «pass the request path
as-is, *not* a pre-computed default-path».

**Второй дефект того же шва, закрыт заодно.** `CookieJar::get_for_request`
матчил путь вместе с query: `path_matches("/auth/step?cid=42", "/auth/step")`
даёт `false` (после prefix-а идёт `?`, а не `/`), то есть cookie с **явным**
`Path=/auth/step` не уходила на тот же путь с query-строкой. Оба входа теперь
нормализуются одним внутренним `uri_path()`. Отдельным багом не заводился:
это та же ошибка — сырой request-target в роли пути, — и держать половину
шва сломанной, переписывая вторую, смысла нет.

`SameSite=Lax` на cross-site (см. ниже) не трогался — отдельная тема.

Тесты: `default_path_*` (6 случаев §5.1.4, включая query/fragment и
malformed), `provider_set_cookie_without_path_uses_default_path` /
`provider_explicit_path_attribute_wins` (сквозной hop-сценарий на реальном
jar) в `lumen-storage`; `redirect_hop_passes_own_request_path_to_cookie_jar`
в `lumen-network` (mock-jar на 303-цепочке — фиксирует, что network отдаёт
путь именно того hop-а, который принёс `Set-Cookie`). Плюс живой прогон
репро-таблицы выше на локальном сервере: обе цели получают
`Cookie: SSO_CSRF=abc123`.

## Смежные наблюдения (не часть этого бага)

* `CookieJarProvider::get_for_request` (`crates/storage/src/cookies.rs:597`)
  безусловно отбрасывает `SameSite=Lax`-куки на любом cross-site запросе, с
  явным комментарием «We don't distinguish top-level navigation here; Phase 0
  conservative». По RFC 6265bis §5.4 Lax-кука **обязана** уходить на top-level
  safe-навигации; на этом сценарии не сработало (все куки `SameSite=None`), но
  на других сайтах со сквозным логином сработает.
* Второй дефект, найденный на этой же цепочке, — URL документа не обновляется
  после HTTP-редиректа: [BUG-757](BUG-757-FIXED.md).
