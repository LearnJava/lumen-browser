# BUG-1751 — `VTTCue`: собственные data-свойства вместо IDL-аксессоров, нет валидации, нет глобала `TextTrack`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-VTT-SCOUT)
**Область:** js (`crates/js/src/shim/video_element.js:286` `function VTTCue`, `globalThis.VTTCue` стр. 323)

## Симптом

```js
const c = new VTTCue(0, 1, 'x');
Object.getOwnPropertyDescriptor(VTTCue.prototype, 'position'); // undefined — поле собственное
c.position = 150;          // не бросает IndexSizeError
c.align = 'bogus';         // присваивается, хотя по WebIDL enum невалидное значение игнорируется
new VTTCue(NaN, 1, 'x');   // не бросает TypeError
typeof TextTrack;          // "undefined"
```

`VTTCue` в шиме — функция, кладущая все атрибуты собственными data-свойствами экземпляра. Нет геттеров/сеттеров на прототипе, нет `[EnforceRange]`/`unrestricted double`-проверок, нет `IndexSizeError` для `position`/`size` вне 0…100, `positionAlign`/`lineAlign`/`vertical` принимают любые строки; глобала `TextTrack` (и `TextTrackCue` как базового класса) нет.

## Как найдено

WPT-VTT-SCOUT, кластер H6 (`docs/wpt-vendor-notes/webvtt.md`): `api/VTTCue/{constructor,constructor-exceptions,position,positionAlign,size}.html`, `api/historical.html` — 6 id, 7/17 сабтестов, потолок ≈4.5 балла (1.4 п.п. `webvtt`). Смежно с [BUG-902](BUG-902-OPEN.md) (`getCueAsHTML`, `VTTRegion`) — тот же класс, но другой механизм.

## Что делать

1. `TextTrackCue` (базовый, `EventTarget`, конструктор — `TypeError`) и `VTTCue extends TextTrackCue` с атрибутами-аксессорами на прототипе по WebVTT §6.1 / HTML §4.8.11.11; внутреннее состояние — в скрытых слотах.
2. Сеттеры: `position`/`size` — `IndexSizeError` вне диапазона; enum-атрибуты (`vertical`, `align`, `lineAlign`, `positionAlign`) игнорируют невалидные значения; `startTime`/`endTime` в конструкторе и сеттерах — `TypeError` на NaN/Infinity.
3. Глобалы `TextTrack`, `TextTrackList`, `TextTrackCueList` как интерфейсные объекты (нужны и для `historical.html`, и для `idlharness`).

## Как проверить

`run_corpus.py --prefixes webvtt/api --processes 6`: `constructor-exceptions`, `position`, `size` — все сабтесты PASS.
