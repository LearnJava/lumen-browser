# BUG-1250 — `position:absolute; height:N%` внутри `position:relative` предка с определённой `height` разрешается в 0 (или в высоту содержимого), а не в N% высоты 

**Статус:** OPEN
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — разрешение процентных `height` у `position:absolute`)

## Симптом

`position:absolute; height:N%` внутри `position:relative` предка с определённой `height` разрешается в 0 (или в высоту содержимого), а не в N% высоты padding-box предка. Воспроизводится без flex. `top:0;bottom:0` тот же предок обрабатывает верно. WPT-RUN-14-S1: 16 reftest `css/css-flexbox/abspos/*` и корня (`abspos-autopos-htb-ltr.html` и 5 аналогов), все с `width:100%;height:100%` у abspos-ребёнка.

## Описание

Через `--dump-layout`:

```html
<div style="position:relative;width:100px;height:100px"><div style="position:absolute;width:100%;height:100%;background:green"></div></div>
```

ребёнок получает `rect=(0,0,100,0)` (высота 0), ожидается 100. `height:50%` с `width:50px` — тоже 0. С `top:0;bottom:0` — 100, верно.

## Как найдено

WPT-RUN-14 срез 1, кластер «abspos height:%» (16 id). Причина установлена пробой, не по имени файла.

## Как проверить

`abspos-autopos-htb-ltr.html` через `run_corpus.py --prefixes css/css-flexbox/abspos`.
