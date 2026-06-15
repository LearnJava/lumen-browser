# BUG-118

**Статус:** FIXED 2026-06-09
**Компонент:** test/snapshot
**Файл:** `graphic_tests/snapshots/cpu/`

## Описание

snapshot_cpu reference PNGs outdated for 12 pages: references saved before BUG-117/107/106/096 fixes. Fixed by regenerating via SAVE_CPU_SNAPSHOTS=1.
