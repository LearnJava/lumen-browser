# BUG-1612 — Выход по Ctrl+W на последней вкладке и по Esc не сохраняет сессию

**Статус:** FIXED 2026-10-12
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell (`lumen/tabs_cmd.rs:219`, `app/mod.rs:239`, `lumen/session.rs`)

## Симптом и место в коде

Сессия сохраняется (save_session_on_close и save_full_session в lumen/session.rs:16, :50) только в трёх местах: при CloseRequested (app/mod.rs:239-240), при перезапуске для обновления (update_ui.rs:358-359) и после подтверждения диалога (dialogs.rs:123-124). Периодического автосохранения нет. Два других пути выхода сессию не пишут. Первый: close_tab на последней вкладке (Ctrl+W или × в вертикальной полосе) сразу вызывает event_loop.exit() (lumen/tabs_cmd.rs:219-223). Второй: Esc → KeyCommand::Exit (keyboard.rs:674). on_exiting (app/mod.rs:61-74) только печатает сводки. При следующем запуске restore_session (lumen/session.rs:106) поднимет набор вкладок, сохранённый при прошлом нормальном закрытии, то есть устаревший. Найдено при сверке.

## Где смотреть

- crates/shell/src/lumen/tabs_cmd.rs:219-223
- crates/shell/src/lumen/keyboard.rs:674
- crates/shell/src/app/mod.rs:239-240
- crates/shell/src/app/mod.rs:61-74
- crates/shell/src/update_ui.rs:358-359
- crates/shell/src/lumen/dialogs.rs:123-124
- crates/shell/src/lumen/session.rs:16
- crates/shell/src/lumen/session.rs:50
- crates/shell/src/lumen/session.rs:106

## Объём и приёмка

Выход не через крестик окна не пишет сессию: Ctrl+W на последней вкладке → event_loop.exit() (lumen/tabs_cmd.rs:219-223), Esc — то же (keyboard.rs:674); save_session_on_close/save_full_session зовутся только из CloseRequested (app/mod.rs:239-240), обновления (update_ui.rs:358-359) и диалога (dialogs.rs:123-124), автосохранения нет. Следующий запуск (restore_session, lumen/session.rs:106) поднимает устаревший набор вкладок. Исправление: один путь выхода с записью сессии. Приёмка: 3 вкладки, выход тремя Ctrl+W → last_session.db отражает состояние на момент выхода. docs/research/browser-ux/report.md §1 п.3; areas/05 «Выход, закрытие окна, сессия»

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.

## Исправление

Esc исправлен ранее в BUG-1611. Последняя вкладка: `close_tab` перед `event_loop.exit()` зовёт `save_session_on_close` и `save_full_session` (lumen/tabs_cmd.rs).
