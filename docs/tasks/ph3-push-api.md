# Задача: Push API (через Service Worker)

**Developer:** P1
**Ветка:** `p1-push-api`
**Размер:** M
**Крейты:** `lumen-js`, `lumen-network`, `lumen-storage`

## Goal
W3C Push API L1: `registration.pushManager.subscribe()` создаёт реальную подписку с
persist-хранением, поднимает push-канал доставки (WebPush, RFC 8030/8291) и диспатчит
`push`-событие в Service Worker при приходе сообщения.

## Current state (сверено с кодом 2026-07-05) — PARTIAL, подтверждено
- `crates/js/src/push_api.rs:24-137` — JS-шим `PushManager`/`PushSubscription`:
  - `subscribe(options)` (`:72-105`) — валидирует `userVisibleOnly`/`applicationServerKey`,
    генерит **статический фейковый** endpoint `https://push.lumen.local/v1/subscription/<rand>`
    (`:88`) и **мок-ключи** `p256dh`(65)/`auth`(16) как пустые ArrayBuffer (`:91-94`).
  - `getSubscription()` (`:109-112`) — возвращает in-memory `this.subscription` или null.
  - `permissionState()` (`:116-118`) — **всегда `'granted'`** (заглушка).
  - `unsubscribe()` (`:56-62`), `getKey()` (`:34-43`), `toJSON()` (`:46-52`).
- `crates/js/src/push_api.rs:99-101` — вызывает нативный `_lumen_push_subscribe(endpoint,
  userVisibleOnly)` **если он определён** — но самого биндинга в Rust НЕТ
  (grep `_lumen_push_subscribe` → только этот файл, определения на Rust-стороне нет).
- Подписки живут **только в памяти** одного JS-контекста (`this.subscription`, `:67`),
  не персистятся, теряются при перезагрузке.
- `permissionState` не связан с реальным permission-стором.
- **Нет** доставки: ни WebPush-endpoint, ни `push`-события в Service Worker,
  ни расшифровки (RFC 8291), ни VAPID/`applicationServerKey`-обработки.

## Entry points
- `crates/js/src/push_api.rs:18` — `init_push_api` (установка шима).
- `crates/js/src/push_api.rs:72` — `subscribe` (заменить фейк-endpoint/ключи).
- `crates/js/src/push_api.rs:100` — вызов `_lumen_push_subscribe` (нужен Rust-биндинг).
- Service Worker слой — `crates/js/src/worker.rs` (диспатч `push`-события).
- Permission-стор — искать существующий permissions-механизм (`crates/network/src/
  permissions_policy.rs`, `crates/js/src/permissions_policy.rs`) для `permissionState`.
- Persist — `lumen-storage` (по ADR-012 SQLite; подписки = долгоживущие → SQLite).

## Срезы (декомпозиция)
### Срез 1 — S — Нативный биндинг + persist подписок — **сделано 2026-09-21 (P1)**
Реализовано: `lumen_core::ext::PushBackend` (`crates/core/src/ext.rs`, рядом с `SwBackend`/
`CacheBackend`) — best-effort трейт `push_subscribe`/`push_get`/`push_unsubscribe`, ключ
`(origin, scope)`. `lumen_storage::PushStore` (`crates/storage/src/push_store.rs`) — тонкий
адаптер над уже существовавшей (закоммичена ранее, но никуда не подключена)
`crates/storage/src/push_subscriptions.rs::PushSubscriptions` (SQLite, `UNIQUE(origin, scope)`).
`crates/js/src/push_api.rs::install_push_api_v8` теперь принимает
`Option<Arc<dyn PushBackend>>`, регистрирует три натива через `register_native`/`into_v8_fn2`/
`into_v8_fn6`; JS-шим кодирует mock-ключи в base64 (`_push_ab2b64`/`_push_b642ab`, поверх
`btoa`/`atob`) для передачи через натив. `getSubscription()` читает из стора (не из
in-memory поля) — подписка переживает пересоздание JS-контекста (юнит
`test_subscribe_then_reload_context_sees_persisted_subscription`, 10/10 тестов `push_api`
зелёные, 5/5 `push_store`).

Параметр `push_backend: Option<Arc<dyn PushBackend>>` доведён до `install_dom` →
`run_scripts_with_dom` → `render_bytes`/`parse_and_layout` → `FrameLoadEnv` (та же позиция,
что у `cache_backend`, во всех ~100 сигнатурах/call-сайтах, включая тесты `lumen-js`/
`lumen-shell`).

**Проводка до живой вкладки — сделано 2026-09-21 (P1, ветка p1-pushapi-tab-wiring):**
`push_store: Arc<lumen_storage::PushStore>` — session-scoped поле `App`
(`crates/shell/src/lumen/state.rs`, рядом с `cache_store`; `PushStore` сам партиционирует по
`(origin, scope)`, поэтому пер-табовая обёртка вроде `SwStore` не нужна). Инициализация в
`window_mode.rs` (`PushStore::new(PushSubscriptions::open_in_memory())`, тот же паттерн, что
`cache_store`/`cookie_jar` — in-memory на сессию, без файла профиля). Главный навигационный
путь `app/user_event.rs` теперь строит `Some(Arc::clone(&self.push_store) as Arc<dyn
PushBackend>)` вместо хардкода `None` — `_lumen_push_*`-натив достижим из живой вкладки.
Fallback-путь `page_source.rs::PageSource::load` (headless/тесты без `GpuSession`-окна)
по-прежнему передаёт `None` — как и `cache_backend` там же, у него нет доступа к
session-scoped стораджам `App`, это не push-специфичный пробел.

### Срез 2 — S — Реальные ключи подписки (ECDH P-256) — **сделано 2026-09-21 (P1)**
`generate_push_keys` (`crates/js/src/push_api.rs`) — P-256 keypair (OS CSPRNG seed →
`p256::SecretKey::from_slice`, тот же паттерн, что `subtle_crypto.rs`'s `"ECDH"`
`generateKey`) + 16-байтный auth-секрет. `p256dh` = uncompressed SEC1 point (65B,
ведущий байт `0x04`), `auth` = 16 случайных байт (RFC 8291). Приватный скаляр
никогда не проходит через JS: нативный `_lumen_push_subscribe(origin, scope,
endpoint, userVisibleOnly)` генерирует ключи сам и возвращает JS только
`[p256dhBase64, authBase64]`; `getKey()` отдаёт реальные `ArrayBuffer` из них.
Приватный ключ персистится в новой колонке `push_subscriptions.private_key`
(v2-миграция) через расширенный `PushBackend::push_subscribe` — задел под
расшифровку push-сообщений в срезе 4; `push_get`/`getSubscription()` его не
возвращают. 4 юнита на `generate_push_keys` (SEC1-точка, длина auth, различность
между вызовами, валидность приватного скаляра) + 1 end-to-end (`getKey('p256dh')`
через живой `PushManager`), 19/19 тестов `push_api`, 14/14 `lumen-storage`
push-тестов.

### Срез 3 — S — permissionState через реальный permission-стор — **сделано 2026-09-21 (P1)**
`lumen_storage::PermissionKind::Push` (новый вариант, `"push"`) поверх уже существовавшей
(но никуда не подключённой) SQLite-таблицы `permissions` (`crates/storage/src/permissions.rs`).
`lumen_core::ext::PushBackend` получил `push_permission_state(origin) -> String`
(`"granted"`/`"denied"`/`"prompt"`) и `push_set_permission(origin, state)`; `PushStore`
реализует оба через `Permissions::query`/`set` (второй конструкторский параметр
`Arc<Permissions>` — своя in-memory таблица на сессию, тот же паттерн, что `cache_store`).
JS: новый нативный `_lumen_push_permission_state(origin)`, `permissionState()` отдаёт его
результат вместо хардкода `'granted'`; `subscribe()` при `'denied'` отклоняет промис
`DOMException(..., 'NotAllowedError')`, `'prompt'` (дефолт — нет решения на записи) по-прежнему
пропускает subscribe. `navigator.permissions.query({name:'push'})` (`permissions.rs`)
намеренно не тронут — остаётся статическим `DENIED`: реальной доставки push всё ещё нет
(срезы 4-5), а этот флаг должен отражать, что вызов действительно что-то делает.

### Срез 4 — M — Push-канал доставки (WebPush, RFC 8030) — **сделано 2026-09-21 (P1)**
`crates/storage/src/push_crypto.rs` — RFC 8291 aes128gcm decrypt: ECDH(P-256) shared
secret → HKDF (RFC 5869, hand-rolled — same generic shape as `subtle_crypto.rs::
hkdf_derive`, reimplemented because the two crates can't share a private helper) →
CEK/nonce → AES-128-GCM decrypt, RFC 8188 §2.1 single-record framing (salt/record-size/
keyid header, padding delimiter strip). No `ece`/webpush crate vendored (ADR-027 §5) —
built from the already-vendored `p256`+`hmac`+`sha2`+`aes-gcm` (new `p256` dependency
added to `lumen-storage`, same version/features already in `lumen-js`/`lumen-network`).
`crates/storage/src/push_messages.rs` — new `PushMessages` table, in-memory FIFO queue
of decrypted plaintexts keyed by `subscription_id` (an undelivered message has no
cross-restart value, unlike the subscription itself). `PushBackend` (`lumen-core::ext`)
gained `push_deliver(origin, scope, payload) -> bool` (looks up the subscription,
decrypts `payload` with its stored `private_key`/`auth`, enqueues the plaintext) and
`push_take_pending(origin, scope) -> Option<Vec<u8>>` (FIFO pop), both implemented on
`PushStore`. No external push service exists to interop against — `push.lumen.local`
endpoints (срез 1) are not reachable from anywhere — so, as this срез anticipated,
correctness is verified by a mock relay: `push_crypto::encrypt_for_test` (test-only)
produces a spec-shaped ciphertext that `push_deliver`'s tests decrypt via the real
`decrypt()` path. 15 new unit tests (8 `push_crypto` incl. wrong-key/corrupted-ciphertext/
malformed-input rejection, 6 `push_messages` FIFO/isolation, 5 `push_store` end-to-end
deliver→take-pending). `cargo clippy -p lumen-storage -p lumen-core --all-targets -D
warnings` and `-p lumen-js --all-targets --features v8-backend -D warnings` green.
Remaining: nothing yet calls `push_deliver`/`push_take_pending` from JS — dispatching the
plaintext into the Service Worker as a `push` event is срез 5.

### Срез 5 — S — Диспатч `push`-события в Service Worker — **сделано 2026-09-21 (P1)**
`lumen_core::ext::SwWorkerHandle::tx` расширен с одноцелевого канала фетч-запросов
до `SwWorkerMessage` (`Fetch`/`Push`/`PushSubscriptionChange`) — то, что реально
исполняет каждое сообщение, остаётся единственным потоком, владеющим V8-изолятом
SW (`crates/js/src/sw_worker.rs`, не `worker.rs` — тот для dedicated/shared воркеров).
`_sw_fire_push(payloadB64)` строит `PushEvent`/`PushMessageData` (`.text()`/`.json()`/
`.arrayBuffer()`/`.blob()`, байты через тот же base64-конвейер, что и весь остальной
файл) и вызывает зарегистрированные `push`-обработчики; `_sw_fire_push_subscription_change`
зеркалит это для `PushSubscriptionChangeEvent` (`oldSubscription`/`newSubscription`,
минимальный объект с `endpoint`/`getKey()`). `dispatch_push_v8`/
`dispatch_push_subscription_change_v8` — тонкие Rust-обёртки (глобалы + `eval`), тот же
паттерн, что `dispatch_fetch_v8`.

Реального push-сервиса нет (см. срез 4) — точка входа "сообщение пришло" —
`_lumen_push_deliver_test(origin, scope, payloadB64)` (`crates/js/src/push_api.rs`,
не часть W3C Push API, шим её не оборачивает): декодирует payload, зовёт
`PushBackend::push_deliver` (RFC 8291 расшифровка, срез 4) → `push_take_pending`
(плейнтекст) → шлёт `SwWorkerMessage::Push` через `SwWorkerStore` на `(origin, scope)`.

`pushsubscriptionchange`: нет реального push-сервиса, который мог бы ротировать
подписку сам по себе, поэтому стенд-ин триггер — повторный `subscribe()` для того же
`(origin, scope)`, у которого уже была подписка (`push_get` до `push_subscribe`
детектирует замену); первый (не заменяющий) `subscribe()` событие не шлёт.

`install_push_api_v8` получил третий параметр `sw_worker_store: Option<SwWorkerStore>`
(та же карта, что `install_service_worker`) — `None` (headless/без SW) делает и
диспатч push, и pushsubscriptionchange безопасными no-op, как остальные push-натива.
2 новых теста `crates/storage/src/sw_interceptor.rs` (тип канала), 5 новых
`crates/js/src/sw_worker.rs` (PushEvent/PushMessageData round-trip, отсутствие
обработчика — no-op, PushSubscriptionChangeEvent, полный цикл через реальный SW-поток
Push→Fetch-маркер), 4 новых `crates/js/src/push_api.rs` (resubscribe → диспатч с
верным old/new endpoint, первый subscribe → без диспатча, `_lumen_push_deliver_test`
без подписки/без бэкенда → `false`). `cargo clippy -p lumen-core -p lumen-storage -p
lumen-js --all-targets --features lumen-js/v8-backend -D warnings` и
`cargo check --workspace --all-targets --features lumen-js/v8-backend` зелёные.
Остаток: `showNotification`/уведомление из `push`-обработчика — отдельная от Push API
`Notifications`-поверхность, уже реализована (`P3-notifications`), но не проверялась
именно в связке с `push`-событием; реальный WebPush-транспорт (не mock) вне скоупа
Push API — нет push-сервиса, который мог бы прислать сообщение с интернета.

### Срез 6 — XS — Доки
`CAPABILITIES.md` (JS/ServiceWorker) 🟡; `ROADMAP.md:165` уточнить остаток;
`subsystems/js.md`/`subsystems/storage.md`.

## Tests
- `lumen-js`: subscribe возвращает подписку с реальными ключами; getSubscription persist
  (срез 1–2); permissionState отражает стор (срез 3).
- `lumen-js`/integration: mock-relay доставляет зашифрованное сообщение → `push`-событие
  с корректным `event.data.text()` (срез 4–5).
- Регресс: существующие 7 тестов `push_api.rs:159-254` продолжают проходить (форма API).

## Definition of done
- [x] Нативные push-биндинги реализованы, подписки persist в SQLite.
- [x] Реальные P-256 ключи `p256dh`/`auth`.
- [x] `permissionState` связан с permission-стором (не хардкод `granted`).
- [x] (полный DoD) Доставка WebPush + `push`-событие в SW; при отсутствии сервиса — mock.
- [ ] Тесты зелёные; `CAPABILITIES.md`/`ROADMAP.md`/`subsystems/` обновлены (срез 6).
