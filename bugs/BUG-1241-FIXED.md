# BUG-1241 — `cargo test -p lumen-js --lib` не компилируется: в `dom/tests/*` не резолвятся `make_doc`, `Arc`, `Mutex`, `Document`

**Статус:** FIXED 2026-10-07
**Тип:** сборка тестов.
**Заведён:** 2026-10-02 (P4, найден в гейте задачи `block-size-logical`, не относится к ней)
**Область:** js (`crates/js/src/dom/tests/v8_bug1148_srcset_img_load.rs`, `v8_bug1158_raw_text_inner_html.rs`, `v8_bug1167_ce_wrapper_gc.rs` и др.).

## Симптом

`cargo test -p lumen-js --lib` → `error: could not compile lumen-js (lib test) due to 28 previous errors`: E0425 `cannot find function make_doc` (10), `cannot find type Arc` (5), `Document` (3), `Mutex` (4), плюс `unresolved import crate::v8_runtime`. Воспроизводится на `main` без правок P4 (проверено `git stash`). `scoped-test.sh` из-за этого помечает `-p lumen-js --lib` как упавший.

## Как проверить

`cargo test -p lumen-js --lib --no-run` на чистом `main`.

## Корень и исправление

В `crates/js/src/dom/tests/mod.rs` атрибут `#[cfg(feature = "v8-backend")]` стоял только у первого `mod` блока; 20 следующих модулей (`v8_bug935_s58…s77`, `v8_bug1148…`, `v8_bug1158…`, `v8_bug1167…`, `v8_bug1207…`, `v8_soft_navigation_s1` и др.) компилировались без фичи и не находили v8-only символы (`make_doc`, `Arc`, `crate::v8_runtime`, `v8_bug935_s55_content_journal`). Каждому модулю добавлен свой `cfg`. Проверка: `cargo test -p lumen-js --lib --no-run` (без фичи) и с `--features v8-backend` собираются.

Не относится к багу: `cargo clippy -p lumen-js -- -D warnings` без `v8-backend` красный на самом lib (неиспользуемые импорты в `push_api.rs`, `worker.rs`, `deterministic_patch_script`); рабочий гейт идёт с `--features v8-backend`.
