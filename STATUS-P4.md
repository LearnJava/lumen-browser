# STATUS-P4 — CSS Properties

**Developer:** Программист 4 (CSS implementation ONLY)

**Источник задач (по приоритету):**
1. `docs/tasks/p4-*.md` — разобранные задачи (если есть незакрытые).
2. `CSS-SPECS.md` — per-property ⬜/🟡 в секции `## Full Property Inventory`, порядок по Tier.
3. `// CSS:` хэндоффы в коде: `grep -rn "// CSS:" crates/engine`.

**Правило индекса:** строки ниже — только указатели `file:line` / `docs/tasks/…md`.
Описание задачи живёт по ссылке и здесь не дублируется (иначе файл дрейфует).

**⚠️ Перед взятием — сверь с кодом** (списки протухают): свойство реально не реализовано,
если `grep "<field>" crates/engine/layout/src/style.rs` пуст.
Полный workflow — CLAUDE.md §«CSS ownership: P4 only».

---

## In progress

_(none)_ — последняя задача p4-css-function влита 2026-07-15.

<!-- формат строки в работе:
In progress: <task>   branch: p4-<task>
Next step: <первый шаг>   <file.rs:line | docs/tasks/p4-<task>.md> -->

## Next

Полный аудит бэклога 2026-07-15: все три источника (P4 Work Queue, Full Property
Inventory, `// CSS:` хэндоффы в коде) собраны, дедуплицированы и сверены с кодом.
52 задачи ниже — реально открытые (подтверждено grep/чтением кода), не просто
записи из документа. Отдельно — 24 записи `CSS-SPECS.md`, которые оказались
дрейфом (см. секцию «Doc drift» ниже) — их закрывать не нужно, только поправить
статус в документе.

### Позиционирование

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `position: sticky` — scroll-driven paint transform + `%`/`em`/`rem` insets (сейчас только `Length::Px` через `to_px_opt()`) | `crates/engine/layout/src/lib.rs:504-540` (`StickyBox`/`collect_sticky_boxes`) · `crates/engine/layout/src/box_tree.rs:6771` | M |
| `anchor()` / `anchor-size()` функции (база anchor-name/position-anchor/inset-area уже сделана) | `CSS-SPECS.md:675` | M |

### Маскирование / компоузинг

| Задача | Указатель | Размер |
|--------|-----------|--------|
| CSS Masking остаток: `mask-composite` (нужна multi-layer `mask-image` инфраструктура — сейчас `mask_image` не список), `mask-repeat: space`, `mask-clip: no-clip/fill-box/stroke-box/view-box`, femtovg URL image-mask backend (сейчас no-op) | `CSS-SPECS.md:429-435` · `crates/engine/paint/src/display_list.rs:4887` | L |
| `isolation` — stacking-context изоляция | `CSS-SPECS.md:443` (auto/isolate парсится, изоляция ⬜) | M |

### Шрифты

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `font-variant-caps` полный набор (сейчас только small-caps) | `CSS-SPECS.md:219` | S |
| `font-stretch` — матчинг по stretch-axis (сейчас `%` парсится, не используется в подборе шрифта) | `CSS-SPECS.md:220` | S |
| `font-palette` / `@font-palette-values` — COLR/CPAL растеризация | `CSS-SPECS.md:225,227` (резолвится до `DrawText.font_palette`, рендерер игнорирует; уровень `lumen-font`, не только layout) | L |

### Текст / письмо

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `line-break` — CJK-aware перенос | `CSS-SPECS.md:246` | M |
| `writing-mode: vertical-*` — inline-текст всё ещё «сайдвейс»-стаб (block-уровень уже работает) | `crates/engine/layout/src/vertical.rs:13,69` · `crates/engine/layout/src/box_tree.rs:5386` | L |
| `direction` / `unicode-bidi` — полный Unicode Bidi Algorithm (сейчас только fragment mirroring) | `CSS-SPECS.md:644,647` | L |
| `text-orientation` — поворот глифов в вертикальном тексте | `CSS-SPECS.md:646` | M |

### Grid / Flexbox alignment

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `place-items` / `place-self` / `place-content` — применение к grid | `CSS-SPECS.md:567` | M |
| `justify-items` — контейнерный default для block-детей | `CSS-SPECS.md:283,565` | S |
| `masonry-auto-flow` свойство (сам алгоритм masonry уже готов) | `CSS-SPECS.md:483` | S |

### Transforms

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `perspective` / `perspective-origin` — реальная 3D-проекция при рендере | `CSS-SPECS.md:293,498` | L |

### Backgrounds

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `background-attachment: scroll/fixed` — поведение | `CSS-SPECS.md:463` | M |
| `background-clip: text` — рендер | `CSS-SPECS.md:464` | M |
| `cross-fade()` — paint-композитинг (парсинг/хранение уже готовы) | `CSS-SPECS.md:470` | M |

### Scroll / overscroll

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `scroll-margin*` / `scroll-padding*` — применение к геометрии snap-порта | `CSS-SPECS.md:520` | M |
| `overscroll-behavior*` — gesture boundary | `CSS-SPECS.md:522` | M |
| `scrollbar-gutter` block-axis — `scrollbar_gutter_block()` есть, но `#[allow(dead_code)]`, нигде не вызывается (inline-версия уже вызывается) | `crates/engine/layout/src/box_tree.rs:98` | S |

### Multi-column

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `column-span` — layout спэннинг-элемента | `CSS-SPECS.md:532` | M |
| `column-fill` — балансировка колонок | `CSS-SPECS.md:533` | M |

### UI / ввод

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `user-select` — enforcement выделения текста (HitTest уже проведён) | `CSS-SPECS.md:588` | M |
| `pointer-events: auto` — shell-level enforcement | `CSS-SPECS.md:589` | M |
| `touch-action` — обработка жестов | `CSS-SPECS.md:590` | L |
| `resize` — drag-UI (грипп уже рисуется, драг не работает) | `CSS-SPECS.md:591` | M |
| `caret-color` — привязка к каретке текстового поля | `CSS-SPECS.md:593` | S |
| `inert` — не хватает UA-правила `[inert] { pointer-events: none; }` (layout-фильтрация уже есть, но `ComputedStyle.pointer_events` этого не отражает) | `crates/engine/layout/src/inert.rs:19-27` · `crates/engine/layout/src/lib.rs:306` | S |

### At-rules / values

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `@media` — live-обновление `MediaQueryList` при resize (сам шим `matchMedia` уже есть, нужно проверить/дошить change-событие) | `CSS-SPECS.md:704` · `crates/js/src/dom.rs:10107` | S |
| `@import` — реальная загрузка внешнего файла (сейчас URL только извлекается) | `CSS-SPECS.md:602` | M |
| `@import layer()` модификатор | `CSS-SPECS.md:393` | S |
| `content: url()` значение | `CSS-SPECS.md:558` | S |
| `env()` — UA registry для `safe-area-inset-*`/`titlebar-area-*` (низкий приоритет, desktop) | `CSS-SPECS.md:507,632` | S |
| `stretch` / `available` — различимая семантика (сейчас алиас на `FitContent(None)`) | `CSS-SPECS.md:492` | S |
| `offset-anchor` + `url()` motion paths (offset-path/distance/rotate/ray уже готовы) | `CSS-SPECS.md:654` | S |
| `content-visibility` — above-viewport skip + `contain-intrinsic-size` интеграция (hidden/below-viewport уже готовы) | `CSS-SPECS.md:660,661` | M |
| `@color-profile` — реальная ICC-трансформация + валидация declared-name (см. BUG-282) | `CSS-SPECS.md:609,684` | M |
| `@function` — типизация `returns` + условные group rules в теле | `CSS-SPECS.md:613` | M |

### Houdini / продвинутое (межкрейтовая координация с JS)

| Задача | Указатель | Размер |
|--------|-----------|--------|
| CSS Paint API — реальное исполнение `paint(name)` worklet (сейчас захардкоженный серый прямоугольник) | `crates/engine/paint/src/display_list.rs:4805` · `crates/engine/layout/src/style.rs:18404` | L |
| `::picker(select)` / `appearance: base-select` — стилизация (сейчас всегда нативный `<select>`) | `crates/engine/layout/src/box_tree.rs:438,3859` · `crates/js/src/dom.rs:5316` | M |
| `::view-transition-group()` / `-image-pair()` / `-old()` / `-new()` — функциональные псевдоэлементы (сейчас захардкожен 300ms) | `crates/engine/layout/src/lib.rs:1319` | M |

### Списки / прочее малое

| Задача | Указатель | Размер |
|--------|-----------|--------|
| `list-style-position` — применение позиционирования | `CSS-SPECS.md:382` | S |
| `shape-outside` — алгоритм обтекания float по форме (парсинг, включая `path()`, готов) | `CSS-SPECS.md:375,653` | L |
| Logical properties RTL/vertical: `block-size`/`inline-size`/min-max-варианты сейчас только LTR | `CSS-SPECS.md:305,306` | M |
| `@nest` legacy at-rule (низкий приоритет, deprecated синтаксис) | `CSS-SPECS.md:345` | S |
| `print-color-adjust` / `color-adjust` — print-рендеринг (низкий приоритет: paged media вне скоупа проекта) | `CSS-SPECS.md:209` | S |

---

## Doc drift — правки статуса CSS-SPECS.md, НЕ задачи реализации

Найдено при сверке 2026-07-15: `## Full Property Inventory` содержит строки
🟡/⬜, которые противоречат `## P4 Work Queue` / Tier-таблицам (там ✅ с датой
и деталями) или прямому чтению кода. Значит фича готова, а маркер в Inventory
протух. Чинится одной мелкой правкой статуса, без кода:

- `CSS-SPECS.md:627` `var()` substitution — WQ#1 (`CSS-SPECS.md:694`) подтверждает ✅ (40 тестов).
- `CSS-SPECS.md:322-326` transitions per-frame interpolation — WQ#2 (`:695`) ✅.
- `CSS-SPECS.md:332-338,606` `@keyframes`/animation scheduler — WQ#3 (`:696`) ✅.
- `CSS-SPECS.md:351-354` table display layout engine — Tier1 #5 (`:53`) ✅ (BoxKind::Table, 6 тестов); residual — только `caption-side`/`table-layout` не покрыты отдельным пунктом Next, но это не layout-engine.
- `CSS-SPECS.md:226,605` `@font-face` file loading — WQ#20 (`:713`) ✅ (font-display:swap, async fetch).
- `CSS-SPECS.md:384` `counter-reset`/`counter-increment` resolution — T3 Counters (`:550`) ✅ (`precompute_counters()`).
- `CSS-SPECS.md:422` `backdrop-filter` compositing — Tier2 #13 (`:66`) + WQ#28 (`:721`) ✅ (LRU cache).
- `CSS-SPECS.md:459,461` multiple backgrounds / `background-image` marker — Tier2 note (`:70`) + WQ#18 (`:711`) ✅.
- `CSS-SPECS.md:482` `subgrid` marker — WQ#30 (`:723`) ✅, маркер строки просто не обновлён.
- `CSS-SPECS.md:476` `grid-template-columns`/`rows` marker — собственный note той же строки описывает полную поддержку.
- `CSS-SPECS.md:603` `@media` condition eval — Tier1 #12 (`:60`) ✅; реальный остаток — только матчмедиа live-update, см. задачу в Next.
- `CSS-SPECS.md:604` `@supports` feature detection — Tier3 #36 (`:94`) ✅ (`selector()`/`font-tech()`/`font-format()`).
- `CSS-SPECS.md:187` `contain-intrinsic-size` — Tier2 #38 (`:96`) ✅ (done 2026-06-14).
- `CSS-SPECS.md:269,451` `::marker` — WQ#16 (`:709`) ✅ (`MarkerBox` в `box_tree.rs`).
- `CSS-SPECS.md:271` `::placeholder` — `PseudoElementKind::Placeholder` уже есть, `crates/engine/layout/src/style.rs:7221`.
- `CSS-SPECS.md:268,450` `::first-line`/`::first-letter` segment override — уже реализовано, `apply_first_letter_pseudo`/`apply_first_letter_style` в `crates/engine/layout/src/box_tree.rs:1664-1995`.
- `CSS-SPECS.md:283` vs `:565` `justify-items` grid cells — внутреннее противоречие (одна строка ✅, другая ⬜ для того же); grid-cells реализация старая и рабочая, protuh только `:565`.
- `CSS-SPECS.md:434` `mask-origin` — note описывает полный wiring, маркер 🟡 не обновлён.
- `CSS-SPECS.md:645` `writing-mode` marker — WQ#29 (`:722`) говорит ✅, но это верно только для block-axis; inline-текст всё ещё стаб — см. задачу в Next, маркер надо перевести в 🟡 с уточнением, не в ✅.
- `CSS-SPECS.md:654` motion path `offset-*` — Tier4 #44 (`:107`) ✅ для offset-path/distance/rotate/ray; residual только `offset-anchor`/`url()`, см. задачу в Next.
- `CSS-SPECS.md:534,535` fragmentation (`break-*`/`orphans`/`widows`) — Tier4 #45 (`:108`) ✅ (`pagination.rs`); paged media вне скоупа проекта, дальнейший residual покрыт `column-span`/`column-fill` в Next.
- `CSS-SPECS.md:207` `color-scheme` UA switching — уже реализовано полностью (form controls + system-color resolution), `crates/engine/layout/src/style.rs:6394,6973,7020,12391-12393`.
- `CSS-SPECS.md:581` `scrollbar-width`/`scrollbar-color` rendering — визуальный рендер уже есть (`crates/engine/paint/src/display_list.rs:5282,5291`); реальный residual — только `scrollbar-gutter` block-axis, см. задачу в Next.
- `CSS-SPECS.md:703` (WQ#10) `:is()`/`:where()`/`:has()` matching — маркер был `M`/`none`, хотя Tier1 (`:59`) уже отмечает ✅ 2026-05-24; проверено чтением кода (`PseudoClass::Is/Where/Has` в `parser.rs`, `matches_relative` в `layout/src/style.rs`) и тестами (`cargo test -p lumen-layout --lib has_` → 20/20 зелёных). Найдено 2026-07-15 при аудите перед взятием задачи из Next.

## Recent (последние 5; полная история — `git log -- STATUS-P4.md`)

| Дата | Свойство | Указатель |
|------|----------|-----------|
| 2026-07-15 | `revert` — UA-stylesheet revert через `ua_baseline`-снэпшот в `compute_style` (был alias на `unset`) | `CSS-SPECS.md:162` |
| 2026-07-15 | `:state()` custom-state pseudo-class — `PseudoClass::State`, парсинг + матчер на `data-lumen-state-<name>` sentinel-атрибут | `crates/js/src/element_internals.rs:183` |
| 2026-07-15 | `@layer`/`@scope`/`@container` nested at-rule cascade wiring (2nd pass) — nested conditional-group at-rules внутри body теперь bubble на stylesheet-уровень (`Parser::bubbled`) | `CSS-SPECS.md:720` |
| 2026-07-15 | `@function` (CSS Functions and Mixins L1) — парсинг + вызов `--name(<args>)` в property value | `CSS-SPECS.md:613` |
| 2026-07-1x | `@color-profile` — парсинг + `color(--name ...)` в `color()` | commit `6dfbfb7e` |
</content>
