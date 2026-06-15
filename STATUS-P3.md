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
