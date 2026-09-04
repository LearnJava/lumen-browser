# BUG-986 — `Document::get()` паникует на `NodeId` из чужого документа: `index out of bounds`

**Статус:** FIXED 2026-09-04
**Заведён:** 2026-09-04 (наблюдение за живым прогоном корпуса «топ-100 зарубежных», сессия-наблюдатель)
**Область:** `crates/engine/dom/src/lib.rs` (`Document::get`/`get_mut`), `crates/js/src/v8_runtime/install/dom_core.rs` (нативы дерева)
**Владелец:** P3

## Симптом

Процесс падает с паникой главного потока:

```
thread 'main' (9796) panicked at crates\engine\dom\src\lib.rs:706:20:
index out of bounds: the len is 39 but the index is 238
```

Место — индексация арены документа без bounds-check. Стек до паники (stderr
прогонов 2026-09-04, файл `BUG-986-OPEN.md` → история до фикса):

```
… _lumen_append_child … _ctor.appendChild …    ← amazon AWS WAF challenge.js
… JS event error: target must be an Element …  ← owa (outlook)
… Cannot read properties of undefined (reading 'navigationStart') …  ← bing
```

Во всех пяти паниках за три прогона на стеке — нативы дерева
(`_lumen_append_child` и соседние), вызванные парс-тайм скриптами. Нативы
принимают `u32`-NodeId со страницы и слали его прямо в индекс арены.

## Что говорит измерение

Пять паник, индекс ВСЕГДА больше длины, с большим отрывом (len 39 → index 238,
len 159 → 238, len 143 → 190) — это не выход за границу растущего документа, а
`NodeId` из другого, более крупного документа (кандидат: идентификатор,
переживший навигацию, или пересёкший границу вкладки). Сам источник чужих id в
этой сессии не локализован (нужен живой прогон; см. «Что осталось»).

## Фикс (P3, 2026-09-04)

Три слоя.

### 1. DOM — checked-доступ + диагностика (`crates/engine/dom/src/lib.rs`)

- добавлены `Document::contains_id(id)`, `Document::try_get(id) ->
  Option<&Node>`, `Document::try_get_mut(id) -> Option<&mut Node>` —
  bounds-check против арены, `None` на чужом id;
- `get`/`get_mut` теперь делают bounds-check и паникуют через
  `#[track_caller]`-функцию `foreign_id_panic` с сообщением
  `BUG-986: NodeId N вне арены документа (len M) — …; вызывающий: <call site>`.
  `#[track_caller]` называет Rust-место вызова (натив) в логе живого прогона
  **без `RUST_BACKTRACE=1`** — первый шаг из заявки («бэктрейса нет, первый
  шаг бесплатный») теперь происходит автоматически при каждом нарушении.
  Исключение `clippy::panic` зарегистрировано в `docs/lint-policy.md` §10.

### 2. JS-граница — чужие id больше не роняют процесс (`crates/js/src/v8_runtime/install/dom_core.rs`)

Нативы, чьи `u32`-id приходят со страницы, валидируют их через
`contains_id`/`try_get` до обращения к арене:

- мутации (`_lumen_append_child`, `_lumen_remove_child`, `_lumen_insert_before`,
  `_lumen_set_text_content`, `_lumen_set_inner_html`): на чужом id — строка
  `[BUG-986] <натив>: NodeId N вне арены документа (len M) — операция
  пропущена` в stderr (его читает живой аудит, `live.stderr.*.log`) и return
  без изменения дерева — окно живёт, страница видит «неуспех» и продолжает;
- чтения (`_lumen_get_children`, `_lumen_get_parent`, геттеры
  textContent/innerHTML/outerHTML): деградируют через `try_get` — пустой
  список / `None` / `""`. Для `_lumen_get_parent` это значит, что bubble-обход
  события на устаревшем узле просто останавливается (None), а не валит
  процесс посреди диспатча.

Помощник `log_foreign_node_id` вынесен в начало файла.

### 3. Регресс-тесты (`crates/engine/dom/src/lib.rs::tests`)

5 тестов на форму паники из живых прогонов (чужой id = 238):

- `contains_id_false_for_foreign_node_id` — `contains_id(238)` = false при 3
  узлах, валидные id = true;
- `try_get_returns_none_for_foreign_node_id` / `try_get_mut_returns_none_…`;
- `get_panics_with_bug986_diagnostic_on_foreign_id` /
  `get_mut_panics_with_bug986_diagnostic_on_foreign_id` —
  `#[should_panic(expected = "BUG-986")]`.

## Проверка

- `cargo test -p lumen-dom --lib` — 292/292 (287 прежних + 5 новых).
- `cargo clippy -p lumen-dom -p lumen-js --features v8-backend --all-targets --
  -D warnings` — чисто.
- `cargo test -p lumen-js --lib` — 222/222 (дефолтные фичи).

## Что осталось

Сам источник чужих `NodeId` не локализован: пять паник воспроизводятся только
на живых сайтах (amazon WAF, owa, bing) и упираются в «нужен бэктрейс». Теперь
каждое будущее нарушение печатает `BUG-986: NodeId …; вызывающий: <натив>` в
stderr — повторный прогон корпуса (гейт регрессий, `docs/perf/reports/
top100-foreign-2026-09-04/HANDOFF.md` §8.4) назовёт конкретного вызывающего, и
источник (переживший навигацию / пересёкший границу вкладки id) закроется
отдельным багом.

## Дополнение (P2, срез 3 WPT-RUN-7, 2026-09-04) — детерминированная репродукция через вендоренный WPT-тест

Найдено при генерации expectations baseline для категории `input-events`, **до**
пересборки на фиксе выше (бинарь ещё со старым `Document::get`, без checked-
доступа) — тот же класс паники, но без внешнего сайта, воспроизводится
детерминированно одним вендоренным тестом, что для отладки удобнее amazon/
outlook/bing:

```
thread 'main' panicked at crates/engine/dom/src/lib.rs:706:20:
index out of bounds: the len is 42 but the index is 456
```

Тест: `input-events/input-events-get-target-ranges-deleting-in-list-items.tentative.html?Delete,ul`
(`run_report.py --binary target/dev-release/lumen --check --all --root input-events --recursive`).
Паника рвёт BiDi-сессию (`navigate: live window closed before replying`), следующий
тест в том же окне получает `ERROR`, wptrunner перезапускает браузер — из-за этого
`--check` сразу после `--update-expected` на категории `input-events` даёт нестабильный
результат (`Test OK, expected ERROR` на соседнем тесте: паника не всегда бьёт в одном
и том же месте между двумя прогонами одной и той же категории). И здесь индекс (456)
намного больше длины (42) — тот же паттерн «`NodeId` из чужого документа/арены», не
рост документа. Тест работает с `StaticRange`/`getTargetRanges()` при удалении элементов
списка через input events — правдоподобный источник разжившегося `NodeId`: диапазон
кэширует узлы, которые синхронно удаляются тем же действием.

Первый шаг отладки — прогнать именно этот тест с `RUST_BACKTRACE=1` через
`tests/wpt/run_smoke.py 'input-events/input-events-get-target-ranges-deleting-in-list-items.tentative.html?Delete,ul'` —
самовоспроизводится без прогона всей категории и без внешней сети, бэктрейс отсюда
бесплатный, в отличие от живого прогона по внешним сайтам. Требует перепроверки на
бинаре с фиксом выше (checked-доступ мог накрыть и этот call site — `StaticRange`,
похоже, читает узлы напрямую, не только через перечисленные в фиксе нативы).

## Сырые данные

`.tmp/perf-audit/20260904-*/live.stderr.*.log`, разбор —
`.tmp/observe/OBSERVATION-2026-09-04.md` §3.
