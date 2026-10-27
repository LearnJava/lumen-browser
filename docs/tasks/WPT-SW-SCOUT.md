# WPT-SW-SCOUT — handoff (сессия 1, 2026-10-27)

Ветка `p2-wpt-sw-scout`, слот `p2-work`. Задача — `ROADMAP.md` строка `WPT-SW-SCOUT`. Код не чинить.

## Сделано

- Прогон: `dev-release` от `main` `c9bcc4bab`, `run_corpus.py --prefixes service-workers --processes 6 --out-dir .tmp/wpt-sw --run-json .tmp/wpt-sw/run.json` (venv `D:/RustProjects/lumen-browser/.tmp/venv-wpt`). Длительность 1133 с. Результат: 331 id, **11.52 %**, балл 38.14, harness OK 161, сабтесты 299 / 2 367. Статусы: OK 160, PASS 1, TIMEOUT 146, ERROR 21, CRASH 2, FAIL 1. Одно убийство по RSS-лимиту (4.02 ГБ, 11:55:41).
- Сырые данные лежат в `p2-work/.tmp/wpt-sw/` (`service-workers.json`, `.log`).
- Скрипты разбора (в `p2-work/.tmp/`):
  - `sw_an.py` — `groups|msgs|subs|list <substr>|test <id>`; для `test` нужен `MSYS2_ARG_CONV_EXCL='*'`;
  - `sw_cl.py` — грубые правила;
  - `sw_scan.py` — статический скан исходника теста;
  - `sw_final.py` — итоговые кластеры, `sum` или `ids <кластер>`.
- Заведены [BUG-1742](../../bugs/BUG-1742-OPEN.md) (кластер C, жизненный цикл) и [BUG-1743](../../bugs/BUG-1743-OPEN.md) (кластер J, фасад iframe по значению). В `BUGS.md` и `STATUS-P3.md` их ещё **нет**.

## Кластеры (`sw_final.py sum`; балл = потерянный потолок, п.п. = балл / 331)

| Кл. | Группа | id | Статусы | Балл | Куда |
|---|---|---|---|---|---|
| A | `*.any.serviceworker.html` 0/0 | 16 | TIMEOUT 16 | 16.0 | [BUG-1226](../../bugs/BUG-1226-OPEN.md) (в очередях **нет** — добавить `BUGS.md:129` в `STATUS-P3.md`). В логе 70 «importScripts: HTTP 404» и сырые scope `https://localhost:18443resources/...` — дописать в BUG-1226 |
| B | обмен сообщениями страница↔SW, Clients | 79 | TIMEOUT 59, OK 20 | 74.6 | фича `SW-MSG` (ROADMAP + STATUS-P1). `ServiceWorker.postMessage` — no-op (`web_api_shim_mid_b.js:475`); `clients.get/matchAll` — заглушки (`sw_worker.rs:92-97`); нет `message` в обе стороны, `ExtendableMessageEvent`, `startMessages`, `openWindow`/`navigate`; `waitUntil` — no-op. Зависит от BUG-1742 |
| C | регистрация и жизненный цикл | 33 | TIMEOUT 17, OK 16 | 27.3 | BUG-1742 |
| D | точность перехвата fetch | 21 | OK 15, TIMEOUT 5, FAIL 1 | 19.8 | новый баг: `respondWith` теряет status и headers (`lib.rs:4731` всегда 200), `preventDefault`/reject не дают network error, у FetchEvent нет `clientId`/`resultingClientId`/`handled`, не-GET не перехватывается, воркеры не перехватываются (`dedicated-worker-service-worker-interception`), sync XHR 404 |
| E | Cache API в окне | 17 | OK 12, TIMEOUT 5 | 14.3 | новый баг: нет глобала `Cache`; `statusText` «OK» вместо ""; тело хранится как «78,101,…» (Uint8Array→строка); нет `url`; `bodyUsed`; не учитываются ignoreSearch/ignoreMethod/ignoreVary, HEAD, фрагмент, Vary; `keys()` не в порядке вставки; не отклоняются 206, !ok, не-GET, вызов без аргументов, `add` после abort; `CacheStorage.has/delete` после `open` дают false |
| F | `caches` в Dedicated/SharedWorker | 23 | ERROR 16, OK 7 | 23.0 | новый баг: `self.caches` undefined, cleanup «reading 'delete'» → ERROR; класс [BUG-1705](../../bugs/BUG-1705-OPEN.md) (`worker.rs`/`shared_worker.rs`) |
| G | IDL | 8 | | 7.1 | новый баг: нет глобалов `ServiceWorker`, `ServiceWorkerContainer`, `NavigationPreloadManager`; `idlharness` 118/410 |
| H | navigation preload | 7 | | 6.2 | фича `SW-PRELOAD-ROUTER` (с H2) |
| H2 | static router `addRoutes` | 13 | TIMEOUT 11 | 13.0 | то же |
| I | модульные SW (`type:'module'`) | 12 | TIMEOUT 9 | 11.7 | фича `SW-MODULE`; в логе 26 «Cannot use import statement outside a module» |
| J | фасад iframe по значению | 64 | OK 54, TIMEOUT 7, ERROR 3 | 58.7 | BUG-1743 |
| K | раннер | 8 | CRASH 2, TIMEOUT 2, OK 4 | 8.0 | шум: BiDi lost (`fetch-event.https.h2.html`, `local-url-inherit-controller`), `LUMEN_WPT_FOREIGN_GRACE_S`, testdriver «element click intercepted» (download, tentative), crashtest `test-wait`. Задач не нужно |
| Z | прочее | 14 | TIMEOUT 13 | 13.2 | canvas-tainting, CSP в SW-скоупе, `close()`, конструктор `ExtendableMessageEvent`, same-site-cookies — ждут B/C |
| — | PASSED | 16 | | 0 | |

Всего потеряно 292.9 балла. Кластеры перекрываются: после фикса J/C всплывут B/D.

## Дальше

1. Подтянуть `origin/main` и проверить свободные номера багов (на 2026-10-27 занято до BUG-1741).
2. Завести 4 бага: D, E, F, G — номера 1744…1747 или следующие свободные.
3. Дописать в BUG-1226 данные кластера A.
4. Строки `BUGS.md` для 1742…1747 (формат строки BUG-1737) и указатели `BUGS.md:NN` в `STATUS-P3.md` в порядке J (1743) → C (1742) → BUG-1226 → F → D → E → G.
5. Строки `ROADMAP.md`: `SW-MSG` (L, Depends BUG-1742), `SW-MODULE` (M), `SW-PRELOAD-ROUTER` (M), формат строки `WEBCRYPTO-SYM`; указатели в `STATUS-P1.md`.
6. Добавить раздел «Разведка WPT-SW-SCOUT» в `docs/wpt-vendor-notes/service-workers.md` с таблицей выше, по образцу `WebCryptoAPI.md`. Строку `service-workers` в `docs/wpt-status.md:340` дополнить итогом и ссылкой на заметку.
7. Строку `WPT-SW-SCOUT` в `ROADMAP.md` перевести в `done`. Удалить `ROADMAP.md:1275` из `STATUS-P2.md`, удалить этот handoff-файл, затем `/lumen-task-finish` (только docs: gate — `python scripts/check_claude_md.py` не нужен; код не менялся).
