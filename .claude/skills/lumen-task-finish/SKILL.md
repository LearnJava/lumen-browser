---
name: lumen-task-finish
description: >
  Завершает задачу по протоколу Lumen: clippy + тесты, обновление CLAUDE.md
  и lumen-plan.md, merge --no-ff в main, удаление worktree.
  Используй когда задача реализована и готова к слиянию.
when_to_use: >
  Фразы-триггеры: "заверши задачу", "смерджи ветку", "влей ветку", "merge task",
  "задача готова", "ready to merge", "закончил задачу". Также когда все тесты
  проходят и реализация завершена.
model: claude-sonnet-4-6
allowed-tools: Bash(git *) Bash(cargo *) Bash(export PATH*) Read Edit
---

# Завершение задачи — протокол merge в Lumen

$ARGUMENTS — имя ветки/задачи (например `font-fallback`).
Если не передан — определи из текущей ветки: `git branch --show-current`

> **Этот скилл — финальный гейт качества (workspace clippy + test).**
> НЕ запускай per-crate `cargo clippy -p … / cargo test -p …` вручную прямо
> перед его вызовом — это двойная оплата за те же крейты. В процессе работы
> достаточно `cargo check`; полную проверку делает скилл один раз ниже.

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

> sccache + rust-lld уже включены глобально в `.cargo/config.toml` — отдельно
> прокидывать не нужно. Профиль `dev` (по умолчанию) быстрее компилируется,
> чем `dev-release`, для корректностного гейта — НЕ навешивай `--profile dev-release`
> на clippy/test (он оправдан только в `graphic_tests/run.py`, где важен рантайм рендера).

Если есть warnings — исправь их **до** продолжения. Не делай `#[allow(...)]`
без явной причины.

## Шаг 2 — Тесты затронутых крейтов (scoped)

Шаг 1 (`clippy --workspace --all-targets`) уже **скомпилировал весь workspace** и
поймал кросс-крейтовую поломку сборки. Поэтому здесь гоняем тесты только
затронутых крейтов + их транзитивных обратных зависимостей, а не весь workspace:
на 22 крейта `test --workspace` — это ~110 отдельных линковок тест-бинарей (~30 мин;
замеры в памяти `project_build_test_perf_findings`).

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
bash scripts/scoped-test.sh > .tmp/gate-test.log 2>&1   # база = main; иная: scoped-test.sh <ref>
tail -20 .tmp/gate-test.log             # упавшие тесты: grep -B2 "FAILED\|panicked" .tmp/gate-test.log
```

Тоже синхронно (timeout 600000), не в фоне — см. правило перед шагом 1.
Скрипт сам берёт затронутые пакеты из `git diff` (коммиты ветки + рабочее дерево)
и считает замыкание обратных зависимостей. Правки только в доках/конфигах → тестов нет.

Если тесты падают — исправь. Не коммить красные тесты.

> Полный `cargo test --workspace` не нужен принудительно: если правка трогает
> корневой крейт (`lumen-core` и т.п.), замыкание само раскроется почти на весь
> workspace. `lumen-driver` (64 тест-бинаря) втягивается почти всегда — его
> консолидация вынесена отдельной задачей в `STATUS-P1.md`.

## Шаг 3 — Обнови lumen-plan.md

В файле `lumen-plan.md`:

1. В блоке `## 🔄 В работе сейчас` — удали строку резервации этой задачи.
   Если осталось пусто — восстанови `_(никто ничего не зарезервировал)_`.

2. В блоке `## Статус реализации` — смени маркер:
   - `⬜` → `✅` (или `🟡` → `✅`) для реализованного пункта
   - Если частично — оставь `🟡` с пометкой что готово

## Шаг 4 — Обнови CLAUDE.md

В файле `CLAUDE.md`:

1. **[SUBSYSTEMS.md](../../../SUBSYSTEMS.md)** — расширь раздел крейта:
   - В «Готово» добавь что реализовано (одна строка на фичу)
   - Обнови число тестов (`N тестов`)
   - Если пункт был в «Отложено» — убери

2. **«История последних merge-ов»** — добавь в начало списка:
   ```
   *            <имя-ветки>           — <одна строка что сделано>
   ```

3. **«Roadmap — что предстоит»** — если пункт реализован, удали его.

4. **[DECISIONS.md](../../../DECISIONS.md)** — если приняли архитектурное решение в ходе задачи,
   добавь запись.

## Шаг 5 — Коммит обновлений документации

Если CLAUDE.md / lumen-plan.md не были обновлены в коммите с кодом —
сделай отдельный коммит:

```bash
git add CLAUDE.md lumen-plan.md
git commit -m "Обновить статус задачи <имя> в плане и CLAUDE.md

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

## Шаг 6 — Merge в main

**Важно:** main должен быть свободен (не занят другим worktree с uncommitted changes).

Проверь:
```bash
git worktree list
```

!`git worktree list`

Merge выполняется через временный worktree на main **или** через основной
клон, если он не занят:

```bash
# Вариант А — через основной клон (если main свободен):
git -C /d/kostja/Lumen-browser checkout main
git -C /d/kostja/Lumen-browser merge --no-ff $ARGUMENTS \
    -m "Влить ветку $ARGUMENTS: <однострочное описание>"

# Вариант Б — через временный detached worktree:
git worktree add --detach /tmp/lumen-merge-$ARGUMENTS main
git -C /tmp/lumen-merge-$ARGUMENTS merge --no-ff $ARGUMENTS \
    -m "Влить ветку $ARGUMENTS: <однострочное описание>"
git worktree remove /tmp/lumen-merge-$ARGUMENTS
```

`--no-ff` обязателен — сохраняет видимую структуру в `git log --graph`.

## Шаг 7 — Удали ветку и worktree

```bash
git worktree remove .claude/worktrees/$ARGUMENTS
git branch -d $ARGUMENTS
```

Если `git branch -d` отказывает (ветка не полностью смержена) —
убедись что merge прошёл успешно, затем `-D` вместо `-d`.

## Шаг 8 — Проверь результат

```bash
git log --oneline --graph -5
```

!`git log --oneline --graph -5`

Убедись что merge-коммит виден с правильным сообщением.
Сообщи пользователю: что смержено, сколько тестов теперь в workspace.
