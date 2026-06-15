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
