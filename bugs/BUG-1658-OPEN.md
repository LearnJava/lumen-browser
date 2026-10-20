# BUG-1658 — `counter-reset` не живёт до конца следующих соседей

**Статус:** OPEN
**Заведён:** 2026-10-20 (P6, остаток BUG-1395)
**Область:** layout (`crates/engine/layout/src/counters.rs::walk`, `CounterCtx::pop_reset`)

## Симптом

CSS Lists L3 §4.4: экземпляр счётчика, созданный `counter-reset`, виден самому элементу, его потомкам и следующим соседям; `counter-reset` следующего соседа с тем же именем заменяет этот экземпляр. `walk` снимает reset при выходе из элемента (`pop_reset(&style.counter_reset)`), поэтому следующие соседи видят внешний экземпляр.

Проба (`css/css-contain/contain-style-counters-005.html`, ожидание в `reference/contain-style-counters-005-ref.html`): внутри `contain:style` блок `increment, reset, increment` должен дать `11` во втором `increment`, у нас — `12`.

## Что делать

Хранить reset до выхода из родителя (как `fresh_log` в BUG-1395), заменяя экземпляр, созданный предыдущим соседом. Затем проверить `contain-style-counters-005`.
