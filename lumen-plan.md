# Lumen — браузер на Rust с собственным движком

> **Lumen** (лат. *свет*, единица светового потока) — приватный, лёгкий, прозрачный браузер. Имя отражает философию проекта: показывать пользователю всё, что происходит, и не быть тяжелее, чем нужно.

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

---

## 2. Реалистичный scope движка

Полный веб-стандарт нереалистичен. Мы целимся в **подмножество**, постепенно расширяя.

### v0.1 — «текстовый веб» (читалка)
- HTML5 (без `<form>` пока)
- CSS 2.1 + box model + блочный/инлайн layout
- Картинки (PNG, JPEG)
- HTTP/1.1, HTTPS
- Без JS

Цель: открывать Wikipedia, MDN, GitHub README, статьи блогов.

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
│   │   └── filters/               # adblock-rust
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

### Язык и тулинг
- **Rust** edition 2024, MSRV — последний stable.
- `cargo` workspace, `cargo-deny` для аудита зависимостей, `cargo-vet` для supply-chain.
- Сборка релизов через `cargo-dist`, кросс-компиляция через `cross`.

### Что пишем сами
- HTML parser, CSS parser, DOM, style, layout, paint, compositor.
- Web API биндинги.
- Network service, storage service, UI shell.

### Что берём готовое (FFI или crates)
| Задача | Решение | Почему не сами |
|---|---|---|
| JS-движок | **QuickJS** (v0.1–0.5), **V8** через `rusty_v8` (v1.0+) | JS-движок — это ещё 10 лет работы |
| Шрифты: загрузка/парсинг | `ttf-parser`, `font-kit` | TrueType — древняя сложность |
| Шрифты: shaping | `rustybuzz` (порт HarfBuzz на Rust) | Шейпинг арабской/индийской вязи — отдельная наука |
| Bidi / line breaking | `unicode-bidi`, `xi-unicode` | Unicode-алгоритмы стандартизованы |
| Изображения | `image` crate (PNG, JPEG, WebP, GIF) | Кодеки = много CVE при самописе |
| TLS | `rustls` | Криптография — не место для самописа |
| HTTP/1.1, /2 | `hyper` | Зрелая, переиспользуемая |
| HTTP/3 / QUIC | `quinn` | То же |
| DNS | `hickory-resolver` | DoH/DoT встроены |
| GPU | `wgpu` | Кросс-API абстракция |
| Окно | `winit` | Стандарт de facto |
| База | `redb` | Чистый Rust, ACID |
| Сериализация IPC | `postcard` + `serde` | Компактно, быстро |
| Async | `tokio` | Стандарт |
| Параллелизм CPU | `rayon` | Style/layout легко параллелятся |
| UI-фреймворк | `egui` для MVP → возможно `iced`/`Slint` позже | Иммедиат-режим простой, кросс-платформенный |
| Адблок | `adblock` (Brave) | Зрелый, совместим с uBO-листами |
| Reader mode | `readability` | Алгоритм Mozilla |
| URL парсинг | `url` crate | По WHATWG spec |

### Не взятые «соблазны»
- **Skia** — заманчиво, но это привязка к C++ и 1+ млн строк зависимостей. Будем рисовать через wgpu сами.
- **ICU** — берём только нужные подмножества как Rust-крейты.
- **Servo crates напрямую** (`html5ever`, `stylo`, `selectors`) — спорно. Можно взять их как старт и постепенно заменить. Это разумный компромисс: вы изучите устройство, не повторяя их парсер байт-в-байт. **Решение: используем как стартовую точку, помечаем как «replaceable», заменяем по мере необходимости.**

---

## 6. Движок: компоненты детально

### 6.1 HTML parser

**Что это:** превращает поток байт в DOM-дерево по спеке [HTML5 parsing algorithm](https://html.spec.whatwg.org/multipage/parsing.html).

**Состоит из:**
- **Tokenizer** — конечный автомат с ~80 состояниями. Принимает байты, выдаёт токены: `StartTag`, `EndTag`, `Character`, `Comment`, `Doctype`.
- **Tree construction** — берёт токены и строит DOM с учётом «insertion modes» (~20 режимов). Тут вся магия: `<table>` особо обрабатывает `<tr>`, `<form>` нельзя вложить в `<form>` и т.д.
- **Encoding sniffing** — определение кодировки из BOM, meta, заголовков.

**Crate (свой):** `engine/html-parser`. Старт — `html5ever` (Servo), затем постепенный переписать.

**Сложность:** не алгоритмическая, а в точности следования спеке. Тесты — `html5lib-tests` (10 тыс. testcases).

### 6.2 CSS parser

**Что это:** байты → CSSOM (StyleSheet → Rule → Declaration → Value).

**Состоит из:**
- **Tokenizer** по [CSS Syntax Level 3](https://www.w3.org/TR/css-syntax-3/).
- **Parser** для разных грамматик: selector, declaration, at-rule (`@media`, `@font-face`, `@keyframes`, `@supports`, `@container`).
- **Value parser** для каждого property (color, length, calc(), gradient, transform-function...).

**Старт:** `cssparser` + `selectors` (Rust, написаны для Servo, production-quality).

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

**Старт:** `stylo` (style engine из Firefox/Servo). Постепенно адаптируем под наши нужды.

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

**Подсказка:** взять `taffy` (Rust crate, реализует flex + grid + block) как стартовое ядро. Не идеален, но даёт работающий MVP.

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

**Альтернатива на старте:** **`tiny-skia`** (Rust, CPU-растеризация). Медленнее, но проще. Используем для v0.1, GPU — с v0.5.

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

- **Встроенный adblock** на основе `adblock-rust` (Brave). Тот же движок, что и в Brave — production-quality.
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

Старые RU-сайты часто отдают **Windows-1251** или **KOI8-R**, реже CP866. HTML parser определяет кодировку из `Content-Type`, `<meta charset>`, BOM или (в крайнем случае) байт-паттернов и конвертирует в UTF-8 на входе DOM. **Crate:** `encoding_rs` — тот же, что использует Firefox.

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

Readability heuristics родом из английского. Регулярно тестируем на: Wikipedia (ru), Habr, ТАСС, Lenta, Meduza, КП. Возможна настройка порогов «main content vs sidebar» под кириллические тексты.

### 10.10 Перенос слов

CSS `hyphens: auto` с русскими правилами переноса. Откладываем до Phase 2 — не блокирует чтение, улучшает вёрстку. **Crate:** `hyphenation` (TeX-словари для русского доступны).

### 10.11 Тесты на RU-вебе

Отдельный CI-прогон по топу русскоязычных сайтов: Wikipedia ru, Yandex, VK, OK, Mail.ru, Habr, Lenta, RT, ТАСС, Госуслуги. Скриншот-сравнение с Chromium как baseline. Отдельный от глобального топ-1000.

---

## 11. Безопасность

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

## 12. Производительность

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

## 13. Тестирование

### 12.1 Уровни

1. **Unit-тесты** для каждого crate (`cargo test`).
2. **Парсер-тесты:**
   - `html5lib-tests` для HTML parser.
   - WPT-style тесты для CSS parser.
3. **Render snapshot tests** — рендерим страницу, сравниваем display list (не пиксели, так стабильнее).
4. **Pixel snapshot tests** — для финальной картинки, с допуском.
5. **Web Platform Tests** — берём подмножество (DOM, CSS, fetch). Цель: 60% pass к v1.0.
6. **Integration tests** — запуск браузера, тест UI через `egui`-test-harness или внешний driver.
7. **Fuzzing** в CI.
8. **Top 1000 sites test** — на каждом релизе автоматический прогон, скриншоты, сравнение с Chromium как baseline.

### 12.2 CI

GitHub Actions: Linux/macOS/Windows, debug+release, `cargo test` + `cargo clippy -- -D warnings` + `cargo deny` + fuzzing 10 минут на PR.

---

## 14. Фазы разработки (реалистично)

### Фаза 0 — Прототип (3 месяца)
- Workspace, base crates.
- HTML parser (форк html5ever или с нуля).
- CSS parser (форк cssparser).
- DOM (арена + базовые типы).
- Layout: только block + inline.
- Paint: `tiny-skia` CPU.
- UI: одно окно, одна вкладка, адресная строка.
- HTTP/1.1 (через `hyper`), HTTPS.
- **Цель:** открыть Wikipedia без стилей. Доказательство концепции.

### Фаза 1 — v0.1 «Reader» (9 месяцев от старта)
- CSS 2.1 + flexbox.
- Картинки.
- Вкладки, история, закладки.
- Network service в отдельном процессе.
- Storage service.
- Базовый adblock, DoH.
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
- **Цель:** публичная альфа, форумы и простые SPA.

### Фаза 3 — v1.0 (36–48 месяцев)
- Переход на V8 (`rusty_v8`).
- Tier 2 Web APIs.
- IndexedDB, Canvas 2D.
- HTTP/3.
- Service Workers.
- WebFonts (WOFF2).
- Расширения (свой минимальный формат).
- WPT pass rate ≥ 60%.
- **Цель:** стабильный релиз.

### Фаза 4 — После 1.0
- Подмножество WebGL (по запросам).
- Mobile (Android через NDK; iOS — упрётся в Apple-policy).
- Sync (E2E-шифрованный, опциональный, self-host).
- Локализация.

---

## 15. Команда и ресурсы

| Фаза | Состав | Длительность |
|---|---|---|
| 0 — прототип | 2 senior Rust | 3 мес |
| 1 — v0.1 | 3–4 (Rust, систем, UX) | 9 мес |
| 2 — v0.5 | 5–7 (+ JS-эксперт, security) | 12–18 мес |
| 3 — v1.0 | 8–12 | 18–24 мес |

Бюджетная оценка: **минимум 4–5 миллионов USD до v1.0** (если коммерчески), или 4–5 лет с маленькой full-time командой энтузиастов.

---

## 16. Риски и митигация

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

## 17. Лицензия

- **MPL-2.0** — позволяет связывание со внешним кодом, требует open-source модифицированных файлов. Совместимо с экосистемой Servo/Firefox.
- DCO вместо CLA.
- Публичный roadmap, RFC-процесс.

---

## 18. Первые конкретные шаги

1. `cargo new --bin lumen` + создать workspace с пустыми crates.
2. `engine/html-parser` — взять `html5ever` или начать со скелета токенизатора. Прогнать `html5lib-tests`.
3. `engine/css-parser` — `cssparser` + `selectors`.
4. `engine/dom` — арена, NodeId, базовые API.
5. `engine/layout` — block + inline; интегрировать `taffy`.
6. `engine/paint` + `tiny-skia` — нарисовать первый бокс.
7. `shell` — окно winit + egui, рендер картинки от движка.
8. **Веха «hello world»:** открыть страницу `<html><body><h1>Hello</h1></body></html>` локально, увидеть текст.
9. **Веха «Wikipedia»:** открыть статью Википедии, прокрутить, перейти по ссылке.
10. После этого — `network` отдельным процессом, IPC.

---

## 19. Чего я НЕ обещаю

- Что v1.0 будет «как Chrome». Не будет. Будет браузер, в котором работает 80% сайтов и который вы понимаете до последней строки.
- Что это коммерчески выгодно. Скорее всего, нет — это исследовательский / идеологический проект.
- Что Servo/Ladybird не обгонят. Возможно, обгонят. Тогда имеет смысл влить силы туда.
