# Lumen

Приватный, лёгкий, прозрачный браузер на Rust с собственным движком.

> **Lumen** (лат. *свет*, единица светового потока) — показывает пользователю всё, что происходит, и не весит больше, чем нужно.

## Зачем это

Существующие браузеры — либо проприетарные обёртки над Chromium (Chrome, Edge, Opera, Brave, Arc), либо форки Firefox. В обоих случаях приоритеты пользователя не совпадают с приоритетами компании-владельца: телеметрия по умолчанию, реклама как источник дохода, локализация и кириллица — по остаточному принципу, движок — чёрный ящик в несколько миллионов строк, который нельзя ни прочитать целиком, ни поменять архитектурно.

Lumen — попытка сделать браузер, где:

- Никакой телеметрии и облачных сервисов по умолчанию.
- Адблок встроен в сетевой стек и не может быть отключён давлением рекламной модели платформы (как это происходит с Manifest V3 в Chrome).
- Кириллица, русская локаль и не-латинские шрифты/кодировки — часть движка с первого дня, а не патч поверх.
- Каждое расширение — WASM-песочница с явными capability-разрешениями (модель как у Zed и Figma), а не произвольный JS с доступом ко всему.
- Движок для HTML/CSS/DOM/layout/paint/JS-биндингов написан с нуля. Внешних библиотек — минимум, и каждая обоснована (см. [«Что не своё»](#что-не-своё-и-почему)).

Подробный design doc, мотивация по каждому пункту и план развития по фазам — [lumen-plan.md](lumen-plan.md).

## Что это такое на практике

Lumen — это не только «движок рендеринга HTML». Это три слоя, которые можно рассматривать по отдельности:

1. **Браузер** — обычное десктопное приложение: вкладки, вкладочные группы, адресная строка, история, закладки, загрузки, заметки/read-later, workspaces (профили-в-профиле), настройки, разрешения по сайтам, приватность/shields, DevTools.
2. **Движок** — своя реализация тех частей веб-платформы, без которых нельзя открыть современный сайт: HTML5-парсер, CSS-парсер и каскад, DOM, layout (block/inline/flex/grid/таблицы/многоколоночная вёрстка), paint (CPU- и GPU-рендер), шрифты и текст (шейпинг, bidi, перенос строк), Canvas 2D, SVG, работа с изображениями и цветом. JavaScript выполняется движком **V8** (`rusty_v8`) — тем же, что в Chrome/Node.js, — но все DOM/Web API вокруг него написаны в Lumen.
3. **Платформа автоматизации** — браузером можно управлять программно: через WebDriver BiDi (стандартный протокол Selenium/Playwright), через Model Context Protocol (MCP — для AI-агентов) и через нативный Rust API (`BrowserSession`). Это делает Lumen пригодным и как обычный браузер, и как headless-движок для тестов/скриптов/AI-агентов.

## Как это устроено внутри

Путь от файла на диске до пикселя на экране:

```
HTML-байты ──► HTML-парсер ──► DOM
                                 │
CSS-байты ──► CSS-парсер ──► каскад (style) ──► Layout (геометрия)
                                                     │
                                                   Paint (display list)
                                                     │
                                          ┌──────────┴──────────┐
                                          ▼                     ▼
                                   CPU-растеризатор      GPU (wgpu)
                                   (детерминированные      (живое окно,
                                    скриншоты, тесты)       Vulkan/DX12/GL)
```

JavaScript не стоит рядом с этим пайплайном, а управляет им: скрипт через DOM API читает и меняет дерево, мутация запускает пересчёт стилей → layout → paint автоматически.

```
JS-код ──► V8 ──► DOM / CSSOM / fetch / Canvas / Storage / Workers / … ──► (снова в пайплайн выше)
```

Слои живут в отдельных крейтах Rust-workspace и не образуют циклов зависимостей: `lumen-core` → парсеры/DOM/шрифты → layout → paint → shell (пользовательский интерфейс). Полная схема крейтов — ниже, в [«Структура проекта»](#структура-проекта).

## Что уже работает

Это сводка. Подробный, построчно сверяемый с кодом список — **единственный источник истины** — [`CAPABILITIES.md`](CAPABILITIES.md); там же список того, чего ещё нет (помечено ⬜). Разбивка по CSS-свойствам — [`CSS-SPECS.md`](CSS-SPECS.md).

- **HTML** — полный HTML5-токенайзер и tree builder (все 23 insertion mode), именованные и числовые entity, `srcset`/`sizes`/`<picture>`, Declarative Shadow DOM, potенциальный incremental-парсинг по мере прихода байт по сети.
- **CSS** — свой парсер селекторов (L3 + большая часть L4, включая `:has()`, `:is/:where/:not`, `:nth-*(of …)`) и деклараций; каскад с `@media`/`@supports`/`@layer`/`@scope`/`@container`/`@property`/`@font-face`/`@keyframes`; ~139 свойств проведены от парсинга до отрисовки (полный список — `CSS-SPECS.md`).
- **Layout** — block- и inline-flow (перенос строк, схлопывание отступов, baseline-выравнивание инлайн-контента), Flexbox, CSS Grid (включая subgrid), многоколоночная вёрстка, таблицы, позиционирование (`relative/absolute/fixed/sticky`), floats, CJK line-breaking, bidi/RTL, CSS Anchor Positioning.
- **Paint/рендер** — два независимых бэкенда: GPU через `wgpu` (Vulkan/DX12/GL — живое окно) и детерминированный CPU-растеризатор (для тестов и headless-скриншотов). Градиенты, тени, фильтры, backdrop-filter, `clip-path`, 3D-трансформации, маски, стэкинг-контексты, wide-gamut (DisplayP3/Rec2020) вывод.
- **Шрифты и текст** — собственный TrueType/OpenType-парсер, шейпинг, перенос строк по Unicode UAX #14/#29, hyphenation (11 локалей), bidi по UAX #9, COLR/CPAL цветные глифы, вертикальные режимы письма (`writing-mode`).
- **JavaScript и Web API** — **V8**, единственный JS-движок в проекте (миграция с QuickJS завершена в августе 2026, `rquickjs` полностью удалён из workspace). DOM API, fetch/XHR/WebSocket/EventSource, Web Workers/Shared Workers, Service Workers + Cache Storage, IndexedDB, Web Storage, Web Animations, WebAuthn/passkeys, SubtleCrypto, Canvas 2D, software WebGL 1.0. Покрытие Web Platform Tests неполное — часть API размечена частично (🟡) в `CAPABILITIES.md`.
- **Сеть** — HTTP/1.1, HTTP/2, HTTPS (rustls), брoтли/gzip/deflate, куки, CORS, HSTS, DNS-over-HTTPS/TLS, SOCKS5-прокси, блокировка рекламы на уровне фильтров (EasyList-совместимые правила, по умолчанию выключена). HTTP/3 (QUIC) — в разработке.
- **Хранилища** — всё на SQLite: история, закладки, заметки, read-later, куки, IndexedDB, Service Worker store, загрузки, разрешения по сайтам, workspaces, вкладочные сессии. Профильное хранилище шифруется (AES-256-GCM + PBKDF2).
- **Интерфейс браузера** — вкладки (вертикальные/горизонтальные, группы), адресная строка с подсказками, командная палитра, настройки, менеджер закладок/истории/загрузок, панель приватности (shields), сертификатный вьюер, DevTools (консоль, DOM-инспектор, сеть), Picture-in-Picture, печать в PDF.
- **Автоматизация** — WebDriver BiDi сервер (реальная навигация/eval/скриншоты/ввод против живого окна), MCP-сервер (тот же набор как инструменты для AI-агента), нативный `BrowserSession` API, CDP-подмножество (`--devtools-port`), headless-режимы с детерминированным CPU-рендером — используется в собственном тестовом стенде и Web Platform Tests runner'е.
- **Accessibility** — дерево доступности по WAI-ARIA 1.2 поверх Shadow DOM composed tree, интеграция с Windows UI Automation (MSAA/UIA события).

## Чего пока нет или сделано частично

- Полной совместимости с современным вебом нет — часть CSS L3/L4 свойств распарсена, но не влияет на рендер (см. ⬜/🟡 в `CAPABILITIES.md`), часть Web API — заглушки, которые резолвятся/реджектятся, но не работают (например часть WebCodecs/WebHID).
- HTTP/3 (QUIC) — низкоуровневые кодеки готовы, полная интеграция в сетевой клиент ещё нет.
- CDP (Chrome DevTools Protocol) — только небольшое подмножество, не для полноценной замены `chrome-devtools-protocol`-клиентов.
- Композитный (не-Windows) accessibility-мост для macOS/Linux — в разработке.
- Полный список открытых дефектов — [`BUGS.md`](BUGS.md), исправленных — [`BUGS-FIXED.md`](BUGS-FIXED.md), план развития — [`ROADMAP.md`](ROADMAP.md).

## Требования

- **Rust ≥ 1.95** (`rust-version` в `Cargo.toml`). CI и чекаут по умолчанию собираются на 1.97.0 из [`rust-toolchain.toml`](rust-toolchain.toml) — `rustup` подхватит её сам; более новая (например 1.98) тоже работает: `rustup override set <версия>` в каталоге репозитория. Подробности — [`docs/conventions.md`](docs/conventions.md).
- **Windows:** Visual Studio Build Tools 2022+ (MSVC-линкер `link.exe`).
- **Linux:** GCC/Clang, X11/Wayland dev-пакеты (для `winit`).
- **macOS:** Xcode Command Line Tools.
- **sccache ≥ 0.17.0** — обязателен, репозиторный `.cargo/config.toml` включает его как `RUSTC_WRAPPER`; более старая версия падает с `0xc0000409` на каждом вызове `rustc`/`clippy-driver`.

### Установка Rust

Если Rust ещё нет, поставь `rustup` — официальный менеджер версий:

**Windows:**
```powershell
winget install Rustlang.Rustup
```
Если Visual Studio Build Tools отсутствует, `rustup-init.exe` предложит его установить.

**Linux / macOS:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

После установки перезапусти терминал и проверь:
```bash
rustc --version    # 1.97.0 по умолчанию, или твой override
cargo --version
```

### Установка sccache

```bash
cargo install sccache --version 0.17.0
sccache --version
```

Если sccache не нужен — собирай с `RUSTC_WRAPPER=` (пустое значение перебивает конфиг), так делает и CI.

## Сборка и запуск

Клонируй репозиторий и из его корня:

```bash
# Быстрая проверка (без линковки бинаря)
cargo check

# Полная debug-сборка
cargo build

# Открыть браузер
cargo run -p lumen-shell

# Открыть конкретный файл или URL
cargo run -p lumen-shell -- samples/page.html
cargo run -p lumen-shell -- https://example.com

# Прогнать все тесты
cargo test --workspace

# Линтер строго (warnings = ошибки)
cargo clippy --workspace --all-targets -- -D warnings
```

При первой сборке Cargo скачает несколько сотен транзитивных зависимостей (в основном из-за `wgpu` — это GPU-абстракция, и `rusty_v8` — предсобранный движок JavaScript). Это может занять от нескольких минут до пары десятков в зависимости от интернета и CPU. Последующие сборки — секунды, благодаря sccache и инкрементальной компиляции.

Полностью оптимизированная сборка (то, что нужно для реального использования браузера, а не для разработки):

```bash
cargo build --release
```

### Полезные флаги запуска

- `--maximized` — открыть окно развёрнутым (рекомендуется при живом тестировании реальных сайтов — маленькое окно меняет CSS-вьюпорт и скрывает часть багов).
- `--devtools-port <N>` — поднять CDP-подобный WebSocket-сервер.
- `--bidi-port <N>` — поднять WebDriver BiDi сервер против живого окна.
- `--mcp-live-port <N>` / `--mcp [url]` — отдать браузер как инструмент MCP-серверу (для AI-агентов).
- `--ipc-server` — TCP RPC для управления вкладками извне.

Подробности автоматизационных поверхностей (порты, токены доступа, протоколы) — [`docs/automation.md`](docs/automation.md).

## Структура проекта

```
lumen-browser/
├── Cargo.toml                 — workspace
├── lumen-plan.md               — design doc и план фаз
├── CAPABILITIES.md             — что реально работает прямо сейчас (истина)
├── ROADMAP.md                  — план развития
├── BUGS.md / BUGS-FIXED.md     — открытые / исправленные дефекты
├── CSS-SPECS.md                — статус каждого CSS-свойства
├── rust-toolchain.toml         — пин версии Rust
├── docs/                       — архитектура, инварианты, гайды разработки
├── subsystems/                 — документация по каждому крейту
├── samples/                    — тестовые HTML-страницы
└── crates/
    ├── shell/                  — бинарь браузера: окно, интерфейс, точка входа
    ├── chrome/                 — рендер собственного UI (тулбар, вкладки, панели)
    ├── core/                   — общие типы: Error, Url, Event, Capability, geometry
    ├── engine/
    │   ├── html-parser/        — HTML5-токенайзер и tree builder
    │   ├── css-parser/         — CSS-селекторы и декларации
    │   ├── dom/                — arena-based DOM
    │   ├── layout/             — каскад стилей, block/inline/flex/grid-раскладка
    │   ├── paint/               — display list, CPU- и wgpu-рендер
    │   ├── font/                — TrueType/OpenType-парсер, растеризация глифов
    │   ├── canvas/               — Canvas 2D
    │   ├── a11y/                 — дерево доступности (WAI-ARIA)
    │   ├── encoding/             — кодировки (cp1251/KOI8-R/…), Unicode line-break/bidi
    │   ├── image/                — декодирование изображений
    │   └── media-ffmpeg/         — декодирование видео (`<video>`)
    ├── js/                      — биндинги V8 ↔ DOM/Web API
    ├── network/                 — HTTP/HTTPS/HTTP2, DNS, куки, адблок
    ├── storage/                 — SQLite-хранилища (история, куки, IndexedDB, …)
    ├── knowledge/               — полнотекстовый поиск по истории/заметкам, омнибокс-команды
    ├── ai/                      — AI-панель, backend-абстракция
    ├── ipc/                     — межпроцессный RPC (управление вкладками)
    ├── driver/                  — headless-интерфейс движка (BrowserSession)
    ├── bidi-server/             — WebDriver BiDi сервер
    ├── mcp/                     — Model Context Protocol сервер
    ├── devtools/                — CDP-over-WebSocket (частичный)
    ├── renderer/                — низкоуровневые примитивы отрисовки
    └── bench/                   — бенчмарки
```

## Что не своё, и почему

Lumen пишется с нуля — собственный движок, а не обёртка над Blink/WebKit/Gecko. Внешние зависимости допускаются только там, где решение уже принято индустрией и переизобретать его вредно:

| Зависимость | Зачем | Почему не сами |
|---|---|---|
| `winit` | OS event loop + окна | Win32 + X11 + Wayland + AppKit — годы платформенных багов |
| `wgpu` | GPU API (Vulkan/Metal/DX12/GL) | Четыре разных графических API, драйверные баги, годы работы |
| `rustls` | TLS + криптография | Общее правило безопасности: не пишите свой крипто-код |
| `rusty_v8` (V8) | Исполнение JavaScript | 15+ лет работы Google над JIT-компилятором и спецификацией ECMAScript |
| `rusqlite` (SQLite) | Персистентное хранилище | Проверенный десятилетиями движок хранения данных |

Всё остальное — HTML/CSS-парсеры, DOM, layout, paint, шрифтовый рендеринг, сетевой стек поверх TCP, интерфейс браузера — код Lumen. Полное обоснование границы «своё vs готовое» — [ADR-027](docs/decisions/ADR-027-own-vs-vendored-boundary.md) и [`docs/plan/tech-stack.md`](docs/plan/tech-stack.md) §5.

## Тестирование

- Юнит- и интеграционные тесты по крейтам: `cargo test -p <crate>`.
- Пиксельные (графические) тесты сверяют рендер с эталонами — см. [`docs/graphic-tests.md`](docs/graphic-tests.md).
- Соответствие веб-стандартам проверяется вендоренными Web Platform Tests через собственный WebDriver BiDi раннер — см. [`docs/wpt-status.md`](docs/wpt-status.md).
- Тестируешь свой сайт в Lumen? Гайд с рабочими примерами (Selenium/WebDriver BiDi, MCP, нативный Rust API) — [`docs/testing-your-site-with-lumen.md`](docs/testing-your-site-with-lumen.md).

## Лицензия

- **Код Lumen:** [MPL-2.0](https://www.mozilla.org/MPL/2.0/).
- **Bundled шрифт Inter:** [SIL Open Font License 1.1](assets/fonts/OFL.txt). Совместимо с MPL.

## Куда дальше

- [`lumen-plan.md`](lumen-plan.md) — подробный design doc (scope, архитектура, план фаз, плагинная модель).
- [`CAPABILITIES.md`](CAPABILITIES.md) — точное, построчно проверяемое состояние каждой подсистемы.
- [`ROADMAP.md`](ROADMAP.md) — что делается дальше.
- `samples/page.html` — тестовая страница; открой её в Lumen и в «настоящем» браузере рядом, чтобы сравнить.
- [`docs/testing-your-site-with-lumen.md`](docs/testing-your-site-with-lumen.md) — как проверить свой сайт в Lumen.
