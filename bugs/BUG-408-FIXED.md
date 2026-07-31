# BUG-408 — панель авто-архива вкладок недостижима под движковым хромом

**Статус:** FIXED 2026-07-31 (P1)
**Компонент:** shell (`crates/shell/src/tabs/archive.rs`, `crates/shell/src/main.rs`), chrome (`crates/chrome/src/model.rs`, `assets/chrome/chrome.html`, `docs/design/lumen-v3_3.html`)
**Найден:** P1, CC-15-3 (2026-07-28), при вырезании легаси-покраски таб-бара

## Симптом

Авто-архив (7A.5) продолжает работать: фоновая вкладка, простоявшая > 12 ч
(`ARCHIVE_AFTER_MS`), закрывается и её `ArchivedTab { id, title, url, container }`
кладётся в `TabArchive` (`main.rs`, блок авто-архивации в `about_to_wait`).

Единственным входом в UI архива была кнопка в правых 36 px легаси-таб-бара
(`archive::build_button` + `archive::hit_test_button` → `TabArchive::toggle()`), а
единственным рендером списка — `archive::build_panel`. Оба жили внутри
легаси-гейта `!self.focus.active && !self.css_chrome_enabled`. С флипа дефолта
(CC-14, 2026-07-28) этот гейт закрыт у всех, кроме `LUMEN_LEGACY_CHROME=1`, а
`assets/chrome/chrome.html` эквивалента не содержит — в движковом хроме архива нет
ни кнопки, ни панели, ни `data-action`.

Итог: вкладка молча исчезает из полосы через 12 ч, и восстановить её через UI
нельзя. `archive.visible` больше нечем выставить в `true`, поэтому
негейтированный `hit_test_panel` (`main.rs`, путь «клик вне панели») тоже никогда
не срабатывает.

Регрессия появилась на флипе CC-14, а не в CC-15-3: под дефолтным хромом
легаси-блок уже не рисовался и клики в его область уходили в `chrome_hit_test`.
CC-15-3 лишь удалила уже мёртвый код и тем самым сделала пробел видимым —
`ArchivedTab::title`/`::container` остались без читателей (помечены
`#[allow(dead_code, reason = "BUG-408: …")]`, данные по-прежнему записываются).

## Что нужно сделать

Перенести архив в движковый хром по образцу уже перенесённых панелей (CC-10/CC-10b):
разметка в `assets/chrome/chrome.html`, модель в `lumen_chrome::model` (список строк
с `title`/`url`/цветом контейнера), биндинг в `bind_model`, `data-action` для
toggle/restore/dismiss в `ChromeAction` + `dispatch_chrome_action`. После этого
снять оба `#[allow(dead_code)]` в `tabs/archive.rs`.

Промежуточный, более дешёвый вариант, если полный перенос не влезает в срез:
повесить toggle архива на команду палитры/горячую клавишу, чтобы восстановление
вкладок хотя бы не требовало `LUMEN_LEGACY_CHROME=1` — но это оставит панель без
рендера, поэтому годится только вместе с движковым списком.

## Фикс (2026-07-31, P1)

Выбран полный перенос, по образцу CC-9/CC-10 (`#downloadsPanel`). Замороженный
эталон (`docs/design/lumen-v3_3.html`) не содержал вообще никакой архивной
разметки — единственный существующий хук, `.nt-restore` («Восстановить
закрытые») на `about:newtab`, был декоративным (`onclick` отсутствовал). Правило
генератора «изменения только через реф + перегенерация» (см. `CC-13` в
`docs/tasks/p1-css-chrome.md`) соблюдено: сама разметка добавлена в эталон, а
не в `assets/chrome/chrome.html` напрямую.

1. **Эталон** (`docs/design/lumen-v3_3.html`): новая кнопка тулбара
   `#archiveToggleBtn` (иконка `i-clock`, рядом с `#dlToggleBtn`), `.nt-restore`
   получил реальный `onclick="toggleArchive()"`, и новая панель
   `#archivePanel`/`.arc-list`/`.arc-card` (структура 1:1 с
   `.downloads-panel`/`.dl-list`/`.dl-card`, плюс `.arc-stripe` — левая цветная
   полоска контейнера, тот же приём что `.container-stripe` у вкладок) с одной
   демо-строкой для CSS-гейта. Новый CSS-блок `.archive-panel`/`.arc-*`.
2. **`scripts/gen_chrome_assets.py`**: три новых `onclick`→`data-action`
   маппинга (`toggleArchive()`→`toggle-archive`, `archiveRestore(this)`→
   `archive-restore`, `archiveDismiss(this)`→`archive-dismiss`) + два новых
   `ARIA_LABEL_RULES` для иконка-only кнопок ряда (restore/dismiss); `toggle-archive`
   переиспользует то же «неточное имя лучше пустого» решение, что
   `toggle-downloads`/`toggle-find` уже приняли для своих собственных
   close-кнопок (`.arc-close` получает aria-label «Архив вкладок», а не «Закрыть»).
3. **`crates/chrome/src/model.rs`**: `ChromeModel::{archive_open, archive}` +
   `ChromeArchiveEntryModel{id, fav_letter, title, url, container_color}` +
   `bind_archive`/`build_arc_card` — рёбра `data-archive-id`/`data-action`
   ставятся на сами кнопки restore/dismiss (своя копия, не только на строку),
   тот же приём что `.tab-close`'s `data-tab-id`.
4. **`crates/shell/src/main.rs`**: `chrome_model_snapshot` проецирует
   `self.archive.{visible,entries}` в модель; `dispatch_chrome_action` получил
   `ChromeAction::ToggleArchive` (`self.archive.toggle()` — новый метод,
   мирроring `DownloadManager::toggle_visible`), `ArchiveRestore`/`ArchiveDismiss`
   (читают `data-archive-id`, переиспользуют существующую логику `archive.take`/
   `navigate_to(PageSource::Url(...))`, которая уже жила в легаси-геометрическом
   click-обработчике — эта старая ветка теперь на практике недостижима, так как
   `self.archive.visible` включается только через новый движковый путь).
5. **`crates/shell/src/tabs/archive.rs`**: `ArchivedTab::{title,container}`
   расчехлены (`#[allow(dead_code)]` снят — оба поля читает
   `chrome_model_snapshot`); добавлен `TabArchive::toggle()`.

Легаси `hit_test_panel` (пиксельная геометрия, `toolbar::CHROME_H`) осталась
нетронутой — она не гейтирована по `css_chrome_enabled` (см. BUG-404), но
поскольку `archive.visible` теперь взводится только через новый движковый
путь, а не через удалённую в CC-15-3 легаси-кнопку, класс риска BUG-404 для
этого конкретного вызывающего места не открывается заново: единственный живой
писатель в `archive.visible` — `ChromeAction::ToggleArchive`.

Верификация: `cargo test -p lumen-chrome`, `cargo test -p lumen-shell`,
`python graphic_tests/run.py --continue-on-fail` (новая разметка хрома может
двигать пиксели).

## Связанные

* [BUG-403](bugs/BUG-403-FIXED.md), [BUG-404](bugs/BUG-404-OPEN.md) — тот же корень
  (пробелы паритета, не пойманные чек-листом CC-14), другие вызывающие места.
* [BUG-409](bugs/BUG-409-OPEN.md), [BUG-410](bugs/BUG-410-OPEN.md) — соседние
  непереносённые куски того же легаси-таб-бара/омнибокса, найдены тем же срезом.
* CC-15-3 (`ROADMAP.md`, `docs/tasks/p1-css-chrome.md` §CC-15) — срез, вскрывший пробел.
