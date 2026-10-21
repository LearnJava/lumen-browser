# DEVX-20 — хвосты BiDi: фазы responseStarted/authRequired и верификация storage.*

Дорожка DEVX. Владелец — P2. Остаток BUG-295 (задокументирован в `docs/automation.md` §WebDriver BiDi).

## Зачем

`network.continueResponse`/`continueWithAuth` сейчас accepted-and-stored: сервер отвечает success,
но на живое окно не действует (`crates/bidi-server/src/protocol.rs:693`), а фазы
`responseStarted`/`authRequired` принимаются в `addIntercept`, но запрос на них реально не
останавливается (`crates/network/src/intercept.rs:29`). `storage.getCookies|setCookie|deleteCookies`
не верифицированы против живого окна (DEVX-6 их не покрывал). Это блокирует часть сетевых
WPT-тестов и честность протокола: success без эффекта хуже честного `unsupported operation`.

## Что сделать

1. `crates/network/src/intercept.rs`: расширить process-global registry паузой на фазе
   `responseStarted` — перехват после получения заголовков ответа, до отдачи тела потребителю;
   `continueResponse{request}` возобновляет, `failRequest` обрывает. Схема та же, что у
   действующей фазы `beforeRequestSent` (пауза по id + resume/fail).
2. `authRequired`: пауза на 401/407 до вызова `continueWithAuth` (`action: provideCredentials |
   default | cancel`). Если HTTP-auth-challenge в `lumen_network` сейчас не выделен как событие —
   честный вариант-минимум: НЕ принимать фазу молча, а возвращать `unsupported operation` на
   `addIntercept` с `authRequired` и снять комментарий «accepted silently». Решение зафиксировать
   в коде и `docs/automation.md`.
3. `crates/bidi-server/src/protocol.rs:691-693`: `network.continueResponse`/`continueWithAuth`
   подключить к registry вместо пустого success; события `network.responseStarted` с
   `isBlocked:true` для подписанных сессий.
4. Верификация cookies: добавить в `tests/wpt/verify_devx6_bidi_scenarios.py` (или рядом,
   `verify_devx20_bidi_network.py`) живые сценарии: setCookie → виден в `document.cookie` страницы;
   getCookies после `Set-Cookie` от локального тестового сервера; deleteCookies; пауза
   responseStarted → continueResponse → страница дорисовалась.
5. Уровни введённых поверхностей — по ADR-024 (эти команды — стандартный BiDi, не `x-`), поведение
   задокументировать в `docs/automation.md` §WebDriver BiDi, сняв строки про «accepted-and-stored
   only» (doc-sync в том же коммите).

## Done when

- `cargo clippy -p lumen-bidi-server -p lumen-network --all-targets -- -D warnings` чисто,
  существующие тесты protocol.rs/intercept.rs проходят, новые юнит-тесты на паузу responseStarted.
- Live-сценарии verify-скрипта проходят против реального окна.
- В `docs/automation.md` не осталось утверждений, противоречащих новому поведению.

## Не трогать

- CDP (`--devtools-port`) — осознанный stub, вне задачи.
- Модель доступа (токены, ADR-024) не ослаблять.

## Статус WIP (ветка `p2-devx-20`, 2026-10-22)

Сделано (код, не закоммичено в main):
- `crates/network/src/intercept.rs`: фаза `responseStarted` — `pause_for_response_started(url, status)`, `InterceptAnnouncement {request_id,url,phase,status}` вместо кортежа; вызов в `fetch_with_redirect` (lib.rs) после `RequestCompleted`, до CORS/статус-ветвления; юнит-тесты + интеграционный тест `global_response_started_intercept_pauses_after_headers_until_resolved`.
- `crates/driver`: `InterceptedRequest` получил `phase`, `status`; `crates/shell/src/app/about_to_wait.rs` проброс.
- `crates/bidi-server/src/protocol.rs`: `network.continueResponse` (реальный resolve; неизвестный id → `no such request`; override-поля → `unsupported operation`), `continueWithAuth` → `no such request`, `addIntercept{authRequired}` → `unsupported operation`, неизвестная фаза → `invalid argument`, событие `network.responseStarted` (isBlocked, response.status). `cargo test -p lumen-bidi-server` — 121 ок.
- `tests/wpt/verify_devx20_bidi_network.py` — написан, ещё НЕ запускался (нужен dev-release бинарь + venv).

Найдено: `storage.getCookies|setCookie|deleteCookies` в protocol.rs работают только по `BidiState.cookies` (память) и не касаются jar живого окна; `value` принимается только строкой (по спецификации — BytesValue `{type:"string",value}`).

Осталось:
1. Подключить storage.* к живому jar: `lumen_storage::CookieJar::list(top_level_site, now)` (нет метода «все cookie»); `LiveCookie` + `AutomationCommand::{ListCookies,SetCookie,DeleteCookie}` / `AutomationReply::Cookies` в `crates/driver/src/types.rs`; методы `BrowserSession` (по умолчанию «нет живого jar» → in-memory) и `LiveWindowSession`; обработчик в `crates/shell/src/app/about_to_wait.rs` (jar — `self.lumen.active_cookie_jar()`, как у `document.cookie`); в protocol.rs принимать `value` и строкой, и BytesValue, отдавать BytesValue; `storage_get_cookies` сейчас берёт `&BidiState` — нужен `&mut`.
2. `cargo test -p lumen-network intercept`, `cargo test -p lumen-driver`, `cargo check -p lumen-shell`, затем `cargo clippy -p lumen-bidi-server -p lumen-network -p lumen-driver --all-targets -- -D warnings`.
3. Собрать `dev-release` (`LUMEN_PROFILE=dev-release`) и прогнать `tests/wpt/verify_devx20_bidi_network.py` и `verify_devx6_bidi_scenarios.py` против живого окна; поправить скрипт по факту.
4. doc-sync: `docs/automation.md` §WebDriver BiDi (строка про «accepted-and-stored only» — заменить: responseStarted/continueResponse живые; authRequired — unsupported operation; storage.* — по результату п.1), `CAPABILITIES.md` (строка DEVX-6, ~330), модульные комментарии в `crates/driver/src/lib.rs`.
5. Закрытие: гейт, `/lumen-task-finish`, в `ROADMAP.md` DEVX-20 → done, удалить указатель из `STATUS-P2.md` (в main-репо он `ROADMAP.md:1243`).
