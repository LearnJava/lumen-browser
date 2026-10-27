# BUG-1742 — регистрация сервис-воркера: таймерная имитация вместо Register/Update/Install, `updatefound` до резолва `register()`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_sw_container.register`, `_sw_run_lifecycle`, `_sw_make_registration`; `crates/js/src/sw_worker.rs` — install/activate в изоляте SW)

## Симптом

```js
const reg = await navigator.serviceWorker.register("resources/empty-worker.js",
                                                   {scope: "resources/scope/x"});
reg.scope;            // "resources/scope/x" — сырая строка, не абсолютный URL
reg.installing;       // ServiceWorker, но `updatefound` уже был: он диспатчится синхронно внутри register()
await reg.update();   // всегда resolve(undefined), скрипт не перечитывается
await reg.unregister(); await reg.unregister(); // true, true — второй должен быть false
```

`_sw_run_lifecycle` (`web_api_shim_mid_b.js:548`) — имитация на `setTimeout(0)`:

- `updatefound` диспатчится до того, как `register()` вернул промис, поэтому `wait_for_update(t, reg)` из `test-helpers.sub.js` ждёт вечно (TIMEOUT `registration-service-worker-attributes`, `update*`, `ServiceWorkerGlobalScope/update`);
- состояния `installed`/`activating`/`activated` выставляются без ожидания скрипта: скрипт воркера скачивается только **после** фейковой активации. Поэтому ошибка парсинга, ошибка `install`/`waitUntil`, неверный MIME (`text/plain`, `application/json`, без типа) и битый chunked-ответ не отклоняют регистрацию (`registration-script`, `registration-mime-types`, `oninstall-script-error`). `activating` и `activated` ставятся в одном тике (`synced-state`);
- scope не резолвится: по умолчанию `'/'`, а не `URL("./", scriptURL)` (`register-default-scope`); явный scope хранится сырой строкой, без нормализации `./`, без снятия фрагмента и без percent-encoding (`registration-scope`, `registration-basic`). Нет max scope и заголовка `Service-Worker-Allowed` (`Service-Worker-Allowed-header`, `registration-security-error`). Ключ реестра — та же сырая строка, отсюда и идентичность `https://localhost:18443does/not/exist` из [BUG-1226](BUG-1226-OPEN.md);
- повторный `register()` с тем же скриптом возвращает существующую регистрацию, у которой `installing === null`. Тест падает на «wait_for_state needs a ServiceWorker object» или «reading 'scriptURL' of null» (`skip-waiting`, `register-same-scope-different-script-url`). Смена скрипта на той же scope не ставит новый воркер в `waiting`;
- `update()` — заглушка (`:522`): нет byte-check, нет `updateViaCache`, нет отказа на отсутствующий `importScripts`. Обновление после навигации или через сутки не происходит;
- `unregister()` дважды даёт `true`. `getRegistration()` ищет по префиксу сырой строки. Clear-Site-Data регистрации не снимает.

## Как найдено

WPT-SW-SCOUT, кластер C (`docs/wpt-vendor-notes/service-workers.md`): 33 id (17 TIMEOUT, 16 OK с провалами), 56 из 167 сабтестов, потеряно 27.3 балла (8.2 п.п.). Представители: `registration-service-worker-attributes.https.html` (TIMEOUT на `wait_for_update`) и `register-default-scope.https.html` (`expected ".../resources/" but got "/"`).

## Что делать

Заменить имитацию алгоритмами SW §Jobs:

1. `register()` резолвит scope и скрипт относительно базового URL документа. Scope по умолчанию — `./` от скрипта. Ключ реестра — сериализованный URL scope без фрагмента.
2. Job queue на scope: Register → Update → Install → Activate.
3. Update скачивает скрипт с проверкой MIME (JavaScript), max scope и `Service-Worker-Allowed`. При ошибке сети или парсинга промис отклоняется, а регистрация удаляется, если она новая.
4. Install исполняет скрипт в изоляте `sw_worker.rs` и ждёт `install` с `waitUntil`. Состояния `installed` → `activating` → `activated` берутся из ответов изолята; `statechange` и `updatefound` идут задачами после резолва `register()`.
5. `update()` выполняет byte-check главного скрипта и импортов. `unregister()` возвращает `false` для уже снятой регистрации.

## Как проверить

`run_corpus.py --prefixes service-workers/service-worker --processes 6`: `registration-*`, `register-*`, `update*`, `unregister*`, `skip-waiting*`, `synced-state` без TIMEOUT. Кластер C в заметке пересчитать.
