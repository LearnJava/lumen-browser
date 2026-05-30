# STATUS-P3 — Bug Fixes + Driver Infrastructure

**Developer:** Программист 3 (Bug fixes + lumen-driver infrastructure)

---

## In progress
_(none)_

## Next

Приоритет сверху вниз. Каждая — отдельная ветка `p3-bug-<id>` / `p3-8a6-...`, отдельный worktree.

### 1. 8A.6 — миграция graphic_tests (приоритет: средний, большая задача)

**Текущее состояние (2026-05-30):** каркас есть (50 HTML, 50 PNG в `graphic_tests/snapshots/`, 50 Rust-тестов `crates/driver/tests/test_00..49.rs`), все зелёные. **Подзадачи (а) и (б)-каркас ЗАВЕРШЕНЫ.** Остаётся только (б-2) — расширение покрытия пиксельных эталонов.

Что сделать:
- **(а) Усилить заглушки до структурных ассертов — DONE.** Все `test_NN.rs` проверяют реальную геометрию/стиль по ground-truth из HTML. Образец — `crates/driver/tests/test_01_sanity.rs`.
- **(б) Pixel-сравнение с эталонами PNG** (план §15 «уровень 3») — **каркас DONE (2026-05-30)**: добавлены детерминированный CPU-путь `InProcessSession::screenshot_cpu_rgba/png` (за feature `cpu-render` driver → `lumen-paint/cpu-render`, tiny-skia) и тест `crates/driver/tests/snapshot_cpu.rs`, который пиксельно сравнивает 7 geometry-страниц (00/01/04/05/06/16/36 — все 4 примитива cpu_raster) с эталонами в `graphic_tests/snapshots/cpu/`. Эталоны сгенерированы CPU-путём → кросс-OS детерминированы. Запуск: `cargo test -p lumen-driver --features cpu-render`; регенерация: `SAVE_CPU_SNAPSHOTS=1 …`.
- **(б-2) Расширить покрытие geometry-страниц — DONE (2026-05-30):** `PAGES` (`snapshot_cpu.rs`) расширен 7 → 20. Добавлены все чисто-геометрические тесты, у которых ≥2% не-фоновых пикселей при рендере через CPU-путь (измерено): 02,03 colors; 07 box-sizing; 08 padding; 09 margin; 10,11 min-max; 12 display; 17 calc; 38 z-index; 41 table; 42 sticky; 43 intrinsic-sizing. 13 новых эталонов в `graphic_tests/snapshots/cpu/`; 7 прежних перегенерировались байт-в-байт идентично (подтверждает детерминизм). Дальнейшее расширение на текст/картинки/градиенты — только после реализации соответствующих примитивов в `crates/engine/paint/src/cpu_raster.rs` (сейчас пропускаются → почти-пустые эталоны).

**Не «понижать планку»:** тестовые HTML — ground truth, при расхождении чинить движок (или заводить BUG), а не упрощать тест.

### Постоянно

- `cargo test -p lumen-paint` и `cargo test -p lumen-layout` держать зелёными. Если parallel-сессии (P1/P2/P4) мерджат и ломают тесты — это твой приоритет №0 (как было с BUG-043/044/045 29.05).
- Проверять `grep "OPEN" BUGS.md` на новые баги.

---

## Workflow

1. **Run graphic tests** to identify visual regressions:
   ```bash
   python graphic_tests/run.py --continue-on-fail
   ```

2. **Check BUGS.md** for open issues:
   ```bash
   grep "OPEN" BUGS.md
   ```

3. **Pick highest-deviation bug** from the list and locate via SYMBOLS.md + grep

4. **Fix + test + mark as FIXED:**
   - Add regression test to existing test file
   - `cargo clippy -p <crate> -- -D warnings` → pass
   - `cargo test -p <crate>` → pass
   - Update BUGS.md: `OPEN → FIXED 2026-05-28`
   - Commit with message: `P3: fix BUG-NNN — <description>`

5. **Branch naming:** `p3-bug-<id>`, e.g. `p3-bug-042-transition-fill`

---

## Recent fixes

- **BUG-047 line-clamp — мисдиагноз, движок корректен** (2026-05-30) — расследование показало, что `-webkit-line-clamp` **реально усекает контент**: `--dump-layout` на `48-line-clamp.html` даёт InlineRun внутри `.box` высотой 40/80/120/160 (ровно 1-4 строки по 40px), а без clamp (`.ref`) — 560 (14 строк). Высота самого `.box` = 160 у всех четырёх — это **корректный** `align-items: stretch`: flex-элементы тянутся до cross-size самой высокой колонки (`.b4` = 4×40 = 160). Edge рендерит идентично — `graphic_tests/screenshots/48-edge.png` показывает 4 равных бокса в ряду 2, лесенка только в ряду 3 (явные высоты). Усечение строк уже покрыто lib-тестами `line_clamp_*` в `lumen-layout`. Изменений в движке не требуется. `#[ignore]`-тест `test_48_line_clamp_height_truncation` (ассертил ошибочную лесенку 40/80/120/160 на `.box`) переписан в `test_48_line_clamp_flex_items_stretch_equal` — фиксирует verified-by-Edge факт «все `.box` = 160px», `#[ignore]` снят. `cargo test -p lumen-driver --test test_48` 2/2 зелёных. Влито `p3-bug-047-line-clamp`. (Замечание: graphic-пайплайн `run.py --only 48` дал ложный 100% FAIL — gdigrab захватил пустое окно; инфраструктура захвата, не движок.)
- **BUG-049 shell build** (2026-05-30) — `lumen-shell` снова не компилировался: мерж p2 print-pages добавил `DisplayCommand::PageBreak`, а `content_height_of`/`content_width_of` (`shell/src/main.rs:4219,4272`) не покрывали его (E0004) → падали и сборка, и dump-режимы. PageBreak — маркер пагинации печати, не несёт rect и не контент viewport-а → добавлен в ветку `continue` (как BUG-048 DrawScrollbar). Проверено: build default + `--features quickjs`, clippy `--all-targets -D warnings` чисто, `cargo test -p lumen-shell` 278/278. Влито `p3-bug-049-shell-pagebreak`.
- **8A.6(б) каркас pixel-снапшотов** (2026-05-30) — детерминированный CPU-путь рендера в driver: feature `cpu-render` (→ `lumen-paint/cpu-render`, tiny-skia), методы `InProcessSession::screenshot_cpu_rgba/png` (`crates/driver/src/session.rs`), и тест `crates/driver/tests/snapshot_cpu.rs` — пиксельное сравнение 7 geometry-страниц (00/01/04/05/06/16/36, покрывают FillRect/FillRoundedRect/DrawBorder/DrawOutline) с эталонами `graphic_tests/snapshots/cpu/*.png`. Эталоны сгенерированы CPU-путём → кросс-OS детерминированы (в отличие от прежних GPU-PNG). Тест gated на feature: обычный `cargo test -p lumen-driver` компилирует его в ничто. Запуск: `cargo test -p lumen-driver --features cpu-render`. clippy чист (с feature и без), driver-suite зелёный. Влито `p3-8a6-cpu-snapshot`. Остаётся (б-2) — расширять `PAGES` по мере роста `cpu_raster`.
- **8A.6(a) усиление driver-тестов** (2026-05-30) — все 50 `crates/driver/tests/test_NN.rs` переведены с vacuous-заглушек (`assert!(!boxes.is_empty())`) на структурные ассерты box-model/computed-style по ground-truth из HTML. Последний батч — 10 тестов (02,03 colors; 05 border-width; 08 padding; 13 visibility/opacity; 18 images; 22,46 transforms; 36 border-radius; 39 gradients). Заведён **BUG-047** (`-webkit-line-clamp` парсится, но не усекает высоту — регрессионный тест в `test_48.rs` за `#[ignore]`). Удалён дубль `test_26.rs`. `cargo test -p lumen-driver` зелёный, clippy чист. Влито `p3-8a6-migrate-graphic-tests`. Остаётся подзадача (б) — pixel-сравнение с PNG.
- **BUG-048 shell build** (2026-05-30) — `lumen-shell` снова не компилировался: `p2-scrollbar-rendering` добавил `DisplayCommand::DrawScrollbar`, а `content_height_of`/`content_width_of` (`shell/src/main.rs:4219,4271`) не покрывали его (E0004) → падали и сборка, и dump-режимы. Скроллбар — UI viewport-а, не контент → добавлен в ветку `continue` (как BUG-044). Проверено: build default + `--features quickjs`, clippy `--all-targets -D warnings` чисто, `cargo test -p lumen-shell` 278/278. Влито `p3-bug-048-shell-scrollbar`.
- **BUG-046 lumen-layout --lib green** (2026-05-30) — 3 устаревших теста, не регрессии: (1) webp теперь реально декодируется (`image/webp` в `supported_mime_types()`, `decode_webp`), поэтому `collect_picture_*` обновлены — `unsupported_type_falls_back` переведён на `image/avif` (реально неподдерживаемый), `supported_type_picked` ожидает `hero.webp`; (2) `non_cell_col_row_span_defaults_to_one` — `lay()` возвращает body-box напрямую, убран лишний уровень `first_element_child`. `cargo test -p lumen-layout --lib` 2063/2063 зелёных, clippy чист. Влито `p3-bug-046-layout-tests`.
- **BUG-043 + BUG-045 paint suite green** (2026-05-29) — `cargo test -p lumen-paint` был красным (19 падений, BUG-043 описывал лишь 7). Причины: (1) 5 golden устарели после `font-optical-sizing` (`var=["opsz"=16]`) → регенерированы; (2) overflow visible+hidden coercion (BUG-020) → `auto` = scroll-container → клип через `PushScrollLayer` (p2-scroll-layer), 5 тестов переписаны (вкл. чужой `ordered_overflow_x_alone_triggers_clip`); (3) half-leading 1.6px первой строки (CSS 2.1 §10.8.1) — 5 baseline/wrap-тестов обновлены; (4) **BUG-045**: `backdrop-filter` не создавал stacking context (`creates_stacking_context` не проверял `backdrop_filter`) → пустой DL, добавлена проверка + regression-тест. lumen-paint 391+21+5 зелёных. Влито `p3-bug-043-snapshot-golden`. **BUG-046 (OPEN)** — 3 пред-существующих падения lumen-layout (picture webp, table colspan), не связаны.
- **BUG-044 shell build** (2026-05-29) — `lumen-shell` не компилировался (default + `--features quickjs`): non-exhaustive match по `DisplayCommand` в `content_height_of`/`content_width_of` (`shell/src/main.rs:4219,4265`) после P2-мерджей, добавивших `PushMaskLayer`/`PopMaskLayer`/`DrawSvgPath`/`BoxModelOverlay`. `PushMaskLayer` → rect-ветка, остальные → continue. Браузер и dump-режимы снова рабочие. Влито `p3-bug-044-shell-match`.
- **8A.2 InProcessSession** (2026-05-29) — headless in-process сессия `BrowserSession` в `crates/driver/src/session.rs:53` (полный pipeline encode→parse→CSS→layout без GPU + adapter для `lumen-core::ext::BrowserSession`). Проверено: `cargo test -p lumen-driver` (все зелёные), `cargo clippy --all-targets -- -D warnings` чисто, `todo!()` нет. `lumen-plan.md` уже ✅. Влито `p3-8a2-in-process-session`.
- **8A.1 BrowserSession trait** (2026-05-29) — `BrowserSession` trait + `NullBrowserSession` заглушка в `crates/core/src/ext.rs:1514` (object-safe, `Send`). Тесты: null-impl, object-safety, Send. `lumen-plan.md` ⬜→✅. Влито `p3-8a1-browser-session`.

---

## BUGS.md reference

**Current open bugs:** See [BUGS.md](BUGS.md) for full list of OPEN items.

**Format in BUGS.md:**
```
BUG-042 | OPEN  | transition fill-modes wrong on nested divs | layout/src/flow.rs:312
BUG-043 | FIXED 2026-05-28 | composite glyphs missing | font/src/parser.rs:201
```

---

## Notes

- **Don't context-switch:** Bug fixes are your only focus, finish one before starting another
- **Regression tests:** Every fix gets a test in the same commit — prevents future regressions
- **Coordinate with P1/P2:** Your fixes might unblock their feature work
- **CSS bugs:** If bug is in CSS, note in STATUS-P4.md and continue with implementation bug

See CLAUDE.md §"Bug ownership: P3 only" for full workflow details.
