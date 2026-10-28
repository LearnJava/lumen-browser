# BUG-1676 — microsoft.com / cnn.com не доходят до `DocumentReady` за 90 с, bbc.com — «слишком долго не отвечает»

**Статус:** FIXED 2026-10-28 (P3; microsoft — исправлено, остальные три сайта разобраны, остаток cnn — [BUG-1778](BUG-1778-OPEN.md))
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md))
**Область:** layout (`crates/engine/layout/src/style/cascade.rs`, `cascade_index.rs`, `env.rs` — `SHADOW_SHEETS`)

## Симптом

Прогон `scripts/geom_diff.py` (Lumen `target/dev-release`, `LUMEN_NO_ADBLOCK=1`, окно 1280×720): microsoft.com и
cnn.com — `page still loading` после ~90 с, bbc.com — страница ошибки сети, ria.ru — `script.evaluate` голодает
за redraw-задачами.

## Разбор по сайтам

### microsoft.com — причина найдена и исправлена

`lumen --dump-layout https://www.microsoft.com/` не завершался и через 170 с: все ресурсы получены (`← 200` на
последнем модуле), один поток крутится на 100 % CPU. Страница — Lit-компоненты (`store-*`), у каждого свой
shadow-root с листом дизайн-системы в тысячи правил.

Локализация: PDB-символы + `StackWalk64` по горячему потоку (скрипт-сэмплер, `SymFromAddr` по RVA) показали
`compute_style_shareable` → `Vec<Rule>::clone` → `Vec<Declaration>::clone` → `Stylesheet::clone` и
`drop_glue<Stylesheet>`. В [`cascade.rs`](../crates/engine/layout/src/style/cascade.rs) для **каждого элемента**
`own_shadow` / `host_shadow` / `interior_shadow` брались из `SHADOW_SHEETS` через `.cloned()` — глубокая копия
всего листа shadow-дерева. Стоимость: элементы × размер листа × число рестайлов (Lit перерисовывается после
каждого загруженного модуля). `StyleEnvSnapshot::capture()` копировал тот же словарь целиком при каждом снимке для
рабочего потока.

Побочный след, из-за которого поиск шёл через JS: перебор `querySelectorAllDeep` в `clientlib-reimagine-telemetry`
(16 тыс. вызовов, ~10 тыс. `matches`) — быстрый, к зависанию не причастен.

Исправление: `SHADOW_SHEETS: HashMap<NodeId, Arc<Stylesheet>>`; `set_shadow_sheets` (публичная сигнатура прежняя)
оборачивает листы в `Arc` один раз за проход, поиск и снимок среды копируют указатель. Тест
`shadow_sheet_lookup_shares_the_stylesheet_instead_of_cloning_it`.

Результат (`--dump-layout`, прямое соединение): microsoft.com ≥173 с без конца → 32 с; в живом окне
`geom_diff --only microsoft ria` снимает оба сайта (microsoft 30.7 % сопоставлено, ria 50.5 %).

### bbc.com — не дефект движка

Прямое соединение с этой сети: `TLS handshake: … не получен нужный отклик` (так же `--dump-tls www.bbc.com`),
`curl` проходит только через системный прокси `127.0.0.1:12334`. С `lumen --proxy http://127.0.0.1:12334`
страница грузится за 29 с (4121 строк layout-дампа). Для иностранных сайтов нужен прокси (правило проекта);
`geom_diff.py` запускает Lumen без `--proxy`, поэтому bbc в нём останется «не снят» на такой сети.

### ria.ru — не воспроизведено

`--dump-layout` — 12 с; в живом окне после исправления снимок берётся. Голодание `script.evaluate` за
redraw-задачами из заявки было следствием того же рестайла (UI-поток занят каскадом).

### cnn.com — отдельная причина → BUG-1778

`--dump-layout` (прямое) — 56 с, через прокси дольше; в живом окне задача `about_to_wait.rs:290` держит поток
движка 25 с, `geom_diff` получает `engine thread busy`. Горячий поток в каскаде псевдоэлементов. Вынесено в
[BUG-1778](BUG-1778-OPEN.md).

## Как проверить

`lumen --dump-layout https://www.microsoft.com/` завершается (≈30 с); `python scripts/geom_diff.py --only microsoft ria`.
