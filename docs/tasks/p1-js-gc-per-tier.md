# Задача: JS GC tuning per tab tier (10L)

**Developer:** P1  
**Ветка:** `p1-js-gc-per-tier`  
**Размер:** S (~60 строк + 4 теста)  
**Крейты:** `lumen-core`, `lumen-js`, `lumen-shell`

---

## Контекст

Активная вкладка получает мягкий GC (редкий, не блокирует UI). Фоновые и
гибернирующие вкладки должны получать агрессивный GC (несколько проходов) —
сейчас GC не вызывается вовсе при изменении tier'а.

`rquickjs::Runtime` (поле `QuickJsRuntime::inner._rt: Runtime`) имеет метод
`rt.run_gc()`. `Inner` недоступен снаружи; нужен метод на `QuickJsRuntime`.

---

## Пред-запуск

- [ ] Прочесть `crates/js/src/lib.rs:183–200` — `struct Inner`, `QuickJsRuntime::new`
- [ ] Прочесть `crates/core/src/ext.rs` — найти `pub trait JsRuntime` (grep `trait JsRuntime`)
- [ ] Прочесть `crates/shell/src/main.rs` — найти `tick_lifecycle` (grep `fn tick_lifecycle`)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/js-gc -b p1-js-gc-per-tier
cd .claude/worktrees/js-gc
```

### 2. Добавить методы в JsRuntime trait

Файл: `crates/core/src/ext.rs`

Найти `pub trait JsRuntime` (grep `-n "pub trait JsRuntime"`). Добавить **после**
последнего существующего метода (перед закрывающей `}`):

```rust
    /// Run a single GC pass — mild eviction suitable for the active tab (T0).
    ///
    /// Default: no-op (NullJsRuntime, testing stubs).
    fn gc_soft(&self) {}

    /// Run multiple GC passes — aggressive eviction for background/hibernated tabs (T2/T3).
    ///
    /// Default: calls `gc_soft()` once. Override for real runtimes.
    fn gc_aggressive(&self) {
        self.gc_soft();
    }
```

### 3. Реализовать в QuickJsRuntime

Файл: `crates/js/src/lib.rs`

Найти `impl JsRuntime for QuickJsRuntime` (grep `-n "impl JsRuntime for QuickJsRuntime"`).
Добавить **после** последнего метода в impl (перед закрывающей `}`):

```rust
    fn gc_soft(&self) {
        if let Ok(guard) = self.inner.lock() {
            guard._rt.run_gc();
        }
    }

    fn gc_aggressive(&self) {
        // Three passes ≈ free most cyclic garbage in QuickJS.
        for _ in 0..3 {
            self.gc_soft();
        }
    }
```

> Если компилятор жалуется на `_rt` (underscore = intended unused) — переименуй
> поле `_rt` → `rt` в `struct Inner` и в `QuickJsRuntime::new`:
> `inner: Mutex::new(Inner { rt, ctx })`.

### 4. Создать gc_policy.rs

Файл: `crates/js/src/gc_policy.rs` (новый файл):

```rust
//! Per-tier GC policy (ADR-008 §10L).
//!
//! Called from shell `tick_lifecycle` on tier transitions.

use lumen_core::JsRuntime;

/// Called when a tab transitions to T0 (Active) or T1 (BackgroundRecent).
/// Runs a single mild GC pass.
pub fn on_tab_foregrounded(rt: &dyn JsRuntime) {
    rt.gc_soft();
}

/// Called when a tab transitions to T2 (BackgroundOld).
/// Runs aggressive GC to free dead objects before heap snapshotting.
pub fn on_tab_backgrounded_old(rt: &dyn JsRuntime) {
    rt.gc_aggressive();
}

/// Called when a tab transitions to T3 (Hibernated).
/// Runs aggressive GC to minimise heap before suspension.
pub fn on_tab_hibernated(rt: &dyn JsRuntime) {
    rt.gc_aggressive();
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::NullJsRuntime;

    // NullJsRuntime has no-op gc_soft/gc_aggressive — just check no panic.

    #[test]
    fn on_tab_foregrounded_no_panic() {
        on_tab_foregrounded(&NullJsRuntime);
    }

    #[test]
    fn on_tab_backgrounded_old_no_panic() {
        on_tab_backgrounded_old(&NullJsRuntime);
    }

    #[test]
    fn on_tab_hibernated_no_panic() {
        on_tab_hibernated(&NullJsRuntime);
    }
}
```

### 5. Зарегистрировать модуль

Файл: `crates/js/src/lib.rs` (в начале, где `pub mod` объявления):

Добавить:

```rust
pub mod gc_policy;
```

### 6. Добавить вызов в shell tick_lifecycle

Файл: `crates/shell/src/main.rs`

Найти `fn tick_lifecycle` (grep `-n "fn tick_lifecycle"`). Найти блок, где
`tr.to == TabState::Hibernated`. **После** hiberation-логики добавить вызовы:

```rust
use lumen_js::gc_policy;

// В tick_lifecycle, после loop по transitions:
for tr in &transitions {
    match tr.to {
        lumen_shell::tab_lifecycle::TabState::BackgroundOld => {
            if let Some(js) = &self.js_ctx {
                gc_policy::on_tab_backgrounded_old(js.as_ref());
            }
        }
        lumen_shell::tab_lifecycle::TabState::Hibernated => {
            if let Some(js) = &self.js_ctx {
                gc_policy::on_tab_hibernated(js.as_ref());
            }
        }
        _ => {}
    }
}
```

> **Важно:** `self.js_ctx` — найди реальный тип через grep
> `grep -n "js_ctx" crates/shell/src/main.rs | head -5`.
> Если тип `Arc<Mutex<dyn JsRuntime>>` — нужно lock() + gc_soft().
> Если тип `QuickJsRuntime` — прямой вызов `self.js_ctx.gc_aggressive()`.
> Адаптируй под реальный тип.

### 7. Добавить тест для GC counts в QuickJsRuntime

Файл: `crates/js/src/lib.rs` в `#[cfg(test)]`:

```rust
#[test]
fn gc_aggressive_runs_without_panic() {
    let rt = QuickJsRuntime::new().expect("QuickJS runtime");
    rt.gc_soft();
    rt.gc_aggressive(); // 3 passes — should not panic or deadlock
}
```

### 8. Проверить

```bash
cargo clippy -p lumen-core --all-targets -- -D warnings
cargo clippy -p lumen-js --all-targets -- -D warnings
cargo test -p lumen-js 2>&1 | tail -5
```

### 9. Закоммитить и влить

```bash
git add crates/core/src/ext.rs \
        crates/js/src/lib.rs \
        crates/js/src/gc_policy.rs \
        crates/shell/src/main.rs
git commit -m "P1: JS GC per tier — gc_soft/gc_aggressive + gc_policy (10L)

JsRuntime::gc_soft()/gc_aggressive() с default no-op.
QuickJsRuntime: rt.run_gc() × 1 / × 3. gc_policy.rs: on_tab_backgrounded_old
и on_tab_hibernated вызывают gc_aggressive. Shell tick_lifecycle вызывает
политику на T2/T3 переходах. 4 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-js-gc-per-tier -m "Merge p1-js-gc-per-tier: GC tuning per tab tier"
git branch -d p1-js-gc-per-tier
git add STATUS-P1.md && git commit -m "P1: отметить p1-js-gc-per-tier завершённой"
git push origin main
git worktree remove .claude/worktrees/js-gc
```

---

## Критерии готовности

- [ ] `JsRuntime::gc_soft()` / `gc_aggressive()` в trait с default no-op
- [ ] `QuickJsRuntime::gc_soft()` вызывает `_rt.run_gc()` под Mutex
- [ ] `gc_policy.rs` создан с тремя функциями
- [ ] Shell `tick_lifecycle` вызывает политику на T2/T3 переходах
- [ ] 4 unit-теста проходят (3 policy + 1 QuickJS)
- [ ] Clippy чист (`lumen-core`, `lumen-js`)
