---
name: Complete task workflow — merge, delete, push
description: полный workflow завершения задачи: merge, delete branch, update STATUS, push
type: feedback
originSessionId: dc5387dd-66d8-40ae-8fd5-7976d99ac95d
---
**После завершения любой задачи выполнять ВСЕ 7 шагов по порядку:**

```bash
# 1. Убедиться что код готов
cargo clippy -p <crate> -- -D warnings
cargo test -p <crate>

# 2. Слить в main с --no-ff
git checkout main
git merge --no-ff p<N>-task-name -m "Merge p<N>-task-name: описание"

# 3. УДАЛИТЬ ветку сразу после merge
git branch -d p<N>-task-name

# 4. Обновить STATUS-PN.md на main
# — убрать из "In progress"
# — переместить задачу в "Recent" 
git add STATUS-PN.md
git commit -m "P<N>: отметить task-name как завершённую"

# 5. Пушить в origin
git push origin main

# 6. ВЫЙТИ из worktree и УДАЛИТЬ его
git worktree remove .claude/worktrees/<task-name>

# (автоматически вернёт сессию в исходную директорию)
```

**Why:** Если пропустить удаление ветки (шаг 3) и обновление STATUS (шаг 4) — ветки накапливаются, теряется история завершённых задач, параллельные сессии не видят что уже сделано. На 2026-05-28 накопилось 7 старых веток, потому что эти шаги пропускались.

**How to apply:** Создать чек-лист в конце каждой сессии:
- [ ] `cargo clippy` pass
- [ ] `cargo test` pass
- [ ] `git merge --no-ff` в main
- [ ] `git branch -d <branch>` — удалить ветку
- [ ] `git add STATUS-PN.md` + коммит обновления
- [ ] `git push origin main`
- [ ] `git worktree remove .claude/worktrees/<task-name>` — удалить worktree

Не считать задачу завершённой, пока все 7 пунктов не сделаны. **Оставленный worktree блокирует другие сессии** (они не смогут `git checkout main`).
