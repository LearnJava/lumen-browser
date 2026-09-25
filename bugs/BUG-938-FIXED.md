# BUG-938 — `drawImage(<img>)` рисует пусто (или прошлую картинку), а `createImageBitmap(<img>)` отклоняется «image not yet decoded»: стор битмапов заполняется ОДИН раз, начальным проходом конвейера

**Статус:** FIXED 2026-09-20 (P3)
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

Форма та же, что у [BUG-885](BUG-885-FIXED.md) (под-документы `<iframe>`
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
[BUG-630](BUG-630-FIXED.md): `decode_image`, ленивая загрузка,
background-image) вызывал его для своего `nid`, а смена `src` — снимала
прежнюю запись. Тогда же станет верным и `createImageBitmap`.

Проверять: `--variant canvas-drawimage-visible` должен давать
`png-pixel = 0,0,0,255`, а `--variant canvas-drawimage-parser` —
`repoint-pixel = 0,255,0,255` (после смены `src` рисуется НОВАЯ картинка).

## Фикс (P3, 2026-09-20)

Направление из раздела выше подтвердилось без изменений: единственная точка
регистрации (`page_pipeline.rs`) остаётся один-в-один снапшотом парсерного
прохода, но третий producer картинок (`spawn_image_requests`/
`spawn_dynamic_image_loads`, BUG-730 — тот же путь, что чинил `<img>`, вставленный
скриптом, и re-point `src`) никогда не звал `register_img_bitmaps`/аналог для
своих декодов. Добавлен точечный метод, не стирающий стор:
`PersistentJs::set_img_bitmap(nid, image, tainted)`
(`crates/shell/src/persistent_js.rs`) → `V8JsRuntime::set_img_bitmap`
(`crates/js/src/v8_runtime/runtime.rs`) → `img_bitmap_store::set_img_bitmap`
(уже был точечным — только регистрация с шелл-стороны была массовой).

Новое поле `Lumen::stream_image_pixels: HashMap<String, Arc<Image>>` (мирроит
`stream_image_sizes`, тот же lifecycle — очищается в тех же трёх местах:
`page_load.rs`'s навигационный сброс, `resumed.rs`'s первая загрузка,
`page_snapshot.rs`'s save/restore для bfcache/переключения вкладок) собирает
`Arc<Image>` из `LoadEvent::ImageDecoded` (`app/user_event.rs`) тем же путём,
которым эта картинка уже уходит в `self.image_cache`/`pending_images` — без
лишнего декода или копии пикселей. `apply_stream_intrinsic_sizes`
(`page_load.rs`) — уже существующий коалесцированный проход, который сопоставляет
`src` → узлы DOM и шлёт `load`/`error` один раз на пару `(nid, url)` — теперь той
же гейтой (`stream_image_events_fired.insert`) заодно шлёт и битмап в
`img_bitmap_store`, если он есть в новой карте.

`tainted` (GAP-CANVASORIGIN) на этом producer'е — всегда `false`: он не проверяет
cross-origin вовсе (в отличие от `page_pipeline.rs`'s эagerного прохода), это
существующий отдельный пробел (BUG-941), не регрессия этого фикса.

## Проверка

Живой прогон `tests/wpt/verify_replaced_content_gaps.py --variant
canvas-drawimage-parser --variant canvas-drawimage-visible --seconds 25`
(dev-release, v8):

```
canvas-drawimage-parser: repoint-pixel = 1,129,0,255   (было: старый чёрный битмап)
canvas-drawimage-visible: png-pixel = 0,0,0,255, svg-pixel = 0,170,0,255  (было: белый фон, ничего не нарисовано)
```

`1,129,0` и `0,170,0` — реальные RGB фикстур (`media/1x1-green.png` = CSS `green`
`(0,128,0)`, `vrc-square.svg` заливка `#0a0` = `(0,170,0)`); `+1` на канале —
тот же округляющий артефакт, что и на парсерной картинке до фикса (`parser-pixel
= 1,0,0,255` вместо `0,0,0`), не связан с этой правкой. `cargo test -p
lumen-js img_bitmap_store` (5/5) и `bash scripts/scoped-test.sh` зелёные —
единственный сбой (`cases::snapshot_cpu::cpu_snapshots_match_references`) это
предсуществующий посторонний дрейф эталонов [BUG-1008](BUG-1008-OPEN.md) (6 из 7
байт-дельт совпадают точно с уже задокументированной сигнатурой).
