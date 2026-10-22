# WPT-RUN-16 — заметка передачи (WIP, 2026-10-22)

Ветка `p2-wpt-run-16`, слот `p2-work`.

## Сейчас (2026-10-22 23:40): полный прогон готов, идёт перепрогон 104 шардов «с попапами» — НЕ ПЕРЕЗАПУСКАТЬ

**Полный прогон закончен** (попытка 4, `RUN_EXIT=0`, 22:52): **42.65 %** (29 051 из 68 116), снимок
`docs/wpt/runs/2026-10-22.json` (не закоммичен — лежит в слоте, коммитить вместе с итогом), отчёты
`.tmp/wpt-corpus/`. Статусы: OK 29 690 · PASS 16 309 · FAIL 13 168 · TIMEOUT 5 113 · ERROR 2 806 ·
NOT-RUN 734 · SKIP 178 · CRASH 116 · пустой статус 1 (`html5lib_write_single.html?file=webkit01`).
Сабтесты 379 686 / 1 138 470; убийств сторожем RSS 60 (было 8). Like-for-like со скорингом 08-18 — 20.15 %.
По типам: reftest 14 293 · testharness 12 742 · crashtest 1 829 · print-reftest 187 (новый исполнитель:
в 2026-10-05 все 416 print-reftest были «нет исполнителя»).

**Аудит (`score_audit.py`):** 722 — нет исполнителя (wdspec 626, aamtest 96), **12 необъяснённых** —
объяснены: wptrunner молча выбрасывает тест, когда браузер не стартовал после рестарта под нехваткой
памяти: `Max restarts exceeded` — 10 (css-break 5, html/browsers 3, css-ui 1, referrer-policy 1), гонка
`init_failed in state RunningState` — 1 (editing), трейсбек старта после рестарта — 1 (IndexedDB).
Тест есть в `suite_start`, `test_start` нет. Цена ≤ 12 id = 0.018 п.п. Скрипт — `.tmp/run16_lost.py`.

**Сравнение с 2026-10-05-subdomains** (`.tmp/run16_compare.py`, `.tmp/run16_drops.py` → `.tmp/run16-cmp.txt`,
`.tmp/run16-drops.txt`): рост в основном `css` +5 915.9 (+17.66 п.п.), дальше svg +50, media-source +39,
compat +23, fullscreen +14, mixed-content +12, fetch +11, scroll-animations +11, close-watcher +10.5, dom +10.
Падений больше 1 балла — 18. Вердикты на сейчас:
- `webrtc` −21.05 — **движок, намеренно**: COMPAT-API-01-WEBRTC (`c1eb0435f`, ADR-034) убрал `RTCPeerConnection`.
- `client-hints` −10.17 — **раннер**: гейт user activation (UX-USER-ACTIVATION, `79fcd8923`) включён в
  оконном режиме всегда, `window.open` из скрипта → `null` (67 сабтестов «Popup windows not allowed»).
  Починка в этой ветке: `env["LUMEN_ALLOW_POPUPS"] = "1"` в `tools/wptrunner/wptrunner/browsers/lumen.py`.
  Проверка на одной категории: 30.67 → **47.83** (было 40.17).
- `selection` −8.00 — **движок**: 9 рефтестов `cross-shadow-boundary-*` были ложным PASS до HIGHLIGHT-PAINT;
  выделение не заходит в shadow root → [BUG-1686](../../bugs/BUG-1686-OPEN.md) (+ `BUGS.md:366` в `STATUS-P3.MD`).
- `workers` −5.80 — воспроизводится на том же бинаре без нагрузки (194.53); ждать перепрогона с попапами
  (workers в списке), потом разбирать.
- Перепрогон без нагрузки (`.tmp/wpt-run16-check/`, 6 процессов): client-hints 30.67, selection 69.41 —
  значит не шум памяти.
- `html/syntax` 47 CRASH (сторож RSS, пик 1.2 ГБ на один `html5lib_write.html?file=tests2`) — цене цифры
  < 1 балла: `html5lib_write*` и без CRASH проходят 0 сабтестов. В оговорки, бага не заводить.
- Остальные (cookies −4.05, editing −4.03, quirks −2.47, mediacapture-streams −2.25, connection-allowlist −2.19,
  permissions-policy −2.04, webstorage −2.00, serial −1.85, preload −1.60, WebCryptoAPI −1.55, IndexedDB −1.42,
  webaudio −1.23, webhid −1.09, webusb −1.07) — вердикт после перепрогона. Уже известно: WebCryptoAPI —
  110 тестов `*.any.worker.html` с `crypto is not defined`, но `crypto` в выделенном воркере нет и на
  `d913a39b9` (`worker.rs` `install_worker_scope_globals_v8`, шим `web_api_shim_tail_b.js` в воркер не идёт) —
  не регрессия; потеря скорее в 39 `navigate: wait timeout: DocumentRead…` (шард шёл в своп до 17:33 → раннер).
  quirks — 11 тестов `win.document.open is not a function`: фасад `contentWindow.document`
  (`frame_bridge.rs` `docFacade`) без `open/write/close` и на `d913a39b9` — не регрессия, искать в другом; serial/webhid/webusb —
  TIMEOUT `*-permissions-policy*` без сообщения; editing — `live window closed` (память, раннер).

**Перепрогон «с попапами»** запущен в 23:30 отдельно от сессии: `.tmp/run16-popups.cmd` (тот же бинарь,
`--prefixes` из `.tmp/run16-popup-prefixes.txt` — 104 шарда, 20 088 id, где в исходниках есть `window.open(`,
`clipboard.write`, `requestPermission`; `2 × 10`, out-dir `.tmp/wpt-run16-popups`, лог `.tmp/run16-popups.log`,
в конце `RUN_EXIT=`; при обрыве — тот же cmd с `--resume`). Сумма шардов в полном прогоне — 5 ч машинного времени.

## Что дальше (новая сессия)

1. Дождаться `RUN_EXIT=` в `.tmp/run16-popups.log`. Свести снимок «с попапами» по прецеденту WPT-RUN-9:
   скопировать `.tmp/wpt-corpus` в `.tmp/wpt-corpus-popups`, подменить отчёты 104 шардов (`*.json`,
   `*.raw.jsonl`, `*.log`) из `.tmp/wpt-run16-popups`, пересчитать (`run_corpus.py --all --resume
   --out-dir .tmp/wpt-corpus-popups --run-json docs/wpt/runs/2026-10-22-popups.json` с тем же `--binary`;
   проверить, что ничего не запускает) — две цифры «как снято» / «с попапами».
2. Повторить `.tmp/run16_drops.py` для «с попапами» против 2026-10-05-subdomains, дописать вердикты
   оставшихся падений; падение-движок > 5 баллов → баг + указатель в `STATUS-P3.MD`.
3. `docs/wpt/pass-rate.md`: раздел «Четвёртая полная цифра» (по образцу «Третьей») + 2 строки в журнал;
   `python scripts/quality_report.py`; цифра в `CAPABILITIES.md` и `docs/wpt-status.md`; ROADMAP WPT-RUN-16 → done.
4. Закрыть задачу: коммит (снимки, доки, lumen.py, BUG-1686) → merge в main → убрать `ROADMAP.md:1246` из `STATUS-P2.md`.

Попытка 3 (3 × 14) остановлена в 19:23 по решению пользователя «avail → 0 → 2 × 10». До 17:33 шла сносно
(avail 1.5–6 ГБ, 17 шардов «ran»), в 17:36 свободная память упала до 0 и машина ~1.5 ч стояла в свопе
(лаг памяти 5 312 с, своп 9 ГБ). В этом окне полосы 0 и 1 умерли: `websockets` и `html/syntax` ушли по
бюджету, `taskkill` ещё держал их пустой `wptreport.json`, `os.remove` бросил `WinError 32` и убил поток
полосы; дальше шла одна полоса. Раннер починен (`remove_released` ждёт освобождения файла до 30 с, исключение
шарда в `lane_main` пишется в `failures` и возвращает шард в очередь). `html/browsers` (timeout 6 319 с
посреди свопа) идёт заново через `--retry-timeouts`. Лог попытки 3 — `.tmp/run16-try3.log`.

Запущен в 19:25: `.tmp/run16.cmd --resume --retry-timeouts` (cmd теперь 2 × 10 и дописывает в `.tmp/run16.log`,
а не перезаписывает); 182 wptrunner-процесса осталось из 211. Шарды, прошедшие до свопа (до 17:33), взяты из
кэша — при сравнении с WPT-RUN-9 их падения проверять на «раннер» (шли под нехваткой памяти, `editing` 2× медленнее).

Первые 20 мин попытки 4: avail проседает до 0–1 ГБ кратковременно, лаг до 13 с, своп ~2.5 ГБ — терпимо.
**Кандидат в баг движка:** `html/syntax` — 47 CRASH из 243 к 19:45, почти все от `--max-browser-gb 4`
(браузер 4.2–5.3 ГБ): `html5lib_write.html`, `html5lib_write_single.html`, `html5lib_url.html` (много iframe +
`document.write`). В WPT-RUN-9 у `html/syntax` было 1 убийство по RSS. После прогона — репро на одном
`html5lib_write.html?file=tests2` и `BUG-NNN` в очередь P3, если категория упала больше 5 баллов.

### Попытка 3 (16:17–19:23)

[BUG-1682](../../bugs/BUG-1682-FIXED.md) исправлен (`c0443d20e`), `origin/main` влит в ветку (`4243d0f36`),
бинарь `dev-release` с фиксом — `.tmp/wpt-run16-bin/lumen.exe`. Проверка фикса: `ifr-empty.html` 151–165 МБ,
`navframes.html` 269–320 МБ за 30 с; `referrer-policy/gen/iframe.meta/unset/` под `run_smoke.py` — пик 980 МБ
(до фикса ~2.5 ГБ). Решение пользователя: рецепт WPT-RUN-9 **3 × 14**, при нехватке памяти (avail → 0) —
остановить и продолжить `2 × 10` с `--resume`.

Запущен в 16:17 отдельно от сессии: `.tmp/run16.cmd` (3 × 14, `--run-json docs\wpt\runs\2026-10-22.json`),
лог `.tmp/run16.log` (в конце `RUN_EXIT=`), out-dir `.tmp/wpt-corpus`, память `.tmp/memlog.txt`
(`.tmp/run16_memlog.py`, тоже отдельный процесс). Логи попытки 2 — `.tmp/run16-try2.log`, `.tmp/memlog-try2.txt`.
Первые 15 мин: avail 1.1–5.7 ГБ, своп ~2 ГБ, CPU 100 %, убийств по `--max-browser-gb` нет; `editing` ~2× медленнее
WPT-RUN-9 (1 076 с против 525 с). Полоса сама ждёт, пока свободно меньше `--min-free-gb 6` («waiting» в логе) — это
не зависание. Сводка одной строкой: `python .tmp/run16_status.py <минуты>` (прогресс, avail/своп/лаг, убийства).

Новая сессия: сначала `Get-CimInstance Win32_Process` по `run_corpus` — если прогон жив, только наблюдать;
`RUN_EXIT=` в логе — прогон кончился (при `RUN_EXIT=1` и строках `lane N: …` в конце лога — `--resume` ещё раз),
иначе переходить к шагу 3 «Что дальше»; процесса нет и `RUN_EXIT=` нет — тот же `.tmp/run16.cmd --resume`.
Осиротевшие `lumen.exe` с путём `wpt-run16-bin` после остановки прогона — убивать по PID (не по имени образа).

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
