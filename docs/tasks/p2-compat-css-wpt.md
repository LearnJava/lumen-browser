# COMPAT-CSS-WPT — заметка передачи между сессиями (P2)

Ветка: `p2-compat-css-wpt` (на origin, от `origin/main` e0c276e88). Строка очереди: `ROADMAP.md:1160`.
Статус: базовая линия снята, **отчёт, `.ini`, баги и закрытие строки ещё не сделаны**.

## Главная находка: testharness по BiDi сломан на `main` (не связано с задачей, но блокирует её)

Любой testharness-id в `run_corpus.py`/`run_report.py` на текущем `main` даёт `TIMEOUT`
(«browser did not answer a BiDi call within 20s»; в логе лумена нет `→ GET` — navigate
до главного потока не доходит). Один процесс — 100% TIMEOUT, при `--processes 7` примерно
треть id проходит случайно. Reftest (`--ipc-server`) не затронуты.

- Бисект `main` (8f777aca9 хороший → e0c276e88 плохой, 8 шагов): первый плохой коммит
  **27e264dc5 UX-LOADING** (индикатор загрузки вкладки и кнопка «Остановить»). Бинарь 07.10
  проходит `compat/historical.html` за 9,7 с.
- Сужение: блок в `about_to_wait.rs` (`chrome_loading_shown`) не причина (убрал — всё ещё TIMEOUT).
  Причина — строка в `crates/shell/src/chrome_ui.rs` (~404, модель вкладки хрома):
  `loading: Some(t.id) == active_id && self.nav_start.is_some()`. Замена на `loading: false`
  (и больше ничего) на чистом `main` возвращает `historical.html` в OK за 9,5 с, а полный
  `compat/` — в стабильные 14 с.
- Скорее всего та же причина, что BUG-1629 (живое окно: первый кадр приходит ровно по
  истечении таймаута `wait`). Не проверено в живом окне.

Починку не делал (код шелла, UX-LOADING — P6, баги — P3). Воспроизведение для фиксера:
`loading: false` в chrome_ui.rs → пересборка `cargo build --profile dev-release --bin lumen`
→ прогон из раздела «Команда» ниже.

## Базовая линия `compat/` (бинарь: `main` e0c276e88 + `loading: false`, три прогона подряд идентичны)

`16 из 38` id по шкале wpt.fyi (41,68%), подтестов `50/138`; статусы: FAIL=16, OK=13, PASS=9.
Полностью зелёных id 13: 9 reftest PASS + 4 testharness (`historical` 3/3,
`webkit-box-removing-triggering-anonymous-merge` 1/1, `webkit-gradient-sign` 2/2,
`webkit-scrollbar-parse` 1/1). Reftest исполнялись впервые: 25 id, 9 PASS / 16 FAIL.
Для сравнения: прежний run_report — 13 testharness-id из 39, 9/71 подтестов.

Провалы и привязка (нужна проверка текста строк при составлении отчёта):

| id | результат | строка CSS-SPECS / BUG |
|---|---|---|
| `css-style-declaration-alias-enumeration` | 0/51 | CSS-SPECS.md:781 (`-webkit-*` алиасы) |
| `idlharness.window` | 23/29, 6 FAIL: `window.orientation`, `onorientationchange` (Window, HTMLBodyElement) | **нет строки → завести BUG** |
| `webkit-mask-box-enumeration` | 0/6 (`webkitMaskBox*`) | **нет строки → завести BUG** (`-webkit-mask-box-image*` в CSS-SPECS нет) |
| `webkit-box-blockify`, `-horizontal-reverse-variants`, `-horizontal-rtl-variants`, `-ignores-flex-wrap.tentative`, `-rtl-flex` (reftest FAIL); `webkit-box-item-shrink-001` 12/20, `-002` 5/18; `webkit-box-vertically-centered` 0/1 | | CSS-SPECS.md:785 (legacy flexbox) |
| `webkit-linear-gradient-line-{top,right,bottom,left}` (reftest FAIL); `webkit-gradient-comma` 1/2; `webkit-radial-gradient-radii` 2/3 | | CSS-SPECS.md:784 |
| `webkit-text-fill-color-property-001a/b/c/d, 002, 004, 006` (reftest FAIL); `webkit-text-fill-color-currentColor` 0/1 | | CSS-SPECS.md:786 |

Зелёные reftest: `webkit-background-origin-text`, `-box-align-001`, `-box-clamp-bottom-border`,
`-box-clamp-visibility-change`, `-box-fieldset`, `-box-z-index`, `-linear-gradient-diff-unprefixed`,
`-text-fill-color-property-003`, `-005`.

Старые `.ini` в `tests/wpt/metadata/compat/` устарели: `run_report --check` показал 18 неожиданных PASS
(`webkit-box-item-shrink-001/002` — сузить ожидания), `idlharness.window.js.ini` ждёт TIMEOUT (теперь OK),
`webkit-scrollbar-parse.html.ini` ждёт FAIL (теперь OK), `webkit-gradient-sign` OK.

## Что осталось сделать

1. Собрать бинарь с `loading: false` (см. выше; не коммитить эту правку!). Для холодного слота нужен
   `RUSTY_V8_ARCHIVE` на копию `p2-work/target/dev-release/gn_out/obj/rusty_v8.lib` (потом удалить `.tmp/rusty_v8.lib`).
   Бинарь уже собран в `target/dev-release/lumen.exe` этого worktree в состоянии «`loading: false`»;
   если `target/` цел, пересборка не нужна — проверить `historical.html` одиночным прогоном (OK за ~10 с).
2. Завести баги (свободны BUG-1630…): (а) testharness по BiDi стоит из-за UX-LOADING (находка выше; в
   `BUGS.md` после строки BUG-1629, файл `bugs/BUG-1630-OPEN.md`, перекрёстная ссылка с BUG-1629; новую строку
   поставить в начало `STATUS-P3.md`: без неё любой WPT-срез недействителен); (б) `webkitMaskBox*`;
   (в) `window.orientation` / `onorientationchange`. Файл `BUGS.md` содержит сырые CR — править только `Edit`/`sed`.
3. Обновить `.ini` в `tests/wpt/metadata/compat/`: `run_report.py --all --root compat --recursive --update-expected`
   (testharness) и вручную `expected: FAIL` для 16 FAIL-reftest; затем `--check` без отклонений.
4. Отчёт `docs/wpt/compat.md` (числа выше, таблица привязки, команда повтора, сноска про BUG-1630);
   снимок `--run-json docs/wpt/runs/2026-10-14-compat.json`.
5. `ROADMAP.md:1160` → done с числами; убрать строку из `STATUS-P2.md`; `/lumen-task-finish`
   (clippy не нужен — Rust не менялся; `scripts/scoped-test.sh` и `python graphic_tests/dump_golden.py` по CLAUDE.md).
6. Убрать `.claude/worktrees` хвосты по памяти (`worktree-pool.sh gc`).

## Команда

```
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
D:/RustProjects/lumen-browser/.claude/worktrees/p2-work/tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_corpus.py --binary '<abs path>\target\dev-release\lumen.exe' \
  --prefixes compat --out-dir .tmp/wpt-compat --processes 7 --skip-manifest-update \
  --run-json .tmp/wpt-compat/run.json
```

(`MANIFEST.json` не отслеживается git: при первом прогоне без `--skip-manifest-update` он пересоздаётся, ~40 с.)
Слот `p2-work` не взять: локальный `main` в главном дереве отстал от origin, `worktree-pool.sh` считает чужие
слитые коммиты «невлитыми» — сессия работала в собственном worktree на ветке `p2-compat-css-wpt`.
