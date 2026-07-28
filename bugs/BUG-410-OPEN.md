# BUG-410 — строка dropdown омнибокса потеряла текстовый тег типа

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs::chrome_model_snapshot`,
`crates/shell/src/address_bar.rs`), chrome (`ChromeSuggestionModel`,
`assets/chrome/chrome.html` `#omniDropdown`)
**Найден:** P1, CC-15-3 (2026-07-28), при удалении легаси-`build_dropdown`

## Симптом

Легаси-dropdown рисовал в правой части каждой строки текстовый тег типа подсказки
(`OmniboxSuggestion::tag()`): «история» · «заметка» · «позже» · «вкладка» ·
«закладка» · «ai» · «запрос», причём для ранее вводившегося поискового запроса
вместо «запрос» показывался счётчик использований `×N` при `frequency > 1`.

`ChromeSuggestionModel` (CC-9) переносит только `idx`/`label`/`sub_label`/`color`
(цвет `.dd-icon`-плашки). Текстового тега в модели нет, в `#omniDropdown` — тоже.
С флипа дефолта (CC-14) пользователь различает тип подсказки только по цвету
плашки, а частота запроса не показывается вообще.

CC-15-3 удалила уже мёртвый `tag()` вместе с остальным легаси-рендером; из-за
этого поле `OmniboxSuggestion::SearchQuery::frequency` осталось без читателей
(помечено `#[allow(dead_code, reason = "BUG-410: …")]` — значение по-прежнему
берётся из `SearchHistory::prefix_match`, данные сохранены для переноса).

Приоритет ниже, чем у [BUG-408](bugs/BUG-408-OPEN.md)/[BUG-409](bugs/BUG-409-OPEN.md):
здесь потеряна подпись, а не доступ к функции.

## Что нужно сделать

Вернуть `tag()` (или его эквивалент, считающий строку из варианта подсказки +
`frequency`), добавить поле `tag: String` в `ChromeSuggestionModel`, элемент
`.dd-tag` в `#omniDropdown` (`assets/chrome/chrome.html`) и биндинг в `bind_model`.
После этого снять `#[allow(dead_code)]` с `frequency`.

## Связанные

* [BUG-408](bugs/BUG-408-OPEN.md), [BUG-409](bugs/BUG-409-OPEN.md) — соседние
  непереносённые куски легаси-хрома, найдены тем же срезом.
* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#omniDropdown` получил
  `label`/`sub_label`/цвет, но не тег.
* CC-15-3 (`ROADMAP.md`) — срез, вскрывший пробел. Тот же срез исправил вторую,
  более серьёзную находку в этом же снапшоте — DS-6 punycode-guard не применялся
  к `label`/`sub_label` строк dropdown (`address_bar::chrome_suggestion_text`).
