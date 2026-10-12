# BUG-1348 — `html { height: 100% }` не разрешается против вьюпорта: высота корня — по содержимому

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — процентная `height` корневого элемента, начальный содержащий блок = вьюпорт, CSS 2.1 §10.5)

## Симптом

`<html style="height:100%"><body style="margin:0"><p>x</p>`, `--viewport 800x600`: `document.documentElement.getBoundingClientRect().height` = **19.36** (высота строки), ожидается **600**. Без DOCTYPE и с XHTML DOCTYPE одинаково. `--dump-layout`: `Block rect=(0,0,800,81.72) h=100.00%` — процент остаётся записанным, но высота считается по содержимому.

## Как найдено

WPT-RUN-14 срез 11: `backgrounds/background-bg-pos-204.xht`, `-206.xht` — `html { background: bottom right url(diamond.png) no-repeat; height: 100% }`: ромб у Lumen на `y = 72…81` (низ корня 82 px), у эталона — `y = 590…599` (низ вьюпорта 600).

## Что делать

Для корневого элемента процент высоты резолвить против высоты начального содержащего блока (вьюпорта), как для `<body>` при определённой высоте родителя.

## Как проверить

`css/CSS2/backgrounds/background-bg-pos-204.xht`.
