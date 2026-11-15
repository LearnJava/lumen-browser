# BUG-1815 — встроенные объекты кадра со своим изолятом приходят копией без методов: `G.parse is not a function` в бандле reCAPTCHA

**Статус:** FIXED (2026-11-15, P3, срез 6 [BUG-1647](BUG-1647-OPEN.md))
**Заведён:** 2026-11-15 (P3, срез 5 [BUG-1647](BUG-1647-OPEN.md), после
[BUG-1814](BUG-1814-FIXED.md))
**Область:** js — фасад окна, чтение чужих глобалов через границу изолятов:
`crates/js/src/frame_bridge.rs` (`wrapWinFacadeGlobals`, снимок встроенных),
`crates/js/src/v8_runtime/eval.rs` (`peer_global_get`)

## Симптом

После фикса [BUG-1814](BUG-1814-FIXED.md) (`JSON.parse` больше не получает
строку вместо значения, `.tmp/pl1814.py` — 0 падений) стенд
`python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score` —
`не_отрисован`, в stderr вместо `SyntaxError` новый класс:

```
[unhandled-rejection] TypeError: G.parse is not a function
    at <anonymous>:113:132        <- G.parse(L)
    at <anonymous>:451:477
    at Array.<anonymous> (<anonymous>:802:396)
    at Array.<anonymous> (<anonymous>:451:318)
    at kz.l (<anonymous>:113:105)
```

`reCAPTCHA Timeout` оставался. Все документы стенда (anchor `google.com`,
страница стенда, `about:blank`) в своём realm отдают `JSON=object
parse=function` (`.tmp/pl1815.py`), `contentWindow` через
`HTMLIFrameElement.prototype` бандл на этом пути не читает — значит `G` —
встроенный объект ЧУЖОГО realm, полученный фасадом.

## Что было измерено (`.tmp/fr1815.py`, живое окно, тот же бинарь)

Страница `data:text/html,<p>top</p>`, в неё вставлен `<iframe srcdoc>`:

| момент | `w.JSON` | `w.JSON.parse` | `w.JSON === window.JSON` | `w.Math.max` | `w.Object.keys` |
| --- | --- | --- | --- | --- | --- |
| сразу (начальный `about:blank`, своего изолята ещё нет) | object | function | true | — | — |
| после загрузки `srcdoc` (свой изолят) | object | **undefined** | false | **undefined** | **undefined** |

По HTML LS §7.2.3 same-origin доступ к `window.JSON` чужого кадра отдаёт
настоящий объект того realm — `JSON.parse` вызываем. Chrome именно так и
работает, поэтому бандл и берёт «pristine JSON» из свежего кадра.

## Причина

`_lumen_f_global_get` отдаёт значение target-изолята через JSON-конверт:
объекты материализуются плоскими копиями, функции выпадают. Для встроенных
объектов realm'а пира (`JSON`, `Math`, `Reflect`, …) это заведомо неверный
ответ — Chrome отдаёт объект С МЕТОДАМИ.

## Фикс

1. Шим бриджа (`frame_bridge.rs`) при установке, до скриптов страницы, снимает
   `_lumen_pristine_intrinsics` — значения встроенных объектов ECMAScript
   (`JSON`, `Math`, `Reflect`, `Atomics`, `Intl`) — и кладёт снимок в контейнер
   (`__lumen_C`, странице не виден).
2. `peer_global_get` (`v8_runtime/eval.rs`) сверяет прочитанное значение со
   снимком **по идентичности**: неизменённый встроенный объект отвечает новым
   тегом `builtin` вместо сериализованной копии. Собственное значение страницы
   под тем же именем в снимок не попадает и уходит данными, как раньше.
3. Шим фасада на `builtin` отдаёт одноимённый встроенный объект ИЗ СВОЕГО
   изолята (прецедент [BUG-1813](BUG-1813-FIXED.md)): методы живы.

Ограничения: `contentWindow.JSON` такого кадра — объект ЧИТАТЕЛЯ, то есть
`contentWindow.JSON === window.JSON` (в Chrome у кадра со своим realm — `false`);
патч `JSON.parse` в читателе через такой фасад виден. Полноценный realm чужого
кадра — задача FRAME. Конструкторы (`Object`, `Array`, …) остаются на прежнем
пути через мост: они и так вызываемы, а `Object.keys` через фасад по-прежнему
`undefined`.

## Проверка

- `cargo test -p lumen-js --lib --features v8-backend frame_bridge` — 75/75:
  новый `a_frame_with_its_own_isolate_hands_builtin_objects_with_methods`
  (`_lumen_f_global_get(0,'JSON').kind == 'builtin'`, `JSON.parse('{"a":1}').a
  === 1`, `Math.max(1,2) === 2`, собственное значение страницы — данными) и
  `a_page_value_shadowing_a_builtin_name_stays_data` (страница переопределила
  `JSON` → `kind == 'value'`).
- Живая проба `.tmp/fr1815.py`: после загрузки `srcdoc` —
  `parse:"function" mathMax:"function" same:true` (было `parse:"undefined"`).
- `.tmp/pl1815.py` на странице стенда: `[unhandled-rejection]`/`TypeError` в
  логе — **0** (было `G.parse is not a function`).
- `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score`:
  `G.parse`/`Timeout` ушли, `grecaptcha.execute` резолвится быстро (~9 с против
  30,5 с), но Google отвергает токен — `score: null`,
  `error-codes: ["browser-error", …]`. Следующий блокер —
  [BUG-1816](BUG-1816-OPEN.md).
