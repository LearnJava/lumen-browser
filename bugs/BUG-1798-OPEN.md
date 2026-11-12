# BUG-1798 — `canvas.drawElementImage` — заглушка, не рендерит элемент

**Статус:** OPEN
**Заведён:** 2026-11 (по строке `BUGS.md`; файл создан при ревизии очередей P5 2026-11-12 — ссылка из `BUGS.md` была битой)
**Область:** js (`crates/js/src/canvas2d.rs`, `crates/js/src/shim/web_api_shim_mid.js` — `drawElementImage`)

## Суть

`canvas.drawElementImage(element, dx, dy)` — заглушка: только помечает canvas tainted/dirty, не рендерит элемент.

## Что нужно

Реальная реализация: найти LayoutBox по `__nid__`, построить display list поддерева, отрендерить в offscreen bitmap (CPU rasterizer), нарисовать на canvas. Требует передачи layout root в JS-поток или shell-side callback (IPC bitmap).

## Проверка

WPT: `tests/wpt/html/canvas/element/manual/draw-element-image/*.tentative.html` — все падают.
