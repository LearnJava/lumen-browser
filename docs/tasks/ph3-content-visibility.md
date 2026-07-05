# Задача: CSS `content-visibility` + `contain-intrinsic-size` (доводка)

**Developer:** P1
**Ветка:** `p1-content-visibility`
**Размер:** S
**Крейты:** `lumen-css-parser`, `lumen-layout`, `lumen-shell`

## Goal

Довести `content-visibility` (CSS Containment L2 §4) до полноты: включить skip-rendering для боксов **выше** видимой области (не только ниже), и гарантировать, что `contain-intrinsic-size` даёт корректный размер-плейсхолдер для пропущенных off-screen поддеревьев, чтобы высота страницы и позиция скроллбара не «прыгали».

## Current state (сверено с кодом 2026-07-05)

Фича реализована сильнее, чем помечено в ROADMAP (PARTIAL). Фактически:

- **Парсинг** готов: `content-visibility` (`visible`/`auto`/`hidden`) — `crates/engine/layout/src/style.rs:13464`; `contain-intrinsic-size`/`-width`/`-height` + логические алиасы + shorthand — `style.rs:13477`, `13487`; парсеры `parse_contain_intrinsic_one` / `parse_contain_intrinsic_size` — `style.rs:15137`, `15153`. Оба свойства зарегистрированы в `crates/engine/css-parser/src/lib.rs:128`, `137`.
- **ComputedStyle**: поля `content_visibility: ContentVisibility` (`style.rs:3047`), `contain_intrinsic_width/height: Option<Length>` (`style.rs:3055`, `3058`); enum `ContentVisibility` (`style.rs:3631`); все NOT inherited, наследование в `compute_style` — `style.rs:15717`, `15724`.
- **Layout — `hidden`**: `style.rs`… → `box_tree.rs:3946` очищает детей при `content_visibility == Hidden`.
- **Layout — `auto` skip**: модуль `crates/engine/layout/src/content_visibility.rs` (BB-4) — thread-local ratchet (`cv_should_skip`, `set_cv_scroll`, `set_cv_relevant`, `take_cv_skipped`); вызов `cv_should_skip` в `box_tree.rs:5297`, очистка детей `box_tree.rs:5300`.
- **Layout — размер-плейсхолдер**: `size_contained` (`box_tree.rs:5343`) включается для `contain: size` ∨ `hidden` ∨ auto-skipped; `contained_content_height` (`box_tree.rs:5202`) даёт `contain-intrinsic-height` для высоты блока; ширина size-contained inline-block — `box_tree.rs:5474`.
- **Shell-протокол**: одноходовый (scroll → relevant → layout → drain), задокументирован в шапке `content_visibility.rs`.
- **Тесты**: 5 unit в `content_visibility.rs`; ~10 в `box_tree.rs` (`content_visibility_*`, `contain_intrinsic_size_*`); ~10 в `style.rs`. Graphic — только `114-contain-intrinsic-size.html` (size-hint), выделенного content-visibility нет.

**Реальный остаток (≈15%):**
1. **Above-viewport skip** — `content_visibility.rs:11` явно ограничивает skip только боксами, начинающимися **ниже** расширенного вьюпорта; боксы, ушедшие вверх за верхний край, не пропускаются (нужна оценка высоты до layout — плейсхолдер из `contain-intrinsic-*`).
2. **Размер-плейсхолдер для `auto`-skipped, у которого `contain-intrinsic-height` = None** — сейчас auto-высота коллапсирует в 0 (`contained_content_height` → 0.0), что для `auto` (в отличие от `hidden`) не идеально: без intrinsic-size высота должна оцениваться, а не обнуляться (иначе прыгает скролл). Проверить и, при необходимости, использовать последнюю известную высоту (ratchet-кэш).
3. Выделенный graphic-тест на content-visibility отсутствует.

## Entry points

- `crates/engine/layout/src/content_visibility.rs:86` — `cv_should_skip` (добавить верхнюю границу).
- `crates/engine/layout/src/box_tree.rs:5297` — точка вызова skip-проверки (start_y известен).
- `crates/engine/layout/src/box_tree.rs:5202` — `contained_content_height` (плейсхолдер-высота).
- `crates/engine/layout/src/box_tree.rs:5343` — вычисление `size_contained`.
- Shell-сторона (drain/ratchet скролла) — искать вызовы `take_cv_skipped`/`set_cv_scroll` в `crates/shell/src/` (grep `cv_scroll`, `cv_relevant`).

## Срезы (декомпозиция)

### Срез 1 — XS — плейсхолдер-высота для auto без intrinsic-size
В `contained_content_height` (`box_tree.rs:5202`) различить `hidden` (0 допустимо) и `auto`-skipped: для последнего при `contain_intrinsic_height == None` использовать оценку (константа/ratchet-кэш последней измеренной высоты), а не 0. Добавить unit-тест «auto-skipped без intrinsic-height не коллапсирует в 0».

### Срез 2 — S — above-viewport skip
Расширить `cv_should_skip` (`content_visibility.rs:86`): помимо нижней границы `sy + vh*(1+slack)`, добавить верхнюю `sy - vh*slack` — бокс, чей нижний край (start_y + оценка высоты) выше верхней границы, тоже пропускается. Оценку высоты брать из `contain-intrinsic-*` или ratchet-кэша. Обновить шапку модуля (снять оговорку «only below»). Добавить unit-тесты симметрично существующим (skip выше расширенного вьюпорта; не skip внутри полосы).

### Срез 3 — XS — синхронизация shell-ratchet
Проверить, что shell-сторона (drain `take_cv_skipped`) корректно обрабатывает боксы, ставшие релевантными при скролле **вверх** (не только вниз). При необходимости — минимальная правка в `crates/shell/src/…` (relayout при входе above-viewport бокса во вьюпорт).

### Срез 4 — XS — graphic-тест
Добавить `graphic_tests/NN-content-visibility.html` (магента-рамка): длинная колонка блоков с `content-visibility: auto` + `contain-intrinsic-size`, проверить стабильную высоту документа. Демо в `1000000-final.html`, строка в `COVERAGE.md` и `TESTS` в `run.py`.

## Tests

- Unit: `content_visibility.rs` — above/below симметрия skip; `box_tree.rs` — auto-skipped плейсхолдер-высота ≠ 0.
- Graphic: `NN-content-visibility.html` + демо в `1000000-final.html`.

## Definition of done

- [ ] `auto`-skipped бокс без `contain-intrinsic-height` не коллапсирует высоту в 0 (срез 1)
- [ ] Above-viewport skip работает симметрично below (срез 2)
- [ ] Shell relayout корректен при скролле вверх (срез 3)
- [ ] Выделенный graphic-тест зелёный; `COVERAGE.md` + `run.py` обновлены (срез 4)
- [ ] `cargo clippy -p lumen-layout --all-targets -- -D warnings` чистый
- [ ] `CAPABILITIES.md` / `CSS-SPECS.md:661` обновлены (above-viewport skip → ✅)
