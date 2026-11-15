# BUG-1814 — данные `postMessage`/`MessagePort` доезжают строкой (`base64url`, `[object Object]`) вместо значения отправителя: `JSON.parse` в бандле reCAPTCHA падает

**Статус:** FIXED 2026-11-15 (P3, срез 5 [BUG-1647](BUG-1647-OPEN.md))
**Заведён:** 2026-11-15 (P3, срез 4 [BUG-1647](BUG-1647-OPEN.md), после
[BUG-1813](BUG-1813-FIXED.md))

## Исправлено (2026-11-15, срез 5 BUG-1647)

**Причина.** `build_worker_messages_json` встраивает полезную нагрузку как
JSON-значение (`{"id":N,"json":<value>}`), то есть на JS-стороне `m.json` —
уже разобранное значение. Получатели же разбирали его ещё раз:
`Worker.prototype._deliver` (`worker_shims.rs`), `_lumen_deliver_port_messages`
(там же) и `_deliver` клиентского порта `SharedWorker` (`shared_worker.rs`).
Для объекта/массива повторный `JSON.parse` бросал и срабатывал `catch`
(значение выживало), но строка, которая сама является JSON-текстом
(`'{"a":1}'`, а протокол reCAPTCHA гоняет именно JSON-текст), превращалась
в объект — тип терялся, `typeof data === 'string'` на приёме давал false.

**Починено.** Все три получателя берут `m.json` как есть; `JSON.parse`
остался только там, где Rust действительно передаёт текст (воркерный
`_lumen_worker_deliver_port_message`, вход `_lumen_worker_dispatch_message`).
Фикс не трогает `build_worker_messages_json` — на «сырое» встраивание
опираются `BroadcastChannel` и `_deliverError` (конверт ошибок).

**Тесты** (`cargo test -p lumen-js --features v8-backend --lib --
v8_webworker::worker_message`): `worker_message_json_text_string_stays_a_string`
и `worker_message_port_json_text_string_stays_a_string` — без фикса падают
(`Object([("n", Number(1.0))])` вместо `String("{\"n\":1}")`).
**Область:** js — транспорт сообщений между изолятами и в воркер:
`crates/js/src/frame_bridge.rs` (`w.postMessage`, `_lumen_f_post_message`,
`_lumen_deliver_frame_message`), `crates/js/src/shim/message_channel_shim.js`
(`_lumen_port_walk_serialize`/`_lumen_port_walk_deserialize`,
`_lumen_deliver_port_messages`), воркерный `_deliver` (`Worker._deliver`,
`crates/js/src/worker.rs` и его JS)

## Симптом

После фикса [BUG-1813](BUG-1813-FIXED.md) (`contentWindow.JSON` начального
`about:blank`-кадра) ошибка `TypeError … (reading 'parse'|'stringify')`
в anchor-кадре ушла, но `grecaptcha.execute` по-прежнему не отдаёт валидный
токен: стенд `python scripts/captcha_stand.py --arm lumen --only
recaptcha-v3-score` — `не_отрисован`, в stderr новый класс ошибок:

```
[unhandled-rejection] SyntaxError: Unexpected token 'B', "Beq-BFi86J"... is not valid JSON
    at JSON.parse (<anonymous>)
    at <anonymous>:113:132          <- G.parse(L), где G — «pristine» JSON кадра
    at <anonymous>:451:477
    at Array.<anonymous> (<anonymous>:802:396)
    at kz.l (<anonymous>:113:105)
    at Array.<anonymous> (<anonymous>:164:317)
```

## Что измерено (`.tmp/pl1814.py`, preload оборачивает `JSON.parse` в каждом
документе и пересылает лог верхнему окну)

Стенд тот же: `https://recaptcha-demo.appspot.com/recaptcha-v3-request-scores.php`,
`--maximized`, `LUMEN_NO_ADBLOCK=1`, бинарь ветки BUG-1813. Все падения
`JSON.parse` за прогон:

| len | голова строки | стек |
| --- | --- | --- |
| 3611 | `BqQAXMyOGwEimQVymhUclNF7eikDlVVf85XoxO3Xjkj7FTXeq5BIQYzGO20HeSW2ApUAyM1f1zxbTdFy…` | `G.parse(L)` (113:132) ← 451:477 |
| 226 | `Beau9BYVb3icZ4yphY_mGRNdhqmRG-GFocaveFOIs3icZ4yphY_mGRNdhqndT_SpjceyBCItptmR4pDh…` | то же |
| 156 | `BONwUIqcLUQohdVPyAbVKWNxiTgd8SnW0b9w6IqcLIWUuRSCscstNN68ZO2UuXyCsGNwKcuQJYi5teWy…` | то же |
| 472 | `Beau9Fno6ZidBZSt2aqvIFmEyLSkUZCprZeGGBWI5KTBLIG12SavIQTlsbmAaLiNaILOcUjRsMSdZQm0…` | `G.parse(L)` ← 451:307 |
| 15 | `[object Object]` | `Worker._deliver` (шим, `:240:22`) ← `_lumen_deliver_worker_messages` |

Строки — валидный base64url (ASCII `BqQA…`, `Beau9…`), то есть до `JSON.parse`
доходит **сериализованное представление** значения, а не его JSON-текст.
Пятая строка — отдельный, более явный случай того же класса: `Worker._deliver`
зовёт `JSON.parse` для полезной нагрузки-объекта, и `String(value)` даёт
`"[object Object]"`.

`kz.l` (`recaptcha__en.js:113:105`) — это внутренний декодер бандла; его
`G.parse(L)` ждёт JSON-текст, который в Chrome приходит от якоря/воркера как
есть (структурированное клонирование сохраняет тип значения).

## Рабочая гипотеза

Транспорт Lumen превращает значение в строку там, где Chrome сохраняет
структуру: бинарные данные (`Uint8Array`/`ArrayBuffer`) — в base64url, объекты —
через `String(...)` (`[object Object]`). Три места-кандидата:

1. `_lumen_port_walk_serialize`/`_lumen_port_walk_deserialize`
   (`shim/message_channel_shim.js`) — двоичные значения на пути MessagePort;
2. `w.postMessage` фасада окна (`frame_bridge.rs`) — `JSON.stringify(payload)`
   до упаковки в конверт (типизированный массив не клонируется, а
   сериализуется);
3. воркерный путь — `Worker._deliver` разбирает `JSON.parse(data)` без проверки,
   что пришло строкой.

## Что сделать

1. Прогнать ту же пробу (`JSON.parse`-логгер) в Chrome на том же URL и снять
   тип/голову полезных нагрузок, чтобы точно назвать значение, которое Lumen
   портит (`Uint8Array`? `ArrayBuffer`? объект?).
2. Починить сериализацию: либо сохранять тип (структурированный клон: типизированные
   массивы и `ArrayBuffer` — как есть через base64-границу с восстановлением
   типа), либо, если значение уже строка, не трогать её вовсе.
3. `Worker._deliver`: не звать `JSON.parse` для не-строковой нагрузки.
4. Тест в `crates/js` (типизированный массив и объект через `postMessage`
   кадра и через порт — на приёме тот же тип и то же содержимое).
5. Проверка: `.tmp/pl1814.py` — падений `JSON.parse` нет; затем
   `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score`.
