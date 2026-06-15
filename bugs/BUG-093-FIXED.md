# BUG-093

**Статус:** FIXED 2026-06-11
**Компонент:** paint
**Файл:** `graphic_tests/run.py`

## Описание

scrollbar rendering TEST-51: 1.39% — **2026-06-10 closure was wrong**: threshold calibration 0.5→2.0% masked a real defect (no scrollbar skin involved — neither Edge headless nor Lumen drew scrollbars on this page). Real cause = BUG-123 (scroll container's own border/background clipped by its PushScrollLayer scissor). Thresholds reverted to 0.5; **threshold changes are forbidden** — fix the engine instead
