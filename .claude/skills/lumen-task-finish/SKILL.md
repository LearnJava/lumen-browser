---
name: lumen-task-finish
description: >
  Завершает задачу по протоколу Lumen: clippy + scoped-тесты, синхронизация
  статусных документов, merge --no-ff в main, push, освобождение слота.
  Используй когда задача реализована и готова к слиянию.
when_to_use: >
  Фразы-триггеры: "заверши задачу", "смерджи ветку", "влей ветку", "merge task",
  "задача готова", "ready to merge", "закончил задачу". Также когда все тесты
  проходят и реализация завершена.
model: claude-sonnet-4-6
allowed-tools: Bash(git *) Bash(cargo *) Bash(bash scripts/*) Bash(python scripts/*) Bash(export PATH*) Read Edit
---

# Завершение задачи — протокол merge в Lumen

$ARGUMENTS — имя ветки/задачи (например `font-fallback`).
Если не передан — определи из текущей ветки: `git branch --show-current`

> **Этот скилл — финальный гейт качества (workspace clippy + scoped test).**
> НЕ запускай per-crate `cargo clippy -p … / cargo test -p …` вручную прямо
> перед его вызовом — это двойная оплата за те же крейты. В процессе работы
> достаточно `cargo check`; полную проверку делает скилл один раз ниже.

> **Правило «merge+push после каждого коммита» (пользователь, 2026-08-19)
> означает, что шаги 1–4 уже прогонялись по ходу задачи.** Здесь они закрывают
> хвост последнего коммита. Если после последнего merge в main ничего не
> менялось — сразу к шагу 7.

> **Оба гейта (шаги 1–2) гони СИНХРОННО** — обычный Bash-вызов с
> `timeout: 600000`, НЕ `run_in_background`. Фоновые output-файлы буферизуются
> через пайпы, выглядят пустыми и провоцируют минуты поллинга + повторный
> прогон той же команды (двойная оплата). Вывод пиши в `.tmp/` и фильтруй
> grep-ом по файлу — никогда не перезапускай cargo ради другого фильтра.

## Шаг 1 — Финальный clippy

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
mkdir -p .tmp
cargo clippy --workspace --all-targets -- -D warnings > .tmp/gate-clippy.log 2>&1
tail -5 .tmp/gate-clippy.log            # детали ошибок: grep -E "^error" .tmp/gate-clippy.log
```

> **sccache отключён** с 2026-08-19 — версия 0.15.0 валит компилятор под
> тулчейном 1.97.0 (`.cargo/config.toml`, обёртка закомментирована). Ничего
> прокидывать не нужно. Профиль `dev` (по умолчанию) компилируется быстрее,
> чем `dev-release`; для корректностного гейта **не** навешивай
> `--profile dev-release` — он оправдан только в `graphic_tests/run.py`, где
> важен рантайм рендера.

Если есть warnings — исправь их **до** продолжения. Не делай `#[allow(...)]`
без явной причины.

## Шаг 2 — Тесты затронутых крейтов (scoped)

Шаг 1 (`clippy --workspace --all-targets`) уже **скомпилировал весь workspace** и
поймал кросс-крейтовую поломку сборки. Поэтому здесь гоняем тесты только
затронутых крейтов + их транзитивных обратных зависимостей: на 22 крейта
`test --workspace` — это ~110 отдельных линковок тест-бинарей (~30 мин).

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
bash scripts/scoped-test.sh > .tmp/gate-test.log 2>&1   # база = main; иная: scoped-test.sh <ref>
tail -20 .tmp/gate-test.log             # упавшие тесты: grep -B2 "FAILED\|panicked" .tmp/gate-test.log
```

Тоже синхронно (timeout 600000), не в фоне — см. правило перед шагом 1.
Скрипт сам берёт затронутые пакеты из `git diff` (коммиты ветки + рабочее дерево)
и считает замыкание обратных зависимостей. Правки только в доках/конфигах → тестов нет.

> **Гейт сломан на `lumen-network`: [BUG-805](../../../bugs/BUG-805-OPEN.md).**
> `udp_round_trip` (`crates/network/src/h3/udp.rs`) висит, и скрипт не доходит
> до конца, когда замыкание втягивает этот крейт. **Не жди зависания до
> таймаута и не считай его красным гейтом:** прогони тесты своих крейтов
> адресно (`cargo test -p <crate>`) и напиши в теле коммита, почему общий гейт
> не зелёный. Не «чинить» это правкой чужого теста.

Если тесты падают — исправь. Не коммить красные тесты.

## Шаг 3 — Синхронизируй документы (матрица doc-sync)

Полная матрица — [CLAUDE.md](../../../CLAUDE.md) §«Doc sync rules». Что нужно
почти всегда:

1. **`STATUS-PN.md`** — удали строку-указатель завершённой задачи. Указатель —
   это НОМЕР СТРОКИ в источнике, чужие правки его уводят: сверяй по файлу, а не
   по памяти.
2. **`CAPABILITIES.md` + `subsystems/<crate>.md`** — новая возможность:
   ⬜/🟡 → ✅ и строка в «Готово».
3. **`BUGS.md` → `BUGS-FIXED.md`** — багфикс: **перенеси строку** в архив со
   статусом `FIXED <дата>` (закрытые в `BUGS.md` не остаются с 2026-08-31) и
   **переименуй файл** `bugs/BUG-NNN-OPEN.md` → `-FIXED.md`. О переименовании
   забывают чаще всего; после него почини кросс-ссылки (`grep -rl "BUG-NNN-OPEN"`).
   Перенос строки сдвигает указатели: `python scripts/remap_status_pointers.py --apply`
   (один прогон, до коммита; сам скажет, какой указатель протух и подлежит снятию).
4. **`ROADMAP.md`** — если менялась структура задач, затем
   `python scripts/gen_roadmap.py`. Одна задача = ровно одна строка таблицы:
   перенос строки внутри ячейки молча обрезает всю таблицу ниже.
5. **`CLAUDE.md` §Known gotchas** — только если найдена ЖИВАЯ ловушка.
   Починенный баг оттуда **удаляется**, а не переписывается в «~~было~~ —
   fixed»; переносимый урок метода идёт в `docs/probe-method.md`.

`SYMBOLS.md` генерируемый и не коммитится — держи свежим локально, если
пользуешься (`python scripts/gen_symbols.py`). `docs/roadmap-*.html`
коммитятся (это рукописные вьюверы, генератор лишь подставляет в них данные),
но нести их регенерацию в каждый коммит больше не требуется.

## Шаг 4 — Коммит документации

Если документы не были обновлены в коммите с кодом — отдельный коммит:

```bash
git add -A && git commit -m "Обновить статус задачи <имя>

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

## Шаг 5 — Merge в main

**CI не ждём** (решение пользователя 2026-08-19, [docs/ci-offload.md](../../../docs/ci-offload.md) §8):
локальный гейт — единственная предмерджевая проверка, ожидание прогона стоило
бы ~30 мин на каждый коммит. За красным CI на `main` следим после пуша и чиним
отдельно.

**Важно:** корень часто держит чужие незакоммиченные файлы и блокирует merge.

```bash
git worktree list
```

```bash
# Вариант А — через главный чекаут (если он на main и не конфликтует):
git -C /d/RustProjects/lumen-browser merge --no-ff $ARGUMENTS \
    -m "Влить ветку $ARGUMENTS: <однострочное описание>"

# Вариант Б — если корень блокирует merge: временный worktree.
# Путь ТОЛЬКО внутри папки браузера (/tmp и ../lumen-* запрещены рабочей границей).
# ВНИМАНИЕ: чекаут этого репозитория — ~59 000 файлов, обычный `worktree add`
# не укладывается в таймаут инструмента. Бери sparse без tests/wpt, иначе
# получишь недокачанное дерево и индекс, в котором ВЕСЬ репозиторий помечен
# удалённым (коммит в таком состоянии сносит дерево):
git worktree add --no-checkout --detach .claude/worktrees/merge-$ARGUMENTS main
git -C .claude/worktrees/merge-$ARGUMENTS sparse-checkout set --no-cone '/*' '!/tests/wpt'
git -C .claude/worktrees/merge-$ARGUMENTS checkout
git -C .claude/worktrees/merge-$ARGUMENTS status --short          # ОБЯЗАТЕЛЬНО: должно быть пусто
git -C .claude/worktrees/merge-$ARGUMENTS merge --no-ff $ARGUMENTS \
    -m "Влить ветку $ARGUMENTS: <однострочное описание>"
git -C .claude/worktrees/merge-$ARGUMENTS push origin HEAD:main
git worktree remove .claude/worktrees/merge-$ARGUMENTS   # не забывать: осиротевшие
                                                          # merge-* каталоги копятся
```

`--no-ff` обязателен — сохраняет видимую структуру в `git log --graph`.

## Шаг 6 — Push

```bash
git push origin main
```

Если merge шёл вариантом Б — локальный `main` отстаёт от `origin/main`, и это же
заставит `worktree-pool.sh release` отказать. Освобождать слот тогда через
`git checkout --detach origin/main` внутри слота.

## Шаг 7 — Освободи слот и удали ветку

Порядок важен: пока слот держит ветку, `git branch -d` отказывает
(«cannot delete branch ... used by worktree at ...»).

```bash
bash scripts/worktree-pool.sh release p<N>-work   # detached HEAD, прогретый target/ остаётся
git branch -d $ARGUMENTS
```

Слот **не удаляем** — в нём прогретый `target/`, ради которого пул и сделан.
Удалять надо только ad-hoc worktree, если задача велась в нём:
`git worktree remove .claude/worktrees/$ARGUMENTS`.

Если ветка пушилась в origin — удалить и там:

```bash
git push origin --delete $ARGUMENTS
```

Не пропускать: удалённые ветки копятся (31 штука к 2026-08-31, старейшая с июня).

## Шаг 8 — Проверь результат

```bash
git log --oneline --graph -5
```

Убедись, что merge-коммит виден с правильным сообщением. Сообщи пользователю:
что смержено и в каком состоянии гейт (зелёный / обойдён из-за BUG-805).
