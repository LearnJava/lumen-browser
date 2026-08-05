# BUG-657 — `ServiceWorkerRegistration` global class никогда не определяется в продакшн-инсталляции V8: реальные регистрации `navigator.serviceWorker.register()` — plain-объекты без `pushManager`/`sync`/`showNotification`/`periodicSync`/`backgroundFetch`/`cookies`/`index`, вопреки заявлению BUG-549 об обратном

**Статус:** OPEN
**Компонент:** js — `crates/js/src/dom.rs` (`_sw_make_registration`, ~строка 4843: `Object.assign({...}, et)` без привязки к какому-либо прототипу) + все семь модулей, вешающих себя на `ServiceWorkerRegistration.prototype` (`push_api.rs:124`, `background_sync.rs:66`, `periodic_sync.rs:74`, `background_fetch.rs:116`, `cookie_store.rs:229`, `content_index.rs:73`, `notifications_bindings.rs:280`) — каждый guard`ится `if (typeof ServiceWorkerRegistration !== 'undefined')`, но ни один модуль (включая сам `content_index.rs`, чей док-комментарий утверждает обратное) не определяет этот глобал в продакшн V8-инсталляции; единственные определения — локальные тестовые стабы внутри `#[cfg(all(test, feature = "v8-backend"))] mod tests` каждого файла
**Найден:** P2, WPT-VENDOR-push-api (2026-08-05), живая проба через `--mcp-live-port` после того, как `run_report.py --all --root push-api --recursive` не дал сигнала (все 4 исполнившихся id упёрлись в TLS-гэп `UnknownIssuer`, см. ROADMAP.md WPT-VENDOR-push-api)

## Механизм

`content_index.rs:14` документирует инвариант: «Must run after the service-worker shim so that `ServiceWorkerRegistration` is already defined on `globalThis`» — подразумевая, что какой-то другой модуль определяет базовый класс первым. Это утверждение ложно для реального V8-пути:

- `navigator.serviceWorker` реализован в `crates/js/src/dom.rs` (гигантский `WEB_API_SHIM`, `_sw_container`/`_sw_make_registration`/`_sw_run_lifecycle`, строки ~4843-5000) — `grep -n "ServiceWorkerRegistration" crates/js/src/dom.rs` даёт **ноль совпадений**.
- `_sw_make_registration(scope, scriptUrl)` строит регистрацию как `Object.assign({scope, scriptURL, ..., unregister: ...}, _sw_make_event_target())` — обычный объектный литерал, никак не связанный ни с каким прототипом.
- Семь Phase-0 модулей (`push_api`, `background_sync`, `periodic_sync`, `background_fetch`, `cookie_store`, `content_index`, `notifications_bindings`) вешают свои члены на `ServiceWorkerRegistration.prototype` **только если** `typeof ServiceWorkerRegistration !== 'undefined'` — а поскольку ни один production-путь этот глобал не создаёт, каждый такой guard молча не срабатывает.
- Единственные места, где `ServiceWorkerRegistration` вообще существует как identifier — юнит-тесты каждого модуля по отдельности (`var ServiceWorkerRegistration = function() {};` внутри `with_push_api`/`with_content_index`/… helper), что и объясняет, почему юнит-тесты (`test_service_worker_registration_has_push_manager` и аналоги) зелёные: они тестируют шим на изолированном фейковом классе, а не на реальном объекте, который создаёт `navigator.serviceWorker.register()`.
- `BUG-549-FIXED.md` (закрыт 2026-08-04) прямо утверждает: «`ServiceWorkerRegistration.prototype` itself (bare constructor, defined in `content_index.rs`, V8-ported) exists and real registrations are created — only these four sub-APIs attached to it are missing» — это утверждение **не подтверждается кодом** ни тогда, ни сейчас: `content_index.rs`'s V8-install (`install_content_index_api_v8`) не определяет базовый класс, только читает его тем же guard'ом, что и остальные шесть модулей.

## Живое воспроизведение

Через `--mcp-live-port` (`file://` страница с одним `<script>`, `navigator.serviceWorker.register('/sw.js', {scope: '/'})`):

```json
{
  "hasPushManager": "undefined",
  "hasSync": "undefined",
  "hasShowNotification": "undefined",
  "isSWRInstance": "no-global-class",
  "ownKeys": ["scope","scriptURL","updateViaCache","installing","waiting","active",
              "onupdatefound","update","unregister","addEventListener",
              "removeEventListener","dispatchEvent"]
}
```

`typeof ServiceWorkerRegistration` на реальной странице — `"undefined"`; на зарегистрированном объекте отсутствуют `pushManager`, `sync`, `showNotification` (и по построению — `periodicSync`, `backgroundFetch`, `cookies`, `index`, ни один из которых не входит в `ownKeys`).

## Симптом

Любой сайт (или WPT-тест — `push-api/push-event.https.any.js`, `push-api/permission.https.html`, `background-sync/*`, `periodic-background-sync/*`, `content-index/*`, `notifications/*` service-worker-scoped варианты), выполняющий канонический паттерн

```js
navigator.serviceWorker.register('/sw.js').then(reg => reg.pushManager.subscribe(...))
```

падает с `TypeError: Cannot read properties of undefined (reading 'subscribe')` — не `NotSupportedError`/rejected Promise, как для настоящего Phase-0-стаба, а сырой `TypeError` до того, как код вообще доходит до Phase-0-логики модуля. `typeof PushManager === 'function'` (presence-detection на глобальном классе, то, что чинил BUG-549) остаётся `true` — маскирует то, что **инстанс-уровневая** проводка полностью отсутствует. Тот же класс ошибки, что BUG-368 (`innerHTML` — round-trip зелёный, реальное DOM не то) и BUG-386 (permissions `query()`): юнит-тест зелёный на изолированном фейке, реальный путь — другой.

## Что НЕ является причиной

Не TLS-гэп (описанный в ROADMAP.md WPT-VENDOR-push-api) — тот блокирует сетевую навигацию до JS вообще, здесь речь о живой странице без TLS. Не про Push-уведомления как таковые (реальная доставка push — вне скоупа 🚫, нужен push-сервис) — баг про то, что даже Phase-0-заглушка (`subscribe()` → resolved Promise с моковым endpoint) недостижима с реального объекта регистрации.

## Предлагаемый фикс

Определить `function ServiceWorkerRegistration() {}` + `globalThis.ServiceWorkerRegistration = ServiceWorkerRegistration;` один раз в продакшн-инсталляции (например, в `dom.rs` рядом с `_sw_make_registration`, до вызова всех семи `install_v8!(...)` модулей в `v8_runtime.rs`), и переключить `_sw_make_registration`/`_sw_make_worker` на `Object.setPrototypeOf(reg, ServiceWorkerRegistration.prototype)` (или строить через `Object.create(ServiceWorkerRegistration.prototype)` вместо `Object.assign({...}, et)`), чтобы прототипные методы семи модулей реально становились видны на живых регистрациях.
