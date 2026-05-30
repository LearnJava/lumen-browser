# STATUS-P3 — Bug Fixes + Driver Infrastructure

**Developer:** Программист 3 (Bug fixes + lumen-driver infrastructure)

---

## In progress
_(none)_

## Next

Приоритет сверху вниз. Каждая — отдельная ветка `p3-bug-<id>` / `p3-8a6-...`, отдельный worktree.

### 1. 8A.6 — миграция graphic_tests (приоритет: средний, большая задача)

**Текущее состояние (2026-05-30):** каркас есть (50 HTML, 50 PNG в `graphic_tests/snapshots/`, 50 Rust-тестов `crates/driver/tests/test_00..49.rs`), все зелёные. **Подзадачи (а), (б)-каркас, (б-2), (б-3 градиенты), (б-4 SVG-пути), (б-5 image-placeholder), (б-6 clip/overflow), (б-7 conic-градиент) ЗАВЕРШЕНЫ.** Дальнейшее расширение пиксельных эталонов — только по мере роста примитивов в `cpu_raster` (текст).

Что сделать:
- **(а) Усилить заглушки до структурных ассертов — DONE.** Все `test_NN.rs` проверяют реальную геометрию/стиль по ground-truth из HTML. Образец — `crates/driver/tests/test_01_sanity.rs`.
- **(б) Pixel-сравнение с эталонами PNG** (план §15 «уровень 3») — **каркас DONE (2026-05-30)**: добавлены детерминированный CPU-путь `InProcessSession::screenshot_cpu_rgba/png` (за feature `cpu-render` driver → `lumen-paint/cpu-render`, tiny-skia) и тест `crates/driver/tests/snapshot_cpu.rs`, который пиксельно сравнивает 7 geometry-страниц (00/01/04/05/06/16/36 — все 4 примитива cpu_raster) с эталонами в `graphic_tests/snapshots/cpu/`. Эталоны сгенерированы CPU-путём → кросс-OS детерминированы. Запуск: `cargo test -p lumen-driver --features cpu-render`; регенерация: `SAVE_CPU_SNAPSHOTS=1 …`.
- **(б-2) Расширить покрытие geometry-страниц — DONE (2026-05-30):** `PAGES` (`snapshot_cpu.rs`) расширен 7 → 20. Добавлены все чисто-геометрические тесты, у которых ≥2% не-фоновых пикселей при рендере через CPU-путь (измерено): 02,03 colors; 07 box-sizing; 08 padding; 09 margin; 10,11 min-max; 12 display; 17 calc; 38 z-index; 41 table; 42 sticky; 43 intrinsic-sizing. 13 новых эталонов в `graphic_tests/snapshots/cpu/`; 7 прежних перегенерировались байт-в-байт идентично (подтверждает детерминизм).
- **(б-3) Градиенты в `cpu_raster` + страница 39 — DONE (2026-05-30):** реализованы `DrawLinearGradient`/`DrawRadialGradient` в `crates/engine/paint/src/cpu_raster.rs` через нативные tiny-skia `LinearGradient`/`RadialGradient` (включая repeating через `SpreadMode::Repeat`, анизотропный ellipse через post-scale `Transform`, разрешение стопов зеркалит GPU `resolve_gradient_stops`). `PAGES` 20 → 21 (добавлен `39-gradients`: 5 linear + 4 radial + repeating-linear/radial). 20 прежних эталонов перегенерировались байт-в-байт идентично. Дальнейшее расширение на текст/картинки — только после реализации этих примитивов в `cpu_raster` (conic-градиент тоже отложен: у tiny-skia нет нативного conic-шейдера, в тесте 39 не используется).
- **(б-4) SVG-пути в `cpu_raster` + страница 47 — DONE (2026-05-30):** реализован `DrawSvgPath` в `crates/engine/paint/src/cpu_raster.rs` — плоский список треугольников (вершины уже в page-координатах, fill/stroke-opacity вшит в цвет) собирается в один `PathBuilder` и заливается одним `SourceOver`-проходом (Winding) → объединение тесселяции композитится ровно один раз (без AA-швов по внутренним рёбрам, как и GPU одним `Fill`-op). `PAGES` 21 → 22 (добавлен `47-svg-basic`). SVG basic shapes (rect/circle/ellipse/line) эмитятся как FillRect/FillRoundedRect/DrawBorder — их `cpu_raster` уже умел, так что 47 покрывает их; `<path>` ни в одном graphic-тесте нет, поэтому новый примитив покрыт прямыми unit-тестами `cpu_raster::tests::svg_path_*`. 21 прежний эталон перегенерировался байт-в-байт идентично.

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

- **8A.6(б-7) conic-градиент в cpu_raster + страница 40** (2026-05-30) — `crates/engine/paint/src/cpu_raster.rs` теперь рендерит `DrawConicGradient`. У tiny-skia нет нативного conic-шейдера, поэтому угловой свип считается попиксельно: для центра каждого пикселя внутри `rect` берётся полярный угол вокруг `(center_x_pct,center_y_pct)` через детерминированную libm-free `atan2`-аппроксимацию (Rajan, только IEEE-точные `+`/`-`/`*`/`/`/`min`/`max`/`abs`) → bit-identical между Windows/macOS/Linux, как требует exact-match снапшот-гейт; конвенция CSS (0° = верх `-y`, по часовой, `from_angle_deg` — старт), repeating тайлит resolved-span внутри оборота. Прокинут активный rect-clip (coverage-маска умножает альфу источника). `PAGES` (`snapshot_cpu.rs`) 24 → 25 (добавлен `40-conic-gradients`, эталон 269 КБ — 9 conic-градиентов: default / from-angle / at-position / angle-stops / repeating). 24 прежних эталона перегенерировались байт-в-байт идентично → детерминизм. Новый примитив дополнительно покрыт unit-тестами `cpu_raster::tests::conic_sweeps_first_to_last_stop` / `conic_empty_stops_noop`. clippy чист (paint+driver, feature), `cargo test -p lumen-paint --features cpu-render --lib` 442/442, `cargo test -p lumen-driver --features cpu-render --test snapshot_cpu` 1/1. Влито `p3-8a6-cpu-conic`.
- **8A.6(б-6) clip-примитивы в cpu_raster + страница 14-overflow** (2026-05-30) — `crates/engine/paint/src/cpu_raster.rs` теперь рендерит `PushClipRect`/`PopClip` и `PushScrollLayer`/`PopScrollLayer` (`overflow: hidden/scroll/auto`). Активные clip-прямоугольники держатся стеком; эффективный clip — их пересечение (`clip_intersection`), реализуется как tiny-skia `Mask` (`build_clip_mask`). Маска применяется к draw только когда его bounds пересекает край clip (`effective_clip` + `rect_contains` с eps): полностью попадающий в clip контент рисуется без маски и остаётся байт-идентичным неклипнутому пути. Scroll-трансляция не моделируется (offscreen-снапшоты scrollTop=0). `PAGES` (`snapshot_cpu.rs`) 23 → 24 (добавлен `14-overflow`); покрытие clip дополнено unit-тестами `cpu_raster` (`push_clip_rect_clips_fill`/`pop_clip_restores_full_drawing`/`nested_clip_intersects`). Ветка отделилась до мержа (б-5 DrawImage) — при влитии main разрешён конфликт в `cpu_raster`: DrawImage-ветка теперь прокидывает `effective_clip` в `rasterize_image_placeholder` (4-й параметр), эталон `18-images.png` регенерирован (изображение `width:300px` корректно усекается `.__f { overflow:hidden }`, разошлись 588 краевых пикселей; 23 прочих эталона байт-идентичны). clippy чист (paint+driver, feature+no-feature), `cargo test -p lumen-paint --features cpu-render --lib` 440/440, `snapshot_cpu` 24-страничный 1/1, default-suite paint+driver зелёные. Влито `p3-8a6-cpu-clip`.
- **8A.6(б-5) `<img>` placeholder в cpu_raster + страница 18** (2026-05-30) — `crates/engine/paint/src/cpu_raster.rs` теперь рендерит `DrawImage`: детерминированный CPU-путь (`render_to_image_cpu`) не регистрирует декодированные пиксели, поэтому каждая `<img>`-коробка заливается серым placeholder-квадратом `rgba8(217,217,217,255)` — точное зеркало GPU-fallback'а (`renderer.rs`, ветка `DrawImage`, linear `[0.85,0.85,0.85,1.0]` → 0.85×255≈217). Alt-текст не рисуется (в CPU-растеризаторе ещё нет текстового примитива), поэтому страница покрывается точно только при пустом `alt`. `PAGES` (`snapshot_cpu.rs`) 22 → 23 (добавлен `18-images`, эталон 73 КБ — 16 фрейм-ячеек с серыми placeholder'ами на тёмном `#0d1520`; все `<img>` имеют пустой `alt` + явные `width`/`height`, поэтому layout детерминирован без декодирования). 22 прежних эталона перегенерировались байт-в-байт идентично → детерминизм. Новый примитив дополнительно покрыт unit-тестом `cpu_raster::tests::draw_image_fills_grey_placeholder`. clippy чист (paint+driver, feature+no-feature), `cargo test -p lumen-paint --features cpu-render --lib` 433/433 (компоситорный timing-тест `compositor_thread_wakes_*` — известный flaky, проходит на ретрае), `cargo test -p lumen-driver --features cpu-render --test snapshot_cpu` 1/1, default driver-suite зелёный. Влито `p3-8a6-cpu-image`.
- **8A.6(б-4) SVG-пути в cpu_raster + страница 47** (2026-05-30) — `crates/engine/paint/src/cpu_raster.rs` теперь рендерит `DrawSvgPath`: плоский список треугольников (`vertices`, кратно 3, уже в page-координатах; `fill-opacity`/`stroke-opacity` вшиты в `color`) собирается в один `PathBuilder` (по 3 вершины → `move_to`/`line_to`×2/`close`) и заливается одним `SourceOver`-проходом с `FillRule::Winding`. Один общий fill (а не по треугольнику) даёт объединение тесселяции без AA-швов по внутренним рёбрам — зеркалит GPU, рисующий всю фигуру одним `Fill`-op. `PAGES` (`snapshot_cpu.rs`) 21 → 22 (добавлен `47-svg-basic`, эталон 64 КБ — визуально подтверждён: rect/circle/ellipse/rounded-rect/обводки во всех 4 рядах). SVG basic shapes (rect/circle/ellipse/line) эмитятся существующими FillRect/FillRoundedRect/DrawBorder — их CPU-путь уже умел; `<path>` ни в одном graphic-тесте нет, поэтому новый примитив дополнительно покрыт прямыми unit-тестами `cpu_raster::tests::svg_path_fills_triangle_interior` / `svg_path_empty_is_noop`. 21 прежний эталон перегенерировался байт-в-байт идентично → детерминизм. clippy чист (paint+driver, feature+no-feature), `cargo test -p lumen-paint --features cpu-render --lib` 425/425, `cargo test -p lumen-driver --features cpu-render --test snapshot_cpu` 1/1, default driver-suite зелёный. Влито `p3-8a6-cpu-svg`.
- **8A.6(б-3) градиенты в cpu_raster + страница 39** (2026-05-30) — `crates/engine/paint/src/cpu_raster.rs` теперь рендерит `DrawLinearGradient`/`DrawRadialGradient` через нативные tiny-skia `LinearGradient`/`RadialGradient`. Разрешение стопов (`resolve_stop_positions`) зеркалит GPU `resolve_gradient_stops` (unspecified first/last → 0/100%, равномерное распределение пробелов, `Length::Px` ÷ `line_len`); углы линии — порт `linear_gradient_uv_endpoints`; радиальный farthest-corner-ellipse строится как unit-круг + post-scale `Transform::from_row(rx,0,0,ry,cx,cy)`; repeating — `SpreadMode::Repeat` с перешкалированием стопов в один тайл `[0,1]`. `PAGES` (`snapshot_cpu.rs`) 20 → 21 (`39-gradients`: 5 linear + 4 radial + repeating-linear/radial), эталон 223 КБ — содержательный (визуально подтверждён: полосы 45°, RGB-bullseye, концентрические кольца). 20 прежних эталонов перегенерировались байт-в-байт идентично → детерминизм. conic отложен (нет нативного tiny-skia conic, в тесте не нужен). clippy чист (paint+driver, feature+no-feature), `cargo test -p lumen-driver --features cpu-render` 21/21, default-suite paint+driver зелёные. Влито `p3-8a6-cpu-gradients`.
- **BUGS.md doc-sync: устаревшие OPEN-статусы → FIXED** (2026-05-30) — раздел «Детали багов» в `BUGS.md` сохранял `**Статус:** OPEN` в 11 секциях (BUG-020/021/022/023/024/025/026/029/032/036/037), хотя сводная таблица давно помечает их FIXED. `grep "OPEN" BUGS.md` (шаг P3-воркфлоу поиска багов) давал 11 ложных срабатываний и мог увести сессию в правку закрытых багов. Статусы приведены в соответствие со сводной таблицей (дата + ссылка на неё). Теперь `grep "OPEN" BUGS.md` возвращает только легенду статусов и одну датированную историческую заметку — реальных OPEN-багов в трекере нет. Только документ, движок не тронут; все suite зелёные (paint 423+21+5, layout --lib 2063, driver). Влито `p3-bugs-doc-sync`.
- **8A.6(б-2) расширение CPU-снапшотов 7 → 20** (2026-05-30) — `PAGES` в `crates/driver/tests/snapshot_cpu.rs` расширен с 7 до 20 geometry-страниц. Подход: написал временный диагностический рендер каждой `graphic_tests`-страницы через CPU-путь (tiny-skia) и измерил долю не-фоновых пикселей; добавил все чисто-геометрические тесты с содержательностью ≥2% (02,03 colors; 07 box-sizing; 08 padding; 09 margin; 10,11 min-max; 12 display; 17 calc; 38 z-index; 41 table; 42 sticky; 43 intrinsic) — иначе эталон был бы почти-пустой рамкой. 13 новых эталонов в `graphic_tests/snapshots/cpu/`; 7 прежних перегенерировались байт-в-байт идентично (подтверждает детерминизм tiny-skia-пути). Дальнейшее расширение на текст/картинки/градиенты — только после реализации этих примитивов в `cpu_raster`. clippy чист (feature + no-feature), `cargo test -p lumen-driver --features cpu-render` 20/20, default-suite зелёный. Влито `p3-8a6-cpu-coverage`.
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
