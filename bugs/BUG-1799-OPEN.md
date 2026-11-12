# BUG-1799 — workspace-clippy на чистом main красный: мёртвые `rasterize_cpu*` в lumen-paint

**Статус:** OPEN
**Заведён:** 2026-11-12 (P5, ревизия очередей)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs:217`, `:233`)

## Суть

`cargo clippy -p lumen-paint --all-targets -- -D warnings` падает на чистом `main` (проверено stash-экспериментом 2026-11-12):

```
error: function `rasterize_cpu` is never used        (cpu_raster.rs:217)
error: function `rasterize_cpu_with_fonts` is never used (cpu_raster.rs:233)
```

Это ломает гейт закрытия задачи (`clippy -p <crate> --all-targets -- -D warnings`) для любого, кто трогает lumen-paint.

## Происхождение

Вызовы ушли при рефакторе border-image (BUG-492, `d6006b287f`, след `7ea95d863c`). Функции остались; на Windows-таргете вызывателей нет.

## Что делать

Либо удалить обе функции (тесты `#[cfg(test)]` в этом же файле их не зовут — сверить), либо вернуть вызывателя. Решение за фиксу; проверку делать на `--all-targets`.
