# BUG-422 — `#view-history`/`#view-bookmarks` не поддерживают ни одного действия над записями

**Статус:** FIXED 2026-08-20
**Компонент:** chrome (`docs/design/lumen-v3_3.html` → `assets/chrome/chrome.html`
`#view-history`/`#view-bookmarks`, `crates/chrome/src/model.rs`
`bind_history`/`bind_bookmarks`), shell (`dispatch_chrome_action`)
**Найден:** P1, CC-15-6 (2026-07-28), при удалении легаси-хит-тестов панелей
**Исправлен:** P3, 2026-08-20

## Симптом

Легаси-оверлеи истории и закладок были интерактивными: `HistoryHit`
(`Navigate(url)` · `Delete(id)` · `ClearAll` · `FocusSearch` · `Close`) и
`BookmarkHit` (`Bookmark(id)` · `DeleteBookmark(id)` · `SelectFolder(folder)` ·
`FocusSearch` · `Close`) плюс drag-and-drop перекладывания закладки в папку
(`begin_drag` → `finish_bookmark_drop` → `Bookmarks::set_folder`).

В движковых представлениях действий над записями не было вовсе: грep
`data-action` внутри `#view-history` давал пустой список, внутри
`#view-bookmarks` — только `archive-card` и `set-settings-section`
(навигационные, к записям не относятся). То есть под дефолтным хромом:

* по записи истории/закладке нельзя перейти кликом;
* нельзя удалить запись истории или закладку, нельзя очистить историю целиком;
* нельзя отфильтровать закладки по папке;
* поиск внутри панелей всё ещё работает — но только с клавиатуры.

Регрессия флипа CC-14 (не CC-15-6): легаси-панели перестали рисоваться уже
тогда, CC-15-4 удалила их покраску, CC-15-6 — хит-тесты, `bookmark_anchor`,
`history_panel_anchor`, `finish_bookmark_drop` и всю drag-механику.

## Что сделано

Восемь новых `data-action`, все с реальным бэкендом:

| `data-action` | Элемент | Обработчик |
|---|---|---|
| `open-history-entry` | `.hist-item` | `navigate_to(PageSource::Url(…))` |
| `bookmark-history-entry` | `.hist-actions` звезда | `Bookmarks::add` (upsert по URL) |
| `copy-history-entry` | `.hist-actions` копия | `PlatformClipboard::write_text` |
| `delete-history-entry` | `.hist-actions` корзина | `History::delete` |
| `clear-history` | `.hist-head` «Очистить» | `History::clear` |
| `open-bookmark` | `.bm-card` | `navigate_to(PageSource::Url(…))` |
| `delete-bookmark` | новая `.bm-actions` корзина | `Bookmarks::delete` |
| `select-folder` | `.bm-folder` | `BookmarkPanel::selected_folder` |

Правка идёт в эталон `docs/design/lumen-v3_3.html` + регенерация
`scripts/gen_chrome_assets.py` (правило 4 `docs/tasks/p1-css-chrome.md`), а не
руками в ассете: `ChromeAction` генерируется `build.rs`-ом из значений
`data-action` **статической** разметки ассета, поэтому нового варианта без
правки эталона не появилось бы вовсе. Разметка `.hist-actions` (звезда/копия/
корзина) в эталоне уже была — ей добавлены `onclick`; для закладок добавлен
новый узел `.bm-actions` с одной кнопкой и три строки CSS, собранных из уже
поддерживаемых конструкций (`position:absolute` + `opacity` + `:hover`-потомок
— ровно то, чем живёт соседний `.hist-actions`), поэтому парс-гейт `build.rs`
не трогался.

### Почему URL, а не id записи

Контекст клика едет в `data-hist-url`/`data-bm-url`/`data-bm-folder`, а не в
целочисленном id (как `data-tab-id`/`data-dl-id` у вкладок и загрузок).
Причины две: и `History::delete`, и `Bookmarks::{add,delete}` принимают URL, а
не id; и `Lumen::refresh_history` в FTS-ветке (когда в панели активен поиск)
**фабрикует** `HistoryItem::id` из позиции результата — по такому id удалилась
бы чужая запись. Индекс строки не годится по той же причине. Новый
`Lumen::chrome_data_attr` — строковый близнец `chrome_data_id`, отдаёт
`String` (а не `&str`), потому что каждый обработчик сразу мутирует `self`.

### Почему кнопка внутри строки не конфликтует со строкой

`.hist-item`/`.bm-card` несут собственный `data-action` (открыть), а кнопки
внутри — свой. Конфликта нет: `Lumen::chrome_action_at` идёт по `hit.path`
от ближайшего узла к корню и берёт первый распознанный `data-action`, поэтому
кнопка всегда «затеняет» строку. Тот же приём уже работает у `.tab-close`
внутри `.tab-row`.

Строки истории/карточки закладок целиком удаляются и пересобираются
`bind_history`/`bind_bookmarks` на каждом релэйауте, поэтому `data-action` и
контекстные атрибуты ставятся в Rust (`build_hist_item`/`build_bm_card`), а не
достаются из статической разметки. Побочно `build_hist_item` впервые собирает
не букву-заглушку, а настоящую иконку спрайта (`<svg class="icon"><use
href="#i-…"/></svg>`, новый `append_icon`) — у этих кнопок нет текста вообще,
буква была бы бессмысленна. `aria-label` дублирует то, что
`gen_chrome_assets.py::ARIA_LABEL_RULES` вписывает в статические копии тех же
кнопок, так что дерево доступности одинаково до и после первого биндинга.

Открытие записи заодно возвращает `#contentArea` на `#view-page`: четыре view
взаимоисключающие, и без этого страница грузилась бы **за** списком, который
остаётся на экране. Зеркалит ветку `"page"` у `ShowView`.

### Что сознательно не сделано

* **Drag-and-drop перекладывания закладки в папку** — как и указано в исходной
  заявке (п. 3 «отдельно и позже»): в движковом хроме нет источника
  перетаскивания, нужна pointer-механика поверх DOM хрома, это отдельная
  задача, а не тот же по форме фикс, что остальные восемь действий.
* **Клик по полю поиска** (`.hist-search input`) — тот же класс пробела, что
  `#omniInput`/`#findInput`: редактирование текста в движковом хроме живёт
  отдельным гибридным путём (CC-7/CC-9), а не через `data-action`. Поиск
  по-прежнему работает с клавиатуры.
* **`.hist-head` «Даты»/«Экспорт»** — под ними нет состояния вообще (нет ни
  фильтра по датам, ни экспортёра истории), то же honesty-over-fabrication
  решение, что у BUG-420/421.

## Проверка

* `cargo test -p lumen-chrome` — 79/79, из них 3 новых
  (`bug422_history_rows_carry_open_and_per_row_actions`,
  `bug422_history_row_action_buttons_carry_the_sprite_icon`,
  `bug422_bookmark_folders_and_cards_carry_actions`).
* `cargo test -p lumen-shell` — 1578/1578.
* `cargo clippy -p lumen-chrome -p lumen-shell --all-targets -D warnings` —
  зелёный; `python scripts/gen_chrome_assets.py --check` — без дрейфа.
* Геометрия новых узлов снята `--dump-layout` по ассету с принудительно
  активной view: `.hist-actions` — `(908, …, 82, 26)` (3×26 + 2×2 gap, правый
  край полосы), `.bm-actions` — `26×26`, `position=absolute`, `opacity=0.000`,
  отступ 6 px + 1 px рамки от правого-нижнего угла карточки; `<use
  href="#i-trash">` разворачивается в два `SvgShape` — иконка настоящая, а не
  пустой `<svg>`.

Интерактивная проверка клика в живом окне не гонялась — тот же пробел, что
документировали CC-5/6/7 и BUG-420/421: инструмента, умеющего кликать по
хрому (а не по странице), в репозитории нет; MCP/BiDi `click` адресует
страничный документ.

## Связанные

* [BUG-408](BUG-408-FIXED.md) — панель архива вкладок, тот же класс.
* [BUG-420](BUG-420-FIXED.md), [BUG-421](BUG-421-FIXED.md) — соседние находки
  того же среза: печать и настройки.
* [BUG-426](BUG-426-FIXED.md) — оставшиеся `data-action`-заглушки; `archive-card`
  на `.bm-card.readlater` по-прежнему мёртв (карточки этого варианта
  `bind_bookmarks` стирает), фикс его не воскрешает.
* CC-9 (`docs/tasks/p1-css-chrome.md`) — срез, где `#view-history`/
  `#view-bookmarks` получили отображение списков, но не действия над ними.
