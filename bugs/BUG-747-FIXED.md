# BUG-747 — все ~220 членов живой обёртки узла лежат собственными свойствами инстанса, а не операциями прототипа

**Статус:** FIXED 2026-09-18 (P3) — побочный эффект [BUG-849](BUG-849-FIXED.md)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_build_element`, `_lumen_wrapper_proto_for`; почти пустые `Element.prototype`/`Node.prototype`)
**Найден:** P2, WPT-VENDOR-fenced-frame (2026-07-28) как пункт 4 [BUG-367](BUG-367-FIXED.md); выделен в отдельную заявку P3 2026-08-10 при закрытии остальных четырёх пунктов

## Симптом

`_lumen_build_element` строит обёртку живого узла одним объектным литералом, то
есть каждый метод и каждый геттер интерфейса — СОБСТВЕННОЕ свойство инстанса.
Прототипы при этом почти пусты: на них попало только то, что добавляли
отдельными коммитами.

Замер на дефолтной (V8) сборке, проба `--dump-layout` (`.tmp/bug367-probe.html`,
2026-08-10):

```
Object.getOwnPropertyNames(host).length              = 217
Object.getOwnPropertyNames(Element.prototype).length = 3     ["constructor","attachInternals","setHTML"]
Object.getOwnPropertyNames(Node.prototype)           = ["constructor","hasChildNodes"]
Object.keys(host)[0]                                 = tagName        (в браузере — пусто)
host own 'getAttribute' = true    Element.prototype own 'getAttribute' = false
```

(в отчёте BUG-367 от 2026-07-28 было 134 собственных свойства — с тех пор
литерал только вырос.)

Наблюдаемые последствия:

- `Object.keys(el)`, `for…in`, spread и `JSON.stringify(el)` выдают всю
  реализацию биндинга; у настоящего `Element` они пусты.
- `delete el.getAttribute` ломает метод у ОДНОГО узла, не задевая остальные.
- Любой `idlharness.*` по узловым интерфейсам не может пройти в принципе:
  он проверяет, что операции лежат на прототипе.
- Каждая обёртка несёт ~220 собственных свойств с замыканиями вместо
  разделяемого прототипа — прямой перф/память-налог на страницах с большим
  числом обёрнутых узлов.

## Причина

Литерал `var _obj = { __nid__: nid, get tagName() {…}, …, getAttribute: function(){…}, … }`
захватывает `nid` замыканием, поэтому каждый член ОБЯЗАН быть своим у инстанса —
иначе замыкание не к чему привязать. `Object.setPrototypeOf` в хвосте
(BUG-322) даёт цепочку прототипов, но членов на прототипе не появляется.

## Что нужно сделать

Перенести на `Element.prototype`/`Node.prototype` (через `Object.defineProperties`
для аксессоров) всё, что можно выразить через `this.__nid__` вместо захваченного
`nid`. Литерал должен схлопнуться до `__nid__` и того, что действительно требует
замыкания на инстансе (`_classList`/`_style`/`_dataset`/`_attributes`/
`_returnValue` и подобные кэши).

## Гочи, известные заранее

- `__nid__` теперь non-writable/non-enumerable/non-configurable (BUG-367,
  пункт 3) — `this.__nid__` читать можно, переприсваивать нельзя.
- Обёртки текстовых и комментарийных узлов строятся ТОЙ ЖЕ функцией и получают
  `Text.prototype`/`Comment.prototype`; члены, перенесённые на
  `Element.prototype`, для них исчезнут — проверить, что ни один внутренний
  потребитель не звал элементные методы на текстовом узле.
- Ветвление внутри литерала (например, `on*`-обработчики и специфичные для
  тега свойства навешиваются пост-фактум циклами) должно остаться на инстансе.
- Порядок важен: `Object.setPrototypeOf` вызывается в хвосте функции, а часть
  членов навешивается до него.

## Закрытие 2026-09-18 (P3) — побочный эффект BUG-849, не переклассифицировано вовремя

Симптом этой заявки (~220 собственных свойств на инстансе, пустые
`Element.prototype`/`Node.prototype`, `Object.keys(el)` выдаёт всю реализацию)
не воспроизводится: [BUG-849](BUG-849-FIXED.md) (FIXED 2026-09-18 — на самом
деле 2026-08-23, спустя две недели после этой заявки) перенесла интерфейс
обёртки на общий прототип-на-интерфейс тем же ходом, который эта заявка
просила («на прототип, а не на экземпляр»), но была заведена как отдельный
дефект (перф/OOM на `createElement`) и не сослалась на BUG-747 при закрытии.

Живая проверка (`cargo test -p lumen-js --features v8-backend wrapper_`,
`dom/tests/v8_core/mod.rs`):

- `wrapper_members_are_inherited_not_own` — `Object.keys(el).length === 0`,
  `hasOwnProperty('tagName')` и `hasOwnProperty('onclick')` оба `false`,
  `hasOwnProperty('__nid__')` `true`, `instanceof HTMLDivElement`/`Element`
  не сломаны;
- `wrapper_lazy_slots_are_per_node_and_stable` — `style`/`classList`/`dataset`/
  `attributes` остались отдельными на узел и стабильными под `===`, ровно
  как просил раздел «Что нужно сделать» этой заявки;
- `wrapper_on_handlers_and_expandos_stay_per_node` — `on*`-обработчики общие
  по имени на прототипе, значение и произвольный expando (`el.mine = 7`)
  остаются собственностью инстанса.

Все три зелёные на `main`. Код физически переехал из `dom.rs` в
`crates/js/src/shim/web_api_shim_mid.js` (`_LUMEN_WRAPPER_MEMBERS`,
`_lumen_wrapper_proto_for`) при том же BUG-849. Правок кода не потребовалось —
только реклассификация в трекере.
