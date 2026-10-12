# BUG-1611 — Esc на странице закрывает весь браузер без сохранения сессии

**Статус:** FIXED 2026-10-12 (Esc → KeyCommand::StopLoading: только останавливает загрузку, окно не закрывает)
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell (`crates/shell/src/input/keybindings.rs:172`, `lumen/keyboard.rs:674`)

## Симптом и место в коде

input/keybindings.rs:172 сопоставляет Escape без модификаторов с KeyCommand::Exit; в lumen/keyboard.rs:673 Esc при идущей загрузке её останавливает, иначе :674 вызывает event_loop.exit(). Путь минует beforeunload и сохранение сессии: их делает только WindowEvent::CloseRequested (app/mod.rs:236-240), on_exiting сессию не пишет (app/mod.rs:61). Esc перехватывают раньше только открытые панели, диалоги, полноэкранный режим, pointer lock и меню (keyboard.rs:31-301), поэтому на обычной загруженной странице одиночное нажатие Esc закрывает браузер. Существующего бага нет.

## Где смотреть

- crates/shell/src/input/keybindings.rs:172
- crates/shell/src/lumen/keyboard.rs:673
- crates/shell/src/lumen/keyboard.rs:674
- crates/shell/src/app/mod.rs:236
- crates/shell/src/app/mod.rs:61

## Сюда же: Esc закрывает весь браузер без сохранения сессии

Часть требований выполнена. Backspace вне поля ни к чему не привязан: в keybinding_for (crates/shell/src/input/keybindings.rs:164-288) его нет, «Назад» — только Alt+← (:181). Ctrl+Q не привязан, Ctrl+Shift+Q открывает панель доступности (:264-266), а не выход. Но Esc без модификаторов привязан к KeyCommand::Exit (keybindings.rs:172), и это вызывает event_loop.exit() (lumen/keyboard.rs:674). Исключение одно: во время загрузки Esc останавливает её (:673). Esc доходит до этой таблицы и при фокусе в поле ввода: ветки полей (keyboard.rs:302-466) ловят только печатные символы, Backspace, Delete и стрелки. Выход проходит мимо beforeunload и не записывает сессию: сессия сохраняется только в CloseRequested (app/mod.rs:234-241), а on_exiting (:61-74) её не пишет. Поведение закреплено тестом keybinding_escape_exit (crates/shell/src/tests/page_resources.rs:1498) и комментарием в panels/focus_panel.rs:10 («instead of quitting the app»).

## Объём и приёмка

Esc без модификаторов на загруженной странице закрывает весь браузер: keybindings.rs:172 Escape → KeyCommand::Exit, keyboard.rs:674 → event_loop.exit() — мимо beforeunload и сохранения сессии (их делает только CloseRequested, app/mod.rs:236-240; on_exiting сессию не пишет, app/mod.rs:61). Одиночная клавиша теряет все вкладки и несохранённые формы. Ожидаемо: Esc вне полей ничего не закрывает (останавливает загрузку, если она идёт), выход — только через закрытие окна. Приёмка: страница загружена, Esc ×10 → окно открыто, вкладки на месте. Баг для BUGS.md (P3); срочность — на усмотрение пользователя. docs/research/browser-ux/report.md §4.2 INPUT-07, §9 №13

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
