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

## Рабочая зона

**Писать код можно только в пределах папки браузера** — `/home/konstantin/RustroverProjects/lumen-browser/` и её worktree-копии в `.claude/worktrees/*`. То же касается любых других правок в репо: документации, конфигов, snapshot-тестов. Всё, что вне этого корня — `~/.bashrc`, `~/.config/*`, системные dotfile-ы, соседние проекты, **ad-hoc worktree-ы вроде `~/RustroverProjects/lumen-<task>/`** — не трогаем даже «для удобства». Если задача требует правки чего-то вовне (например, шага установки в README) — описываем словами, что пользователю сделать, и ждём его согласия.

`git worktree add` подчиняется тому же правилу: путь worktree-а — `.claude/worktrees/<имя-задачи>/` (внутри папки браузера), **не** `../lumen-<имя-задачи>/` или любая другая папка снаружи. Подробнее — раздел «Изоляция через `git worktree`» в Git workflow.

Исключение: память Claude (`~/.claude/projects/.../memory/`) — она и так живёт вне репо by design; ей правила «только в папке» не касаются.

---

## Распределение задач между программистами

Над проектом параллельно работают **три программиста** (3 сессии Claude Code, каждая в своём `git worktree` — см. «Координация параллельных сессий»). Каждый закреплён за своей доменной зоной, чтобы конфликты merge-а были минимальными. Бывшая роль P4 (shell + JS + runtime + UI) объединена с P3 — роли упростились до трёх, без потери покрытия.

**Если в начале сессии пользователь говорит «ты программист N» — найди свою колонку ниже, и бери задачи с маркером `[PN]` из раздела «Roadmap — приоритизация задач» в `lumen-plan.md`. Если все задачи с твоим маркером взяты другими сессиями (видно через `git branch` + блок «🔄 В работе сейчас» в `lumen-plan.md`) — спроси пользователя, какую следующую брать.**

| Программист | Доменная зона | Основные крейты / подсистемы |
|---|---|---|
| **P1** | Frontend engine: исходник → layout-дерево | `lumen-html-parser`, `lumen-css-parser`, `lumen-dom`, `lumen-layout`, `lumen-encoding`; форм-DOM (ValidityState / pseudo-classes), Shadow DOM cascade, accessibility tree **construction**, Web Animations **value interpolation**, print **pagination algorithm**, contenteditable **DOM mutations + Selection model**, preload-scanner tokenizer mode, **stacking contexts model**, **property trees построение**, **push-tokenizer + incremental tree builder**, Quirks-mode application |
| **P2** | Backend rendering: layout-дерево → пиксели | `lumen-font`, `lumen-paint`, `lumen-image`; **compositor thread + property trees + layer tree**, **layer-tree hit testing**, `mix-blend-mode` / `backdrop-filter` pipeline, CSS Painting Order traversal, color management (ICC / P3 / Rec2020), `<picture>` / `srcset` resource selection (image-side), `<img>` GPU upload, Canvas 2D, print **PDF generation**, font fallback / matcher, WebFonts (WOFF2), variable fonts |
| **P3** | Runtime + system: всё, что вне engine | `lumen-shell`, `lumen-network`, `lumen-storage`, `lumen-knowledge`, `lumen-core::ext`, JS-integration (`rquickjs` → `rusty_v8`), `lumen-ai`; **SOP / CORS / mixed-content / iframe-sandbox enforcement**, connection pooling + Brotli + Range + keep-alive + HTTP/2, WebSocket / SSE / Fetch backend, HTTP auth + client certs, OCSP / CT, Safe Browsing, Service Worker (fetch interception backend + JS worker context + lifecycle), spell-check (storage + UI), **HTML event loop + microtasks + rAF + observers**, streaming pipeline shell coordination, JS ↔ DOM bindings, GC integration, Web Animations **scheduling**, navigation API + bfcache, forms **UI** (file picker, autofill popup, validation tooltip), IME composition, find-in-page, DevTools + CDP server, accessibility **platform bridges** (UIA / AT-SPI / NSAccessibility), permission / download UI, focus mode, customization, scroll + DPR, site isolation, GPU process + sandbox |

> **Подзадачи с несколькими маркерами** (`[P1+P2]`, `[P1+P3]`, `[P1+P2+P3]`) встречаются часто — в основном из-за runtime, который пересекает domain boundaries. В таких случаях **первый маркер = главный owner**; остальные участвуют ревью / интерфейсом / реализуют свою часть в отдельных PR-ах. Маркер `[P3]` теперь покрывает и бывшие `[P4]`-задачи; в исторических commit-сообщениях `[P4]` сохраняется без правок.

### Правила взаимодействия

- **Crate ownership.** Если ты P1 — не лезешь в `lumen-paint` без согласования с P2; если P3 — не правишь layout без согласования с P1. Это снижает merge-конфликты, а не запрещает ревью.
- **`lumen-core` — общая поверхность.** Trait-ы в `lumen-core::ext` правит обычно P3 (Network/Storage/EventSink/Url/JsRuntime), но если P2 нужен новый `FontProvider` trait или P1 — `AccessibilityProvider` — добавляют сами, не блокируясь на P3. Coordination через коммит-сообщение.
- **`lumen-shell` — у P3.** Это единственный shell-интегратор. Каждая новая capability у P1 / P2, требующая интеграции в окно/loop/runtime, завершается тем, что P3 поднимает её отдельной задачей. Не интегрируешь сам, если ты не P3 — описываешь интеграционную точку в commit-body.
- **Runtime пересекает домены.** Compositor, Web Animations, Forms, contenteditable, Service Worker, Print — каждая такая подсистема **разделена** между несколькими программистами (см. таблицу). Главный owner координирует, но не блокирует остальных: каждый делает свою часть в отдельной ветке, интеграция — следующей задачей.
- **Interface-first.** Любая cross-team задача начинается с того, что owner публикует **типы/трейты** (с `todo!()` или stub) в отдельный коммит. Потребители пишут импл *против stub-а* и не ждут реальной реализации. Стыковка проходит drop-in: пустой stub → реальный impl, потребитель ничего не правит.
- **Точки расширения добавляет тот, кому нужно.** Не блокируй другую сессию на «P3 ещё не добавил trait» — добавь trait сам, P3 ревьюит post-factum.

### Как зарезервировать задачу под себя

Стандартный протокол из раздела «Координация параллельных сессий»: создаёшь feature-ветку (`git checkout -b <имя>`) → первым же коммитом добавляешь строку в блок «🔄 В работе сейчас» в `lumen-plan.md` в формате:

```
- 🔄 <имя задачи> [PN] — <имя ветки> — <YYYY-MM-DD>
```

`[PN]` в строке — чтобы другие сессии видели, кто чем занят, и не дублировались.

---

## Project Skills (скиллы)

Проект содержит 4 скилла в `.claude/skills/`. Используй их вместо ручного следования протоколам:

| Скилл | Когда применять |
|---|---|
| `/lumen-add-css-property` | Добавляешь новое CSS-свойство в `lumen-layout` |
| `/lumen-task-start <имя>` | Берёшь новую задачу из roadmap (создаёт worktree + резервирует в плане) |
| `/lumen-task-finish <имя>` | Задача готова к merge (clippy → тесты → merge --no-ff → worktree remove) |
| `/lumen-new-crate <имя>` | Создаёшь новый Cargo-крейт в workspace |

`lumen-task-start` и `lumen-task-finish` — только по явному вызову (`/`).
`lumen-add-css-property` и `lumen-new-crate` — Claude может вызвать сам по контексту.

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

# Headless dump-режимы (без winit / wgpu). Pipeline до нужной фазы,
# результат в stdout, диагностика в stderr — удобно для CI и сравнения.
cargo run -p lumen-shell -- --dump-source samples/page.html
cargo run -p lumen-shell -- --dump-layout samples/page.html
cargo run -p lumen-shell -- --dump-display-list samples/page.html

# ASCII-превью растеризации глифов из Inter.
cargo run --example preview -p lumen-font

# Baseline-замеры pipeline (decode → parse → layout → paint) на samples/page.html.
# По умолчанию 100 итераций, переопределяется через LUMEN_BENCH_ITERS=...
cargo run -p lumen-bench --release
```

### Важно про PATH (Windows + Git Bash)

`cargo` установлен через `winget Rustlang.Rustup` и лежит в `C:\Users\konstantin\.cargo\bin`. В Git Bash на этой машине эта папка **не подхватывается автоматически**. Перед каждой командой `cargo` (или после установки в `~/.bashrc`):

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
```

В новых терминалах cmd / PowerShell это не нужно — там PATH корректный.

### Текущее число тестов и crates

На момент написания: 3823 теста, 14 крейтов (`shell`, `core`, `network`, `storage`, `knowledge`, `bench`, `dom`, `html-parser`, `css-parser`, `layout`, `paint`, `font`, `encoding`, `image`). При прохождении следующих фаз появится `lumen-ai` и др.

---

## Графические тесты

`graphic_tests/NN-*.html` — 21 страница (00–20), каждая под один визуальный эффект, viewport 1024×720. Все тесты — только графические объекты, без текста. `1000000-final.html` — финальный тест, содержащий все реализованные свойства в одном окне. Триггеры: «Ищи баги по скрину N», «Ищи баги по всем скринам». Технический workflow захвата (ffmpeg gdigrab, Edge headless, diff) — в моей памяти `reference_ffmpeg_screenshot.md`, здесь только правила.

### Правило: добавление нового CSS-свойства

При реализации любого нового CSS-свойства **в том же коммите** обязательно:

1. Добавить объект(ы) в соответствующий тест серии `01–19` (или создать новый файл, если свойство не покрыто ни одним).
2. Добавить демонстрацию в `graphic_tests/1000000-final.html` — финальный тест, где все свойства видны сразу.
3. Обновить таблицу в `graphic_tests/COVERAGE.md` — добавить строку для нового свойства.

Текущее покрытие и список непокрытых свойств — в `graphic_tests/COVERAGE.md`.

### Правила прогона

1. **Скрины не сохраняем в репо.** `bugs/screenshots/*.png` — рабочие артефакты для текущего анализа, не коммитим (ни Edge-эталон, ни Lumen-захват, ни diff). В коммит идёт только обновлённый `BUGS.md`.
2. **Багом считается только визуально заметный артефакт.** Любые ненулевые пиксели в `NN-diff.png` сами по себе — не баг. Если расхождение видно только при попиксельном сравнении и пользователь его не заметит — пропускаем.
3. **Текст пока игнорируем.** Антиалиасинг глифов гарантированно расходится с Edge — не фиксируем как баги до отдельной задачи. Это касается subpixel-rendering, hinting, kerning, weight rendering. Геометрия text-box, padding/margin вокруг текста, line-height — это **не текст**, это layout, его проверяем как обычно.
4. **`BUGS.md` обновляем при каждом прогоне, историю оставляем.** Найден новый — добавляем следующим номером. Уже зафиксированный воспроизвёлся — обновляем дату/скриншот-описание, статус не трогаем. Фикснули — меняем статус на `FIXED` с датой, **запись не удаляем**. WONTFIX до Phase N+ — тоже остаётся в файле.

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
    ├── encoding/         — детектор и однобайтовые декодеры (cp1251/koi8-r/cp866)
    └── image/            — PNG-декодер: CRC32 + chunks + IHDR + inflate + filter undo
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

**Уже определены:** `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `RequestFilter`, `EncodingDetector`, `EventSink`, `DnsResolver`, `HstsEnforcement`, `HttpCredentialProvider`, `FontProvider`, `JsRuntime` (`NullJsRuntime` stub).

**Sprint 0 P3 trait-anchors (stub-реализации `Null*` — «не поддерживается», см. roadmap «Sprint 0 — Контракты»):** `UnicodeProvider` (под `icu4x`), `IdnaProvider` (под `idna`), `PublicSuffixList` (под `publicsuffix`), `ContentDecoder` (расширение под `brotli-decompressor` / `ruzstd`; есть `UnsupportedContentDecoder` stub), `FontFormat` (под `woff2`), `SpellChecker` (под `hunspell-rs`), `HyphenationProvider` (под `hyphenation`).

**Запланированы:** `WindowingBackend` (за winit), `RenderBackend` (за wgpu), `TlsBackend` (за rustls), `KnowledgeStore`, `AiBackend`. Подробно — в §12 плана.

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

**Стратегия: сначала рабочий браузер, потом разговор «что переписывать самим».** Lumen про собственный rendering engine — это ядро, его не трогаем. Всё остальное, что не определяет идентичность проекта (декодеры медиа-форматов, Unicode-таблицы, sub-протоколы), берём из готовых решений, чтобы добраться до Phase 1 в обозримом будущем. После этого — пересматриваем provisional-список и решаем, где есть смысл писать своё.

«Не делаем Google Chrome» означает: ядро (HTML/CSS/DOM/style/layout/paint/font/encoding, URL, HTTP/1.1, DNS, adblock matcher, knowledge layer, UI shell) — наше. Всё остальное — прагматика.

### Две категории exception

**Permanent (5 шт.) — никогда не переписываем сами.** Универсальные правила безопасности / здравого смысла: свой crypto / GPU API / OS event loop / SQL engine / JS engine не пишут.

| Crate | За что | Почему не сами |
|---|---|---|
| `winit` | OS event loop + окна | Win32 + X11 + Wayland + AppKit — годы платформенных багов |
| `wgpu` | GPU API (Vulkan/Metal/DX12) | 4 разных API, driver-баги, годы работы |
| `rustls` + `webpki-roots` | TLS / crypto + bundle корневых CA-сертификатов | **Никогда не пиши свой crypto.** rustls — аудит + формальная верификация. `webpki-roots` — Mozilla CA bundle, без него HTTPS не валидируется |
| `rusqlite` (`bundled` SQLite) | Persistent storage: history, bookmarks, notes, read-later, cookies-TTL, профили + FTS5 для §12.1 | 25 лет TH3-тестирования, стандарт индустрии. Цена ошибки persistent storage — молчаливая порча данных пользователя; та же асимметрия, что у crypto. FTS5 закрывает §12.1 без своего inverted index |
| JS engine (`rquickjs` → `rusty_v8`) | Исполнение JavaScript | V8 — 15 лет, миллиарды долларов, сотни инженеров |

**Provisional accelerators — берём готовое сейчас, переписываем своё «когда».** У каждого — trait-anchor в `lumen-core::ext` и «graduation criterion» (событие, не дата). Список растёт по мере фаз; полная таблица — в §5 плана.

| Crate (кандидаты) | Trait-anchor | Graduation criterion |
|---|---|---|
| Image decoders для JPEG/WebP/GIF (`zune-jpeg`, `image-webp`, узкий `image`) | `ImageDecoder` | Едва ли когда-то; PNG уже свой, остальные — black-box форматы без архитектурной ценности |
| `icu4x` (segmentation / line-break / bidi / normalization) | `UnicodeProvider` | Когда P1 закроет CSS Selectors L4 и появится bandwidth на Unicode-таблицы; реалистично — никогда |
| `brotli-decompressor` | расширение `ContentDecoder` | Едва ли когда-то; формат стабилен |
| `ruzstd` / `zstd-safe` | расширение `ContentDecoder` для HTTP `Content-Encoding: zstd` | Реалистично — никогда; формат стабилен |
| `idna` (полный UTS#46) | `IdnaProvider` | Когда найдём real edge-case, который наш Punycode-only не покрывает |
| `publicsuffix` (или свой loader PSL.dat) | `PublicSuffixList` для cookie domain matching, eTLD+1 | Едва ли; формат простой, но обновляется регулярно |
| `hyphenation` | `HyphenationProvider` | Phase 2+, когда дойдём до типографики; TeX-словари можно переписать на свой формат позже |
| `woff2` | `FontFormat` (расширение) | Phase 2 при добавлении WebFonts; формат стабилен, маловероятно |
| `hunspell-rs` / `spellbook` | `SpellChecker` | Phase 3 при spell-check; русская морфология сложна, переписывать дороже чем стоит |
| `quinn` (HTTP/3 / QUIC) | расширение `NetworkTransport` | Никогда в обозримом; QUIC = год+ работы |

«Реалистично никогда» — это не лицемерие, а честная маркировка: trait-anchor существует, заменить технически тривиально, но политических причин для замены нет.

### Что НЕ переводится даже временно (ядро Lumen)

Эти подсистемы — наша идентичность. Готовые crate-ы рассматриваются и **отвергаются**:

- HTML parser (свой по WHATWG spec) — ~html5ever~
- CSS parser, selectors, cascade — ~cssparser, selectors, stylo~
- DOM (arena-based) — наш
- Layout (block/inline/flex/grid) — ~taffy~
- Paint (display list + wgpu rasterizer) — ~tiny-skia~
- Font parser + rasterizer (TrueType) — ~ttf-parser, font-kit~
- Encoding (cp1251/KOI8-R/CP866/UTF-8 + детектор) — ~encoding_rs~
- URL parser (WHATWG) — ~url~
- HTTP/1.1 + HTTP/2 — ~hyper~
- DNS resolver с DoH/DoT — ~hickory-resolver~
- Adblock matcher — ~adblock~
- Punycode базовый, MD5/SHA-256 (для Digest), Base64 — все свои, маленькие, фундамент
- PNG-декодер с собственным DEFLATE (уже написан, DEFLATE переиспользуется для HTTP gzip/deflate)
- Knowledge layer (§12) — ценность для пользователя, не должна быть кому-то делегирована
- UI shell — иммедиат-режим поверх своих paint-примитивов

Если кто-то предлагает «возьми готовое» для пункта из этого списка — это шаг к Chrome-форку. Не делаем.

### Правило «no new dep без обоснования»

Если в коммите добавляется новая запись в `[dependencies]`, в commit-теле обязателен пункт:

> **Why this dependency:** \<категория (permanent / provisional), trait-anchor, graduation criterion если provisional\>

Provisional-список расширяется по мере того, как фаза упирается в реальную задачу — не превентивно.

---

## Конвенции кода

### Rust версия и edition

- **Rust 1.95+ stable**, зафиксировано в `rust-toolchain.toml`.
- **Edition 2024**, resolver "3".
- MSVC toolchain на Windows.

### Стиль

- Профиль `dev` использует `opt-level = 1` для своего кода (компромисс: debug-сборка медленнее на 10%, но layout/paint работают в 5-10 раз быстрее) и `opt-level = 3` для зависимостей через `[profile.dev.package."*"]` (wgpu / winit / rustls в чистом debug невыносимы; обоснование — в [DECISIONS.md](DECISIONS.md)).
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

Над репозиторием могут одновременно работать несколько сессий Claude Code. Чтобы они не брали из roadmap одну и ту же задачу:

1. **Перед стартом** — прочитать `git branch` и блок **«🔄 В работе сейчас»** в шапке `lumen-plan.md`. Если ветка с нужным именем уже существует или задача в списке — выбрать другую.
2. **Зарезервировать задачу**: создать feature-ветку (`git checkout -b <имя>`) и в **первом же коммите на этой ветке** добавить строку в «В работе сейчас»: `- 🔄 <имя задачи> — <имя ветки> — <YYYY-MM-DD>`. Резервация видна другим сессиям через `git branch`.
3. **При merge в `main`** — в merge-коммите убрать строку из «В работе сейчас» и обновить статусы в плане/CLAUDE.md как обычно.
4. **Если работа отменена** — удалить ветку; строку из «В работе сейчас» убрать в отдельной ветке `cleanup-<имя>`, слить в main.

**Почему ветка — достаточная резервация:** `git branch` виден всем сессиям в том же репозитории без fetch. Имя ветки = имя задачи = резервация. Предыдущий протокол с мини-коммитами на `main` нарушал правило «no commits to main» и создавал лишний шум в истории.

#### Изоляция через `git worktree` — обязательная

**Каждая параллельная сессия Claude Code ОБЯЗАНА работать в отдельном `git worktree`, а не делать `git checkout` в одном и том же каталоге с другой сессией.**

Создание worktree для новой задачи (путь — **внутри** папки браузера, см. «Рабочая зона»):
```bash
# из основного клона:
git worktree add .claude/worktrees/<имя-задачи> -b <имя-ветки>
cd .claude/worktrees/<имя-задачи>
```

После этого сессия живёт в `.claude/worktrees/<имя-задачи>/` и не трогает чужой рабочий tree. Worktree-ы снаружи (`../lumen-<имя-задачи>/`, `~/RustroverProjects/lumen-<имя-задачи>/`) — запрещены: они нарушают правило «писать код только в папке браузера». По завершении (после merge в main):
```bash
git worktree remove .claude/worktrees/<имя-задачи>
```

**Почему обязательна:** при `git checkout <чужая-ветка>` в shared рабочем дереве git стэшит чужие незакомиченные правки в `git stash` и перебрасывает обоих на новую ветку. Восстановление возможно (`git stash list` / `git stash pop`), но трудоёмко: легко получить файлы от чужой задачи, частично потерять собственные правки, попасть в конфликт. Worktree-ы дают каждой сессии собственный изолированный рабочий каталог при общем git-репо (`.git/`), `git branch` всё ещё виден всем — резервация сохраняется.

#### Запрещено в shared working tree

- `git checkout <чужая-ветка>` если в твоём рабочем дереве есть uncommitted changes — даже если ветка пустая. Если очень нужно переключиться — сначала `git commit -am "wip"` или `git stash push -m "<описание>"`.
- Если по ошибке оказался на чужой ветке: НЕ делай `git restore .` или `git checkout -- .` — сначала проверь `git stash list`, верни своё через `git stash pop`, и только потом переключайся обратно. Восстановление через stash хрупкое; если есть сомнения — не торопись.

#### Defensive WIP-коммиты

Перед любой длинной паузой (debug, прогон тестов, большая правка нескольких файлов) — `git commit -am "wip: <описание>"` на своей ветке. Это защита от:
- внезапного `git checkout` чужой сессией (в shared dir, если worktree почему-то не используется);
- потери uncommitted работы при сбое процесса;
- конфликтов при попытке восстановить из stash.

Для cleanup перед merge: можно `git rebase -i HEAD~N` локально, склеив wip-коммиты — но **только** пока ветка ещё не публиковалась (т.е. пока её не подтянула другая сессия / merge не выполнен).

#### Никогда не оставлять worktree на `main` с uncommitted / staged изменениями

Worktree на `main` (с веткой `[main]` или detached HEAD на main HEAD) — это **временная конструкция для атомарного merge**. Сразу после merge / `update-ref` — `git worktree remove <path>`. Длительный worktree на main с любым грязным состоянием — **блокер для всех остальных сессий**:

- Другая сессия не может сделать `git checkout main` (для `git merge --no-ff <task>`) — git откажет: `fatal: 'main' is already used by worktree at <твой path>`.
- Атомарный merge через `update-ref refs/heads/main NEW OLD` с детачем не помогает — пока кто-то держит main checked out, никто другой не может встать на main в своём worktree.

**Признак зомби-worktree:** имя пути не совпадает с веткой (например, `.claude/worktrees/css-foo/` где `[main]`) — это след давно мёртвой сессии. Если git status показывает large staged-diff, который выглядит как «откат недавно влитых задач» — это устаревший снимок, не намеренный revert.

**Лечение зомби, не теряя WIP:**
```bash
# 1. Архивируем patch на всякий случай
git -C <zombie-path> diff --cached > .claude/archive/zombie-staged-$(date +%Y%m%d).patch

# 2. Переводим worktree со staged-state на отдельную ветку
git -C <zombie-path> checkout -B zombie-stale-wip-<дата>
git -C <zombie-path> commit -m "WIP from zombie session ..."

# 3. main свободен. Ветка остаётся как карантин — удалить через
#    git branch -D zombie-stale-wip-<дата>, если revert признан мусором.
```

**Для своих временных worktree** (например, при атомарном merge через `git worktree add --detach`): создал → выполнил `update-ref` → `git worktree remove <path>` в той же команде. Не оставляй на потом, не уходи спать с открытым worktree на main.

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

Текущее состояние отражается в `lumen-plan.md`, `SUBSYSTEMS.md` и `CLAUDE.md` (плюс commit-message). Все обновляются **в том же коммите**, что и сама реализация — не отдельным.

### 1. `lumen-plan.md`

В шапке — блок **«Статус реализации»**, и в §16 — маркеры рядом с задачами фазы. Условные обозначения: ✅ готово · 🟡 в работе / частично · ⬜ запланировано.

После реализации:
- меняй ⬜ → ✅ (или 🟡 → ✅);
- если работа разбита — ставь 🟡 с пометкой, что готово, что нет;
- при появлении новых крупных тем (новая trait-точка, новый крейт) — добавляй строку в соответствующий подсписок.

### 2. Сопутствующие файлы

При значимых вехах обновляй:

- **[SUBSYSTEMS.md](SUBSYSTEMS.md)** — расширь раздел соответствующего крейта (что добавлено в «Готово» / убрано из «Отложено» / число тестов).
- **`lumen-plan.md` → «Roadmap — приоритизация задач»** — если пункт реализован, удали из roadmap (он стал готовым).
- **[DECISIONS.md](DECISIONS.md)** — если приняли архитектурное решение (например, новый exception в политику зависимостей, выбор API подхода).
- **CLAUDE.md → «Известные нюансы и ловушки»** — если ловушка устранена (например, composite-глифы перестали пропускаться) или добавлена новая.

Что **не** требует ручного обновления документации:
- Тривиальные правки (опечатки, форматирование, мелкие refactor-ы без изменения API).
- Тесты, не меняющие capability крейта.
- Документация в коде / комментарии.
- История merge-ов — выводится из `git log --oneline`.

Для всех таких — достаточно обновить план.

---

## Состояние подсистем (детально)

Полное состояние каждого крейта — в [SUBSYSTEMS.md](SUBSYSTEMS.md): scope, что готово / отложено, инварианты для будущих изменений. Обновляй этот файл при каждом коммите, реализующем пункт плана (см. «Статус реализации — поддерживай актуальным»).

---

## Decisions log

Архитектурные решения и их обоснования — в [DECISIONS.md](DECISIONS.md). При появлении нового решения добавляй туда (а не сюда).

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
- **Composite glyphs теперь поддерживаются** через `Font::glyph_resolved` (с max recursion depth 8). Оба варианта alignment — `Anchor::Offset(dx, dy)` (современный ARGS_ARE_XY_VALUES=1) и `Anchor::Points { parent, child }` (рудиментарный TrueType pre-1996, ARGS_ARE_XY_VALUES=0). Кириллические заглавные `А / В / Е / К / М / Н / О / Р / С / Т / Х` (которые в Inter composite через Latin-эквиваленты) и их строчные — рендерятся. Renderer вызывает `glyph_resolved`, не `glyph` напрямую.
- **Тесты в `lumen-paint::display_list` и `lumen-paint::atlas`** — это unit-тесты. Renderer (`renderer.rs`) визуальный, без автотестов; проверяй через `cargo run`. Display list snapshot-тесты реализованы в `tests/snapshot_tests.rs`.
- **Multi-size + variation-aware glyph atlas.** Глифы растеризируются на bin-подобранный размер (`SIZE_BINS = [8, 12, 16, 20, 24, 32, 48, 64]`), display-масштаб = `font_size / size_bin`. При font-size, совпавшем с bin (12/16/24/32...), масштаба нет — текст резкий; иначе небольшая интерполяция (16→20 ≈ 0.88×). Раньше всё рисовалось на фиксированном 24 px — заметный блюр на 12/16/32+. Cache-ключ — `AtlasKey { face_id, glyph_id, size_bin, coords_hash }` где `coords_hash` от normalized variation coords (variable fonts L1): variant-глиф (`wght=700`) не перезаписывает base (`wght=400`). Empty coords / default-instance → hash=0, ключ совпадает с pre-VF поведением (backward-compatible для snapshot-тестов и не-VF страниц).
- **Font fallback / matcher реализован полностью (CSS Fonts L4 §3.1+§5.2+§5.3).** Renderer держит `Vec<LoadedFace>` + `font_provider: Option<Arc<dyn FontProvider>>` (по умолчанию `SystemFontIndex`). При `font-family: Roboto, Arial` обходит имена в приоритете, для первого найденного через `pick_face` (§5.2 weight/style matcher) грузит TTF лениво. Per-char codepoint cascade (§5.3): если в primary face нет глифа для символа — обходим остальные loaded faces; если ни у кого нет — рисуется `.notdef`. **Ограничение Phase 0:** cascade работает только по уже-загруженным face-ам — если CSS не упоминает CJK/эмодзи-шрифт, символ не покроется. Eager preload курируемого fallback-списка — отдельная задача. Generic CSS-family (`serif`/`sans-serif`/`monospace`) пока пропускаются на этапе face resolution.
- **HiDPI / DPR частично поддержан.** Renderer хранит `scale_factor` от `winit` и делит viewport uniform на него — на 4K с 200% scaling 1 CSS px = 2 device px, текст и shape-ы выглядят корректно. `WindowEvent::ScaleFactorChanged` обновляет коэффициент on-the-fly (drag окна между мониторами). **Что ещё не поддержано:** layout viewport остаётся hardcoded 1024×720 — окно открывается этим размером, real `inner_size` в layout не передаётся, relayout при `Resized` отсутствует. Это отдельная ловушка, требует structural refactor pipeline (layout вызывается до создания окна) и трогает P1.
- **Scroll-state поддержан вертикально.** `Lumen { scroll_y, content_height }` + `Renderer::render(content, overlay, scroll_y)`: page-полоса display list-а смещается на `-scroll_y`, overlay-полоса (find-bar + scrollbar) рисуется без смещения = viewport-locked. Ввод: MouseWheel (LineDelta — 40 CSS px на «notch», PixelDelta делится на DPR), стрелки ↑/↓ (40 px, auto-repeat), PageDown/PageUp/Space (90% viewport), Home/End. Кламп `[0, max(0, content_height − viewport_height)]`. Reload сбрасывает scroll в 0. **Scroll-to-match для find:** `find::scroll_to_match(rect, vh, scroll) -> Option<f32>` (None если match уже виден, иначе target такой, чтобы match попал в верхнюю четверть viewport-а); `Lumen::scroll_to_active_match` дёргается после next/prev/backspace/text-input в find-bar и сам делает clamp через `scroll_to`. **Vertical scrollbar overlay:** `lumen-shell::scrollbar::build_scrollbar_overlay(scroll_y, content_height, vw, vh)` — pure-fn, возвращает 2 FillRect (track + thumb у правого края, 8 px), скрыт при `content_height <= viewport_height`. Thumb-геометрия: `h = max(24, vh²/ch) clamp до vh`, `top = (vh − h) × scroll / max_scroll`. `RedrawRequested` подмешивает scrollbar в overlay-буфер перед find-bar-командами. **Drag + track-click:** `scrollbar::classify_track_click(x, y, scroll_y, ch, vw, vh) -> TrackClick { None, Thumb, Above, Below }` — единая точка решения для MouseDown: Thumb → старт drag (`ScrollDrag { start_scroll_y, start_mouse_y }` + `scroll_for(current_y, ch, vh)` = `start + Δy × max_scroll / (vh − h)`, caller клампит), Above/Below → page-jump на ±`page_step(vh)` ≈ 90% viewport (та же формула, что у клавиш PageUp/PageDown), None → клик мимо scrollbar-а. MouseUp / reload сбрасывают drag. Lumen хранит `cursor_position` (winit physical px → CSS px через DPR) и `scroll_drag: Option<ScrollDrag>`. **Smooth-scroll** (`lumen-shell::scroll_anim`): keyboard / wheel / page-jump / find-scroll-to-match плавно анимируются (out-cubic easing, 200 ms) вместо мгновенного прыжка. `Lumen { scroll_anim: Option<ScrollAnim { start_y, target_y, start_time_ms }> }`; `start_smooth_scroll(target)` запускает анимацию (cancel-ит активную), `scroll_by_smooth(delta)` аддитивен поверх текущего target-а — repeat-wheel/keys не «откатывает» к старту. `RedrawRequested` тикает через `advance_scroll_anim()` → `request_redraw()` пока не завершилась. Drag thumb scrollbar-а остаётся instant (interactive). Reload и page-load сбрасывают `scroll_anim = None`. **Ограничения:** только Y-axis (горизонтального скролла нет), нет momentum (free-flick на trackpad), нет cursor-icon feedback на hover thumb-а. Relayout-on-resize всё ещё нет (см. ловушку выше).
- **Pipeline блокирующий, окно открывается после полной загрузки.** `lumen-shell::resumed()` делает синхронно: fetch HTML → parse → собрать `<link href>` → fetch каждого CSS подряд → layout → paint → `window.create()`. До этого момента winit event loop даже не запущен — окно не появляется. На современных сайтах с десятками внешних CSS пользователь смотрит в терминал (network log в stderr) много секунд. `HttpClient` тоже синхронный (`std::net::TcpStream` + blocking rustls). Progressive / streaming rendering — отдельная задача в roadmap «Ближайшее» п.3.
- **Параллельные сессии в одном working tree = катастрофа.** Если две Claude-сессии в одной папке делают `git checkout` разных веток — git стэшит работу одной из них и переключает обе на новую ветку. Восстановление через `git stash list` / `git stash pop` хрупкое: легко получить файлы от чужой задачи, потерять часть собственной работы при `git restore`, попасть в конфликты. **Решение — обязательные `git worktree`-ы** для каждой сессии (см. раздел «Изоляция через `git worktree`» в Git workflow). Если попался на чужой ветке — `git stash list` покажет, что было утеряно; не делай `git restore .` пока не восстановил.

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
