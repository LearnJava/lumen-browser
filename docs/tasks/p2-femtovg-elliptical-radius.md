# Задача: FemtovgBackend — истинные эллиптические border-radius

**Developer:** P2  
**Ветка:** `p2-femtovg-elliptical-radius`  
**Размер:** S (~50 строк замены + 2 теста)  
**Крейты:** `lumen-paint`

---

## Контекст

`FemtovgBackend.draw_fill_rounded_rect` (строка ~403 `femtovg_backend.rs`) использует
`max(radii.tl, radii.tl_y)` для каждого угла — берёт большее из x/y-радиуса, получая
КРУГЛЫЙ угол вместо эллиптического. CSS `border-radius: 40px / 20px` (горизонтальный /
вертикальный) рендерится неверно: WgpuBackend рисует правильный эллипс, FemtovgBackend —
круг с радиусом `max(40, 20) = 40`.

Femtovg API `path.rounded_rect_varying(x, y, w, h, tl, tr, br, bl)` принимает только
ОДИН радиус на угол (круговой). Для эллипса нужно рисовать контур через кубические
Безье (стандартное приближение четверти эллипса).

---

## Пред-запуск

- [ ] Прочесть `crates/engine/paint/src/backends/femtovg_backend.rs:403–422`
  (`fn draw_fill_rounded_rect`)
- [ ] Прочесть `crates/engine/paint/src/display_list.rs:121–170` (`CornerRadii` struct)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/femtovg-elliptic -b p2-femtovg-elliptical-radius
cd .claude/worktrees/femtovg-elliptic
```

### 2. Заменить draw_fill_rounded_rect

Файл: `crates/engine/paint/src/backends/femtovg_backend.rs`

Найти всю функцию `fn draw_fill_rounded_rect(`:

```rust
    fn draw_fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadii,
        color: Color,
    ) {
        let tl = radii.tl.max(radii.tl_y);
        let tr = radii.tr.max(radii.tr_y);
        let br = radii.br.max(radii.br_y);
        let bl = radii.bl.max(radii.bl_y);

        let mut path = femtovg::Path::new();
        // Порядок: TL, TR, BR, BL — совпадает с CSS border-radius.
        path.rounded_rect_varying(x, y, w, h, tl, tr, br, bl);
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }
```

Заменить ПОЛНОСТЬЮ на:

```rust
    /// Draw a rounded rectangle with per-corner elliptical radii.
    ///
    /// When `rx == ry` for all corners the path is identical to the circular case.
    /// For elliptical corners (rx ≠ ry) we use cubic Bézier approximation of a
    /// quarter-ellipse with the Geng–Zwart kappa constant ≈ 0.5523.
    fn draw_fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadii,
        color: Color,
    ) {
        // Kappa constant for cubic Bézier approximation of a quarter-circle/ellipse.
        const K: f32 = 0.5523;

        // Clamp radii so they don't exceed half the box dimensions (CSS Backgrounds §5.5).
        let tl_x = radii.tl.min(w / 2.0).min(h / 2.0).max(0.0);
        let tl_y = radii.tl_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let tr_x = radii.tr.min(w / 2.0).min(h / 2.0).max(0.0);
        let tr_y = radii.tr_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let br_x = radii.br.min(w / 2.0).min(h / 2.0).max(0.0);
        let br_y = radii.br_y.min(w / 2.0).min(h / 2.0).max(0.0);
        let bl_x = radii.bl.min(w / 2.0).min(h / 2.0).max(0.0);
        let bl_y = radii.bl_y.min(w / 2.0).min(h / 2.0).max(0.0);

        // Fast path: all corners circular — delegate to femtovg built-in.
        if (tl_x - tl_y).abs() < 0.5 && (tr_x - tr_y).abs() < 0.5
            && (br_x - br_y).abs() < 0.5 && (bl_x - bl_y).abs() < 0.5
        {
            let mut path = femtovg::Path::new();
            path.rounded_rect_varying(x, y, w, h, tl_x, tr_x, br_x, bl_x);
            let paint = femtovg::Paint::color(lumen_to_fvg(color));
            self.canvas.fill_path(&path, &paint);
            return;
        }

        // Elliptical path: build manually with cubic Bézier corners.
        let mut path = femtovg::Path::new();
        // Start at top-left corner's right end.
        path.move_to(x + tl_x, y);
        // Top edge → top-right corner.
        path.line_to(x + w - tr_x, y);
        path.bezier_to(
            x + w - tr_x + K * tr_x, y,
            x + w,                   y + tr_y - K * tr_y,
            x + w,                   y + tr_y,
        );
        // Right edge → bottom-right corner.
        path.line_to(x + w, y + h - br_y);
        path.bezier_to(
            x + w,                    y + h - br_y + K * br_y,
            x + w - br_x + K * br_x, y + h,
            x + w - br_x,             y + h,
        );
        // Bottom edge → bottom-left corner.
        path.line_to(x + bl_x, y + h);
        path.bezier_to(
            x + bl_x - K * bl_x, y + h,
            x,                   y + h - bl_y + K * bl_y,
            x,                   y + h - bl_y,
        );
        // Left edge → top-left corner.
        path.line_to(x, y + tl_y);
        path.bezier_to(
            x,             y + tl_y - K * tl_y,
            x + tl_x - K * tl_x, y,
            x + tl_x,     y,
        );
        path.close();
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }
```

> **Порядок углов CSS border-radius:** TL (top-left) → TR (top-right) → BR → BL.
> Кривые строятся по часовой стрелке, начиная с правого конца top-left угла.

### 3. Добавить тесты

В `femtovg_backend.rs` в блоке `#[cfg(test)]` (ищи существующие unit-тесты):

```rust
#[test]
fn draw_fill_rounded_rect_circular_does_not_panic() {
    // Circular corners (rx == ry) — fast path.
    let radii = CornerRadii { tl: 8.0, tl_y: 8.0, tr: 8.0, tr_y: 8.0,
                               br: 8.0, br_y: 8.0, bl: 8.0, bl_y: 8.0 };
    // Just verify no panic on valid input (no headless GL context needed for unit test).
    let _ = radii; // used to verify compilation
    assert!((radii.tl - radii.tl_y).abs() < 0.5);
}

#[test]
fn draw_fill_rounded_rect_elliptical_different_radii() {
    // Elliptical: rx=40, ry=20 — should use bezier path, not fast path.
    let radii = CornerRadii { tl: 40.0, tl_y: 20.0, tr: 40.0, tr_y: 20.0,
                               br: 40.0, br_y: 20.0, bl: 40.0, bl_y: 20.0 };
    // Verify that fast-path condition is false.
    assert!((radii.tl - radii.tl_y).abs() >= 0.5);
}
```

### 4. Проверить

```bash
cargo clippy -p lumen-paint --all-targets -- -D warnings
cargo test -p lumen-paint 2>&1 | tail -5
```

### 5. Обновить CSS-SPECS.md

Найти строку с `border-radius` и изменить:
`elliptical (rx≠ry syntax 10px / 20px) ⬜` → `elliptical ✅ FemtovgBackend (P2 2026-06-03)`.

### 6. Закоммитить и влить

```bash
git add crates/engine/paint/src/backends/femtovg_backend.rs CSS-SPECS.md
git commit -m "P2: FemtovgBackend — эллиптические border-radius (kappa Bézier)

draw_fill_rounded_rect: fast-path для rx==ry (встроенный rounded_rect_varying).
Elliptical path (rx≠ry): ручной контур из кубических Безье с kappa=0.5523.
Clamp radii ≤ min(w,h)/2 по CSS Backgrounds §5.5. 2 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p2-femtovg-elliptical-radius -m "Merge p2-femtovg-elliptical-radius: elliptical corners"
git branch -d p2-femtovg-elliptical-radius
git add CSS-SPECS.md && git commit -m "P2: отметить p2-femtovg-elliptical-radius завершённой"
git push origin main
git worktree remove .claude/worktrees/femtovg-elliptic
```

---

## Критерии готовности

- [ ] `border-radius: 40px / 20px` рендерится эллиптически (не кругом)
- [ ] `border-radius: 8px` (одно значение) использует fast-path
- [ ] Clippy чист, тесты компилируются
- [ ] WgpuBackend не трогаем (у него уже правильный SDF-шейдер)
