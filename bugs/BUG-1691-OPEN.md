# BUG-1691 — Псевдоэлементы в <style> внутри shadow tree не стилизуются

**Статус:** OPEN
**Заведён:** 2026-10-23 (P1, SHADOW-PARTS)
**Область:** layout (`crates/engine/layout/src/style/pseudo.rs` — `compute_pseudo_element_style` берёт только лист документа)

## Симптом

Псевдоэлементы (`::before`, `::after`, `::placeholder`, `::file-selector-button`) в `<style>` внутри shadow tree не стилизуются: `compute_pseudo_element_style` смотрит правила листа документа, а не листа корня тени элемента. Тесты `css/css-shadow/part` с `::before` в shadow-листе (часть `part/*`) остаются красными.

## Проба

`css/css-shadow/part/*` в `python tests/wpt/run_corpus.py --prefixes css/css-shadow/part`: после SHADOW-PARTS 41 из 48 зелёные, остаток — этот дефект и `::cue`/часть псевдоклассов в `test_valid_selector`.

## Как найдено

SHADOW-PARTS, прогон `css/css-shadow/part`.

## Что делать

Устранить причину в указанной области; затем перепрогнать `css/css-shadow/part`.

## Как проверить

`css/css-shadow/part/exportparts-different-scope.html`, `animation-part.html`, `interaction-with-placeholder.html` и тесты с `::before` в shadow-листе.
