# BUG-849 — `document.createElement` стоит ~140 мкс и навсегда удерживает JS-обёртку: 20 000 элементов — 2,8 с, 40 000 — фатальный OOM V8 и смерть процесса

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 23 — найден прямым прогоном страниц, маркер `dom-wrapper-oom` по измеренному списку id)
**Область:** `crates/js/src/dom.rs:3507` (`_lumen_make_element` — интернирование в `_lumen_element_wrappers[nid]`), `crates/js/src/dom.rs:4091` (`_lumen_build_element` — тело обёртки, ~1 480 строк и 26 `Object.defineProperty` на элемент), `crates/js/src/dom.rs:16524` (`_lumen_gc_collect` — единственный, кто чистит карту, и зовут его по idle-тику шелла)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
for (var i = 0; i < 20000; i++) { document.body.appendChild(document.createElement('div')); }
```

20 000 элементов создаются 2 847 мс. На 40 000 процесс умирает:

```
# Fatal JavaScript out of memory: Ineffective mark-compacts near heap limit
… Mark-Compact 1394.3 (1402.5) -> 1393.9 (1402.5) MB … allocation failure
```

Выход процесса — 133 (core dump). Никакого исключения в страницу не приходит:
умирает весь браузер, а вместе с ним — весь остаток шарда в прогоне WPT.

## Прямое измерение

2026-08-22, dev-release, Linux, коммит `3ae02b208`, `--dump-layout` на локальной
странице (`.tmp/s23/dom20k.html`, `dom40k.html`):

| элементов | результат |
|---|---|
| 20 000 | `[JS] created 20000 in 2847 ms` (≈142 мкс на элемент), процесс жив |
| 40 000 | куча дорастает до 1,4 ГБ, `Fatal JavaScript out of memory`, `rc=133` |

## Причина (локализована чтением кода)

```js
// dom.rs:3507
var cached = _lumen_element_wrappers[nid];
if (cached !== undefined) return cached;
var built = _lumen_build_element(nid);
_lumen_element_wrappers[nid] = built;
```

Интернирование само по себе обязательно — оно даёт `===`-идентичность узлов и
переживание expando-свойств ([BUG-291](BUG-291-FIXED.md)). Проблема в двух
других вещах:

1. **Цена одной обёртки.** `_lumen_build_element` — функция на ~1 480 строк с 26
   `Object.defineProperty`; каждый элемент получает собственный комплект
   аксессоров и замыканий, отсюда ~142 мкс и заметные мегабайты на тысячу узлов.
   Обычный движок кладёт это на прототип и хранит на объекте только `nid`.
2. **Карта только растёт.** `_lumen_element_wrappers` — обычный объект с сильными
   ссылками; чистит его один `_lumen_gc_collect(nids)`, которому шелл на idle-тике
   передаёт освобождённые арены nid. Узлы живого документа не освобождаются
   никогда, поэтому обёртки живут до конца документа — даже те, к которым скрипт
   больше не обратится. `WeakRef`/`FinalizationRegistry` здесь не используются.

## Масштаб

**1 id** остатка снимка WPT-RUN-5 — `/css/selectors/invalidation/has-complexity.html`
(строит дерево из десятков тысяч узлов, чтобы померить инвалидацию `:has()`).
Та же страница умирает одинаково и в корпусном прогоне, и в пробе, то есть это
не артефакт пробы. Кроме WPT дефект бьёт по любой странице, строящей крупный DOM
из скрипта: реальный сайт с 40 000 узлов — не экзотика.

## Проверка фикса

Страница целиком (сохранить куда угодно и открыть `--dump-layout`):

```html
<!doctype html><meta charset=utf-8><body><script>
var t0 = Date.now();
for (var i = 0; i < 40000; i++) { document.body.appendChild(document.createElement('div')); }
console.log('created 40000 in', Date.now() - t0, 'ms');
</script>
```

Сегодня она умирает с `rc=133`; после фикса должна печатать строку, а время на
элемент — упасть на порядок (сейчас ~142 мкс). Отдельно стоит проверить, что `===`-идентичность и expando-свойства
(тесты BUG-291) не сломались.
