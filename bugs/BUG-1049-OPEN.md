# BUG-1049 — динамически подгруженный классический скрипт: его верхнеуровневый `const`/`let` не виден модулю как свободная переменная (`ReferenceError`)

**Статус:** OPEN
**Заведён:** 2026-09-13 (GAP-NAVCTX срез 16, живая проверка BUG-797 закрытия — `html/semantics/embedded-content/bfcache`)
**Область:** js (механизм `loadScript()`-подгрузки классических скриптов и видимость их верхнеуровневых лексических деклараций для JS-модулей — со стороны шима или движка, не локализовано глубже прогона)
**Владелец:** P1/P3 (движок). Заведён P1 в ходе GAP-NAVCTX-задачи, здесь не чинится.

## Симптом

`tests/wpt/run_report.py --root html/semantics/embedded-content/bfcache --recursive --all`
(dev-release, 2026-09-13) — 0/6 harness OK, все шесть файлов падают одинаково:

```
ERROR /html/semantics/embedded-content/bfcache/embedded-js.html - Unhandled rejection: originSameOrigin is not defined
ReferenceError: originSameOrigin is not defined
    at Module.runBfcacheTestForEmbeds (.../resources/common.js:18:21)
```

`resources/common.js` — JS-модуль (`type=module`), в самом верху дожидается
`await loadBfCacheTestHelperResources()`, которая асинхронно догружает три
классических скрипта через `loadScript()`, включая
`back-forward-cache/resources/helper.sub.js` — тот объявляет `const
originSameOrigin = …` на своём верхнем уровне. После `await` модуль читает
`originSameOrigin` как свободную переменную (`targetOrigin:
originSameOrigin` в объектном литерале) — в спеке это должно резолвиться
через глобальную лексическую среду, которую делят между собой все
классические скрипты и модули документа. У нас — `ReferenceError`, то есть
объявление из догруженного скрипта туда не долетает.

## Почему это баг, а не доработка (`docs/probe-method.md` §8)

Классические скрипты исполняются (`helper.sub.js` явно загружается и не
падает сам по себе — иначе ошибка была бы `Failed to load script`, а не
`ReferenceError` внутри модуля), модули исполняются, `const`/`let` — не
отсутствующая языковая фича. Не хватает не функциональности, а связи между
двумя уже существующими механизмами — точечный кандидат на правку, а не
проектирование модели состояния.

## Цена по WPT

Минимум 6 id (`html/semantics/embedded-content/bfcache/*.html`) падают
ИМЕННО на этой ошибке, не дойдя до собственной сути теста (bfcache).
Не исследовано, сколько ещё id в дереве используют тот же паттерн
(`loadScript()` + módule top-level await + свободная переменная) —
`back-forward-cache/resources/helper.sub.js` — общий хелпер, используемый
далеко за пределами `bfcache/`.

## Что дальше

Не локализовано глубже прогона (не открыт `loadScript()`/движок скриптов).
Первый шаг — узнать, реализован ли `loadScript()` в шиме через честную
вставку `<script>` в DOM (тогда `var`/функции всплывают в `window`, но
`const`/`let` — нет даже в реальных браузерах и через это не работает) или
через `eval()` в изолированном скоупе (тогда не всплывает вообще ничего).
Если верно первое — это не баг движка, а баг самого WPT-хелпера
(`helper.sub.js` использует `const` там, где спека ожидает `var`-подобную
видимость) — сверить с апстримом WPT, возможно, апстрим тоже полагается на
что-то отличное от плоского `<script>`-инклюда. Если верно второе — баг в
`loadScript()`-шиме: подгружаемый скрипт должен делить top-level scope
документа, а не жить в собственном.
