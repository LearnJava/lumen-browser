# BUG-379 — защита от детекта автоматизации сама является отпечатком: 15 маркеров (`__playwright`, `__webdriver_evaluate`, `__selenium_*`, `_phantom`, `domAutomationController`, …) присутствуют own-свойствами `window` с `configurable:false`, тогда как в Chrome/Firefox их нет ни одного

**Статус:** OPEN
**Компонент:** js (`crates/js/src/surface_api.rs:163-181` — список `_automationGlobals` и цикл `Object.defineProperty(globalThis, _g, {get, set, configurable:false, enumerable:false})`; устанавливается на обоих движках: `crates/js/src/lib.rs:944` и `crates/js/src/v8_runtime.rs:3950`)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fledge-probe.html`, `.tmp/fledge-probe2.html`)

## Симптом

```
MARK.present as own props = 15/15: ["__playwright","__pwInitScripts","__pwExecPath",
    "__selenium_unwrapped","__selenium_evaluate","__webdriver_evaluate",
    "__webdriver_script_fn","__webdriver_script_func","__lastWatirAlert",
    "__lastWatirConfirm","__lastWatirPrompt","_phantom","callPhantom",
    "domAutomation","domAutomationController"]

MARK.in operator      = '__webdriver_evaluate' in window = true, typeof = undefined
MARK.hasOwnProperty   = true
MARK.descriptor       = {"get":"function","set":"function","enumerable":false,"configurable":false}
MARK.one-line detector = detector hits = 15 (Chrome/Firefox: 0)
```

Замысел кода прямо описан комментарием (`surface_api.rs:158-162`):

> Ensure no automation globals leak through. These are read-only shims that
> return `undefined` even if something tries to set them. We only define them if
> they do not already exist (they should not — Lumen never defines them — but
> this is a belt-and-braces guard…).

Проверка `typeof globalThis[_g] === 'undefined'` (`surface_api.rs:172`)
истинна всегда, потому что Lumen эти имена и правда никогда не определяет —
значит защита **всегда** создаёт все 15 свойств. Результат обратный
задуманному: детектор, который в Chrome ищет `__webdriver_evaluate` и не
находит ничего, на Lumen находит own-свойство. Достаточно одной строки:

```js
Object.getOwnPropertyNames(window).some(n => /^(__webdriver|__selenium|__playwright|_phantom)/.test(n))
```

— `false` в Chrome и Firefox, `true` в Lumen. То же даёт оператор `in` и
`hasOwnProperty`.

Дополнительно `configurable:false` делает маркеры неустранимыми: ни страница,
ни сам движок позже удалить их не могут, так что «замести следы» после
установки шима нельзя.

Сигнатура ещё и высокоэнтропийная: дескриптор — пара `get`/`set`, обе функции
возвращают `undefined`; в реальном браузере с настоящим Playwright эти имена —
data-свойства с объектами. То есть маркеры отличаются от обоих состояний,
которые встречаются в природе («нет вовсе» и «есть по-настоящему»), и образуют
третье, уникальное для Lumen.

## Причина

Дефект замысла, а не опечатка: чтобы имя гарантированно читалось как
`undefined`, код **создаёт** свойство. Но `undefined` при чтении — не то же
самое, что отсутствие: спека различает эти состояния через
`getOwnPropertyNames`/`in`/`hasOwnProperty`, а детекторы автоматизации именно
их и используют (проверка `typeof window.__x === 'undefined'` — самый наивный
из вариантов).

## Влияние

- Приватность — заявленная ценность проекта, а здесь механизм анти-фингерпринта
  **добавляет** уникальный признак вместо того, чтобы убрать.
- Затрагивает обе JS-ветки (V8 по умолчанию и rquickjs), т.е. любую сборку.
- Сочетается с BUG-378: даже убрав эти 15 имён, страница опознаёт Lumen по 592
  перечисляемым `_lumen_*`. Чинить имеет смысл вместе — по отдельности каждый
  фикс не даёт наблюдаемого эффекта.

## Как чинить

1. Просто не определять имена, которых нет. Цикл `surface_api.rs:170-181`
   удалить целиком; для случая «внешний скрипт впрыснул маркер через `eval`»
   защиты и не получалось — впрыснутое значение всё равно попало бы в
   `getOwnPropertyNames`.
2. Если нужна защита именно от *чужой* инъекции, она должна не создавать
   свойства, а перехватывать попытку записи — то есть жить на уровне
   `globalThis`-прокси или проверяться при подготовке контекста, а не заранее
   резервировать имена.
3. Тест `crates/driver/tests/cases/antidetect_surface_api.rs` сейчас закрепляет
   неверную инвариантность: он проверяет, что имена не *экспортируются*
   (`!source.contains("__playwright")` по тексту шима — а список лежит в другом
   модуле), но не проверяет, что их нет в `Object.getOwnPropertyNames(window)`.
   Утверждение теста нужно переписать на наблюдаемое из страницы состояние —
   иначе фикс не будет защищён (см. [[feedback_green_test_can_mask_broken_feature]]).

## Заметки

- `navigator.webdriver` при этом ведёт себя корректно (`typeof undefined`,
  значение `undefined`) — но в Chrome это свойство **существует** и равно
  `false`, а его отсутствие тоже отличает Lumen от Chrome. Это отдельный
  вопрос выбора модели («притворяться Chrome» против «не притворяться никем»),
  и его стоит решить явно — сейчас модель смешанная: маркеры создаются как у
  «настоящего браузера, где автоматизации нет», а `navigator.webdriver`
  отсутствует, как ни в одном из браузеров.
- Проба и вывод целиком: `.tmp/fledge-probe.html`/`.log`,
  `.tmp/fledge-probe2.html`/`.log`.
