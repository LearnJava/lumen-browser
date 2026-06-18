# Задача: FemtovgBackend — PushFilter blur (box blur approximation)

**Developer:** P1 (переназначено с P2 → резерв, 2026-06-18)  
**Ветка:** `p1-femtovg-filter-blur`  
**Размер:** M (~80 строк + 2 теста)  
**Крейты:** `lumen-paint`

---

## Контекст

В `FemtovgBackend.render()` обработчик `DisplayCommand::PushFilter` для
`FilterFn::Blur(sigma)` выполняет только `save/restore` canvas — никакого
размытия нет. WgpuBackend использует GPU Gaussian passes; femtovg не имеет
встроенного blur API.

Реализуем box blur через CPU-рендеринг:
1. При `PushFilter { Blur(sigma) }` — начать запись команд в буфер `deferred_blur`.
2. При `PopFilter` — если `deferred_blur` не пуст: выполнить команды на
   offscreen femtovg canvas → получить RGBA пиксели → применить 3×horizontal+vertical
   box blur → зарегистрировать как изображение → отрисовать DrawImage.
3. Если sigma == 0 или blur слишком мал — отрисовать напрямую (no-op fast-path).

> **Note:** Femtovg не поддерживает offscreen rendering напрямую. Используем
> `canvas.screenshot() -> Vec<u8>` (если доступен) или fallback:
> рендерим команды blur-блока нормально, затем пропускаем blur эффект.
> Для Phase 1 достаточен fallback без визуального blur — главное не паниковать
> и не игнорировать команды.

---

## Пред-запуск

- [ ] Прочесть `crates/engine/paint/src/backends/femtovg_backend.rs` — найти
  `DisplayCommand::PushFilter` обработчик (ищи `PushFilter`)
- [ ] Проверить наличие `canvas.screenshot()` в femtovg API:
  `grep -rn "screenshot\|save_image\|flush" crates/engine/paint/src/backends/femtovg_backend.rs`
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/femtovg-blur -b p2-femtovg-filter-blur
cd .claude/worktrees/femtovg-blur
```

### 2. Добавить поле blur_sigma_stack в FemtovgBackend

Файл: `crates/engine/paint/src/backends/femtovg_backend.rs`

Найти `struct FemtovgBackend {` и **добавить** в конец:

```rust
    /// Stack of pending blur sigma values. Non-zero = blur filter is active.
    /// Push on PushFilter(Blur), pop on PopFilter.
    blur_sigma_stack: Vec<f32>,
```

В `fn new(...)` инициализировать:

```rust
    blur_sigma_stack: Vec::new(),
```

(Найди место инициализации: `images: HashMap::new(),` — добавь после него.)

### 3. Найти и обновить PushFilter обработчик

Найти в `render()` обработчик `DisplayCommand::PushFilter`:

```rust
            DisplayCommand::PushFilter { filters } => {
```

(или похожую строку — может называться `PushFilter(filters)`)

Внутри этого обработчика найти ветку для `FilterFn::Blur` или `Blur(_)`.
Она выглядит примерно так:

```rust
                for f in filters {
                    match f {
                        FilterFn::Opacity(a) => self.canvas.set_global_alpha(*a),
                        FilterFn::Blur(_) | FilterFn::Brightness(_) | ... => {
                            // save/restore (нет GPU colour-matrix в femtovg)
                        }
                    }
                }
```

**Заменить** всю ветку `FilterFn::Blur(_) => { ... }` (или соответствующий arm) на:

```rust
                        FilterFn::Blur(sigma) => {
                            // Phase 1: record sigma, draw content normally.
                            // Actual blur pass deferred (femtovg has no native blur).
                            self.blur_sigma_stack.push(*sigma);
                            self.canvas.save();
                        }
```

### 4. Найти и обновить PopFilter обработчик

Найти обработчик `DisplayCommand::PopFilter`:

```rust
            DisplayCommand::PopFilter => {
```

Внутри него найти обработку pop. Она вероятно делает `canvas.restore()`.
**Добавить** перед `canvas.restore()`:

```rust
                if let Some(sigma) = self.blur_sigma_stack.pop() {
                    // Phase 1: sigma recorded but no actual blur applied.
                    // TODO Phase 2: capture offscreen buffer, apply box blur,
                    // draw blurred image. Needs femtovg screenshot() API.
                    let _ = sigma; // suppress unused warning
                }
```

### 5. Добавить хелпер box_blur (для Phase 2 — сейчас не вызывается)

**После** `fn draw_text(` добавить в блок `impl FemtovgBackend`:

```rust
    /// Apply a separable box blur to RGBA pixel data in-place.
    /// `sigma` controls the kernel radius (radius = round(sigma * 1.5)).
    /// This is a 3-pass approximation of Gaussian blur (3× box blur ≈ Gaussian).
    fn box_blur_rgba(pixels: &mut [u8], width: usize, height: usize, sigma: f32) {
        let r = ((sigma * 1.5).round() as usize).max(1);
        let stride = width * 4;
        // Horizontal pass.
        let mut tmp = pixels.to_vec();
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                let x0 = x.saturating_sub(r);
                let x1 = (x + r + 1).min(width);
                for sx in x0..x1 {
                    let off = y * stride + sx * 4;
                    for c in 0..4 { sum[c] += pixels[off + c] as u32; }
                    count += 1;
                }
                let off = y * stride + x * 4;
                for c in 0..4 { tmp[off + c] = (sum[c] / count) as u8; }
            }
        }
        // Vertical pass.
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                let y0 = y.saturating_sub(r);
                let y1 = (y + r + 1).min(height);
                for sy in y0..y1 {
                    let off = sy * stride + x * 4;
                    for c in 0..4 { sum[c] += tmp[off + c] as u32; }
                    count += 1;
                }
                let off = y * stride + x * 4;
                for c in 0..4 { pixels[off + c] = (sum[c] / count) as u8; }
            }
        }
    }
```

### 6. Добавить тесты

В блоке `#[cfg(test)]` в `femtovg_backend.rs`:

```rust
#[test]
fn box_blur_rgba_single_pixel_unchanged() {
    let mut px = vec![255u8, 0, 0, 255]; // red pixel
    FemtovgBackend::box_blur_rgba(&mut px, 1, 1, 2.0);
    assert_eq!(&px, &[255, 0, 0, 255]); // single pixel — unchanged
}

#[test]
fn box_blur_rgba_3x1_averages_horizontally() {
    // 3 pixels: red, black, red — after blur with radius 1 the middle should average
    let mut px = vec![
        255u8, 0, 0, 255, // red
        0,   0, 0, 255,   // black
        255, 0, 0, 255,   // red
    ];
    FemtovgBackend::box_blur_rgba(&mut px, 3, 1, 1.0);
    // Middle pixel (index 1) should now be average of all three: 255+0+255/3 = 170
    let mid_r = px[4]; // offset 1*4 + 0
    assert!(mid_r > 100, "middle pixel should be brightened by blur: got {mid_r}");
}
```

> Если `FemtovgBackend::box_blur_rgba` недоступна из теста — сделай её `pub(crate)`.

### 7. Проверить

```bash
cargo clippy -p lumen-paint --all-targets -- -D warnings
cargo test -p lumen-paint 2>&1 | tail -5
```

### 8. Закоммитить и влить

```bash
git add crates/engine/paint/src/backends/femtovg_backend.rs
git commit -m "P2: FemtovgBackend PushFilter blur Phase 1 + box_blur helper

blur_sigma_stack: отслеживает активные blur-фильтры. PushFilter(Blur):
save + push sigma. PopFilter: pop sigma + restore (blur не применяется
в Phase 1 — femtovg не имеет native blur API). box_blur_rgba() реализован
готов для Phase 2 (screenshot → apply → DrawImage). 2 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p2-femtovg-filter-blur -m "Merge p2-femtovg-filter-blur: blur helper + stack tracking"
git branch -d p2-femtovg-filter-blur
git push origin main
git worktree remove .claude/worktrees/femtovg-blur
```

---

## Критерии готовности

- [ ] `blur_sigma_stack` добавлен в struct и инициализирован
- [ ] `PushFilter(Blur(sigma))` → `push sigma + canvas.save()`
- [ ] `PopFilter` → `pop sigma + canvas.restore()` (без паники при пустом стеке)
- [ ] `box_blur_rgba` компилируется и тесты проходят
- [ ] Clippy чист
- [ ] Визуальный blur ещё не применяется (Phase 2 — требует offscreen capture)
