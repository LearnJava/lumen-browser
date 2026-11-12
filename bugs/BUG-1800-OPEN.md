# BUG-1800 — ebay.com: JS-задачи по 30–40 с — same-tick flush исполняется внутри DOM-чтений (`getAttribute`/`getBoundingClientRect`) из колбэков RO/IO; `document_ready` задержан на +9.5…+140 с

**Статус:** OPEN
**Открыт:** 2026-11-12 (P3, перф-аудит ebay, перф-скилл)
**Компонент:** js/layout (`crates/js/src/v8_runtime/style_flush.rs`, `crates/engine/layout/src/scoped_collect.rs` — same-tick flush; DOM-читалки шима, вызывающие его)

## Симптом

`https://www.ebay.com/`, dev-release `b88c91e13`+UA157, `--maximized`, `LUMEN_NO_ADBLOCK=1`, холодный `data/`, прямой маршрут, 3 compat-прогона (`perf_audit.py`, `LUMEN_JS_STALL_SAMPLE_MS=200`, `LUMEN_FRAME_LOG=1`). Отчёт: [docs/perf/runs/2026-11-12-ebay-audit.md](../docs/perf/runs/2026-11-12-ebay-audit.md), журнал: [docs/perf/journal.md](../docs/perf/journal.md) §2026-11-12.

- `final_applied_s` стабилен: 10.32/10.38/10.39 с, но `ready_s`: **19.79 / 150.53 / 84.06 с** (у Chrome база load 9.3–10.3 с) — готовность задержана штормом задач на +9.5/+140.2/+73.7 с.
- Задач ≥200 мс на прогон: 19/42/40, сумма **93.0/124.8/108.1 с**; максимальная задача — **40.7/32.9/38.3 с**, стек `getAttribute (<no-script>)` (по 2 задачи 24–41 с на прогон; сумма по стекам за 3 прогона: `getAttribute` 160 с, `getBoundingClientRect` 50 с, `_io_cb_chain` 33 с, `_ro_content_geometry` 23 с, `readProp` 19 с).
- `maybe_flush` — 120/185/139 вызовов, сумма **92.8/123.5/106.1 с**, p50 **925/891/1035 мс**, max 2.05 с — на 647–648 touched узлах (DOM 2517 узлов). Сумма flush ≈ сумме длинных задач (93.0 ↔ 92.8 в прогоне 1) — задачи состоят из flush'ей.
- MCP-проба страницы стояла 48 с (прогон 1); CPU на сессию 165–281 с; кадры >500 мс — 68–69 за прогон (см. BUG-1802).
- Шторм кластеризуется в последней трети сессии (после ready): eBay Live-виджеты (счётчики зрителей «LIVE • N») непрерывно мутируют DOM, колбэки ResizeObserver/IntersectionObserver читают атрибуты/геометрию → каждый gust чтений тянет синхронный flush ~1 с.

## Гипотеза

Каждое DOM-чтение, требующее актуального layout (геттеры атрибутов/геометрии), запускает same-tick flush (механизм BUG-935) по полному/дорогому пути (~1 с на 647 узлах — в 2–4 раза дороже lenta.ru после срезов 59–60), а непрерывный поток колбэков ebay не даёт очереди flush'ей опустеть — движковый поток занят 74–90 % времени сессии. Это подтверждает гипотезу BUG-1717 (vkvideo, gBCR-цикл → принудительный relayout) на втором сайте и расширяет её на `getAttribute`.

## Репро

```
LUMEN_EXE=target/dev-release/lumen.exe python scripts/perf_audit.py --mode compat \
  --corpus docs/perf/corpus-top100-foreign.txt --only ebay --timeout 240 \
  --env LUMEN_JS_STALL_SAMPLE_MS=200 --env LUMEN_FRAME_LOG=1
```
Смотреть `ready_s`, `[js-stall] задача идёт …` и `maybe_flush done …` в stderr-логе.

## Что сделать

Направления (в порядке дешевизны): чтения без требования свежего layout — по пути среза 59 BUG-935 (`getComputedStyle`); копакетирование нескольких чтений одного тика в один flush; ограничение частоты full-путей; разбор причин стоимости flush на 647 узлах (`LUMEN_PROFILE_TREE=1` на ebay). Перф-улучшение крупнее бага — см. предложение PERF в отчёте §7.
