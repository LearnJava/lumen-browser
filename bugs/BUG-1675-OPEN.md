# BUG-1675 — `<template>` в потоке получает `display: block` вместо `none`

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md))
**Область:** layout/UA-стили (`html.css` движка: правило `[hidden], area, base, ..., template, ... { display: none }`, HTML §15.3.1)

## Симптом

```html
<!doctype html><template id=tp></template><style id=st></style>
```

`getComputedStyle(document.getElementById('tp')).display`: Chrome `none`, Lumen `block`
(`style` у Lumen — `none`, то есть правило есть, но `template` в него не входит).

## Что делать

Добавить `template` в UA-правило `display: none` (HTML «Hidden elements»: `area, base, basefont,
datalist, head, link, meta, noembed, noframes, param, rp, script, style, template, title`). Проверить,
что содержимое шаблона (DocumentFragment) не попадает в дерево боксов и `getBoundingClientRect()` у
`template` — нулевой.

## Как проверить

Репро выше: `none`. Репро найдено `.tmp/gd_repro.py nobox` (Chrome и Lumen рядом).
