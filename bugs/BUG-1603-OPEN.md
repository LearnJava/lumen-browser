# BUG-1603 — `color-scheme: dark` не меняет фон холста и начальный `color`; iframe не наследует схему

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** layout/paint (`color-scheme` — «UA switching wired: form controls + system-color resolution» по `CSS-SPECS.md:229`; холст и `color: initial` не подключены)

## Симптом

`<html style="color-scheme:dark">x` рисуется на белом холсте чёрным текстом: пиксели (90,50) и (5,55) — `(255,255,255)`, `getComputedStyle(body).color` — `rgb(0, 0, 0)`. По CSS Color Adjust 1 §2 при `dark` холст `Canvas` и начальный `CanvasText` — тёмные. `color-scheme-color-initial-affected-by-color-scheme-property.html`: оба подтеста падают — `assert_not_equals: got disallowed value "rgb(0, 0, 0)"`.

## Охват

WPT-RUN-14 S28: `css/css-color-adjust` — 36 id, зелёных 12. `rendering/dark-color-scheme/*` — 19 id (14 reftest `thick`, 1 `no-match-ref`, 2 TIMEOUT-iframe — по BUG-480/957, на новом бинаре ещё 3 из-за BUG-1597); CSSOM `parsing/color-scheme-{valid,computed,invalid}` и `animation/*-no-interpolation` — 5 id (47 из 135 подтестов: `light only` сериализуется как `only light`, `normal dark`/`only normal` принимаются). Причина «холст не тёмный» установлена пробой; iframe-кластер — по именам.

## Как проверить

`css/css-color-adjust/rendering/dark-color-scheme/color-scheme-color-initial-affected-by-color-scheme-property.html`, `color-scheme-iframe-preferred-page-dark.html`, `parsing/color-scheme-invalid.html`.
