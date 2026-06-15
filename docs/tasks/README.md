# docs/tasks/ — Подробные инструкции к задачам

Каждый файл — одна задача. Формат предназначен для сессии **Haiku** (малый контекст):  
никаких архитектурных решений, точные файлы/строки, готовый код.

---

## Правила

| Правило | Детали |
|---------|--------|
| **Один файл = одна задача** | Не смешивать правки разных фич |
| **Размер** | XS < 20 строк кода · S 20–80 · M 80–200 · всё выше — делить |
| **Ссылка в STATUS** | Каждая задача должна иметь строку в `STATUS-PN.md "Next"` вида `→ [docs/tasks/…]` |
| **Удалять после мержа** | После влития задачи удалять файл (или помечать `## Status: MERGED`) |

---

## Шаблон

```markdown
# Задача: <название>

**Developer:** P<N>  
**Ветка:** `p<N>-<kebab-name>`  
**Размер:** XS / S / M  
**Крейты:** `lumen-XXX`

## Контекст
Почему задача существует. 2–3 предложения.

## Пред-запуск
- [ ] Прочитать: `<file>:<range>` (секция "…")
- [ ] Убедиться, что ветка `main` чиста: `git status`

## Шаги

### 1. Создать ветку и worktree
```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/<kebab-name> -b p<N>-<kebab-name>
cd .claude/worktrees/<kebab-name>
```

### 2. Изменить код
Файл: `path/to/file.rs`

Найти (поиск по строке):
```
<exact search string>
```
Заменить на:
```rust
<new code>
```

### 3. Добавить тесты
В файл `path/to/file.rs` (или `tests/`) добавить:
```rust
#[test]
fn <test_name>() {
    // ...
}
```

### 4. Проверить
```bash
cargo clippy -p lumen-XXX --all-targets -- -D warnings
cargo test -p lumen-XXX
```

### 5. Закоммитить и влить
```bash
git add <files>
git commit -m "P<N>: <сообщение>

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

# Влить в main
git checkout main
git merge --no-ff p<N>-<kebab-name> -m "Merge p<N>-<kebab-name>: <описание>"
git branch -d p<N>-<kebab-name>
git add STATUS-P<N>.md && git commit -m "P<N>: отметить <task> завершённой"
git push origin main
git worktree remove .claude/worktrees/<kebab-name>
```

## Критерии готовности
- [ ] `cargo clippy` чист
- [ ] Тесты проходят
- [ ] В STATUS-PN.md задача перенесена в "Recent"
```
