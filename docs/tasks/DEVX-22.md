# DEVX-22 — зелёный main: добить чистку тестов и гейтов после 2026-10-22

**Developer:** P6  
**Ветка:** `p6-devx-22`  
**Размер:** M  
**Крейты:** по результатам прогона (layout, js, network — нестабильные тесты)

## Контекст

2026-10-22 сессия Chat по просьбе пользователя («проверь всё, чтобы все тесты работали, файлы —
по правилам») прогнала `cargo test --workspace` и CI-проверки и влила в `main`:

| Что | Ветки (влиты) |
|---|---|
| Гейт размера файлов (`scripts/check_file_sizes.py`, docs/lint-policy.md §5.1): механические разрезы js, network, shell, bidi-server, chrome, core, driver, css-parser, dom, html-parser, layout, paint | `chat-split-js`, `chat-split-js-2`, `chat-split-net`, `chat-split-shell`, `chat-split-misc`, `chat-split-engine` |
| Базовая линия `scripts/file-size-baseline.tsv` (`--update`): понижена под разрезанные файлы, убраны ушедшие под потолок | `chat-file-size-baseline` |
| `check_doc_links.py`: 55 новых битых ссылок | `chat-doc-links` |
| CPU-эталон `graphic_tests/snapshots/cpu/34-forms.png` после BUG-1666 (сверено с Edge) | `chat-cpu-snap-34` |
| `LayoutBox`: итеративный `Clone` (тест `grid_trampoline` рушил стек) | `chat-fix-grid-depth` |
| clippy `lumen-paint` без фич | `chat-fix-paint-clippy` |
| Тесты `<details>` через `checkVisibility` (ожидание после BUG-1191) | `chat-fix-details-summary` |
| BUG-1608, `bug341_s21`, тест подресурсов фрейма (lumen-shell) | `chat-fix-shell-tests` |
| Стек трамплинов table/block-flow (BUG-1266), area-имена в `grid-template-*` (BUG-1663), `@container` без бокса | `chat-fix-layout-tests` |

Заведены: BUG-1684 (P3; сначала проверить, не закрыт ли BUG-1480), DEVX-21 (P1, кэш сборки).

## Что сделать

1. **Полный прогон на свежем `main`, на свободной машине** (без параллельных сборок — под нагрузкой
   часть тестов флакает): `cargo test --workspace --no-fail-fast > .tmp/ws.log 2>&1`, затем
   `grep -E "^test .* FAILED$|test result: FAILED|overflowed" .tmp/ws.log`. Каждое падение
   перепроверить отдельно (`cargo test -p <crate> -- <test> --exact`): детерминированное — чинить
   (корень, не порог) или заводить баг; флак — см. п. 2.
   Внимание: `lumen-js` компилирует тесты только с `--features v8-backend`
   (`cargo test -p lumen-js --features v8-backend --lib -- <filter>`); в `--workspace` фича
   включается сама.
2. **Известные флаки под нагрузкой** (все проходили по одному):
   - `lumen-layout` `block_flow_trampoline::deep_chain_with_margins_collapses_through_the_trampoline` —
     порог 10 с реального времени на 200 000 уровней (1.86 с один, 3.36 с в полном прогоне).
     Заменить абсолютный порог проверкой линейности: время `DEPTH` против `DEPTH/4` растёт ~×4,
     а не ×16 (квадратичность, которую тест и ловит).
   - `lumen-network` `h2::pool::tests::waiter_goes_direct_when_the_handshake_fails`;
     `lumen-js` `v8_runtime::tests::dom_suspend_focus::bounded_document_lock_waits_out_another_thread`;
     `lumen-shell` `chrome_incremental::bug341_s13_hover_flip_…` — найти зависимость от времени /
     общего состояния, починить или завести баг с пробой.
   - BUG-1677 (буфер обмена, code cache в `lumen-js`) — уже в очереди P3, не дублировать.
3. **Пиксели.** Ветки `chat-fix-grid-depth` (Clone), `chat-fix-layout-tests` (трамплины, grid),
   `chat-fix-shell-tests` (`children_to_build` в `box_tree/build.rs`) прогнали только
   `dump_golden.py` (12/12) — полный `python graphic_tests/run.py --continue-on-fail` требует окна
   в фокусе и не запускался. Прогнать его (docs/graphic-tests.md); новые FAIL против прошлого
   прогона — разобрать.
4. **Гейты-скрипты** — все должны быть зелёными на `main`: `python scripts/check_file_sizes.py`,
   `python scripts/check_doc_links.py`, `check_claude_md.py`, `check_crate_names.py`,
   `check_marketing_words.py`, `corpus_c.py check`, `cargo clippy --workspace --all-targets -- -D
   warnings`. Сессии продолжают вливать код — если `check_file_sizes.py` снова красный из-за
   свежего роста, разрезать по docs/tasks/p1-monolith-split-queue.md §2–§3 (только перенос), не
   `--update` ради роста.
5. **Уборка:** удалить worktree `.claude/worktrees/chat-*` (`git worktree remove --force …`), чьи
   ветки влиты в `main` (проверить `git branch -r --merged origin/main`), и влитые ветки
   `origin/chat-*` — только после проверки, что они влиты.
6. Документация: в `docs/commands.md` (раздел про тесты) — строка про `--features v8-backend` для
   `lumen-js`, если её там нет.

## Готово, когда

- `cargo test --workspace --no-fail-fast` на свободной машине: 0 падений, кроме перечисленных
  в BUGS.md открытых багов (с указателями в очередях).
- Флаки п. 2 починены или заведены баги с пробой и указателем.
- Полный `graphic_tests/run.py` прогнан, результат и регенерированные эталоны (если были
  намеренные изменения) — в том же коммите.
- Все гейты п. 4 зелёные; влитые `chat-*` worktree и ветки убраны.
