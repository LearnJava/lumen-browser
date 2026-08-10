# BUG-751 — `navigator.userActivation` захардкожен в `{isActive: true, hasBeenActive: true}`: транзиентной активации в движке нет, поэтому каждый гейт по жесту пользователя вырождается в «всегда разрешено»

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `navigator.userActivation`), плюс путь диспатча пользовательских событий
**Найден:** P3, при закрытии [BUG-374](BUG-374-FIXED.md), 2026-08-10

## Симптом

```js
navigator.userActivation.isActive        // true, всегда
navigator.userActivation.hasBeenActive   // true, всегда
```

Объект заморожен на этапе установки шима:

```js
Object.defineProperty(navigator, 'userActivation', {
  value: Object.freeze({ isActive: true, hasBeenActive: true }),
  configurable: true, writable: false, enumerable: true,
});
```

Комментарий рядом объясняет это тем, что Lumen — однопользовательское
интерактивное приложение. Для самого атрибута это допустимое приближение, но
он же — единственный источник ответа на вопрос «мы сейчас внутри обработки
жеста пользователя?» для всех API, которые обязаны его задавать.

## Почему это важно

HTML LS определяет **transient activation** как окно (около 5 секунд) после
пользовательского ввода, а не «страница когда-либо получала ввод». На этом
окне построены гейты, которые в спецификациях сформулированы как «иначе
`SecurityError`»:

* `showOpenFilePicker()` / `showSaveFilePicker()` / `showDirectoryPicker()`
  (File System Access §8.1) — 3 сабтеста из 37 в
  `showPicker-errors.https.window.js`. Гейт добавлен в [BUG-374](BUG-374-FIXED.md)
  и написан правильно, но `isActive` никогда не бывает `false`, поэтому он не
  срабатывает: файловый диалог открывается по любому скрипту без жеста;
* `element.requestFullscreen()`, `navigator.share()`, `window.open()`,
  `navigator.clipboard.write()`, `PaymentRequest.show()` — тот же вопрос.

То есть дефект не в одном атрибуте, а в отсутствии механизма, на который
опираются несколько подсистем сразу.

## Ожидается

Транзиентная активация как состояние документа: путь диспатча пользовательских
событий (`click`, `keydown`/`keyup` кроме модификаторов, `mousedown`,
`pointerdown`, `touchend`) ставит отметку времени, `isActive` считается как
«с отметки прошло меньше окна активации», `hasBeenActive` — «отметка когда-либо
ставилась». Отдельно нужен способ *потребить* активацию (спека называет это
consuming user activation) для API, которые срабатывают один раз на жест.

После этого гейт в `filesystem_access.rs::requireUserActivation` начнёт
работать сам, без изменений — он уже читает `navigator.userActivation.isActive`.

## Заметки

- Отдельно от этого: в юнит-тестах и headless-прогонах жестов нет вовсе, так
  что вводить гейт без режима, в котором автоматизация может активацию
  подделать (`--deterministic`? driver-API?), значит сломать собственные
  прогоны. Решать вместе.
