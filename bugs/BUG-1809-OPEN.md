# BUG-1809 — lumen-driver: computed `display` boxless-элементов = "" — два красных теста на чистом main

**Статус:** OPEN
**Открыт:** 2026-11-14 (P3, гейт BUG-1802)
**Компонент:** driver/js (`crates/driver/tests/cases/bug1191_boxless_computed_style.rs:32`, `crates/driver/tests/cases/bug489_display_contents_computed_style.rs:32` — computed-style пути boxless-элементов)

## Симптом

- `getComputedStyle(document.getElementById('empty')).display` → "" вместо "inline"
  (`bug1191…boxless_elements_report_full_computed_style`, `#empty` — пустой `<span>` в плоском дереве);
- `getComputedStyle(<display:contents>).display` → "" вместо "contents"
  (`bug489…display_contents_element_reports_its_own_computed_style`).

Оба соло-воспроизводимы: `cargo test -p lumen-driver --test all bug1191_boxless / bug489_display_contents`.
Соседние тесты тех же файлов (`boxless_elements_are_still_not_rendered`,
`display_contents_is_blockified_for_the_root_element`) зелёные.

## Что известно

- Детерминированные на чистом main `023241ed5` (2026-11-14, сессия P3): stash-прогон без
  правок BUG-1802 — те же FAILED с теми же значениями; правки BUG-1802 (layout kind/shell
  overlay) ни при чём. Семейство BUG-1191/BUG-489 — оба теста добавлены зелёными при
  закрытии своих карточек — бисектить окно мержей между их закрытием и 2026-11-14.

Найдено P3 при scoped-гейте BUG-1802; туда же всплыло в полном прогоне.
