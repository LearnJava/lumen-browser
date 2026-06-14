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

### 6.11 Automation API (lumen-driver)

Полное обоснование архитектурных решений — [ADR-006](docs/decisions/ADR-006-automation-api.md).

Automation — **first-class поверхность движка**, не пристройка debug-протокола. Один внутренний trait `BrowserSession` в крейте `lumen-driver`, три транспорта поверх него (in-process Rust / MCP / WebDriver BiDi). Это даёт три эффекта одновременно:

1. **Собственное тестирование изнутри** — `graphic_tests/` мигрируют с пиксельных diff-ов против Edge на структурные числовые ассерты (`box.border_box.width == 200.0`) плюс in-process snapshot tests без ffmpeg / gdigrab / Windows-only crop offsets. Запускается за миллисекунды, кросс-платформенно, на любом CI.
2. **Эмбеддинг как библиотека** — `cargo add lumen-driver` даёт чужому Rust-приложению полноценный браузерный движок с API «открой → проверь layout → кликни», без отдельного процесса.
3. **Внешние клиенты автоматизации** — AI-агенты через MCP, Playwright/Selenium/Cypress через BiDi, без специальных обёрток.

#### `BrowserSession` trait — поверхность

| Группа | Методы |
|---|---|
| **Lifecycle** | `new(opts)`, `navigate(url)`, `reload()`, `close()` |
| **Computer-use primitives** (vision-агенты) | `screenshot(opts)`, `input_event(ev)` — нативная инжекция, **не** synthetic JS, `viewport(w, h)` |
| **Semantic surface** | `a11y_tree()`, `query(Query::Role/Name/Text/Css)`, `layout_box(handle)`, `computed_style(handle)`, `eval_js(code)` |
| **Wait conditions** (auto-wait внутри движка) | `wait_for(Cond::Visible/Stable/NetworkIdle/JsIdle)` |
| **Observability** | `network_log()`, `console()`, `display_list()` |
| **Determinism** | `set_clock(ClockMode)`, `set_rng_seed(u64)`, `freeze_fingerprint(profile)` |
| **Storage / context** | `cookies()`, `local_storage()`, изоляция per-session по умолчанию |

#### Транспорты (поверх trait-а)

| Крейт | Протокол | Кто потребляет |
|---|---|---|
| `lumen-driver` | Rust API in-process | Свои `graphic_tests/`, embed-пользователи |
| `lumen-mcp-server` | Model Context Protocol (JSON-RPC over stdio/socket) | Claude Computer Use, OpenAI Operator/CUA, Browser Use, локальные LLM-агенты |
| `lumen-bidi-server` | WebDriver BiDi (W3C, WebSocket) | Playwright, Selenium 5, Cypress |
| `lumen-cdp-shim` (опционально, **по запросу**) | Chrome DevTools Protocol subset | Legacy Puppeteer — только если будет реальный спрос |

#### Что Lumen даёт сверх BiDi-спеки

W3C BiDi на май 2026 — Working Draft с известными пробелами (см. blocker-issues Playwright #32577 и Cypress #30447). Lumen реализует их **с первого дня**, потому что контролирует свой стек:

- Полный доступ к response body и `resourceType` в network events
- Locale / timezone / offline emulation
- Per-context user-agent и extra HTTP headers
- Viewport до загрузки popup / new tab
- Preload-скрипты per browsing context
- Полный download lifecycle (begin → progress → complete + body)
- Cookie change events, per-origin storage clear
- Дешёвая network interception (не «prohibitively expensive» как в текущей BiDi)

#### Что Lumen **не** делает

- **WebDriver Classic** — мёртвый HTTP request-response протокол, в проект не входит.
- **CDP как primary** — Lightpanda пошёл этим путём и теперь несёт груз нестабильного API. У нас CDP может появиться **только как thin shim** в Phase 3+ при реальном спросе.
- **DOM-селекторы как primary локаторы** — поддерживаются как fallback, но рекомендуются role+name запросы по a11y-tree. Это снимает 70% maintenance-боли тестов (industry data 2026).
- **Synthetic JS events (dispatchEvent) для input** — анти-боты их распознают, реальные сайты с `event.isTrusted` ведут себя иначе. Только нативная инжекция через event loop шелла.
- **Wait-логика в клиенте** — auto-wait живёт в движке (на тик layout/network/JS-idle), не в SDK retry-loop.

**Crates:** `lumen-driver` (P3 owner), `lumen-mcp-server` (P3), `lumen-bidi-server` (P3). A11y tree строится в `lumen-a11y` (P1 owner) — automation использует её как готовую структуру, не дублирует.

---

