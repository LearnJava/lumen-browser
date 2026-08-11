# BUG-402 — HSTS enforcement never wired into any real `HttpClient`

**Статус:** FIXED 2026-08-11 (P3)
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

## Исправление (2026-08-11, P3)

Добавлена недостающая точка подключения и вызвана из **всех** продакшн-фабрик клиента.

**1. Один store на процесс — `lumen_storage::hsts::shared_store(private)`** (`crates/storage/src/hsts.rs`).
`OnceLock<Option<Arc<HstsStore>>>`, ровно по образцу `shell::config::shared_http_cache`: persistent
SQLite в `<exe_dir>/data/hsts/hsts.db` (портативная папка браузера), а при `private = true`
(Tor / `no_persistent_state`) — in-memory. Резолвер `<exe_dir>/data` продублирован внутри
`lumen-storage` — крейт лежит ниже шелла в графе зависимостей и `shell::adblock::browser_data_dir`
позвать не может; тот же приём уже применён в `lumen-paint::backend_probe` и
`lumen-js::filesystem_access`. Тестируемое ядро вынесено в `open_shared_store(private, path)` —
без глобала, чтобы юнит-тест не замораживал `OnceLock`.

Почему **один** store, а не по одному на клиента: HSTS-policy — свойство хоста, а не запроса.
Разные store у навигации, subresource-fetch, загрузок и медиа означали бы, что один и тот же хост
апгрейдится через раз, в зависимости от того, какая половина браузера сходила туда первой.

**2. Деградация при ошибке диска — in-memory, а не «выключить».** Штатный паттерн соседа
(`build_http_cache` → `None`) здесь неверен: у кэша отказ стоит скорости, у HSTS — защиты от
downgrade-атаки. Preload-лист консультируется внутри `maybe_upgrade_url_to_https`, а та вызывается
**только когда store подключён**, поэтому «нет store» = «нет и встроенной preload-защиты», хотя она
не требует ни диска, ни предыдущего визита. `None` остаётся только если не открывается даже
in-memory SQLite.

**3. `purge_expired` получил вызывающую сторону** (п.3 заявки) — вызывается в `open_shared_store`
сразу после открытия. Открытие store — единственная точка входа, значит и естественная точка GC;
отдельный таймер не нужен, потому что просроченные записи и так игнорируются в `is_https_only`
(`expires_at > ?`), то есть покупает он только размер базы.

**4. Проводка во все продакшн-фабрики:**
* `shell::config::apply_http` — централизованный конфигуратор клиента (main.rs ×4, download.rs);
* `driver::session::build_http_client` и `driver::winit_session` — через новый
  `driver::types::with_shared_hsts`. Драйвер лежит ниже шелла и `apply_http` позвать не может, но
  берёт **тот же** процесс-глобальный store: шелл и автоматизация обязаны видеть одну политику;
* `shell::main.rs` (фоновая нить «сохранить на потом») и `shell::platform::audio_player` строили
  голый `HttpClient::new()` — переведены на `config::global().apply_http(...)`. Побочно это чинит
  и то, что медиа и read-later ходили мимо прокси, DoH и кэша.

Не подключён `crates/network/src/bin/network_service.rs` — `lumen-network` намеренно не зависит от
`lumen-storage` (ради чего и существует trait-граница `HstsEnforcement`), а главное, через этот
подпроцесс сегодня не идёт ни один запрос: `--network-service` порождает процесс и роняет
транспорт на пол (побочная находка, [BUG-769](BUG-769-OPEN.md)).

## Чем проверено

Заявка (п.4) требовала подтверждения end-to-end, а не только юнит-теста «`with_hsts` работает,
если его подключить». Проверено обоими направлениями протокола на собранном `dev-release`:

* **Запись policy.** `lumen.exe --dump-layout https://github.com/` с нуля создал
  `<exe_dir>/data/hsts/hsts.db` и записал туда `github.com` (`max-age=31536000; includeSubDomains;
  preload`) **и** `github.githubassets.com` — второй хост доказывает, что через тот же store идут и
  subresource-запросы, а не только навигация.
* **Апгрейд URL.** Локальный `python -m http.server 8901`, одна и та же команда
  `--dump-layout http://localhost:8901/index.html`, разница только в содержимом базы:
  * пустая база → страница грузится, layout содержит маркер;
  * строка `localhost` с живым `expires_at` → `→ GET https://localhost:8901/index.html` и падение
    на TLS-handshake (сервер-то plain HTTP).

  Это отделяет HSTS-апгрейд от серверного 301-редиректа, на который проверка «итоговый URL стал
  https» попалась бы: порт 8901 сохранён (RFC 6797 §8.3), сервер редиректов не шлёт вовсе.

Регрессия-гейт — не «HSTS работает», а «фабрика зовёт `with_hsts`»: добавлен интроспектор
`HttpClient::has_hsts()` и два теста на нём — `config::tests::apply_http_wires_hsts` (shell) и
`session::tests::build_http_client_wires_hsts` (driver). Именно этого наблюдения не хватало, чтобы
исходный разрыв был виден тестами: механизм был покрыт полностью, его вызов — никак.

## Урок

Полностью реализованная и зелёная подсистема может не исполняться ни разу. Тест на механизм
(«`with_hsts` апгрейдит http→https») и тест на проводку («фабрика клиента зовёт `with_hsts`») —
разные тесты, и второго не было. Признак класса: публичный builder-метод или публичный метод
обслуживания (`purge_expired`), у которого `grep` находит вызовы только под `#[cfg(test)]`.

## Связанные

* [BUG-769](BUG-769-OPEN.md) — побочная находка этого разбора: `--network-service` порождает
  подпроцесс, но `RemoteNetworkTransport` не доходит до загрузчика ресурсов; тот же класс
  «половина связки отсутствует», из-за чего у HSTS там нет и предмета для подключения.
* [BUG-368](bugs/BUG-368-OPEN.md) — та же категория дефекта (реализация+тесты существуют,
  `CAPABILITIES.md` фиксирует ✅, но продакшн-путь либо не достигает кода, либо код — заглушка).
* `docs/wpt-status.md` категория `hsts`: единственный вендоренный тест
  (`third-party-subframe-hsts-upgrade.tentative.sub.html`) сам по себе непоказателен — он
  `.tentative.` (не финализированная в спеке проверка партиционирования HSTS-состояния по
  top-level site против bounce-tracking) и зависит от невендоренного `/common/get-host-info.sub.js`,
  поэтому TIMEOUT прогона не является ни подтверждением, ни опровержением этой находки — она
  получена независимым Rust-грепом продакшн call sites, не самим WPT-прогоном.
