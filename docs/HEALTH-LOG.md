# HEALTH-LOG — журнал прогонов P5

Append-only журнал свипов здоровья кодовой базы (роль P5, скилл
`/lumen-health-check`). Одна запись на прогон, новые сверху. Фиксирует факт и
дату прогона даже когда свип чистый и не дал отдельных коммитов.

Формат записи: дата · таргет · сводка по подсистемам · что сделано безопасно ·
что заведено задачами.

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
