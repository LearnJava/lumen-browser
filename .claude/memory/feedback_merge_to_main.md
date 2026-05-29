---
name: Merge to main after task completion
description: всегда сливать ветку в main после завершения задачи
type: feedback
originSessionId: dc5387dd-66d8-40ae-8fd5-7976d99ac95d
---
**Правило:** После завершения любой задачи (feature ветка завершена, тесты прошли, код готов) — сразу слить ветку в main через `git merge --no-ff`.

**Why:** Оставленные не слитые ветки — это долг техдолга. Они загромождают список ветвей, затрудняют отслеживание статуса, и создают путаницу в параллельных сессиях (какие ветки активны, какие брошены).

**How to apply:** В конце каждой сессии:
1. `cargo clippy` + `cargo test` — убедиться что всё чистое
2. `git merge --no-ff <branch>` в main
3. `git branch -d <branch>` — удалить локальную ветку
4. Обновить `STATUS-PN.md` — очистить "In progress", переместить задачу в "Recent"
5. При наличии доступа — `git push origin main`
