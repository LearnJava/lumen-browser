# BUG-986 — `Document::get()` паникует на `NodeId` из чужого документа: `index out of bounds`

**Статус:** OPEN
**Заведён:** 2026-09-04 (наблюдение за живым прогоном корпуса «топ-100 зарубежных», сессия-наблюдатель)
**Область:** `crates/engine/dom/src/lib.rs:706` (`Document::get` → `&self.nodes[id.index()]`)
**Владелец:** P3

## Симптом

Процесс падает с паникой главного потока:

```
thread 'main' (9796) panicked at crates\engine\dom\src\lib.rs:706:20:
index out of bounds: the len is 39 but the index is 238
```

Место — `Document::get()`:

```rust
pub fn get(&self, id: NodeId) -> &Node {
    &self.nodes[id.index()]
}
```

## Что говорит измерение

Три прогона корпуса 2026-09-04 дали **пять** таких паник. Индекс во всех случаях
**больше длины**, причём с большим отрывом:

| Прогон | len | index |
|---|---|---|
| 20260904-144122 (`live.stderr.0`) | 39 | 238 |
| 20260904-150504 (`live.stderr.0`) | 159 | 238 |
| 20260904-150604 (`live.stderr.0`) | 39 | 238 |
| 20260904-150604 (`live.stderr.2`) | 143 | 190 |
| 20260904-144122 (`live.stderr.2`) | — | — |

Индекс 238 повторяется при разной длине массива узлов — то есть это **не выход за
границу растущего документа**, а `NodeId`, принадлежащий другому, более крупному
документу. Аудит открывает каждому сайту свою вкладку (`new_tab`), поэтому
кандидаты — идентификатор, переживший навигацию, или пересёкший границу вкладки.

## Воспроизведение

Повторяется на одних и тех же сайтах в разных прогонах:

- `https://amazon.com/` — падение на скрипте челленджа AWS WAF
  (`*.token.awswaf.com/.../challenge.js`);
- `https://outlook.com/` (owa) — после `owa.mailindex.*.js`;
- `https://bing.com/`.

Бэктрейса нет — прогон шёл без `RUST_BACKTRACE=1`. **Первый шаг — прогнать любой из
трёх URL с `RUST_BACKTRACE=1`**, это назовёт вызывающего бесплатно.

## Сопутствующее (не заводить отдельно)

Та же паника роняет движковый поток вторичной паникой:

```
thread 'lumen-engine' (10500) panicked at crates\shell\src\relayout.rs:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
```

Это `document.lock().unwrap()` на мьютексе, отравленном первой паникой, — симптом
BUG-986, а не самостоятельный дефект.

## Сырые данные

`.tmp/perf-audit/20260904-*/live.stderr.*.log`, разбор —
`.tmp/observe/OBSERVATION-2026-09-04.md` §3.

## Вторая репродукция — самодостаточный WPT-тест (2026-09-04, срез 3 WPT-RUN-7)

Тот же класс паники, но без внешнего сайта — воспроизводится детерминированно
одним вендоренным тестом, что для отладки удобнее amazon/outlook/bing:

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
бесплатный, в отличие от живого прогона по внешним сайтам.
