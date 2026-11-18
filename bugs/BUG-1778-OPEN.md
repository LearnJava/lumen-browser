# BUG-1778 — cnn.com: поток движка занят задачей `about_to_wait.rs:290` на 25 с, `geom_diff` не снимает страницу

**Статус:** OPEN (DEBTOR) — закрыт для P3 2026-11-17; остаток — [PERF-WIDE-ROAD](../docs/tasks/PERF-WIDE-ROAD.md)/[ADR-036](../docs/decisions/ADR-036-incremental-restyle-invalidation.md) (P6), граница долга в конце карточки
**Заведён:** 2026-10-28 (P3, остаток [BUG-1676](BUG-1676-FIXED.md))
**Область:** shell / js / layout — локализовано: same-tick флаши внутри `run-scripts` (см. §Перепись)

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
- **vc-ru** (2026-11-13, A/B BUG-1800) — TIMEOUT 240 с 2/2 (ready нет), CPU сайта 360–370 с, DOM 2244, 990 флошей за сессию; на ON и OFF одинаково — не каскад.
- Хвост SITES-TRIAGE-2 (2026-11-13, `.tmp/handoff5-rerun`): **tutu-ru** — текст 0 2/2 при ~9,8 тыс. у Chrome, busy `about_to_wait.rs:295`; **elpais-com** — run0 200, busy + `null (reading 'postMessage')`, run1 уже 403 (стена поверх); **film-ru/bild-de** — из зависаний ушли, но во второй попытке тот же busy-флап text=0. **dailymail-com** — TIMEOUT 2/2 без busy-улики (`DocumentReady` после 301, Chrome сам флапает) — в баг не дописан.

Из базового списка зависаний при этом ушли 22 из 30 сайтов (эффект фиксов после 2026-10-28, прежде всего BUG-1676).

## Перепись (P3, 2026-11-17) — срез 1: шторм = 15 полных флашей (`forced_same` 99,7 %) внутри run-scripts 49 с из 79; рычаг (2) карточки снят срезом 8 BUG-1800 (2026-11-14); негейтованный рычаг один — dependency-checked reuse, владелец BUG-1800

Метод: постоянная перепись (`LUMEN_FRAME_LOG=1`, `[engine] incr cascade`/`incr deep roots`/`incr stages`), waterfall `--trace-nav`, сэмплер `scripts/native_sample.py` (12 срезов на t≈35 с, RVA через `native_resolve.py`). Код не менялся; сборка dev-release ветки `p3-bug-1778` (= `bdda6efb9`, срез 15 BUG-1800).

**Воспроизведение** (headless, `edition.cnn.com` через прокси): `--dump-layout` — 147/118/193 с в трёх прогонах (форма сессии гуляет; в карточке 56 с на сборке 2026-10-28), дамп 4573 строк; `--trace-nav` — 79 с.

**Waterfall** (`--trace-nav`, 79 с): `run-scripts` **49,4 с**, web-fonts 8,5 с, layout 2,6+1,3+1,1 с, fetch-images 2,4 с, fetch-document 1,5 с. Шторм живёт внутри исполнения скриптов: чтения DOM скриптами cnn форсируют same-tick флаши (`maybe_flush` гейтится `flush_stale` — все семантически принудительные).

**Флаши** (перепись, 135 шт.): 120 дешёвых (recomputed <100, walk ~4 мс) + **15 полных** — recomputed 3529–3539 (весь документ), forced 3539, `forced_same` 3530 (99,7 %), walk 2,0–4,6 с каждый (сумма 45,2 с); candidates 357,9 тыс. на флаш (universal 53 %, class 30 %, type 10 %, attr 7 %), match_calls 983k, вложенные матчи `:is/:where/:not/:has` 1,41M, ancestor-bloom отсёк 213k обходов предков (срез 5 BUG-1800 работает).

**Deep-корни полных флашей**: `#node.(9207)` — безымянный узел = **корень документа** от стража `has_in_shadow_doc` (консервативный режим теней); `html.userconsent-cntry-us userconsent-state-…(9206)` — класс на `<html>` от consent-баннера; класс-записи больших контейнеров (`div.ad-slot-header__wrapper(22)` → `header…(2044)`, `div.layout__content-wrapper…(6941)`). Union = весь документ.

**Сэмплер** (горячий поток, 74 % CPU за 2 с): `compute_style_shareable` → `matches_chain` → `matches_pseudo_class` (+`input_type_lower`); `compute_pseudo_element_style_inner` → `apply_declaration` → `parse_display`; `ShareCache::compute`/`is_fingerprintable`; лист — `RtlAllocateHeap`. `drop_glue<ComplexSelector>` в горячих стеках отсутствует.

**Рычаг (2) карточки снят**: пункт «не клонировать `ComplexSelector` в `matches_complex_for_pseudo`» реализован срезом 8 BUG-1800 (2026-11-14, «матч псевдоселекторов без клонов», pre-match subject-компаунда до построения модифицированного селектора, `pseudo.rs::matches_complex_for_pseudo`) — улика сэмплера 2026-10-28 устарела.

**Вердикт.** Задача из `about_to_wait.rs:290` не локализуется в отдельный дефект тика: 25 с занятости потока живого окна = те же same-tick флаши, форсированные чтениями DOM из таймеров/скриптов (семья BUG-1717/BUG-1800; на ebay тот же стек `getAttribute`). Держатель цены флаша на cnn тот же, что на github (срезы 12–13 BUG-1800): doc-root от стража + класс-записи больших поддеревьев; снятие стража дважды откачено (s12, s13 — «третьей попытки без новых фактов не предпринимать»). Негейтованный рычаг ровно один — **dependency-checked reuse**: поглощён в BUG-1800 из BUG-1794, назван в переписи с2 («forced_same 1550/1552»), ни разу не строился; на cnn закрыл бы 3530 из 3539 форсированных пересчётов каждого полного флаша. Владелец рычага — BUG-1800 (строка 1 очереди P3); сам по себе это механизм класса PERF-INVALIDATION-S2 (перенос грязного поддерева), у P6 зарезервирован стенд S1. *(Дополнение, срез 16 BUG-1800, 2026-11-17: рычаг закрыт разбором как пустой по построению — resurrect prev не имеет потребителя между проходами, а skip-лицензия по identity `inherited` отказывает под каждым has-reach-кандидатом и doc-корнем стража, чьи поддеревья и есть union каждого полного флаша; cnn и github остаются на gated-классе — has-reach-диф / lift консерватизма.)*

**Гейт:** `cargo clippy -p lumen-layout -p lumen-js --features v8-backend -p lumen-shell --all-targets -D warnings` чистый (код не менялся, дерево бит-в-байт HEAD); `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py` без расхождений. Коммит — только документация.

Попутно: ловушка вызова `--trace-nav` — форма `--dump-layout <url> --trace-nav <out.json>` молча читает out.json как источник страницы («os error 2» в предупреждении), см. [BUG-1821](BUG-1821-OPEN.md); ловушка дописана в `docs/probe-method.md` §2.

## Закрытие для P3 (2026-11-17) — граница долга

**Статус `OPEN (DEBTOR)`:** отдельного дефекта в тике `about_to_wait.rs:290` нет — P3 задачу не берёт.

- Симптом объяснён переписью §Перепись: 25 с занятости потока = 15 полных same-tick флашей внутри `run-scripts` (49,4 с из 79), не таймеры и не пампы.
- Оба рычага карточки закрыты: клонирование `ComplexSelector` в `matches_complex_for_pseudo` — срезом 8 BUG-1800; dependency-checked reuse — срезом 16 BUG-1800 (пуст по построению).
- Остаток — тот же gated-класс, что у BUG-1800: lift консерватизма shadow-стража и doc-wide корни лист-дельт (`forced_same` 99,7 %); архитектурный контур закрыт [ADR-036](../docs/decisions/ADR-036-incremental-restyle-invalidation.md), владелец — P6, не P3.
- Возобновление P3: только если `geom_diff --only cnn` останется красным после закрытия gated-класса или появится новый факт вне него.
- Сайты-свидетели той же семьи (discord-com, magnit-ru, tutu-ru, elpais-com, ebay-com) отдельных P3-рычагов не добавляют; vc-ru — не каскад (TIMEOUT одинаков на ON/OFF).
