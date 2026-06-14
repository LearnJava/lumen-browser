## 5. Технологический стек

### Политика зависимостей

**Стратегия (обновлено 2026-05-15): сначала рабочий браузер, потом разговор «что переписывать самим».** Раньше §5 формулировался бинарно — «всё своё, кроме 5 exception». На практике это упирало в задачи, которые ничего не определяют в идентичности Lumen, но стоят месяцев работы (image decoders, Unicode UAX-таблицы, Brotli, WOFF2, HTTP/3). Новая формулировка — две категории exception:

- **Permanent.** Никогда не пишем сами. Универсальное правило безопасности / здравого смысла. 5 шт.
- **Provisional accelerators.** Берём готовое сейчас ради скорости Phase 1-3, но за trait-anchor в `lumen-core::ext`, чтобы при желании заменить. У каждого — «graduation criterion»: событие, при котором имеет смысл писать своё. Большинство criterion-ов в духе «реалистично — никогда» (формат стабильный, без архитектурной ценности для Lumen) — это не лицемерие, а честная маркировка.

**«Не делаем Google Chrome» — это про ядро.** Lumen остаётся проектом про собственный rendering engine. HTML/CSS/DOM/style/layout/paint/font/encoding, URL, HTTP/1.1, DNS-резолвер с DoH/DoT, adblock matcher, knowledge layer (§12), UI shell — всегда наши. Если кто-то предлагает «возьми готовое» для пункта из этого списка — это уже Chrome-форк, а не Lumen.

Поэтому мы по-прежнему пишем **свой** код для:

- HTML / CSS парсеров, DOM, style cascade, selectors;
- layout (block, inline, flex, grid), paint, compositing;
- URL-парсинга и базового Punycode (RFC 3492 — IDNA UTS#46 опционально через provisional `idna`);
- HTTP/1.1, HTTP/2, DNS-резолвера с DoH/DoT (HTTP/3 — через provisional `quinn`);
- определения и конвертации кодировок (cp1251, KOI8-R, CP866 и др.);
- PNG-декодера (готов в `lumen-image` + свой DEFLATE, переиспользуемый для HTTP gzip/deflate); JPEG/WebP/GIF — через provisional image-decoder-crate;
- TrueType-парсинга и text shaping для Latin / Cyrillic (WOFF2 — через provisional `woff2`);
- движка адблок-фильтров;
- 2D-растеризации поверх GPU-абстракции;
- ephemeral KV-хранилища (in-memory, для тестов и session-scope данных);
- IPC, async-примитивов, work-stealing thread pool;
- UI-фреймворка (иммедиат-режим поверх своих paint-примитивов);
- собственных MD5 / SHA-256 (для HTTP Digest, не security-критично — challenge-response, не KDF), Base64;
- knowledge layer §12 — это пользовательская ценность Lumen, не делегируется внешним библиотекам.

Bidi (UAX #9), line breaking (UAX #14), segmentation (UAX #29), normalization (UAX #15) — формально были в «своё», но писать свои Unicode-таблицы — это годы работы с обновлениями при каждом релизе Unicode. Переходят в provisional через `icu4x`.

### Permanent exceptions (5 шт., никогда не переписываем)

Это единственные deps, для которых принципиально нет смысла писать своё. Каждая прячется за trait в [`lumen-core::ext`](crates/core/src/ext.rs).

| Crate | Что покрывает | Trait-anchor | Почему не сами |
|---|---|---|---|
| **`winit`** | OS event loop, окна, ввод | `WindowingBackend` | Win32 + X11 + Wayland + AppKit — ~50–100k LOC платформенно-специфичных багов и behaviour quirks |
| **`wgpu`** | GPU API (Vulkan / Metal / DX12 / GL) | `RenderBackend` | 4 разных API, разные семантики, driver-баги. Свой = годы работы и регрессий |
| **`rustls`** + **`webpki-roots`** | TLS, X.509, X25519, AES-GCM, HKDF; `webpki-roots` — bundle корневых CA-сертификатов (Mozilla CA bundle). Без него HTTPS не валидируется. | `TlsBackend` | **Универсальное правило безопасности:** не пишите свой crypto. rustls — аудит + формальная верификация частей кода. `webpki-roots` — pure data + lookup, partner-crate к rustls |
| **SQLite** (`rusqlite` с `bundled` feature) | Персистентное хранилище: history, bookmarks, notes, read-later, cookies-TTL, профили. FTS5 для §12.1 полнотекстового поиска. | `StorageBackend` + `KnowledgeStore` | 25 лет TH3-тестирования (100% MC/DC branch coverage), стандарт индустрии браузеров (Firefox/Chromium/Safari). Цена ошибки persistent storage асимметрична — молчаливая порча данных пользователя; та же логика, что у crypto. FTS5 закрывает §12.1 без своего inverted index |

> **Долгосрочная стратегия: pure-Rust storage (redb + tantivy).** SQLite остаётся permanent exception на Phase 0–2. Однако `rusqlite` с `bundled` тянет ~250 КБ C-кода, который нельзя аудировать средствами Rust — это противоречит принципу «свой код = прозрачность». Целевая архитектура (Phase 3+): **redb** (pure Rust, ACID copy-on-write B+tree, ноль `unsafe`, используется в `cargo`) для key-value подсистем (localStorage, sessionStorage, IndexedDB, HTTP cache) + **tantivy** (Rust-native FTS) для полнотекстового поиска §12.1. Оба за `StorageBackend` / `KnowledgeStore` trait — drop-in замена. Graduation criterion: замерить p99 latency SQLite WAL vs redb на реальной нагрузке; если SQLite < 1 мс — миграция не срочна, но остаётся целью ради чистоты стека.
| **JS engine** (`rquickjs` v0.5 → `rusty_v8` v1.0+) | Исполнение JavaScript | `JsRuntime` | V8 — 15 лет, миллиарды долларов, сотни инженеров. QuickJS на старте, V8 в v1.0+ |

### Provisional accelerators (берём готовое сейчас, заменяем по событию)

Trait-anchor у каждого — в `lumen-core::ext`. Подключаем по мере того, как фаза реально упирается в задачу. Список открыт.

| Crate (кандидаты) | За что | Trait-anchor | Phase | Graduation criterion |
|---|---|---|---|---|
| `zune-jpeg`, `image-webp`, узкий `image` без default features | Декодирование JPEG / WebP / GIF в RGBA. PNG **остаётся свой** в `lumen-image` | `ImageDecoder` | 1 | Едва ли когда-то. Форматы стабильные, без архитектурной ценности; цена реализации (JPEG — DCT+Хаффман+chroma subsampling+progressive; WebP — VP8/VP8L) непропорциональна выгоде |
| `icu4x` (выборочные модули: segmentation, line-break, bidi, normalization, CLDR-минимум) | Unicode UAX #9 / #14 / #29 / #15 + локалевые таблицы | `UnicodeProvider` | 1–2 | Реалистично — никогда. Unicode Consortium = «универсальное правило безопасности» для Unicode, аналогично rustls для crypto. Своя реализация = годы поддержки таблиц на каждом релизе Unicode |
| `brotli-decompressor` | Brotli decompression для HTTP `Content-Encoding: br` | расширение `ContentDecoder` | 1–2 | Едва ли. Формат RFC 7932 стабилен, своя реализация = недели с собственным dictionary |
| `ruzstd` / `zstd-safe` | Zstandard decompression для HTTP `Content-Encoding: zstd` (Cloudflare и nginx уже отдают; через 1-2 года будет распространено) | расширение `ContentDecoder` | 1–2 | Реалистично — никогда. Формат RFC 8478 стабилен, без архитектурной ценности; своя реализация = недели |
| `publicsuffix` (или собственный загрузчик `publicsuffix.org/list/public_suffix_list.dat`) | Public Suffix List для cookie domain matching (`example.co.uk` ≠ `co.uk`), eTLD+1 расчёта, `SameSite=Strict` boundary | `PublicSuffixList` | 1 | Едва ли. Данные обновляются раз в неделю-месяц, формат — простой текст; собственный loader тривиален, но crate избавляет от поддержки парсера |
| `idna` | Полный UTS#46 mapping table для IDN (ß, ZWJ, контекстные правила) | `IdnaProvider` (на базе текущего `Url::host_ascii()`) | 1–2 | Когда найдём real edge-case, который наш `str::to_lowercase`-Punycode не покрывает |
| `hyphenation` | Перенос слов (TeX-словари, включая русский) | `HyphenationProvider` | 2 | Phase 2+ при типографике. Словари можно переписать на свой формат, но low priority |
| `woff2` | Распаковка WOFF2 в TTF | расширение `FontFormat` | 2 | Phase 2 при WebFonts. Формат стабилен, маловероятно писать своё |
| `hunspell-rs` / `spellbook` | Spell-check (русская морфология обязательна) | `SpellChecker` | 3 | Phase 3 при spell-check. Морфология русского сложна, цена своей реализации перекрывает выгоду |
| `quinn` | HTTP/3 / QUIC | расширение `NetworkTransport` | 3 | Реалистично — никогда. QUIC = год+ работы (congestion control, packet loss recovery, 0-RTT, key updates) |
| `redb` | Pure Rust ACID key-value (copy-on-write B+tree). Альтернативный storage backend для горячих key-value (localStorage, IndexedDB, HTTP cache) | `StorageBackend` | 2–3 | Замерить p99 latency SQLite WAL vs redb на реальной нагрузке localStorage/IndexedDB. Если SQLite < 1 мс — не нужен |
| `tantivy` | Rust-native полнотекстовый поиск. Замена SQLite FTS5 для §12.1 knowledge layer при миграции на pure-Rust storage | `KnowledgeStore` | 3+ | Только вместе с redb — при решении полностью отказаться от SQLite C-кода |

**Принципы работы с provisional-категорией:**

- **Trait-anchor обязателен.** Перед добавлением dep в `Cargo.toml` сначала появляется trait в `lumen-core::ext` и default-имплементация (наша, заглушечная или wrapped-around готового crate). Это гарантирует, что замена в будущем — drop-in, без переписывания потребителей.
- **Подключение «по событию», не превентивно.** Не добавляем `icu4x` пока bidi реально не понадобится; не добавляем `quinn` пока HTTP/3 не на повестке.
- **Annual review.** Раз в год — проход по provisional-списку: какие graduation criteria сработали → завести задачу на свой код; какие нет → продлить.
- **Расширение списка — через DECISIONS.md.** Каждое добавление в provisional — новая запись в [DECISIONS.md](DECISIONS.md) с обоснованием и graduation criterion.

### Что НЕ берём как зависимости (даже временно — ядро Lumen)

Эти крейты регулярно обсуждаются как «возьми готовое», но для всех решение — **отвергнуть**. Это идентичность проекта.

- ~~`html5ever`~~ → свой HTML-парсер по [HTML5 spec](https://html.spec.whatwg.org/multipage/parsing.html) (см. §6.1).
- ~~`cssparser` + `selectors`~~ → свой CSS-парсер по CSS Syntax L3 (§6.2).
- ~~`stylo`~~ → свой каскад и computed values (§6.4).
- ~~`taffy`~~ → свой layout: block, inline, flex, grid (§6.5).
- ~~`tiny-skia`~~ → свой 2D-растеризатор (CPU для v0.1, GPU через `wgpu` дальше).
- ~~`hyper`~~ → свой HTTP/1.1 и HTTP/2 поверх `rustls` + std (только HTTP/3 через provisional `quinn`).
- ~~`hickory-resolver`~~ → свой DNS-резолвер с DoH/DoT поверх `rustls`.
- ~~`ttf-parser` / `font-kit`~~ → свой TrueType-парсер и font matcher (только WOFF2-распаковка через provisional `woff2`).
- ~~`rustybuzz`~~ → свой shaper для Latin / Cyrillic. Сложные скрипты (арабский, индийский, тайский) — в v1.0+, отдельным модулем; пока «не поддерживается».
- ~~`encoding_rs`~~ → свои таблицы декодирования (cp1251, KOI8-R, CP866, UTF-8, ASCII, Win-1252).
- ~~`url`~~ → свой URL parser по WHATWG URL spec (текущий стаб в `lumen-core::url`).
- ~~`unicode-security`~~ → свои homograph checks для IDN.
- ~~`adblock`~~ (Brave) → свой filter matcher.
- ~~`readability`~~ → своя реализация readability heuristics с настройкой под кириллицу (§10.9).
- ~~`postcard` + `serde`~~ → своя компактная binary serialization для IPC.
- ~~`tokio`~~ → свой минимальный async-исполнитель поверх std + epoll/kqueue/IOCP (или single-threaded на старте).
- ~~`rayon`~~ → свой work-stealing thread pool, когда понадобится параллельный layout / style.
- ~~`egui` / `iced` / `Slint`~~ → свой иммедиат-режим UI поверх `wgpu`-примитивов из paint-крейта.
- ~~`flate2` / `miniz_oxide`~~ для PNG — отвергнуто (см. [DECISIONS.md](DECISIONS.md)). PNG-декодер с собственным DEFLATE уже написан в `lumen-image`; DEFLATE переиспользуется для HTTP `Content-Encoding: gzip/deflate`.

### Devtools (не runtime — допустимы)

Инструменты, которые не попадают в бинарь, но используются на CI / при разработке:

- `cargo-deny` — аудит лицензий и CVE четырёх exceptions и их транзитивных зависимостей.
- `cargo-vet` — supply-chain reviews.
- `cargo-dist` — упаковка релизов (опционально).
- `cross` — кросс-компиляция на CI.

### Принцип «no new dep без обоснования»

Если в коммите / Pull Request добавляется новая зависимость в `Cargo.toml`, в описании обязателен пункт:

> **Why this dependency:** \<обоснование, почему свой код тут категорически неуместен — иначе пишем сами\>

CI-чек на новые `[dependencies]`-строки добавим, когда появится remote.

### Язык и тулинг

- **Rust** edition 2024, MSRV — последний stable (сейчас 1.95).
- `cargo` workspace.
- Сборка релизов — `xtask`-крейт, опционально `cargo-dist` поверх.

---

