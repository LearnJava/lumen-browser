# BUG-234

**Статус:** FIXED 2026-06-23
**Компонент:** shell (network)
**Файл:** `crates/shell/src/config.rs` (`HTTP_CACHE` / `build_http_cache` / `apply_http`)

## Решение (2026-06-23)

Подключён общий кросс-навигационный HTTP-кэш. В `config.rs` добавлен
process-global `HTTP_CACHE: OnceLock<Option<Arc<dyn HttpCacheBackend>>>`,
инициализируемый лениво через `build_http_cache(private)`:

- обычная сессия → `DiskHttpCache` по пути `<exe_dir>/data/cache/http_cache.db`
  (политика портативного хранения `browser_data_dir`, переживает рестарт);
- `no_persistent_state` / `http_profile == TorBrowser` → in-memory `HttpCache`
  (на диск ничего не пишется);
- если дисковую БД открыть не удалось → `None` (кэш отключён, поведение как
  раньше — всё из сети).

Проводка `.with_http_cache(Arc::clone(...))` добавлена в `apply_http` — это
choke point, через который собираются **все** клиенты (главная навигация,
подресурсы `http_client_for_subresource`, `fetch()`), поэтому один общий кэш
(шарится через `Arc`) покрывает весь трафик. RFC 7234-семантика
(`Cache-Control`/`ETag`/`Last-Modified`/304/freshness) уже была реализована в
`HttpClient::fetch_subresource`. DoH-bootstrap клиент строится в обход
`apply_http` и кэш не получает (корректно).

Тесты: `private_http_cache_is_in_memory_and_present`, `http_cache_is_shared_via_arc`
(config.rs). RFC 7234-поведение покрыто существующими тестами `lumen-network`
(MockTransport, lib.rs:6849–6952).

## Описание (исходная)

HTTP-кэш не подключён к загрузке страниц/подресурсов. В `lumen-network` есть
полноценный RFC 7234 кэш — `HttpCache` (in-memory LRU, 50 МБ) и `DiskHttpCache`
(SQLite, переживает рестарт, `crates/network/src/http_cache.rs`), с поддержкой
`Cache-Control`/`ETag`/`Last-Modified`/304/heuristic-freshness. Но `with_http_cache(...)`
**нигде в шелле не вызывается** — только в тестах `lumen-network` (lib.rs:6849–6952).

`http_client_for_subresource` ([main.rs:3024](../crates/shell/src/main.rs)) и
`config::apply_http` ([config.rs:213](../crates/shell/src/config.rs)) собирают
HTTP-клиент без кэша. Следствие: **каждая навигация, включая повторные заходы на
тот же сайт, заново качает по сети все подресурсы** (JS-бандлы, CSS, шрифты,
картинки). Edge почти всё отдаёт из дискового кэша мгновенно.

Подтверждено замером (lenta.ru, 2026-06-22, headless, медиана 3 прогонов):

| Сценарий | Время |
|---|---|
| Edge, холодный кэш | ~9.0 с |
| Edge, прогретый кэш (повторный заход) | ~2.0 с |
| Lumen (кэша нет → всегда «холодный») | ~3.9 с |

Дисковый кэш даёт Edge ×4.5 на повторных заходах; Lumen платит полную сетевую
цену каждый раз. Это главный измеримый вклад в «сайты долго грузятся vs Edge».

## Как починить

1. Создать один общий `DiskHttpCache` на сессию, хранить в папке браузера —
   `<exe_dir>/Data/cache/` (по конвенции портативного хранения, см. `browser_data_dir`
   в `shell/src/adblock.rs`; **не** в `%APPDATA%`/XDG).
2. Прокинуть `.with_http_cache(Arc::clone(&cache))` в `apply_http`
   (`config.rs:213`) либо в `http_client_for_subresource` (`main.rs:3024`), чтобы
   все subresource/`fetch()`-клиенты использовали общий кэш.
3. Инвалидация: уважать generation-guard навигации (как `prefetch::PREFETCH_CACHE`)
   и заголовки `Cache-Control: no-store/no-cache`. `Vary` уже не кэшируется кэшем
   (безопасно).
4. Проверить взаимодействие с уже существующими `prefetch::PREFETCH_CACHE` и
   `image_cache::IMAGE_CACHE` (per-load): HTTP-кэш — кросс-навигационный слой
   под ними, не дублировать.

Размер: S–M (инфраструктура кэша готова, нужна только проводка + путь хранения +
тест на повторный заход без сети).

## Контекст

Найдено при анализе скорости загрузки vs Edge (2026-06-22). Сетевые фиксы из
прошлой диагностики (параллельный fetch, вынос пайплайна с UI-потока — BUG-171,
FOUT — BUG-170, дедуп картинок — BUG-172) уже влиты; отсутствие кросс-навигационного
кэша — оставшийся крупный сетевой долг. Второй фактор — BUG-233 (JS-бандлы падают
на `self`).
