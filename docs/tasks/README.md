# docs/tasks/ — Подробные инструкции к задачам

Каждый файл — одна задача. Формат предназначен для сессии **Haiku** (малый контекст):  
никаких архитектурных решений, точные файлы/строки, готовый код.

## Схема задач (инвариант)

```
ROADMAP.md (строка, status ≠ done)   ← главный список задач P1/P2 (P3 → BUGS.md, P4 → CSS-SPECS.md)
   │
   ├─ STATUS-PN.md: голая строка-указатель `<источник>:NN`
   │       <источник> = ROADMAP.md (P1/P2) · BUGS.md (P3) · CSS-SPECS.md (P4) · код `file:line`
   │
   └─ docs/tasks/<id>.md             ← подробный бриф (ТОЛЬКО для нереализованного)
          │
          ▼  выполнение
   удалить строку из STATUS-PN.md  +  удалить task-файл  +  ROADMAP status → done  +  CAPABILITIES/CSS-SPECS/BUGS
```

- **Task-файл существует только для нереализованной задачи.** Реализованная задача = просто
  строка `done` в `ROADMAP.md`; отдельного файла у неё нет (после мержа файл удаляется).
- **Источник правды по структуре — главный список роли** (P1/P2 `ROADMAP.md`, P3 `BUGS.md` OPEN,
  P4 `CSS-SPECS.md` ⬜/🟡). `STATUS-PN.md` — только голые строки-указатели `<источник>:NN` на
  открытые задачи (приоритет сверху вниз), без заголовков/описаний/завершённых (история = `git log`).
- **Переиндексация при вставке (обязательно).** Вставил `K` строк в файл-источник
  (`ROADMAP.md`/`BUGS.md`/`CSS-SPECS.md`) на строке `L` → все указатели в тот же файл во всех
  `STATUS-PN.md` с `NN ≥ L` сдвинуть на `+K`. После любой правки, меняющей нумерацию, проверить,
  что каждый указатель попадает в нужную строку (`sed -n 'NNp' <источник>`). По возможности
  дописывать строки в конец (ROADMAP — конец блока фазы; BUGS.md — конец файла) — меньше сдвиг.

---

## Правила

| Правило | Детали |
|---------|--------|
| **Только для нереализованного** | Файл заводится, когда задача `≠ done` в `ROADMAP.md`. По готовой задаче файла быть не должно |
| **Один файл = одна задача** | Не смешивать правки разных фич |
| **Размер** | XS < 20 строк кода · S 20–80 · M 80–200 · всё выше — делить |
| **Ссылка в STATUS** | Каждая открытая задача = голая строка-указатель `<источник>:NN` в `STATUS-PN.md` (`<источник>` = ROADMAP.md / BUGS.md / CSS-SPECS.md / код `file:line`) |
| **Строка в ROADMAP** | У каждого task-файла должна быть строка-задача в `ROADMAP.md` (status ≠ done); нет — добавить |
| **Удалять после мержа** | После влития: удалить файл (или `## Status: MERGED`) + строку из STATUS + ROADMAP status → done |

---

## Шаблон

```markdown
# Задача: <название>

**Developer:** P<N>  
**Ветка:** `p<N>-<kebab-name>`  
**Размер:** XS / S / M  
**Крейты:** `lumen-XXX`

## Контекст
Почему задача существует. 2–3 предложения.

## Пред-запуск
- [ ] Прочитать: `<file>:<range>` (секция "…")
- [ ] Убедиться, что ветка `main` чиста: `git status`

## Шаги

### 1. Создать ветку и worktree
```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/<kebab-name> -b p<N>-<kebab-name>
cd .claude/worktrees/<kebab-name>
```

### 2. Изменить код
Файл: `path/to/file.rs`

Найти (поиск по строке):
```
<exact search string>
```
Заменить на:
```rust
<new code>
```

### 3. Добавить тесты
В файл `path/to/file.rs` (или `tests/`) добавить:
```rust
#[test]
fn <test_name>() {
    // ...
}
```

### 4. Проверить
```bash
cargo clippy -p lumen-XXX --all-targets -- -D warnings
cargo test -p lumen-XXX
```

### 5. Закоммитить и влить
```bash
git add <files>
git commit -m "P<N>: <сообщение>

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

# Влить в main
git checkout main
git merge --no-ff p<N>-<kebab-name> -m "Merge p<N>-<kebab-name>: <описание>"
git branch -d p<N>-<kebab-name>
git add STATUS-P<N>.md && git commit -m "P<N>: отметить <task> завершённой"
git push origin main
git worktree remove .claude/worktrees/<kebab-name>
```

## Критерии готовности
- [ ] `cargo clippy` чист
- [ ] Тесты проходят
- [ ] Из STATUS-PN.md удалён указатель завершённой задачи (история = git log)
```

---

## Индекс фазовых задач

Декомпозиция `docs/plan/phases.md` (Фазы 3 и 4) в отдельные task-файлы. Каждый
файл — Goal / Current state / Entry points (реальные `file:line`) / Steps / Tests
/ Definition of done. Многие пункты уже частично реализованы — DoD каждого файла
помечает фактический остаток (см. также расхождения плана с кодом ниже).

### Фаза 3 — RP: рендер-паритет реального веба

Цель группы `RP` — открывать произвольные сайты так же, как Edge. Все четыре —
**не greenfield**: инфраструктура частично есть, брифы помечают фактический остаток.

| Файл | Задача |
|---|---|
| [rp-1-percentage-sizing.md](rp-1-percentage-sizing.md) | RP-1: проценты в block-потоке (width/height/margin/padding против containing-block) |
| [rp-2-resize-viewport.md](rp-2-resize-viewport.md) | RP-2: relayout под живой размер окна (убрать хардкод 1024×720) |
| [rp-3-gzip-deflate.md](rp-3-gzip-deflate.md) | RP-3: HTTP gzip/deflate Content-Encoding декодер (flate2 уже в депах) |
| [rp-4-float-layout.md](rp-4-float-layout.md) | RP-4: проброс float-контекста в вложенные блоки (общий float-поток) |

### Фаза 3 — v1.0 (Tier 2 Web APIs, движок-замена, безопасность)

| Файл | Задача |
|---|---|
| [ph3-tier2-web-apis.md](ph3-tier2-web-apis.md) | Tier 2 Web APIs — зонтичный индекс |
| [ph3-indexeddb.md](ph3-indexeddb.md) | IndexedDB |
| [ph3-service-workers.md](ph3-service-workers.md) | Service Worker runtime |
| [ph3-websockets-sse-fetch.md](ph3-websockets-sse-fetch.md) | WebSockets + SSE + Fetch/AbortController |
| [ph3-web-animations-api.md](ph3-web-animations-api.md) | Web Animations API runtime |
| [ph3-navigation-history-api.md](ph3-navigation-history-api.md) | Navigation API + History API |
| [ph3-contenteditable-selection.md](ph3-contenteditable-selection.md) | contenteditable + Input Events L2 + Selection/Range |
| [ph3-bfcache.md](ph3-bfcache.md) | Back/forward cache (bfcache) |
| [ph3-extensions.md](ph3-extensions.md) | Extensions (minimal native format) |
| [ph3-permission-download-ui.md](ph3-permission-download-ui.md) | Permission prompt UI + Download UI |
| [ph3-spell-check.md](ph3-spell-check.md) | Spell check (Hunspell) |
| [ph3-print-pipeline.md](ph3-print-pipeline.md) | Print pipeline (pagination + vector PDF + preview) |
| [ph3-color-management.md](ph3-color-management.md) | Color management (Display P3 / Rec2020 / wide gamut) |
| [ph3-variable-fonts.md](ph3-variable-fonts.md) | Variable fonts (axes runtime) |
| [ph3-woff2-webfonts.md](ph3-woff2-webfonts.md) | WebFonts (@font-face + WOFF2) |
| [ph3-http3.md](ph3-http3.md) | HTTP/3 (QUIC) |
| [ph3-tls-security-hardening.md](ph3-tls-security-hardening.md) | TLS/security hardening (OCSP+CT+cert UI, Negotiate/NTLM+mTLS) |
| [ph3-gpu-process-sandbox.md](ph3-gpu-process-sandbox.md) | GPU process / sandbox |
| [ph3-gc-js-dom.md](ph3-gc-js-dom.md) | GC integration JS ↔ DOM (cross-boundary cycles) |
| [ph3-v8-migration.md](ph3-v8-migration.md) | Migrate JS engine to V8 (rusty_v8) |
| [ph3-cdp-shim.md](ph3-cdp-shim.md) | lumen-cdp-shim (Chrome DevTools Protocol subset) |
| [ph3-ai-module.md](ph3-ai-module.md) | AI module (lumen-ai) + semantic bookmarks |

### Фаза 4 — пост-v1.0 (платформы, экосистема, знание)

| Файл | Задача |
|---|---|
| [ph4-webgl.md](ph4-webgl.md) | WebGL subset (GLSL execution) |
| [ph4-knowledge-graph.md](ph4-knowledge-graph.md) | Knowledge graph visualization |
| [ph4-e2e-sync.md](ph4-e2e-sync.md) | E2E-encrypted sync |
| [ph4-ui-localization.md](ph4-ui-localization.md) | UI localization |
| [ph4-mobile.md](ph4-mobile.md) | Mobile (Android NDK) |

### Расхождения плана с кодом (найдены при декомпозиции)

Многие «будущие» пункты уже частично реализованы — DoD соответствующих файлов
помечает устаревшие записи. Ключевые:

- **WebGL** — GLSL ES интерпретатор уже написан (`crates/engine/paint/src/glsl.rs`),
  шейдеры исполняются; реальный gap — фреймбуфер `SoftwareWebGl::pixels()` не
  выводится на экран (только через `readPixels`).
- **IndexedDB / Service Workers / WebSockets / SSE / Fetch** — работают end-to-end
  против реального `HttpClient`, не заглушки. Остаток = hardening.
- **Variable fonts** — реально ~90%: femtovg-бэкенд (окно) рендерит default
  instance, варьируются только CPU/wgpu пути.
- **bfcache / extensions / permissions / downloads / cert UI / Navigation API /
  AiBackend** — у всех рабочие слои/трейты; файлы описывают точечный остаток.
- **Хранение в OS-каталогах** (`%APPDATA%`) вместо портативного `<exe>/data/`:
  `lumen_idb_dir` (`main.rs:4304`), `extensions_dir` (`extensions/mod.rs:80`) —
  заведено как [BUG-235](../../BUGS.md), помечено в task-файлах как Step 1.
