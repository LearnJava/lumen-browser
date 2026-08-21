# BUG-807 — `IntersectionObserver.observe()` не доставляет первичное наблюдение: колбэк приходит только побочным эффектом чужого релэйаута

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 15 — кластер `intersection-observer`, 31 TIMEOUT)
**Область:** `crates/js/src/dom.rs:9555` (`IntersectionObserver.prototype.observe` — кладёт цель в `_observations` и всё), `crates/js/src/dom.rs:9586` (`_lumen_deliver_intersection_observers`), `crates/shell/src/main.rs:3316` (`deliver_layout_observers` — единственный вызывающий, работает по релэйауту)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Тест наблюдает элемент и ждёт первое уведомление — и не получает его
никогда, хотя элемент видим, страница жива, а исключений нет:

```js
// intersection-observer/observer-callback-arguments.html
const io = new IntersectionObserver(function (entries, observer) {
  t.step(() => { /* … */ t.done(); });   // не вызывается
});
io.observe(document.body);
```

Спека (Intersection Observer §3.2 `observe`) требует поставить в очередь
задачу первичного наблюдения: колбэк обязан прийти вскоре после `observe()`
сам по себе, без каких-либо изменений в документе.

## Прямое измерение

`tests/wpt/verify_event_delivery_gaps.py` (живое окно, http, улики из stderr;
dev-release, Linux, 2026-08-21, коммит `a7ee9468f`):

| проба | получено |
|---|---|
| `io-initial` — наблюдаем видимый `<div>`, страницу не трогаем | **колбэк не пришёл ни разу** за 8 с (15 тиков `setInterval` — страница жива) |
| `io-initial-then-relayout` — то же, но через 1 с меняем высоту **постороннего** элемента | `mutating-unrelated`, затем `io-cb ratio=1` |
| `io-after-mutation` — цель заведомо за экраном, затем двигаем её в вид | ровно один колбэк (тот, что после сдвига); первичного снова нет |
| `io-v2-trackvisibility` — `{trackVisibility: true, delay: 100}` на видимом элементе | колбэка нет (тот же механизм, не отдельный дефект v2) |

То есть доставка есть, но она привязана исключительно к релэйауту:
достаточно чужой, никак не связанной с целью мутации, чтобы очередь
разобралась.

## Причина (локализована чтением кода)

`observe()` (`dom.rs:9555`) только добавляет запись в `this._observations`
(с `lastRatio = -1`, то есть «первая доставка обязана выстрелить») и не
планирует ничего. Единственный вызывающий
`_lumen_deliver_intersection_observers()` — шелловский
`deliver_layout_observers()` (`main.rs:3316`), который выполняется в конце
релэйаута. На статической странице после загрузки релэйаутов больше нет,
поэтому очередь `_observations` так и остаётся неразобранной: `lastRatio`
никогда не сравнивается, «первая доставка» не наступает.

Это отдельный от уже открытых дефект: [BUG-626](BUG-626-OPEN.md) (нет
валидации аргументов), [BUG-627](BUG-627-OPEN.md) (`root`/`scrollMargin`
игнорируются) и [BUG-628](BUG-628-OPEN.md) (`takeRecords`/`root`/
`rootMargin`/`thresholds` отсутствуют) описывают *содержимое* доставки; здесь
доставки нет вовсе, и потому тест не падает, а виснет.

## Масштаб

Механизм `intersection-observer-initial` в `tests/wpt/timeout_audit.py`
забирает 33 id остатка снимка WPT-RUN-5: 29 в самой категории
`intersection-observer` (из них 17 — `v2/*`, где ждут первичного уведомления
с `isVisible`) и по одному в `css/css-contain/content-visibility`,
`css/css-overflow`, `css/css-anchor-position` и `resize-observer`. Тесты, которые *меняют* вёрстку
перед ожиданием, проходят харнесс и попадают в FAIL/PASS, а не сюда, — цена
дефекта именно в наблюдай-и-жди форме.

## Направление починки (не предписание)

Поставить доставку в очередь из самого `observe()` — микрозадача или тик
event loop'а, а не следующий релэйаут; тот же путь нужен и `unobserve`/
`disconnect` (спека требует не терять уже поставленные записи). Дешёвый
вариант в существующей архитектуре: попросить шелл прогнать
`deliver_layout_observers()` на ближайшем кадре, даже если релэйаут не
понадобился.

## Как проверить фикс

1. `verify_event_delivery_gaps.py --variant io-initial` печатает `io-cb`.
2. `--variant io-v2-trackvisibility` печатает `io-cb` (значение `isVisible`
   при этом останется неверным — это [BUG-628](BUG-628-OPEN.md)/v2, не здесь).
3. WPT: `run_report.py --all --root intersection-observer --recursive` —
   TIMEOUT'ы уходят, часть тестов при этом станет FAIL (по BUG-626/627/628),
   и это ожидаемый результат починки.
