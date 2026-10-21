# BUG-1671 — `getBoundingClientRect` = 0,0,0,0 у пустого inline и у inline с блочным потомком

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md))
**Область:** js/layout (прямоугольник строчного элемента для JS: фрагменты inline-бокса)

## Симптом

```html
<!doctype html><p>aaa<span id=e></span>bbb</p>
<!doctype html><p>aaa <a id=b href=#><div>blk</div></a></p>
```

Chrome: `#e` = `29.3,16,0,17` (нулевая ширина, высота строки, на месте в строке), `#b` = `29.3,16,0,17`
(прямоугольник первого строчного фрагмента до блока). Lumen — `0,0,0,0` у обоих. Inline с текстом
(`<a>link</a>`) — верно.

На сайтах: HN `a×20` и `span×18` (пустые `<a id=up_…>`, `<span class=…>`), crates.io `a×30` —
ссылка-карточка вокруг `div`. Скрипты, которые берут позицию якоря/ссылки (`scrollIntoView`,
всплывающие подсказки, IntersectionObserver), получают начало координат.

## Что делать

Для элемента без собственных строчных фрагментов с текстом брать прямоугольник по спецификации
CSSOM View: пустой inline даёт фрагмент нулевой ширины высотой строки в точке вставки; inline,
разорванный блочным потомком (block-in-inline), — объединение `getClientRects()` его фрагментов
(анонимных строчных частей и блочной части). Проверить, что `getClientRects()` для этих случаев
тоже не пуст.

## Как проверить

Репро выше — числа Chrome (±1px по метрикам шрифта). `python scripts/geom_diff.py --only hn crates` —
пропадает строка «нулевой прямоугольник у Lumen по тегам: a×…, span×…».
WPT: `css/cssom-view/getBoundingClientRect-empty-inline.html`, `getClientRects-inline-*.html`.
