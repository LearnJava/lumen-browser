---
name: lumen-new-crate
description: >
  Создаёт новый крейт в Cargo workspace Lumen: правильная директория,
  Cargo.toml с workspace-наследованием, минимальный lib.rs, регистрация
  в корневом Cargo.toml. Используй когда plan.md требует новый lumen-* крейт.
when_to_use: >
  Фразы-триггеры: "создай крейт", "новый крейт", "добавь крейт", "new crate",
  "добавить lumen-X", "создать lumen-knowledge", "создать lumen-ai".
  Также когда задача из roadmap явно указывает на появление нового крейта.
disable-model-invocation: true
model: claude-opus-4-7
allowed-tools: Bash(cargo *) Bash(export PATH*) Bash(git *) Read Edit Write Glob
---

# Создание нового крейта в workspace Lumen

$ARGUMENTS — имя крейта без префикса `lumen-`, например `knowledge` → крейт `lumen-knowledge`.
Если аргумент не передан — спроси у пользователя.

Текущий состав workspace:
!`grep 'members' /d/kostja/Lumen-browser/Cargo.toml -A 20 | head -22`

## Шаг 1 — Определи место крейта

Правило расположения из структуры workspace:
- **`crates/engine/`** — крейты движка (парсинг, layout, paint, font, image, encoding, DOM)
- **`crates/`** — верхнеуровневые крейты (shell, core, network, storage, bench)

Таблица принадлежности по программистам (CLAUDE.md):
- P1 → `crates/engine/` (парсеры, layout)
- P2 → `crates/engine/` (font, paint, image)
- P3 → `crates/` (network, storage, knowledge)
- P4 → `crates/shell`, будущий `lumen-ai`

Скажи пользователю куда кладёшь и почему, прежде чем создавать.

## Шаг 2 — Проверь граф зависимостей

Lumen запрещает циклы зависимостей. Однонаправленная цепочка:
```
lumen-core → lumen-dom → lumen-html-parser
                        → lumen-css-parser
                        → lumen-layout → lumen-paint → lumen-shell
lumen-core → lumen-font → lumen-paint
lumen-core → lumen-network, lumen-storage, lumen-encoding, lumen-image
```

Новый крейт:
- Может зависеть от всего, что ниже него в цепочке
- НЕ может зависеть от `lumen-shell` (он финальный бинарь)
- НЕ может создавать цикл

Перечисли планируемые зависимости нового крейта перед созданием.

## Шаг 3 — Создай директорию и Cargo.toml

Пример для `lumen-knowledge` (P3, верхнеуровневый):

```
crates/knowledge/
├── Cargo.toml
└── src/
    └── lib.rs
```

Шаблон `Cargo.toml` (скопируй из lumen-core, замени name и description):

```toml
[package]
name = "lumen-<name>"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Lumen: <одна строка что делает крейт>"

[dependencies]
# Добавляй только то, что реально нужно сейчас.
# Новая external dep = "Why this dependency:" в коммите + обновление CLAUDE.md.
lumen-core.workspace = true
```

Шаблон `src/lib.rs`:

```rust
//! <Однострочное описание крейта.>
```

**Правило политики зависимостей (CLAUDE.md §Политика зависимостей):**
Если в `[dependencies]` появляется что-то кроме lumen-* крейтов — обязателен
комментарий в commit-body:
> **Why this dependency:** <обоснование, почему свой код категорически неуместен>

## Шаг 4 — Зарегистрируй в workspace

Файл: `Cargo.toml` (корень проекта).

1. В секцию `[workspace] members` добавь путь нового крейта.
2. В секцию `[workspace.dependencies]` добавь алиас:
   ```toml
   lumen-<name> = { path = "crates/<path>/lumen-<name>" }
   ```

Порядок в `members` — алфавитный внутри группы (engine/* вместе, top-level вместе).

## Шаг 5 — Проверь сборку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo check -p lumen-<name>
cargo clippy -p lumen-<name> -- -D warnings
```

!`export PATH="/c/Users/konstantin/.cargo/bin:$PATH" && echo "cargo ready"`

Новый крейт должен компилироваться чисто без warnings.

## Шаг 6 — Добавь базовые тесты

В `src/lib.rs` добавь хотя бы один smoke-тест:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // TODO: добавить реальные тесты при первой реализации
    }
}
```

```bash
cargo test -p lumen-<name>
```

## Шаг 7 — Обнови документацию

**`CLAUDE.md`** — раздел «Состояние подсистем»:
- Добавь новый раздел `### lumen-<name> ⬜ (запланировано)` с минимальным описанием
- Обнови счётчик крейтов: «N крейтов» в разделе «Инфраструктура»

**`lumen-plan.md`** — если крейт соответствует пункту roadmap — смени ⬜ → 🟡.

## Шаг 8 — Коммит

```bash
git add crates/ Cargo.toml Cargo.lock CLAUDE.md lumen-plan.md
git commit -m "$(cat <<'EOF'
Создать крейт lumen-<name>: <однострочное зачем>

<Объяснение роли крейта в архитектуре, какие trait-ы планируются,
почему отдельный крейт, а не часть существующего.>

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```
