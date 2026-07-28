# BUG-412 — `#findCount` не выделяет невалидный regex-паттерн цветом

**Статус:** OPEN
**Компонент:** chrome (`assets/chrome/chrome.html` `#findCount`/`.find-count`,
`ChromeFindModel::count_label`), shell (`chrome_model_snapshot`)
**Найден:** P1, CC-15-6 (2026-07-28), при удалении легаси find-bar overlay

## Симптом

Легаси find-bar (`find::append_bar`, удалён CC-15-6) в regex-режиме проверял
паттерн через `find::is_valid_regex_pattern` и при невалидном показывал вместо
счётчика строку `ERR` **красным** (`BAR_ERR = rgb(255, 80, 80)`), отличая её от
обычного «ничего не найдено» (`0/0`, приглушённый `BAR_DIM`).

Движковый `#findBar` (CC-9) переносит из легаси-бара только текст счётчика
(`ChromeFindModel::count_label` → `#findCount`). CC-15-6 перенесла **текст**
`ERR` в `count_label`, чтобы невалидный паттерн не выглядел как «0 совпадений»,
но цветового различия нет: `.find-count` в `assets/chrome/chrome.html` имеет
единственный стиль `color:var(--text-secondary)`, класса ошибки в ассете нет.

Косметика: информация о невалидном паттерне доступна (текст `ERR`), потерян
только цветовой акцент.

## Что нужно сделать

Добавить в `ChromeFindModel` признак ошибки (`error: bool` вместо/помимо
разбора `count_label == "ERR"`), в `assets/chrome/chrome.html` — правило
`.find-count.error{ color:var(--danger) }` (генератор `scripts/gen_chrome_assets.py`),
в `bind_find_bar` — переключение класса через уже существующий `set_class_token`
(тем же приёмом, что `.open` у `#findBar`).

## Связанные

* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#findBar` получил
  `value`/`count_label`, но не состояние ошибки.
* CC-15-6 (`ROADMAP.md`) — срез, вскрывший пробел и перенёсший текстовую часть.
* [BUG-410](BUG-410-OPEN.md) — тот же класс «модель CC-9/CC-10 переносит не всё,
  что рисовал легаси-виджет».
