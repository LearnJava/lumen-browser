# Задача: CLAUDE.md — команда взятия задачи через task-claim.sh

**Developer:** P6
**Ветка:** `p6-claude-md-task-claim`
**Размер:** XS (одна строка в `CLAUDE.md`)
**Крейты:** нет — только документ

## Контекст

2026-10-07 BUG-1240 взяли в работу на двух машинах. Старая проверка брони
(`git pull origin main; git branch -a`) не видела чужую ветку: `pull origin main`
не обновляет `origin/*`, а сверялось точное имя ветки. Коммит `6310aa7e4`
(влит в `33c4a3642`) добавил `scripts/task-claim.sh` и перевёл на него скилл
`/lumen-task-start`, `docs/git-workflow.md`, `docs/dev-roles.md`,
`docs/roles/P6.md` и агента `css-rust-architect`.

Не переведён только `CLAUDE.md` — пункт 3 раздела «Session start and git» до
сих пор велит занимать слот через `worktree-pool.sh` и называет бронью саму
ветку, без пуша и проверки origin. Сессия, которая идёт по `CLAUDE.md`, а не по
`/lumen-task-start`, снова пропустит чужую бронь.

Прошлая сессия (Hermes) правку сделать не смогла: Hermes считает `CLAUDE.md`
защищённым файлом и требует подтверждения в UI, а запрос трижды истёк без
ответа. Запретов в репозитории нет (`.claude/settings.json` правку
`CLAUDE.md` не запрещает), так что Claude Code правит его обычным `Edit`.
Если и у тебя запись упрётся в подтверждение — не обходи его через терминал,
спроси пользователя через `ask-user`.

## Правка

`CLAUDE.md`, раздел «Session start and git», пункт 3. Найти по тексту (номер
строки может съехать, сейчас это строка 34):

```
3. Work in your pool slot: `cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-<task> | tail -1)"`. The branch is the reservation.
```

Заменить целиком на:

```
3. Take the task: `cd "$(bash scripts/task-claim.sh <N> <task-id> | tail -1)"` — fetches origin, refuses if any unmerged branch carries the task id, occupies the pool slot and pushes the branch. Only a branch **on origin** reserves.
```

Больше в `CLAUDE.md` ничего не трогать.

## Шаги

1. `git pull origin main`.
2. Взять задачу новым способом (заодно проверка, что скрипт работает):
   ```bash
   cd "$(bash scripts/task-claim.sh 6 claude-md-task-claim p6-claude-md-task-claim | tail -1)"
   ```
   Если слот `p6-work` занят чужой незакоммиченной работой (там может лежать
   ветка `p6-bug-1315`) — скрипт откажет. Тогда не трогай слот, возьми
   ad-hoc worktree:
   ```bash
   bash scripts/task-claim.sh --check 6 claude-md-task-claim p6-claude-md-task-claim
   git worktree add .claude/worktrees/p6-claude-md -b p6-claude-md-task-claim main
   git push origin p6-claude-md-task-claim
   cd .claude/worktrees/p6-claude-md
   ```
3. Внести правку выше через `Edit`.
4. В том же коммите:
   - удалить этот файл (`docs/tasks/P6-CLAUDE-MD-TASK-CLAIM.md`);
   - удалить строку `docs/tasks/P6-CLAUDE-MD-TASK-CLAIM.md:1` из `STATUS-P6.md`
     (она первая).
5. Проверки:
   ```bash
   python scripts/check_claude_md.py      # лимит 6144 B; сейчас ~5.6 КБ, правка +~110 B
   python scripts/check_doc_links.py
   grep -n "worktree-pool.sh p<N>-work" CLAUDE.md   # должно быть пусто
   ```
6. Коммит (сообщение на русском), например:
   `CLAUDE.md: взятие задачи через task-claim.sh` — в теле: почему (BUG-1240
   взят дважды, CLAUDE.md остался на старой команде).
   Stage явно: `git add CLAUDE.md STATUS-P6.md docs/tasks/P6-CLAUDE-MD-TASK-CLAIM.md`.
7. `merge --no-ff` в `main` → `git push origin main` → удалить ветку локально и
   на origin; если брал ad-hoc worktree — `git worktree remove` его.

## Критерий готовности

- В `origin/main` пункт 3 `CLAUDE.md` содержит `scripts/task-claim.sh`.
- `check_claude_md.py` зелёный.
- Этого файла и его строки в `STATUS-P6.md` больше нет.
