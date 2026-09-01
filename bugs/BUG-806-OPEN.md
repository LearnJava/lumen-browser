# BUG-806 — SMIL-анимация SVG отсутствует целиком: `<animate>`/`<set>` ничего не анимируют и никогда не шлют `beginEvent`/`endEvent`/`repeatEvent`

**Статус:** OPEN (ДОРАБОТКА → [GAP-SMIL](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-SMIL` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 15 — кластер `svg/animations`, 26 из 31 TIMEOUT категории)
**Область:** движок целиком — нет модели времени SMIL нигде в воркспейсе; в JS-слое `crates/js/src/svg.rs:762-795` четыре класса анимационных элементов (`SVGAnimateElement`, `SVGAnimateTransformElement`, `SVGAnimateMotionElement`, `SVGSetElement`) существуют как явные заглушки с пустыми телами `beginElement()`/`endElement()`, и даже они достижимы только через `document.createElementNS` ([BUG-685](BUG-685-OPEN.md))
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Каждый тест `svg/animations`, ждущий анимационного события, висит до таймаута
раннера с полностью пустым логом браузера — исключения нет, ждать нечего:

```xml
<!-- svg/animations/onbegin.svg -->
<animate id="anim" attributeName="visibility" to="visible" begin="0s" end="2s"
         onbegin="document.getElementById('anim2').beginElement()"/>
<set id="anim2" attributeName="width" to="100" begin="indefinite"/>
<script>
  set.addEventListener('beginEvent', t.step_func_done(...));   // никогда
</script>
```

## Прямое измерение

`tests/wpt/verify_event_delivery_gaps.py --variant smil-events --variant smil-dom`
(живое окно, http, улики читаются из stderr браузера; dev-release, Linux,
2026-08-21, коммит `a7ee9468f`):

| проба | ожидалось | получено |
|---|---|---|
| `<animate begin="0s" dur="100ms" repeatCount="2">` + слушатели `beginEvent`/`endEvent`/`repeatEvent` + атрибут `onbegin` | три события + вызов атрибута | **ни одного**, страница живёт (15 тиков `setInterval`) |
| ширина `<rect>` через 1 с после начала анимации | анимируется | `10` — исходное значение |
| `document.getElementById('s').constructor.name` для разметочного `<set>` | `SVGSetElement` | `HTMLUnknownElement` |
| `typeof s.beginElement` | `function` | `undefined` |

## Причина

Двухслойная, и оба слоя нужно чинить:

1. **Нет самого механизма SMIL.** В воркспейсе нет ни разбора атрибутов
   `begin`/`dur`/`end`/`repeatCount`/`fill`, ни расписания активных
   интервалов, ни применения анимированного значения к `animVal`, ни
   диспетчеризации трёх событий SMIL. `grep -rn "attributeName" crates/`
   даёт только `MutationRecord.attributeName` — совпадений по SMIL нет.
2. **Даже заглушки недостижимы из разметки.** `svg.rs` регистрирует четыре
   класса анимационных элементов, но только через monkey-patch
   `createElementNS`; парсер, не реализующий foreign content
   ([BUG-685](BUG-685-OPEN.md)), отдаёт разметочный `<animate>` как
   `HTMLUnknownElement`, поэтому `anim.beginElement()` — `TypeError`, а не
   no-op.

Заметьте разницу с CSS-анимациями ([BUG-503](BUG-503-OPEN.md)): там движок
анимацию планирует, но не сообщает о ней JS; здесь нет и самой анимации.

## Масштаб

26 из 31 TIMEOUT категории `svg/animations` в снимке WPT-RUN-5 (Linux) —
весь остаток категории после того, как классификатор
`tests/wpt/timeout_audit.py` разобрал прочие механизмы; механизм
`smil-animation` забирает 27 id по всему снимку (плюс один в `svg/linking`).
В вендоренном корпусе 314 файлов содержат хотя бы один из тегов
`<animate>`/`<animateTransform>`/`<animateMotion>`/`<set >` (290 из них —
сама `svg/animations`), так что цена починки выше, чем 27 id остатка: часть
этих файлов сейчас падает по другим, более ранним причинам и до остатка не
доходит.

## Направление починки (не предписание)

Минимум, разблокирующий категорию, — не полный SMIL, а модель времени +
события: разобрать `begin`/`dur`/`end`/`repeatCount`, вести активный
интервал в том же тике, что и CSS-анимации
(`crates/shell/src/animation_scheduler.rs`), диспатчить `beginEvent`,
`repeatEvent`, `endEvent` (плюс IDL-свойства `onbegin`/`onrepeat`/`onend`,
которых тоже нет), и применять `to`/`from`/`values` к `animVal` целевого
атрибута. Без [BUG-685](BUG-685-OPEN.md) это работать не будет — разметочный
`<animate>` обязан быть SVG-узлом.

## Как проверить фикс

1. Проба выше: `verify_event_delivery_gaps.py --variant smil-events` печатает
   `beginEvent`, `repeatEvent`, `endEvent`.
2. `--variant smil-dom` печатает `ctor SVGSetElement` и `has-beginElement`.
3. WPT: `run_report.py --all --root svg/animations --recursive` — счётчик
   TIMEOUT уходит от 26 к единицам.

## Уточнение цены (срез 16, 2026-08-21)

Механизм `smil-animation` в `tests/wpt/timeout_audit.py` уменьшился с 27 id
до 15: 12 файлов `svg/animations/*.svg` подключают harness как
`<script xmlns="http://www.w3.org/1999/xhtml" src="/resources/testharness.js"/>`,
то есть самозакрывающимся тегом, который HTML-парсер не закрывает
([BUG-786](BUG-786-OPEN.md), третья грань). Их лог это подтверждает напрямую:
`testharness.js` загружен, `testharnessreport.js` — нет, тело теста съедено
как текст первого скрипта. До ожидания SMIL-события такой тест не доходит,
поэтому его TIMEOUT принадлежит BUG-786, а не этому багу. Сам дефект (SMIL нет
целиком) от этого не меняется — меняется только то, сколько TIMEOUT-ов
корпуса объясняются им **сегодня**: после починки BUG-786 эти 12 вернутся
сюда.
