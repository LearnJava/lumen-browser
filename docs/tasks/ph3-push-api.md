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
`lumen-shell`). **Не сделано в этом срезе:** ни один реальный call-сайт (`page_source.rs`,
`app/user_event.rs`) не строит настоящий `Arc<dyn PushBackend>` — оба передают `None` с
явным комментарием. У `sw_backend`/`cache_backend` есть пер-табовое поле
(`frames.rs`/`lumen/state.rs`, пересоздаётся в `tabs_cmd.rs`/`page_snapshot.rs`/
`window_mode.rs`); у `push_backend` такого поля пока нет — это первая задача среза 2 или
отдельного среза до него, иначе подписки останутся недостижимы из живой вкладки несмотря
на полностью рабочий и протестированный слой ниже.

### Срез 2 — S — Реальные ключи подписки (ECDH P-256)
Генерить настоящую P-256 keypair (переиспользовать `p256` из WebAuthn/subtle_crypto),
`p256dh` = uncompressed public point (65B), `auth` = 16 случайных байт (RFC 8291).
`getKey()` отдаёт реальные ArrayBuffer. Юнит: `p256dh` — валидная SEC1-точка.

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
- [ ] Нативные push-биндинги реализованы, подписки persist в SQLite.
- [ ] Реальные P-256 ключи `p256dh`/`auth`.
- [ ] `permissionState` связан с permission-стором (не хардкод `granted`).
- [ ] (полный DoD) Доставка WebPush + `push`-событие в SW; при отсутствии сервиса — mock.
- [ ] Тесты зелёные; `CAPABILITIES.md`/`ROADMAP.md`/`subsystems/` обновлены.
