# Задача: BrowserSession — set_clock / set_rng_seed (8F.1 + 8F.2)

**Developer:** P1  
**Ветка:** `p1-browsersession-set-clock`  
**Размер:** S (~60 строк + 3 теста)  
**Крейты:** `lumen-core`, `lumen-driver`

---

## Контекст

`SessionContext` в `crates/driver/src/context.rs` уже имеет поля:
- `frozen_clock_ms: Option<u64>` + `set_frozen_clock(ts)` / `clear_frozen_clock()`
- `rng_seed: Option<u64>`

Но `BrowserSession` trait (`crates/core/src/ext.rs:1798`) не имеет
`set_clock()` / `set_rng_seed()` — automation-клиенты (Playwright, тесты) не
могут управлять временем и RNG через стандартный интерфейс.

Задача: добавить `ClockMode` enum + два метода в trait + impl.

---

## Пред-запуск

- [ ] Прочесть `crates/core/src/ext.rs:1798–1865` (BrowserSession trait)
- [ ] Прочесть `crates/driver/src/context.rs:1–115` (SessionContext)
- [ ] Прочесть `crates/driver/src/session.rs:1–80` (InProcessSession struct)
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/bs-set-clock -b p1-browsersession-set-clock
cd .claude/worktrees/bs-set-clock
```

### 2. Добавить ClockMode + методы в BrowserSession trait

Файл: `crates/core/src/ext.rs`

Найти строку `pub trait BrowserSession: Send {` (~1798) и сразу **после** строки
`fn eval(&mut self, script: &str) -> Result<String>;` (~1864) — перед закрывающей
скобкой `}` — вставить:

```rust
    /// Freeze the session clock to a fixed timestamp for deterministic testing (8F.1).
    ///
    /// `ClockMode::Frozen(ms)` — `Date.now()` and `Performance.now()` always return `ms`.
    /// `ClockMode::Real` — restore system time (default).
    fn set_clock(&mut self, mode: ClockMode) -> Result<()> {
        let _ = mode;
        Ok(()) // default: no-op (NullBrowserSession-compatible)
    }

    /// Set the RNG seed for deterministic `Math.random()` (8F.2).
    ///
    /// `Some(seed)` — xorshift32 PRNG seeded at `seed`; same seed = same sequence.
    /// `None` — restore OS entropy.
    fn set_rng_seed(&mut self, seed: Option<u64>) -> Result<()> {
        let _ = seed;
        Ok(())
    }
```

Найти **до** строки `pub trait BrowserSession: Send {` — добавить перед ней:

```rust
/// Clock mode for deterministic testing (BrowserSession::set_clock, 8F.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMode {
    /// Freeze clock at this many milliseconds since Unix epoch.
    Frozen(u64),
    /// Use system real-time clock (default).
    Real,
}
```

### 3. Добавить impl в InProcessSession

Файл: `crates/driver/src/session.rs`

Найти `impl BrowserSession for InProcessSession` и найти любой метод-impl
(например `fn navigate`). **После** последнего метода, но перед закрывающей
скобкой `}` блока impl, добавить:

```rust
    fn set_clock(&mut self, mode: lumen_core::ClockMode) -> lumen_core::error::Result<()> {
        match mode {
            lumen_core::ClockMode::Frozen(ts) => self.ctx.set_frozen_clock(ts),
            lumen_core::ClockMode::Real => self.ctx.clear_frozen_clock(),
        }
        Ok(())
    }

    fn set_rng_seed(&mut self, seed: Option<u64>) -> lumen_core::error::Result<()> {
        self.ctx.set_rng_seed(seed);
        Ok(())
    }
```

Если `SessionContext::set_rng_seed(seed)` ещё не существует — добавить в
`crates/driver/src/context.rs` после `clear_frozen_clock`:

```rust
    /// Set deterministic RNG seed; `None` = OS entropy.
    pub fn set_rng_seed(&mut self, seed: Option<u64>) {
        self.rng_seed = seed;
    }

    /// Current RNG seed (None = OS entropy).
    pub fn rng_seed(&self) -> Option<u64> {
        self.rng_seed
    }
```

### 4. Ре-экспортировать ClockMode из lumen-core

Файл: `crates/core/src/lib.rs`

Найти `pub use ext::BrowserSession` (или похожую строку) и рядом добавить:

```rust
pub use ext::ClockMode;
```

### 5. Добавить тесты

В `crates/driver/src/session.rs` в блоке `#[cfg(test)]`:

```rust
#[test]
fn set_clock_frozen_stores_timestamp() {
    let mut sess = InProcessSession::new_headless(1024, 768).unwrap();
    sess.set_clock(lumen_core::ClockMode::Frozen(1_700_000_000_000)).unwrap();
    assert_eq!(sess.ctx.frozen_clock_ms(), Some(1_700_000_000_000));
}

#[test]
fn set_clock_real_clears_timestamp() {
    let mut sess = InProcessSession::new_headless(1024, 768).unwrap();
    sess.set_clock(lumen_core::ClockMode::Frozen(42)).unwrap();
    sess.set_clock(lumen_core::ClockMode::Real).unwrap();
    assert_eq!(sess.ctx.frozen_clock_ms(), None);
}

#[test]
fn set_rng_seed_stores_and_clears() {
    let mut sess = InProcessSession::new_headless(1024, 768).unwrap();
    sess.set_rng_seed(Some(12345)).unwrap();
    assert_eq!(sess.ctx.rng_seed(), Some(12345));
    sess.set_rng_seed(None).unwrap();
    assert_eq!(sess.ctx.rng_seed(), None);
}
```

> Если `InProcessSession::new_headless` требует аргументов — найди реальную
> сигнатуру грепом: `grep -n "fn new_headless" crates/driver/src/session.rs`

### 6. Проверить

```bash
cargo clippy -p lumen-core --all-targets -- -D warnings
cargo clippy -p lumen-driver --all-targets -- -D warnings
cargo test -p lumen-driver 2>&1 | tail -5
```

### 7. Закоммитить и влить

```bash
git add crates/core/src/ext.rs crates/core/src/lib.rs \
        crates/driver/src/context.rs crates/driver/src/session.rs
git commit -m "P1: BrowserSession::set_clock + set_rng_seed (8F.1 + 8F.2)

ClockMode { Frozen(u64), Real } + default no-op методы в trait.
InProcessSession делегирует в SessionContext::set_frozen_clock/clear.
SessionContext::set_rng_seed (поле уже было). 3 unit-теста.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-browsersession-set-clock -m "Merge p1-browsersession-set-clock: set_clock/set_rng_seed"
git branch -d p1-browsersession-set-clock
git add STATUS-P1.md && git commit -m "P1: отметить p1-browsersession-set-clock завершённой"
git push origin main
git worktree remove .claude/worktrees/bs-set-clock
```

---

## Критерии готовности

- [ ] `ClockMode` enum в `lumen-core::ext` + ре-экспорт
- [ ] `set_clock` / `set_rng_seed` в `BrowserSession` trait с default no-op
- [ ] `InProcessSession` impl делегирует в `SessionContext`
- [ ] `SessionContext::set_rng_seed` / `rng_seed()` exist
- [ ] 3 unit-теста проходят
- [ ] Clippy чист (`lumen-core` + `lumen-driver`)
