# Lumen — браузер на Rust с собственным движком

> **Lumen** (лат. *свет*, единица светового потока) — приватный, лёгкий, прозрачный браузер. Имя отражает философию проекта: показывать пользователю всё, что происходит, и не быть тяжелее, чем нужно.

## 🔄 В работе сейчас

Задачи, взятые в работу параллельными сессиями. **Не дублировать.** Подробнее о протоколе — в `CLAUDE.md`, раздел «Координация параллельных сессий».

- 🔄 case-insensitive `[attr=val i]` — `css-attr-case-insensitive` — 2026-05-12

## Статус реализации

**Текущая фаза:** Phase 0 (прототип). Этот блок обновляется при каждом коммите, реализующем пункт плана. Условные обозначения: ✅ готово · 🟡 в работе · ⬜ запланировано.

### Инфраструктура
- ✅ Cargo workspace, 10 крейтов
- ✅ `rust-toolchain.toml` (stable + rustfmt + clippy)
- ✅ `.gitattributes` (LF в репо, кросс-платформенные line endings)
- ✅ Ветка `main`, локальные коммиты, без remote

### Крейты
- ✅ `lumen-core` — типы и trait-ы: `Error`, `Url`, `Event`, `Capability`, `Module`, геометрия (`Rect`, `Point`, `Size`), `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `EncodingDetector`
- ✅ `lumen-dom` — арена + `NodeId` + `Document/Node/NodeData`, API: create/append/detach/Display, 7 тестов (включая кириллицу)
- 🟡 `lumen-shell` — точка входа: три режима (пустое окно / файл / URL). Внешний CSS через `<link rel=stylesheet>`: загружается с диска (относительно HTML-файла) или по сети (относительно базового URL). Bundled Inter-Regular.ttf через `include_bytes!`
- 🟡 `lumen-html-parser` — минимальный токенизатор (Data/Tag/Attribute/Comment, named + numeric entities) + lenient tree builder. 31 тест (включая кириллицу). Отложено: DOCTYPE-разбор, CDATA, raw-text script/style, полный набор named entities, insertion modes
- 🟡 `lumen-css-parser` — расширенные селекторы: simple (type/class/id/universal/attribute/pseudo), compound (`p.foo#bar`), complex с combinator-ами (` `, `>`, `+`, `~`); attribute-операторы `=`, `~=`, `|=`, `^=`, `$=`, `*=`; structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`); функциональные pseudo (`:nth-child(an+b)`, `:nth-last-child`, `:nth-of-type`, `:nth-last-of-type` с ключевыми словами `odd`/`even`; `:not(compound)`; **CSS4 `:is(selector-list)` и `:where(selector-list)`** — selector-list внутри, specificity = max-of-list для :is, 0 для :where); interactive (`:hover` и т.д.) парсятся, не матчат; pseudo-elements `::name` (парсятся, не матчат). Specificity по CSS Selectors Level 3+4. 72 теста. Отложено: `:has(...)`, `:not(complex)`, case-insensitive `[a=v i]`, namespace prefix, типизированные значения деклараций
- 🟡 `lumen-layout` — block-flow + **inline-flow** с specificity-based style cascade и line wrapping: compound и complex selectors (combinators, attribute, structural и функциональные pseudo, `:not`), наследование (color, font-size, line-height, text-align, text-decoration), color (named + hex 3/4/6/8 digit + rgb/rgba/hsl/hsla с modern syntax), display (block/inline/none), margin/padding (включая shorthand), text-align (left/center/right), text-decoration (underline / overline / line-through, можно комбинировать, `none` сбрасывает), **width / height (px)**, **border (width/style/color, все shorthands, box model)**, **box-sizing (content-box / border-box)**. Length-units: px, em, rem, % (em/rem/% для font-size и line-height; % в margin/padding пока игнорируется до containing-block). `TextMeasurer` trait + `layout_measured()` для word-wrap по реальным шрифтовым метрикам. `InlineRun` объединяет текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`, `<strong>`, и т.д.) в один поток строк с per-сегментными стилями; каждый `InlineFrag` хранит свою ширину для align_lines и подрисовки декорации. `serialize_layout_tree` + golden snapshot-тесты (`UPDATE_SNAPSHOTS=1` для регенерации). Отложено: flex/grid, float, абсолютное позиционирование, font-weight/style на inline-уровне
- 🟡 `lumen-paint` — display list (FillRect, **DrawBorder**, DrawText) + wgpu-растеризатор с двумя pipeline-ами (fill + text), glyph atlas 512×512, текстурированные квады из atlas-а. `DrawBorder` рендерится 4 fill-quad-ами (top/right/bottom/left edges), цвет с currentColor fallback. Под/над/перечёркивающие линии text-decoration эмитятся как FillRect-ы у baseline каждого фрагмента. `FontMeasurer` для TextMeasurer. Внешние зависимости: `wgpu` (exception #2), `winit` (exception #1)
- 🟡 `lumen-font` — собственный TrueType-парсер (head/maxp/cmap format 4+12/hhea/hmtx/loca/glyf) + scanline-растеризатор (квадратичные Безье, 4×4 AA, even-odd fill). cmap format 12 — Sequential Groups, полный Unicode U+10FFFF (эмодзи U+1F600+, SMP). 62 unit + 9 integration тестов. Отложено: hinting, GSUB/GPOS shaping, CFF outlines, variable fonts, color glyphs
- 🟡 `lumen-encoding` — детектор кодировок и однобайтовые декодеры (Windows-1251, KOI8-R, CP866). Пайплайн: BOM → `<meta charset>`-sniff (1 КБ) → HTTP content-type hint → UTF-8 валидность → частотная эвристика по русским буквам. Реализует `EncodingDetector` из `lumen-core::ext`. 41 тест (35 unit + 6 integration round-trip). Отложено: UTF-16 как отдельная кодировка, ISO-8859-5, MacCyrillic, prescan по HTML5 spec §12.2.3.2 (точные правила парсинга атрибутов)
- ✅ `lumen-network` — HTTP/1.1 + HTTPS клиент (rustls, exception #3). Redirect, chunked TE. `HttpClient` реализует `NetworkTransport`. 12 тестов.
- ✅ `lumen-storage` — in-memory KV + origin-партиционирование + snapshot LUMEN_KV_V1. 17 тестов.
- ⬜ `lumen-knowledge` (§12) — FTS-индекс над историей и заметками, read-later каталог. Phase 2
- ⬜ `lumen-ai` (§12.5) — опциональный, embedding + RAG поверх локального LLM. Phase 3+, feature-flag

### Политика зависимостей (§5)
- ✅ Зафиксирована: «default — своё». 4 разрешённых exceptions, всё остальное — свой код.
- ✅ Exception #1: `winit` (OS event loop) — за `WindowingBackend`
- ✅ Exception #2: `wgpu` (GPU API) — за `RenderBackend` — пока не подключён
- ✅ Exception #3: `rustls` (TLS / crypto) — за `TlsBackend` — активирован в `lumen-network`
- ✅ Exception #4: JS engine (`rquickjs` → `rusty_v8`) — за `JsRuntime` — пока не подключён

### Точки расширения (trait-ы из `lumen-core::ext`)
- ✅ `StorageBackend` — реализован в `lumen-storage::InMemoryStorage` (origin-партиционирование, snapshot LUMEN_KV_V1, 17 тестов)
- ✅ `NetworkTransport` — реализован в `lumen-network::HttpClient` (HTTP/1.1 + HTTPS через rustls, redirect, chunked, 12 тестов)
- 🟡 Интерфейсы: `SearchProvider`, `FilterListSource` — определены, реализаций нет
- ✅ `EncodingDetector` — реализован в `lumen-encoding::HeuristicDetector` (BOM + meta + content-type + UTF-8 + частотная эвристика)
- ⬜ Trait-ы для 4 exceptions: `WindowingBackend`, `RenderBackend`, `TlsBackend`, `JsRuntime` — задокументированы как future в `lumen-core::ext`, code-уровень добавим при первом использовании
- ⬜ `KnowledgeStore` (§12) — FTS / read-later / notes. Phase 2
- ⬜ `AiBackend` (§12.5) — embed / generate, опционально. Phase 3+

### Уникальные фичи (§12) — план на Phase 1-4
- ⬜ Tab session export / import (§12.7) — Phase 1
- ⬜ Полнотекстовый поиск по истории (§12.1) — Phase 2
- ⬜ Аннотации и заметки (§12.2) — Phase 2
- ⬜ Read-later / офлайн-чтение (§12.3) — Phase 2
- ⬜ Поиск по содержимому открытых вкладок (§12.4) — Phase 2
- ⬜ Focus mode (§12.6) — Phase 2
- ⬜ Кастомизация UI (drag&drop, темы) (§12.10) — Phase 2-3
- ⬜ Локальный AI layer (§12.5) — Phase 3+, опционально
- ⬜ Семантические закладки (§12.8) — Phase 3, зависит от AI
- ⬜ Граф знаний (§12.9) — Phase 3+
- ⬜ Кросс-устройственная синхронизация E2E (§12.11) — Phase 4+, требует mobile

### Локализация / RU (§10)
- ✅ DOM держит кириллицу (UTF-8) — зафиксировано тестами
- ✅ `Url::parse` принимает кириллические домены (тест на `президент.рф`)
- ✅ Encoding detection (cp1251, KOI8-R, CP866) — крейт `lumen-encoding`, подключён в shell
- ⬜ Cyrillic font fallback в paint
- ⬜ Punycode/IDN
- ⬜ Локаль ru-RU (дата/время/числа)
- ⬜ UI-переводы (Fluent)

### Следующие шаги
- 🟡 HTML parser — минимум готов; полный набор insertion modes / named entities / DOCTYPE-разбор — позже, по запросу
- 🟡 CSS parser — селекторы готовы (compound, combinators, attribute, structural+functional pseudo, `:not`, specificity); типизированные значения (length/color/calc), `:is/:where/:has` — позже
- 🟡 Layout — block-flow + inline-flow + style cascade (specificity) + word-wrap готовы; flex/grid, float, абсолютное позиционирование — позже
- ✅ Paint — display list + wgpu-rasterizer + glyph atlas + text rendering
- ✅ Связка движка с UI: shell открывает `samples/page.html` с фонами и текстом
- ⬜ Composite glyphs в lumen-font (Cyrillic 'А' и другие)
- ⬜ Свой HTTP/1.1 + TLS через `rustls` — для загрузки внешней страницы

---

## 0. Терминология

- **Lumen** — кодовое и публичное имя проекта. Бинарь: `lumen`, конфиг: `~/.config/lumen/`, репозиторий: `lumen`.
- **Browser** — общий термин: конечное приложение (окно, вкладки, UI, настройки).
- **Engine (rendering engine, browser engine)** — то, что превращает HTML+CSS+JS в пиксели: парсеры, DOM, layout, paint, compositor. Примеры: Blink, WebKit, Gecko, Servo, Ladybird.
- **JS engine** — отдельная подсистема, исполняющая JavaScript: V8, SpiderMonkey, JavaScriptCore, QuickJS.
- **WebView** — системный встраиваемый компонент (WebView2, WKWebView, WebKitGTK). Использует чужой движок.
- В этом плане мы пишем **rendering engine с нуля**, а JS-движок **встраиваем готовый** (см. §6.8).

---

## 1. Принципы проекта

1. **Приватность по умолчанию.** Никакой телеметрии, никаких аккаунтов, никаких облачных сервисов без явного включения.
2. **Лёгкость.** Цель — холодный старт < 300 мс, ~100 МБ RAM на пустой вкладке.
3. **Контролируемая поверхность.** Поддерживаем только нужные веб-API. Экзотика (WebUSB, WebBluetooth, WebMIDI, WebSerial, FedCM, Payment Request, WebNFC) — не реализуется.
4. **Прозрачность.** Каждый исходящий байт виден пользователю.
5. **Стабильный UI.** Минимализм, без «редизайнов» каждый релиз.
6. **Memory safety.** `unsafe` только на FFI-границах, всё ревьюится.
7. **Русский язык — first-class.** Кодировки, шрифты, IDN, локаль, переводы — на всех этапах, а не отложенная «фаза i18n». Подробности в §10.
8. **Knowledge layer как ценность для пользователя.** Браузер хранит и индексирует то, что пользователь видел / отметил / сохранил, локально. Это закрывает запросы, которые мейнстрим-браузеры не закрывают по бизнес-причинам: полнотекстовый поиск по истории, аннотации, офлайн-чтение, опциональный локальный AI-ассистент. Подробности в §12.

---

## 2. Реалистичный scope движка

Полный веб-стандарт нереалистичен. Мы целимся в **подмножество**, постепенно расширяя.

### v0.1 — «текстовый веб» (читалка)
- HTML5 (без `<form>` пока)
- CSS 2.1 + box model + блочный/инлайн layout
- Картинки (PNG, JPEG)
- HTTP/1.1, HTTPS
- Без JS

Цель: открывать энциклопедийные статьи, MDN, GitHub README, статьи блогов.

### v0.5 — «интерактивный читатель»
- CSS Flexbox
- Формы, базовый ввод
- JS через embedded QuickJS (без сложных Web APIs)
- HTTP/2
- WebFonts (WOFF2)

Цель: открывать форумы, Hacker News, простые SPA.

### v1.0 — «современный браузер»
- CSS Grid, transforms, animations
- Canvas 2D
- Полноценный JS через V8/SpiderMonkey
- DOM API (полное подмножество HTML Living Standard)
- Fetch, XHR, WebSocket
- IndexedDB, localStorage
- HTTP/3
- Service Workers (опционально)

Цель: открывать большинство сайтов, кроме самых тяжёлых SPA.

### Что НЕ берём (и в v1.0 тоже)
- WebGL, WebGPU (отдельный масштабный проект)
- WebRTC (медиа-кодеки = огромный отдельный мир)
- DRM / Widevine
- WebAssembly (можно добавить, когда JS-движок встанет)
- WebUSB / WebBluetooth / WebMIDI / WebSerial / WebNFC / Payment Request
- Native messaging для расширений
- PDF viewer (отдельным приложением или библиотекой)

---

## 3. Архитектура высокого уровня

```
┌──────────────────────────────────────────────────────────┐
│                    UI Process (shell)                    │
│   winit ▸ wgpu ▸ egui ▸ tabs ▸ omnibox ▸ shortcuts       │
└──────────────────┬───────────────────────────────────────┘
                   │ typed IPC (postcard over pipes)
       ┌───────────┼────────────┬──────────────┐
       ▼           ▼            ▼              ▼
   ┌────────┐  ┌────────┐  ┌────────┐   ┌────────────────┐
   │Renderer│  │Renderer│  │Renderer│   │ Network Service│
   │ proc 1 │  │ proc 2 │  │ proc N │   │  (one process) │
   │        │  │        │  │        │   │                │
   │ engine │  │ engine │  │ engine │   │ HTTP/TLS/DNS   │
   │ + JS   │  │ + JS   │  │ + JS   │   │ Filters/Cache  │
   └────────┘  └────────┘  └────────┘   └────────────────┘
                                                 │
                                        ┌────────┴────────┐
                                        │ Storage Service │
                                        │ cookies, idb,   │
                                        │ history (redb)  │
                                        └─────────────────┘
```

- **UI process** — единственный, кто рисует окно и принимает ввод.
- **Renderer process на каждый origin** — site isolation как в Chromium. Краш одной вкладки не валит браузер. Эксплойт в одной вкладке не лезет в другую.
- **Network service** — единственный, кто ходит в сеть. Все TLS, DNS, фильтры рекламы, кэш — здесь. Центральная точка приватности.
- **Storage service** — единственный, кто пишет на диск (кроме логов и кэша). Cookies, IndexedDB, история, закладки.

IPC через `postcard` (компактный, бинарный, serde-совместимый) поверх:
- Unix: `tokio::net::UnixStream`
- Windows: Named Pipes
- macOS: Unix Domain Sockets

---

## 4. Структура репозитория

```
lumen/
├── Cargo.toml                     # workspace
├── crates/
│   ├── shell/                     # UI process
│   ├── ipc/                       # типы сообщений, транспорт
│   │
│   ├── engine/                    # сам движок
│   │   ├── html-parser/           # токенизатор + tree construction
│   │   ├── css-parser/            # токенизатор + grammar
│   │   ├── dom/                   # DOM-дерево, события
│   │   ├── style/                 # каскад, computed values
│   │   ├── selectors/             # матчинг CSS-селекторов
│   │   ├── layout/                # box generation, layout algorithms
│   │   ├── paint/                 # display list, рисование
│   │   ├── compositor/            # слои, GPU-композитинг
│   │   ├── text/                  # shaping, bidi, line breaking
│   │   ├── image/                 # декодирование PNG/JPEG/WebP
│   │   ├── font/                  # загрузка шрифтов, WOFF2
│   │   └── js-binding/            # мост к JS-движку
│   │
│   ├── webapi/                    # реализация Web API
│   │   ├── dom-api/               # document.querySelector и т.д.
│   │   ├── fetch/                 # fetch(), XHR
│   │   ├── canvas/                # Canvas 2D
│   │   ├── storage/               # localStorage, sessionStorage
│   │   └── timers/                # setTimeout, requestAnimationFrame
│   │
│   ├── renderer/                  # renderer process: связывает engine + webapi
│   │
│   ├── network/                   # network service
│   │   ├── http/                  # HTTP/1.1, /2, /3
│   │   ├── tls/                   # rustls wrapper
│   │   ├── dns/                   # DoH, DoT, обычный
│   │   ├── cache/                 # HTTP cache
│   │   ├── cookies/               # cookie jar с партиционированием
│   │   └── filters/               # свой adblock-матчер
│   │
│   ├── storage/                   # storage service (redb)
│   │
│   ├── profiles/                  # управление профилями, шифрование
│   │
│   └── common/                    # общие типы, конфиг, URL parsing
│
├── assets/                        # иконки, default filter lists
├── tests/
│   ├── wpt/                       # Web Platform Tests subset
│   └── snapshots/                 # render snapshot tests
├── docs/
└── xtask/                         # build, release tasks
```

---

## 5. Технологический стек

### Политика зависимостей

**Default: пишем сами.** Lumen — это про собственный движок, не про обёртку над чужими крейтами. Каждая внешняя зависимость в `Cargo.toml` должна иметь обоснование в этом разделе.

Поэтому мы пишем **свой** код для:

- HTML / CSS парсеров, DOM, style cascade, selectors;
- layout (block, inline, flex, grid), paint, compositing;
- URL-парсинга, Punycode / IDN;
- HTTP/1.1, HTTP/2, DNS-резолвера с DoH/DoT;
- определения и конвертации кодировок (cp1251, KOI8-R, CP866 и др.);
- декодеров изображений (PNG, JPEG);
- TrueType-парсинга и text shaping для Latin / Cyrillic;
- bidi и line breaking по Unicode UAX #9 / #14;
- движка адблок-фильтров;
- 2D-растеризации поверх GPU-абстракции;
- KV-хранилища (минимальный B-tree с fsync);
- IPC, async-примитивов, work-stealing thread pool;
- UI-фреймворка (иммедиат-режим поверх своих paint-примитивов).

### Разрешённые exceptions (4 шт.)

Это единственные внешние зависимости, которые мы оставляем. Каждая прячется за trait в [`lumen-core::ext`](crates/core/src/ext.rs), чтобы при желании можно было заменить.

| Crate | Что покрывает | Trait-anchor | Почему не сами |
|---|---|---|---|
| **`winit`** | OS event loop, окна, ввод | `WindowingBackend` | Win32 + X11 + Wayland + AppKit — ~50–100k LOC платформенно-специфичных багов и behaviour quirks |
| **`wgpu`** | GPU API (Vulkan / Metal / DX12 / GL) | `RenderBackend` | 4 разных API, разные семантики, driver-баги. Свой = годы работы и регрессий |
| **`rustls`** | TLS, X.509, X25519, AES-GCM, HKDF | `TlsBackend` | **Универсальное правило безопасности:** не пишите свой crypto. rustls — аудит + формальная верификация частей кода |
| **JS engine** (`rquickjs` v0.5 → `rusty_v8` v1.0+) | Исполнение JavaScript | `JsRuntime` | V8 — 15 лет, миллиарды долларов, сотни инженеров. QuickJS на старте, V8 в v1.0+ |

### Что НЕ берём как зависимости (ранее планировалось — теперь пишем сами)

Эти крейты были в первой редакции §5 как «готовые». Решение пересмотрено: всё своё, по принципу «default — сами».

- ~~`html5ever`~~ → свой HTML-парсер по [HTML5 spec](https://html.spec.whatwg.org/multipage/parsing.html) (см. §6.1).
- ~~`cssparser` + `selectors`~~ → свой CSS-парсер по CSS Syntax L3 (§6.2).
- ~~`stylo`~~ → свой каскад и computed values (§6.4).
- ~~`taffy`~~ → свой layout: block, inline, flex, grid (§6.5).
- ~~`tiny-skia`~~ → свой 2D-растеризатор (CPU для v0.1, GPU через `wgpu` дальше).
- ~~`hyper`~~ → свой HTTP/1.1 и HTTP/2 поверх `rustls` + std.
- ~~`quinn`~~ → свой QUIC / HTTP/3 (Phase 3, после v1.0).
- ~~`hickory-resolver`~~ → свой DNS-резолвер с DoH/DoT поверх `rustls`.
- ~~`image`~~ → свои PNG / JPEG декодеры; AVIF / WebP откладываем до v1.0.
- ~~`ttf-parser` / `font-kit`~~ → свой TrueType-парсер и font matcher.
- ~~`rustybuzz`~~ → свой shaper для Latin / Cyrillic. Сложные скрипты (арабский, индийский, тайский) — в v1.0+, отдельным модулем; пока «не поддерживается».
- ~~`unicode-bidi`, `xi-unicode`~~ → свои реализации UAX #9, UAX #14.
- ~~`encoding_rs`~~ → свои таблицы декодирования (cp1251, KOI8-R, CP866, UTF-8, ASCII, Win-1252).
- ~~`url`~~ → свой URL parser по WHATWG URL spec (текущий стаб в `lumen-core::url`).
- ~~`idna`~~ → свой Punycode (RFC 3492) + IDNA правила.
- ~~`unicode-security`~~ → свои homograph checks для IDN.
- ~~`adblock`~~ (Brave) → свой filter matcher.
- ~~`readability`~~ → своя реализация readability heuristics с настройкой под кириллицу (§10.9).
- ~~`hyphenation`~~ → свои словари переноса (Phase 2).
- ~~`redb` / `sled`~~ → свой минимальный B-tree KV-store с fsync (для Phase 0 — in-memory + JSON-снапшот).
- ~~`postcard` + `serde`~~ → своя компактная binary serialization для IPC.
- ~~`tokio`~~ → свой минимальный async-исполнитель поверх std + epoll/kqueue/IOCP (или single-threaded на старте).
- ~~`rayon`~~ → свой work-stealing thread pool, когда понадобится параллельный layout / style.
- ~~`egui` / `iced` / `Slint`~~ → свой иммедиат-режим UI поверх `wgpu`-примитивов из paint-крейта.

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

## 6. Движок: компоненты детально

### 6.1 HTML parser

**Что это:** превращает поток байт в DOM-дерево по спеке [HTML5 parsing algorithm](https://html.spec.whatwg.org/multipage/parsing.html).

**Состоит из:**
- **Tokenizer** — конечный автомат с ~80 состояниями. Принимает байты, выдаёт токены: `StartTag`, `EndTag`, `Character`, `Comment`, `Doctype`.
- **Tree construction** — берёт токены и строит DOM с учётом «insertion modes» (~20 режимов). Тут вся магия: `<table>` особо обрабатывает `<tr>`, `<form>` нельзя вложить в `<form>` и т.д.
- **Encoding sniffing** — определение кодировки из BOM, meta, заголовков.

**Crate (свой):** `engine/html-parser`. Пишем с нуля по HTML5 spec.

**Сложность:** не алгоритмическая, а в точности следования спеке. Тесты — `html5lib-tests` (10 тыс. testcases).

### 6.2 CSS parser

**Что это:** байты → CSSOM (StyleSheet → Rule → Declaration → Value).

**Состоит из:**
- **Tokenizer** по [CSS Syntax Level 3](https://www.w3.org/TR/css-syntax-3/).
- **Parser** для разных грамматик: selector, declaration, at-rule (`@media`, `@font-face`, `@keyframes`, `@supports`, `@container`).
- **Value parser** для каждого property (color, length, calc(), gradient, transform-function...).

**Свой парсер.** Пишем токенизатор + parser по CSS Syntax L3 spec; селекторы — по CSS Selectors L4. Не берём `cssparser`/`selectors` (см. политику §5).

**Сложность:** объём. CSS properties — 600+. Реализуем по приоритету (display, position, margin, padding, color, font, background — первая сотня покрывает 95% сайтов).

### 6.3 DOM

**Что это:** дерево узлов в памяти + API мутаций + события.

**Ключевые решения:**
- **Хранение:** не наивные `Rc<RefCell<Node>>` (слишком медленно, циклы), а **арена** (`Vec<NodeData>`) с `NodeId(u32)`. Так делает Servo. Дёшево клонировать, кэш-дружелюбно.
- **Сильные/слабые ссылки:** parent-child через индексы, никаких `Rc`-циклов.
- **Mutations:** все через mutator API, чтобы записывать инвалидацию стилей/layout.
- **Events:** capture/bubble фазы, ленивая регистрация listeners.
- **MutationObserver** — поддерживаем (нужен для современных фреймворков).

**Crate:** `engine/dom`.

### 6.4 Style system (cascade)

**Что это:** для каждого DOM-узла вычислить **computed style** — финальные значения всех CSS-property.

**Этапы:**
1. **Selector matching:** для каждого узла найти все matching rules. Оптимизация — bloom filter ancestor cache (как в WebKit/Blink).
2. **Cascade:** отсортировать по специфичности + origin (user-agent / user / author) + `!important`.
3. **Inheritance:** свойства типа `color`, `font-*` наследуются.
4. **Computed values:** `em` → `px`, `red` → rgba, относительные → абсолютные.

**Параллельность:** style resolution параллелится по поддеревьям через `rayon`. Это главное преимущество Servo-подхода.

**Своя реализация.** Bloom-filter ancestor cache, параллельный matching через свой work-stealing pool. Не берём `stylo` (см. §5).

**Crate:** `engine/style`.

### 6.5 Layout

**Что это:** computed style + DOM → дерево боксов с координатами и размерами.

**Алгоритмы по приоритету:**
1. **Block & inline (CSS 2.1)** — базис. Block formatting context, inline formatting context, line boxes.
2. **Floats & clear** — устаревшее, но ещё много где встречается.
3. **Positioning** — static / relative / absolute / fixed / sticky.
4. **Flexbox** — `flex-direction`, `justify-content`, `align-items`, `flex-grow/shrink/basis`.
5. **Grid** — самый сложный. Track sizing algorithm, named lines, auto-placement.
6. **Tables** — отдельный мир алгоритмов (table-fixed vs table-auto layout).
7. **Multi-column, transforms, writing-modes** — позже.

**Архитектура:** layout tree отдельно от DOM (как в Blink/Servo). Один DOM-узел может породить несколько layout-боксов (анонимные боксы, `::before`/`::after`).

**Своя реализация.** Block + inline на старте (Phase 0), затем flex (Phase 2), grid (Phase 3). Не берём `taffy` (см. §5) — алгоритмы Grid и Flex описаны в spec, реализуемы.

**Crate:** `engine/layout`.

### 6.6 Paint

**Что это:** layout tree → display list (список команд рисования: «нарисовать прямоугольник 10,10–100,50 цвета red»).

**Команды display list:**
- `DrawRect(rect, paint)`
- `DrawText(glyphs, position, font, paint)`
- `DrawImage(image, src_rect, dst_rect)`
- `DrawPath(path, paint)` (для borders, gradients)
- `PushClip(rect)` / `PopClip`
- `PushTransform(matrix)` / `PopTransform`
- `PushOpacity(alpha)` / `PopOpacity`

**Почему display list:** разделяет «что рисовать» от «как рисовать». Удобно для:
- кэширования (если layout не поменялся — переиспользуем),
- передачи в compositor,
- тестирования (snapshot-тесты на display list, а не на пиксели).

**Crate:** `engine/paint`.

### 6.7 Compositor

**Что это:** превращает display list в реальные пиксели через GPU.

**Подход:** **WebRender-style** — каждый кадр выгружается в GPU как набор примитивов, GPU параллельно растеризует. Никаких CPU-растеризованных слоёв.

- Слои для `position: fixed`, `transform`, `opacity`, `will-change`.
- Tiling для больших страниц (рисуем только видимое + буфер).
- Анимации через compositor (transform/opacity без relayout).

**Стек:** `wgpu` (под Vulkan/Metal/DX12/GL). Свои шейдеры на WGSL.

**Своя реализация.** На старте — простой CPU-растеризатор (line/rect/path/text) в `lumen-paint`. С v0.5 — GPU-pipeline поверх `wgpu` (единственная внешняя зависимость в этом слое, см. §5). Не берём `tiny-skia` / `skia`.

**Crate:** `engine/compositor`.

### 6.8 JS engine integration

**Решение:**
- **v0.1:** без JS.
- **v0.5:** **QuickJS** через `rquickjs` crate. Маленький (~200 КБ), ES2020-совместимый, простой биндинг. Медленнее V8 в 10–50 раз, но для не-SPA сайтов хватает.
- **v1.0:** **V8** через `rusty_v8` (Deno-style) или **SpiderMonkey** через `mozjs`. V8 быстрее, SpiderMonkey ближе по духу. **Рекомендация: V8** — больше документации, тесты Deno как референс.

**Биндинги (важно):**
- Каждый Web API экспортируется в JS как объект/функция.
- Биндинги генерируем из WebIDL (`weedle` crate для парсинга IDL).
- Сборщик мусора JS-движка должен «видеть» Rust-объекты, к которым держит ссылки. У V8 — wrapper objects + tracing handles. Это **самая хрупкая граница** проекта.
- `unsafe` неизбежен на этой границе. Изолируем в `engine/js-binding`, ревью + fuzzing обязательны.

**Crate:** `engine/js-binding` + `webapi/*`.

### 6.9 Text rendering

**Этапы:**
1. **Font matching** — найти шрифт для каждого глифа (CSS font fallback chain).
2. **Shaping** — текст + шрифт → последовательность глифов с позициями. `rustybuzz`.
3. **Line breaking** — `xi-unicode` (Unicode UAX #14).
4. **Bidi** — `unicode-bidi` (UAX #9). Арабский, иврит.
5. **Rasterization** — `ab_glyph` или `fontdue` для CPU, или прямо на GPU через signed distance fields.

**Crate:** `engine/text`.

### 6.10 Image decoding

`image` crate покрывает PNG, JPEG, GIF, WebP, BMP, ICO. AVIF — через `libavif` (C dep). SVG — через `resvg` (Rust). Все декодируем **в renderer-процессе**, не в network. Это важно для безопасности: декодеры — частый источник CVE.

**Crate:** `engine/image`.

---

## 7. Web APIs

Реализуем по приоритету.

### Tier 1 (нужны для большинства сайтов)
- `document.*`, `Element.*`, `Node.*` — DOM API
- `querySelector`, `querySelectorAll`
- `addEventListener`, `removeEventListener`
- `fetch()`, `XMLHttpRequest`
- `localStorage`, `sessionStorage`
- `setTimeout`, `setInterval`, `requestAnimationFrame`
- `console.*`
- `window.location`, `window.history`
- `URL`, `URLSearchParams`
- `FormData`, `Blob`, `File`
- `Promise` (даёт JS-движок)

### Tier 2
- `Canvas 2D`
- `IndexedDB`
- `WebSocket`
- `MutationObserver`, `IntersectionObserver`, `ResizeObserver`
- `requestIdleCallback`
- Clipboard API (read/write с разрешения)

### Tier 3 (опционально)
- Service Workers
- Web Workers
- Shadow DOM
- Custom Elements
- WebAssembly (через JS-движок «бесплатно»)

### Не реализуем
- WebUSB, WebBluetooth, WebMIDI, WebSerial, WebNFC, Payment Request, FedCM, WebHID, EME (DRM), Background Sync, Push, Notifications API (на старте).

**Crate:** `webapi/*`.

---

## 8. UI оболочка

### 8.1 Технологический выбор
- **`winit`** — окна, события.
- **`wgpu`** — рендеринг UI и engine compositor через один GPU-контекст.
- **`egui`** для v0.1–v1.0 — иммедиат-режим, очень быстро разрабатывается, кросс-платформенный.
- Возможный переход на `iced` или `Slint` к 2.0 для более polished UX.

### 8.2 Структура UI
```
┌──────────────────────────────────────────────────┐
│ [≡] [◀][▶][↻] [omnibox.................][⋯][↓]  │  toolbar
├────┬─────────────────────────────────────────────┤
│ ▾ Work             ┌─────────────────────────┐   │
│  ├ GitHub          │                         │   │
│  ├ Linear          │      Active tab         │   │
│  └ Docs            │      content area       │   │
│ ▾ Personal         │                         │   │
│  ├ HN              │                         │   │
│  └ Mail            └─────────────────────────┘   │
│ + New tab                                        │
├────┴─────────────────────────────────────────────┤
│ Network log: 12 req, 340 KB, 4 blocked          │  status bar
└──────────────────────────────────────────────────┘
```

### 8.3 Возможности UI

**Базовые:**
- Адресная строка (omnibox) с локальным поиском по истории/закладкам. Поисковые подсказки — **выключены по умолчанию**.
- Вкладки: вертикальные с деревьями (parent → children).
- Закладки: дерево, теги.
- История: полнотекстовый поиск по локальной БД.
- Find in page (Ctrl+F).
- Zoom (Ctrl+/Ctrl-).

**Продвинутые:**
- **Workspaces** — наборы вкладок, переключение Ctrl+1..9. Каждый — со своим контекстом cookies (опционально).
- **Tab tree** — вкладки иерархично, складываются по группам.
- **Tab hibernation** — фоновые вкладки выгружаются через N минут.
- **Split view** — две вкладки рядом.
- **Picture-in-picture** для видео.
- **Reader mode** (Ctrl+R) на основе `readability`.
- **Команд-палитра** Ctrl+Shift+P — все действия клавиатурой (как VS Code).
- **Network log панель** — что уходит, куда, сколько байт, что заблокировано.

**Темы:**
- Light, dark, system, AMOLED-black.
- Без анимаций по умолчанию (можно включить).
- Без округлых иконок 12-цветной палитры — функциональный минимум.

### 8.4 Чего НЕ делаем в UI
- Лент новостей, рекомендаций, шопинга, погоды.
- ИИ-сайдбара по умолчанию.
- Welcome-screens, туториалов, бейджей.
- «Вы давно не заходили» нотификаций.
- Forced sign-in.

---

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

### 9.5 Anti-fingerprinting

- **Canvas randomization** — Canvas.getImageData возвращает данные с микро-шумом (как в Brave). Per-session seed.
- **WebGL renderer / vendor strings** — обобщённые («Generic GPU», «WebKit»).
- **AudioContext fingerprint** — мизерный шум.
- **Fonts enumeration** — белый список из системных шрифтов, без эксклюзивов.
- **Timezone** — опция «использовать UTC».
- **Screen resolution** — опция округления до 100px.
- **Hardware concurrency** — фиксируем на 2 или 4.

Три пресета:
- **Standard** — total cookie protection, adblock, strip URL params. Сайты работают.
- **Strict** — + fingerprinting protection, JS-блокировка на сомнительных доменах.
- **Tor-mode** — + через Tor, фиксированный fingerprint, никаких persistent данных.

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

---

## 10. Локализация и поддержка русского языка

Поддержка русского — first-class требование, не «потом». Контракт на каждом этапе разработки.

### 10.1 Кодировки

Старые RU-сайты часто отдают **Windows-1251** или **KOI8-R**, реже CP866. HTML parser определяет кодировку из `Content-Type`, `<meta charset>`, BOM или (в крайнем случае) байт-паттернов и конвертирует в UTF-8 на входе DOM. **Реализация — своя:** таблицы декодирования — это публичные данные, hand-rolled SIMD не нужен на старте. Trait — `EncodingDetector` в `lumen-core::ext`.

### 10.2 Шрифты

Font fallback chain обязательно содержит шрифты с кириллицей:

- **Windows:** Segoe UI, Tahoma, Arial.
- **macOS:** SF Pro, Helvetica Neue.
- **Linux:** Noto Sans, DejaVu Sans, Liberation Sans.

Fallback работает на каждый символ-сирота, не на всю строку (стандартное поведение `cosmic-text` + `rustybuzz`). Регрессионный тест: «Привет, мир» с Latin-only шрифтом должен показать кириллицу из fallback.

### 10.3 URL и IDN

Кириллические домены (`президент.рф`, `почта.рф`) — RFC 5890. В сетевом запросе → Punycode (`xn--...`), в UI → всегда Unicode. Защита от homograph-атак по правилам IDNA. **Crates:** `idna`, `unicode-security`.

### 10.4 Локаль `ru-RU`

- Дата: `12.05.2026` (dd.mm.yyyy).
- Время: 24-часовое, `14:30`.
- Числа: `1 234,56` (NBSP-разделитель тысяч, запятая для десятичных).
- Неделя начинается с понедельника.

**Crate:** `icu` (модульный, подключаем нужные компоненты).

### 10.5 Anti-fingerprinting vs язык

Tor Browser форсирует `Accept-Language: en-US,en` ради единого fingerprint — это ломает русскоязычный UX (получаешь английские версии сайтов). Lumen в strict-mode **НЕ** нормализует язык до английского, оставляет `ru,en;q=0.5`. Остальные fingerprint-метрики (timezone, screen, canvas, fonts) нормализуем как обычно. Это сознательный компромисс: UX > fingerprint resistance для одной метрики.

### 10.6 Поисковые движки

Встроенные опции, пользователь выбирает при первом запуске:

- DuckDuckGo,
- Brave Search,
- **Яндекс** — для русскоязычных,
- Mojeek,
- свой URL.

Без «облачных» подсказок по умолчанию — поиск только при Enter.

### 10.7 Сортировка и поиск по тексту

История, закладки, omnibox-поиск с кириллицей:

- collation по русскому алфавиту, не по Unicode codepoints,
- Ё↔Е equivalence (опционально),
- транслитерационный поиск: ввод `privet` находит «привет».

**Crate:** `icu_collator`.

### 10.8 UI-переводы

Русский — первый язык наравне с английским, не «после релиза». Формат **Fluent** (FTL, Mozilla) — корректная плюрализация (1 файл / 2 файла / 5 файлов), грамматические падежи. Дизайн UI учитывает: русский текст в среднем на ~30% длиннее английского, тулбары/диалоги не должны обрезаться.

### 10.9 Reader mode

Readability heuristics родом из английского. Регулярно тестируем на: Habr, ТАСС, Lenta, Meduza, КП. Возможна настройка порогов «main content vs sidebar» под кириллические тексты.

### 10.10 Перенос слов

CSS `hyphens: auto` с русскими правилами переноса. Откладываем до Phase 2 — не блокирует чтение, улучшает вёрстку. **Crate:** `hyphenation` (TeX-словари для русского доступны).

### 10.11 Тесты на RU-вебе

Отдельный CI-прогон по топу русскоязычных сайтов: Yandex, VK, OK, Mail.ru, Habr, Lenta, RT, ТАСС, Госуслуги. Скриншот-сравнение с Chromium как baseline. Отдельный от глобального топ-1000.

---

## 11. Модульность и расширяемость

Lumen строится из независимых модулей с явными интерфейсами. Это две связанные, но разные задачи: модульность собственного кода и поддержка сторонних плагинов.

### 11.1 Внутренняя модульность

Принципы:

- **Однонаправленные зависимости.** `lumen-core` — основание, на него опираются все остальные крейты. Никаких циклов. Каждый крейт зависит только от «ниже» по уровню.
- **Стабильные публичные API.** Каждый крейт экспортирует узкий публичный интерфейс (как правило, `trait` + базовые типы). Внутренности — `pub(crate)`.
- **Cargo features.** Опциональные подсистемы за feature gates: `v8`, `quickjs`, `webgl`, `ru-hyphenation`, `tor`. По умолчанию минимальный набор.
- **Базовый крейт `lumen-core`.** Общие типы: `Url`, `MimeType`, `Error`, `EventBus`, `Capability`. Всё, что нужно более чем одному модулю, живёт здесь.

Точки расширения для собственного кода (через `trait` в `lumen-core` или соседних crates):

| Trait | Назначение | Возможные реализации |
|---|---|---|
| `JsRuntime` | мост к JS-движку | QuickJS, V8, SpiderMonkey, mock |
| `StorageBackend` | БД для cookies / IndexedDB | redb, sqlite, in-memory |
| `NetworkTransport` | HTTP-стек | свой HTTP/1.1, /2 (Phase 1+); mock для тестов |
| `RenderBackend` | растеризация | свой CPU-rasterizer (Phase 0), свой GPU-pipeline поверх wgpu (Phase 1+); headless для тестов |
| `EncodingDetector` | определение кодировки HTML | свой по байт-таблицам (cp1251, KOI8-R, CP866, UTF-8, ASCII, Win-1252) |
| `WindowingBackend` | OS event loop + окна | winit (exception, см. §5); потенциально свой нативный — Phase 3+ |
| `TlsBackend` | TLS + crypto | rustls (exception, см. §5); потенциально системный (SChannel / Network.framework) |
| `SearchProvider` | поисковая система | DuckDuckGo, Brave, Яндекс, кастомный |
| `FilterListSource` | источник списков рекламы | EasyList, локальный файл, OTA-канал |
| `FontProvider` | поиск шрифтов | системный, bundled, веб |

Каждый trait — точка для будущей замены без правки потребителей.

### 11.2 Сторонние плагины

Три реальных архитектурных пути:

| Подход | Плюсы | Минусы | Примеры в индустрии |
|---|---|---|---|
| **WASM** через wasmtime | Песочница из коробки, кросс-язычность, capability-based security, стабильный ABI (WASI 0.2) | Медленнее native (но достаточно для не-hot path), runtime ~5 МБ | Zed, Figma, Envoy, Shopify Functions |
| **Native dylib** | Максимальная скорость, прямой доступ к API | Полное доверие, нестабильный Rust ABI, частый источник crashes/CVE | Bevy plugins |
| **WebExtensions (JS)** | Огромная экосистема (uBO и др.) | Привязка к JS-движку, сотни `browser.*` API, всё через JS | Firefox, Chrome, Safari |

**Рекомендация: WASM через `wasmtime`.** Почему для privacy-first браузера это правильный выбор:

- Плагин **по умолчанию не имеет доступа** к ФС, сети, других вкладок, cookies, истории. Хост (Lumen) выдаёт capability tokens на конкретные операции.
- Плагин можно писать на **любом языке**, компилируемом в WASM (Rust, Go, AssemblyScript, TS-через-AS).
- WASI 0.2 и Component Model — общепринятый стандарт, не маргинальная экзотика.
- Производительность для плагинов приемлема — они не на критическом пути рендера.

Альтернативы оставляем не закрытыми: подмножество WebExtensions API можно реализовать поверх WASM-инфраструктуры в виде плагина-шима, который транслирует `browser.*` JS-вызовы в capability-вызовы.

### 11.3 Plugin API — черновик

Что плагин **может**:

- Подписываться на события: `tab_created`, `tab_closed`, `page_loaded`, `request_intercepted`, `key_pressed`, `selection_changed`.
- Регистрировать команды в команд-палитре (Ctrl+Shift+P).
- Регистрировать пункты в контекстном меню (правый клик).
- Рисовать UI в выделенном rect сайдбара (своя «вкладка» в боковой панели).
- Получать выделенный текст, манипулировать им.
- Делать сетевые запросы — только если выдан capability `network` с whitelist доменов.
- Читать/писать в свой namespace `KV`-хранилища.

Что плагин **НЕ может**:

- Менять движок рендера, парсер, layout.
- Читать cookies / storage других сайтов без явного `storage:<origin>` capability.
- Запускать произвольный код на хосте, лезть в чужие плагины.
- Постоянно «висеть» в фоне без причины — runtime ограничивает CPU/память.

### 11.4 Capability-модель (вместо «разрешений» Chrome)

В Chrome/Firefox у плагина есть статический список permissions в манифесте; пользователь видит «доступ ко всем сайтам». Это устарело. В Lumen:

- Плагин при установке заявляет **категории** capabilities (network, storage, clipboard, UI-sidebar).
- При первом использовании каждой capability — runtime prompt с конкретикой («плагин X хочет послать запрос на api.example.com — разрешить раз / всегда / запретить»).
- Capability можно отозвать в любой момент.
- Список выданных capability-tokens видно в UI настроек плагина.

### 11.5 Этапность

- **Phase 0–1:** внутренняя модульность, `lumen-core`, основные traits как точки замены. Никаких сторонних плагинов.
- **Phase 2:** первая версия Plugin API + wasmtime host. Один-два дев-плагина для проверки (например, sidebar для заметок, кастомный adblock-провайдер).
- **Phase 3+:** расширенный capability-набор, дистрибуция через self-hosted manifests + minisign-подписи. Никакого централизованного «магазина» с reviewers (как Chrome Web Store) — пользователь сам решает, кому доверять.

---

## 12. Knowledge layer и уникальные фичи Lumen

Раздел фиксирует функциональность, которой нет в массовых браузерах не из-за технической сложности, а из-за конфликта интересов их вендоров (Google, Microsoft зарабатывают именно на том, что эти фичи отсутствуют). Lumen, не имея рекламной модели и облачных сервисов, закрывает эти пробелы first-class.

Архитектурно эти фичи живут в новом крейте `lumen-knowledge` (хранение + индексация), опциональном `lumen-ai` (локальные эмбеддинги + RAG) и UI-расширениях `lumen-shell` (omnibox-фильтры, боковые панели).

### 12.1 Полнотекстовый поиск по истории

**Что:** omnibox ищет не только по URL и заголовкам, но по полному содержимому всех ранее посещённых страниц.

**Почему:** классическая боль *«найди ту статью про переработку лития, что я читал в марте»*. Chrome намеренно не делает — это конфликт с поиском Google.

**Реализация:**
- При навигации фоновый readability-extract извлекает основной текст без UI-шума (то же ядро, что в §10.9 reader-mode).
- Текст идёт в локальный полнотекстовый индекс — наша реализация: token stream + inverted index с term frequency. Поверх `lumen-storage` KV-store.
- Хранится: URL, title, full text, дата визита, favicon hash, sha256 от текста.
- Объём: средняя текстовая статья ~10 КБ; 100 000 страниц ≈ 1 ГБ; с LZ4-компрессией ~250 МБ. Лимит по диску настраивается (по умолчанию 500 МБ → авто-вытеснение старого).
- Запрос: omnibox с префиксом `@history` или просто текст — результаты из истории / заметок / закладок выше внешнего поиска.
- Ранжирование: tf-idf + рекордовая частота + recency boost.

**Локализация:** токенизатор нормализует кириллицу (lowercase, ё↔е equivalence), опциональный Porter-stemmer для русского (см. §10).

**Фаза:** 2 (после HTTP-клиента, когда есть смысл накапливать историю).

### 12.2 Аннотации и заметки

**Что:** выделил текст на странице → команда «сохранить как заметку с контекстом». Заметка хранит выделенный фрагмент, окружающий абзац, URL, дату, опциональный комментарий пользователя.

**Почему:** замена внешних сервисов (Readwise, Hypothesis, Notion Web Clipper, Obsidian). Это базовая для читателя функциональность, которой нет встроенной нигде.

**Реализация:**
- Selection / Range API в DOM (стандартный, нужен и для других целей — поиск по странице, copy-to-clipboard).
- Context-menu действие в shell + горячая клавиша.
- Хранение — в той же `lumen-knowledge` БД, индексируется тем же FTS из §12.1.
- При повторном открытии страницы заметки восстанавливаются поверх DOM как highlight-наложения (опционально включается).
- Экспорт всех заметок в Markdown / JSON — кнопка в Notes panel.
- Per-profile (заметки личного профиля не видны в рабочем).

**Фаза:** 2.

### 12.3 Read-later / офлайн-чтение

**Что:** кнопка «сохранить страницу офлайн». Берёт полный snapshot — DOM + CSS + изображения — и кладёт в profile-каталог. Дальше страница доступна без сети сколь угодно долго.

**Почему:** замена Pocket / Instapaper. Базовая функциональность читателя — должна быть встроенной, а не подписочной.

**Реализация:**
- При сохранении: walk текущий DOM, скачиваем все ресурсы (`<img>`, `<link rel=stylesheet>`, inline-background-image из стилей), сохраняем как single-file HTML (data-URI inline) или связанный набор файлов.
- Per-profile квота по диску (по умолчанию 1 ГБ), настраивается; FIFO-вытеснение по дате доступа.
- Список «Read Later» в боковой панели shell; клик открывает локально без сети с пометкой «офлайн-копия от \<дата\>».
- Текст офлайн-копий тоже идёт в индекс §12.1.
- Опционально: одноразовое чтение из RSS / Atom-фидов (тоже офлайн).

**Фаза:** 2.

### 12.4 Поиск по содержимому открытых вкладок

**Что:** omnibox с префиксом `@tabs` ищет среди *сейчас открытых* вкладок (не истории) по содержимому — title, видимый текст, форма URL. Удобно, когда открыто 50 вкладок и не вспомнить, какая нужна.

**Почему:** один из самых частых запросов на форумах. Edge / Arc частично закрыли. Chrome / Firefox — нет.

**Реализация:**
- Live-индекс открытых вкладок (subset §12.1 механики, но без диск-persistence).
- Учитывает hibernated вкладки тоже (по сохранённому DOM-snapshot).
- Фильтр по workspace / profile.

**Фаза:** 2.

### 12.5 Локальный AI layer (опциональный)

**Что:** маленькая локальная модель + локальный embedding для:
- **Семантического поиска** по истории / заметкам / закладкам. *«Что я читал про электрокары»* находит даже если в статье нет слова «электрокар», но смысл совпал.
- **Суммаризации** страницы по запросу (никаких облачных API).
- **Q&A над собственной историей** (RAG): «какие источники говорили про X».

**Почему:** индустрия идёт в облачные ИИ-агенты (Atlas от OpenAI, Comet от Perplexity, Dia, Edge Copilot). У них три фундаментальные проблемы: приватность утекает наружу, дорого по токенам, prompt-injection как класс уязвимостей. Локальная модель решает все три.

**Реализация:**
- Отдельный крейт `lumen-ai`, под Cargo feature-флагом `ai`. По умолчанию **выключен** в bundle (бинарь Lumen без AI меньше и проще).
- Backend через HTTP API уже установленной Ollama (если есть) — нулевая интеграция, дёшево. Альтернатива: встроенный llama.cpp через FFI — это потенциально **5-е exception** в §5 с обоснованием. Решение откладываем до момента включения модуля.
- Эмбеддинги (`bge-small`, `nomic-embed-text` или подобное) предвычисляются при индексации страницы (§12.1) если модуль включён.
- Векторный store: HNSW-индекс в `lumen-knowledge` — приближённый ближайший сосед за O(log N).
- UI: команда `@ai` в omnibox или отдельная панель «Ask Lumen».
- Capability `local-ai` для плагинов: WASM-плагин может запросить эмбеддинг или генерацию через Lumen-runtime, никаких сетевых вызовов.

**Фаза:** 3+. Не критичная, но потенциально killer-feature. Phase 0-2 работает без AI.

### 12.6 Focus mode

**Что:** режим, в котором браузер активно снижает когнитивную нагрузку:
- Скрыты боковые панели, badges, нотификации.
- Фоновые вкладки автоматически hibernated (агрессивнее обычной гибернации).
- Reader mode принудительный для текстовых страниц.
- Один таб виден за раз, минимальный chrome.
- Опционально: Pomodoro-таймер; по окончании цикла — нотификация.

**Почему:** ни один массовый браузер не помогает пользователю фокусироваться — все оптимизированы на engagement (engagement = time-on-platform = реклама). Lumen без рекламной модели может прямо помогать пользователю выйти из браузера.

**Реализация:** UI feature поверх существующей инфраструктуры. Не требует новых крейтов. Toggle через команду или горячую клавишу.

**Фаза:** 2.

### 12.7 Tab session export / import

**Что:** сериализация набора открытых вкладок (включая дерево, workspaces, scroll-позиции, базовые значения форм) в файл; импорт восстанавливает сессию.

**Почему:** переезд между компьютерами, бэкап перед переустановкой, шаринг рабочей сессии с коллегой. Все хотят, никто не делает в полном виде.

**Реализация:**
- Формат: компактный JSON или TOML (бинарный не нужен).
- Поля: URL, title, scroll position, form values (textarea / input), parent в дереве, workspace.
- Импорт: lazy — вкладки восстанавливаются как hibernated, активируются по клику.
- Cross-profile (можно экспортировать рабочий и импортнуть в личный с подтверждением).

**Фаза:** 1-2 (легко, можно сделать рано).

### 12.8 Семантические закладки

**Что:** вместо «сохрани ссылку» — *«сохрани смысл, напомни связанное»*. Закладка содержит автоматическую суммаризацию + теги + эмбеддинг. При релевантных omnibox-запросах закладка всплывает сама.

**Почему:** обычные закладки = «складировал и забыл». Семантические превращают коллекцию в активный граф знаний.

**Реализация:** расширение §12.2 + §12.5:
- Суммаризация через локальный AI (если модуль включён) или вручную пользователем.
- Эмбеддинг суммаризации хранится рядом с закладкой.
- Поиск похожих — cosine similarity на эмбеддингах при текущем omnibox-запросе.
- Без AI-модуля — обычные tag-based закладки (теги вручную).

**Фаза:** 3 (зависит от §12.5).

### 12.9 Граф знаний пользователя

**Что:** интерактивная визуализация связей между прочитанными страницами / заметками / закладками. Темы, домены, пересечения, кластеры.

**Почему:** *«что я знаю про X»*, *«какие источники мне доверять по теме Y»*, *«какие темы давно не трогал»* — таких инструментов нет в браузерах в принципе. Для пользователей, активно работающих со знанием (исследователи, журналисты, аналитики).

**Реализация:**
- Граф строится на данных §12.1-12.5: узлы — страницы / заметки / закладки; рёбра — by-domain / by-tag / by-semantic-proximity (через AI-эмбеддинги) / by-link-citation.
- Render — SVG или Canvas с force-directed layout.
- Интерактив: фильтры по дате / тегам / профилю, поиск, drill-in.
- Опционально: экспорт в формат для Obsidian / Roam Research.

**Фаза:** 3+. Опционально.

### 12.10 Кастомизация UI

**Что:** пользователь переаранжирует toolbar, скрывает / показывает панели, выбирает темы, настраивает omnibox-поведение. По духу — Firefox 2008, до того как массовые браузеры стали неконфигурируемыми.

**Почему:** один из самых частых запросов на форумах. Vivaldi нишево, Chrome / Edge / Safari почти не дают. Кастомизация — не «advanced опция», а право пользователя на свой инструмент.

**Реализация:**
- Все UI-блоки (toolbar, sidebar, status bar, omnibox) — переставляемые drag&drop.
- Темы: JSON с цветовой схемой + опциональные CSS-оверрайды (для chrome-UI, не для страниц).
- Конфиг в `~/.config/lumen/ui.toml`, edit-able руками или через Settings.
- Плагины (§11) могут добавлять свои UI-блоки в любую панель.

**Фаза:** 2-3.

### 12.11 Кросс-устройственная синхронизация (E2E)

**Что:** опциональная синхронизация состояния (вкладки, история, закладки, заметки, скролл-позиция, форма) между устройствами с end-to-end шифрованием. Self-hosted сервер или peer-to-peer.

**Почему:** *«начал читать в метро на телефоне → пришёл домой, открыл ноут, продолжается с того же места»*. Safari ближе всех в этом, но только в Apple-экосистеме.

**Реализация:**
- Self-host сервер: маленький HTTP-сервис-релей, который не видит содержимого — только зашифрованные blob-ы.
- Шифрование: X25519 + AES-GCM, ключи производятся из паролей профилей через Argon2id KDF.
- Альтернатива (без сервера) — peer-to-peer через LAN / Tailscale.
- НЕ строим централизованный облачный сервис «Lumen Sync» — это против философии (см. §1).

**Фаза:** 3+. Mobile-клиент необходим для real use-case, что упирается в mobile из §16 фаз.

### 12.12 Где это всё трогает архитектуру

Новые крейты:
- **`lumen-knowledge`** — FTS-индекс, аннотации, read-later каталог, хранение в KV-store (`lumen-storage`).
- **`lumen-ai`** (опционально, feature-flag) — embedding pipeline, HNSW-индекс векторов, мост к локальному LLM-backend.

Новые trait-точки расширения в `lumen-core::ext`:
- **`KnowledgeStore`** — абстракция с FTS-методами (insert / search / delete).
- **`AiBackend`** (опционально) — `embed(text) → Vec<f32>`, `generate(prompt, context) → Stream`.

UI расширения в `lumen-shell`:
- Omnibox-префиксы: `@history`, `@notes`, `@tabs`, `@bookmarks`, `@ai`, `@read-later`.
- Боковая панель «Knowledge» с разделами: Notes, Read Later, Bookmarks, Knowledge Graph.
- Context-menu действие «Save as note» в paint-слое.
- Focus-mode toggle в shell.

Capability-модель плагинов (§11.4) расширяется:
- **`KnowledgeRead`** — читать историю / заметки / закладки текущего профиля.
- **`KnowledgeWrite`** — добавлять / редактировать заметки / закладки.
- **`LocalAi`** — запрашивать embed / generate через локальный AI.

---

## 13. Безопасность

### 10.1 Sandboxing

- **Linux:** seccomp-bpf фильтр (whitelist syscalls), user namespaces, дополнительно Landlock для FS.
- **macOS:** App Sandbox через `sandbox_init`, entitlements в plist.
- **Windows:** AppContainer + Job Object + Restricted Token + Mitigation Policies (DEP, ASLR, CFG).

Каждый renderer-процесс — в своём сэндбоксе, без доступа к сети (только через IPC к network service) и без доступа к диску (только через IPC к storage service).

### 10.2 Memory safety

- Rust исключает 70% типичных CVE (use-after-free, buffer overflow, data races).
- `unsafe` — только в:
  - FFI к JS-движку (V8/QuickJS) — `engine/js-binding`,
  - FFI к декодерам, если используем C-либы (AVIF),
  - кастомных аренах DOM (когда индексы выходят за рамки borrow checker).
- Все `unsafe`-блоки помечены, документированы, ревью обязательно.
- `cargo-geiger` для мониторинга `unsafe` в зависимостях.

### 10.3 Process isolation

- Site isolation по eTLD+1.
- COOP / COEP / CORP — поддерживаем.
- `SharedArrayBuffer` — только с правильными заголовками (защита от Spectre).
- Process per origin для opaque origins (`data:`, sandboxed iframes).

### 10.4 Updates

- Подписанные релизы (minisign или sigstore).
- Update-проверка раз в день (можно отключить), не загружает ничего без согласия (или авто-загрузка, как опция).
- Roadmap — детерминированные сборки (reproducible builds) к 1.0.

### 10.5 Дополнительно

- CSP, Mixed Content, Subresource Integrity — строгие дефолты.
- HSTS preload list — встроенный, обновляемый.
- Certificate transparency — проверяем SCT.
- Safe Browsing — **НЕ используем Google API**. Опционально подключаем собственный список через DNS (например, Quad9 уже блокирует malware).
- Fuzzing: `cargo-fuzz` на HTML parser, CSS parser, image decoders, URL parser, JS-binding границы. Запуск в CI.

---

## 14. Производительность

### 11.1 Цели

| Метрика | Цель v0.1 | Цель v1.0 |
|---|---|---|
| Cold start до окна | < 300 мс | < 500 мс |
| Cold start до загруженной google.com | n/a | < 1.5 с |
| RAM на пустую вкладку | < 50 МБ | < 80 МБ |
| RAM на 5 типичных вкладок | < 250 МБ | < 600 МБ |
| RAM на 100 hibernated вкладок | < 200 МБ | < 300 МБ |
| Speedometer 3.0 | n/a | в пределах 2× от Chromium |
| Идл CPU (видимое окно) | < 1% | < 1% |

### 11.2 Стратегии

- **Параллельный layout / style** через `rayon` — главный архитектурный плюс перед Blink (Blink в этом плане монолитен).
- **Lazy tabs** — при восстановлении сессии вкладки не загружаются.
- **Tab hibernation** — освобождение renderer-процесса с сохранением навигации.
- **GPU-композитинг** — всё на wgpu.
- **Кэширование** — display list, computed styles переиспользуются при инвалидации.
- **Инвалидация** — точечная, не «пересчитать всё дерево».
- **Image decoding** — на отдельных тредах, прогрессивный.

### 11.3 Профилирование

- `tracy` интегрирован, активируется флагом `--profile`.
- Бенчмарки в CI: layout простой страницы, парсинг HTML 10 МБ, JS Speedometer.
- Tracking регрессий — графики по коммитам.

---

## 15. Тестирование

### 12.1 Уровни

1. **Unit-тесты** для каждого crate (`cargo test`).
2. **Парсер-тесты:**
   - `html5lib-tests` для HTML parser.
   - WPT-style тесты для CSS parser.
3. ✅ **Render snapshot tests** — рендерим страницу, сравниваем display list (не пиксели, так стабильнее). Реализовано: `serialize_display_list` + 6 golden-файлов в `lumen-paint/tests/snapshots/`. `UPDATE_SNAPSHOTS=1` для регенерации.
4. **Pixel snapshot tests** — для финальной картинки, с допуском.
5. **Web Platform Tests** — берём подмножество (DOM, CSS, fetch). Цель: 60% pass к v1.0.
6. **Integration tests** — запуск браузера, тест UI через `egui`-test-harness или внешний driver.
7. **Fuzzing** в CI.
8. **Top 1000 sites test** — на каждом релизе автоматический прогон, скриншоты, сравнение с Chromium как baseline.

### 12.2 CI

GitHub Actions: Linux/macOS/Windows, debug+release, `cargo test` + `cargo clippy -- -D warnings` + `cargo deny` + fuzzing 10 минут на PR.

---

## 16. Фазы разработки (реалистично)

### Фаза 0 — Прототип (3 месяца)
- ✅ Workspace, base crates.
- 🟡 HTML parser — минимум готов (см. выше).
- 🟡 CSS parser — минимум готов (см. выше).
- ✅ DOM (арена + базовые типы).
- ✅ Layout: block-flow + word-wrapping (TextMeasurer + FontMeasurer).
- 🟡 Paint: FillRect через wgpu готов; глифы — позже.
- 🟡 UI: одно окно (готово), вкладки и адресная строка — нет.
- ⬜ HTTP/1.1 + HTTPS.
- **Цель:** открыть простую текстовую статью без стилей. Доказательство концепции.

### Фаза 1 — v0.1 «Reader» (9 месяцев от старта)
- CSS 2.1 + flexbox.
- Картинки.
- Вкладки, история, закладки.
- Network service в отдельном процессе.
- Storage service.
- Базовый adblock, DoH.
- **Tab session export / import** (§12.7) — простая фича, экономит много боли.
- Пакеты под Linux/macOS/Windows.
- **Цель:** ежедневный браузер для чтения статей.

### Фаза 2 — v0.5 «Interactive» (18–24 месяца)
- QuickJS интеграция.
- Tier 1 Web APIs.
- Формы, базовая интерактивность.
- HTTP/2.
- GPU compositor (wgpu).
- CSS Grid.
- Site isolation.
- Профили, шифрование.
- Anti-fingerprinting.
- **Knowledge layer ядро (§12):**
  - `lumen-knowledge` крейт: FTS-индекс над историей (§12.1).
  - Аннотации и заметки (§12.2).
  - Read-later / офлайн-чтение (§12.3).
  - Поиск по содержимому открытых вкладок (§12.4).
  - Focus mode (§12.6).
- **Кастомизация UI** — drag&drop панелей, темы (§12.10).
- **Цель:** публичная альфа, форумы и простые SPA, в Lumen начинают **жить** долго.

### Фаза 3 — v1.0 (36–48 месяцев)
- Переход на V8 (`rusty_v8`).
- Tier 2 Web APIs.
- IndexedDB, Canvas 2D.
- HTTP/3.
- Service Workers.
- WebFonts (WOFF2).
- Расширения (свой минимальный формат).
- WPT pass rate ≥ 60%.
- **Опциональный AI-модуль (§12.5):** `lumen-ai` крейт за feature-флагом. Семантический поиск, суммаризация, RAG над собственной историей. Bundle без AI остаётся basic-вариантом.
- **Семантические закладки (§12.8)** — опционально, требует AI.
- **Цель:** стабильный релиз.

### Фаза 4 — После 1.0
- Подмножество WebGL (по запросам).
- Mobile (Android через NDK; iOS — упрётся в Apple-policy).
- **Sync через E2E (§12.11)** — self-host или P2P. Mobile-клиент критичен для real use-case.
- **Граф знаний (§12.9)** — визуализация коллекции.
- Локализация UI.

---

## 17. Команда и ресурсы

| Фаза | Состав | Длительность |
|---|---|---|
| 0 — прототип | 2 senior Rust | 3 мес |
| 1 — v0.1 | 3–4 (Rust, систем, UX) | 9 мес |
| 2 — v0.5 | 5–7 (+ JS-эксперт, security) | 12–18 мес |
| 3 — v1.0 | 8–12 | 18–24 мес |

Бюджетная оценка: **минимум 4–5 миллионов USD до v1.0** (если коммерчески), или 4–5 лет с маленькой full-time командой энтузиастов.

---

## 18. Риски и митигация

| Риск | Митигация |
|---|---|
| Веб слишком велик, не успеваем за стандартами | Фокус на читаемый веб, явный scope, отказ от экзотики |
| JS-биндинги хрупкие, текут CVE | Изоляция unsafe, fuzzing, ревью каждой биндинг-функции |
| Сайты ломаются (думают, что мы IE) | UA fixed на актуальный Chrome для совместимости |
| Compositor нестабильный на разных GPU | wgpu абстрагирует, тестируем на 3 GPU min (NV/AMD/Intel) |
| Memory safety не спасает от логических уязвимостей | Sandbox, site isolation, audit |
| Apple запрещает свои движки на iOS | iOS откладываем; либо тонкая обёртка над WKWebView под iOS как исключение |
| Выгорание | Жёсткий scope, чёткие версии, регулярные релизы |
| Supply chain (crates.io) | `cargo-vet`, `cargo-deny`, минимизируем зависимости |

---

## 19. Лицензия

- **MPL-2.0** — позволяет связывание со внешним кодом, требует open-source модифицированных файлов. Совместимо с экосистемой Servo/Firefox.
- DCO вместо CLA.
- Публичный roadmap, RFC-процесс.

---

## 20. Первые конкретные шаги

1. `cargo new --bin lumen` + создать workspace с пустыми crates.
2. `engine/html-parser` — свой токенизатор (FSM по HTML5 spec), затем tree construction. Прогнать `html5lib-tests` (тесты — внешние данные, не код).
3. `engine/css-parser` — свой токенизатор + parser + selectors.
4. `engine/dom` — арена, NodeId, базовые API.
5. `engine/layout` — свой block + inline.
6. `engine/paint` — свой CPU-растеризатор; нарисовать первый бокс в окне.
7. `shell` — окно winit + egui, рендер картинки от движка.
8. **Веха «hello world»:** открыть страницу `<html><body><h1>Hello</h1></body></html>` локально, увидеть текст.
9. **Веха «Внешняя страница»:** открыть удалённую текстовую статью по HTTP, прокрутить, перейти по ссылке.
10. После этого — `network` отдельным процессом, IPC.

---

## 21. Чего я НЕ обещаю

- Что v1.0 будет «как Chrome». Не будет. Будет браузер, в котором работает 80% сайтов и который вы понимаете до последней строки.
- Что это коммерчески выгодно. Скорее всего, нет — это исследовательский / идеологический проект.
- Что Servo/Ladybird не обгонят. Возможно, обгонят. Тогда имеет смысл влить силы туда.
