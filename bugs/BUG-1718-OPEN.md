# BUG-1718 — `window.opener` попапа теряется после его навигации, а сам опенер — заглушка только с `postMessage`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P1, по ходу [VT-API-2](../ROADMAP.md))
**Область:** js/shell (`crates/js/src/shim/web_api_shim_tail_mc.js` — `window.opener`, `_lumen_install_opener`; `crates/shell/src/app/about_to_wait.rs` — пара опенер/попап)

## Симптом

1. `window.opener` попапа — объект только с `closed` и `postMessage` (`_lumen_install_opener`); обращение к любому полю окна-открывателя (`opener.did_reveal(e)`, `opener.events.push(...)`, `opener.document`) бросает или даёт `undefined`. WPT `css/css-view-transitions/navigation/pagereveal-with-view-transition.html` и все `pageswap-*` пишут в `window.opener.*` и остаются красными, хотя события приходят (проверено пробой через `BroadcastChannel`).
2. После того как попап сам навигируется (`location.replace('?x')`, `location.href = …`) в новом документе `window.opener === null` — пара опенер/попап ставится только на первой загрузке попапа. Спецификация (HTML §7.3.1.1) сохраняет opener за browsing context.

## Как проверить

Страница открывает `window.open('?child')`, ребёнок сразу шлёт `BroadcastChannel`-сообщение `opener=<window.opener !== null>`, затем через два rAF делает `location.replace('?second')` и шлёт то же из второго документа: первое сообщение `true`, второе `false`.

## Заметки

Нужен настоящий кросс-окно `WindowProxy` (общий доступ к свойствам открывателя той же origin), а не `postMessage`-заглушка. Часть пункта 2 решается проще: перевооружить пару опенер/попап для каждой загрузки в той же вкладке.
