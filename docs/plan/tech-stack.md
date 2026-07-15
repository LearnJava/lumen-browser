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
- IPC, async-примитивов;
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
| **`aes-gcm`** + **`aes`** (RustCrypto) | AEAD `AEAD_AES_128_GCM` + сырой AES-128 блок для QUIC packet protection (RFC 9001 §5.3/§5.4) в `lumen-network::h3`; тот же `aes-gcm` уже под Web Crypto (`lumen-js`) и шифрованием хранилища (`lumen-storage`) | нет отдельного (низкоуровневый примитив под QUIC) | То же правило «не пишите свой crypto» (Exception #3). Pure-Rust RustCrypto, без native-кода; ручная реализация AEAD/AES — security antipattern |
| **`cbc`** (RustCrypto) | AES-CBC режим с PKCS7 padding для SubtleCrypto `AES-CBC` (`lumen-js::subtle_crypto`, W3C WebCrypto §17.7); оборачивает `aes 0.8` | нет отдельного (примитив режима шифрования под WebCrypto) | Pure-Rust RustCrypto cipher-mode wrapper; ручная реализация CBC с padding — security antipattern |
| **`ctr`** (RustCrypto) | AES-CTR stream cipher (Ctr128BE) для SubtleCrypto `AES-CTR` (`lumen-js::subtle_crypto`, W3C WebCrypto §17.8); оборачивает `aes 0.8` | нет отдельного (примитив режима шифрования под WebCrypto) | Pure-Rust RustCrypto; ручная реализация CTR — security antipattern |
| **`x25519-dalek`** (dalek-cryptography) | X25519 (Curve25519) ECDH для TLS 1.3 `key_share` (RFC 7748 / RFC 8446 §4.2.8) в `lumen-network::h3::key_agreement` — превращает пару ephemeral `KeyShareEntry` в `(EC)DHE` shared secret для `tls_schedule::handshake_secret` | нет отдельного (низкоуровневый примитив под QUIC/TLS) | То же правило «не пишите свой crypto» (Exception #3). Constant-time скалярное умножение, pure-Rust, без native-кода; ручная реализация Curve25519 — security antipattern |
| **`ed25519-dalek`** (dalek-cryptography) | Ed25519 (EdDSA над Curve25519, RFC 8032) — верификация подписи схемы `ed25519` в TLS 1.3 `CertificateVerify` (RFC 8446 §4.2.3) в `lumen-network::h3::tls_cert_verify::ed25519_verify` (peer-аутентификация QUIC/HTTP/3). Переиспользует `curve25519-dalek` уже в дереве (транзитивно через `x25519-dalek`) | нет отдельного (низкоуровневый примитив под QUIC/TLS) | То же правило «не пишите свой crypto» (Exception #3). Verify-only, constant-time, pure-Rust dalek; ручная реализация Ed25519 — security antipattern |
| **`rsa`** (RustCrypto) | (1) RSASSA-PSS с SHA-256 — верификация подписи схемы `rsa_pss_rsae_sha256` в TLS 1.3 `CertificateVerify` (RFC 8446 §4.2.3, RFC 8017 §8.1) в `lumen-network::h3::tls_cert_verify::rsa_pss_sha256_verify`. (2) W3C WebCrypto SubtleCrypto — RSA-OAEP encrypt/decrypt (P3-webcrypto slice 3), RSA-PSS sign/verify, RSASSA-PKCS1-v1_5 sign/verify + generateKey/importKey/exportKey для всех трёх схем в `lumen-js::subtle_crypto`. Переиспользует `sha2` уже в дереве | нет отдельного | То же правило «не пишите свой crypto» (Exception #3). Pure-Rust RustCrypto, без native-кода; ручная реализация RSA — security antipattern |
| **`rand_core`** (RustCrypto) | OsRng (`OsRng::default()`) для генерации RSA-ключей (`rsa::RsaPrivateKey::new`) в `lumen-js::subtle_crypto::generate_key` (W3C WebCrypto SubtleCrypto). Companion crate к `rsa` — тот уже транзитивно тянет `rand_core`; явный dep для признака `getrandom` | нет отдельного (примитив CSRNG под keygen) | RustCrypto; без `getrandom` feature `OsRng` недоступен на Windows |
| **`p384`, `p521`** (RustCrypto) | ECDSA P-384/P-521 — верификация подписи схем `ecdsa_secp384r1_sha384` / `ecdsa_secp521r1_sha512` в TLS 1.3 `CertificateVerify` (RFC 8446 §4.2.3) в `lumen-network::h3::tls_cert_verify::ecdsa_p384_sha384_verify` / `ecdsa_p521_sha512_verify` (peer-аутентификация QUIC/HTTP/3; более крупные NIST-сиблинги `p256`-схемы). Тот же RustCrypto `ecdsa`/`signature` v2, что и `p256` | нет отдельного (низкоуровневый примитив под QUIC/TLS) | То же правило «не пишите свой crypto» (Exception #3). Verify-only, pure-Rust RustCrypto, без native-кода; ручная реализация ECDSA — security antipattern |
| **SQLite** (`rusqlite` с `bundled` feature) | Персистентное хранилище: history, bookmarks, notes, read-later, cookies-TTL, профили. FTS5 для §12.1 полнотекстового поиска. | `StorageBackend` + `KnowledgeStore` | 25 лет TH3-тестирования (100% MC/DC branch coverage), стандарт индустрии браузеров (Firefox/Chromium/Safari). Цена ошибки persistent storage асимметрична — молчаливая порча данных пользователя; та же логика, что у crypto. FTS5 закрывает §12.1 без своего inverted index |

> **Долгосрочная стратегия: pure-Rust storage (redb + tantivy).** SQLite остаётся permanent exception на Phase 0–2. Однако `rusqlite` с `bundled` тянет ~250 КБ C-кода, который нельзя аудировать средствами Rust — это противоречит принципу «свой код = прозрачность». Целевая архитектура (Phase 3+): **redb** (pure Rust, ACID copy-on-write B+tree, ноль `unsafe`, используется в `cargo`) для key-value подсистем (localStorage, sessionStorage, IndexedDB, HTTP cache) + **tantivy** (Rust-native FTS) для полнотекстового поиска §12.1. Оба за `StorageBackend` / `KnowledgeStore` trait — drop-in замена. Graduation criterion: замерить p99 latency SQLite WAL vs redb на реальной нагрузке; если SQLite < 1 мс — миграция не срочна, но остаётся целью ради чистоты стека.
| **`rayon`** | Work-stealing parallel iterator framework. ADR-016 M4.1: параллельный `compute_style` для независимых дочерних узлов flex/grid/table item containers в `lumen-layout::box_tree`. | нет (внутренний примитив параллелизма) | 8+ лет в production, правильный NUMA-aware work-stealing; своя реализация — недели без ценности для идентичности Lumen. Параллелизм layout — не ядро браузерного engine |
| **JS engine** (`rquickjs` v0.11 now; `v8` v150.1.0 optional behind `v8-backend`, cutover at S12) | Исполнение JavaScript | `JsRuntime` | V8 — 15 лет, миллиарды долларов, сотни инженеров. QuickJS на старте Phase 0–2, V8 (rusty_v8) v1.0+. S0 go/no-go PASS 2026-07-13. |

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
- ~~`egui` / `iced` / `Slint`~~ → свой иммедиат-режим UI поверх `wgpu`-примитивов из paint-крейта.
- ~~`flate2` / `miniz_oxide`~~ для PNG — отвергнуто (см. [DECISIONS.md](DECISIONS.md)). PNG-декодер с собственным DEFLATE уже написан в `lumen-image`; DEFLATE переиспользуется для HTTP `Content-Encoding: gzip/deflate`.

### Devtools (не runtime — допустимы)

Инструменты, которые не попадают в бинарь, но используются на CI / при разработке:

- `cargo-deny` — аудит лицензий и CVE четырёх exceptions и их транзитивных зависимостей.
- `cargo-vet` — supply-chain reviews.
- `cargo-dist` — упаковка релизов (опционально).
- `cross` — кросс-компиляция на CI.
- `cargo-hakari` + `workspace-hack` (internal crate) — feature-unification: 55 общих транзитивных зависимостей пришпилены к единому набору фич; устраняет перекомпиляцию при переключении между `-p lumen-shell` и `-p lumen-driver`. Категория: постоянная dev-инфраструктура; публикации нет; `workspace-hack` — path-dep только внутри workspace. Добавлено 2026-07-13 (§3.4 `docs/build-speed.md`).
- `tracy-client` — визуальный профайлер (Tracy GUI, <https://github.com/wolfpld/tracy>), фича `tracy` (`lumen-core`/`lumen-layout`/`lumen-shell`), **не в default**. В отличие от остальных пунктов этого раздела — МОЖЕТ попасть в собранный бинарь, но только при явном `--features tracy`; обычная сборка (`cargo build -p lumen-shell`) не тянет его вовсе (`dep:tracy-client` опционален, макрос `lumen_core::tracy_zone!` компилируется в no-op без фичи). Категория: provisional (см. workspace `Cargo.toml`, комментарий над версией). Добавлено 2026-07-15 (BUG-284 perf investigation, `docs/plan/security-performance.md` §14.3).

### Принцип «no new dep без обоснования»

Если в коммите / Pull Request добавляется новая зависимость в `Cargo.toml`, в описании обязателен пункт:

> **Why this dependency:** \<обоснование, почему свой код тут категорически неуместен — иначе пишем сами\>

CI-чек на новые `[dependencies]`-строки добавим, когда появится remote.

### Язык и тулинг

- **Rust** edition 2024, MSRV — последний stable (сейчас 1.95).
- `cargo` workspace.
- Сборка релизов — `xtask`-крейт, опционально `cargo-dist` поверх.

---

