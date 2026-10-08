# BUG-1232 — `Range` не обновляется при мутациях DOM (live range)

**Статус:** OPEN
**Заведён:** 2026-10-01 (P3, остаток [BUG-1159](BUG-1159-FIXED.md))
**Область:** js/dom — `_lumen_make_range` хранит `[nid, offset]` и не перепривязывается: DOM §4.2.x
"live ranges" (remove, insert, replace data, split text, adopt) не реализованы.

## Симптом

`dom/ranges`: `Range-mutations-appendChild/insertBefore/replaceChild` — TIMEOUT без сабтестов;
`Range-mutations-dataChange` 209/2808, `-removeChild` 2/20, `-replaceData` 845/1146, `-deleteData`
456/564, `-insertData` 326/382, `Range-adopt-test` 0/4 (`expected 0 but got 1`: удаление/перенос
единственного элемента диапазона не схлопывает его).

## Что чинить

Реестр живых диапазонов и правила обновления границ при remove/insert/replace data/splitText/adopt
(DOM §5.5 «live range»); без утечки диапазонов (слабые ссылки).
