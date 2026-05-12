# CLAUDE.md

Контекст проекта для Claude Code. Этот файл подгружается автоматически в каждой сессии, открытой в этой папке. Цель — чтобы Claude (или другой ассистент) сразу знал суть проекта, не задавая вопросов, на которые ответ есть в коде или соседних документах.

Если правишь архитектуру, инварианты или политики — синхронизируй это и здесь.

---

## Что это

**Lumen** — приватный, лёгкий, прозрачный браузер на Rust с собственным движком. Не обёртка над Chromium / WebKit, а отдельный rendering engine, JS-движок встраивается готовый.

Текущая фаза — **Phase 0 (прототип)**. Цель фазы: открыть локальный HTML с CSS и нарисовать в окне через собственный pipeline. На момент написания этого файла фаза близка к завершению — открывается `samples/page.html`, рисуются фоны и текст через bundled Inter.

Главные документы:

| Файл | Что внутри |
|---|---|
| `README.md` | User-facing: установка, команды, что увидит пользователь. |
| `lumen-plan.md` | Подробный design doc (~1200 строк, 22 главы): принципы, scope, архитектура, фазы, уникальные фичи. **Шапка содержит блок «Статус реализации»** — обновляй при каждой реализации пункта плана. |
| `CLAUDE.md` | (этот файл) Конвенции и инварианты для ассистента. |
| `samples/page.html` | Тестовая страница для прогона pipeline. |
| `assets/fonts/Inter-Regular.ttf` | Bundled шрифт (SIL OFL 1.1). |

---

## Команды для работы

```bash
# Быстрая проверка (без линковки) — 1-2 сек.
cargo check

# Все тесты в workspace.
cargo test --workspace

# Тесты конкретного крейта.
cargo test -p lumen-font

# Интеграционные тесты на bundled Inter.
cargo test -p lumen-font --test inter_real_font

# Clippy строго (warnings = ошибки). Обязательно перед коммитом.
cargo clippy --workspace --all-targets -- -D warnings

# Запуск браузера с тестовой страницей (фоны + текст).
cargo run -p lumen-shell -- samples/page.html

# Пустое окно.
cargo run -p lumen-shell

# ASCII-превью растеризации глифов из Inter.
cargo run --example preview -p lumen-font
```

### Важно про PATH (Windows + Git Bash)

`cargo` установлен через `winget Rustlang.Rustup` и лежит в `C:\Users\konstantin\.cargo\bin`. В Git Bash на этой машине эта папка **не подхватывается автоматически**. Перед каждой командой `cargo` (или после установки в `~/.bashrc`):

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
```

В новых терминалах cmd / PowerShell это не нужно — там PATH корректный.

### Текущее число тестов и crates

На момент написания: 155 тестов, 8 крейтов (`shell`, `core`, `dom`, `html-parser`, `css-parser`, `layout`, `paint`, `font`). При прохождении следующих фаз появятся `lumen-knowledge`, `lumen-ai`, `lumen-network` и др.

---

## Архитектура (краткая)

### Структура workspace

```
crates/
├── shell/                — бинарь `lumen`: окно, ввод, точка входа
├── core/                 — фундамент: типы, trait-точки расширения
└── engine/
    ├── html-parser/      — HTML5 tokenizer + tree builder
    ├── css-parser/       — селекторы и declarations
    ├── dom/              — arena-based DOM (NodeId, Document)
    ├── layout/           — block flow + style cascade
    ├── paint/            — display list + wgpu-rasterizer + glyph atlas
    └── font/             — TrueType parser + scanline rasterizer
```

### Направление зависимостей

Однонаправленное, без циклов. Внизу — `lumen-core`. Все остальные крейты зависят на него; он не зависит ни на что Lumen-внутреннее.

```
                       ┌──────────────┐
                       │  lumen-core  │
                       └──────┬───────┘
              ┌───────────────┼───────────────┐
              │               │               │
       lumen-dom         lumen-font   (другие крейты)
              │               │
       lumen-html-parser      │
       lumen-css-parser       │
              │               │
              └──→  lumen-layout  ←──┘
                          │
                    lumen-paint
                          │
                    lumen-shell
```

### Trait-точки расширения

Все живут в `lumen-core::ext`. Каждая — место, где может появиться альтернативная реализация без правки потребителей.

**Уже определены:** `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `EncodingDetector`.

**Запланированы:** `WindowingBackend` (за winit), `RenderBackend` (за wgpu), `TlsBackend` (за rustls), `JsRuntime` (за V8/QuickJS), `KnowledgeStore`, `AiBackend`. Подробно — в §12 плана.

---

## Принципы (8 штук, §1 плана)

1. **Приватность по умолчанию** — никакой телеметрии, аккаунтов, облака.
2. **Лёгкость** — холодный старт < 300 мс, RAM < 100 МБ на пустую вкладку.
3. **Контролируемая поверхность** — экзотические Web API (WebUSB, WebBluetooth, FedCM, и т.д.) не реализуем принципиально.
4. **Прозрачность** — каждый исходящий байт виден пользователю в network log.
5. **Стабильный UI** — никаких «редизайнов» каждый релиз.
6. **Memory safety** — `unsafe` только на FFI-границах, всё ревьюится.
7. **Русский язык — first-class** — кодировки, шрифты, IDN, локаль, переводы. На всех этапах. См. §10.
8. **Knowledge layer как ценность для пользователя** — полнотекстовый поиск истории, аннотации, офлайн-чтение, опциональный локальный AI. Это то, что массовые браузеры не делают по бизнес-причинам. См. §12.

---

## Политика зависимостей

**Default — пишем сами.** Lumen — про собственный движок, не про обёртку над `html5ever` / `taffy` / `image` / `tokio`.

### Четыре разрешённых exception (только эти, всё остальное — свой код)

| Crate | За что | Почему не сами |
|---|---|---|
| `winit` | OS event loop + окна | Win32 + X11 + Wayland + AppKit — годы платформенных багов |
| `wgpu` | GPU API (Vulkan/Metal/DX12) | 4 разных API, driver-баги, годы работы |
| `rustls` | TLS / crypto (когда подключим сеть) | **Никогда не пиши свой crypto** |
| JS engine (`rquickjs` / `rusty_v8`) | Исполнение JavaScript | 15 лет работы Google/Mozilla |

Подробности и таблица «зачёркнутых» зависимостей (что пишем сами, хотя соблазн взять готовое) — в §5 плана.

### Правило «no new dep без обоснования»

Если в коммите добавляется новая запись в `[dependencies]`, в commit-теле обязателен пункт:

> **Why this dependency:** \<обоснование, почему свой код категорически неуместен\>

Без обоснования — пишем сами.

---

## Конвенции кода

### Rust версия и edition

- **Rust 1.95+ stable**, зафиксировано в `rust-toolchain.toml`.
- **Edition 2024**, resolver "3".
- MSVC toolchain на Windows.

### Стиль

- Профиль `dev` использует `opt-level = 1` (компромисс: debug-сборка медленнее на 10%, но layout/paint работают в 5-10 раз быстрее).
- `clippy::all` + `clippy::pedantic` пока **не включены** глобально, но `cargo clippy --workspace --all-targets -- -D warnings` обязан проходить перед коммитом.
- Никаких лишних комментариев: только если объясняют *почему*, а не *что*. Doc-комментарии (`///`) на публичных API — приветствуются.
- Имена — `snake_case` для функций/полей, `PascalCase` для типов, `SCREAMING_SNAKE` для констант (стандарт Rust).

### Tests-first для парсеров и алгоритмов

Для парсеров (`html-parser`, `css-parser`, `font`) и алгоритмов (rasterizer, layout) сначала пишутся тесты, потом код. Иначе очень легко получить код, который выглядит правильно, но не работает на спорных входах.

Особенно — **интеграционные тесты на реальных данных**. Юнит-тесты на синтетических байтах TTF прошли успешно, но баг в hhea-парсере (skip 16 вместо 22) обнаружил только интеграционный тест на bundled Inter. Урок: синтетика **не заменяет** реальность.

### Обработка ошибок

- В user-facing API — `Result<T, E>` с осмысленным `Error` enum.
- Internal: `Option` где None означает «не нашли» / «не applicable» (а не «ошибка»).
- Никаких `panic!` / `unwrap()` в продакшн-коде; в тестах — допустимо.
- На FFI-границах (wgpu, V8 в будущем) — `unsafe` инкапсулирован в один модуль, документирован, ревьюится.

### Регулирование `unsafe`

- Запрещён вне FFI-границ.
- Каждый `unsafe`-блок должен иметь `// SAFETY:` комментарий.

---

## Git workflow

### Ветки

Каждая задача — отдельная feature-ветка от свежего `main`:

```bash
git checkout -b text-rendering
# ... коммиты ...
git checkout main
git merge --no-ff text-rendering -m "Влить ветку text-rendering: ..."
git branch -d text-rendering
```

**`--no-ff` обязателен** — сохраняет видимую структуру «эта серия коммитов = одна задача» в `git log --graph`.

Имена веток — короткие kebab-case, без префиксов (`text-rendering`, `font-atlas`, `http-client`, `claude-md`).

### Коммиты

- **Один логический шаг = один коммит.** Не батчить несвязанные изменения.
- **Перед коммитом** должно проходить минимум `cargo check`. Лучше — полные тесты + clippy.
- **Сообщение на русском.** Заголовок краткий (под 80 символов), потом пустая строка, тело объясняет *почему*, не *что* (что видно по diff).
- **Trailer всегда в конце:**
  ```
  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```
  Машинно-читаемый, остаётся английским.
- **Стейджинг по конкретным файлам** (`git add path1 path2`), не `git add -A` / `.` — защита от случайного попадания секретов / архивов.

### Запрещено

- Force-push на `main`.
- Rewrite опубликованной истории.
- `git config` изменения (никогда).
- Skip hooks (`--no-verify`).
- `git push` без явной просьбы пользователя.

---

## Коммуникация (когда отвечаешь пользователю)

- **Язык ответа — русский** по умолчанию. Пользователь говорит по-русски.
- **Тон — техничный, без лишних эмодзи**. Эмодзи только если пользователь сам их использует.
- **Кратко, по делу.** Если ответ на вопрос — короткий ответ + что сделал. Не разворачивать в развёрнутые маркетинговые тексты.
- **Файлы кликабельными ссылками:** `[lumen-plan.md](lumen-plan.md)`, `[crates/engine/font/src/rasterizer.rs:48](crates/engine/font/src/rasterizer.rs)`.

### Запрещённые слова

«Wikipedia» / «Википедия» — пользователь явно попросил не использовать. Вместо этого «энциклопедийная статья», «текстовая статья», «внешняя страница».

---

## Статус реализации — поддерживай актуальным

Текущее состояние отражается в **двух местах** (плюс commit-message). Оба обновляются **в том же коммите**, что и сама реализация — не отдельным.

### 1. `lumen-plan.md`

В шапке — блок **«Статус реализации»**, и в §16 — маркеры рядом с задачами фазы. Условные обозначения: ✅ готово · 🟡 в работе / частично · ⬜ запланировано.

После реализации:
- меняй ⬜ → ✅ (или 🟡 → ✅);
- если работа разбита — ставь 🟡 с пометкой, что готово, что нет;
- при появлении новых крупных тем (новая trait-точка, новый крейт) — добавляй строку в соответствующий подсписок.

### 2. `CLAUDE.md` (этот файл)

При значимых вехах обновляй:

- **«Состояние подсистем (детально)»** — расширь раздел соответствующего крейта (что добавлено в «Готово» / убрано из «Отложено» / число тестов).
- **«Roadmap — что предстоит»** — если пункт реализован, удали из roadmap (он стал готовым).
- **«История последних merge-ов»** — добавь новую запись с однострочным описанием задачи.
- **«Decisions log»** — если приняли архитектурное решение (например, новый exception в политику зависимостей, выбор API подхода).
- **«Известные нюансы и ловушки»** — если ловушка устранена (например, composite-глифы перестали пропускаться) или добавлена новая.

Что **не** требует обновления CLAUDE.md:
- Тривиальные правки (опечатки, форматирование, мелкие refactor-ы без изменения API).
- Тесты, не меняющие capability крейта.
- Документация в коде / комментарии.

Для всех таких — достаточно обновить план.

Ниже — детальный срез состояния каждой подсистемы (на момент последнего обновления CLAUDE.md). Чтобы знать точное «сейчас» — смотри `git log --oneline | head -10` и упомянутый блок статуса в плане.

---

## Состояние подсистем (детально)

### `lumen-core` ✅ (фундамент стабилен)

- Типы: `Error`, `Result<T>`, `Url`, `Event` (TabCreated/Closed, Navigation, PageLoaded, RequestStarted/Completed/Blocked), `TabId`, `Capability`, `CapabilityToken`, `Module` trait.
- Геометрия: `Rect`, `Point`, `Size`.
- `lumen-core::ext` — определённые trait-точки расширения: `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `EncodingDetector`.
- В комментариях задокументированы будущие trait-точки: `WindowingBackend`, `RenderBackend`, `TlsBackend`, `JsRuntime`, `FontProvider`, `HyphenationEngine`, `DnsResolver`, `Hasher`. Тело trait-а добавим при первой реализации.
- 3 теста (url parsing).

### `lumen-dom` ✅ (полный API на текущий scope)

- Arena-based: `Vec<Node>` + `NodeId(u32)`. Нет `Rc/RefCell`, нет циклов.
- Типы: `Document`, `Node` (parent + children + data), `NodeData` (Document / Doctype / Element / Text / Comment), `QualName`, `Namespace` (HTML/SVG/MathML/Xml/XmlNs/XLink), `Attribute`.
- API: `create_element / create_text / create_comment / create_doctype`, `append_child`, `detach`, `get / get_mut`, `root`, `len`.
- `Display` impl печатает дерево с отступами — для отладки.
- 7 тестов, включая cyrillic-инварианты.

### `lumen-html-parser` 🟡 (минимум)

- **Готово:** iterator-based FSM (Tokenizer); состояния Data, TagOpen, TagName, EndTag, BeforeAttributeName, AttributeName, AfterAttributeName, BeforeAttributeValue, AttributeValue (quoted/unquoted), SelfClosingStartTag, MarkupDeclarationOpen, Comment, CommentEnd. Character references: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, numeric `&#NNN;` / `&#xHHHH;`. Lenient tree builder с void-элементами и self-closing.
- **Отложено:** DOCTYPE-разбор (пропускаем содержимое), CDATA, raw-text mode для `<script>` / `<style>` (сейчас они парсятся «как текст» — это работает потому что внутри них нет угловых скобок, но не по spec), полный набор named character references (~2000 в HTML5 spec), insertion modes (in_table, in_select, in_caption, и т.д.).
- 31 тест.

### `lumen-css-parser` 🟡 (минимум)

- **Готово:** `selector_list { decl_list }` парсинг, селекторы Type / Class / Id / Universal, декларации как пары строк (property, value), lenient recovery (битая декларация не валит правило), комментарии `/* */`, пропуск `@`-правил (`@import`, `@media`) включая вложенные блоки.
- **Отложено:** pseudo-classes / pseudo-elements, combinators (`>`, ` `, `+`, `~`), attribute selectors `[name=val]`, типизированные значения (color/length/calc/gradient), специфичность, `!important` как отдельное поле декларации.
- 20 тестов, включая cyrillic class `.привет`.

### `lumen-layout` 🟡 (block + word-wrap)

- **Готово:** `LayoutBox` дерево, style cascade (last-rule-wins без specificity), селекторы type/class/id/universal. Наследуемые свойства: `color`, `font-size`, `line-height`. Свойства с парсингом: `display` (block/inline/none), `color` (named 10 цветов + `#RRGGBB` + `#RGB`), `background-color`, `font-size`, `line-height`, `margin` (+ 4 стороны), `padding` (+ 4 стороны). Whitespace-only текстовые узлы и комментарии пропускаются. **Line wrapping:** `TextMeasurer` trait + `layout_measured()` разбивают текст по словам на строки с реальными шрифтовыми метриками. `BoxKind::Text(Vec<String>)` хранит строки после переноса.
- **Отложено:** true inline-flow (inline-элементы вроде `<a>` пока трактуются как block — появятся в одной строке с текстом позже), flex, grid, float, абсолютное позиционирование, units кроме px, функции color (rgb/hsl/rgba), `box-sizing`, borders.
- 23 теста, включая кириллику, wrapping edge-cases, nested inheritance.

### `lumen-paint` 🟡 (fill rects + textured text + FontMeasurer)

- **Готово:** `DisplayCommand` enum (FillRect, DrawText), `build_display_list` обход LayoutBox с painter's order — для `BoxKind::Text(lines)` эмитирует по одному `DrawText` на строку с правильным y-смещением. wgpu Renderer с двумя pipeline-ами: fill (vertex pos + color) и text (vertex pos + uv + color). Два WGSL-шейдера, общий uniform (viewport), bind group для атласа (R8 texture + linear sampler). `GlyphAtlas` 512×512 со shelf packer-ом. `FontMeasurer<'a>` — реализация `TextMeasurer` на основе TTF hmtx/cmap для shell. Per-glyph metadata кеш (atlas position + left/top offset + advance_native). Atlas заливается на GPU только при dirty.
- **Отложено:** snapshot-тесты на display list / pixel buffer, multi-size atlas (сейчас один размер растеризации — 24px, display масштабируется linear sampler-ом), GPU-pipeline для скруглений/градиентов/теней, layer-tree compositor.
- 20 тестов (display_list + atlas + wrapping).

### `lumen-font` 🟡 (TTF read + raster)

- **Готово:** парсеры таблиц head, maxp, cmap (format 4), hhea, hmtx, loca, glyf. Glyf обрабатывает simple-глифы (контуры с on-curve / off-curve, квадратичные Безье) и **composite-глифы** (ссылки на другие глифы с 2×2 transform + offset). `Font::glyph_resolved` рекурсивно разворачивает composite в Simple с max-depth 8. Scanline-растеризатор с 4×4 supersampling, even-odd fill, 1px padding. `Bitmap` с метриками left/top для placement. Bundled Inter v4.1 Regular.
- **Отложено:** cmap format 12 (Unicode SMP/SIP — эмодзи), hinting (TT-инструкции), GSUB/GPOS (advanced shaping для лигатур, kerning, Arabic/Indic), CFF outlines (для PostScript-OpenType `.otf` без TT-таблиц), variable fonts (fvar/gvar/avar/HVAR), color glyphs (COLR/CPAL, sbix), bitmap strikes (EBDT/EBLC), composite с ARGS_ARE_XY_VALUES=0 (point alignment, рудимент — сейчас offset = (0,0)).
- 60 unit-тестов + 9 интеграционных на bundled Inter. Включает тест на composite кириллической `А`.

### `lumen-shell` 🟡 (окно + рендер)

- **Готово:** winit 0.30 с `ApplicationHandler` API. Два режима: `lumen` (пустое окно 1024×720) и `lumen <path.html>` (парсит HTML, извлекает `<style>` через walk DOM, парсит CSS, layout, paint, рисует фоны + текст в окне через `Renderer::render`). Inter-Regular.ttf bundled через `include_bytes!` (~411 КБ к binary). Обработчики Resized + RedrawRequested.
- **Отложено:** вкладки, омнибокс, навигация, истории сессий, бэка для CSS-загрузки внешних файлов через `<link>`, scroll, обработка input-событий.
- Авто-тестов нет (визуальная проверка через `cargo run`). Snapshot-тесты для рендера — TODO.

### Инфраструктура

- Cargo workspace, edition 2024, resolver 3, MSRV 1.95.
- 8 крейтов в `crates/`: shell, core, engine/{html-parser, css-parser, dom, layout, paint, font}.
- Bundled assets: `assets/fonts/Inter-Regular.ttf` (+ OFL.txt лицензия).
- Тестовая страница: `samples/page.html` со встроенным `<style>`.
- 4 разрешённых внешних зависимости: `winit = "0.30"`, `wgpu = "26"`, `rustls` (зарезервирована, не подключена), JS engine (зарезервирована).
- Внутренние deps: workspace.dependencies на 8 крейтов.
- `.gitattributes` форсит LF для всех текстовых файлов; binary-метка для `.ttf / .png / .woff2`.
- `.gitignore` игнорирует `/target`, `/*.zip`, `/*.tar*`, `.idea/`, `.vscode/`, swap-файлы.

### Численно

- **Всего тестов в workspace:** 168 (на момент последнего обновления).
- **`cargo clippy --workspace --all-targets -- -D warnings`** проходит без warnings.
- **Внешних зависимостей runtime:** 2 активных (winit, wgpu) + 2 зарезервированных.
- **Транзитивно через wgpu/winit:** ~200 crates.

---

## Roadmap — что предстоит реализовать

Приоритизированный список. Порядок может меняться, ориентируйся на план §16 «Фазы».

### Ближайшее (закрывает Phase 0)

1. **Encoding detection** (§10.1) — cp1251 / KOI8-R / CP866. Сейчас shell принимает только UTF-8 файл (panic при не-UTF-8).
2. **HTTP/1.1 + TLS client через rustls** — загрузка внешних страниц. Активация exception #3. Новый крейт `lumen-network`.
3. **Snapshot-тесты для paint** — гарантия от регрессии визуального вывода. Сериализация display list + diff. Можно сделать сейчас, не дожидаясь больших фич.
4. **Inline elements в layout** (`<a>`, `<span>`, `<em>`, `<strong>`) — сейчас трактуются как block. Нужны line boxes для размещения inline-элементов в одной строке с текстом.

### Средний приоритет (Phase 1+)

6. **CSS combinators / pseudo-classes** — `descendant`, `>`, `:hover`, `:first-child`, `[attr=val]`.
7. **Cmap format 12** — Unicode SMP/SIP (эмодзи, math symbols, исторические письменности).
8. **`lumen-storage` крейт** — KV-store для cookies, history, profile data. Свой минимальный B-tree или in-memory + JSON snapshot для Phase 0-1.
9. **Tab session export / import** (§12.7) — JSON serialize. Простое, экономит много боли.
10. **Картинки на страницах** — `<img>` рендеринг. Нужны PNG/JPEG декодеры (свои, по §5).

### Большое (Phase 2+)

11. **QuickJS интеграция через `rquickjs`** — exception #4. Базовое исполнение JS. `lumen-core::ext::JsRuntime` trait.
12. **`lumen-knowledge` крейт** (§12.1-12.4) — FTS-индекс над историей и заметками, omnibox-префиксы `@history` / `@notes` / `@tabs` / `@read-later`.
13. **CSS Grid + полный Flexbox** в layout.
14. **HTTP/2** поверх свои rustls-based транспорта.
15. **DoH / DoT resolver** в network-слое.
16. **Site isolation** (process per origin) — `lumen-renderer` процесс отдельно от shell.
17. **Profiles + шифрование** (§9.3) — XChaCha20-Poly1305, Argon2id KDF.
18. **Focus mode** (§12.6) — UI feature, не требует новых крейтов.
19. **Кастомизация UI** (§12.10) — drag&drop панелей, темы.

### Очень большое (Phase 3+)

20. **V8 переход** с `rusty_v8`. Реализуем `JsRuntime` для V8, не ломая QuickJS path.
21. **`lumen-ai` крейт** (§12.5) — embedding + RAG + опциональный LLM-backend через Ollama HTTP или встроенный llama.cpp.
22. **Семантические закладки** (§12.8) — требует §12.5.
23. **Service Workers**, Canvas 2D, IndexedDB.
24. **WebFonts через WOFF2** в `lumen-font`.

### Не приоритет, держим в голове

- Variable fonts (fvar/gvar/avar/HVAR) в `lumen-font`.
- GSUB/GPOS shaping (для арабского, индийского, тайского). Текущая позиция — добавим как exception #5 (rustybuzz) или сами для базовых случаев. См. анализ qwen.ai и обсуждение в плане.
- ADR-инфраструктура (`docs/decisions/`) — формализация decisions log.
- StorageBackend trait: добавить origin partitioning параметр (`(origin, top_level_site)`) ДО первой реализации, чтобы не переделывать.
- Snapshot-тесты для layout (insta crate или собственный diff).
- Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — для битых старых шрифтов.

---

## Decisions log

Это короткие записи решений и их обоснования. Полные обсуждения — в commit-сообщениях и плане.

### Зафиксированные

- **Свой rendering engine** (не обёртка над Chromium/WebKit/Servo). Главное идеологическое решение, без него проект теряет смысл. Закреплено в §1 принципах.
- **4 разрешённых external dependencies:** winit, wgpu, rustls, JS engine. Подробное обоснование каждого — в §5 плана. Любой пятый — только через коммит с пунктом «Why this dependency» в теле + обновление CLAUDE.md и плана.
- **Composite glyphs разворачиваются** через `Font::glyph_resolved` с max recursion depth = 8. Renderer вызывает `glyph_resolved`, не `glyph` напрямую.
- **Atlas размер растеризации фиксирован на 24px**, display масштабируется через quad scale + linear sampler. Multi-size atlas — позже, когда увидим, что качество критично.
- **Inter Regular bundled** через `include_bytes!` в lumen-shell (~411 КБ к binary). Не runtime-loading, чтоб не было path-проблем.
- **WASM плагины через wasmtime** — выбор сделан в §11.2 vs native dylib / WebExtensions. Решает проблему prompt-injection-class уязвимостей через capability tokens.
- **Capability-модель** для плагинов вместо статических permissions (§11.4).
- **Memory-safe Rust, `unsafe` только на FFI-границах** с обязательным `// SAFETY:` комментарием. Текущие `unsafe` блоки: `as_bytes` в renderer.rs, FFI к wgpu внутри wgpu crate (не наш код).
- **Feature-branch + `--no-ff` merge** workflow. Видимая структура «коммит-серия = задача» в git log --graph.
- **`opt-level = 1` в dev профиле** — компромисс: debug-сборка чуть медленнее, но layout/paint работают в 5-10 раз быстрее. Стандарт в графических Rust-проектах.
- **Cargo features пока не используются**, но запланированы для `ai`, `webgl`, `tor`, `ru-hyphenation` опциональных модулей.
- **`TextMeasurer` trait в `lumen-layout`, `FontMeasurer<'a>` в `lumen-paint`**: layout не зависит от font напрямую; shell создаёт `FontMeasurer<'static>` из `INTER_FONT` и передаёт через `layout_measured()`. `BoxKind::Text(Vec<String>)` хранит строки post-wrap. `layout()` без измеритея — backward compat, без переноса.

### Открытые вопросы (решим, когда упрёмся)

- **AI backend:** Ollama HTTP API (нулевая интеграция, требует, чтобы у пользователя был Ollama) ИЛИ встроенный llama.cpp через FFI (5-е exception, нет внешних зависимостей у пользователя). Откладываем до Phase 3.
- **Shaping для сложных скриптов** (Arabic/Indic/Thai): свой shaper за месяцы или rustybuzz как 5-е exception. Откладываем до Phase 2-3.
- **iOS:** Apple-policy требует WebKit на iPhone/iPad. Это противоречит принципу собственного движка. Возможный путь — тонкий shell поверх WKWebView только для iOS, остальные ОС — наш движок. Откладываем до Phase 4.
- **Storage partitioning в StorageBackend trait:** текущая сигнатура `get/put(key)` не принимает origin. Нужно обновить ДО первой реализации (`get/put(origin, top_level_site, key)`), иначе ретрофит будет болезненным.

### Намеренно отвергнутые альтернативы

- **WebView2 / wry / CEF обёртка** — это другой проект, не Lumen. Отказались.
- **html5ever / cssparser / taffy / image / encoding_rs / ttf-parser / rustybuzz / tokio / rayon / redb / egui** — все эти crates рассмотрены и **не взяты**: для них пишется свой код по принципу «default — своё». См. зачёркнутый список в §5.

---

## История последних merge-ов

Чтобы быстро понять, что было сделано в недавних сессиях. Последние сверху.

```
*   (soon)   inline-flow            — TextMeasurer + FontMeasurer + line wrapping: текст переносится по словам
*   e2864ac  hide-head-elements     — <title>, <style>, <script> и др. метаданные больше не рендерятся
*   061c2c7  claude-md-self-update-rule — правило обновлять CLAUDE.md вместе с планом
*   586f8ba  claude-md-state        — детальное состояние подсистем + roadmap + decisions log
*   7811eee  composite-glyphs       — TTF composite + Font::glyph_resolved → кириллица 'А' рисуется
*   a9a9278  claude-md              — этот файл, первая версия
*   be0bdee  knowledge-layer-plan   — §12 «Уникальные фичи Lumen» в плане (11 фич)
*   60c617d  text-rendering         — TTF parser + scanline raster + glyph atlas + text в окне
*   0bd59b1                          wgpu растеризатор окна (на main до ветки text-rendering)
*   5c81bf7                          Display list в lumen-paint
*   ceddb9d                          Block-flow layout + style cascade
*   a38d940                          Минимальный CSS-парсер
*   58782ce                          shell + html-parser связка (dump mode)
*   60b05bf                          Чистка нежелательного слова из всех артефактов
*   c8bbcbb                          Минимальный HTML-парсер
*   29d74c3                          Политика «default — своё» зафиксирована
*   74b73b4                          Блок «Статус реализации» добавлен в план
*   173cdbb                          Замена lumen-common на lumen-core
*   d014809                          §11 Модульность и плагины в план
*   45e419e                          Окно через winit (первая версия)
*   c7e4ce9                          Тесты с кириллицей в DOM
*   6f93f57                          Russian language first-class (§10)
*   36fd7e0                          Arena-based DOM
*   83ddaf0                          .gitattributes
*   fbb4875                          Initial workspace setup
```

`git log --oneline --graph -20` всегда даст самую свежую картинку. Этот список обновляй, когда мерджишь крупное.

---

## Уникальные фичи Lumen (§12 плана)

Это то, чем мы будем отличаться от Chromium-форков. Не приоритет Phase 0, но архитектура должна оставлять место.

| Фича | Фаза | Заметка |
|---|---|---|
| Tab session export / import | 1 | Простое, экономит много боли |
| Полнотекстовый поиск по истории | 2 | Cyrillic-aware токенизация. То, что Chrome не делает по бизнес-причинам |
| Аннотации и заметки | 2 | Локально, ищется тем же FTS |
| Read-later / офлайн | 2 | Замена Pocket / Instapaper |
| Поиск по содержимому открытых вкладок | 2 | `@tabs` префикс в omnibox |
| Focus mode | 2 | Снижение когнитивной нагрузки |
| Кастомизация UI | 2-3 | drag&drop панелей, темы |
| Локальный AI layer | 3+ | Опциональный, через Ollama HTTP или встроенный llama.cpp (потенциальный 5-й exception) |
| Семантические закладки | 3 | Зависит от AI |
| Граф знаний | 3+ | Визуализация коллекции |
| Кросс-устройственный sync (E2E) | 4+ | Self-host или P2P, без облачного сервиса |

---

## Известные нюансы и ловушки

- **Cargo.lock коммитится** (workspace включает binary).
- **Line endings:** `.gitattributes` форсит LF в репо. Если Git ругается на CRLF→LF — это норма, не паникуй.
- **Архивы в корне репо игнорируются** (`/*.zip`, `/*.tar*`). Если пользователь скачал что-то — оно не попадёт в коммит случайно.
- **Composite glyphs теперь поддерживаются** через `Font::glyph_resolved` (с max recursion depth 8). Кириллические заглавные `А / В / Е / К / М / Н / О / Р / С / Т / Х` (которые в Inter composite через Latin-эквиваленты) и их строчные — рендерятся. Renderer вызывает `glyph_resolved`, не `glyph` напрямую.
- **Тесты в `lumen-paint::display_list` и `lumen-paint::atlas`** — это unit-тесты. Renderer (`renderer.rs`) визуальный, без автотестов; проверяй через `cargo run`. Snapshot-тесты для display list — TODO.
- **`font_size` влияет на масштаб quad-а, но не на разрешение растеризации.** Глифы всегда рисуются на 24 px и масштабируются. Это компромисс Phase 0 — multi-size atlas позже.

---

## Память (`~/.claude/projects/.../memory/`)

Это **локально на машине**, не в репо. В ней хранятся пользовательские предпочтения, ускоряющие работу в новых сессиях. Туда я (Claude Code) пишу:

- `user_*.md` — про пользователя (язык общения, опыт).
- `feedback_*.md` — правила и пожелания пользователя.
- `project_*.md` — факты про проект.
- `reference_*.md` — ссылки на внешние ресурсы.

На другой машине эта память не появится. Поэтому всё важное должно дублироваться в коде / `CLAUDE.md` / `lumen-plan.md` (которые в репо).

---

## Когда что-то непонятно

- **Архитектура / scope** — `lumen-plan.md`.
- **Как запустить / собрать** — `README.md`.
- **Что есть сейчас в коде** — `git log --oneline` или статус-блок в плане.
- **Почему такое решение принято** — комментарии в коде или commit-сообщения.

Если вопрос не закрывается этими источниками — спроси пользователя, не предполагай.
