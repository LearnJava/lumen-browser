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

Над проектом параллельно работают **четыре программиста** (4 сессии Claude Code, каждая в своём `git worktree` — см. «Координация параллельных сессий»). Каждый закреплён за своей доменной зоной, чтобы конфликты merge-а были минимальными.

**Если в начале сессии пользователь говорит «ты программист N» — найди свою колонку ниже, и бери задачи с маркером `[PN]` из раздела «Roadmap — приоритизация задач» в `lumen-plan.md`. Если все задачи с твоим маркером взяты другими сессиями (видно через `git branch` + блок «🔄 В работе сейчас» в `lumen-plan.md`) — спроси пользователя, какую следующую брать.**

| Программист | Доменная зона | Основные крейты / подсистемы |
|---|---|---|
| **P1** | Парсинг, DOM, style cascade, layout, derived deree, animation interpolation | `lumen-html-parser`, `lumen-css-parser`, `lumen-dom`, `lumen-layout`, `lumen-encoding`; форм-DOM (ValidityState / pseudo-classes), Shadow DOM cascade, accessibility tree **construction**, Web Animations **value interpolation**, print **pagination algorithm**, contenteditable **DOM mutations + Selection model**, preload-scanner tokenizer mode |
| **P2** | Шрифты, растр, изображения, compositor pipeline, blending | `lumen-font`, `lumen-paint`, `lumen-image`; **compositor thread + property trees + layer tree**, **layer-tree hit testing** (paint-side), `mix-blend-mode` / `backdrop-filter` pipeline, CSS Painting Order (paint-side), color management (ICC / P3 / Rec2020), `<picture>` / `srcset` resource selection (image-side), `<img>` GPU upload, Canvas 2D, print **PDF generation** |
| **P3** | Сеть, хранилище, knowledge layer, crypto, security enforcement | `lumen-network`, `lumen-storage`, `lumen-knowledge`, `lumen-core::ext`; **SOP / CORS / mixed-content / iframe-sandbox enforcement** (не только parsing), connection pooling + Brotli + Range + keep-alive, WebSocket / SSE / Fetch **backend**, HTTP auth + client certs, OCSP / CT, Safe Browsing list, Service Worker **fetch interception**, spell-check **словарь storage** |
| **P4** | Shell, JS engine, runtime, UI features, browser-level UX, platform integration | `lumen-shell`, JS integration (`rquickjs` / `rusty_v8`), `lumen-ai`; **HTML event loop + microtasks + rAF + observers**, streaming pipeline shell coordination, JS ↔ DOM bindings, GC integration, Web Animations **scheduling**, navigation API + bfcache, forms **UI** (file picker, autofill popup, validation tooltip), IME composition, find-in-page, DevTools + CDP server, accessibility **platform bridges** (UIA / AT-SPI / NSAccessibility), permission / download UI, spell-check **UI**, focus mode, customization, scroll + DPR, site isolation, GPU process + sandbox |

> **Подзадачи с несколькими маркерами** (`[P1+P2]`, `[P1+P4]`, …) встречаются часто — в основном из-за runtime, который пересекает domain boundaries. В таких случаях **первый маркер = главный owner**; остальные участвуют ревью / интерфейсом / реализуют свою часть в отдельных PR-ах.

### Правила взаимодействия

- **Crate ownership.** Если ты P1 — не лезешь в `lumen-paint` без согласования с P2; если P3 — не правишь layout без согласования с P1. Это снижает merge-конфликты, а не запрещает ревью.
- **`lumen-core` — общая поверхность.** Trait-ы в `lumen-core::ext` правит обычно P3 (Network/Storage/EventSink/Url), но если P2 нужен новый `FontProvider` trait или P1 — `AccessibilityProvider` — добавляют сами, не блокируясь на P3. Coordination через коммит-сообщение.
- **`lumen-shell` — у P4.** Каждая новая capability у других программистов завершается тем, что P4 интегрирует её в shell отдельной задачей. Не интегрируешь сам, если ты не P4 — описываешь интеграционную точку в commit-body, P4 поднимет.
- **Runtime пересекает домены.** Compositor, Web Animations, Forms, contenteditable, Service Worker — каждая такая подсистема **разделена** между несколькими программистами (см. таблицу). Главный owner координирует, но не блокирует остальных: каждый делает свою часть в отдельной ветке, интеграция — следующей задачей.
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

На момент написания: 1207 тестов, 14 крейтов (`shell`, `core`, `network`, `storage`, `knowledge`, `bench`, `dom`, `html-parser`, `css-parser`, `layout`, `paint`, `font`, `encoding`, `image`). При прохождении следующих фаз появится `lumen-ai` и др.

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

**Уже определены:** `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `RequestFilter`, `EncodingDetector`, `EventSink`, `DnsResolver`.

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

**Исключение из «своё» — хранение данных пользователя.** Persistent storage (history, bookmarks, notes, read-later, cookies, профили, knowledge layer FTS, HTTP-кэш ресурсов) берём из готовых решений — стандарт индустрии браузеров, плюс зрелая БД с audit-ом против data-loss / corruption. Decoder-ы и parsers (PNG inflate, TTF parse, HTML/CSS) **под это правило не подпадают** — это streaming-форматы, не persistent data. In-memory структуры (DOM arena, layout tree, glyph atlas) — тоже не storage. Подробнее — Decisions log и [project-db-for-history](~/.claude/projects/.../memory/project_db_for_history.md).

### Пять разрешённых exception (только эти, всё остальное — свой код)

| Crate | За что | Почему не сами |
|---|---|---|
| `winit` | OS event loop + окна | Win32 + X11 + Wayland + AppKit — годы платформенных багов |
| `wgpu` | GPU API (Vulkan/Metal/DX12) | 4 разных API, driver-баги, годы работы |
| `rustls` | TLS / crypto (когда подключим сеть) | **Никогда не пиши свой crypto** |
| JS engine (`rquickjs` / `rusty_v8`) | Исполнение JavaScript | 15 лет работы Google/Mozilla |
| SQLite (`rusqlite` с `bundled`) | Персистентное хранилище: history, bookmarks, notes, read-later, cookies-TTL, профили + FTS5 для §12.1 | 25 лет, миллиарды inst, TH3-тестирование. Стандарт индустрии браузеров (Firefox/Chromium/Safari). FTS5 = полнотекстовый поиск без своего inverted index |

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

- Профиль `dev` использует `opt-level = 1` для своего кода (компромисс: debug-сборка медленнее на 10%, но layout/paint работают в 5-10 раз быстрее) и `opt-level = 3` для зависимостей через `[profile.dev.package."*"]` (wgpu / winit / rustls в чистом debug невыносимы; обоснование — в Decisions log).
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
- **CLAUDE.md → «Decisions log»** — если приняли архитектурное решение (например, новый exception в политику зависимостей, выбор API подхода).
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

Это короткие записи решений и их обоснования. Полные обсуждения — в commit-сообщениях и плане.

### Зафиксированные

- **Свой rendering engine** (не обёртка над Chromium/WebKit/Servo). Главное идеологическое решение, без него проект теряет смысл. Закреплено в §1 принципах.
- **5 разрешённых external dependencies:** winit, wgpu, rustls, JS engine, **SQLite** (через `rusqlite` с feature `bundled` — компилируется в бинарь, без runtime-зависимости от системного `libsqlite3`). Подробное обоснование каждого — в §5 плана. Любой шестой — только через коммит с пунктом «Why this dependency» в теле + обновление CLAUDE.md и плана.
- **Composite glyphs разворачиваются** через `Font::glyph_resolved` с max recursion depth = 8. Renderer вызывает `glyph_resolved`, не `glyph` напрямую.
- **Atlas размер растеризации фиксирован на 24px**, display масштабируется через quad scale + linear sampler. Multi-size atlas — позже, когда увидим, что качество критично.
- **Inter Regular bundled** через `include_bytes!` в lumen-shell (~411 КБ к binary). Не runtime-loading, чтоб не было path-проблем.
- **WASM плагины через wasmtime** — выбор сделан в §11.2 vs native dylib / WebExtensions. Решает проблему prompt-injection-class уязвимостей через capability tokens.
- **Capability-модель** для плагинов вместо статических permissions (§11.4).
- **Memory-safe Rust, `unsafe` только на FFI-границах** с обязательным `// SAFETY:` комментарием. Текущие `unsafe` блоки: `as_bytes` в renderer.rs, FFI к wgpu внутри wgpu crate (не наш код).
- **Feature-branch + `--no-ff` merge** workflow. Видимая структура «коммит-серия = задача» в git log --graph.
- **`opt-level = 1` в dev профиле для своего кода + `opt-level = 3` для всех зависимостей** (`[profile.dev.package."*"]`). Wildcard `*` в Cargo.toml применяется только к dependencies, не к workspace members, поэтому наши крейты сохраняют быструю компиляцию и читаемые stack-trace на уровне 1, а ~200 транзитивных deps (wgpu, winit, rustls и прочее) собираются с full optimization. wgpu в чистом debug режиме невыносим: одна frame занимает секунды на трекинг wgpu-internal validation. opt-level=3 на deps — одноразовый удар по времени первой компиляции, компенсируется за счёт работоспособного dev-loop. Стандарт в графических Rust-проектах.
- **Cargo features пока не используются**, но запланированы для `ai`, `webgl`, `tor`, `ru-hyphenation` опциональных модулей.
- **`TextMeasurer` trait в `lumen-layout`, `FontMeasurer<'a>` в `lumen-paint`**: layout не зависит от font напрямую; shell создаёт `FontMeasurer<'static>` из `INTER_FONT` и передаёт через `layout_measured()`. `BoxKind::InlineRun { segments, lines }` хранит строки post-wrap и per-segment стили. `layout()` без измерителя — backward compat, без переноса.
- **`InlineRun` вместо `Text`**: `BoxKind::Text` упразднён. Текстовые узлы и inline-элементы теперь всегда объединяются в `InlineRun`. Слияние фрагментов — через `ComputedStyle::text_rendering_eq` (только color/font_size/line_height), а не `PartialEq`: это нужно, чтобы `<span>` (display:inline) и соседний текстовый узел (display:block inherited) не расщеплялись в отдельные DrawText при одинаковом цвете/размере.
- **`StorageBackend` trait с origin-партиционированием:** сигнатура `get/put/delete(origin, top_level_site, key)` + `list_keys(origin, top_level_site)`. `None` и `""` — один namespace. Решение принято при первой реализации (`lumen-storage`), чтобы не переделывать сигнатуру позже.
- **Encoding detection — частотная эвристика по русским буквам**, не bi-gram / n-gram модель. Соотношение «вес = доля буквы в обычных русских текстах» по таблице из 32 строчных. Этого достаточно, чтобы уверенно различать cp1251 / KOI8-R / CP866 на тексте длиннее ~30 байт: фонетическая раскладка KOI8-R и DOS-блоки CP866 дают резко разный результат при ошибочной декодировке. Более сложные модели (CharsetDetect / chardet) — overkill для Phase 0. Если упрёмся в edge case — пересмотрим.
- **Specificity-based style cascade.** Каскад собирает все matched declarations с ключом `(specificity, rule_order, decl_index)`, сортирует по возрастанию и применяет по порядку — выигрывает максимальная specificity, при равенстве — позже объявленная (CSS spec). Specificity считается tuple `(a=ids, b=classes+attrs+pseudo, c=types+pseudoelements)`, universal и combinator не учитываются. Альтернативой был «last-rule-wins без подсчёта» — отказались, потому что для реальных CSS-стилей нужно правильное поведение `#main` против `.section`.
- **Right-to-left matching с back-tracking** для complex-селекторов. Алгоритм `matches_chain` (recursive): последний compound матчит `node`; для combinator-а, идущего перед ним, перебираются все потенциальные кандидаты — все предки для descendant, все earlier-siblings для later-sibling — и для каждого рекурсивно проверяется суффикс. Child / next-sibling combinator-ы имеют ровно одного кандидата, без перебора. Раньше был greedy без back-tracking (фиксировал первого подходящего и не пробовал остальных), что промахивалось на патологии вроде `.x + a ~ span` с несколькими `a`-siblings. Перешли на честный recursive matcher; для большинства реальных стилей время O(N×M) (N — длина селектора, M — глубина / число siblings), на патологии — экспоненциально, но реальный CSS такого не пишет. Альтернатива — Selectors-движок уровня Servo/Stylo с bloom filter — нужна только при очень больших stylesheets.
- **Interactive pseudo-classes (`:hover`, `:focus`, `:active`, …) парсятся, но всегда не матчат** в Phase 0. Хранятся как `PseudoClass::Unsupported(name)` — это не data loss, а honest «нет интерактивного состояния». Когда появится input/hover state — там и заработают. До этого правило `a:hover { color: red }` корректно парсится и просто не применяется ни к чему.
- **`NthSpec { a, b }` хранит формулу `:nth-*` как пару чисел, а не как parsed AST.** Матчинг `spec.matches(i)` решает уравнение `i = a*n + b` при `n ≥ 0` напрямую, без виртуальных «итераторов». Это даёт O(1) на проверку одного элемента и единственный путь для `odd` / `even` / целых констант / любых линейных форм. Альтернатива (хранить `Vec<i32>` индексов или callable) — overengineering для линейной формулы.
- **`:not(compound)` хранится как `Box<CompoundSelector>`**, а не как полный selector. CSS3 запрещает combinator-ы и nested `:not(:not(...))` внутри, а CSS4-расширения (`:not(complex)`, `:not(list)`) добавим позже отдельной задачей. Сейчас неподдерживаемые формы (`:not(a b)`, `:not(:not(x))`) парсер откатывает на `Unsupported("not")` — content не сохраняется, но и не теряется грамматика правила.
- **`text-decoration` наследуется через каскад в Phase 0** (CSS3 формально не наследует `text-decoration-line` — вместо этого «декорация распространяется на потомков» через box tree, что требует двух проходов layout-а). Прямое наследование даёт интуитивный результат для `a { text-decoration: underline }` и оставляет возможность сбросить декорацию у потомка через `text-decoration: none`. Подрисовка линий — приблизительной геометрией (baseline = line_y + font_size * 0.80, толщина ~7% fs, цвет = currentColor): без `post`/`OS_2` метрик шрифта точные позиции underline_position / underline_thickness нам недоступны, ratio 0.80 совпадает с ascent ratio Inter, которым рендерер позиционирует глифы.
- **`InlineFrag.width` хранится в самом фрагменте, не вычисляется заново по тексту**. Альтернатива (recompute по chars + char_width в paint) потребовала бы прокидывать `TextMeasurer` в `build_display_list`, что нарушает текущую границу «paint не зависит от font». Хранение width-а — одно поле на фрагмент, заполняется в wrap_inline_run.
- **`box-sizing` хранится в `ComputedStyle` как enum, ветвление — только в `lay_out`.** Альтернативой было передавать `BoxSizing` отдельным параметром или хранить уже посчитанные `rect.width = w` как «семантику», но смешивание моделей в одной точке (`if let Some(w) = s.width { match s.box_sizing { ... } }`) — единственный шаг, где модели расходятся, и держать его на месте чтения проще и читабельнее. `box-sizing` явно не наследуется в `compute_style` (сброс на ContentBox в каждом элементе), даже несмотря на отсутствие лишнего шага: иначе все потомки `div { box-sizing: border-box }` тихо ломали бы свой `width: 100%`. Анонимный `InlineRun` тоже сбрасывает box-sizing — у него `width` и `height` уже None, но иначе snapshot пачкается лишним полем.
- **RAWTEXT и RCDATA объединены в одном поле `text_only: Option<(String, bool)>` токенизатора.** Iterator-based архитектура у нас уже опирается только на `pos: usize` — добавлять явный `enum State` ради двух режимов — overengineering. `bool` = `decode_entities`: `false` для RAWTEXT (`<script>`/`<style>`), `true` для RCDATA (`<title>`/`<textarea>`). Различаются они только обработкой `&` в потоке, в остальном идентичны — общий контракт «литеральный текст до `</tag` + терминатор». Поле ставится в `consume_start_tag` сразу после распознавания имени тега, проверяется в `next()` первой же веткой. `.take()` гарантирует, что режим всегда сбрасывается за один проход — даже если в теле не было `</tag` (тогда читаем до EOF и возвращаемся в обычное русло). `is_raw_text_element(name)` и `is_rcdata_element(name)` — две отдельных функции, легко расширяемые для будущих специальных контентов (iframe, noembed и т.д.). Self-closing `<script/>` / `<title/>` режим **не включают**: симметрично с tree_builder, который для self-closing не пушит элемент в стек. Альтернатива (отдельные поля `raw_text: Option<String>` + `rcdata: Option<String>`) — две почти идентичные ветки в `next()`, лишняя дупликация. Альтернатива (`enum TextMode { RawText, RcData }` + одно поле) — синтаксически чище, но bool в текущем размере (2 значения) экономит typedef и не теряет ясности при правильном комментарии.
- **`:is(list)` и `:where(list)` хранятся как `Vec<ComplexSelector>`** (в отличие от `:not(compound)` — там запрещены combinator-ы по CSS3, поэтому достаточно `Box<CompoundSelector>`). CSS4 явно разрешает combinator-ы внутри `:is`/`:where`, поэтому хранилище — полный selector list. Matcher — `list.iter().any(...)` рекурсивно через `matches_complex`; это естественно корректно из коробки. Specificity: `:is` contributes максимальную specificity по списку (CSS4 §17 «specificity of an :is/:not/:has is the specificity of the most specific selector in its arg»), `:where` — всегда 0. Для max-вычисления добавлена функция `max_list_specificity(&[ComplexSelector]) -> Option<Specificity>` рядом с `accumulate_specificity`. Парсер: внутри тела `:is(...)` зовём существующий `parse_selector_list`; чтобы он корректно останавливался на `)`, в `parse_complex_selector` добавлено `)` в список break-токенов tail-цикла. Пустые `:is()` / `:where()` возвращают `Unsupported(name)` — это даёт ту же fallback-семантику, что у `:not(a b)`.
- **`!important` отделяется от value на уровне парсера, хранится булевым полем в `Declaration`.** Альтернатива (хранить `!important` как часть строки `value` и парсить заново при apply) была раньше — теперь убрана. Причина: `apply_declaration` имеет 30+ свойств, каждое со своим парсингом — добавлять везде стрипа `!important` дороже и легче пропустить. Извлечение делается функцией `extract_important(&str) -> (String, bool)`, которая работает с уже trim-нутой строкой и проверяет суффикс через `eq_ignore_ascii_case` на байтах. Cascade использует ключ `(important, specificity, rule_order, decl_index)` — `important` идёт первым, потому что в Rust `true > false`, и ascending sort ставит !important в конец, давая ему победу. Этот же ключ корректно обрабатывает все остальные правила каскада (between two !important winning by specificity, then later-wins-on-tie). UA / user origin не реализованы (только author), поэтому не требуется отдельная иерархия origin.
- **Ollama-backend (опциональный AI в Phase 3+) — НЕ exception #5.** Ollama-протокол это просто HTTP к `localhost`. `lumen-network::HttpClient` это уже умеет через exception #3 (rustls). Никакой новой зависимости не нужно — это просто использование разрешённого транспорта. Только встраивание `llama.cpp` через FFI стало бы exception #5; от этого варианта на данный момент отказываемся в пользу Ollama HTTP. Это уточняет открытый вопрос «AI backend» — путь через Ollama бесплатен политически.
- **Persistent storage — SQLite (exception #5), всё хранение данных через готовые решения.** Прежнее решение «пишем свой B-tree KV» отменено: пользователь явно расширил политику — **всё, что связано с хранением данных и информации, сами не пишем, берём готовые решения** (см. project-db-for-history в memory). Сюда входит history, bookmarks, notes, read-later, cookie jar с TTL, профили, knowledge layer FTS, HTTP-кэш ресурсов, IndexedDB-эквивалент. Сужение: это **не** распространяется на decoder-ы streaming-форматов (PNG inflate, TTF parse, HTML/CSS) и in-memory pipeline-структуры (DOM arena, layout tree) — это парсеры и transient state, не storage. SQLite через `rusqlite` с feature `bundled` — компилируется в бинарь, без runtime libsqlite3. Почему именно SQLite: (1) стандарт в индустрии браузеров (Firefox places.sqlite, Chromium History, Safari) — 25 лет audit-а, TH3-тестирование; (2) встроенный FTS5 закрывает §12.1 полнотекстовый поиск без своего inverted index — это десятки KLOC, которые не нужно писать и поддерживать; (3) принцип «свой crypto никто не пишет» (rustls exception) распространяется на long-lived persistent данные пользователя — зрелая БД даёт ту же гарантию против data-loss / corruption инцидентов, что rustls против crypto-багов. Текущий `InMemoryStorage` (`LUMEN_KV_V1` text snapshot) остаётся для тестов и ephemeral session-scope данных. `SqliteStorage` реализует тот же `StorageBackend` trait для disk-persistent path. Альтернативы (`redb` — чистый Rust embedded KV без FFI; `sled` — LSM) — оставлены в плане как fallback на случай отказа от FFI, но тогда придётся писать свой FTS, что нарушает расширенную политику.
- **Punycode-конвертация инкапсулирована в `Url::host_ascii()`, не делается прямо в `Url::parse`.** Хранилище host в `Url` остаётся Unicode (для отображения в адресной строке), а конвертация в `xn--…` — отдельный метод, который дёргает только тот, кому нужна ASCII-форма: `lumen-network` для DNS lookup, TLS SNI (`ServerName::try_from`) и Host header (RFC 7230 §5.4, RFC 6066 §3). Альтернатива (конвертить host в ASCII прямо в `Url::parse`) ломала бы: (1) Unicode-форму для адресной строки; (2) семантику data:/file://, где Punycode не применим. Старый дизайн «`parse_url` в `lumen-network` парсит свою копию URL и конвертит host через idn» теперь убран: единственный URL-парсер живёт в `lumen_core::url::Url`, потребители обращаются через `host_ascii()`. Без NFC normalization и UTS #46: для русских доменов (NFC-стабильная кириллица) практически достаточно `str::to_lowercase`. Если упрёмся в edge case (например, ß или ZWJ) — добавим mapping table.
- **UTF-16: голый label `utf-16` мапится на LE, не на BE.** Это WHATWG Encoding Standard §4.2: алиасы `utf-16`, `unicode`, `ucs-2` — все на UTF-16 LE, потому что подавляющее большинство «UTF-16»-файлов в природе — это `Save As → Unicode` из Windows Notepad, который пишет LE. UTF-16 BE достижим только через явный `utf-16be`. Это противоречит здравому смыслу (BE — natural byte order для big-endian network), но менять — значит ломать существующий веб. Декодер же спокойно снимает BOM любого endian-а в начале потока: если detect отдал Utf16Le, а реальный BOM `FE FF` — мы корректно его обрезаем (детектор бы выбрал Utf16Be, но defensive coding). Lone surrogates и нечётное число байт превращаются в U+FFFD — invalid-on-input, никаких panic-ов. Surrogate-пары обрабатываются вручную: `0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)`; не используем `char::decode_utf16` из std, потому что нам нужна полная управляемость над non-strict обработкой (std возвращает Result-итератор, лень обёртывать ради того же результата).
- **`calc()` хранится как AST (`Box<CalcNode>`) внутри `Length`, не предварительно резолвится.** Резолв требует `em_basis`, `percent_basis`, `viewport` — все известны только в момент применения декларации к конкретному элементу (em и % зависят от позиции в дереве). Альтернатива «парсить calc → сразу `Length::Px(f32)`» возможна для чистых px-выражений (`calc(10px + 5px)` = `15px`), но требовала бы анализа «содержит ли выражение em/%/vh/vw» и разветвления; AST даёт одну однородную точку. **`Length::Calc(Box<CalcNode>)`, не `Calc(CalcNode)`:** убирает Length из Copy (Box не Copy), но cascade-код не дёргает Length-копии в hot-path — все 78+ usage-сайтов либо match по варианту с десоставлением f32, либо вызов `.resolve(...)`. Стоимость `Box::new` амортизирована: на page без calc() аллокации нет вообще; на странице с десятком calc() — 10 boxes, по 24 байта каждый = 240 байт, мизерно. **Recursive-descent парсер, не shunting-yard:** грамматика двухуровневая (term/factor) с фиксированными приоритетами `*//` > `+-`, нет дополнительных операторов; recursive descent читается напрямую как грамматика, shunting yard потребовал бы стека операторов и стека операндов — больше кода и меньше явности. Унарный минус через `factor := '-' factor | ...` представляется как `Sub(Number(0), x)` (а не отдельный вариант `Neg(x)`), потому что `resolve` для `Sub` уже корректно обрабатывает: `0 - x`. **Унарный `-` решается на этапе парсинга, не лексинга:** альтернатива «токенайзер делает Num(-5, "px") если `-` перед числом» порвалась на `10px - 5px` (whitespace-разделение): `-` после `10px` шло перед `5px`, токенайзер бы решил «знак» вместо «бинарный минус» и потерял бы оператор. Парсер видит контекст (что слева от `-` уже разобран term), и однозначно: если `-` в позиции factor — унарный, иначе — бинарный из expr. **Деление на 0 → `None` (declaration invalid), не `f32::INFINITY`.** CSS spec не определяет — оба варианта в дикой природе встречаются. None даёт «декларация игнорится, наследованное значение остаётся», что безопаснее, чем INF, который сломает layout. **`line-height: calc(...)` использует общий resolve→делим на font_size:** для чистого calc-числа (`calc(1 + 0.5)` = 1.5) это даёт неверный результат `1.5 / fs`. Отличить «unitless итог» от «length итог» можно было бы через возврат типизированного `ResolvedLength { px: f32, was_unitless: bool }`, но это инвазивная правка всего `resolve` API ради одного edge case. В Phase 0 — известное ограничение: для коэффициента используйте bare-form `line-height: 1.5`.
- **CSS Variables L1 — substitution на этапе layout, не парсера.** `--name: value` declarations и `var(--name [, fallback])` references парсятся css-parser-ом естественно, без специальной грамматики (value читается до `;`/`}` с уважением к строкам и скобкам; `--main-color` — валидный ident, потому что `is_ident_start` уже допускает `-`). Семантика — резолв custom property cascade-ом и substitution `var()` в значениях — целиком в `lumen-layout::compute_style`. Альтернатива (парсить `var(...)` в css-parser в типизированный AST с placeholder-ами) была отвергнута: (1) css-parser намеренно держит values сырыми строками — типизация декларации это работа layout-а (см. existing decision «типизированные значения отложены»); (2) парсинг `var()` без custom_props контекста бесполезен — всё равно substitution делается в момент применения; (3) текущий подход — одна точка изменения (apply_declaration в начале) добавляет поддержку для всех 30+ свойств сразу. **Three-pass cascade в `compute_style`:** font-size pre-pass (фиксирует em-basis) → custom-props pass (заполняет `style.custom_props` по cascade-порядку) → main-pass (применяет обычные свойства; `var()` в значениях разворачивается через `expand_vars` с уже готовым `style.custom_props`). Custom-pass обязательно отдельный, иначе `color: var(--c); --c: red` (в одном правиле) дал бы default цвет, потому что main-pass идёт в порядке declaration-индекса. **`expand_vars` рекурсивный**, depth limit 32: разворачивает `var()` ⇒ resolved (custom_props.get или fallback) ⇒ может содержать ещё `var()`, расширяется до фиксированной точки. Циклы вида `--a: var(--b); --b: var(--a)` ловятся depth limit-ом и declaration treated as invalid (CSS Variables L1 §3.3). **`Option<String>` возврат, не Cow-ish заморочка:** allocation amortized — большинство значений не содержит `var(`, в этом случае ранний `if !contains("var(")` минует expand вообще; для значений с var(), мы и так делаем substitution, format! не дороже остального. **Custom properties как `HashMap<String, String>` в `ComputedStyle`, не `Arc<HashMap>`:** ComputedStyle уже клонируется без оптимизаций (font_family: Vec, text_shadow: Vec, box_shadow: Vec), HashMap.clone() O(n) в общем потоке размытия. Когда производительность станет приоритетом — переедем на `Arc<HashMap>` + `Arc::make_mut` для copy-on-write. **`!important` для custom properties работает само собой**, потому что custom-pass идёт по уже отсортированному `matched`-списку — `(important, specificity, rule_order, decl_index)` ставит important-декларации в конец, и они побеждают позже-применением.
- **`RequestFilter` — отдельный trait от `FilterListSource`, а не метод на нём.** `FilterListSource` это «загрузчик правил» (`fetch_rules() -> String` — текст в формате EasyList/uBlock); `RequestFilter` это «применитель» (`should_block(&Url) -> Option<String>`). Они живут в разных слоях: `HttpClient` зависит ТОЛЬКО от `RequestFilter` и ничего не знает о формате правил; типичная цепочка `FilterListSource → парсер правил → RequestFilter`. Альтернатива (один trait `FilterListSource` с обоими методами и дефолтным `should_block` → None) была бы короче, но смешивает «как достать правила» и «как их применить» — два разных subsystem-а с разной частотой использования (rules-fetch — один раз при старте, may be background; should_block — на каждый исходящий запрос). Альтернатива «boxed closure `Fn(&Url) -> Option<String>`» отбрасывает паттерн ext-traits и теряет имя реализации (полезно для debug-вывода `name()`). **Sink-ы плагинов и filter-ы плагинов — это две независимые подсистемы**: одна про «сообщать о», другая про «решать на». Их и трактуем независимо.
- **`RequestBlocked` эмитим ДО `RequestStarted`, не вместо одного из них.** Альтернативой было эмитить `RequestStarted` + `RequestCompleted{status: 0}` или special status code для блока. Отказались: «started» это обещание «отправил байт» (или скоро отправит), которое для блокированного запроса ложно — TCP-соединение не открывалось вообще. Семантика принципа №4 — «каждый исходящий байт виден»; блок означает «байт НЕ исходил, и вот почему». Это качественно другая категория события. Поэтому: на bad-scheme — ничего (URL невалиден); на блок — `RequestBlocked` и больше ничего; на разрешённый запрос — `RequestStarted` → … → `RequestCompleted`. Каждый redirect-hop фильтруется независимо: hop1 разрешён → Started+Completed(302); hop2 блок → RequestBlocked, fetch возвращает Err. Это правильно потому, что цепочка редиректов это серия «outgoing-byte событий», и блокировку трекера на 3-м шаге надо видеть как блок именно на 3-м URL, не размазывать на всю цепочку.
- **HTTP/1.1 keep-alive: stale-detection через retry-on-error, а не active health-check.** Альтернативой было перед каждым `acquire` делать `set_nonblocking(true) + read([0u8; 0])` для проверки, что TCP сокет ещё жив (peek-style). Отказались по двум причинам: (1) на TLS-потоке это нетривиально — `rustls::StreamOwned::read` пропустит свой TLS-уровень, и `WouldBlock` ничего не говорит о реальном TCP-состоянии (TLS-буфер может быть пустой при живом сокете); надо лезть в `TcpStream` через приватный доступ, не предусмотренный API rustls. (2) В типичном случае keep-alive **работает** (сервер не закрыл) — health-check тратит syscall на каждый fetch впустую, retry-on-stale платит только когда уже плохо. Поэтому: pooled connection используется «оптимистично», и если первый write/read падает с `BrokenPipe` / `ConnectionReset` / `UnexpectedEof` (or нашими «EOF before status line» / «EOF in headers» — эти возникают, когда server сделал shutdown между нашими write_request и first read_line) — fetch однократно retry-ит на свежем connect-е. `is_stale_error` распознаёт ошибки по содержимому `Debug`-сообщения `io::Error` (`format!("{err:?}")` содержит kind в виде `BrokenPipe`/`ConnectionReset`/...) — менее элегантно, чем `e.kind() == ErrorKind::BrokenPipe`, но `Error::Network(String)` уже потерял `io::Error` на пути вверх; рефакторить error-type ради этого — больше шума, чем пользы. **`IDLE_TIMEOUT = 30 секунд`** — короче среднего серверного keep-alive (Apache default 5 c, nginx 75 c) с запасом на сетевую задержку; entries старше — drop на acquire, retry-on-stale страхует от случаев «закрыли раньше». **`MAX_IDLE_PER_HOST = 6`** — соответствует браузерному дефолту параллельных коннектов; для последовательного клиента с запасом, но защищает от деградации, если кто-то будет сыпать запросы. **Общий cap на пул не делается** — в Phase 0 (один процесс, последовательный fetch, единицы origin-ов) не нужен. **`Connection` хранит `BufReader<RawStream>` постоянно, а не пересоздаёт на каждый запрос:** raw `read_response` ради простоты раньше делал `BufReader::new(conn)`, который consume-ил `Connection` и выбрасывал в drop вместе со всеми невычитанными байтами в буфере. Для keep-alive это смертельно — между запросами нельзя терять накопленные `chunked`-байты или partial-headers. Сейчас `BufReader` живёт всё время жизни `Connection`; для write используется `reader.get_mut()` → `&mut RawStream` (поскольку BufReader.get_mut выдаёт mutable доступ к underlying R, который у нас Write). **`read_chunked` дочитывает trailer-секцию:** RFC 7230 §4.1 — после `last-chunk` (= "0\r\n") идёт `trailer-part = *( header-field CRLF )` + final CRLF. Раньше мы останавливались сразу на `0\r\n`, оставляя финальный `\r\n` (а возможно и trailer-headers) в BufReader. С `Connection: close` это никого не волновало — drop. Для keep-alive — следующий `read_line` для status-line прочитал бы `\r\n` (пустую строку) и упал бы на `parse_status` или, если был trailer-header, прочитал бы `X-Trailer: foo` как `HTTP/1.1 200 OK`. Нашёл это, написав снапшот-тест `chunked_consumes_trailer_section` который сразу красным горел до исправления.
- **`EventSink::emit(&self, &Event)` — `&self`, не `&mut self`, и `&Event`, не `Event` по значению.** `&self` потому, что типичная реализация хранит state под `Mutex` / atomic / channel-sender и должна быть shared между потоками (фоновая загрузка favicon + main thread, network-log UI thread); `&mut self` навязал бы один обладатель и потребовал бы `Arc<Mutex<dyn EventSink>>` у каждого вызывающего, что превратило бы emit в hot-path с двойной indirection. `&Event` потому, что caller (network) обычно не нуждается в событии после emit (он отбрасывает значение), а sink сам решает: счётчик может извлечь нужное поле без clone, collector — клонировать. Платить `event.clone()` на стороне caller-а при том, что половина sink-ов его выкинет — антипаттерн. Альтернатива (`emit_owned(Event)`) — добавим параллельно, если найдётся sink, которому zero-copy ownership критичен; пока такого случая нет. **`Option<Arc<dyn EventSink>>` в `HttpClient`, не `Arc<NoopEventSink>` по умолчанию.** Дефолтный путь «события никому не нужны» — самый горячий (тесты, batch-fetch без UI); каждое лишнее `noop.emit(&...)` это виртуальный вызов через trait object и lock-acquire в типичной реализации. `if let Some(s) = sink` обходится в одну ветку без indirection. Hot-path remains zero-cost. **Sink дёргается ПОСЛЕ валидации, но ДО I/O.** `RequestStarted` эмитим после `parse_url` (bad scheme не эмитит — байт даже не подумал улетать), но до `TcpStream::connect` (иначе observer не узнает о попытке соединения, если DNS упал). `RequestCompleted` — после status line, до анализа кода: 4xx — это всё ещё «outgoing byte получил response», observer должен это видеть, а fetch вернёт Err. Каждый redirect-hop генерит свою пару — `fetch_with_redirect` рекурсивно зовёт сам себя с уменьшенным `hops_left`, и каждый виток эмитит на своём URL.

### Открытые вопросы (решим, когда упрёмся)

- **Shaping для сложных скриптов** (Arabic/Indic/Thai): свой shaper за месяцы или rustybuzz как 5-е exception. Откладываем до Phase 2-3.
- **iOS:** Apple-policy требует WebKit на iPhone/iPad. Это противоречит принципу собственного движка. Возможный путь — тонкий shell поверх WKWebView только для iOS, остальные ОС — наш движок. Откладываем до Phase 4.

### Намеренно отвергнутые альтернативы

- **WebView2 / wry / CEF обёртка** — это другой проект, не Lumen. Отказались.
- **html5ever / cssparser / taffy / image / encoding_rs / ttf-parser / rustybuzz / tokio / rayon / redb / egui** — все эти crates рассмотрены и **не взяты**: для них пишется свой код по принципу «default — своё». См. зачёркнутый список в §5.
- **`image` / `png` / `flate2` / `miniz_oxide` для PNG-декодера — отвергнуты.** PNG это compression engine + image format parser — оба классически в зоне «default — своё», аналогично собственному TTF-parser-у в `lumen-font`. DEFLATE/inflate (RFC 1951) укладывается в ~500 LOC, zlib-обёртка + adler-32 — ещё ~100. Свой код даёт полный контроль над allocations (нет hidden buffer pools), детерминированный fail-mode (любая ошибка — `InflateError` с конкретной причиной), и снимает supply-chain риск (PNG-декодер — частая точка CVE; см. CVE-2015-8126 в libpng). Свой PNG также даёт честный baseline для сравнения с эталонными декодерами (нужно для §16 целей по cold start / memory). Этот аргумент о «политической дороговизне нового exception» был частично отменён при принятии SQLite как #5 — но для PNG/JPEG специфика (зона compression engine + supply-chain riskful image parser) сохраняется в силе.

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
- **Font fallback отсутствует — рендерер всегда `Inter Regular`.** `font-family` в CSS парсится, в paint игнорируется. Реальная страница с эмодзи / CJK / `font-family: Roboto` отрисуется в `?`-глифы (для не-Latin / не-кириллицы) или fallback на Inter (для имён, которых Inter не содержит). Блокер для любой Phase 1 демонстрации. См. roadmap «Ближайшее» п.2.
- **HiDPI / DPR не учитывается.** `winit` отдаёт `scale_factor`, в layout/paint не прокинут. На 4K мониторе всё отрисуется в реальный 0.5× от ожидаемого. См. roadmap «Ближайшее» п.4.
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
