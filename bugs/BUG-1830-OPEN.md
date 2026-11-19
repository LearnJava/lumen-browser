# BUG-1830-OPEN: скриншот-пайплайн не пиннит `position: fixed` — эталоны рефтестов уезжают со страницей

- **Статус:** OPEN
- **Область:** paint/shell (`crates/engine/paint/src/display_list/box_layer.rs` — порядковый сборщик; `crates/shell/src/lumen/automation.rs:render_current_page_to_png`, `crates/engine/paint/src/cpu_raster.rs` — скриншот-растеризация)
- **Приоритет:** P3 (блокирует критерий готовности ANCHOR-SCROLL-3-S3 и любые reftest-пары с `fixed`-контентом, где обе стороны не прокручиваются одинаково)

## Симптом

`css/css-anchor-position/anchor-scroll-fixedpos.html` (и `-nested-001/-002`) детерминированно
FAIL в `run_report.py --all --root css/css-anchor-position`: тест ожидает, что `position: fixed`-цель
останется на месте вьюпорта при прокрутке документа (`scrollTop = 200`), эталон — fixed на
`top: 100px`. В скриншоте оба уезжают на величину прокрутки: хэш теста стабилен от прогона к прогону
(`3db60d9f…`), изменения движка на якорях (ANCHOR-SCROLL-3-S3, ветка `p1-anchor-scroll-3-s3`) не
меняют ни одного пикселя этого теста.

`-002/-003/-004` при этом PASS — но случайно: у пар «тест/эталон» все боксы прокручиваются
одинаково, расхождение не проявляется.

## Причина

Скобки `BeginFixedLayer`/`EndFixedLayer` (композитор по ним отменяет запечённый `-scroll`,
`cpu_raster.rs:1026`) эмитит только walk-путь (`display_list/walk.rs:1192`,
`display_list/svg_text_decoration.rs:1188`). Порядковый сборщик (`box_layer.rs`, `fill_buckets`)
скобок не эмитит вовсе, а BUG-159 (фильтр `kept_scroll`, `box_layer.rs:762`) снимает с fixed только
наследование `PushScrollLayer` контейнеров-предков — скролл страницы в порядковом пути не слой
контейнера, а аргумент растеризатора, который пекётся в ВСЕ команды (`cpu_raster.rs:305-312`).

Скриншот живого окна (`AutomationCommand::Screenshot` → `render_current_page_to_png`) растеризует
именно порядковый список (`self.display_list = paint_ordered(lb)`, `about_to_wait.rs:1699`) со
скроллом вкладки. Итог: в скриншоте любой `position: fixed` прокручивается как содержимое страницы.

## Измерено (проба 2026-11-19, живое окно `--mcp-live-port`, `.tmp/probe_fixedpos.py` срез S3)

Разметка `anchor-scroll-fixedpos.html` + несanchor-ный `#plain{position:fixed;left:600px;top:100px}`,
скрипт `documentElement.scrollTop = 200`, viewport 1024×720:

- `window.scrollY` → `200` — состояние скролла на месте;
- якорь (doc y 300..400) → нарисован на y=100..200 — прокрутка дошла до растера;
- `#anchored` (doc y 300..400) → y=100..200 — прокручивается (после S3 — корректно: якорь в потоке);
- `#plain` (вьюпорт y 100..200) → **не нарисован вовсе** (уехал на -100..0) — пиннинга нет.

До S3 обе fixed-цели вели себя так же (хэши рефтестов до/после совпадают байт-в-байт): смена не в
логике якорей, а в отсутствии fixed-раздела в порядковом пути.

## Что чинить

1. Порядковый сборщик эмитит `BeginFixedLayer`/`EndFixedLayer` для бокса
   `Position::Fixed` при `!in_fixed_cb && !anchor_scroll.page` (то же условие, что `walk.rs:1186`
   после ANCHOR-SCROLL-3-S3; флаг `in_fixed_cb` в `fill_buckets` уже носится — `fixed_cb_clips`).
   CPU-растеризатор скобки уже понимает; GPU-бэкенды (`femtovg_backend.rs:5130`, `renderer.rs:3051`)
   — нет, им потребуется отдельный шаг (раздел-диапазоны уже умеет `overlay_partition::overlay_ranges`).
2. Альтернатива — пиннинг на уровне композитора (ADR-016 scroll-blit) без команд в списке.

Важно: любая из дорог меняет рендеринг КАЖДОГО reftest/голдена с `fixed`-контентом — нужна полная
верификация корпуса (`run.py --continue-on-fail`, полный прогон `css/` в WPT) и перегенерация
эталонов одним коммитом. Критерии: `anchor-scroll-fixedpos.html`, `-nested-001` (и по возможности
`-nested-002`) PASS; `-002/-003/-004` остаются PASS уже по настоящей семантике.

Найден P1 при ANCHOR-SCROLL-3-S3, 2026-11-19.
