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

**Если в начале сессии пользователь говорит «ты программист N» — найди свою колонку ниже, и бери задачи с маркером `[PN]` из раздела «Roadmap — что предстоит реализовать» этого файла. Если все задачи с твоим маркером взяты другими сессиями (видно через `git branch` + блок «🔄 В работе сейчас» в `lumen-plan.md`) — спроси пользователя, какую следующую брать.**

| Программист | Доменная зона | Основные крейты |
|---|---|---|
| **P1** | Парсинг + каскад + layout | `lumen-html-parser`, `lumen-css-parser`, `lumen-layout` |
| **P2** | Шрифты, растровая графика, изображения | `lumen-font`, `lumen-paint`, будущий `lumen-image` |
| **P3** | Сеть, хранилище, knowledge layer, crypto | `lumen-network`, `lumen-storage`, будущий `lumen-knowledge`, `lumen-core::ext` |
| **P4** | Shell, окно, JS-движок, AI, UI-фичи | `lumen-shell`, JS integration (`rquickjs`/`rusty_v8`), будущий `lumen-ai`, UI features |

### Правила взаимодействия

- **Crate ownership.** Если ты P1 — не лезешь в `lumen-paint` без согласования с P2; если P3 — не правишь layout без согласования с P1. Это снижает merge-конфликты, а не запрещает ревью.
- **`lumen-core` — общая поверхность.** Trait-ы в `lumen-core::ext` правит обычно P3 (Network/Storage/EventSink/Url), но если P2 нужен новый `FontProvider` trait — добавляет сам, не блокируясь на P3. Coordination через коммит-сообщение.
- **`lumen-shell` — у P4.** Каждая новая capability у других программистов завершается тем, что P4 интегрирует её в shell отдельной задачей. Не интегрируешь сам, если ты не P4 — описываешь интеграционную точку в commit-body, P4 поднимет.
- **Точки расширения добавляет тот, кому нужно.** Не блокируй другую сессию на «P3 ещё не добавил trait» — добавь trait сам, P3 ревьюит post-factum.

### Как зарезервировать задачу под себя

Стандартный протокол из раздела «Координация параллельных сессий»: создаёшь feature-ветку (`git checkout -b <имя>`) → первым же коммитом добавляешь строку в блок «🔄 В работе сейчас» в `lumen-plan.md` в формате:

```
- 🔄 <имя задачи> [PN] — <имя ветки> — <YYYY-MM-DD>
```

`[PN]` в строке — чтобы другие сессии видели, кто чем занят, и не дублировались.

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

На момент написания: 838 тестов, 13 крейтов (`shell`, `core`, `network`, `storage`, `bench`, `dom`, `html-parser`, `css-parser`, `layout`, `paint`, `font`, `encoding`, `image`). При прохождении следующих фаз появятся `lumen-knowledge`, `lumen-ai` и др.

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
- `lumen-core::ext` — trait-точки расширения: `NetworkTransport`, `StorageBackend` (с origin-партиционированием + `list_keys`), `SearchProvider`, `FilterListSource`, `EncodingDetector`, **`EventSink`** (`fn emit(&self, event: &Event)`, `Send + Sync`, sink принимает `&Event` без `clone`; «молчаливого дефолта» нет — потребители держат `Option<Arc<dyn EventSink>>` и `None` означает «события никому не нужны»).
- В комментариях задокументированы будущие trait-точки: `WindowingBackend`, `RenderBackend`, `TlsBackend`, `JsRuntime`, `FontProvider`, `HyphenationEngine`, `DnsResolver`, `Hasher`. Тело trait-а добавим при первой реализации.
- **`lumen_core::punycode::encode`** — RFC 3492 Punycode encode (bootstring base=36, tmin=1, tmax=26, skew=38, damp=700, initial_bias=72, initial_n=128). 8 unit-тестов на известных IDN-метках (пример → e1afmkfd, рф → p1ai, президент → d1abbgf6aiiy, тест → e1aybc, рус → p1acf, CJK 你好 → 6qq79v).
- **`lumen_core::idn::domain_to_ascii`** — IDNA `ToASCII` (упрощённое подмножество): lowercase → split по `.` → label-by-label (ASCII passthrough, иначе `xn--<punycode>`). Не делает NFC normalization и UTS #46 mapping (для русских доменов уже в NFC — практически достаточно). 10 unit-тестов (включая идемпотентность для уже `xn--…`, mixed ASCII+IDN субдоменов, trailing dot для FQDN, case-нормализация).
- 22 теста (url parsing + Punycode + IDN).

### `lumen-dom` ✅ (полный API на текущий scope)

- Arena-based: `Vec<Node>` + `NodeId(u32)`. Нет `Rc/RefCell`, нет циклов.
- Типы: `Document`, `Node` (parent + children + data), `NodeData` (Document / Doctype / Element / Text / Comment), `QualName`, `Namespace` (HTML/SVG/MathML/Xml/XmlNs/XLink), `Attribute`.
- API: `create_element / create_text / create_comment / create_doctype`, `append_child`, `detach`, `get / get_mut`, `root`, `len`.
- `Display` impl печатает дерево с отступами — для отладки.
- 7 тестов, включая cyrillic-инварианты.

### `lumen-html-parser` 🟡 (минимум)

- **Готово:** iterator-based FSM (Tokenizer); состояния Data, TagOpen, TagName, EndTag, BeforeAttributeName, AttributeName, AfterAttributeName, BeforeAttributeValue, AttributeValue (quoted/unquoted), SelfClosingStartTag, MarkupDeclarationOpen, Comment, CommentEnd, **RAWTEXT** (для `<script>` и `<style>`), **RCDATA** (для `<title>` и `<textarea>`), **DOCTYPE**. Character references: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, numeric `&#NNN;` / `&#xHHHH;`. Lenient tree builder с void-элементами и self-closing.
- **RAWTEXT / RCDATA детали:** объединённое поле `text_only: Option<(String, bool)>` — `bool` = `decode_entities`. После `<script>`/`<style>` (не self-closing) выставляется `(name, false)` — RAWTEXT, character references литеральны. После `<title>`/`<textarea>` — `(name, true)` — RCDATA, entities декодируются (нужно для `<title>Foo &amp; Bar</title>` → `Foo & Bar`). В обоих режимах `<` без `/tag` — текст; завершение только `</tag` + терминатор (whitespace / `/` / `>` / EOF), case-insensitive. `</scripto>` НЕ матчит (`o` не терминатор). Self-closing `<script/>` / `<title/>` режим **не** включают (симметрично tree_builder, который для self-closing не пушит элемент в стек). `is_raw_text_element(name)` и `is_rcdata_element(name)` — две отдельных функции; расширяемо для будущих специальных контентов (`iframe`, `noembed`, `noframes`, `noscript`, `plaintext`).
- **DOCTYPE детали:** `Token::Doctype { name, public_id, system_id }` (HTML5 §13.2.5.53–72). После `<!DOCTYPE` keyword (case-insensitive `doctype`/`DOCTYPE`) парсятся: name (lower-case, до whitespace или `>`), опционально `PUBLIC "id" "id"` или `SYSTEM "id"` с поддержкой одинарных и двойных кавычек. Lenient: `<!DOCTYPE>` без имени даёт пустой name, неполные DOCTYPE-ы не валятся. Tree builder создаёт `NodeData::Doctype` узел (раньше токен пропускался). Прочие markup declarations типа `<![CDATA[...]]>` или `<!ENTITY ...>` по-прежнему молча skip-аются до `>`.
- **Отложено:** CDATA, полный набор named character references (~2000 в HTML5 spec), insertion modes (in_table, in_select, in_caption, и т.д.), `<iframe>` / `<noembed>` / `<noframes>` / `<noscript>` / `<plaintext>` text modes (нужны при первой реальной странице).
- 74 теста (Tokenizer + tree_builder, включая 15 RCDATA + 2 интеграционных).

### `lumen-css-parser` 🟡 (полный набор CSS3-селекторов)

- **Готово:** `selector_list { decl_list }` парсинг. Selectors Level 3: simple selectors (`Type`, `Class`, `Id`, `Universal`, `Attribute`, `PseudoClass`, `PseudoElement`), compound (`p.foo#bar:first-child`), complex с combinator-ами (descendant ` `, child `>`, next-sibling `+`, later-sibling `~`). Attribute-операторы: `=`, `~=`, `|=` (для `lang`), `^=`, `$=`, `*=`. **Case-insensitive флаг `[attr=val i]`** (CSS Selectors L4 §6.3.6) — после value распознаётся `i` / `I` (ASCII case-insensitive) или `s` / `S` (явно case-sensitive, default); хранится в `AttrSelector.case_insensitive: bool`; применим ко всем шести операторам. **Structural pseudo:** `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`. **Функциональные pseudo:** `:nth-child(an+b)`, `:nth-last-child(an+b)`, `:nth-of-type(an+b)`, `:nth-last-of-type(an+b)` с ключевыми словами `odd` / `even` (хранятся как `NthSpec { a, b }`, `.matches(index)` решает уравнение `i = a*n + b` при `n ≥ 0`). `:not(compound)` — отрицание; запрещены combinator-ы внутри и nested `:not(:not(...))`, такие формы дают `Unsupported`. **CSS4 `:is(selector-list)` и `:where(selector-list)`** — матчат, если матчит хоть один из селекторов; внутри разрешены любые complex-селекторы (combinator-ы, attribute, structural pseudo). Пустой список `:is()` / `:where()` → `Unsupported(name)`. Interactive (`:hover`, `:focus`, …) сохраняются как `Unsupported(name)` и при матчинге всегда возвращают false. Pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат. **Specificity** по CSS3 §16 / CSS4 §17: `:not` сам не считается, contributes specificity внутреннего compound. `:is(...)` сам не считается, contributes максимум specificity по списку. `:where(...)` всегда 0. Прочие pseudo-classes считаются как class. Декларации — как пары строк. Lenient recovery, комментарии `/* */`, пропуск `@`-правил. Парсер `parse_complex_selector` прерывается также на `)`, чтобы корректно работать внутри функциональных pseudo.
- **`!important` флаг (CSS Cascade L4 §8.1):** парсер `extract_important` отделяет `!important` от value (с опциональным whitespace между `!` и словом, ASCII case-insensitive). Не трогает `!important` внутри строковых литералов: `content: "!important"` остаётся value=`"!important"`, important=false. Хранится в `Declaration.important: bool`.
- **CSS Variables L1 (`--name` declarations):** парсер не требует никакой специальной грамматики — `--main-color: red;` это обычная декларация, поскольку `is_ident_start` уже допускает `-` (а `--` — это два валидных ident-символа подряд). Value читается `parse_value_until_terminator` до `;`/`}` с уважением к строковым литералам, что естественно покрывает `var(--c, fallback)` с запятыми внутри и `rgba(0, 0, 0, 0.5)` в fallback. Substitution `var()` делается уже в layout (см. `lumen-layout`). `!important` для custom properties работает через тот же `extract_important`.
- **Отложено:** `:not(complex)` со списком селекторов или combinator-ами, namespace prefix в селекторах, типизированные значения деклараций (length / color / calc — типы хранятся в layout, не в parser).
- 99 тестов.

### `lumen-layout` 🟡 (block + inline-flow + word-wrap + cascade)

- **Готово:** `LayoutBox` дерево, **specificity-based style cascade с !important**: для каждого правила берётся максимальная specificity среди его complex-селекторов, все matched declarations сортируются по `(important, specificity, rule_order, decl_index)` и применяются по возрастанию. `important` идёт первым в ключе — `true > false` ставит !important-декларации в конец, и они выигрывают у normal даже при меньшей specificity (CSS Cascade L4 §8.1). При равенстве — позже объявленная. **Matching complex selector-а — справа налево с back-tracking** (`matches_chain`): для descendant и later-sibling combinator-ов перебираются ВСЕ кандидаты (предки / earlier-siblings) и рекурсивно проверяется суффикс — это корректно решает патологические случаи вроде `.x + a ~ span` с несколькими `a`-siblings без класса `.x`. Для child / next-sibling — один кандидат, без back-tracking. Combinator-ы: descendant / child / next-sibling / later-sibling. **Pseudo-classes:** все CSS3 structural и functional — `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`, `:nth-child(an+b)`, `:nth-last-child`, `:nth-of-type`, `:nth-last-of-type`, `:not(compound)`. **CSS4 `:has(rs-list)`** через `matches_relative` — для implicit descendant / `>` / `+` / `~` ищется кандидат в подходящем месте дерева; сам элемент `E` не проверяется (descendants only). Helpers `element_index` (1-based, среди element-sibling-ов) и `element_index_of_type` (среди sibling-ов с тем же тегом) с `from_end` опцией. Attribute selectors: все операторы, с поддержкой ASCII case-insensitive флага (`[attr=val i]`) — сравнение через `eq_ignore_ascii_case` на байтах, чтобы не упираться в char-boundary в UTF-8 строках; не-ASCII (cyrillic) сравнивается побайтово. Наследуемые свойства: `color`, `font-size`, `line-height`, `font-style`, `font-weight`, `font-family`, `text-transform`, `text-align`, `text-decoration-line`. **`FontStyle` (Normal / Italic / Oblique)** через `font-style` property + UA stylesheet для семантических тегов (`<em>`, `<i>`, `<cite>`, `<dfn>`, `<address>`, `<var>` → italic); **`FontWeight` (1..1000, normal=400 / bold=700)** через `font-weight` property с поддержкой keyword-ов (`normal`, `bold`, относительных `lighter`/`bolder` по таблице CSS Fonts L4 §2.4.3) и числовой шкалы; UA stylesheet для `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>` → bold. **`FontStretch` (CSS Fonts L4 §2.5)** — хранится в десятых долях процента (`FontStretch(1000)` = 100%); 9 keyword-ов (`ultra-condensed` … `ultra-expanded` с дробными `semi-condensed` = 87.5%, `semi-expanded` = 112.5%); численные `%` парсятся и клампятся в [50%, 200%]; inherited; real stretch-варианты требуют variable-font wdth-axis или отдельные fontfiles, в Phase 0 рендерер всегда Inter Regular. `text_rendering_eq` сравнивает font_style, font_weight, font_variant и font_stretch, чтобы italic/bold/small-caps/stretched-фрагменты не сливались с обычными. Реальная отрисовка italic / bold вариантов в paint пока не реализована (нужны Italic / Bold fontfiles или affine skew transform / faux-bold), но layout уже различает. **`font-family`** — `Vec<String>` приоритизированного списка, парсер поддерживает quoted (`"Times New Roman"` / `'Open Sans'`) и unquoted multiword имена со схлопыванием whitespace; generic-family (`serif`, `sans-serif`, `monospace`, …) хранятся как обычные строки. Inherited. Phase 0 рендерер всегда Inter — задел под будущий font matcher. **`TextTransform` (None / Uppercase / Lowercase / Capitalize)** — `text-transform: …` применяется к `InlineSegment.text` при сборке (до wrapping и measurer), используя `char::to_uppercase`/`to_lowercase` стандартной библиотеки — корректно работает для кириллицы. `capitalize` — упрощённо первая буква каждого whitespace-разделённого токена. **`text-indent` (resolved px)** — отступ перед первой строкой inline-content (CSS Text L3 §7.1), inherited; `wrap_inline_run` стартует `current_x = text_indent` для первой строки, последующие — с 0. Поддерживает px/em/rem/vh/vw; `%` пока игнорируется (нужен containing-block-width). **`white-space` (Normal / Nowrap)** — `Nowrap` отключает word-wrap (передаём `f32::INFINITY` как `wrap_width`); `Normal` — обычный greedy wrap; pre/pre-wrap/pre-line отложены (нужен preserved whitespace в input). Inherited. **`opacity` (0..1)** — CSS Color L3 §3.2, не наследуется; парсер принимает число и проценты, clamp вне диапазона; в layout только хранится — реальный alpha-blending paint-уровня — отдельная задача. **`outline` (`outline-width` / `-style` / `-color` + shorthand + `outline-offset`)** — CSS UI L4 §3, не наследуется; **не занимает места в коробке** (в отличие от border) — `rect.width`/`height` неизменны, отрисовка позже как «слой» поверх / снаружи; цвет `None` = currentColor; offset поддерживает отрицательные значения (рисует внутрь). **`visibility` (Visible / Hidden / Collapse)** — CSS Display L3 §4, **inherited** (отличается от display); `Hidden` оставляет коробку в layout (высота сохраняется), но не рисуется — потомок может явно вернуть себя через `visibility: visible`. **`overflow` / `overflow-x` / `overflow-y`** (Visible / Hidden / Clip / Scroll / Auto) — CSS Overflow L3, не наследуется; shorthand принимает 1 или 2 значения; реальный clipping / scrollbars в paint pipeline пока нет. **`text-overflow`** (Clip / Ellipsis) — CSS UI L4 §10.1, не наследуется; работает только в комбинации с overflow != Visible + nowrap; real truncation в paint отложен. **`cursor`** — CSS UI L4 §8.1, inherited; полный набор из 36 standard keyword-ов (auto/default/pointer/text/wait/move/grab/grabbing/all 8 resize-направлений/zoom-in/zoom-out/…); URL-fallback парсер игнорирует и использует последний keyword из comma-list. Использование при mouse-handling — позже. **`box-shadow`** — `Vec<BoxShadow>` (offset_x/y, blur, spread, color, inset), CSS Backgrounds L3 §4.6, не наследуется; парсер `parse_box_shadow_one` собирает токены, балансируя `()` чтобы `rgba(0,0,0,0.5)` не порвался; список через `split_top_level_commas`. Реальная отрисовка теней (Gaussian blur, spread, inset clipping) в paint pipeline — отдельная большая задача. **`text-shadow`** — `Vec<TextShadow>` (offset_x/y, blur, color), CSS Text Decoration L3 §4, **inherited** (отличается от box-shadow); тот же парсер с балансировкой `()`; `none` нужен явно, чтобы откатить родительское inherited значение. **`border-radius`** — 4 поля `border_{tl,tr,br,bl}_radius: f32` (resolved px), CSS Backgrounds L3 §5; shorthand с правилом 1-4 токенов через `expand_border_4` (TL TR BR BL); индивидуальные `border-{corner}-radius`; elliptical-форма `Npx / Npx` берёт только горизонтальный радиус (Phase 0 без vertical-radius); отрицательные значения clamp до 0; не наследуется. Реальный rounded-rect clipping в paint pipeline — отдельная задача. **`letter-spacing` (resolved px)** — дополнительное расстояние между парами символов (CSS Text L3 §11.2), inherited; добавляется в word-width (`(n−1)·ls`) и в word-boundary (`space_w + ls`); может быть отрицательным; `text_rendering_eq` учитывает letter_spacing, чтобы фрагменты с разным spacing не сливались. **`word-spacing` (resolved px)** — дополнительное расстояние **только** между словами (CSS Text L3 §11.3), inherited; добавляется в `gap_with_ls = space_w + ls + ws`; может быть отрицательным; ширина одиночного слова неизменна. Свойства с парсингом: `display` (block/inline/none), `color` (полный CSS3 набор named colors — 147 цветов + `rebeccapurple` из CSS4 §6.1 + `transparent`; `gray`/`grey` варианты эквивалентны; матчинг case-insensitive через бинарный поиск по сортированной таблице `NAMED_COLORS`; hex `#RGB`/`#RRGGBB`/`#RGBA`/`#RRGGBBAA` + `rgb()` / `rgba()` / `hsl()` / `hsla()` с запятыми или whitespace, slash-alpha modern syntax `rgb(r g b / a)`, проценты для каналов, hue в `deg`/`turn`/`rad`/`grad` (CSS Color L4 §9), clamp вне диапазона), `background-color`, `font-size`, `line-height`, `margin` (+ 4 стороны), `padding` (+ 4 стороны), `text-align` (left/center/right), **`text-decoration` / `text-decoration-line`** (комбинируемые keyword-ы `underline` / `overline` / `line-through` / `none`, прочие токены `solid` / `wavy` / `dashed` / `blink` и цвет — игнорируются), **`width` / `height` (px/em/rem; `auto` = не задано)**. `TextDecorationLine` хранится как struct из трёх булевых полей; наследуется на детей через каскад. Whitespace-only текстовые узлы и комментарии пропускаются. **Line wrapping:** `TextMeasurer` trait + `layout_measured()` разбивают текст по словам на строки с реальными шрифтовыми метриками. **Inline-flow:** `BoxKind::InlineRun { segments, lines }` — текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`, `<strong>`, и т.д.) группируются в один поток; каждый сегмент хранит свой стиль (цвет ссылки, decoration); слова с одинаковым rendering-стилем сливаются в один `InlineFrag` на строке; `InlineFrag.width` хранит измеренную ширину текста (для align_lines и подрисовки text-decoration в paint). `ComputedStyle::text_rendering_eq` сравнивает color/font_size/line_height/text_decoration_line. `align_lines()` сдвигает `frag.x` после wrap для center/right выравнивания.
- **Готово (тестовая инфра):** `serialize_layout_tree(&LayoutBox) → String` — детерминированный текстовый формат всего layout-дерева (kind / rect / non-default style включая text-align, w=, h=, box-sizing, decoration / segments / lines), плюс 16 golden-тестов в `tests/snapshot_tests.rs` (empty / paragraph / styles / nested / inline + link / line wrap / cyrillic / stacked / display:none / nth-child(odd) / :not(.x) / descendant / underline-on-link / border-solid / border-top / box-sizing border-box). Механизм `UPDATE_SNAPSHOTS=1` для регенерации (как в lumen-paint).
- **Готово (relative units):** `Length { Px, Em, Rem, Percent, Vh, Vw, Vmin, Vmax }` + `parse_length`. Cascade — два прохода: pre-pass для `font-size` (em/% относительно parent fs, rem от `ROOT_FONT_SIZE` = 16, vh/vw/vmin/vmax — от `viewport`), затем main-pass для остального (em/% относительно computed fs текущего элемента). `line-height: 150%` / `1.5em` корректно превращается в коэффициент 1.5; `5vh` / `2vw` тоже поддерживаются. **Viewport units** (CSS Values L3 §6.1.2): `1vh = 1% от viewport.height`, `1vw = 1% от viewport.width`, `vmin = 1% от min(w,h)`, `vmax = 1% от max(w,h)`. Viewport прокинут через `compute_style → apply_declaration → resolve_box_length → Length::resolve` — все четыре функции принимают `viewport: Size`. `%` в margin/padding требует containing-block-width и Phase 0 игнорируется (молча).
- **Готово (borders):** `BorderStyle` enum (None/Solid/Dashed/Dotted) + 12 полей `border_{top,right,bottom,left}_{width,style,color}` в `ComputedStyle`. Парсинг: `border` shorthand, `border-{side}` per-side shorthand, `border-{width,style,color}` multi-value (1–4 токена по CSS-правилу), `border-{side}-{prop}` individual. `border_*_color: Option<Color>` (None = currentColor). Контент-область корректно уменьшается на ширины border: `content_x/y` учитывают border, `content_width` убирает border_left+border_right. Высота и ширина бокса включают border-widths. `snapshot.rs` выводит `bw=(...)` и `bs=(...)` для ненулевых border.
- **Готово (box-sizing):** `BoxSizing` enum (ContentBox/BorderBox) + поле `box_sizing` в `ComputedStyle`. Парсер `box-sizing: content-box | border-box` (case-insensitive). Не наследуется (CSS Basic UI 3 §4.1) — сбрасывается на ContentBox в каждом `compute_style`. В `lay_out`: при `width`/`height`, заданных в content-box — добавляем padding+border сверху (старая модель), в border-box — `rect.width = w`, `rect.height = h`, контент-область сжимается. Анонимный `InlineRun` тоже сбрасывает box-sizing для чистоты snapshot-а. `snapshot.rs` печатает `box-sizing=border-box` только когда отличается от default.
- **Готово (min/max dimensions):** 4 поля `min_width` / `max_width` / `min_height` / `max_height: Option<f32>` (CSS 2.1 §10.4), не наследуются. None = «нет ограничения» (для min эквивалентно 0, для max — `none`). Парсер: `min-width: auto` и `max-width: none` оставляют None; отрицательные значения отбрасываются спецификацией; px/em/rem/vh/vw поддерживаются (Phase 0 без `%`). В `lay_out` clamp применяется к `rect.width` после `width`-расчёта и к `rect.height` после `height`/content-расчёта; порядок `max сначала, потом min` автоматически даёт правило «при min > max побеждает min». min-/max- интерпретируются в той же box-sizing модели (content-box добавляет padding+border, border-box — нет). Анонимный `InlineRun` сбрасывает все 4 поля.
- **Готово (writing modes):** `Direction` enum (Ltr / Rtl) + поле `direction: Direction` в `ComputedStyle`, **inherited** (CSS Writing Modes L3 §2.1). Парсер `direction: ltr | rtl` (ASCII case-insensitive по Values L4 §2.4); невалидное значение оставляет inherited. В Phase 0 layout только хранит и распространяет через каскад — реальный RTL line-flow / bidi reordering (UAX #9 Unicode Bidi Algorithm) требуют отдельного движка и переписанного `wrap_inline_run`. Точка хранения зафиксирована заранее, чтобы при добавлении `dir`-атрибута / `<bdo>` / bidi не нужно было ретрофитить структуру.
- **Готово (CSS `calc()` для length-свойств — CSS Values L4 §10):** `Length::Calc(Box<CalcNode>)` хранит AST выражения, `CalcNode::resolve(em, pb, vp) -> Option<f32>` рекурсивно вычисляет в `f32`-пиксели. Парсер `parse_calc` — recursive-descent поверх `tokenize_calc`: ASCII case-insensitive префикс `calc(` распознаётся в `parse_length` через `strip_calc_wrapper`, тело лексируется в токены (`Num(f32, unit)` / `Plus` / `Minus` / `Star` / `Slash` / `LParen` / `RParen`) и разбирается грамматикой `expr := term (('+'|'-') term)*`, `term := factor (('*'|'/') factor)*`, `factor := ('-'|'+') factor | Num | '(' expr ')'`. Унарный `-` — через `0 - factor` рекурсивно (корректно для `calc(-10px + 5px)` и `calc(20px + (-5px))`); унарный `+` — no-op. Поддерживаемые единицы внутри calc: те же что у `parse_length` (px/em/rem/vh/vw/vmin/vmax/%) + unitless `Number`. Неизвестная единица (`pt`/`mm`) → declaration invalid. Деление на 0 → `None`. Интеграция с var(): `var()` разворачивается раньше (см. ниже), результат — строка `calc(...)` парсится обычным путём. Phase 0 ограничения: `line-height: calc(...)` использует общий путь «резолв в px → делим на font-size», что для unitless-чистого calc даст неверный результат (используйте bare `line-height: 1.5`); нет вложенного `calc(calc(...) + 10px)` (нужно распознать ident `calc` среди токенов в `parse_calc_factor`); нет `min()` / `max()` / `clamp()` (CSS Values L4 §10.6).
- **Готово (CSS Variables L1 — `--name` + `var()`):** `ComputedStyle.custom_props: HashMap<String, String>` хранит resolved-cascade всех custom property declarations. Все custom properties inherited по спеке (`compute_style` копирует `inherited.custom_props.clone()` в init). Каскад — три прохода поверх отсортированного `matched`: font-size pre-pass → **custom-properties pass** (для каждой declaration с `property.starts_with("--")` делает `custom_props.insert(name, value.clone())`) → main-pass. Custom-pass отдельный, чтобы любая обычная декларация в main-pass видела финальное значение custom property независимо от source order (например, `color: var(--c); --c: red` корректно даёт красный). Substitution `var(--name [, fallback])` происходит в `apply_declaration` в самом начале: если `decl.value.contains("var(")` — рекурсивно вызывается `expand_vars(&decl.value, &style.custom_props, 0)`, результат заменяет `val: &str` для оставшегося match-кода. При неудаче (имя не найдено и нет fallback / превышена глубина рекурсии 32 / синтаксис сломан) функция возвращает `None` и `apply_declaration` ранний return — declaration treated as if not present (CSS Variables L1 §3.3 «invalid at computed value time»). `find_var_open` ищет `var(` byte-wise с пропуском строковых литералов (одинарные и двойные кавычки), `parse_balanced_to_close` учитывает вложенные `(`/`)` и строки, `split_var_args` режет аргументы по первой top-level запятой (чтобы `rgba(0,0,0,0.5)` внутри fallback не порвался). Custom property declarations НЕ применяются к obычным CSS-свойствам (`--display: block` лежит только в `custom_props`, не меняет `style.display`).
- **Отложено:** flex, grid, float, абсолютное позиционирование, `%` в margin/padding/width/height/min-/max- (нужен containing block), единицы `ch`/`ex` (требуют font metrics), color spaces CSS4 (`lab`, `lch`, `oklab`, `oklch`, `color()`), реальная отрисовка bold/italic вариантов в paint, **реальное применение `direction: rtl`** (RTL line-flow + UAX #9 bidi reordering — нужны вместе), registered custom properties через `@property` (с типом / inherits / initial).

### `lumen-paint` 🟡 (fill rects + textured text + FontMeasurer)

- **Готово:** `DisplayCommand` enum (FillRect, DrawBorder, DrawText), `build_display_list` обход LayoutBox с painter's order — для `BoxKind::Block` с border эмитирует `DrawBorder`; для `BoxKind::InlineRun` эмитирует по одному `DrawText` на фрагмент с правильными X/Y-смещениями плюс FillRect-ы для подрисовки text-decoration. wgpu Renderer с двумя pipeline-ами: fill (vertex pos + color) и text (vertex pos + uv + color). Два WGSL-шейдера, общий uniform (viewport), bind group для атласа (R8 texture + linear sampler). `GlyphAtlas` 512×512 со shelf packer-ом. `FontMeasurer<'a>` — реализация `TextMeasurer` на основе TTF hmtx/cmap для shell. Per-glyph metadata кеш (atlas position + left/top offset + advance_native). Atlas заливается на GPU только при dirty. `DrawBorder` renderer: 4 fill-quad-а (top/right/bottom/left edges), цвет `border_*_color.unwrap_or(style.color)`.
- **Готово (text-decoration):** для каждого фрагмента с непустой `TextDecorationLine` после `DrawText` эмитятся FillRect-ы: underline — под baseline (+10% fs), line-through — выше baseline (-30% fs), overline — у верха строки (-78% fs относительно baseline). Толщина ≈ 7% font_size, минимум 1px. Цвет — `frag.style.text_decoration_color.unwrap_or(frag.style.color)` (CSS Text Decoration L3 §3 — currentColor fallback). Ширина FillRect берётся из `InlineFrag.width`.
- **Готово:** `serialize_display_list(&[DisplayCommand]) → String` — детерминированный текстовый формат для snapshot-тестов. 6 интеграционных golden-тестов в `tests/snapshot_tests.rs` (пустая страница, параграф, фон, вложенный paint-порядок, кириллица, line wrap). Механизм `UPDATE_SNAPSHOTS=1` для регенерации golden-файлов.
- **Отложено:** pixel snapshot tests, multi-size atlas (сейчас один размер растеризации — 24px, display масштабируется linear sampler-ом), GPU-pipeline для скруглений/градиентов/теней, layer-tree compositor, double/wavy/dashed стили линий декорации.
- 33 unit-тестов (display_list + atlas + wrapping + inline-flow + text-decoration) + 6 snapshot-тестов = 39 тестов.

### `lumen-font` 🟡 (TTF read + raster)

- **Готово:** парсеры таблиц head, maxp, cmap (format 4 + **format 12**), hhea, hmtx, loca, glyf. Glyf обрабатывает simple-глифы (контуры с on-curve / off-curve, квадратичные Безье) и **composite-глифы** (ссылки на другие глифы с 2×2 transform + offset). `Font::glyph_resolved` рекурсивно разворачивает composite в Simple с max-depth 8. Scanline-растеризатор с 4×4 supersampling, even-odd fill, 1px padding. `Bitmap` с метриками left/top для placement. Bundled Inter v4.1 Regular. **cmap format 12** — Sequential Groups, полный Unicode U+0000..U+10FFFF, включая SMP (эмодзи U+1F600+, математику U+1D400+, исторические письменности); `CmapSubtable` enum с rank-based выбором лучшей записи (platform 3/encoding 10 → rank 0, 3/1 → rank 2); бинарный поиск по группам O(log n).
- **Отложено:** hinting (TT-инструкции), GSUB/GPOS (advanced shaping для лигатур, kerning, Arabic/Indic), CFF outlines (для PostScript-OpenType `.otf` без TT-таблиц), variable fonts (fvar/gvar/avar/HVAR), color glyphs (COLR/CPAL, sbix), bitmap strikes (EBDT/EBLC), composite с ARGS_ARE_XY_VALUES=0 (point alignment, рудимент — сейчас offset = (0,0)).
- 62 unit-тестов + 9 интеграционных на bundled Inter. Включает тест на composite кириллической `А`.

### `lumen-encoding` 🟡 (детектор + декодеры)

- **Готово:** таблицы декодирования `Windows-1251`, `KOI8-R`, `CP866` (по WHATWG Encoding Standard) + **UTF-16 LE/BE декодер** с обработкой surrogate-пар (U+10000+ supplementary plane), lone surrogate → U+FFFD, нечётное число байт → trailing U+FFFD. Декодер `decode(encoding, bytes) → String` для всех шести вариантов с lossy-обработкой нелегальных байт и автоматическим срезом UTF-8/UTF-16 BOM. Детектор `detect(bytes, content_type_hint) → Encoding` с приоритетами: BOM (`EF BB BF` → UTF-8, `FF FE` → UTF-16 LE, `FE FF` → UTF-16 BE) → `<meta charset>`/`<meta http-equiv>` в первом килобайте → HTTP content-type hint → валидный UTF-8 → частотная эвристика по русским буквам (взвешенный score из 32 частот). Реализует `lumen_core::ext::EncodingDetector` через `HeuristicDetector`. `Encoding::from_label` парсит WHATWG-алиасы (`cp1251`, `koi8r`, `ibm866`, `utf-16` → LE, `utf-16le`, `utf-16be`, `unicode` → LE, `ucs-2` → LE).
- **Отложено:** ISO-8859-5 и MacCyrillic (не встречаются в природе), полный HTML5 prescan algorithm §12.2.3.2 (наш sniff проще, чем spec, но для практики хватает), UTF-32 (исчезающе редко в дикой природе).
- 53 unit-теста + 6 интеграционных round-trip (включая 18 UTF-16: ASCII/cyrillic LE и BE, BOM-stripping, supplementary 😀 через surrogate pair в обоих endian, lone high/low surrogate, odd byte count, empty input, labels).

### `lumen-image` 🟡 (PNG decode end-to-end, 8-битные форматы)

- **Готово (PNG-pipeline):** свой CRC32 (IEEE 802.3 reflected, таблица предкомпилирована `const fn` — без runtime-инициализации, без `LazyLock`); chunk reader по PNG §11.2.2 (4 BE length + 4 type + data + 4 CRC, ограничение length < 2^31, ошибки `BadCrc { kind, expected, actual }`, `ChunkTooLong`); парсер IHDR (13 байтов: width/height u32 BE, bit_depth + color_type + compression/filter/interlace методы, валидация таблицы 11.1 — разрешённые комбинации bit_depth × color_type). `Image { width, height, format, data }` хранится плотно row-major без padding-а.
- **Готово (DEFLATE/zlib):** свой inflate по RFC 1951 + zlib-обёртка по RFC 1950. Три типа DEFLATE-блоков: stored (00, копия после byte alignment), fixed Huffman (01, таблицы 7/8/9 бит из §3.2.6), dynamic Huffman (10, code-length-codes из §3.2.7 + re-run коды 16/17/18). LZ77 со sliding window до 32 КБ через побайтовое копирование (корректно работает на overlapping back-references вроде `distance=1`). zlib header: CMF.CM=8, проверка `(CMF<<8|FLG) % 31 == 0`, FDICT=0. Adler-32 в трейлере. `BitReader` LSB-first; канонический Huffman через ranges-of-codes (`first_code[L]`, `count[L]`, `offset[L]` + `sorted_symbols`) с Kraft-McMillan валидацией при build-е. Bounded allocations: 3 маленьких Huffman-декодера + output Vec.
- **Готово (filter undo):** все 5 PNG-фильтров скан-линий (PNG §9.2): None / Sub / Up / Average / Paeth. Wraparound u8 арифметика; Paeth-предиктор в i16 для корректного сравнения abs. Для первой строки `b = c = 0`, для первых bpp байтов `a = c = 0`.
- **Готово (orchestrator `decode_png`):** проверка 8-байтовой сигнатуры `89 50 4E 47 0D 0A 1A 0A`; первый чанк обязательно IHDR; auxiliary-чанки (sRGB / gAMA / pHYs / tEXt / iCCP / cHRM) игнорируются (PNG §11.3 — ancillary safe-to-ignore); IDAT-ы конкатенируются в один zlib-поток; IEND маркирует конец. Поддержаны `PixelFormat::{Gray8, GrayAlpha8, Rgb8, Rgba8}` при `bit_depth = 8` без interlacing. Прочие комбинации (palette, 16-bit, interlaced, sub-byte) явно возвращают `Unsupported(...)`.
- **Отложено:** 16-битная глубина (умножение байт на пиксель × 2), palette (color_type 3, потребует чтения PLTE + опц. tRNS), Adam7 interlacing (7 sub-images с фиксированной геометрией), JPEG (свой DCT/Huffman/marker parser — отдельный крейт-объём работы), WebP / AVIF (Phase 2+).
- 50 unit-теста (CRC32, signature, chunk reader, IHDR, BitReader, Huffman build/decode, fixed/dynamic/stored inflate, LZ77, adler32, фильтры 0–4) + 9 интеграционных на реальных PNG-фикстурах (RGB 3×2, RGBA 4×2, Gray 4×4, GrayAlpha 2×2, mixed filters 2×4, Paeth 2×2, rejects бракованных). Фикстуры сгенерированы Python-zlib скриптом (см. `tests/fixtures/`).

### `lumen-storage` ✅ (in-memory KV + snapshot)

- **Готово:** `InMemoryStorage` — `HashMap<PartitionedKey, Vec<u8>>` с полным origin-партиционированием: каждый вызов принимает `origin: Option<&str>` и `top_level_site: Option<&str>`. `None` и `""` — один namespace (глобальный профиль). Реализует `lumen_core::ext::StorageBackend` (get/put/delete/list_keys). Snapshot-формат `LUMEN_KV_V1` — текстовый, hex-encoded composite key + hex-encoded value, без внешних зависимостей. `serialize()` / `deserialize()` для in-memory round-trip; `save(path)` / `load(path)` для диска.
- **Отложено:** B-tree persistent backend (сейчас вся структура в RAM), TTL для cookies, namespace helpers (`cookies::`, `history::`, `profile::`), `clear_origin(origin)` для быстрой чистки всех данных источника.
- 17 тестов: CRUD, origin-изоляция, top_level_site-партиционирование, list_keys, snapshot round-trip (включая binary и кириллицу), ошибки десериализации.

### `lumen-network` ✅ (HTTP/1.1 + HTTPS)

- **Готово:** `HttpClient` реализует `NetworkTransport` из `lumen-core::ext`. Поддержка HTTP и HTTPS (rustls + webpki-roots, exception #3). Redirect-следование до 5 хопов (абсолютные + относительные `Location`). `chunked` Transfer-Encoding decoder. URL-парсинг (scheme/host/port/path), case-insensitive заголовки. Box-обёртка вокруг TLS stream (clippy large-enum-variant). **IDN-домены** конвертятся в Punycode на этапе `parse_url` через `lumen_core::idn::domain_to_ascii` — DNS lookup, TLS SNI (`ServerName::try_from`) и `Host:` header (RFC 7230 §5.4) всегда получают ASCII-форму.
- **Готово (EventSink — принцип №4):** `HttpClient::with_sink(Arc<dyn EventSink>)` + `with_tab(TabId)` — fluent builder. `fetch_with_redirect` эмит `Event::RequestStarted { tab_id, url }` **после** успешного `parse_url` (bad scheme → ни Started, ни Completed) и перед TCP-сокетом; `Event::RequestCompleted { tab_id, url, status }` — после чтения статус-строки, **до** анализа кода ответа (2xx / 3xx / 4xx — все эмитятся как completed). Каждый редирект-hop генерит свою пару (Started/Completed), не одну на цепочку. Инвариант «Started без Completed = network failure» (DNS / refused / TLS handshake error прерывают между ними); явный `RequestFailed` добавим, когда наблюдателям станет мало. `sink: Option<Arc<dyn EventSink>>` — по умолчанию None, sink не дёргается совсем (не нужен hot-path NoopEventSink для типичного случая «события никому не нужны»).
- **Отложено:** HTTP/2, keep-alive соединения, кэш (Cache-Control), аутентификация, cookie jar, проксирование, `RequestFailed` событие (для DNS/connect/TLS-ошибок до `RequestCompleted`).
- 20 тестов: URL-парсинг (включая IDN-кейсы — кириллический host, IDN+port, mixed ASCII subdomain), status line, header lookup, chunked decoder; **5 новых** интеграционных через mock-`TcpListener`: Started+Completed на 200, 4 события на 2-hop редирект, Completed-на-404, no-events при bad scheme, builder без sink.

### `lumen-shell` 🟡 (окно + рендер + сеть)

- **Готово:** winit 0.30 с `ApplicationHandler` API. Три режима: `lumen` (пустое окно 1024×720), `lumen <path.html>` (файл → кодировка → HTML → layout → paint), `lumen <http(s)://...>` (сеть через `HttpClient` → те же этапы). Внешний CSS: `<link rel="stylesheet" href="...">` загружается с диска (относительно HTML-файла) или по сети (относительно базового URL). `ResourceBase` enum изолирует логику разрешения относительных URL. Inter-Regular.ttf bundled через `include_bytes!`. Обработчики Resized + RedrawRequested.
- **Готово (network log):** `StdoutEventSink` — простейший наблюдатель сетевых событий, печатает в stdout: `→ GET <url>`, `← <status> <url>`, `✗ <url> (<reason>)`. Подключается к `HttpClient` в shell, чтобы каждый исходящий байт был виден пользователю — это и есть Phase 0 версия network log из принципа №4. Позже заменится на структурированный UI-логгер (отдельная панель в окне).
- **Готово (window title):** `extract_title(&Document)` находит первый `<title>` в дереве, склеивает текстовые дети и сжимает whitespace через `split_whitespace().join(" ")` (отрабатывает `\n\t`, длинные пробелы). Энтити уже декодированы tokenizer-ом (RCDATA). `LoadedPage { display_list, title }` возвращается из `load_url` / `load_page` / `render_bytes` — единая точка для будущих расширений (favicon, current URL, scroll state). `window_title(Option<&str>)` форматирует заголовок: с title — `"<title> — Lumen"`, без — fallback на `Lumen <version>`.
- **Отложено:** вкладки, омнибокс, навигация, истории сессий, scroll, обработка input-событий, динамическое обновление title при навигации (сейчас title подставляется один раз в `resumed`).
- 19 unit-тестов (resolve_url, ResourceBase::resolve, collect_link_hrefs, extract_title, window_title).

### `lumen-bench` ✅ (baseline pipeline measurements)

- **Готово:** отдельный bin-крейт без сторонних зависимостей. Прогоняет `decode → parse_html → parse_css → layout → paint::build_display_list` на `samples/page.html` + `samples/page.css` + bundled Inter; печатает min / median / mean / p95 / max на фазу и TOTAL. `LUMEN_BENCH_ITERS` env var переопределяет число измерений (по умолчанию 100). 10 warm-up итераций перед измерениями. `std::hint::black_box` оборачивает результаты pipeline-а — защита от dead-code elimination в LTO-release-сборке. Запуск: `cargo run -p lumen-bench --release`.
- **Зачем:** до этой задачи цели плана (cold start <300 мс, RAM <100 МБ на пустую вкладку) были лозунгами без точки отсчёта. Теперь регрессии при росте функциональности отслеживаются — каждая фаза должна оставаться в своём бюджете.
- **Baseline (dev profile, samples/page.html 667 B HTML + 300 B CSS, 49 DOM-узлов, 7 CSS-правил, 18 paint-команд, на x86_64 CachyOS):** decode ~2 μs, parse_html ~19 μs, parse_css ~13 μs, layout ~48 μs (доминирует), paint ~1 μs, TOTAL ~85 μs. Release-сборка должна показать примерно те же цифры или быстрее (dev уже на opt-level=3 для deps).
- **Отложено:** более крупные strona-test-cases (реальные статьи в десятки KB), per-phase profile breakdown (cascade vs measure внутри layout), measurement of font parse cost (сейчас амортизированный — один раз перед циклом), CI-trend tracking, тесты как proper #[bench] (требует nightly или criterion как exception #5).

### Инфраструктура

- Cargo workspace, edition 2024, resolver 3, MSRV 1.95.
- 13 крейтов в `crates/`: shell, core, network, storage, bench, engine/{html-parser, css-parser, dom, layout, paint, font, encoding, image}.
- Bundled assets: `assets/fonts/Inter-Regular.ttf` (+ OFL.txt лицензия).
- Тестовая страница: `samples/page.html` со встроенным `<style>`.
- 4 разрешённых внешних зависимости: `winit = "0.30"`, `wgpu = "26"`, `rustls = "0.23"` + `webpki-roots = "0.26"` (активированы в lumen-network), JS engine (зарезервирована).
- Внутренние deps: workspace.dependencies на 11 крейтов.
- `.gitattributes` форсит LF для всех текстовых файлов; binary-метка для `.ttf / .png / .woff2`.
- `.gitignore` игнорирует `/target`, `/*.zip`, `/*.tar*`, `.idea/`, `.vscode/`, swap-файлы.

### Численно

- **Всего тестов в workspace:** 903 (на момент последнего обновления).
- **`cargo clippy --workspace --all-targets -- -D warnings`** проходит без warnings.
- **Внешних зависимостей runtime:** 2 активных (winit, wgpu) + 2 зарезервированных.
- **Транзитивно через wgpu/winit:** ~200 crates.

---

## Roadmap — что предстоит реализовать

Приоритизированный список. Порядок может меняться, ориентируйся на план §16 «Фазы».

### Ближайшее (Phase 0 закрыта, Phase 1 начало)

Порядок — по impact/effort. Внешний ревью указало, что текущий рендерер сломается на любой реальной странице из-за шрифтов и DPR; это перевешивает любые архитектурные рефакторинги.

1. **`[P2]` Font fallback / matcher** — рендерер сейчас всегда Inter Regular. Любая реальная страница с эмодзи / CJK / явным `font-family: Roboto` отрисуется в `?`-глифы. Минимум: системный font-loader (Win32 GDI / fontconfig / CoreText напрямую, без сторонних crate-ов), cascade «Inter → системный по unicode-блоку». Парсер `font-family` в `lumen-css-parser` уже есть, в paint не используется. **Это блокер для Phase 1 как демонстрации.**
2. **`[P3]` `Url` как структурированный тип** — `struct { scheme, host, port, path, query, fragment }`. Сейчас `lumen-core::Url` это `Url(String)`, network ad-hoc парсит то же самое в `parse_url`. Дедуплицировать до того, как появятся CSP / cookie jar / cross-origin checks. День работы.
3. **`[P4]` Scroll + DPR-awareness в shell.** Вместе, потому что без `scale_factor` от winit scroll выглядит игрушечно на 4K. Открывает возможность работать с реальными статьями.
4. **`[P3]` `RequestBlocked` event + место для FilterListSource-чек** — Started/Completed уже emit-ятся, Blocked пока нет (нет источника блокировок). Добавить, как только появится первый фильтр (трекеры / ad-blocker), чтобы каждый «не-исходящий байт» тоже был виден.

### Средний приоритет (Phase 1+)

6. **`[P1]` CSS — типизированные значения деклараций** — length / color / calc. Селекторы Level 3 готовы полностью. **CSS Variables (`--name` + `var()`) и `calc()` — реализованы** в `lumen-layout` (см. «Состояние подсистем»). Осталось: типизированный `Length` для всех свойств (сейчас Length типизирован для большинства, но в каскаде значения хранятся строкой и парсятся в момент apply), типизированные `Color`-значения у всех свойств, `min()`/`max()`/`clamp()` (CSS Values L4 §10.6), nested `calc(calc(...) + ...)`, registered custom properties (`@property`).
7. **`[P3]` Tab session export / import** (§12.7) — сериализация в snapshot-формат lumen-storage. Простое, экономит много боли.
8. **`[P2]` Картинки на страницах — продолжение** — крейт `lumen-image` создан и декодирует PNG (8-битные Gray/GrayA/RGB/RGBA, фильтры 0–4, свой DEFLATE/inflate). Дальнейшие подзадачи: интеграция `<img>`-элемента в HTML/DOM/layout (с учётом intrinsic dimensions и `width`/`height` атрибутов), GPU-загрузка декодированных пикселей в paint (новый pipeline для RGBA-quad с per-image текстурой, не glyph atlas), сетевая подгрузка `<img src="http(s)://...">`, свой JPEG-декодер (отдельный объём — DCT, marker parsing, Huffman, color conversion), 16-bit/palette/Adam7 в PNG.

### Большое (Phase 2+)

11. **`[P4]` QuickJS интеграция через `rquickjs`** — exception #4. Базовое исполнение JS. `lumen-core::ext::JsRuntime` trait.
12. **`[P3]` `lumen-knowledge` крейт** (§12.1-12.4) — FTS-индекс над историей и заметками, omnibox-префиксы `@history` / `@notes` / `@tabs` / `@read-later`.
13. **`[P1]` CSS Grid + полный Flexbox** в layout.
14. **`[P3]` HTTP/2** поверх свои rustls-based транспорта.
15. **`[P3]` DoH / DoT resolver** в network-слое.
16. **`[P4]` Site isolation** (process per origin) — `lumen-renderer` процесс отдельно от shell.
17. **`[P3]` Profiles + шифрование** (§9.3) — XChaCha20-Poly1305, Argon2id KDF.
18. **`[P4]` Focus mode** (§12.6) — UI feature, не требует новых крейтов.
19. **`[P4]` Кастомизация UI** (§12.10) — drag&drop панелей, темы.

### Очень большое (Phase 3+)

20. **`[P4]` V8 переход** с `rusty_v8`. Реализуем `JsRuntime` для V8, не ломая QuickJS path.
21. **`[P4]` `lumen-ai` крейт** (§12.5) — embedding + RAG + LLM-backend через Ollama HTTP (без exception, используем существующий `lumen-network`). Встроенный llama.cpp как exception #5 — отвергнут (см. Decisions log).
22. **`[P3]` Семантические закладки** (§12.8) — требует §12.5 (lumen-ai от P4).
23. **`[P4]` Service Workers** · **`[P2]` Canvas 2D** · **`[P3]` IndexedDB**.
24. **`[P2]` WebFonts через WOFF2** в `lumen-font`.

### Не приоритет, держим в голове

- **`[P2]`** Variable fonts (fvar/gvar/avar/HVAR) в `lumen-font`.
- **`[P2]`** GSUB/GPOS shaping (для арабского, индийского, тайского). Текущая позиция — добавим как exception #5 (rustybuzz) или сами для базовых случаев. См. анализ qwen.ai и обсуждение в плане.
- **(любой)** ADR-инфраструктура (`docs/decisions/`) — формализация decisions log.
- **`[P3]`** StorageBackend trait: добавить origin partitioning параметр (`(origin, top_level_site)`) ДО первой реализации, чтобы не переделывать.
- **`[P2]`** Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — для битых старых шрифтов.
- CSS4 pseudo-class `:has(...)` — реализовано в `css-has-pseudo`, см. историю merge-ов.

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
- **Persistent storage пишем сами, `redb` / `sled` / SQLite не берём как exception #5.** Storage — engine-adjacent: своё crypto = security antipattern (поэтому `rustls` оправдан), а свой log-structured KV (append-only log + atomic rename + периодическая компакция) — несколько недель работы, в духе принципа «default — своё». Промежуточный шаг до v0.5 — бинарный `LUMEN_KV_V2` snapshot + atomic write (write-temp + rename). Текущий текстовый `LUMEN_KV_V1` достаточен пока история <10 МБ. Каждый принятый exception дешевит обоснование четырёх уже существующих — это не та граница, где стоит уступать.
- **Punycode применяется на этапе `parse_url` в `lumen-network`, не в `Url::parse`.** Url остаётся тонкой обёрткой над String (как было задумано в комментарии url.rs: «правильная Punycode-конвертация реализуется в network-слое»). Альтернатива (превращать host в ASCII прямо в `Url::parse`) ломала бы две вещи: (1) `Url` должен уметь хранить и отдавать оригинальную Unicode-форму для отображения пользователю в адресной строке; (2) для file:// и других схем Punycode не применим. Network-слой — единственный потребитель, которому нужна ASCII-форма (DNS lookup, TLS SNI, Host header). Поэтому `idn::domain_to_ascii` зовётся внутри `parse_url`, после того как host извлечён из authority. Без NFC normalization и UTS #46: для русских доменов (NFC-стабильная кириллица) практически достаточно `str::to_lowercase`. Если упрёмся в edge case (например, ß или ZWJ) — добавим mapping table.
- **UTF-16: голый label `utf-16` мапится на LE, не на BE.** Это WHATWG Encoding Standard §4.2: алиасы `utf-16`, `unicode`, `ucs-2` — все на UTF-16 LE, потому что подавляющее большинство «UTF-16»-файлов в природе — это `Save As → Unicode` из Windows Notepad, который пишет LE. UTF-16 BE достижим только через явный `utf-16be`. Это противоречит здравому смыслу (BE — natural byte order для big-endian network), но менять — значит ломать существующий веб. Декодер же спокойно снимает BOM любого endian-а в начале потока: если detect отдал Utf16Le, а реальный BOM `FE FF` — мы корректно его обрезаем (детектор бы выбрал Utf16Be, но defensive coding). Lone surrogates и нечётное число байт превращаются в U+FFFD — invalid-on-input, никаких panic-ов. Surrogate-пары обрабатываются вручную: `0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)`; не используем `char::decode_utf16` из std, потому что нам нужна полная управляемость над non-strict обработкой (std возвращает Result-итератор, лень обёртывать ради того же результата).
- **`calc()` хранится как AST (`Box<CalcNode>`) внутри `Length`, не предварительно резолвится.** Резолв требует `em_basis`, `percent_basis`, `viewport` — все известны только в момент применения декларации к конкретному элементу (em и % зависят от позиции в дереве). Альтернатива «парсить calc → сразу `Length::Px(f32)`» возможна для чистых px-выражений (`calc(10px + 5px)` = `15px`), но требовала бы анализа «содержит ли выражение em/%/vh/vw» и разветвления; AST даёт одну однородную точку. **`Length::Calc(Box<CalcNode>)`, не `Calc(CalcNode)`:** убирает Length из Copy (Box не Copy), но cascade-код не дёргает Length-копии в hot-path — все 78+ usage-сайтов либо match по варианту с десоставлением f32, либо вызов `.resolve(...)`. Стоимость `Box::new` амортизирована: на page без calc() аллокации нет вообще; на странице с десятком calc() — 10 boxes, по 24 байта каждый = 240 байт, мизерно. **Recursive-descent парсер, не shunting-yard:** грамматика двухуровневая (term/factor) с фиксированными приоритетами `*//` > `+-`, нет дополнительных операторов; recursive descent читается напрямую как грамматика, shunting yard потребовал бы стека операторов и стека операндов — больше кода и меньше явности. Унарный минус через `factor := '-' factor | ...` представляется как `Sub(Number(0), x)` (а не отдельный вариант `Neg(x)`), потому что `resolve` для `Sub` уже корректно обрабатывает: `0 - x`. **Унарный `-` решается на этапе парсинга, не лексинга:** альтернатива «токенайзер делает Num(-5, "px") если `-` перед числом» порвалась на `10px - 5px` (whitespace-разделение): `-` после `10px` шло перед `5px`, токенайзер бы решил «знак» вместо «бинарный минус» и потерял бы оператор. Парсер видит контекст (что слева от `-` уже разобран term), и однозначно: если `-` в позиции factor — унарный, иначе — бинарный из expr. **Деление на 0 → `None` (declaration invalid), не `f32::INFINITY`.** CSS spec не определяет — оба варианта в дикой природе встречаются. None даёт «декларация игнорится, наследованное значение остаётся», что безопаснее, чем INF, который сломает layout. **`line-height: calc(...)` использует общий resolve→делим на font_size:** для чистого calc-числа (`calc(1 + 0.5)` = 1.5) это даёт неверный результат `1.5 / fs`. Отличить «unitless итог» от «length итог» можно было бы через возврат типизированного `ResolvedLength { px: f32, was_unitless: bool }`, но это инвазивная правка всего `resolve` API ради одного edge case. В Phase 0 — известное ограничение: для коэффициента используйте bare-form `line-height: 1.5`.
- **CSS Variables L1 — substitution на этапе layout, не парсера.** `--name: value` declarations и `var(--name [, fallback])` references парсятся css-parser-ом естественно, без специальной грамматики (value читается до `;`/`}` с уважением к строкам и скобкам; `--main-color` — валидный ident, потому что `is_ident_start` уже допускает `-`). Семантика — резолв custom property cascade-ом и substitution `var()` в значениях — целиком в `lumen-layout::compute_style`. Альтернатива (парсить `var(...)` в css-parser в типизированный AST с placeholder-ами) была отвергнута: (1) css-parser намеренно держит values сырыми строками — типизация декларации это работа layout-а (см. existing decision «типизированные значения отложены»); (2) парсинг `var()` без custom_props контекста бесполезен — всё равно substitution делается в момент применения; (3) текущий подход — одна точка изменения (apply_declaration в начале) добавляет поддержку для всех 30+ свойств сразу. **Three-pass cascade в `compute_style`:** font-size pre-pass (фиксирует em-basis) → custom-props pass (заполняет `style.custom_props` по cascade-порядку) → main-pass (применяет обычные свойства; `var()` в значениях разворачивается через `expand_vars` с уже готовым `style.custom_props`). Custom-pass обязательно отдельный, иначе `color: var(--c); --c: red` (в одном правиле) дал бы default цвет, потому что main-pass идёт в порядке declaration-индекса. **`expand_vars` рекурсивный**, depth limit 32: разворачивает `var()` ⇒ resolved (custom_props.get или fallback) ⇒ может содержать ещё `var()`, расширяется до фиксированной точки. Циклы вида `--a: var(--b); --b: var(--a)` ловятся depth limit-ом и declaration treated as invalid (CSS Variables L1 §3.3). **`Option<String>` возврат, не Cow-ish заморочка:** allocation amortized — большинство значений не содержит `var(`, в этом случае ранний `if !contains("var(")` минует expand вообще; для значений с var(), мы и так делаем substitution, format! не дороже остального. **Custom properties как `HashMap<String, String>` в `ComputedStyle`, не `Arc<HashMap>`:** ComputedStyle уже клонируется без оптимизаций (font_family: Vec, text_shadow: Vec, box_shadow: Vec), HashMap.clone() O(n) в общем потоке размытия. Когда производительность станет приоритетом — переедем на `Arc<HashMap>` + `Arc::make_mut` для copy-on-write. **`!important` для custom properties работает само собой**, потому что custom-pass идёт по уже отсортированному `matched`-списку — `(important, specificity, rule_order, decl_index)` ставит important-декларации в конец, и они побеждают позже-применением.
- **`EventSink::emit(&self, &Event)` — `&self`, не `&mut self`, и `&Event`, не `Event` по значению.** `&self` потому, что типичная реализация хранит state под `Mutex` / atomic / channel-sender и должна быть shared между потоками (фоновая загрузка favicon + main thread, network-log UI thread); `&mut self` навязал бы один обладатель и потребовал бы `Arc<Mutex<dyn EventSink>>` у каждого вызывающего, что превратило бы emit в hot-path с двойной indirection. `&Event` потому, что caller (network) обычно не нуждается в событии после emit (он отбрасывает значение), а sink сам решает: счётчик может извлечь нужное поле без clone, collector — клонировать. Платить `event.clone()` на стороне caller-а при том, что половина sink-ов его выкинет — антипаттерн. Альтернатива (`emit_owned(Event)`) — добавим параллельно, если найдётся sink, которому zero-copy ownership критичен; пока такого случая нет. **`Option<Arc<dyn EventSink>>` в `HttpClient`, не `Arc<NoopEventSink>` по умолчанию.** Дефолтный путь «события никому не нужны» — самый горячий (тесты, batch-fetch без UI); каждое лишнее `noop.emit(&...)` это виртуальный вызов через trait object и lock-acquire в типичной реализации. `if let Some(s) = sink` обходится в одну ветку без indirection. Hot-path remains zero-cost. **Sink дёргается ПОСЛЕ валидации, но ДО I/O.** `RequestStarted` эмитим после `parse_url` (bad scheme не эмитит — байт даже не подумал улетать), но до `TcpStream::connect` (иначе observer не узнает о попытке соединения, если DNS упал). `RequestCompleted` — после status line, до анализа кода: 4xx — это всё ещё «outgoing byte получил response», observer должен это видеть, а fetch вернёт Err. Каждый redirect-hop генерит свою пару — `fetch_with_redirect` рекурсивно зовёт сам себя с уменьшенным `hops_left`, и каждый виток эмитит на своём URL.

### Открытые вопросы (решим, когда упрёмся)

- **Shaping для сложных скриптов** (Arabic/Indic/Thai): свой shaper за месяцы или rustybuzz как 5-е exception. Откладываем до Phase 2-3.
- **iOS:** Apple-policy требует WebKit на iPhone/iPad. Это противоречит принципу собственного движка. Возможный путь — тонкий shell поверх WKWebView только для iOS, остальные ОС — наш движок. Откладываем до Phase 4.

### Намеренно отвергнутые альтернативы

- **WebView2 / wry / CEF обёртка** — это другой проект, не Lumen. Отказались.
- **html5ever / cssparser / taffy / image / encoding_rs / ttf-parser / rustybuzz / tokio / rayon / redb / egui** — все эти crates рассмотрены и **не взяты**: для них пишется свой код по принципу «default — своё». См. зачёркнутый список в §5.
- **`image` / `png` / `flate2` / `miniz_oxide` для PNG-декодера — отвергнуты.** PNG это compression engine + image format parser — оба классически в зоне «default — своё», аналогично собственному TTF-parser-у в `lumen-font`. DEFLATE/inflate (RFC 1951) укладывается в ~500 LOC, zlib-обёртка + adler-32 — ещё ~100. Свой код даёт полный контроль над allocations (нет hidden buffer pools), детерминированный fail-mode (любая ошибка — `InflateError` с конкретной причиной), и снимает supply-chain риск (PNG-декодер — частая точка CVE; см. CVE-2015-8126 в libpng). Свой PNG также даёт честный baseline для сравнения с эталонными декодерами (нужно для §16 целей по cold start / memory). 5-е exception сейчас политически дорогое: каждый принятый удешевляет обоснование четырёх уже разрешённых.

---

## История последних merge-ов

Чтобы быстро понять, что было сделано в недавних сессиях. Последние сверху.

```
*            window-title           — `<title>` из загруженного документа в `window.set_title(...)`. `extract_title` находит первый <title> в DOM и схлопывает whitespace; `LoadedPage { display_list, title }` — единая точка возврата из `load_url` / `load_page` для будущих UX-данных (favicon, scroll и т.д.). Без динамического обновления при навигации — один раз в `resumed`. 8 новых тестов
*            css-calc                — CSS calc() (CSS Values L4 §10): Length::Calc(Box<CalcNode>) + recursive-descent парсер с приоритетами +-*/ и скобками, унарный минус через `0 - factor`, поддержка всех length-единиц parse_length-а + unitless для умножения; интегрировано в width/height/padding/margin/font-size/line-height; работает поверх уже готового var()-substitution (`padding: calc(var(--gap) + 5px)`). 23 layout теста
*            lumen-image-png        — крейт lumen-image: PNG-декодер для 8-битных Gray/GrayA/RGB/RGBA, без interlacing, без palette, без 16-bit. Свой CRC32 (IEEE 802.3 reflected, const-fn таблица), chunk reader (PNG §11.2.2), IHDR parser с валидацией §11.2.2 table 11.1, DEFLATE/inflate (RFC 1951: stored/fixed/dynamic Huffman + LZ77 + canonical codes + Kraft-McMillan), zlib wrapper (RFC 1950 + adler-32), фильтры скан-линий 0–4 (PNG §9.2). 50 unit + 9 integration на реальных PNG-фикстурах. Никаких сторонних crate-ов (image / png / flate2 / miniz_oxide отвергнуты — §5)
*            css-var                 — CSS Variables L1 (`--name` + `var()`): custom_props в ComputedStyle inherited через cascade, three-pass cascade с custom-pass перед main-pass, рекурсивный expand_vars (depth limit 32) разворачивает var(--name [, fallback]) в значениях обычных свойств; --name declarations и var()-refs парсятся css-parser-ом без специальной грамматики. 4 css-parser + 18 layout тестов
*            css-direction          — direction: ltr | rtl (CSS Writing Modes L3 §2.1): Direction enum, inherited через каскад, case-insensitive парсер, snapshot печатает direction=rtl; реальный RTL line-flow / bidi (UAX #9) отложен — задел под будущий движок. 6 новых тестов
*            network-event-sink     — EventSink trait в lumen-core + emit RequestStarted/Completed в HttpClient (по hop, до сокета / до анализа status), StdoutEventSink в shell. Принцип №4 «каждый исходящий байт виден» оживлён. Option<Arc<dyn EventSink>> вместо NoopEventSink — zero-cost когда никто не слушает. 5 новых тестов через mock-TcpListener
*            bench-baseline         — крейт lumen-bench: baseline-замеры pipeline (decode→parse→layout→paint) на samples/page.html без сторонних deps; min/median/mean/p95/max, warm-up 10 iters, LUMEN_BENCH_ITERS env override; ~85 μs TOTAL на тестовой странице
*            dev-deps-opt-level     — `[profile.dev.package."*"] opt-level = 3` в корневом Cargo.toml: deps собираются с full optimization, наш код остаётся на opt-level=1; wgpu в dev перестаёт быть невыносим, без влияния на release/clippy/test
*            css-min-max-dimensions — min-width / max-width / min-height / max-height (CSS 2.1 §10.4): clamp в lay_out, min beats max, не наследуются, отрицательные отбрасываются; 10 новых тестов
*            css-has-pseudo         — :has(rs-list) (CSS Selectors L4 §17.2): combinator?+complex, descendant/child/+/~, specificity max-of-list, descendants only (не сам E); 8 css-parser + 5 layout тестов
*            claude-md-worktree-rule — обязательные git worktree для параллельных сессий + WIP-коммиты + запреты в shared dir + новая запись в «Известные ловушки»
*            css-oklch-color        — oklch() color (CSS Color L4 §10.3): OKLCH→OKLab→linear sRGB→gamma, L%/C%, hue в любых единицах, slash-alpha; 8 unit-тестов
*            css-accent-color       — accent-color (CSS UI L4 §6.1): Option<Color>, inherited, `auto` → None; парсится через тот же parse_color, что и color/background; real применение к form widgets отложено; 6 новых тестов
*            utf16-decoder           — UTF-16 LE/BE: Encoding::Utf16Le/Be, BOM-детектор (FF FE / FE FF), декодер с surrogate-парами (U+10000+) и lossy fallback для lone surrogates / odd byte count. WHATWG label `utf-16` → LE. 18 новых тестов
*            html-rcdata             — RCDATA для <title>/<textarea>: тело — литеральный текст до </tag, но character references декодируются. Объединил с RAWTEXT в `text_only: Option<(String, bool)>`. 15 tokenizer + 2 tree_builder теста
*            css-font-stretch       — font-stretch (CSS Fonts L4 §2.5): десятые доли процента (u16), 9 keyword-ов с дробными semi-condensed=87.5% / semi-expanded=112.5%, численные % клампятся в [50%, 200%], inherited; попутно font_variant добавлен в text_rendering_eq; 7 новых тестов
*            punycode-idn            — Punycode (RFC 3492) + idn::domain_to_ascii в lumen-core; network.parse_url конвертит host для DNS/TLS/Host header. 8 punycode + 10 idn + 3 network тестов
*            css-font-variant       — font-variant: normal | small-caps (CSS Fonts L4 §6, упрощённый), inherited; font-variant-caps — алиас; real small-caps rendering отложен; 5 новых тестов
*            css-selector-backtracking — selector matching с back-tracking: matches_chain рекурсивный, перебор всех ancestor/earlier-sibling кандидатов; фикс патологии .x+a~span; 2 новых теста, find_ancestor больше не нужен
*            css-text-overflow      — text-overflow: clip | ellipsis (CSS UI L4 §10.1), не наследуется; real truncation в paint отложен; 5 новых тестов
*            css-border-radius      — border-radius (CSS Backgrounds L3 §5): 4 угла, shorthand 1-4 токена, individual border-X-radius, elliptical берёт горизонтальный, clamp; 9 новых тестов
*            css-text-shadow        — text-shadow parsing (CSS Text Decoration L3 §4): Vec<TextShadow>, inherited (≠ box-shadow), `none` сбрасывает; real paint отложен; 7 новых тестов
*            css-box-shadow         — box-shadow parsing (CSS Backgrounds L3 §4.6): Vec<BoxShadow>, inset, blur, spread, comma-separated, rgba() не рвётся; real paint отложен; 9 новых тестов
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
*            css-text-decoration-color — text-decoration-color (CSS Text Decoration L3 §3): Option<Color>, inherited через каскад как и text-decoration-line; парсер shorthand принимает color между keyword-ами линий/стилей; paint использует с currentColor fallback; 8 новых тестов
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
- **Font fallback отсутствует — рендерер всегда `Inter Regular`.** `font-family` в CSS парсится, в paint игнорируется. Реальная страница с эмодзи / CJK / `font-family: Roboto` отрисуется в `?`-глифы (для не-Latin / не-кириллицы) или fallback на Inter (для имён, которых Inter не содержит). Блокер для любой Phase 1 демонстрации. См. roadmap «Ближайшее» п.2.
- **HiDPI / DPR не учитывается.** `winit` отдаёт `scale_factor`, в layout/paint не прокинут. На 4K мониторе всё отрисуется в реальный 0.5× от ожидаемого. См. roadmap «Ближайшее» п.4.
- **`lumen-core::Url` это `Url(String)` без декомпозиции.** `parse_url` в `lumen-network` ad-hoc парсит scheme/host/port/path заново. Дублирование. См. roadmap «Ближайшее» п.3.
- **`RequestBlocked` пока не emit-ится.** `RequestStarted` / `RequestCompleted` уже выходят из `HttpClient`, а `Blocked` нужен только когда появится `FilterListSource` или другой источник причин блокировки. До того момента — событие объявлено, но никогда не вылетает (это не баг, а отложенная функциональность).
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
