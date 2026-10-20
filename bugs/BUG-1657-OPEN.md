# BUG-1657 — разрешённые размеры дорожек grid (`getComputedStyle().gridTemplateColumns`) расходятся с Grid L1 §11 для auto/content-sized дорожек

**Статус:** OPEN
**Заведён:** 2026-10-20 (P3, найден при закрытии BUG-1383)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs`, `grid_auto_cols.rs`, `grid_trampoline.rs` — определение размеров дорожек)

## Симптом

С BUG-1307 `getComputedStyle` отдаёт used-размеры дорожек, но сами размеры не совпадают с ожидаемыми:

- `auto`-дорожки при `justify-content: start` (`.contentStart`) растягиваются на всю ширину: `grid-template-columns: auto auto`, один элемент `17px` во второй колонке, контейнер `800px` → получено `391.5px 408.5px`, ожидается `0px 17px`. Auto-дорожки растягиваются только при `normal`/`stretch` (Grid L1 §11.8).
- `min-content` / `max-content` / `minmax(min-content|max-content, …)` дорожки без содержимого получают `0px` или полную ширину: `grid-content-sized-columns-resolution.html` — `auto auto` → `504px 504px` вместо `45px 45px`, `min-content minmax(min-content,30px)` → `0px 0px` вместо `20px 30px` (часть расхождений — Ahem в стенде `run_smoke.py` не применяется, см. `docs/probe-method.md`; их отделить пробой с цветными боксами против Edge).
- Процентные дорожки в контейнере без определённого размера: `gridWithPercentWithoutSize` получено `16px 0px`, ожидается `4px 12px`.
- `minmax(…)` с процентом/`calc()`: `gridWithMinMaxAndFixed` `45px 15px` вместо `240px 15px`; `gridWithFractionMinMax` `0px 800px` вместо `45px 755px`; `16em minmax(16px, 20px)` → `160px 16px` вместо `160px 20px`.

## Как проверить

`css/css-grid/parsing/grid-columns-rows-get-set-multiple.html` (14 подтестов из 40 падают, 26 проходят), `grid-content-sized-columns-resolution.html` (0/42). Их `.ini` в `tests/wpt/metadata` всё ещё `expected: TIMEOUT` (причина — BUG-467, давно закрыт): тесты завершаются `OK`, ожидание нужно обновить `run_report.py --update-expected` после починки.
