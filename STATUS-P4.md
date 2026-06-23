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

_(none)_ — последняя задача p4-svg-text-anchor влита 2026-06-21.

<!-- формат строки в работе:
In progress: <task>   branch: p4-<task>
Next step: <первый шаг>   <file.rs:line | docs/tasks/p4-<task>.md> -->

## Next

Очередь по приоритету. Один указатель = одна задача (отсутствие поля в `style.rs` проверено 2026-06-23).

| # | Задача | Указатель (описание по ссылке) | Размер |
|---|--------|--------------------------------|--------|
| 1 | `font-feature-settings` | `CSS-SPECS.md:221` (⬜ OT feature flags) | M |
| 2 | `font-palette` + `@font-palette-values` | `CSS-SPECS.md:224` · `crates/engine/layout/src/font_palette.rs:17` | M |
| 3 | `ruby-align` / `ruby-merge` / `ruby-position` | `crates/engine/layout/src/ruby.rs:76` | M |
| 4 | `math-style` / `math-depth` | `crates/engine/layout/src/mathml.rs:132` | S |
| 5 | `backface-visibility` 3D flip | `CSS-SPECS.md:289` | S |
| 6 | `hyphens: auto` | `CSS-SPECS.md:242` (none/manual ✅, auto ⬜ — нужен HyphenationProvider) | M |
| 7 | `forced-color-adjust` → Forced Colors Mode | `CSS-SPECS.md:207` (parsed ✅, рендер ⬜) | M |

Дальнейшие кандидаты — `CSS-SPECS.md` ⬜/🟡 по Tier и `grep -rn "// CSS:" crates/engine`.

## Recent (последние 5; полная история — `git log -- STATUS-P4.md`)

| Дата | Свойство | Указатель |
|------|----------|-----------|
| 2026-06-21 | `text-anchor` / `dominant-baseline` как CSS-свойства | `CSS-SPECS.md:121` |
| 2026-06-20 | `inverted-colors` media feature | `CSS-SPECS.md:93` |
| 2026-06-19 | `scripting` media feature | `CSS-SPECS.md:93` |
| 2026-06-19 | `prefers-reduced-transparency` media feature | `CSS-SPECS.md:93` |
| 2026-06-19 | `@supports font-tech()` / `font-format()` | `CSS-SPECS.md` (Conditional L4) |
