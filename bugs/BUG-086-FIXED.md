# BUG-086

**Статус:** FIXED 2026-06-09
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

conic-gradient: femtovg triangle-fan не обрезался по box (гигантские круги) + игнорировал repeating; TEST-40 56.53%→15.92% (остаток — AA/тесселяция, класс BUG-085)
