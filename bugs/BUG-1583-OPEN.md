# BUG-1583 — `AnimationEvent` и `TransitionEvent`: конструктор не требует тип, нет `animationName`/`pseudoElement`/`elapsedTime` на прототипе, интерфейс перечисляем

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** js (`crates/js/src/shim/*.js` — определение `AnimationEvent`/`TransitionEvent`)

## Симптом

События доставляются (`animationstart`/`animationend`/`transitionrun`/`transitionstart`/`transitionend` приходят для разметочных анимаций — проба ниже), но сами интерфейсы описаны вне WebIDL: атрибуты — собственные свойства экземпляра, а не геттеры прототипа; конструктор допускает вызов без типа.

## Проба

`run_smoke.py` + testharness:

| вызов | у нас | ожидается |
|---|---|---|
| `new AnimationEvent()` | не бросает | `TypeError` |
| `AnimationEvent.length` | 2 | 1 |
| `"animationName" in AnimationEvent.prototype` | `false` | `true` |
| `"elapsedTime" in AnimationEvent.prototype`, `"pseudoElement" in …` | `false` | `true` |
| `"propertyName" in TransitionEvent.prototype` | `false` | `true` |
| `Object.keys(window).includes("AnimationEvent")` | `true` | `false` |
| события `animationstart`/`animationend` у `<div style="animation:o .3s linear">` | приходят (`animationstart:o`, `animationend:o`) | приходят |
| `transitionrun`/`transitionstart`/`transitionend` при смене `opacity` | приходят | приходят |

## Как найдено

WPT-RUN-14 срез 26: `css/css-animations/animationevent-*.html`, `css/css-transitions/transitionevent-interface.html`, `events-008.html`, `idlharness*.html`.

## Что делать

Описать оба интерфейса по WebIDL: геттеры на прототипе, обязательный `type`, `enumerable: false` у конструктора, атрибут `animation` у `TransitionEvent` (CSS Transitions 2), корректный `pseudoElement`.

## Как проверить

`css/css-animations/animationevent-interface.html`, `css/css-transitions/idlharness.html`.
