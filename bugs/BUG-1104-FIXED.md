# BUG-1104 — `CustomEvent.prototype.initCustomEvent` (legacy DOM init method) missing entirely

**Статус:** FIXED (P6, 2026-09-23), в срезе того же дня, что и заведение
**Заведён:** P6, 2026-09-23, попутно при живой пробе [BUG-791](BUG-791-FIXED.md) (dzen.ru SSO)
**Компонент:** js (`crates/js/src/shim/web_api_shim_head.js`)

## Симптом

Живой прогон `dzen.ru` (после починки MCP-канала в BUG-683 срезе 2)
показал в консоли:

```
Uncaught TypeError: n.initCustomEvent is not a function
    at new s (<anonymous>:1:1611610)
    ...
```

Dzen SSO-проверочный бандл вызывает легаси-метод `CustomEvent.prototype.
initCustomEvent(type, bubbles, cancelable, detail)` (DOM LS §2.2, тот же
класс методов, что и уже реализованные `Event.prototype.initEvent`/
`UIEvent.prototype.initUIEvent`/`MouseEvent.prototype.initMouseEvent`/
`StorageEvent.prototype.initStorageEvent`), но `CustomEvent` его не имел
вовсе — конструктор объявлен в `crates/js/src/shim/web_api_shim_head.js:192`,
`initEvent`-семейство рядом реализовано для всех соседей, `initCustomEvent`
пропущен.

## Фикс

`CustomEvent.prototype.initCustomEvent` добавлен по образцу
`UIEvent.prototype.initUIEvent`: переинициализирует три легаси-поля через
`initEvent` (что по спеке также сбрасывает `isTrusted` в `false`) и
перезаписывает `detail`. Тест —
`crates/js/src/dom/tests/v8_event_classes.rs::customevent_init_custom_event_reinitializes_and_sets_detail`.

## Замер

A/B на живом `dzen.ru` (тот же `--mcp-live-port`, два прогона с разницей
только в этом фиксе): ошибка `initCustomEvent is not a function` исчезла
из `resource://console` после фикса. Итог SSO-хендшейка (`sso_failed=blocked`
в итоговом URL) не изменился — остаток относится к [BUG-791](BUG-791-FIXED.md)
(другая цепочка ошибок, `Cannot read properties of null (reading
'childNodes')`, не эта), фикс не претендует на его закрытие.
