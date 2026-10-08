# BUG-1403 — Блок без BFC растёт до своих float: высота охватывает float, а следующий блок не обтекает его

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout (`crates/engine/layout/src/box_tree/block_flow_trampoline.rs:819-830` — «the container height must also enclose all floats»; закреплено юнит-тестом `tests/half_leading_columns_floats.rs:621` `container_height_encloses_float`)

## Симптом

`--dump-layout`, `body{margin:0;font:20px/1 Ahem}`:

| страница | получено | ожидается (CSS 2.1 §9.5, §10.6.3, §10.6.7) |
|---|---|---|
| `<div id=a style="width:200px;background:yellow"><div style="float:left;width:20px;height:100px;background:red"></div>x</div><div id=b style="width:200px;background:green">y</div>` | `a`: 200×100; строка `y` на `y=100`, `x=0` | `a`: 200×20 (высота по строке `x`; float выступает вниз); `b` на `y=20`, строка `y` обтекает float: `x=20` |
| `<div><div id=a><div style="float:left;height:50px;width:10px"></div></div></div><div id=b style="height:5px"></div>` | `a` высотой 50, `b` на `y=50` | `a` высотой 0, `b` на `y=0`, обтекает float (`x=10`) |
| то же, но `#a{overflow:hidden}` | `a` высотой 50 | `a` высотой 50 (BFC-корень охватывает float) |
| `<div style="float:left;height:50px;width:10px"></div><div id=b style="height:5px"></div>` (float — сосед, обёртки нет) | `b` на `x=10` | верно |

Код `block_flow_trampoline.rs:819-826` называет это «CSS 2.1 §9.5: the container height must also enclose all floats» — §9.5 говорит про обтекание, а охват float по высоте задаёт §10.6.7 и только для корня форматирующего контекста блоков. Если поведение сознательное (компромисс для страниц, где clearfix сделан иначе), его нужно записать как отступление; иначе это дефект с большим радиусом — затрагивает любой блок с float внутри и без `overflow`/`display:flow-root`/`clear`.

## Как найдено

WPT-RUN-14 срез 17, побочно: проба `contain: layout` с float внутри показала, что обёртка без `contain` ведёт себя так же. Отдельно WPT не измерялось: в `css/CSS2/floats*` срезов 13 похожие падения записаны как «BFC-бокс рядом с float» и «схлопывание полей и clearance» ([css.md §срез 13](../docs/wpt-vendor-notes/css.md)).

## Что делать

Проверить `css/CSS2/floats/*`, `floats-clear/*` до и после: ограничить охват float по высоте BFC-корнями (`establishes_bfc`, `bfc.rs`), поправить юнит-тест. Правка двигает пиксели — полный `graphic_tests/run.py --continue-on-fail` и новые эталоны в том же коммите.

## Как проверить

Страница из таблицы; `css/CSS2/floats/floats-001.xht` и соседние `floats-*.xht`.
