# Задача: Loading spinner при restore гибернированной вкладки > 200ms (10K.3)

**Developer:** P1 (переназначено с P2 → резерв, 2026-06-18)  
**Ветка:** `p1-tab-restore-spinner`  
**Размер:** S (~60 строк + 3 теста)  
**Крейты:** `lumen-shell`

---

## Контекст

При восстановлении T3-гибернированной вкладки из SQLite может потребоваться
>200 мс. Сейчас UI висит без обратной связи. Нужен spinner overlay на время
восстановления (10K.3).

Spinner = анимированное кольцо (triangle fan из `DrawSvgPath` или `DrawSvgPath`-arc)
поверх страницы, исчезает когда restore завершён.

---

## Пред-запуск

- [ ] `grep -n "restore_hibernated_tab\|fn restore_hibernated" crates/shell/src/main.rs | head -5`
- [ ] `grep -n "overlay_buf\|render_overlay" crates/shell/src/main.rs | head -5`
- [ ] `grep -n "now_ms\|self.epoch\|elapsed\|Instant::now" crates/shell/src/main.rs | head -5`
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/tab-spinner -b p2-tab-restore-spinner
cd .claude/worktrees/tab-spinner
```

### 2. Добавить restore_started_ms в Lumen

Файл: `crates/shell/src/main.rs`

Найти `struct Lumen {` и добавить:

```rust
    /// Timestamp (wall ms) когда начался restore гибернированной вкладки.
    /// `Some(ms)` = spinner активен; `None` = нет восстановления.
    restore_spinner_start_ms: Option<f64>,
```

Инициализировать в `Default` или `new()`:

```rust
    restore_spinner_start_ms: None,
```

### 3. Установить spinner при начале restore

Найти `fn restore_hibernated_tab` (или место где вызывается restore). Перед
долгой операцией (SQLite fetch + deserialize) добавить:

```rust
self.restore_spinner_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
self.window.request_redraw(); // показать spinner немедленно
```

### 4. Сбросить spinner после restore

После завершения restore (relayout + paint готов) добавить:

```rust
self.restore_spinner_start_ms = None;
```

### 5. Добавить build_restore_spinner

Создать новый файл: `crates/shell/src/panels/restore_spinner.rs`:

```rust
//! Fullscreen spinner overlay во время restore T3-гибернированной вкладки (10K.3).

use lumen_paint::DisplayCommand;

/// Количество миллисекунд, после которых показывается spinner.
const THRESHOLD_MS: f64 = 200.0;

/// Угловая скорость spinner'а в радианах в секунду.
const SPEED: f64 = std::f64::consts::TAU * 0.8; // ~290°/сек, ~один оборот за 1.25 сек

/// Build spinner overlay if restore has taken longer than THRESHOLD_MS.
///
/// `elapsed_ms` — сколько мс прошло с начала restore.
/// `win_w`, `win_h` — размер окна.
/// Returns `None` если < порога.
pub fn build_spinner(elapsed_ms: f64, win_w: f32, win_h: f32) -> Option<Vec<DisplayCommand>> {
    if elapsed_ms < THRESHOLD_MS {
        return None;
    }

    let angle = (elapsed_ms / 1000.0 * SPEED) as f32;
    let cx = win_w / 2.0;
    let cy = win_h / 2.0;
    let r_outer = 28.0_f32;
    let r_inner = 18.0_f32;
    let arc_span = std::f32::consts::PI * 1.5; // 270° дуга

    // Строим SVG path arc: рисуем кольцо-сегмент.
    let n = 32_usize;
    let mut d = String::new();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let a = angle + t * arc_span;
        let (sin_a, cos_a) = a.sin_cos();
        if i == 0 {
            d.push_str(&format!("M {} {} ", cx + cos_a * r_outer, cy + sin_a * r_outer));
        } else {
            d.push_str(&format!("L {} {} ", cx + cos_a * r_outer, cy + sin_a * r_outer));
        }
    }
    for i in (0..=n).rev() {
        let t = i as f32 / n as f32;
        let a = angle + t * arc_span;
        let (sin_a, cos_a) = a.sin_cos();
        d.push_str(&format!("L {} {} ", cx + cos_a * r_inner, cy + sin_a * r_inner));
    }
    d.push('Z');

    let mut cmds = Vec::new();

    // Полупрозрачный фон-затемнение.
    cmds.push(DisplayCommand::FillRect {
        rect: lumen_paint::Rect { x: 0.0, y: 0.0, width: win_w, height: win_h },
        color: [0.0, 0.0, 0.0, 0.45],
    });

    // Spinner arc.
    cmds.push(DisplayCommand::DrawSvgPath {
        d,
        fill_color: Some([0.4, 0.7, 1.0, 1.0]),
        stroke_color: None,
        stroke_width: 0.0,
        fill_rule: lumen_paint::FillRule::NonZero,
        stroke_params: None,
    });

    Some(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_none_before_threshold() {
        assert!(build_spinner(100.0, 1024.0, 768.0).is_none());
    }

    #[test]
    fn spinner_some_after_threshold() {
        let cmds = build_spinner(300.0, 1024.0, 768.0);
        assert!(cmds.is_some());
        assert!(cmds.unwrap().len() >= 2); // backdrop + arc
    }

    #[test]
    fn spinner_some_at_exact_threshold() {
        // 200.0 ms = exactly at threshold — should show.
        assert!(build_spinner(200.0, 1024.0, 768.0).is_some());
    }
}
```

> **Адаптируй типы** по реальному API:
> - `DisplayCommand::FillRect { rect, color }` — найди реальные поля через grep
> - `DisplayCommand::DrawSvgPath { d, fill_color, ... }` — найди через grep
> - `lumen_paint::Rect` / `FillRule` — убедись что импорты корректны

### 6. Зарегистрировать модуль

Файл: `crates/shell/src/panels/mod.rs` (или где объявлены `pub mod` панелей):

```rust
pub mod restore_spinner;
```

### 7. Вызвать spinner при рендере

В `main.rs` в `RedrawRequested`, в секции overlay_buf, добавить:

```rust
// Restore spinner (показываем поверх всего, включая tab bar overlay).
if let Some(start_ms) = self.restore_spinner_start_ms {
    let elapsed_ms = self.epoch.elapsed().as_secs_f64() * 1000.0 - start_ms;
    if let Some(spinner) = restore_spinner::build_spinner(elapsed_ms, win_w as f32, win_h as f32) {
        overlay_buf.extend(spinner);
        // Пока spinner виден — продолжать редроуить для анимации.
        self.window.request_redraw();
    }
}
```

### 8. Проверить

```bash
cargo clippy -p lumen-shell --all-targets -- -D warnings
cargo test -p lumen-shell 2>&1 | tail -5
```

### 9. Закоммитить и влить

```bash
git add crates/shell/src/main.rs \
        crates/shell/src/panels/restore_spinner.rs \
        crates/shell/src/panels/mod.rs
git commit -m "P2: Loading spinner при restore T3-вкладки >200ms (10K.3)

restore_spinner_start_ms: Option<f64> в Lumen. build_spinner возвращает
None до 200ms порога, затем — backdrop + кольцо-arc с вращением.
request_redraw() пока spinner активен. 3 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p2-tab-restore-spinner -m "Merge p2-tab-restore-spinner: spinner при restore гибернации"
git branch -d p2-tab-restore-spinner
git add STATUS-P2.md && git commit -m "P2: отметить p2-tab-restore-spinner завершённой"
git push origin main
git worktree remove .claude/worktrees/tab-spinner
```

---

## Критерии готовности

- [ ] `restore_spinner_start_ms` устанавливается перед долгим restore
- [ ] `build_spinner` возвращает `None` до 200 мс
- [ ] `build_spinner` возвращает backdrop + вращающуюся дугу после 200 мс
- [ ] Spinner исчезает (start_ms = None) после завершения restore
- [ ] `request_redraw()` продолжает анимацию пока spinner активен
- [ ] 3 unit-теста проходят
- [ ] Clippy чист
