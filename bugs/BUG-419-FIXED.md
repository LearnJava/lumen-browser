# BUG-419 — `#findCount` не выделяет невалидный regex-паттерн цветом

**Статус:** FIXED 2026-07-31
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

## Фикс (2026-07-31, P1)

`ChromeFindModel` получил поле `error: bool`; `main.rs::chrome_model_snapshot`
считает условие невалидного regex один раз (`find_is_error`) и пишет его и в
`count_label` (`"ERR"`), и в новое поле. `assets/chrome/chrome.html` получил
правило `.find-count.error{ color:var(--red-badge); }` (переиспользован
существующий токен, которым уже красились `.perm-btn.deny`/`.console-line.error`
— отдельного `--danger` в палитре ассета нет), добавлено в замороженный эталон
`docs/design/lumen-v3_3.html` и перегенерировано `gen_chrome_assets.py`.
`bind_find_bar` переключает класс через уже существующий `set_class_token`
(тот же приём, что `.open` у `#findBar`).

Пиксельно нейтрально в дефолтных состояниях (класс применяется только когда
find-бар открыт с невалидным regex — ни один графический тест этого состояния
не создаёт): `cargo test -p lumen-chrome` (69/69, 1 новый), `cargo clippy
-p lumen-chrome -p lumen-shell --all-targets -- -D warnings`, `python
scripts/gen_chrome_assets.py --check`, полный `graphic_tests/run.py
--continue-on-fail` (152 теста) — дельта vs main пустая.

## Связанные

* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#findBar` получил
  `value`/`count_label`, но не состояние ошибки.
* CC-15-6 (`ROADMAP.md`) — срез, вскрывший пробел и перенёсший текстовую часть.
* [BUG-410](BUG-410-FIXED.md) — тот же класс «модель CC-9/CC-10 переносит не всё,
  что рисовал легаси-виджет».
