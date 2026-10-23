# BUG-1510 — `ViewTransition` не является интерфейсом: нет глобала `ViewTransition`, `ViewTransitionTypeSet`, `CSSViewTransitionRule`, `PageRevealEvent`, `PageSwapEvent`; нет `types`, `transitionRoot`, `document.activeViewTransition`; `view-transition-class`/`-group` не разбираются

**Статус:** FIXED 2026-10-23 (P3)
**Тип:** нереализованная функциональность; срез 1 (интерфейсы и автомат) сделан задачей `VT-API` 2026-10-21, остаток — `VT-API-2`.
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

## Сделано в VT-API (2026-10-21, ветка `p1-vt-api`)

Проба из таблицы выше: `typeof ViewTransition|ViewTransitionTypeSet|CSSViewTransitionRule|PageRevealEvent|PageSwapEvent` — `function`;
`"activeViewTransition" in document`, `typeof vt.types|vt.transitionRoot`, `vt.constructor.name === 'ViewTransition'`,
`"onpagereveal" in window`, `"onpageswap" in window`, `getComputedStyle(a).viewTransitionName === 'a'` — выполняются.
`startViewTransition({update, types})`, отказ по дублю имени, `TimeoutError` колбэка, `waitUntil` — работают
(`crates/js/src/shim/view_transitions_shim.js`, тесты `crates/js/src/dom/tests/v8_vt_api.rs`).

## Остаток (VT-API-2 и P4)

- События `pagereveal`/`pageswap` не отправляются (BUG-945); интерфейсы и обработчики есть.
- `CSS.supports('view-transition-class','b')` и `('view-transition-group','normal')` — `false`: свойства не разбираются (строки `CSS-SPECS.md` для P4).
- `Element.prototype.startViewTransition`/`activeViewTransition` — VT-SCOPED (BUG-1511).
- UA-стиль и анимации псевдодерева — BUG-1509.

## Исправление (2026-10-23, P3, ветка `p3-bug-1510`)

Свойства `view-transition-class` (`none | <custom-ident>+`) и `view-transition-group`
(`normal | contain | nearest | <custom-ident>`): поля `ComputedStyle::view_transition_class`/
`view_transition_group`, разбор с валидацией в `style/apply/motion.rs` (недопустимое значение
отбрасывает объявление), css-wide ключевые слова в `css_wide.rs`, запись в `SUPPORTED_PROPERTIES`
(`CSS.supports`), отдача в `getComputedStyle` (`selector_query.rs`). Тесты:
`anchor_positioning_tests::view_transition_{class,group}_parsed_and_validated`,
`v8_vt_api::class_and_group_properties_are_supported_and_reported`.

Остаток вынесен: `pagereveal`/`pageswap` — BUG-945; селектор `::view-transition-group(name.class)`
и вложенные группы (`nested/*`) — VT-API-2.
