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

## Источник: сайты (дополнение SITES-TRIAGE-3, 2026-11-13)

Перепрогон топ-30 TIMEOUT/HUNG базового прогона 2026-10-20 на сборке 2026-11-13 (`.tmp/hang30-rerun/results.jsonl`, отчёт §Разбор зависаний):

- **cnn-com** (вес 3) — воспроизводится и headless: `--trace-nav` не завершается за 90 с (навигация не закрывается); в живом окне 2/2 `wait timeout: DocumentReady` после 301. Основной носитель бага.
- **discord-com** (вес 3) — headless завершается за 23 с (краска есть), живое окно: run0 «engine busy» + перезапуск, run1 `wait timeout: DocumentReady` 2/2 при text=0. Зависание только живого окна — стадия тика/рестайла.
- **magnit-ru** (вес 3) — `engine thread busy: … about_to_wait.rs:295 for 13,4–13,6 с` 2/2, видимый текст 0 при 10 тыс. у Chrome; готова 19–20 с, но eval не проходит.
- **ebay-com** (вес 3) — разовый `about_to_wait.rs:295 for 1,4 с` (run1); переход из TIMEOUT базового прогона в DEGRADED — улика та же, вклад мал.
- Хвост SITES-TRIAGE-2 (2026-11-13, `.tmp/handoff5-rerun`): **tutu-ru** — текст 0 2/2 при ~9,8 тыс. у Chrome, busy `about_to_wait.rs:295`; **elpais-com** — run0 200, busy + `null (reading 'postMessage')`, run1 уже 403 (стена поверх); **film-ru/bild-de** — из зависаний ушли, но во второй попытке тот же busy-флап text=0. **dailymail-com** — TIMEOUT 2/2 без busy-улики (`DocumentReady` после 301, Chrome сам флапает) — в баг не дописан.

Из базового списка зависаний при этом ушли 22 из 30 сайтов (эффект фиксов после 2026-10-28, прежде всего BUG-1676).
