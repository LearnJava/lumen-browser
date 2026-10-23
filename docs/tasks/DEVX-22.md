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

## Состояние (WIP, сессия P6, 2026-10-23)

Ветка `p6-devx-22`. **Сделано:**
- П. 1: `cargo test --workspace --no-fail-fast` на `afaa0feda` (с `CARGO_INCREMENTAL=0` — диск D: почти полон, `incremental` в `target/debug` рос до 7 ГБ и ронял сборку): 22 538 ok, 2 FAILED — оба `lumen-js`, оба BUG-1677 (`navigator_clipboard_read_asks_and_returns_items`, `perf9_code_cache_full_still_stores_new_entry`), дубликатов не заводить.
- П. 2: флаки починены в коде — `deep_chain_with_margins…` (порог 10 с → рост времени ×4 глубины < ×10, лучшее из двух мелких прогонов); `h2::pool::waiter_goes_direct_when_the_handshake_fails` (вместо `sleep(50ms)` ждёт `cfg(test)`-счётчик `parked`); `bounded_document_lock_waits_out_another_thread` (держатель не отпускает замок, пока главный поток не проверил `try_lock`). `bug341_s13_…` **не воспроизведён** (25 прогонов под 3 CPU-хогами + 4 полных прогона `lumen-shell`, всё зелёное; счётчики thread-local, гонки не видно) — оставить как есть, при повторе под нагрузкой завести баг с логом `{last:?}`.
- П. 3 (пиксели, `LUMEN_PROFILE=debug`, слот не умеет dev-release по диску): полный `run.py --continue-on-fail` дважды обрывался `IPC WinError 10054` на TEST-08 / TEST-134 (по одному оба проходят; причина не найдена — возможно, чужой `taskkill lumen.exe`). Против прошлого прогона (`5b8345773`): **TEST-18** DEBTOR 6.09 → 2.08 % (улучшение; baseline в `KNOWN_DEBTORS_IPC` снижен до 2.08, строка 673 `run.py`); **TEST-61** (view-transitions) DEBTOR 3.26 → **FAIL 10.81 %** — регрессия VT-API (`d30f7bec7`, 21.10): колбэк `startViewTransition` теперь идёт `setTimeout(0)`, а IPC-снимок (`render_current_page_to_png` берёт `self.display_list`) его не видит — страница остаётся в состоянии «до»; бинарь от 20.10 даёт 3.26 %, любой от 22.10 — 10.81 %. Заметка: в IPC-режиме и обычный `setTimeout(800)`, меняющий DOM, в снимке не виден даже через 2.5 с (проба — страница с логом таймера) — вероятно, перерасклад идёт только в `redraw_requested`, а окно скрыто.

**Осталось:**
1. Завести `BUG-NNN` (следующий номер в `BUGS.md`) на TEST-61 / IPC-снимок не видит правок DOM от таймеров, с указателем в очередь владельца (P1, VT-API-2, или P3); либо починить: перед `Screenshot` тикнуть таймеры, дрейнить VT-события и сделать `relayout`. Решение про baseline 61 — только вместе с багом.
2. Повторить полный `graphic_tests/run.py --continue-on-fail` (foreground, на dev-release/debug), убедиться, что TEST-134…165 проходят (обрыв IPC на 134 — перепроверить), закоммитить `results/*.json` и baseline-правку.
3. П. 4 — гейты-скрипты: `check_file_sizes.py`, `check_doc_links.py`, `check_claude_md.py`, `check_crate_names.py`, `check_marketing_words.py`, `corpus_c.py check`, `cargo clippy --workspace --all-targets -- -D warnings`.
4. П. 5 — уборка влитых `chat-*` worktree/веток (`git branch -r --merged origin/main`).
5. П. 6 — строка про `--features v8-backend` в `docs/commands.md`, если её нет.
6. Гейт перед закрытием: `cargo clippy -p lumen-layout -p lumen-network -p lumen-js --all-targets -- -D warnings` (lumen-js — с `--features v8-backend`), затем `/lumen-task-finish`, удалить указатель `ROADMAP.md:1252` из `STATUS-P6.md`, ROADMAP-строку DEVX-22 → `done`.

**Как проверить сделанное:** `cargo test -p lumen-layout --lib deep_chain_with_margins`, `cargo test -p lumen-network --lib h2::pool`, `cargo test -p lumen-js --features v8-backend --lib bounded_document_lock`.
