# Задача: @scope cascade wiring

**Developer:** P1  
**Ветка:** `p1-scope-cascade`  
**Размер:** S (~50 строк + 3 теста)  
**Крейты:** `lumen-layout`

---

## Контекст

CSS-парсер уже парсит `@scope (<root>) [to (<limit>)] { rules }` в `Stylesheet.scope_rules`
(CSS Cascade L6). Но функция `compute_style` в `style.rs` никогда не итерирует
`sheet.scope_rules` — все scope-правила молча игнорируются.

После этой задачи `@scope (.card) { color: blue; }` будет применяться к потомкам `.card`.

---

## Пред-запуск

- [ ] Прочесть `style.rs` строки 4816–4840 (блок `for supports in &sheet.supports_rules`)
  — это образец, который нужно скопировать и адаптировать
- [ ] Прочесть `style.rs` строки 7727–7736 (`fn is_self_or_ancestor`)
- [ ] Убедиться: `lumen_css_parser::parse_selector_list` экспортирована  
  (`grep "pub fn parse_selector_list" crates/engine/css-parser/src/lib.rs`)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/scope-cascade -b p1-scope-cascade
cd .claude/worktrees/scope-cascade
```

### 2. Добавить вспомогательную функцию node_is_in_scope

Файл: `crates/engine/layout/src/style.rs`

Найти функцию `fn is_self_or_ancestor(` (строка ~7727) и **перед ней** вставить:

```rust
/// CSS Cascade L6 §5.1 — true when `node` is a descendant of (or is) an element
/// matching any selector in `root_sel_str`. Empty `root_sel_str` → always true
/// (implicit scope = document root, i.e. the rule applies everywhere).
fn node_is_in_scope(doc: &Document, node: NodeId, root_sel_str: &str) -> bool {
    if root_sel_str.trim().is_empty() {
        return true;
    }
    let selectors = lumen_css_parser::parse_selector_list(root_sel_str);
    if selectors.is_empty() {
        return false;
    }
    // Walk node and its ancestors; return true if any matches the scope root.
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == doc.root() { break; }
        for complex in &selectors {
            if matches_complex(complex, doc, n) {
                return true;
            }
        }
        cur = doc.get(n).parent;
    }
    false
}
```

### 3. Добавить @scope loop в compute_style

Файл: `crates/engine/layout/src/style.rs`

Найти блок (строка ~4840):

```rust
    }
    // Inline-style declarations подключаются с `is_inline = true` и
```

(Это конец `for supports in &sheet.supports_rules { }` блока.)

Вставить **между** закрывающим `}` supports-блока и строкой `// Inline-style`:

```rust
    // CSS Cascade L6 §5 — @scope rules: apply only when node is in scope.
    for scope_rule in &sheet.scope_rules {
        if !node_is_in_scope(doc, node, &scope_rule.root) {
            next_rule_idx += scope_rule.rules.len();
            continue;
        }
        // Scope limit: if `to (<limit>)` is set, skip nodes that are
        // descendants of the limit selector *within* this scope.
        if let Some(ref limit_sel) = scope_rule.limit {
            if node_is_in_scope(doc, node, limit_sel) {
                next_rule_idx += scope_rule.rules.len();
                continue;
            }
        }
        for rule in &scope_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, next_rule_idx, decl_idx, decl));
                }
            }
            next_rule_idx += 1;
        }
    }
```

> **Важно:** переменные `layer_n`, `next_rule_idx`, `matched` уже существуют в scope —
> они инициализируются выше в `compute_style` вместе с media/supports loops.

### 4. Добавить тесты

В блоке `#[cfg(test)]` в `style.rs`:

```rust
#[test]
fn scope_rule_applies_to_descendant() {
    // @scope (.wrapper) { color: blue; } applies to .child inside .wrapper.
    let html = r#"<style>@scope (.wrapper) { .child { color: blue; } }</style>
    <div class="wrapper"><span class="child">x</span></div>"#;
    let (doc, sheet) = parse_doc(html, "");
    // Find .child
    let child_id = doc.all_elements().into_iter()
        .find(|&n| doc.get(n).get_attr("class").map(|v| v == "child").unwrap_or(false))
        .expect(".child not found");
    let style = compute_style(&doc, child_id, &sheet, &ComputedStyle::default(),
                               Size::new(400.0, 400.0), false);
    assert_eq!(style.color.b, 255, "scope rule should apply to .child inside .wrapper");
}

#[test]
fn scope_rule_does_not_apply_outside() {
    // @scope (.wrapper) { color: blue; } does NOT apply to .child outside .wrapper.
    let html = r#"<style>@scope (.wrapper) { .child { color: blue; } }</style>
    <div class="child">x</div>"#;
    let (doc, sheet) = parse_doc(html, "");
    let child_id = doc.all_elements().into_iter()
        .find(|&n| doc.get(n).get_attr("class").map(|v| v == "child").unwrap_or(false))
        .expect(".child not found");
    let style = compute_style(&doc, child_id, &sheet, &ComputedStyle::default(),
                               Size::new(400.0, 400.0), false);
    assert_eq!(style.color.b, 0, "scope rule should NOT apply outside .wrapper");
}

#[test]
fn scope_rule_empty_root_applies_everywhere() {
    // @scope { color: red; } (no root) applies to any element.
    let html = r#"<style>@scope { span { color: red; } }</style><span>x</span>"#;
    let (doc, sheet) = parse_doc(html, "");
    let span_id = doc.all_elements().into_iter()
        .find(|&n| doc.get(n).tag_name().map(|t| t == "span").unwrap_or(false))
        .expect("span not found");
    let style = compute_style(&doc, span_id, &sheet, &ComputedStyle::default(),
                               Size::new(400.0, 400.0), false);
    assert_eq!(style.color.r, 255, "empty-root scope should apply everywhere");
}
```

> Если `doc.all_elements()` не существует — используй:
> ```rust
> fn find_by_class(doc: &lumen_dom::Document, cls: &str) -> lumen_dom::NodeId {
>     let mut stack = vec![doc.root()];
>     while let Some(n) = stack.pop() {
>         if doc.get(n).get_attr("class").map(|v| v == cls).unwrap_or(false) {
>             return n;
>         }
>         stack.extend(doc.children_of(n));
>     }
>     panic!("{cls} not found");
> }
> ```

### 5. Проверить

```bash
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout 2>&1 | tail -5
```

### 6. Обновить CSS-SPECS.md

Найти строку `CSS Nesting (scope)` и изменить `⬜` → `🟡` с пометкой
`@scope root matching ✅ (P1 2026-06-03); limit/inner-scope — Phase 2`.

### 7. Закоммитить и влить

```bash
git add crates/engine/layout/src/style.rs CSS-SPECS.md
git commit -m "P1: @scope cascade wiring (CSS Cascade L6 §5)

node_is_in_scope() проверяет предков node на соответствие root-селектору.
compute_style итерирует sheet.scope_rules аналогично media/supports.
scope.limit обрабатывается через повторный вызов node_is_in_scope.
3 теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-scope-cascade -m "Merge p1-scope-cascade: @scope cascade"
git branch -d p1-scope-cascade
git add CSS-SPECS.md && git commit -m "P1: отметить p1-scope-cascade завершённой"
git push origin main
git worktree remove .claude/worktrees/scope-cascade
```

---

## Критерии готовности

- [ ] `@scope (.wrapper) { .child { color: blue; } }` применяется к `.child` внутри `.wrapper`
- [ ] `@scope (.wrapper) { .child { color: blue; } }` НЕ применяется к `.child` вне `.wrapper`
- [ ] `@scope { ... }` (без root) применяется ко всем элементам
- [ ] Clippy чист, 3 теста проходят
