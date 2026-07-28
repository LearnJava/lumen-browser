# Журнал памяти как временного ряда (PERF-5)

Хронология прогонов харнесса памяти — `python scripts/mem_perf.py`
([`scripts/mem_perf.py`](../../scripts/mem_perf.py)). Отвечает на два вопроса
длинной сессии, которые разовый дамп `LUMEN_MEM_REPORT` не покрывает: выходит ли
RSS на **плато** после открытия N вкладок и не **течёт** ли память со временем.
БЕЗ нового кода в движке — поверх уже существующих поверхностей, как PERF-2/3/4.

## Что и как меряется

Харнесс поднимает живое окно (`--mcp-live-port`), навигирует первую вкладку на
фикстуру, затем открывает остальные через MCP `new_tab`, и всё это время снимает
**плотный ряд RSS** процесса (WinAPI `GetProcessMemoryInfo`, `WorkingSetSize` —
тот же приём, что `_win_proc_stats()` в `perf_audit.py`) каждые `--sample-ms`.

Две фазы:

| Фаза | Что делает | Метрики |
|---|---|---|
| `ramp` | открыть N вкладок на фикстуру, dwell на вкладку | `per_tab_mb`, `ramp_slope_mb_per_tab` |
| `hold` | простоять idle `--hold-s` секунд | `plateau_rss_mb`, `hold_slope_mb_per_min` (leak-детектор) |

**Разбивка «куда ушла память»** на конец прогона:

| Бакет | Источник | Смысл |
|---|---|---|
| `rust_known_mb` | сумма Rust-структур из `MEM_REPORT` (dl-cache, image/prefetch-кэши, web-fonts, GIF, femtovg raw_images) | известная Rust-heap движка |
| `cheap_js_mb` | `js_malloc` из `MEM_REPORT` | арена V8 (C-heap) |
| `unattributed_mb` | `RSS − rust_known − js_malloc` | GPU-драйвер / фрагментация Rust-кучи / прочий C-heap |
| `gpu_mb` | `Get-Counter '\GPU Process Memory(pid_*)\Local Usage'` (`--gpu`) | GPU-память процесса, best-effort |

`gpu_mb` показан **отдельно и не вычтен**: на интегрированной графике это
системная RAM, уже сидящая в RSS (вычесть = двойной учёт). Его роль — объяснить,
из чего состоит «неатрибуцированный» остаток.

## Честные ограничения (чисто-тулинговый путь)

1. **Точного Rust-heap нет.** `rust_known` — сумма *известных* хранилищ, а не показ
   инструментированного аллокатора, поэтому невидимая Rust-фрагментация уходит в
   «неатрибуцировано». Точная цифра ждёт постоянного counting-allocator в движке
   (`reference_memory_diagnosis_toolkit` п.3; задача P1/P3).
2. **`MEM_REPORT` событийный, не таймерный.** Он печатается в `about_to_wait` не
   реже раз в 10 с, но только когда event-loop просыпается; на idle-окне без
   анимации пробуждений мало, поэтому за прогон ловится **мало** строк отчёта
   (иногда одна). Поэтому ряд для **плато/утечки** строится на плотном RSS
   (снимает харнесс извне), а `MEM_REPORT`-разбивка — снимок «на последний отчёт».
3. **`js_malloc` часто недоступен.** В дефолтном flag-off режиме многопоточности
   движок печатает sentinel `-1/1e6` («-0.0MB»), харнесс распознаёт его по знаку и
   ставит `cheap_js = «—»`; доля V8 тогда уходит в «неатрибуцировано».
4. **RSS и GPU-counter — Windows.** Вне Windows ряд RSS пуст → прогон падает с
   понятной ошибкой; чистая статистика при этом проверяется `--selftest` (в воротах).

## Запуск

```bash
cargo build -p lumen-shell --profile dev-release
python scripts/mem_perf.py                              # фикстура, 6 вкладок + hold 30с
python scripts/mem_perf.py --tabs 10 --hold-s 60        # длиннее сессия — точнее leak-наклон
python scripts/mem_perf.py --page https://example.com --tabs 4
python scripts/mem_perf.py --gpu                        # + GPU-counter (медленно)
python scripts/mem_perf.py --json docs/perf/memory-runs/<date>.json
python scripts/mem_perf.py --compare docs/perf/memory-runs/<date>.json
python scripts/mem_perf.py --selftest                   # статистика без браузера (ворота)
```

Сырые прогоны — `docs/perf/memory-runs/*.json` (коммитятся, как metrics-runs/input-runs/startup-runs).

## Хронология

| Дата | Commit | Вкладок | RSS старт→конец (МБ) | На вкладку (МБ) | Плато (МБ) | Наклон hold (МБ/мин) | GPU (МБ) | Заметка |
|---|---|---|---|---|---|---|---|---|
| 2026-07-18 | ef5acda9 (PERF-5) | 6 | 568.0→686.7 | 19.8 | 686.7 | 0.0 | 413.5 | Базлайн. Фикстура `scripts/perf-fixtures/mem.html`, dwell 2с, hold 30с. Утечки на idle нет (наклон 0). GPU = 60% RSS — прямое подтверждение находки reference_memory_diagnosis_toolkit «гигабайт вне Rust-кучи = GPU»; ср. неатрибуцированное RAM-плато ~850МБ в exp-wgpu-only. `js_malloc` = sentinel (недоступен). Файл: [`memory-runs/2026-07-18.json`](memory-runs/2026-07-18.json) |

## Как читать регрессию

`--compare <prev.json>` печатает дельту ключевых метрик (`plateau_rss_mb`,
`per_tab_mb`, `hold_slope_mb_per_min`, `unattributed_mb`); рост > 20% помечается
⚠ (память вверх = хуже). Отдельный флаг: `leak_suspected` = наклон hold больше
`--leak-mb-per-min` (по умолчанию 5). Порог для гейта — задача PERF-7 (ночной
перф-гейт поверх метрик PERF-2/3/4/5, механизм в
[`crates/bench/src/ci_gate.rs`](../../crates/bench/src/ci_gate.rs)).
