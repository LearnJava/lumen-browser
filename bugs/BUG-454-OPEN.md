# BUG-454 — Анти-fingerprint у canvas закрывает только `toDataURL`/`toBlob`: `getImageData` отдаёт настоящие пиксели, а `toDataURL` игнорирует запрошенный тип и размер

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — фабрика элемента, `toDataURL`;
`crates/js/src/webgl_canvas.rs:492-500` — резервная заглушка `_addCanvasStubs`), privacy
**Найден:** 2026-07-29 (P2), WPT-VENDOR-html-canvas, проба `--dump-layout`
**Предыстория:** заведён вместо BUG-419 прежней сессии. Та описывала асимметрию четырёх
путей создания элемента (`createElement` защищён, `createElementNS`/`cloneNode`/разметка —
нет); эта половина **починена** попутно с BUG-348, перепроверено 2026-07-29. Осталась и
подтверждена вторая половина.

## Симптом 1: защищён один канал чтения пикселей из двух

`CAPABILITIES.md` (строка Graphics) заявляет «toDataURL blank (anti-fingerprint)», ADR-007
описывает это как меру против pixel-hash-снятия отпечатка. Проба (холст 60×30, залит
`#123456`):

```
P1 parsed 2d=true url=data:image/png;base64, len=118
P1 made   2d=true url=data:image/png;base64, len=118
P1 madeNS 2d=true url=data:image/png;base64, len=118
P1 cloned 2d=true url=data:image/png;base64, len=118
P1 getImageData=18,52,86,255
```

`toDataURL` одинаково заглушён на всех четырёх путях создания (это и есть починенная
часть), а `getImageData` не заглушён нигде и отдаёт точные пиксели `18,52,86,255`
(= `#123456`). Канонические скрипты снятия отпечатка (fingerprintjs и производные)
читают **оба** канала и при недоступности первого штатно переключаются на второй, где
хешируют массив `ImageData.data` напрямую. То есть заявленная мера сейчас не
закрывает свой класс атаки, а лишь удорожает его на одну строку кода:

```js
const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;   // полный хеш холста
```

Побочно: `WebGLRenderingContext.readPixels` и `OffscreenCanvas.convertToBlob` в этой же
логике не рассмотрены вовсе.

## Симптом 2: заглушка не является валидным ответом спеки

```
C5 painted = data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==
C5 empty   = <та же строка>
C5 identical=true      toDataURL('image/jpeg') → data:image/png;…
```

Возвращается один и тот же PNG **1×1**, независимо от:

- содержимого холста (это и есть цель меры — возражений нет);
- **размера холста** — спека требует изображение ровно `canvas.width × canvas.height`;
- **запрошенного типа** — `toDataURL('image/jpeg')` обязан вернуть `data:image/jpeg;…`
  либо, если тип не поддержан, PNG, но тогда и тип в строке должен быть `image/png`
  честно, а не как молчаливая подмена запрошенного JPEG.

Из-за размера 1×1 ломаются не только тесты отпечатка, но и обычные сценарии
«снять картинку и вставить обратно»: `img.src = canvas.toDataURL()` даёт 1×1.

## Данные WPT

Срез `html/canvas/element` (числа — в строке `WPT-VENDOR-html-canvas` ROADMAP.md).
Прицельно: `element/canvas-host/2d.canvas.host.todataurl.*` (тип и размер),
`element/pixel-manipulation/*` (там же живёт [BUG-448](BUG-448-OPEN.md) — `getImageData`
игнорирует прямоугольник).

## Направление починки — решение продуктовое, не техническое

Три взаимоисключающих варианта, выбор за владельцем ADR-007:

1. **Довести меру до класса атаки.** Заглушить и `getImageData` (шум на младшие биты,
   как в Brave: детерминированный по сессии+origin), тогда `toDataURL` можно вернуть
   честный — отпечаток всё равно не снимется, а спека соблюдена.
2. **Сделать меру переключаемой** (щит уровня страницы), по умолчанию — честное
   поведение; сейчас она включена всегда и молча.
3. **Оставить как есть, но починить форму:** отдавать заглушку правильного размера и
   запрошенного типа. Класс атаки при этом остаётся открытым (симптом 1).

Вариант 1 — единственный, при котором заявление в `CAPABILITIES.md` становится
правдой. До решения строку `CAPABILITIES.md` про anti-fingerprint держать в 🟡.
