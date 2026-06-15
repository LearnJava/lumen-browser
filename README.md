# Lumen

Приватный, лёгкий, прозрачный браузер на Rust с собственным движком.

> **Lumen** (лат. *свет*, единица светового потока) — показывает пользователю всё, что происходит, и не весит больше, чем нужно.

## Зачем это

Существующие браузеры либо проприетарные обёртки над Chromium, либо нишевые форки Firefox. Ни в одном из них приоритеты пользователя не совпадают с приоритетами компании-владельца (Google, Microsoft, Mozilla, Apple, Brave Software). Lumen — попытка сделать браузер, где:

- Никакой телеметрии и облачных сервисов по умолчанию.
- Адблок не отключается под давлением рекламной модели платформы.
- Кириллица и русская локаль — first-class, не «потом».
- Каждый плагин — WASM-песочница с явными capability-разрешениями (как Zed и Figma).
- Движок написан с нуля (без обёрток над Blink/WebKit), но с четырьмя строго оговорёнными исключениями (см. ниже).

Подробный design doc и план фаз — [lumen-plan.md](lumen-plan.md).

## Текущее состояние

**Phase 2 — v0.5 «Interactive» (в работе), версия приложения v0.2.0.** Phase 0 (прототип) закрыта, Phase 1 «Reader» в основном выполнена. Сейчас в активной разработке интерактивный слой: QuickJS, Canvas 2D, CSS Grid, Shadow DOM, accessibility tree, формы, find-in-page, DevTools/CDP, knowledge layer; часть фич Phase 3 (IndexedDB, Service Workers, WebSockets, WOFF2, печать в PDF) уже подтянута вперёд.

> Актуальный детальный статус реализации — в [docs/plan/status.md](docs/plan/status.md) и `STATUS-PN.md`. Список ниже — ранние вехи прототипа (Phase 0); полный набор возможностей давно шире.

Базовые вехи движка:

- ✅ Свой HTML-парсер (~30 тестов, обрабатывает `samples/page.html` с кириллицей, entities, комментариями)
- ✅ Свой CSS-парсер (~20 тестов, селекторы type/class/id/universal, кириллический `.привет`)
- ✅ Block-flow layout со style cascade (~17 тестов, наследование, color/font-size/margin/padding)
- ✅ Окно через winit + wgpu, рисуем фоновые прямоугольники
- ✅ Свой TrueType-парсер (~60 тестов, парсит bundled Inter-Regular.ttf)
- ✅ Scanline-растеризатор глифов (квадратичные Безье, even-odd fill, 4×4 AA)
- ✅ Текст в окне (glyph atlas + рендеринг через femtovg-бэкенд)

Тесты прогоняются покрейтно (`cargo test -p <crate>`), `cargo clippy -p <crate> --all-targets -- -D warnings` чист перед каждым коммитом.

## Требования

- **Rust 1.95+** stable. Версия закреплена в [`rust-toolchain.toml`](rust-toolchain.toml).
- **Windows:** Visual Studio Build Tools 2022+ (для MSVC-линкера `link.exe`).
- **Linux:** GCC или Clang, X11/Wayland dev-пакеты (для winit).
- **macOS:** Xcode Command Line Tools.

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
rustc --version    # должно показать 1.95.0 или новее
cargo --version
```

## Сборка и запуск

Клонируй репозиторий и из его корня:

```bash
# Быстрая проверка (без линковки бинаря) — обычно 1–2 секунды
cargo check

# Полная debug-сборка
cargo build

# Открыть пустое окно
cargo run -p lumen-shell

# Открыть HTML-файл: распарсить, layout, paint в окне
cargo run -p lumen-shell -- samples/page.html

# Прогнать все тесты
cargo test --workspace

# Прогнать линтер строго (warnings = ошибки)
cargo clippy --workspace --all-targets -- -D warnings
```

При первой сборке Cargo скачает ~200 транзитивных зависимостей (в основном из-за `wgpu` — это GPU-абстракция). Это занимает 3–10 минут в зависимости от интернета и CPU. Последующие сборки — секунды.

## Что увидишь

### `cargo run -p lumen-shell`
Окно 1024×720 с заголовком «Lumen 0.2.0» (версия берётся из `Cargo.toml`), белый фон. Закрытие — крестик в углу. Это «нулевая» проверка, что winit/wgpu работают на твоей системе.

### `cargo run -p lumen-shell -- samples/page.html`
Окно с распарсенной [`samples/page.html`](samples/page.html): фоновые цвета блоков, текст через bundled Inter, рамки, инлайн-поток. Сравнить с тем, как страницу показывает «настоящий» браузер, можно открыв тот же HTML в Chrome/Firefox.

В консоль печатается:
```
Lumen v0.2.0 — Phase 2 (Interactive, in progress)
Распарсено: 47 DOM-узлов, 7 CSS-правил, 8 paint-команд
```

### Поглядеть на работу растеризатора шрифтов

```bash
cargo run --example preview -p lumen-font
```

Печатает ASCII-арт нескольких букв (`A`, `M`, `g`, `Я`, `ж`, `п`, `?`) из bundled `Inter-Regular.ttf` через наш собственный TrueType-парсер и растеризатор. Должны быть узнаваемые формы букв.

## Структура проекта

```
Lumen-browser/
├── Cargo.toml                 — workspace
├── lumen-plan.md              — подробный design doc и план фаз
├── rust-toolchain.toml        — пин версии Rust
├── assets/
│   └── fonts/
│       ├── Inter-Regular.ttf  — bundled шрифт (SIL OFL 1.1)
│       └── OFL.txt            — текст лицензии шрифта
├── samples/
│   └── page.html              — тестовая страница с кириллицей
└── crates/
    ├── shell/                 — бинарь `lumen`: окно, ввод, точка входа
    ├── core/                  — общие типы (Error, Url, Event, Capability, Module, geometry)
    └── engine/
        ├── html-parser/       — HTML5 tokenizer + lenient tree builder
        ├── css-parser/        — селекторы и declarations
        ├── dom/               — arena-based DOM (NodeId, Document, Node)
        ├── layout/            — block flow + style cascade
        ├── paint/             — display list + wgpu-rasterizer
        └── font/              — TrueType parser + glyph rasterizer
```

## Политика зависимостей

Lumen пишется с нуля — собственный движок, не обёртка. Внешних crate-зависимостей **четыре** (см. §5 плана):

| Зависимость | Зачем | Почему не сами |
|---|---|---|
| `winit` | OS event loop + окна | Win32 + X11 + Wayland + AppKit = годы платформенных багов |
| `wgpu` | GPU API (Vulkan/Metal/DX12/GL) | 4 разных API, driver-баги, годы работы |
| `rustls` | TLS + крипто (когда подключим сеть) | Универсальное правило безопасности: не пишите свой crypto |
| JS engine (QuickJS → V8) | Исполнение JavaScript | 15 лет работы Google/Mozilla |

Всё остальное (парсеры HTML/CSS, DOM, layout, paint, шрифты, HTTP, и т.д.) — свой код. Подробности в [§5 плана](lumen-plan.md#5-технологический-стек).

## Разработка

### Workflow с git

- Главная ветка — `main`.
- Каждая задача делается в отдельной feature-ветке (`text-rendering`, `font-atlas`, и т.п.) и сливается в `main` через `git merge --no-ff` (видна структура).
- Внутри ветки — несколько коммитов на логические шаги задачи.
- Тесты должны проходить перед коммитом (`cargo check` минимум).
- Commit-сообщения на русском, тело объясняет «почему», не «что».

### Запуск конкретного теста

```bash
# Все тесты конкретного крейта
cargo test -p lumen-font

# Конкретный модуль
cargo test -p lumen-html-parser tokenizer::

# Конкретный тест по имени
cargo test -p lumen-font rasterize_uppercase_a

# Интеграционные тесты на bundled Inter
cargo test -p lumen-font --test inter_real_font
```

### Профили сборки

`Cargo.toml` использует `opt-level = 1` для dev-профиля. Это компромисс: debug-сборка идёт чуть медленнее, зато сам движок (особенно layout/paint) работает в 5–10 раз быстрее, чем при `opt-level = 0`. Стандартный приём в графических Rust-проектах.

Полностью оптимизированная сборка:
```bash
cargo build --release
```

### IDE

Проект — стандартный Rust-workspace, любая IDE с `rust-analyzer` подхватит:
- VS Code + rust-analyzer extension
- IntelliJ IDEA / RustRover
- Helix, Neovim с LSP, и т.д.

## Известные ограничения

Движок Phase 2 многое уже умеет (полный список и текущие пробелы — в [docs/plan/status.md](docs/plan/status.md), [BUGS.md](BUGS.md) и `CSS-SPECS.md`). Крупные нереализованные блоки:

- HTML parser — без полного набора HTML5 insertion modes; lenient к ошибкам.
- JS-движок — QuickJS (rquickjs), не V8; переход на `rusty_v8` запланирован на Phase 3.
- Сетевой стек — HTTP/1.1 + HTTP/2; HTTP/3 (QUIC) — позже.
- Часть CSS-свойств и WPT-покрытие ещё в работе (см. `CSS-SPECS.md`).
- JavaScript — нет.
- Кодировки — только UTF-8 на входе. cp1251/KOI8-R — задача §10.1 плана.
- Composite glyphs в шрифтах не растеризуются (например, кириллическая `А` в Inter, которая собрана из латинской `A`). Уникальные кириллические буквы (Я, ж, п и т.д.) — работают.

Полный список в `lumen-plan.md`.

## Лицензия

- **Код Lumen:** [MPL-2.0](https://www.mozilla.org/MPL/2.0/).
- **Bundled шрифт Inter:** [SIL Open Font License 1.1](assets/fonts/OFL.txt). Совместимо с MPL.

## Куда дальше

- [`lumen-plan.md`](lumen-plan.md) — подробный design doc (~20 разделов, от scope до плагинной модели).
- `samples/page.html` — тестовая страница, открой её в обоих браузерах и сравни.
