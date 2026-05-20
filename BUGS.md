# BUGS.md — Баг-трекер Lumen Browser

Живой список известных багов движка. Пополняется из `python graphic_tests/run.py`.

**Как добавить баг:**
1. Скопируй скриншот в `graphic_tests/screenshots/bug-NNN-краткое-имя.png` (не коммитится)
2. Добавь запись в таблицу ниже

**Статусы:** `OPEN` · `IN PROGRESS` · `FIXED <date>` · `WONTFIX (Phase N+)`

---

## Сводная таблица

```
BUG-001 | FIXED 2026-05-15 | layout          | display:none on inline elements not working
BUG-003 | FIXED 2026-05-15 | layout          | style="" attribute not processed by cascade
BUG-030 | FIXED 2026-05-20 | layout          | IFC: no whitespace gap between inline-block siblings (CSS §4.1.2)
BUG-031 | FIXED 2026-05-20 | layout          | IFC: missing strut descent causes rows to be ~4px too short
BUG-023 | OPEN  REGRESSION | paint           | opacity property broken (was FIXED 2026-05-19, regression after)
BUG-024 | OPEN             | layout          | box-sizing: content-box — border not added to outer size
BUG-025 | OPEN             | layout          | max-height does not clamp block height
BUG-026 | OPEN             | layout/paint    | <img> CSS/HTML width+height ignored — renders at natural size
BUG-027 | OPEN  [P1]       | layout          | block element ignores explicit width — body stretches to viewport
BUG-028 | OPEN  [P3]       | shell           | relayout-on-resize + maximized window triggers BUG-027
BUG-029 | OPEN             | paint           | border-style: dotted renders square dots instead of circles
BUG-020 | OPEN             | layout/paint    | overflow: scroll/auto/hidden treated as visible
BUG-006 | OPEN  WONTFIX P1 | layout          | table layout not implemented (td/th render as blocks)
BUG-021 | OPEN             | html-parser     | HTML bgcolor attribute ignored
BUG-022 | OPEN             | css-parser      | Quirks-mode hashless hex colors not parsed
```

---

## Прогон 2026-05-20 (graphic_tests, --continue-on-fail)

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL 22.04%   color-named       ← BUG-027 (layout only, colors OK)
TEST-03: FAIL 32.12%   color-formats     ← BUG-027 (layout only, colors OK)
TEST-04: FAIL 15.67%   color-alpha       ← BUG-027 (layout only)
TEST-05: FAIL 13.67%   border-width      ← BUG-027 (layout only)
TEST-06: FAIL 23.12%   border-sides      ← BUG-027 (layout only)
TEST-07: FAIL  8.60%   box-sizing        ← BUG-024
TEST-08: FAIL 11.35%   padding           ← BUG-027 (layout only)
TEST-09: PASS  1.95%   margin
TEST-10: PASS  3.52%   min-max-width
TEST-11: FAIL 15.90%   min-max-height    ← BUG-025
TEST-12: FAIL 13.76%   display           ← BUG-027 + BUG-025
TEST-13: FAIL 16.58%   visibility-opacity← BUG-023 (regression)
TEST-14: FAIL 20.39%   overflow          ← BUG-020
TEST-15: FAIL  6.44%   box-shadow        ← BUG-027 (layout only)
TEST-16: FAIL 20.37%   outline           ← BUG-027 (outline itself works)
TEST-17: PASS  3.52%   calc
TEST-18: FAIL 31.73%   images            ← BUG-026 + BUG-027
TEST-19: FAIL 22.53%   object-fit        ← BUG-027 (layout only)
TEST-20: FAIL 30.62%   quirks-bgcolor    ← BUG-006/021/022
TEST-21: FAIL 19.07%   border-style      ← BUG-027 + BUG-029 (dotted=square)
```

**Выводы:**
- outline работает (BUG-019 закрыт визуально, TEST-16 fails из-за BUG-027)
- dashed / double рамки работают корректно
- Если исправить BUG-027+028 — пройдут тесты 02–06, 08, 12, 15, 16, 19 (~6 из 17 fail станут pass)
- BUG-023 (opacity) — **регрессия**: было FIXED 2026-05-19 (коммит `356ba0d`), снова OPEN

---

## Детали багов

### BUG-027 · Block-элемент игнорирует explicit `width` [P1]

**Статус:** OPEN  
**Компонент:** `lumen-layout` — block width computation  
**Обнаружен:** 2026-05-19

Block-элемент с `width: 400px` берёт 100% ширины viewport. При открытии максимизированного окна (~1920px) body растягивается до 1920px. Все тесты с `display: inline-block` внутри ограниченного body ломаются.

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — resolve block width. После `resolve_length` для `width`: если задан явно (не `auto`) — использовать это значение; если `auto` — брать `available_width`.

---

### BUG-028 · relayout-on-resize + `.with_maximized(true)` [P3]

**Статус:** OPEN  
**Компонент:** `lumen-shell` — `Lumen::relayout()`

Окно открывается максимизированным, winit сразу стреляет `Resized(~1920×1040)`. `relayout()` пересчитывает с viewport 1920px → BUG-027 проявляется.

**Временный фикс:** убрать `.with_maximized(true)` в `crates/shell/src/main.rs:1033`.

---

### BUG-023 · opacity игнорируется (РЕГРЕССИЯ)

**Статус:** OPEN (REGRESSION — было FIXED коммитом `356ba0d`, ветка `offscreen-opacity-1b4`)  
**Компонент:** `lumen-paint`

TEST-13: 6 боксов с opacity 0.1–1.0 в Edge дают градиент прозрачности. В Lumen все одинаковый сплошной цвет. Нужно найти что сломало фикс после мержа.

---

### BUG-024 · box-sizing: content-box — border не добавляется к outer size

**Статус:** OPEN  
**Компонент:** `lumen-layout` — box model

TEST-07: content-box боксы в Lumen уже чем в Edge на `2 × border_width`.

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — вычисление `rect.width` / `rect.height` для `content-box`.

---

### BUG-025 · max-height не зажимает высоту блока

**Статус:** OPEN  
**Компонент:** `lumen-layout` — block height clamping

TEST-11: При `height: 160px; max-height: 80px` блок рендерится 160px (max-height игнорируется).

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — после вычисления `height`, найти применение `min_height`/`max_height`.

---

### BUG-026 · `<img>` не масштабируется по CSS/HTML width/height

**Статус:** OPEN  
**Компонент:** `lumen-layout` / `lumen-paint`

TEST-18: `<img width="300" height="225">` рендерится в натуральном размере файла. Команда `DrawImage` должна использовать layout-rect, не натуральный размер текстуры.

---

### BUG-029 · border-style: dotted — квадратные точки вместо круглых

**Статус:** OPEN  
**Компонент:** `lumen-paint` — border rendering

TEST-21: `border-style: dotted` рисует квадратные точки. По CSS-спеке dots должны быть круглыми (filled circles). dashed и double работают корректно.

**Где смотреть:** `crates/engine/paint/src/display_list.rs` — секция отрисовки dotted-border, заменить FillRect на рисование окружностей через примитив или GPU-path.

---

### BUG-020 · overflow: scroll/auto/hidden не реализован

**Статус:** OPEN  
**Компонент:** `lumen-layout` / `lumen-paint`

TEST-14: все варианты overflow ведут себя как `visible`. В Edge видны scrollbar-ы и клиппинг.

---

### BUG-021 · HTML-атрибут bgcolor игнорируется

**Статус:** OPEN  
**Компонент:** `lumen-html-parser` (presentational hints)

TEST-20: `<body bgcolor="#1a2030">` даёт белый фон вместо тёмно-синего.

---

### BUG-022 · CSS hashless hex colors (Quirks-mode) не парсятся

**Статус:** OPEN  
**Компонент:** `lumen-css-parser`

TEST-20: `bgcolor="44aa66"` не распознаётся как `#44aa66` в quirks-mode.

---

## Ограничения Phase 0 (не баги — запланировано позже)

| Фича | Фаза |
|---|---|
| `display:inline-block` | Phase 1 |
| `float` | Phase 1 |
| `position:absolute/fixed/relative` | Phase 1 |
| `flexbox` (`display:flex`) | Phase 1 |
| `grid` | Phase 2 |
| `border-radius` | Phase 1 |
| `box-shadow` | Phase 1 |
| CSS-градиенты | Phase 2 |
| CSS-анимации | Phase 2 |
| Table layout (`BUG-006`) | Phase 1 |
| HiDPI / DPR-масштабирование | Phase 1 |
