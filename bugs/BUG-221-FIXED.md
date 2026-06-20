# BUG-221

**Статус:** FIXED 2026-06-20
**Компонент:** paint (`crates/engine/paint/src/cpu_raster.rs`, `Renderer::render_to_image_cpu`)
**Тест:** TEST-18 (images), TEST-36 (border-radius), TEST-39 (gradients) и др. — только в `--ipc`/`--screenshot` CPU-пути

## Исправление (2026-06-20)

Все три примитива доведены до паритета с femtovg/gdigrab по `run.py --ipc`:

- **border-radius** (TEST-36 **60.60% → 0.47%**): `rasterize_fill_rounded_rect`
  не клампил радиусы к боксу — пилюли `border-radius: 999px` уводили кубические
  control-точки далеко за прямоугольник и заливали гигантский диагональный клин
  поверх страницы. Теперь радиусы клампятся (`CornerRadii::clamped_to_box`) и путь
  строится общим kappa-Безье-контуром `push_rounded_rect_outline`.
- **gradients** (TEST-39 **10.68% → 1.40%**): `rasterize_radial_gradient` рисовал
  анизотропный эллипс (`rx`/`ry`), а femtovg-окно — изотропный **круг** радиусом
  `hypot(dx, dy)` (farthest-corner). CPU-путь переписан на круг. Попутно gradient-mask
  тест TEST-26 5.02% → 0.00%.
- **images** (TEST-18 **52.22% → 2.15%**): `DrawImage`/`LazyImageSlot` рисовали
  серый placeholder. Теперь декодированные пиксели прокидываются в `rasterize_cpu`
  (`render_source_to_png` передаёт `parsed.images`), рисуются с учётом
  `object-fit`/`object-position` через общий `display_list::fit_image_rect`, и —
  как в femtovg — пред-уменьшаются area-averaged фильтром
  (`lumen_image::resize_area_avg`) до placement-размера (bilinear-сэмплинг
  полноразмерного фото иначе шумел против Edge).

Регрессия: 4 unit-теста в `cpu_raster.rs`; 801 тест `lumen-paint` зелёный. Затронут
только CPU-снимок (`cpu_raster.rs` + плумбинг) — femtovg-окно и gdigrab-гейт не изменены.

## Описание

CPU-бэкенд снимка (`render_to_image_cpu`, feature `cpu-render`, tiny-skia), на
котором работают `--screenshot` (U-0) и `--ipc-server`/`run.py --ipc` (TAB-7), **не
достигает визуального паритета** с femtovg-бэкендом окна для нескольких примитивов:

- **border-radius** — скруглённые боксы рисуются прямоугольниками (углы заливаются
  цветом бокса вместо клипа). TEST-36 CPU-снимок: вся область 200×100 = сплошной
  `(252,129,129)`, скругления нет → diff с Edge 60.6% (gdigrab-путь: 1.11%).
- **gradients** — linear/radial-gradient фон не рендерится как градиент. TEST-39
  CPU diff 10.7% (gdigrab: PASS).
- **images** — `<img>`/object-fit не рисуются или рисуются иначе. TEST-18 CPU diff
  52.2% (gdigrab debtor baseline 2.11%).

Геометрия/цвет/текст/transform/opacity/padding/margin/box-shadow/outline в CPU-пути
совпадают с Edge пиксель-в-пиксель (TEST-00..13/22 = 0.00–0.22% по `run.py --ipc`).

## Воспроизведение

```bash
lumen.exe --screenshot t36.png graphic_tests/36-border-radius.html
# t36.png: скруглённый бокс нарисован квадратом
python graphic_tests/run.py --ipc --only 36   # FAIL 60.60% (gdigrab: DEBTOR 1.11%)
```

## Влияние

`run.py --ipc` (TAB-7) — рабочий детерминированный capture-транспорт (Python-клиент
bincode к `--ipc-server` корректен, протокол верифицирован), но **пока опционален**:
gdigrab остаётся дефолтом, потому что CPU-снимок не на паритете по этим примитивам.
Полностью убрать gdigrab из пайплайна можно только после паритета CPU-бэкенда.
Пересекается с информационным CPU-vs-Edge гейтом (BUG-121, `snapshot_vs_edge`).

## Как чинить

Довести `cpu_raster.rs` до паритета с femtovg по:
1. `DrawImage` — растеризация декодированных картинок с object-fit/resampling.
2. Заливки градиентами (linear/radial/conic) в `FillRect`/`FillPath`.
3. Клип по `border-radius` (rounded-rect клип-маска), как в femtovg-бэкенде.

После каждого — `python graphic_tests/run.py --ipc --only NN` должен сойтись к
diff gdigrab-пути для того же теста.
