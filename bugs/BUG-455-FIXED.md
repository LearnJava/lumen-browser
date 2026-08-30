# BUG-455 — `ctx.restore()` не восстанавливает ни одного JS-видимого атрибута: после save/restore чтение состояния и рисование расходятся навсегда

**Статус:** FIXED 2026-08-31
**Компонент:** js (`crates/js/src/dom.rs` — `save`/`restore` в фабрике контекста
пробрасываются в натив, а `fillStyle`/`font`/`lineWidth`/… хранятся отдельными
полями JS-объекта и в стек не попадают)
**Найден:** 2026-07-29 (P2), WPT-VENDOR-html-canvas — прогон среза `html/canvas/element`

## Симптом

Проба: для каждого атрибута — прочитать, `save()`, записать другое значение,
`restore()`, прочитать снова.

```
S fillStyle                before=#000000            during=#00ff00      after=#00ff00      restored=false
S strokeStyle              before=#000000            during=#00ff00      after=#00ff00      restored=false
S lineWidth                before=1                  during=7            after=7            restored=false
S lineCap                  before=butt               during=round        after=round        restored=false
S lineJoin                 before=miter              during=round        after=round        restored=false
S miterLimit               before=10                 during=3            after=3            restored=false
S globalAlpha              before=1                  during=0.5          after=0.5          restored=false
S globalCompositeOperation before=source-over        during=xor          after=xor          restored=false
S shadowBlur               before=0                  during=5            after=5            restored=false
S shadowColor              before=rgba(0, 0, 0, 0)   during=#00ff00      after=#00ff00      restored=false
S shadowOffsetX            before=0                  during=4            after=4            restored=false
S shadowOffsetY            before=0                  during=4            after=4            restored=false
S font                     before=10px sans-serif    during=20px serif   after=20px serif   restored=false
S textAlign                before=start              during=center       after=center       restored=false
S textBaseline             before=alphabetic         during=top          after=top          restored=false
```

**Ни один из 15 атрибутов не восстановлен.**

## Почему это хуже, чем «геттер косметический»

Натив состояние восстанавливает **правильно** — расходятся именно две копии:

```
ctx.fillStyle = '#ff0000'; ctx.fillRect(...);      // база красная
ctx.save(); ctx.fillStyle = '#0000ff'; ctx.restore();
ctx.fillRect(...);                                  // рисует КРАСНЫМ — верно по спеке
ctx.fillStyle                                       // читается '#0000ff' — неверно
```

То есть после любой пары save/restore страница видит одно, а движок рисует другим,
и дальше расхождение только накапливается: читающий-модифицирующий код
(`ctx.lineWidth = ctx.lineWidth * 2` вокруг save/restore — обычный приём) получает
неверную базу. Молчаливое расхождение опаснее честной поломки: ошибки нет ни в
консоли, ни в картинке до тех пор, пока страница не прочитает состояние.

## Причина

Атрибуты контекста живут двумя независимыми копиями: `_lumen_canvas2d_*`-нативы
держат настоящее состояние (и его же кладут/снимают в `save`/`restore`), а
JS-обёртка держит собственные поля, потому что интерфейса с аксессорами нет вовсе
(см. [BUG-449](BUG-449-FIXED.md) — контекст является обычным объектным литералом,
все члены — собственные свойства экземпляра). `save`/`restore` просто зовут натив
и о JS-копии ничего не знают.

Тот же корень даёт и вторую половину [BUG-451](BUG-451-FIXED.md): геттер
`fillStyle` отдаёт записанную строку, а не то, чем реально рисуют.

## Смежное, из той же пробы

`ctx.shadowColor` по умолчанию — `rgba(0, 0, 0, 0)`; HTML LS §4.12.5.1.5 требует
`'#000000'` (непрозрачный чёрный, тень выключена нулевыми offset/blur, а не
прозрачностью).

## Данные WPT

Срез `html/canvas/element`: серия `2d.state.saverestore.*` в
`element/the-canvas-state/` — по одному падающему сабтесту на каждый атрибут в
каждом из четырёх размеров холста (`for a canvas of size (N, N)`), сообщение
`assert_equals: ctx.<attr> === old`. Это же тянет за собой `element/reset/*`
(29 падающих сабтестов: `reset()` проверяется тем же способом).

## Направление починки

Правильное — общее с [BUG-449](BUG-449-FIXED.md): завести настоящий
`CanvasRenderingContext2D.prototype` с аксессорами, которые читают/пишут натив, и
тогда отдельной JS-копии состояния не станет вовсе, а `save`/`restore` окажутся
корректны бесплатно.

Дешёвое временное — вести в обёртке собственный стек: `save()` кладёт снимок
JS-полей, `restore()` снимает. Это лечит симптом, но оставляет вторую копию
истины и вместе с ней весь класс расхождений (`setTransform`, `clip`, `filter`
имеют ту же природу).

## Что починено 2026-08-31

Взят дешёвый вариант (полный `CanvasRenderingContext2D.prototype` с
аксессорами над нативом — отдельная задача, `getTransform()` для этого всё
равно ждёт `DOMMatrix`, BUG-522). `_lumen_make_canvas2d_ctx` заводит второй
ненумеруемый слот `__canvas2d_stack__` (пустой массив); `save()` после
нативного вызова кладёт в него неглубокую копию JS-состояния (`st`, без
`nid`/`canvas`), `restore()` после нативного вызова снимает верхний снимок и
копирует его поля обратно в `st` — геттеры/сеттеры читают `st` напрямую, так
что синхронизация бесплатна для остальных 59 членов интерфейса. `reset()`
дополнительно опустошает `__canvas2d_stack__` — §4.12.5.1.2 требует опустошить
весь стек состояний, а не только сбросить текущее; без этого `restore()`
после `reset()` вернул бы состояние из ДО сброса. Стиль-атрибуты
(`fillStyle`/`strokeStyle`, могут держать `CanvasGradient`/`CanvasPattern`)
восстанавливаются по ссылке — это обычное присваивание поля объекта, копия не
клонирует сам градиент.

`corner()`/`ellipse()` вызывают нативные `_lumen_canvas2d_save`/`_restore`
напрямую (только вокруг CTM для дуг), минуя `ctx.save()`/`ctx.restore()` —
на JS-стек это не влияет и рассинхронизации не создаёт.

**Смежная заявка не подтвердилась**: `shadowColor` по умолчанию —
`rgba(0, 0, 0, 0)`, что и есть спековое значение (проверено по вендоренному
`element/shadows/2d.shadow.attributes.shadowColor.initial.html`:
`_assertSame(ctx.shadowColor, 'rgba(0, 0, 0, 0)', …)`); `'#000000'` в заявке
было ошибочным — значение не трогалось.

**Регрессионные тесты** — `crates/js/src/dom/tests/v8_core/canvas_object_model.rs`:
все 15 атрибутов восстанавливаются, `save()` сам ничего не меняет, `restore()`
на пустом стеке — no-op, вложенные save/restore ведут себя как настоящий стек,
`reset()` опустошает стек, градиент переживает save/restore по ссылке.
