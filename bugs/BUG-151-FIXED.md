# BUG-151

**Статус:** FIXED 2026-06-13
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

Parent-first-child margin collapse не применяется (CSS 2.1 §8.3.1): TEST-109 c3 — родитель 300×300 без border/padding/BFC, ребёнок с margin:70px; Edge коллапсирует margin-top сквозь родителя (родитель уезжает на y+70, ребёнок прижат к его верху), Lumen держит ребёнка на 70px внутри (dump-layout: родитель y=375, ребёнок y=445). Доминирующий остаток TEST-109 (4.80%); вероятно связан с BUG-136 (TEST-105, контроль c5 — блоки с margin)
