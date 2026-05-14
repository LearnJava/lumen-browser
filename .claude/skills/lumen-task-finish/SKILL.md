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
disable-model-invocation: true
model: claude-sonnet-4-6
allowed-tools: Bash(git *) Bash(cargo *) Bash(export PATH*) Read Edit
---

# Завершение задачи — протокол merge в Lumen

$ARGUMENTS — имя ветки/задачи (например `font-fallback`).
Если не передан — определи из текущей ветки: `git branch --show-current`

## Шаг 1 — Финальный clippy

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo clippy --workspace --all-targets -- -D warnings
```

!`export PATH="/c/Users/konstantin/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5`

Если есть warnings — исправь их **до** продолжения. Не делай `#[allow(...)]`
без явной причины.

## Шаг 2 — Полные тесты

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo test --workspace
```

Текущий счётчик тестов:
!`export PATH="/c/Users/konstantin/.cargo/bin:$PATH" && cargo test --workspace --quiet 2>/dev/null | grep "test result" | head -20`

Если тесты падают — исправь. Не коммить красные тесты.

## Шаг 3 — Обнови lumen-plan.md

В файле `lumen-plan.md`:

1. В блоке `## 🔄 В работе сейчас` — удали строку резервации этой задачи.
   Если осталось пусто — восстанови `_(никто ничего не зарезервировал)_`.

2. В блоке `## Статус реализации` — смени маркер:
   - `⬜` → `✅` (или `🟡` → `✅`) для реализованного пункта
   - Если частично — оставь `🟡` с пометкой что готово

## Шаг 4 — Обнови CLAUDE.md

В файле `CLAUDE.md`:

1. **«Состояние подсистем»** — расширь раздел крейта:
   - В «Готово» добавь что реализовано (одна строка на фичу)
   - Обнови число тестов (`N тестов`)
   - Если пункт был в «Отложено» — убери

2. **«История последних merge-ов»** — добавь в начало списка:
   ```
   *            <имя-ветки>           — <одна строка что сделано>
   ```

3. **«Roadmap — что предстоит»** — если пункт реализован, удали его.

4. **«Decisions log»** — если приняли архитектурное решение в ходе задачи,
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
