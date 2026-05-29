# STATUS-P3 — Bug Fixes + Driver Infrastructure

**Developer:** Программист 3 (Bug fixes + lumen-driver infrastructure)

---

## In progress
_(none)_

## Next

| # | Task | Branch | Актуальное состояние |
|---|---|---|---|
| 8A.6 | Migrate graphic tests | `p3-8a6-migrate-graphic-tests` | **Частично готово** — `crates/driver/tests/test_01..49.rs`. Нужно: проверить покрытие всех 22 оригинальных HTML-тестов, сгенерировать эталонные PNG в `graphic_tests/snapshots/` |

**Порядок:** 8A.1 → 8A.2 → 8A.6 (каждая зависит от предыдущей)

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
