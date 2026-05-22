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
BUG-007 | FIXED 2026-05-20 | layout          | <sub>/<sup>/<small> missing UA styles
BUG-008 | FIXED 2026-05-20 | layout          | <del>/<ins>/<u>/<s> text-decoration missing UA styles
BUG-009 | FIXED 2026-05-20 | layout          | <a> missing UA styles (no blue color, no underline)
BUG-012 | FIXED 2026-05-20 | layout          | <del>/<ins> break inline flow (each on new line)
BUG-016 | FIXED 2026-05-20 | css-parser/paint| border-style: dashed/double now work; dotted still square (→ BUG-029)
BUG-019 | FIXED 2026-05-20 | css-parser/paint| outline not rendered at all
BUG-027 | FIXED 2026-05-20 | layout          | block element ignores explicit width — body stretches to viewport
BUG-030 | FIXED 2026-05-20 | layout          | IFC: no whitespace gap between inline-block siblings (CSS §4.1.2)
BUG-031 | FIXED 2026-05-20 | layout          | IFC: missing strut descent causes rows to be ~4px too short
BUG-002 | FIXED 2026-05-20 | layout/paint    | inline padding/border/margin stacks vertically instead of flowing
BUG-004 | OPEN             | layout          | height on inline elements ignored
BUG-005 | FIXED 2026-05-21 | layout+paint    | <img> inside <span> not rendered
BUG-010 | FIXED 2026-05-20 | layout          | <hr> renders nothing
BUG-011 | OPEN             | layout/paint    | list markers (bullet, numbers) not rendered
BUG-013 | FIXED 2026-05-22 | layout          | adjacent <span style="..."> stack vertically without separator
BUG-014 | FIXED 2026-05-21 | image           | JPEG not decoded (PNG only)
BUG-015 | OPEN             | shell/paint     | broken <img> src shows no alt text
BUG-017 | FIXED 2026-05-22 | layout/paint    | text-decoration-style ignored (all render as solid)
BUG-018 | FIXED 2026-05-22 | layout          | text-decoration-color ignored (always inherits text color)
BUG-023 | OPEN             | layout+paint    | opacity deviation 2.20% — compositing correct; root: InlineBlockRow baseline + no edge-AA
BUG-024 | FIXED 2026-05-21 | layout          | box-sizing: content-box — border not added to outer size; height% resolved against width
BUG-025 | FIXED 2026-05-22 | layout          | max-height does not clamp block height; InlineSpace not included in shrink-to-fit width
BUG-026 | OPEN             | layout/paint    | <img> CSS/HTML width+height ignored — renders at natural size
BUG-028 | OPEN  [P3]       | shell           | relayout-on-resize + maximized window triggers BUG-027
BUG-029 | FIXED 2026-05-21 | paint           | border-style: dotted renders square dots instead of circles
BUG-020 | OPEN             | paint/layout    | overflow: scroll/auto — scrollbar UI не рендерится; hidden clip частично работает
BUG-006 | FIXED 2026-05-21 | layout          | table layout not implemented (td/th render as blocks)
BUG-021 | OPEN             | html-parser     | HTML bgcolor attribute ignored
BUG-022 | OPEN             | css-parser      | Quirks-mode hashless hex colors not parsed
BUG-032 | FIXED 2026-05-22 | paint/image     | object-fit image quality ~16%: area averaging заменяет bilinear при downscale
BUG-033 | OPEN             | paint           | box-shadow: нет Gaussian blur — рендерится solid прямоугольник вместо размытой тени
BUG-034 | OPEN             | layout          | CSS transform не реализован — translate/rotate/scale/skew/matrix игнорируются
BUG-035 | OPEN             | layout          | ::before/::after pseudo-elements не генерируются в box_tree (реализация частичная)
```

---

## Прогон 2026-05-21 v3 (graphic_tests, --continue-on-fail, порог 1%)

BUG-024 FIXED: height% теперь резолвится против высоты containing block, а не ширины. TEST-06 и TEST-07 перешли в PASS.
TableRow добавлен в paint (display_list.rs), TEST-25 PASS.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  0.39%   color-named
TEST-03: PASS  0.11%   color-formats
TEST-04: PASS  0.39%   color-alpha
TEST-05: PASS  0.37%   border-width
TEST-06: PASS  0.26%   border-sides       ← BUG-024 FIXED
TEST-07: PASS  0.70%   box-sizing         ← BUG-024 FIXED
TEST-08: PASS  0.93%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: FAIL 13.77%   min-max-height     ← BUG-025
TEST-12: FAIL 11.27%   display            ← BUG-025 + display modes
TEST-13: PASS  0.24%   visibility-opacity
TEST-14: FAIL  2.68%   overflow           ← BUG-020
TEST-15: FAIL  1.92%   box-shadow         ← BUG-033
TEST-16: FAIL  1.88%   outline            ← sub-pixel геометрия
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 10.77%   images             ← BUG-026
TEST-19: FAIL 13.00%   object-fit         ← BUG-032
TEST-20: FAIL  8.68%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  1.75%   border-style       ← остаточный BUG-029
TEST-22: FAIL  9.79%   CSS transform      ← BUG-034
TEST-23: PASS  0.00%   pseudo-elements
TEST-24: PASS  1.10%   vertical-align
TEST-25: PASS  0.00%   table-layout       ← TableRow paint FIXED
```

---

## Прогон 2026-05-21 v2 (graphic_tests, --continue-on-fail, порог 1%)

Инфраструктура: полная 1px магента-рамка (body #ff00ff + .__f wrapper), overflow:hidden на body.
Устранены ложные срабатывания от Edge-scrollbar: 10 тестов перешли FAIL→PASS.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  0.39%   color-named
TEST-03: PASS  0.11%   color-formats
TEST-04: PASS  0.39%   color-alpha
TEST-05: PASS  0.37%   border-width
TEST-06: FAIL  2.43%   border-sides       ← BUG-024 (box-sizing) + BUG-020 overflow
TEST-07: FAIL  6.56%   box-sizing         ← BUG-024
TEST-08: PASS  0.93%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: FAIL 14.02%   min-max-height     ← BUG-025
TEST-12: FAIL 11.27%   display            ← BUG-025 + display modes
TEST-13: PASS  0.24%   visibility-opacity
TEST-14: FAIL  6.89%   overflow           ← BUG-020 (scrollbar UI отсутствует)
TEST-15: FAIL  1.92%   box-shadow         ← BUG-033 (solid тень, нет blur)
TEST-16: FAIL  1.88%   outline            ← sub-pixel геометрия
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 11.06%   images             ← BUG-026
TEST-19: FAIL 12.62%   object-fit         ← BUG-032
TEST-20: FAIL 27.84%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  1.77%   border-style       ← BUG-029 частично исправлен, ещё >1%
TEST-22: FAIL  8.39%   CSS transform      ← BUG-034 (transform не реализован)
TEST-23: FAIL  5.97%   pseudo-elements    ← BUG-035 (::before/::after не рендерятся)
```

**Сравнение с предыдущим прогоном (v1, старая .__m полоска):**

| Тест | Было | Стало | |
|---|---|---|---|
| TEST-01 sanity | 0.00% | 0.00% | = |
| TEST-02 color-named | 2.35% FAIL | 0.39% PASS | ▼ ложный FAIL устранён |
| TEST-03 color-formats | 2.06% FAIL | 0.11% PASS | ▼ |
| TEST-04 color-alpha | 2.35% FAIL | 0.39% PASS | ▼ |
| TEST-05 border-width | 3.89% FAIL | 0.37% PASS | ▼ |
| TEST-08 padding | 4.45% FAIL | 0.93% PASS | ▼ |
| TEST-09 margin | 1.95% FAIL | 0.00% PASS | ▼ |
| TEST-10 min-max-width | 3.52% FAIL | 0.00% PASS | ▼ |
| TEST-13 opacity | 2.20% FAIL | 0.24% PASS | ▼ |
| TEST-17 calc | 3.52% FAIL | 0.00% PASS | ▼ |

Все улучшения — устранение ложных FAIL от Edge scrollbar (3.52% = 15px scrollbar × 2 стороны).

---

## Прогон 2026-05-21 (graphic_tests, --continue-on-fail, порог 1%)

Инфраструктура: foreground-window fix (Alt-trick), Edge timeout 60s, калибровка по периметру.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity                 ← было 38.98% — foreground fix устранил смещение
TEST-02: FAIL  2.35%   color-named            ← sub-pixel антиалиасинг
TEST-03: FAIL  2.06%   color-formats          ← sub-pixel антиалиасинг
TEST-04: FAIL  2.35%   color-alpha            ← rgba edge rendering
TEST-05: FAIL  3.89%   border-width           ← sub-pixel рендеринг границы
TEST-06: FAIL  5.95%   border-sides           ← BUG-024 (box-sizing)
TEST-07: FAIL  8.60%   box-sizing             ← BUG-024
TEST-08: FAIL  4.45%   padding                ← padding + sub-pixel
TEST-09: FAIL  1.95%   margin                 ← margin edge
TEST-10: FAIL  3.52%   min-max-width          ← min/max width clamping
TEST-11: FAIL 17.54%   min-max-height         ← BUG-025
TEST-12: FAIL 13.23%   display                ← BUG-025 + display modes
TEST-13: FAIL  2.20%   visibility-opacity     ← BUG-023
TEST-14: FAIL 10.41%   overflow               ← BUG-020
TEST-15: FAIL  3.87%   box-shadow
TEST-16: FAIL  5.40%   outline                ← BUG-024 геометрия
TEST-17: FAIL  3.52%   calc
TEST-18: FAIL 14.58%   images                 ← BUG-026 (было 14.68%)
TEST-19: FAIL 16.54%   object-fit             ← BUG-032 (86% было ложным — устаревший бинарник; реальный baseline 16%)
TEST-20: FAIL 30.49%   quirks-bgcolor         ← BUG-021 + BUG-022
TEST-21: FAIL  5.28%   border-style
TEST-22: FAIL 13.31%   CSS transform          ← первый прогон
```

---

## Прогон 2026-05-20 v2 (graphic_tests, --continue-on-fail, порог 1%)

Порог снижен с 5% до 1%. Видно значительное улучшение по многим тестам после мержа IFC-фиксов (BUG-030, BUG-031).

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL  2.35%   color-named        ← sub-pixel антиалиасинг границ
TEST-03: FAIL  2.06%   color-formats      ← sub-pixel антиалиасинг
TEST-04: FAIL  2.35%   color-alpha        ← rgba edge rendering
TEST-05: FAIL  3.89%   border-width       ← sub-pixel рендеринг границы
TEST-06: FAIL  5.95%   border-sides       ← BUG-024 (box-sizing)
TEST-07: FAIL  8.60%   box-sizing         ← BUG-024
TEST-08: FAIL  4.45%   padding            ← padding + sub-pixel
TEST-09: FAIL  1.95%   margin             ← margin edge (1px over threshold)
TEST-10: FAIL  3.52%   min-max-width      ← min/max width clamping
TEST-11: FAIL 17.54%   min-max-height     ← BUG-025
TEST-12: FAIL 13.23%   display            ← BUG-025 + display modes
TEST-13: FAIL  2.20%   visibility-opacity ← BUG-023 (улучшилось: 16.58%→2.20%)
TEST-14: FAIL 10.41%   overflow           ← BUG-020
TEST-15: FAIL  3.87%   box-shadow         ← box-shadow rendering
TEST-16: FAIL  5.40%   outline            ← BUG-024 влияет на геометрию
TEST-17: FAIL  3.52%   calc               ← calc() sub-pixel
TEST-18: FAIL 14.68%   images             ← BUG-026
TEST-19: FAIL 16.14%   object-fit         ← object-fit не реализован
TEST-20: FAIL 30.49%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  5.28%   border-style       ← BUG-029 (dotted=square)
```

**Сравнение с предыдущим прогоном (до IFC-фиксов):**

| Тест | Было | Стало | Δ |
|---|---|---|---|
| TEST-02 color-named | 22.04% | 2.35% | ▼19.7 — BUG-027 устранён |
| TEST-03 color-formats | 32.12% | 2.06% | ▼30.1 — BUG-027 устранён |
| TEST-04 color-alpha | 15.67% | 2.35% | ▼13.3 — BUG-027 устранён |
| TEST-05 border-width | 13.67% | 3.89% | ▼9.8 — BUG-027 устранён |
| TEST-06 border-sides | 23.12% | 5.95% | ▼17.2 — BUG-027 устранён |
| TEST-08 padding | 11.35% | 4.45% | ▼6.9 — BUG-027 устранён |
| TEST-13 opacity | 16.58% | 2.20% | ▼14.4 — BUG-023 в основном исправлен |
| TEST-14 overflow | 20.39% | 10.41% | ▼10.0 |
| TEST-15 box-shadow | 6.44% | 3.87% | ▼2.6 |
| TEST-16 outline | 20.37% | 5.40% | ▼15.0 — BUG-027 устранён |
| TEST-18 images | 31.73% | 14.68% | ▼17.1 |
| TEST-19 object-fit | 22.53% | 16.14% | ▼6.4 |
| TEST-21 border-style | 19.07% | 5.28% | ▼13.8 — BUG-027 устранён |

**Выводы:**
- BUG-027 (block width) **фактически устранён** — все зависящие тесты упали на 10–30%
- BUG-023 (opacity) **существенно улучшился**: 16.58% → 2.20% (порог 1% не проходит, но регрессия устранена)
- Главные оставшиеся блокеры: BUG-024 (box-sizing), BUG-025 (max-height), BUG-020 (overflow), BUG-026 (images), BUG-021/022 (quirks-bgcolor)
- TEST-02..05, 08, 09, 13 проваливаются только из-за sub-pixel антиалиасинга: реальная разница < 4%, при пороге 1% неизбежны

---

## Прогон 2026-05-20 v1 (graphic_tests, --continue-on-fail, порог 5%)

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
- BUG-023 (opacity) — **регрессия**: было FIXED 2026-05-19 (коммит `356ba0d`), снова OPEN

---

## Детали багов

### BUG-027 · Block-элемент игнорирует explicit `width` [P1]

**Статус:** FIXED 2026-05-20
**Компонент:** `lumen-layout` — block width computation

Block-элемент с `width: 400px` берёт 100% ширины viewport. После фикса: если задан явно (не `auto`) — использовать это значение; если `auto` — брать `available_width`.

---

### BUG-028 · relayout-on-resize + `.with_maximized(true)` [P3]

**Статус:** OPEN  
**Компонент:** `lumen-shell` — `Lumen::relayout()`

Окно открывается максимизированным, winit сразу стреляет `Resized(~1920×1040)`. `relayout()` пересчитывает с viewport 1920px → BUG-027 проявляется.

**Временный фикс:** убрать `.with_maximized(true)` в `crates/shell/src/main.rs:1033`.

---

### BUG-023 · opacity sub-pixel deviation

**Статус:** OPEN (deviation 2.20% > 1% threshold; opacity compositing correct)  
**Компонент:** `lumen-paint` + `lumen-layout`

Opacity compositing математически корректен: `PushOpacity`/`PopOpacity` + off-screen layer composite shader (`c.rgb * in.alpha + white * (1 - in.alpha)`). TEST-13 (2.20%) не хуже TEST-02 color-named (2.35%) без opacity — т.е. opacity не добавляет ошибку.

Оставшиеся 2.2%: (1) ~0.6% — InlineBlockRow добавляет 3.86px descender-зону из-за font-baseline strut, смещая opacity-боксы относительно Edge; (2) ~1.6% — edge antialiasing: Edge сглаживает рёбра, Lumen нет.

**Для снижения ниже 1%:** P1-фикс InlineBlockRow baseline height + edge antialiasing в renderer (MSAA). Точечным фиксом в P2 не решается.

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

### BUG-032 · Качество масштабирования изображений: ~16% расхождение с Edge

**Статус:** OPEN  
**Компонент:** `lumen-paint`, `lumen-image`

TEST-19 (object-fit), TEST-18 (images): пиксельная разница ~16% при большом коэффициенте уменьшения (~4.7x, 852×725 → 180×120).

#### Что сделано (2026-05-21)

1. **CPU-side bilinear resize** — реализован в `lumen-image/src/lib.rs`:
   - `Image::to_rgba8()` — конвертация любого формата в RGBA8
   - `pub fn resize_bilinear(src: &Image, dst_w: u32, dst_h: u32) -> Image` — 4-tap bilnear с half-pixel offset
   - В `renderer.rs` добавлен pre-pass перед render loop: для каждого `DrawImage` вызывается `ensure_image_gpu_key()`, которая создаёт CPU-ресайзированную текстуру и кеширует под ключом `"src@WxH"`.
   - Разделение на `compute_image_gpu_key(&self)` (иммутабельный) + `ensure_image_gpu_key(&mut self)` (мутабельный pre-pass) обязательно — иначе borrow-checker блокирует (в render loop `parsed_faces: Vec<Option<ParsedFace<'_>>>` держит `&self.faces`).

2. **Результат:** минимальное улучшение: TEST-18 14.68% → 14.44%, TEST-19 16.14% → 16.54% (шум, не улучшение).

#### Почему не помогло

CPU bilinear ≈ GPU bilinear — оба делают 4-выборки. При коэффициенте уменьшения 4.7x область покрытия одного выходного пикселя = 4.7×4.7 = ~22 исходных пикселей, из которых bilinear учитывает лишь 4. Антиалиасинг не обеспечивается.

Edge/Chrome используют **Skia**, который при downscale применяет **Lanczos-3** (или area averaging) — усредняет все пиксели в области покрытия. Поэтому разные браузеры дают одинаковый результат: они используют одну библиотеку (Skia).

Дополнительная причина: текстуры загружаются как `Rgba8Unorm` (linear), хотя PNG-файлы хранят sRGB. Блендинг в linear-пространстве при правильных финальных значениях дал бы совпадение, но sRGB→linear конвертация при загрузке не выполняется → цветовые ошибки ~2-5%.

#### Что нужно сделать

1. **[Приоритет 1] Area averaging (box filter) для downscale:**
   ```rust
   // Заменить resize_bilinear на resize_area_avg для случаев (dst < src)
   pub fn resize_area_avg(src: &Image, dst_w: u32, dst_h: u32) -> Image;
   // Алгоритм: для каждого dst-пикселя вычислить float-прямоугольник в src-координатах,
   // усреднить все целые пиксели + частичные веса по краям.
   ```
   Ожидаемый результат: совпадение с Edge ~2-4% (только sRGB-девиация останется).

2. **[Приоритет 2] sRGB при загрузке текстур:**  
   Изменить формат текстуры с `Rgba8Unorm` на `Rgba8UnormSrgb` в `renderer.rs` → wgpu автоматически конвертирует sRGB→linear при sampling. Требует также перевода surface в sRGB (`TextureFormat::Bgra8UnormSrgb`). Запланировано на Phase 3+.

#### Файлы

- `crates/engine/image/src/lib.rs` — `to_rgba8()`, `resize_bilinear()`
- `crates/engine/paint/src/renderer.rs` — pre-pass, `ensure_image_gpu_key()`, `compute_image_gpu_key()`, `make_gpu_image_entry()`

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
