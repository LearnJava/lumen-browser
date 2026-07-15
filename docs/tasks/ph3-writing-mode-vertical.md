# Задача: CSS `writing-mode` — вертикальный текст (полный layout + paint)

**Developer:** P1
**Ветка:** `p1-writing-mode-vertical`
**Размер:** M
**Крейты:** `lumen-layout`, `lumen-paint`, `lumen-font`

## Goal

Довести вертикальные режимы письма (CSS Writing Modes L4 §3–§4: `vertical-rl`/`vertical-lr`/`sideways-*`) до визуальной корректности: блок-axis-swap уже есть, вертикальный inline-поток уже есть — **остался paint**: реальный поворот/ориентация глифов (upright vs sideways) в оконном (femtovg) и CPU-рендерерах.

## Current state (сверено с кодом 2026-07-05)

ROADMAP помечает PARTIAL «дошить Phase 2 vertical inline flow». **Это устарело**: Phase 2 (вертикальный inline-поток) в layout уже реализован. Реальный незакрытый пробел — **paint**: оба рендерера игнорируют `text_orientation` и рисуют глифы горизонтально.

- **Парсинг**: `writing-mode` (`vertical-rl`/`vertical-lr`/`sideways-rl`/`sideways-lr`) + `text-orientation` (`mixed`/`upright`/`sideways`) — `crates/engine/layout/src/style.rs:13555` и рядом; enum `WritingMode`, `TextOrientation` (`style.rs:3906`); поля `writing_mode`, `text_orientation: TextOrientation` (`style.rs:3085`, initial `Mixed`).
- **Layout — блок-axis-swap**: модуль `crates/engine/layout/src/vertical.rs`, `lay_out_vertical_block` (`vertical.rs:75`); диспатч из `box_tree.rs:5316` (Block/FlowRoot). CSS `height`→inline-size, `width`→block-size; RL/LR стек детей; shrink-to-fit ширины. 9 unit-тестов.
- **Layout — вертикальный inline-поток (Phase 2)**: `lay_out_vertical_inline_run` (`vertical.rs:498`) + `wrap_inline_run_vertical` (`vertical.rs:557`) — текст переносится top→bottom, wrap по inline-size; диспатч из `box_tree.rs:5590` перед горизонтальной веткой InlineRun. **Уже подключено и работает** (позиции корректны).
- **Paint — DisplayCommand**: `DrawText` несёт `text_orientation: Option<TextOrientation>` (`display_list.rs:419`), выставляется во всех точках эмиссии текста при `writing_mode != HorizontalTb` (`display_list.rs:2457`, `2601`, `3169`, `4472`, `4538`, `4913`, `4947`, `5053`, `6703`).
- **Тесты**: 9 unit `vertical.rs`; парсинг writing-mode/text-orientation в `style.rs:29111`+.

**Реальный остаток (≈45%):**
1. **Поворот глифов не выполняется**. femtovg-бэкенд игнорирует поле — `crates/engine/paint/src/backends/femtovg_backend.rs:2576` деструктурирует `text_orientation: _`; глиф рисуется как есть. То же в CPU — `crates/engine/paint/src/cpu_raster.rs:454` (`..` в паттерне DrawText). Итог: layout ставит буквы в вертикальную колонку, но каждый глиф остаётся горизонтальным (боком), а не повёрнут/upright.
2. **`text-orientation` не различается на paint**: `upright` (CJK-подобная постановка прямо), `mixed` (латиница повёрнута на 90° CW, CJK прямо), `sideways` (весь текст повёрнут 90°) — все три рисуются одинаково.
3. Выделенного graphic-теста на writing-mode нет (`graphic_tests/` — только `24-vertical-align.html`, это другое свойство).

Замечание по коду: `vertical.rs:281` — комментарий BUG-264 про `items_after_test_module` (функции inline-run объявлены после `#[cfg(test)]`-модуля; подавлено `#[allow]`). Реордер — опциональная чистка в рамках задачи.

## Entry points

- `crates/engine/paint/src/backends/femtovg_backend.rs:2576` — `DrawText` handler (добавить поворот по `text_orientation`).
- `crates/engine/paint/src/cpu_raster.rs:454` — `DrawText` handler CPU (аналогично).
- `crates/engine/paint/src/display_list.rs:419` — поле `text_orientation` в `DrawText` (уже есть).
- `crates/engine/layout/src/vertical.rs:557` — `wrap_inline_run_vertical` (пробрасывает `text_orientation`; проверить, что для `upright` advance = высота глифа, а для `sideways` — ширина).
- `crates/engine/font/` — глиф-метрики/пути для повёрнутого рендера.

## Срезы (декомпозиция)

### Срез 1 — S — поворот глифов в CPU-рендерере
В `cpu_raster.rs:454` при `text_orientation == Some(Sideways)` (и `Mixed` для латиницы) рисовать run повёрнутым на 90° CW: либо через существующий transform-слой (`PushTransform`, `cpu_raster.rs:486`), либо рендер run в offscreen + `draw_pixmap` с поворотом. `Upright` — глифы без поворота, но с вертикальным advance. CPU-путь детерминирован (кросс-OS bit-identity) — тесты по пикселям надёжны. Unit-тест: run с `Sideways` даёт инк, повёрнутый относительно `None`.

### Срез 2 — S — поворот глифов в femtovg-бэкенде
В `femtovg_backend.rs:2576` симметрично: применить `femtovg::Transform2D` поворот 90° вокруг позиции run для `Sideways`/`Mixed`-латиницы; `Upright` — вертикальная раскладка без поворота. Это оконный (боевой) путь — правки здесь видны в реальном окне (femtovg — дефолтный бэкенд).

### Срез 3 — S — различение `mixed` / `upright` / `sideways`
Для `Mixed` (initial): латинские глифы повёрнуты 90° CW, CJK — прямо (upright). Нужна пер-символьная классификация (есть `vertical::is_cjk`, `vertical.rs:33`). Проверить, что `wrap_inline_run_vertical` уже кладёт advance по правильной оси для каждого случая; при необходимости пробросить per-glyph ориентацию в `DrawText` (сейчас поле — на весь run). Unit-тесты на три значения.

### Срез 4 — XS — реордер BUG-264 (опц. чистка)
Перенести `lay_out_vertical_inline_run`/`wrap_inline_run_vertical` (`vertical.rs:498`+) ДО `#[cfg(test)]`-модуля, снять `#[allow(clippy::items_after_test_module)]` (`vertical.rs:288`) и комментарий BUG-264. Закрыть layout-часть BUG-264.

### Срез 5 — S — graphic-тест
`graphic_tests/NN-writing-mode.html` (магента-рамка): блоки `vertical-rl`/`vertical-lr` с латинским и CJK-текстом, `text-orientation: mixed/upright/sideways`. Демо в `1000000-final.html`, `COVERAGE.md`, `TESTS` в `run.py`. Ожидать возможный debtor-класс по метрикам Inter↔Edge (порог 0.5%) — при KNOWN_DEBTOR оформить как TEST-58/71.

## Tests

- Unit `cpu_raster.rs`: инк повёрнут для `Sideways` vs `None`.
- Unit `vertical.rs`: advance-ось по `text_orientation`.
- Graphic: `NN-writing-mode.html` + демо.

## Progress (2026-07-15) — Срез 1 (CPU-рендерер) DONE

`crates/engine/paint/src/cpu_raster.rs`:
- `DrawText`-обработчик прокидывает `text_orientation` в `rasterize_text`.
- `rasterize_text` при `Some(Sideways | Mixed)` делегирует в новую
  `rasterize_text_rotated`: рендерит run горизонтально в локальный
  full-canvas-sized буфер через тот же `rasterize_text` (origin `(0,0)`, без
  клипа — рекурсия с `text_orientation: None`, без дублирования
  shaping/blit-цикла), затем композитит его на `pixmap` с поворотом на 90° CW
  вокруг локального origin + translate к `rect` (`tiny_skia::Transform::
  from_row(0, 1, -1, 0, rect.x, rect.y)`), клип — через существующий
  `build_clip_mask`.
- `Upright` и `None` не тронуты (текущее горизонтальное поведение).
- Per-glyph различение `mixed` (латиница повёрнута / CJK прямо) — отдельно,
  Срез 3; в этом срезе `Mixed` трактуется как `Sideways` (весь run повёрнут).
- Тест `draw_text_sideways_rotates_ink_bbox` (`cpu_raster.rs`): ink bbox для
  `Sideways` выше, чем шире (обратный аспект горизонтального рендера).

**Остаток:** Срез 2 (femtovg — ныне fallback-бэкенд) + новый пункт, не
учтённый в исходном брифе (файл писался 2026-07-05, до ADR-017 2026-07-13,
когда wgpu стал дефолтным бэкендом): `crates/engine/paint/src/renderer.rs`
(wgpu, live default) **тоже** игнорирует `text_orientation`
(`text_orientation: _` в `DrawText`-обработчике) — приоритетнее femtovg для
следующего среза, так как это путь, который реально видит пользователь.
Срез 3 (mixed/upright различение), Срез 4 (BUG-264 реордер, опц.), Срез 5
(graphic-тест) — не начаты.

## Definition of done

- [x] Глифы реально повёрнуты/upright в CPU-рендерере (срез 1)
- [ ] Глифы реально повёрнуты/upright в femtovg-окне (срез 2)
- [ ] `mixed`/`upright`/`sideways` дают разный визуальный результат (срез 3)
- [ ] BUG-264 (layout-часть) закрыт, `#[allow(items_after_test_module)]` снят (срез 4, опц.)
- [ ] `cargo clippy -p lumen-paint --all-targets -- -D warnings` и `-p lumen-layout` чистые
- [ ] Graphic-тест зелёный/оформлен debtor; `COVERAGE.md` + `run.py` обновлены
- [ ] `CSS-SPECS.md:104/645` (`writing-mode` vertical → ✅ layout+paint) и `CAPABILITIES.md` обновлены; шапки `vertical.rs:10` («Phase 2 tasks») и `box_tree.rs:5315` переписаны
