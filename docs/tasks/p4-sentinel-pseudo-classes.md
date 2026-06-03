# Задача: Wire :fullscreen и :popover-open pseudo-classes

**Developer:** P4  
**Ветка:** `p4-sentinel-pseudos`  
**Размер:** XS (2 строки кода + 2 теста)  
**Крейты:** `lumen-layout`

---

## Контекст

`:fullscreen` и `:popover-open` уже парсятся в `PseudoClass` enum, но
`matches_pseudo_class` всегда возвращает `false` для обоих.
JS-стороны уже готовы: `element.requestFullscreen()` выставляет
`data-lumen-fullscreen`, `element.showPopover()` — `data-lumen-popover-open`.
Задача: заменить `false` на проверку атрибута.

---

## Пред-запуск

- [ ] `git status` — убедиться что main чист
- [ ] Ничего читать не нужно — все детали в этом файле

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/sentinel-pseudos -b p4-sentinel-pseudos
cd .claude/worktrees/sentinel-pseudos
```

### 2. Изменить style.rs

Файл: `crates/engine/layout/src/style.rs`

**Правка 1.** Найти точно эту строку (вместе с соседними для контекста):

```rust
        // CSS: :fullscreen — P4: check doc.get_attr(node.id,"data-lumen-fullscreen").is_some()
        PseudoClass::Fullscreen => false,
```

Заменить `PseudoClass::Fullscreen => false,` на:

```rust
        PseudoClass::Fullscreen => doc.get(node).get_attr("data-lumen-fullscreen").is_some(),
```

**Правка 2.** Найти точно эту строку:

```rust
        PseudoClass::PopoverOpen => false,
```

Заменить на:

```rust
        PseudoClass::PopoverOpen => doc.get(node).get_attr("data-lumen-popover-open").is_some(),
```

> **Контекст:** `doc` и `node` — параметры `matches_pseudo_class`. Паттерн
> `doc.get(node).get_attr("attr-name")` используется в той же функции для
> других атрибутов (см. `PseudoClass::Defined`, `PseudoClass::Target` выше).

### 3. Добавить тесты

В том же файле `style.rs`, внутри блока `#[cfg(test)]` (ищи `mod tests {`),
добавить два теста **рядом с существующими тестами pseudo-class** (ищи функцию
`fn hover_matches_hovered_node` или `fn focus_matches_focused_node`):

```rust
#[test]
fn fullscreen_pseudo_matches_sentinel_attr() {
    let html = r#"<div id="el" data-lumen-fullscreen="">content</div>"#;
    let (doc, sheet) = parse_doc(html, "");
    let root = layout_measured(&doc, &sheet, Size::new(200.0, 200.0), &Fixed8);
    // Find the div node.
    let el_id = doc.get_element_by_id("el").expect("el not found");
    let style = compute_style(&doc, el_id, &sheet, &ComputedStyle::default(), Size::new(200.0, 200.0), false);
    // :fullscreen should not affect computed style by itself,
    // but we verify matching works via a rule.
    let html2 = r#"<style>:fullscreen { color: red; }</style><div id="el" data-lumen-fullscreen="">x</div>"#;
    let (doc2, sheet2) = parse_doc(html2, "");
    let el2 = doc2.get_element_by_id("el").expect("el not found");
    let style2 = compute_style(&doc2, el2, &sheet2, &ComputedStyle::default(), Size::new(200.0, 200.0), false);
    assert_eq!(style2.color.r, 255, ":fullscreen rule should apply when sentinel attr present");
}

#[test]
fn popover_open_pseudo_matches_sentinel_attr() {
    let html = r#"<style>:popover-open { color: blue; }</style><div id="p" data-lumen-popover-open="">x</div>"#;
    let (doc, sheet) = parse_doc(html, "");
    let el = doc.get_element_by_id("p").expect("p not found");
    let style = compute_style(&doc, el, &sheet, &ComputedStyle::default(), Size::new(200.0, 200.0), false);
    assert_eq!(style.color.b, 255, ":popover-open rule should apply when sentinel attr present");
    assert_eq!(style.color.r, 0);
}
```

> Если в файле нет хелпера `parse_doc` — ищи `fn parse_doc` в том же `mod tests`.
> Если он называется иначе (например, `parse_html`), адаптируй вызов.
> Если `get_element_by_id` не существует — замени на:
> ```rust
> let el_id = doc.children_of(doc.root())
>     .find(|&n| doc.get(n).get_attr("id").map(|v| v == "el").unwrap_or(false))
>     .expect("el not found");
> ```

### 4. Проверить

```bash
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout 2>&1 | tail -5
```

Ожидаемый результат: `test result: ok. N passed`.

### 5. Обновить STATUS-P4.md

В файле `STATUS-P4.md` найти секцию `### :fullscreen CSS pseudo-class` и
в конце описания добавить строку:

```
- **Статус: WIRED** (p4-sentinel-pseudos, 2026-06-03) — `PseudoClass::Fullscreen` и `PseudoClass::PopoverOpen` проверяют sentinel-атрибуты.
```

### 6. Закоммитить и влить

```bash
git add crates/engine/layout/src/style.rs STATUS-P4.md
git commit -m "P4: wire :fullscreen и :popover-open к sentinel-атрибутам

:fullscreen → doc.get(node).get_attr(\"data-lumen-fullscreen\").is_some()
:popover-open → doc.get(node).get_attr(\"data-lumen-popover-open\").is_some()
Паттерн: JS-side выставляет атрибут, CSS-side его читает.
2 новых теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p4-sentinel-pseudos -m "Merge p4-sentinel-pseudos: wire :fullscreen + :popover-open"
git branch -d p4-sentinel-pseudos
git add STATUS-P4.md && git commit -m "P4: отметить p4-sentinel-pseudos завершённой"
git push origin main
git worktree remove .claude/worktrees/sentinel-pseudos
```

---

## Критерии готовности

- [ ] `PseudoClass::Fullscreen` возвращает `true` когда у элемента есть `data-lumen-fullscreen`
- [ ] `PseudoClass::PopoverOpen` возвращает `true` когда у элемента есть `data-lumen-popover-open`
- [ ] Оба теста проходят
- [ ] Clippy чист
