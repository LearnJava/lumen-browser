# BUG-221

**Статус:** OPEN
**Компонент:** paint (`crates/engine/paint/src/cpu_raster.rs`, `Renderer::render_to_image_cpu`)
**Тест:** TEST-18 (images), TEST-36 (border-radius), TEST-39 (gradients) и др. — только в `--ipc`/`--screenshot` CPU-пути

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
