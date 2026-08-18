//! Single consolidated integration-test binary for lumen-driver (BT-1).
//! See `cases/mod.rs` for rationale and the per-test submodules.

// `panic!` — штатный способ провалить тест. Исключение `allow-panic-in-tests`
// из clippy.toml сюда не достаёт: оно освобождает только тело `#[test]`-функции,
// а не хелперы тестового крейта. Подробности — docs/lint-policy.md §10.
#![allow(clippy::panic)]

mod cases;
