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
                                        │ history (SQLite)│
                                        └─────────────────┘
```

- **UI process** — единственный, кто рисует окно и принимает ввод.
- **Renderer process на каждый origin** — site isolation как в Chromium. Краш одной вкладки не валит браузер. Эксплойт в одной вкладке не лезет в другую.
- **Network service** — единственный, кто ходит в сеть. Все TLS, DNS, фильтры рекламы, кэш — здесь. Центральная точка приватности.
- **Storage service** — единственный, кто пишет на диск (кроме логов и кэша). Cookies, IndexedDB, история, закладки.

Внутри **UI process** слой chrome (вкладки, омнибокс, панели) отделён от движка и модели
контрактом `BrowserController` + трейтом `ChromeView`. Интерфейс — сменяемый «вид» над моделью;
поддерживаются два взаимозаменяемых бэкенда (нативный Rust-UI и web-chrome, отрисовываемый самим
движком), выбираемые через `LUMEN_CHROME`/feature-флаг. Состояние живёт в модели, поэтому смену
интерфейса и добавление новых интерфейсов можно делать без правок движка. Детали и отложенный
выбор «нативный vs web» — [ADR-015](../decisions/ADR-015-swappable-chrome-view.md).

IPC через `postcard` (компактный, бинарный, serde-совместимый) поверх:
- Unix: `tokio::net::UnixStream`
- Windows: Named Pipes
- macOS: Unix Domain Sockets

---

## 4. Структура репозитория

Актуальный список крейтов — `[workspace] members` в `Cargo.toml` (источник истины;
сверяй там при расхождении с деревом ниже):

```
lumen-browser/
├── Cargo.toml                     # workspace, 22 крейта + workspace-hack
├── crates/
│   ├── shell/                     # UI process: окно, вкладки, event loop, DevTools/CDP host
│   ├── core/                      # extension traits (WindowingBackend, RenderBackend, TlsBackend, …)
│   ├── ipc/                       # межпроцессные типы сообщений, транспорт
│   ├── bidi-server/               # WebDriver BiDi сервер (automation)
│   ├── devtools/                  # Chrome DevTools Protocol (Phase 0 minimal)
│   ├── driver/                    # BrowserSession / automation API (in-process + MCP + BiDi)
│   ├── mcp/                       # Model Context Protocol сервер для automation
│   ├── js/                        # JS-движок и байндинги (V8 default; QuickJS-совместимость — см. ADR-018)
│   ├── network/                   # HTTP/1.1,/2,/3, TLS, DNS, cookies, adblock-фильтры
│   ├── storage/                   # persistent storage (SQLite по ADR-003/012, redb для blob-кэшей)
│   ├── knowledge/                 # knowledge layer (§12 unique features)
│   ├── bench/                     # перф-бенчмарки
│   │
│   └── engine/                    # рендеринг-движок
│       ├── html-parser/           # токенизатор + tree construction
│       ├── css-parser/            # токенизатор + grammar + cascade at-rules
│       ├── dom/                   # DOM-дерево (арена), события
│       ├── layout/                # ComputedStyle, каскад, box generation, layout-алгоритмы
│       ├── paint/                 # display list, femtovg/wgpu/CPU-рендер-бэкенды (ADR-010/017)
│       ├── font/                  # парсер шрифтов, шейпинг (GSUB/GPOS), рендер, WOFF2
│       ├── encoding/              # декодирование текстовых кодировок страниц
│       ├── image/                 # декодирование PNG/JPEG/WebP/GIF/AVIF
│       ├── canvas/                # Canvas 2D (Path2D, растеризация)
│       └── a11y/                  # accessibility tree
│
├── workspace-hack/                # cargo-hakari unified feature resolution
├── assets/fonts/                  # bundled Inter font (SIL OFL 1.1)
├── samples/                       # тестовые HTML для pipeline-прогонов
├── graphic_tests/                 # визуальные регресс-тесты (magenta-frame, vs Edge)
├── docs/
│   ├── plan/                      # design doc (этот файл — часть него)
│   ├── decisions/                 # ADR
│   └── tasks/                     # брифы крупных многосрезовых задач
└── scripts/                       # gen_roadmap.py, gen_symbols.py, вспомогательные тулы
```

Историческая заметка: ранняя версия этого раздела описывала другую, более
гранулярную структуру (`style/`, `selectors/`, `compositor/`, `text/`,
`js-binding/`, `webapi/{dom-api,fetch,canvas,storage,timers}/`, `renderer/`,
`profiles/`, `common/`). Эти крейты не заводились как отдельные units —
их обязанности слились в `lumen-layout` (style+selectors+cascade),
`lumen-paint` (compositor), `lumen-font` (text shaping), `lumen-js`
(js-binding+webapi), `lumen-shell` (renderer+profiles). Многопроцессная
модель §3 (UI/Renderer/Network/Storage как отдельные OS-процессы) тоже
осталась целевой архитектурой, не текущей: сейчас это один процесс с
внутренними модулями по тем же границам.

---

