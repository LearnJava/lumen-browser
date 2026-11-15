# BUG-1813 — начальный `about:blank`-кадр не отдаёт глобалы: `contentWindow.JSON === undefined`

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-11-15 (P3, срез 3 [BUG-1647](BUG-1647-FIXED.md))
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
`undefined`, и порт-канал якоря падал с
`TypeError: Cannot read properties of undefined (reading 'parse'|'stringify')`
(`.tmp/pl1812.py`, стек `<anonymous>:113:132` и `:562:337`). Найден при срезе 3
[BUG-1647](BUG-1647-FIXED.md) после того, как перенос `MessagePort` заработал
([BUG-1812](BUG-1812-FIXED.md)).

## Причина

Кадр без собственного изолята (`_lumen_f_initial_frame` создаёт биндинг на
`about:blank`) фасад проксирует в `_lumen_f_global_get`, а у биндинга нет
`peer` (`FrameDocBinding::peer`) — `resolve_peer` отвечал `kind: 'undefined'`
(тег `absent`) на любой запрос. Осознанное ограничение среза 3
[BUG-1790](BUG-1790-OPEN.md) для `eval` («код, исполненный здесь, не увидит
собственных глобалов кадра»), но оно было распространено на все глобалы,
включая встроенные объекты, которые подделкой не являются: `JSON`, `Object`,
`Array`, `Math`, `TextEncoder`, …

## Что сделано (2026-11-15, P3)

1. **Отдельный тег на «доступ есть, изолята нет».** `frame_bridge_globals.rs`
   `resolve_peer` отвечает `nopeer`, когда биндинг найден и `accessible`, но
   живого peer-рантайма нет. Не-доступный и несуществующий биндинг
   по-прежнему `absent` — cross-origin/opaque гейт не ослаблен.
2. **Отступление в шиме фасада.** `wrapWinFacadeGlobals` на `nopeer` отдаёт
   значение из СВОЕГО изолята, но только для имён, снятых при установке шима
   (`pristineGlobals`, до скриптов страницы): переменные страницы-читателя
   туда не попадают, `__leak`-подобные имена так и остаются `undefined`.
   `eval` исключён явно — решение среза 3 BUG-1790 в силе.
3. **Тест** `v8_runtime::tests::initial_about_blank_frame_answers_builtin_globals`:
   вставленный кадр — `contentWindow.JSON === window.JSON`, `Math`/`Object` те
   же, `JSON.parse` — функция, `contentWindow.__leak` (переменная страницы) —
   `undefined`, `contentWindow.eval` — `undefined`, cross-origin сосед —
   `undefined`.

**Ограничение (в рамках среза).** Кадр без собственного изолята получает
встроенные объекты ЧИТАТЕЛЯ, поэтому патч страницы-читателя (`JSON.parse =
…`) через такой фасад виден; в Chrome это pristine-объекты кадра. Полноценный
realm начального кадра — задача FRAME.

## Проверка

- `cargo test -p lumen-js --features v8-backend --lib -- initial_about_blank_frame_answers_builtin_globals` — 1/1;
  `frame_bridge_globals::tests::dead_peer_reads_as_nopeer` — 1/1.
- Живая проба `.tmp/blank1812.py` (бинарь ветки): `appended-blank:
  json:"object" sameJson:true parse:"function" evalT:"undefined"
  url:"about:blank"`, то же для `about:blank` в `src`.
- Стенд `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score`:
  `TypeError … ('parse'|'stringify')` ушёл, балла пока нет — следующий блокер
  [BUG-1814](BUG-1814-OPEN.md) (в `G.parse(L)` приходит base64url-строка вместо
  JSON-текста).
