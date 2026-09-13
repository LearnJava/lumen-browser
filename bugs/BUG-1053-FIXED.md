# BUG-1053 — ГЕЙТ СЛОМАН: `cargo clippy --workspace --all-targets --profile dev-release -- -D warnings` красный на `main` независимо от задачи

**Статус:** FIXED 2026-09-13 (P1)
**Заведён:** 2026-09-13 (P3, финальный гейт BUG-561)
**Область:** layout (`crates/engine/layout/src/invariants.rs:50,55,75` — `check_geometry`/`check_finite`/`check_containment`)
**Владелец:** P1/P5

## Симптом

```
error: function `check_geometry` is never used
  --> crates\engine\layout\src\invariants.rs:50:15
error: function `check_finite` is never used
  --> crates\engine\layout\src\invariants.rs:55:4
error: function `check_containment` is never used
  --> crates\engine\layout\src\invariants.rs:75:4
error: could not compile `lumen-layout` (lib) due to 3 previous errors
```

Воспроизведено в отдельном `git worktree add … origin/main` (чистый `HEAD`
`92cd96d2b`, без единого моего изменения) тем же `cargo clippy -p
lumen-layout --all-targets --profile dev-release -- -D warnings` — значит не
дрейф ветки `p3-bug561-css-supports-allowlist`, а поломка самого `main`.

## Причина

`check_geometry`/`check_finite`/`check_containment` (DEVX-8a, `debug_assert!`-
only инварианты) вызываются только из трёх `#[cfg(debug_assertions)]`-сайтов
в `box_tree/entry.rs`. `dev-release` (`Cargo.toml:188`) `inherits = "release"`
→ `debug-assertions = false`, поэтому в `dev-release`-сборке все три вызова
исчезают вместе со своим `cfg`, и вне тестового target (`lib`, не `tests`)
функции остаются без единого вызывающего — `dead_code`.

По CLAUDE.md §Testing gates полный `--workspace` прогон происходит только в
`/lumen-task-finish` и P5-ревизии — отсюда и то, что поломка не всплывала
раньше: обычная работа гоняет `-p <crate>`, где `lumen-layout` целиком не
попадает в область.

## Масштаб

Не изолировано в `lumen-layout`: обычный `cargo build --workspace --profile
dev-release` (без `-D warnings`, поэтому не падает, но предупреждает) кажет
тот же паттерн в `crates/engine/paint/src/invariants.rs` —
`check_coverage`/`check_clip_stack_balance`/`check_origins_resolve`/
`check_visible_boxes_have_spans` тоже `never used` в `dev-release`. Похоже,
это системная дыра в паре `debug_assert!`-инвариантов и профиля `dev-release`
(`debug-assertions = false` через `inherits = "release"`), а не разовая
ошибка одного модуля — вероятно, стоит поискать остальные экземпляры across
the workspace, а не чинить по одному.

## Что дальше

Не чинил в рамках BUG-561 (несвязанный крейт, drive-by fix запрещён
конвенцией) — либо `#[cfg_attr(not(any(debug_assertions, test)), allow(dead_code))]`
на все три функции, либо перевод вызовов в `entry.rs` на что-то не завязанное
на `debug_assertions` (например собственный feature-флаг), если инварианты
должны молчать и в `dev-release`/`release` тоже. Блокирует любой финальный
гейт `/lumen-task-finish`, пока не закрыт.

## Исправление (P1, 2026-09-13)

`#[cfg_attr(not(debug_assertions), allow(dead_code))]` (в `lumen-layout` — с
дополнительным `test`-исключением, т.к. `check_geometry` там вызывается
напрямую из `#[cfg(test)] mod tests` этого же файла) на все функции обеих
цепочек: `check_geometry`/`check_finite`/`check_containment`
(`crates/engine/layout/src/invariants.rs`) и
`check`/`check_coverage`/`check_clip_stack_balance`/`check_origins_resolve`/
`check_visible_boxes_have_spans` (`crates/engine/paint/src/invariants.rs`).
Тот же приём подтверждён `cargo build --workspace --profile dev-release`
(0 warnings) — других экземпляров этого паттерна в воркспейсе не нашлось,
масштаб из раздела «Масштаб» ограничился этими двумя крейтами.
`cargo clippy --workspace --all-targets [--profile dev-release] -- -D
warnings` зелён; `cargo test -p lumen-layout -p lumen-paint invariants` —
20/20.
