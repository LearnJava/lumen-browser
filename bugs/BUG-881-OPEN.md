# BUG-881 — Navigation API: событие `navigate` приходит только из `navigation.navigate()`; `location.href = "#x"` не диспатчит ничего, `on<type>`-свойств у `navigation` нет

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, варианты `navigation-api`/`navigation-onprops`)
**Область:** `crates/js/src/dom.rs` — объект `navigation` и его `navigate()`; путь фрагментной навигации `_lumen_navigate_or_fragment`/`_lumen_location_update` о `navigation` не знает
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`window.navigation` есть, и вызов `navigation.navigate('#x')` действительно
диспатчит `navigate` и `currententrychange`. Но любая другая навигация того
же документа — `location.href = "#1"`, `location.hash = …`, клик по ссылке —
проходит мимо: адрес меняется (`location.hash === "#1"`,
`navigation.entries().length === 1`, `currentEntry` заполнен), а событий нет
ни одного.

Вторая половина — `on<type>`-свойства. `'onnavigate' in navigation`,
`'onnavigatesuccess'`, `'onnavigateerror'`, `'oncurrententrychange'` — все
`false`; присваивание `navigation.onnavigate = fn` «прилипает» как обычное
поле объекта и никем не читается (та же форма, что
[BUG-874](BUG-874-OPEN.md) у `document`).

Замеренные мелочи того же объекта: `e.canIntercept`, `e.navigationType`,
`e.hashChange` и `e.from` у `NavigationCurrentEntryChangeEvent` —
`undefined`; `navigation.currentEntry` до первой навигации — `null`
(спека требует запись для стартового документа).

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant navigation-onprops`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
nop-onnavigate-in=false  nop-onnavigatesuccess-in=false
nop-onnavigateerror-in=false  nop-oncurrententrychange-in=false
nop-assigned onnavigate=function
nop-hash-assigned hash=#1
nop-checked hash=#1 entries=1 currentEntry=entry
```

Ни `nop-onnavigate-fired`, ни `nop-listener-fired` (обычный
`addEventListener('navigate', …)`) не напечатаны — то есть дело не только в
`on<type>`-свойстве: при `location.href = "#1"` событие не диспатчится
вообще никак. Контрольный вариант `--variant navigation-api` показывает
обратное для явного вызова: `na-navigate-event intercept=function`,
`na-intercepted`, `na-currententrychange from=undefined`.

## Цена по WPT

Шесть id остатка WPT-RUN-5, все шесть — из-за первой половины (событие не
приходит), причём пять из них дополнительно упираются во вторую
(`navigation.onnavigate = …`):

`navigation-api/navigate-event/intercept-after-dispatch.html`,
`…/intercept-handler-null-or-undefined.html`,
`…/intercept-handler-returns-non-promise.html`,
`…/intercept-resolve.html`,
`…/defer/tentative/defer-after-dispatch.html`,
`navigation-api/currententrychange-event/properties.html`.

Категория `navigation-api/` не вендорена целиком (это ~250 id), так что
цена по остатку — нижняя граница.

## Что дальше

HTML LS «navigate event firing algorithm» требует диспатчить `navigate` на
*каждой* навигации документа, включая same-document (фрагмент, `pushState`,
traversal), а не только на программной через `navigation.navigate()`. Точка
подключения — общий путь фрагментной навигации, тот же, в котором сидит
[BUG-833](BUG-833-OPEN.md) (клик по `<a href="#x">` идёт мимо
`_lumen_navigate_or_fragment`), поэтому чинить их разумно вместе.
