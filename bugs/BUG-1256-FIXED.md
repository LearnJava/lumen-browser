# BUG-1256 — `<img>` с одной заданной стороной (`height:30px`, `width:auto`) в row-контейнере получает ширину intrinsic-размера (1 px у GIF 1×1), а не перенесённую

**Статус:** FIXED 2026-10-07
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs` — `<img>` как flex-элемент, перенос пропорций)

## Симптом

`<img>` с одной заданной стороной (`height:30px`, `width:auto`) в row-контейнере получает ширину intrinsic-размера (1 px у GIF 1×1), а не перенесённую по пропорции (30 px), CSS Flexbox L1 §9.2 / §4.5. В column-контейнере с обеими сторонами — верно (BUG-736 закрыл частный случай). WPT-RUN-14-S1: кластер «img/svg/aspect-ratio items» — 73 id (`flex-aspect-ratio-img-*`, `image-as-flexitem-size-*`, `aspect-ratio-intrinsic-size-*`, `svg-root-as-flex-item-*`; оценка по именам; причина подтверждена одной пробой), 107 упавших сабтестов.

## Описание

Проба: `<div style="display:flex;width:200px"><img src="data:image/gif;base64,R0lGODlhAQABAIAAAAUEBAAAACwAAAAAAQABAAACAkQBADs=" style="height:30px"></div>` — `Image rect=(0,0,1,30)`, ожидается `30×30`.

## Как найдено

WPT-RUN-14 срез 1.

## Как проверить

`css/css-flexbox/image-as-flexitem-size-*.html`.

## Исправление

flex.rs, ветка `FlexBasis::Auto` row: у replaced-элемента с ratio и авторской px-`height` базовый размер — высота, перенесённая через ratio (Flexbox §9.2). Тест `bug1256_row_flex_img_with_authored_height_transfers_width`. Остаток кластера (svg-root, `aspect-ratio` у не-img) — не проверялся.
