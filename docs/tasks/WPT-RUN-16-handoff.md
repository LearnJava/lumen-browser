# WPT-RUN-16 — заметка передачи (WIP, 2026-10-22)

Ветка `p2-wpt-run-16`, слот `p2-work`. **Задача заблокирована [BUG-1682](../../bugs/BUG-1682-OPEN.md)**
(указатель первым в `STATUS-P3.md`): прогон запускать только когда баг станет FIXED.

## Что сделано

1. **Попытка 1** (10:24–11:55, рецепт WPT-RUN-9: `--parallel-shards 3 --processes 14`, 42 браузера, бинарь
   `dev-release` на `1e87683d6`) прервана. Браузеры вместе доходят до ~22 ГБ RSS (у машины 32 ГБ), свободная
   память падает до 0, система уходит в своп на 3–5 мин. На это время `CreateProcess` у всех трёх
   wptrunner'ов висит 200–350 с: запуск идёт под `_SPAWN_LOCK` в `tools/wptrunner/wptrunner/browsers/lumen.py`.
   Итог — ложные TIMEOUT/CRASH: `editing` шёл 2 710 с против 525 с в WPT-RUN-9.
2. **Попытка 2** (`--parallel-shards 2 --processes 10`) — то же самое, прервана.
3. **Причина — BUG-1682.** Рантайм V8 каждого iframe (поток + изолят, ~20 МБ) не освобождается ни при
   удалении `<iframe>`, ни при замене страницы (цикл `Arc` `parent_peer` ↔ `peer`). Под раннером браузер
   переиспользуется от теста к тесту и в `referrer-policy` доходит до 2.5 ГБ за ~10 тестов. Автономные
   репро-страницы — в баге, копии в `.tmp/leakpages/` слота.
4. **Диапазон регрессии из первой версии заметки (`ea2ba3351..421516c14`) не подтвердился.** Утечка
   воспроизводится и на `ea2ba3351`, и на `release` от 2026-10-20. Автономный замер на
   `a-tag.http.html` под отдельным wptserve на всех трёх бинарях стабилен (~130 МБ): прошлый «рост 20 МБ/с»
   был именно тестом под раннером, а не простоем страницы. Почему WPT-RUN-9 пережил тот же рецепт, не выяснялось.

## Что дальше

1. Дождаться `BUG-1682` FIXED (`git log origin/main --oneline --grep=BUG-1682`). Проверка починки:
   `.tmp/run16_idle.py 30 <lumen.exe> --no-paint file:///…/.tmp/leakpages/ifr-empty.html` и
   `navframes.html` не растут после первых секунд. Для страниц под HTTP:
   `.tmp/venv-wpt/Scripts/python.exe .tmp/run16_serve.py` поднимает wptserve на портах 18300/18443
   (`check_subdomains: false`, `browser_host: localhost` в `.tmp/run16_serve.json`). Пиковая память под
   раннером: `.tmp/run16_smokemem.py <exe> /referrer-policy/gen/iframe.meta/unset/` с `MSYS_NO_PATHCONV=1`
   (до починки — пик ~2.5 ГБ на браузер).
2. Свежий `main` → `cargo build --profile dev-release --bin lumen` → копия бинаря в `.tmp/wpt-run16-bin/` →
   рецепт WPT-RUN-9 (3 × 14, `--max-browser-gb 4`). Запуск отдельно от сессии: `.tmp/run16.cmd` (поправить
   на 3 × 14) через `Start-Process -WindowStyle Hidden cmd.exe -ArgumentList '/c',<path>`; при обрыве — тот же
   cmd с `--resume`. Контроль — `.tmp/run16_memlog.py .tmp/memlog.txt`: avail не должен уходить к нулю.
3. Дальше по «Готово, когда» строки WPT-RUN-16 в `ROADMAP.md`.

**Осторожно:** основной checkout `D:/RustProjects/lumen-browser` бывает занят другими сессиями (2026-10-22
там стояла WIP-ветка P4). Перед `git pull` или `checkout` в нём — `git branch --show-current` и `git status`.
Работать в слоте `p2-work`.

Скрипты замеров (`.tmp/run16_*.py`, `.tmp/leakpages/`) и логи попыток лежат в `.tmp/` слота p2-work, в git их нет.
