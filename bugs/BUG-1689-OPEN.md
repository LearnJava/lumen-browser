# BUG-1689 — Вложенный custom element в shadowRoot.innerHTML другого custom element не получает shadow tree

**Статус:** OPEN
**Заведён:** 2026-10-23 (P1, SHADOW-PARTS)
**Область:** dom/js (custom elements — `attachShadow` в конструкторе вложенного элемента)

## Симптом

Вложенный custom element, созданный через `shadowRoot.innerHTML` другого custom element, не получает shadow tree: конструктор `my-bar` не выполняется до рендера, `my-bar` пуст. Из-за этого `css/css-shadow/part/exportparts-different-scope.html`, `animation-part.html`, `interaction-with-placeholder.html` рендерятся пустыми.

## Проба

`css/css-shadow/part/*` в `python tests/wpt/run_corpus.py --prefixes css/css-shadow/part`: после SHADOW-PARTS 41 из 48 зелёные, остаток — этот дефект и `::cue`/часть псевдоклассов в `test_valid_selector`.

## Как найдено

SHADOW-PARTS, прогон `css/css-shadow/part`.

## Что делать

Устранить причину в указанной области; затем перепрогнать `css/css-shadow/part`.

## Как проверить

`css/css-shadow/part/exportparts-different-scope.html`, `animation-part.html`, `interaction-with-placeholder.html` и тесты с `::before` в shadow-листе.
