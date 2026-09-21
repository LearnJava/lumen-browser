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

### Срез 3 — S — permissionState через реальный permission-стор
Связать `permissionState()`/`subscribe()` с механизмом разрешений (notifications/push):
`'prompt'` по умолчанию, `'denied'` блокирует subscribe. Убрать хардкод `'granted'`.

### Срез 4 — M — Push-канал доставки (WebPush, RFC 8030)
Endpoint = реальный push-сервис (или локальный relay для теста). Поднять подписку на
доставку, принимать зашифрованные сообщения, расшифровать (RFC 8291 aes128gcm).
Это самый крупный срез; при отсутствии внешнего push-сервиса — mock-relay в тестах.

### Срез 5 — S — Диспатч `push`-события в Service Worker
При приходе сообщения — сконструировать `PushEvent` (`data`: PushMessageData) и
диспатчить в SW (`worker.rs`). `pushsubscriptionchange` при ротации подписки.

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
- [ ] `permissionState` связан с permission-стором (не хардкод `granted`).
- [ ] (полный DoD) Доставка WebPush + `push`-событие в SW; при отсутствии сервиса — mock.
- [ ] Тесты зелёные; `CAPABILITIES.md`/`ROADMAP.md`/`subsystems/` обновлены.
