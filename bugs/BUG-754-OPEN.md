# BUG-754 — модель анти-фингерпринта смешанная: `navigator.webdriver` отсутствует полностью, тогда как в Chrome и Firefox это существующее свойство `false`, а остальной surface_api сознательно мимикрирует под Chrome

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — литерал `navigator`, свойство не заводится; намеренность отсутствия задокументирована в `crates/js/src/surface_api.rs`). Тесты, закрепляющие текущую модель: `crates/js/src/surface_api.rs::tests::webdriver_absent_in_navigator`, `crates/js/tests/cases/no_automation_markers.rs::webdriver_not_in_navigator`, `crates/driver/tests/cases/antidetect_surface_api.rs::navigator_webdriver_is_absent`
**Найден:** P3, 2026-08-10 — остаток [BUG-379](BUG-379-FIXED.md) (раздел «Заметки» заявки: «это отдельный вопрос выбора модели, и его стоит решить явно»)

## Симптом

```js
'webdriver' in navigator            // Lumen: false · Chrome: true · Firefox: true
navigator.webdriver                 // Lumen: undefined · Chrome: false · Firefox: false
```

То есть Lumen отличим от обоих браузеров ровно тем же механизмом, из-за
которого закрыт BUG-379: детектор смотрит не на значение, а на наличие
свойства.

## Причина

Не дефект реализации, а неявно выбранная и внутренне противоречивая модель.
`surface_api.rs` одной половиной сознательно мимикрирует под Chrome:

- `navigator.appName === 'Netscape'`
- `navigator.vendor === 'Google Inc.'`
- `navigator.product === 'Gecko'`, `productSub === '20030107'`
- `navigator.appVersion` — Chrome-строка
- `navigator.cookieEnabled === true`, `doNotTrack === null` (Chrome-дефолт)
- пустые `plugins`/`mimeTypes` вместо отсутствующих

а другой половиной (`navigator.webdriver`) изображает браузер, которого не
существует. Комментарий в шиме объясняет выбор так: «Defining it via
`Object.defineProperty` would make `'webdriver' in navigator` return true,
which is itself a detection signal» — верно ровно наоборот: `true` здесь как
раз совпадает с Chrome и Firefox, а `false` (отсутствие) не совпадает ни с
одним из них.

## Влияние

- Один дополнительный высокоуверенный признак Lumen в наборе, который проект
  как раз старается сузить (см. приватность как заявленную ценность).
- Признак дешёвый: одна строка `'webdriver' in navigator`, доступна любому
  скрипту без разрешений.

## Как чинить

Решение бинарное и продуктовое, а не техническое — код обеих ветвей
тривиален:

1. **Совпадать с реальными браузерами.** Определить
   `navigator.webdriver === false` (own-свойство на прототипе `Navigator`,
   `configurable: true`, `enumerable: true` — как в Chrome). Три теста,
   перечисленных выше, переворачиваются с «свойства нет» на «свойство есть и
   равно `false`». Внимание: `configurable: false` здесь недопустим — это
   был бы ровно дефект BUG-379 в новой форме (у Chrome свойство
   configurable, значит и «неудаляемость» отличима).
2. **Оставить отсутствие** и признать признак осознанной ценой позиции «не
   притворяться никем», задокументировав выбор в `docs/plan/privacy.md`
   вместе с уже мимикрирующими под Chrome свойствами — тогда встаёт
   следующий вопрос: почему `vendor`/`appName`/`productSub` притворяются, а
   это свойство нет; последовательная версия этой модели требует убрать и
   их.

Смешанное состояние (текущее) — единственный вариант, который не защищает
ни одну из двух моделей.

## Заметки

- Тестовая форма уже готова: `assert_marker_absent`
  (`no_automation_markers.rs`) и `ev_bool`
  (`antidetect_surface_api.rs`) проверяют наблюдаемое из страницы состояние,
  так что после решения достаточно поменять утверждение, а не механику
  проверки.
