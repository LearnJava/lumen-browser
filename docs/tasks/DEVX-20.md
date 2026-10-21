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
