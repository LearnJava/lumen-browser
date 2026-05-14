---
name: lumen-add-css-property
description: >
  Добавляет новое CSS-свойство в Lumen: поле в ComputedStyle, наследование
  в compute_style, парсинг в apply_declaration, тесты, обновление CLAUDE.md
  и lumen-plan.md. Используй когда нужно реализовать CSS property,
  которого ещё нет в lumen-layout.
when_to_use: >
  Фразы-триггеры: "добавь CSS свойство", "реализуй CSS X", "implement CSS property",
  "добавь поддержку X", "нет свойства X в layout". Также автоматически когда
  задача из roadmap [P1] касается нового CSS property.
model: claude-sonnet-4-6
allowed-tools: Read Grep Glob Bash(cargo check *) Bash(cargo test *) Bash(cargo clippy *) Bash(export PATH*)
---

# Добавление CSS-свойства в Lumen

Выполняй шаги последовательно. Каждый шаг — отдельный логический блок.
Не перепрыгивай вперёд без завершения текущего.

## Контекст проекта

!`export PATH="/c/Users/konstantin/.cargo/bin:$PATH" && echo "OK"`

Ключевые файлы:
- `crates/engine/layout/src/style.rs` — `ComputedStyle` (struct ~418), `compute_style` (~639), `apply_declaration` (~2361)
- `crates/engine/css-parser/src/parser.rs` — CSS парсер (если нужны новые типы)
- `crates/engine/layout/tests/snapshot_tests.rs` — snapshot-тесты

Текущее число тестов:
!`export PATH="/c/Users/konstantin/.cargo/bin:$PATH" && cargo test --workspace --quiet 2>/dev/null | tail -3`

## Шаг 1 — Прочитай спецификацию

Определи по названию свойства:
- Модуль CSS (CSS Text L3, CSS Backgrounds L3, CSS Fonts L4, и т.д.)
- Наследуется ли (`inherited: yes/no` в спецификации)
- Тип значения (enum / f32 / Vec / Option<Color> / …)
- Начальное значение (`initial value`)
- Применимость (`applies to: all elements / …`)

Скажи мне (пользователю) что нашёл, прежде чем идти дальше.

## Шаг 2 — Добавь поле в ComputedStyle

Файл: `crates/engine/layout/src/style.rs`, struct `ComputedStyle` (~строка 418).

Правила именования:
- Одно логическое свойство → одно поле (`text_align: TextAlign`)
- Shorthand → несколько полей (`margin_top`, `margin_right`, …)
- Опциональный цвет → `Option<Color>` (None = currentColor)
- Logical collections → `Vec<T>` (box-shadow, text-shadow)

После добавления поля — `cargo check -p lumen-layout` должен пройти
(Rust покажет все места, где нужен Default / PartialEq).

## Шаг 3 — Настрой наследование в compute_style

Файл: `crates/engine/layout/src/style.rs`, fn `compute_style` (~строка 639).

**Если свойство наследуется** — добавь в блок инициализации `ComputedStyle { ... }`:
```rust
my_property: inherited.my_property,
```

**Если НЕ наследуется** — укажи начальное значение (initial value по спеке).
Большинство non-inherited полей инициализируются Default или явным значением.

Проверь: `cargo check -p lumen-layout`

## Шаг 4 — Добавь парсинг в apply_declaration

Файл: `crates/engine/layout/src/style.rs`, fn `apply_declaration` (~строка 2361).

Найди подходящее место в `match property { ... }` и добавь ветку.
Примеры паттернов рядом в том же match:
- Enum-свойство: `"display" => style.display = parse_display(val)`
- Length: `"font-size" => if let Some(l) = parse_length(val) { style.font_size = l.resolve(...) }`
- Color: `"color" => style.color = parse_color(val).unwrap_or(style.color)`
- Vec: `"box-shadow" => if let Some(v) = parse_box_shadow(val) { style.box_shadow = v }`

Если нужен новый enum или парсер — добавь рядом в том же файле.

После добавления: `cargo check -p lumen-layout`

## Шаг 5 — Напиши тесты

Файл: `crates/engine/layout/src/style.rs` (unit) или новый `#[cfg(test)]` блок.

Минимум 4 теста:
1. Базовое значение (`property: value` применяется)
2. Начальное значение (без объявления = initial)
3. Наследование или его отсутствие (проверить через вложенный элемент)
4. Пограничный случай (невалидное значение — ignores, cascade override)

Запусти: `cargo test -p lumen-layout`

## Шаг 6 — Обнови snapshot если нужно

Если добавленное свойство влияет на serialized layout tree (в `snapshot.rs`
есть fn `serialize_layout_tree`) — регенерируй baseline:

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
UPDATE_SNAPSHOTS=1 cargo test -p lumen-layout --test snapshot_tests
```

После регенерации запусти тесты без флага и убедись, что все проходят.

## Шаг 7 — Clippy + полные тесты

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Исправь все warnings. Не коммить с warnings.

## Шаг 8 — Обнови документацию

**[SUBSYSTEMS.md](../../../SUBSYSTEMS.md)** — раздел `lumen-layout 🟡`:
- Добавь свойство в список «Готово» с кратким описанием (одна строка)
- Обнови число тестов
- Если свойство было в «Отложено» — убери оттуда

**`lumen-plan.md`** — шапка «Статус реализации»:
- ⬜ → ✅ (или 🟡 → ✅) для соответствующего пункта

Оба файла обновляются **в том же коммите**, что и сам код.

## Шаг 9 — Коммит

```bash
git add crates/engine/layout/src/style.rs \
        crates/engine/layout/tests/ \
        CLAUDE.md lumen-plan.md
git commit -m "$(cat <<'EOF'
Добавить CSS свойство <name> в lumen-layout

<Короткое зачем: какой сценарий открывает, ссылка на §X спеки.>

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

Заголовок под 80 символов. Тело — зачем, не что (что видно по diff).
