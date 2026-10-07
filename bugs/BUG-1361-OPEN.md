# BUG-1361 — `position:absolute` вне непозиционированного `overflow:scroll|hidden` предка обрезается его клипом, хотя containing block — снаружи

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** paint/layout (`crates/engine/layout/src/box_tree/`, `crates/engine/paint/src/display_list/` — `PushScrollLayer` охватывает abspos-потомка, чей containing block не внутри скролл-контейнера)

## Симптом

`--dump-display-list`, `body{margin:0}`: `<div style="height:20px;background:red"></div><div style="width:100px;height:100px;overflow:scroll;margin-top:40px"><div style="position:absolute;top:0;left:0;width:200px;height:20px;background:green"></div></div>`. Containing block abspos — начальный (у `overflow`-блока нет `position`), поэтому зелёный блок должен лежать в `(0,0,200,20)` поверх красного и **не** обрезаться:

```
FillRect (0,0,500,20) #ff0000        // красный
PushScrollLayer clip=(0,60,100,100)  // клип скролл-контейнера
FillRect (0,0,200,20) #008000        // зелёный — внутри клипа (0,60…), не виден
```

С `position:relative` на контейнере клип корректен (зелёный `(0,60,…)`). По CSS 2.1 §11.1.1 клип `overflow` не действует на потомка, чей containing block — предок контейнера.

## Как найдено

WPT-RUN-14 срез 13: `positioning/abspos-overflow-001…010.xht` — 10 id: тест рисует «FAIL» красным, а «PASS» зелёным поверх (`abspos-overflow-001`: тест — красный `ff0000` 3033 px, эталон — зелёный `008000`).

## Что делать

Не оборачивать abspos-потомка в `PushScrollLayer` контейнера, если containing block лежит снаружи (клип — только для потомков, у которых контейнер в цепочке containing block).

## Как проверить

`css/CSS2/positioning/abspos-overflow-001.xht`.
