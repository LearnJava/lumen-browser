# BUG-1838 — Видео-сессии открываются синхронно в обработчике кадра: кадры по 2.6–9.8 с на потоке браузера (okko.tv)

**Статус:** OPEN
**Заведён:** 2026-11-23 (ZCode, задача «okko.tv: сравнить с Chrome и ускорить»)
**Компонент:** shell ([`crates/shell/src/video_tick.rs`](../crates/shell/src/video_tick.rs) — `tick_video_sessions`; [`crates/shell/src/app/window_event/redraw_requested.rs`](../crates/shell/src/app/window_event/redraw_requested.rs) — шаг 2.6)

## Симптом

`https://okko.tv/` (баннер — немое видео 1920×1080, 103 с, ~67 МБ): в логе `LUMEN_FRAME_LOG=1` кадры

```
[frame] total 9818.38ms  ... top: scroll 0.09 sda 0.03 anim 9400.04 js 417.33 build 0.30 paint 0.59
[frame] total 6873.81ms  ... top: scroll 0.08 sda 0.03 anim 6790.18 js 79.97  build 3.16 paint 0.38
[frame] total 2645.14ms  ... top: scroll 0.14 sda 0.08 anim 2641.00 js 0.00  build 3.62 paint 0.30
```

Каждый стоит сразу после `video: загружен nid=… (1920×1080, 102869мс)` и `[media] видео H264: Media Foundation,
D3D11VA на Intel(R) UHD Graphics`. Сумма `build` и `paint` за весь прогон — около 2 с, то есть рисование ни при чём.

## Механизм (по коду)

`redraw_requested.rs:384–391` (шаг 2.6 кадра) вызывает `tick_video_sessions` в потоке браузера. Внутри
(`video_tick.rs:395–569`) на каждый отложенный запрос загрузки синхронно: `fetch_video_bytes` (весь файл),
`decoder.open(&bytes)`, `session.frame_at(0.0)` (первое обращение поднимает Media Foundation / D3D11VA),
`register_image`. Комментарий PERF-INPUT-1c уже признаёт проблему и лишь откладывает слив очереди до
`window.load`. Что именно из трёх шагов занимает секунды — **не разделено** (инструментов у шагов нет).

## Что измерено и что нет

- Измерено: длительность трёх кадров (9.8 / 6.9 / 2.6 с) и их привязка к загрузке видео.
- Измерено: поток `lumen-render` — 30.4–33.6 CPU-с за прогон (`site_ab.py`, `threads_top`).
- Вывод, не измерено: пока кадр не закрыт, поток браузера не выпускает следующие кадры и не обрабатывает ввод,
  то есть после `load` страница на секунды перестаёт отвечать. На первый кадр это не влияет: слив очереди
  загрузки видео отложен до `window.load` (`video_tick.rs:404–416`), поэтому первый тяжёлый кадр идёт после него.
- Не опознано: 8 безымянных потоков по 3.5–4.4 CPU-с каждый (около трети CPU процесса за прогон, 90–120 с).
  Адреса старта всех потоков указывают на общий вход `std` потока Rust, по ним поток не определить. Пул на 8
  потоков = число логических ядер; связь с видео или пулом предпрохода матчинга — предположение.

## Что делать

1. Разметить шаги (`fetch` / `open` / `frame_at(0.0)` / `register_image`) в `[frame]`-логе — определить источник секунд.
2. Открытие сессии и декодирование — вне потока браузера (фоновый поток, кадры в обработчик только забираются);
   загрузку файла — потоком сети, как прочие подресурсы.
3. Опознать 8 потоков — `python scripts/native_sample.py <pid> <exe> 60` (`docs/perf-method.md`); первый
   кандидат — пул предпрохода матчинга LOAD-6 (`style/pre_match.rs`, размер по числу ядер). Решить, это работа
   или ожидание вхолостую.

## Репро

```
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 1 --arms lumen --settle 12 \
    --env LUMEN_FRAME_LOG=1 --out .tmp/site-ab/frames
grep -A2 -E '^\[frame\] total +[0-9]{3,}' .tmp/site-ab/frames/r01-lumen/live.stderr.*.log
```
