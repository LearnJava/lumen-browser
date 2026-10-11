# BUG-1604 — Geometry: `DOMMatrix`/`DOMQuad`/`DOMRectList` — разбор строк, стрингификатор, воркеры, IDL

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** js (`crates/js/src/` — `DOMMatrix`/`DOMMatrixReadOnly`/`WebKitCSSMatrix`/`DOMQuad`; воркерный глобал)

## Симптом

`new DOMMatrix("foo")` бросает `TypeError`, но `new DOMMatrix("NONE")` и `new DOMMatrix("none/**/")` — `could not parse transform string`: регистр и комментарии CSS не разбираются. В воркере `DOMMatrix`/`DOMMatrixReadOnly` отсутствуют (`idlharness.any.worker.html`: `ReferenceError: DOMMatrix`, 117 + 50 + 42 подтестов).

## Охват

WPT-RUN-14 S28: `css/geometry` — 30 id, зелёных 11; не зелёных 19, 802 из 1 392 подтестов: `idlharness.any.html` (167 из 372), `idlharness.any.worker.html` (354 из 360), `DOMMatrix-001` (92 из 138), `DOMMatrix-stringifier` (21 из 78), `DOMMatrix2DInit-validate-fixup` (26 из 112), `DOMMatrixInit-validate-fixup` (18 из 91), `DOMMatrix-003`, `DOMMatrix-attributes`, `DOMQuad-001/002`, `historical`, `structured-serialization`, `spec-examples`, `WebKitCSSMatrix`. Причина установлена пробой только для разбора строки и воркера; остальное — по сообщениям. В `docs/engine-gaps.md` строка про `DOMMatrix` сказано «shipped 2026-09-05 (GAP-GEOM)» — отгружена не полностью.

## Как проверить

`css/geometry/DOMMatrix-001.html`, `DOMMatrix-stringifier.html`, `idlharness.any.worker.html`.
