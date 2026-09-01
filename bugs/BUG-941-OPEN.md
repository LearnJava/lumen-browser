# BUG-941 — у канвы нет флага origin-clean: cross-origin картинка рисуется и читается обратно, `getImageData`/`toDataURL` не бросают `SecurityError` никогда

**Статус:** OPEN (ДОРАБОТКА → [GAP-CANVASORIGIN](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — модели origin-clean в движке нет вовсе (`grep` по `crates/js` за `origin.clean`/`SecurityError` в `canvas2d.rs`/`offscreen_canvas.rs` даёт ноль), и она требует не одной правки, а сквозного состояния: режим запроса по атрибуту `crossorigin`, результат CORS-проверки ответа, распространение флага через `drawImage`/`createPattern`/`ImageBitmap`/`transferToImageBitmap` и проверка на трёх читающих членах. Ведётся как задача `GAP-CANVASORIGIN` в [ROADMAP.md](../ROADMAP.md); P3 как баг не берёт.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — живой замер, вариант `canvas-taint-crossorigin`)
**Область:** js (`crates/js/src/canvas2d.rs` — `getImageData`/`toDataURL`/`drawImage`; `crates/js/src/offscreen_canvas.rs`), network (режим запроса и CORS-ответ до канвы не доходят — ср. [BUG-859](BUG-859-OPEN.md): исходящий запрос не несёт даже `Origin`)
**Владелец:** дорожка `GAP-CANVASORIGIN`.

## Симптом

Канва, в которую нарисовали картинку с ЧУЖОГО источника, читается обратно
полностью: `getImageData` возвращает пиксели, `toDataURL` — data-URL. По HTML
LS §4.12.5.1.2 такая канва перестаёт быть origin-clean и оба члена обязаны
бросить `SecurityError`.

Проверки нет ни в какой форме: атрибут `crossorigin` отражается
(`corsed.crossOrigin === "anonymous"`), но ни на что не влияет — CORS-ответа
никто не спрашивает, и результат одинаков с ним и без него.

## Прямое измерение

`tests/wpt/verify_replaced_content_gaps.py --variant canvas-taint-crossorigin`
(2026-09-01, dev-release, Linux, `main` = `287562e61`). Второй origin — второй
порт собственного сервера пробы; алиасы `www1.`, которыми это делает сам
прогон WPT, на машине не разрешаются (`WPT-RUN-10`), и мерить надо было бы
их, а не предмет:

```
crossOrigin-reflects = anonymous
same-draw = ok    same-read = 0,127,1,255      ← своя картинка, читается
cross-draw = ok   cross-read = 0,0,1,255       ← ЧУЖАЯ картинка, читается
cross-toDataURL = data:image/png;base64,       ← и сериализуется
[server saw: GET /media/1x1-green.png?taint=same,
             GET [alt]/images/black-rectangle.png?taint=cors,
             GET [alt]/images/black-rectangle.png?taint=cross]
```

Картинки написаны парсером намеренно: скриптовая в стор битмапов канвы не
попадает вовсе ([BUG-938](BUG-938-OPEN.md)), рисует пусто, и через неё вопрос
о загрязнении задать нельзя — первый замер этого среза так и вышел
бессодержательным.

## Кого это держит

`svg/embedded/image-crossorigin.sub.html` — 4 сабтеста, две пары «можно
прочитать» / «нельзя прочитать», то есть файл проверяет обе стороны флага;
`html/canvas/element/manual/drawing-images-to-the-canvas/drawimage_svg_image_with_foreign_object_does_not_taint.html`
— отрицательная сторона (SVG с `<foreignObject>` НЕ должен загрязнять).
Оба из остатка WPT-RUN-5. Больший счёт — в невендоренных категориях
`html/canvas/*`, где это одно из основных правил.

## Почему это заявка на безопасность, а не на совместимость

Origin-clean — единственное, что мешает странице прочитать пиксели чужого
документа через канву (классический пример — приватная картинка, отданная по
cookie пользователя). Сегодня такая проверка отсутствует, поэтому в этой
части движок разрешает больше, чем любой браузер, а не просто отвечает не то.
