# BUG-1630 — testharness по BiDi стоит в TIMEOUT на `main`: `loading` в модели вкладки хрома (UX-LOADING)

**Статус:** OPEN
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
