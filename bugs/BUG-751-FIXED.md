# BUG-751 — `navigator.userActivation` захардкожен в `{isActive: true, hasBeenActive: true}`: транзиентной активации в движке нет, поэтому каждый гейт по жесту пользователя вырождается в «всегда разрешено»

**Статус:** FIXED 2026-09-21 (P6, ДОРАБОТКА → [GAP-USERACT](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-USERACT` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
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

## Исправлено

**2026-09-21 (P6, `p6-gap-useract`).** `V8JsRuntime` получил три атомарных
поля (`activation_last_ms`/`activation_ever`/`activation_consumed`,
`crates/js/src/v8_runtime/runtime.rs`) и `install_user_activation`
(`crates/js/src/v8_runtime/install/platform.rs`) — четыре натива:
`_lumen_mark_user_activation` (ставит отметку временем того же клока, что
`_lumen_now_ms`, включая `--deterministic`/`--monotonic-clock`),
`_lumen_user_activation_is_active` (окно 5 с, HTML LS не фиксирует точное
число — то же, что у Chromium/Firefox), `_lumen_user_activation_has_been_active`,
`_lumen_consume_user_activation`. Разметка вшита в сам путь диспетчеризации
доверенного ввода (`_lumen_dispatch_bubble`'click', `_lumen_dispatch_mouse_event`,
`_lumen_dispatch_pointer_event`, `_lumen_dispatch_key_event` —
`web_api_shim_mid.js`), а не в отдельный Rust-коллбэк на каждый тип события:
эти функции — единственный путь, которым шелл доставляет реальный OS-ввод
(`crates/shell/src/input/mod.rs`'s гарантия `isTrusted=true`), поэтому
страничный `dispatchEvent()` подделать активацию не может. Клавиатурный
модификатор сам по себе (Shift/Control/Alt/Meta/…) активацию не ставит.
`navigator.userActivation` стал живыми геттерами поверх натива вместо
замороженного литерала — форма объекта (два булевых свойства) не изменилась,
как и предполагалось в разделе «Симптом» этого файла.

Ровно как предсказано в разделе «Ожидается»: `filesystem_access.rs`,
`window_management.rs`, `local_font_access.rs`, `media_devices.rs` и
`requestFullscreen()`'s `_lumen_fs_request_error` (см. [BUG-758](BUG-758-FIXED.md))
заработали без правки самой проверки `isActive === false` — они уже читали
`navigator.userActivation`. Дополнительно к «Ожидается»: каждый из этих пяти
гейтов теперь зовёт `_lumen_consume_user_activation()` сразу после успешной
проверки (спековое «consume user activation»), так что второй вызов той же
функции без нового жеста между ними получает отказ. `navigator.share()`,
`window.open()`, `navigator.clipboard.write()`, `PaymentRequest.show()`
активацию до сих пор не проверяют вовсе (не только не работали — гейта там
не было и раньше) — отдельная задача поверх готового механизма, не часть
этого GAP.

Автоматизация (последняя заметка выше): под `--deterministic` без
`--monotonic-clock` `_lumen_now_ms` заморожен на 0, поэтому одна отметка
активации держит `isActive` истинным до явного `consume()` — то есть
headless/WPT-прогоны не ломаются синтетическим кликом, ровно то поведение,
которое здесь и просили.

Тесты: 6 новых `dom::tests::v8_gap_useract` (маркировка, окно распада,
consume, игнор untrusted `dispatchEvent()`, игнор голого модификатора,
интеграция с гейт-функцией) + 5 тестов `v8_fullscreen_locks` адаптированы под
реальный гейт вместо старого «всегда true». `cargo test -p lumen-js --features
v8-backend` — 4087/4087; `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` — чисто.
