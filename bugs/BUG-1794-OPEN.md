# BUG-1794 — github.com: сталл движка на загрузке — eval >5 с, страница blank

**Статус:** OPEN (срез 2 — `popover`/`inert` стали attr-local; живой github идёт в
conservative-режим теневого DOM, `ready_s` им не двигается — см. срез 2)
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** js/shell — производительность V8 на тяжёлом JS (Primer/React), `about_to_wait.rs`

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://github.com` — HUNG в обоих прогонах: движок занят >5 с на одном eval во время загрузки (`about_to_wait.rs:295`), `ready_s` 47–63 с против ~4 с у Chrome.
- Итог: `blank:near-empty`, скриншот и a11y-дерево не сняты.
- Остальные 9 сайтов корпуса проходят — это не общий сталл, а специфика тяжёлого JS github (Primer CSS, React, большой DOM).

## Что сделать

1. **Сначала перепроверить на dev-release** (`target/dev-release/lumen.exe`, собрать `cargo build --profile dev-release -p lumen-shell` при необходимости) — debug-профиль может exaggerate CPU-стоимость; если на dev-release страница грузится за разумное время, severity ниже, но баг остаётся.
2. Если воспроизводится — снять CPU-профиль V8 на загрузке github.com (V8 `--prof`/`--trace` через существующие механизмы или sampling-профилировщик), найти горячий цикл.
3. Кандидатные направления: длинные синхронные layout/relayout при DOM-мутациях React, повторные `getComputedStyle`/selector-матчи, O(N²) в flat-tree/collect-обходах на большом DOM.

## Проверка

- compat_crawl на github.com: status/blank/exceptions зелёные, `ready_s` в пределах разумного от Chrome.
- Регрессионный бенчмарк или WPT-стиль тест на паттерн, который вешал eval.

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
- Близкие места: `crates/shell/src/app/about_to_wait.rs:295` (eval-сталл watchdog), engine-thread pump (`relayout.rs::pump_raf_engine_thread`).

## Срез 2 (2026-11-15, P3): `popover`/`inert` attr-local; куда реально уходит живая загрузка

**Живой github (dev-release, `--maximized`, `LUMEN_NO_ADBLOCK=1`, compat-crawl
`--only github`)**: `ready_s` 24.9/35.3 с против 3.1–3.2 с у Chrome, `text_len 0`,
`engine thread busy: eval not run within 5.0 s … about_to_wait.rs:329 for 13.2 s`.
stderr-разбор (`LUMEN_FRAME_LOG=1 LUMEN_JS_STALL_SAMPLE_MS=2000`, 4.3 МБ лога):
все 7 флешей за загрузку — суммы 22.9 с, худшие 3.2–6.4 с; в каждом
`incr deep roots: ["#node.(2693)"]` — deep-корень **весь документ** (2693 узла),
`forced_same 2033 из 2087` (почти все пересчёты дают тот же стиль). Стеки JS
показывают `getBoundingClientRect` из `react-dom-client` — React вынуждает
флеши после мутаций.

**Причина deep-корня — не каскад, а страж теневого DOM.** `#node` в логе —
`NodeData::Document` (у него нет имени элемента), а не изменённый узел.
`NodeRestyleIndex` строится с `conservative = document_has_shadow_roots(doc)`
(`restyle.rs`), а `node_change.rs` при `has_dependent && has_in_shadow_doc`
возвращает `doc.root()` для **любого** изменения. У github есть авторские
shadow-корни (кастом-элементы Catalyst: `ghcc-consent`, `fullstory-capture`,
`cookie-consent-link`) и `:has()` в листе — то есть narrowing выключен всегда,
и каждая запись атрибута пересчитывает весь документ.

**Замер стража (эксперимент, откачен).** Временно `conservative = false` и
ранний возврат выключен: суммы флешей 22.9 → 13.3/13.8 с, корни становятся
узкими (`deep=0 shallow=6` на записях `img:style`), `LUMEN_VERIFY_INCR_CASCADE=1`
отвечает `incr cascade: 2086 styles, equal to a full cascade` ×4 (инкремент
эквивалентен полному каскаду). **Но `ready_s` не двигается**: 24.5/24.8 с против
24.9/27.6/35.3 с без правки. Значит готовность страницы держит не каскад
рестайла — выигрыш тонул в остальном пути загрузки. Правка откачена по
дисциплине S32/S36 (нет измеримого выигрыша — нет кода).

**Что сделано в срезе.** Записи `popover`/`inert` (ровно тот набор, что виден в
живом `changes=`-логе: контейнеры поповеров Primer) перестали быть deep-корнем:
`attr_change_stays_local` отвечает `true`, корень — сам узел и его прямые дети
(глубина только если стиль узла реально сдвинулся). Soundness: оба атрибута
читаются на самом элементе (UA-правила `[popover]`, кэш
`data-lumen-popover-open`, top-layer/inert-механика), ни один псевдокласс из
`pseudo_class_read_attr_names` их не читает, а `[popover] .x`/`[inert] .x`
попадает в `deps.attrs` и остаётся на deep-пути. Тест
`a_popover_or_inert_write_stays_local_unless_a_selector_reads_it_from_above`
(`style/tests/node_fanout_tests.rs`): до правки deep-корень, после — shallow;
вторая половина теста держит отрицательное плечо (`[popover] .item` — deep).

Стенд `.tmp/popover_flush_stand.html` (1200 ячеек, `popover`/`inert` туда-обратно +
`getBoundingClientRect`): форма корней `deep=1 shallow=1` → `deep=0 shallow=2`
в каждом тике (A/B через `LUMEN_NO_ATTR_LOCAL_ROOTS=1`); время флеша на стенде не
двигается (лист мелкий) — это правка точности reach, а не измеренный перф-выигрыш.
На живом github она тоже ничего не меняет: страж теневого DOM обнуляет narrowing.

**Следующий срез — не каскад.** Готовность держат: первый полный проход
(`layout_measured_hyp` 2.7–3.4 с, из него `cs_match` 1.5 с), флеши по 3 с во время
React-гидрации и paint; измерять надо живой путь целиком (`--trace-nav` + кадровый
лог), а страж `has_in_shadow_doc` трогать отдельной задачей: выигрыш по сумме
флешей 23 → 13 с уже измерен выше, но на `ready_s` он не влияет.

**Гейт:** `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib` 6011 зелёных, 4 ignored; `graphic_tests/dump_golden.py`
(LUMEN_PROFILE=dev-release) — 12/12 совпадают.

## Срез 1 (2026-11-14, `p3-bug-1794`): форк карты custom-properties на каждом узле

**Воспроизведение на dev-release** (`target/dev-release/lumen.exe`, `LUMEN_NO_ADBLOCK=1`,
`compat_crawl.py --only github`): `lumen=DEGRADED`, `ready_s` 27.9 с против 2.7 с у Chrome,
`text_len` 0 — eval не прошёл: `engine thread busy: eval not run within 5.0 s (running task from
crates\shell\src\app\about_to_wait.rs:329:13 for 5.7 s)`. Headless тот же дефект:
`--trace-nav https://github.com/` — 53.5 с до конца навигации, first-paint на 48.3 с.

**Перепись** (`LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1 lumen --dump-layout --viewport
1920x1080 https://github.com/`; 3941 бокс, документ 14538 px, 2078 `compute_style` за проход):

| стадия | мс/проход | доля |
|---|---|---|
| `precompute_counters` (каскад) | 2818 | 86 % |
|  · `compute_style` ×2078 | 2602 | 80 % |
|  · · `cs_init` | 934 | 29 % |
|  · · `cs_match` | 1414 | 43 % |
| `build_box` | 319 | 10 % |
| `lay_out` (геометрия) | 125 | 4 % |

Внутри `cs_init` почти весь бюджет держал блок сброса `inherits: false`-свойств:
`cs_props_copy` — 740 мс на **2077 из 2078** узлов (356 мкс на копию), сам скан — 0.45 мс.

**Причина.** Primer на github.com регистрирует `@property --dialog-scrollgutter { syntax:
'<length>'; inherits: false; initial-value: 0 }` (и `--prc-dialog-scrollgutter`). В карте узла
лежит **вычисленное** значение `0px`, а в дескрипторе — **сырое** `0`. Защита BUG-683 среза 8
(`iv == value && validate_against_syntax(iv, …)`) сравнивает строки, поэтому «`0` ≠ `0px`»
читалось как «значение другое»: `Arc::make_mut` + `retain` форкали карту из 2143 записей на
каждом узле, а проходов раскладки на странице три.

**Фикс.** `property_syntax.rs::initial_value_equivalent` сравнивает вычисленные формы обеих
сторон (`cascade_compute_registered_value`); равенство признаётся только если оно держится при
**двух разных em-базах** (16 и 32 px) — значение с зависимостью от `font-size` (`2em` против
фиксированного `32px`) эквивалентным не считается, и тогда прежнее поведение (сброс и форк)
сохраняется. `cascade.rs` использует его в `resets_inherited`. Регресс-тест:
`custom_props_non_inherited_registered_unitless_initial_value_stays_shared` (соседний
`…_resets_to_initial_in_child` держит отрицательное плечо).

**Эффект** (dev-release, эта машина; разброс между прогонами ±30 %, поэтому — порядок величин):

| метрика | до | после |
|---|---|---|
| `cs_init` на проход | 934 мс | 17 мс |
| проход раскладки | 3.27 с | 1.50 с |
| `--dump-layout` целиком | 24.9 с | 9.6 с |
| `--trace-nav` до конца навигации | 53.5 с | 24.9 с |
| first-paint | 48.3 с | 24.5 с |

**Остаток после среза** (не чинилось). `cs_match` 0.8–1.6 с на проход: индекс кандидатов
отдаёт в среднем ~170 правил на узел (1.53 млн пар за страницу, максимум 293; сам запрос
`cs_cands` — 13 мс), а цикл по ним (`cs_loop`) 1.21 с, т.е. ~3.4 мкс на правило-кандидат.
`paint` 10.2 с (было 13.8 с — частично шум станка) и `web-fonts` 2.5 с на headless-проходе.
Приёмочный критерий карточки (compat-crawl github зелёный, `ready_s` в пределах разумного от
Chrome) срез не закрывает.

**Гейт:** `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib` 6010 зелёных (единственный красный
`deep_chain_with_margins_collapses_through_the_trampoline` — дедлайн-флейк BUG-1026 под
нагрузкой, соло и в повторе зелёный); `scripts/scoped-test.sh` — красные только
задокументированные (BUG-1809, BUG-1236, BUG-1785/1795, BUG-1807, hevc — железо, BUG-1804)
плюс два тайминговых флейка shell (`load3a_live_async_script_does_not_delay_the_page`,
`warm_preload_cache_sends_no_request_forbidden_by_csp`), проходящих соло;
`graphic_tests/dump_golden.py` — 12/12.
