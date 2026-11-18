# PERF-WIDE-ROAD — shadow-DOM страницы выходят из широкой дороги инкрементального рестайла

**Developer:** P6
**Ветка:** `p6-perf-wide-road`
**Размер:** L (три среза, лимит как у PERF-INVALIDATION)
**Крейты:** `lumen-layout`, `lumen-js` (стенд), `lumen-shell` (живой замер)

Заведена 2026-11-18 по решению пользователя. Владелец остатка
[BUG-935](../../bugs/BUG-935-OPEN.md), [BUG-1778](../../bugs/BUG-1778-OPEN.md),
[BUG-1800](../../bugs/BUG-1800-OPEN.md): все три 2026-11-17 закрыты для P3 как `OPEN (DEBTOR)`,
и остаток остался без владельца — ни строки в `ROADMAP.md`, ни указателя в `STATUS-PN.md`.
Задача закрывает все три бага своими воротами.

## Почему это не ещё один срез BUG-1800

[ADR-036](../../docs/decisions/ADR-036-incremental-restyle-invalidation.md) §Decision п.4:
для shadow-DOM класса ничего не строится без нового решения, а §Future называет ровно двух
кандидатов и требует «new task with its own stand and a fresh user decision» — этой задачи
до сих пор не было.

Узкая рука инкрементального рестайла (инвалидационные наборы правил, PERF-INVALIDATION-S1,
`crates/engine/layout/src/style/invalidation.rs`) **инертна на страницах с авторским shadow root**:
`NodeRestyleIndex::invalidated_rules` возвращает `None` при `has_in_shadow_doc`
([`style/restyle.rs:1974`](../../crates/engine/layout/src/style/restyle.rs:1974)), потому что
`InvalidationIndex::build` сканирует только лист документа. github (Primer) и cnn (author shadow)
поэтому каждый флаш идут широкой дорогой — полный список кандидатов на каждый пересчитанный узел.

## Текущее состояние (замеры 2026-11-17, срез 17 BUG-1800 уже в `main`)

| Сайт | Флаши | Стоимость | Что держит union |
|---|---|---|---|
| github.com | 31 / сессию | сумма 64,9 с; paired `walk_sum` 36,1 с; `ready_s` 23–37 с | doc-wide корни has-reach и стража `has_in_shadow_doc`; клон `ComputedStyle` в `CustomFlow::inherit` 21–25 мс |
| cnn.com | 15 полных из 135 | 2,0–4,6 с каждый, сумма 45,2 с внутри `run-scripts` 49,4 с из 79 | корень документа от `has_in_shadow_doc`, класс consent-баннера на `<html>`, класс-записи больших контейнеров |

`forced_same` на cnn — 3530/3539 (99,7 %): пересчёты форсированы чтениями DOM, а не изменением стиля.
`--dump-layout` cnn в тот же день — 147/118/193 с в трёх прогонах.

**Замеры старше LOAD-4…9 (2026-11-17/18) и не перемерялись** — S1 начинается с перезамера, а не
с этих чисел. Приёмка BUG-1800 переехала с ebay (403 с 2026-11-12) на github ещё срезом 11.

## Границы — не повторять без нового факта

Срезы 12/13/16 BUG-1800 уже проверили и откатили (ADR-036 §Alternatives):

- снятие `conservative` / стража `has_in_shadow_doc` целиком;
- сужение adjacent-записей до области хоста (`shadow_sheet_root`);
- dependency-checked reuse (пуст по построению на сайтах шторма);
- лицензия `inherited` по identity.

Семантику каскада и матчера не трогать — меняются только лицензии и стоимость. Широкая дорога
остаётся байт-в-байт для всего, что не лицензировано; `LUMEN_VERIFY_INCR_CASCADE=1` — звуковой гейт
каждого среза.

## S1 — перепись на текущем `main` и выбор (кода нет)

**Зачем:** выбрать между двумя кандидатами ADR-036 §Future или закрыть класс числом. Без этой
таблицы S2 не начинается.

**Шаги:**

1. Собрать `dev-release` на текущем `main` ([`docs/commands.md`](../commands.md) §Build cost).
2. github, две пары прогонов: `LUMEN_NO_ADBLOCK=1 python scripts/compat_crawl.py --only github`
   (`--maximized`), плюс `LUMEN_PROFILE_TREE=1` и `LUMEN_FRAME_LOG=1`; снять число флашей,
   `walk_sum`, `ready_s`, корзины `[engine] incr cascade`.
3. cnn, две пары: `lumen --trace-nav <out.json> <url>` и `lumen --dump-layout <url>`;
   сэмплер
   `scripts/native_sample.py` — не меньше трёх срезов на горячем участке `run-scripts`.
4. Разложить стоимость полного флаша на статьи: (a) широкий матчинг, (b) клон `ComputedStyle`
   в `CustomFlow`, (c) обход и сборка боксов, (d) прочее. Проценты — от измеренной суммы.
5. Таблица — в карточку BUG-1800 §Срез 18 и в [`docs/perf/journal.md`](../perf/journal.md).

**Ворота S1 (могут не взяться):** статьи покрывают ≥ 90 % измеренной суммы, названная доминанта —
самая большая и ≥ 40 %. Если доминанты нет или она вне двух кандидатов — S2 не начинается:
ADR-036 дополняется числом, BUG-935/1778/1800 переводятся в `WONTFIX (Phase 3+)` в том же коммите.
Это законный исход среза, а не провал.

**Гейт S1:** код не менялся — `cargo check` не нужен, нужен коммит только документации.

## S2 — механизм по выбору S1 (один из двух, не оба)

### Вариант A — shadow-листы в индексацию правил

Точки входа: `style/invalidation.rs::InvalidationIndex::build` (сейчас «shadow sheets excluded»,
`//!`-шапка `:13`), `style/restyle.rs:1967` (`invalidated_rules`), `style/restyle.rs:2080`
(`has_in_shadow_doc`), `style/restyle/node_change.rs:289` (`change_can_reach_shadow_sheets`).

Сделать: индекс строится на пару (лист документа, лист теневого корня) с ревизией листа в ключе;
`:host`, `:host-context()` и `::slotted()`-компаунды сканируются как ключи теневого листа; узел
внутри теневого дерева консультируется по объединению индекса документа и всех объемлющих теневых
индексов. Лицензия выдаётся только если объединение — звуковая надмножество прошлого matched set;
всё невыразимое (`:state()`, `:scope`) остаётся в широкой дороге.

Стенд: фикстура с host + shadow root + `::slotted` (расширить
`crates/engine/layout/src/box_tree/tests/bug935_cascade_bench.rs` или новый
`v8_bug935_shadow_stand`). Ворота: кандидатов на флаш ≤ 1/10 от широкой дороги, вывод
`ComputedStyle` побайтово равен широкому пути, `LUMEN_VERIFY_INCR_CASCADE=1` — equal.

### Вариант B — класс для клона `ComputedStyle`

Точка входа: `crates/engine/layout/src/custom_flow.rs:199` (`CustomFlow::inherit` клонирует
`prev: &ComputedStyle` целиком на каждого потомка), `:177`/`:229` (`probe = new.clone()` в
`dep`/`resume`), `:247` (`below`).

Сделать: общая база стиля + переопределения (или `Arc`-снимок), чтобы наследование custom properties
не копировало весь `ComputedStyle`; `custom_props`-карта остаётся единственным изменяемым полем.

Стенд: `v8_bug935_s101_stand` (2200 элементов, 300 флашей) и `bug935_cascade_bench`. Ворота:
доля клона из S1 ≤ 1/3, сумма флашей стенда ≤ 70 % от HEAD, `LUMEN_VERIFY_SCOPE_PRUNE=1` — 0
расхождений.

### Общий гейт S2

`cargo clippy -p lumen-layout -p lumen-js --features v8-backend -p lumen-shell --all-targets -- -D warnings`;
`cargo test -p lumen-layout bug935`; `cargo test -p lumen-js --features v8-backend v8_bug935`;
`python graphic_tests/dump_golden.py` 12/12. Пиксели не трогаются — полный `graphic_tests/run.py`
не нужен.

## S3 — живые ворота и закрытие трёх багов

**Живой замер, две пары сессий (A/B по env-переключателю механизма, интерливед):**

- github: `LUMEN_NO_ADBLOCK=1 python scripts/compat_crawl.py --only github` ×2 на механизме и ×2
  без него. Ворота: медиана `walk_sum` ≤ 50 % базы и медиана `ready_s` не хуже базы; `verify`
  equal на каждой сессии.
- cnn: `python scripts/geom_diff.py --only cnn --nav-timeout 120` — страница снимается (сейчас
  красный: `engine thread busy … about_to_wait.rs:290`), 2 из 3 попыток; `--trace-nav` `run-scripts`
  ≤ 25 с (база 49,4 с).

**Если ворота взяты** — BUG-935, BUG-1778, BUG-1800 переезжают в `BUGS-FIXED.md` одним коммитом,
файлы `bugs/BUG-935-OPEN.md` → `-FIXED.md` и т. д., ADR-036 получает статус `Superseded` (или
дополнение о снятой границе).

**Если ворота не взяты** — ADR-036 дополняется числом и остаётся в силе, баги остаются `OPEN
(DEBTOR)` с новой границей. Четвёртого среза без нового решения пользователя не начинать.

## Критерии готовности

- [x] S1: таблица статей в карточке и journal, доминанта названа или класс закрыт числом —
  **закрыт числом**: доминанты ≥ 40 % нет, таблицы — BUG-1800 §Срез 18 и journal §2026-11-18.
- [ ] ~~S2~~ — не выполняется по вердикту S1 (ворота «доминанта ≥ 40 %» не взяты).
- [ ] ~~S3~~ — не выполняется по вердикту S1.
- [x] ADR-036 синхронизирован (§Addendum 2026-11-18); `STATUS-P6.md` очищён; `ROADMAP.md` → `done` (`gen_roadmap.py`); BUG-935/1778/1800 → `WONTFIX (Phase 3+)`.

## Исход (2026-11-18)

S1 закрыл класс WONTFIX с числом — законный исход по брифу. Ворота S1: статьи покрывают
100 % измеренной суммы; доминанта «самая большая и ≥ 40 %» не названа ни на одном сайте
(github — матчинг 25–31 % честно; cnn — `quote_pseudos` ~38 % с DETAIL-завышением, вне
кандидатов; клон-кандидат 0,5–1,2 % на обоих). Названные переписью кандидаты Phase 3+
(каждый < 40 %, требуют своего решения): псевдо-проба по флашу (cnn), scoped-лицензия
`custom_props` при непустом `cq_boxless` (github).

