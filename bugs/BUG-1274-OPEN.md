# BUG-1274 — `writing-mode` с `<body>` не становится главным режимом документа: вертикальный `<body>` прижат к левому краю

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 2 — `css/css-writing-modes/wm-propagation-*`)
**Область:** layout (корень дерева боксов / ICB; не локализовано — `grep -i "principal"` по `crates/` пуст)

## Симптом

```html
<body style="writing-mode:vertical-rl;margin:0"><div style="width:100px;height:100px;background:blue"></div></body>
```

`--dump-layout` (800×600): `<body>` — `Block rect=(0, 0, 100, 100)`, блок — `(0, 0, 100, 100)`, то есть синий квадрат в
левом верхнем углу. Ожидается правый верхний: по CSS Writing Modes L3 §8 главный режим документа берётся с `<body>`,
если он есть, поэтому вьюпорт ведёт себя как `vertical-rl`, и блочный поток начинается с правого края.

Тот же `writing-mode` на `<html>` работает: `<html style="writing-mode:vertical-lr">` даёт вертикальную укладку от левого
края.

## Причина

Распространения главного режима с `<body>` на корень/вьюпорт нет. `<body>` раскладывается как вертикальный бокс внутри
горизонтального `<html>` — ортогональный поток со shrink-to-fit по содержимому.

## Что делать

CSS Writing Modes L3 §8 (Principal Writing Mode): `writing-mode`/`direction` `<body>` (а без него — корня) задают
режим ICB; корневой бокс и `<body>` раскладываются в нём. Проверить и `scrollLeft` вьюпорта у `vertical-rl`
(`wm-propagation-body-scroll-offset-vertical-rl.html`: ожидается 0, получено 1000).

## Как проверить

WPT `css/css-writing-modes/wm-propagation-*` (29 id: 27 reftest и 2 testharness), например
`wm-propagation-body-032.html` против `block-flow-direction-025-ref.xht`.
