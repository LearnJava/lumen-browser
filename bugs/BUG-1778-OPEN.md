# BUG-1778 — cnn.com: поток движка занят задачей `about_to_wait.rs:290` на 25 с, `geom_diff` не снимает страницу

**Статус:** OPEN
**Заведён:** 2026-10-28 (P3, остаток [BUG-1676](BUG-1676-FIXED.md))
**Область:** shell / js / layout — не локализовано

## Симптом

`python scripts/geom_diff.py --only cnn --nav-timeout 120 --lumen target/dev-release/lumen.exe`:
`script.evaluate: … engine thread busy: eval not run within 5.0 s (running task from
crates\shell\src\app\about_to_wait.rs:290:13 for 25.2 s)`. Строка 290 — пакетный `route_task_js` тика
(`tick_timers`, `pump_websockets`, `pump_sse`, `pump_media`, `pump_workers`, …): какой из вызовов держит поток —
неизвестно.

Тот же сайт в `lumen --dump-layout https://www.cnn.com/` (без прокси): 56 с, 4597 строк дампа. Сэмплер
горячего потока (PDB + `StackWalk64`) на 50-й секунде показал каскад псевдоэлементов: `drop_glue<ComplexSelector>`,
`matches_chain` ← `matches_complex_for_pseudo` (в [`pseudo.rs`](../crates/engine/layout/src/style/pseudo.rs) на каждый
кандидат собирается копия селектора без псевдоэлемента: `stripped` + `complex.tail.clone()`), `apply_decl_text`.
Имена символов приблизительные (в `dev-release` только публичные символы).

## Что делать

1. В живом окне (`--maximized`, прокси для иностранных сайтов `--proxy http://127.0.0.1:12334`) определить, что
   именно в тике занимает 25 с: таймеры JS (принудительный layout из колбэка?) или рестайл. Профиль —
   `LUMEN_PROFILE_TREE=1`.
2. Если это `matches_complex_for_pseudo`: не клонировать `ComplexSelector` на каждый элемент — сопоставлять
   последний compound с пропуском `PseudoElement` без копии.

## Как проверить

`python scripts/geom_diff.py --only cnn --nav-timeout 120` снимает страницу.
