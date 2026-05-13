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

На момент написания: 311 тестов, 11 крейтов (`shell`, `core`, `network`, `storage`, `dom`, `html-parser`, `css-parser`, `layout`, `paint`, `font`, `encoding`). При прохождении следующих фаз появятся `lumen-knowledge`, `lumen-ai` и др.

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
    ├── font/             — TrueType parser + scanline rasterizer
    └── encoding/         — детектор и однобайтовые декодеры (cp1251/koi8-r/cp866)
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

**Главное правило: вся работа выполняется в feature-ветках. Прямые коммиты на `main` запрещены.**

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

- **Любые коммиты прямо на `main`** — включая правки документации, «мелкие фиксы» и координационные пометки.
- Force-push на `main`.
- Rewrite опубликованной истории.
- `git config` изменения (никогда).
- Skip hooks (`--no-verify`).
- `git push` без явной просьбы пользователя.

### Координация параллельных сессий

Над репозиторием могут одновременно работать несколько сессий Claude Code (в разных worktree-ах). Чтобы они не брали из roadmap одну и ту же задачу:

1. **Перед стартом** — прочитать `git branch` и блок **«🔄 В работе сейчас»** в шапке `lumen-plan.md`. Если ветка с нужным именем уже существует или задача в списке — выбрать другую.
2. **Зарезервировать задачу**: создать feature-ветку (`git checkout -b <имя>`) и в **первом же коммите на этой ветке** добавить строку в «В работе сейчас»: `- 🔄 <имя задачи> — <имя ветки> — <YYYY-MM-DD>`. Резервация видна другим сессиям через `git branch`.
3. **При merge в `main`** — в merge-коммите убрать строку из «В работе сейчас» и обновить статусы в плане/CLAUDE.md как обычно.
4. **Если работа отменена** — удалить ветку; строку из «В работе сейчас» убрать в отдельной ветке `cleanup-<имя>`, слить в main.

**Почему ветка — достаточная резервация:** `git branch` виден всем сессиям в том же репозитории без fetch. Имя ветки = имя задачи = резервация. Предыдущий протокол с мини-коммитами на `main` нарушал правило «no commits to main» и создавал лишний шум в истории.

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
- `lumen-core::ext` — trait-точки расширения: `NetworkTransport`, `StorageBackend` (с origin-партиционированием + `list_keys`), `SearchProvider`, `FilterListSource`, `EncodingDetector`.
- В комментариях задокументированы будущие trait-точки: `WindowingBackend`, `RenderBackend`, `TlsBackend`, `JsRuntime`, `FontProvider`, `HyphenationEngine`, `DnsResolver`, `Hasher`. Тело trait-а добавим при первой реализации.
- 3 теста (url parsing).

### `lumen-dom` ✅ (полный API на текущий scope)

- Arena-based: `Vec<Node>` + `NodeId(u32)`. Нет `Rc/RefCell`, нет циклов.
- Типы: `Document`, `Node` (parent + children + data), `NodeData` (Document / Doctype / Element / Text / Comment), `QualName`, `Namespace` (HTML/SVG/MathML/Xml/XmlNs/XLink), `Attribute`.
- API: `create_element / create_text / create_comment / create_doctype`, `append_child`, `detach`, `get / get_mut`, `root`, `len`.
- `Display` impl печатает дерево с отступами — для отладки.
- 7 тестов, включая cyrillic-инварианты.

### `lumen-html-parser` 🟡 (минимум)

- **Готово:** iterator-based FSM (Tokenizer); состояния Data, TagOpen, TagName, EndTag, BeforeAttributeName, AttributeName, AfterAttributeName, BeforeAttributeValue, AttributeValue (quoted/unquoted), SelfClosingStartTag, MarkupDeclarationOpen, Comment, CommentEnd, **RAWTEXT** (для `<script>` и `<style>`), **DOCTYPE**. Character references: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, numeric `&#NNN;` / `&#xHHHH;`. Lenient tree builder с void-элементами и self-closing.
- **RAWTEXT детали:** после `<script>` / `<style>` (не self-closing) тело читается литерально до `</tag` + терминатор (whitespace / `/` / `>` / EOF), case-insensitive. `<` без `/` или `</scripto>` остаются текстом. Character references (`&amp;`) внутри **не декодируются** — это spec-compliant поведение HTML5. После `</script>` токенизатор возвращается в data state. `is_raw_text_element(name)` определяет список (сейчас только script/style).
- **DOCTYPE детали:** `Token::Doctype { name, public_id, system_id }` (HTML5 §13.2.5.53–72). После `<!DOCTYPE` keyword (case-insensitive `doctype`/`DOCTYPE`) парсятся: name (lower-case, до whitespace или `>`), опционально `PUBLIC "id" "id"` или `SYSTEM "id"` с поддержкой одинарных и двойных кавычек. Lenient: `<!DOCTYPE>` без имени даёт пустой name, неполные DOCTYPE-ы не валятся. Tree builder создаёт `NodeData::Doctype` узел (раньше токен пропускался). Прочие markup declarations типа `<![CDATA[...]]>` или `<!ENTITY ...>` по-прежнему молча skip-аются до `>`.
- **Отложено:** CDATA, RCDATA для `<title>` / `<textarea>` (как RAWTEXT, но с декодированием entities), полный набор named character references (~2000 в HTML5 spec), insertion modes (in_table, in_select, in_caption, и т.д.).
- 57 тестов (Tokenizer + tree_builder).

### `lumen-css-parser` 🟡 (полный набор CSS3-селекторов)

- **Готово:** `selector_list { decl_list }` парсинг. Selectors Level 3: simple selectors (`Type`, `Class`, `Id`, `Universal`, `Attribute`, `PseudoClass`, `PseudoElement`), compound (`p.foo#bar:first-child`), complex с combinator-ами (descendant ` `, child `>`, next-sibling `+`, later-sibling `~`). Attribute-операторы: `=`, `~=`, `|=` (для `lang`), `^=`, `$=`, `*=`. **Case-insensitive флаг `[attr=val i]`** (CSS Selectors L4 §6.3.6) — после value распознаётся `i` / `I` (ASCII case-insensitive) или `s` / `S` (явно case-sensitive, default); хранится в `AttrSelector.case_insensitive: bool`; применим ко всем шести операторам. **Structural pseudo:** `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`. **Функциональные pseudo:** `:nth-child(an+b)`, `:nth-last-child(an+b)`, `:nth-of-type(an+b)`, `:nth-last-of-type(an+b)` с ключевыми словами `odd` / `even` (хранятся как `NthSpec { a, b }`, `.matches(index)` решает уравнение `i = a*n + b` при `n ≥ 0`). `:not(compound)` — отрицание; запрещены combinator-ы внутри и nested `:not(:not(...))`, такие формы дают `Unsupported`. **CSS4 `:is(selector-list)` и `:where(selector-list)`** — матчат, если матчит хоть один из селекторов; внутри разрешены любые complex-селекторы (combinator-ы, attribute, structural pseudo). Пустой список `:is()` / `:where()` → `Unsupported(name)`. Interactive (`:hover`, `:focus`, …) сохраняются как `Unsupported(name)` и при матчинге всегда возвращают false. Pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат. **Specificity** по CSS3 §16 / CSS4 §17: `:not` сам не считается, contributes specificity внутреннего compound. `:is(...)` сам не считается, contributes максимум specificity по списку. `:where(...)` всегда 0. Прочие pseudo-classes считаются как class. Декларации — как пары строк. Lenient recovery, комментарии `/* */`, пропуск `@`-правил. Парсер `parse_complex_selector` прерывается также на `)`, чтобы корректно работать внутри функциональных pseudo.
- **`!important` флаг (CSS Cascade L4 §8.1):** парсер `extract_important` отделяет `!important` от value (с опциональным whitespace между `!` и словом, ASCII case-insensitive). Не трогает `!important` внутри строковых литералов: `content: "!important"` остаётся value=`"!important"`, important=false. Хранится в `Declaration.important: bool`.
- **Отложено:** `:has(...)`, `:not(complex)` со списком селекторов или combinator-ами, namespace prefix в селекторах, типизированные значения деклараций (length / color / calc / `--var`).
- 87 тестов.

### `lumen-layout` 🟡 (block + inline-flow + word-wrap + cascade)

- **Готово:** `LayoutBox` дерево, **specificity-based style cascade с !important**: для каждого правила берётся максимальная specificity среди его complex-селекторов, все matched declarations сортируются по `(important, specificity, rule_order, decl_index)` и применяются по возрастанию. `important` идёт первым в ключе — `true > false` ставит !important-декларации в конец, и они выигрывают у normal даже при меньшей specificity (CSS Cascade L4 §8.1). При равенстве — позже объявленная. Matching complex selector-а — справа налево, жадно (без back-tracking; патологические `a b c` с вложенными `a` могут промахнуться — известное упрощение). Combinator-ы: descendant / child / next-sibling / later-sibling. **Pseudo-classes:** все CSS3 structural и functional — `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`, `:nth-child(an+b)`, `:nth-last-child`, `:nth-of-type`, `:nth-last-of-type`, `:not(compound)`. Helpers `element_index` (1-based, среди element-sibling-ов) и `element_index_of_type` (среди sibling-ов с тем же тегом) с `from_end` опцией. Attribute selectors: все операторы, с поддержкой ASCII case-insensitive флага (`[attr=val i]`) — сравнение через `eq_ignore_ascii_case` на байтах, чтобы не упираться в char-boundary в UTF-8 строках; не-ASCII (cyrillic) сравнивается побайтово. Наследуемые свойства: `color`, `font-size`, `line-height`, `font-style`, `font-weight`, `font-family`, `text-transform`, `text-align`, `text-decoration-line`. **`FontStyle` (Normal / Italic / Oblique)** через `font-style` property + UA stylesheet для семантических тегов (`<em>`, `<i>`, `<cite>`, `<dfn>`, `<address>`, `<var>` → italic); **`FontWeight` (1..1000, normal=400 / bold=700)** через `font-weight` property с поддержкой keyword-ов (`normal`, `bold`, относительных `lighter`/`bolder` по таблице CSS Fonts L4 §2.4.3) и числовой шкалы; UA stylesheet для `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>` → bold. `text_rendering_eq` сравнивает font_style и font_weight, чтобы italic/bold-фрагменты не сливались с обычными. Реальная отрисовка italic / bold вариантов в paint пока не реализована (нужны Italic / Bold fontfiles или affine skew transform / faux-bold), но layout уже различает. **`font-family`** — `Vec<String>` приоритизированного списка, парсер поддерживает quoted (`"Times New Roman"` / `'Open Sans'`) и unquoted multiword имена со схлопыванием whitespace; generic-family (`serif`, `sans-serif`, `monospace`, …) хранятся как обычные строки. Inherited. Phase 0 рендерер всегда Inter — задел под будущий font matcher. **`TextTransform` (None / Uppercase / Lowercase / Capitalize)** — `text-transform: …` применяется к `InlineSegment.text` при сборке (до wrapping и measurer), используя `char::to_uppercase`/`to_lowercase` стандартной библиотеки — корректно работает для кириллицы. `capitalize` — упрощённо первая буква каждого whitespace-разделённого токена. **`text-indent` (resolved px)** — отступ перед первой строкой inline-content (CSS Text L3 §7.1), inherited; `wrap_inline_run` стартует `current_x = text_indent` для первой строки, последующие — с 0. Поддерживает px/em/rem/vh/vw; `%` пока игнорируется (нужен containing-block-width). **`white-space` (Normal / Nowrap)** — `Nowrap` отключает word-wrap (передаём `f32::INFINITY` как `wrap_width`); `Normal` — обычный greedy wrap; pre/pre-wrap/pre-line отложены (нужен preserved whitespace в input). Inherited. **`opacity` (0..1)** — CSS Color L3 §3.2, не наследуется; парсер принимает число и проценты, clamp вне диапазона; в layout только хранится — реальный alpha-blending paint-уровня — отдельная задача. **`outline` (`outline-width` / `-style` / `-color` + shorthand + `outline-offset`)** — CSS UI L4 §3, не наследуется; **не занимает места в коробке** (в отличие от border) — `rect.width`/`height` неизменны, отрисовка позже как «слой» поверх / снаружи; цвет `None` = currentColor; offset поддерживает отрицательные значения (рисует внутрь). **`visibility` (Visible / Hidden / Collapse)** — CSS Display L3 §4, **inherited** (отличается от display); `Hidden` оставляет коробку в layout (высота сохраняется), но не рисуется — потомок может явно вернуть себя через `visibility: visible`. **`overflow` / `overflow-x` / `overflow-y`** (Visible / Hidden / Clip / Scroll / Auto) — CSS Overflow L3, не наследуется; shorthand принимает 1 или 2 значения; реальный clipping / scrollbars в paint pipeline пока нет. **`cursor`** — CSS UI L4 §8.1, inherited; полный набор из 36 standard keyword-ов (auto/default/pointer/text/wait/move/grab/grabbing/all 8 resize-направлений/zoom-in/zoom-out/…); URL-fallback парсер игнорирует и использует последний keyword из comma-list. Использование при mouse-handling — позже. **`letter-spacing` (resolved px)** — дополнительное расстояние между парами символов (CSS Text L3 §11.2), inherited; добавляется в word-width (`(n−1)·ls`) и в word-boundary (`space_w + ls`); может быть отрицательным; `text_rendering_eq` учитывает letter_spacing, чтобы фрагменты с разным spacing не сливались. **`word-spacing` (resolved px)** — дополнительное расстояние **только** между словами (CSS Text L3 §11.3), inherited; добавляется в `gap_with_ls = space_w + ls + ws`; может быть отрицательным; ширина одиночного слова неизменна. Свойства с парсингом: `display` (block/inline/none), `color` (полный CSS3 набор named colors — 147 цветов + `rebeccapurple` из CSS4 §6.1 + `transparent`; `gray`/`grey` варианты эквивалентны; матчинг case-insensitive через бинарный поиск по сортированной таблице `NAMED_COLORS`; hex `#RGB`/`#RRGGBB`/`#RGBA`/`#RRGGBBAA` + `rgb()` / `rgba()` / `hsl()` / `hsla()` с запятыми или whitespace, slash-alpha modern syntax `rgb(r g b / a)`, проценты для каналов, hue в `deg`/`turn`/`rad`/`grad` (CSS Color L4 §9), clamp вне диапазона), `background-color`, `font-size`, `line-height`, `margin` (+ 4 стороны), `padding` (+ 4 стороны), `text-align` (left/center/right), **`text-decoration` / `text-decoration-line`** (комбинируемые keyword-ы `underline` / `overline` / `line-through` / `none`, прочие токены `solid` / `wavy` / `dashed` / `blink` и цвет — игнорируются), **`width` / `height` (px/em/rem; `auto` = не задано)**. `TextDecorationLine` хранится как struct из трёх булевых полей; наследуется на детей через каскад. Whitespace-only текстовые узлы и комментарии пропускаются. **Line wrapping:** `TextMeasurer` trait + `layout_measured()` разбивают текст по словам на строки с реальными шрифтовыми метриками. **Inline-flow:** `BoxKind::InlineRun { segments, lines }` — текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`, `<strong>`, и т.д.) группируются в один поток; каждый сегмент хранит свой стиль (цвет ссылки, decoration); слова с одинаковым rendering-стилем сливаются в один `InlineFrag` на строке; `InlineFrag.width` хранит измеренную ширину текста (для align_lines и подрисовки text-decoration в paint). `ComputedStyle::text_rendering_eq` сравнивает color/font_size/line_height/text_decoration_line. `align_lines()` сдвигает `frag.x` после wrap для center/right выравнивания.
- **Готово (тестовая инфра):** `serialize_layout_tree(&LayoutBox) → String` — детерминированный текстовый формат всего layout-дерева (kind / rect / non-default style включая text-align, w=, h=, box-sizing, decoration / segments / lines), плюс 16 golden-тестов в `tests/snapshot_tests.rs` (empty / paragraph / styles / nested / inline + link / line wrap / cyrillic / stacked / display:none / nth-child(odd) / :not(.x) / descendant / underline-on-link / border-solid / border-top / box-sizing border-box). Механизм `UPDATE_SNAPSHOTS=1` для регенерации (как в lumen-paint).
- **Готово (relative units):** `Length { Px, Em, Rem, Percent, Vh, Vw, Vmin, Vmax }` + `parse_length`. Cascade — два прохода: pre-pass для `font-size` (em/% относительно parent fs, rem от `ROOT_FONT_SIZE` = 16, vh/vw/vmin/vmax — от `viewport`), затем main-pass для остального (em/% относительно computed fs текущего элемента). `line-height: 150%` / `1.5em` корректно превращается в коэффициент 1.5; `5vh` / `2vw` тоже поддерживаются. **Viewport units** (CSS Values L3 §6.1.2): `1vh = 1% от viewport.height`, `1vw = 1% от viewport.width`, `vmin = 1% от min(w,h)`, `vmax = 1% от max(w,h)`. Viewport прокинут через `compute_style → apply_declaration → resolve_box_length → Length::resolve` — все четыре функции принимают `viewport: Size`. `%` в margin/padding требует containing-block-width и Phase 0 игнорируется (молча).
- **Готово (borders):** `BorderStyle` enum (None/Solid/Dashed/Dotted) + 12 полей `border_{top,right,bottom,left}_{width,style,color}` в `ComputedStyle`. Парсинг: `border` shorthand, `border-{side}` per-side shorthand, `border-{width,style,color}` multi-value (1–4 токена по CSS-правилу), `border-{side}-{prop}` individual. `border_*_color: Option<Color>` (None = currentColor). Контент-область корректно уменьшается на ширины border: `content_x/y` учитывают border, `content_width` убирает border_left+border_right. Высота и ширина бокса включают border-widths. `snapshot.rs` выводит `bw=(...)` и `bs=(...)` для ненулевых border.
- **Готово (box-sizing):** `BoxSizing` enum (ContentBox/BorderBox) + поле `box_sizing` в `ComputedStyle`. Парсер `box-sizing: content-box | border-box` (case-insensitive). Не наследуется (CSS Basic UI 3 §4.1) — сбрасывается на ContentBox в каждом `compute_style`. В `lay_out`: при `width`/`height`, заданных в content-box — добавляем padding+border сверху (старая модель), в border-box — `rect.width = w`, `rect.height = h`, контент-область сжимается. Анонимный `InlineRun` тоже сбрасывает box-sizing для чистоты snapshot-а. `snapshot.rs` печатает `box-sizing=border-box` только когда отличается от default.
- **Отложено:** flex, grid, float, абсолютное позиционирование, `%` в margin/padding/width/height (нужен containing block), единицы `ch`/`ex` (требуют font metrics), color spaces CSS4 (`lab`, `lch`, `oklab`, `oklch`, `color()`), реальная отрисовка bold/italic вариантов в paint, селектор-matching с back-tracking.

### `lumen-paint` 🟡 (fill rects + textured text + FontMeasurer)

- **Готово:** `DisplayCommand` enum (FillRect, DrawBorder, DrawText), `build_display_list` обход LayoutBox с painter's order — для `BoxKind::Block` с border эмитирует `DrawBorder`; для `BoxKind::InlineRun` эмитирует по одному `DrawText` на фрагмент с правильными X/Y-смещениями плюс FillRect-ы для подрисовки text-decoration. wgpu Renderer с двумя pipeline-ами: fill (vertex pos + color) и text (vertex pos + uv + color). Два WGSL-шейдера, общий uniform (viewport), bind group для атласа (R8 texture + linear sampler). `GlyphAtlas` 512×512 со shelf packer-ом. `FontMeasurer<'a>` — реализация `TextMeasurer` на основе TTF hmtx/cmap для shell. Per-glyph metadata кеш (atlas position + left/top offset + advance_native). Atlas заливается на GPU только при dirty. `DrawBorder` renderer: 4 fill-quad-а (top/right/bottom/left edges), цвет `border_*_color.unwrap_or(style.color)`.
- **Готово (text-decoration):** для каждого фрагмента с непустой `TextDecorationLine` после `DrawText` эмитятся FillRect-ы: underline — под baseline (+10% fs), line-through — выше baseline (-30% fs), overline — у верха строки (-78% fs относительно baseline). Толщина ≈ 7% font_size, минимум 1px. Цвет — `frag.style.color` (Phase 0 нет text-decoration-color, fallback на currentColor). Ширина FillRect берётся из `InlineFrag.width`.
- **Готово:** `serialize_display_list(&[DisplayCommand]) → String` — детерминированный текстовый формат для snapshot-тестов. 6 интеграционных golden-тестов в `tests/snapshot_tests.rs` (пустая страница, параграф, фон, вложенный paint-порядок, кириллица, line wrap). Механизм `UPDATE_SNAPSHOTS=1` для регенерации golden-файлов.
- **Отложено:** pixel snapshot tests, multi-size atlas (сейчас один размер растеризации — 24px, display масштабируется linear sampler-ом), GPU-pipeline для скруглений/градиентов/теней, layer-tree compositor, double/wavy/dashed стили линий декорации.
- 33 unit-тестов (display_list + atlas + wrapping + inline-flow + text-decoration) + 6 snapshot-тестов = 39 тестов.

### `lumen-font` 🟡 (TTF read + raster)

- **Готово:** парсеры таблиц head, maxp, cmap (format 4 + **format 12**), hhea, hmtx, loca, glyf. Glyf обрабатывает simple-глифы (контуры с on-curve / off-curve, квадратичные Безье) и **composite-глифы** (ссылки на другие глифы с 2×2 transform + offset). `Font::glyph_resolved` рекурсивно разворачивает composite в Simple с max-depth 8. Scanline-растеризатор с 4×4 supersampling, even-odd fill, 1px padding. `Bitmap` с метриками left/top для placement. Bundled Inter v4.1 Regular. **cmap format 12** — Sequential Groups, полный Unicode U+0000..U+10FFFF, включая SMP (эмодзи U+1F600+, математику U+1D400+, исторические письменности); `CmapSubtable` enum с rank-based выбором лучшей записи (platform 3/encoding 10 → rank 0, 3/1 → rank 2); бинарный поиск по группам O(log n).
- **Отложено:** hinting (TT-инструкции), GSUB/GPOS (advanced shaping для лигатур, kerning, Arabic/Indic), CFF outlines (для PostScript-OpenType `.otf` без TT-таблиц), variable fonts (fvar/gvar/avar/HVAR), color glyphs (COLR/CPAL, sbix), bitmap strikes (EBDT/EBLC), composite с ARGS_ARE_XY_VALUES=0 (point alignment, рудимент — сейчас offset = (0,0)).
- 62 unit-тестов + 9 интеграционных на bundled Inter. Включает тест на composite кириллической `А`.

### `lumen-encoding` 🟡 (детектор + однобайтовые декодеры)

- **Готово:** таблицы декодирования `Windows-1251`, `KOI8-R`, `CP866` (по WHATWG Encoding Standard). Декодер `decode(encoding, bytes) → String` для всех четырёх (включая UTF-8) с lossy-обработкой нелегальных байт и автоматическим срезом UTF-8 BOM. Детектор `detect(bytes, content_type_hint) → Encoding` с приоритетами: BOM → `<meta charset>`/`<meta http-equiv>` в первом килобайте → HTTP content-type hint → валидный UTF-8 → частотная эвристика по русским буквам (взвешенный score из 32 частот). Реализует `lumen_core::ext::EncodingDetector` через `HeuristicDetector`. `Encoding::from_label` парсит WHATWG-алиасы (`cp1251`, `koi8r`, `ibm866`, …).
- **Отложено:** UTF-16 как отдельная кодировка (BOM сейчас падает в эвристику и в большинстве случаев работает), ISO-8859-5 и MacCyrillic (не встречаются в природе), полный HTML5 prescan algorithm §12.2.3.2 (наш sniff проще, чем spec, но для практики хватает).
- 35 unit-тестов (декодер + таблицы + детектор + trait) + 6 интеграционных round-trip (encode → detect → decode по «Бородино»).

### `lumen-storage` ✅ (in-memory KV + snapshot)

- **Готово:** `InMemoryStorage` — `HashMap<PartitionedKey, Vec<u8>>` с полным origin-партиционированием: каждый вызов принимает `origin: Option<&str>` и `top_level_site: Option<&str>`. `None` и `""` — один namespace (глобальный профиль). Реализует `lumen_core::ext::StorageBackend` (get/put/delete/list_keys). Snapshot-формат `LUMEN_KV_V1` — текстовый, hex-encoded composite key + hex-encoded value, без внешних зависимостей. `serialize()` / `deserialize()` для in-memory round-trip; `save(path)` / `load(path)` для диска.
- **Отложено:** B-tree persistent backend (сейчас вся структура в RAM), TTL для cookies, namespace helpers (`cookies::`, `history::`, `profile::`), `clear_origin(origin)` для быстрой чистки всех данных источника.
- 17 тестов: CRUD, origin-изоляция, top_level_site-партиционирование, list_keys, snapshot round-trip (включая binary и кириллицу), ошибки десериализации.

### `lumen-network` ✅ (HTTP/1.1 + HTTPS)

- **Готово:** `HttpClient` реализует `NetworkTransport` из `lumen-core::ext`. Поддержка HTTP и HTTPS (rustls + webpki-roots, exception #3). Redirect-следование до 5 хопов (абсолютные + относительные `Location`). `chunked` Transfer-Encoding decoder. URL-парсинг (scheme/host/port/path), case-insensitive заголовки. Box-обёртка вокруг TLS stream (clippy large-enum-variant).
- **Отложено:** HTTP/2, keep-alive соединения, кэш (Cache-Control), аутентификация, cookie jar, проксирование.
- 12 тестов: URL-парсинг, status line, header lookup, chunked decoder (несколько chunk-ов, пустое тело).

### `lumen-shell` 🟡 (окно + рендер + сеть)

- **Готово:** winit 0.30 с `ApplicationHandler` API. Три режима: `lumen` (пустое окно 1024×720), `lumen <path.html>` (файл → кодировка → HTML → layout → paint), `lumen <http(s)://...>` (сеть через `HttpClient` → те же этапы). Внешний CSS: `<link rel="stylesheet" href="...">` загружается с диска (относительно HTML-файла) или по сети (относительно базового URL). `ResourceBase` enum изолирует логику разрешения относительных URL. Inter-Regular.ttf bundled через `include_bytes!`. Обработчики Resized + RedrawRequested.
- **Отложено:** вкладки, омнибокс, навигация, истории сессий, scroll, обработка input-событий.
- 11 unit-тестов (resolve_url, ResourceBase::resolve, collect_link_hrefs).

### Инфраструктура

- Cargo workspace, edition 2024, resolver 3, MSRV 1.95.
- 11 крейтов в `crates/`: shell, core, network, storage, engine/{html-parser, css-parser, dom, layout, paint, font, encoding}.
- Bundled assets: `assets/fonts/Inter-Regular.ttf` (+ OFL.txt лицензия).
- Тестовая страница: `samples/page.html` со встроенным `<style>`.
- 4 разрешённых внешних зависимости: `winit = "0.30"`, `wgpu = "26"`, `rustls = "0.23"` + `webpki-roots = "0.26"` (активированы в lumen-network), JS engine (зарезервирована).
- Внутренние deps: workspace.dependencies на 11 крейтов.
- `.gitattributes` форсит LF для всех текстовых файлов; binary-метка для `.ttf / .png / .woff2`.
- `.gitignore` игнорирует `/target`, `/*.zip`, `/*.tar*`, `.idea/`, `.vscode/`, swap-файлы.

### Численно

- **Всего тестов в workspace:** 627 (на момент последнего обновления).
- **`cargo clippy --workspace --all-targets -- -D warnings`** проходит без warnings.
- **Внешних зависимостей runtime:** 2 активных (winit, wgpu) + 2 зарезервированных.
- **Транзитивно через wgpu/winit:** ~200 crates.

---

## Roadmap — что предстоит реализовать

Приоритизированный список. Порядок может меняться, ориентируйся на план §16 «Фазы».

### Ближайшее (Phase 0 закрыта, Phase 1 начало)

### Средний приоритет (Phase 1+)

6. **CSS — типизированные значения деклараций** — length / color / calc / `--var`. Селекторы Level 3 готовы полностью (compound, combinators, attribute, structural+functional pseudo, `:not`, specificity).
7. **Tab session export / import** (§12.7) — сериализация в snapshot-формат lumen-storage. Простое, экономит много боли.
8. **Картинки на страницах** — `<img>` рендеринг. Нужны PNG/JPEG декодеры (свои, по §5).

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
- Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — для битых старых шрифтов.
- Полноценный selector-matching с back-tracking для complex-селекторов (текущий — right-to-left greedy, может промахиваться на патологических `a b c` с вложенными предками).
- CSS4 pseudo-class `:has(...)` — единственный из CSS4 functional pseudo, что остался; `:is(...)`, `:where(...)`, `:nth-*`, `:not(compound)` уже работают.

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
- **`TextMeasurer` trait в `lumen-layout`, `FontMeasurer<'a>` в `lumen-paint`**: layout не зависит от font напрямую; shell создаёт `FontMeasurer<'static>` из `INTER_FONT` и передаёт через `layout_measured()`. `BoxKind::InlineRun { segments, lines }` хранит строки post-wrap и per-segment стили. `layout()` без измерителя — backward compat, без переноса.
- **`InlineRun` вместо `Text`**: `BoxKind::Text` упразднён. Текстовые узлы и inline-элементы теперь всегда объединяются в `InlineRun`. Слияние фрагментов — через `ComputedStyle::text_rendering_eq` (только color/font_size/line_height), а не `PartialEq`: это нужно, чтобы `<span>` (display:inline) и соседний текстовый узел (display:block inherited) не расщеплялись в отдельные DrawText при одинаковом цвете/размере.
- **`StorageBackend` trait с origin-партиционированием:** сигнатура `get/put/delete(origin, top_level_site, key)` + `list_keys(origin, top_level_site)`. `None` и `""` — один namespace. Решение принято при первой реализации (`lumen-storage`), чтобы не переделывать сигнатуру позже.
- **Encoding detection — частотная эвристика по русским буквам**, не bi-gram / n-gram модель. Соотношение «вес = доля буквы в обычных русских текстах» по таблице из 32 строчных. Этого достаточно, чтобы уверенно различать cp1251 / KOI8-R / CP866 на тексте длиннее ~30 байт: фонетическая раскладка KOI8-R и DOS-блоки CP866 дают резко разный результат при ошибочной декодировке. Более сложные модели (CharsetDetect / chardet) — overkill для Phase 0. Если упрёмся в edge case — пересмотрим.
- **Specificity-based style cascade.** Каскад собирает все matched declarations с ключом `(specificity, rule_order, decl_index)`, сортирует по возрастанию и применяет по порядку — выигрывает максимальная specificity, при равенстве — позже объявленная (CSS spec). Specificity считается tuple `(a=ids, b=classes+attrs+pseudo, c=types+pseudoelements)`, universal и combinator не учитываются. Альтернативой был «last-rule-wins без подсчёта» — отказались, потому что для реальных CSS-стилей нужно правильное поведение `#main` против `.section`.
- **Right-to-left greedy matching без back-tracking** для complex-селекторов. Для `a b c`: проверяем `c` на текущем элементе, для `b` — ищем первого подходящего предка/sibling и фиксируем его, для `a` — то же относительно зафиксированного. Это упрощение: для патологического `div p span` с несколькими `div`-предками, где только дальний обёрнут вокруг `p`, мы можем промахнуться. Сознательное упрощение Phase 0 — реальные стили редко полагаются на back-tracking, а правильный matcher с back-tracking требует Selectors-движка как у Servo/Stylo. До этого — известное ограничение.
- **Interactive pseudo-classes (`:hover`, `:focus`, `:active`, …) парсятся, но всегда не матчат** в Phase 0. Хранятся как `PseudoClass::Unsupported(name)` — это не data loss, а honest «нет интерактивного состояния». Когда появится input/hover state — там и заработают. До этого правило `a:hover { color: red }` корректно парсится и просто не применяется ни к чему.
- **`NthSpec { a, b }` хранит формулу `:nth-*` как пару чисел, а не как parsed AST.** Матчинг `spec.matches(i)` решает уравнение `i = a*n + b` при `n ≥ 0` напрямую, без виртуальных «итераторов». Это даёт O(1) на проверку одного элемента и единственный путь для `odd` / `even` / целых констант / любых линейных форм. Альтернатива (хранить `Vec<i32>` индексов или callable) — overengineering для линейной формулы.
- **`:not(compound)` хранится как `Box<CompoundSelector>`**, а не как полный selector. CSS3 запрещает combinator-ы и nested `:not(:not(...))` внутри, а CSS4-расширения (`:not(complex)`, `:not(list)`) добавим позже отдельной задачей. Сейчас неподдерживаемые формы (`:not(a b)`, `:not(:not(x))`) парсер откатывает на `Unsupported("not")` — content не сохраняется, но и не теряется грамматика правила.
- **`text-decoration` наследуется через каскад в Phase 0** (CSS3 формально не наследует `text-decoration-line` — вместо этого «декорация распространяется на потомков» через box tree, что требует двух проходов layout-а). Прямое наследование даёт интуитивный результат для `a { text-decoration: underline }` и оставляет возможность сбросить декорацию у потомка через `text-decoration: none`. Подрисовка линий — приблизительной геометрией (baseline = line_y + font_size * 0.80, толщина ~7% fs, цвет = currentColor): без `post`/`OS_2` метрик шрифта точные позиции underline_position / underline_thickness нам недоступны, ratio 0.80 совпадает с ascent ratio Inter, которым рендерер позиционирует глифы.
- **`InlineFrag.width` хранится в самом фрагменте, не вычисляется заново по тексту**. Альтернатива (recompute по chars + char_width в paint) потребовала бы прокидывать `TextMeasurer` в `build_display_list`, что нарушает текущую границу «paint не зависит от font». Хранение width-а — одно поле на фрагмент, заполняется в wrap_inline_run.
- **`box-sizing` хранится в `ComputedStyle` как enum, ветвление — только в `lay_out`.** Альтернативой было передавать `BoxSizing` отдельным параметром или хранить уже посчитанные `rect.width = w` как «семантику», но смешивание моделей в одной точке (`if let Some(w) = s.width { match s.box_sizing { ... } }`) — единственный шаг, где модели расходятся, и держать его на месте чтения проще и читабельнее. `box-sizing` явно не наследуется в `compute_style` (сброс на ContentBox в каждом элементе), даже несмотря на отсутствие лишнего шага: иначе все потомки `div { box-sizing: border-box }` тихо ломали бы свой `width: 100%`. Анонимный `InlineRun` тоже сбрасывает box-sizing — у него `width` и `height` уже None, но иначе snapshot пачкается лишним полем.
- **RAWTEXT state хранится как `Option<String>` в самом токенизаторе, а не как отдельное «состояние» в стиле full FSM.** Iterator-based архитектура у нас уже опирается только на `pos: usize` — добавлять явный `enum State` ради одного режима — overengineering. Поле `raw_text: Option<String>` ставится в `consume_start_tag` сразу после распознавания `<script>` / `<style>`, и проверяется в `next()` первой же веткой. `.take()` гарантирует, что режим всегда сбрасывается за один проход — даже если в теле не было `</tag` (тогда читаем до EOF и возвращаемся в обычное русло). `is_raw_text_element(name)` — отдельная функция: её удобно расширять для будущих RCDATA-тегов и не приходится править `next()`. Self-closing `<script/>` — режим **не включается**: симметрично с tree_builder, который для self-closing не пушит элемент в стек. RAWTEXT — это RAWTEXT в смысле HTML5 §13.2.5.2 (entities не декодируются и угловые скобки литеральны); RCDATA для `<title>`/`<textarea>` (где entities декодируются) — отдельная задача.
- **`:is(list)` и `:where(list)` хранятся как `Vec<ComplexSelector>`** (в отличие от `:not(compound)` — там запрещены combinator-ы по CSS3, поэтому достаточно `Box<CompoundSelector>`). CSS4 явно разрешает combinator-ы внутри `:is`/`:where`, поэтому хранилище — полный selector list. Matcher — `list.iter().any(...)` рекурсивно через `matches_complex`; это естественно корректно из коробки. Specificity: `:is` contributes максимальную specificity по списку (CSS4 §17 «specificity of an :is/:not/:has is the specificity of the most specific selector in its arg»), `:where` — всегда 0. Для max-вычисления добавлена функция `max_list_specificity(&[ComplexSelector]) -> Option<Specificity>` рядом с `accumulate_specificity`. Парсер: внутри тела `:is(...)` зовём существующий `parse_selector_list`; чтобы он корректно останавливался на `)`, в `parse_complex_selector` добавлено `)` в список break-токенов tail-цикла. Пустые `:is()` / `:where()` возвращают `Unsupported(name)` — это даёт ту же fallback-семантику, что у `:not(a b)`.
- **`!important` отделяется от value на уровне парсера, хранится булевым полем в `Declaration`.** Альтернатива (хранить `!important` как часть строки `value` и парсить заново при apply) была раньше — теперь убрана. Причина: `apply_declaration` имеет 30+ свойств, каждое со своим парсингом — добавлять везде стрипа `!important` дороже и легче пропустить. Извлечение делается функцией `extract_important(&str) -> (String, bool)`, которая работает с уже trim-нутой строкой и проверяет суффикс через `eq_ignore_ascii_case` на байтах. Cascade использует ключ `(important, specificity, rule_order, decl_index)` — `important` идёт первым, потому что в Rust `true > false`, и ascending sort ставит !important в конец, давая ему победу. Этот же ключ корректно обрабатывает все остальные правила каскада (between two !important winning by specificity, then later-wins-on-tie). UA / user origin не реализованы (только author), поэтому не требуется отдельная иерархия origin.

### Открытые вопросы (решим, когда упрёмся)

- **AI backend:** Ollama HTTP API (нулевая интеграция, требует, чтобы у пользователя был Ollama) ИЛИ встроенный llama.cpp через FFI (5-е exception, нет внешних зависимостей у пользователя). Откладываем до Phase 3.
- **Shaping для сложных скриптов** (Arabic/Indic/Thai): свой shaper за месяцы или rustybuzz как 5-е exception. Откладываем до Phase 2-3.
- **iOS:** Apple-policy требует WebKit на iPhone/iPad. Это противоречит принципу собственного движка. Возможный путь — тонкий shell поверх WKWebView только для iOS, остальные ОС — наш движок. Откладываем до Phase 4.

### Намеренно отвергнутые альтернативы

- **WebView2 / wry / CEF обёртка** — это другой проект, не Lumen. Отказались.
- **html5ever / cssparser / taffy / image / encoding_rs / ttf-parser / rustybuzz / tokio / rayon / redb / egui** — все эти crates рассмотрены и **не взяты**: для них пишется свой код по принципу «default — своё». См. зачёркнутый список в §5.

---

## История последних merge-ов

Чтобы быстро понять, что было сделано в недавних сессиях. Последние сверху.

```
*            css-cursor             — cursor (CSS UI L4 §8.1): 36 keyword-ов, inherited, comma-list с url() игнорируется в пользу последнего keyword; 5 новых тестов
*            css-overflow           — overflow / overflow-x / overflow-y (CSS Overflow L3): visible/hidden/clip/scroll/auto, не наследуется, shorthand 1-2 значения; 6 новых тестов
*            css-visibility         — visibility: visible/hidden/collapse (CSS Display L3 §4): inherited, hidden оставляет место в layout (≠ display:none); 6 новых тестов
*            css-outline            — outline (CSS UI L4 §3): width/style/color + shorthand + outline-offset, не занимает места в коробке; 6 новых тестов
*            css-opacity            — opacity (CSS Color L3 §3.2): 0..1, не наследуется, parses number/percentage, clamp вне диапазона; 7 новых тестов
*            css-white-space-nowrap — white-space: normal | nowrap; nowrap передаёт INFINITY в wrap_inline_run, отключая перенос; 5 новых тестов
*            css-font-family        — font-family parsing: Vec<String>, quoted/unquoted, schloop whitespace, inherited; рендерер пока всегда Inter; 7 новых тестов
*            css-word-spacing       — word-spacing (CSS Text L3 §11.3): inherited, gap только на word-boundary, ширина одиночного слова неизменна; 6 новых тестов
*            css-letter-spacing     — letter-spacing (CSS Text L3 §11.2): inherited, добавочный gap между symbol+word, отрицательные значения; 6 новых тестов
*            css-text-indent        — text-indent (CSS Text L3 §7.1): отступ перед первой строкой, inherited, применяется в wrap_inline_run; 6 новых тестов
*            css-text-transform     — text-transform: none/uppercase/lowercase/capitalize; cyrillic case-folding через char::to_uppercase; 8 новых тестов
*            css-font-weight        — font-weight: normal/bold/numeric/lighter/bolder + UA bold для b/strong/h1-h6/th, lighter/bolder по таблице L4 §2.4.3; 8 новых тестов
*            css-font-style         — font-style: normal | italic | oblique + UA stylesheet (em/i/cite/dfn/address/var → italic); 6 новых тестов
*            html-doctype-parsing   — DOCTYPE parsing (HTML5 §13.2.5.53-72): Token::Doctype с name/public_id/system_id, NodeData::Doctype узел; 9 новых тестов
*            css-hue-units          — HSL hue в turn/rad/grad (CSS Color L4 §9): parse_hue_component распознаёт суффиксы, конвертирует в degrees; 4 новых теста
*            css-viewport-units     — vh/vw/vmin/vmax (CSS Values L3 §6.1.2): Length варианты, parse_length распознаёт суффиксы, viewport прокинут через compute_style; 5 unit + 7 layout тестов
*            css-named-colors       — полный CSS3 набор named colors (147) + rebeccapurple (CSS4): сортированная таблица NAMED_COLORS + binary_search_by_key; 10 новых тестов
*            css-important          — !important флаг (CSS Cascade L4 §8.1): Declaration.important, extract_important парсер, sort key (important, specificity, rule_order, decl_index); 8 css-parser + 5 layout тестов
*            css-attr-case-insensitive — case-insensitive [attr=val i] (CSS L4 §6.3.6): AttrSelector.case_insensitive, парсер `i`/`s`, ASCII-only fold через as_bytes; 7 css-parser + 9 layout тестов
*            html-raw-text          — RAWTEXT для <script> и <style>: содержимое — литеральный текст до </tag + терминатор, entities не декодируются; 13 tokenizer + 3 tree_builder теста
*            css-is-where           — CSS4 :is() / :where(): PseudoClass::Is/Where + matcher (any-match) + specificity (max-of-list / 0); 11 css-parser + 7 layout тестов
*            css-box-sizing         — CSS box-sizing (content-box / border-box): BoxSizing enum, парсер, корректировка width/height в lay_out; 14 unit + 1 snapshot тест
*            text-decoration        — CSS text-decoration (underline / overline / line-through): TextDecorationLine + InlineFrag.width + FillRect-ы у baseline
*            css-borders            — CSS border: BorderStyle, 12 полей border_*_{width,style,color}, DrawBorder renderer, 8 новых тестов
*            css-dimensions         — CSS width/height (px/em/rem): явные размеры блоков; 7 новых тестов
*            css-relative-lengths   — Length {Px,Em,Rem,%}: em/rem/% в font-size/line-height, двухпроходный cascade
*            text-align             — CSS text-align: left/center/right: align_lines() сдвигает frag.x; 5 новых тестов
*            cmap12                 — cmap format 12: Sequential Groups, полный Unicode U+10FFFF, эмодзи/SMP, бинарный поиск
*            link-stylesheet        — <link rel=stylesheet>: внешний CSS с диска и по сети; 11 тестов
*            lumen-network          — крейт lumen-network: HTTP/1.1 + HTTPS через rustls; shell открывает URL
*            css-selectors          — расширенные CSS-селекторы: combinators, pseudo-classes, attribute selectors, specificity
*            lumen-storage          — крейт lumen-storage: InMemoryStorage + origin-партиционирование + snapshot LUMEN_KV_V1
*            inline-elements        — InlineRun: <a>/<span>/<em>/<strong> в одной строке с текстом, per-segment стили
*   358c05f  task-coordination      — протокол резервации задач между параллельными сессиями (Git workflow + блок в шапке плана)
*   a4e5249  snapshot-tests         — serialize_display_list + 6 golden-тестов (пустая страница, параграф, фон, кириллица, line wrap)
*   8e6bdeb  encoding-detection     — крейт lumen-encoding: BOM + meta + heuristic, cp1251/koi8-r/cp866
*   90b849a  inline-flow            — TextMeasurer + FontMeasurer + line wrapping: текст переносится по словам
*   bcd79bb  hide-head-elements     — <title>, <style>, <script> и др. метаданные больше не рендерятся
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
- **Тесты в `lumen-paint::display_list` и `lumen-paint::atlas`** — это unit-тесты. Renderer (`renderer.rs`) визуальный, без автотестов; проверяй через `cargo run`. Display list snapshot-тесты реализованы в `tests/snapshot_tests.rs`.
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
