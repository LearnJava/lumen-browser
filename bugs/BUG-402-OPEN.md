# BUG-402 — HSTS enforcement never wired into any real `HttpClient`

**Статус:** OPEN
**Компонент:** network/shell (`crates/network/src/lib.rs::with_hsts`, `crates/shell/src/config.rs::apply_http`)
**Найден:** P2, WPT-VENDOR-hsts (2026-07-28), при отсутствии сигнала от прогона (единственный тест
категории — `.tentative.sub.html`, зависящий от невендоренного `/common/get-host-info.sub.js` → TIMEOUT
инфраструктурного класса, не находка) — пробой по правилу «пробуй даже без сигнала»

## Симптом

RFC 6797 HSTS полностью реализован и юнит-протестирован на уровне `lumen-network`/`lumen-storage`
(`crates/network/src/hsts.rs`, `crates/storage/src/hsts.rs`: парсинг `Strict-Transport-Security`,
per-host store с `includeSubDomains`, pre-request HTTP→HTTPS upgrade, HSTS Preload List), но
**`HttpClient::with_hsts(...)` не вызывается ни в одном продакшн-пути браузера** — ни в
`crates/driver/src/session.rs`/`winit_session.rs`, ни в `crates/shell/src/config.rs::apply_http`
(единственное место, которое централизованно конфигурирует реальный `HttpClient` — fingerprint
profile, TLS profile, HTTP/3, DoH, proxy — но не HSTS), ни в любом из прямых `HttpClient::new()` в
`shell/src/main.rs`/`download.rs`/`platform/audio_player.rs`. Грепом по всему воркспейсу
(`grep -rn "with_hsts\b" --include=*.rs .`) находки — только внутри `#[cfg(test)]` в самом
`network/src/lib.rs`.

Следствие: ни один реальный переход по HTTP на сайт, приславший `Strict-Transport-Security`
(включая ranний, встроенный HSTS Preload List — `get_preload_list()`, который тоже участвует
только в этой мёртвой ветке), не апгрейдится на HTTPS. Downgrade-атака (активный MITM понижает
`https://` до `http://` на первом визите или в промежутке) полностью работает против Lumen —
именно тот класс атаки, для защиты от которого существует HSTS.

`CAPABILITIES.md:158` при этом утверждает `✅ HTTP auth ..., HSTS (+ preload), ...` — тот же класс
доковой рассинхронизации, что BUG-368 (`innerHTML` помечен ✅ full read/write при том, что setter —
Phase 0 text-заглушка): реализация и её тесты существуют и проходят, но код никогда не выполняется
в реальном браузере.

## Причина

`HstsStore` (persistent SQLite) не имеет ни одной точки создания вне `#[cfg(test)]` —
`grep -rn "HstsStore::open\b" --include=*.rs .` не находит продакшн-вызовов вовсе. `with_hsts` —
builder-метод `HttpClient`, добавленный вместе с `hsts.rs`/интеграционными тестами
(`with_hsts_known_host_attempts_upgrade` и др., `network/src/lib.rs:6083+`), но подключение
к `apply_http`/фабрикам `HttpClient` в shell/driver, судя по всему, не было сделано отдельным
шагом — задача осталась «наполовину сделанной»: клиентская интеграция готова и протестирована,
storage-слой готов и протестирован, но связка `shell → storage::HstsStore::open(<путь>) →
Arc<dyn HstsEnforcement> → HttpClient::with_hsts(...)` отсутствует.

## Что нужно сделать

1. Завести persistent `HstsStore` рядом с другими storage-компонентами browser data dir
   (см. `feedback_browser_folder_storage` — `<exe_dir>/data/`, тот же паттерн, что adblock/DnsCache/
   SafeBrowsing из `CAPABILITIES.md:172`), одна на процесс.
2. Прокинуть `Arc<dyn HstsEnforcement>` через `AppConfig`/`apply_http` (или туда, где сейчас
   собираются DoH/proxy/TLS-профиль) во все продакшн-фабрики `HttpClient`, перечисленные в
   Симптоме — не только основной navigation-путь, но и `download.rs`/`audio_player.rs`/
   subresource fetch, иначе HSTS будет частично работать для одних запросов и не работать для
   других того же хоста.
3. Проверить, что `HstsStore::purge_expired` вызывается на каком-то cadence (сейчас есть только
   как публичный метод без вызывающей стороны — тот же паттерн незавершённого подключения).
4. Обновить `CAPABILITIES.md:158` на 🟡, пока связка не подтверждена работающей end-to-end (реальный
   HTTP→HTTPS upgrade после получения заголовка от живого HTTPS-сервера, не только unit-тест
   `HttpClient::with_hsts` с in-memory моком).

## Связанные

* [BUG-368](bugs/BUG-368-OPEN.md) — та же категория дефекта (реализация+тесты существуют,
  `CAPABILITIES.md` фиксирует ✅, но продакшн-путь либо не достигает кода, либо код — заглушка).
* `docs/wpt-status.md` категория `hsts`: единственный вендоренный тест
  (`third-party-subframe-hsts-upgrade.tentative.sub.html`) сам по себе непоказателен — он
  `.tentative.` (не финализированная в спеке проверка партиционирования HSTS-состояния по
  top-level site против bounce-tracking) и зависит от невендоренного `/common/get-host-info.sub.js`,
  поэтому TIMEOUT прогона не является ни подтверждением, ни опровержением этой находки — она
  получена независимым Rust-грепом продакшн call sites, не самим WPT-прогоном.
