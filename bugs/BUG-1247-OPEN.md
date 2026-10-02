# BUG-1247 — автономный `.svg`-документ: элементы в пространстве имён XHTML, SMIL не работает

**Статус:** OPEN
**Заведён:** 2026-10-02 (P6, найден в BUG-1095 срез 4)
**Область:** dom/js

## Симптом

В `svg/animations/repeatcount-numeric-limit.tentative.svg` (корень `<svg xmlns="http://www.w3.org/2000/svg">`) проба показала: `document.getElementsByTagName('animate')[0].namespaceURI === 'http://www.w3.org/1999/xhtml'`, `constructor.name === 'HTMLUnknownElement'`, у родительского `<rect>` то же. Следствие: `instanceof SVGAnimationElement` false, SMIL-тик пропускает элемент, `endEvent` не приходит, тест `TIMEOUT`. Также `document.readyState` в момент `promise_test` — `loading`.

## Не проверялось

Это свойство всех `.svg`-документов верхнего уровня или только файлов с `<h:script>` (XHTML-префикс) — причина не разобрана.
