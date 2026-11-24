# scripts/perf — живое профилирование загрузки сайтов

Инструменты для замеров реальных сайтов в живом окне Lumen (MCP `eval`, один процесс браузера за раз). Вывод и логи пишутся в `.tmp/prof-run/` (gitignored) — в репозиторий попадают только сами скрипты.

| файл | назначение |
|---|---|
| `prof_site.py <url> [wait_s]` | живой прогон с `LUMEN_PROFILE_TREE=1` + `LUMEN_FRAME_LOG=1` + `LUMEN_PROFILE_DETAIL=1`; ждёт `document.readyState === "complete"`, печатает топ `[profile]`-скоупов по суммарному ms |
| `agg_prof.py <log>` | агрегирует `[profile]`-строки stderr-лога по имени скоупа |
| `wall_site.py <url> [wait_s]` | чистый wall-clock без профайлера: секунды до `readyState=interactive`/`complete` + `performance.timing` — честное время загрузки для сравнения с Chrome |
| `probe_collect.py` | повторяющиеся eval-прогоны `habr_collect.js` до `readyState=complete`; сохраняет resource-timing/navigation/paint/longtask в `.tmp/habr_data.json` |
| `habr_collect.js` | JS-сниппет для `probe_collect.py`: `performance.getEntriesByType('navigation'|'resource'|'paint'|'longtask')` |
| `site_ab.py --url <url> --runs N [--arms lumen,chrome] [--challenge-selector S]` | один сайт в Lumen и в Chrome: время до `load`/контента, сеть, консоль, ошибки, CPU по потокам. Только напрямую, без прокси (Lumen `LUMEN_NO_SYSTEM_PROXY=1`, Chrome `--no-proxy-server`); холодный профиль и свежий процесс на прогон (у Lumen — жёсткая ссылка на exe в пустом каталоге), прогоны чередуются. `<out>/results.json`, `r<NN>-<arm>/` (stderr-лог, `screen.jpg`) |

Запуск (Windows, Python — полный путь к pythoncore):

```bash
python scripts/perf/prof_site.py "https://habr.com/ru/" 90
python scripts/perf/agg_prof.py .tmp/prof-run/live.stderr.prof.0.log
python scripts/perf/wall_site.py "https://habr.com/ru/" 120
```

`LUMEN_PROFILE_TREE=1` печатает в stderr строку `[profile] <ms> <скоуп>` на каждый вызов `profile::scope` — `prof_site.py` включает его сам. Важно: под флагом wall-clock не показателен (каждый скоуп — это eprintln), для реального времени загрузки — `wall_site.py`. `LUMEN_PROFILE_DETAIL=1` добавляет мелкие скоупы (`scope_detail`), `LUMEN_FRAME_LOG=1` — `[engine]`-диагностику (`incr cascade CascadeStats`, `incr stages`, `pseudo_store`). Произвольные флаги V8 — `LUMEN_V8_FLAGS` (например `--trace-opt`).

Зависимости: `scripts/perf_audit.py` (`LiveBrowser`, MCP-обёртка). Правила живых прогонов — `docs/automation.md` §Flags (`--maximized`, `LUMEN_NO_ADBLOCK=1`). Контекст эпика — `HANDOFF-PERF.md`.
