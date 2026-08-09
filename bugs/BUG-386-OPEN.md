# BUG-386 — `navigator.permissions.query()` отвечает `granted` на любое имя вне списка из 11 запрещённых, включая нереализованные и выдуманные; `PermissionStatus` — не `EventTarget`, поля записываемы

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7865-7890` — `WEB_API_SHIM`, секция
«Permissions API (W3C Permissions §5)»: `_perm_denied` + `navigator.permissions`)
**Найден:** P2, WPT-VENDOR-font-access (2026-07-28), проба `--dump-layout`
(`.tmp/fa-probe3.html`)

## Симптом

```
query(local-fonts)                     -> state=granted
query(geolocation)                     -> state=granted
query(notifications)                   -> state=granted
query(persistent-storage)              -> state=granted
query(totally-made-up-permission-xyz)  -> state=granted
query(camera)                          -> state=denied
```

Выдуманное имя `totally-made-up-permission-xyz` не отвергается, а получает
`granted`. По спеке (W3C Permissions §5.2, шаг 2) дескриптор конвертируется в
WebIDL и **нераспознанное имя обязано отклонять промис `TypeError`-ом** — именно
на этом построено feature detection: страница спрашивает `query({name:'X'})`,
чтобы узнать, поддерживается ли `X` вообще. Lumen отвечает «поддерживается и
разрешено» на всё.

Форма объектов:

```
permissions descriptor      = {"e":true,"w":true,"c":true}   ← собственное
                                                               записываемое
                                                               свойство navigator
permissions own names       = query
typeof window.Permissions       = undefined
typeof window.PermissionStatus  = function
query(*) -> isEventTarget=false  stateOwnWritable=true/true  proto=PermissionStatus
```

То есть: интерфейсного объекта `Permissions` нет; `navigator.permissions` —
обычный объектный литерал (`Object.getPrototypeOf(…)` — `Object.prototype`,
`constructor === Object`), перезаписываемый страницей целиком; `PermissionStatus`
не наследует `EventTarget` (`addEventListener` отсутствует, `onchange` — обычное
поле, событие `change` не диспатчится никогда); `name`/`state` — собственные
записываемые перечисляемые данные вместо readonly-геттеров прототипа, так что
`status.state = 'granted'` молча «повышает» уже полученный ответ.

## Причина

Реализация — 25 строк в `WEB_API_SHIM` с явно зафиксированным допущением в
комментарии: «Lumen is a single-user desktop app. Sensors and AV hardware that do
not exist in headless mode are 'denied'; everything else is 'granted'». Список
`_perm_denied` содержит 11 имён (микрофон, камера, midi, speaker-selection,
четыре датчика, display-capture, screen-wake-lock, nfc); всё остальное —
`granted` без разбора, включая имена, для которых в движке нет ни строки
реализации (`geolocation`, `notifications`, `persistent-storage`,
`local-fonts`), и включая любую опечатку.

Допущение «одно приложение, один пользователь» объясняет, почему нет UI выдачи
разрешений, но не объясняет ответ `granted` для **нераспознанного** имени: это
не политика по умолчанию, а отсутствие валидации имени.

## Влияние

* **Приватность.** `local-fonts` — один из сильнейших векторов отпечатка
  (список установленных шрифтов почти уникален для машины). Сейчас разрешение
  на него уже выдано; как только появится реальное перечисление
  ([BUG-385](BUG-385-OPEN.md), Phase 1), оно заработает молча и без спроса. То
  же для `geolocation` и `notifications` — оба `granted` заранее. Для браузера,
  чья заявленная цель — приватность, это дефект по умолчанию, а не мелочь формы.
* **Feature detection ломается в обе стороны.** Страница, спрашивающая
  `query({name:'X'})`, не может отличить поддерживаемое от несуществующего:
  ответ всегда `granted`. Код вида
  `try { await navigator.permissions.query({name:'push'}) } catch { /* нет поддержки */ }`
  уходит по ветке «поддержка есть» и падает дальше, на самом API.
* **`navigator.permissions` перезаписываем** любым скриптом на странице —
  третья сторона может подменить его целиком, и всё, что спрашивает разрешения
  после неё, получит подделанные ответы. Тот же класс, что
  [BUG-366](BUG-366-OPEN.md).
* **Отсутствие `EventTarget`** означает, что штатный приём «подписаться на
  `permissionStatus.change` и отреагировать на отзыв разрешения» не работает —
  подписка не бросает ошибку, просто никогда не срабатывает
  (см. [[feedback_green_test_can_mask_broken_feature]] — молчаливо зелёный код).

## Как чинить

1. Ввести список **распознаваемых** имён (реестр Permissions API + расширения,
   которые Lumen действительно реализует) и отклонять всё остальное
   `TypeError`-ом, как требует спека. Это отделяет «не поддерживается» от
   «запрещено».
2. Для распознанных, но не реализованных имён отвечать `denied`, а не
   `granted` — по умолчанию закрыто. `local-fonts`, `geolocation`,
   `notifications`, `persistent-storage` попадают именно сюда.
3. `PermissionStatus` — наследник `EventTarget`, readonly-геттеры `name`/`state`
   на прототипе, `onchange` как аксессор обработчика, `Symbol.toStringTag`.
4. `navigator.permissions` — геттер на `Navigator.prototype`, отдающий синглтон
   типа `Permissions`; сам интерфейсный объект — на `window`.

Регрессия проверяется без WPT: страница, утверждающая, что
`navigator.permissions.query({name:'заведомо-нет-такого'})` отклоняется, а
`query({name:'local-fonts'})` даёт `denied`, пока реализации нет.

## Связанные

* [BUG-385](BUG-385-OPEN.md) — Local Font Access; вместе с этим багом
  определяет поведение по умолчанию при появлении перечисления шрифтов.
* [BUG-366](BUG-366-OPEN.md) — `navigator.credentials`: тот же класс дефекта
  (методы на экземпляре вместо прототипа, контейнер перезаписываем).
* [BUG-361](BUG-361-FIXED.md) — соседний слой той же темы: `permissionsPolicy`
  не сообщает список поддерживаемых фич.
* [BUG-379](BUG-379-OPEN.md) — собственные свойства глобала как отпечаток.
