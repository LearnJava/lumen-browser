# BUG-1643 — `contentWindow` соседнего iframe из дочернего фрейма через `parent.document` — `undefined`

**Статус:** FIXED 2026-10-28 (P3)
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

## Причина и исправление

Фасад элемента под-документа (`frameElem`, `frame_bridge.rs`) не имел `contentWindow`/
`contentDocument` вовсе, а реестр ребёнка знает только своих детей, `parent` и `top` — iframe-брат
в нём не зарегистрирован. Теперь для `<iframe>`, прочитанного через фасад предка
(`PARENT_BID`/`TOP_BID`), натив `_lumen_f_sibling_binding` лениво копирует биндинг брата из реестра
предка (реестры рантаймов лежат в глобальной карте по ключу `Arc::as_ptr(&doc)`, `register_frame_registry`)
в `FrameDocSlots::siblings`; bid брата = `SIBLING_BASE + i`, а сам ребёнок даёт `SELF_BID` — шим
подставляет настоящий `window`. Доступность пары = доступность и ребёнка, и брата к предку; у
cross-origin брата (как `passport.pochta.ru` в репро) `contentWindow.postMessage` работает,
`contentDocument`/`.document` — `null`. `postMessage` брату идёт через `SourceKind::Sibling`:
получатель находит отправителя в реестре общего предка, `event.source` — фасад брата.
Замки: реестр предка берётся только после своего и только «вверх» по цепочке, циклов ожидания нет.

Тесты `child_reaches_sibling_content_window_through_parent_document`,
`cross_origin_sibling_window_exists_but_hides_document`,
`post_message_between_siblings_reaches_the_sibling_with_a_source`.

Остаток: `contentWindow` iframe, вложенного в чужой дочерний документ (дед читает внука через
`child.document`), по-прежнему `undefined` — реестр внука лежит у среднего фрейма.
