# BUG-945 — `pagereveal` не диспатчится нигде: страница, ждущая его, зависает на первом `await`

**Статус:** OPEN
**Тип:** нереализованная функциональность — событие никогда не диспатчится ни из одного места.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** js (`crates/js/src/view_transitions.rs`, персистентная навигация `crates/shell/src/persistent_js.rs`)
**Владелец:** P3.

## Симптом

`window.addEventListener('pagereveal', …)` никогда не вызывается —
workspace-wide `grep -rn pagereveal crates/` даёт ноль совпадений. По HTML
Living Standard §7.3 (и CSS View Transitions Module Level 2, которая
расширяет его событие `PageRevealEvent.viewTransition`) `pagereveal`
диспатчится один раз на `window` после каждой навигации, до первого
рендера документа — независимо от того, участвует ли навигация в view
transition.

## Прямое измерение

Workspace-wide `grep -rn pagereveal crates/` — ноль совпадений.
`view_transitions.rs` реализует `document.startViewTransition` целиком
(проверено живыми пробами слайсов 30/31 — `elements-at-point`/`pseudo-
computed-style` варианты сработали), но навигационные события View
Transitions 2 (`pagereveal`/`pageswap`) в него не заведены.

## Кого это держит

`css/css-view-transitions/navigation/pagereveal-no-view-transition.html` —
тест ждёт `pagereveal` на обычной (не-view-transition) навигации; событие
не приходит никогда, страница висит на первом `await`.

## Направление починки

Найти единственную точку, где документ уже помечается «загружен и готов к
первому рендеру» (там же, где сейчас диспатчится `load`/`DOMContentLoaded`
после навигации — `persistent_js.rs`), и добавить туда диспатч `pagereveal`
до первого paint с `viewTransition: null` для обычной навигации. Второй шаг
(`viewTransition` заполнен, когда навигация участвует в cross-document view
transition) зависит от того, насколько уже реализованы cross-document view
transitions — вне этого бага, если совсем не заведены.
