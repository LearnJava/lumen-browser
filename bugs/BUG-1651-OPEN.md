# BUG-1651 — youtube: engine-поток занят одной задачей таймеров/pump 8–9 с

**Статус:** OPEN
**Заведён:** 2026-10-19 (Chat, проверка youtube после BUG-1239/1633/1634/1638/1639)
**Область:** shell / js (`crates/shell/src/app/about_to_wait.rs:287` — `route_task_js` → `tick_timers`/`pump_*`) — не локализовано

## Симптом

Та же сборка и запуск, что в [BUG-1650](BUG-1650-FIXED.md), главная `https://www.youtube.com/`. Живой `eval`
(`timeout_ms: 5000`) примерно на 50–70-й секунде после открытия дважды подряд отвечает:

```
Eval error: engine thread busy: eval not run within 5.0 s
(running task from crates\shell\src\app\about_to_wait.rs:287:13 for 8.7 s)
```

`about_to_wait.rs:287` — пер-тиковый батч `route_task_js(|j| { j.tick_timers(); j.pump_websockets(); j.pump_sse();
j.pump_media(); j.pump_workers(); … })`. Одна такая задача идёт ≥ 8,7 с: всё это время страница не реагирует
на ввод (тот же поток). В Chrome главная youtube не даёт долгих задач такого масштаба.

## Что сделать

1. Выяснить, что внутри батча занимает секунды: таймер-колбэк страницы (какой скрипт), `pump_workers`
   (ServiceWorker youtube), `pump_media` — замер по вызовам (`LUMEN_FRAME_LOG`/трасса `--trace-nav` или
   временные метки в `.tmp`-сборке).
2. Если это JS страницы — сравнить с Chrome Performance (длинные задачи главной youtube) и найти, что
   у нас медленнее (горячая функция шима, синхронный relayout из JS, GC).
3. Если это наш pump — ограничить работу за тик (бюджет), чтобы ввод и `eval` не ждали секунды.

**Готово, когда:** на главной youtube за первые 90 с нет задачи engine-потока длиннее ~1 с; живой `eval`
с `timeout_ms: 5000` не получает `engine thread busy`.

Связано: [BUG-935](BUG-935-OPEN.md) (долгие задачи `about_to_wait` на rAF-мутациях) — проверить, не тот ли путь.
