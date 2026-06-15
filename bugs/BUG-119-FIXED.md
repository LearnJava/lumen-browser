# BUG-119

**Статус:** FIXED 2026-06-10
**Компонент:** test/html
**Файл:** `graphic_tests/*.html`

## Описание

6 run.py regressions (TEST-27/28/29/40/41/68) blamed on selector rule index (bb1f8e99) — actual root cause: bulk title-tag commit 88cdb9e1 (same evening, between runs) left a raw U+0001 byte in `<head>` of 17 test pages. Non-whitespace char closes `<head>` per HTML spec → byte rendered as body text, 19.2px line at top, all content shifted ~20px down (diff_region top:0 full-width on every degraded test). Rule index exonerated: `--dump-layout`/`--dump-display-list` byte-identical with index vs brute-force on all 6 pages. Fix: U+0001 lines replaced with the `<meta charset="utf-8">` they had overwritten; regression test `graphic_test_pages_have_no_stray_control_bytes`.
