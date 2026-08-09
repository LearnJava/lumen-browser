# BUG-365 — `EyeDropper.open()` всегда падает с `ReferenceError: _lumen_eye_dropper_open is not defined`: нативная привязка не установлена, `?.` не защищает необъявленный идентификатор, а 6 юнит-тестов при этом зелёные

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/eye_dropper.rs:58` — тело шима; установка шима — `crates/js/src/v8_runtime.rs:4249`, единственный оставшийся движок)
**Найден:** P2, WPT-VENDOR-eyedropper (2026-07-28), проба `--dump-layout` вне WPT (сам WPT-тест категории — SKIP по testdriver)
**Актуализировано:** P1, 2026-08-04 (P3-v8-post-audit) — rquickjs-путь установки шима (был `lib.rs:1187`) удалён целиком в S12b-F2/F3; дефект не зависел от rquickjs и полностью переживает снос движка

## Фикс (P3, 2026-08-09)

1. Небезопасное чтение привязки `_lumen_eye_dropper_open?.call?.(null)` заменено на
   `typeof globalThis._lumen_eye_dropper_open === 'function' ? ... : null` — `?.`
   не защищает от `ReferenceError` на необъявленном идентификаторе, `typeof` защищает.
   Нативной привязки по-прежнему нет (платформенный пикер не реализован), поэтому
   `open()` теперь предсказуемо доходит до документированного фолбэка и резолвится
   `{ sRGBHex: '#ffffff' }` вместо падения `ReferenceError`.
2. Мёртвая `extern "C" fn _lumen_eye_dropper_open` (C-ABI-заглушка, никак не связанная
   с JS-глобалом того же имени, нигде не вызывалась) удалена из `eye_dropper.rs` —
   решение по пункту 2 «возможного фикса» ниже: честно оставить API стабом, а не
   регистрировать привязку без платформенной реализации.
3. Сопутствующие WebIDL-отклонения исправлены: `constructor()` больше не принимает
   `options` (спека: конструктор без аргументов) → `EyeDropper.length === 0`, и у
   инстанса больше нет собственного свойства `options`; добавлен
   `EyeDropper.prototype[Symbol.toStringTag] = 'EyeDropper'`.
4. `test_eye_dropper_resolve_value` переписан: вместо `.then(...)` без ожидания
   теперь глобалы `__ok`/`__err` заполняются в `.then(resolve, reject)`, второй
   `eval` читает результат (микрозадачи V8 дренируются между двумя вызовами `eval`,
   `MicrotasksPolicy::kAuto`, тот же паттерн, что `shared_storage.rs::promise_result`).
   Добавлены `test_eye_dropper_constructor_length_is_zero`,
   `test_eye_dropper_no_stray_options_property`, `test_eye_dropper_to_string_tag`.
   `cargo test -p lumen-js --features v8-backend eye_dropper` — 9/9 зелёных.

**Не исправлено, вынесено в [BUG-698](BUG-698-OPEN.md):** проверка transient user
activation (спека WICG требует `NotAllowedError` без предшествующего пользовательского
жеста) — в кодовой базе нет инфраструктуры отслеживания активации ни для одного API
(grep по `user activation`/`UserActivation`/`transient activation` — пусто), заводить
её ради одного `EyeDropper` вне скоупа точечного бага; тот же класс пробела уже
отдельно заведён на [BUG-390](BUG-390-OPEN.md) (`requestFullscreen`),
[BUG-655](BUG-655-OPEN.md) (`requestPointerLock`),
[BUG-667](BUG-667-OPEN.md) (`getScreenDetails`).

## Симптом

`EyeDropper` в Lumen существует и конструируется, но **ни один вызов `open()`
не может завершиться успешно** — промис всегда отклоняется `ReferenceError`.
Проба `--dump-layout` (все строки — фактический вывод):

```
typeof EyeDropper                       = function
'EyeDropper' in window                  = true
new EyeDropper()                        = OK
open() no gesture                       = rejected name=ReferenceError
open() then abort                       = rejected name=ReferenceError
e.message                               = _lumen_eye_dropper_open is not defined
typeof _lumen_eye_dropper_open          = undefined
'_lumen_eye_dropper_open' in globalThis = false
open(aborted signal)                    = rejected name=AbortError ctor=DOMException
```

Единственный путь, который работает, — ранний выход по уже прерванному
`AbortSignal` (он стоит до обращения к привязке). Всё остальное — и штатный
вызов, и документированный в коде фолбэк «вернуть белый цвет, если нативной
привязки нет» — недостижимо.

## Причина

Шим (`eye_dropper.rs:58`) обращается к привязке так:

```js
const result = _lumen_eye_dropper_open?.call?.(null);
```

Два независимых дефекта в одной строке:

1. **Привязки нет.** `_lumen_eye_dropper_open` не регистрируется как глобал на
   V8 (единственном оставшемся движке — rquickjs-путь снесён целиком в S12b-F2/F3,
   до этого дефект был идентичен на обоих). В Rust есть `pub extern "C" fn
   _lumen_eye_dropper_open` (`eye_dropper.rs:24`), но это C-ABI-функция,
   возвращающая `*const u8`; она не связана с JS ничем, кроме совпадения имени,
   и во всём репозитории на неё нет ни одной ссылки, кроме собственного
   определения. Комментарий рядом (`eye_dropper.rs:20`) прямо это подтверждает:
   «platform integration deferred to P3» — интеграция не написана.
2. **`?.` не защищает от необъявленного идентификатора.** Опциональная
   цепочка коротит только на значениях `null`/`undefined`; разрешение самого
   имени `_lumen_eye_dropper_open` происходит до неё и на несуществующем
   биндинге даёт `ReferenceError`. Автор шима явно рассчитывал на обратное —
   иначе фолбэк `if (!result) resolve({ sRGBHex: '#ffffff' })` (строки 66-71)
   не имел бы смысла. Правильная защита — `typeof _lumen_eye_dropper_open ===
   'function'` или чтение через `globalThis._lumen_eye_dropper_open`.

Итог: `#ffffff`-фолбэк, разбор JSON-ответа и поздний `abort` (строки 49-82) —
мёртвый код целиком.

## Почему это не поймали тесты

`cargo test -p lumen-js eye_dropper` — 6/6 зелёных (прогнано в этой сессии,
`dev-release`), при полностью нерабочем `open()`. Ни один тест не может упасть
по построению:

- `test_eye_dropper_open_returns_promise` и `test_eye_dropper_open_accepts_options`
  проверяют только `result instanceof Promise` — `async`-функция возвращает
  промис и при отклонении, так что утверждение выполняется всегда;
- `test_eye_dropper_resolve_value` — единственный тест про результат — вешает
  проверки внутрь `.then(...)`, который на отклонённом промисе не вызывается
  вовсе, и ничего не ждёт: обработчик отказа отсутствует, необработанное
  отклонение из `ctx.eval` не всплывает, тест зелёный;
- остальные три проверяют только конструктор и экспорт в глобалы.

То есть набор проверяет форму API, но ни одного его наблюдаемого результата.
Верификация фикса должна идти через `await`/`assert` на разрешённом значении
(`sRGBHex`), а не через `instanceof Promise`.

## Масштаб

Категория `eyedropper` — 4 файла-кандидата, 2 отобранных id, оба недоступны
исполнителю: `eye-dropper-abort-signal.tentative.https.html` — SKIP (тест
синтезирует клик через `test_driver.Actions`, потому что `open()` по спеке
требует пользовательской активации), `idlharness.https.window.html` — TIMEOUT
по HTTPS-порт-гэпу. Поэтому баг найден пробой вне WPT, а не прогоном; WPT
подтвердит фикс только после появления testdriver-исполнителя.

Практический масштаб за пределами WPT — весь публичный API: любой сайт,
вызвавший `new EyeDropper().open()`, получает `ReferenceError` вместо цвета или
внятного отказа (`NotAllowedError`/`AbortError`). Никакой платформенный
диалог выбора цвета (PowerShell ColorDialog / zenity / osascript), обещанный
в шапке модуля, не вызывается — этой части нет вовсе.

## Сопутствующие отклонения от WebIDL (тот же файл, чинить заодно)

Проверено той же пробой:

- `EyeDropper.length` === 1, по спеке конструктор без аргументов → 0;
- `EyeDropper.prototype[Symbol.toStringTag]` === `undefined`, отсюда
  `Object.prototype.toString.call(new EyeDropper())` === `[object Object]`
  вместо `[object EyeDropper]`;
- у инстанса есть собственное свойство `options` (`this.options = options || {}`),
  которого в спеке нет;
- требование пользовательской активации не реализовано: по спеке WICG
  `open()` без transient activation обязан отклоняться `NotAllowedError`, а
  здесь активация не проверяется вовсе (сейчас это маскируется `ReferenceError`,
  но всплывёт сразу после починки п.1).

Что уже соответствует спеке и ломать при фиксе не надо: вызов без `new`
бросает `TypeError`, `open` лежит на прототипе, ранний отказ по прерванному
сигналу — `DOMException` с `name === "AbortError"`.

## Возможный фикс (план на момент находки — пункты 1/2/4 реализованы, см. «Фикс» выше)

1. ~~Читать привязку безопасно~~ — сделано.
2. ~~Решить судьбу нативной части~~ — сделано (снята мёртвая заглушка, честный стаб).
3. Добавить проверку пользовательской активации с отказом `NotAllowedError` —
   **не сделано**, вынесено в [BUG-698](BUG-698-OPEN.md) (нет инфраструктуры
   отслеживания активации ни для одного API в кодовой базе).
4. ~~Переписать `test_eye_dropper_resolve_value`~~ — сделано.
