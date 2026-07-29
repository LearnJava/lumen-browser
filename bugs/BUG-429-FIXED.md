# BUG-429 — the CPU-snapshot gate renders pages without running their scripts and without any images

**Статус:** FIXED 2026-07-29 (P3, ветка `p3-bug-429`) — скрипты страницы исполняются, canvas-битмапы
доезжают до растеризатора. Декодированные `<img>` — отдельный дефект, вынесен в [BUG-430](BUG-430-OPEN.md).
**Компонент:** driver (`crates/driver/src/session.rs::run_pipeline` / `screenshot_cpu_rgba`)
**Найден:** P3, 2026-07-29, while fixing [BUG-428](BUG-428-FIXED.md) — BUG-428's «Масштаб»
section assumed the deterministic CPU snapshots share the shell's headless render path.
They do not; this is what actually stands between that gate and Canvas 2D coverage.

## Симптом

`graphic_tests/snapshots/cpu/57-canvas-2d.png` is blank where the canvas bitmaps should be,
and BUG-428's fix (draining `flush_canvas_updates` into the shell's `--screenshot` image set)
does **not** change it: the snapshot gate never reaches that code.

## Причина

`cases::snapshot_cpu` renders through `InProcessSession` (`lumen-driver`), not through the
shell:

* `run_pipeline` (`crates/driver/src/session.rs:263`) parses HTML, collects `<style>` blocks,
  lays out and builds the display list. It installs a V8 runtime with DOM bindings
  (`new_v8_runtime`, line 992) so that `click`/`type_text`/`eval` work — but it **never
  executes the page's own `<script>` elements**. Nothing draws into the canvas buffers, so
  there is no drain to feed anywhere.
* `screenshot_cpu_rgba` (line 428) calls `render_to_image_cpu(…, &[], …)` — the image set is
  unconditionally **empty**. Even decoded `<img>` bitmaps are absent by design (documented in
  `snapshot_cpu.rs`: every image box paints the grey placeholder).

So two independent gaps stack: no script execution → no canvas pixels; empty image set → no
way to hand pixels to the rasterizer even if they existed.

## Фикс (2026-07-29, `crates/driver/src/session.rs`)

**1. Порядок в `run_pipeline` переставлен под шелловский.** Документ сразу заворачивается в
`Arc<Mutex<Document>>`, V8-рантайм ставится и **скрипты страницы исполняются до layout** —
ровно как в `parse_and_layout` → `run_scripts_with_dom` → layout (`crates/shell/src/main.rs`).
Раньше рантайм ставился в самом конце, когда box tree и display list уже построены, поэтому
даже вручную запущенный скрипт не мог повлиять на картинку.

Новый `run_page_scripts` собирает **inline classic** `<script>` в порядке документа
(классификация типа — копия шелловской `is_classic_script_type`), снимает лок с документа
(DOM-биндинги берут тот же мьютекс), выполняет каждый скрипт — ошибка одного не прерывает
остальные, как в браузере — и доводит `document.readyState` до `interactive` → `complete`
(`_lumen_apply_ready_state`), чтобы отработали слушатели `DOMContentLoaded`/`load`.

Внешние `<script src>` **сознательно не грузятся**: это сделало бы детерминированный офлайн-гейт
зависимым от сети и от раскладки файлов вокруг страницы. Их количество печатается в stderr —
страница не может молча потерять поведение. `<script type="module">` пропускается там же
(модули на этом рантайме не подняты — BUG-350).

**2. Canvas-битмапы копятся в сессии и уезжают в растеризатор.** Новое поле
`canvas_images: Mutex<HashMap<u32, Arc<Image>>>` + `drain_canvas_updates()` / `canvas_image_set()`.
`flush_canvas_updates` отдаёт каждый грязный буфер **ровно один раз** (живой шелл дренирует его
раз в кадр в реестр картинок рендерера) — у сессии кадрового цикла нет, поэтому дренаж
аккумулируется, а не потребляется: второй скриншот нетронутого canvas по-прежнему видит пиксели.
Ключ — `canvas:{nid}`, тот же формат, что `display_list.rs` кладёт в `DrawImage.src`, а
шелловский `canvas_updates_as_images` строит на своей стороне. `screenshot_cpu_rgba` дренирует
перед рендером (чтобы перерисовка через `eval`/`click` после навигации тоже попала в кадр) и
передаёт накопленный набор в `render_to_image_cpu`.

Поле и хелперы под `#[cfg(feature = "v8")]`; сборка `--no-default-features --features cpu-render`
проверена отдельно.

## Гейт

Новый модуль `crates/driver/tests/cases/scripted_render.rs` — три теста, каждый утверждает
именно сломанное свойство, а не равенство снапшоту:

* `page_script_mutates_dom_before_layout` — созданный скриптом `div` 111×37 присутствует в
  `layout_snapshot()` (до фикса скрипты не исполнялись вовсе);
* `canvas_2d_pixels_reach_the_cpu_raster` — заливка canvas видна в `screenshot_cpu_rgba()`;
* `canvas_redrawn_after_navigation_is_picked_up` — пиксели не исчезают на втором скриншоте
  (аккумуляция дренажа) и перерисовка через `eval()` попадает в следующий кадр.

Эталон `graphic_tests/snapshots/cpu/57-canvas-2d.png` перегенерирован — единственная из ~80
страниц `PAGES`, которая сдвинулась (595600 различающихся байт); теперь на нём реальный вывод
Canvas 2D (fillRect, arc, path-треугольник, strokeRect + radius) вместо пустых прямоугольников.
Живой пайплайн `graphic_tests/run.py` и `dump_golden.py` не затронуты — они рендерят через
`lumen.exe`, а не через драйвер.

## Остаток

Декодированные `<img>` по-прежнему рисуются серой заглушкой: у `InProcessSession` нет загрузки
подресурсов, а выбор источника (`srcset`/`<picture>`) живёт в шелле. Это отдельный дефект —
[BUG-430](BUG-430-OPEN.md); страницы `18-images`/`19-object-fit` остаются гейтом геометрии,
а не пикселей.
