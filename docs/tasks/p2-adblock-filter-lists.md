# Задача: Ad-block — внешние фильтр-листы (хранение, обновление, парсинг)

**Developer:** P2
**Ветка:** `p2-adblock-filter-lists`
**Размер:** L (новый модуль ~300–400 строк + дисковый кэш + фоновый рефреш + правка install)
**Крейты:** `lumen-network` (основное), `lumen-shell` (запуск рефреша при старте)

---

## Контекст

Ad-block уже работает end-to-end, но **список вшит в бинарь и не обновляется**:

```
FilterListSource (текст правил) → EasyListFilter::parse (индекс) → RequestFilter::should_block → сетевой гейт fetch_single
```

Что уже есть (НЕ переписывать, строить поверх):

| Компонент | Файл | Назначение |
|---|---|---|
| `FilterListSource` | `crates/core/src/ext.rs:100` | трейт-сим источника правил: `fetch_rules() -> Result<String>` |
| `RequestFilter` | `crates/core/src/ext.rs:116` | `should_block(&Url) -> Option<String>` |
| `EasyListFilter::parse` | `crates/network/src/filter/easylist.rs:67` | парсер EasyList-синтаксиса (`\|\|domain^`, `@@`, `\|prefix\|`, keyword, `/regex/`) |
| `DefaultFilterList` | `crates/network/src/filter/default_list.rs` | вшитый набор ~40 доменов (fallback) |
| `set_global_adblock_enabled` / `install_global_adblock_filter` / `global_adblock_filter` | `crates/network/src/lib.rs` (блок «Process-global ad-block filter») | процесс-глобальный тумблер + установка фильтра, консультируется гейтом `fetch_single` |
| `config::init_adblock()` | `crates/shell/src/config.rs` | ставит `EasyListFilter(DefaultFilterList)` + включает при старте |
| per-tab чекбокс | `crates/shell/src/tabs/strip.rs` | UI-тумблер per-tab (готов, не трогать) |

**Цель задачи:** заменить «вшитый список» на подгружаемые **внешние фильтр-листы** (EasyList, EasyPrivacy) с дисковым кэшем и периодическим обновлением — как у Chrome-расширений (uBlock/ABP). Движок сопоставления (`EasyListFilter`) уже умеет парсить эти листы; нужен слой загрузки/хранения/обновления поверх `FilterListSource`.

---

## Архитектура

### 1. Где и как хранятся списки

Директория: `lumen_cache_dir()/filterlists/` (хелпер `lumen_network::lumen_cache_dir()` — `crates/network/src/http_cache.rs:567`):
- Windows: `%APPDATA%\lumen\cache\filterlists\`
- Unix: `$XDG_CACHE_HOME/lumen/cache/filterlists/` (или `~/.cache/lumen/cache/filterlists/`)

Раскладка файлов:

```
filterlists/
  subscriptions.json     ← манифест подписок (см. ниже)
  <slug>.txt             ← сырой текст листа (EasyList-формат, как скачан)
  <slug>.meta.json       ← метаданные кэша: url, etag, last_modified, fetched_at_unix, rule_count
  custom.txt             ← пользовательские правила (Phase 3; пока пустой/опционально)
```

`<slug>` — стабильный слаг URL листа (sanitized host+hash, например `easylist-easylist-to` или `sha1(url)[..16]`). Без коллизий между подписками.

`subscriptions.json` — список подписок:
```json
{
  "version": 1,
  "subscriptions": [
    { "url": "https://easylist.to/easylist/easylist.txt",     "title": "EasyList",      "enabled": true },
    { "url": "https://easylist.to/easylist/easyprivacy.txt",  "title": "EasyPrivacy",   "enabled": true }
  ]
}
```
Сидируется дефолтами при первом запуске (если файла нет). Сериализация — `serde_json` (уже в зависимостях network через другие модули; проверить `Cargo.toml`, при отсутствии — добавить с обоснованием permanent/serde).

### 2. Как обновляются (offline-first + условный GET)

**При старте (быстро, без сети):** прочитать `subscriptions.json` + все включённые `<slug>.txt` из кэша → склеить текст → `EasyListFilter::parse` → `install_global_adblock_filter`. Если кэша нет — fallback на вшитый `DefaultFilterList`, чтобы блокировка работала сразу.

**Фоновый рефреш (отдельный поток, после старта):** для каждой включённой подписки, если `now - fetched_at > REFRESH_INTERVAL` (EasyList рекомендует expiry ~4 дня; `const REFRESH_INTERVAL_SECS: u64 = 4*24*3600`):
1. Условный GET через `HttpClient` (с fingerprint-профилем из `config::global().apply_http(...)`):
   - `If-None-Match: <etag>` и/или `If-Modified-Since: <last_modified>` из `<slug>.meta.json`.
2. `304 Not Modified` → обновить `fetched_at` в meta, ничего не перепарсивать.
3. `200 OK` → перезаписать `<slug>.txt`, обновить meta (новый etag/last_modified/rule_count/fetched_at), пометить «надо переустановить фильтр».
4. После обхода всех подписок, если хоть одна обновилась — склеить заново → `EasyListFilter::parse` → переустановить фильтр (hot-swap, см. п.3 ниже).

Ошибки сети не критичны: остаёмся на кэшированной версии, логируем `eprintln!`.

### 3. Как попадают в систему (hot-swap install)

**ВАЖНО — текущий блокер:** `GLOBAL_ADBLOCK_FILTER` в `crates/network/src/lib.rs` — это `OnceLock`, переустановить нельзя (первый `set` побеждает). Для обновления на лету заменить на свопаемый контейнер:

```rust
// было:
static GLOBAL_ADBLOCK_FILTER: std::sync::OnceLock<Arc<dyn RequestFilter>> = ...;
// стало:
static GLOBAL_ADBLOCK_FILTER: std::sync::RwLock<Option<Arc<dyn lumen_core::ext::RequestFilter>>> =
    std::sync::RwLock::new(None);
```
- `install_global_adblock_filter(f)` → `*GLOBAL_ADBLOCK_FILTER.write().unwrap() = Some(f);` (теперь идемпотентность не нужна — каждый вызов заменяет).
- `global_adblock_filter()` → если enabled: `GLOBAL_ADBLOCK_FILTER.read().unwrap().clone()`.

`RwLock` достаточно (читается на каждый запрос, пишется редко при апдейте — contention ничтожный). Новый dep `arc_swap` НЕ нужен.

### 4. Парсинг

`EasyListFilter::parse(&str)` (`easylist.rs`) уже парсит несколько листов из склеенного текста: `@@`-исключения работают глобально по всему набору, поэтому **склеивать все листы в одну строку и парсить один раз** (не делать `CompositeFilter` из отдельных фильтров — иначе exception из EasyList не отменит block из EasyPrivacy). Комментарии (`!`/`#`) и косметика (`##`) уже игнорируются.

---

## Фазирование

### Phase 1 (ядро — эта задача)
- Новый модуль `crates/network/src/filter/remote.rs`:
  - `struct FilterListStore { dir: PathBuf }` — чтение/запись кэша + `subscriptions.json` + meta.
  - `impl FilterListSource for FilterListStore` (`fetch_rules()` = склейка включённых кэшированных листов; fallback на `DefaultFilterList` если пусто).
  - `fn refresh(&self, client: &HttpClient) -> bool` — условный GET всех просроченных подписок, возвращает «что-то обновилось».
  - `fn default_subscriptions() -> Vec<Subscription>` (EasyList + EasyPrivacy).
- `lumen-network/lib.rs`: `OnceLock → RwLock` для `GLOBAL_ADBLOCK_FILTER` (hot-swap).
- `shell/config.rs::init_adblock()`: грузить из `FilterListStore` (offline-first) → установить; вернуть `Arc<FilterListStore>` для рефреша.
- `shell/main.rs` startup: после `init_adblock()` заспавнить поток, который зовёт `store.refresh(&client)` и при изменениях переустанавливает фильтр (`EasyListFilter::parse` → `install_global_adblock_filter`).
- Тесты: парс meta, слаг URL, склейка листов, 304-ветка (через `MockTransport` — `crates/network/src/mock.rs`).

### Phase 2 (тип ресурса — `$options`)
- Расширить `easylist.rs`: не отбрасывать `$script,image,third-party,...`, а хранить и учитывать в `should_block`. Гейт уже прокидывает `RequestDestination` (`crates/network/src/mixed_content.rs`) — сделать вариант `should_block_typed(url, destination)`. Отдельная задача-продолжение.

### Phase 3 (UI подписок — handoff P3)
- Панель управления подписками: добавить/удалить лист, «обновить сейчас», показ last-updated/rule_count, редактор `custom.txt`. Шелл-интеграция — домен P3 (описать интеграционные точки в commit body, P3 подхватит).

---

## Пред-запуск

- [ ] Прочесть блок «Process-global ad-block filter» в `crates/network/src/lib.rs`
- [ ] Прочесть `crates/network/src/filter/easylist.rs:67` (`parse`) и `default_list.rs`
- [ ] Прочесть `crates/network/src/http_cache.rs:567` (`lumen_cache_dir`) и `mock.rs` (`MockTransport` для тестов)
- [ ] Прочесть `crates/shell/src/config.rs` (`init_adblock`, `apply_http`) и точку старта в `main.rs` (рядом с `config::init_adblock()`)
- [ ] `git status` — main чист

---

## Шаги

1. `git worktree add .claude/worktrees/adblock-lists -b p2-adblock-filter-lists` → в первом коммите пометить «In progress» в STATUS-P2.md.
2. `lib.rs`: `GLOBAL_ADBLOCK_FILTER` `OnceLock → RwLock<Option<...>>`; поправить `install_*`/`global_adblock_filter`.
3. Новый `filter/remote.rs`: `Subscription`, `FilterListStore` (load/save/slug/meta), `impl FilterListSource`, `refresh(client)`; реэкспорт в `filter/mod.rs` и `lib.rs`.
4. `config.rs::init_adblock()` → строить `FilterListStore` (offline-first install), вернуть `Arc<FilterListStore>`.
5. `main.rs` startup → `std::thread::spawn` рефреш-поток (условный GET → переустановка при изменениях). Поток — best-effort, паники изолированы.
6. Тесты network (slug/meta/склейка/304 через MockTransport) + `cargo clippy -p lumen-network -p lumen-shell --all-targets -- -D warnings`.
7. Обновить `CLAUDE.md` (ext-traits: упомянуть RemoteFilterList) и `subsystems/network.md`.
8. Завершить по чеклисту из CLAUDE.md (merge --no-ff → delete branch → STATUS-P2 → push → worktree remove).

---

## Критерии готовности

- [ ] Первый запуск без кэша: сидируются дефолтные подписки, блокировка работает (через `DefaultFilterList` fallback, пока листы не скачаны).
- [ ] После рефреша: `filterlists/easylist*.txt` + meta на диске; фильтр переустановлен; реальные правила EasyList/EasyPrivacy блокируют (проверить на `\|\|google-analytics.com^` и любом домене из скачанного EasyList).
- [ ] Повторный запуск: грузится из кэша мгновенно (offline-first), сеть не блокирует старт.
- [ ] `304 Not Modified` не перепарсивает; `200` — перезаписывает и hot-swap’ит фильтр без перезапуска.
- [ ] Тумблер per-tab по-прежнему включает/выключает (направление: галка = блокировка вкл).
- [ ] Clippy чист (network + shell); тесты проходят; main чист после merge.

---

## Замечания

- **Приватность.** Lumen — приватный браузер: скачивание листов с CDN раскрывает факт использования. По умолчанию подписки включены (как в адблокерах), но в Phase 3 дать переключатель «не обновлять автоматически». Запросы идут через обычный `HttpClient` с fingerprint-профилем — не выделяются.
- **Без новых тяжёлых зависимостей.** Условный GET — существующим `HttpClient`; своп — `std::sync::RwLock`; JSON — `serde_json` (если ещё не в network — добавить с обоснованием permanent). `arc_swap` не нужен.
- **Bug-политика.** Найденный по дороге баг — строкой в `BUGS.md` (OPEN, следующий BUG-NNN), не чинить в этой задаче.
