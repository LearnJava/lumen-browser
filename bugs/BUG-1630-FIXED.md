# BUG-1630 — testharness по BiDi стоит в TIMEOUT на `main`: `loading` в модели вкладки хрома (UX-LOADING)

**Статус:** FIXED 2026-10-18 (P3)
**Заведён:** 2026-10-14 (P2, при первом прогоне `compat/` — COMPAT-CSS-WPT)
**Область:** shell (`crates/shell/src/chrome_ui.rs`, модель вкладки хрома)

## Симптом

На `main` e0c276e88 любой testharness-id в `tests/wpt/run_corpus.py` / `run_report.py` даёт `TIMEOUT`
(«browser did not answer a BiDi call within 20s»). В логе лумена нет `→ GET`: navigate не доходит до
главного потока. Один процесс — 100 % TIMEOUT; при `--processes 7` примерно треть id проходит случайно.
Reftest (`--ipc-server`) не затронуты.

## Бисект

`main`: 8f777aca9 хороший → e0c276e88 плохой (8 шагов). Первый плохой коммит — **27e264dc5 UX-LOADING**
(индикатор загрузки вкладки и кнопка «Остановить»). Бинарь от 07.10 проходит `compat/historical.html` за 9,7 с.

Сужение: блок `chrome_loading_shown` в `about_to_wait.rs` не причина (убран — TIMEOUT остался). Причина —
строка в модели вкладки хрома, `crates/shell/src/chrome_ui.rs` (~404):

```
loading: Some(t.id) == active_id && self.nav_start.is_some()
```

Замена на `loading: false` (и больше ничего) на чистом `main` возвращает `historical.html` в `OK` за 9,5 с, а
весь `compat/` — в стабильные 14 с (3 прогона подряд идентичны).

## Связь с BUG-1629

Скорее всего та же причина (живое окно: первый кадр приходит ровно по истечении таймаута `wait`). Не проверено
в живом окне; если подтвердится — закрывать оба одним исправлением.

## Воспроизведение

```
# 1. на чистом main собрать
cargo build --profile dev-release --bin lumen
# 2. один testharness-id
python tests/wpt/run_corpus.py --binary target/dev-release/lumen.exe --prefixes compat --processes 1 --skip-manifest-update
```

Ожидание: `historical.html` — `OK` за ~10 с. Факт: `TIMEOUT`. С `loading: false` в chrome_ui.rs — `OK`.

## Что нужно

1. Понять, почему вычисление `loading` (или связанная с ним перерисовка/перекомпоновка хрома) блокирует
   обработку BiDi-команд, пока идёт навигация (`nav_start.is_some()`), и убрать блокировку, сохранив индикатор.
2. Проверка: `compat/` под `--processes 7` даёт 16 из 38 id (41,68 %) три раза подряд без `TIMEOUT`.

## Кого блокирует

Любой срез WPT-RUN-* и COMPAT-CSS-WPT: прогон testharness недействителен, пока баг открыт.

## Исправление (2026-10-18, P3)

**Причина.** `loading: true` включает в хроме спиннер с `animation: spin .7s linear infinite`.
`RedrawRequested` при живой анимации хрома вызывал `request_redraw()` сразу, то есть следующий кадр
ставился из самого кадра. Пока окно показывается с vsync, `present` (Fifo) ограничивает цепочку 60 Гц. У окна,
которое накрыто или свёрнуто (так идут прогоны WPT и `perf_audit`), `present` возвращается сразу: 2800 кадров/с,
очередь сообщений Windows всегда содержит `WM_PAINT`, и `about_to_wait` не вызывается — замер
`ATW gap` показал разрыв 8,6 и 10,9 с. BiDi `browsingContext.navigate` берёт команду и перепроверяет
`wait{document_ready}` именно в `about_to_wait`, поэтому отвечал только когда спиннер гас. `loading: false`
«лечил» баг тем, что убирал бесконечную анимацию.

**Правка.** Следующий кадр анимации хрома ставится таймером: `RedrawRequested` пишет
`Lumen::chrome_anim_redraw_at` (`+16 мс`), `about_to_wait` либо просит перерисовку, когда срок настал, либо
вкладывает срок в `ControlFlow::WaitUntil`. Между кадрами цикл доходит до `about_to_wait`.

**Проверка.** 4 навигации BiDi подряд на локальную страницу: 0,2–0,4 с (до правки 5–20 с или таймаут);
`tests/wpt/run_smoke.py` — 0 unexpected (до правки `TIMEOUT`); `run_corpus.py --prefixes compat --processes 7`
три раза подряд — 41,68 % (16 из 38), без `TIMEOUT`; `perf_audit.py --only example` — `ready_s` 0,98 вместо `HUNG`.

**Остаток** — в BUG-1629: после этого исправления окно доходит до `document_ready`, но `resource://screenshot`
по-прежнему не отдаёт кадр (`DEGRADED`, `no_png_reason`).

