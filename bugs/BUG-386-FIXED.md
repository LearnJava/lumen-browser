# BUG-386 — `navigator.permissions.query()` отвечает `granted` на любое имя вне списка из 11 запрещённых, включая нереализованные и выдуманные; `PermissionStatus` — не `EventTarget`, поля записываемы

**Статус:** FIXED 2026-08-10
**Компонент:** js (`crates/js/src/permissions.rs` — новый модуль; до фикса —
`crates/js/src/dom.rs`, `WEB_API_SHIM`, секция «Permissions API (W3C
Permissions §5)»: `_perm_denied` + `navigator.permissions`)
**Найден:** P2, WPT-VENDOR-font-access (2026-07-28), проба `--dump-layout`
(`.tmp/fa-probe3.html`)
**Исправлен:** P3, 2026-08-10

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
  ([BUG-385](BUG-385-FIXED.md), Phase 1), оно заработает молча и без спроса. То
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
  [BUG-366](BUG-366-FIXED.md).
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

## Как починено (P3, 2026-08-10)

Реализация переехала из `WEB_API_SHIM` в свой модуль
[`crates/js/src/permissions.rs`](../crates/js/src/permissions.rs) — рядом с
`local_font_access.rs`, по тому же образцу (шим + свои юнит-тесты).

**Пункт 1 — реестр имён.** 34 имени реестра W3C; всё остальное отклоняется
`TypeError`-ом с текстом WebIDL про `enumeration PermissionName`. Ошибки
конверсии дескриптора (не объект, нет `name`) — тоже отклонение, а не
синхронный throw: `query` объявлен возвращающим промис. Лишние члены
дескриптора (`sysex`, `userVisibleOnly`, `panTiltZoom`) игнорируются, как
положено словарю.

**Пункт 2 — состояние описывает то, что действительно происходит.** У каждого
распознаваемого имени состояние проставлено явно, ветки «по умолчанию» нет —
новое имя нельзя добавить, не классифицировав его (гейт
`every_recognised_name_has_a_state`). Правило: `granted` только там, где вызов
API сегодня даёт заявленный наблюдаемый эффект, иначе `denied`.

| Состояние | Имена | Почему |
|---|---|---|
| `granted` | `clipboard-read`, `clipboard-write` | `readText()`/`writeText()` ходят в буфер ОС через `_lumen_clipboard_read`/`_lumen_clipboard_write` |
| `granted` | `storage-access`, `top-level-storage-access` | хранилище не партиционировано по top-level site, так что непартиционированный доступ у страницы уже есть |
| `granted` | `idle-detection` | `IdleDetector.start()` реально опрашивает `__lumen_idle_get_idle_ms` и диспатчит `change`; `IdleDetector.requestPermission()` и так отвечает `granted` — два ответа обязаны совпадать |
| живой опрос | `notifications` | читается с `Notification.permission` при каждом запросе: это собственный ответ движка на тот же вопрос, и копия в таблице разъехалась бы с оригиналом ('default' спеки → 'prompt') |
| `denied` | остальные 28 | заглушка, резолвящаяся ничего не сделав (`wakeLock.request`, `storage.persist`, `requestPointerLock`, background-*), API, который всегда падает (`geolocation` зовёт error-колбэк с `PERMISSION_DENIED` — `FakeCoords` в воркспейсе никто не конструирует), или которого нет вовсе |

`local-fonts` → `denied` — это то, что держит гейт `queryLocalFonts()`
закрытым: перечисление шрифтов ОС не включится молча в тот день, когда
появятся нативы Phase 1 ([BUG-385](BUG-385-FIXED.md)).

**Пункт 3 — форма.** `PermissionStatus` наследует шимовый `EventTarget`
(`Reflect.construct`, чтобы тело базы отработало в обход собственного
конструктора-заглушки), так что `addEventListener('change', …)` ведёт в реально
работающий диспатч. `name`/`state` — readonly-геттеры прототипа поверх
приватного `WeakMap`; `state` пересчитывается на каждом чтении, поэтому не
может устареть, а `status.state = 'granted'` больше не «повышает» уже
полученный ответ. `onchange` — аксессор обработчика, есть `Symbol.toStringTag`,
ни `Permissions`, ни `PermissionStatus` со страницы не конструируются.

Чтобы механизм `change` не остался украшением, внутренний
`_lumen_permission_state_changed(name)` вызывается из
`Notification.requestPermission()` — и только при реальном сдвиге значения:
событие о неизменившемся состоянии врало бы о движке. Имя `_lumen_*` прячет и
замораживает пасс BUG-378, так что со страницы его не позвать.

**Пункт 4 — не сделан, и не здесь.** `navigator.permissions` как аксессор
`Navigator.prototype` требует интерфейса `Navigator`, которого в движке нет
вовсе: `navigator` — объектный литерал в `WEB_API_SHIM`, все ~48 членов —
собственные данные. Это [BUG-624](BUG-624-OPEN.md), одна правка на весь объект.
Взят прецедент `navigator.credentials` ([BUG-366](BUG-366-FIXED.md)):
незаписываемое собственное свойство, так что подменить контейнер целиком и
отвечать за движок третья сторона уже не может.

**Проверка.** 33 теста: 25 в `permissions.rs` (реестр, состояния, форма,
диспатч `change`), 4 в `dom.rs` на реальном пути установки — модуль тестируется
против стаба `EventTarget`, потому что в чистом V8 его нет, и эти четыре
закрывают разрыв, — 2 в `notifications_bindings.rs` на проводку уведомления и 2
обновлённых. Плюс проба `--dump-layout` на живой странице
(`.tmp/probe-386.html`): выдуманное имя отклоняется `TypeError`-ом,
`local-fonts`/`geolocation`/`notifications`/`persistent-storage` дают `denied`,
дескриптор `navigator.permissions` — `{w:false,e:true,c:true}`, присваивание не
проходит, `Object.prototype.toString.call(status)` = `[object
PermissionStatus]`, `status instanceof EventTarget` = `true`.

## Связанные

* [BUG-385](BUG-385-FIXED.md) — Local Font Access; закрыт 2026-08-10 по форме
  (`queryLocalFonts()`, WebIDL, гейты §2 написаны и fail-closed), но само
  перечисление шрифтов ОС намеренно не включено, пока этот баг открыт: гейт
  спрашивает `local-fonts` и получает `granted` без спроса, так что Phase 1 над
  ним заработает молча.
* [BUG-366](BUG-366-FIXED.md) — `navigator.credentials`: тот же класс дефекта
  (методы на экземпляре вместо прототипа, контейнер перезаписываем).
* [BUG-361](BUG-361-FIXED.md) — соседний слой той же темы: `permissionsPolicy`
  не сообщает список поддерживаемых фич.
* [BUG-379](BUG-379-FIXED.md) — собственные свойства глобала как отпечаток.
