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

## Прогресс (P3, срез 2026-11-17)

Сделано в `web_api_shim_mid_b.js` / `sw_worker.rs` / `sw_clients.rs` / `sw_interceptor.rs`:

- **Scope резолвится и становится абсолютным ключом** (§9.2 Start Register): явный scope — от API base URL документа, по умолчанию — `URL("./", scriptURL)`; фрагмент снят; ключ реестра = сериализованный URL. Чинит `register-default-scope`, `registration-scope`, `getRegistration`-cleanup всех хелперов, scope-ключ изолята/интерцептора/claim.
- **Path restriction + `Service-Worker-Allowed`** (§Update fetch hook): scope-путь должен лежать под max scope (скриптовая директория или хедер), иначе SecurityError. `registration-security-error` 8/8, `Service-Worker-Allowed-header` 7/8.
- **MIME-проверка для classic и module** → SecurityError; полный HTML-список JS MIME (`text/javascript1.x`, `text/jscript`, `text/livescript`, `text/x-*`…). `registration-mime-types` 32/39 → 37/39.
- **Отклонение редиректа скрипта** → SecurityError.
- **`unregister()` дважды → `false`**; `updateViaCache` (enum-валидация + хранение); `getRegistration` — абсолютный longest-prefix + SecurityError на кросс-ориджин; fast-path повторного `register()` по §Register 4.1.
- **`importScripts` изолята резолвится от URL скрипта** через `set_global` (не eval-присваивание), интерцептор и `claim()` матчат абсолютный scope по его пути.

Результат на кластере C (53 файла): **15.65% → 42.88%** (8/51 → 22/51 id), TIMEOUT 23 → 9.

**Срез 2026-11-18 (P3, job queue на scope):** конкурентные `register()` на одном scope больше не перезаписывают `installing` — в `web_api_shim_mid_b.js` добавлена per-scope FIFO-очередь `_sw_enqueue_scope`. Первый job на scope выполняется инлайн (одиночный `register()` по-прежнему заполняет `_sw_registrations` синхронно, как и раньше — тесты/WPT-харнесс читают его сразу), а второй и последующие конкурентные `register()` ждут, пока promise предыдущего не осядет, и только затем ставят своего worker в `installing`. Регресс-тест `sw_register_concurrent_same_scope_serializes` (в `v8_events_cache.rs`) гонит два `register()` на одном scope и проверяет, что первый резолвится с `installing.scriptURL === script1`, а второй — с `script2`. Полный `sw_`-набор — 85/85.

**Срез 2026-11-18 (P3, install → `redundant`):** провал `waitUntil` в `install` теперь роняет worker в `redundant`, а не ведёт его в `activated`. В `sw_worker.rs` `drain_extend_v8` возвращает, реджектился ли install-`waitUntil` (`_sw_extend_error__`), и после install-дрейна результат уходит в страницу новым каналом `sw_clients::push_install_result` (fan-out по origin, как `push_eval_result`); в `web_api_shim_mid_b.js` `_sw_run_lifecycle` разбит на `_sw_finish_install`, который держит worker в `installing` до прихода результата изолята (`_lumen_deliver_sw_install_results` ставит `_installOk`/будит отложенный колбэк) и на `ok=false` зовёт `_sw_fail_eval` (redundant + снятие регистрации без `active`). Регресс-тесты `sw_install_waituntil_reject_makes_worker_redundant` и `sw_install_waituntil_fulfill_reaches_activated` (в `v8_events_cache.rs`), полный `sw_`-набор — 87/87.

**Срез 2026-11-18 (P3, script resource map):** byte-check страницы и `importScripts` изолята больше не ходят в сеть дважды. `_sw_script_bytes` теперь возвращает `{bytes, imports}` — тексты импортов (absolute URL → текст) кладутся в `reg._importScripts`; `_sw_start_isolate`/`_lumen_sw_activate_script` (новый 6-й аргумент `script_resources_json`) передают их в изолят, а `importScripts` изолята читает `_sw_script_resources_json__` и исполняет уже скачанный текст вместо `_lumen_sw_net_fetch` (network остаётся fallback'ом для импортов, которых нет в карте). Чинит `update-missing-import-scripts` «Initialize global state» — stash-гвард `.py` больше не съедается вторым запросом. Регресс-тест `sw_import_scripts_reuses_passed_resource_map` (в `sw_worker.rs`), полный `sw_`-набор — 105/105.

Осталось (глубже, отдельные срезы):

- **`Service-Worker: script` хедер** — не шлётся (`service-worker-header` 1/1 → 0/1, честный FAIL вместо ложного PASS).
- **Модель controller** — `controller` существующих клиентов должен переживать `unregister`+повторную регистрацию (`unregister-then-register` «does not resurrect»).
