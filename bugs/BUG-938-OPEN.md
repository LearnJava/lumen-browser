# BUG-938 — `drawImage(<img>)` рисует пусто (или прошлую картинку), а `createImageBitmap(<img>)` отклоняется «image not yet decoded»: стор битмапов заполняется ОДИН раз, начальным проходом конвейера

**Статус:** OPEN
**Тип:** дефект реализованного кода — путь есть и работает для парсерной картинки, ломается для любой, появившейся позже.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — живой замер, варианты `canvas-drawimage-parser` / `canvas-drawimage-visible`)
**Область:** shell (`crates/shell/src/page_pipeline.rs:773-794` — единственный вызов `register_img_bitmaps`, источник — `collect_image_requests` по DOM на момент разбора), js (`crates/js/src/img_bitmap_store.rs` — `clear_img_bitmaps()` + перезапись всего стора на каждый вызов)
**Владелец:** P3.

## Симптом

`ctx.drawImage(img, …)` не бросает, помечает канву грязной и не меняет ни
одного пикселя, если `<img>` создан скриптом (`document.createElement('img')`)
или если парсерному элементу присвоили новый `src` из скрипта. Ошибки нет
нигде: `drawImage` возвращает `undefined`, как и положено.

`createImageBitmap(img)` на том же элементе отклоняется с
`Error: createImageBitmap from HTMLImageElement: image not yet decoded` —
единственное место, где движок вообще называет причину.

Парсерная картинка при этом рисуется правильно, и `createImageBitmap` на ней
резолвится. То есть дефект не в канве, не в декодере и не в сети: сервер
пробы видит запрос за скриптовой картинкой ровно так же, как за парсерной.

## Прямое измерение

`tests/wpt/verify_replaced_content_gaps.py --variant canvas-drawimage-parser`
(2026-09-01, dev-release, Linux, `main` = `287562e61`). Канва залита белым
(`255,255,255,255`), затем в неё рисуют:

```
parser-draw = ok       ctx.drawImage(<парсерный img>, 0,0,20,20)
parser-pixel = 0,0,0,255          ← нарисовалось (чёрный прямоугольник)
parser-bitmap-ok 100              ← createImageBitmap резолвится, width=100
repoint = ok           p.src = "media/1x1-green.png"   (сервер запрос видит)
repoint-draw = ok
repoint-pixel = 0,0,1,255         ← СТАРЫЙ битмап, не зелёный
```

и на соседней странице, где обе картинки построены скриптом
(`--variant canvas-drawimage-visible`):

```
png-naturalWidth = undefined      (это BUG-630)
png-draw = ok
png-pixel = 255,255,254,255       ← белый фон, ничего не нарисовано
svg-draw = ok
svg-pixel = 254,255,254,255       ← то же для SVG-картинки
white-control = 255,255,255,255   ← контроль: незатронутый угол
[server saw: GET /images/black-rectangle.png, GET /vrc-square.svg]
```

`drawImage(<canvas>)` на той же странице работает
(`drawImage-canvas = 254,0,0,255`), то есть канва-источник и канва-приёмник
исправны — отличается только ветка `isImg`.

## Корень

`crates/shell/src/page_pipeline.rs:773-794` — единственный в воркспейсе вызов
`register_img_bitmaps`:

```rust
let img_reqs = { let d = doc_arc.lock().unwrap();
                 lumen_layout::collect_image_requests(&d, viewport) };
let bitmaps = img_reqs.iter().filter_map(|req| { … }).collect();
if !bitmaps.is_empty() { js.register_img_bitmaps(bitmaps); }
```

Он выполняется в проходе разбора документа, по слепку DOM на тот момент.
`V8JsRuntime::register_img_bitmaps` (`crates/js/src/v8_runtime/runtime.rs:781`)
начинает с `clear_img_bitmaps()`, так что стор всегда равен последнему
слепку — ни один более поздний `<img>` в него не попадает и ни один прежний
из него не выбывает при смене `src`. Натив
`_lumen_canvas2d_draw_image_from_img` (`crates/js/src/canvas2d.rs:1010`)
устроен как `with_img_bitmap(img_nid, …)` — `Option`, чей `None` просто
ничего не делает, поэтому промах стора неотличим от успешной отрисовки
прозрачного.

Форма та же, что у [BUG-885](BUG-885-OPEN.md) (под-документы `<iframe>`
грузятся одним проходом `parse_and_layout`, и всё, что вставил скрипт, мертво):
однопроходная регистрация ресурса, живущая в конвейере разбора.

## Кого это держит

Канва + картинка — основная идиома целого семейства WPT: тест рисует
изображение и сравнивает пиксели. В остатке WPT-RUN-5 на этом стоят
`html/dom/elements/images/bypass-cache-revalidation.html` (`getImagePixel`
сравнивает `[0,255,0,255]`), `svg/embedded/image-crossorigin.sub.html` (4
сабтеста, все через `getImageData` нарисованного), `html/canvas/element/manual/
drawing-images-to-the-canvas/drawimage_svg_image_with_foreign_object_does_not_taint.html`.
Все они строят `<img>` из скрипта — иначе не получится, URL у них
вычисляемый.

## Направление починки

Регистрировать битмап в момент, когда картинка декодирована, а не в момент
разбора документа, и не стирать стор целиком: `set_img_bitmap(nid, image)`
уже точечный. Достаточно, чтобы декодер (все три места, перечисленные в
[BUG-630](BUG-630-OPEN.md): `decode_image`, ленивая загрузка,
background-image) вызывал его для своего `nid`, а смена `src` — снимала
прежнюю запись. Тогда же станет верным и `createImageBitmap`.

Проверять: `--variant canvas-drawimage-visible` должен давать
`png-pixel = 0,0,0,255`, а `--variant canvas-drawimage-parser` —
`repoint-pixel = 0,255,0,255` (после смены `src` рисуется НОВАЯ картинка).
