# BUG-422 — `#view-history`/`#view-bookmarks` не поддерживают ни одного действия над записями

**Статус:** OPEN
**Компонент:** chrome (`assets/chrome/chrome.html` `#view-history`,
`#view-bookmarks`), shell (`crates/shell/src/panels/history_panel.rs`,
`panels/bookmark_panel.rs`, `dispatch_chrome_action`)
**Найден:** P1, CC-15-6 (2026-07-28), при удалении легаси-хит-тестов панелей

## Симптом

Легаси-оверлеи истории и закладок были интерактивными: `HistoryHit`
(`Navigate(url)` · `Delete(id)` · `ClearAll` · `FocusSearch` · `Close`) и
`BookmarkHit` (`Bookmark(id)` · `DeleteBookmark(id)` · `SelectFolder(folder)` ·
`FocusSearch` · `Close`) плюс drag-and-drop перекладывания закладки в папку
(`begin_drag` → `finish_bookmark_drop` → `Bookmarks::set_folder`).

В движковых представлениях действий над записями нет вовсе: грep `data-action`
внутри `#view-history` даёт пустой список, внутри `#view-bookmarks` — только
`archive-card` и `set-settings-section` (навигационные, к записям не относятся).
То есть под дефолтным хромом:

* по записи истории/закладке нельзя перейти кликом;
* нельзя удалить запись истории или закладку, нельзя очистить историю целиком;
* нельзя отфильтровать закладки по папке и нельзя перенести закладку в папку;
* поиск внутри панелей всё ещё работает — но только с клавиатуры
  (`append_search`/`backspace_search` привязаны к key-обработчику, не к клику по
  полю).

Состояние панелей (`entries`, `folders`, `selected_folder`, `search`,
`scroll_y`) живо и читается `chrome_model_snapshot` — потеряны именно действия.

Регрессия флипа CC-14 (не CC-15-6): легаси-панели перестали рисоваться уже
тогда, CC-15-4 удалила их покраску, CC-15-6 — хит-тесты, `bookmark_anchor`,
`history_panel_anchor`, `finish_bookmark_drop` и всю drag-механику
(`BookmarkPanel::{drag, begin_drag, take_drag}`).

## Что нужно сделать

1. Разметить строки истории/закладок в `assets/chrome/chrome.html`
   (`data-action="open-history-entry"`/`"delete-history-entry"`/
   `"clear-history"`/`"open-bookmark"`/`"delete-bookmark"`/`"select-folder"`)
   с идентификатором записи в `data-*`, регенерировать через
   `scripts/gen_chrome_assets.py`.
2. Обработать новые `ChromeAction`-варианты в `dispatch_chrome_action`, читая id
   из атрибута (тем же приёмом, что `data-dl-id` у `#downloadsPanel` и
   `data-tab-id` у вкладок).
3. Drag-and-drop перекладывания закладки — отдельно и позже: в движковом хроме
   нет источника перетаскивания, потребуется pointer-механика поверх DOM хрома.

## Связанные

* [BUG-408](BUG-408-OPEN.md) — панель архива вкладок, тот же класс (доступ к
  функции потерян полностью).
* [BUG-420](BUG-420-OPEN.md), [BUG-421](BUG-421-OPEN.md) — соседние находки того
  же среза: печать и настройки.
* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#view-history`/
  `#view-bookmarks` получили отображение списков, но не действия над ними.
