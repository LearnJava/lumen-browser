# WPT-RUN-16 — заметка передачи (WIP, 2026-10-22)

Ветка `p2-wpt-run-16` (бронь перехвачена с другой машины решением пользователя), слот `p2-work`.
Бинарь прогона: `dev-release` на `1e87683d6`, лежит в `.tmp/wpt-run16-bin/lumen.exe` (слот p2-work, не в git).

## Что сделано

1. **Попытка 1** (10:24–11:55, рецепт WPT-RUN-9: `--parallel-shards 3 --processes 14`, 42 браузера) —
   прервана. Браузеры вместе доходят до ~22 ГБ RSS, свободная память падает до 0, система уходит в своп на
   3–5 мин; на это время `CreateProcess` у всех трёх wptrunner'ов висит 200–350 с (запуск идёт под
   `_SPAWN_LOCK` в `tools/wptrunner/wptrunner/browsers/lumen.py`, поэтому встают все менеджеры шарда разом).
   Итог — ложные TIMEOUT/CRASH: `editing` шёл 2 710 с против 525 с в WPT-RUN-9, 75 TIMEOUT против
   почти нуля (harness OK 801/842 в прогоне 2026-10-05).
2. **Попытка 2** (12:05–12:20, `--parallel-shards 2 --processes 10`, 20 браузеров) — то же самое: 21.5 ГБ
   RSS, avail 0 МБ. Прервана.
3. **Причина — утечка памяти в движке, не раннер.** Свежий браузер в шардах `referrer-policy`/`encoding`
   растёт линейно, ~20 МБ/с (за 70 с с 90 МБ до 1.4 ГБ), пока раннер его не перезапустит.
   Воспроизводится и без раннера: `lumen.exe --no-paint http://localhost:18300/referrer-policy/gen/iframe.meta/unset/a-tag.http.html`
   (сервер — wptserve полосы 0 работающего прогона) — 89 → 301 МБ за 45 с и дальше растёт.
   `dom/nodes/Node-cloneNode.html` стабилен (125 МБ); простые file://-страницы (iframe, setInterval,
   setTimeout) не текут; простой запуск `--bidi-port --no-paint` без страницы — 83 МБ стабильно.
4. **Диапазон регрессии** (по бинарям соседних слотов на той же странице):
   - `p1-work/target/dev-release/lumen.exe` (собран 2026-10-20 16:00 на `ea2ba3351`) — 73 МБ стабильно;
   - `p4-work/target/dev-release/lumen.exe` (собран 2026-10-20 21:45 на `421516c14`) — 65 → 270 МБ за 45 с.
   - Между ними 20 слияний first-parent: `git log --oneline --first-parent ea2ba3351..421516c14`
     (subgrid/css-gaps срезы 93–97, BUG-1414/1415/1416/1419/1420, SVG-CLIPPATH, SCROLL-MARKERS,
     TEXT-JUSTIFY, BUG-1391/1392/1394/1395, COMPAT-CORPUS-2).

## Что дальше

1. Бисекция по 20 слияниям: `cargo build --profile dev-release --bin lumen` на кандидате (в слоте p2-work
   target тёплый, `RUSTY_V8_ARCHIVE` — копия `rusty_v8.lib` из `../p4-work/target/dev-release/gn_out/obj/`,
   см. память «холодный слот»), замер — `.tmp/run16_idle.py 45 <lumen.exe> --no-paint <url>` (печатает
   private MB каждые 3 с) против страницы referrer-policy под wptserve. Сервер без прогона поднимает
   `run_smoke.py`; заготовка `.tmp/run16_smokemem.py <lumen.exe> <test_id>...` (пиковая память
   браузеров во время run_smoke, ещё не запускалась).
2. Завести `bugs/BUG-NNN-OPEN.md` (утечка ~20 МБ/с, репро, виновное слияние) + строка `BUGS.md` +
   указатель **в начало** `STATUS-P3.md`: без починки полный прогон даёт цифру, испорченную свопом.
3. WPT-RUN-16 перезапускать только после починки: свежий `main` → `dev-release` → копия бинаря →
   рецепт WPT-RUN-9 (3 × 14). Контроль во время прогона — `.tmp/run16_memlog.py .tmp/memlog.txt`
   (avail не должен уходить к нулю). Запуск отдельно от сессии: `.tmp/run16.cmd` через
   `Start-Process -WindowStyle Hidden cmd.exe -ArgumentList '/c',<path>`; при обрыве — тот же cmd с `--resume`.
4. Дальше по «Готово, когда» строки WPT-RUN-16 в `ROADMAP.md`.

Все скрипты замеров (`.tmp/run16_*.py`) и логи попыток (`.tmp/run16-try1.log`, `.tmp/memlog-try1.txt`)
лежат в `.tmp/` слота p2-work, в git их нет.
