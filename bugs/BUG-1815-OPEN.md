# BUG-1815 — встроенные объекты кадра со своим изолятом приходят копией без методов: `G.parse is not a function` в бандле reCAPTCHA

**Статус:** OPEN
**Заведён:** 2026-11-15 (P3, срез 5 [BUG-1647](BUG-1647-OPEN.md), после
[BUG-1814](BUG-1814-FIXED.md))
**Область:** js — фасад окна, чтение чужих глобалов через границу изолятов:
`crates/js/src/frame_bridge.rs` (`_lumen_f_global_get`, `wrapWinFacadeGlobals`,
`resolve_peer`), `frame_bridge_globals.rs`

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

`reCAPTCHA Timeout` остаётся. Все документы стенда (anchor `google.com`,
страница стенда, `about:blank`) в своём realm отдают
`JSON=object parse=function` (`.tmp/pl1815.py`), `contentWindow` через
`HTMLIFrameElement.prototype` бандл на этом пути не читает — значит `G` —
встроенный объект ЧУЖОГО realm, полученный фасадом.

## Что измерено (`.tmp/fr1815.py`, живое окно, тот же бинарь)

Страница `data:text/html,<p>top</p>`, в неё вставлен `<iframe srcdoc>`:

| момент | `w.JSON` | `w.JSON.parse` | `w.JSON === window.JSON` | `w.Math.max` | `w.Object.keys` |
| --- | --- | --- | --- | --- | --- |
| сразу (начальный `about:blank`, своего изолята ещё нет) | object | function | true | — | — |
| после загрузки `srcdoc` (свой изолят) | object | **undefined** | false | **undefined** | **undefined** |

То есть по [BUG-1813](BUG-1813-FIXED.md) глобалы `about:blank`-кадра без
изолята отдаются из realm читателя (методы живы), а как только у кадра
появляется свой изолят, тот же `JSON` приезжает через фасад **данными**:
функции теряются при сериализации значения через границу.

По HTML LS §7.2.3 same-origin доступ к `window.JSON` чужого кадра отдаёт
настоящий объект того realm — `JSON.parse` вызываем. Chrome именно так и
работает, поэтому бандл и берёт «pristine JSON» из свежего кадра.

## Рабочая гипотеза

`_lumen_f_global_get` отдаёт значение target-изолята через JSON-конверт:
объекты материализуются как плоские копии, функции выпадают. Для
собственных глобалов target-изолята, которые являются встроенными
объектами/функциями (`JSON`, `Math`, `Object`, `Array`, `Reflect`, …),
семантически верный ответ — встроенный объект realm-читателя (прецедент —
BUG-1813 для кадров без изолята); для остальных значений — текущая
сериализация.

## Что сделать

1. Замерить в Chrome (CDP, та же вставка `srcdoc`): `w.JSON === window.JSON`,
   `w.JSON.parse` — зафиксировать эталон.
2. Отличить встроенный глобал target-изолята от собственного значения
   страницы (снимок имён при установке шима, как `pristineGlobals` в
   BUG-1813, плюс сверка с встроенными именами realm-читателя).
3. Тест в `crates/js`: same-origin кадр со своим изолятом —
   `contentWindow.JSON.parse` — функция, `contentWindow.Math.max(1,2) === 2`;
   собственное значение страницы (`window.foo = {a:1}`) по-прежнему приходит
   данными.
4. Проверка: `python scripts/captcha_stand.py --arm lumen --only
   recaptcha-v3-score` → «пройдено», балл v3 в JSON-записи.
