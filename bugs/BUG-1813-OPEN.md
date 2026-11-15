# BUG-1813 — начальный `about:blank`-кадр не отдаёт глобалы: `contentWindow.JSON === undefined`

**Статус:** OPEN
**Заведён:** 2026-11-15 (P3, срез 3 [BUG-1647](BUG-1647-OPEN.md))
**Область:** js (`crates/js/src/frame_bridge.rs` — фасад окна для кадра без
собственного изолята: `winFacade`/`wrapWinFacadeGlobals` →
`_lumen_f_global_get`; `frame_bridge_globals.rs`; `_lumen_f_initial_frame`)

## Симптом

У кадра, который создан скриптом и сразу вставлен в документ (начальный
`about:blank`, HTML LS §7.1), `contentWindow` есть синхронно, но его глобалы
не читаются:

```
appended-blank:        json:"undefined"  sameJson:false  evalT:"undefined"  url:"about:blank"
appended-about-blank:  json:"undefined"  sameJson:false  evalT:"undefined"  url:"about:blank"
detached:              contentWindow === null (верно)
```

(проба `.tmp/blank1812.py`) — в Chrome это pristine-глобал того же
происхождения: `contentWindow.JSON === window.JSON`.

**Почему это важно.** Обфусцированный бандл reCAPTCHA (`recaptcha__en.js`,
`<anonymous>:451:477`) добывает «неиспорченный» `JSON` именно так — читает
`N.JSON` у свежего кадра и зовёт `G.parse`/`v.stringify`. В Lumen `N.JSON` —
`undefined`, и порт-канал якоря падает с
`TypeError: Cannot read properties of undefined (reading 'parse'|'stringify')`
(`.tmp/pl1812.py`, стек `<anonymous>:113:132` и `:562:337`). Найден при срезе 3
[BUG-1647](BUG-1647-OPEN.md) после того, как перенос `MessagePort` заработал
([BUG-1812](BUG-1812-FIXED.md)); это следующий блокер `grecaptcha.execute`.

## Причина

Кадр без собственного изолята (`_lumen_f_initial_frame` создаёт биндинг на
`about:blank`) фасад проксирует в `_lumen_f_global_get`, а у биндинга нет
`peer` (`FrameDocBinding::peer`) — натив отвечает `kind: 'undefined'`.
Осознанное ограничение среза 3 [BUG-1790](BUG-1790-OPEN.md) для `eval`
(«код, исполненный здесь, не увидит собственных глобалов кадра»), но оно
распространено на все глобалы, включая встроенные объекты, которые
подделкой не являются: `JSON`, `Object`, `Array`, `Math`, `TextEncoder`, …

## Что сделать

1. Для кадра **того же происхождения** без peer отдавать встроенные глобалы
   этого изолята (они и есть спецификационные: `contentWindow.JSON` в Chrome —
   тот же объект, что `window.JSON`). Минимум — встроенные V8-объекты и
   `undefined`-безопасный ответ для страничных имён.
2. Не ослаблять гейт для cross-origin/opaque: там `_lumen_f_global_get`
   обязан молчать (`undefined`), как сейчас.
3. Тест в `crates/js`: appended `about:blank` кадр —
   `contentWindow.JSON === window.JSON`, `contentWindow.Math === window.Math`,
   `contentWindow.Object === window.Object`; cross-origin сосед — `undefined`.
4. Проверка: `.tmp/blank1812.py` → `json:"object"`, `sameJson:true`; затем
   `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score`.
