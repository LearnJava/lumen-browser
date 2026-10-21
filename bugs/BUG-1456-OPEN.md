# BUG-1456 — `CSSPseudoElement` и `Element.pseudo()` не реализованы: 12 id, 72 упавших сабтеста

**Статус:** OPEN (ДОРАБОТКА → CSSOM-12)
**Сделано в CSSOM-11 (2026-10-21):** `CSSPseudoElement`, `Element.pseudo()`, геометрия блочных псевдоэлементов и `convert*FromNode` — `CSSPseudoElement-identity.html`, `-convertPoint.tentative.html`, `-getBoxQuads.tentative.html` зелёные, `-geometry-edge-cases.tentative.html` 22/23. Остаток ниже и в `CSSOM-12`: события/`pseudoTarget`/`:hover` на псевдоэлементе, живое дерево `::view-transition*`, бокс у пустого inline-`::before`, `idlharness.html`.
**Тип:** остаток — отсутствующая функциональность (задача ROADMAP `CSSOM-12`), не дефект.
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js (`Element.prototype.pseudo()`, `CSSPseudoElement`, `getPseudoElements()` — отсутствуют вовсе)

## Симптом

`typeof document.getElementById('a').pseudo` = `undefined`, `typeof CSSPseudoElement` = `undefined`, `typeof el.getPseudoElements` и `window.getPseudoElements` — `undefined`. Страдают `CSSPseudoElement-convertPoint.tentative.html` (7 из 7), `CSSPseudoElement-geometry-edge-cases.tentative.html` (23 из 23), `CSSPseudoElement-getBoxQuads.tentative.html` (8 из 8), `CSSPseudoElement-identity.html`, `CSSPseudoElement-view-transitions*.tentative.html`, `events-on-after-before-marker.tentative.html`, `events-on-pseudo-element*.tentative.html`, `hover-on-pseudo-elements.tentative.html`, `idlharness.html` (часть из 29 упавших сабтестов: `window.getPseudoElements is not a function`, `… is not defined`). Это интерфейс (CSS Pseudo 4 §3 / `CSSPseudoElement`), без него события и геометрия псевдоэлементов недостижимы.

## Проба

Проба: `document.getElementById('a').pseudo`, `typeof CSSPseudoElement`, `typeof window.getPseudoElements` — все `undefined`.

## Как найдено

WPT-RUN-14 срез 20: сообщения `target.pseudo is not a function`, `el.pseudo is not a function`, `CSSPseudoElement interface not supported`.

## Что делать

Задача `CSSOM-12` (`ROADMAP.md`; интерфейс и геометрия — `CSSOM-11`, сделана): `Element.pseudo(type)`, интерфейс `CSSPseudoElement` (`element`, `type`, `getBoundingClientRect`, `getClientRects`, цель событий), `getPseudoElements()`.

## Как проверить

`css/css-pseudo/CSSPseudoElement-identity.html`, `css/css-pseudo/idlharness.html`.
