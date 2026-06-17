# STATUS-P3 — Bug Fixes + Driver Infrastructure

**Developer:** Программист 3 (Bug fixes + lumen-driver infrastructure)

---

## In progress

_(пусто)_

## Next

Приоритет сверху вниз. Каждая — отдельная ветка `p3-bug-<id>`, отдельный worktree.

### 0. ПРИОРИТЕТ 0 — регрессии (исправить немедленно, блокируют Phase 2)

При падении `cargo test -p lumen-paint` или `cargo test -p lumen-layout` — исправить немедленно.

_(BUG-119 закрыт 2026-06-10 — rule index оказался невиновен, см. Recent. Кеш-ключ
`(sheet_ptr, sheet_rules_len)` из ревизии остаётся теоретическим риском, но инвалидация
на каждый layout-проход (`box_tree.rs:1756`, merge 26d4386e) его покрывает.)_

**Из ревизии 2026-06-10 ([docs/paint-pipeline-review-2026-06.md](docs/paint-pipeline-review-2026-06.md)) — задачи P3:**

_(BUG-121 закрыт 2026-06-10 — informational-режим по умолчанию, см. Recent. Корень был не в
порогах: гейт рендерит через wgpu fallback `Renderer`, а не femtovg. Follow-up-инфраструктура —
femtovg headless путь для snapshot_vs_edge, чтобы пороги run.py снова стали применимы — не
запланирована, брать после исчерпания OPEN-багов.)_

_(BUG-120 закрыт 2026-06-10 — невидимые Cc стрипаются на уровне inline-сегментов, см. Recent.)_

- BUG-085 (градиенты 12%): расследовать геометрию, НЕ цветовое пространство (TEST-39 опровергает
  sRGB-гипотезу — стопы непрозрачные hex + transparent с тем же RGB). Кандидаты: radial default
  sizing (farthest-corner), hard stops AA, femtovg `fill_gradient` kernel. После P2 PA-1 (gradient_math.rs).
_(BUG-093 закрыт 2026-06-10 — порог TEST-51 откалиброван 0.5→2.0%, см. Recent.)_

- BUG-082/094/098/076 — НЕ брать точечно: закрываются фичами femtovg у P2 (PA-2..PA-4 в STATUS-P2.md).

### 0.5. Interaction-слой graphic_tests (новое, 2026-06-11)

Серия 100–109 (`graphic_tests/1NN-*.html`) — взаимодействия свойств, юнит-тесты которых зелёные.
Все 10 FAIL на Edge-сравнении → **BUG-131…BUG-140** в BUGS.md. Диагностика:
`python graphic_tests/run.py --bisect <id>` (прогоняет юнит-зависимости, печатает вердикт),
при FAIL run.py печатает разошедшиеся ячейки сетки (REGIONS).

_(BUG-139 закрыт 2026-06-12, см. Recent. Серия перегнана полным прогоном 2026-06-12 08:58
(commit cbe87ae4): TEST-108 PASS 0.0016%, TEST-106 PASS 0.0152% → BUG-137 тоже закрыт (фиксом
BUG-139 + PA-3, помечен FIXED). BUG-133 (TEST-102 → 0.00%) и BUG-140 (TEST-109 13.69→4.80%,
юнит TEST-31 → PASS) закрыты, см. Recent. BUG-131 (TEST-100, transform×overflow 9.57%) закрыт
2026-06-13, см. Recent. BUG-138 (TEST-107, shadow×radius×overflow) закрыт 2026-06-13, см. Recent.
Остались FAIL: 104 (51.97%), 103 (3.15% после BUG-146), 105 (4.84%), 101 (4.04%),
109 (4.80% — остаток целиком BUG-151 margin-collapse, layout).)_

- BUG-135 (TEST-104, mask×gradient×radius, 51.97%) — НЕ брать пока P2 работает над BUG-085
  (градиенты, ветка `p2-bug085-gradient`): контрольная ячейка без маски — gradient+radius,
  вероятно зависит от фикса градиентов. Перепроверить после мержа P2.

### 1. Открытые баги (после BUG-119)

Много OPEN-багов из графических тестов TEST-58…70 (`grep "OPEN" BUGS.md`).
Бери по убыванию отклонения, исключая CSS-свойства (домен P4) и Phase 2 фичи.

_(BUG-110 закрыт 2026-06-14 — object-fit SVG viewBox FIXED. BUG-128 — text-underline geometry
расследован 2026-06-14: не paint-баг, вся дельта от font-parity (Inter vs Edge serif); кандидат
в KNOWN_DEBTORS, а не P3-задача. BUG-129 FIXED — border-collapse collapse, остаток paint-side
varied-width — отдельная эмиссия общих границ — следующая по таблицам.)_

**Рекомендуемый порядок (прогон 2026-06-15):**

_(BUG-156 и BUG-157 закрыты 2026-06-15 как ложные регрессии — прогон 06-15 гонял устаревший
бинарь от 12.06 без PH3-4/PH3-5; свежая сборка PASS 0.38%/0.48%. См. Recent. BUG-134 закрыт
2026-06-15 тем же образом — TEST-103 PASS 0.04% свежей сборкой, «29.11%» от устаревшего
бинаря cf54c92d. См. Recent.)_

Следить за новыми: `grep "OPEN" BUGS.md`.

### 3. Shell wiring

_(нет — handoff-задачи перераспределены на P1/P2)_

> Перенесено 2026-06-02: `Event::RequestFailed` → network-panel **→ P2** (задача #30, владеет `devtools/network_panel.rs`). P3 фокусируется только на баг-фиксах и регрессиях (см. CLAUDE.md «Bug ownership: P3 only»).

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

Полная история — `git log --oneline` (ветки фиксов P3 с префиксом `p3-bug-<id>`)
и файлы `bugs/BUG-NNN-FIXED.md`. Ниже — только последние, как быстрый контекст:

- **BUG-177** (2026-06-17) — table-cell `height` как минимум (TEST-115 13.45% → 0.00% PASS).
  `height` на `display:table-cell` трактовался как фиксированная высота border-box, а не
  минимальная (CSS 2.1 §17.5.3). Ячейка `td{height:64px;border:4px;box-sizing:border-box}`
  (content-box 56px) с содержимым выше (52×32 блок + `margin:16px` = 64px content) зажималась
  в 64px → content переполнял ячейку в border-spacing-зазор, pitch строки занижался, ошибка
  накапливалась вниз по таблице (нижние ряды уезжали вверх на px). Замер колонки x=95: Edge
  navy-блоки y=69/229/365/525, Lumen до фикса 69/213/341/485. Фикс в общей ветке вычисления
  высоты блока (`box_tree.rs:5471`): при `s.display == Display::TableCell` used-height =
  `max(specified, content_box)`. Высота строки уже = max ячеек, поэтому подросшая ячейка
  поднимает pitch автоматически. Тесты `table_cell_height_is_minimum_grows_to_fit_content`,
  `table_cell_height_honoured_when_taller_than_content`. Без регрессий (layout 2904, paint
  741+21; полный прогон: единственная дельта 115 FAIL→PASS). TEST-64/69 остаются FAIL по
  другой причине.

- **BUG-144** (2026-06-17) — backdrop-filter row-flip (TEST-30 16.42% → DEBTOR 10.48%).
  Карточки `backdrop-filter` (row 4) рисовались в неверном ряду: элемент `y=439,h=102`
  в вьюпорте 718px появлялся на `y≈177` (`718−(439+102)` — чистый вертикальный флип).
  `elem_id` — GPU-FBO с содержимым элемента, сэмплируемый как `Paint::image` в
  `composite_backdrop_filter_layer`, создавался с одним `PREMULTIPLIED` без `FLIP_Y` →
  bottom-up строки FBO сэмплировались вверх ногами (как opacity/filter offscreen-слои
  до BUG-133/BUG-146). Фикс: `elem_id` через `offscreen_layer_image_flags()`
  (`femtovg_backend.rs:2313`). `filtered_backdrop_id` остаётся без флага (CPU-upload,
  top-down). `backdrop-filter` в Lumen всегда внутри offscreen-слоя → `prev_rt` всегда
  FBO, флип нужен всегда. Тест `offscreen_layer_flags_flip_y_and_premultiplied` (doc
  расширен). TEST-30 → KNOWN_DEBTORS (BUG-144, 10.5). Остаток: filter pixel-parity
  rows 1-3 + backdrop захват тёмным внутри opacity-слоя row 4 + gradient hard-stop
  row 2 (BUG-085). Без регрессий (cargo test -p lumen-paint 741+21 зелёные).

- **BUG-175** (2026-06-17) — скруглённые рамки (TEST-36 border-radius 1.50% → 1.11%).
  `border-radius` + `border`: фон рисовался скруглённым (`FillRoundedRect`), но рамка
  (`DrawBorder`) — 4 axis-aligned прямоугольниками сторон, игнорируя `radii` → квадратные
  углы рамки вокруг скруглённого фона (видно на пилюлях/кругах/эллипсах с бордером).
  Оба пиксельных бэкенда (femtovg live + cpu_raster снапшоты) игнорировали поле
  `radii: CornerRadii` у команды. Фикс: при `border-radius` + однородной (один цвет)
  `solid` рамке граница рисуется **even-odd кольцом** между внешним скруглённым rect
  (border-box) и внутренним (padding-box, внутренние радиусы = внешний − ширина стороны,
  CSS Backgrounds L3 §5.5). Геометрия — `CornerRadii::clamped_to_box`/`inner_for_border`
  (`display_list.rs`) + общий outline-строитель `append_rounded_rect_outline` (femtovg) /
  `push_rounded_rect_outline` (cpu_raster). Неоднородные цвета / dashed-dotted-double →
  fallback на квадратные стороны. Тесты: `draw_border_rounded_corner_is_not_square`
  (пиксельный, cpu-render) + `inner_for_border_*` / `clamped_to_box_caps_at_half`.
  Остаток 1.11% (edge-AA + эллиптические углы kappa) = BUG-176, TEST-36 → KNOWN_DEBTORS.
  Без регрессий (53/64/80 без изменений, 101 4.04% → 3.90%).

- **BUG-174** (2026-06-17) — in-flow SVG `<path>` (TEST-119 paint-order 56.35% → 0.81%).
  `<path>` у `display:inline-block` SVG рисовался в raw user-координатах `d` без смещения
  на origin своего вьюпорта → все пути разных ячеек схлопывались в верхний левый угол
  (видна только первая, чей clip накрывал raw-координаты). Причина: `svg_shape_bbox(Path)`
  = `Rect::ZERO`, а `apply_transform_to_bbox` обнуляет origin для нулевого bbox →
  художник сдвигает вершины на `(0,0)`. У `position:absolute` SVG работало случайно через
  пост-`shift_tree`. Фикс симметричен ветке `SvgText`: для Path якорим `b.rect` в
  `composed.transform_point(ox, oy)` (`box_tree.rs:1198`). Тест
  `inflow_svg_path_box_anchored_at_viewport_origin`. Остаток 0.81% = BUG-173 (40px stroke
  AA-швы), TEST-119 → KNOWN_DEBTORS. Без регрессий TEST-47/54/60/82.

- **BUG-108** (2026-06-17) — TEST-66 5.24%→1.08%. Реальная причина была НЕ `::selection`
  (правила в тесте информационные, выделение не триггерится — видимый контент это свотчи), а
  отсутствие **parent↔last-child bottom margin collapse** (CSS 2.1 §8.3.1): bottom-маргин
  последнего ребёнка оставался внутри `content_height` родителя вместо того чтобы убегать
  наружу. `.section` была 113.6px вместо 83.6px + свой margin → свотчи дрейфовали вниз
  +30px/секция. Фикс симметричен top-коллапсу: `last_collapsible_child` +
  `collapsed_bottom_margin` + `b_collapses_bottom` (`box_tree.rs`); из `content_height`
  вычитается escaped bottom-маргин последнего ребёнка, `child_mb` стал `collapsed_bottom_margin`
  (collapse-through). Корень элемента не коллапсит (`in_block_flow == false`). Остаток 1.08% —
  текст (font-parity, rule 3) + border-radius AA. Тесты:
  `parent_last_child_bottom_margin_collapses`, `bottom_margin_not_collapsed_through_padding`;
  обновлён snapshot `paragraph_with_styles` (body 49→44px). Прогон 09:53 без регрессий.

- **BUG-142** (2026-06-17) — `:host`/`::slotted` (TEST-72: 11.24% → 0.00%). Две причины.
  (1) Каскад без скоупинга: shadow-tree `<style>` вообще не собирались, а document-scope
  `:host`/`::slotted` матчились на любой хост → все 3 хоста красились #3366cc, slotted-цвета
  не применялись. Фикс — `build_shadow_sheets` (per-host лист из shadow root) + thread-local
  `SHADOW_SHEETS`/`SHADOW_HOST_SCOPE`; `:host` матчится только в скоупе своего хоста,
  document-scope `:host`/`::slotted` стали no-op (CSS Scoping L1 §6.1-6.2). (2) Парсер терял
  `<slot>` после `<style>` в declarative shadow `<template>`: rawtext оставлял insertion mode
  `InHead` → `<slot>` не попадал в shadow root → slotted-дети не раскладывались. Фикс в
  `mode_in_template` (`tree_builder.rs`): `original_insertion_mode` `InHead`→`InTemplate`.
  Тесты: `shadow_dom_selectors::*` (8, +`*_in_document_scope_is_noop`),
  `declarative_shadow_dom_slot_after_style_preserved`.

- **BUG-102** (2026-06-17) — SVG advanced stroke (TEST-60: 11.51%). Две причины.
  (1) Главная: `stroke-width`/`stroke-dasharray`/`stroke-dashoffset` (unitless SVG
  user units) молча терялись на standards-mode страницах (`<!DOCTYPE html>`) —
  `apply_declaration` резолвил их через `resolve_box_length`→`parse_length_q`,
  который отвергает unitless-числа вне quirks. Штрихи рисовались дефолтной
  inherited-шириной 1px, dash не применялся. Юнит-тест проходил т.к. его HTML без
  doctype = quirks. Фикс — `resolve_svg_length` (unitless→px независимо от quirks).
  (2) Переписан `stroke_contour_ex` (`svg_path.rs`): quad на сегмент с общими
  per-vertex точками (folded inner-miter + общая miter-точка на выпуклой стороне в
  пределах limit; bevel/round/over-limit — внешний клин через `emit_join`). Гладко
  на flattened-кривых, чисто в острых углах. TEST-60 11.51%→1.41%, TEST-54
  5.58%→2.30%. Остаток (triangle-soup AA-швы, stroke-edge AA, self-intersecting
  fill ear_clip, dash-on-curve) → **BUG-173**, оба теста в `KNOWN_DEBTORS`.
  Тесты: `svg_stroke_geometry_unitless_in_standards_mode`,
  `stroke_ex_bevel_join_has_extra_triangle`.

- **BUG-166** (2026-06-16) — `video_bindings::tests::native_video_load_registers_pending`
  падал при параллельном прогоне `lumen-js` (в изоляции — PASS). Корень: два теста
  (`native_video_load_registers_pending` и `native_video_ready_false_before_decode`)
  гонялись за процесс-глобальный синглтон `video_gif_store`
  (`STORE: OnceLock<RwLock<Option<Arc>>>`). Биндинг `__lumen_video_load` захватывает
  `get_video_gif_store()` в момент `install`; если второй тест перезаписывал глобал
  между `set_video_gif_store` и `install`/`load` первого — load уходил в чужой store,
  а проверка `store.pending_loads` видела пустоту. Фикс — test-only
  `static STORE_GUARD: Mutex<()>`, оба теста берут lock на всё тело, сериализуя доступ
  к глобалу. Прогон 3× зелёный, clippy чист.

- **BUG-163** (2026-06-15) — картинки на lenta.ru показывались серыми боксами. Все
  116 `<img>` там `loading="lazy"`. Две причины: (1) `LazyImageSlot` всегда красил
  серый placeholder даже после загрузки картинки (атрибут `loading=lazy` не
  сбрасывается → при relayout снова `LazyImageSlot`) → теперь несёт
  `object_fit`/`object_position`, бэкенды femtovg+wgpu рисуют по нему
  зарегистрированную картинку с fallback на серый; (2) proximity-check был только в
  `relayout()`, на initial paint не выполнялся → `apply_loaded_page` после
  `register_lazy_images` сразу прогоняет proximity-check + redraw. Регресс-тест
  `lazy_img_slot_carries_object_fit`. Подтверждено скриншотом vs Edge.

- **BUG-165** (2026-06-15) — flex `align-content` (TEST-65: 16.40%) сдвигал строку,
  прибавляя `offset` только к `children[i].rect.y`, но не к поддереву item-а. Потомки
  flex-item-а уже разложены в абсолютных координатах, поэтому при сдвиге строки
  оставались на месте. Заметнее всего на вложенных flex-контейнерах: grandparent
  `__f` (дефолтный `align-content:stretch`) растягивал строки контейнеров, двигал их
  боксы, но items оставались на не-растянутой позиции → вылезали выше контейнеров.
  Фикс в `lay_out_flex` (`box_tree.rs` ~7090): `children[i].rect.y += offset` →
  `shift_y_box(&mut children[i], offset)` (рекурсивный сдвиг поддерева, зеркало
  `shift_tree` из абсолютного позиционирования). Регресс-тест
  `flex_align_content_shifts_item_subtree`. Подтверждено `--dump-layout`: все items
  сидят внутри контейнеров, совпадает с Edge.

- **BUG-158** (2026-06-15) — карточки новостей lenta.ru налезали друг на друга.
  Корень: `<a class="card-mini _topnews">` — flex-item column-flex контейнера
  `.topnews__column` со стилем `flex:1` (→ `flex-basis:0`). В column-flex с
  неопределённой высотой свободного места нет, flex-grow не растит item, и его
  высота оставалась равной flex-basis = 0. Отсутствовал CSS Flexbox §4.5
  *automatic minimum size*. Фикс в `lay_out_flex` (`box_tree.rs`, ветка
  `FlexBasis::Length`/`is_column`): пол высоты = `item.rect.height` из prelim-прохода
  (content height, уже ограниченный реальным `height`), guard `min_height:auto` +
  `overflow_y:visible`. Важно — floor НЕ отключается при `height.is_some()`, иначе
  самозапись `style.height` flex-ом во втором проходе grandparent-row-flex снова
  схлопывала item в 0. Регресс-тест `flex_column_basis_zero_item_keeps_content_height`
  (row-flex > column-flex > `flex:1`, двухпроходный путь). Проверено на живом lenta.ru.

- **BUG-164** (2026-06-15) — внешние `<script src>` не скачивались/не исполнялись (сборщик
  брал только инлайны), из-за чего SPA-бандлы (lenta.ru owlBundle.js и т.д.) молчали.
  Новый `collect_scripts_ordered` помечает внешние скрипты как `ScriptSource::External`,
  `resolve_script_sources` дозагружает их тела через subresource-фетчер
  (`RequestDestination::Script`, зеркало `load_linked_stylesheets`), `run_scripts_with_dom`
  принимает готовые classic/module списки в порядке документа. `src` побеждает inline,
  не-JS блоки (importmap/ld+json/json/speculationrules) игнорируются. То же на restore из
  hibernation. 5 регресс-тестов + функциональная проверка (инъекция `<p>` внешним скриптом
  попала в display list). Снимает в части загрузки JS первопричину BUG-163.
- **BUG-159** (2026-06-15) — z-indexed (own-SC) потомок плоского `overflow:auto`/`scroll`
  scroll-контейнера (не являющегося SC-owner) сбегал из scroll-слоя: его `PushScrollLayer`/
  `PopScrollLayer` эмитятся inline в `contents` родительского SC и закрываются до того, как
  потомок-SC рисуется в позднем слоте painting order → потомок вёл себя как `position:fixed`
  (не скроллился). Фикс в `fill_buckets` (`paint/src/display_list.rs`): non-SC ветка наследует
  `PushScrollLayer` дочерним SC (зеркало clip-наследования BUG-131), `fixed`/`sticky` исключены.
  Регресс-тесты `ordered_zindexed_child_scrolls_with_overflow_auto_ancestor` +
  `ordered_fixed_child_does_not_inherit_ancestor_scroll_layer`; CPU snapshot gate байт-нейтрален.
- **BUG-160** (2026-06-15) — WOFF2-шрифты не декодировались («unexpected end of font data»),
  падал любой реальный сайт с woff2-вебшрифтами. Корень — целиком в реконструкции transformed
  `glyf`/`loca` (`font/src/woff2.rs`, WOFF2 spec §5.2): координаты точек читались из `flagStream`
  вместо `glyphStream`, `instructionLength` — не в том порядке/стриме, формула триплет-декода
  была произвольной, а синтезированная `loca` не согласовывалась с `head.indexToLocFormat`.
  Переписано по эталонному алгоритму (`with_sign` + 6 диапазонов флага); `loca` всегда long-form +
  патч `head` offset 50; bbox simple-глифа без явной записи считается по точкам; поддержан
  `overlapSimpleBitmap`. Регресс — `tests/woff2_real_font.rs` на реальном Fira Mono Regular .woff2.
- **BUG-161** (2026-06-15) — HTTP/2 HPACK-декодер отвергал легальный dynamic table size update
  (ya.ru не грузился): `H2Conn::connect_with_profile` создавал `Decoder::new()` с дефолтным
  `proto_max=4096`, хотя клиент анонсировал `SETTINGS_HEADER_TABLE_SIZE=65536`. Фикс — проставить
  `decoder.set_proto_max(settings.header_table_size)` (`network/src/h2/conn.rs`); симметрия к тому,
  как SETTINGS пира управляют нашим encoder.
- **BUG-162** (2026-06-15) — детектор кодировки выдавал ibm866 на чистом ASCII (example.com): добавлен
  ASCII-shortcut в `detect()` (`encoding/src/detect.rs`) — нет байт ≥0x80 → UTF-8, минуя кириллическую
  эвристику (где `max_by` среди равных score возвращал последний — Cp866).
- **BUG-154** (2026-06-15) — `mix_polar` читал hue из неверного индекса для LCH/Oklch (`layout/src/color_mix.rs`).
- **BUG-122** (2026-06-15) — flaky compositor timing-тесты: idle-tick вынесен в `CompositorThread::spawn_with_tick()`.
- **BUG-155** (2026-06-15) — тест PerformanceObserver LCP: невалидный NodeId 42 → реальный 6 (баг теста).
- **BUG-134 / BUG-156 / BUG-157** (2026-06-15) — ложные регрессии: `run.py` гонял устаревший `lumen.exe`.
  Урок: перед бисектом регрессии сверять timestamp `target/release/lumen.exe` с временем мержа
  (memory `project_runpy_stale_binary`).

---

## Где брать баги

- **Список открытых:** `grep "OPEN" BUGS.md` или `ls bugs/*-OPEN.md`.
- **Детали бага:** файл `bugs/BUG-NNN-OPEN.md` — описание + `file:line`.
- **Закрытие:** переименуй `bugs/BUG-NNN-OPEN.md` → `-FIXED.md` и обнови строку статуса в таблице `BUGS.md`.

Имена файлов дают только список и статус. **Приоритет и исключения** (что НЕ брать —
чужой домен P2/P4, Phase 2 фичи, уже закрытые ложные регрессии) живут в разделе **Next** выше.
Поэтому STATUS-P3 нужен как слой поверх `bugs/`, но дублировать в нём перечень открытых багов не нужно.

---

## Notes

- **Don't context-switch:** Bug fixes are your only focus, finish one before starting another
- **Regression tests:** Every fix gets a test in the same commit — prevents future regressions
- **Coordinate with P1/P2:** Your fixes might unblock their feature work
- **CSS bugs:** If bug is in CSS, note in STATUS-P4.md and continue with implementation bug

See CLAUDE.md §"Bug ownership: P3 only" for full workflow details.
