# BUG-234

**Статус:** OPEN
**Компонент:** shell (network)
**Файл:** `crates/shell/src/main.rs:3024` (`http_client_for_subresource`),
`crates/shell/src/config.rs:213` (`apply_http`)

## Описание

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
