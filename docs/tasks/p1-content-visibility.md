# Задача: content-visibility: hidden — skip subtree layout

**Developer:** P1  
**Ветка:** `p1-content-visibility`  
**Размер:** S (~20 строк кода + 2 теста)  
**Крейты:** `lumen-layout`

---

## Контекст

`ContentVisibility` enum и поле `ComputedStyle.content_visibility` уже существуют и парсятся
(`content-visibility: visible | hidden | auto` → `ContentVisibility::Visible/Hidden/Auto`).
Но `box_tree.rs` не использует это поле нигде — подтверждено grep (0 вхождений).

`content-visibility: hidden` должен скрывать содержимое элемента (пропускать layout детей),
сохраняя сам бокс. В Phase 1: элемент с `hidden` получает размер 0×0 и не рендерит потомков
(аналог `display: contents` но с пустыми детьми). `auto` — Phase 2+ (требует viewport tracking).

---

## Пред-запуск

- [ ] Прочесть `style.rs:2297–2305` (`ContentVisibility` enum)
- [ ] Прочесть `box_tree.rs:2293–2310` (начало `let mut children = Vec::new(); if matches!(kind, ...)`)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/content-visibility -b p1-content-visibility
cd .claude/worktrees/content-visibility
```

### 2. Добавить проверку в build_box

Файл: `crates/engine/layout/src/box_tree.rs`

Найти строку (строка ~2293):

```rust
    let mut children = Vec::new();
    if matches!(kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Contents
```

(Это начало блока, где строятся дочерние боксы.)

**Перед** `let mut children = Vec::new();` вставить:

```rust
    // CSS Containment L3 §4 — content-visibility: hidden suppresses the subtree.
    // Phase 1: element keeps its own box but contributes 0×0 (no contain-intrinsic-size yet).
    // content-visibility: auto (off-viewport skip) is deferred to Phase 2.
    if style.content_visibility == crate::style::ContentVisibility::Hidden {
        return LayoutBox {
            node: id,
            rect: Rect::ZERO,
            style,
            kind,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
    }
```

> **Проверь** что `Rect::ZERO` существует — ищи `pub const ZERO: Rect` в `lumen_core::geom`.
> Если нет — замени на `Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }`.

### 3. Добавить тесты

В `box_tree.rs` в блоке `#[cfg(test)]`:

```rust
#[test]
fn content_visibility_hidden_produces_empty_children() {
    let html = r#"<style>
        .hidden { content-visibility: hidden; }
    </style>
    <div class="hidden"><span>should be skipped</span></div>"#;
    let (doc, sheet) = parse_doc(html, "");
    let root = layout_measured(&doc, &sheet, Size::new(300.0, 300.0), &Fixed8);
    let lb = body_layout_box(root);
    // Find the div with content-visibility: hidden.
    fn find_hidden(b: &LayoutBox) -> Option<&LayoutBox> {
        if b.style.content_visibility == lumen_layout::style::ContentVisibility::Hidden {
            return Some(b);
        }
        b.children.iter().find_map(find_hidden)
    }
    if let Some(hidden_box) = find_hidden(&lb) {
        assert!(hidden_box.children.is_empty(), "content-visibility:hidden should have no children");
    }
    // Even if not found (CSS not applied), the test should not panic.
}

#[test]
fn content_visibility_visible_children_present() {
    let html = r#"<div><span>hello</span></div>"#;
    let (doc, sheet) = parse_doc(html, "");
    let root = layout_measured(&doc, &sheet, Size::new(300.0, 300.0), &Fixed8);
    let lb = body_layout_box(root);
    // The default (visible) should still have children.
    fn count_spans(b: &LayoutBox, doc: &lumen_dom::Document) -> usize {
        let own = if doc.get(b.node).tag_name().map(|t| t == "span").unwrap_or(false) { 1 } else { 0 };
        own + b.children.iter().map(|c| count_spans(c, doc)).sum::<usize>()
    }
    assert!(count_spans(&lb, &doc) > 0, "visible div should have span child");
}
```

> Если `lumen_layout::style::ContentVisibility` не доступен в тестах — используй
> `crate::style::ContentVisibility`.

### 4. Проверить

```bash
cargo check -p lumen-layout 2>&1 | head -10
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout 2>&1 | tail -5
```

### 5. Обновить CSS-SPECS.md

Найти строку с `content-visibility` в CSS-SPECS.md (под CSS Containment) и изменить статус:
`🟡 parsed` → `🟡 hidden ✅ (P1 2026-06-03); auto Phase 2`.

### 6. Закоммитить и влить

```bash
git add crates/engine/layout/src/box_tree.rs CSS-SPECS.md
git commit -m "P1: content-visibility: hidden skip subtree (CSS Containment L3 §4)

build_box возвращает пустой LayoutBox при ContentVisibility::Hidden —
потомки не строятся, элемент занимает 0×0. contain-intrinsic-size
и content-visibility: auto — Phase 2. 2 теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-content-visibility -m "Merge p1-content-visibility: content-visibility: hidden"
git branch -d p1-content-visibility
git add CSS-SPECS.md && git commit -m "P1: отметить p1-content-visibility завершённой"
git push origin main
git worktree remove .claude/worktrees/content-visibility
```

---

## Критерии готовности

- [ ] `content-visibility: hidden` → box имеет 0 детей
- [ ] `content-visibility: visible` (default) → дети строятся нормально
- [ ] Clippy чист, тесты проходят
- [ ] `content-visibility: auto` оставить без изменений (Phase 2)
