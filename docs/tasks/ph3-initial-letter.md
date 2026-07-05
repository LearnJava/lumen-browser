# Задача: CSS `initial-letter` — буквица (drop-cap), доводка

**Developer:** P1
**Ветка:** `p1-initial-letter`
**Размер:** M
**Крейты:** `lumen-layout` (при необходимости `lumen-paint`)

## Goal

Довести `initial-letter` (CSS Inline Layout L3 §5) от Phase-0-аппроксимации до спек-точности: корректное выравнивание буквицы по cap-height/baseline (а не приближение `font-size = size × line-height`), поддержка raised-cap (`sink < size`) и RTL/`inline-start` (сейчас только LTR/left).

## Current state (сверено с кодом 2026-07-05)

Phase 0 реализован (drop-cap как inline-start float). Работает базовый случай, но с явными аппроксимациями.

- **Парсинг**: `initial-letter: normal | <number> <integer>?` — `parse_initial_letter` (`crates/engine/layout/src/style.rs:11693`), apply — `style.rs:13381`. Валидирует size ≥ 1, sink ≥ 1 (`0.5` и `3 0` отклоняются).
- **ComputedStyle**: `initial_letter_size: f32` (`style.rs:2581`, initial 1.0), `initial_letter_sink: u32` (`style.rs:2585`, initial 0 = auto = `floor(size)`); NOT inherited.
- **Layout**: `extract_initial_letter` (`box_tree.rs:2105`) — промотирует первую букву в inline-start float `Block`, спан `size × line-height`, текст обтекает через float-машинерию. Точка активации + приоритет `::first-letter`-псевдо над свойством элемента — `box_tree.rs:4157`–`4179`. Эффективная буквица включается при `size > 1`.
- **Тесты**: unit `parse_initial_letter` (`style.rs:20637`); drop-cap layout — модуль `initial_letter` в `box_tree.rs:11753` (element/pseudo extract, sink, `normal` не создаёт cap). Graphic — демо в `1000000-final.html` (2 кейса), помечено как text-parity debtor (BUG-100).

**Явные аппроксимации Phase 0 (шапка `box_tree.rs:2094`), = остаток (≈40%):**
1. **Cap-height/baseline** — вместо точного выравнивания cap-height буквицы по baseline `sink`-й строки используется `font-size = size × parent line-height` + клип по высоте `sink` строк. Нужна метрика cap-height из шрифта (`lumen-font`) и точное позиционирование baseline.
2. **Raised-cap** — `sink < size` (буквица выше блока текста, «поднятая») не поддержан отдельно; сейчас sink трактуется как число зарезервированных строк, поднятие не моделируется.
3. **RTL / `inline-start`** — `box_tree.rs:2094` фиксирует «inline-start = left (LTR only)»; для RTL/`direction: rtl` буквица должна вставать справа.

## Entry points

- `crates/engine/layout/src/box_tree.rs:2105` — `extract_initial_letter` (геометрия буквицы).
- `crates/engine/layout/src/box_tree.rs:4157` — вычисление эффективной буквицы + активация.
- `crates/engine/layout/src/style.rs:2581/2585` — поля size/sink.
- `crates/engine/font/` — источник cap-height/ascent метрик (grep `cap_height`, `ascent`); проверить, экспонирована ли метрика в `TextMeasurer`.

## Срезы (декомпозиция)

### Срез 1 — S — cap-height метрика
Проверить/добавить доступ к cap-height (или ascent как приближение) буквенного глифа через `TextMeasurer`/`lumen-font`. Если метрики нет — минимальное расширение trait в `lumen-core::ext` + реализация в font-крейте. Unit-тест: метрика для Inter 'A' в разумном диапазоне.

### Срез 2 — M — точное baseline-выравнивание (drop-cap)
В `extract_initial_letter` заменить `font-size = size × line-height` на: подобрать размер глифа так, чтобы его cap-height покрывал ровно `size` строк, а baseline буквицы совпадал с baseline `size`-й (или `sink`-й) строки текста. Позиционировать float по точному cap-top, а не по строчному боксу. Обновить unit-тесты геометрии (`box_tree.rs:11753`), сверить с ожидаемыми пикселями.

### Срез 3 — S — raised-cap (`sink < size`)
Смоделировать поднятую буквицу: когда `sink < size`, буквица выступает над первой строкой блока на `(size − sink)` строк вверх (в надлежащий отрицательный оффсет), текст обтекает только `sink` строк. Unit-тест: `initial-letter: 3 1` даёт raised-cap.

### Срез 4 — S — RTL / inline-start
Учесть `direction: rtl` (и/или logical inline-start): буквица встаёт у правого края, float становится right-float. Снять оговорку «LTR only» из шапки `box_tree.rs:2094`. Unit-тест на `direction: rtl`.

### Срез 5 — XS — graphic-тест
Расширить демо в `1000000-final.html` кейсами raised-cap и RTL; при желании выделенный `graphic_tests/NN-initial-letter.html` (магента-рамка). Обновить `COVERAGE.md` + `run.py`. Учитывать text-parity debtor-класс (BUG-100): порог 0.5% может не пройти по метрикам Inter↔Edge — при KNOWN_DEBTOR оформить как в TEST-58.

## Tests

- Unit `box_tree.rs` (модуль `initial_letter`): baseline-геометрия, raised-cap, RTL размещение.
- Unit `lumen-font`: cap-height метрика.
- Graphic: демо в `1000000-final.html`.

## Definition of done

- [ ] Буквица выровнена по cap-height/baseline, без `size × line-height`-приближения (срез 2)
- [ ] Raised-cap (`sink < size`) работает (срез 3)
- [ ] RTL/`inline-start` размещает буквицу справа (срез 4)
- [ ] Шапка `extract_initial_letter` (`box_tree.rs:2094`) переписана без «Phase 0 approximations»
- [ ] `cargo clippy -p lumen-layout --all-targets -- -D warnings` чистый
- [ ] `CSS-SPECS.md` / `CAPABILITIES.md` (`initial-letter` → ✅) обновлены; демо-тесты обновлены
