# BUG-1802 — ebay.com: 65–69 кадров за прогон стоят 0.5–1.9 с в статье `sbar` (перестройка scrollbar-overlay), хотя внутри интервала только чистые операции на кэшированных данных

**Статус:** OPEN
**Открыт:** 2026-11-12 (P3, перф-аудит ebay; до исправления — точечная бисекция)
**Компонент:** shell (`crates/shell/src/app/window_event/redraw_requested.rs:740–810` — интервал `bmarks[0]..bmarks[1]`; `crates/shell/src/lumen/cursor.rs:49`, `crates/shell/src/frames.rs:2051`)

## Симптом

`https://www.ebay.com/`, dev-release, живое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, `LUMEN_FRAME_LOG=1` (3 compat-прогона 2026-11-12):

- кадров >500 мс за прогон: **68 / 65 / 69**, max **1748/1706/1907 мс**, при p50 кадра 0.73–7.8 мс;
- разбиение тяжёлого кадра всегда одно: `build: chrome 0.70 sbar 1736.94 panels 6.15` — вся стоимость в интервале `sbar`;
- часто `delta ContentChanged`; кластеризация по времени — последняя треть сессии, синхронно со штормом задач движкового потока (BUG-1800).

Пользователь видит это как фризы интерфейса по 0.5–1.9 с при живой странице; порог «Не отвечает» (5 с) не достигался — 0 с за все прогоны.

## Гипотеза и что сделать

Внутри интервала `bmarks[0]..bmarks[1]` вызовы чистые и работают на кэшированных данных (`page_thumb_states`, `build_scrollbar_overlay_with_state`, `build_hscrollbar_overlay`, `frames::frame_scrollbar_overlay`, `build_find_marks` — ни одного вызова в движок; проверено чтением кода). Вытеснение CPU не объясняет: система ≤41 %, Lumen ≤233 % (4C/8T). Кандидаты: (а) скрытая блокировка (мьютекс render-состояния, занятого wgpu-потоком), (б) недостоверность атрибуции — время ожидания где-то раньше попадает в следующий интервал, (в) десcheduling потока shell движковым штормом на уровне ОС. Сделать: `LUMEN_FRAME_LOG=3`/точечные `Instant`-таймеры внутри интервала на одном тяжёлом кадре; после локализации — фикс.

## Репро

```
LUMEN_EXE=target/dev-release/lumen.exe python scripts/perf_audit.py --mode compat \
  --corpus docs/perf/corpus-top100-foreign.txt --only ebay --timeout 240 \
  --env LUMEN_FRAME_LOG=1
```
grep `\[frame\] total` >500 мс и `sbar` в stderr-логе.

Отчёт: [docs/perf/runs/2026-11-12-ebay-audit.md](../docs/perf/runs/2026-11-12-ebay-audit.md) §2.3. Связь: BUG-405 (статьи кадра), BUG-1800.
