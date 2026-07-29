# BUG-447 — на движке по умолчанию (V8) `drawImage(imgElement, …)` молча не рисовал ничего: реестр декодированных `<img>` никогда не наполнялся

**Статус:** FIXED 2026-07-29
**Компонент:** js (`crates/js/src/v8_runtime.rs` — отсутствовал `V8JsRuntime::register_img_bitmaps`), shell (`crates/shell/src/main.rs` — `impl PersistentJs for V8PersistentJs` не переопределял `register_img_bitmaps`)
**Найден:** 2026-07-29, P1, при портировании Canvas-2D-семейства тест-монолита `dom.rs` на V8 (срез S12b-24-core)

---

## Суть

Canvas 2D читает пиксели `<img>`-источника из потоко-локального реестра
`crates/js/src/img_bitmap_store.rs` (`with_img_bitmap`, вызовы в `canvas2d.rs:862`,
`:889`, `:1813`, `:1830` и `offscreen_canvas.rs:187`). Наполнять реестр умел
ровно один метод — `QuickJsRuntime::register_img_bitmaps` (`crates/js/src/lib.rs:1911`).

`V8JsRuntime` такого метода **не имел вовсе**. В шелле это не дало ошибки
компиляции, потому что `register_img_bitmaps` объявлен в трейте `PersistentJs`
с дефолтной реализацией-ноопом (`main.rs:2720`, комментарий «Default no-op
covers non-QuickJS builds and `NullPersistentJs`»), а `impl PersistentJs for
V8PersistentJs` его не переопределял. Вызов из конвейера загрузки
(`main.rs:5551`, сразу после `fetch_and_decode_images`) на дефолтной сборке
попадал в этот нооп.

Следствие: на движке по умолчанию (ADR-018 — V8) реестр оставался пустым всю
сессию, и `ctx.drawImage(img, …)` с `<img>`-источником — во всех трёх формах
(3-, 5- и 9-аргументной) — не рисовал ничего. Ошибки при этом не возникало:
`with_img_bitmap` для незарегистрированного nid просто не зовёт колбэк, что по
контракту неотличимо от «изображение ещё не декодировано», поэтому дефект был
полностью бесшумным. `drawImage` с `<canvas>`-источником, `putImageData` и
`createImageBitmap` не задеты — они не ходят в этот реестр.

Дефект существовал с самого флипа дефолта на V8 (S12, 2026-07-14).

## Почему не ловилось тестами

Три теста, покрывающих ровно этот путь (`canvas_draw_image_from_img_element_3arg`,
`…_5arg`, `…_9arg_crop`), жили в QuickJS-монолите `dom.rs::mod tests` и гонялись
против `QuickJsRuntime`, где метод есть. Это тот же класс, что [BUG-442](BUG-442-FIXED.md):
`dom::tests` были слепы ко всему, что расходится между движками. Дефект вскрылся
в первый же час портирования этих тестов на V8 — `rt.register_img_bitmaps(...)`
не скомпилировался.

## Фикс

1. `V8JsRuntime::register_img_bitmaps` — зеркало QuickJS-версии: очистка реестра
   (он привязан к навигации) и запись пар `(nid, Arc<Image>)` **на JS-потоке**
   через `self.run(...)`, потому что `img_bitmap_store` — `thread_local!`.
   `Arc` разделяется с кэшем декодированных изображений шелла, копии пикселей
   нет (инвариант BUG-272 срез 20 сохранён).
2. `impl PersistentJs for V8PersistentJs` — переопределение, делегирующее в п.1.

## Проверка

`cargo test -p lumen-js --features v8-backend v8_core` — 99/99 зелёных, включая
три портированных `canvas_draw_image_from_img_element_*`, которые сравнивают
пиксели после `flush_canvas_updates()` и до фикса не имели откуда взять источник.

## Рядом

Сверка наборов методов `impl PersistentJs` двух движков (`main.rs`) дала ещё два
расхождения, оба **не** дефекты этого класса и намеренно оставлены:

- `suspend` — снятие снимка кучи для bfcache; на V8 отсутствует осознанно,
  это пункт 5 «Definition of done» в `docs/tasks/ph3-v8-migration.md`
  (round-trip data-глобалов, замыкания вне рамок), не регрессия;
- `debug_js_heap` — временная диагностика QuickJS-кучи (`TEMP BUG-272`),
  у V8 эквивалента нет и не планируется.
