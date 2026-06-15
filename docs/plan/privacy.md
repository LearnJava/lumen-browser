## 9. Приватность

### 9.1 Сетевой уровень

**DNS:**
- DoH (DNS over HTTPS) по умолчанию. Провайдеры — на выбор: Cloudflare 1.1.1.1, Quad9, NextDNS, свой.
- DoT (DNS over TLS) — альтернатива.
- DNS cache — в network service, не зависит от ОС.
- DNS-prefetch — выключен по умолчанию.

**TLS:**
- `rustls` only, никакого OpenSSL.
- Минимум TLS 1.2, по умолчанию 1.3.
- ECH (Encrypted Client Hello) — поддерживаем, когда доступно.
- TLS ClientHello fingerprint — нормализованный (uTLS-style), чтобы не выделяться.

**HTTP:**
- `Referer` на cross-origin — `strict-origin-when-cross-origin` по умолчанию.
- `User-Agent` — фиксированная строка (как у Tor Browser), без минорных версий ОС.
- `Accept-Language` — нормализованная.
- Strip URL params: `utm_*`, `fbclid`, `gclid`, `mc_*`, `_ga`, `yclid`, `igshid` и т.д. Списки обновляемые.

**Прокси:**
- SOCKS5, HTTP, HTTPS.
- Tor — нативная поддержка (запуск `tor` бинаря, либо `arti` — Rust Tor).
- Per-tab proxy — можно назначить разный прокси разным вкладкам.

### 9.2 Cookies и storage

- **Total cookie protection** — cookies партиционированы по top-level eTLD+1. Третьесторонний сайт получает свой cookie jar для каждого встраивающего сайта.
- **SameSite=Lax по умолчанию** — даже если сайт не указал.
- **First-Party Isolation** — IndexedDB, localStorage, cache — всё партиционировано.
- **Целевой pure-Rust backend (Phase 3+):** redb для горячих key-value (localStorage, sessionStorage, IndexedDB, HTTP cache) + tantivy для FTS — замена SQLite C-кода за `StorageBackend` trait (см. §5).
- **Auto-clear:** опционально, при закрытии вкладки/окна/сессии.
- **Cookie viewer** — UI для просмотра и удаления.

### 9.3 Профили

- Несколько изолированных профилей (личный/работа/анонимный/гость).
- Каждый — отдельная директория + отдельный мастер-ключ (Argon2id KDF из пароля).
- Storage внутри профиля шифруется (XChaCha20-Poly1305) — даже если кто-то получит диск.
- **Quick profile switch** — Ctrl+Shift+M.

### 9.4 Контентная фильтрация

- **Встроенный adblock — свой матчер.** Поддерживаем формат фильтров uBlock / EasyList (синтаксис задокументирован). Реализуем как `lumen-network::filters`. Не берём `adblock-rust` (см. §5).
- Подписки: EasyList, EasyPrivacy, uBO filters, NoCoin, Fanboy social.
- **Фильтрация на уровне network service** — НЕ зависит от движка. Сайт не может обойти через какой-нибудь Manifest V3-аналог.
- Cosmetic filtering (скрытие элементов) — через стили, инжектится в renderer.
- Per-site disable — пользовательский whitelist.

### 9.5 Anti-fingerprinting / Anti-detection privacy stack

Полное архитектурное обоснование и red lines — [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md).

**Принцип:** пользователь имеет право посещать публичный сайт со своего устройства. Lumen — user agent в интересах пользователя, не сайта-оператора. Privacy-stack устанавливается **по умолчанию для всех** (как в Firefox Strict / Brave / Tor), не как opt-in «stealth mode». Побочный эффект — устойчивость к anti-bot системам (Cloudflare/DataDome/Akamai/PerimeterX/Kasada/Imperva), которые иначе ложно-помечают любой не-Chrome браузер.

Anti-detection покрывает **шесть слоёв**, потому что современные детекторы 2026 работают глубже, чем «canvas pixel hash»:

#### Слой 1 — Surface API: нет automation-маркеров (always-on, default)

- `navigator.webdriver` **не существует** (не `false`, а отсутствует — как в clean Chrome без `--enable-automation`).
- Нет `chrome.runtime`, `__playwright`, `__puppeteer`, `cdc_*` (ChromeDriver), `_phantom`, `callPhantom`, `Buffer`, `emit`-on-window и других классических маркеров.
- JS-runtime (`rquickjs` Phase 0, V8 Phase 3+) **не инструментирован** для automation. Автоматизация идёт через `BrowserSession` (см. §6.11, ADR-006) — она не касается JS-окружения, если страница сама к нему не обращается.
- `event.isTrusted = true` для native-injected input — события приходят в event loop тем же путём, что от ОС.

#### Слой 2 — TLS fingerprint (default + per-profile)

- `rustls` сконфигурирован с **cipher suite ordering, extension list и supported groups, совпадающими с current stable Chrome** (default profile). ALPN: `h2`, `http/1.1` — порядок Chrome-овский.
- Цель: сайт не должен мочь выделить юзера Lumen только потому, что мы выбрали другую Rust-TLS библиотеку, — мы конфигурируем `rustls` так, как `rustls` уже умеет конфигурироваться.
- **Per-profile**: privacy-strict профиль использует `rustls`-defaults, корпоративный — pinned JA3, Tor-profile — JA3 Tor Browser.
- **Что мы не делаем:** не патчим криптографию, не имитируем «быть» Chrome поверх собственной идентичности (UA остаётся `Lumen/0.x`).

#### Слой 3 — HTTP layer (default + per-profile)

- **HTTP/1.1**: порядок и casing заголовков (`User-Agent`, `Accept`, `Accept-Encoding`, `Accept-Language`, ...) — как у текущего Chrome.
- **HTTP/2**: `SETTINGS` frame values (`SETTINGS_HEADER_TABLE_SIZE = 65536`, `SETTINGS_MAX_CONCURRENT_STREAMS = 1000`, `SETTINGS_INITIAL_WINDOW_SIZE = 6291456`, …), stream priority frames — как у Chrome.
- `Accept-Language` по умолчанию `en-US,en;q=0.9` (не палит реальную локаль юзера); пользователь может переопределить вручную.
- Client Hints (`Sec-CH-UA`, etc.) — отдаём свой UA на запрос, либо ничего на Strict (как Tor).

#### Слой 4 — Rendering fingerprint (Brave-style, default)

Старый §9.5 — оставлен и формализован:

- **Canvas randomization** — `Canvas.getImageData` с микро-шумом, per-session deterministic seed (как Brave).
- **WebGL renderer / vendor** — обобщённые строки («Generic GPU», «WebKit»); shader compilation timing нормализован.
- **AudioContext fingerprint** — мизерный шум.
- **Fonts enumeration** — белый список + только bundled fonts на Strict.
- **Timezone** — опция UTC на Strict; иначе реальный.
- **Screen resolution** — округление до 100px на Strict.
- **Hardware concurrency** — фиксированное значение на Strict.
- **Battery API** — отключён (no information) на Strict.
- **WebRTC** — только mDNS host candidates, без public IP leak (как Brave/Safari).

#### Слой 5 — Behavioral input (opt-in **только для automation API**)

- `BrowserSession::input_event()` (см. §6.11 / ADR-006 task 8C) принимает `InputMode::HumanLike` опционально — Bézier-кривые движения мыши, variable inter-keystroke timing, малые dwell-time перед кликами.
- **Назначение** — тестировщики, которые хотят чтобы автотесты проходили те же code paths, что реальный юзер (event coalescing, hover transitions, slow-pointer logic). Это **не stealth-фича**, реальный человеческий input через шелл — уже человеческий и mimicry не требует.

#### Слой 6 — Профили (расширение существующих трёх)

- **Standard** (default) — Слои 1+2+3 + слой 4 на низкой интенсивности + total cookie protection + adblock + strip URL params. Сайты работают.
- **Strict** — Слои 1+2+3 + слой 4 на высокой интенсивности + WebRTC mDNS-only + Client Hints отключены + JS-блокировка на сомнительных доменах.
- **Tor-mode** — Strict + Tor circuit + Tor Browser JA3/UA/screen/font pinning + zero persistent state.
- **Per-context override** — `BrowserSession::set_fingerprint_profile(profile)` для automation-юзеров с конкретной identity (ADR-006 task 8F.3 уже включает `freeze_fingerprint`).

#### Red lines (никогда не делаем — см. ADR-007 «Consequences»)

- ❌ **CAPTCHA-solver** (on-device или через сервис) — у сайтов есть legitimate interest в human-verification для определённых функций.
- ❌ **Built-in IP rotation / residential proxy integration** — network identity это выбор и ответственность юзера, не функция движка.
- ❌ **Anti-fraud-detection bypass для банков, платежей, госуслуг** — эти системы защищают от реального вреда.
- ❌ **Marketing как «scraping browser» / «stealth automation»** — Lumen позиционируется как privacy-браузер; то что он чистая automation-поверхность (ADR-006), коммуницируется в техдоках разработчикам, не в продуктовом маркетинге.
- ❌ **Платный «stealth-tier»** — инвертирует экономику и создаёт стимул держать юзеров blocked-by-default.

### 9.6 Прозрачность

- **Network log в UI** (всегда видимый, Ctrl+Shift+N для деталей):
  - сколько запросов, куда, сколько байт, что заблокировано.
- **Permission UI** — каждое разрешение (камера/гео/нотификации) отдельным prompt, по умолчанию `deny`. Никаких «remember for this site» автоматически.
- **No silent network** — если что-то идёт во время idle (телеметрия, prefetch, update check), это видно и отключаемо.

### 9.7 Принципиальный отказ

- Никакой телеметрии, ни анонимной, ни «opt-in» по умолчанию.
- Никаких облачных аккаунтов в браузере.
- Никаких поисковых подсказок «из коробки» (опт-ин в настройках).
- Никаких «recommended extensions» магазинов.
- Никакой phone-home, кроме проверки обновлений (можно отключить).

### 9.8 Диагностика и crash reports

Расширение принципа №7: **диагностика — обязательно локальная, никогда не отправляется автоматически.** Это касается и crash dump-ов, и developer-log-ов, и performance-трейсов. Если что-то выходит наружу — только потому, что пользователь сам приложил файл к bug report.

**Три потока диагностической информации:**

| Слой | Кому | Где живёт | Видимость |
|---|---|---|---|
| Network log | Пользователю (real-time) | UI-панель (§9.6) | Всегда видна, Ctrl+Shift+N для деталей |
| Developer log | Разработчику / advanced user | stderr (по умолчанию); файл — только при явном `--log-file <path>` | По умолчанию `warn`+, фильтр через `LUMEN_LOG=lumen_network=debug` env var |
| Crash dump | Разработчику через пользователя | `<profile>/crashes/lumen_<timestamp>.log` (текстовый) | Никогда не отправляется автоматически. Пользователю показывается путь и фраза «приложите этот файл к issue» |

**Структура crash dump-а:**
- Версия Lumen, target triple, флаги сборки.
- Stacktrace (если доступен — Rust panic message + backtrace).
- **Последние 50 событий из `EventSink`** — даёт контекст «что делал браузер за миг до падения» без необходимости включать verbose-logging заранее. Это и есть причина, по которой `EventSink` (§9.6) — центральная подсистема, а не «опция».
- Содержимое open-tabs snapshot (URL + title, без cookies и form-state — последние утечь не должны).
- Список загруженных WASM-плагинов и их capability-токенов.

**`lumen --diagnose <path>` CLI:**
- Собирает версию, env, конфиг профиля (без секретов), последние N developer-log-ов, last crash dump в один txt-файл.
- **Не отправляет ничего.** Просто пишет файл и сообщает путь.
- Идиоматичный сценарий: пользователь натолкнулся на баг → `lumen --diagnose ~/lumen-bug.txt` → прикладывает к issue.

**Логирование как trait, не зависимость:**
- Свой минимум: `log!(level, target, "msg", k=v)` макрос пишет в стуб (stderr / файл / EventSink-наблюдателя), без `tracing` / `log` крейтов. ~200 строк, никаких новых dep.
- Через `EventSink::emit` идут структурированные события (`Request*`, `Tab*`, `Navigation`, `PageLoaded`); developer-log — отдельный поток для «плоских» сообщений (parser error, layout warning).
- Если потом упрёмся в необходимость span-trace для перформанса — пересмотрим, возможно tracing как exception #5.

**Дополнения к `EventSink` (см. §9.6):**
- ✅ **`RequestFailed { tab_id, url, stage, reason }`** — событие для DNS / connect / TLS-ошибок **до** `RequestCompleted`. Делает явным invariant «Started без Completed = failure»: ровно один из `RequestCompleted` / `RequestFailed` / `RequestBlocked` следует за каждым `RequestStarted`. `RequestStage` (`Dns` / `Tcp` / `Tls` / `Read`) в `lumen-core::event`, классификация по префиксу `Error::Network`-сообщения (`classify_failure_stage` в `lumen-network`), эмит симметрично `RequestStarted` на обоих `fetch_single` call-site (preflight + actual). Shell network-panel wiring (`record_failed`) — handoff P3.
- ✅ **Crash hook на `EventSink`** — `CrashRecorder` (`lumen-core::crash`) — декоратор `EventSink` с кольцевым буфером последних 50 событий (configurable, опциональный downstream-sink). `install_panic_hook(dir)` ставит process-global `std::panic::set_hook`, который при панике пишет дамп (`format_crash_dump`: текст паники + snapshot буфера) в `lumen-crash-<unix_ms>.log` через `write_crash_dump`, затем вызывает прежний hook. Чистые куски (`format_crash_dump` / `write_crash_dump`) юнит-тестируемы отдельно от panic-hook. Shell wiring (`CrashRecorder::install_panic_hook` при старте, рекордер в цепочке `EventSink`) — handoff P3.

**Чего НЕ делаем:**
- ❌ Sentry / Bugsnag / любые SaaS crash-aggregator-ы.
- ❌ Анонимный «opt-in» сбор статистики падений. Любая статистика — это телеметрия, см. §9.7.
- ❌ Автоматический «send report?» dialog. Только пользователь решает, что и куда отправлять.

---

