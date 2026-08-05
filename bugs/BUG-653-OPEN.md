# BUG-653: `HTMLVideoElement.disablePictureInPicture` не отражает content-атрибут после патчинга

**Статус:** OPEN
**Компонент:** js (`crates/js/src/video_pip.rs:74-91` — `patchVideoPip`)
**Найден:** P2, WPT-VENDOR-picture-in-picture, 2026-08-05

## Симптом

Категория `picture-in-picture` (скоуп 🚫, нет медиа-конвейера видео) —
вендорена и прогнана целиком (`run_report.py --all --root
picture-in-picture --recursive`, ~51 с, 17 id): **14/17 harness OK, 0/36
сабтестов**. Большинство FAIL — `ReferenceError: getVideoURI is not
defined` (`/common/media.js` не вендорен, ожидаемый гэп для этой
категории, не новая находка).

Один FAIL не связан с медиа-гэпом и указывает на реальный дефект в уже
реализованном PiP-шиме: `disable-picture-in-picture.html`, тест "Test
disablePictureInPicture IDL attribute" — `assert_true: expected true got
false` на шаге

```js
video.setAttribute('disablepictureinpicture', 'foo');
assert_true(video.disablePictureInPicture);
```

Живая проба (`--mcp-live-port`) подтверждает механизм напрямую:

```js
var video = document.createElement('video');
video.disablePictureInPicture               // false (верно, дефолт)
video.setAttribute('disablepictureinpicture', 'foo');
video.hasAttribute('disablepictureinpicture') // true — атрибут выставлен
video.disablePictureInPicture               // false — геттер лжёт
```

## Причина

`patchVideoPip` (`crates/js/src/video_pip.rs:74-91`) кеширует состояние
атрибута один раз в момент патчинга элемента:

```js
var _disabled = el.hasAttribute ? el.hasAttribute('disablepictureinpicture') : false;

Object.defineProperty(el, 'disablePictureInPicture', {
  get: function() { return _disabled; },
  set: function(v) {
    _disabled = !!v;
    if (_disabled && el.hasAttribute) {
      el.setAttribute('disablepictureinpicture', '');
    } else if (el.removeAttribute) {
      el.removeAttribute('disablepictureinpicture');
    }
  },
  configurable: true,
});
```

Геттер читает переменную-замыкание `_disabled`, а не сам content-атрибут.
Синхронизация односторонняя: IDL-сеттер (`video.disablePictureInPicture =
true`) корректно проставляет content-атрибут, но обратного пути нет —
прямой `setAttribute`/`removeAttribute('disablepictureinpicture', ...)`
(а также патчинг элемента, у которого атрибут выставлен HTML-парсером
до вызова `patchVideoPip`, если patch произошёл не в момент создания)
не обновляет `_disabled`. Спека (`reflect`, boolean content attribute)
требует, чтобы IDL-атрибут был живым отражением *присутствия*
content-атрибута, а не снапшотом на момент установки геттера/сеттера.

## Как воспроизвести

```js
var v = document.createElement('video');
v.setAttribute('disablepictureinpicture', 'anything');
v.disablePictureInPicture // ожидается true, получаем false
```

## Предлагаемое исправление

Убрать переменную-кеш `_disabled`; геттер должен читать `el.hasAttribute
? el.hasAttribute('disablepictureinpicture') : false` напрямую при каждом
обращении (как и делает сеттер для записи) — тот же паттерн, что и у
остальных boolean-reflected IDL-атрибутов в кодовой базе. Также стоит
свериться с `requestPictureInPicture()`'s внутренним чтением этого же
значения (строка 94, `if (_disabled) { ... }`) — она использует ту же
устаревшую переменную и должна быть переведена на прямое чтение атрибута.
