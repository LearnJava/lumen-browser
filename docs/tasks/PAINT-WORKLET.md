# PAINT-WORKLET — передача между сессиями (ветка `p1-paint-worklet`, слот `p1-work`)

Задача — `ROADMAP.md` строка PAINT-WORKLET (XL). Решение: **срез 1 = Paint API** (эта ветка);
**срез 2 = Layout API** (`registerLayout`, `display: layout()`, `LayoutChild`/`LayoutFragment`,
`intrinsicSizes()`, `autoBlockSize`) выносится в новую строку `PAINT-WORKLET-LAYOUT` в ROADMAP
(указатель на неё — в `STATUS-P1.md` вместо указателя на PAINT-WORKLET). Критерий среза 1:
`css/css-paint-api/background-repeat-x.https.html` совпадает с `-ref.html` (проверка вручную:
`lumen --screenshot` теста и референса, сравнить PNG; reftest-wait.js снимает класс после
`addModule` + `document.body.animate(...).finished`).

## Сделано (WIP-коммит)
- `lumen-layout`: `BackgroundImage::Paint { name, args }` (было `Paint(String)`), `parse_paint_function`
  → `(name, args)` с делением по запятым верхнего уровня; тесты в `style/tests/cascade.rs`.
- `lumen-paint`: `display_list/paint_worklet.rs` — `PaintRequest`, `collect_paint_requests(root, dpr)`,
  `paint_image_key` (`paint:{node}:{hash(name,args,w,h)}`), `paint_tile_size` (auto/cover/contain →
  область позиционирования; % и px по осям). Эмиттер в `background_mask.rs` кладёт
  `DrawBackgroundImage { src: <ключ>, size: Length(Px(w), Px(h)) }`. Экспорт из `lib.rs`.
  `cargo check -p lumen-paint --tests` проходит; тесты paint ещё НЕ запускались.

## Осталось (в порядке)
1. **JS-сторона** (`crates/js/src/paint_worklet.rs`, шим вынести в `src/shim/paint_worklet.js`):
   - глобальный `Worklet` (`addModule(url, opts)` → Promise: `fetch(url).text()` (проверить blob:), выполнить
     модуль через `new Function('registerPaint', …shadowed globals…)`), `CSS.paintWorklet = new Worklet`.
   - `registerPaint(name, cls)` по спецификации: имя строка, `cls` конструктор с `paint`, `inputProperties`
     (итерируемое строк), `inputArguments` (синтаксисы), `contextOptions.alpha`; повтор имени →
     `InvalidModificationError`; после успешной регистрации — взвести «эпоху реестра» (native).
   - рендер: `PaintRenderingContext2D` — обёртка над `new OffscreenCanvas(w*dpr, h*dpr).getContext('2d')`
     (без `fillText`/`getImageData`/`canvas`), `ctx.scale(dpr,dpr)`; вызвать `new cls().paint(ctx, {width,height}, styleMap, args)`;
     `styleMap` из `getComputedStyle(el).getPropertyValue(prop)` по `inputProperties` (el по `node`).
   - пиксели забрать `offscreen_canvas::take_offscreen_pixels(canvas_id) -> Option<(w,h,rgba)>` (уже есть).
2. **Хост**: `V8JsRuntime::set_paint_requests(Vec<PaintRequest-подобный>)` и `flush_paint_images() ->
   Vec<(String key, u32 w, u32 h, Vec<u8>)>` (через `self.run`, thread_local на JS-потоке; рендер лениво
   при флаше, если запросы изменились или эпоха реестра выросла). Метод в трейте `PersistentJs`
   (`crates/shell/src/persistent_js.rs`, реализация ~стр. 1437).
3. **Живой shell**: после релейаута (`relayout_job.rs` ~517, рядом с `spawn_dynamic_background_image_loads`)
   и в начальном пайплайне — `collect_paint_requests(layout, dpr)` → `set_paint_requests`; флаг
   `paint_worklet_active`; в `app/about_to_wait.rs` рядом с дренажом canvas (стр. ~615), если флаг
   взведён: `drain_query_js(|j| j.flush_paint_images())` → `renderer.register_image(key, Image{Rgba8})` →
   `request_redraw`.
4. **Headless**: `crates/driver/src/session.rs` (после `load_background_images`, `screenshot_cpu_rgba`) и
   `crates/shell/src/dump_mode.rs` (~250, куда добавляется `canvas_updates_as_images`) — то же через
   `flush_paint_images` в набор изображений. Скрипты страницы к этому моменту уже отработали; `addModule`
   асинхронен — убедиться, что очередь микрозадач/таймеров прокачана до снимка (reftest-wait).
5. WPT-проверка: `tests/wpt/css/css-paint-api/background-repeat-x.https.html` (+ `background-image-*`,
   `paint-arguments`, `parse-input-arguments-*`); для `worklet-reftest.js` нужны `URL.createObjectURL`+fetch(blob:)
   и `Element.animate(...).finished`.
6. Docs в том же коммите: `CAPABILITIES.md` (Houdini Paint API), `subsystems/js.md`/`paint.md`,
   `ROADMAP.md` (строка PAINT-WORKLET → done со ссылкой на срез; новая PAINT-WORKLET-LAYOUT),
   `STATUS-P1.md` (заменить указатель), BUG-779 → FIXED (если критерий среза закрыт), `docs/engine-gaps.md` при необходимости.
7. Гейт: `cargo clippy -p lumen-layout -p lumen-paint -p lumen-js -p lumen-shell --all-targets -- -D warnings`,
   `scripts/scoped-test.sh`, затем полный `python graphic_tests/run.py --continue-on-fail` только если менялись пиксели
   (paint() раньше рисовался серой заглушкой — снапшоты/голдены с `paint:` проверить grep-ом).

## Выбор дизайна (чтобы не переоткрывать)
Ключ картинки не содержит значений `inputProperties` — их читает JS из `getComputedStyle` при каждом рендере;
перерендер на каждый новый набор запросов (после релейаута) и при росте эпохи реестра. `fixed`/`local`
фоны дают другой origin — ключ может не совпасть (картинка не нарисуется), для среза 1 допустимо.
Только `background-image`; `border-image`/`mask-image`/`list-style-image` с `paint()` — отдельным срезом.
