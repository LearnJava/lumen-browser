# BUG-413 — движковый `#printOverlay` ничего не печатает и не связан с настройками печати

**Статус:** OPEN
**Компонент:** chrome (`assets/chrome/chrome.html` `#printOverlay`,
`ChromeModel`), shell (`crates/shell/src/panels/print_panel.rs`,
`chrome_model_snapshot`, `dispatch_chrome_action`)
**Найден:** P1, CC-15-6 (2026-07-28), при удалении легаси-хит-теста печати

## Симптом

`Ctrl+P` открывает движковый `#printOverlay` (в модель переносится ровно один
бит — `print_open: self.print_panel.visible`). Дальше:

* обе кнопки подвала ассета — `<button data-action="close-modal">Отмена</button>`
  и `<button class="primary" data-action="close-modal">Печать</button>` — просто
  закрывают модалку. **Печать из UI не запускается вообще**; единственные живые
  пути в PDF — CLI `--print-to-pdf` и JS `window.print()`
  (`do_print_to_pdf_with_opts` с захардкоженными `scale=100`, `backgrounds=true`);
* контролы в `.print-settings` (принтер, страницы, ориентация, масштаб, «Фон и
  графика», колонтитулы) — статическая разметка эталона: без `data-action`, без
  биндинга, значения никуда не читаются и не пишутся.

Из-за этого поля `PrintPanel::{paper, orientation, margins, scale, color_mode,
print_backgrounds}` и метод `margin_px()` остались без единого читателя, а
`PrintField::{PageRange, OutputPath}` — без конструктора, из-за чего мёртв и
клавиатурный путь редактирования полей (`handle_print_key` → `push_char`/
`pop_char` при `editing_field == None` — no-op). Всё перечисленное помечено
`#[allow(dead_code, reason = "BUG-413: …")]`: данные и дефолты сохранены для
переноса, удалять их до реализации нельзя.

Регрессия флипа CC-14 (не CC-15-6): легаси-панель печати перестала рисоваться
уже тогда, CC-15-4 удалила её покраску, CC-15-6 — хит-тест. Проявляется у всех,
кроме тех, кто выставлял `LUMEN_LEGACY_CHROME=1` (флаг удалён этим же срезом).

## Что нужно сделать

1. Добавить действия в `assets/chrome/chrome.html` (`data-action="print-confirm"`,
   контролы настроек) и соответствующие варианты `ChromeAction`
   (`crates/chrome/build.rs` генерирует enum из ассета).
2. Расширить `ChromeModel` снапшотом настроек печати и связать его в
   `bind_model` (тем же приёмом, что `bind_find_bar`/`bind_downloads`).
3. В `dispatch_chrome_action` завести `print-confirm` на
   `do_print_to_pdf_with_opts` с реальными `margin_px()`/`scale`/
   `print_backgrounds` вместо текущих констант.
4. Снять `#[allow(dead_code)]` с полей `PrintPanel` и `PrintField`.

## Связанные

* [BUG-408](BUG-408-OPEN.md), [BUG-409](BUG-409-OPEN.md), [BUG-410](BUG-410-OPEN.md),
  [BUG-411](BUG-411-OPEN.md) — тот же класс: функциональность легаси-хрома, не
  перенесённая в движковый, вскрытая срезами CC-15-3/4.
* [BUG-414](BUG-414-OPEN.md) — тот же пробел в `#view-settings`.
* CC-10 (`docs/tasks/p1-css-chrome.md`) — срез, где `#printOverlay` получил
  только флаг открытости.
