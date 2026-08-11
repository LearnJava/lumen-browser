# BUG-762 — `PositionOptions` игнорируются целиком: `timeout`/`maximumAge` не приводят к ошибке `TIMEOUT`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/geolocation.rs` — `GEO_SHIM`,
`getCurrentPosition`/`watchPosition`)
**Найден:** P3, при закрытии [BUG-395](BUG-395-FIXED.md) (2026-08-11),
чтением исходника шима + вендоренных тестов категории

## Симптом

Третий аргумент `getCurrentPosition`/`watchPosition` — словарь
`PositionOptions { boolean enableHighAccuracy = false; unsigned long
timeout = 0xFFFFFFFF; unsigned long maximumAge = 0; }` (W3C Geolocation
API §6). Шим объявляет методы как `function(success, error)` и после
фикса BUG-395 проверяет тип третьего аргумента, но **никогда не читает
его члены**: строк `timeout`/`maximumAge`/`enableHighAccuracy` в
`crates/js/src/geolocation.rs` нет вовсе.

Следствия:

* `getCurrentPosition(succ, err, { timeout: 0 })` обязан отдать ошибку с
  `code === 3` (`TIMEOUT`) — так это формулирует вендоренный
  `PositionOptions.https.html`, «Set timeout and maximumAge to 0, check
  that timeout error raised»; шим отдаёт `PERMISSION_DENIED`
  (code 1) либо, при заданных `FakeCoords`, успешную позицию;
* отрицательные `timeout`/`maximumAge` должны клампиться к 0 (WebIDL
  `unsigned long`) — клампить нечего, значения не читаются;
* `GeolocationPositionError.TIMEOUT` (константа 3) объявлена, но ни один
  путь шима её не конструирует — единственный производимый код это
  `PERMISSION_DENIED`;
* `enableHighAccuracy` не влияет ни на что (это само по себе допустимо —
  спека разрешает подсказку игнорировать).

## Причина

Тот же исходный стаб Phase 0/1 («Geolocation API stub», doc-comment
`geolocation.rs:1`), что и у BUG-395: реализован только
happy-path/denied-path вызов колбэка, ни таймеров, ни кэша позиции, ни
разбора словаря опций заложено не было. BUG-395 закрыл валидацию
*типов* аргументов; *содержимое* словаря по-прежнему не участвует ни в
одном решении.

## Как чинить

Читать члены словаря по WebIDL (`ToBoolean` для `enableHighAccuracy`,
`unsigned long`-конверсия с клампом к `[0, 0xFFFFFFFF]` для `timeout` и
`maximumAge`) и завести ветку `TIMEOUT`: при `timeout === 0` —
немедленный (отложенный на такт, не синхронный) вызов error-колбэка с
`new GeolocationPositionError(3, …)`; при ненулевом `timeout` — таймер
на `setTimeout`, снимаемый при доставке позиции и при `clearWatch`.
`maximumAge` требует кэша последней позиции с меткой времени — сейчас
кэша нет вовсе.

Тесты: вендоренные `PositionOptions.https.html` (6 сабтестов, из них 4
про `TIMEOUT`) и `watchposition-timeout.https.window.js`. Оба сейчас
прогоном недостижимы — первый гейчен `testdriver.js`
(`test_driver.set_permission`), оба на `.https.`, — поэтому регрессию
закрывать юнит-тестами модуля + живой пробой `--mcp-live-port`, как
сделано для BUG-395.

## Связанные

* [BUG-395](BUG-395-FIXED.md) — WebIDL-валидация аргументов тех же двух
  методов (закрыт 2026-08-11); тип третьего аргумента проверяется, его
  содержимое — нет.
* Ветка `TIMEOUT` наблюдаема только там, где вообще выдаётся позиция:
  по умолчанию (`FakeCoords = None`) шим отвечает `PERMISSION_DENIED`
  раньше любых опций — порядок шагов спеки при фиксе надо сверять, а не
  дописывать проверку опций «сверху».
