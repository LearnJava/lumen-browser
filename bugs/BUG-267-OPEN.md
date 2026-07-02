# BUG-267

**Статус:** OPEN
**Компонент:** paint (CPU-растеризатор)
**Симптом:** CPU-путь рендера (`--screenshot`, `--print-to-pdf`, IPC-снимок, cpu_vs_edge-гейт) на страницах с большим полотном и множеством слоёв/фильтров тратит десятки-сотни секунд. Замер 2026-07-02: `lenta.ru` через `--screenshot` = **141.7 с**, при том что fetch+JS+layout (`--dump-layout`) той же страницы = **5.3 с**. Полотно 1024×7324. Значит ~136 с — чистая растеризация.

> **Область:** только CPU-бэкенд. Живое окно рендерит через femtovg (GPU/OpenGL, оффскрин-таргеты с учётом bounds), там этого blow-up нет. Но CPU-путь общий для скриншотов, печати и всего графического тест-гейта, поэтому баг тормозит инструментальные прогоны и headless-потребителей.

---

## Первопричина

`crates/engine/paint/src/cpu_raster.rs` (строки ~155–485): **каждый `DisplayCommand::Push*` аллоцирует `tiny_skia::Pixmap::new(width, height)` во всё полотно** — 16 мест (155, 277, 286, 390, 403, 415, 428, 441, 447, 460, 474 и т.д.). Для lenta это ~30 МБ (1024×7324×4) на слой. Страница с десятками теней/фильтров/масок → десятки полнополотных аллокаций + столько же полнополотных blend-проходов при `Pop*`.

Отдельно `PushFilter` **игнорирует своё поле `bounds`** — строка ~427:
```rust
DisplayCommand::PushFilter { filters, bounds: _ } => {
    let layer = tiny_skia::Pixmap::new(width, height)  // всё полотно
        .ok_or("Failed to create filter layer")?;
```
Гауссово размытие `box-shadow`/`text-shadow` (`gaussian_blur`, 3 box-pass H+V) прогоняется по всем `width*height` пикселям вместо bbox тени. Для страницы с десятками теней это доминирующая стоимость.

`display_list.rs` уже несёт нужный bbox почти во всех Push-командах:
- `PushFilter { filters, bounds: Option<Rect> }` (display_list.rs:671)
- `PushBackdropFilter { filters, bounds: Rect }` (:691)
- `PushClipRect { rect }` (:525), `PushClipRoundedRect { rect, .. }` (:532)
- `PushMaskImage/LinearGradient/RadialGradient/ConicGradient { rect, .. }` (:576/588/595/602)

То есть данные для оптимизации **уже прокинуты** — CPU-путь их отбрасывает.

## Как чинить (срез за срезом, каждый — отдельный merge)

**Срез A — `PushFilter`/`PushBackdropFilter` по bounds (наибольший выигрыш):**
1. В обработчике `PushFilter` использовать `bounds` (если `Some`): аллоцировать слой размером с `bounds` (в device-пикселях, с запасом на радиус размытия ≈ `ceil(3σ)` по каждой стороне), рисовать вложенный субдерево со сдвигом `-bounds.origin`, размывать только этот прямоугольник, при `PopFilter` композить обратно со сдвигом `+bounds.origin`.
2. Клэмпить bbox к полотну (пустой/вне-экранный слой → skip subtree).
3. `bounds == None` → текущее полнополотное поведение (fallback, сохранить).

**Срез B — clip-слои по rect:** `PushClipRect`/`PushClipRoundedRect` — аллоцировать слой по пересечению `rect` с текущим полотном, а не полное.

**Срез C — маски по rect:** `PushMask*` — то же по их `rect`.

**Срез D — `PushOpacity`/`PushTransform` без bounds:** у них bbox нет. Вариант — вычислять bbox субдерева проходом вперёд по display-list до парного `Pop` (дешёвый scan рамок команд), либо (проще и почти всегда достаточно) оставить полнополотными — они дешевле фильтров (один alpha-blend без blur).

## Валидация

- Замер до/после: `time lumen --screenshot out.png https://lenta.ru/` (цель — секунды, не минуты). Бенч можно временно повесить на `samples/` большую страницу с тенями.
- **Пиксельная эквивалентность:** `python graphic_tests/run.py --continue-on-fail` — результат обязан совпасть с baseline (`Изменений нет`); порог 0.5% не трогать. Особое внимание тестам с `box-shadow`/`filter`/`backdrop-filter`/`mask`/`clip-path` (TEST-30, 31, 36, 45, 39).
- Регресс-юниты в `cpu_raster.rs`: слой по bounds даёт тот же пиксель, что полнополотный (сравнить два прохода на маленьком display-list с тенью у края).

## Заметки

- `femtovg_backend.rs` уже использует оффскрин-таргеты с учётом bounds для backdrop-filter — можно свериться с его логикой bbox как с эталоном.
- Не трогать `gaussian_blur` (integer-only, кросс-ОС бит-идентичный) — менять только РАЗМЕР пиксмапа, который в него подаётся.
