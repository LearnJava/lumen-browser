# BUG-1627 — WM_GETOBJECT не доходит до моста MSAA, CAPABILITIES обещает обратное

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** a11y/shell (`a11y/platform/windows.rs:159`, `window_mode.rs:34`)

## Симптом и место в коде

CAPABILITIES.md:269 утверждает, что WM_GETOBJECT возвращает настоящий корень MSAA через CreateStdAccessibleObject. Метод WinUiaBridge::handle_wm_get_object есть (crates/engine/a11y/src/platform/windows.rs:159), но shell его не вызывает: подключается только init_hwnd (app/resumed.rs:62), а hook оконных сообщений у event loop нет (window_mode.rs:34-51). Сообщение до моста не доходит, обещание в CAPABILITIES не выполняется. Отсутствие того же hook не даёт принять WM_SETTINGCHANGE (UX-OSPARAMS) и мешает сообщать ОС системную каретку (UX-CARET, CARET-10).

## Где смотреть

- CAPABILITIES.md:269
- crates/engine/a11y/src/platform/windows.rs:159
- crates/shell/src/app/resumed.rs:62
- crates/shell/src/window_mode.rs:34

## Объём и приёмка

CAPABILITIES.md:269 утверждает, что WM_GETOBJECT отдаёт корень MSAA, но WinUiaBridge::handle_wm_get_object (a11y/platform/windows.rs:159) нигде в shell не вызывается: у event loop нет msg hook (window_mode.rs:34-51), сообщение до моста не доходит. Это та же дыра, что мешает принять WM_SETTINGCHANGE (UX-OSPARAMS) и сообщать системную каретку (UX-CARET). Объём: либо подключить через with_msg_hook вместе с UX-OSPARAMS, либо исправить запись в CAPABILITIES. Приёмка: Inspect/AccChecker видит корень, созданный мостом (или CAPABILITIES не обещает этого). docs/research/browser-ux/report.md §4.1 CARET-10

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
