# BUG-420 — движковый `#printOverlay` ничего не печатает и не связан с настройками печати

**Статус:** FIXED 2026-08-01
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
`#[allow(dead_code, reason = "BUG-420: …")]`: данные и дефолты сохранены для
переноса, удалять их до реализации нельзя.

Регрессия флипа CC-14 (не CC-15-6): легаси-панель печати перестала рисоваться
уже тогда, CC-15-4 удалила её покраску, CC-15-6 — хит-тест. Проявляется у всех,
кроме тех, кто выставлял `LUMEN_LEGACY_CHROME=1` (флаг удалён этим же срезом).

## Закрыт 2026-08-01 (P1)

Печать из UI теперь запускает реальный экспорт PDF; из шести контролов
`.print-settings` реально подключены два — «Ориентация» (чистое 1:1
соответствие `Книжная`/`Альбомная` ↔ `Orientation::Portrait`/`Landscape`) и
«Фон и графика» (checkbox ↔ `PrintPanel::print_backgrounds`), плюс сама
кнопка «Печать»:

1. Эталон (`docs/design/lumen-v3_3.html`, регенерируется
   `scripts/gen_chrome_assets.py`): кнопка «Печать» получила
   `onclick="confirmPrint()"` → `data-action="print-confirm"`; `<select
   id="printOrientationSelect">` — `onclick="cyclePrintOrientation()"` →
   `data-action="cycle-print-orientation"`; чекбокс «Фон и графика» обёрнут в
   `<label onclick="togglePrintBackgrounds()">` → `data-action=
   "toggle-print-backgrounds"`, сам `<input>` получил `id="printBgCheck"`.
   Три записи добавлены в `ONCLICK_EXACT_ACTIONS` генератора.
2. `ChromeModel::print_open: bool` заменён на `ChromeModel::print:
   ChromePrintModel { open, landscape, backgrounds }`; новая `bind_print`
   двигает атрибут `selected` между двумя `<option>` `#printOrientationSelect`
   и атрибут `checked` на `#printBgCheck` — тот же механизм (DOM-атрибуты),
   которым `lumen_layout::box_tree` уже красит `<select>`/чекбоксы у
   обычного веб-контента (`crates/shell/src/forms.rs`).
3. `dispatch_chrome_action`: `CyclePrintOrientation` переключает
   `PrintPanel::orientation` (у контрола ровно 2 значения — клик просто
   инвертирует, полноценного попапа `<select>` в движковом хроме нет);
   `TogglePrintBackgrounds` инвертирует `print_backgrounds`; `PrintConfirm`
   — новый `Lumen::handle_print_confirm()`, вызывает
   `do_print_to_pdf_with_opts` с живыми `margin_px()`/`scale`/
   `print_backgrounds`/`landscape` вместо жёстких констант старого JS-пути.
   `do_print_to_pdf_with_opts` получил параметр `landscape: bool` — меняет
   местами ширину/высоту итогового растра страницы (не влияет на `scale`,
   который остаётся зумом контента внутри уже выбранного размера страницы).
4. Сняты `#[allow(dead_code)]` с полей, которые стали реально читаться:
   `PrintPanel::{orientation, margins, scale, print_backgrounds}`,
   `Orientation::Landscape`, `margin_px()`.

**Сознательно не тронуто** (следующий тем же классом, что и
honesty-over-fabrication в `ChromeSettingsModel`/`#statAds`): «Принтер»
(нет альтернативного бэкенда, кроме PDF-экспорта — нечего выбирать),
«Страницы» и «Масштаб» (в чистовом хроме нет клика-в-текстовое-поле/попапа
`<select>` — для «Масштаба» к тому же варианты эталона `По умолчанию`/`По
ширине`/`75%` не 1:1 ложатся на `PrintPanel::scale: i32`), чекбокс
«Колонтитулы» (нет соответствующего поля в `PrintPanel` вовсе). Их
`PaperSize::{Letter,Legal}`/`ColorMode::Grayscale`/`MarginPreset::{Narrow,
Wide}`/`PrintField::{PageRange,OutputPath}`/`PrintPanel::paper`/
`PrintPanel::color_mode` остаются под `#[allow(dead_code)]` — честно, ничего
их не читает.

## Связанные

* [BUG-408](BUG-408-FIXED.md), [BUG-409](BUG-409-FIXED.md), [BUG-410](BUG-410-FIXED.md) —
  тот же класс (уже закрыты): функциональность легаси-хрома, не
  перенесённая в движковый, вскрытая срезами CC-15-3/4.
* [BUG-411](BUG-411-OPEN.md) — тот же класс, ещё открыт.
* [BUG-421](BUG-421-OPEN.md) — тот же пробел в `#view-settings` (в том числе
  общая проблема неразличимых `data-action="toggle-switch"` — здесь не
  требовалась, у печати свои собственные `data-action`).
* CC-10 (`docs/tasks/p1-css-chrome.md`) — срез, где `#printOverlay` получил
  только флаг открытости.
