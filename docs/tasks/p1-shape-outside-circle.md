# Задача: shape-outside: circle() в FloatContext

**Developer:** P1  
**Ветка:** `p1-shape-outside-circle`  
**Размер:** M (~100 строк кода + 3 теста)  
**Крейты:** `lumen-layout`

---

## Контекст

CSS `shape-outside` парсится и хранится как `ShapeOutside::Value(String)` в `ComputedStyle`,
но `FloatContext` (структура float-layout) её полностью игнорирует — реализован комментарий
«no shape-outside wrapping» (`box_tree.rs:2726`). Задача: реализовать `circle(r)` —
самый распространённый случай — в `FloatContext.left_edge_at/right_edge_at`.

---

## Пред-запуск

- [ ] Прочесть `crates/engine/layout/src/box_tree.rs:2728–2788` (`FloatContext` struct + impl)
- [ ] Прочесть `crates/engine/layout/src/box_tree.rs:3253–3269` (размещение floats, `add_left`/`add_right`)
- [ ] Прочесть `crates/engine/layout/src/style.rs:2863–2870` (`ShapeOutside` enum)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/shape-outside-circle -b p1-shape-outside-circle
cd .claude/worktrees/shape-outside-circle
```

### 2. Расширить FloatContext

Файл: `crates/engine/layout/src/box_tree.rs`

Найти строку:

```rust
struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    right: Vec<(f32, f32)>,
}
```

Заменить на:

```rust
struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    right: Vec<(f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: circle(r)` overrides.
    /// `(top_y, bottom_y, is_left, center_x, center_y, radius)`.
    /// `is_left=true` → left float, `false` → right float.
    shape_circles: Vec<(f32, f32, bool, f32, f32, f32)>,
}
```

### 3. Обновить FloatContext::new()

Найти:

```rust
    fn new() -> Self {
        Self { left: Vec::new(), right: Vec::new() }
    }
```

Заменить на:

```rust
    fn new() -> Self {
        Self { left: Vec::new(), right: Vec::new(), shape_circles: Vec::new() }
    }
```

### 4. Обновить left_edge_at и right_edge_at

Найти функцию `fn left_edge_at`:

```rust
    fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
            .fold(default_x, f32::max)
    }
```

Заменить на:

```rust
    fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
            .fold(default_x, f32::max);
        // CSS Shapes L1: override rectangular edge with circle boundary.
        self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| *is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx + hw
            })
            .fold(rect_edge, f32::max)
    }
```

Найти функцию `fn right_edge_at`:

```rust
    fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
            .fold(default_x, f32::min)
    }
```

Заменить на:

```rust
    fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
            .fold(default_x, f32::min);
        // CSS Shapes L1: override rectangular edge with circle boundary.
        self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| !is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx - hw
            })
            .fold(rect_edge, f32::min)
    }
```

### 5. Добавить вспомогательную функцию parse_circle_radius

В том же файле `box_tree.rs`, **перед** `impl FloatContext` или рядом с ней:

```rust
/// CSS Shapes L1 §5.1 — parse `circle(<length-px>)` from a raw shape string.
/// Returns the radius in px. Only handles `circle(Npx)` without `at` clause.
/// Returns `None` for any unrecognised syntax (fallback to rectangular float).
fn parse_circle_px(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("circle(")?.strip_suffix(')')?;
    let token = inner.split_whitespace().next()?;
    // Accept "50px" or bare "50" (assume px).
    let digits = token.strip_suffix("px").unwrap_or(token);
    digits.parse::<f32>().ok().filter(|&r| r > 0.0)
}
```

### 6. Подключить parse_circle_px при размещении float

Найти блок `FloatSide::Left =>` (строка ~3254):

```rust
                        match child.style.float_side {
                            FloatSide::Left => {
                                let lx = fc.left_edge_at(child_y, content_x);
                                child.rect.x = lx + fml;
                                child.rect.y = child_y + fmt;
                                fc.add_left(child_y + fmt + fh + fmb, lx + fml + fw + fmr);
                            }
                            FloatSide::Right => {
                                let rx = fc.right_edge_at(child_y, container_right);
                                child.rect.x = rx - fmr - fw;
                                child.rect.y = child_y + fmt;
                                fc.add_right(child_y + fmt + fh + fmb, rx - fmr - fw - fml);
                            }
```

Заменить весь блок `match child.style.float_side { ... }` на:

```rust
                        match child.style.float_side {
                            FloatSide::Left => {
                                let lx = fc.left_edge_at(child_y, content_x);
                                child.rect.x = lx + fml;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let right_edge = lx + fml + fw + fmr;
                                fc.add_left(bot_y, right_edge);
                                // CSS Shapes L1 — wire circle(r) shape-outside.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, true, cx, cy, r));
                                    }
                                }
                            }
                            FloatSide::Right => {
                                let rx = fc.right_edge_at(child_y, container_right);
                                child.rect.x = rx - fmr - fw;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let left_edge = rx - fmr - fw - fml;
                                fc.add_right(bot_y, left_edge);
                                // CSS Shapes L1 — wire circle(r) shape-outside.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, false, cx, cy, r));
                                    }
                                }
                            }
```

> **Важно:** весь блок `match child.style.float_side { FloatSide::None => unreachable!() }` остаётся как есть — добавляй только внутрь `FloatSide::Left` и `FloatSide::Right`.

### 7. Добавить тесты

В `box_tree.rs` в `#[cfg(test)]` блок:

```rust
#[test]
fn parse_circle_px_valid() {
    assert_eq!(parse_circle_px("circle(50px)"), Some(50.0));
    assert_eq!(parse_circle_px("circle(0px)"), None);
    assert_eq!(parse_circle_px("circle(10)"), Some(10.0));
    assert_eq!(parse_circle_px("CIRCLE(30PX)"), Some(30.0)); // case-insensitive
}

#[test]
fn parse_circle_px_invalid() {
    assert_eq!(parse_circle_px("none"), None);
    assert_eq!(parse_circle_px("ellipse(30px 20px)"), None);
    assert_eq!(parse_circle_px("polygon(0 0, 10 0, 10 10)"), None);
}

#[test]
fn shape_outside_circle_left_edge_narrows() {
    // Left float with shape-outside: circle(50px) in a 100px wide, 100px tall box.
    // At y = float center (50px from float top), the circle's right edge extends
    // to center_x + radius = 50 + 50 = 100 — same as margin box. But at y = top,
    // half-width = sqrt(50^2 - 50^2) = 0, so left edge would be at center_x = 50.
    let mut fc = FloatContext::new();
    // Simulate a 100×100 left float at x=0, y=0.
    fc.add_left(100.0, 100.0); // rectangular entry (margin box right edge = 100)
    // Add circle: center at (50, 50), radius 50.
    fc.shape_circles.push((0.0, 100.0, true, 50.0, 50.0, 50.0));
    // At y=50 (circle center): right edge = 50 + sqrt(50^2 - 0) = 100.
    assert!((fc.left_edge_at(50.0, 0.0) - 100.0).abs() < 0.01);
    // At y=0 (circle top): right edge = 50 + sqrt(50^2 - 50^2) = 50.
    // So the circle narrows the margin box from 100 to 50 at the top.
    assert!((fc.left_edge_at(0.0, 0.0) - 50.0).abs() < 0.01);
}
```

### 8. Обновить CSS-SPECS.md

Найти строку с `CSS Shapes L1 | css-shapes-1` и изменить статус `🟡` → `🟡` (остаётся, т.к. только `circle()`, не `polygon()`), добавить примечание `circle() ✅ (P1 2026-06-03)`.

### 9. Проверить и закоммитить

```bash
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout 2>&1 | tail -5

git add crates/engine/layout/src/box_tree.rs CSS-SPECS.md
git commit -m "P1: shape-outside: circle() в FloatContext (CSS Shapes L1 §5.1)

FloatContext.shape_circles хранит активные circle-формы. left_edge_at /
right_edge_at вычисляют горизонтальный радиус окружности на каждом y.
При размещении left/right float с ShapeOutside::Value circle(Npx)
добавляется circle entry в shape_circles. 3 теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-shape-outside-circle -m "Merge p1-shape-outside-circle: shape-outside circle() algorithm"
git branch -d p1-shape-outside-circle
git add CSS-SPECS.md && git commit -m "P1: отметить p1-shape-outside-circle завершённой"
git push origin main
git worktree remove .claude/worktrees/shape-outside-circle
```

---

## Критерии готовности

- [ ] `parse_circle_px("circle(50px)")` → `Some(50.0)`
- [ ] `FloatContext.left_edge_at(y)` возвращает меньшее значение вблизи топа/боттома circle
- [ ] `FloatContext.left_edge_at(y)` возвращает большее значение в центре circle
- [ ] Clippy чист, 3 теста проходят
- [ ] `shape-outside: ellipse()` и `polygon()` оставить как `ShapeOutside::Value` без обработки (задачи Phase 2)
