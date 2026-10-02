# BUG-1248 — анимированное значение SMIL не попадает в `getComputedStyle`

**Статус:** OPEN
**Заведён:** 2026-10-02 (P6, найден в BUG-1095 срез 4)
**Область:** js/layout

## Симптом

`_lumen_smil_overrides` читается только геттером `animVal` длин. Презентационные атрибуты (`fill`, `class`, `opacity`…) анимацией не меняются в computed style и не рисуются: `syncbase-escaped-dots.html` (2 подтеста), `smil-values-empty-segments.html` (class), `animatetransform-type-missing-value-default.html` и ~200 однопод­тестовых файлов на `SVGAnimationTestCase` остаются красными. Это нереализованная функциональность (ROADMAP), а не регрессия.
