# BUG-1510 — `ViewTransition` не является интерфейсом: нет глобала `ViewTransition`, `ViewTransitionTypeSet`, `CSSViewTransitionRule`, `PageRevealEvent`, `PageSwapEvent`; нет `types`, `transitionRoot`, `document.activeViewTransition`; `view-transition-class`/`-group` не разбираются

**Статус:** OPEN (ДОРАБОТКА → VT-API)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** js (`crates/js/src/view_transitions.rs` — `startViewTransition` возвращает простой объект)

## Симптом

`document.startViewTransition()` возвращает объект с `ready`/`finished`/`updateCallbackDone`/`skipTransition` — `vt.constructor.name` — `Object`. Нет: глобалов `ViewTransition`, `ViewTransitionTypeSet`, `CSSViewTransitionRule`, `PageRevealEvent`, `PageSwapEvent`, `NavigationActivation`-связки; членов `types` (L2), `transitionRoot`, `document.activeViewTransition`; обработчиков `onpagereveal`/`onpageswap` (BUG-945); `startViewTransition({update,types})`; `CSS.supports('view-transition-class','b')` и `('view-transition-group','normal')` — `false` (свойства не разбираются — строки `CSS-SPECS.md` для P4), `view-transition-name`/`-class` не отдаются в `getComputedStyle`. Объём — семейство API с моделью состояния (`skipTransition`, `duplicate-tag` → отказ, типы, `activeViewTransition`), а не один член.

## Проба

Проба (`--mcp`):

| проверка | у нас | ожидается |
|---|---|---|
| `typeof ViewTransition`, `…ViewTransitionTypeSet`, `…CSSViewTransitionRule`, `…PageRevealEvent`, `…PageSwapEvent` | `undefined` ×5 | `function` |
| `typeof document.startViewTransition` | `function` | `function` |
| `"activeViewTransition" in document` | `false` | `true` |
| `typeof vt.types`, `typeof vt.transitionRoot` | `undefined` | `object` |
| `vt.constructor.name` | `Object` | `ViewTransition` |
| `"onpagereveal" in window`, `"onpageswap" in window` | `false` | `true` |
| `CSS.supports('view-transition-name','a')` | `true` | `true` |
| `CSS.supports('view-transition-class','b')`, `('view-transition-group','normal')` | `false` | `true` |
| `getComputedStyle(a).viewTransitionName` | `` | `a` |

## Как найдено

WPT-RUN-14 срез 22: `css-view-transitions/{idlharness (17 из 66 сабтестов), document-active-view-transition, duplicate-tag-rejects-*, auto-name-get-animations, parsing/view-transition-{name,class,group}-*}` и др. — 24 id в кластере API + 15 id `parsing/` (1 006 сабтестов, из них 56 — BUG-1439).

## Что делать

Реализовать интерфейс `ViewTransition` и родственные (L1 + `types`/`activeViewTransition` из L2), разбор `view-transition-class`/`view-transition-group`, события `pagereveal`/`pageswap` (BUG-945), `CSSViewTransitionRule` (CSSOM-10).

## Как проверить

Таблица выше; `css/css-view-transitions/idlharness.html`, `document-active-view-transition.html`.
