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

**Решение (пользователь, 2026-06-16): всё хранить ТОЛЬКО в папке браузера, OS-каталоги
(`%APPDATA%`, `~/.config`, `~/.cache`) НЕ использовать.** Провизорно — «пока так, дальше
посмотрим». НЕ звать `lumen_cache_dir()` / `config_path()` для адблок-данных.

Корень всех пользовательских данных — рядом с бинарём: `<папка_бинаря>/data/`, где
`<папка_бинаря>` = каталог `std::env::current_exe()`. **Структура аккуратная и понятная
человеку: один корень `data/`, внутри по подсистемам; адблок — в `data/adblock/`.**
В dev запуск идёт из `target/release/` (внутри репозитория) — нарушения boundary нет.

Хелпер (в `filter/remote.rs`), без OS-папок:
```rust
/// Корень данных браузера (portable): <exe_dir>/data. Подсистемы кладут свои
/// данные в именованные подпапки (adblock/, в будущем cache/, profiles/ …).
fn browser_data_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("data"))
}
```
Если `current_exe()` недоступен (редкий случай) — fallback на `./data` от текущего каталога;
наружу (в OS-папки) НЕ уходить.

**Раскладка `data/adblock/` — структурное состояние в SQLite, крупный/редактируемый контент в файлах:**

```
data/
└── adblock/
    ├── adblock.db          ← SQLite (lumen-storage::AdblockStore): таблицы subscriptions + list_meta
    ├── custom-rules.txt    ← правила пользователя (Phase 3) — текстовый файл, правится вручную
    └── lists/              ← СКАЧАННЫЕ тела листов (большие, человекочитаемые)
        ├── easylist.txt
        └── easyprivacy.txt
```

**Что в БД, что в файлах (и почему НЕ всё в БД).** Горячий путь `should_block(url)` вызывается
на КАЖДЫЙ запрос и работает по in-memory индексу `EasyListFilter` (`HashMap`), а не по диску.
SQL-запрос на каждый сетевой запрос был бы МЕДЛЕННЕЕ — матчинг остаётся в RAM, списки парсятся
в память один раз при старте. Поэтому:

| Данные | Где | Почему |
|---|---|---|
| `subscriptions` (url, title, enabled) | **SQLite** | структурно/атомарно/запрашиваемо; надёжнее JSON (нет торн-райта при краше) |
| `list_meta` (slug, url, etag, last_modified, fetched_at_unix, rule_count, content_hash) | **SQLite** | один SELECT «что просрочено»; hash → пропуск перепарса, если не менялось |
| тела листов `lists/<slug>.txt` (~2 МБ) | **файлы** | читаются раз при старте → в RAM; 2-МБ BLOB только раздует БД без выигрыша; файл инспектируется глазами |
| `custom-rules.txt` | **файл** | редактируется пользователем вручную |
| сам матчинг `should_block` | **RAM** (`EasyListFilter`) | БД тут = регрессия |

`<slug>` — человекочитаемый, не хэш: `easylist`, `easyprivacy` (sanitize title/host: lowercase,
`[a-z0-9-]`, коллизии суффиксом `-2`). `list_meta.slug` ↔ `lists/<slug>.txt` связаны одним слагом.

**Схема SQLite** (в новом `crates/storage/src/adblock.rs`, по образцу `print_prefs.rs`/`bookmarks.rs`):
```sql
CREATE TABLE IF NOT EXISTS subscriptions (
    url      TEXT PRIMARY KEY,
    title    TEXT NOT NULL,
    enabled  INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS list_meta (
    slug          TEXT PRIMARY KEY,
    url           TEXT NOT NULL,
    etag          TEXT,
    last_modified TEXT,
    fetched_at    INTEGER NOT NULL DEFAULT 0,   -- unix seconds
    rule_count    INTEGER NOT NULL DEFAULT 0,
    content_hash  TEXT                          -- skip re-parse when unchanged
);
```
`subscriptions` сидируется дефолтами (EasyList + EasyPrivacy) при пустой таблице на первом запуске.

### 2. Как обновляются (offline-first + условный GET)

**При старте (быстро, без сети):** прочитать включённые подписки из `AdblockStore` + соответствующие `lists/<slug>.txt` с диска → склеить текст → `EasyListFilter::parse` → `install_global_adblock_filter`. Если файлов листов ещё нет — fallback на вшитый `DefaultFilterList`, чтобы блокировка работала сразу. Парсинг — желательно в фоновом потоке (не блокировать первый кадр; до готовности активен `DefaultFilterList`).

**Фоновый рефреш (отдельный поток, после старта):** для каждой включённой подписки, если `now - fetched_at > REFRESH_INTERVAL` (EasyList рекомендует expiry ~4 дня; `const REFRESH_INTERVAL_SECS: u64 = 4*24*3600`):
1. Условный GET через `HttpClient` (с fingerprint-профилем из `config::global().apply_http(...)`):
   - `If-None-Match: <etag>` и/или `If-Modified-Since: <last_modified>` из строки `list_meta`.
2. `304 Not Modified` → `UPDATE list_meta SET fetched_at=now`, ничего не перепарсивать.
3. `200 OK` → перезаписать `lists/<slug>.txt`, `UPDATE list_meta` (etag/last_modified/rule_count/content_hash/fetched_at); если `content_hash` не изменился — перепарс не нужен, иначе пометить «переустановить фильтр».
4. После обхода всех подписок, если хоть одна реально изменилась — склеить заново → `EasyListFilter::parse` → переустановить фильтр (hot-swap, см. п.3 ниже).

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

Разделение по крейтам (зависимости чистые: network НЕ тянет storage; shell зависит от обоих):

- **`crates/storage/src/adblock.rs`** — новый `AdblockStore` (SQLite, по образцу `print_prefs.rs`):
  - `open(path)` / `open_in_memory()`; `CREATE TABLE` для `subscriptions` + `list_meta` (схема выше).
  - методы: `list_subscriptions()`, `set_subscription(url,title,enabled)`, `seed_defaults_if_empty(&[Subscription])`, `get_meta(slug)`, `upsert_meta(ListMeta)`.
  - реэкспорт `AdblockStore` в `crates/storage/src/lib.rs`.
- **`crates/network/src/lib.rs`** — `OnceLock → RwLock` для `GLOBAL_ADBLOCK_FILTER` (hot-swap). Это ЕДИНСТВЕННАЯ правка network; движок `EasyListFilter` не трогаем.
- **`crates/shell/src/adblock.rs`** (новый модуль оркестрации):
  - `browser_data_dir()` → `<exe_dir>/data`; создать `data/adblock/lists/`.
  - `default_subscriptions()` (EasyList + EasyPrivacy).
  - `load_and_install(&AdblockStore)` — прочитать включённые подписки + `lists/<slug>.txt` → склеить → `EasyListFilter::parse` → `install_global_adblock_filter` (offline-first; fallback `DefaultFilterList`, если файлов нет).
  - `refresh(&AdblockStore, &HttpClient) -> bool` — условный GET просроченных (по `list_meta`), запись `lists/<slug>.txt` + `upsert_meta`, content_hash → пропуск перепарса; вернуть «надо переустановить».
- **`crates/shell/src/config.rs::init_adblock()`** — открыть `AdblockStore(data/adblock/adblock.db)`, `seed_defaults_if_empty`, `load_and_install` (быстрый старт); вернуть `Arc<AdblockStore>` для рефреша.
- **`crates/shell/src/main.rs`** startup — `std::thread::spawn` рефреш-поток: `refresh(...)` → при изменениях `EasyListFilter::parse` → `install_global_adblock_filter`. Best-effort, паники изолированы.
- **Тесты:** storage (`AdblockStore` CRUD + seed на `open_in_memory`), shell (склейка листов, слаг, 304/200-ветки через `MockTransport` — `crates/network/src/mock.rs`).

### Phase 2 (тип ресурса — `$options`)
- Расширить `easylist.rs`: не отбрасывать `$script,image,third-party,...`, а хранить и учитывать в `should_block`. Гейт уже прокидывает `RequestDestination` (`crates/network/src/mixed_content.rs`) — сделать вариант `should_block_typed(url, destination)`. Отдельная задача-продолжение.

### Phase 3 (UI подписок — handoff P3)
- Панель управления подписками: добавить/удалить лист, «обновить сейчас», показ last-updated/rule_count, редактор `custom-rules.txt`. Шелл-интеграция — домен P3 (описать интеграционные точки в commit body, P3 подхватит).

---

## Пред-запуск

- [ ] Прочесть блок «Process-global ad-block filter» в `crates/network/src/lib.rs`
- [ ] Прочесть `crates/network/src/filter/easylist.rs:67` (`parse`) и `default_list.rs`
- [ ] Прочесть `crates/storage/src/print_prefs.rs` (образец стораджа: `open`/`open_in_memory`/`CREATE TABLE`/CRUD) — по нему делать `adblock.rs`
- [ ] Прочесть `mock.rs` (`MockTransport` для тестов); БД и файлы — рядом с бинарём через `std::env::current_exe()` (НЕ `lumen_cache_dir`)
- [ ] Прочесть `crates/shell/src/config.rs` (`init_adblock`, `apply_http`) и точку старта в `main.rs` (рядом с `config::init_adblock()`)
- [ ] `git status` — main чист

---

## Шаги

1. `git worktree add .claude/worktrees/adblock-lists -b p2-adblock-filter-lists` → в первом коммите пометить «In progress» в STATUS-P2.md.
2. `network/src/lib.rs`: `GLOBAL_ADBLOCK_FILTER` `OnceLock → RwLock<Option<...>>`; поправить `install_*`/`global_adblock_filter`.
3. `storage/src/adblock.rs`: `AdblockStore` (SQLite: `subscriptions` + `list_meta`, CRUD/seed) + реэкспорт в `storage/src/lib.rs`.
4. `shell/src/adblock.rs`: `browser_data_dir()`, `default_subscriptions()`, `load_and_install(store)`, `refresh(store, client)`.
5. `shell/config.rs::init_adblock()` → открыть `AdblockStore` в `data/adblock/adblock.db`, seed, `load_and_install`; вернуть `Arc<AdblockStore>`. `main.rs` startup → рефреш-поток.
6. Тесты storage (`AdblockStore` in-memory) + shell (склейка/304 через MockTransport) + `cargo clippy -p lumen-storage -p lumen-network -p lumen-shell --all-targets -- -D warnings`.
7. Обновить `CLAUDE.md` и `subsystems/storage.md` (новый `AdblockStore`).
8. Завершить по чеклисту из CLAUDE.md (merge --no-ff → delete branch → STATUS-P2 → push → worktree remove).

---

## Критерии готовности

- [ ] Первый запуск без кэша: сидируются дефолтные подписки, блокировка работает (через `DefaultFilterList` fallback, пока листы не скачаны).
- [ ] После рефреша: `data/adblock/lists/easylist.txt` (и easyprivacy) на диске, строки в `list_meta` (etag/fetched_at/rule_count/content_hash) в `adblock.db`; фильтр переустановлен; реальные правила EasyList/EasyPrivacy блокируют (проверить на `\|\|google-analytics.com^` и любом домене из скачанного EasyList).
- [ ] Структура аккуратная: структурное состояние (подписки + меты) в `adblock.db`, тела листов в `lists/`, правила пользователя в `custom-rules.txt` — не свалено в один файл/уровень.
- [ ] Повторный запуск: грузится из кэша мгновенно (offline-first), сеть не блокирует старт.
- [ ] `304 Not Modified` не перепарсивает; `200` — перезаписывает и hot-swap’ит фильтр без перезапуска.
- [ ] Тумблер per-tab по-прежнему включает/выключает (направление: галка = блокировка вкл).
- [ ] Clippy чист (network + shell); тесты проходят; main чист после merge.

---

## Замечания

- **Приватность.** Lumen — приватный браузер: скачивание листов с CDN раскрывает факт использования. По умолчанию подписки включены (как в адблокерах), но в Phase 3 дать переключатель «не обновлять автоматически». Запросы идут через обычный `HttpClient` с fingerprint-профилем — не выделяются.
- **Хранение только в папке браузера, структура аккуратная.** Все данные адблока — под `<exe_dir>/data/adblock/` (см. §1), OS-каталоги (`%APPDATA%`/`~/.config`/`~/.cache`) и хелперы `lumen_cache_dir()`/`config_path()` НЕ использовать (решение пользователя 2026-06-16, провизорно). Внутри — разложить по назначению (`subscriptions.json` / `lists/` / `meta/` / `custom-rules.txt`), не сваливать в один уровень. Корень `data/` рассчитан на будущие подсистемы (каждая — своя подпапка).
- **Без новых тяжёлых зависимостей.** Структурное состояние — SQLite через `lumen-storage` (rusqlite уже там, как у всех стораджей); своп фильтра — `std::sync::RwLock`; условный GET — существующим `HttpClient`. Новые `arc_swap`/`serde_json` НЕ нужны.
- **БД ускоряет не везде.** Матчинг `should_block` остаётся в RAM (`EasyListFilter`); в БД — только подписки и метаданные кэша (структурное/запрашиваемое состояние), тела листов — файлами. Не класть сам матчинг в SQLite (см. таблицу «что в БД, что в файлах» в §1).
- **Bug-политика.** Найденный по дороге баг — строкой в `BUGS.md` (OPEN, следующий BUG-NNN), не чинить в этой задаче.
