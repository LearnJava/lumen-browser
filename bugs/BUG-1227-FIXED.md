# BUG-1227 — `height: N%` на replaced-элементе при неопределённой высоте контейнера даёт 0, а не auto

**Статус:** FIXED 2026-09-30 (P6)
**Компонент:** layout (inline replaced: `<iframe>`, `<img>`, `<object>`)
**Найден:** P6, при срезе 28 [BUG-1011](BUG-1011-OPEN.md), 2026-09-30

## Симптом

CSS 2.1 §10.5: процентная высота при неопределённой высоте containing block ведёт себя как `auto`.
Для replaced-элемента это 150 (iframe) или intrinsic-высота. Lumen даёт 0:

```html
<div style="width:800px;height:600px"><div>
  <iframe style="height:100%"></iframe>   <!-- ожидается 300x150, получаем 300x0 -->
  <img style="height:100%" src="…svg width=50 height=20"> <!-- ожидается 20, получаем 0 -->
</div></div>
```

`--dump-layout` даёт `Iframe rect=(…, 300.00, 0.00) h=100.00%`. Воспроизводится и с атрибутом `height="100%"`
(после среза 28 BUG-1011 он маппится в `Length::Percent`).

Гейт: `svg-embedded-sizing/svg-in-{iframe,img,object}-percentage.html` (все 216 сабтестов каждого падают на
`Wrong height expected 150 but got 0`).

## Исправление (P6 2026-09-30)

Две причины. (1) `finalize_block_height`: при неразрешимом `height: N%` у replaced-элемента бралась
`content_height` (0) — теперь `replaced_percent_height_as_auto` даёт intrinsic-высоту (ratio от ширины
для `<img>`, 150 для iframe/video, размер холста). (2) `InlineBlockRow` раскладывал детей с
`available_height = None`, хотя высота блока-родителя определена — теперь пробрасывается своя.
`svg-embedded-sizing/svg-in-iframe-percentage` — 216/216 PASS. Остаток `svg-in-{img,object}-*` — intrinsic-размер SVG
без width/height и `<object data>` (BUG-798 закрыт для иных сценариев), не этот баг. Display-list A/B по 181 странице
корпуса — байт-в-байт.
