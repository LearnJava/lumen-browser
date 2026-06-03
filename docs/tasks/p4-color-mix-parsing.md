# Задача: CSS color-mix() parsing

**Developer:** P4  
**Ветка:** `p4-color-mix-parsing`  
**Размер:** S (~60 строк кода + 3 теста)  
**Крейты:** `lumen-layout`

---

## Контекст

Алгоритм `color-mix()` уже реализован P1 в `crates/engine/layout/src/color_mix.rs`
(25 unit-тестов, все пространства цветов). Функция `parse_function_color` в `style.rs`
содержит комментарий-заглушку `// CSS: color-mix()` — нужно добавить парсинг.

После этой задачи CSS вида `color: color-mix(in oklab, red 40%, blue)` будет работать.

---

## Пред-запуск

- [ ] Прочесть `crates/engine/layout/src/color_mix.rs` строки 38–77
  (enum `MixColorSpace` + `from_css`)
- [ ] Прочесть `crates/engine/layout/src/style.rs` строки 15125–15148
  (функция `parse_function_color`)
- [ ] Прочесть `crates/engine/layout/src/style.rs` строки 6676–6693
  (функция `split_top_level_commas`)
- [ ] `git status` — убедиться что main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/color-mix-parsing -b p4-color-mix-parsing
cd .claude/worktrees/color-mix-parsing
```

### 2. Добавить вспомогательные функции в style.rs

Файл: `crates/engine/layout/src/style.rs`

Найти функцию `fn split_color_args(body: &str) -> Vec<String> {` (строка ~15383).
**Перед ней** (не внутри) вставить два новых приватных хелпера:

```rust
/// CSS Color L5 §10.2 — parse `color-mix(in <space>, <c1> [pct]?, <c2> [pct]?)`
/// from the inner body (without outer `color-mix(` and `)`).
/// Returns `None` on any parse error; invalid inputs are silently ignored per spec.
fn parse_color_mix(body: &str) -> Option<Color> {
    let parts = split_top_level_commas(body);
    if parts.len() != 3 {
        return None;
    }
    // Part 0: "in <space>" — case-insensitive per CSS Values §3.
    let part0 = parts[0].trim().to_ascii_lowercase();
    let space_str = part0.strip_prefix("in ")?.trim();
    let space = crate::color_mix::MixColorSpace::from_css(space_str)?;

    let (c1, w1_raw) = parse_color_with_pct(parts[1].trim())?;
    let (c2, w2_raw) = parse_color_with_pct(parts[2].trim())?;

    // Normalize weights: CSS Color L5 §10.2 §3 weight normalization.
    let (w1, w2) = match (w1_raw, w2_raw) {
        (None, None)             => (0.5, 0.5),
        (Some(w), None)          => (w, 1.0 - w),
        (None, Some(w))          => (1.0 - w, w),
        (Some(w1), Some(w2))     => (w1, w2),
    };

    let to_f = |c: Color| -> [f32; 4] {
        [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0]
    };
    let out = crate::color_mix::mix_colors(space, to_f(c1), w1, to_f(c2), w2);
    Some(Color {
        r: (out[0] * 255.0).round().clamp(0.0, 255.0) as u8,
        g: (out[1] * 255.0).round().clamp(0.0, 255.0) as u8,
        b: (out[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        a: (out[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    })
}

/// Parse `"<color> [N%]?"` → `(Color, Option<fraction>)`.
/// Fraction = percentage / 100. Returns `None` only when color itself is invalid.
fn parse_color_with_pct(s: &str) -> Option<(Color, Option<f32>)> {
    // Check if the last whitespace-separated token looks like "N%".
    if let Some(sp_pos) = s.rfind(char::is_whitespace) {
        let last = s[sp_pos + 1..].trim();
        if let Some(digits) = last.strip_suffix('%') {
            if let Ok(v) = digits.parse::<f32>() {
                let color_str = s[..sp_pos].trim();
                return Some((parse_color(color_str)?, Some(v / 100.0)));
            }
        }
    }
    // No percentage suffix; entire string is the color.
    Some((parse_color(s)?, None))
}
```

### 3. Вызвать parse_color_mix из parse_function_color

Файл: `crates/engine/layout/src/style.rs`

Найти функцию `fn parse_function_color(s: &str) -> Option<Color> {` (строка ~15125).
Внутри неё найти строки:

```rust
    // CSS: color-mix() — P4 task: detect "color-mix(in <space>, ...)" here,
    // call `lumen_layout::color_mix::mix_colors(space, c1, w1, c2, w2)`, return Color.
    // Algorithm stub: crates/engine/layout/src/color_mix.rs.
    let lower = s.to_ascii_lowercase();
```

Заменить эти 4 строки на:

```rust
    let lower = s.to_ascii_lowercase();
    // CSS Color L5 §10.2 color-mix().
    if lower.starts_with("color-mix(") && s.ends_with(')') {
        return parse_color_mix(&s["color-mix(".len()..s.len() - 1]);
    }
```

> **Важно:** строку `let lower = s.to_ascii_lowercase();` оставляем — она нужна
> ниже для `rgba(`, `hsl(` и др. Просто вставляем `if color-mix` **после** неё.

### 4. Добавить тесты

В том же файле, в блоке `#[cfg(test)] mod tests {`, добавить:

```rust
#[test]
fn color_mix_srgb_equal_weights() {
    // color-mix(in srgb, red, blue) → 50% blend → rgb(128, 0, 128)
    let c = parse_color("color-mix(in srgb, red, blue)").expect("should parse");
    assert_eq!(c.r, 128, "r");
    assert_eq!(c.b, 128, "b");
    assert_eq!(c.g, 0, "g");
}

#[test]
fn color_mix_with_percentages() {
    // color-mix(in srgb, red 100%, blue 0%) → pure red
    let c = parse_color("color-mix(in srgb, red 100%, blue 0%)").expect("should parse");
    assert_eq!(c.r, 255);
    assert_eq!(c.b, 0);
}

#[test]
fn color_mix_invalid_returns_none() {
    // Missing "in" keyword → None
    assert!(parse_color("color-mix(srgb, red, blue)").is_none());
    // Only 2 comma-separated parts → None
    assert!(parse_color("color-mix(in srgb, red)").is_none());
}
```

### 5. Проверить

```bash
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout 2>&1 | tail -5
```

Если `color_mix_srgb_equal_weights` провалился с r=127 вместо 128 — это нормально
(round vs floor для 127.5). Поменяй assert на `assert!(c.r >= 127 && c.r <= 128)`.

### 6. Обновить CSS-SPECS.md

Найти строку с `color-mix()` в `CSS-SPECS.md` и поменяй статус `🟡` → `✅`.

### 7. Обновить STATUS-P4.md

В секции `## Needs wiring` найти `### CSS color-mix() function` и добавить в конец:

```
- **WIRED** (p4-color-mix-parsing, 2026-06-03): парсинг `color-mix()` добавлен в `parse_function_color`.
```

### 8. Закоммитить и влить

```bash
git add crates/engine/layout/src/style.rs CSS-SPECS.md STATUS-P4.md
git commit -m "P4: парсинг color-mix() в parse_function_color (CSS Color L5 §10.2)

Добавлены parse_color_mix() + parse_color_with_pct() в style.rs.
Вызываются из parse_function_color при обнаружении prefix color-mix(.
Алгоритм mix_colors() уже готов в color_mix.rs (P1, p1-color-mix).
3 новых теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p4-color-mix-parsing -m "Merge p4-color-mix-parsing: CSS color-mix() end-to-end"
git branch -d p4-color-mix-parsing
git add STATUS-P4.md && git commit -m "P4: отметить p4-color-mix-parsing завершённой"
git push origin main
git worktree remove .claude/worktrees/color-mix-parsing
```

---

## Критерии готовности

- [ ] `parse_color("color-mix(in srgb, red, blue)")` возвращает `Some(Color { r≈128, g=0, b≈128, .. })`
- [ ] `parse_color("color-mix(in srgb, red 100%, blue 0%)")` возвращает красный
- [ ] Невалидный вход возвращает `None`
- [ ] Clippy чист, все 3 теста проходят
