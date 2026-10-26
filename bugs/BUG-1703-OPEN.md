# BUG-1703 — Событие `navigate` приходит не синхронно внутри `navigation.navigate()`/`back()`/`traverseTo()`, а на следующем ходе оболочки: порядок «navigate → микрозадача» нарушен, `intercept()`-обработчик не успевает

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-NAV-SCOUT)
**Область:** js/shell (`crates/js/src/navigation_api.rs` — `Navigation._request` кладёт запрос в очередь через `_lumen_navigation_request`; `crates/shell/src/app/about_to_wait.rs` — разбор `take_nav_updates`; `crates/shell/src/lumen/navigation.rs` — `navigate_to_inner` диспатчит `_lumen_dispatch_navigate` уже после возврата из скрипта)

## Симптом

HTML LS §7.2.9.4 («navigate event firing»): `navigation.navigate(url)` запускает «inner navigate event firing algorithm» синхронно, до возврата из вызова — слушатель `navigate` уже отработал, `e.intercept({handler})` уже вызван, `navigation.transition` уже установлен. В Lumen `navigate()` лишь кладёт запрос в очередь (`_lumen_navigation_request`), событие диспатчит оболочка на следующем проходе цикла. Наблюдаемое:

* `assert_array_equals: expected property 0 to be "navigate" but got "promise microtask"` (21 id, `ordering-and-transition/*`) — микрозадача, поставленная сразу после `navigation.navigate(...)`, обгоняет `navigate`;
* `TypeError: intercept_resolve is not a function` (4 id, `focus-reset/*`) — тест вызывает функцию, которую должен был присвоить `intercept`-обработчик во время синхронного диспатча;
* `assert_array_equals: lengths differ` (21 id, 14 из них пересекаются с предыдущим пунктом) — недостающие записи порядка событий.

## Как найдено

WPT-NAV-SCOUT, кластер D (`docs/wpt-vendor-notes/navigation-api.md`): 32 id (OK 26, ERROR 6), 0/32 сабтестов, потолок +32 балла (+0.05 п.п. корпуса); `ordering-and-transition` 28, `focus-reset` 4. Тот же механизм у `history.pushState()`/`replaceState()`/`history.back()` (`currententrychange-event/history-pushState.html`: `oncurrententrychange_called` ещё `false` сразу после `pushState`) — но там событие не приходит вообще, см. [BUG-881](BUG-881-OPEN.md).

## Что делать

Диспатчить `navigate` (и ставить `transition`, запоминать `intercept`-обработчик) синхронно из JS-метода `navigate/reload/back/forward/traverseTo`, а оболочке отдавать уже итог (отмена / перехват / коммит). Асинхронной остаётся только сама загрузка для cross-document.

## Как проверить

```js
navigation.addEventListener('navigate', e => log.push('navigate'));
navigation.navigate('#x'); Promise.resolve().then(() => log.push('microtask'));
```

— после `await` в `log` порядок `['navigate','microtask']`. `ordering-and-transition/*?currententrychange` — первое утверждение порядка проходит.
