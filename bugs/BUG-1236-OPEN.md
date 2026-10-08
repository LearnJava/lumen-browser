# BUG-1236 — `cpu_snapshots_match_references` падает на чистом `main` (9517871d0, без правок P4)

**Статус:** OPEN
**Тип:** не локализован / верификация.
**Заведён:** 2026-10-01 (P4, найден в гейте задачи `pointer-events`, не относится к ней)
**Область:** test/snapshot (`crates/driver/tests/cases/snapshot_cpu.rs`, `graphic_tests/snapshots/cpu/`).

## Симптом

`cpu_snapshots_match_references` падает на чистом `main` (9517871d0, без правок P4): 12-display 5280, 13-visibility-opacity 7740, 18-images 6590, 47-svg-basic 21520, 57-canvas-2d 3600, 20-quirks-bgcolor 5280, 34-forms 12300 различающихся байт из 2949120. Воспроизводится идентично с `git stash` и без; причина не установлена (эталоны устарели после чужих merge или зависят от машины).

## Как проверить

`cargo test -p lumen-driver --test all cpu_snapshots_match_references` на чистом `main`. Если расхождения — следствие чужого пиксельного коммита, регенерировать `SAVE_CPU_SNAPSHOTS=1`; если от машины/шрифтов — отделить от эталонов.
