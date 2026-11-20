# Lumen — что умеет браузер, для кого он и как им управлять

Сводный документ для пользователя и разработчика. Источники: `CAPABILITIES.md`
(верифицировано по коду), `crates/shell/src`, `crates/storage/src/browser_settings.rs`,
`docs/automation.md`, `docs/commands.md`.

**Верификация:** флаги, env-переменные, ключи `settings.db` и `fingerprint.toml`
сверены напрямую с кодом ветки `devin-travel` (worktree `.claude/worktrees/devin-work`):
`cli_args.rs::print_usage`, `config.rs::apply_key`, `browser_settings.rs::set_*`.
Свойства подсистем — по `CAPABILITIES.md` плюс точечная проверка наличия модулей
в `crates/shell/src/` (hibernation, MSE, threat_feeds, password_store, extensions и др.).
NB: скины chrome (`zen`/`deck`/`midnight`) существуют только в ветке `devin-travel` —
в `main` их нет.
Snapshot: Phase 2 «Interactive», app v0.5.0, ~21 crate, WPT 44.80 %.

---

## 1. Что такое Lumen и для кого

**Lumen** — приватный, лёгкий браузер на **собственном движке** (не обёртка над Chromium/WebKit).
Весь конвейер свой: HTML-парсер → DOM-арена → CSS-каскад → layout → paint → Rust-оболочка (winit + wgpu/femtovg).
JS-движок один — **V8** (`rusty_v8` 150.x), persistent-контекст на вкладку.

**Для кого:**

- тем, кто не хочет «чёрный ящик» Chromium — движок open-source, каждое решение можно прочитать;
- приватникам — щиты, Tor, GPC, fingerprint-режимы, отсутствие телеметрии;
- пользователям российских сайтов — TLS-рукопожатие Chrome-уровня (BoringSSL), корни Windows,
  промежуточные по AIA, НУЦ-догрузка, HTTP/3;
- разработчикам — встроенная автоматизация (MCP, BiDi, CDP-stub, IPC), дампы layout/paint, golden-тесты;
- минималистам — скины chrome (`default`, `midnight`, `contrast`, `compact`, `zen`, `deck`),
  «Дзен»-режим, вертикальные/деревянные вкладки, workspaces, контейнеры.

**Плюсы vs Chromium-браузеры:**

- нет Google-сервисов и телеметрии в принципе — код не содержит их;
- скорость на простых страницах: нет лишних процессов, persistent V8, bfcache, сон вкладок (T3);
- приватность по умолчанию: щиты + fingerprint-профили + per-site разрешения + Tor;
- прозрачность: `--dump-layout`, `--dump-display-list`, `--trace-nav`, golden-gate — отладка без DevTools-зависимости;
- русскоязычный UX: страницы ошибок, «сайт работает не так?», отчёт о поломке, раскладка ЙЦУКЕН↔QWERTY.

---

## 2. Возможности по подсистемам

### Движок: парсинг и DOM (`lumen-dom`, `lumen-html-parser`, `lumen-css-parser`)

- ✅ HTML5-парсер: все 23 insertion mode, токенизатор FSM, ~250 сущностей, quirks-detection,
  `<template shadowrootmode>` (declarative shadow DOM), инкрементальный push-парсинг,
  preload-scanner (ранние `<link>`/`<img>`/`<script>` подсказки).
- ✅ DOM: арена `Vec<Node>` без `Rc/RefCell`, shadow DOM + `<slot>`, включая UA-shadow-деревья
  `<video>`/`<audio>`/`<select>`/`<details>`; contenteditable + undo/redo; drag&drop; выделение текста.
- ✅ CSS: ~139 свойств end-to-end, селекторы L3+L4 (`:has`, `:is`/`:where`/`:not`, `:nth-*(of)`,
  `:lang`/`:dir`/`:target`/`:state()`, `:heading`), at-rules: `@media`, `@supports` (typed evaluate),
  `@layer`, `@scope`, `@container` (включая scroll-state queries), `@font-face`, `@keyframes`,
  `@property`, `@import` с загрузкой файлов и `layer()`/`supports()`-гейтами, `@color-profile` (ICC),
  `@function`/`@mixin`/`@apply`. Динамически вставленный `<style>` применяется на лету.
- ✅ Кодировки: UTF-8/16/32, BOM, весь WHATWG single-byte набор (windows-1251, KOI8-R, IBM866…),
  авто-детект по BOM/meta/http/эвристике; URL-кодирование query по кодировке документа.
- ⬜ Legacy CJK (Shift_JIS, GBK, Big5), CDATA, `<plaintext>`.

### Layout и рендер (`lumen-layout`, `lumen-paint`, `lumen-font`, `lumen-image`, `lumen-canvas`)

- ✅ Поток: block/inline, margin-collapsing, `margin:auto`, flexbox полный (grow/shrink/basis,
  wrap, gap, **baseline alignment**, auto-margins на обеих осях), inline-block/atomic inlines
  с правильным baseline, CJK-переносы по UAX #14 (ICU4x), тайский/кхмерский/мьянманский по словарю,
  `line-break`/`word-break`/`white-space` L3–L4, `hyphens:auto` (Knuth-Liang, 11 локалей).
- ✅ Grid, table, transform/stacking contexts, `content-visibility`, container queries.
- ✅ Шрифты: системные + `@font-face`, ICU4x line-break/grapheme/bidi (UAX #9/#14/#29).
- ✅ Paint: wgpu (Vulkan/DX12) + femtovg-CPU fallback; display list с clip/transform/opacity-уровнями;
  скролл-композитор (page-compose), scrollable overflow, `overscroll-behavior`.
- ✅ Canvas 2D (битмапы композитятся в кадр), изображения (PNG/JPEG/WebP/GIF через `lumen-image`).

### JS и Web API (`lumen-js` — V8 150)

- ✅ Persistent V8-контекст на вкладку, DOM-шим (`_lumen_*`), workers, `postMessage`.
- ✅ Web API: `fetch`, XHR, WebSocket (+permessage-deflate, sub-protocol), EventSource (reconnect,
  `Last-Event-ID`, `retry:`), `URL`/`URLSearchParams`, `localStorage`/`sessionStorage`/IndexedDB-слой,
  `navigator.*` по fingerprint-профилю, `getSelection`/`Selection`, `contenteditable` API,
  `alert`/`confirm`/`prompt` (нативный диалог), `requestAnimationFrame`, таймеры, Promises, async/await.
- ✅ Trusted Types L2 частично (`setTimeout` string sink), CSP-enforcement для inline-скриптов и `img-src`.
- 🟡 Trusted Types остальные sink'и, module-скрипты частично, `<style>` из innerHTML (BUG-744).

### Сеть (`lumen-network`)

- ✅ HTTP/1.1 keep-alive + пул, **HTTP/2** (HPACK, один коннект на origin), **HTTP/3/QUIC**
  (RFC 9000–9001, QPACK, Alt-Svc, 0-RTT handshake-цепочка реализована по спецификации).
- ✅ TLS: BoringSSL ClientHello уровня Chrome, корни Windows (CurrentUser/LocalMachine/GPO),
  AIA-догрузка промежуточных, CT SCT-верификация, stapled OCSP, session tickets; HTTPS-инспекция
  Kaspersky/ESET совместима; `--dump-tls` — инспекция цепочки.
- ✅ Cookie jar (per-profile), CORS preflight+enforcement, Referrer-Policy полный,
  Fetch Metadata (`Sec-Fetch-*`), Priority hints + 103 Early Hints, port-blocking (83 «плохих» порта),
  HSTS + preload, CSP parsing/enforcement, Mixed-Content, COOP, Origin, Sandbox.
- ✅ Прокси: HTTP, SOCKS5 (proxy-side DNS — Tor-ready), системный прокси Windows (HKCU Internet Settings,
  `LUMEN_NO_SYSTEM_PROXY=1` отключает), per-site «без прокси» список (банки/Госуслуги/маркетплейсы).
- ✅ DNS: системный + DoH (RFC 8484) + DoT (RFC 7858); IDN→Punycode; EasyList/hosts ad-block
  с `$`-опциями (resource-type, third-party, domain); per-subscription фильтры в настройках.
- ✅ Кэш: `DiskHttpCache` (`data/cache/http_cache.db`) — RFC 7234 freshness, 304 revalidation,
  `Vary`, инвалидация по unsafe-методам; fetch()-кэш с `Request.cache` режимами; приватные/Tor —
  только в памяти.
- ✅ Загрузки через Range, HTTP auth Basic+Digest, WebSocket, SOCKS5/Tor (`--tor`, `--proxy`).

### Хранилище (`lumen-storage`)

- ✅ SQLite-хранилища: `settings.db`, `profiles.db`, workspaces, закладки, история (FTS5+bm25),
  read-later, заметки, cookie jar, HSTS, CSP-политики, сессии (`.lsession`).
- ✅ Профили: `ProfileRegistry` — имя, `storage_path`, `settings_json`, `encrypted_key_blob`;
  4 сидовых (Личный/Рабочий/Анонимный/Гость) со своими акцентами `data-profile`.
- 🟡 Полная per-profile изоляция данных — DS-16 (отложено); сейчас изоляция на уровне
  storage_path профиля, общий кеш/некоторые журналы.

### Оболочка (`lumen-shell`) — UI

**Вкладки и навигация:**
- ✅ Неблокирующая навигация: fetch/layout/JS на worker-потоке, progressive paint,
  streaming parse — окно не фризит; bfcache, **гибернация вкладок двух уровней**
  (T2 «фоновые старые» — состояние в SQLite с баннером «Восстановление вкладки…»;
  T3 «спящие» — заморозка DOM+JS с фуллскрин-спиннером при пробуждении >200 мс),
  восстановление сессии, reopen closed (Ctrl+Shift+T), вкладки-группы, контейнеры
  (изоляция cookie + цветная полоса), древо-вкладок и вертикальный сайдбар,
  lazy background-вкладки (Ctrl+click, средняя кнопка), link-modifiers полный набор.
- ✅ Сплит-вью, PiP трёх видов (in-page карточка, OS-окно для `<video>`,
  Document PiP — отдельное always-on-top окно с авторским CSS), reader view,
  find-in-page (Ctrl+F, «е»↔«ё», NBSP=пробел, метки на скроллбаре), source view,
  заметки к фрагментам, read-later очередь, speed-dial на newtab.
- ✅ Мультимедиа: `<video>`/`<audio>` с нативными контролами, **MSE**
  (`blob:MediaSource` — `appendBuffer` из страницы играет в нативном плеере),
  SMIL-анимации, Media Session (кнопки ОС), Document PiP.

**Пространства и профили:**
- ✅ Workspaces (`workspaces.db`): именованные группы вкладок, цвет, `Ctrl+Shift+W`,
  опциональная изоляция cookie; нижняя панель-чипы `[Default ×][Work ×][+]`.
- ✅ Профили: dropdown-переключатель, 4 сидовых, акцент через `data-profile`;
  «Анонимный»/«Гость» — без записи на диск, «режим толпы» `en-US,en`+UTC,
  «Личный»/«Рабочий» — язык и часовой пояс системы.
- ✅ Контейнеры: изоляция cookie/storage на вкладку, цветные страйпы.

**Приватность и безопасность:**
- ✅ Щиты (adblock + трекеры + fingerprint), per-site разрешения (камера/микрофон/
  уведомления/гео), GPC (`Sec-GPC: 1`), fingerprint-профили (Chrome/Firefox/Safari/Edge/
  Tor/Lumen/Strict), Tor-режим (SOCKS5, отдельный профиль, без записи),
  DoH, «Принудительный HTTPS» (в разработке), safe-browsing-подобные предупреждения
  на cert-error interstitial.
- ✅ **Safe Browsing** (`threat_feeds.rs`/`threat_store.rs`): открытые фиды угроз
  (URLhaus — вредоносное ПО, OpenPhish и Phishing.Database — фишинг) качаются в
  `safe_browsing.db` целиком и проверяются локально; адрес никуда не уходит;
  обновление раз в 6 часов, сетевая ошибка не трогает прежние записи.
- ✅ «Сайт работает не так?» — чип-фрустрация (3 перезагрузки/белый экран/антибот-петля),
  панель с лесенкой шагов (перезагрузить без кэша → сброс данных → другой браузер),
  `about:compat` с пакетом исправлений совместимости.
- ✅ Передача во внешний браузер: страница ошибки → «Открыть в другом браузере» с причиной
  (ГОСТ/ЭП, DRM, звонки, «не работает»), правила «всегда открывать там» на 30 дней.
- ✅ Отчёт о проблеме сайта (`about:report`): 16 причин, обязательный предпросмотр
  с вычисткой PII, экспорт в файл — приёмника нет.

**Уведомления и диалоги:**
- ✅ `alert`/`confirm`/`prompt` — нативный `#dialogBox`, очередь, Enter/Esc.
- ✅ «Страница не отвечает» — `#hangBar` через 5 с (`LUMEN_SCRIPT_STALL_MS`),
  «Остановить скрипт» = `TerminateExecution`; UI-поток не ждёт engine >1.5 с.
- ✅ CapsLock/раскладка-индикатор над password-полями; автоперевод ЙЦУКЕН↔QWERTY
  в омнибоксе и Ctrl+F; spellcheck с per-language словарями.

**Ввод и доступность:**
- ✅ Vim-режим (`input/vim.rs`: Normal/Insert/…), режим link-hints
  (`hint_mode.rs` + `hints`: буквенные метки на ссылках, активация с клавиатуры),
  жесты мышью (правый drag → навигация/команды вкладок), `accesskey`,
  scroll-to-text-fragment (`#…:~:text=…` — переход к фрагменту текста в URL).
- ✅ `lumen-a11y`: a11y-дерево, `query_a11y` (getByRole), focus-трекинг,
  `forced-colors`, полная клавиатурная навигация.

**Современные веб-фичи:**
- ✅ CSS View Transitions (`document.startViewTransition` с событиями и
  кроссфейдом в композиторе), Paint Worklet / Houdini
  (`background-image: paint()` рисует битмап на запрос), `caret-shape`/
  `caret-animation`, `interactivity: inert`, `input-security`.
- ✅ Resource Timing для engine-загружаемых ресурсов, site memory —
  prefetch известных сабресурсов со второй загрузки (PERF-15),
  параллельный fetch сабресурсов, prefetch-кэш (BUG-171).

**Данные и учётные записи:**
- ✅ **Менеджер паролей**: `logins.db` — записи AES-256-GCM + отдельный
  `logins.key` (защита от чтения файла БД); предложение «сохранить?» при
  сабмите формы.
- ✅ **Автозаполнение**: `autofill.db` — открытые значения форм (без паролей);
  платёжные карты хранятся только по явной кнопке «Сохранить карту» в
  `logins.db` под служебным origin.
- ✅ **Локальный перевод страниц** (`translate_manager.rs`, UX-TRANSLATE):
  перевод текстовых узлов локальной моделью в фоновом потоке; язык и модель —
  в настройках.
- ✅ **Самообновление** (`update.rs`, UPD-трек): подписанный манифест
  `latest.json` (проверка подписи до применения), бэкап `data/*.db` перед
  миграцией, авто-проверка раз в сутки.
- ✅ **Расширения — фаза 0** (`extensions/mod.rs`): загрузка из
  `data/extensions/<id>/manifest.json`, инъекция `content_scripts`,
  stub `chrome.runtime.sendMessage`. Полноценного API нет — заглушка.
- ✅ **Страница падения вкладки** (UX-CRASH): паника в обработчике события
  не убивает браузер — вкладка показывает «Страница упала» с reload;
  engine-поток переживает панику.

**Хром и скины:**
- ✅ Весь chrome рендерится собственным движком из `assets/chrome/chrome.html`
  (ADR-021, legacy-рендер удалён в CC-15-6). Скины `body[data-skin]`:
  `default`, `midnight` (OLED-тёмная), `contrast`, `compact`, `zen` (минимализм,
  кнопка-луна в тулбаре, запоминает предыдущий скин), `deck` (layout-скин:
  топ-бар + filmstrip вкладок + coverflow-обзор + нижний dock, `body[data-deck-cards]`,
  `#contentAreaDeck` — второй page-host). Дефолт скина — `deck` (DEFAULT_SKIN в
  `browser_settings.rs`, переопределяется записью в settings.db).
- ✅ Дизайн-система v3.3: `theme_tokens.rs` (радиусы/отступы/поверхности/акценты профилей),
  ~22 панели на Palette-токенах, светлая/тёмная/системная темы.

**Автоматизация и диагностика:**
- ✅ `--dump-source` / `--dump-layout` / `--dump-display-list` / `--screenshot` /
  `--trace-nav` / `--print-to-pdf` / `--dump-tls` — headless one-shot.
- ✅ MCP JSON-RPC: 8 инструментов (`navigate`, `new_tab`, `wait`, `click`, `type`,
  `scroll`, `eval`, `query`) + ресурсы (screenshot/a11y/layout/console/network) +
  L1-инструменты `x-explain-element`, `x-explain-page`, `x-scope-*`, `x-computed-style`,
  tab-инструменты `list_tabs`/`switch_tab`/`close_tab`.
- ✅ WebDriver BiDi (`--bidi-port`): navigate/screenshot/setViewport/evaluate/
  callFunction/preload/input.performActions/network intercept/session/storage/
  timezone/userAgent-override.
- ✅ IPC TCP (`--ipc-server`): CreateTab/NavigateTab/Screenshot/CloseTab.
- ✅ DevTools CDP stub (`--devtools-port`).
- ✅ `lumen-driver`: `BrowserSession` для Rust-тестов (layout_box_by_selector,
  computed_style_snapshot, query_a11y, network_log).
- ✅ Session-health журнал (`--health-log` → JSONL), activity/click-логи,
  `--import-session` (восстановление `.lsession`), детерминизм (`--deterministic`,
  `--rng-seed`, `--monotonic-clock`, `--viewport`).

**Доступность:** `lumen-a11y` — a11y-дерево, `query_a11y` (getByRole-стиль),
focus-трекинг, `forced-colors` режим, keyboard-навигация полная.

---

## 3. Настройки (`about:settings` → `settings.db`)

Хранилище: `<exe_dir>/data/settings.db`, снапшот `BrowserSettingsSnapshot`
(`crates/storage/src/browser_settings.rs`). Все изменения применяются сразу и переживают рестарт.

| Настройка | Ключ | Что делает |
|---|---|---|
| Домашняя страница | `homepage` | URL стартовой/new-tab страницы |
| Поисковый движок | `search_engine_id` | ID провайдера для омнибокса (0 = нет; сейчас Google, выбор в разработке) |
| Щиты | `shields_enabled` | Глобальный вкл/выкл блокировщика (реклама+трекеры+fingerprint) |
| Режим fingerprint | `fingerprint_mode` | `"standard"` / `"strict"` / `"off"` — уровень защиты от фингерпринтинга |
| DoH | `doh_enabled` | DNS-over-HTTPS глобально (RFC 8484) |
| Размер шрифта | `font_size` | Base font size в px (по умолчанию 16.0) |
| Тема | `theme` | `"dark"` / `"light"` / `"system"` |
| Путь загрузок | `download_path` | Абсолютный путь; пусто = системная «Загрузки» |
| Раскладка вкладок | `tab_layout` | `"horizontal"` (лента сверху) / `"vertical"` (сайдбар); есть и tree-режим в UI |
| Раскладка панелей | `panel_layout` | Сериализованная раскладка док-панелей; пусто = дефолт |
| Скин chrome | `skin` | `default`/`midnight`/`contrast`/`compact`/`zen`/`deck`; дефолт `deck` |
| Язык перевода | `translate_language` | Целевой язык локального перевода страниц |
| Модель перевода | `translate_model` | Какая локальная модель используется |
| Пакет совместимости | `compat_pack_off` | Глобальный выключатель `about:compat`-записей |
| Отключённые записи compat | `compat_disabled_ids` | Список id записей пакета, выключенных пользователем |
| Журнал compat | `compat_journal` | Локальный журнал применённых исправлений совместимости |
| Лимитер чипа «сайт не так» | `compat_chip_limiter` | Счётчики/пороги показа чипа-фрустрации |
| Сообщённые сайты | `compat_reported_sites` | Локальный дедуп-список сайтов, о которых уже сообщали |
| НУЦ-хосты | `nuc_consent_hosts` | Хосты, для которых показывается согласие на НУЦ-сертификаты |
| Язык UI / spellcheck | `data/spell/prefs.txt` | Мастер-тумблер + per-language словари орфографии |
| Автообновление | settings | Проверка `latest.json` раз в сутки, без cookies/идентификаторов |
| Восстановление сессии | settings | Вкладки+позиции при перезапуске |
| Reader view по умолчанию | settings | Статьи сразу в режиме чтения |
| HTTP/3 | `fingerprint.toml` | QUIC on/off, применяется со следующего запуска |
| Список «без прокси» | `no_proxy_sites` | eTLD+1-хосты, идущие мимо HTTP/SOCKS прокси и Tor |
| Акцент темы | per-profile | Цвет акцента из `data-profile` токенов |
| Сессия | `*.lsession` | Снапшот URL+scroll для `--import-session` |

**`fingerprint.toml`** (`<exe_dir>/data/fingerprint.toml`, плоский `key = value`)
— все ключи, проверено по `config.rs::apply_key`:

| Ключ | Что задаёт |
|---|---|
| `http_profile` | HTTP-фингерпринт: `chrome`/`firefox`/`safari`/`edge`/`lumen`/`strict`/`tor` |
| `tls_profile` | TLS ClientHello: `standard`/`strict`/`tor` (пусто = выводится из http_profile) |
| `hardware_concurrency` | `navigator.hardwareConcurrency` |
| `device_memory` | `navigator.deviceMemory` (ГиБ) |
| `platform` | `navigator.platform` |
| `languages` | `navigator.languages` (первый = `language`); явный ключ сильнее OS-списка |
| `noise_exceptions` | Хосты, где выключен canvas/audio fingerprint-шум |
| `screen_width` / `screen_height` | `screen.width`/`height`/`availWidth`/`availHeight` |
| `color_depth` | `screen.colorDepth`/`pixelDepth` |
| `timezone_offset` | Часовой пояс в минутах (+ = за UTC); сильнее режима профиля |
| `doh_url` | URL DoH-резолвера (пусто = выкл / системный DNS) |
| `proxy` | HTTP-прокси `http://host:port` |
| `socks5_proxy` / `socks5` | SOCKS5 `socks5://[user:pass@]host:port` |
| `no_persistent_state` | `true` — ничего не писать на диск (cookie/storage в памяти) |
| `http3` | `true` — QUIC/HTTP3 по Alt-Svc |
| `identity` | `chrome_lumen` (дефолт) / `chrome_unbranded` — токен `Lumen/x.y` в UA и CH-бренде |

Неизвестные ключи игнорируются; пустые значения оставляют дефолт.

**Per-site разрешения** (поповер щитов на сайте): камера, микрофон, уведомления,
геолокация, pop-up'ы — Allow/Block/Ask на каждый сайт, хранятся per-origin.

---

## 4. Флаги командной строки и env-переменные

### Headless one-shot (без окна)

| Флаг | Что делает |
|---|---|
| `--dump-source <src>` | Декодированный HTML после парсера → stdout |
| `--dump-layout <src>` | Box-tree геометрия → stdout (текст-диффабельно) |
| `--dump-display-list <src>` | Paint-команды по порядку → stdout |
| `--screenshot <out.png> <src>` | CPU-скриншот (tiny-skia), без GPU; JS проходит один синхронный проход + load + ≤4 rAF |
| `--trace-nav <out.json> <src>` | Load-waterfall в Chrome Trace формате (Perfetto) |
| `--print-to-pdf <out.pdf> <src>` | Печать в PDF (A4) |
| `--dump-tls <host\|url\|@file>...` | TLS-проба: issuer, цепочка, якорь, SCT, OCSP — JSON на хост |
| `--viewport WxH` | Размер вьюпорта для всех headless-режимов (default 1024×720) |

### Серверы автоматизации (все на 127.0.0.1, **все требуют токен** — ADR-024)

| Флаг | Протокол | Токен |
|---|---|---|
| `--mcp [url]` / `--mcp-port N` | MCP JSON-RPC (headless, InProcessSession) | не нужен для stdio |
| `--mcp-live-port N <src>` | MCP к живому окну | `[mcp] token:` в stderr → `initialize.params.token` |
| `--bidi-port N` | WebDriver BiDi (WS) | `[bidi] token:` → `session.new.capabilities.alwaysMatch.token` |
| `--ipc-server [--ipc-port N]` | bincode TCP (CreateTab/Navigate/Screenshot/CloseTab) | `LUMEN_IPC_TOKEN=` в stdout, первый пакет `Auth` |
| `--devtools-port N` | CDP-over-WebSocket (минимальный stub) | — |

`lumen-mcp --attach <port> --token-file <f>` — stdio↔live мост; `--launch` поднимает окно сам.

### Живое окно и прочее

| Флаг / env | Что делает |
|---|---|
| `--maximized` | Окно на весь экран (обязателен для live-тестов реальных сайтов) |
| `--no-paint` / `LUMEN_NO_PAINT=1` | DOM+JS+layout без растеризации (не для скриншотов) |
| `--no-scrollbar` | Чище скриншоты |
| `--deterministic` | Фиксированные Date.now/random/rAF для тестов |
| `--rng-seed N` | Переопределить seed `Math.random` |
| `--monotonic-clock` | Часы +1мс на вызов вместо замороженных |
| `--activity-log` / `--click-log` | Журнал действий → `activity.log` |
| `--health-log` / `LUMEN_HEALTH_LOG=1` | JSONL-журнал здоровья сессии (panic/console.error/load fails) |
| `--import-session <file.lsession>` | Восстановить URL+scroll из снапшота |
| `--network-service` | Сетевой субпроцесс (сейчас транспорт не используется — BUG-769) |
| `--proxy <url>` | HTTP/SOCKS5 прокси (перекрывает системный и fingerprint.toml) |
| `--tor [--tor-port N]` | Tor-режим: SOCKS5 на 127.0.0.1:9050, отдельный профиль, без записи |
| `--forced-colors` / `LUMEN_FORCED_COLORS=1` | Forced Colors Mode до первого layout |
| `--mcp-live-port` + `LUMEN_FRAME_LOG=1/2/3` | stderr-лог кадров; =2 фазы wgpu, =3 per-command collect |
| `LUMEN_MEM_REPORT=1` | Дамп памяти ~10с (GPU bytes, offscreen pool) |
| `LUMEN_PROFILE_TREE=1` | Дерево таймингов layout/cascade в stderr |
| `LUMEN_PROFILE_DETAIL=1` | Per-node разбивка внутри cascade |
| `LUMEN_JS_STALL_SAMPLE_MS=N` | Watchdog: JS-стек длинной задачи каждые N мс |
| `LUMEN_SCRIPT_STALL_MS=N` | Порог «страница не отвечает» (default 5000) |
| `LUMEN_STARTUP_LOG=1` | Разбивка pre-main/config-load/arg-parse/services-init |
| `LUMEN_NO_SYSTEM_PROXY=1` | Игнорировать системный прокси Windows |
| `LUMEN_IDENTITY=chrome_unbranded` | Идентичность без `Lumen`-токена (A/B-тесты) |
| `LUMEN_LEGACY_CHROME` | Удалён в CC-15-6 — legacy chrome-рендер больше не существует |
| `cargo run --features tracy` | GUI-профилировщик Tracy |

**Семейство `LUMEN_NO_*` / `LUMEN_*` тюнинга** (проверено grep'ом по `env::var` в коде):
~100 переменных, в основном перф-бисекция и отладка — `LUMEN_NO_SCROLL_COMPOSITOR`,
`LUMEN_NO_BAND_*`, `LUMEN_NO_*_CACHE`, `LUMEN_BACKEND`, `WGPU_BACKEND`,
`LUMEN_RENDER_THREAD`, `LUMEN_ENGINE_THREAD`, `LUMEN_NO_ENGINE_THREAD`,
`LUMEN_EAGER_PIPELINES`, `LUMEN_WAIT_HOT_PIPELINES`, `LUMEN_SERIAL_PIPELINES`,
`LUMEN_TEXTURE_POOL_MB`, `LUMEN_TEXT_GAMMA`, `LUMEN_SITE_MEMORY[_FILE]`,
`LUMEN_STALL_DUMP_DIR`/`LUMEN_STALL_SNAPSHOT`, `LUMEN_BENCH[_ITERS]`,
`LUMEN_PRESENT[_LOG]`, `LUMEN_DISABLE_AUDIO_NOISE`/`CANVAS_NOISE`,
`LUMEN_CPU_SYSTEM_FONTS`, `LUMEN_ALLOW_POPUPS`, `LUMEN_NO_ADBLOCK`,
`LUMEN_TRANSLATE_MODEL`, `LUMEN_FF_PROFILE_DIR`, `LUMEN_NATIVE_CALL_STATS`,
`LUMEN_SHARECACHE_STATS`, `LUMEN_ESM_TRACE`, `LUMEN_NO_ESM_PREFETCH`,
`LUMEN_VERIFY_*` (проверки инкрементальности), `LUMEN_ALIGN_*` (WPT-выравнивание),
`SAVE_CPU_SNAPSHOTS`/`UPDATE_SNAPSHOTS`/`SNAPSHOT_VS_EDGE_STRICT` (golden-тесты),
`FFMPEG_DIR`/`CARGO_FEATURE_FFMPEG`/`LUMEN_EXPECT_GSTREAMER` (медиа-бэкенды),
`XDG_CACHE_HOME`/`LOCALAPPDATA`/`APPDATA`/`USERPROFILE`/`HOME`/`WINDIR` (пути ОС).

---

## 5. Чего пока нет (честные ⬜)

- Полная per-profile изоляция данных (DS-16), выбор режима идентичности на профиль;
- Приёмник для `about:report` (отчёты только экспортируются в файл);
- Скриншот в отчёте о проблеме сайта;
- Trusted Types — большинство sink'ов, module-скрипты, `innerHTML`-`<style>`;
- CJK legacy-кодировки, CDATA, `<plaintext>`;
- AIA через QUIC/H3, `ldap://` в `.p7c`;
- Реальная сетевая очередь приоритетов (Priority — advisory), Early Hints по H2/H3;
- Process-изоляция вкладок (UX-PROCESS), per-tab диалоги/фон-вкладки
  (диалог показывается в окне спросившей вкладки, фон-вкладки не держат его);
- Полноценный API расширений (сейчас Phase 0: manifest + content_scripts + stub);
- «Принудительный HTTPS» — переключатель в настройках пока не применяется;
- Выбор поискового движка — в разработке (сейчас Google);
- Часть `@container`-случаев (display-switching контейнеры, псевдоэлементы);
- Правка `image-set()`/`src()` строк в `@import`-rebase;
- `--network-service` — заглушка: субпроцесс поднимается, но запросы через него
  не идут (BUG-769).
