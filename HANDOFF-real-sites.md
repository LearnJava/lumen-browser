# Послание следующей сессии: рендеринг реальных сайтов

**Дата:** 2026-06-15. **Автор:** предыдущая сессия (Opus, общий ассистент, не P1–P5).
**Задача пользователя:** открыть реальный сайт в Lumen и довести его рендеринг до рабочего вида. Изначально просили ozon.ru — оказался недостижим (см. §1), переключились на **lenta.ru** как целевой.

---

## 0. Состояние рабочего дерева (НЕ закоммичено)

- `crates/shell/src/main.rs` — **2 осознанные правки** (см. §2). Собираются: `cargo build -p lumen-shell` OK.
- `graphic_tests/results/latest.json`, `last_session.db-shm`, `last_session.db-wal` — **побочные эффекты прогонов браузера, откатить**: `git checkout -- graphic_tests/results/latest.json last_session.db-shm last_session.db-wal`.
- `lumen.exe`, `test46.log`, `.git-rewrite/` — мусор/чужое, не трогать (`.git-rewrite/` — следы filter-repo из памяти проекта).

Правки лежат в main worktree без ветки. Перед продолжением: создать ветку (прямой коммит в main запрещён), напр. `feature-real-site-rendering`, перенести туда правки `main.rs`.

---

## 1. ozon.ru — НЕ тратить время

Ozon защищён **JS-антибот-челленджем** (страница «Antibot Challenge Page», `cdn2.ozone.ru/s3/abt-challenge/`):
```
GET /         → 307 + кука __Secure-ETC → /?__rr=1
GET /?__rr=1  → 403 «enable JavaScript to continue»
```
Проверено: `curl` с идеальными заголовками Edge/Chrome 130 + Client Hints + cookie jar получает тот же 403. Дело не в заголовках/TLS — нужно исполнить обфусцированный антибот-JS (WebCrypto/canvas/navigator-фингерпринт). Без этого недостижим. **Вывод: ozon как цель отложить до полноценного JS+Web API.**

Если упирались в «бесконечный редирект `__rr=1,2,3…`» в dump-режиме — это уже починено (§2.1).

---

## 2. Уже сделанные правки (в `crates/shell/src/main.rs`)

### 2.1. Cookie jar в dump-режиме (`run_dump`, ~стр. 857)
Было: `--dump-*` передавали `cookie_jar=None` → кука не возвращалась → бесконечный цикл редиректов. Стало: dump создаёт `CookieJar::open_in_memory()`, как оконный режим. Цикл устранён.

### 2.2. Кросс-доменные CSS больше не блокируются (`load_linked_stylesheets`, ~стр. 2478) — ВАЖНОЕ
Было: Lumen жёстко резал `<link rel=stylesheet>` на другой домен («sop: cross-origin stylesheet», Phase-0 упрощение). → **любой сайт с CSS на CDN-поддомене рендерился без стилей.** Стало: кросс-доменный CSS грузится (no-cors, как в браузерах) и применяется. Проверено: `icdn.lenta.ru/...css` → 200, применён.
> ⚠️ Это меняет SOP-семантику. Если потребуется ревью безопасности — обосновано тем, что браузеры применяют кросс-доменный CSS по умолчанию; CORS гейтит лишь чтение `cssRules` из скрипта, чего Lumen наружу не отдаёт.

---

## 3. lenta.ru — текущее состояние рендеринга

**Сайт ОТКРЫВАЕТСЯ:** 200 → parse → layout → paint, заголовок окна «Lenta.ru — Новости России… — Lumen», CSS+шрифты грузятся, JS частично исполняется (QuickJS). Display list — ~850 примитивов, новости реально сверстаны (заголовки, картинки, даты).

**НО во вьюпорте (1024×720) — почти белый экран.** Корневые причины (диагностировано через `--dump-layout` / `--dump-display-list`):

### Баг A (главный): контент начинается с y≈1004, ниже сгиба 720px
Верхние ~1004px заняты, видимого контента там нет → пользователь видит только белый верх.
Layout-дерево верха `body (overflow=auto/scroll)`:
```
Block (0,0,0,0) w=0 h=0 overflow=hidden        ← SVG-спрайт иконок (намеренно 0×0, НОРМА)
Block (0,0,1024,720) display=flex opacity=0 visibility=hidden  ← ПРЕЛОADER на весь экран
  Block (487,0,50,50) ...                        ← спиннер
Block (0,0,1280,270) bg=#292929 min-w=1280       ← тёмная шапка
...новости (display list с y=1024)
```
Гипотеза: полноэкранный preloader (720px, `opacity:0; visibility:hidden`) в реальном браузере `position:fixed` (вне потока), а в Lumen занимает 720px в нормальном потоке и толкает контент вниз. Lumen position:absolute/fixed **поддерживает** (`crates/engine/layout/src/box_tree.rs`: стр. 4232, 4595, 4900, 6552, 8921, 8974 и др.) — значит баг в конкретном случае: либо preloader не помечен fixed (CSS-правило не распарсилось / зависит от JS), либо fixed-элемент в flex-контексте всё равно отдаёт высоту в поток.
**Старт расследования:** найти этот preloader-div в `--dump-layout`, проверить его `position` (дамп его НЕ печатает — добавить временно в `serialize_layout_tree`, либо грепнуть CSS lenta на `position:fixed`/`.preloader`/`.loader`). Затем — почему его высота попала в поток родителя.

### Баг B: фон тёмной шапки `#292929` не рисуется
В layout есть `Block (0,0,1280,270) bg=#292929`, но в display list **нет** соответствующего `FillRect #292929` — первые заливки белые, сразу за ними `FillRoundedRect (0,1004,...)`. Фон шапки теряется при отрисовке. Проверить путь background-paint в `crates/engine/paint/src/display_list.rs` для позиционированных/перекрытых блоков.

### Баг C: страница свёрстана шириной 1280px при вьюпорте 1024
`min-width:1280px` на контейнере → картинки на x=940/1090/1195, правая колонка за кадром. Поведение само по себе корректное (горизонтальный overflow), но без горизонтального скролла/уменьшения масштаба правый столбец не виден. Низкий приоритет — это «как в браузере при узком окне».

---

## 4. Прочие найденные баги (для P3/P4 — НЕ чинил, оформить BUG-NNN)

- 🔴 **woff2 не декодируется**: `@font-face: не декодирован WOFF: unexpected end of font data`. woff2-шрифты падают, спасает только woff-fallback. → `crates/engine/font/` (декодер woff2). Затрагивает любой сайт со шрифтами в woff2.
- 🔴 **HTTP/2 HPACK на ya.ru**: `HPACK: dynamic table size update exceeds negotiated max` → ya.ru вообще не грузится. → `crates/network/src/h2/hpack.rs` (обработка dynamic table size update vs negotiated max).
- 🟡 **QuickJS**: не реализованы WebCodecs, SVG DOM API (`init failed: Exception generated by QuickJS`), плюс `expecting ';'`, `cannot read property 'length'`, `no setter for property`. Не блокирует базовый рендер, но ломает динамику.
- 🟡 **Детектор кодировки**: для example.com (чистый ASCII) выдал `ibm866`. → `crates/engine/encoding/`.
- 🟡 **Трекеры не резолвятся** (`os error 11004` WSANO_DATA на mc.yandex.ru/counter.rambler.ru/...). Для контента неважно, но возможен баг DNS-резолвера (тип записи). → `crates/network/src/dns.rs`.

---

## 5. Полезные команды (Windows, Git Bash)

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo build -p lumen-shell                                   # сборка

# Диагностика без GUI (быстро):
./target/debug/lumen.exe --dump-layout       https://lenta.ru/ 2>&1 | grep -v preload | head -80
./target/debug/lumen.exe --dump-display-list https://lenta.ru/ 2>&1 | grep -E "FillRect|FillRoundedRect|DrawImage" | head -30
./target/debug/lumen.exe --dump-source       https://lenta.ru/    # сырой HTML

# Скриншот окна (как graphic_tests/run.py):
./target/debug/lumen.exe https://lenta.ru/ >/tmp/run.log 2>&1 &
LPID=$!; sleep 12
# вывести окно на передний план (иначе GPU-бэкенд не перерисовывает!):
powershell -Command "... SetForegroundWindow по MainWindowTitle '*Lumen*' ..."   # см. историю сессии
sleep 1.5
./utils/ffmpeg.exe -f gdigrab -i desktop -vframes 1 -update 1 /tmp/shot.png -y
kill $LPID
```
**Важно про скриншот:** femtovg-бэкенд перерисовывает окно только при фокусе/redraw. Без `SetForegroundWindow` получишь пустой белый кадр (наступал на эти грабли).

Другие живые тест-сайты без агрессивного антибота: **lenta.ru** (200), **habr.com** (302→200). Мёртвые для Lumen: ozon.ru (антибот), ru.wikipedia.org (403 по UA), ya.ru (HPACK-баг).

---

## 6. Рекомендуемый порядок действий

1. Откатить побочные файлы (§0), создать ветку, перенести правки `main.rs`.
2. **Баг A** — главный для «сайт виден». Найти preloader, понять почему 720px в потоке. Скорее всего самый большой визуальный выигрыш.
3. **Баг B** — фон шапки. Вместе с A даст узнаваемый верх lenta.
4. Завести BUG-NNN на woff2 и HPACK (§4) — отдать P3/P4.
5. Скриншот до/после для подтверждения.

Память проекта (`~/.claude/.../memory/`) содержит: `project_femtovg_default_backend.md` (рендер через femtovg, НЕ wgpu — баги paint чинить в `femtovg_backend.rs`), `reference_ffmpeg_screenshot.md`, `feedback_screenshot_crop.md` (crop клиентской области: offset 8,39 на Win10).
