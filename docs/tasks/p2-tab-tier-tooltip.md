# Задача: Tab tier tooltip при hover (10K.2)

**Developer:** P2  
**Ветка:** `p2-tab-tier-tooltip`  
**Размер:** S (~70 строк + 3 теста)  
**Крейты:** `lumen-shell`

---

## Контекст

`TabState` бейдж (amber/grey dot) показывает tier — но пользователь не знает
что это значит. При hover на вкладку с `BackgroundOld` или `Hibernated` бейджем
должен появляться tooltip: «Вкладка спит — клик восстановит» (10K.2).

Tooltip рисуется как `DisplayList`-оверлей аналогично другим панелям.

---

## Пред-запуск

- [ ] `grep -n "fn build_tab_bar" crates/shell/src/tabs/strip.rs` — найти функцию
- [ ] `grep -n "hovered_tab\|tab_hover\|TOOLTIP" crates/shell/src/main.rs | head -10`
- [ ] `grep -n "TabState::" crates/shell/src/tabs/strip.rs | head -10`
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/tab-tooltip -b p2-tab-tier-tooltip
cd .claude/worktrees/tab-tooltip
```

### 2. Добавить tooltip-состояние в Lumen

Файл: `crates/shell/src/main.rs`

Найти `struct Lumen {` и добавить поле:

```rust
    /// Tab bar: index хвовёранной вкладки для отображения tier-tooltip.
    hovered_tab_idx: Option<usize>,
```

В `impl Default for Lumen` (или в `fn new()`) инициализировать:

```rust
    hovered_tab_idx: None,
```

### 3. Обновить hovered_tab_idx в CursorMoved

Найти в `main.rs` обработчик `WindowEvent::CursorMoved` (grep `-n "CursorMoved"`).
После вычисления позиции курсора (`let pos = ...` или `let x = ...`, `let y = ...`)
добавить:

```rust
// Обновить hover-индекс вкладки для tooltip.
let tab_h = 36.0_f32; // высота tab bar (константа)
self.hovered_tab_idx = if pos.y < tab_h as f64 {
    // Найти вкладку под курсором через hit_test (перепроверь что tab_strip.hit_test существует)
    match self.tab_strip.hit_test(pos.x as f32, pos.y as f32, win_w as f32) {
        crate::tabs::strip::TabHit::Tab(idx) => Some(idx),
        _ => None,
    }
} else {
    None
};
```

> Если `tab_strip.hit_test` имеет другую сигнатуру — найди реальную через:
> `grep -n "fn hit_test" crates/shell/src/tabs/strip.rs`

### 4. Добавить build_tab_tooltip в strip.rs (или main.rs)

Файл: `crates/shell/src/tabs/strip.rs` — добавить функцию после `build_tab_bar`:

```rust
/// Build a small tooltip overlay for a tab with a non-Active tier badge.
///
/// Returns `None` if the hovered tab has no tier badge (Active / BackgroundRecent).
pub fn build_tab_tooltip(
    tab: &TabEntry,
    tab_center_x: f32,
    tab_bar_bottom: f32,
) -> Option<Vec<DisplayCommand>> {
    let msg = match tab.tab_state {
        TabState::BackgroundOld => "Вкладка фоновая — потребляет меньше памяти",
        TabState::Hibernated => "Вкладка спит — клик восстановит (~1 сек)",
        _ => return None,
    };

    const TT_W: f32 = 240.0;
    const TT_H: f32 = 28.0;
    const PAD: f32 = 8.0;
    const RADIUS: f32 = 4.0;

    let x = (tab_center_x - TT_W / 2.0).max(4.0);
    let y = tab_bar_bottom + 4.0;

    let bg = [0.15, 0.15, 0.15, 0.92];
    let text_color = [1.0, 1.0, 1.0, 1.0];
    let font_size = 11.0;

    let mut cmds = Vec::new();
    // Background rounded rect.
    cmds.push(DisplayCommand::FillRoundedRect {
        rect: lumen_paint::Rect { x, y, width: TT_W, height: TT_H },
        radii: lumen_paint::CornerRadii::uniform(RADIUS),
        color: bg,
    });
    // Text.
    cmds.push(DisplayCommand::DrawText {
        text: msg.to_string(),
        x: x + PAD,
        y: y + TT_H / 2.0 + font_size * 0.35,
        font_size,
        color: text_color,
    });

    Some(cmds)
}
```

> **Адаптируй типы** по реальному API:
> - `DisplayCommand::FillRoundedRect` может называться иначе — grep `FillRoundedRect`
> - `lumen_paint::Rect` — найди тип через `grep -rn "pub struct Rect" crates/engine/paint/`
> - `CornerRadii::uniform` — найди через `grep -n "fn uniform\|uniform(" crates/engine/paint/src/display_list.rs`

### 5. Вызвать tooltip при рендере

В `main.rs`, в `RedrawRequested` секции, где строится `overlay_buf` (после
tab bar), добавить:

```rust
// Tab tier tooltip.
if let Some(idx) = self.hovered_tab_idx {
    if let Some(tab) = self.tab_strip.get(idx) {
        let tab_center_x = /* вычислить центр вкладки по idx */
            (idx as f32 + 0.5) * (win_w as f32 / self.tab_strip.len().max(1) as f32);
        if let Some(tooltip_cmds) = build_tab_tooltip(tab, tab_center_x, 36.0) {
            overlay_buf.extend(tooltip_cmds);
        }
    }
}
```

> Найди реальный способ получить вкладку: `grep -n "fn get\|fn tab_at\|fn entry"
> crates/shell/src/tabs/strip.rs | head -5`

### 6. Добавить тесты

В `crates/shell/src/tabs/strip.rs` в `#[cfg(test)]`:

```rust
#[test]
fn tooltip_none_for_active_tab() {
    let mut tab = TabEntry::default();
    tab.tab_state = TabState::Active;
    assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_none());
}

#[test]
fn tooltip_some_for_hibernated_tab() {
    let mut tab = TabEntry::default();
    tab.tab_state = TabState::Hibernated;
    let cmds = build_tab_tooltip(&tab, 100.0, 36.0);
    assert!(cmds.is_some());
    // Tooltip must have at least background + text.
    assert!(cmds.unwrap().len() >= 2);
}

#[test]
fn tooltip_some_for_background_old() {
    let mut tab = TabEntry::default();
    tab.tab_state = TabState::BackgroundOld;
    assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_some());
}
```

### 7. Проверить

```bash
cargo clippy -p lumen-shell --all-targets -- -D warnings
cargo test -p lumen-shell 2>&1 | tail -5
```

### 8. Закоммитить и влить

```bash
git add crates/shell/src/main.rs crates/shell/src/tabs/strip.rs
git commit -m "P2: Tab tier tooltip при hover на спящую вкладку (10K.2)

hovered_tab_idx: Option<usize> в Lumen — обновляется в CursorMoved.
build_tab_tooltip: FillRoundedRect + DrawText с текстом по TabState.
Active/BackgroundRecent → None (tooltip не показывается). 3 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p2-tab-tier-tooltip -m "Merge p2-tab-tier-tooltip: hover tooltip для tier-бейджа"
git branch -d p2-tab-tier-tooltip
git add STATUS-P2.md && git commit -m "P2: отметить p2-tab-tier-tooltip завершённой"
git push origin main
git worktree remove .claude/worktrees/tab-tooltip
```

---

## Критерии готовности

- [ ] `hovered_tab_idx` обновляется при движении мыши
- [ ] `build_tab_tooltip` возвращает `None` для Active и BackgroundRecent
- [ ] Для Hibernated показывает «Вкладка спит — клик восстановит»
- [ ] Для BackgroundOld показывает «Вкладка фоновая»
- [ ] Tooltip отображается поверх контента (в overlay_buf)
- [ ] 3 unit-теста проходят
- [ ] Clippy чист
