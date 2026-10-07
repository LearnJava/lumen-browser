# BUG-1252 — `offsetLeft`/`offsetTop` элемента, чей `offsetParent` — `<body>`, отсчитываются от border-box тела (с учётом его `margin` 8 px), а CSSOM View §5 велит

**Статус:** OPEN
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js::_lumen_offset_origin`)

## Симптом

`offsetLeft`/`offsetTop` элемента, чей `offsetParent` — `<body>`, отсчитываются от border-box тела (с учётом его `margin` 8 px), а CSSOM View §5 велит мерить от начала initial containing block. Ожидаемое 8 даёт 0, ожидаемое 30 даёт 22 — всюду ровно на margin тела меньше. Проверено A/B (БЕЗ коммита): одна строка `if (parent === null || _lumen_is_body(parent)) return [0, 0];` в `_lumen_offset_origin` без регрессий улучшает 21 id `css/css-flexbox` (+148 сабтестов, 3 из них проходят целиком). Задевает любой тест на `checkLayout`/`check-layout-th.js` с `data-offset-x/y`.

## Описание

Проба: `<body style="margin:8px"><div id=a style="width:20px;height:10px"></div>` — `a.offsetLeft` отдаёт 0, ожидается 8 (`offsetParent` = `BODY`; `getBoundingClientRect().left` = 8).

## Причина

`_lumen_offset_origin` (`web_api_shim_mid.js`, рядом с `_lumen_offset_parent_nid`) для `offsetParent`-а берёт `_lumen_get_bounding_rect(parent)` + border. Для `<body>` это (8, 8), тогда как точка отсчёта — (0, 0) viewport/ICB. `offsetLeft` самого `<body>` уже обходится отдельной веткой (BUG-476).

## Как найдено

WPT-RUN-14 срез 1: `align-content-horiz-001a.html` (`offsetLeft expected 8 but got 0`) и 20 других. Правка прогнана на `css/css-flexbox`: 1838 против 1690 сабтестов, 3 теста стали зелёными целиком, 21 улучшен, новых падений 0. Правка откачена — P2 баги не чинит (`docs/dev-roles.md`).

## Как проверить

`run_corpus.py --prefixes css/css-flexbox --out-dir .tmp/wpt-run14/flexbox` до и после; плюс регрессия в `crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`.

После починки перемерить `align-items-baseline-*` (css-flexbox): красны ровно на эти 8 px; остаток FLEX-VWM-5 п. 4 ([ROADMAP.md](../ROADMAP.md)).
