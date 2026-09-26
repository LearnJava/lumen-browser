# BUG-584: Interest Invokers API (`interestfor`, `InterestEvent`) not implemented at all

**Статус:** FIXED 2026-09-26 (P1, GAP-INTERESTINVOKER)
**Тип:** ДОРАБОТКА — целиком нереализованная фича (IDL-атрибут, событие,
hover/focus-delay таймер-модель, новые CSS-свойства `interest-delay-start`/
`interest-delay-end`, псевдоклассы), не дефект реализованного кода.
Перенесено в [GAP-INTERESTINVOKER](../ROADMAP.md).
**Компонент:** js (`crates/js/src/dom.rs` — grep for `interestFor`/
`InterestEvent` returns zero hits anywhere in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - InterestEvent is not defined
```

36 occurrences, entirely within `interestfor/` (all `.tentative.` files —
27 of 31 id's pull `testdriver.js`, per this run's harness pass rate 22/31).

## Причина

Interest Invokers (WHATWG HTML draft addition, hover/focus-triggered
"preview" popovers via `interestfor="idX"` on `<a>`/`<area>`/`<button>`, the
hover/focus analogue of the existing `command`/`commandfor` click-driven
mechanism — see [BUG-582](BUG-582-FIXED.md), same shape of gap) has no
implementation: no `interestFor` IDL reflection, no
`InterestEvent`/`interest`/`loseinterest` dispatch, no hover/focus-delay
timers driving it.

## Масштаб

Whole feature, self-contained to `interestfor/`. Entirely `.tentative.` —
an early-stage draft, not yet broadly implemented even in other engines —
flagging for scope triage rather than implying priority, same posture as
[BUG-583](BUG-583-OPEN.md) (`<permission>`) found in the same run.

## Ревизия P3 2026-09-15

Реклассифицировано в ДОРАБОТКУ, тот же класс, что BUG-553/554/562/568/583:
целиком отсутствующая фича, не точечный дефект. Отличие от соседнего
BUG-582 (Invoker Commands API, `command`/`commandfor` — реализован
целиком в JS-шиме, без изменений в CSS/layout): у Interest Invokers
поведенческие тесты (`interestfor-basic-delays.tentative.html`,
`interestfor-delay-start.tentative.html`, `interestfor-delay-end.tentative.html`,
`createPopoverAndInvokerForHoverTests` в
`tests/wpt/html/semantics/interestfor/resources/invoker-utils.js:96-121`)
напрямую требуют реальных CSS-свойств `interest-delay-start`/
`interest-delay-end` через `getComputedStyle` — `grep` по `crates/layout`,
`crates/css`, `CSS-SPECS.md` на `interest-delay` даёт ноль совпадений, это
новая пара CSS-свойств, а не заглушка. `interestfor-pseudo-classes.tentative.html`
дополнительно требует псевдоклассов `:has-interest`/`:target-of-interest`.
IDL-часть (`interestForElement` по образцу `commandForElement`,
`InterestEvent` по образцу `CommandEvent`) и hover/focus/blur/Escape-таймер
в JS-шиме без реальных CSS-свойств покрыли бы лишь малую часть из 36
сабтестов (в основном `idlharness.tentative.html`/
`interestevent-interface.tentative.html`/`interestelement-interface.tentative.html`)
и оставили бы поведенческие тесты падать с более запутанной картиной, чем
сейчас — целиком неопределённые символы. Перенесено в
[GAP-INTERESTINVOKER](../ROADMAP.md).

## Закрытие 2026-09-26 (P1, GAP-INTERESTINVOKER)

Реализовано в JS-шиме плюс пара CSS-свойств в движке:

- `interestForElement` на `HTMLButtonElement`/`HTMLAnchorElement`/`HTMLAreaElement`/
  `SVGAElement` (`_lumen_install_interest_for`, `web_api_shim_tail_b.js`), `InterestEvent`
  с `source` (ретаргет по shadow-деревьям), события `interest`/`loseinterest`;
- таймерная модель показа/снятия интереса по hover (`mouseover`) и фокусу, снятие по Escape
  без `cancel`, транзитивное «вовлечение» (интерес держится, пока вовлечённый инвокер внутри
  цели), связка с popover (показ/скрытие цели, вложенные auto-popover не закрываются);
- CSS `interest-delay-start`/`interest-delay-end`/шортхенд `interest-delay`
  (`normal | <time [0s,∞]>`, не наследуются, `ComputedStyle`, `getComputedStyle`, CSSOM);
- псевдоклассы `:interest-source`/`:interest-target` через скрытые атрибуты
  `data-lumen-interest-source`/`-target`.

WPT `html/semantics/interestfor`: подтесты 49/268 → 164/244 (знаменатель упал, потому что
три hover-варианта `interestfor-delay-start` теперь доходят до `test_driver` и падают
ERROR на селекторе [BUG-1063](BUG-1063-OPEN.md), а раньше отрабатывали все 8 подтестов
в FAIL). `idlharness` 44/44, `interestelement-interface` 6/6, `interestevent-interface`
10/10, `interestfor-css-shorthands` 26/26, focus-варианты `delay-start`/`delay-end` 7/8 и 5/5.

Остаток вне задачи:

- hover- и Escape-варианты через `test_driver.Actions()` — исполнитель не наводит курсор
  и не шлёт Escape ([BUG-1194](BUG-1194-OPEN.md)), плюс селектор `*|body` ([BUG-1063](BUG-1063-OPEN.md));
- 44 подтеста интерполяции в `interestfor-css-properties` (38/82): CSS Transitions/Animations
  не интерполируют `interest-delay-*` (планировщик переходов в `animation.rs` не знает
  `<time>`-свойств), Web Animations — [BUG-1195](BUG-1195-OPEN.md);
- псевдоэлемент `::interest-button` и reftest'ы `interestfor-pseudo-element-*` не входили.
