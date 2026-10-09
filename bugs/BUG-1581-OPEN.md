# BUG-1581 — Модель CSS Animations/Transitions в JS: нет интерфейсов `CSSAnimation`/`CSSTransition`, `getAnimations()` отдаёт пустые оболочки без эффекта и таймлайна

**Статус:** OPEN (ДОРАБОТКА → CSSANIM-MODEL)
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid_b4.js:761` `_lumen_css_anim_register` — запись `Animation` с пустым `KeyframeEffect`; планировщики `AnimationScheduler`/`TransitionScheduler` стартуют на тике кадра)

## Симптом

Web Animations для CSS-анимаций и переходов держится на слое-заглушке: в `_wa_animations` кладётся `Animation` с пустым `KeyframeEffect`, чтобы `getAnimations().length` был ненулевым, а значения рисует Rust-планировщик (комментарий в `web_api_shim_mid_b4.js:761…`). Всё, что тест читает дальше, — `.effect`, `.ready`, `.currentTime`, `.startTime`, `.animationName`, `.transitionProperty`, `.pending`, `.finished` — либо `undefined`, либо пусто.

## Проба

`run_smoke.py` + testharness, `<div id=a>`, `@keyframes o{from{opacity:0}to{opacity:1}}`, `@keyframes k{…}`:

| вызов | у нас | ожидается |
|---|---|---|
| `typeof CSSAnimation`, `typeof CSSTransition`, `typeof CSSPseudoElement` | `undefined` ×3 | `function` ×2, `function` |
| `a.getAnimations().length` сразу после `a.style.animation = "k 100s"` | 0 | 1 (создаётся при пересчёте стиля) |
| то же через два `requestAnimationFrame` | 1 | 1 |
| `Object.prototype.toString.call(a.getAnimations()[0])` | `[object Object]` | `[object CSSAnimation]` |
| `.animationName`, `.id` | `""`, `k` | `k`, `""` |
| `.effect.getKeyframes().length`, `.effect.getComputedTiming().duration` при `animation: k 100s` | 0, 0 | 2, 100000 |
| `.currentTime`, `.startTime` | пусто | число |
| `.pending` | `false` | `false` |
| `<div style="transition:opacity 5s linear;opacity:0">`, `style.opacity = "1"` → `getAnimations().length` сразу | 0 | 1 |
| то же через 150–200 мс | 0 или 1 (нестабильно, BUG-1293) | 1 |

## Как найдено

WPT-RUN-14 срез 26: 57 id `css-animations/CSS*`, `css-transitions/CSSTransition-*`, `KeyframeEffect-*`, `Document-getAnimations*`, `AnimationEffect-*`, `Element-getAnimations*`, `event-dispatch*`, `animation-composition*` (`Cannot read properties of undefined (reading 'ready'|'effect'|'currentTime')`; для переходов `getAnimations()[0]` — `undefined` ещё и из-за BUG-1234: переход свойства вне opacity/transform/color/background-color/height не создаётся вовсе).

## Что делать

Задача CSSANIM-MODEL: настоящие `CSSAnimation`/`CSSTransition` (наследники `Animation`), эффект из `@keyframes`/пары значений (`getKeyframes()`, `getComputedTiming()`, `target`/`pseudoElement`), общий таймлайн и `ready`/`finished`/`pending`, создание при пересчёте стиля (BUG-1293), `animation-play-state` ↔ `pause()/play()`, замена эффекта (`effect = null`), порядок композиции (`CSSAnimation-compositeOrder`), `CSSPseudoElement` как цель (CSSOM-11).

## Как проверить

`css/css-animations/CSSAnimation-id.tentative.html`, `Element-getAnimations.tentative.html`, `css/css-transitions/CSSTransition-ready.tentative.html`.
