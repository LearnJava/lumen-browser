# BUG-1643 — `contentWindow` соседнего iframe из дочернего фрейма через `parent.document` — `undefined`

**Статус:** OPEN
**Заведён:** 2026-10-19 (живой прогон https://otpravka.pochta.ru/specification)
**Область:** js (фреймы: `crates/js/src/frame_bridge.rs`, доступ к DOM родителя из дочернего realm-а)

## Симптом

Страница содержит два скрытых iframe OIDC session management: `#op-iframe`
(`https://passport.pochta.ru/pc/ext/v1.0/session/state`) и `#rp-iframe`
(`https://otpravka.pochta.ru/api/auth/session-check`, тот же origin). Скрипт `rp-iframe` раз в секунду:

```js
var iframe = window.parent.document.getElementById("op-iframe");
iframe.contentWindow.postMessage(message, targetOP);
```

У нас каждый тик: `[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'postMessage')`
(`at checkStatus`), 60 раз за ~минуту. Из верхнего документа
`document.getElementById('op-iframe').contentWindow` — объект; из дочернего фрейма через
`window.parent.document` — `undefined`.

Последствие: проверка смены/окончания сессии на сайте не работает (выход/перелогин в другой
вкладке не замечается); страница спецификации для гостя не ломается.

## Что сделать

Элемент `<iframe>` родителя, полученный из дочернего realm-а через `parent.document`, должен
отдавать `contentWindow` (WindowProxy соседнего фрейма) так же, как из верхнего документа.
Связано с [BUG-957](BUG-957-OPEN.md) (голый фасад окна фрейма) и [BUG-480](BUG-480-OPEN.md), но
дефект другой: здесь свойства нет вовсе. Тест: два same-origin iframe, второй читает
`parent.document.getElementById('a').contentWindow` и делает `postMessage`.
