# BUG-507: CSS Exclusions (`wrap-flow`/`wrap-through`) entirely unimplemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser/layout — property not recognized anywhere
(`grep -rn "wrap-flow\|wrap-through" crates/` returns zero hits)
**Найден:** WPT-RUN-3 срез 13 (`ROADMAP.md`) — массовый прогон `css/css-exclusions`

## Симптом

Every one of the 8 vendored `css/css-exclusions` files fails, all on the same
root cause — the two properties this Editor's Draft defines
(`wrap-flow`/`wrap-through`) are not parsed/recognized at all:

- 7 files (`wrap-flow-001`…`006`, `wrap-through-001`) call
  `assert_implements('wrap-flow: <value>', ...)`/`assert_implements('wrap-through:
  none', ...)` as a setup precondition before registering any test — the
  assertion throws, so the harness itself reports `ERROR` with **0 subtests
  registered** (`0/0`), not a ordinary subtest FAIL:
  ```
  TEST_END: ERROR, expected OK - Error: assert_implements: 'wrap-flow: clear' undefined
  ```
- `inheritance.html` runs to completion (harness `OK`) but all 4 subtests
  FAIL — `getComputedStyle` never reports a `wrap-flow`/`wrap-through` value
  at all (`assert_true: wrap-flow doesn't seem to be supported in the
  computed style`), consistent with the property never reaching
  `ComputedStyle`.

## Контекст спеки

CSS Exclusions and Shapes Module Level 1 (`https://drafts.csswg.org/css-exclusions/`)
is an old, still-Editor's-Draft-status spec; `wrap-flow`/`wrap-through` never
shipped in any evergreen browser (only an experimental IE10/11
implementation) and are not on any current shipping-priority list — unlike
every other WPT-RUN-3 finding so far this is a "spec never implemented,
correctly" gap rather than a regression, but it's filed per track policy
(vendored test = expected:FAIL + BUG-NNN, never silently weakened).

## Масштаб находки

All 8 files / 8 testharness ids in `css/css-exclusions` (module's entire
vendored surface) — 7 ERROR (0 subtests each) + 4 FAIL subtests in the 8th.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-exclusions/` for all 8
files: `expected: ERROR` (no subtests) for the 7 `assert_implements`
setup-failures, per-subtest `expected: FAIL` for `inheritance.html`.
