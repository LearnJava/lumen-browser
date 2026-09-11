# HEALTH-LOG — журнал прогонов P5

Append-only журнал свипов здоровья кодовой базы (роль P5, скилл
`/lumen-health-check`). Одна запись на прогон, новые сверху. Фиксирует факт и
дату прогона даже когда свип чистый и не дал отдельных коммитов.

Формат записи: дата · таргет · сводка по подсистемам · что сделано безопасно ·
что заведено задачами.

---

## 2026-09-11 — `full` (clippy + stubs + branches + docs + deps)

Ветка: `p5-health-2026-09-11`. Холодный слот (`target/` пуст) — свип занял
~13 мин на сборку clippy (sccache 81.47% hit rate на этот прогон, узкое место —
линковка + сборка v8 150.1.0, оба шага sccache не покрывает).

### clippy — OK
`cargo clippy --workspace --all-targets -- -D warnings`: 0 ошибок, 0
предупреждений. Все `crates/*/Cargo.toml` и `crates/engine/*/Cargo.toml`
несут `[lints] workspace = true` — ярус 0 не потёк.

### stubs — 46 unreachable!()/todo!(), 103 `// CSS:`, 390 OPEN
Все 46 срабатываний `unreachable!()` — exhaustive-match заглушки на местах,
где вариант структурно недостижим (ожидаемый паттерн, не забытый хвост).
Реальных `todo!()`/`unimplemented!()` в production-коде нет — единственное
совпадение было текстом внутри doc-комментария
(`crates/core/src/ext.rs:2847`), описывавшим `BrowserSession` как
«Phase 0: trait + todo!() stubs. Real implementations come with 8A.2+ tasks.»
— дрейф: трейт реализован в четырёх местах (`InProcessSession`,
`WinitSession`, `LiveWindowSession`, `NullBrowserSession`), 8A.7 (SDC)
закрыта. Комментарий исправлен.

103 `// CSS:` хэндофа в `lumen-layout`/`lumen-paint` — большинство трекается
через `CSS-SPECS.md` по имени свойства, а не через `STATUS-P4.md:file:line`
(там сейчас только 4 file:line указателя). 1:1 сверка всех 103 — отдельная
дорогая ревизия, не входит в этот прогон; если понадобится — заводить
отдельным `dupes`-подобным таргетом.

390 `OPEN` в `BUGS.md` — трек P3, без изменений.

### branches — 10 убрано
Влито и удалено 9 веток (не были ни в одном worktree): `merge-tmp-bug529`,
`merge-tmp-bug975`, `merge-tmp-fontload10`, `merge-tmp-wpt7slice31`,
`p1-layout1-blockflow-iterative`, `p1-layout2-flex-trampoline`,
`p1-layout2-flex-trampoline2`, `p1-layout2-table-trampoline`,
`p3-bug1026-margin-collapse-quadratic`. Плюс удалён смёрженный чистый
worktree+ветка `claude/lumen-skill-state-architecture-4c5100` (осиротевший,
HEAD совпадал с ancestor main). `p1-thread3-hangs-slice7` — тоже `--merged`,
но занята активным worktree `p1-work` (upstream `: gone]`) — не трогали,
это чужой слот.

### docs — regenerated, 1 дрейф исправлен
`gen_symbols.py`/`gen_roadmap.py` — оба без ошибки, дерево не обрезано
(`docs/roadmap-B-twotrees.html`: 888 `"id":`). `docs/roadmap-*.html` были
на день позади (сгенерены 2026-09-10, open 382/fixed 613) — перегенерены на
текущий `ROADMAP.md`/`BUGS.md` (open 166/fixed 829, отражает мержи P1/P3/P6
за последние сутки). `docs/plan/phases.md` маркеры (✅/🟡/⬜) сверены с
`git log --oneline -20` — дрейфа не найдено.

### deps — OK
`cargo tree -d`: 25 групп дублирующихся версий (`bitflags` 1/2, `hashbrown`
×3, `nom` 7/8, `thiserror` 1/2, `windows`/`windows-sys` ×2, и т.д.) — все
транзитивные, в основном заданы апстримом (wgpu/winit/resvg тянут разные
поколения), унификация не в руках P5. Выборочная сверка последних коммитов,
добавлявших `[dependencies]` (LIB-6 `url`, LIB-4 `resvg`, LIB-2 `rustybuzz`)
— во всех есть «Почему этот новый dependency» в теле коммита, политика
соблюдается.

### Сделано безопасно
- 10 веток/worktree удалены (список выше).
- `crates/core/src/ext.rs:2847` — исправлен дрейфующий doc-комментарий
  (`BrowserSession` больше не «Phase 0 stub»).
- `docs/roadmap-B-twotrees.html`, `docs/roadmap-svg-cleaves.html` —
  перегенерены (`gen_roadmap.py`).

### Заведено задач
_(нет)_ — все находки этого прогона либо безопасно почищены сразу, либо
информационные (дубли зависимостей, объём `// CSS:` хэндофов), без владельца
для отдельной задачи.

---

## 2026-09-03 — `docs` (чистка протухших брифов `docs/tasks/`)

Ветка: `p5-tasks-cleanup`. Повод — пользователь спросил про 65 файлов в
`docs/tasks/`; по инварианту (`dev-roles.md` §Task tracking schema) бриф
существует только пока задача не сделана.

Сверка (два независимых прохода, второй — по CAPABILITIES.md/коду, а не
только по тексту ROADMAP): **27 из 64** брифов описывали уже закрытые задачи —
15 там, где ROADMAP явно ставит `done` на все под-id темы, 12 там, где ROADMAP
описывает ту же задачу словами `done`, но без обратной ссылки на файл (пример:
`ph3-http3.md` — «Not started, placeholder», хотя `P3-h3` в ROADMAP `done` с
реальным QUIC-транспортом, влитым 2026-07-07). Остальные 37 — законные: тема
ещё открыта (`HAS-ROW`), это будущий Phase 3/4 бэклог (`NO-ROW-STILL-OPEN`),
либо файл-очередь дорожки с не до конца закрытыми под-id.

**Удалено 27 файлов** (`git rm`), см. полный список в коммите. Побочно
исправлены две протухшие строки, найденные по пути:
- `CAPABILITIES.md:121` утверждала, что variable fonts «Not wired into the live
  femtovg window» — код (`femtovg_backend.rs::draw_text_run`) вызывает
  `draw_varied_text` с 2026-06-23 (BUG-109), проверено чтением кода.
- `docs/automation.md:5` держала `DEVX-7…16` как `planned` — в ROADMAP.md все
  одиннадцать `done`.

**Убрано ссылочное разрушение.** Удаление 27 файлов сломало 19 живых markdown-
ссылок (`ROADMAP.md`, `docs/automation.md`, три ADR, `subsystems/ai.md`,
`docs/tasks/README.md`, `docs/tasks/p2-wpt-runner-throughput.md`) — все
починены точечно, где аргумент был содержательным (не просто ссылка, а часть
рассуждения) — переписан своими словами вместо простого вычёркивания. Гейт
`check_doc_links.py` (заведён тем же аудитом накануне) — зелёный, база
обновлена (`--update`). Прозаические упоминания без markdown-скобок (в
`bugs/*-FIXED.md` и `done`-строках самого ROADMAP) намеренно не тронуты — тот
же класс, что уже 94 архивных записи в базовой линии, и ратчет их не отслеживает.

**Не удалено — уроки:** файл-очередь `p1-monolith-split-queue.md` и подобные
(SPLIT/TEST/WPT-RUN) — родитель ещё не `done`, часть под-срезов открыта.
`ph3-webtransport.md` блокирует себя отсутствием живого QUIC, хотя QUIC уже
есть (`P3-h3`, done с 2026-07-07) — стоит перепроверить, разблокирована ли
задача фактически; в ROADMAP не правил, решение за владельцем P1.

---

## 2026-09-03 — `docs` (аудит иерархии документации после разгрузки CLAUDE.md)

Ветка: `p5-doc-audit`. Повод — разгрузка CLAUDE.md 2026-09-03 (394→158 строк)
перенесла знание в профильные файлы, но не перенесла ссылки на него и оставила
часть правил в двух копиях. Инструмент — обход графа ссылок по 155 живым `.md`
(без `bugs/`-архива, worktree и логов).

**Найдено и закрыто — противоречия, дававшие агенту два разных ответа:**

| Что | Где расходилось | Решение |
|---|---|---|
| Охват `clippy` | `commands.md:87` («workspace в финальном гейте») против `commands.md:140` («never `--workspace`») — в одном файле, 53 строки врозь; CLAUDE.md и `dev-roles.md` повторяли вторую версию, скилл завершения реализует первую | Охват задаётся фазой работы, не ролью |
| Схема учёта задач | вторая копия в `docs/tasks/README.md` предписывала ручную переиндексацию 183 указателей вместо `remap_status_pointers.py` | Копия удалена, `dev-roles.md` — единственный источник |
| Протокол завершения | чеклист «7 шагов» против скилла; чеклист велел ровно тот повторный прогон, который скилл запрещает в своей шапке | Исполняемый источник — скилл; в git-workflow остался инвариант |
| Закрытие бага | `REVIEW.md` требовал флип `OPEN`→`FIXED` на месте (протокол сменился 2026-08-31 на перенос в `BUGS-FIXED.md`) | Приведён к действующему |
| Трейлер коммита | четыре разных значения в обращении | Пин заменён на «модель, писавшая коммит» |

**Найдено и закрыто — инструкции, срабатывавшие вхолостую:**

- `/lumen-task-start` читал и правил блок `## 🔄 В работе сейчас` в
  `lumen-plan.md`. Блока нет — файл 21 строка оглавления. Резервация задачи по
  действующему протоколу это сама ветка `p<N>-…`.
- Чеклист в `git-workflow.md` шагом 4 делал прямой коммит в `main`, который
  раздел «Forbidden» того же файла запрещает.

**Проверено и НЕ изменено (важнее находок):** `STATUS-P5.md` выглядит как дрейф
— 107 строк прозы против инварианта «только строки-указатели». Приведение его к
инварианту сломало бы планирование роли: `scripts/orchestrator.py::has_tasks`
штатно разбирает два формата, и `has_tasks("P5")` держится на таблице в секции
`## Next`. Пустой файл вернул бы `False` молча. Исключение записано явно в обе
стороны вместо «починки».

**Заведено на будущее:** `scripts/check_doc_links.py` + базовая линия
(`lint-policy.md` §5.2). 185 битых ссылок зафиксированы поимённо, новая роняет
гейт. Класс растёт сам: переименование `BUG-NNN-OPEN.md` → `-FIXED.md` при
каждом закрытии бага ломает ссылки на старое имя — отсюда 149 из 185.

Задач для P1–P4 не заведено: всё найденное — документация, домен P5. Rust и
манифесты не тронуты, поэтому clippy/тесты не гонялись.

---

## 2026-08-19 — `branches` (CI-4: осиротевшие каталоги в `.claude/worktrees/`)

Ветка: `p5-ci4-orphan-worktrees`. Разбор четырёх непустых каталогов, которых нет
в `git worktree list` (админ-каталогов под `.git/worktrees/` у них нет, поэтому
`git worktree prune` их не видел и не удалял).

| Каталог | Размер | Файлов | Содержимое |
|---|---|---|---|
| `tbank-fixes` | 314 МБ | 61 937 | `tests/wpt/` + `tools/wptrunner/` + `workspace-hack/`, без `.git` |
| `merge-custom-elements` | 98 МБ | 24 994 | то же |
| `p2-ua-client-hints-merge-tmp` | 73 МБ | 14 594 | то же |
| `mt-m3-2-1c-7` | 40 МБ | 1 601 | полный чекаут; `.git` указывал на несуществующий `.git/worktrees/mt-m3-2-1c-7` |

**Проверка на уникальное содержимое перед удалением** (все три метода — до `rm`):

1. Три WPT-огрызка: все 101 525 путей существуют в `main`. Содержимым отличались
   9 / 5 / 5 файлов (`README.md`, `VENDOR.md`, `run_report.py`, `run_smoke.py`,
   `browsers/lumen.py`, `executors/executorlumen.py`, пара `.ini`) — все
   отслеживаются git, в `main` версии новее. Устаревшие снимки, не потерянная работа.
2. `mt-m3-2-1c-7`: `git hash-object` по всем 1 598 файлам + `git cat-file --batch-check`
   — **все блобы уже есть в объектной базе**. Своё только `.tmp/gate-clippy.log`
   и `.tmp/gate-test.log`.
3. Ссылок на эти пути в скриптах/доках/конфигах нет (только в логах чата
   `.claude-manager/`).

**Сделано:** удалены все четыре по явному согласию пользователя — освобождено
~525 МБ и ~103 тыс. файлов. Строка CI-4 снята со `STATUS-P5.md`.

**Заведено:** ничего.

**Приём на будущее:** «осиротевший каталог в `.claude/worktrees/`» ≠ «мёртвый
worktree». `git worktree prune` чистит только админ-записи под `.git/worktrees/`;
каталог без `.git` (или с `.git`, указывающим в пустоту) он не трогает вовсе, так
что такие остатки копятся молча. Дешёвая проверка на потерянную работу —
`git hash-object` + `git cat-file --batch-check`: если все блобы уже в базе,
уникального содержимого нет.

---

## 2026-07-02 — `docs` (аудит документации + чистка)

Ветка: `p5-docs-cleanup`. Полный аудит ~90 md-файлов (4 параллельных ревизора + ручная сверка спорных находок с кодом).

| Подсистема | Итог |
|---|---|
| **удалено** | `docs/plan/history.md` (deprecated-заглушка), `docs/plan/roadmap.md` (76 КБ архив, путал с корневым ROADMAP.md), `SESSION-HANDOFF-2026-06-27.md` (разовый handoff), брифы выполненных задач `docs/tasks/ph3-indexeddb.md` (MERGED 2026-06-25) и `rp-3-gzip-deflate.md` (RP-3 done) |
| **дрейф** | Закрыты все 8 пунктов «Known doc-drift» из CAPABILITIES.md: image.md (GIF/AVIF из Deferred → Done), paint.md (femtovg ⬜→✅ default), dom.md (+contenteditable.rs), js.md (+coverage note ~90 API), network.md (cookie jar/SOCKS5 из Deferred), storage.md (+SW store/CacheStorage), css-parser.md (+at-rules, 229→316 тестов), phases.md:31 (streaming → ✅ PH1-2) |
| **ROADMAP** | P2-usability `active`→`ready` (все подзадачи done, U-6 ready); P3-bfcache/P3-navapi `planned`→`active` (незавершённые ветки `p1-ph3-*`, см. запись 2026-07-01) |
| **ссылки** | Битые ссылки на удалённые файлы почищены: lumen-plan.md, doc-sync.md, commands.md, CLAUDE.md, docs/tasks/README.md (RP-секция: брифы rp-1/2/4 уже отсутствовали), ph3-tier2-web-apis.md, ph3-cdp-shim.md |
| **прочее** | CSS-SPECS.md Quick stats пересчитан (2026-05-24 → 2026-07-02: ✅~237/🟡~135/⬜~114); спекулятивные брифы `p2-view-transitions-l1-full.md`, `p2-wpt-integration.md` помечены «not tracked in ROADMAP.md» |

**Не тронуто (нужны отдельные решения):** переименование `docs/plans/` vs `docs/plan/`; реструктуризация changelog-стиля `subsystems/js.md`/`layout.md`; папка `docs/roles/` с единственным P1.md.

---

## 2026-07-01 — `full` (clippy + stubs + branches + docs + deps)

Ветка: `p5-health-2026-07-01`.

| Подсистема | Итог |
|---|---|
| **clippy** | Был КРАСНЫЙ (BUG-264 OPEN) — теперь OK. `crates/engine/paint/src/renderer.rs` (wgpu-рендер, feature-gated под `--workspace`): убран лишний `;` в макросе `flush_batch!` (16 `redundant_semicolons`), удалена неиспользуемая `rec2020_gamma_decode` (`dead_code`), усечены 14 float-литералов rec2020/P3-матриц (`excessive_precision`, `cargo clippy --fix`). Вслед за этим вскрылись и починены ещё два: `clippy::len_zero` в `crates/bidi-server/src/protocol.rs:2006` и `clippy::too_many_arguments` на `run_window_mode` (`crates/shell/src/main.rs:546`, `#[allow(...)]` как в BUG-263). `cargo clippy --workspace --all-targets -- -D warnings` теперь чист. |
| **stubs** | `todo!()`/`unimplemented!()` в проде нет (только историческое упоминание в doc-комментарии `ext.rs:2477`, реализация давно есть). Все `unreachable!()` — легитимные match-guard'ы. `// CSS:` хэндофы (~100) — стабильный P4-бэклог, без новых висящих указателей. |
| **branches** | Удалено 5 влитых веток/worktree: `graphic-followup-baseline-font-parity`, `graphic-followup-debtors`, `graphic-followup-local` (только шумовой `results/latest.json`), `p1-laguna-t1-140314`, `p1-laguna-t1-143746` (обе — пустые leftover, см. память). **Оставлены нетронутыми** (не `--merged`, по решению пользователя после ревью): `p1-ph3-bfcache` и `p1-ph3-navapi` — по 1 неслитому коммиту поверх точки 99–100 коммитов позади main. Ревью diff'ов показало: работа реальна и НЕ задублирована на main (main до сих пор содержит именно те заглушки, что эти ветки закрывают — см. docs). Задача реинтеграции заведена P1. |
| **docs** | `SYMBOLS.md` актуален после регенерации (сдвиг строк из-за правок renderer.rs в этом же свипе). STATUS-P3/P4 указатели сверены с текущим BUGS.md/CSS-SPECS.md — актуальны, drift нет (последняя P5-сессия уже почистила STATUS-P4/SUBSYSTEMS 2026-07-01, commit a7db572d). **Найден отдельный дрейф**: `docs/tasks/ph3-bfcache.md` и `docs/tasks/ph3-navigation-history-api.md` утверждали «Shell-side freeze/thaw implemented» / «Phase 2a … DONE» со ссылкой на «Merged slice» — по факту это работа только веток `p1-ph3-bfcache`/`p1-ph3-navapi`, в main НЕ смерджена (см. branches). Оба файла поправлены: статус явно помечен «NOT on main», добавлены указатели на stale-ветки и на конкретные заглушки в `crates/shell/src/main.rs`. |
| **deps** | Дубли версий только транзитивные, неустранимые силами P5: `bitflags` 1↔2, `hashbrown` 0.14/0.15/0.16/0.17, `getrandom` 0.2↔0.3, `foldhash` 0.1↔0.2, `thiserror` 1↔2, `webpki-roots` 0.26↔1.0, `windows`/`windows-core`/`windows-result` двух версий, `glow` 0.13↔0.16. Единственный новый `[dependencies]` за последние ~20 коммитов — `lumen-driver.workspace = true` в `crates/bidi-server/Cargo.toml` (SDC-2, cf837fe0) — внутренний workspace-крейт, не подпадает под правило «Why this dependency» (оно для внешних crates.io зависимостей). |

### Сделано безопасно
- Удалены 5 влитых веток + их worktree (см. branches выше), `git worktree prune`.
- `SYMBOLS.md` регенерирован (следствие правок renderer.rs).
- BUG-264 → FIXED; попутно найдены и закрыты BUG-265, BUG-266 (тот же класс: workspace-clippy drift).
- Поправлен дрейф статуса в `docs/tasks/ph3-bfcache.md` и `docs/tasks/ph3-navigation-history-api.md` (см. docs выше).

### Заведено задач
- `STATUS-P1.md`: `docs/tasks/ph3-bfcache.md:9`, `docs/tasks/ph3-navigation-history-api.md:74` — реинтегрировать freeze/thaw (branch `p1-ph3-bfcache`) и `navigate_to_key`/`traverseTo(key)` (branch `p1-ph3-navapi`) в текущий main (rebase, не прямой merge — ~100 коммитов дрейфа).

---

## 2026-06-03 — `full` (clippy + stubs + branches + docs + deps)

Ветка: `p5-health-log-2026-06-03`.

| Подсистема | Итог |
|---|---|
| **clippy** | OK — `cargo clippy --workspace --all-targets -- -D warnings` чистый (0 ошибок, 0 предупреждений). Сборка с нуля 7m27s. |
| **stubs** | `todo!()`/`unimplemented!()` в проде нет. Все `unreachable!()` — легитимные guard'ы в `match`. Висящих `// CSS:` без хозяина нет — каждый actionable-хэндоф (subgrid, `:fullscreen`, `image-set`, dark-mode, 3D transforms) имеет указатель `crates/...:line` в STATUS-P4. |
| **branches** | Удалять P5 нечего. `p2-shape-outside` влита в main, но worktree активен — незакоммиченные правки + новый `shapes.rs` (живая сессия P2, не стейл). Зомби/осиротевших worktree нет. `gone]`-веток нет. |
| **docs** | `SYMBOLS.md` актуален (2899 символов, 20 крейтов) — реальный дрейф отсутствует, только шум CRLF↔LF, откатан. Маркеры lumen-plan.md и указатели STATUS расхождений с `git log` не дали. |
| **deps** | Дубли версий только транзитивные и неустранимые силами P5: `bitflags` 1↔2, `thiserror` 1↔2, `hashbrown` 0.14/0.15/0.16/0.17, `getrandom` 0.2↔0.3, `foldhash` 0.1↔0.2, `webpki-roots` 0.26↔1.0. Provisional crypto-deps уже покрыты заведённой задачей P1 на ADR (коммит c3f1f80). |

**OPEN-баги (на P3):** BUG-054 (network: `stale_pooled_connection_triggers_retry` падает на Windows, WSAECONNRESET), BUG-055 (layout: `<picture>` AVIF→fallback возвращает `.avif`). Оба pre-existing.

### Сделано безопасно
- Откат шумового CRLF-изменения `SYMBOLS.md` (контент актуален).
- Заведён этот журнал `docs/HEALTH-LOG.md`.

### Заведено задач
- Нет новых. Кодовая база здорова: clippy чист, стабов нет, доки актуальны.
