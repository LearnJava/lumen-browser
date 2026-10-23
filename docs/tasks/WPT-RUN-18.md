# WPT-RUN-18 — общий бюджет памяти полного прогона WPT: без свопа и ложных TIMEOUT

**Developer:** P2  
**Ветка:** `p2-wpt-run-18`  
**Размер:** M  
**Крейты:** нет (Python: `tests/wpt/run_corpus.py`, `tests/wpt/browser_rss_cap.py`, `tests/wpt/metadata/`, `docs/wpt/`)

## Зачем

WPT-RUN-16 (`docs/wpt/pass-rate.md` §«Четвёртая полная цифра») шёл **6.5 ч вместо 2 ч 44 мин**
у WPT-RUN-9, и рецепт пришлось менять на ходу:

- попытка 3 (`--parallel-shards 3 --processes 14`, сторож `--max-browser-gb 4`) в 17:33 исчерпала
  свободную память, машина ~1.5 ч стояла в свопе, `CreateProcess` висел 200–350 с;
- шарды, снятые в свопе, дали ложные просадки: `editing` (49 TIMEOUT, `live window closed`),
  `WebCryptoAPI` (39 `navigate: wait timeout`), `webaudio` (38 TIMEOUT). Каждую пришлось
  перепроверять «тихим» перепрогоном;
- сторож RSS убил 60 браузеров (было 8), 47 из них — `html/syntax/html5lib_write*.html` и
  `html5lib_url.html` (много iframe + `document.write`, 4.2–5.3 ГБ на браузер). Эти тесты проходят
  0 сабтестов и без CRASH;
- 7 тестов потеряно из-за `Max restarts exceeded` под нехваткой памяти (их учёт — WPT-RUN-17).

Что есть сейчас: `--min-free-gb 6` (`run_corpus.py:1063`) проверяет свободную память **только
перед стартом шарда**. Шард из 14 процессов, которые потом растут до 4 ГБ каждый, этим не
ограничен: 42 браузера × до 4 ГБ при 32 ГБ RAM. `browser_rss_cap.py` ограничивает только один
браузер, а не их сумму.

## Что сделать

Каждый шаг — с замером «до/после» на одном и том же тяжёлом наборе (п. 0); шаг, не давший
выигрыша, не вливать, а записать результат в `docs/wpt/pass-rate.md`.

0. **Тяжёлый набор для замера.** Префиксы, которые в WPT-RUN-16 давали пики памяти и ложные
   TIMEOUT: `html/syntax`, `referrer-policy`, `editing`, `WebCryptoAPI`, `webaudio`, `IndexedDB`,
   `webstorage`. Замер «до» на свежем `dev-release` (бинарь скопировать в `.tmp/`): рецепт
   `--parallel-shards 3 --processes 14 --shared-queue --batch-small 150 --max-browser-gb 4`, раз в
   10 с писать свободную память и суммарный RSS браузеров прогона в `.tmp/`-лог. Записать стену,
   минимум свободной памяти, число TIMEOUT/CRASH, убийства сторожем.
1. **Сторож суммы, а не одного браузера.** В `browser_rss_cap.py` (поток уже обходит потомков
   прогона) — режим нехватки памяти: если свободной физической памяти меньше порога
   (`--min-free-gb`, тот же флаг), полосы перестают брать новые шарды (уже есть) **и** раннер не
   даёт браузерам расти дальше. Вариант на выбор по замеру: (а) убить самый большой браузер
   прогона выше 2 ГБ (его тест получит CRASH, а не утащит в своп 40 соседних тестов); (б)
   приостановить полосу целиком (`psutil.Process.suspend()` дерева шарда) до освобождения памяти,
   не считая паузу во время тестов — проверить, что таймауты wptrunner при этом не срабатывают.
   Каждое срабатывание — строка в `rss-cap-kills.jsonl` с причиной `low-free`, чтобы CRASH в
   отчёте отличался от падения движка.
2. **Число процессов от памяти, а не константой.** `--processes auto`: по умолчанию
   `min(cpu, (свободная_ГБ − запас) / средний_RSS_браузера)`, средний RSS — 1 ГБ (по
   `browser_rss_cap.py`) или замер первых шардов. Пользоваться им в рецепте полного прогона в
   `docs/wpt/pass-rate.md`.
3. **Известные пожиратели памяти — отдельно.** `html5lib_write*.html`, `html5lib_url.html` и
   остальные тесты с ≥ 2 убийствами сторожем в WPT-RUN-16 (`rss-cap-kills.jsonl` прогона, если
   сохранился; иначе — по замеру п. 0): запускать их одной полосой в конце прогона
   (`--processes 2`), а не в общей очереди. Список — в одном файле (`tests/wpt/heavy_tests.txt` или
   расширить `long_tests`), с причиной у каждой строки. Тесты из знаменателя не исключать.
4. **Документация:** `docs/wpt/pass-rate.md` — новый рецепт полного прогона и таблица замеров
   «до/после»; описание флагов — в `--help` и `docs/commands.md`.

## Готово, когда

- На тяжёлом наборе п. 0 с рецептом `--parallel-shards 3 --processes auto`: свободная память ни
  разу не падает ниже `--min-free-gb`, своп не растёт (лог п. 0), а TIMEOUT в `editing`,
  `WebCryptoAPI`, `webaudio` не больше, чем в «тихих» перепрогонах WPT-RUN-16 (`--processes 6`;
  цифры — в таблице падений `docs/wpt/pass-rate.md`). Стена набора — не больше, чем «до».
- `python tests/wpt/run_corpus.py --selftest` зелёный; новые ветки кода сторожа покрыты
  синтетическим тестом (без запуска браузера).
- Таблица «до/после» и новый рецепт записаны в `docs/wpt/pass-rate.md`.

## Не делать

- Не запускать полный корпус ради приёмки — достаточно тяжёлого набора; полный прогон будет
  следующей отдельной задачей.
- Не снижать знаменатель: тяжёлые тесты переносятся в конец, а не исключаются.
- Не трогать движок: утечки памяти конкретных тестов — баги `bugs/BUG-NNN-OPEN.md` для P3, с
  RSS и тестом в описании.

## Передача между сессиями (WIP, сессия 1 → 2)

Ветка `p2-wpt-run-18` (слот `p2-work`), коммит WIP. Замер «до» **не идёт** — был запущен и убит при
передаче через 6 минут, ничего не сохранилось; после перезапуска сессии прогон надо начинать заново
и держать в той же сессии (память: прогон умирает вместе с сессией).

**Сделано**

- `tests/wpt/browser_rss_cap.py`: триггер `low-free` (`min_free_gb`: при свободной памяти ниже порога
  убивается самый большой браузер прогона не меньше `LOW_FREE_FLOOR_GB`=2, пауза `LOW_FREE_COOLDOWN_S`=5 с;
  запись в `rss-cap-kills.jsonl` с `"reason": "low-free"`, у убийства по лимиту — `"cap"`), лог памяти
  `mem-log.tsv` раз в 10 с (свободно/своп/число и RSS браузеров), CLI-сэмплер
  `python tests/wpt/browser_rss_cap.py --sample PID --out FILE` для прогона без сторожа, самотест
  (`python tests/wpt/browser_rss_cap.py` → `selftest: ok`).
- `tests/wpt/run_corpus.py`: добавлены (пока **не подключены**) `load_heavy_tests`, `expand_heavy`, `is_heavy`,
  `unit_processes`, `heavy_exclusions`, `auto_processes`, `resolve_processes`, `processes_arg`, константы
  `HEAVY_*`, `AUTO_*`; `plan_shards(..., heavy=)` создаёт шард `"<категория> (heavy)"` с `test_ids`;
  `shard_targets` для heavy-шарда отдаёт пути файлов без `?вариант` (путь без запроса выбирает все варианты).
- Подготовлены (в `.tmp/`, не в git): `run18_arm.py` (запуск плеча A/B: `python .tmp/run18_arm.py <arm>
  [--no-sample] -- <аргументы run_corpus>`, ставит `LUMEN_ALLOW_POPUPS=1`, пишет `.tmp/run18-<arm>.log`
  и `RUN_EXIT=`, аварийно убивает плечо, если своп вырос больше чем на 14 ГБ), бинарь
  `.tmp/wpt-run18-bin/lumen.exe` (= бинарь WPT-RUN-16, `dev-release` от 22.10, байт в байт).
  Интерпретатор: `D:/RustProjects/lumen-browser/.tmp/venv-wpt/Scripts/python.exe` (в p2-work venv нет).

**Осталось**

1. `run_corpus.py`: `plan_units` — heavy-шарды не в обычные батчи, а в один юнит последним
   (`{"name": "heavy lane", "members": [...], "heavy": True, "report_path": batches/heavy.json}`, `split_batch`
   разложит по членам); `run_units_parallel`: сортировка «heavy последним» (`key=(is_heavy, -budget)`),
   `budget_of`/`run_shard` с `unit_processes(unit, args.processes)`, heavy-юнит берётся только когда
   `running == 0` (увеличивать `running` внутри `take()` под мьютексом; на пути `PortsBusy` уменьшать);
   то же `unit_processes` в последовательном цикле и в бюджетах `--resume`.
2. `main()`: `--processes` с `type=processes_arg`, `default="7"`, разрешение через `resolve_processes(...,
   args.parallel_shards, args.min_free_gb)` (печатать `note`); `--no-heavy-lane` и `--heavy-tests PATH`;
   `plan_shards(..., heavy=load_heavy_tests())`; два exclude-файла: для обычных шардов (https-список +
   `heavy_exclusions(shards)`), для heavy-юнита — только https-список (`run_shard(exclude_file=...)` на юнит);
   `BrowserRssCap(args.max_browser_gb, out_dir, min_free_gb=args.min_free_gb)` — теперь сторож включается и при
   `--max-browser-gb 0`, если задан `--min-free-gb`; в `state.json` писать `"heavy": bool`; в `run_shard` —
   `state["low_free_kills"]`; в снимке — `low_free_kills`, `min_free_seen_gb`, `max_swap_gb`, `processes`
   разрешённое число, `heavy`.
3. `score_audit.shard_index`: категория шарда — `name.split(" ")[0].split("/")[0]` (сейчас `.replace(" (bare)")`
   не понимает `" (heavy)"`), `plan_shards(..., heavy=load_heavy_tests() if state.get("heavy") else None)`;
   то же в `attribute_shards`/`runnable_ids_per_shard` при необходимости.
4. `tests/wpt/heavy_tests.txt`: `/html/syntax/parsing/html5lib_url.html`, `html5lib_write.html`,
   `html5lib_write_single.html` (60+55+55 вариантов, 47 из 60 убийств сторожем в WPT-RUN-16),
   `/acid/acid3/numbered-tests.html` (BUG-1267), webstorage `*quota*` (`storage_*_setitem_quotaexceedederr`,
   `storage_*_quota_independent_from_*` — найти в манифесте), у каждой строки причина.
5. Самотесты в `run_corpus.py --selftest`: `load_heavy_tests` (причина обязательна, `?` запрещён),
   `plan_shards(heavy=)` + `plan_units` (heavy-юнит один, последний, `processes==2`), `auto_processes`,
   `heavy_exclusions`, `shard_targets` heavy, сторож low-free уже покрыт самотестом `browser_rss_cap`.
6. Замеры на тяжёлом наборе (`--prefixes html/syntax,referrer-policy,editing,WebCryptoAPI,webaudio,IndexedDB,
   webstorage`, 7 шардов, 4149 id): «до» = рецепт WPT-RUN-16 (`--parallel-shards 3 --processes 14 --shared-queue
   --batch-small 150 --max-browser-gb 4`, старое поведение = `--no-heavy-lane` без low-free, т.е.
   `--min-free-gb 0 --processes 14`) с сэмплером; «после» = `--parallel-shards 3 --processes auto`. Внимание:
   сторож портов `reap_lumen_orphans` при старте убил чужой `lumen --ipc-server` из `p6-work` (debug-сборка
   другой сессии) — это поведение раннера, запускать прогон, когда чужих браузеров нет, и сказать об этом в
   отчёте. Свободной памяти на машине при старте ~17–18 ГБ из 32 (фоновые процессы), своп уже 1.5–3 ГБ.
7. Документация: `docs/wpt/pass-rate.md` (таблица «до/после», новый рецепт полного прогона:
   `--all --parallel-shards 3 --processes auto --shared-queue --batch-small 150 --max-browser-gb 4`),
   `--help`, `docs/commands.md`; `docs/doc-sync.md` — что ещё синхронизировать; затем
   `python tests/wpt/run_corpus.py --selftest`, коммит, `merge --no-ff` в main, убрать указатель
   `ROADMAP.md:1254` из `STATUS-P2.md`, статус WPT-RUN-18 в ROADMAP.
