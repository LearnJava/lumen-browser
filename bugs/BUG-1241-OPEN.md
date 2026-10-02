# BUG-1241 — `cargo test -p lumen-js --lib` не компилируется: в `dom/tests/*` не резолвятся `make_doc`, `Arc`, `Mutex`, `Document`

**Статус:** OPEN
**Тип:** сборка тестов.
**Заведён:** 2026-10-02 (P4, найден в гейте задачи `block-size-logical`, не относится к ней)
**Область:** js (`crates/js/src/dom/tests/v8_bug1148_srcset_img_load.rs`, `v8_bug1158_raw_text_inner_html.rs`, `v8_bug1167_ce_wrapper_gc.rs` и др.).

## Симптом

`cargo test -p lumen-js --lib` → `error: could not compile lumen-js (lib test) due to 28 previous errors`: E0425 `cannot find function make_doc` (10), `cannot find type Arc` (5), `Document` (3), `Mutex` (4), плюс `unresolved import crate::v8_runtime`. Воспроизводится на `main` без правок P4 (проверено `git stash`). `scoped-test.sh` из-за этого помечает `-p lumen-js --lib` как упавший.

## Как проверить

`cargo test -p lumen-js --lib --no-run` на чистом `main`.
