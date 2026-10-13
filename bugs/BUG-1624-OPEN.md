# BUG-1624 — Клик в страницу не снимает фокус с адресной строки

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell (`lumen/click.rs`, `lumen/keyboard.rs:47`, `lumen/text_input.rs:185`)

## Симптом и место в коде

Адресная строка закрывается только через Esc, Enter/выбор подсказки или загрузку (address_bar.close вызывается в omnibox_bar.rs:32, address_bar.rs commit, page_load.rs:1254/2364); путь клика по странице (lumen/click.rs, app/window_event/mouse_input.rs) её не закрывает. Клик в поле страницы ставит focused_node, а handle_key продолжает отдавать все клавиши строке (keyboard.rs:47-50): пользователь видит фокус и каретку в поле, а символы дописываются в URL. Каретка поля рисуется без проверки строки (focused_input_caret, text_input.rs:185), каретка строки — тоже (redraw_requested.rs:637), на экране две каретки (нарушение CARET-01). Проверено чтением кода.

## Где смотреть

- crates/shell/src/lumen/keyboard.rs:47
- crates/shell/src/lumen/text_input.rs:185
- crates/shell/src/app/window_event/redraw_requested.rs:637
- crates/shell/src/chrome_ui.rs:1156

## Объём и приёмка

Строка и фокус страницы не исключают друг друга: клик в поле страницы при открытой строке её не закрывает (в click.rs/mouse_input.rs нет address_bar.close), а handle_key отдаёт клавиши строке первой (keyboard.rs:47) — фокус виден в поле, текст уходит в URL; каретка страницы рисуется независимо от строки (focused_input_caret, text_input.rs:185) — на экране две каретки. Ожидаемо: клик в страницу снимает фокус со строки, каретка одна. Приёмка: Ctrl+L, клик в input страницы, набор «abc» → «abc» в поле, строка закрыта, видна одна каретка. Баг для BUGS.md (P3). docs/research/browser-ux/report.md §4.1 CARET-01, §4.2 INPUT-01

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
