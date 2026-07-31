# BUG-410 — строка dropdown омнибокса потеряла текстовый тег типа

**Статус:** FIXED 2026-07-31 (P1)
**Компонент:** shell (`crates/shell/src/main.rs::chrome_model_snapshot`,
`crates/shell/src/address_bar.rs`), chrome (`crates/chrome/src/model.rs::ChromeSuggestionModel`,
`assets/chrome/chrome.html` `#omniDropdown`, `docs/design/lumen-v3_3.html`)
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

## Фикс (2026-07-31, P1)

Восстановлен `OmniboxSuggestion::tag()` (удалённый CC-15-3 вместе с легаси-рендером,
тот же текст: «история»/«заметка»/«позже»/«вкладка»/«закладка»/«ai», `×N` для
`SearchQuery` при `frequency > 1`, иначе «запрос»). `#[allow(dead_code)]` на
`SearchQuery::frequency` снят — поле снова читается.

`ChromeSuggestionModel` получил поле `tag: String`; `chrome_model_snapshot` заполняет
его вызовом `s.tag()` рядом с уже существующим `tag_color()`. `build_dd_row` добавляет
`<span class="dd-tag">` третьим элементом строки (после `.dd-icon`/`.dd-text`).

Разметка и `.dd-tag` CSS (`font-size:10.5px; color:var(--text-secondary); flex:none;
white-space:nowrap;`) добавлены в замороженный эталон (`docs/design/lumen-v3_3.html`,
по одному тегу на каждую из 5 демо-строк `#omniDropdown`) и перенесены в
`assets/chrome/chrome.html` через `scripts/gen_chrome_assets.py`.

Тесты: `crates/shell/src/address_bar.rs` — `search_query_suggestion_tag_is_generic_label_below_frequency_two`,
`search_query_suggestion_tag_shows_use_count_from_frequency_two`, `tab_suggestion_tag_is_kind_label`;
`crates/chrome/src/model.rs::dropdown_is_rebuilt_from_suggestions_and_toggles_open` расширен
проверкой текста `.dd-tag`. `cargo clippy -p lumen-chrome -p lumen-shell --all-targets -- -D warnings`
чист.

## Связанные

* [BUG-408](bugs/BUG-408-OPEN.md), [BUG-409](bugs/BUG-409-OPEN.md) — соседние
  непереносённые куски легаси-хрома, найдены тем же срезом.
* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#omniDropdown` получил
  `label`/`sub_label`/цвет, но не тег.
* CC-15-3 (`ROADMAP.md`) — срез, вскрывший пробел. Тот же срез исправил вторую,
  более серьёзную находку в этом же снапшоте — DS-6 punycode-guard не применялся
  к `label`/`sub_label` строк dropdown (`address_bar::chrome_suggestion_text`).
