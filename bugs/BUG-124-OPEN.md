# BUG-124

**Статус:** OPEN
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

TEST-51 residual 1.09% (thr 0.5%): 1px horizontal AA lines at every block edge — fractional layout Y coords (52.20/72.20/196.20 from h2 line-height 19.2px) vs Edge integer device-pixel snapping. Systemic, affects most tests; root-cause task = PS-1 «pixel snapping единая политика» (reserved by P1 2026-06-10, STATUS-P1.md). Re-run TEST-51 after PS-1 lands
