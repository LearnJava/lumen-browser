# BUG-114

**Статус:** FIXED 2026-06-24 (DEBTOR)
**Компонент:** css-parser → layout (style.rs)
**Файл:** `crates/engine/layout/src/style.rs`

## Описание

`font` shorthand drops font-size/line-height: `font: 700 13px/1.4 sans-serif` and `font: 11px/1.5 monospace` render at 16px (default), only font-weight applied — TEST-53 residual ~4px vertical + text width drift. font-size/line-height components of the shorthand not parsed into ComputedStyle.

## Причина

В `apply_declaration` (style.rs) не было арма `"font" =>` — shorthand молча падал в `_ => {}`. Свойство `font` присутствует в KNOWN_PROPERTIES и проходит каскад, но ни один компонент кроме случайно совпавших longhand-ов не применялся. font-size резолвится в pre-pass (`apply_font_size`), который игнорировал `font`.

## Фикс

1. `parse_font_shorthand(val) -> Option<FontShorthand>` (CSS Fonts L4 §6.10): нормализует `/`, токенизирует, потребляет leading-секцию (`style || variant || weight || width`) до первого валидного `<font-size>`, затем размер, опц. `/ line-height`, остаток — `font-family`. Возвращает `None` для system-font/CSS-wide keyword-ов и невалидных значений (нет размера/семейства).
2. `apply_font_size` (pre-pass) теперь обрабатывает `font` — резолвит только `<font-size>`-компонент через выделенный `resolve_font_size`.
3. Арм `"font" =>` в main-pass применяет остальные longhand-ы (style/variant/weight/stretch/line-height/family), предварительно сбросив их в initial (CSS Cascade L4 §3.1 — shorthand сбрасывает все управляемые longhand-ы).
4. Логика line-height вынесена в `apply_line_height_value` (общая для longhand-а и shorthand-а).

7 юнит-тестов в style.rs (`font_shorthand_*`). Проверено end-to-end: `--dump-display-list` на 53-background-origin.html показывает текст 13px w=700 / 11px monospace (было 16px default).

## Результат

TEST-53: 9.62% → **1.71%** (gdigrab gate). Остаток = font-parity (sans-serif/monospace рисуются Inter vs Edge, rule 3) + накопленный line-height-дрейф → класс BUG-128, KNOWN_DEBTORS '53'.
